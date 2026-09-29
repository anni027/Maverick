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
use xai_grok_tools::implementations::skills::discovery::{find_skill_md_paths, parse_skill_files};
use xai_grok_tools::implementations::skills::types::{SkillInfo, SkillScope};

use crate::config::MarketplaceSource;

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
    for sub in [
        ".maverick/skills",
        "skills",
        ".grok/skills",
        ".agents/skills",
    ] {
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
        roots.push((
            home.join(".maverick").join("hub").join("skills"),
            SkillScope::Server,
        ));
    }
    roots.push((app_data_dir.join("hub").join("skills"), SkillScope::Server));
    // Bundled (lowest)
    if let Some(home) = dirs_next::home_dir() {
        roots.push((
            home.join(".maverick").join("bundled").join("skills"),
            SkillScope::Bundled,
        ));
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

fn walk_hub_recursive(
    root: &Path,
    files: &mut Vec<(PathBuf, SkillScope)>,
    seen: &mut HashSet<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
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
    // `repo` installs stay inside the app data dir (which discovery scans);
    // everything else goes to the user's skill directory when available.
    let target_dir = match scope.as_deref() {
        Some("repo") => app_data_dir.join("skills").join(&safe_name),
        _ => {
            if let Some(home) = dirs_next::home_dir() {
                home.join(".maverick").join("skills").join(&safe_name)
            } else {
                app_data_dir.join("skills").join(&safe_name)
            }
        }
    };
    std::fs::create_dir_all(&target_dir)?;
    let target = target_dir.join("SKILL.md");
    // Ensure content has frontmatter name
    let final_content = if content.trim_start().starts_with("---") {
        content.to_string()
    } else {
        format!("---\nname: {safe_name}\ndescription: Custom skill {safe_name}\n---\n{content}")
    };
    // Reinstalling must not silently destroy local edits: keep the previous
    // version next to it whenever the content actually changes.
    if let Ok(existing) = std::fs::read_to_string(&target) {
        if existing != final_content {
            std::fs::copy(&target, target_dir.join("SKILL.md.bak"))?;
        }
    }
    std::fs::write(&target, final_content)?;
    Ok(target)
}

pub fn remove_skill(app_data_dir: &Path, name: &str) -> Result<()> {
    let safe = sanitize_skill_name(name)?;
    let mut candidates = Vec::new();
    if let Some(home) = dirs_next::home_dir() {
        candidates.push(home.join(".maverick").join("skills").join(&safe));
        candidates.push(
            home.join(".maverick")
                .join("hub")
                .join("skills")
                .join(&safe),
        );
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
        // And marketplace installs: hub/skills/marketplace/<source>/<dir>,
        // where the dir may be flattened ("a-x") or carry a frontmatter
        // name that differs from the directory name.
        let mp_root = home
            .join(".maverick")
            .join("hub")
            .join("skills")
            .join("marketplace");
        candidates.extend(marketplace_remove_candidates(&mp_root, &safe));
    }
    let mp_root_app = app_data_dir
        .join("hub")
        .join("skills")
        .join("marketplace");
    candidates.extend(marketplace_remove_candidates(&mp_root_app, &safe));
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
    let n = name.trim().to_lowercase().replace(
        |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
        "-",
    );
    let n = n.trim_matches('-').to_string();
    if n.is_empty() || n.len() > 64 {
        anyhow::bail!("Invalid skill name '{}'", name);
    }
    if !n
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("Invalid skill name '{}'", name);
    }
    Ok(n)
}

