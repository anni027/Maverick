//! Loopback static server behind the artifact viewer's live preview.
//!
//! A `srcDoc` iframe has no origin, so relative `<link href="styles.css">`,
//! `<script src="app.js">`, module imports and `fetch()` all resolve against
//! `about:srcdoc` and miss — a multi-file page renders blank or unstyled.
//! Serving the workspace from a loopback port gives the preview a real origin,
//! so those references behave exactly as they do in a browser.
//!
//! Deliberate properties:
//!
//! * Bound to `127.0.0.1` only — never LAN-reachable, and on Windows a
//!   localhost bind draws no firewall prompt.
//! * Scoped to one workspace root, reusing [`crate::commands::ws_resolve`], so
//!   `..` segments and symlinks that escape the root are refused by the same
//!   guard the file commands use.
//! * No directory listing and no write verbs; only `GET`/`HEAD` of a file.
//! * `Cache-Control: no-store` — the preview must show the agent's *latest*
//!   write, not a cached previous render.
//!
//! One server per served root, up to [`MAX_SERVERS`]. Artifacts remember the
//! URL they were opened with, so a root must not be torn down just because the
//! next artifact lives somewhere else — evicting the oldest entry is the only
//! way a listener ever stops.
//!
//! The root is the session workspace when it holds the file, otherwise the
//! file's own directory (the agent's write cwd is not always the workspace the
//! UI stores). See `commands::preview_target`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Live preview servers, one per served root.
static RUNNING: OnceLock<tokio::sync::Mutex<Vec<Running>>> = OnceLock::new();

/// Cap so an agent writing into many one-off folders cannot accumulate
/// listeners for the life of the process.
const MAX_SERVERS: usize = 4;

struct Running {
    root: PathBuf,
    url: String,
    /// Owns the accept loop: dropping the entry aborts it and frees the port.
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn slot() -> &'static tokio::sync::Mutex<Vec<Running>> {
    RUNNING.get_or_init(|| tokio::sync::Mutex::new(Vec::new()))
}

/// Base URL (`http://127.0.0.1:<port>`) for a preview rooted at `root`,
/// starting a server if none is already serving that root.
pub async fn ensure_url(root: PathBuf) -> Result<String, String> {
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("workspace unavailable: {e}"))?;
    let mut guard = slot().lock().await;
    if let Some(running) = guard.iter().find(|r| r.root == root_c) {
        return Ok(running.url.clone());
    }
    let (url, served_root, task) = bind_and_serve(root_c).await?;
    if guard.len() >= MAX_SERVERS {
        // Oldest first; dropping the entry aborts its accept loop.
        guard.remove(0);
    }
    guard.push(Running {
        root: served_root,
        url: url.clone(),
        task,
    });
    Ok(url)
}

/// Bind an ephemeral loopback port and serve `root` until the task is aborted.
async fn bind_and_serve(root: PathBuf) -> Result<(String, PathBuf, tokio::task::JoinHandle<()>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("could not bind a local preview port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not read preview port: {e}"))?
        .port();
    // The accept loop outlives this call, so it gets its own clone rather
    // than taking the one we hand back to the caller.
    let root_for_task = root.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let root = root_for_task.clone();
            tokio::spawn(async move {
                let _ = serve_connection(stream, root).await;
            });
        }
    });
    Ok((format!("http://127.0.0.1:{port}"), root, task))
}

/// Handle one request/response pair. Errors are swallowed: an aborted browser
/// connection is routine (the iframe reloads on every write) and never worth
/// logging.
///
/// Generic over the stream so tests can drive it through an in-memory duplex
/// pair instead of binding a port.
async fn serve_connection<S>(mut stream: S, root: PathBuf) -> std::io::Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let head = read_head(&mut stream).await?;
    let mut parts = head.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(m), Some(t)) => (m, t),
        _ => return respond(stream, 400, "text/plain", b"bad request", true).await,
    };
    match method {
        "GET" | "HEAD" => {}
        _ => return respond(stream, 405, "text/plain", b"method not allowed", true).await,
    }

    let rel = request_target_to_rel(target);
    // Directories (and the bare root) serve their `index.html`, which is what
    // a browser would do for a real static host.
    let path = match crate::commands::ws_resolve(&root, &rel) {
        Ok(p) if p.is_dir() => p.join("index.html"),
        Ok(p) => p,
        Err(_) => return respond(stream, 404, "text/plain", b"not found", true).await,
    };
    let body = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => return respond(stream, 404, "text/plain", b"not found", true).await,
    };
    respond(
        stream,
        200,
        mime_for(&path),
        &body,
        method == "GET",
    )
    .await
}

