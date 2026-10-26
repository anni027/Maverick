//! `FEATURES` is the source of truth and the operator tables are hand-maintained mirrors with no compile-time check of their own.
//! This test is that check.
//!
//! The operator tables live under `docs/internal/`, which is not synced to the
//! public `grok-build` mirror (only the test is). When the docs are absent the
//! check cannot run, so it reports a skip instead of failing; where the docs
//! are present (private monorepo, future syncs) it enforces full coverage.

use xai_grok_shell::agent::config::FEATURES;

/// Read an operator doc relative to the crate root, or `None` when this
/// checkout does not carry `docs/internal/`.
fn operator_doc(relative: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).ok()
}

#[test]
fn every_registered_feature_reaches_the_operator() {
    let Some(enterprise) = operator_doc("docs/internal/25-enterprise.md") else {
        eprintln!(
            "SKIP: docs/internal/25-enterprise.md is not synced to this checkout; \
             cannot verify the enterprise pinning table"
        );
        return;
    };
    let Some(env_vars) = operator_doc("docs/internal/22-environment-variables.md") else {
        eprintln!(
            "SKIP: docs/internal/22-environment-variables.md is not synced to this checkout; \
             cannot verify the environment-variable reference"
        );
        return;
    };
    for spec in FEATURES {
        assert!(
            enterprise.contains(&format!("`{}`", spec.key)),
            "{} has no row in the 25-enterprise.md pinning table",
            spec.key,
        );
        assert!(
            env_vars.contains(&format!("`{}`", spec.env)),
            "{} is undocumented in 22-environment-variables.md",
            spec.env,
        );
    }
}