/// Validate one remote-supplied path segment (`owner`, `name`, `version`)
/// before it is joined into the hub cache directory. Rejects separators,
/// `..`, and anything that is not a plain file-system segment, so a crafted
/// hub reference cannot write outside `hub/skills/`.
fn sanitize_path_segment(value: &str, field: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.len() > 64 {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    if v == "." || v == ".." || v.contains('/') || v.contains('\\') || v.contains('\0') {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    Ok(v.to_string())
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
    // Joined into both the request URL and the local cache path — keep them
    // to single path segments.
    let owner = sanitize_path_segment(owner, "owner")?;
    let name = sanitize_path_segment(name, "name")?;
    let version_clone = match version {
        Some(v) => Some(sanitize_path_segment(&v, "version")?),
        None => None,
    };
    let url = if hub_url.contains("agentskills.io") {
        if let Some(v) = version_clone.clone() {
            format!(
                "{}/{}/{}/SKILL.md?version={v}",
                hub_url.trim_end_matches('/'),
                owner,
                name
            )
        } else {
            format!(
                "{}/{}/{}/SKILL.md",
                hub_url.trim_end_matches('/'),
                owner,
                name
            )
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
        let mut p = home
            .join(".maverick")
            .join("hub")
            .join("skills")
            .join(&owner)
            .join(&name);
        if let Some(v) = version_clone.clone() {
            p = p.join(v);
        }
        p
    } else {
        let mut p = app_data_dir
            .join("hub")
            .join("skills")
            .join(&owner)
            .join(&name);
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
        let dto = skills
            .iter()
            .find(|s| s.name == full_name)
            .map(SkillDto::from)
            .unwrap_or(SkillDto {
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
        for skill in std::fs::read_dir(owner.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            // Skip the marketplace namespace here — those installs are
            // Server scope via discovery, listed by the marketplace UI.
            if owner_name == "marketplace" {
                continue;
            }
            let skill_name = skill.file_name().to_string_lossy().to_string();
            let entry_path = skill.path().join("SKILL.md");
            if entry_path.exists() {
                idx.skills.insert(
                    format!("{owner_name}/{skill_name}"),
                    HubEntry {
                        version: "latest".to_string(),
                        description: "cached hub skill".to_string(),
                        author: Some(owner_name.clone()),
                        path: entry_path.to_string_lossy().to_string(),
                    },
                );
            } else {
                // Check version subdirs
                for ver in std::fs::read_dir(skill.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let v_path = ver.path().join("SKILL.md");
                    if v_path.exists() {
                        idx.skills.insert(
                            format!(
                                "{owner_name}/{skill_name}@{}",
                                ver.file_name().to_string_lossy()
                            ),
                            HubEntry {
                                version: ver.file_name().to_string_lossy().to_string(),
                                description: "cached hub skill".to_string(),
                                author: Some(owner_name.clone()),
                                path: v_path.to_string_lossy().to_string(),
                            },
                        );
                    }
                }
            }
        }
    }
    idx
}

// ── Skills marketplace (GitHub-backed catalog) ────────────────────────────
// A marketplace source is a GitHub repo scanned for SKILL.md files. Browse
// results are cached on disk so listing/searching never needs the network;
// only explicit Refresh and Install hit GitHub (unauthenticated rate limit
// is 60 req/hour, so refreshes are deliberately manual).

/// One browsable skill in a marketplace source catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSkill {
    pub source_id: String,
    /// Repo-relative directory containing SKILL.md, e.g. `skills/pdf`.
    pub dir: String,
    pub name: String,
    pub description: String,
    /// True when this exact install already exists in the hub cache.
    #[serde(default)]
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MarketplaceCatalogCache {
    fetched_at: String,
    skills: Vec<MarketplaceSkill>,
}

/// Hub root, preferring the real home dir (matches `fetch_hub_skill`).
fn hub_root(app_data_dir: &Path) -> PathBuf {
    if let Some(home) = dirs_next::home_dir() {
        home.join(".maverick").join("hub")
    } else {
        app_data_dir.join("hub")
    }
}

fn catalog_cache_path(app_data_dir: &Path, source_id: &str) -> PathBuf {
    hub_root(app_data_dir)
        .join("catalogs")
        .join(format!("{source_id}.json"))
}

fn marketplace_install_dir(app_data_dir: &Path, source_id: &str, flat: &str) -> PathBuf {
    hub_root(app_data_dir)
        .join("skills")
        .join("marketplace")
        .join(source_id)
        .join(flat)
}

/// Extract repo-relative `SKILL.md` directories from a GitHub Trees API
/// (`?recursive=1`) response, restricted to `skills_path`.
///
/// Pure function (no network) so catalog parsing is unit-testable.
/// `skills_path` empty means the repo root. Nested layouts up to two
/// segments below `skills_path` are accepted (covers both flat
/// `skills/<name>` repos and `plugins/*` style repos pointed at a
/// `plugins` root); deeper hits are skipped as unlikely skill dirs.
pub fn marketplace_skill_dirs(tree: &serde_json::Value, skills_path: &str) -> Vec<String> {
    let prefix = skills_path.trim_matches('/');
    let mut dirs: Vec<String> = Vec::new();
    if let Some(items) = tree.get("tree").and_then(|t| t.as_array()) {
        for item in items {
            let kind = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let path = item.get("path").and_then(|p| p.as_str()).unwrap_or("");
            if kind != "blob" || !(path == "SKILL.md" || path.ends_with("/SKILL.md")) {
                continue;
            }
            // Must live under skills_path.
            let rel = if prefix.is_empty() {
                path.to_string()
            } else if let Some(rest) = path.strip_prefix(&format!("{prefix}/")) {
                rest.to_string()
            } else {
                continue;
            };
            let dir = rel
                .strip_suffix("/SKILL.md")
                .unwrap_or("")
                .to_string();
            if dir.is_empty() {
                continue;
            }
            // Depth cap: at most two segments below skills_path.
            if dir.split('/').count() > 2 {
                continue;
            }
            // Rebuild the repo-relative dir for download URLs.
            let full_dir = if prefix.is_empty() {
                dir
            } else {
                format!("{prefix}/{dir}")
            };
            if !dirs.contains(&full_dir) {
                dirs.push(full_dir);
            }
            if dirs.len() >= 2000 {
                break;
            }
        }
    }
    dirs.sort();
    dirs
}

/// Parse `name:` / `description:` out of a SKILL.md frontmatter block.
/// Returns `(None, None)` when there is no frontmatter. Pure/testable.
pub fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    // Which value continuation lines (indented YAML wraps) append to.
    let mut current: Option<bool> = None; // true = name, false = description
    for line in lines {
        if line.trim() == "---" || line.trim() == "..." {
            break;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            name = Some(unquote_frontmatter_value(rest));
            current = Some(true);
        } else if let Some(rest) = line.strip_prefix("description:") {
            description = Some(unquote_frontmatter_value(rest));
            current = Some(false);
        } else if !line.trim().is_empty()
            && line.starts_with(|c: char| c == ' ' || c == '\t')
        {
            // Indented continuation of a wrapped value.
            let piece = line.trim();
            match current {
                Some(true) => {
                    name = Some(format!(
                        "{} {}",
                        name.unwrap_or_default(),
                        piece
                    ));
                }
                Some(false) => {
                    description = Some(format!(
                        "{} {}",
                        description.unwrap_or_default(),
                        piece
                    ));
                }
                None => {}
            }
        } else {
            current = None;
        }
    }
    (name, description)
}

fn unquote_frontmatter_value(raw: &str) -> String {
    let v = raw.trim();
    // Strip a single layer of matching quotes.
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"'))
            || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].trim().to_string()
    } else {
        v.to_string()
    }
}

/// Validate a user-supplied marketplace source before it is stored or used
/// in request URLs / cache paths.
pub fn sanitize_marketplace_source(source: &MarketplaceSource) -> Result<MarketplaceSource> {
    let id = sanitize_skill_name(&source.id)?;
    let owner = sanitize_repo_field(&source.owner, "owner", false)?;
    let repo = sanitize_repo_field(&source.repo, "repo", false)?;
    let branch = sanitize_repo_field(&source.branch, "branch", true)?;
    let skills_path = sanitize_repo_field(&source.skills_path, "skills_path", true)?;
    Ok(MarketplaceSource {
        id,
        display_name: source.display_name.trim().to_string(),
        owner,
        repo,
        branch: if branch.is_empty() {
            "main".to_string()
        } else {
            branch
        },
        skills_path,
    })
}

/// GitHub `owner`/`repo`/`branch`/`path`-ish field validation. `slash_ok`
/// allows `/` (branches and nested skills paths); `..` segments, empty
/// values (except skills_path, handled by the caller… here empty is
/// rejected — the command layer maps "" to root), and oversized values
/// are rejected.
fn sanitize_repo_field(value: &str, field: &str, slash_ok: bool) -> Result<String> {
    // skills_path may legitimately be empty (= repo root).
    if value.is_empty() {
        if field == "skills_path" {
            return Ok(String::new());
        }
        anyhow::bail!("Invalid {field}: empty");
    }
    let v = value.trim().trim_matches('/');
    if v.is_empty() || v.len() > 128 {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    if v.contains('\0') || v.contains('\\') || v.contains(' ') {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    for seg in v.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            anyhow::bail!("Invalid {field} '{value}'");
        }
        if !slash_ok && seg != v {
            anyhow::bail!("Invalid {field} '{value}'");
        }
        if !seg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            anyhow::bail!("Invalid {field} '{value}'");
        }
    }
    if !slash_ok && v.contains('/') {
        anyhow::bail!("Invalid {field} '{value}'");
    }
    Ok(v.to_string())
}

fn github_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("maverick-app")
        .timeout(std::time::Duration::from_secs(20))
        .build()?)
}

