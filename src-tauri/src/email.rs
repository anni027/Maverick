//! Gmail integration: OAuth 2.0 Desktop (loopback) connect flow, token
//! refresh, and a minimal Gmail REST client (send / list / read).
//!
//! Tokens live in `[email]` in `config.toml` next to provider API keys —
//! same file, same risk class. They are never logged and never cross the
//! Tauri boundary (see `EmailStatus` in `config.rs`).

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::ConfigManager;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1";
const SCOPES: &str =
    "https://www.googleapis.com/auth/gmail.send https://www.googleapis.com/auth/gmail.readonly";

/// How long the connect flow waits for the user to finish in the browser.
const CONNECT_TIMEOUT_SECS: u64 = 180;
/// Refresh the access token this far before its real expiry.
const TOKEN_SKEW_SECS: i64 = 120;
/// Cap on decoded message bodies handed to the model (chars).
pub const MAX_BODY_CHARS: usize = 50_000;
/// Cap on messages listed per call.
pub const MAX_LIST_RESULTS: u32 = 25;

pub struct TokenPair {
    pub access_token: String,
    pub expires_at: i64,
    pub refresh_token: String,
}

pub struct ListedEmail {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
}

pub struct ReadEmail {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub snippet: String,
    pub body: String,
}

/* ------------------------------------------------------------------ */
/* OAuth: authorization URL + loopback connect flow                    */
/* ------------------------------------------------------------------ */

/// Google consent URL for the Desktop/loopback flow. `redirect_uri` must be
/// `http://127.0.0.1:{port}/callback` — Google only allows loopback IPs for
/// installed apps. `access_type=offline&prompt=consent` guarantees a refresh
/// token even for repeat connects.
pub fn authorization_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let q = [
        ("client_id", client_id.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("response_type", "code".to_string()),
        ("scope", SCOPES.to_string()),
        ("access_type", "offline".to_string()),
        ("prompt", "consent".to_string()),
        ("state", state.to_string()),
    ];
    let query = q
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{GOOGLE_AUTH_URL}?{query}")
}

/// Unique-enough CSRF state for a loopback flow (attacker would already need
/// local access; the state mainly prevents cross-flow mixups).
fn new_state() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("mav-{}-{}", std::process::id(), nanos)
}

/// Open a URL in the default browser without new dependencies. If this fails
/// the caller surfaces the URL so the user can open it by hand.
#[allow(clippy::disallowed_methods)]
// The ban targets session-child processes that must die with their scope;
// a browser is deliberately detached UX (it must outlive the connect call).
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // `start` needs a (possibly empty) window-title argument first.
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
}

/// Parse `/callback?code=…&state=…` (or `error=…`) out of an HTTP request
/// line. Returns `(code, state)` or a human-readable failure.
fn parse_callback_request(request: &str) -> Result<(String, String), String> {
    let line = request.lines().next().unwrap_or_default();
    let query = line
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split_once('?'))
        .map(|(_, q)| q)
        .unwrap_or_default();
    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = urlencoding::decode(v)
            .map(|s| s.into_owned())
            .unwrap_or_default();
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => {
                return Err(format!(
                    "Google refused the connection: {v}. Check the OAuth consent screen and try again."
                ));
            }
            _ => {}
        }
    }
    match (code, state) {
        (Some(c), Some(s)) if !c.is_empty() => Ok((c, s)),
        _ => Err("browser callback carried no authorization code".to_string()),
    }
}

const CONNECT_OK_PAGE: &str = "<html><body style=\"font-family:sans-serif;padding:40px\">\
    <h2>Maverick is connected to Gmail</h2>\
    <p>You can close this tab and return to the app.</p></body></html>";
const CONNECT_FAIL_PAGE: &str = "<html><body style=\"font-family:sans-serif;padding:40px\">\
    <h2>Gmail connection failed</h2>\
    <p>Return to Maverick and try again.</p></body></html>";

