//! Unified cross-session memory: global + per-workspace `MEMORY.md` files.
//!
//! The agent loop injects the combined memory text into every request's
//! `<memory-context>` slot (the vendored builder already supports it —
//! Maverick previously always passed `None`). Memories are written by three
//! paths: post-run background extraction, the `memory_save` /
//! `memory_forget` agent tools, and the Settings editor (via commands).
//!
//! Files are plain Markdown so users can read and edit them by hand:
//! `<app_data>/memory/MEMORY.md` plus
//! `<app_data>/memory/workspaces/<slug>-<hash>/MEMORY.md`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::MemoryScope;
use xai_grok_sampling_types::{ContentPart, ConversationItem};

/// Subdirectory of the app-data dir holding every memory file.
pub const MEMORY_DIR_NAME: &str = "memory";
/// File name for both the global and the per-workspace memory.
pub const MEMORY_FILE_NAME: &str = "MEMORY.md";
/// Sections the store merges bullets into.
pub const MEMORY_SECTIONS: [&str; 3] = ["Facts", "Preferences", "Notes"];
/// Marker prepended when the combined text is truncated to the config cap.
pub const MEMORY_TRUNCATION_MARKER: &str = "[... older memories omitted for context ...]";

const MEMORY_FILE_TEMPLATE: &str = "# Maverick Memory\n\n\
> Managed by Maverick unified memory. Edit freely — the Facts, Preferences\n\
> and Notes sections below are merged automatically across chats.\n";

/// Which file(s) a write targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFileScope {
    Global,
    Workspace,
}

/// Serializable stats for the Settings UI / composer indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub global_bullets: usize,
    pub global_chars: usize,
    pub workspace_name: String,
    pub workspace_bullets: usize,
    pub workspace_chars: usize,
}

/// File-backed memory store rooted at `<app_data_dir>/memory`.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    app_data_dir: PathBuf,
}

