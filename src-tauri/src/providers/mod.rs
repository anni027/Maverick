//! Unified provider-plugin system.
//!
//! Every backend — a native LLM (xAI / OpenAI / Anthropic) or an external
//! agent runtime (Kilo Code / OpenCode invoked as a subprocess or over MCP) —
//! implements [`Provider`]. The agent loop only depends on this trait, so
//! backends are fully interchangeable.
//!
//! The wire format is the `xai-grok-sampling-types` vocabulary:
//! `ConversationRequest` in, `ConversationResponse` (assistant text +
//! [`ToolCall`](xai_grok_sampling_types::ToolCall)) out. That vocabulary is
//! deliberately backend-agnostic, so a subprocess provider that parses a CLI's
//! stdout and a native HTTP provider that streams SSE both satisfy the same
//! contract.

pub mod provider;
pub mod registry;
pub mod xai;
pub mod openai;
pub mod anthropic;
pub mod subprocess;

pub use provider::{
    Provider, ProviderCapabilities, ProviderConfig, ProviderInfo, ProviderInfoDto, ProviderKind,
};
pub use registry::ProviderRegistry;
pub use xai::XaiProvider;
pub use openai::OpenAiProvider;
pub use anthropic::AnthropicProvider;
pub use subprocess::SubprocessProvider;

use std::sync::Arc;

/// Defaults for each provider family.
pub fn default_provider_config(id: &str) -> (String, String) {
    match id {
        "xai" => ("https://api.x.ai/v1".to_string(), "grok-4".to_string()),
        "openai" => ("https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
        "anthropic" => ("https://api.anthropic.com/v1".to_string(), "claude-3-5-sonnet-20240620".to_string()),
        _ => ("https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
    }
}

/// Normalize base URL ensuring correct /v1 prefix for endpoints that require it.
pub fn normalize_base_url(id: &str, base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return default_provider_config(id).0;
    }
    match id {
        "xai" => {
            if trimmed == "https://api.x.ai" || trimmed == "http://api.x.ai" {
                "https://api.x.ai/v1".to_string()
            } else {
                trimmed.to_string()
            }
        }
        "openai" => {
            if trimmed == "https://api.openai.com" || trimmed == "http://api.openai.com" {
                "https://api.openai.com/v1".to_string()
            } else {
                trimmed.to_string()
            }
        }
        "anthropic" => {
            if trimmed == "https://api.anthropic.com" || trimmed == "http://api.anthropic.com" {
                "https://api.anthropic.com/v1".to_string()
            } else {
                trimmed.to_string()
            }
        }
        _ => {
            if trimmed == "http://localhost:11434" || trimmed == "http://127.0.0.1:11434" {
                format!("{trimmed}/v1")
            } else {
                trimmed.to_string()
            }
        }
    }
}

/// Create a provider for an id with optional API key + base_url/model/kind overrides.
/// For unknown ids (custom), creates OpenAI- or Anthropic-compatible based on kind.
pub fn create_provider(
    id: &str,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    kind: Option<String>,
) -> Option<Arc<dyn Provider>> {
    let (def_base, def_model) = default_provider_config(id);
    let raw_base = base_url.filter(|s| !s.trim().is_empty()).unwrap_or(def_base);
    let base = normalize_base_url(id, &raw_base);
    let mdl = model.filter(|s| !s.trim().is_empty()).unwrap_or(def_model);
    // For custom ids, respect explicit kind if provided
    let effective_kind = kind.as_deref().unwrap_or(id);
    match id {
        "xai" => Some(Arc::new(XaiProvider::new(api_key, base, mdl))),
        "openai" => Some(Arc::new(OpenAiProvider::new(api_key, base, mdl))),
        "anthropic" => Some(Arc::new(AnthropicProvider::new(api_key, base, mdl))),
        _ => {
            if effective_kind == "anthropic" {
                Some(Arc::new(AnthropicProvider::new(api_key, base, mdl)))
            } else {
                // Custom OpenAI-compatible (e.g. ollama, together, groq, local)
                Some(Arc::new(OpenAiProvider::new(api_key, base, mdl)))
            }
        }
    }
}

/// Build a ProviderInfo for a provider id + api_key + overrides. Used for registry.
/// `kind` is used for custom providers (`openai`/`anthropic`); for known ids it is ignored.
pub fn provider_info_for(
    id: &str,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    kind: Option<String>,
) -> Option<ProviderInfo> {
    provider_info_for_with_kind(id, api_key, base_url, model, kind)
}

pub fn provider_info_for_with_kind(
    id: &str,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    kind: Option<String>,
) -> Option<ProviderInfo> {
    let provider = create_provider(id, api_key.clone(), base_url.clone(), model.clone(), kind.clone())?;
    let (def_base, def_model) = default_provider_config(id);
    let raw_base = base_url.filter(|s| !s.trim().is_empty()).unwrap_or(def_base);
    let base = normalize_base_url(id, &raw_base);
    let mdl = model.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| def_model);
    // Use the resolved base/model for the displayed config (so UI shows effective values)
    let (kind_enum, name, config) = match id {
        "xai" => (
            ProviderKind::Xai,
            "xAI".to_string(),
            ProviderConfig::Http {
                base_url: base,
                api_key,
            },
        ),
        "openai" => (
            ProviderKind::OpenAi,
            "OpenAI".to_string(),
            ProviderConfig::Http {
                base_url: base,
                api_key,
            },
        ),
        "anthropic" => (
            ProviderKind::Anthropic,
            "Anthropic".to_string(),
            ProviderConfig::Http {
                base_url: base,
                api_key,
            },
        ),
        _ => {
            let is_anthropic = kind.as_deref() == Some("anthropic");
            let display_name = if is_anthropic {
                format!("{} (Anthropic-compat)", id)
            } else {
                format!("{} (OpenAI-compat)", id)
            };
            let k = if is_anthropic { ProviderKind::Anthropic } else { ProviderKind::OpenAi };
            (
                k,
                display_name,
                ProviderConfig::Http {
                    base_url: base,
                    api_key,
                },
            )
        }
    };
    let _ = mdl;
    Some(ProviderInfo {
        id: id.to_string(),
        name,
        kind: kind_enum,
        provider,
        config,
    })
}

