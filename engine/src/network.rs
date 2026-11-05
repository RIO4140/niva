//! Network for Niva v1 (Meeting 22, DESIGN §16): one stdlib
//! function, `شبكة.اجلب(url)` — HTTP GET only, returns the
//! response body as text.
//!
//! Permission: network is OFF by default. The gate lives here
//! (`set_network_allowed` / `network_allowed`); the interpreter
//! and the CLI set it, and `اجلب` refuses with a clear Arabic
//! error while it is off. The permission error message starts
//! with `PERMISSION_ERROR_MARKER`, which the interactive
//! consoles use to offer the first-use permission prompt.
//!
//! Platform split (an adopted, documented limit — not a bug
//! hidden from the user):
//! - Native (desktop / Termux): plain HTTP only, hand-rolled
//!   HTTP/1.1 over std TcpStream, no new dependency (the native
//!   engine stays dependency-free). HTTPS is refused with a
//!   clear Arabic error: Rust std has no TLS.
//! - WASM (playground / Android app): HTTPS works through the
//!   browser. The fetch is a synchronous XMLHttpRequest
//!   (web-sys, wasm32-only): the interpreter is synchronous,
//!   so there is no way to await a Promise here. Synchronous
//!   XHR is deprecated in browsers but still works; a future
//!   async-capable interpreter could use fetch properly.
//!   Two protections cannot be enforced in the browser and are
//!   documented as such: the resolved IP is not exposed to
//!   JavaScript (so the private-IP check cannot run — the
//!   browser's CORS policy is the remaining guard), and the
//!   browser follows redirects itself (so the redirect count
//!   cannot be counted — only the final host and scheme are
//!   checked). The native side enforces all of them.
//!
//! Protections (§7), enforced on native for every request:
//! - One DNS resolution, then the IP is checked, then the
//!   connection goes to that same IP (no re-resolution).
//! - Private, loopback, link-local, unspecified and multicast
//!   IPs are refused — unless the hostname was explicitly
//!   authorized as a local host (`allow_local_host`; the CLI
//!   exposes it as `--مضيف-محلي`, and tests use the API
//!   directly). There is no other way to reach a local host.
//! - No redirect to a different host (host + port must match),
//!   same-host redirects at most 5, HTTPS targets refused
//!   (native: HTTPS is refused outright, so an HTTPS redirect
//!   target is refused with the same error).
//!
//! Resource limits (§9): 30 second timeout, 5 MB maximum
//! response body. Errors are Arabic, in the engine's existing
//! style (kind + message, no stack traces to the user).

use crate::error::{RioError, RioResult};
use crate::value::Value;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

/// The start of the network-permission error message. Consoles
/// match on it to offer the first-use permission prompt.
pub const PERMISSION_ERROR_MARKER: &str = "الشبكة مغلقة افتراضيا";

/// 30 seconds, the adopted default timeout (§9).
pub const TIMEOUT_SECS: u64 = 30;
/// 5 MB, the adopted maximum response body (§9).
pub const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
/// At most 5 same-host redirects (§7).
pub const MAX_REDIRECTS: usize = 5;

struct NetworkPolicy {
    allowed: bool,
    local_hosts: HashSet<String>,
}

/// Test-only serialization for the process-wide network
/// policy: cargo runs tests in parallel threads, and the policy
/// is one static. Every test that touches the flag locks this
/// first (network.rs tests and repl.rs tests alike).
#[cfg(test)]
pub(crate) static NET_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

static POLICY: LazyLock<Mutex<NetworkPolicy>> = LazyLock::new(|| {
    Mutex::new(NetworkPolicy {
        allowed: false,
        local_hosts: HashSet::new(),
    })
});

/// Enable or disable network for this process (single source of
/// truth at fetch time). The interpreter owns the per-instance
/// flag and syncs it here at the start of every top-level
/// evaluation (see interpreter.rs), so the gate always reflects
/// the interpreter that is currently evaluating. Called by the
/// interpreter when its `network_allowed` flag is set (CLI
/// --اسمح-بالشبكة, the WASM console's setNetworkAllowed,
/// tests) and by run_source to reset the gate explicitly.
pub fn set_network_allowed(allowed: bool) {
    if let Ok(mut policy) = POLICY.lock() {
        policy.allowed = allowed;
    }
}