impl MemoryStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self { app_data_dir }
    }

    fn dir(&self) -> PathBuf {
        self.app_data_dir.join(MEMORY_DIR_NAME)
    }

    pub fn global_path(&self) -> PathBuf {
        self.dir().join(MEMORY_FILE_NAME)
    }

    pub fn workspace_path(&self, workspace_dir: &Path) -> PathBuf {
        self.dir()
            .join("workspaces")
            .join(workspace_key(workspace_dir))
            .join(MEMORY_FILE_NAME)
    }

    fn read_text(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn write_text(path: &Path, text: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Combined memory text for injection: global and/or workspace file with
    /// scope labels, capped at `max_chars` (head kept for prompt-cache
    /// stability, cut at a line boundary). Empty when nothing is stored, so
    /// callers can pass `None` as the request reminder.
    pub fn load_combined(
        &self,
        scope: MemoryScope,
        workspace_dir: Option<&Path>,
        max_chars: usize,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        if scope.includes_global() {
            let text = Self::read_text(&self.global_path());
            if !text.trim().is_empty() {
                parts.push(format!("### Global memory\n{text}"));
            }
        }
        if scope.includes_workspace() {
            if let Some(ws) = workspace_dir {
                let text = Self::read_text(&self.workspace_path(ws));
                if !text.trim().is_empty() {
                    let name = ws
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "workspace".to_string());
                    parts.push(format!("### Workspace memory ({name})\n{text}"));
                }
            }
        }
        let combined = parts.join("\n\n");
        truncate_head(&combined, max_chars)
    }

    /// Append `bullets` under `section` (created when missing). Bullets
    /// already present anywhere in the file (normalized) are skipped.
    /// Returns the number of bullets actually added.
    pub fn append_bullets(
        &self,
        file_scope: MemoryFileScope,
        workspace_dir: Option<&Path>,
        section: &str,
        bullets: &[String],
    ) -> Result<usize> {
        let path = match file_scope {
            MemoryFileScope::Global => self.global_path(),
            MemoryFileScope::Workspace => match workspace_dir {
                Some(ws) => self.workspace_path(ws),
                None => self.global_path(),
            },
        };
        let section = normalize_section(section);
        let mut text = Self::read_text(&path);
        if text.trim().is_empty() {
            text = MEMORY_FILE_TEMPLATE.to_string();
        }
        let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let existing: std::collections::HashSet<String> =
            lines.iter().filter_map(|l| bullet_text(l)).map(dedupe_key).collect();

        let fresh: Vec<String> = bullets
            .iter()
            .map(|b| b.trim().trim_start_matches(['-', '*']).trim().to_string())
            .filter(|b| !b.is_empty() && !existing.contains(&dedupe_key(b)))
            .collect();
        if fresh.is_empty() {
            return Ok(0);
        }

        let header = format!("## {section}");
        let mut idx = lines
            .iter()
            .position(|l| is_section_header(l, section));
        if idx.is_none() {
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            idx = Some(lines.len() - 1);
        }
        // End of the section = next `## ` header or EOF.
        let start = idx.unwrap_or(0) + 1;
        let mut end = lines.len();
        for (i, line) in lines.iter().enumerate().skip(start) {
            if line.trim_start().starts_with("## ") {
                end = i;
                break;
            }
        }
        let mut insert_at = end;
        // Keep exactly one blank line before the next section header.
        for bullet in &fresh {
            lines.insert(insert_at, format!("- {bullet}"));
            insert_at += 1;
        }
        let mut out = lines.join("\n");
        out.push('\n');
        Self::write_text(&path, &out)?;
        Ok(fresh.len())
    }

    /// Remove every bullet line containing `pattern` (case-insensitive).
    /// `both` searches the global and the workspace file. Returns removals.
    pub fn remove_matching(
        &self,
        both: bool,
        only_workspace: bool,
        workspace_dir: Option<&Path>,
        pattern: &str,
    ) -> Result<usize> {
        let needle = pattern.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(0);
        }
        let mut paths = Vec::new();
        if !only_workspace {
            paths.push(self.global_path());
        }
        if both || only_workspace {
            if let Some(ws) = workspace_dir {
                paths.push(self.workspace_path(ws));
            }
        }
        let mut removed = 0;
        for path in paths {
            let text = Self::read_text(&path);
            if text.trim().is_empty() {
                continue;
            }
            let mut kept = Vec::new();
            for line in text.lines() {
                if bullet_text(line).is_some_and(|b| b.to_lowercase().contains(&needle)) {
                    removed += 1;
                } else {
                    kept.push(line.to_string());
                }
            }
            if removed > 0 {
                let mut out = kept.join("\n");
                out.push('\n');
                Self::write_text(&path, &out)?;
            }
        }
        Ok(removed)
    }

    /// Overwrite one memory file verbatim (Settings editor).
    pub fn overwrite(
        &self,
        file_scope: MemoryFileScope,
        workspace_dir: Option<&Path>,
        text: &str,
    ) -> Result<()> {
        let path = match file_scope {
            MemoryFileScope::Global => self.global_path(),
            MemoryFileScope::Workspace => match workspace_dir {
                Some(ws) => self.workspace_path(ws),
                None => self.global_path(),
            },
        };
        Self::write_text(&path, text)
    }

    /// Delete the global and/or workspace memory file.
    pub fn clear(
        &self,
        both: bool,
        only_workspace: bool,
        workspace_dir: Option<&Path>,
    ) -> Result<()> {
        if !only_workspace {
            let _ = std::fs::remove_file(self.global_path());
        }
        if both || only_workspace {
            if let Some(ws) = workspace_dir {
                let _ = std::fs::remove_file(self.workspace_path(ws));
            }
        }
        Ok(())
    }

    pub fn stats(&self, workspace_dir: Option<&Path>) -> MemoryStats {
        let global = Self::read_text(&self.global_path());
        let (ws_name, ws_bullets, ws_chars) = match workspace_dir {
            Some(ws) => {
                let text = Self::read_text(&self.workspace_path(ws));
                let name = ws
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "workspace".to_string());
                (name, count_bullets(&text), text.len())
            }
            None => ("-".to_string(), 0, 0),
        };
        MemoryStats {
            global_bullets: count_bullets(&global),
            global_chars: global.len(),
            workspace_name: ws_name,
            workspace_bullets: ws_bullets,
            workspace_chars: ws_chars,
        }
    }
}