async fn http_page(stream: &mut tokio::net::TcpStream, ok: bool) {
    let body = if ok { CONNECT_OK_PAGE } else { CONNECT_FAIL_PAGE };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Full connect flow: loopback listener → browser → code → tokens → profile.
/// Blocks (up to the timeout) while the user completes consent.
pub async fn run_connect_flow(
    client_id: &str,
    client_secret: &str,
) -> Result<(String, String, i64, String), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("could not bind a local callback port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not read callback port: {e}"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = new_state();
    let url = authorization_url(client_id, &redirect_uri, &state);
    open_browser(&url).map_err(|_| {
        format!("could not open a browser — open this URL by hand to connect Gmail:\n{url}")
    })?;

    let (mut stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        listener.accept(),
    )
    .await
    .map_err(|_| {
        "Gmail connection timed out — no browser callback arrived within 3 minutes".to_string()
    })
    .and_then(|r| r.map_err(|e| format!("callback listener failed: {e}")))?;

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(30), stream.read(&mut chunk))
            .await
            .map_err(|_| "timed out reading the browser callback".to_string())?
            .map_err(|e| format!("failed reading the browser callback: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 16_384 {
            http_page(&mut stream, false).await;
            return Err("browser callback request was too large".to_string());
        }
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&buf);
    let (code, got_state) = match parse_callback_request(&request) {
        Ok(pair) => pair,
        Err(e) => {
            http_page(&mut stream, false).await;
            return Err(e);
        }
    };
    if got_state != state {
        http_page(&mut stream, false).await;
        return Err("browser callback state mismatch — retry the connection".to_string());
    }
    http_page(&mut stream, true).await;

    let tokens = exchange_code(client_id, client_secret, &code, &redirect_uri).await?;
    let account = get_profile_email(&tokens.access_token).await?;
    Ok((
        tokens.refresh_token,
        tokens.access_token,
        tokens.expires_at,
        account,
    ))
}

/* ------------------------------------------------------------------ */
/* OAuth: token exchange + refresh                                     */
/* ------------------------------------------------------------------ */

#[allow(clippy::disallowed_methods)]
// Public Google endpoints need default rustls roots, not the grok TLS
// policy (backend pin + extra CA bundle for internal infra). Same shape as
// the existing Kilo/GitHub ad-hoc clients in commands.rs and skills.rs.
fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("could not build an HTTP client: {e}"))
}

async fn token_request(form: &[(&str, &str)]) -> Result<TokenPair, String> {
    let client = http_client()?;
    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("token request failed: {e}"))?;
    let status = res.status();
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("token response was not JSON: {e}"))?;
    if !status.is_success() {
        let detail = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown token error");
        return Err(format!("Google token error: {detail}"));
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "token response carried no access token".to_string())?
        .to_string();
    let expires_in = body.get("expires_in").and_then(serde_json::Value::as_i64);
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let now = chrono::Utc::now().timestamp();
    Ok(TokenPair {
        access_token,
        expires_at: now + expires_in.unwrap_or(3600),
        refresh_token,
    })
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<TokenPair, String> {
    let pair = token_request(&[
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ])
    .await?;
    if pair.refresh_token.is_empty() {
        return Err(
            "Google returned no refresh token — remove Maverick's access at myaccount.google.com/permissions and reconnect".to_string(),
        );
    }
    Ok(pair)
}

/// Exchange a stored refresh token for a fresh access token.
pub async fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<(String, i64), String> {
    let pair = token_request(&[
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "refresh_token"),
    ])
    .await?;
    Ok((pair.access_token, pair.expires_at))
}

/// Cached-or-refreshed access token plus the account address. Refreshes and
/// re-persists automatically; errors are user-actionable, never leaky.
pub async fn valid_access_token(manager: &ConfigManager) -> Result<(String, String), String> {
    let cfg = manager.email_config().await;
    if !cfg.enabled {
        return Err("email integration is disabled — enable it in Settings → Email".to_string());
    }
    if cfg.refresh_token.trim().is_empty() {
        return Err("Gmail is not connected — connect it in Settings → Email".to_string());
    }
    if cfg.client_id.trim().is_empty() || cfg.client_secret.trim().is_empty() {
        return Err(
            "Gmail client credentials are missing — paste the OAuth client id/secret in Settings → Email"
                .to_string(),
        );
    }
    let now = chrono::Utc::now().timestamp();
    if !cfg.access_token.is_empty() && cfg.expires_at - now > TOKEN_SKEW_SECS {
        return Ok((cfg.access_token, cfg.account_email));
    }
    let (access_token, expires_at) =
        refresh_access_token(&cfg.client_id, &cfg.client_secret, &cfg.refresh_token).await?;
    let mut updated = cfg.clone();
    updated.access_token = access_token.clone();
    updated.expires_at = expires_at;
    manager
        .set_email_config(updated)
        .await
        .map_err(|e| format!("could not persist the refreshed Gmail token: {e}"))?;
    Ok((access_token, cfg.account_email))
}