pub fn network_allowed() -> bool {
    POLICY.lock().map(|p| p.allowed).unwrap_or(false)
}

/// Explicitly authorize a local hostname (loopback / private
/// address) for fetching. The only way a local host can be
/// reached: the IP check refuses it otherwise. Hostnames are
/// matched case-insensitively, exactly as written in the URL.
pub fn allow_local_host(host: &str) {
    if let Ok(mut policy) = POLICY.lock() {
        policy.local_hosts.insert(host.to_ascii_lowercase());
    }
}

fn local_host_allowed(host: &str) -> bool {
    POLICY
        .lock()
        .map(|p| p.local_hosts.contains(&host.to_ascii_lowercase()))
        .unwrap_or(false)
}

/// The اجلب implementation: gate, then the platform fetch.
/// Returns the response body as text.
pub fn fetch(url: &str, line: usize, col: usize) -> RioResult<Value> {
    if !network_allowed() {
        return Err(RioError::runtime(
            "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول",
            line,
            col,
        ));
    }
    fetch_allowed(url, line, col)
}

/// Fetch with the permission gate already passed. Kept separate
/// so tests can exercise the fetch without going through the
/// gate (they set the flag via `set_network_allowed` anyway).
fn fetch_allowed(url: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        fetch_wasm(url, line, col)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        fetch_native(url, line, col)
    }
}

// ----------------------------------------------------------------
// URL parsing (shared by both platforms)
// ----------------------------------------------------------------

struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str, line: usize, col: usize) -> RioResult<ParsedUrl> {
    let url = url.trim();
    if url.is_empty() {
        return Err(RioError::runtime("رابط فارغ", line, col));
    }
    if url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(RioError::runtime(
            format!("رابط غير صالح: {}", url),
            line,
            col,
        ));
    }
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => {
            return Err(RioError::runtime(
                "الرابط يجب أن يبدأ بـ http:// أو https://",
                line,
                col,
            ))
        }
    };
    if scheme != "http" && scheme != "https" {
        return Err(RioError::runtime(
            "الرابط يجب أن يبدأ بـ http:// أو https://",
            line,
            col,
        ));
    }
    // Authority is up to the first '/', '?' or '#'.
    let auth_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let mut path = rest[auth_end..].to_string();
    if path.is_empty() {
        path = "/".to_string();
    }
    if path.starts_with('#') {
        path = "/".to_string();
    }
    if authority.is_empty() || authority.contains('@') {
        return Err(RioError::runtime(
            format!("رابط غير صالح: {}", url),
            line,
            col,
        ));
    }
    // Host + optional port. Bracketed IPv6: [::1] or [::1]:8080.
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, after) = match bracketed.split_once(']') {
            Some((h, a)) => (h, a),
            None => {
                return Err(RioError::runtime(
                    format!("رابط غير صالح: {}", url),
                    line,
                    col,
                ))
            }
        };
        if host.is_empty() {
            return Err(RioError::runtime(
                format!("رابط غير صالح: {}", url),
                line,
                col,
            ));
        }
        let port = if let Some(p) = after.strip_prefix(':') {
            match p.parse::<u16>() {
                Ok(p) if p != 0 => p,
                _ => {
                    return Err(RioError::runtime(
                        format!("رابط غير صالح: {}", url),
                        line,
                        col,
                    ))
                }
            }
        } else if !after.is_empty() {
            return Err(RioError::runtime(
                format!("رابط غير صالح: {}", url),
                line,
                col,
            ));
        } else if scheme == "http" {
            80
        } else {
            443
        };
        (host.to_string(), port)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => {
                if h.is_empty() {
                    return Err(RioError::runtime(
                        format!("رابط غير صالح: {}", url),
                        line,
                        col,
                    ));
                }
                let port = match p.parse::<u16>() {
                    Ok(p) if p != 0 => p,
                    _ => {
                        return Err(RioError::runtime(
                            format!("رابط غير صالح: {}", url),
                            line,
                            col,
                        ))
                    }
                };
                (h.to_string(), port)
            }
            None => (
                authority.to_string(),
                if scheme == "http" { 80 } else { 443 },
            ),
        }
    };
    Ok(ParsedUrl {
        scheme,
        host,
        port,
        path,
    })
}