/// Stable, human-readable key for a workspace dir: `<slug>-<16 hex fnv1a>`.
/// FNV-1a is used instead of `DefaultHasher` on purpose — `DefaultHasher`
/// is randomly seeded per process, which would orphan the workspace file on
/// every restart.
pub fn workspace_key(dir: &Path) -> String {
    let raw = dir.to_string_lossy();
    let slug: String = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = slug.chars().take(24).collect::<String>();
    let slug = if slug.is_empty() { "ws".to_string() } else { slug };
    format!("{slug}-{:016x}", fnv1a64(&raw))
}

fn fnv1a64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Map a free-form section name onto one of [`MEMORY_SECTIONS`].
pub fn normalize_section(input: &str) -> &'static str {
    let lower = input.trim().to_lowercase();
    let lower = lower
        .trim_start_matches('#')
        .trim()
        .trim_end_matches(':')
        .trim();
    for section in MEMORY_SECTIONS {
        if lower == section.to_lowercase()
            || (section == "Facts" && lower == "fact")
            || (section == "Preferences" && lower == "preference")
            || (section == "Notes" && lower == "note")
        {
            return section;
        }
    }
    "Facts"
}

fn is_section_header(line: &str, section: &str) -> bool {
    let t = line.trim();
    let body = t.trim_start_matches('#').trim().trim_end_matches(':').trim();
    t.starts_with("##") && body.eq_ignore_ascii_case(section)
}

