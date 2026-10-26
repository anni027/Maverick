# Skills Marketplace

One-click skill discovery and install, backed by GitHub repos as catalog
sources. Built 2026-09-29. Default source: `anthropics/skills` (19 skills
at the time of writing).

## What it does

- **Browse** a skill catalog from any GitHub repo with `SKILL.md` files —
  no API key, no login. Catalogs are cached on disk; browsing and
  searching never touch the network after the first fetch.
- **Search** the cached catalog by name/description.
- **Install** with one click. The skill lands in the hub cache as
  Server scope and is live for the model after the automatic ToolBridge
  refresh (`refresh_skills_internal`, same path as every other install).
- **Add/remove sources** — any `owner/repo/branch/path` triple, e.g. a
  company skill repo or `xai-org/plugin-marketplace`-style layout
  (nested `SKILL.md` up to two levels below the scan root is accepted).

## How it works

```
Settings → Skills tab → Marketplace panel
  source select + search + Refresh + Install buttons
  │
  ▼ Tauri commands (commands.rs)
  list_marketplace_sources / add_marketplace_source / remove_marketplace_source
  list_marketplace_skills (cache, auto-fetch on first browse, refresh=true to renew)
  search_marketplace_skills (cache filter) / install_marketplace_skill
  │
  ▼ catalog (skills.rs)
  GitHub Trees API ?recursive=1 → repo-relative SKILL.md dirs
  → new dirs only: raw.githubusercontent SKILL.md → frontmatter name/description
  → disk cache hub/catalogs/<source-id>.json
  │
  ▼ install (skills.rs)
  validate dir stays under skills_path (no `..`, no escapes)
  → download SKILL.md → hub/skills/marketplace/<source>/<flat>/SKILL.md
  → vendored parser → SkillDto → bridge refresh
```

- Source records live in `config.toml` (`[marketplace_sources]`);
  the built-in default is seeded in memory at startup, so fresh installs
  browse immediately and pre-marketplace configs keep working.
- Unauthenticated GitHub limit is 60 req/hr. A full refresh costs ~1 API
  request plus one raw fetch **per new skill only** — known entries reuse
  cached name/description. Refresh is manual (button), never automatic.
- `installed` badges are computed by matching install paths under
  `hub/skills/marketplace/<source>/`, so reinstalls across restarts
  still show correctly.
- `remove_skill` also finds marketplace installs (by flattened dir or
  parsed frontmatter name); the hub-index listing skips the
  `marketplace/` namespace since those are Server scope via discovery.

## Data locations

| What | Where |
|---|---|
| Sources | `config.toml` → `marketplace_sources` |
| Catalog caches | `~/.maverick/hub/catalogs/<source-id>.json` |
| Installed skills | `~/.maverick/hub/skills/marketplace/<source>/<flat>/SKILL.md` |
| Fallback (no home dir) | `<app_data>/hub/...` (same layout) |

## Verification (2026-09-29)

- `cargo test -p maverick-backend`: **82 passed** (12 new marketplace
  tests: tree parsing flat/nested/root, frontmatter incl. wrapped
  descriptions, source sanitization, dir validation, flattening, cache
  round-trip, remove-candidate matching). No network in tests.
- `cargo check`, `tsc --noEmit`, `vite build`: all exit 0.
- Live contract check against the real GitHub API: tree not truncated,
  19 `skills/*/SKILL.md` dirs, raw SKILL.md frontmatter parses
  (`name: pdf`, long `description:`).
- Vault anchor check: 174 anchors, 0 issues.

## Known limits / follow-ups

- Installs fetch **SKILL.md only** — bundled `scripts/`, `references/`,
  templates are not downloaded (noted in the UI explainer).
- No update-check for installed marketplace skills yet.
- Descriptions wrap-joined with spaces; exotic frontmatter stays best-effort.
- If GitHub rate-limiting bites, the error names it and cached results
  keep working; an optional token field is the escape hatch.
- SkillsMP (175k skills) was evaluated and deferred: no documented
  public REST API, reverse-engineering risk, mixed quality.
