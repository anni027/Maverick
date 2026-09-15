//! Session persistence for Maverick.
//!
//! Implements the `ChatPersistence` trait from `xai-chat-state` using a
//! simple JSONL file per session. Each session gets a directory under
//! `app_data_dir()/sessions/<session_id>/` with `chat_history.jsonl`.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write, BufRead, BufReader};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::oneshot;

use xai_chat_state::persistence::ChatPersistence;
use xai_chat_state::commands::{StrictAppendAck, StrictAppendError};
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
}

impl ChatPersistence for JsonlChatPersistence {
    fn persist_message(&mut self, item: &ConversationItem) {
        self.history.lock().unwrap().push(item.clone());
        let _ = self.write_item(item);
    }

    fn persist_working_directory_switch_and_ack(
        &mut self,
        item: &ConversationItem,
    ) -> oneshot::Receiver<Result<StrictAppendAck, StrictAppendError>> {
        let (reply, receiver) = oneshot::channel();
        let result = self.write_item(item);
        let ack = result
            .map(|_| StrictAppendAck::Appended)
            .map_err(|e| StrictAppendError::Indeterminate(
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            ));
        let _ = reply.send(ack);
        receiver
    }

    fn replace_history(&mut self, items: &[ConversationItem]) {
        let history_path = self.sessions_dir.join("chat_history.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&history_path);

        if let Ok(file) = file {
            let mut writer = BufWriter::new(file);
            for item in items {
                if let Ok(json) = serde_json::to_string(item) {
                    let _ = writeln!(writer, "{}", json);
                }
            }
            let _ = writer.flush();
        }

        *self.history.lock().unwrap() = items.to_vec();
        // Replace the writer
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path);
        if let Ok(file) = new_file {
            *self.writer.lock().unwrap() = BufWriter::new(file);
        }
    }

    fn replace_history_for_strip_and_ack(
        &mut self,
        items: &[ConversationItem],
    ) -> oneshot::Receiver<Result<(), std::io::Error>> {
        let (reply, receiver) = oneshot::channel();
        self.replace_history(items);
        let _ = reply.send(Ok(()));
        receiver
    }

    fn flush(&mut self) {
        let _ = self.writer.lock().unwrap().flush();
    }
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
                let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                Some((modified, name))
            })
            .collect();

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.into_iter().map(|(_, name)| name).collect()
    }

    /// Delete a session directory and its persistent files.
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(session_id);

        let session_path = self.app_data_dir.join("sessions").join(session_id);
        if session_path.exists() {
            std::fs::remove_dir_all(session_path)?;
        }
        Ok(())
    }

    /// Rename a session directory on disk.
    pub fn rename_session(&self, old_id: &str, new_id: &str) -> Result<()> {
        if old_id == new_id {
            return Ok(());
        }
        let sessions_dir = self.app_data_dir.join("sessions");
        let old_path = sessions_dir.join(old_id);
        let new_path = sessions_dir.join(new_id);

        if !old_path.exists() {
            anyhow::bail!("Session `{}` does not exist", old_id);
        }
        if new_path.exists() {
            anyhow::bail!("A session named `{}` already exists", new_id);
        }

        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(old_id);

        std::fs::rename(old_path, new_path)?;
        Ok(())
    }
}