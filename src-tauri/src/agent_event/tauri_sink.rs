//! Tauri-specific [`AgentEventSink`] implementation.

use tauri::{AppHandle, Emitter};

use super::{AgentEvent, AgentEventSink};

/// Sink that forwards agent events to the Tauri frontend.
pub struct TauriSink {
    app: AppHandle,
    session_id: String,
}

impl TauriSink {
    pub fn new(app: AppHandle, session_id: String) -> Self {
        Self { app, session_id }
    }
}

#[async_trait::async_trait]
impl AgentEventSink for TauriSink {
    async fn on_event(&self, event: AgentEvent) {
        // Include session_id in the payload so the frontend can route it
        let payload = serde_json::json!({
            "session_id": self.session_id,
            "event": event,
        });
        let _ = self.app.emit("agent-event", payload);
    }
}