/// Bullet body of a `- ` / `* ` list line, else `None`.
fn bullet_text(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn dedupe_key(bullet: &str) -> String {
    bullet
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn count_bullets(text: &str) -> usize {
    text.lines().filter(|l| bullet_text(l).is_some()).count()
}

/// Keep the head of `text` within `max_chars`, cut at a line boundary with a
/// marker. Head-first keeps the file prefix stable across runs (better
/// prompt-cache reuse than newest-first truncation).
fn truncate_head(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars || max_chars == 0 {
        return text.to_string();
    }
    let mut cut = max_chars.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    // Back up to the previous newline so no line is cut mid-sentence.
    let mut line_cut = cut;
    while line_cut > 0 && !text[..line_cut].ends_with('\n') {
        line_cut -= 1;
        while line_cut > 0 && !text.is_char_boundary(line_cut) {
            line_cut -= 1;
        }
    }
    if line_cut == 0 {
        line_cut = cut;
    }
    format!(
        "{}\n{MEMORY_TRUNCATION_MARKER}",
        text[..line_cut].trim_end()
    )
}

// ─── Extraction ─────────────────────────────────────────────────────────────

/// Prompt asking a cheap model to distill durable memories from a transcript.
/// The model must answer in `SECTION:` / `- bullet` form, or exactly `NONE`.
pub fn build_extraction_prompt(transcript: &str) -> String {
    format!(
        "You maintain long-term memory for a coding assistant. \
Read the conversation below and extract only DURABLE, reusable facts: \
the user's identity, preferences, tooling, and project conventions or \
decisions that should persist across chats. Ignore one-off task details, \
code dumps, and tool output.\n\n\
Reply in exactly this shape (omit empty sections):\n\
FACTS:\n- <fact>\nPREFERENCES:\n- <preference>\nNOTES:\n- <project note>\n\n\
If nothing is worth remembering, reply with exactly: NONE\n\n\
Conversation:\n{transcript}"
    )
}

/// Parse an extraction reply into `(section, bullet)` pairs. Lenient:
/// `#`-style headers and `FACTS:`/`Facts` forms all match; bullets without a
/// preceding header default to Facts. `NONE` (or empty) yields no pairs.
pub fn parse_extraction(text: &str) -> Vec<(String, String)> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current = "Facts";
    for line in trimmed.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let header = t
            .trim_start_matches('#')
            .trim()
            .trim_end_matches(':')
            .trim();
        if !header.is_empty()
            && ["facts", "fact", "preferences", "preference", "notes", "note"]
                .contains(&header.to_lowercase().as_str())
        {
            current = normalize_section(header);
            continue;
        }
        if let Some(body) = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .map(str::trim)
            .filter(|b| !b.is_empty())
        {
            if body.eq_ignore_ascii_case("none") {
                continue;
            }
            let mut bullet = body.to_string();
            if bullet.len() > 500 {
                let mut cut = 500;
                while cut > 0 && !bullet.is_char_boundary(cut) {
                    cut -= 1;
                }
                bullet.truncate(cut);
            }
            out.push((current.to_string(), bullet));
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

/// Render the tail of a transcript for extraction: user + assistant text
/// only (system prompts and tool-result dumps are noise for memory).
/// Capped at `max_chars`, newest content kept.
pub fn render_transcript(items: &[ConversationItem], max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for item in items {
        match item {
            ConversationItem::User(u) => {
                let text: Vec<String> = u
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => {
                            let t = text.trim();
                            (!t.is_empty()).then(|| t.to_string())
                        }
                        ContentPart::Image { .. } => None,
                    })
                    .collect();
                if !text.is_empty() {
                    lines.push(format!("User: {}", text.join("\n")));
                }
            }
            ConversationItem::Assistant(a) => {
                let text = a.content.trim();
                if text.is_empty() && a.tool_calls.is_empty() {
                    continue;
                }
                let mut line = String::from("Assistant: ");
                line.push_str(text);
                for call in &a.tool_calls {
                    line.push_str(&format!(" [called tool: {}]", call.name));
                }
                lines.push(line);
            }
            _ => {}
        }
    }
    let full = lines.join("\n");
    if full.len() <= max_chars {
        return full;
    }
    // Keep the newest content; cut at a line boundary.
    let mut start = full.len() - max_chars.min(full.len());
    while start < full.len() && !full.is_char_boundary(start) {
        start += 1;
    }
    if let Some(nl) = full[start..].find('\n') {
        start += nl + 1;
    }
    full[start..].to_string()
}

// ─── Agent-tool argument parsing ────────────────────────────────────────────

/// Parsed `memory_save` arguments.
#[derive(Debug, Clone)]
pub struct MemorySaveArgs {
    pub text: String,
    pub section: String,
    pub scope: Option<String>,
}

/// Parsed `memory_forget` arguments.
#[derive(Debug, Clone)]
pub struct MemoryForgetArgs {
    pub pattern: String,
    pub scope: Option<String>,
}

pub fn parse_save_args(args: &serde_json::Value) -> Result<MemorySaveArgs, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing required parameter `text` (non-empty string)".to_string())?;
    if text.len() > 2000 {
        return Err("`text` is too long (max 2000 chars); save a concise fact instead".to_string());
    }
    Ok(MemorySaveArgs {
        text: text.to_string(),
        section: args
            .get("section")
            .and_then(|v| v.as_str())
            .map(normalize_section)
            .unwrap_or("Facts")
            .to_string(),
        scope: args
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty()),
    })
}

pub fn parse_forget_args(args: &serde_json::Value) -> Result<MemoryForgetArgs, String> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing required parameter `pattern` (non-empty string)".to_string())?;
    Ok(MemoryForgetArgs {
        pattern: pattern.to_string(),
        scope: args
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty()),
    })
}

// ─── Agent tools ────────────────────────────────────────────────────────────

/// Client-facing tool name for persisting a memory mid-run.
pub const MEMORY_SAVE_TOOL_NAME: &str = "memory_save";
/// Client-facing tool name for deleting memories mid-run.
pub const MEMORY_FORGET_TOOL_NAME: &str = "memory_forget";

