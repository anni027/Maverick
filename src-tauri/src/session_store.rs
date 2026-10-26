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
    sessions_dir: PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
    /// Conversation with per-item wall-clock ms (0 = legacy line written
    /// before timestamps were recorded).
    history: Arc<Mutex<Vec<(i64, ConversationItem)>>>,
}

/// JSONL envelope: one line per item, `{ts, item}`. A bare
/// `ConversationItem` line (the pre-envelope format) still loads, as ts 0.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedLine {
    ts: i64,
    item: ConversationItem,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Poison-tolerant lock: one panic elsewhere in the process must not turn
/// every later persistence call into a cascade.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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
            let mut items: Vec<(i64, ConversationItem)> = Vec::new();
            let mut dropped = 0usize;
            for line in reader.lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => {}
                    Ok(line) => {
                        if let Ok(entry) = serde_json::from_str::<PersistedLine>(&line) {
                            items.push((entry.ts, entry.item));
                        } else if let Ok(item) = serde_json::from_str::<ConversationItem>(&line) {
                            // Legacy line: persist with no recorded timestamp.
                            items.push((0, item));
                        } else {
                            dropped += 1;
                        }
                    }
                    Err(_) => dropped += 1,
                }
            }
            if dropped > 0 {
                tracing::warn!(
                    dropped,
                    path = %history_path.display(),
                    "chat history had unreadable lines; restored session may be truncated"
                );
            }
            items
        } else {
            Vec::new()
        };

        Ok(Self {
            sessions_dir,
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
            history: Arc::new(Mutex::new(history)),
        })
    }

    /// Get the loaded history.
    pub fn history(&self) -> Vec<ConversationItem> {
        lock(&self.history)
            .iter()
            .map(|(_, item)| item.clone())
            .collect()
    }

    /// Loaded history paired with persisted timestamps (0 = unrecorded).
    pub fn history_with_timestamps(&self) -> Vec<(i64, ConversationItem)> {
        lock(&self.history).clone()
    }

    /// Write a single item to the JSONL file.
    fn write_item(&self, item: &ConversationItem, ts: i64) -> Result<()> {
        let mut writer = lock(&self.writer);
        let json = serde_json::to_string(&PersistedLine { ts, item: item.clone() })?;
        writeln!(writer, "{}", json)?;
        // Batch the disk flush instead of flushing per line: on Windows,
        // `File::flush` hits FlushFileBuffers (a disk round-trip costing
        // milliseconds), and a turn persists 3-5 items. Durability for the
        // tail comes from `ChatStateCommand::Flush` at segment end; this
        // bounds the crash window inside long unflushed runs.
        if writer.buffer().len() >= 32 * 1024 {
            writer.flush()?;
        }
        Ok(())
    }

    /// Rewrite the on-disk history and adopt the new file handle.
    ///
    /// The writer lock is taken *first*: dropping a `BufWriter` that still has
    /// buffered items would flush them into the file *after* the rewrite,
    /// resurrecting exactly the messages the caller just stripped.
    fn replace_history_inner(&self, items: &[ConversationItem]) -> Result<()> {
        let history_path = self.sessions_dir.join("chat_history.jsonl");
        // History rewrites are routine (compaction, request building), so an
        // item that survives a rewrite keeps its original timestamp: match by
        // serialized form. New items get `now_ms()`. Both maps are built
        // before taking the writer lock, so the two mutexes are never held
        // at once.
        let stamp: HashMap<String, i64> = lock(&self.history)
            .iter()
            .filter_map(|(ts, item)| {
                serde_json::to_string(item).ok().map(|key| (key, *ts))
            })
            .collect();
        let mut guard = lock(&self.writer);
        // Drain pre-rewrite buffered lines into the old file first: since
        // `write_item` batches its disk flush, the old buffer may still hold
        // lines — dropping it after the truncate (via the handle swap below)
        // would flush them into the NEW file, resurrecting stripped messages.
        guard.flush()?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&history_path)?;
        let mut writer = BufWriter::new(file);
        // Each item is serialized exactly once: the item JSON doubles as the
        // timestamp-map key and as the `"item"` half of the envelope line
        // (`{"ts":…,"item":…}`, matching `PersistedLine` with no spaces).
        // Only the writer lock is held here (`stamp` is owned), so the two
        // mutexes are never held at once.
        let mut pairs: Vec<(i64, ConversationItem)> = Vec::with_capacity(items.len());
        for item in items {
            let item_json = serde_json::to_string(item)?;
            let ts = stamp.get(&item_json).copied().unwrap_or_else(now_ms);
            writeln!(writer, "{{\"ts\":{ts},\"item\":{item_json}}}")?;
            pairs.push((ts, item.clone()));
        }
        writer.flush()?;
        // Swap handles while still holding the lock, so no write can land
        // between the truncate and the re-open.
        *guard = writer;
        drop(guard);
        *lock(&self.history) = pairs;
        Ok(())
    }
}