fn marketplace_raw_skill_url(source: &MarketplaceSource, dir: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}/SKILL.md",
        source.owner, source.repo, source.branch, dir
    )
}

/// Repo-relative `dir` (e.g. `skills/pdf`) must stay under the source's
/// `skills_path`: no escapes, no absolute paths.
fn validate_marketplace_dir(source: &MarketplaceSource, dir: &str) -> Result<String> {
    let prefix = source.skills_path.trim_matches('/');
    let d = dir.trim().trim_matches('/');
    if d.is_empty() || d.len() > 256 || d.contains('\\') || d.contains('\0') {
        anyhow::bail!("Invalid skill path '{dir}'");
    }
    for seg in d.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            anyhow::bail!("Invalid skill path '{dir}'");
        }
        if !seg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            anyhow::bail!("Invalid skill path '{dir}'");
        }
    }
    if !(d == "SKILL.md"
        || prefix.is_empty()
        || d == prefix
        || d.starts_with(&format!("{prefix}/")))
    {
        anyhow::bail!("Skill path '{dir}' is outside '{prefix}'");
    }
    Ok(d.to_string())
}

/// Flatten a repo-relative skill dir to a single cache segment:
/// `skills/pdf` → `pdf`, `plugins/foo/skills` → `foo-skills`.
fn flatten_marketplace_dir(source: &MarketplaceSource, dir: &str) -> String {
    let prefix = source.skills_path.trim_matches('/');
    let rel = if prefix.is_empty() {
        dir.to_string()
    } else if let Some(rest) = dir.strip_prefix(&format!("{prefix}/")) {
        rest.to_string()
    } else {
        dir.to_string()
    };
    rel.replace('/', "-")
}