async fn get_profile_email(access_token: &str) -> Result<String, String> {
    let client = http_client()?;
    let res = client
        .get(format!("{GMAIL_API}/users/me/profile"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail profile request failed: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("Gmail profile request failed: HTTP {}", res.status()));
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Gmail profile response was not JSON: {e}"))?;
    body.get("emailAddress")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Gmail profile carried no email address".to_string())
}

/* ------------------------------------------------------------------ */
/* MIME building (text + HTML alternative, no attachments in v1)       */
/* ------------------------------------------------------------------ */

/// RFC 2047-encode a header value when it is not pure ASCII.
fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        value.replace(['\n', '\r'], " ")
    } else {
        let clean: String = value
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(clean.as_bytes());
        format!("=?UTF-8?B?{b64}?=")
    }
}

fn mime_boundary() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("maverick-{}-{}", std::process::id(), nanos)
}

/// Build the raw RFC 822 message. `to` is required; `html` is optional (when
/// present the message becomes multipart/alternative).
pub fn build_mime(
    from: &str,
    to: &str,
    cc: &[String],
    bcc: &[String],
    subject: &str,
    text: &str,
    html: Option<&str>,
) -> String {
    let std = base64::engine::general_purpose::STANDARD;
    let mut msg = format!(
        "From: {}\r\nTo: {}\r\n",
        encode_header(from),
        encode_header(to)
    );
    if !cc.is_empty() {
        msg.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    if !bcc.is_empty() {
        msg.push_str(&format!("Bcc: {}\r\n", bcc.join(", ")));
    }
    msg.push_str(&format!(
        "Subject: {}\r\nMIME-Version: 1.0\r\n",
        encode_header(subject)
    ));
    match html {
        Some(html_body) => {
            let boundary = mime_boundary();
            msg.push_str(&format!(
                "Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n\
                 --{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n\
                 {}\r\n--{boundary}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n\
                 {}\r\n--{boundary}--\r\n",
                std.encode(text.as_bytes()),
                std.encode(html_body.as_bytes())
            ));
        }
        None => {
            msg.push_str(
                "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
            );
            msg.push_str(&std.encode(text.as_bytes()));
            msg.push_str("\r\n");
        }
    }
    msg
}

/// base64url (RFC 4648 §5, no padding) for Gmail `raw` payloads.
pub fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/* ------------------------------------------------------------------ */
/* Gmail REST client                                                   */
/* ------------------------------------------------------------------ */

fn gmail_error(status: reqwest::StatusCode, body: &serde_json::Value) -> String {
    if status.as_u16() == 401 {
        return "Gmail rejected the access token — reconnect in Settings → Email".to_string();
    }
    let detail = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown Gmail error");
    format!("Gmail API error (HTTP {status}): {detail}")
}

/// Send a pre-built MIME message. Returns the Gmail message id.
pub async fn send_raw(access_token: &str, raw_b64url: &str) -> Result<String, String> {
    let client = http_client()?;
    let res = client
        .post(format!("{GMAIL_API}/users/me/messages/send"))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "raw": raw_b64url }))
        .send()
        .await
        .map_err(|e| format!("Gmail send request failed: {e}"))?;
    let status = res.status();
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Gmail send response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(gmail_error(status, &body));
    }
    body.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Gmail send response carried no message id".to_string())
}

/// List message ids (newest first). `query` is Gmail search syntax, passed
/// through verbatim (`urlencoding` handles the escaping).
pub async fn list_message_ids(
    access_token: &str,
    query: &str,
    max_results: u32,
) -> Result<Vec<String>, String> {
    let client = http_client()?;
    let max = max_results.clamp(1, MAX_LIST_RESULTS);
    let mut url = format!("{GMAIL_API}/users/me/messages?maxResults={max}");
    if !query.trim().is_empty() {
        url.push_str(&format!("&q={}", urlencoding::encode(query.trim())));
    }
    let res = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail list request failed: {e}"))?;
    let status = res.status();
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Gmail list response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(gmail_error(status, &body));
    }
    let ids = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(ids)
}