/// [`ToolSpec`] advertising `memory_save` to the model.
pub fn memory_save_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: MEMORY_SAVE_TOOL_NAME.to_string(),
        description: Some(
            "Persist a durable fact, user preference, or project note to long-term memory so \
             future chats remember it. Use for identity, preferences, tooling, and project \
             conventions — not one-off task details. Saving is silent: the result confirms \
             what was stored."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Concise fact to remember, e.g. 'User prefers fish shell'"
                },
                "section": {
                    "type": "string",
                    "description": "Facts, Preferences, or Notes (default Facts)"
                },
                "scope": {
                    "type": "string",
                    "description": "'global' or 'workspace' (default follows memory settings)"
                }
            },
            "required": ["text"]
        }),
    }
}

/// [`ToolSpec`] advertising `memory_forget` to the model.
pub fn memory_forget_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: MEMORY_FORGET_TOOL_NAME.to_string(),
        description: Some(
            "Delete memories whose text contains the given pattern (case-insensitive). \
             Use when the user corrects or retracts something previously remembered."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Substring to match against stored memory bullets"
                },
                "scope": {
                    "type": "string",
                    "description": "'global', 'workspace', or omitted for both"
                }
            },
            "required": ["pattern"]
        }),
    }
}

/// Resolve which file a save targets: explicit arg wins, otherwise the
/// configured scope (`Both` defaults to global — personal facts outnumber
/// project notes).
pub fn resolve_save_target(
    scope_arg: Option<&str>,
    cfg: MemoryScope,
) -> MemoryFileScope {
    match scope_arg.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("workspace") => MemoryFileScope::Workspace,
        Some("global") => MemoryFileScope::Global,
        _ => match cfg {
            MemoryScope::Workspace => MemoryFileScope::Workspace,
            _ => MemoryFileScope::Global,
        },
    }
}

