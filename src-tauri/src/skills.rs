//! Maverick Skills — local + hub + marketplace-backed SKILL.md
//!
//! Thin wrapper over vendored `xai-grok-tools` discovery. Scans
//! known skill roots, parses frontmatter via the vendored parser,
//! and exposes CRUD + hub fetch. Hub cache is injected as
//! `SkillScope::Server` so dedup/priority just works.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use xai_grok_tools::implementations::skills::discovery::{
    find_skill_md_paths, parse_skill_files,
};
use xai_grok_tools::implementations::skills::types::{SkillInfo, SkillScope};

/// Serializable view for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDto {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub path: String,
    pub scope: String,
    pub enabled: bool,
    pub plugin_name: Option<String>,
    pub when_to_use: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}

impl From<&SkillInfo> for SkillDto {
    fn from(s: &SkillInfo) -> Self {
        let scope = format!("{:?}", s.scope).to_lowercase();
        Self {
            name: s.name.clone(),
            display_name: s.display_name.clone(),
            description: s.description.clone(),
            path: s.path.clone(),
            scope,
            enabled: s.enabled,
            plugin_name: s.plugin_name.clone(),
            when_to_use: s.when_to_use.clone(),
            allowed_tools: s.allowed_tools.clone(),
        }
    }
}

/// Hub manifest entry (agentskills.io spec minimal).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubIndex {
    pub skills: HashMap<String, HubEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubEntry {
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub path: String, // cache relative
}

#[allow(dead_code)]
fn maverick_home(app_data_dir: &Path) -> PathBuf {
    app_data_dir.to_path_buf()
}

fn home_skill_roots(app_data_dir: &Path, ws_dir: &Path) -> Vec<(PathBuf, SkillScope)> {
    let mut roots = Vec::new();
    // Workspace roots
    for sub in [".maverick/skills", "skills", ".grok/skills", ".agents/skills"] {
        roots.push((ws_dir.join(sub), SkillScope::Local));
    }
    // Repo root (ws parent) - if ws is workspace, parent may be repo
    if let Some(parent) = ws_dir.parent() {
        for sub in [".maverick/skills", "skills"] {
            roots.push((parent.join(sub), SkillScope::Repo));
        }
    }
    // User home — app_data_dir is temp/maverick-app, use home_dir
    if let Some(home) = dirs_next::home_dir() {
        roots.push((home.join(".maverick").join("skills"), SkillScope::User));
        // Compat: ~/.grok/skills
        roots.push((home.join(".grok").join("skills"), SkillScope::User));
        roots.push((home.join(".agents").join("skills"), SkillScope::User));
    }
    // App data dir itself as User-like (for temp installs)
    roots.push((app_data_dir.join("skills"), SkillScope::User));
    // Hub cache as Server scope
    if let Some(home) = dirs_next::home_dir() {
        roots.push((home.join(".maverick").join("hub").join("skills"), SkillScope::Server));
    }
    roots.push((app_data_dir.join("hub").join("skills"), SkillScope::Server));
    // Bundled (lowest)
    if let Some(home) = dirs_next::home_dir() {
        roots.push((home.join(".maverick").join("bundled").join("skills"), SkillScope::Bundled));
    }
    roots
}