fn header(payload: &serde_json::Value, name: &str) -> String {
    payload
        .get("headers")
        .and_then(|h| h.as_array())
        .map(|headers| {
            headers
                .iter()
                .filter(|h| {
                    h.get("name")
                        .and_then(|n| n.as_str())
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
                .filter_map(|h| h.get("value").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Recursively find the first `text/plain` part body (base64url), else the
/// first `text/html` part flagged for HTML fallback.
fn find_part_text(node: &serde_json::Value, want_html: bool) -> Option<(String, bool)> {
    let mime = node.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(parts) = node.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(found) = find_part_text(part, want_html) {
                return Some(found);
            }
        }
        return None;
    }
    let is_plain = mime.eq_ignore_ascii_case("text/plain");
    let is_html = mime.eq_ignore_ascii_case("text/html");
    if (is_plain && !want_html) || (is_html && want_html) {
        let data = node
            .get("body")
            .and_then(|b| b.get("data"))
            .and_then(|d| d.as_str())
            .unwrap_or_default();
        if !data.is_empty() {
            return Some((data.to_string(), is_html));
        }
    }
    None
}

fn decode_b64url(data: &str) -> String {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data)
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Strip HTML to plain text for html-only messages (fallback only).
fn html_to_text(html: &str) -> String {
    let fragment = scraper::Html::parse_fragment(html);
    fragment
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("\n…[truncated]");
        out
    }
}

/// Fetch one message with metadata headers (`format=metadata`) for listings.
pub async fn get_metadata(
    access_token: &str,
    id: &str,
) -> Result<ListedEmail, String> {
    let client = http_client()?;
    let res = client
        .get(format!(
            "{GMAIL_API}/users/me/messages/{id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date"
        ))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail get request failed: {e}"))?;
    let status = res.status();
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Gmail get response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(gmail_error(status, &body));
    }
    let payload = body.get("payload").cloned().unwrap_or(serde_json::Value::Null);
    Ok(ListedEmail {
        id: id.to_string(),
        subject: header(&payload, "Subject"),
        from: header(&payload, "From"),
        date: header(&payload, "Date"),
    })
}

/// Fetch + decode one full message (`format=full`).
pub async fn get_message(access_token: &str, id: &str) -> Result<ReadEmail, String> {
    let client = http_client()?;
    let res = client
        .get(format!("{GMAIL_API}/users/me/messages/{id}?format=full"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail get request failed: {e}"))?;
    let status = res.status();
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Gmail get response was not JSON: {e}"))?;
    if !status.is_success() {
        return Err(gmail_error(status, &body));
    }
    let payload = body.get("payload").cloned().unwrap_or(serde_json::Value::Null);
    let text = find_part_text(&payload, false)
        .map(|(data, _)| decode_b64url(&data))
        .filter(|t| !t.trim().is_empty());
    let text = match text {
        Some(t) => t,
        None => find_part_text(&payload, true)
            .map(|(data, _)| html_to_text(&decode_b64url(&data)))
            .unwrap_or_default(),
    };
    Ok(ReadEmail {
        id: id.to_string(),
        subject: header(&payload, "Subject"),
        from: header(&payload, "From"),
        to: header(&payload, "To"),
        date: header(&payload, "Date"),
        snippet: body
            .get("snippet")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        body: truncate_chars(&text, MAX_BODY_CHARS),
    })
}

/* ------------------------------------------------------------------ */
/* Agent tools (specs mirror memory.rs; dispatch lives in agent_loop)  */
/* ------------------------------------------------------------------ */

/// Client-facing tool name for sending mail mid-run.
pub const EMAIL_SEND_TOOL_NAME: &str = "send_email";
/// Client-facing tool name for searching/listing mail mid-run.
pub const EMAIL_LIST_TOOL_NAME: &str = "list_emails";
/// Client-facing tool name for reading one message mid-run.
pub const EMAIL_READ_TOOL_NAME: &str = "read_email";

/// [`ToolSpec`](xai_grok_sampling_types::ToolSpec) advertising `send_email`.
pub fn send_email_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: EMAIL_SEND_TOOL_NAME.to_string(),
        description: Some(
            "Send an email through the user's connected Gmail account. Use when the user \
             asks you to email something or someone. `body_html` is optional — when given, \
             the message is sent as text+HTML alternative. Confirm the recipient with the \
             user before sending if there is any doubt about the address."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "to": { "type": "string", "description": "Recipient address, e.g. 'bob@example.com'" },
                "subject": { "type": "string", "description": "Subject line (may be empty)" },
                "body_text": { "type": "string", "description": "Plain-text body (required unless body_html is given)" },
                "body_html": { "type": "string", "description": "Optional HTML body" },
                "cc": { "type": "array", "items": {"type": "string"}, "description": "Optional CC addresses" },
                "bcc": { "type": "array", "items": {"type": "string"}, "description": "Optional BCC addresses" }
            },
            "required": ["to"]
        }),
    }
}