/// Resolve which files a forget/clear covers: `(search_both, workspace_only)`.
/// Omitted scope searches both files so a retraction can't miss.
pub fn resolve_forget_target(scope_arg: Option<&str>) -> (bool, bool) {
    match scope_arg.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("global") => (false, false),
        Some("workspace") => (false, true),
        _ => (true, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(name: &str) -> MemoryStore {
        let dir = std::env::temp_dir()
            .join("maverick-memory-tests")
            .join(format!("{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        MemoryStore::new(dir)
    }

    #[test]
    fn append_creates_section_and_merges() {
        let store = test_store("append");
        let added = store
            .append_bullets(
                MemoryFileScope::Global,
                None,
                "Facts",
                &["User prefers Rust".to_string(), "Uses fish shell".to_string()],
            )
            .unwrap();
        assert_eq!(added, 2);
        // Exact dup + case/punctuation-insensitive dup are skipped.
        let added = store
            .append_bullets(
                MemoryFileScope::Global,
                None,
                "facts",
                &[
                    "User prefers Rust".to_string(),
                    "user prefers rust!".to_string(),
                    "Dark mode everywhere".to_string(),
                ],
            )
            .unwrap();
        assert_eq!(added, 1);
        let text = MemoryStore::read_text(&store.global_path());
        assert!(text.contains("## Facts"));
        assert!(text.contains("- Dark mode everywhere"));
    }

    #[test]
    fn load_combined_empty_is_empty() {
        let store = test_store("empty");
        assert_eq!(
            store.load_combined(MemoryScope::Both, None, 8000),
            String::new()
        );
    }

    #[test]
    fn load_combined_respects_scope_and_cap() {
        let store = test_store("scope");
        store
            .append_bullets(
                MemoryFileScope::Global,
                None,
                "Preferences",
                &["Likes concise answers".to_string()],
            )
            .unwrap();
        let ws = Path::new("/tmp/demo-proj");
        store
            .append_bullets(
                MemoryFileScope::Workspace,
                Some(ws),
                "Notes",
                &["Uses pnpm".to_string()],
            )
            .unwrap();
        let both = store.load_combined(MemoryScope::Both, Some(ws), 8000);
        assert!(both.contains("Global memory"));
        assert!(both.contains("Workspace memory"));
        assert!(both.contains("Likes concise answers"));
        assert!(both.contains("Uses pnpm"));
        let global_only = store.load_combined(MemoryScope::Global, Some(ws), 8000);
        assert!(!global_only.contains("Uses pnpm"));
        let tiny = store.load_combined(MemoryScope::Both, Some(ws), 40);
        assert!(tiny.len() <= 40 + MEMORY_TRUNCATION_MARKER.len() + 2);
        assert!(tiny.contains(MEMORY_TRUNCATION_MARKER));
    }

    #[test]
    fn remove_matching_deletes_bullets() {
        let store = test_store("forget");
        store
            .append_bullets(
                MemoryFileScope::Global,
                None,
                "Facts",
                &[
                    "User loves TypeScript".to_string(),
                    "User loves Rust".to_string(),
                ],
            )
            .unwrap();
        let removed = store
            .remove_matching(false, false, None, "typescript")
            .unwrap();
        assert_eq!(removed, 1);
        let text = MemoryStore::read_text(&store.global_path());
        assert!(!text.to_lowercase().contains("typescript"));
        assert!(text.contains("User loves Rust"));
    }

    #[test]
    fn workspace_key_is_stable_and_distinct() {
        let a = workspace_key(Path::new("/tmp/proj"));
        let b = workspace_key(Path::new("/tmp/proj"));
        let c = workspace_key(Path::new("/tmp/other"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("proj-"));
    }

    #[test]
    fn parse_extraction_handles_shapes() {
        assert!(parse_extraction("NONE").is_empty());
        assert!(parse_extraction("  none  ").is_empty());
        let pairs = parse_extraction("FACTS:\n- User is Amir\n## Preferences\n- concise answers\n");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("Facts".to_string(), "User is Amir".to_string()));
        assert_eq!(pairs[1], ("Preferences".to_string(), "concise answers".to_string()));
        // Bullets before any header default to Facts.
        let pairs = parse_extraction("- orphan bullet");
        assert_eq!(pairs[0].0, "Facts");
    }

    #[test]
    fn render_transcript_skips_system_and_tools() {
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("hello"),
            ConversationItem::ToolResult(xai_grok_sampling_types::ToolResultItem {
                tool_call_id: "1".to_string(),
                content: "dump".into(),
                images: vec![],
            }),
        ];
        let text = render_transcript(&items, 8000);
        assert!(text.contains("User: hello"));
        assert!(!text.contains("sys"));
        assert!(!text.contains("dump"));
    }

    #[test]
    fn save_args_require_text() {
        assert!(parse_save_args(&serde_json::json!({})).is_err());
        let args = parse_save_args(
            &serde_json::json!({"text": "x", "section": "hobbies", "scope": "workspace"}),
        )
        .unwrap();
        // Unknown sections fall back to Facts.
        assert_eq!(args.section, "Facts");
        assert_eq!(args.scope.as_deref(), Some("workspace"));
        assert!(parse_forget_args(&serde_json::json!({"pattern": "rust"})).is_ok());
    }

    #[test]
    fn scope_resolution_prefers_explicit_arg() {
        assert_eq!(
            resolve_save_target(Some("workspace"), MemoryScope::Global),
            MemoryFileScope::Workspace
        );
        assert_eq!(
            resolve_save_target(None, MemoryScope::Both),
            MemoryFileScope::Global
        );
        assert_eq!(
            resolve_save_target(None, MemoryScope::Workspace),
            MemoryFileScope::Workspace
        );
        assert_eq!(resolve_forget_target(None), (true, false));
        assert_eq!(resolve_forget_target(Some("global")), (false, false));
        assert_eq!(resolve_forget_target(Some("workspace")), (false, true));
    }
}