/// Helper for simple provider creation without base_url/model overrides.
pub fn provider_info_for_simple(id: &str, api_key: Option<String>) -> Option<ProviderInfo> {
    provider_info_for(id, api_key, None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_provider_config_includes_v1() {
        let (xai_base, xai_model) = default_provider_config("xai");
        assert_eq!(xai_base, "https://api.x.ai/v1");
        assert_eq!(xai_model, "grok-4");

        let (openai_base, openai_model) = default_provider_config("openai");
        assert_eq!(openai_base, "https://api.openai.com/v1");
        assert_eq!(openai_model, "gpt-4o");

        let (anthropic_base, anthropic_model) = default_provider_config("anthropic");
        assert_eq!(anthropic_base, "https://api.anthropic.com/v1");
        assert_eq!(anthropic_model, "claude-3-5-sonnet-20240620");
    }

    #[test]
    fn test_normalize_base_url() {
        assert_eq!(normalize_base_url("openai", "https://api.openai.com"), "https://api.openai.com/v1");
        assert_eq!(normalize_base_url("openai", "https://api.openai.com/"), "https://api.openai.com/v1");
        assert_eq!(normalize_base_url("openai", "https://api.openai.com/v1"), "https://api.openai.com/v1");
        assert_eq!(normalize_base_url("xai", "https://api.x.ai"), "https://api.x.ai/v1");
        assert_eq!(normalize_base_url("anthropic", "https://api.anthropic.com"), "https://api.anthropic.com/v1");
        assert_eq!(normalize_base_url("ollama", "http://localhost:11434"), "http://localhost:11434/v1");
        assert_eq!(normalize_base_url("ollama", "http://localhost:11434/v1"), "http://localhost:11434/v1");
        assert_eq!(normalize_base_url("custom", "https://api.together.xyz/v1"), "https://api.together.xyz/v1");
    }

    #[test]
    fn test_provider_model_reporting() {
        let openai = create_provider("openai", None, None, Some("gpt-4o-mini".to_string()), None).unwrap();
        assert_eq!(openai.model(), "gpt-4o-mini");

        let xai = create_provider("xai", None, None, Some("grok-beta".to_string()), None).unwrap();
        assert_eq!(xai.model(), "grok-beta");

        let anthropic = create_provider("anthropic", None, None, Some("claude-3-opus".to_string()), None).unwrap();
        assert_eq!(anthropic.model(), "claude-3-opus");
    }
}