/// Read just the request line + headers, capped so a bad client can't make us
/// buffer without limit.
async fn read_head<S>(stream: &mut S) -> std::io::Result<String>
where
    S: AsyncReadExt + Unpin,
{
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(text
        .lines()
        .next()
        .unwrap_or_default()
        .to_string())
}

/// `/index.html` → `index.html`, with a percent-decode so `%20` and friends
/// match the file on disk. A leading `/` is stripped; anything still rooted or
/// containing `..` is rejected downstream by `ws_resolve`.
fn request_target_to_rel(target: &str) -> String {
    let path = target.split('?').next().unwrap_or(target);
    let path = path.split('#').next().unwrap_or(path);
    let trimmed = path.trim_start_matches('/');
    percent_decode(trimmed)
}

/// Minimal `%XX` decode. Invalid sequences pass through untouched rather than
/// erroring — a bad encoding should 404 from the resolver, not 500 here.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn respond<S>(
    mut stream: S,
    status: u16,
    content_type: &str,
    body: &[u8],
    send_body: bool,
) -> std::io::Result<()>
where
    S: AsyncWriteExt + Unpin,
{
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        if send_body { body.len() } else { 0 }
    );
    stream.write_all(head.as_bytes()).await?;
    if send_body {
        stream.write_all(body).await?;
    }
    stream.flush().await
}