/// The class of a resolved IP, for the refusal message.
/// `is_private` / `is_link_local` live on Ipv4Addr / Ipv6Addr,
/// not on IpAddr, so the check matches on the address family
/// (IPv6 private is unique-local fc00::/7, link-local is
/// fe80::/10).
fn ip_class(ip: &std::net::IpAddr) -> Option<&'static str> {
    use std::net::IpAddr;
    if ip.is_loopback() {
        Some("حلقي (محلي)")
    } else if ip.is_unspecified() {
        Some("غير محدد")
    } else if ip.is_multicast() {
        Some("متعدد البث")
    } else {
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_private() {
                    Some("خاص")
                } else if v4.is_link_local() {
                    Some("رابط محلي")
                } else {
                    None
                }
            }
            IpAddr::V6(v6) => {
                if v6.is_unique_local() {
                    Some("خاص")
                } else if v6.is_unicast_link_local() {
                    Some("رابط محلي")
                } else {
                    None
                }
            }
        }
    }
}

// ----------------------------------------------------------------
// Native fetch: hand-rolled HTTP/1.1 over std TcpStream.
// ----------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
fn fetch_native(url: &str, line: usize, col: usize) -> RioResult<Value> {
    fetch_native_with_timeout(url, TIMEOUT_SECS, line, col)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_native_with_timeout(
    url: &str,
    timeout_secs: u64,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut current = parse_url(url, line, col)?;
    if current.scheme == "https" {
        return Err(RioError::runtime(
            "HTTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق أو المتصفح للروابط المشفرة",
            line,
            col,
        ));
    }
    let mut redirects = 0usize;
    // Audit م-15: one total deadline for the whole fetch
    // (connect + headers + body + redirects) — a slow server
    // sending a byte every 29 seconds must not hang the
    // session forever.
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let body = fetch_one_native(&current, deadline, line, col)?;
        match body {
            FetchOutcome::Body(text) => return Ok(Value::Str(text)),
            FetchOutcome::Redirect(location) => {
                if redirects >= MAX_REDIRECTS {
                    return Err(RioError::runtime(
                        "عدد إعادات التوجيه تجاوز الحد (5)",
                        line,
                        col,
                    ));
                }
                redirects += 1;
                let next = resolve_redirect(&current, &location, line, col)?;
                if next.scheme == "https" {
                    return Err(RioError::runtime(
                        "HTTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق أو المتصفح للروابط المشفرة",
                        line,
                        col,
                    ));
                }
                if next.host.to_ascii_lowercase() != current.host.to_ascii_lowercase()
                    || next.port != current.port
                {
                    return Err(RioError::runtime(
                        format!(
                            "إعادة التوجيه إلى مضيف مختلف ({}) مرفوضة",
                            next.host
                        ),
                        line,
                        col,
                    ));
                }
                current = next;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
enum FetchOutcome {
    Body(String),
    Redirect(String),
}

/// Resolve a Location value against the current URL: absolute
/// URLs are parsed as-is; relative ones keep scheme/host/port.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_redirect(
    current: &ParsedUrl,
    location: &str,
    line: usize,
    col: usize,
) -> RioResult<ParsedUrl> {
    let location = location.trim();
    if location.is_empty() {
        return Err(RioError::runtime(
            "إعادة توجيه بلا عنوان",
            line,
            col,
        ));
    }
    if location.contains("://") {
        return parse_url(location, line, col);
    }
    // Relative: a path (maybe with a query). Keep the current
    // scheme/host/port.
    let path = if let Some(p) = location.strip_prefix('/') {
        let _ = p;
        location.to_string()
    } else {
        // Relative to the current path's directory.
        let dir = match current.path.rfind('/') {
            Some(i) => &current.path[..=i],
            None => "/",
        };
        format!("{}{}", dir, location)
    };
    Ok(ParsedUrl {
        scheme: current.scheme.clone(),
        host: current.host.clone(),
        port: current.port,
        path,
    })
}

#[cfg(not(target_arch = "wasm32"))]
/// Audit م-15: time left until the total fetch deadline.
/// Every read re-arms the socket timeout with what is left,
/// so the whole fetch — not each read — fits in one budget.
#[cfg(not(target_arch = "wasm32"))]
fn time_left(
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<std::time::Duration> {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        return Err(RioError::runtime("انتهت المهلة الإجمالية (30 ثانية)", line, col));
    }
    Ok(left)
}

fn fetch_one_native(
    url: &ParsedUrl,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};

    // One DNS resolution, then the IP is checked, then the
    // connection goes to that same IP (§7).
    let addr = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|_| {
            RioError::runtime(
                format!("تعذر حل اسم المضيف {}", url.host),
                line,
                col,
            )
        })?
        .next()
        .ok_or_else(|| {
            RioError::runtime(
                format!("تعذر حل اسم المضيف {}", url.host),
                line,
                col,
            )
        })?;
    if let Some(class) = ip_class(&addr.ip()) {
        if !local_host_allowed(&url.host) {
            return Err(RioError::runtime(
                format!(
                    "الوصول إلى العنوان {} مرفوض (عنوان {})",
                    addr.ip(),
                    class
                ),
                line,
                col,
            ));
        }
    }
    let timeout = time_left(deadline, line, col)?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col)
        } else {
            RioError::runtime(
                format!("تعذر الاتصال بالمضيف {}", url.host),
                line,
                col,
            )
        }
    })?;
    let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
    let _ = stream.set_write_timeout(Some(time_left(deadline, line, col)?));
    let host_header = if url.port == 80 {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Niva/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        url.path, host_header
    );
    stream.write_all(request.as_bytes()).map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة إرسال الطلب",
            &format!("تعذر إرسال الطلب إلى {}", url.host),
            line,
            col,
        )
    })?;
    read_response(&mut stream, deadline, line, col)
}