impl ChatPersistence for JsonlChatPersistence {
    fn persist_message(&mut self, item: &ConversationItem) {
        let ts = now_ms();
        lock(&self.history).push((ts, item.clone()));
        if let Err(e) = self.write_item(item, ts) {
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
        let result = self.write_item(item, now_ms());
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
        if let Err(e) = lock(&self.writer).flush() {
            tracing::error!(error = %e, "failed to flush chat history");
        }
    }
}

/// Session ids become directory names under `sessions/`, so reject anything
/// that could escape that root — `..`, separators, or an empty id.
pub fn validate_session_id(id: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        anyhow::bail!("session id cannot be empty");
    }
    // Path separators and traversal
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("session id contains invalid path characters");
    }
    // Unicode path separators (U+FF0F, U+FF3C)
    if id.contains('\u{FF0F}') || id.contains('\u{FF3C}') {
        anyhow::bail!("session id contains invalid path characters");
    }
    // Single dot
    if trimmed == "." {
        anyhow::bail!("session id contains invalid path characters");
    }
    // Trailing dots/spaces (Windows treats "foo." == "foo")
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        anyhow::bail!("session id cannot end with '.' or space");
    }
    // Windows reserved device names (case-insensitive)
    let lower = trimmed.to_ascii_lowercase();
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul",
        "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
        "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    // Also match with extensions (e.g., "nul.txt", "CON.log")
    let base = lower.split('.').next().unwrap_or("");
    if RESERVED.contains(&base) {
        anyhow::bail!("session id uses a reserved device name");
    }
    // Alternate Data Streams (file:stream)
    if id.contains(':') && !id.starts_with("https://") && !id.starts_with("http://") {
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
        let mut sessions = lock(&self.sessions);
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

    /// The cached persistence handle for a session, if it is open. Read-only
    /// callers use this instead of `get_or_create`: opening a session that a
    /// live agent already owns would give the reader a second file handle
    /// writing to the same history.
    pub fn peek(&self, session_id: &str) -> Option<Arc<JsonlChatPersistence>> {
        lock(&self.sessions).get(session_id).cloned()
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
        lock(&self.sessions).remove(session_id);
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
        lock(&self.sessions).remove(old_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use xai_chat_state::persistence::ChatPersistence;

    fn temp_app(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("maverick-session-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn history_file(app: &Path, id: &str) -> PathBuf {
        app.join("sessions").join(id).join("chat_history.jsonl")
    }

    #[test]
    fn envelope_round_trip_keeps_timestamps_and_reads_legacy() {
        let app = temp_app("envelope");
        let path = history_file(&app, "s1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Legacy bare-item line (no timestamp recorded) plus an envelope line.
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&ConversationItem::user("old promise")).unwrap(),
                serde_json::to_string(&PersistedLine {
                    ts: 1_700_000_000_000,
                    item: ConversationItem::user("new promise"),
                })
                .unwrap()
            ),
        )
        .unwrap();

        let p = JsonlChatPersistence::new("s1".to_string(), app.clone()).unwrap();
        let paired = p.history_with_timestamps();
        assert_eq!(paired.len(), 2);
        assert_eq!(paired[0].0, 0, "legacy line carries no timestamp");
        assert_eq!(paired[1].0, 1_700_000_000_000);
        assert_eq!(p.history().len(), 2);
    }

    #[test]
    fn rewrite_preserves_each_surviving_item_timestamp() {
        let app = temp_app("rewrite");
        let mut p = JsonlChatPersistence::new("s2".to_string(), app.clone()).unwrap();
        let kept = ConversationItem::user("keep me");
        let dropped = ConversationItem::user("drop me");
        p.persist_message(&kept);
        p.persist_message(&dropped);
        let original_ts = p.history_with_timestamps()[0].0;
        assert!(original_ts > 0, "persist_message stamps the wall clock");

        // Compaction rewrites the history minus one item; the survivor must
        // keep its original stamp rather than re-appearing as brand new.
        p.replace_history(std::slice::from_ref(&kept));
        let after = p.history_with_timestamps();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].0, original_ts);

        // And the rewrite is what's on disk, in the envelope format.
        let lines: Vec<String> = std::fs::read_to_string(history_file(&app, "s2"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(lines.len(), 1);
        let parsed: PersistedLine = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed.ts, original_ts);

        // Reloading the file reproduces the same pairs.
        let reloaded = JsonlChatPersistence::new("s2".to_string(), app).unwrap();
        let reloaded = reloaded.history_with_timestamps();
        assert_eq!(reloaded.len(), after.len());
        for (i, (ts, item)) in reloaded.iter().enumerate() {
            assert_eq!(*ts, after[i].0);
            assert_eq!(
                serde_json::to_string(item).unwrap(),
                serde_json::to_string(&after[i].1).unwrap()
            );
        }
    }

    #[test]
    fn unreadable_lines_warn_but_do_not_sink_the_session() {
        let app = temp_app("corrupt");
        let path = history_file(&app, "s3");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(
                "not json at all\n{}\n",
                serde_json::to_string(&ConversationItem::user("still here")).unwrap()
            ),
        )
        .unwrap();
        let p = JsonlChatPersistence::new("s3".to_string(), app).unwrap();
        assert_eq!(p.history().len(), 1);
    }

    #[test]
    fn manager_peek_never_creates_a_session_directory() {
        let app = temp_app("peek");
        let manager = SessionManager::new(app.clone()).unwrap();
        assert!(manager.peek("nope").is_none());
        assert!(!app.join("sessions").join("nope").exists());

        let opened = manager.get_or_create("yes").unwrap();
        assert!(Arc::ptr_eq(&manager.peek("yes").unwrap(), &opened));
    }

    #[test]
    fn session_id_rejects_path_escapes() {
        assert!(validate_session_id("../escape").is_err());
        assert!(validate_session_id("a/b").is_err());
        assert!(validate_session_id("  ").is_err());
        assert!(validate_session_id("fine-2026").is_ok());
    }
}