/// [`ToolSpec`](xai_grok_sampling_types::ToolSpec) advertising `list_emails`.
pub fn list_emails_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: EMAIL_LIST_TOOL_NAME.to_string(),
        description: Some(
            "Search the user's Gmail (newest first). `query` is Gmail search syntax \
             (e.g. 'from:alice subject:invoice newer_than:7d'); empty lists recent mail. \
             Returns one line per message with its id — pass the id to `read_email` \
             for the full body."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Gmail search query (default: recent mail)" },
                "max_results": { "type": "number", "description": "Max messages, 1-25 (default 10)" }
            },
            "required": []
        }),
    }
}

/// [`ToolSpec`](xai_grok_sampling_types::ToolSpec) advertising `read_email`.
pub fn read_email_tool_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: EMAIL_READ_TOOL_NAME.to_string(),
        description: Some(
            "Read one Gmail message by id (from `list_emails`). Returns headers plus \
             the decoded body, truncated past ~50k chars."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Gmail message id" }
            },
            "required": ["id"]
        }),
    }
}

pub struct SendEmailArgs {
    pub to: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
}

fn str_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn str_list_arg(args: &serde_json::Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Validate `send_email` arguments (pure — unit-tested).
pub fn parse_send_args(args: &serde_json::Value) -> Result<SendEmailArgs, String> {
    let to = str_arg(args, "to")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.contains('@'))
        .ok_or_else(|| "a valid recipient address in `to` is required".to_string())?;
    let body_text = str_arg(args, "body_text").unwrap_or_default();
    let body_html = str_arg(args, "body_html").filter(|s| !s.trim().is_empty());
    if body_text.trim().is_empty() && body_html.is_none() {
        return Err("`body_text` or `body_html` is required".to_string());
    }
    Ok(SendEmailArgs {
        to,
        subject: str_arg(args, "subject").unwrap_or_default(),
        body_text,
        body_html,
        cc: str_list_arg(args, "cc"),
        bcc: str_list_arg(args, "bcc"),
    })
}

pub struct ListEmailsArgs {
    pub query: String,
    pub max_results: u32,
}