#[cfg(not(target_arch = "wasm32"))]
fn io_timeout_or(
    e: std::io::Error,
    timeout_msg: &str,
    other_msg: &str,
    line: usize,
    col: usize,
) -> RioError {
    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
        RioError::runtime(timeout_msg, line, col)
    } else {
        RioError::runtime(other_msg, line, col)
    }
}

/// Read an HTTP/1.1 response: headers first (capped), then the
/// body by Content-Length, chunked transfer, or until EOF.
/// The body is capped at 5 MB (+1 byte to detect the overflow).
#[cfg(not(target_arch = "wasm32"))]
fn read_response(
    stream: &mut std::net::TcpStream,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
    use std::io::Read;
    const HEADER_CAP: usize = 64 * 1024;
    let mut head: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    let mut header_end = None;
    while head.len() < HEADER_CAP {
        let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                head.push(byte[0]);
                if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
                    header_end = Some(head.len());
                    break;
                }
            }
            Err(e) => {
                return Err(io_timeout_or(
                    e,
                    "انتهت مهلة قراءة الرد (30 ثانية)",
                    "تعذر قراءة الرد من الخادم",
                    line,
                    col,
                ))
            }
        }
    }
    let header_end = match header_end {
        Some(n) => n,
        None => {
            return Err(RioError::runtime(
                "رد غير صالح من الخادم (لا ترويسات)",
                line,
                col,
            ))
        }
    };
    let header_text = String::from_utf8_lossy(&head[..header_end]).into_owned();
    let mut lines = header_text.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if status == 0 {
        return Err(RioError::runtime(
            "رد غير صالح من الخادم (لا رمز حالة)",
            line,
            col,
        ));
    }
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    let mut location: Option<String> = None;
    for l in lines {
        if let Some((name, value)) = l.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            match name.as_str() {
                "content-length" => {
                    content_length = value.parse().ok();
                }
                "transfer-encoding" => {
                    if value.to_ascii_lowercase().contains("chunked") {
                        chunked = true;
                    }
                }
                "location" => {
                    location = Some(value.to_string());
                }
                _ => {}
            }
        }
    }
    // The body: Content-Length, then chunked, then until EOF.
    // Always capped at 5 MB + 1 byte (the extra byte proves an
    // overflow instead of silently truncating).
    let mut body: Vec<u8> = Vec::new();
    let cap = MAX_RESPONSE_BYTES + 1;
    if chunked {
        loop {
            let size_line = read_line_capped(stream, deadline, 1024, line, col)?;
            let size_str = size_line.trim().split(';').next().unwrap_or("");
            let size = usize::from_str_radix(size_str, 16).map_err(|_| {
                RioError::runtime("رد غير صالح من الخادم (ترميز مجزأ)", line, col)
            })?;
            if size == 0 {
                // Trailer + final CRLF: read until an empty line.
                loop {
                    let trailer = read_line_capped(stream, deadline, 1024, line, col)?;
                    if trailer.trim().is_empty() {
                        break;
                    }
                }
                break;
            }
            let mut remaining = size;
            let mut buf = [0u8; 8192];
            while remaining > 0 {
                let want = remaining.min(buf.len());
                let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
                match stream.read(&mut buf[..want]) {
                    Ok(0) => {
                        return Err(RioError::runtime(
                            "رد غير صالح من الخادم (انقطع أثناء الجسم)",
                            line,
                            col,
                        ))
                    }
                    Ok(n) => {
                        body.extend_from_slice(&buf[..n]);
                        if body.len() > cap {
                            return Err(RioError::runtime(
                                "الرد أكبر من الحد الأقصى (5 ميغابايت)",
                                line,
                                col,
                            ));
                        }
                        remaining -= n;
                    }
                    Err(e) => {
                        return Err(io_timeout_or(
                            e,
                            "انتهت مهلة قراءة الرد (30 ثانية)",
                            "تعذر قراءة الرد من الخادم",
                            line,
                            col,
                        ))
                    }
                }
            }
            // The CRLF after the chunk.
            let crlf = read_line_capped(stream, deadline, 16, line, col)?;
            let _ = crlf;
        }
    } else if let Some(len) = content_length {
        let mut remaining = len;
        let mut buf = [0u8; 8192];
        while remaining > 0 {
            let want = remaining.min(buf.len());
            let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
            match stream.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => {
                    body.extend_from_slice(&buf[..n]);
                    if body.len() > cap {
                        return Err(RioError::runtime(
                            "الرد أكبر من الحد الأقصى (5 ميغابايت)",
                            line,
                            col,
                        ));
                    }
                    remaining -= n;
                }
                Err(e) => {
                    return Err(io_timeout_or(
                        e,
                        "انتهت مهلة قراءة الرد (30 ثانية)",
                        "تعذر قراءة الرد من الخادم",
                        line,
                        col,
                    ))
                }
            }
        }
    } else {
        let mut buf = [0u8; 8192];
        loop {
            let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    body.extend_from_slice(&buf[..n]);
                    if body.len() > cap {
                        return Err(RioError::runtime(
                            "الرد أكبر من الحد الأقصى (5 ميغابايت)",
                            line,
                            col,
                        ));
                    }
                }
                Err(e) => {
                    return Err(io_timeout_or(
                        e,
                        "انتهت مهلة قراءة الرد (30 ثانية)",
                        "تعذر قراءة الرد من الخادم",
                        line,
                        col,
                    ))
                }
            }
        }
    }
    if (200..300).contains(&status) {
        Ok(FetchOutcome::Body(
            String::from_utf8_lossy(&body).into_owned(),
        ))
    } else if (300..400).contains(&status) {
        match location {
            Some(loc) => Ok(FetchOutcome::Redirect(loc)),
            None => Err(RioError::runtime(
                format!("الخادم رد برمز الحالة {} بلا عنوان إعادة توجيه", status),
                line,
                col,
            )),
        }
    } else {
        Err(RioError::runtime(
            format!("الخادم رد برمز الحالة {}", status),
            line,
            col,
        ))
    }
}

