//! Session persistence for Maverick.
//!
//! Implements the `ChatPersistence` trait from `xai-chat-state` using a
//! simple JSONL file per session. Each session gets a directory under
//! `app_data_dir()/sessions/<session_id>/` with `chat_history.jsonl`.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::oneshot;

use xai_chat_state::commands::{StrictAppendAck, StrictAppendError};
use xai_chat_state::persistence::ChatPersistence;
use xai_grok_sampling_types::ConversationItem;

/// JSONL-based chat persistence.
/// Each session's history is stored in `app_data/sessions/<id>/chat_history.jsonl`.
pub struct JsonlChatPersistence {
    #[allow(dead_code)]
    session_id: String,
    sessions_dir: PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
    history: Arc<Mutex<Vec<ConversationItem>>>,
}

impl JsonlChatPersistence {
    /// Create a new JSONL persistence for the given session.
    pub fn new(session_id: String, app_data_dir: PathBuf) -> Result<Self> {
        validate_session_id(&session_id)?;
        let sessions_dir = app_data_dir.join("sessions").join(&session_id);
        std::fs::create_dir_all(&sessions_dir)?;

        let history_path = sessions_dir.join("chat_history.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path)?;

        // Load existing history
        let history = if history_path.exists() {
            let f = File::open(&history_path)?;
            let reader = BufReader::new(f);
            reader
                .lines()
                .filter_map(|l| l.ok())
                .filter_map(|l| serde_json::from_str(&l).ok())
                .collect()
        } else {
            Vec::new()
        };

        Ok(Self {
            session_id,
            sessions_dir,
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
            history: Arc::new(Mutex::new(history)),
        })
    }

    /// Get the loaded history.
    pub fn history(&self) -> Vec<ConversationItem> {
        self.history.lock().unwrap().clone()
    }

    /// Write a single item to the JSONL file.
    fn write_item(&self, item: &ConversationItem) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        let json = serde_json::to_string(item)?;
        writeln!(writer, "{}", json)?;
        writer.flush()?;
        Ok(())
    }

    /// Rewrite the on-disk history and adopt the new file handle.
    ///
    /// The writer lock is taken *first*: dropping a `BufWriter` that still has
    /// buffered items would flush them into the file *after* the rewrite,
    /// resurrecting exactly the messages the caller just stripped.
    fn replace_history_inner(&self, items: &[ConversationItem]) -> Result<()> {
        let history_path = self.sessions_dir.join("chat_history.jsonl");
        let mut guard = self.writer.lock().unwrap();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&history_path)?;
        let mut writer = BufWriter::new(file);
        for item in items {
            writeln!(writer, "{}", serde_json::to_string(item)?)?;
        }
        writer.flush()?;
        // Swap handles while still holding the lock, so no write can land
        // between the truncate and the re-open.
        *guard = writer;
        drop(guard);
        *self.history.lock().unwrap() = items.to_vec();
        Ok(())
    }
}

impl ChatPersistence for JsonlChatPersistence {
    fn persist_message(&mut self, item: &ConversationItem) {
        self.history.lock().unwrap().push(item.clone());
        if let Err(e) = self.write_item(item) {
            // The trait is fire-and-forget, but a dropped write would silently
            // lose the message from disk while the session keeps using it.
            tracing::error!(error = %e, "failed to persist chat message");
        }
    }

    fn persist_working_directory_switch_and_ack(
        &mut self,
        item: &ConversationItem,
    ) -> oneshot::Receiver<Result<StrictAppendAck, StrictAppendError>> {
        let (reply, receiver) = oneshot::channel();
        let result = self.write_item(item);
        let ack = result.map(|_| StrictAppendAck::Appended).map_err(|e| {
            StrictAppendError::Indeterminate(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        });
        let _ = reply.send(ack);
        receiver
    }

    fn replace_history(&mut self, items: &[ConversationItem]) {
        if let Err(e) = self.replace_history_inner(items) {
            tracing::error!(error = %e, "failed to rewrite chat history on disk");
        }
    }

    fn replace_history_for_strip_and_ack(
        &mut self,
        items: &[ConversationItem],
    ) -> oneshot::Receiver<Result<(), std::io::Error>> {
        let (reply, receiver) = oneshot::channel();
        // Ack what actually happened: reporting `Ok(())` for a failed rewrite
        // tells the caller the stripped history was durably persisted.
        let result = self
            .replace_history_inner(items)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        let _ = reply.send(result);
        receiver
    }

    fn flush(&mut self) {
        if let Err(e) = self.writer.lock().unwrap().flush() {
            tracing::error!(error = %e, "failed to flush chat history");
        }
    }
}

/// Session ids become directory names under `sessions/`, so reject anything
/// that could escape that root — `..`, separators, or an empty id.
pub fn validate_session_id(id: &str) -> Result<()> {
    if id.trim().is_empty() {
        anyhow::bail!("session id cannot be empty");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("session id contains invalid path characters");
    }
    Ok(())
}

/// Session manager: tracks all sessions and their persistence.
pub struct SessionManager {
    app_data_dir: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Arc<JsonlChatPersistence>>>>,
}

impl SessionManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        let sessions_dir = app_data_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;

        Ok(Self {
            app_data_dir,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create or load a session.
    pub fn get_or_create(&self, session_id: &str) -> Result<Arc<JsonlChatPersistence>> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(p) = sessions.get(session_id) {
            return Ok(p.clone());
        }

        let persistence = Arc::new(JsonlChatPersistence::new(
            session_id.to_string(),
            self.app_data_dir.clone(),
        )?);
        sessions.insert(session_id.to_string(), persistence.clone());
        Ok(persistence)
    }

    /// List all session IDs, ordered by most recently modified first.
    pub fn list_sessions(&self) -> Vec<String> {
        let sessions_dir = self.app_data_dir.join("sessions");
        if !sessions_dir.exists() {
            return Vec::new();
        }

        let mut entries: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(&sessions_dir)
            .ok()
            .into_iter()
            .flat_map(|rd| rd.filter_map(|e| e.ok()))
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                Some((modified, name))
            })
            .collect();

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.into_iter().map(|(_, name)| name).collect()
    }

    /// Delete a session directory and its persistent files.
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        validate_session_id(session_id)?;
        let session_path = self.app_data_dir.join("sessions").join(session_id);
        // Disk first: if the removal fails, keep the index entry so the
        // session still resolves to whatever is left on disk.
        if session_path.exists() {
            std::fs::remove_dir_all(&session_path)?;
        }
        self.sessions.lock().unwrap().remove(session_id);
        Ok(())
    }

    /// Rename a session directory on disk.
    pub fn rename_session(&self, old_id: &str, new_id: &str) -> Result<()> {
        if old_id == new_id {
            return Ok(());
        }
        validate_session_id(old_id)?;
        validate_session_id(new_id)?;
        let sessions_dir = self.app_data_dir.join("sessions");
        let old_path = sessions_dir.join(old_id);
        let new_path = sessions_dir.join(new_id);

        if !old_path.exists() {
            anyhow::bail!("Session `{}` does not exist", old_id);
        }
        if new_path.exists() {
            anyhow::bail!("A session named `{}` already exists", new_id);
        }

        // Disk first, then the index — and drop the cached handle rather than
        // re-keying it: it still points at the old path, so reusing it would
        // recreate the old directory on the next history rewrite.
        std::fs::rename(&old_path, &new_path)?;
        self.sessions.lock().unwrap().remove(old_id);
        Ok(())
    }
}