/// Validate `list_emails` arguments (pure — unit-tested).
pub fn parse_list_args(args: &serde_json::Value) -> ListEmailsArgs {
    let max_results = args
        .get("max_results")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n.clamp(1, u64::from(MAX_LIST_RESULTS)) as u32)
        .unwrap_or(10);
    ListEmailsArgs {
        query: str_arg(args, "query").unwrap_or_default(),
        max_results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_has_loopback_params_and_scopes() {
        let url = authorization_url("cid", "http://127.0.0.1:9/callback", "st");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2Fcallback"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("gmail.send"));
        assert!(url.contains("gmail.readonly"));
        assert!(url.contains("state=st"));
    }

    #[test]
    fn callback_parses_code_and_rejects_errors() {
        let req = "GET /callback?code=abc123&state=mav-1 HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(
            parse_callback_request(req).expect("parses"),
            ("abc123".to_string(), "mav-1".to_string())
        );
        let denied = "GET /callback?error=access_denied HTTP/1.1\r\n\r\n";
        assert!(parse_callback_request(denied).is_err());
        let empty = "GET /callback HTTP/1.1\r\n\r\n";
        assert!(parse_callback_request(empty).is_err());
    }

    #[test]
    fn headers_stay_raw_when_ascii_and_rfc2047_when_not() {
        assert_eq!(encode_header("Hello"), "Hello");
        let encoded = encode_header("Grüße✓");
        assert!(encoded.starts_with("=?UTF-8?B?") && encoded.ends_with("?="));
        // Newlines are stripped, never injected.
        assert!(!encode_header("a\nBcc: evil@x").contains('\n'));
    }

    #[test]
    fn base64url_round_trips_with_url_alphabet() {
        let raw = b"\xfb\xff\xfe binary \x00\x01";
        let encoded = base64url(raw);
        assert!(!encoded.contains(['+', '/', '=']));
        let back = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&encoded)
            .expect("decodes");
        assert_eq!(back, raw);
    }

    #[test]
    fn mime_plain_and_multipart_share_one_shape() {
        let plain = build_mime("a@x", "b@x", &[], &[], "Hi", "hello", None);
        assert!(plain.contains("To: b@x"));
        assert!(plain.contains("Subject: Hi"));
        assert!(plain.contains("Content-Type: text/plain"));

        let cc = vec!["c@x".to_string()];
        let multi = build_mime("a@x", "b@x", &cc, &[], "Hi", "hello", Some("<b>hi</b>"));
        assert!(multi.contains("Cc: c@x"));
        assert!(multi.contains("multipart/alternative"));
        assert!(multi.contains(&base64::engine::general_purpose::STANDARD.encode("hello")));
        // The whole MIME document must survive the Gmail raw round trip.
        let raw = base64url(multi.as_bytes());
        let back = String::from_utf8(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&raw)
                .expect("decodes"),
        )
        .expect("utf8");
        assert_eq!(back, multi);
    }

    #[test]
    fn body_prefers_plain_text_then_html_fallback() {
        let payload = serde_json::json!({
            "mimeType": "multipart/alternative",
            "headers": [
                {"name": "Subject", "value": "T"},
                {"name": "From", "value": "f@x"},
            ],
            "parts": [
                {"mimeType": "text/plain", "body": {"data": base64url(b"plain body")}},
                {"mimeType": "text/html", "body": {"data": base64url(b"<b>html</b> body")}},
            ],
        });
        let (data, is_html) = find_part_text(&payload, false).expect("finds plain");
        assert!(!is_html);
        assert_eq!(decode_b64url(&data), "plain body");

        let html_only = serde_json::json!({
            "mimeType": "text/html",
            "body": {"data": base64url("<p>hello <b>world</b></p>".as_bytes())},
        });
        assert!(find_part_text(&html_only, false).is_none());
        let (data, is_html) = find_part_text(&html_only, true).expect("finds html");
        assert!(is_html);
        assert_eq!(html_to_text(&decode_b64url(&data)), "hello world");
    }

    #[test]
    fn long_bodies_truncate_on_char_boundaries() {
        let s: String = "é".repeat(100);
        let out = truncate_chars(&s, 10);
        assert!(out.chars().count() < 100);
        assert!(out.ends_with("[truncated]"));
        assert!(truncate_chars("short", 10) == "short");
    }

    #[test]
    fn send_args_require_recipient_and_body() {
        let ok = serde_json::json!({"to": "b@x", "subject": "Hi", "body_text": "yo"});
        let parsed = parse_send_args(&ok).expect("parses");
        assert_eq!(parsed.to, "b@x");
        assert!(parsed.cc.is_empty());

        assert!(parse_send_args(&serde_json::json!({"to": "not-an-addr", "body_text": "x"})).is_err());
        assert!(parse_send_args(&serde_json::json!({"to": "b@x"})).is_err());
        // HTML-only bodies are accepted.
        assert!(
            parse_send_args(&serde_json::json!({"to": "b@x", "body_html": "<b>x</b>"}))
                .expect("html-only")
                .body_html
                .is_some()
        );
    }

    #[test]
    fn list_args_default_and_clamp() {
        let d = parse_list_args(&serde_json::json!({}));
        assert_eq!(d.query, "");
        assert_eq!(d.max_results, 10);
        let big = parse_list_args(&serde_json::json!({"max_results": 9999}));
        assert_eq!(big.max_results, MAX_LIST_RESULTS);
    }
}