/// Content type for a served path. Unknown extensions fall back to
/// `application/octet-stream`, which the browser downloads rather than
/// mis-rendering.
fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Drive one request through an in-memory duplex pair and return the raw
    /// response text.
    async fn roundtrip(root: &Path, request: &str) -> String {
        let (mut client, server) = tokio::io::duplex(16 * 1024);
        let root = root.to_path_buf();
        let handle = tokio::spawn(async move {
            let _ = serve_connection(server, root).await;
        });
        client.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        client.read_to_string(&mut out).await.unwrap();
        let _ = handle.await;
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("maverick-preview-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn serves_a_workspace_file_with_no_store_and_its_mime_type() {
        let root = scratch("serve-file");
        std::fs::write(root.join("index.html"), "<h1>hi</h1>").unwrap();

        let res = roundtrip(&root, "GET /index.html HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.starts_with("HTTP/1.1 200 OK"), "status line: {res}");
        assert!(res.contains("Content-Type: text/html; charset=utf-8"), "{res}");
        // The preview must always reflect the agent's latest write, never a
        // cached previous render.
        assert!(res.contains("Cache-Control: no-store"), "{res}");
        assert!(res.ends_with("<h1>hi</h1>"), "body: {res}");
    }

    #[tokio::test]
    async fn root_serves_index_html_and_head_omits_the_body() {
        let root = scratch("serve-root");
        std::fs::write(root.join("index.html"), "<p>root</p>").unwrap();

        let res = roundtrip(&root, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
        assert!(res.ends_with("<p>root</p>"), "{res}");

        // A directory request serves its index rather than listing contents.
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app").join("index.html"), "<p>nested</p>").unwrap();
        let res = roundtrip(&root, "GET /app/ HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.ends_with("<p>nested</p>"), "{res}");

        let res = roundtrip(&root, "HEAD /index.html HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
        assert!(!res.contains("<p>root</p>"), "HEAD must send no body: {res}");
    }

    /// The whole reason for a dedicated server is that relative assets resolve.
    #[tokio::test]
    async fn serves_css_and_svg_with_the_right_types() {
        let root = scratch("mime");
        std::fs::write(root.join("style.css"), "body{color:red}").unwrap();
        std::fs::write(root.join("logo.svg"), "<svg/>").unwrap();

        let res = roundtrip(&root, "GET /style.css HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.contains("Content-Type: text/css; charset=utf-8"), "{res}");
        let res = roundtrip(&root, "GET /logo.svg HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.contains("Content-Type: image/svg+xml"), "{res}");
    }

    /// Percent-encoded traversal must land in the same refusal as a literal
    /// one — the resolver is the single trust boundary.
    #[tokio::test]
    async fn refuses_traversal_and_missing_files() {
        let root = scratch("traversal");
        std::fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
        std::fs::write(root.parent().unwrap().join("secret.txt"), "no").unwrap();

        for target in ["/../secret.txt", "/%2e%2e/secret.txt", "/../../secret.txt"] {
            let res = roundtrip(&root, &format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n")).await;
            assert!(res.starts_with("HTTP/1.1 404"), "{target} → {res}");
        }
        let res = roundtrip(&root, "GET /nope.html HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.starts_with("HTTP/1.1 404"), "{res}");
    }

    #[tokio::test]
    async fn rejects_write_verbs_and_garbage() {
        let root = scratch("verbs");
        std::fs::write(root.join("index.html"), "<p>ok</p>").unwrap();

        let res = roundtrip(&root, "POST /index.html HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(res.starts_with("HTTP/1.1 405"), "{res}");
    }

    /// A workspace path arrives as the browser encoded it; spaces and dots
    /// must survive the trip.
    #[test]
    fn target_decoding_strips_the_route_and_decodes_percents() {
        assert_eq!(request_target_to_rel("/index.html"), "index.html");
        assert_eq!(request_target_to_rel("/"), "");
        assert_eq!(request_target_to_rel("/my%20page.html"), "my page.html");
        assert_eq!(request_target_to_rel("/a.html?v=1#x"), "a.html");
    }

    /// Real sockets, real HTTP framing: this is the path the iframe actually
    /// takes, and it proves a page's relative `<link href="style.css">`
    /// resolves against the server origin (the whole reason `srcDoc` failed).
    #[tokio::test]
    async fn real_http_round_trip_serves_the_page_and_its_relative_asset() {
        let root = scratch("real-http");
        std::fs::write(
            root.join("index.html"),
            "<!doctype html><link rel=\"stylesheet\" href=\"style.css\"><p>ok</p>",
        )
        .unwrap();
        std::fs::write(root.join("style.css"), "body{color:red}").unwrap();

        let base = ensure_url(root).await.expect("server should start");
        assert!(base.starts_with("http://127.0.0.1:"), "loopback only: {base}");

        let page = reqwest::get(format!("{base}/index.html"))
            .await
            .expect("page fetch")
            .text()
            .await
            .expect("page body");
        assert!(page.contains("style.css"), "page: {page}");

        let css = reqwest::get(format!("{base}/style.css")).await.expect("css fetch");
        assert_eq!(css.status().as_u16(), 200);
        let ctype = css
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(ctype.starts_with("text/css"), "content-type: {ctype}");

        // Idempotent: the same root reuses the running server, so the iframe
        // keeps one stable URL across writes.
        let again = ensure_url(
            std::env::temp_dir().join(format!("maverick-preview-{}", "real-http")),
        )
        .await
        .expect("second ensure");
        assert_eq!(again, base);
    }

    /// Artifacts can live in different directories, and each remembers its URL.
    /// Serving a second root must not tear down the port the first URL points
    /// at, or an open artifact would silently go blank.
    #[tokio::test]
    async fn a_second_root_gets_its_own_listener_without_killing_the_first() {
        let a = scratch("multi-a");
        let b = scratch("multi-b");
        std::fs::write(a.join("index.html"), "<p>a</p>").unwrap();
        std::fs::write(b.join("index.html"), "<p>b</p>").unwrap();

        let ua = ensure_url(a).await.expect("first server");
        let ub = ensure_url(b).await.expect("second server");
        assert_ne!(ua, ub, "each root needs its own port");

        let page_a = reqwest::get(format!("{ua}/index.html"))
            .await
            .expect("a fetch")
            .text()
            .await
            .unwrap();
        assert!(page_a.contains("<p>a</p>"), "first root went dark: {page_a}");
        let page_b = reqwest::get(format!("{ub}/index.html"))
            .await
            .expect("b fetch")
            .text()
            .await
            .unwrap();
        assert!(page_b.contains("<p>b</p>"), "second root: {page_b}");
    }
}