/// Read one CRLF-terminated line from the stream, capped.
#[cfg(not(target_arch = "wasm32"))]
fn read_line_capped(
    stream: &mut std::net::TcpStream,
    deadline: std::time::Instant,
    cap: usize,
    line: usize,
    col: usize,
) -> RioResult<String> {
    use std::io::Read;
    let mut out: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while out.len() < cap {
        let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                out.push(byte[0]);
                if out.len() >= 2 && &out[out.len() - 2..] == b"\r\n" {
                    break;
                }
            }
            Err(e) => {
                return Err(io_timeout_or(
                    e,
                    "انتهت مهلة قراءة الرد (30 ثانية)",
                    "تعذر قراءة الرد من الخادم",
                    line,
                    col,
                ))
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

// ----------------------------------------------------------------
// WASM fetch: synchronous XMLHttpRequest (web-sys, wasm32-only).
// The browser follows redirects itself; what can be checked
// afterwards is the final URL (host and scheme) and the size.
// ----------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
fn fetch_wasm(url: &str, line: usize, col: usize) -> RioResult<Value> {
    let parsed = parse_url(url, line, col)?;
    // HTTPS works here (browser TLS); plain HTTP from an https
    // page is blocked by the browser's mixed-content policy,
    // and cross-origin targets need permissive CORS headers —
    // both are browser policies, reported as fetch failures.
    let xhr = web_sys::XmlHttpRequest::new().map_err(|_| {
        RioError::runtime("تعذر إنشاء طلب الشبكة في المتصفح", line, col)
    })?;
    // Synchronous: the interpreter cannot await a Promise.
    xhr.open_with_async("GET", url, false).map_err(|_| {
        RioError::runtime(format!("رابط غير صالح: {}", url), line, col)
    })?;
    xhr.set_timeout(TIMEOUT_SECS as u32 * 1000);
    xhr.send().map_err(|_| {
        RioError::runtime(
            "تعذر الوصول إلى الرابط (تحقق من الرابط ومن اتصال الشبكة)",
            line,
            col,
        )
    })?;
    let status = xhr.status().map_err(|_| {
        RioError::runtime("تعذر قراءة حالة الرد", line, col)
    })?;
    // The browser followed any redirects: check the final URL.
    let final_url = xhr.response_url();
    if !final_url.is_empty() {
        if let Ok(final_parsed) = parse_url(&final_url, line, col) {
            if final_parsed.host.to_ascii_lowercase() != parsed.host.to_ascii_lowercase()
                || final_parsed.port != parsed.port
            {
                return Err(RioError::runtime(
                    format!(
                        "إعادة التوجيه إلى مضيف مختلف ({}) مرفوضة",
                        final_parsed.host
                    ),
                    line,
                    col,
                ));
            }
            if parsed.scheme == "https" && final_parsed.scheme == "http" {
                return Err(RioError::runtime(
                    "إعادة التوجيه من HTTPS إلى HTTP مرفوضة",
                    line,
                    col,
                ));
            }
        }
    }
    if !(200..300).contains(&status) {
        return Err(RioError::runtime(
            format!("الخادم رد برمز الحالة {}", status),
            line,
            col,
        ));
    }
    let text = xhr
        .response_text()
        .map_err(|_| RioError::runtime("تعذر قراءة الرد", line, col))?
        .unwrap_or_default();
    if text.len() > MAX_RESPONSE_BYTES {
        return Err(RioError::runtime(
            "الرد أكبر من الحد الأقصى (5 ميغابايت)",
            line,
            col,
        ));
    }
    Ok(Value::Str(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_http_defaults() {
        let u = parse_url("http://example.com", 1, 1).unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/");
        let u = parse_url("http://127.0.0.1:8080/a/b?x=1", 1, 1).unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 8080);
        assert_eq!(u.path, "/a/b?x=1");
    }

    #[test]
    fn parse_url_https_and_ipv6() {
        let u = parse_url("https://example.com:8443/", 1, 1).unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.port, 8443);
        let u = parse_url("http://[::1]:9000/x", 1, 1).unwrap();
        assert_eq!(u.host, "::1");
        assert_eq!(u.port, 9000);
        assert_eq!(u.path, "/x");
    }

    #[test]
    fn parse_url_rejects_bad() {
        assert!(parse_url("", 1, 1).is_err());
        assert!(parse_url("example.com", 1, 1).is_err());
        assert!(parse_url("ftp://example.com", 1, 1).is_err());
        assert!(parse_url("http://user@example.com", 1, 1).is_err());
        assert!(parse_url("http://example.com:0/", 1, 1).is_err());
        assert!(parse_url("http://example.com:99999/", 1, 1).is_err());
        assert!(parse_url("http://", 1, 1).is_err());
        assert!(parse_url("http://exa mple.com", 1, 1).is_err());
    }

    #[test]
    fn ip_class_covers_spec() {
        use std::net::IpAddr;
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(ip_class(&loopback), Some("حلقي (محلي)"));
        let v6loop: IpAddr = "::1".parse().unwrap();
        assert_eq!(ip_class(&v6loop), Some("حلقي (محلي)"));
        let private: IpAddr = "192.168.1.5".parse().unwrap();
        assert_eq!(ip_class(&private), Some("خاص"));
        let link_local: IpAddr = "169.254.10.20".parse().unwrap();
        assert_eq!(ip_class(&link_local), Some("رابط محلي"));
        let unspecified: IpAddr = "0.0.0.0".parse().unwrap();
        assert_eq!(ip_class(&unspecified), Some("غير محدد"));
        let multicast: IpAddr = "224.0.0.1".parse().unwrap();
        assert_eq!(ip_class(&multicast), Some("متعدد البث"));
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        assert_eq!(ip_class(&public), None);
        let public6: IpAddr = "2001:4860:4860::8888".parse().unwrap();
        assert_eq!(ip_class(&public6), None);
    }

    #[test]
    fn redirect_resolution() {
        let cur = parse_url("http://example.com/a/b", 1, 1).unwrap();
        let rel = resolve_redirect(&cur, "/c", 1, 1).unwrap();
        assert_eq!(rel.host, "example.com");
        assert_eq!(rel.path, "/c");
        let rel2 = resolve_redirect(&cur, "d", 1, 1).unwrap();
        assert_eq!(rel2.path, "/a/d");
        let abs = resolve_redirect(&cur, "http://example.com/e", 1, 1).unwrap();
        assert_eq!(abs.path, "/e");
        assert!(resolve_redirect(&cur, "", 1, 1).is_err());
    }


    #[test]
    fn permission_gate_default_off() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(false);
        let err = fetch("http://127.0.0.1:9/", 1, 1).unwrap_err();
        assert!(
            err.to_string().contains(PERMISSION_ERROR_MARKER),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    // --- Local test HTTP server (native only) ---
    // Binds 127.0.0.1 on an ephemeral port and serves canned
    // responses by path. Each test gets its own server (its own
    // port), so tests never share state.
    #[cfg(not(target_arch = "wasm32"))]
    fn test_server() -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // Read the request headers (until \r\n\r\n).
                let mut req: Vec<u8> = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.len() >= 4 && &req[req.len() - 4..] == b"\r\n\r\n" {
                                break;
                            }
                            if req.len() > 8192 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let req_text = String::from_utf8_lossy(&req).into_owned();
                let path = req_text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                let (status, mut headers, body): (&str, Vec<String>, Vec<u8>) =
                    match path.as_str() {
                        "/hello" => {
                            let b = "مرحبا من الخادم".as_bytes().to_vec();
                            (
                                "200 OK",
                                vec![format!("Content-Length: {}", b.len())],
                                b,
                            )
                        }
                        "/chunked" => (
                            "200 OK",
                            vec!["Transfer-Encoding: chunked".to_string()],
                            b"5\r\nchunk\r\n5\r\ned-ok\r\n0\r\n\r\n".to_vec(),
                        ),
                        "/redirect" => (
                            "302 Found",
                            vec![
                                "Location: /hello".to_string(),
                                "Content-Length: 0".to_string(),
                            ],
                            Vec::new(),
                        ),
                        "/cross" => (
                            "302 Found",
                            vec![
                                format!("Location: http://localhost:{}/hello", port),
                                "Content-Length: 0".to_string(),
                            ],
                            Vec::new(),
                        ),
                        "/loop" => (
                            "302 Found",
                            vec![
                                "Location: /loop".to_string(),
                                "Content-Length: 0".to_string(),
                            ],
                            Vec::new(),
                        ),
                        "/status500" => (
                            "500 Internal Server Error",
                            vec!["Content-Length: 0".to_string()],
                            Vec::new(),
                        ),
                        "/big" => {
                            let b = vec![b'x'; 6 * 1024 * 1024];
                            (
                                "200 OK",
                                vec![format!("Content-Length: {}", b.len())],
                                b,
                            )
                        }
                        "/slow" => {
                            // Never responds quickly: the caller
                            // times out first (test uses 1s).
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            (
                                "200 OK",
                                vec!["Content-Length: 2".to_string()],
                                b"ok".to_vec(),
                            )
                        }
                        _ => (
                            "404 Not Found",
                            vec!["Content-Length: 0".to_string()],
                            Vec::new(),
                        ),
                    };
                headers.push("Connection: close".to_string());
                let mut resp = format!("HTTP/1.1 {}\r\n", status).into_bytes();
                for h in &headers {
                    resp.extend_from_slice(h.as_bytes());
                    resp.extend_from_slice(b"\r\n");
                }
                resp.extend_from_slice(b"\r\n");
                resp.extend_from_slice(&body);
                let _ = stream.write_all(&resp);
            }
        });
        port
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_ok(url: &str) -> String {
        match fetch(url, 1, 1).unwrap() {
            Value::Str(s) => s,
            other => panic!("expected text, got {:?}", other),
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_get_success_plain_and_chunked() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        assert_eq!(
            fetch_ok(&format!("http://127.0.0.1:{}/hello", port)),
            "مرحبا من الخادم"
        );
        assert_eq!(
            fetch_ok(&format!("http://127.0.0.1:{}/chunked", port)),
            "chunked-ok"
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_same_host_redirect_followed() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        assert_eq!(
            fetch_ok(&format!("http://127.0.0.1:{}/redirect", port)),
            "مرحبا من الخادم"
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_cross_host_redirect_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let err = fetch(&format!("http://127.0.0.1:{}/cross", port), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("مضيف مختلف"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_redirect_loop_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let err = fetch(&format!("http://127.0.0.1:{}/loop", port), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("إعادات التوجيه"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_http_error_status() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let err = fetch(&format!("http://127.0.0.1:{}/status500", port), 1, 1).unwrap_err();
        assert!(err.to_string().contains("500"), "got: {}", err);
        let err = fetch(&format!("http://127.0.0.1:{}/missing", port), 1, 1).unwrap_err();
        assert!(err.to_string().contains("404"), "got: {}", err);
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_too_big_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let err = fetch(&format!("http://127.0.0.1:{}/big", port), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("5 ميغابايت"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_loopback_without_explicit_allow_refused() {
        // Allowed=true, but only 127.0.0.1 was authorized in
        // other tests — 127.0.0.2 is loopback too and is NOT in
        // the allowlist, so the IP check must refuse it before
        // any connection is attempted.
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        let err = fetch("http://127.0.0.2:9/", 1, 1).unwrap_err();
        assert!(err.to_string().contains("مرفوض"), "got: {}", err);
        assert!(
            err.to_string().contains("حلقي"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_https_refused_without_traffic() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        let err = fetch("https://example.com/", 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("HTTPS غير مدعوم"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_timeout() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        // A 1 second timeout against the /slow endpoint (5s):
        // the production default is 30s; the timeout logic is
        // the same, only the value differs.
        let err = fetch_native_with_timeout(
            &format!("http://127.0.0.1:{}/slow", port),
            1,
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("مهلة"), "got: {}", err);
        set_network_allowed(false);
    }
}
