//! Per-session workspace mapping: which folder each chat works in.
//!
//! Stored in `<app_data>/session_workspaces.json`:
//! `{ sessions: {id: abs_path}, default: abs_path|null, recent: [abs…] }`.
//! An absent file means today's behavior everywhere (global resolver), so
//! there is no migration. Paths are validated live (must exist, must be a
//! directory); stale entries self-drop on read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

const WORKSPACES_FILE: &str = "session_workspaces.json";
const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkspaceMap {
    #[serde(default)]
    sessions: HashMap<String, String>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    recent: Vec<String>,
}

/// File-backed session → workspace mapping rooted at the app-data dir.
#[derive(Debug, Clone)]
pub struct WorkspaceStore {
    path: PathBuf,
}

impl WorkspaceStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            path: app_data_dir.join(WORKSPACES_FILE),
        }
    }

    fn load(&self) -> WorkspaceMap {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, map: &WorkspaceMap) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(map)?)?;
        Ok(())
    }

    /// Validate a candidate workspace: must exist and be a directory.
    /// Returns the canonical absolute path (forward slashes).
    pub fn validate(path: &str) -> Result<String, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("workspace path required".to_string());
        }
        let canon = Path::new(trimmed)
            .canonicalize()
            .map_err(|_| format!("folder not found: {trimmed}"))?;
        if !canon.is_dir() {
            return Err(format!("not a folder: {trimmed}"));
        }
        Ok(canon.to_string_lossy().replace('\\', "/"))
    }

    fn touch_recent(map: &mut WorkspaceMap, canon: &str) {
        map.recent.retain(|r| r != canon);
        map.recent.insert(0, canon.to_string());
        map.recent.truncate(MAX_RECENT);
    }

    /// Effective override for a session: the mapping, else the stored
    /// default — unless `MAVERICK_WORKSPACE_DIR` is set, which always wins
    /// (explicit launch-time intent). `None` means "use the live global
    /// resolver". Stale entries (moved/deleted folders) self-drop.
    pub fn effective_session_dir(&self, session_id: &str) -> Option<PathBuf> {
        let mut map = self.load();
        let mut dirty = false;
        if let Some(raw) = map.sessions.get(session_id).cloned() {
            match Self::validate(&raw) {
                Ok(canon) => return Some(PathBuf::from(canon)),
                Err(_) => {
                    map.sessions.remove(session_id);
                    dirty = true;
                }
            }
        }
        if std::env::var("MAVERICK_WORKSPACE_DIR")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            if dirty {
                let _ = self.save(&map);
            }
            return None;
        }
        let out = match map.default.clone() {
            Some(raw) => match Self::validate(&raw) {
                Ok(canon) => Some(PathBuf::from(canon)),
                Err(_) => {
                    map.default = None;
                    dirty = true;
                    None
                }
            },
            None => None,
        };
        if dirty {
            let _ = self.save(&map);
        }
        out
    }

    /// Raw mapping for a session (no default fallback), validated live.
    pub fn session_workspace(&self, session_id: &str) -> Option<String> {
        let map = self.load();
        let raw = map.sessions.get(session_id)?.clone();
        match Self::validate(&raw) {
            Ok(canon) => Some(canon),
            Err(_) => {
                let mut map = map;
                map.sessions.remove(session_id);
                let _ = self.save(&map);
                None
            }
        }
    }

    /// Set (or clear with `None`) a session's workspace. Returns the
    /// canonical path when set.
    pub fn set_session_workspace(
        &self,
        session_id: &str,
        path: Option<&str>,
    ) -> Result<Option<String>, String> {
        let mut map = self.load();
        let out = match path {
            Some(p) => {
                let canon = Self::validate(p)?;
                map.sessions.insert(session_id.to_string(), canon.clone());
                Self::touch_recent(&mut map, &canon);
                Some(canon)
            }
            None => {
                map.sessions.remove(session_id);
                None
            }
        };
        self.save(&map).map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Drop a session's mapping (call on session delete).
    pub fn remove_session(&self, session_id: &str) -> Result<()> {
        let mut map = self.load();
        if map.sessions.remove(session_id).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }

    /// Stored default workspace, validated live (self-drops when stale).
    pub fn default_workspace(&self) -> Option<String> {
        let map = self.load();
        let raw = map.default.clone()?;
        match Self::validate(&raw) {
            Ok(canon) => Some(canon),
            Err(_) => {
                let mut map = map;
                map.default = None;
                let _ = self.save(&map);
                None
            }
        }
    }

    /// Global workspace dir honoring the stored default:
    /// `MAVERICK_WORKSPACE_DIR` → stored default → `<cwd>/workspace`.
    /// This replaces bare `resolve_workspace_dir()` everywhere user-visible.
    pub fn global_dir(&self) -> PathBuf {
        if std::env::var("MAVERICK_WORKSPACE_DIR")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            return crate::tools::resolve_workspace_dir();
        }
        if let Some(dir) = self.default_workspace().map(PathBuf::from) {
            return dir;
        }
        crate::tools::resolve_workspace_dir()
    }

    /// Set (or clear with `None`) the default workspace.
    pub fn set_default_workspace(&self, path: Option<&str>) -> Result<Option<String>, String> {
        let mut map = self.load();
        let out = match path {
            Some(p) => {
                let canon = Self::validate(p)?;
                map.default = Some(canon.clone());
                Self::touch_recent(&mut map, &canon);
                Some(canon)
            }
            None => {
                map.default = None;
                None
            }
        };
        self.save(&map).map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Recent workspaces, pruned to folders that still exist.
    pub fn recent(&self) -> Vec<String> {
        self.load()
            .recent
            .into_iter()
            .filter(|r| Path::new(r).is_dir())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(name: &str) -> WorkspaceStore {
        let dir = std::env::temp_dir()
            .join("maverick-workspace-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        WorkspaceStore::new(&dir)
    }

    fn test_folder(store_dir: &std::path::Path, name: &str) -> String {
        let d = store_dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d.to_string_lossy().into_owned()
    }

    #[test]
    fn validate_rejects_missing_and_files() {
        assert!(WorkspaceStore::validate("").is_err());
        assert!(WorkspaceStore::validate("/definitely/not/here-xyz").is_err());
        let dir = std::env::temp_dir().join(format!("mws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(WorkspaceStore::validate(&file.to_string_lossy()).is_err());
        assert!(WorkspaceStore::validate(&dir.to_string_lossy()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_mapping_round_trip_and_clear() {
        let store = test_store("roundtrip");
        let app = store.path.parent().unwrap();
        let folder = test_folder(app, "proj");
        assert_eq!(store.session_workspace("s1"), None);
        let canon = store
            .set_session_workspace("s1", Some(&folder))
            .unwrap()
            .unwrap();
        assert_eq!(store.session_workspace("s1").as_deref(), Some(canon.as_str()));
        assert!(store.recent().contains(&canon));
        store.set_session_workspace("s1", None).unwrap();
        assert_eq!(store.session_workspace("s1"), None);
    }

    #[test]
    fn stale_entries_self_drop() {
        let store = test_store("stale");
        let app = store.path.parent().unwrap().to_path_buf();
        let folder = test_folder(&app, "gone");
        store.set_session_workspace("s1", Some(&folder)).unwrap();
        std::fs::remove_dir_all(app.join("gone")).unwrap();
        assert_eq!(store.session_workspace("s1"), None);
        // Gone from the file too, not just the read.
        let map: WorkspaceMap =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert!(!map.sessions.contains_key("s1"));
    }

    #[test]
    fn default_and_recents() {
        let store = test_store("defaults");
        let app = store.path.parent().unwrap();
        let a = test_folder(app, "a");
        let b = test_folder(app, "b");
        assert_eq!(store.default_workspace(), None);
        store.set_default_workspace(Some(&a)).unwrap();
        store.set_session_workspace("s1", Some(&b)).unwrap();
        assert!(store.default_workspace().is_some());
        let recent = store.recent();
        // Most-recent first, capped and deduped.
        assert_eq!(recent[0], WorkspaceStore::validate(&b).unwrap());
        assert!(recent.len() <= MAX_RECENT);
    }

    #[test]
    fn remove_session_drops_mapping() {
        let store = test_store("remove");
        let app = store.path.parent().unwrap();
        let folder = test_folder(app, "proj");
        store.set_session_workspace("s1", Some(&folder)).unwrap();
        store.remove_session("s1").unwrap();
        // Raw file no longer carries it.
        let map: WorkspaceMap =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert!(!map.sessions.contains_key("s1"));
    }
}