/// Discover all skills from known roots using vendored parser.
pub fn discover_skills(app_data_dir: &Path, ws_dir: &Path) -> Vec<SkillInfo> {
    let roots = home_skill_roots(app_data_dir, ws_dir);
    let mut files: Vec<(PathBuf, SkillScope)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for (root, scope) in roots {
        if !root.exists() {
            continue;
        }
        // find_skill_md_paths handles both dir/SKILL.md and dir/skills/*/SKILL.md
        let found = find_skill_md_paths(&root);
        for p in found {
            // Also handle case where root itself is a skills/ parent containing many SKILL.md
            // find_skill_md_paths already walks depth 5.
            if seen.insert(p.clone()) {
                files.push((p, scope.clone()));
            }
        }
        // Also check if root/skills sub-subdirs via find via walk (redundant safety)
        // For hub cache nested owner/name/version/SKILL.md, need deeper walk
        if root.ends_with("hub/skills") {
            walk_hub_recursive(&root, &mut files, &mut seen);
        }
    }
    if files.is_empty() {
        return vec![];
    }
    let mut skills = parse_skill_files(files);
    // Dedupe by name keeping highest priority scope (Local < Repo < User < Server < Bundled)
    // parse already dedups by canonical path, but we need name dedup.
    skills.sort_by(|a, b| a.scope.cmp(&b.scope));
    let mut by_name: HashMap<String, SkillInfo> = HashMap::new();
    for s in skills {
        by_name.entry(s.name.clone()).or_insert(s);
    }
    let mut out: Vec<SkillInfo> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn walk_hub_recursive(root: &Path, files: &mut Vec<(PathBuf, SkillScope)>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Check for SKILL.md inside 2-3 levels
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() && seen.insert(skill_md.clone()) {
                files.push((skill_md, SkillScope::Server));
            } else {
                // Recurse one more level for owner/name/version
                if let Ok(sub) = std::fs::read_dir(&path) {
                    for sub_entry in sub.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_dir() {
                            let md = sub_path.join("SKILL.md");
                            if md.exists() && seen.insert(md.clone()) {
                                files.push((md, SkillScope::Server));
                            } else if let Ok(sub2) = std::fs::read_dir(&sub_path) {
                                for e2 in sub2.flatten() {
                                    let p2 = e2.path();
                                    if p2.is_dir() {
                                        let md2 = p2.join("SKILL.md");
                                        if md2.exists() && seen.insert(md2.clone()) {
                                            files.push((md2, SkillScope::Server));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Install a skill from a SKILL.md file or raw markdown content.
pub fn install_skill(
    app_data_dir: &Path,
    name: &str,
    content: &str,
    scope: Option<String>,
) -> Result<PathBuf> {
    let safe_name = sanitize_skill_name(name)?;
    let base = match scope.as_deref() {
        Some("repo") => {
            // Use workspace dir via app_data? Fallback to app_data/skills
            app_data_dir.join("skills")
        }
        _ => {
            if let Some(home) = dirs_next::home_dir() {
                let p = home.join(".maverick").join("skills").join(&safe_name);
                std::fs::create_dir_all(&p)?;
                let _ = p.join("SKILL.md");
                p
            } else {
                app_data_dir.join("skills").join(&safe_name)
            }
        }
    };
    // Handle both cases: base already is dir, need to ensure
    let dir = if base.ends_with("SKILL.md") {
        base.parent().unwrap().to_path_buf()
    } else if base.extension().is_some() {
        base.parent().unwrap().to_path_buf()
    } else {
        base
    };
    let _dir = if dir.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
        dir.parent().unwrap().to_path_buf()
    } else {
        dir
    };
    // Actually recompute cleanly
    let target_dir = if let Some(home) = dirs_next::home_dir() {
        home.join(".maverick").join("skills").join(&safe_name)
    } else {
        app_data_dir.join("skills").join(&safe_name)
    };
    std::fs::create_dir_all(&target_dir)?;
    let target = target_dir.join("SKILL.md");
    // Ensure content has frontmatter name
    let final_content = if content.trim_start().starts_with("---") {
        content.to_string()
    } else {
        format!("---\nname: {safe_name}\ndescription: Custom skill {safe_name}\n---\n{content}")
    };
    std::fs::write(&target, final_content)?;
    Ok(target)
}

pub fn remove_skill(app_data_dir: &Path, name: &str) -> Result<()> {
    let safe = sanitize_skill_name(name)?;
    let mut candidates = Vec::new();
    if let Some(home) = dirs_next::home_dir() {
        candidates.push(home.join(".maverick").join("skills").join(&safe));
        candidates.push(home.join(".maverick").join("hub").join("skills").join(&safe));
    }
    candidates.push(app_data_dir.join("skills").join(&safe));
    candidates.push(app_data_dir.join("hub").join("skills").join(&safe));
    // Also search hub owner/name
    if let Some(home) = dirs_next::home_dir() {
        let hub_root = home.join(".maverick").join("hub").join("skills");
        if hub_root.exists() {
            if let Ok(entries) = std::fs::read_dir(&hub_root) {
                for owner in entries.flatten() {
                    candidates.push(owner.path().join(&safe));
                }
            }
        }
    }
    let mut removed = false;
    for c in candidates {
        if c.exists() {
            std::fs::remove_dir_all(&c)?;
            removed = true;
        }
        let md = c.join("SKILL.md");
        if md.exists() {
            let _ = std::fs::remove_file(&md);
            removed = true;
        }
    }
    if !removed {
        anyhow::bail!("Skill '{}' not found", name);
    }
    Ok(())
}

fn sanitize_skill_name(name: &str) -> Result<String> {
    let n = name.trim().to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "-");
    let n = n.trim_matches('-').to_string();
    if n.is_empty() || n.len() > 64 {
        anyhow::bail!("Invalid skill name '{}'", name);
    }
    if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("Invalid skill name '{}'", name);
    }
    Ok(n)
}

/// Hub fetch: GET https://agentskills.io/{owner}/{name}/SKILL.md (or raw github) and cache.
/// For MVP we support agentskills.io and raw github via simple GET.
pub async fn fetch_hub_skill(
    app_data_dir: &Path,
    hub_url: &str,
    owner: &str,
    name: &str,
    version: Option<String>,
) -> Result<SkillDto> {
    let version_clone = version.clone();
    let url = if hub_url.contains("agentskills.io") {
        if let Some(v) = version_clone.clone() {
            format!("{}/{}/{}/SKILL.md?version={v}", hub_url.trim_end_matches('/'), owner, name)
        } else {
            format!("{}/{}/{}/SKILL.md", hub_url.trim_end_matches('/'), owner, name)
        }
    } else {
        hub_url.to_string()
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Hub fetch failed HTTP {}", resp.status());
    }
    let content = resp.text().await?;
    // Cache under hub/skills/owner/name[/version]/SKILL.md
    let cache_dir = if let Some(home) = dirs_next::home_dir() {
        let mut p = home.join(".maverick").join("hub").join("skills").join(owner).join(name);
        if let Some(v) = version_clone.clone() {
            p = p.join(v);
        }
        p
    } else {
        let mut p = app_data_dir.join("hub").join("skills").join(owner).join(name);
        if let Some(v) = version_clone.clone() {
            p = p.join(v);
        }
        p
    };
    std::fs::create_dir_all(&cache_dir)?;
    std::fs::write(cache_dir.join("SKILL.md"), &content)?;
    // Parse to return DTO
    let tmp_path = cache_dir.join("SKILL.md");
    let parsed = parse_skill_files(vec![(tmp_path.clone(), SkillScope::Server)]);
    if let Some(s) = parsed.first() {
        Ok(SkillDto::from(s))
    } else {
        // Fallback: install as local skill with owner/name
        let full_name = format!("{owner}-{name}");
        install_skill(app_data_dir, &full_name, &content, Some("hub".to_string()))?;
        let skills = discover_skills(app_data_dir, &PathBuf::from("."));
        let dto = skills.iter().find(|s| s.name == full_name).map(SkillDto::from).unwrap_or(SkillDto {
            name: full_name,
            display_name: None,
            description: "Hub skill".to_string(),
            path: cache_dir.join("SKILL.md").to_string_lossy().to_string(),
            scope: "server".to_string(),
            enabled: true,
            plugin_name: None,
            when_to_use: None,
            allowed_tools: None,
        });
        Ok(dto)
    }
}

pub fn list_hub_index(app_data_dir: &Path) -> HubIndex {
    let mut idx = HubIndex::default();
    let hub_root = if let Some(home) = dirs_next::home_dir() {
        home.join(".maverick").join("hub").join("skills")
    } else {
        app_data_dir.join("hub").join("skills")
    };
    if !hub_root.exists() {
        return idx;
    }
    // Walk hub skills and build index
    for owner in std::fs::read_dir(&hub_root).into_iter().flatten().flatten() {
        let owner_name = owner.file_name().to_string_lossy().to_string();
        for skill in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
            let skill_name = skill.file_name().to_string_lossy().to_string();
            let entry_path = skill.path().join("SKILL.md");
            if entry_path.exists() {
                idx.skills.insert(format!("{owner_name}/{skill_name}"), HubEntry {
                    version: "latest".to_string(),
                    description: "cached hub skill".to_string(),
                    author: Some(owner_name.clone()),
                    path: entry_path.to_string_lossy().to_string(),
                });
            } else {
                // Check version subdirs
                for ver in std::fs::read_dir(skill.path()).into_iter().flatten().flatten() {
                    let v_path = ver.path().join("SKILL.md");
                    if v_path.exists() {
                        idx.skills.insert(format!("{owner_name}/{skill_name}@{}", ver.file_name().to_string_lossy()), HubEntry {
                            version: ver.file_name().to_string_lossy().to_string(),
                            description: "cached hub skill".to_string(),
                            author: Some(owner_name.clone()),
                            path: v_path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }
    }
    idx
}
