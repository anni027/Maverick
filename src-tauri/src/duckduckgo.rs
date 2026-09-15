//! DuckDuckGo search — keyless HTML scrape for Maverick.
//!
//! Uses POST https://html.duckduckgo.com/html/ with browser headers,
//! parses .result rows, decodes /l/?uddg= URLs, falls back to lite.
//! No API key, <30 req/min/IP, polite pacing via 3 attempts + jitter.

use anyhow::Result;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use xai_tool_runtime::{ListToolsContext, Tool, ToolCallContext, ToolError};
use xai_tool_types::ToolDescription;
use xai_tool_protocol::{ToolCapabilities, ToolId, ToolScope};

const DDG_HTML: &str = "https://html.duckduckgo.com/html/";
const DDG_LITE: &str = "https://lite.duckduckgo.com/lite/";
const TIMEOUT_SECS: u64 = 12;

// ---------- Input / Output ----------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DuckDuckGoInput {
    /// Search query
    #[schemars(description = "Search query")]
    pub query: String,
    /// Max results 1-10
    #[schemars(description = "Max results 1-10")]
    pub count: Option<u8>,
    /// Region kl e.g. us-en, wt-wt
    #[schemars(description = "Region code e.g. us-en")]
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoOutput {
    pub query: String,
    pub content: String,
    pub citations: Vec<String>,
}

impl DuckDuckGoOutput {
    fn formatted(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Results for \"{}\":\n\n", self.query));
        out.push_str(&self.content);
        if !self.citations.is_empty() {
            out.push_str("\n\nCitations:\n");
            for (i, c) in self.citations.iter().enumerate() {
                out.push_str(&format!("[{}] {}\n", i + 1, c));
            }
        }
        out
    }
}

impl xai_tool_runtime::ToolOutput for DuckDuckGoOutput {}

// ---------- Client ----------

#[derive(Clone)]
pub struct DuckDuckGoClient {
    client: reqwest::Client,
}

impl DuckDuckGoClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .user_agent(random_ua())
            .build()?;
        Ok(Self { client })
    }

    pub async fn search(
        &self,
        query: &str,
        region: Option<&str>,
        count: Option<u8>,
    ) -> Result<(String, Vec<String>)> {
        let mut last_err = None;
        for attempt in 0..3 {
            let url = if attempt == 1 { DDG_LITE } else { DDG_HTML };
            let use_region = region.unwrap_or("us-en");
            match self.fetch_once(url, query, use_region).await {
                Ok((content, urls)) if !urls.is_empty() => {
                    let n = count.unwrap_or(8).clamp(1, 10) as usize;
                    let urls_trunc = urls.into_iter().take(n).collect::<Vec<_>>();
                    let content_trunc = truncate_content(&content, n);
                    return Ok((content_trunc, urls_trunc));
                }
                Ok(_) if attempt < 2 => {
                    // empty but try lite fallback
                    tokio::time::sleep(Duration::from_millis(400 + attempt as u64 * 300)).await;
                    continue;
                }
                Ok(v) => return Ok(v),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("rate-limited") || msg.contains("202") {
                        tokio::time::sleep(Duration::from_millis(800 + attempt as u64 * 700)).await;
                        last_err = Some(e);
                        continue;
                    }
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        last_err = Some(e);
                        continue;
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("DuckDuckGo search failed")))
    }

    async fn fetch_once(&self, url: &str, query: &str, region: &str) -> Result<(String, Vec<String>)> {
        let params = [("q", query), ("kl", region), ("b", "")];
        let resp = self
            .client
            .post(url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Referer", "https://html.duckduckgo.com/")
            .header("Origin", "https://html.duckduckgo.com")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "same-origin")
            .form(&params)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if status.as_u16() == 202 || body.contains("anomaly-modal") || body.contains("Unfortunately, bots") {
            anyhow::bail!("rate-limited (202/anomaly)");
        }
        if !status.is_success() {
            anyhow::bail!("HTTP {}", status);
        }

        let (content, urls) = parse_ddg_html(&body);
        if urls.is_empty() && body.len() < 2000 {
            // Likely blocked or no results page
            if body.contains("No results") {
                return Ok(("No results found for query.".to_string(), vec![]));
            }
        }
        // Build content as joined snippets with titles
        let content_str = if content.is_empty() {
            if urls.is_empty() {
                "No results found.".to_string()
            } else {
                urls.join("\n")
            }
        } else {
            content
        };
        Ok((content_str, urls))
    }
}

fn truncate_content(s: &str, n: usize) -> String {
    // Keep first n*500 chars roughly
    let max = n * 600;
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}

fn random_ua() -> String {
    // Rotate Chrome 121+ pool
    const UAS: &[&str] = &[
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
    ];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as usize % UAS.len())
        .unwrap_or(0);
    UAS[idx].to_string()
}