fn read_catalog_cache(app_data_dir: &Path, source_id: &str) -> MarketplaceCatalogCache {
    let path = catalog_cache_path(app_data_dir, source_id);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_catalog_cache(
    app_data_dir: &Path,
    source_id: &str,
    cache: &MarketplaceCatalogCache,
) -> Result<()> {
    let path = catalog_cache_path(app_data_dir, source_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(cache)?)?;
    Ok(())
}

/// Fetch (or refresh) a source catalog. Only *new* skill dirs download
/// their SKILL.md; known entries keep cached name/description so a
/// refresh costs ~1 GitHub API request plus one raw fetch per new skill.
pub async fn fetch_marketplace_catalog(
    app_data_dir: &Path,
    source: &MarketplaceSource,
) -> Result<Vec<MarketplaceSkill>> {
    let client = github_client()?;
    let url = format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
        source.owner, source.repo, source.branch
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::FORBIDDEN
        || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        anyhow::bail!("GitHub API rate limit reached — try again later (cached results still work)");
    }
    if !resp.status().is_success() {
        anyhow::bail!("Marketplace catalog fetch failed: HTTP {}", resp.status());
    }
    let tree: serde_json::Value = resp.json().await?;
    if tree.get("truncated").and_then(|t| t.as_bool()).unwrap_or(false) {
        anyhow::bail!("Repo tree is too large to enumerate — narrow `skills_path` for this source");
    }
    let dirs = marketplace_skill_dirs(&tree, &source.skills_path);
    let mut cache = read_catalog_cache(app_data_dir, &source.id);
    let known: HashMap<String, MarketplaceSkill> = cache
        .skills
        .drain(..)
        .map(|s| (s.dir.clone(), s))
        .collect();
    let mut out = Vec::with_capacity(dirs.len());
    for dir in dirs {
        if let Some(hit) = known.get(&dir) {
            out.push(hit.clone());
            continue;
        }
        // New skill: fetch SKILL.md for its frontmatter.
        let content = client
            .get(marketplace_raw_skill_url(source, &dir))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
            .unwrap_or_default();
        let (name, description) = parse_skill_frontmatter(&content);
        let fallback = dir.rsplit('/').next().unwrap_or(&dir).to_string();
        out.push(MarketplaceSkill {
            source_id: source.id.clone(),
            dir: dir.clone(),
            name: name.filter(|n| !n.is_empty()).unwrap_or(fallback),
            description: description.unwrap_or_default(),
            installed: false,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    write_catalog_cache(
        app_data_dir,
        &source.id,
        &MarketplaceCatalogCache {
            fetched_at,
            skills: out.clone(),
        },
    )?;
    Ok(out)
}

/// Serve the cached catalog (empty when never fetched).
pub fn cached_marketplace_catalog(
    app_data_dir: &Path,
    source_id: &str,
) -> Vec<MarketplaceSkill> {
    read_catalog_cache(app_data_dir, source_id).skills
}

/// Install a marketplace skill: download SKILL.md into the hub cache
/// (`hub/skills/marketplace/<source>/<flat>/`) and parse it. MVP installs
/// SKILL.md only — bundled `scripts/`/`references/` are not fetched.
pub async fn install_marketplace_skill(
    app_data_dir: &Path,
    source: &MarketplaceSource,
    dir: &str,
) -> Result<SkillDto> {
    let dir = validate_marketplace_dir(source, dir)?;
    let client = github_client()?;
    let content = client
        .get(marketplace_raw_skill_url(source, &dir))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if content.trim().is_empty() {
        anyhow::bail!("Empty SKILL.md for '{dir}'");
    }
    let flat = flatten_marketplace_dir(source, &dir);
    let safe_flat = sanitize_skill_name(&flat)?;
    let target_dir = marketplace_install_dir(app_data_dir, &source.id, &safe_flat);
    std::fs::create_dir_all(&target_dir)?;
    std::fs::write(target_dir.join("SKILL.md"), &content)?;
    let parsed = parse_skill_files(vec![(
        target_dir.join("SKILL.md"),
        SkillScope::Server,
    )]);
    if let Some(s) = parsed.first() {
        Ok(SkillDto::from(s))
    } else {
        anyhow::bail!("Downloaded SKILL.md for '{dir}' did not parse as a skill");
    }
}

/// Removal candidates under a marketplace hub root: match by flattened
/// dir name or by parsed frontmatter name (which may differ).
fn marketplace_remove_candidates(mp_root: &Path, safe: &str) -> Vec<PathBuf> {    let mut out = Vec::new();
    let Ok(sources) = std::fs::read_dir(mp_root) else {
        return out;
    };
    for source in sources.flatten() {
        let Ok(entries) = std::fs::read_dir(source.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            if entry.file_name().to_string_lossy() == safe {
                out.push(p);
                continue;
            }
            let md = p.join("SKILL.md");
            if let Ok(content) = std::fs::read_to_string(&md) {
                let (name, _) = parse_skill_frontmatter(&content);
                if name.as_deref() == Some(safe) {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod marketplace_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "maverick-mp-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn tree_entry(path: &str, kind: &str) -> serde_json::Value {
        serde_json::json!({"path": path, "type": kind})
    }

    #[test]
    fn skill_dirs_flat_layout() {
        let tree = serde_json::json!({"tree": [
            tree_entry("skills/pdf/SKILL.md", "blob"),
            tree_entry("skills/docx/SKILL.md", "blob"),
            tree_entry("README.md", "blob"),
            tree_entry("skills", "tree"),
            tree_entry("skills/pdf/references/a.md", "blob"),
        ]});
        assert_eq!(
            marketplace_skill_dirs(&tree, "skills"),
            vec!["skills/docx".to_string(), "skills/pdf".to_string()]
        );
    }

    #[test]
    fn skill_dirs_nested_depth_cap_and_prefix() {
        let tree = serde_json::json!({"tree": [
            tree_entry("plugins/foo/skills/SKILL.md", "blob"),
            tree_entry("plugins/deep/a/b/SKILL.md", "blob"),
            tree_entry("other/x/SKILL.md", "blob"),
        ]});
        // skills_path=plugins: foo/skills is 2 segments below → kept;
        // deep/a/b is 3 → skipped; other/ is outside the prefix.
        assert_eq!(
            marketplace_skill_dirs(&tree, "plugins"),
            vec!["plugins/foo/skills".to_string()]
        );
    }

    #[test]
    fn skill_dirs_empty_path_means_root() {
        let tree = serde_json::json!({"tree": [
            tree_entry("pdf/SKILL.md", "blob"),
            tree_entry("a/b/SKILL.md", "blob"),
        ]});
        assert_eq!(
            marketplace_skill_dirs(&tree, ""),
            vec!["a/b".to_string(), "pdf".to_string()]
        );
    }

    #[test]
    fn frontmatter_basic_and_quotes() {
        let (name, desc) = parse_skill_frontmatter(
            "---\nname: \"pdf\"\ndescription: 'Works with PDF files'\n---\n# body",
        );
        assert_eq!(name.as_deref(), Some("pdf"));
        assert_eq!(desc.as_deref(), Some("Works with PDF files"));
    }

    #[test]
    fn frontmatter_missing_is_none() {
        assert_eq!(
            parse_skill_frontmatter("# no frontmatter here"),
            (None, None)
        );
        // Unterminated block still yields what was seen.
        let (name, _) = parse_skill_frontmatter("---\nname: x\n");
        assert_eq!(name.as_deref(), Some("x"));
    }

    #[test]
    fn frontmatter_wrapped_description_joins() {
        let (_, desc) = parse_skill_frontmatter(
            "---\nname: pdf\ndescription: Reads PDFs\n  and extracts tables.\n---\n",
        );
        assert_eq!(desc.as_deref(), Some("Reads PDFs and extracts tables."));
    }

    fn valid_source() -> MarketplaceSource {
        MarketplaceSource {
            id: "Test-Source".to_string(),
            display_name: " Test ".to_string(),
            owner: "anthropics".to_string(),
            repo: "skills".to_string(),
            branch: "feature/x".to_string(),
            skills_path: "skills".to_string(),
        }
    }

    #[test]
    fn sanitize_source_ok_normalizes() {
        let s = sanitize_marketplace_source(&valid_source()).unwrap();
        assert_eq!(s.id, "test-source");
        assert_eq!(s.display_name, "Test");
        assert_eq!(s.branch, "feature/x");
    }

    #[test]
    fn sanitize_source_rejects_bad_fields() {
        let mut s = valid_source();
        s.owner = "../evil".to_string();
        assert!(sanitize_marketplace_source(&s).is_err());
        let mut s = valid_source();
        s.repo = "a/b".to_string();
        assert!(sanitize_marketplace_source(&s).is_err());
        let mut s = valid_source();
        s.branch = "..".to_string();
        assert!(sanitize_marketplace_source(&s).is_err());
        let mut s = valid_source();
        s.id = "!!!".to_string();
        assert!(sanitize_marketplace_source(&s).is_err());
        // Empty skills_path (= repo root) is allowed.
        let mut s = valid_source();
        s.skills_path = "".to_string();
        assert!(sanitize_marketplace_source(&s).unwrap().skills_path.is_empty());
    }

    #[test]
    fn validate_dir_rejects_escapes() {
        let s = MarketplaceSource::anthropic_official();
        assert!(validate_marketplace_dir(&s, "skills/pdf").is_ok());
        assert!(validate_marketplace_dir(&s, "../evil").is_err());
        assert!(validate_marketplace_dir(&s, "skills/../../x").is_err());
        assert!(validate_marketplace_dir(&s, "other/x").is_err());
        assert!(validate_marketplace_dir(&s, "").is_err());
    }

    #[test]
    fn flatten_dir() {
        let s = MarketplaceSource::anthropic_official();
        assert_eq!(flatten_marketplace_dir(&s, "skills/pdf"), "pdf");
        let nested = MarketplaceSource {
            skills_path: "plugins".to_string(),
            ..s
        };
        assert_eq!(
            flatten_marketplace_dir(&nested, "plugins/foo/skills"),
            "foo-skills"
        );
    }

    #[test]
    fn catalog_cache_roundtrip() {
        let dir = tmp();
        let cache = MarketplaceCatalogCache {
            fetched_at: "123".to_string(),
            skills: vec![MarketplaceSkill {
                source_id: "__mp_test__".to_string(),
                dir: "skills/pdf".to_string(),
                name: "pdf".to_string(),
                description: "d".to_string(),
                installed: false,
            }],
        };
        write_catalog_cache(&dir, "__mp_test__", &cache).unwrap();
        let back = read_catalog_cache(&dir, "__mp_test__");
        assert_eq!(back.skills.len(), 1);
        assert_eq!(back.skills[0].name, "pdf");
        // Cleanup: hub_root() prefers the real home dir, so remove the
        // file wherever it landed.
        let _ = std::fs::remove_file(catalog_cache_path(&dir, "__mp_test__"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_candidates_match_dir_and_frontmatter() {
        let dir = tmp();
        let mp = dir.join("marketplace");
        let by_dir = mp.join("src").join("pdf");
        std::fs::create_dir_all(&by_dir).unwrap();
        std::fs::write(by_dir.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
        let by_name = mp.join("src").join("flat-dir");
        std::fs::create_dir_all(&by_name).unwrap();
        std::fs::write(
            by_name.join("SKILL.md"),
            "---\nname: fancy\n---\n",
        )
        .unwrap();
        let hits = marketplace_remove_candidates(&mp, "pdf");
        assert!(hits.contains(&by_dir));
        let hits = marketplace_remove_candidates(&mp, "fancy");
        assert!(hits.contains(&by_name));
        assert!(marketplace_remove_candidates(&mp, "nope").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