fn parse_ddg_html(html: &str) -> (String, Vec<String>) {
    let document = Html::parse_document(html);
    let result_sel = Selector::parse(".result").unwrap();
    let title_sel = Selector::parse("a.result__a").unwrap();
    let snippet_sel = Selector::parse(".result__snippet").unwrap();

    let mut out = String::new();
    let mut urls = Vec::new();

    for (i, node) in document.select(&result_sel).enumerate() {
        if i >= 10 {
            break;
        }
        let title = node
            .select(&title_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let href = node
            .select(&title_sel)
            .next()
            .and_then(|e| e.value().attr("href"))
            .unwrap_or("")
            .to_string();
        let url = decode_uddg(&href);
        let snippet = node
            .select(&snippet_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if url.is_empty() && title.is_empty() {
            continue;
        }
        if !url.is_empty() {
            urls.push(url.clone());
        }
        if !title.is_empty() || !snippet.is_empty() {
            out.push_str(&format!("{}. {} - {}\n{}\n\n", i + 1, title, url, snippet));
        }
    }

    // Fallback: if .result selector failed, try generic links
    if urls.is_empty() {
        if let Ok(a_sel) = Selector::parse("a[href]") {
            for node in document.select(&a_sel).take(10) {
                if let Some(href) = node.value().attr("href") {
                    if href.contains("uddg=") {
                        let u = decode_uddg(href);
                        if !u.is_empty() && !urls.contains(&u) {
                            urls.push(u);
                        }
                    }
                }
            }
            if !urls.is_empty() && out.is_empty() {
                for (i, u) in urls.iter().enumerate() {
                    out.push_str(&format!("{}. {}\n", i + 1, u));
                }
            }
        }
    }

    (out.trim().to_string(), urls)
}

fn decode_uddg(href: &str) -> String {
    // href like /l/?kh=-1&uddg=https%3A%2F%2Fexample.com%2Fpath&rut=...
    if let Some(start) = href.find("uddg=") {
        let after = &href[start + 5..];
        let end = after.find('&').unwrap_or(after.len());
        let encoded = &after[..end];
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return decoded.into_owned();
        }
        return encoded.to_string();
    }
    // Direct http link
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    // Relative /l/?uddg= without prefix
    String::new()
}

// ---------- Tool ----------

#[derive(Debug, Default, Clone)]
pub struct DuckDuckGoTool;

impl xai_grok_tools::types::tool_metadata::ToolMetadata for DuckDuckGoTool {
    fn kind(&self) -> xai_grok_tools::types::tool::ToolKind {
        xai_grok_tools::types::tool::ToolKind::WebSearch
    }
    fn tool_namespace(&self) -> xai_grok_tools::types::tool::ToolNamespace {
        xai_grok_tools::types::tool::ToolNamespace::GrokBuild
    }
    fn description_template(&self) -> &str {
        "Search DuckDuckGo for up-to-date web results. No API key needed. Use for tail queries, recent info, and general web search. Returns titles, snippets, and citations."
    }
    fn requires_expr(&self) -> xai_grok_tools::types::requirements::Expr<xai_grok_tools::types::requirements::ToolRequirement> {
        xai_grok_tools::types::requirements::Expr::True
    }
}

impl Tool for DuckDuckGoTool {
    type Args = DuckDuckGoInput;
    type Output = DuckDuckGoOutput;

    fn id(&self) -> ToolId {
        ToolId::new("duckduckgo_search").expect("valid id")
    }
    fn description(&self, _ctx: &ListToolsContext) -> ToolDescription {
        ToolDescription::new(
            "duckduckgo_search",
            <Self as xai_grok_tools::types::tool_metadata::ToolMetadata>::sanitized_description_template(self),
        )
    }
    fn capabilities(&self) -> ToolCapabilities {
        ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(ToolScope::Read),
            ..Default::default()
        }
    }

    fn run(
        &self,
        _ctx: ToolCallContext,
        input: Self::Args,
    ) -> impl std::future::Future<Output = Result<Self::Output, ToolError>> + Send {
        async move {
        if input.query.trim().is_empty() {
            return Err(ToolError::invalid_arguments("query is required"));
        }
        if input.query.len() > 499 {
            return Err(ToolError::invalid_arguments("query too long (max 499)"));
        }
        let client = DuckDuckGoClient::new().map_err(|e| ToolError::execution(ToolId::new("duckduckgo_search").unwrap(), e.to_string()))?;
        let count = input.count.unwrap_or(8).clamp(1, 10);
        let region = input.region.as_deref();
        match client.search(&input.query, region, Some(count)).await {
            Ok((content, citations)) => Ok(DuckDuckGoOutput {
                query: input.query,
                content,
                citations,
            }),
            Err(e) => Err(ToolError::execution(ToolId::new("duckduckgo_search").unwrap(), format!("DuckDuckGo search failed: {e}"))),
        }
        }
    }
}
