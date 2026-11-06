//! Network for Niva v1 (Meeting 22, DESIGN §16; server:
//! Meeting 27, DESIGN §42): two stdlib functions,
//! `شبكة.اجلب(url)` — HTTP GET only, returns the response
//! body as text — and `شبكة.اخدم منفذ شهادة مفتاح معالج
//! [مضيف]` — a ready HTTPS server with one Niva handler.
//!
//! Permission: network is OFF by default. The gate lives here
//! (`set_network_allowed` / `network_allowed`); the interpreter
//! and the CLI set it, and `اجلب` refuses with a clear Arabic
//! error while it is off. The permission error message starts
//! with `PERMISSION_ERROR_MARKER`, which the interactive
//! consoles use to offer the first-use permission prompt.
//!
//! Platform split:
//! - Native (desktop / Termux / Android app): hand-rolled
//!   HTTP/1.1 over std TcpStream. Plain HTTP directly; HTTPS
//!   wraps the connected TcpStream in rustls (Meeting 26,
//!   §41): the one audited TLS stack (rustls 0.23 + ring +
//!   webpki-roots), native only, behind the `https` feature
//!   (on by default for the native CLI). Full chain
//!   verification against Mozilla roots; self-signed
//!   certificates are rejected; there is no flag to bypass
//!   verification, ever. Without the feature, HTTPS is
//!   refused with the old Arabic error, before any traffic.
//! - WASM (playground): HTTPS works through the
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
//! HTTP/2 (Meeting 27, DESIGN §42), native only, behind the
//! `http2` feature (which implies `https` — HTTP/2 in Niva
//! is HTTPS-only, negotiated by ALPN; there is no cleartext
//! h2c). No new Niva API: `اجلب` and `اخدم` negotiate h2
//! vs HTTP/1.1 transparently — the client offers
//! ["h2", "http/1.1"], the server answers with what it
//! supports, and each side falls back to the existing
//! HTTP/1.1 code when the peer offers no ALPN or only
//! http/1.1. The engine is synchronous, so the async h2
//! work (h2 0.4 + tokio 1 + tokio-rustls 0.26, the async
//! adapter for the same audited rustls stack) runs on a
//! fresh current-thread tokio runtime inside a dedicated
//! thread, and the result crosses back as plain data — the
//! engine thread never enters the runtime, and no Niva
//! value (Rc) ever crosses a thread: the server still sends
//! every request to the engine thread through the same
//! std mpsc job channel, and the handler still runs there.
//! Multiplexing: the server accepts streams concurrently
//! (one task per stream); the client sends one request per
//! connection (اجلب semantics — one URL per call), so the
//! client proves h2 by speaking it, not by multiplexing.
//! Server push is excluded (deprecated in practice). The
//! same rules hold on h2: IP check before the handshake,
//! full chain verification, no bypass ever, Arabic errors
//! only (no raw rustls/h2 text), 30 second total deadline,
//! 5 MB body cap, at most 100 concurrent streams per
//! connection (h2 Builder setting; connections stay capped
//! at 64). Plain HTTP stays HTTP/1.1 — ALPN needs TLS.
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
//!   same-host redirects at most 5, HTTPS→HTTP downgrades
//!   refused, HTTP→HTTPS upgrades allowed (same host only).
//!   The IP check runs before the TLS handshake, so a
//!   certificate is never even offered to a refused address.
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

pub(crate) fn local_host_allowed(host: &str) -> bool {
    POLICY
        .lock()
        .map(|p| p.local_hosts.contains(&host.to_ascii_lowercase()))
        .unwrap_or(false)
}

/// Shared gate check for the sockets module (catalog task 13):
/// the same closed-gate wording as `fetch` — one message, one
/// console prompt, no second wording to drift.
pub(crate) fn require_network_allowed(line: usize, col: usize) -> RioResult<()> {
    if network_allowed() {
        Ok(())
    } else {
        Err(RioError::runtime(
            "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول",
            line,
            col,
        ))
    }
}

/// Resolve a host once, then refuse the address exactly as
/// `fetch` does (§7): one DNS resolution, the IP is checked,
/// and the connection goes to that same IP. A classified
/// address (loopback / private / link-local / multicast /
/// unspecified) is refused unless the host was explicitly
/// allowed with `--مضيف-محلي` (see `allow_local_host`).
/// Shared by the sockets module so the refusal wording can
/// never drift from `اجلب`'s.
pub(crate) fn resolve_checked(
    host: &str,
    port: u16,
    line: usize,
    col: usize,
) -> RioResult<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|_| {
            RioError::runtime(format!("تعذر حل اسم المضيف {}", host), line, col)
        })?
        .next()
        .ok_or_else(|| {
            RioError::runtime(format!("تعذر حل اسم المضيف {}", host), line, col)
        })?;
    if let Some(class) = ip_class(&addr.ip()) {
        if !local_host_allowed(host) {
            return Err(RioError::runtime(
                format!("الوصول إلى العنوان {} مرفوض (عنوان {})", addr.ip(), class),
                line,
                col,
            ));
        }
    }
    Ok(addr)
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
// HTTPS server (Meeting 27, DESIGN §42)
// ----------------------------------------------------------------

/// The start of the server-permission error message. Consoles
/// match on it to offer the first-use permission prompt, like
/// PERMISSION_ERROR_MARKER for the network.
pub const SERVER_PERMISSION_MARKER: &str = "الخادم مغلق افتراضيا";

/// At most 64 simultaneous connections (§42).
pub const MAX_SERVER_CONNECTIONS: usize = 64;

/// A request (headers + body) is capped at the same 5 MB as a
/// fetch response (§42).
const MAX_SERVER_REQUEST_BYTES: usize = MAX_RESPONSE_BYTES;

/// Test-only stop flag: unit tests run serve() on a thread and
/// ask it to stop through this flag. A real server runs until
/// the process is interrupted (Ctrl+C) and never returns.
#[cfg(test)]
static SERVER_STOP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn request_server_stop() {
    SERVER_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
fn server_stop_requested() -> bool {
    SERVER_STOP.load(std::sync::atomic::Ordering::SeqCst)
}

/// Test-only timeout override (seconds): 0 = the production
/// 30s. The timeout test sets it to 1 so the deadline fires
/// inside the suite instead of after 30 seconds.
#[cfg(test)]
static SERVER_TIMEOUT_OVERRIDE_SECS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
fn server_timeout_secs() -> u64 {
    let v = SERVER_TIMEOUT_OVERRIDE_SECS.load(std::sync::atomic::Ordering::SeqCst);
    if v == 0 {
        TIMEOUT_SECS
    } else {
        v
    }
}

/// Test-only: the port the last serve() call bound (0 = none
/// yet). Unit tests bind port 0 and read the real port here.
#[cfg(test)]
static LAST_SERVER_PORT: std::sync::atomic::AtomicU16 =
    std::sync::atomic::AtomicU16::new(0);

#[cfg(test)]
pub(crate) fn last_server_port() -> u16 {
    LAST_SERVER_PORT.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
impl TimedRead for rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream> {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.get_mut().set_read_timeout(dur)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
struct ServerJob {
    req: HttpRequest,
    reply: std::sync::mpsc::Sender<ServerReply>,
}

/// One handler reply, as parts: the HTTP/1.1 path formats
/// it with build_response_bytes; the h2 path sends the
/// status + headers + body directly. Both protocols share
/// the same reply (run_handler_parts), so the handler's
/// answer is identical on either wire.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
struct ServerReply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
struct HttpRequest {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Counts one live connection; the count drops when the
/// connection thread exits (Drop), even on panic.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
struct ConnectionGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The `شبكة.اخدم` implementation: `شبكة.اخدم منفذ شهادة
/// مفتاح معالج [مضيف]` — a ready HTTPS server with one Niva
/// handler function. The call blocks (serves until the
/// process is interrupted). The server flag alone is the
/// requirement (the network flag is not enough); a handler
/// that itself calls `اجلب` needs the network flag too.
pub fn serve(
    ctx: &mut dyn crate::stdlib::HandlerCaller,
    port: u16,
    cert_path: &str,
    key_path: &str,
    handler: Value,
    host: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    // البوابة أولا في كل بناء (أصلي بميزة أو بلا،
    // وWASM): بلا --اسمح-بالخادم الرفض عربي واحد
    // متطابق في كل مكان؛ خطأ «بلا دعم TLS» أو
    // «غير مدعوم في المتصفح» يظهر فقط مع العلم.
    if !ctx.server_allowed() {
        return Err(RioError::runtime(
            "الخادم مغلق افتراضيا — استخدم --اسمح-بالخادم للسماح بالخادم، أو وافق عند أول استخدام في الكونسول",
            line, col,
        ));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, port, cert_path, key_path, handler, host);
        return Err(RioError::runtime(
            "الخادم غير مدعوم في متصفح الويب",
            line, col,
        ));
    }
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "https")))]
    {
        let _ = (ctx, port, cert_path, key_path, handler, host);
        return Err(RioError::runtime(
            "الخادم يحتاج HTTPS — هذه النسخة بُنيت بلا دعم TLS",
            line, col,
        ));
    }
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    {
        serve_native(ctx, port, cert_path, key_path, handler, host, line, col)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn serve_native(
    ctx: &mut dyn crate::stdlib::HandlerCaller,
    port: u16,
    cert_path: &str,
    key_path: &str,
    handler: Value,
    host: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    // البوابة في serve() قبل هذا الاستدعاء — لا فحص
    // ثان هنا.
    let host = host.trim();
    if host != "127.0.0.1" && host != "0.0.0.0" {
        return Err(RioError::runtime(
            "الخادم يربط محليا فقط: المضيف 127.0.0.1 أو 0.0.0.0",
            line, col,
        ));
    }
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    let (certs, key, self_signed) =
        load_server_identity(&policy, &cwd, cert_path, key_path, line, col)?;
    // `mut` only when the `http2` feature sets ALPN below.
    #[cfg_attr(not(feature = "http2"), allow(unused_mut))]
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| {
            RioError::runtime("ملف الشهادة أو المفتاح غير صالح للخادم", line, col)
        })?;
    // Meeting 27 §42: offer h2 first, then HTTP/1.1 — the
    // client picks; no ALPN from the client means the
    // HTTP/1.1 fallback path. Only when the `http2`
    // feature is on.
    #[cfg(feature = "http2")]
    {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    }
    let config = std::sync::Arc::new(config);
    let listener = std::net::TcpListener::bind((host, port)).map_err(|e| {
        RioError::runtime(bind_error_message(host, port, &e), line, col)
    })?;
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    #[cfg(test)]
    LAST_SERVER_PORT.store(bound_port, std::sync::atomic::Ordering::SeqCst);
    listener.set_nonblocking(true).map_err(|_| {
        RioError::runtime("تعذر تهيئة الخادم", line, col)
    })?;
    // The startup announcement goes straight to stdout: the
    // engine's output buffer is only flushed when run()
    // returns, and a server never returns (until interrupt).
    // Tests wait for this exact line on the child process.
    use std::io::Write;
    println!("الخادم يعمل على https://{}:{}", host, bound_port);
    if self_signed {
        println!("الشهادة موقعة ذاتيا — للتطوير فقط");
    }
    let _ = std::io::stdout().flush();
    let (job_tx, job_rx) = std::sync::mpsc::channel::<ServerJob>();
    let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Accept loop on the calling (engine) thread, polling:
    // a nonblocking listener is accepted when a connection
    // is pending, and one queued handler job is pumped per
    // iteration (10 ms wait). Connection threads do the TLS
    // handshake + HTTP read/parse only — the handler itself
    // always runs here, on the engine thread, because Niva
    // values (Rc) cannot cross threads.
    loop {
        #[cfg(test)]
        if server_stop_requested() {
            return Ok(Value::Nil);
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                let count = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if count > MAX_SERVER_CONNECTIONS {
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    drop(stream);
                    continue;
                }
                let job_tx = job_tx.clone();
                let config = std::sync::Arc::clone(&config);
                let active = std::sync::Arc::clone(&active);
                std::thread::spawn(move || {
                    let _guard = ConnectionGuard(active);
                    handle_connection(stream, config, job_tx);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                match job_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(job) => {
                        let (status, headers, text) =
                            run_handler_parts(ctx, &handler, job.req, line, col);
                        let _ = job.reply.send(ServerReply {
                            status,
                            headers,
                            body: text.into_bytes(),
                        });
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn bind_error_message(host: &str, port: u16, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "تعذر ربط الخادم على {}:{} — المنفذ يحتاج صلاحيات مرتفعة",
            host, port
        ),
        std::io::ErrorKind::AddrInUse => format!(
            "تعذر ربط الخادم على {}:{} — المنفذ مستخدم بالفعل",
            host, port
        ),
        _ => format!("تعذر ربط الخادم على {}:{}", host, port),
    }
}

/// Load the server identity: cert + key are PEM FILES ONLY,
/// resolved inside FilePolicy (program dir unless trusted).
/// Errors name the FILE only (display form) — the key never
/// appears in any error or log. The raw file buffers are
/// wiped after loading (best effort — the copy inside
/// ServerConfig must live as long as the server runs).
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn load_server_identity(
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    cert_path: &str,
    key_path: &str,
    line: usize,
    col: usize,
) -> RioResult<(
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
    bool,
)> {
    let cert_file = policy.resolve_in(cwd, cert_path, line, col)?;
    let key_file = policy.resolve_in(cwd, key_path, line, col)?;
    let cert_display = policy.display_path(&cert_file);
    let key_display = policy.display_path(&key_file);
    let mut cert_bytes = std::fs::read(&cert_file).map_err(|_| {
        RioError::runtime(format!("تعذر قراءة ملف الشهادة: {}", cert_display), line, col)
    })?;
    let mut key_bytes = std::fs::read(&key_file).map_err(|_| {
        RioError::runtime(format!("تعذر قراءة ملف المفتاح: {}", key_display), line, col)
    })?;
    let result =
        parse_server_identity(&cert_bytes, &key_bytes, &cert_display, &key_display, line, col);
    cert_bytes.iter_mut().for_each(|b| *b = 0);
    key_bytes.iter_mut().for_each(|b| *b = 0);
    result
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn parse_server_identity(
    cert_pem: &[u8],
    key_pem: &[u8],
    cert_display: &str,
    key_display: &str,
    line: usize,
    col: usize,
) -> RioResult<(
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
    bool,
)> {
    use rustls::pki_types::pem::PemObject;
    let mut certs = Vec::new();
    for item in rustls::pki_types::CertificateDer::pem_slice_iter(cert_pem) {
        match item {
            Ok(cert) => certs.push(cert),
            Err(_) => {
                return Err(RioError::runtime(
                    format!("ملف الشهادة غير صالح: {}", cert_display),
                    line, col,
                ))
            }
        }
    }
    if certs.is_empty() {
        return Err(RioError::runtime(
            format!("ملف الشهادة لا يحوي شهادة: {}", cert_display),
            line, col,
        ));
    }
    let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| {
        RioError::runtime(format!("ملف المفتاح غير صالح: {}", key_display), line, col)
    })?;
    let self_signed = cert_is_self_signed(&certs[0]);
    Ok((certs, key, self_signed))
}

/// Minimal DER walk: self-signed iff the first certificate's
/// issuer bytes equal its subject bytes. Certificate ::=
/// SEQUENCE { tbsCertificate SEQUENCE { [0] version?,
/// serial INTEGER, signature SEQUENCE, issuer Name,
/// validity SEQUENCE, subject Name, ... } }.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn cert_is_self_signed(cert: &rustls::pki_types::CertificateDer<'_>) -> bool {
    let der: &[u8] = cert.as_ref();
    let Some((0x30, outer, _)) = read_der_tlv(der) else {
        return false;
    };
    let Some((0x30, tbs, _)) = read_der_tlv(outer) else {
        return false;
    };
    let mut rest = tbs;
    if let Some((0xA0, _, used)) = read_der_tlv(rest) {
        rest = &rest[used..];
    }
    let Some((0x02, _, used)) = read_der_tlv(rest) else {
        return false;
    };
    rest = &rest[used..];
    let Some((0x30, _, used)) = read_der_tlv(rest) else {
        return false;
    };
    rest = &rest[used..];
    let Some((0x30, issuer, used)) = read_der_tlv(rest) else {
        return false;
    };
    rest = &rest[used..];
    let Some((0x30, _, used)) = read_der_tlv(rest) else {
        return false;
    };
    rest = &rest[used..];
    let Some((0x30, subject, _)) = read_der_tlv(rest) else {
        return false;
    };
    issuer == subject
}

/// Read one DER TLV: (tag, content, total bytes used).
/// Short and long length forms; indefinite length refused.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn read_der_tlv(bytes: &[u8]) -> Option<(u8, &[u8], usize)> {
    if bytes.len() < 2 {
        return None;
    }
    let tag = bytes[0];
    let first = bytes[1];
    let (len, header) = if first < 0x80 {
        (first as usize, 2)
    } else if first == 0x80 {
        return None;
    } else {
        let count = (first & 0x7F) as usize;
        if count == 0 || count > 4 || bytes.len() < 2 + count {
            return None;
        }
        let mut len = 0usize;
        for b in &bytes[2..2 + count] {
            len = (len << 8) | (*b as usize);
        }
        (len, 2 + count)
    };
    let total = header + len;
    if bytes.len() < total {
        return None;
    }
    Some((tag, &bytes[header..header + len], total))
}

/// One connection, on its own thread: TLS handshake + HTTP
/// request read/parse, then the parsed request (plain data,
/// all Send) goes to the engine thread over the job channel,
/// and the response bytes come back on the reply channel.
/// HTTP/1.1, one response per connection, then close (no
/// keep-alive in v1). The 30s deadline bounds the whole
/// request: the handler itself cannot be interrupted (v1
/// limit), but a late response is never written to a dead
/// socket — the wait here is recv_timeout(time_left).
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn handle_connection(
    stream: std::net::TcpStream,
    config: std::sync::Arc<rustls::ServerConfig>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
) {
    let timeout_secs = {
        #[cfg(test)]
        {
            server_timeout_secs()
        }
        #[cfg(not(test))]
        {
            TIMEOUT_SECS
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(timeout_secs)));
    // Meeting 27 §42: with the `http2` feature the TLS
    // handshake runs through tokio-rustls on a dedicated
    // runtime and ALPN decides h2 vs the HTTP/1.1 fallback
    // (which runs here, sync, exactly as before).
    #[cfg(feature = "http2")]
    {
        handle_connection_h2_or_http1(stream, config, job_tx, deadline);
        return;
    }
    #[cfg(not(feature = "http2"))]
    {
        let conn = match rustls::ServerConnection::new(config) {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut tls = rustls::StreamOwned::new(conn, stream);
        handle_http1(&mut tls, job_tx, deadline);
    }
}

/// The HTTP/1.1 exchange over an established TLS stream:
/// read the request, send the job to the engine thread,
/// write the reply, close gracefully. Shared by the
/// no-http2 build and the http2 ALPN fallback.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn handle_http1(
    tls: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
    deadline: std::time::Instant,
) {
    let req = match read_request(tls, deadline) {
        Ok(r) => r,
        Err(RequestError::TooBig) => {
            let bytes = build_response(413, "الطلب أكبر من الحد", &[]);
            write_all_deadline(tls, &bytes, deadline);
            close_tls_gracefully(tls);
            return;
        }
        Err(RequestError::Bad) => return,
    };
    let (reply_tx, reply_rx) = std::sync::mpsc::channel::<ServerReply>();
    if job_tx.send(ServerJob { req, reply: reply_tx }).is_err() {
        return;
    }
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    match reply_rx.recv_timeout(left) {
        Ok(reply) => {
            let bytes = build_response_bytes(reply.status, &reply.headers, &reply.body);
            write_all_deadline(tls, &bytes, deadline);
            close_tls_gracefully(tls);
        }
        Err(_) => {}
    }
}

// ----------------------------------------------------------------
// HTTP/2 server (Meeting 27, DESIGN §42)
// ----------------------------------------------------------------

/// What the async server handshake produced: either the h2
/// connection was served inside the runtime (Done), or the
/// (already handshaked) TLS connection came back for the
/// HTTP/1.1 fallback — the handshake exchanged no
/// application data, so the sync StreamOwned can continue
/// with read_request.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
enum ServerOutcome {
    Done,
    Http1(std::net::TcpStream, rustls::ServerConnection),
}

/// The connection thread when the `http2` feature is on:
/// TLS handshake + ALPN dispatch on a dedicated runtime;
/// the h2 connection is served there, the HTTP/1.1
/// fallback comes back here as a converted StreamOwned.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
fn handle_connection_h2_or_http1(
    stream: std::net::TcpStream,
    config: std::sync::Arc<rustls::ServerConfig>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
    deadline: std::time::Instant,
) {
    let timeout = deadline.saturating_duration_since(std::time::Instant::now());
    let job_tx_async = job_tx.clone();
    let outcome = run_on_runtime(async move {
        serve_h2_or_http1_async(stream, config, job_tx_async, timeout).await
    });
    if let Ok(ServerOutcome::Http1(std_stream, conn)) = outcome {
        // Back to blocking mode with the connection
        // timeouts: from_std required non-blocking; the sync
        // StreamOwned path needs blocking + SO_RCVTIMEO.
        let _ = std_stream.set_nonblocking(false);
        let _ = std_stream.set_read_timeout(Some(timeout));
        let _ = std_stream.set_write_timeout(Some(timeout));
        let mut tls = rustls::StreamOwned::new(conn, std_stream);
        handle_http1(&mut tls, job_tx, deadline);
    }
    // Done (h2 served, or the connection failed before
    // dispatch) and Err(()) (runtime/thread failure) both
    // end the connection here.
}

/// Async half of handle_connection_h2_or_http1: convert the
/// connected socket, run the TLS handshake through
/// tokio-rustls, and dispatch on the negotiated ALPN
/// protocol.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn serve_h2_or_http1_async(
    stream: std::net::TcpStream,
    config: std::sync::Arc<rustls::ServerConfig>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
    timeout: std::time::Duration,
) -> ServerOutcome {
    let _ = stream.set_nonblocking(true);
    let tcp = match tokio::net::TcpStream::from_std(stream) {
        Ok(t) => t,
        Err(_) => return ServerOutcome::Done,
    };
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let tls = match tokio::time::timeout(timeout, acceptor.accept(tcp)).await {
        Ok(Ok(t)) => t,
        _ => return ServerOutcome::Done,
    };
    if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
        serve_h2_connection(tls, job_tx, timeout).await;
        ServerOutcome::Done
    } else {
        let (tcp, conn) = tls.into_inner();
        match tcp.into_std() {
            Ok(std_stream) => ServerOutcome::Http1(std_stream, conn),
            Err(_) => ServerOutcome::Done,
        }
    }
}

/// The h2 connection loop: accept streams until the
/// connection closes or errors; each stream is handled on
/// its own task so streams run concurrently (multiplexing).
/// At most 100 concurrent streams (h2 Builder setting).
/// An idle connection — no stream for the whole timeout
/// and no stream running — is closed, matching the
/// HTTP/1.1 socket timeout; a connection with streams
/// running stays alive until they finish.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn serve_h2_connection(
    tls: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
    timeout: std::time::Duration,
) {
    let mut conn = match tokio::time::timeout(
        timeout,
        h2::server::Builder::new()
            .max_concurrent_streams(100)
            .handshake(tls),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => return,
    };
    let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    loop {
        match tokio::time::timeout(timeout, conn.accept()).await {
            Ok(Some(Ok((req, respond)))) => {
                let job_tx = job_tx.clone();
                let active = std::sync::Arc::clone(&active);
                tokio::spawn(async move {
                    handle_h2_stream(req, respond, job_tx, timeout, active).await;
                });
            }
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => {
                if active.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                    break;
                }
            }
        }
    }
}

/// Counts one live h2 stream; the count drops when the
/// stream task exits (Drop), even on panic.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
struct H2StreamGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
impl H2StreamGuard {
    fn new(active: &std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        active.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(std::sync::Arc::clone(active))
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
impl Drop for H2StreamGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// One h2 stream: read the request (headers + body, 5 MB
/// cap, flow-control window released as the body is read),
/// send the job to the engine thread, poll the reply, and
/// send the response. Any failure resets the stream; a
/// handler that never answers within the deadline leaves
/// the stream dropped (the handler still runs on the engine
/// thread; its reply is discarded — the documented limit,
/// same as the HTTP/1.1 recv_timeout path).
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn handle_h2_stream(
    req: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<bytes::Bytes>,
    job_tx: std::sync::mpsc::Sender<ServerJob>,
    timeout: std::time::Duration,
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let _guard = H2StreamGuard::new(&active);
    let deadline = std::time::Instant::now() + timeout;
    let (parts, mut body_stream) = req.into_parts();
    let method = parts.method.to_string();
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().unwrap_or("").to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in parts.headers.iter() {
        headers.push((
            name.as_str().to_string(),
            String::from_utf8_lossy(value.as_bytes()).into_owned(),
        ));
    }
    let mut body: Vec<u8> = Vec::new();
    let mut too_big = false;
    loop {
        let left = match time_left_async(deadline) {
            Some(l) => l,
            None => {
                respond.send_reset(h2::Reason::CANCEL);
                return;
            }
        };
        match tokio::time::timeout(left, body_stream.data()).await {
            Ok(Some(Ok(chunk))) => {
                let _ = body_stream.flow_control().release_capacity(chunk.len());
                body.extend_from_slice(&chunk);
                if body.len() > MAX_SERVER_REQUEST_BYTES {
                    too_big = true;
                    break;
                }
            }
            Ok(Some(Err(_))) => {
                respond.send_reset(h2::Reason::CANCEL);
                return;
            }
            Ok(None) => break,
            Err(_) => {
                respond.send_reset(h2::Reason::CANCEL);
                return;
            }
        }
        if body_stream.is_end_stream() {
            break;
        }
    }
    if too_big {
        let _ = send_h2_response(
            &mut respond,
            413,
            &[],
            "الطلب أكبر من الحد".as_bytes(),
            deadline,
        )
        .await;
        return;
    }
    let http_req = HttpRequest {
        method,
        path,
        query,
        headers,
        body,
    };
    let (reply_tx, reply_rx) = std::sync::mpsc::channel::<ServerReply>();
    if job_tx
        .send(ServerJob {
            req: http_req,
            reply: reply_tx,
        })
        .is_err()
    {
        respond.send_reset(h2::Reason::CANCEL);
        return;
    }
    // Poll the reply without blocking the runtime: the
    // engine thread answers through the std channel.
    let reply = loop {
        match reply_rx.try_recv() {
            Ok(r) => break Some(r),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    };
    let Some(reply) = reply else {
        respond.send_reset(h2::Reason::CANCEL);
        return;
    };
    let _ = send_h2_response(
        &mut respond,
        reply.status,
        &reply.headers,
        &reply.body,
        deadline,
    )
    .await;
}

/// Send one h2 response: headers first, then the body in
/// chunks that fit the peer's flow-control window (reserve
/// once per chunk, wait for capacity; the body is at most
/// 5 MB, handler text is usually far smaller). A response
/// that cannot be sent (peer gone, deadline passed) is an
/// error for the caller to drop — the stream resets.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn send_h2_response(
    respond: &mut h2::server::SendResponse<bytes::Bytes>,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    deadline: std::time::Instant,
) -> Result<(), ()> {
    let mut builder = http::Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    // The headers passed run_handler_parts, so the build
    // only fails on a status outside 100..=599 — which the
    // handler never produces. A failure resets the stream.
    let response = builder.body(()).map_err(|_| ())?;
    let end_of_stream = body.is_empty();
    let mut send = respond
        .send_response(response, end_of_stream)
        .map_err(|_| ())?;
    if body.is_empty() {
        return Ok(());
    }
    // 16 KB chunks: the h2 default maximum frame size.
    let mut offset = 0usize;
    while offset < body.len() {
        let chunk_len = (body.len() - offset).min(16 * 1024);
        let last = offset + chunk_len == body.len();
        send.reserve_capacity(chunk_len);
        loop {
            if send.capacity() >= chunk_len {
                break;
            }
            let left = time_left_async(deadline).ok_or(())?;
            match tokio::time::timeout(
                left,
                std::future::poll_fn(|cx| send.poll_capacity(cx)),
            )
            .await
            {
                Ok(Some(Ok(_))) => continue,
                _ => return Err(()),
            }
        }
        let chunk = bytes::Bytes::copy_from_slice(&body[offset..offset + chunk_len]);
        send.send_data(chunk, last).map_err(|_| ())?;
        offset += chunk_len;
    }
    Ok(())
}

/// Send the TLS close_notify and flush, so the client sees a
/// clean EOF instead of an abrupt close (a rustls client
/// reports an abrupt close as UnexpectedEof on read).
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn close_tls_gracefully(
    tls: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
) {
    use std::io::Write;
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
enum RequestError {
    TooBig,
    Bad,
}

/// Read an HTTP/1.1 request: headers first (capped at 64 KB),
/// then the body by Content-Length or chunked transfer. The
/// total (headers + body) is capped at 5 MB. A request line
/// must be `METHOD SP TARGET SP HTTP/x.y`; header lines with
/// control characters are refused; the body is returned raw
/// (the handler sees it as lossy UTF-8 text).
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn read_request(
    stream: &mut impl TimedRead,
    deadline: std::time::Instant,
) -> Result<HttpRequest, RequestError> {
    use std::io::Read;
    const HEADER_CAP: usize = 64 * 1024;
    let mut head: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    let mut header_end = None;
    while head.len() < HEADER_CAP {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(RequestError::Bad);
        }
        let _ = stream.set_read_timeout(Some(left));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                head.push(byte[0]);
                if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
                    header_end = Some(head.len());
                    break;
                }
            }
            Err(_) => return Err(RequestError::Bad),
        }
    }
    let header_end = match header_end {
        Some(n) => n,
        None => return Err(RequestError::Bad),
    };
    let header_text = String::from_utf8_lossy(&head[..header_end]).into_owned();
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("");
    if method.is_empty() || target.is_empty() || !version.starts_with("HTTP/") {
        return Err(RequestError::Bad);
    }
    let (path, query) = match target.find('?') {
        Some(i) => (target[..i].to_string(), target[i + 1..].to_string()),
        None => (target, String::new()),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7F) {
            return Err(RequestError::Bad);
        }
        let Some(colon) = line.find(':') else {
            return Err(RequestError::Bad);
        };
        let name = line[..colon].trim().to_string();
        let value = line[colon + 1..].trim().to_string();
        if name.is_empty() {
            return Err(RequestError::Bad);
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().ok();
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
        headers.push((name, value));
    }
    let mut body: Vec<u8> = Vec::new();
    if chunked {
        loop {
            let size_line = read_request_line(stream, deadline, 256)?;
            let size_text = String::from_utf8_lossy(&size_line);
            let size_text = size_text.split(';').next().unwrap_or("").trim();
            let size =
                usize::from_str_radix(size_text, 16).map_err(|_| RequestError::Bad)?;
            if header_end + body.len() + size > MAX_SERVER_REQUEST_BYTES {
                return Err(RequestError::TooBig);
            }
            if size == 0 {
                let _ = read_request_line(stream, deadline, 256);
                break;
            }
            read_exact_capped(stream, &mut body, size, deadline)?;
            let crlf = read_request_line(stream, deadline, 256)?;
            if !crlf.is_empty() {
                return Err(RequestError::Bad);
            }
        }
    } else if let Some(len) = content_length {
        if header_end + len > MAX_SERVER_REQUEST_BYTES {
            return Err(RequestError::TooBig);
        }
        read_exact_capped(stream, &mut body, len, deadline)?;
    }
    Ok(HttpRequest {
        method,
        path,
        query,
        headers,
        body,
    })
}

/// Read one CRLF-terminated line (without the CRLF), capped.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn read_request_line(
    stream: &mut impl TimedRead,
    deadline: std::time::Instant,
    cap: usize,
) -> Result<Vec<u8>, RequestError> {
    use std::io::Read;
    let mut line: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < cap {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(RequestError::Bad);
        }
        let _ = stream.set_read_timeout(Some(left));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if line.len() >= 2 && &line[line.len() - 2..] == b"\r\n" {
                    line.truncate(line.len() - 2);
                    return Ok(line);
                }
            }
            Err(_) => return Err(RequestError::Bad),
        }
    }
    Err(RequestError::Bad)
}

/// Read exactly `len` bytes, bounded by the deadline.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn read_exact_capped(
    stream: &mut impl TimedRead,
    buf: &mut Vec<u8>,
    len: usize,
    deadline: std::time::Instant,
) -> Result<(), RequestError> {
    use std::io::Read;
    let mut remaining = len;
    let mut chunk = [0u8; 8192];
    while remaining > 0 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(RequestError::Bad);
        }
        let _ = stream.set_read_timeout(Some(left));
        let n = std::cmp::min(remaining, chunk.len());
        match stream.read(&mut chunk[..n]) {
            Ok(0) => return Err(RequestError::Bad),
            Ok(m) => {
                buf.extend_from_slice(&chunk[..m]);
                remaining -= m;
            }
            Err(_) => return Err(RequestError::Bad),
        }
    }
    Ok(())
}

/// Run the Niva handler on the engine thread: build the
/// request dict (طريقة، مسار، استعلام، ترويسات، جسم) and
/// call the handler through the engine context. The handler
/// returns text (→ 200) or a response dict (حالة، نص،
/// ترويسات). Handler اطبع output is printed straight to
/// the server's stdout (the engine buffer is only flushed
/// when run() returns, and a server never returns). A
/// handler error becomes a generic 500 — the real error
/// goes to the server operator's stderr only, never to the
/// client.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
/// Run the Niva handler for one request and return the
/// response as parts (status, headers, body text). The
/// header policy — valid tokens and values only, otherwise
/// a generic 500; Content-Length and Connection managed by
/// the server; Content-Type defaults to text/plain;
/// charset=utf-8 — is enforced here once, so the HTTP/1.1
/// formatter and the h2 sender share exactly the same
/// reply. The handler still runs only on the engine thread.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn run_handler_parts(
    ctx: &mut dyn crate::stdlib::HandlerCaller,
    handler: &Value,
    req: HttpRequest,
    line: usize,
    col: usize,
) -> (u16, Vec<(String, String)>, String) {
    let mut header_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (name, value) in req.headers {
        header_map
            .entry(name)
            .and_modify(|v| {
                v.push_str(", ");
                v.push_str(&value);
            })
            .or_insert(value);
    }
    let header_values: Vec<(String, Value)> = header_map
        .into_iter()
        .map(|(k, v)| (k, Value::Str(v)))
        .collect();
    let req_dict = vec![
        ("طريقة".to_string(), Value::Str(req.method)),
        ("مسار".to_string(), Value::Str(req.path)),
        ("استعلام".to_string(), Value::Str(req.query)),
        ("ترويسات".to_string(), Value::Dict(header_values)),
        (
            "جسم".to_string(),
            Value::Str(String::from_utf8_lossy(&req.body).into_owned()),
        ),
    ];
    let (res, produced) = ctx.call_handler(handler, Value::Dict(req_dict), line, col);
    if !produced.is_empty() {
        use std::io::Write;
        print!("{}", produced);
        let _ = std::io::stdout().flush();
    }
    let (status, extra, text): (u16, Vec<(String, String)>, String) = match res {
        Ok(Value::Str(text)) => (200, Vec::new(), text),
        Ok(Value::Dict(pairs)) => {
            let status = match dict_get(&pairs, "حالة") {
                Some(Value::Number(n)) if *n >= 100.0 && *n <= 599.0 && n.fract() == 0.0 => {
                    *n as u16
                }
                Some(_) => return (500, Vec::new(), "خطأ في معالج الطلب".to_string()),
                None => 200,
            };
            let text = match dict_get(&pairs, "نص") {
                Some(Value::Str(s)) => s.clone(),
                Some(_) => return (500, Vec::new(), "خطأ في معالج الطلب".to_string()),
                None => String::new(),
            };
            let extra: Vec<(String, String)> = match dict_get(&pairs, "ترويسات") {
                Some(Value::Dict(d)) => {
                    let mut v = Vec::new();
                    for (k, val) in d {
                        match val {
                            Value::Str(s) => v.push((k.clone(), s.clone())),
                            _ => {
                                return (
                                    500,
                                    Vec::new(),
                                    "خطأ في معالج الطلب".to_string(),
                                )
                            }
                        }
                    }
                    v
                }
                Some(_) => return (500, Vec::new(), "خطأ في معالج الطلب".to_string()),
                None => Vec::new(),
            };
            (status, extra, text)
        }
        Ok(_) => (500, Vec::new(), "خطأ في معالج الطلب".to_string()),
        Err(e) => {
            eprintln!("خطأ في معالج الطلب: {}", e);
            (500, Vec::new(), "خطأ في معالج الطلب".to_string())
        }
    };
    // Header policy, shared by both protocols: an invalid
    // name or value turns the whole reply into a generic
    // 500 (never emit a broken header).
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut has_content_type = false;
    for (name, value) in extra {
        if !is_token(&name) || !is_header_value(&value) {
            return (500, Vec::new(), "خطأ في معالج الطلب".to_string());
        }
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        headers.push((name, value));
    }
    if !has_content_type {
        headers.push((
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
    }
    (status, headers, text)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn dict_get<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Build a raw HTTP/1.1 response. Header names must be valid
/// tokens and values must carry no CR/LF/control — otherwise
/// the whole response is a generic 500 (never emit a broken
/// header). Content-Length and Connection are managed by the
/// server; Content-Type defaults to text/plain; charset=utf-8
/// unless the handler sets it.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn build_response(status: u16, text: &str, extra: &[(String, String)]) -> Vec<u8> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut has_content_type = false;
    for (name, value) in extra {
        if !is_token(name) || !is_header_value(value) {
            return build_response(500, "خطأ في معالج الطلب", &[]);
        }
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        headers.push((name.clone(), value.clone()));
    }
    if !has_content_type {
        headers.push((
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
    }
    build_response_bytes(status, &headers, text.as_bytes())
}

/// Format already-validated reply parts (headers from
/// run_handler_parts) as raw HTTP/1.1 bytes. The 413 path
/// and the HTTP/1.1 connection handler use this directly.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn build_response_bytes(status: u16, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let reason = reason_phrase(status);
    let mut out = if reason.is_empty() {
        format!("HTTP/1.1 {}\r\n", status).into_bytes()
    } else {
        format!("HTTP/1.1 {} {}\r\n", status, reason).into_bytes()
    };
    for (name, value) in headers {
        out.extend_from_slice(format!("{}: {}\r\n", name, value).as_bytes());
    }
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b > 0x20
                && b < 0x7F
                && !matches!(
                    b,
                    b'(' | b')'
                        | b'<'
                        | b'>'
                        | b'@'
                        | b','
                        | b';'
                        | b':'
                        | b'\\'
                        | b'"'
                        | b'/'
                        | b'['
                        | b']'
                        | b'?'
                        | b'='
                        | b'{'
                        | b'}'
                        | b' '
                        | b'\t'
                )
        })
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn is_header_value(s: &str) -> bool {
    s.bytes().all(|b| (b >= 0x20 && b != 0x7F) || b == b'\t')
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        500 => "Internal Server Error",
        _ => "",
    }
}

/// Write all bytes, bounded by the deadline. Errors are
/// ignored (a dead socket just closes).
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn write_all_deadline(
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
    bytes: &[u8],
    deadline: std::time::Instant,
) {
    use std::io::Write;
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        return;
    }
    let _ = stream.get_mut().set_write_timeout(Some(left));
    let _ = stream.write_all(bytes);
    let _ = stream.flush();
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
pub(crate) fn ip_class(ip: &std::net::IpAddr) -> Option<&'static str> {
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
                // Meeting 26: an HTTPS→HTTP downgrade is refused;
                // an HTTP→HTTPS upgrade is allowed (same host,
                // checked below). Without the `https` feature the
                // upgrade target is refused inside fetch_one_native
                // with the old error — the same as a direct HTTPS
                // fetch without the feature.
                if current.scheme == "https" && next.scheme == "http" {
                    return Err(RioError::runtime(
                        "إعادة التوجيه من HTTPS إلى HTTP مرفوضة",
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

/// ترميز هدف الطلب (المسار + الاستعلام) بالنسب المئوية
/// قبل إرساله في سطر الطلب — على نمط curl.
///
/// لماذا: سطر الطلب في HTTP/1.1 بايتات ASCII؛ والمسار
/// العربي الخام (UTF-8) يرفضه المحلل الصارم (llhttp في
/// Node) برمز 400 قبل أن يصل الطلب إلى المعالج، بينما
/// المسار ASCII نفسه على الخادم نفسه ينجح (200). والترميز
/// هنا مرة واحدة عند الإرسال فيغطي HTTP/1.1 وHTTP/2
/// (مكتبة http ترفض الـ URI غير ASCII أصلا) وإعادات
/// التوجيه (تمر من هنا ثانية عند كل محاولة).
///
/// القواعد:
/// - المخطط والمضيف والمنفذ لا تُمس (الترميز للمسار
///   والاستعلام فقط).
/// - يبقى كما هو كل بايت ASCII من: غير المحجوز
///   (A-Z a-z 0-9 - _ . ~) + المحددات الفرعية
///   (! $ & ' ( ) * + , ; =) + ( : @ / ? ) — فالمسارات
///   ASCII تبقى بايتيا كما هي.
/// - `%` متبوعة برقمين ست عشريين تُترك كما هي (مرمّز
///   أصلا — بلا ترميز مزدوج)؛ و`%` غير ذلك تُرمّز
///   هي نفسها (%25).
/// - كل بايت آخر (ومنها كل بايت UTF-8 غير ASCII
///   والمسافة) يُرمّز %XX بأحرف ست عشرية كبيرة.
/// - الجزء (#...) لا يُرسل أبدا (كما يفعل curl؛
///   وparse_url يسقطه أصلا حين يكون المسار كله جزءا).
/// - مسار فارغ (دفاعا) يصير "/".
#[cfg(not(target_arch = "wasm32"))]
fn encode_request_target(path: &str) -> String {
    // الجزء لا يُرسل.
    let path = match path.find('#') {
        Some(i) => &path[..i],
        None => path,
    };
    let (head, query) = match path.find('?') {
        Some(i) => (&path[..i], Some(&path[i + 1..])),
        None => (path, None),
    };
    let mut out = String::with_capacity(path.len());
    if head.is_empty() {
        out.push('/');
    } else {
        encode_target_part(&mut out, head);
    }
    if let Some(q) = query {
        out.push('?');
        encode_target_part(&mut out, q);
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_target_part(out: &mut String, part: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let bytes = part.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // ثلاثية مرمّزة أصلا: تُترك (بلا ترميز مزدوج).
        if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
            continue;
        }
        if is_target_safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xF) as usize] as char);
        }
        i += 1;
    }
}

/// بايت ASCII يُترك كما هو في هدف الطلب: غير محجوز +
/// محددات فرعية + ( : @ / ? ).
#[cfg(not(target_arch = "wasm32"))]
fn is_target_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'~'
            | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
            | b':' | b'@' | b'/' | b'?'
        )
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_one_native(
    url: &ParsedUrl,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};

    // Meeting 26: without the `https` feature, HTTPS is refused
    // before any traffic (no DNS, no connect), with the old
    // error. With the feature, the TLS wrap below runs after
    // the §7 IP check.
    #[cfg(not(feature = "https"))]
    if url.scheme == "https" {
        return Err(RioError::runtime(
            "HTTPS غير مدعوم في النسخة الأصلية — استخدم التطبيق أو المتصفح للروابط المشفرة",
            line,
            col,
        ));
    }

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
    // ترميز هدف الطلب قبل الإرسال: المسار العربي الخام
    // يرفضه المحلل الصارم (llhttp) برمز 400. والهدف
    // المرمّز نفسه يمر إلى HTTP/2 أدناه.
    let target = encode_request_target(&url.path);
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Niva/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        target, host_header
    );
    // Meeting 26: HTTPS wraps the connected TcpStream in TLS
    // (after the §7 IP check above, before the handshake).
    // Meeting 27 §42: with the `http2` feature the handshake
    // runs through tokio-rustls on a dedicated thread and
    // ALPN decides h2 vs the HTTP/1.1 fallback.
    #[cfg(feature = "https")]
    if url.scheme == "https" {
        #[cfg(feature = "http2")]
        {
            return fetch_https_h2(stream, url, &request, &target, &host_header, deadline, line, col);
        }
        #[cfg(not(feature = "http2"))]
        {
            let mut tls = tls_wrap(stream, &url.host, line, col)?;
            tls.write_all(request.as_bytes()).map_err(|e| {
                io_timeout_or(
                    e,
                    "انتهت مهلة إرسال الطلب",
                    &format!("تعذر إرسال الطلب إلى {}", url.host),
                    line,
                    col,
                )
            })?;
            return read_response(&mut tls, deadline, line, col);
        }
    }
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

// ----------------------------------------------------------------
// HTTP/2 client (Meeting 27, DESIGN §42)
// ----------------------------------------------------------------

/// Run an async future to completion on a dedicated thread
/// with a fresh current-thread tokio runtime, and return its
/// result. The engine thread never enters the runtime: it
/// spawns this thread and joins it. A thread panic, or a
/// runtime that cannot be built, is Err(()), and the caller
/// maps it to one Arabic error. One runtime per operation —
/// no global runtime, no state shared between fetches.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
fn run_on_runtime<T: Send + 'static>(
    fut: impl std::future::Future<Output = T> + Send + 'static,
) -> Result<T, ()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        match rt {
            Ok(rt) => Some(rt.block_on(fut)),
            Err(_) => None,
        }
    })
    .join()
    .map_err(|_| ())?
    .ok_or(())
}

/// What the async HTTPS handshake produced: either the h2
/// exchange already finished, or the (already handshaked)
/// TLS connection came back for the HTTP/1.1 fallback — the
/// handshake exchanged no application data, so the sync
/// StreamOwned can continue with the request write.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
enum HttpsOutcome {
    H2(FetchOutcome),
    Http1(std::net::TcpStream, rustls::ClientConnection),
}

/// The HTTPS branch of fetch_one_native when the `http2`
/// feature is on: TLS handshake + ALPN dispatch on a
/// dedicated thread, then either the h2 exchange (done
/// there) or the HTTP/1.1 fallback (done here, sync).
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
fn fetch_https_h2(
    stream: std::net::TcpStream,
    url: &ParsedUrl,
    request: &str,
    path: &str,
    host_header: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
    use std::io::Write;
    let host = url.host.clone();
    let host_async = host.clone();
    // الهدف المرمّز من fetch_one_native (لا url.path الخام:
    // مكتبة http ترفض الـ URI غير ASCII أصلا).
    let path = path.to_string();
    let host_header = host_header.to_string();
    let outcome = run_on_runtime(async move {
        fetch_https_h2_async(stream, &host_async, &path, &host_header, deadline, line, col).await
    })
    .map_err(|_| RioError::runtime("تعذر تنفيذ طلب الشبكة", line, col))?;
    match outcome {
        Ok(HttpsOutcome::H2(outcome)) => Ok(outcome),
        Ok(HttpsOutcome::Http1(std_stream, conn)) => {
            // Back to blocking mode with the fetch timeouts:
            // from_std required non-blocking; the sync
            // StreamOwned path needs blocking + SO_RCVTIMEO.
            let _ = std_stream.set_nonblocking(false);
            let _ = std_stream.set_read_timeout(Some(time_left(deadline, line, col)?));
            let _ = std_stream.set_write_timeout(Some(time_left(deadline, line, col)?));
            let mut tls = rustls::StreamOwned::new(conn, std_stream);
            tls.write_all(request.as_bytes()).map_err(|e| {
                io_timeout_or(
                    e,
                    "انتهت مهلة إرسال الطلب",
                    &format!("تعذر إرسال الطلب إلى {}", host),
                    line,
                    col,
                )
            })?;
            read_response(&mut tls, deadline, line, col)
        }
        Err(e) => Err(e),
    }
}

/// Time left until the fetch deadline, for the async side
/// (tokio::time::timeout needs a Duration, not an Instant).
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
fn time_left_async(deadline: std::time::Instant) -> Option<std::time::Duration> {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        None
    } else {
        Some(left)
    }
}

/// Async half of fetch_https_h2: convert the connected
/// socket, run the TLS handshake through tokio-rustls, and
/// dispatch on the negotiated ALPN protocol. The IP check
/// and the DNS resolution already ran on the sync side,
/// before this function was called.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn fetch_https_h2_async(
    stream: std::net::TcpStream,
    host: &str,
    path: &str,
    host_header: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<HttpsOutcome> {
    // tokio needs a non-blocking socket; the sync caller set
    // SO_RCVTIMEO timeouts on the fd, which stay set.
    let _ = stream.set_nonblocking(true);
    let tcp = tokio::net::TcpStream::from_std(stream)
        .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTPS", line, col))?;
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| {
            RioError::runtime(format!("اسم مضيف غير صالح لـ HTTPS: {}", host), line, col)
        })?;
    let connector =
        tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_client_config()));
    let left = time_left_async(deadline).ok_or_else(|| {
        RioError::runtime("انتهت المهلة الإجمالية (30 ثانية)", line, col)
    })?;
    let tls = tokio::time::timeout(left, connector.connect(server_name, tcp))
        .await
        .map_err(|_| RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col))?
        .map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة الاتصال (30 ثانية)",
                "تعذر إنشاء اتصال HTTPS آمن",
                line,
                col,
            )
        })?;
    if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
        let outcome = fetch_h2(tls, path, host_header, deadline, line, col).await?;
        Ok(HttpsOutcome::H2(outcome))
    } else {
        let (tcp, conn) = tls.into_inner();
        let std_stream = tcp
            .into_std()
            .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTPS", line, col))?;
        Ok(HttpsOutcome::Http1(std_stream, conn))
    }
}

/// One h2 request over an established TLS stream: handshake,
/// send GET, read the response. The h2 connection future is
/// driven on a spawned task (the canonical h2 pattern); the
/// request runs on this task. Same result mapping as
/// read_response (2xx → Body, 3xx + Location → Redirect,
/// other statuses → Arabic error), same 5 MB cap, same
/// total deadline — every await is wrapped in the time left.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn fetch_h2(
    tls: tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    path: &str,
    host_header: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
    let (send, conn) = h2::client::handshake(tls)
        .await
        .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTP/2", line, col))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let left = time_left_async(deadline).ok_or_else(|| {
        RioError::runtime("انتهت المهلة الإجمالية (30 ثانية)", line, col)
    })?;
    let mut send = tokio::time::timeout(left, send.ready())
        .await
        .map_err(|_| RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col))?
        .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTP/2", line, col))?;
    // Absolute-form URI so h2 sends :scheme=https (a
    // path-only URI would default :scheme to http).
    let uri = format!("https://{}{}", host_header, path);
    let req = http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", host_header)
        .header("user-agent", "Niva/1.0")
        .header("accept", "*/*")
        .body(())
        .map_err(|_| RioError::runtime("تعذر بناء طلب HTTP/2", line, col))?;
    let (resp_fut, _send_stream) = send
        .send_request(req, true)
        .map_err(|_| RioError::runtime("تعذر إرسال طلب HTTP/2", line, col))?;
    let left = time_left_async(deadline).ok_or_else(|| {
        RioError::runtime("انتهت المهلة الإجمالية (30 ثانية)", line, col)
    })?;
    let resp = tokio::time::timeout(left, resp_fut)
        .await
        .map_err(|_| RioError::runtime("انتهت مهلة قراءة الرد (30 ثانية)", line, col))?
        .map_err(|_| RioError::runtime("تعذر قراءة الرد من الخادم", line, col))?;
    let (parts, mut body_stream) = resp.into_parts();
    let status = parts.status.as_u16();
    let location = parts
        .headers
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let mut body: Vec<u8> = Vec::new();
    let cap = MAX_RESPONSE_BYTES + 1;
    while let Some(chunk) = {
        let left = match time_left_async(deadline) {
            Some(l) => l,
            None => {
                return Err(RioError::runtime(
                    "انتهت المهلة الإجمالية (30 ثانية)",
                    line,
                    col,
                ))
            }
        };
        tokio::time::timeout(left, body_stream.data())
            .await
            .map_err(|_| RioError::runtime("انتهت مهلة قراءة الرد (30 ثانية)", line, col))?
    } {
        let chunk = chunk
            .map_err(|_| RioError::runtime("تعذر قراءة الرد من الخادم", line, col))?;
        // Release the flow-control window as the body is
        // consumed, so a 5 MB body never stalls the peer.
        let _ = body_stream.flow_control().release_capacity(chunk.len());
        body.extend_from_slice(&chunk);
        if body.len() > cap {
            return Err(RioError::runtime(
                "الرد أكبر من الحد الأقصى (5 ميغابايت)",
                line,
                col,
            ));
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

/// A readable stream whose read timeout can be re-armed with
/// the time left until the total fetch deadline. The plain
/// TcpStream and the TLS stream both implement it (the TLS
/// stream delegates to its underlying TcpStream, so a socket
/// timeout still surfaces as an error, never as fake EOF).
#[cfg(not(target_arch = "wasm32"))]
trait TimedRead: std::io::Read {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()>;
}

#[cfg(not(target_arch = "wasm32"))]
impl TimedRead for std::net::TcpStream {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        std::net::TcpStream::set_read_timeout(self, dur)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
impl TimedRead for rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream> {
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        self.get_mut().set_read_timeout(dur)
    }
}

/// Test-only TLS roots (DER bytes of extra CA certificates),
/// injected by the TLS tests so a local server whose
/// certificate is signed by the test CA is accepted. Never
/// set outside tests: production trusts Mozilla roots only,
/// and there is no user-facing way to add a root.
#[cfg(test)]
static TEST_TLS_ROOTS: LazyLock<Mutex<Vec<Vec<u8>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

#[cfg(test)]
fn set_test_tls_roots(ders: Vec<Vec<u8>>) {
    if let Ok(mut guard) = TEST_TLS_ROOTS.lock() {
        *guard = ders;
    }
}

#[cfg(test)]
fn clear_test_tls_roots() {
    if let Ok(mut guard) = TEST_TLS_ROOTS.lock() {
        guard.clear();
    }
}

/// TLS client configuration (Meeting 26, §41): Mozilla roots
/// (webpki-roots), full chain verification, self-signed
/// rejected, TLS 1.2+ only (rustls 0.23 never offers older),
/// SNI = the hostname. No client certificates, no custom CA
/// in v1, and no verification-bypass flag — ever.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn tls_client_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    #[cfg(test)]
    {
        if let Ok(guard) = TEST_TLS_ROOTS.lock() {
            for der in guard.iter() {
                let _ = roots.add(rustls::pki_types::CertificateDer::from(der.clone()));
            }
        }
    }
    // `mut` only when the `http2` feature sets ALPN below.
    #[cfg_attr(not(feature = "http2"), allow(unused_mut))]
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    // Meeting 27 §42: offer h2 first, then HTTP/1.1 — the
    // peer picks; no ALPN from the peer means the HTTP/1.1
    // fallback path. Only when the `http2` feature is on.
    #[cfg(feature = "http2")]
    {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    }
    config
}

/// Wrap a connected TcpStream in TLS. The handshake runs
/// lazily on the first write/read; a bad certificate fails
/// there, and the error is mapped to Arabic by io_timeout_or
/// (it finds the rustls error in the io error's chain).
/// Only without the `http2` feature: with it, the handshake
/// goes through tokio-rustls instead (fetch_https_h2_async).
#[cfg(all(not(target_arch = "wasm32"), feature = "https", not(feature = "http2")))]
fn tls_wrap(
    stream: std::net::TcpStream,
    host: &str,
    line: usize,
    col: usize,
) -> RioResult<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>> {
    let config = std::sync::Arc::new(tls_client_config());
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| {
            RioError::runtime(format!("اسم مضيف غير صالح لـ HTTPS: {}", host), line, col)
        })?;
    let conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTPS", line, col))?;
    Ok(rustls::StreamOwned::new(conn, stream))
}

#[cfg(not(target_arch = "wasm32"))]
fn io_timeout_or(
    e: std::io::Error,
    timeout_msg: &str,
    other_msg: &str,
    line: usize,
    col: usize,
) -> RioError {
    // Meeting 26: a TLS failure hides in the io error's chain
    // (rustls wraps its Error in the io error — conn.rs:701).
    // A certificate failure gets the certificate message; any
    // other TLS failure gets the handshake message. No raw
    // rustls text ever reaches the user.
    #[cfg(feature = "https")]
    {
        // get_ref() yields &(dyn Error + Send + Sync); the walk
        // continues on plain &(dyn Error) — source() returns
        // that shape, and downcast_ref only needs 'static.
        let mut current: Option<&(dyn std::error::Error + 'static)> =
            e.get_ref().map(|r| r as &(dyn std::error::Error + 'static));
        while let Some(err) = current {
            if let Some(rustls_err) = err.downcast_ref::<rustls::Error>() {
                return match rustls_err {
                    rustls::Error::InvalidCertificate(_)
                    | rustls::Error::NoCertificatesPresented
                    | rustls::Error::InvalidCertRevocationList(_) => RioError::runtime(
                        "شهادة الخادم غير موثوقة — رفض اتصال HTTPS",
                        line,
                        col,
                    ),
                    _ => RioError::runtime("تعذر إنشاء اتصال HTTPS آمن", line, col),
                };
            }
            current = err.source();
        }
    }
    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
        RioError::runtime(timeout_msg, line, col)
    } else {
        RioError::runtime(other_msg, line, col)
    }
}

/// Read an HTTP/1.1 response: headers first (capped), then the
/// body by Content-Length, chunked transfer, or until EOF.
/// The body is capped at 5 MB (+1 byte to detect the overflow).
/// Works over the plain TcpStream and the TLS stream alike
/// (Meeting 26): the stream type decides the timeout re-arm.
#[cfg(not(target_arch = "wasm32"))]
fn read_response(
    stream: &mut impl TimedRead,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<FetchOutcome> {
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
    stream: &mut impl TimedRead,
    deadline: std::time::Instant,
    cap: usize,
    line: usize,
    col: usize,
) -> RioResult<String> {
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

    // إصلاح ترميز المسار في اجلب: المسار العربي الخام
    // يُرمّز بالنسب المئوية قبل الإرسال.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_arabic_path() {
        assert_eq!(
            encode_request_target("/أداة.نيفا"),
            "/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_ascii_unchanged() {
        assert_eq!(encode_request_target("/"), "/");
        assert_eq!(
            encode_request_target("/a/b.txt?x=1&y=hello+world"),
            "/a/b.txt?x=1&y=hello+world"
        );
        assert_eq!(
            encode_request_target("/a-b_c.d~e!f$g&h'i(j)k*l+m,n;o=p:q@r"),
            "/a-b_c.d~e!f$g&h'i(j)k*l+m,n;o=p:q@r"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_already_encoded_kept() {
        assert_eq!(
            encode_request_target("/%D8%A3%D8%AF%D8%A7%D8%A9"),
            "/%D8%A3%D8%AF%D8%A7%D8%A9"
        );
        // ست عشري صغير يُترك كما هو أيضا: بلا إعادة
        // ترميز وبلا توحيد حالة الأحرف.
        assert_eq!(encode_request_target("/%d8%a3"), "/%d8%a3");
        assert_eq!(encode_request_target("/a%2Fb"), "/a%2Fb");
        // ثلاثية صحيحة منفردة تُترك (فك ترميزها عند
        // الخادم بايت تالف — لكن الإرسال يحفظها كما كُتبت).
        assert_eq!(encode_request_target("/%D8"), "/%D8");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_bare_percent_encoded() {
        assert_eq!(encode_request_target("/100%"), "/100%25");
        assert_eq!(encode_request_target("/%ZZ"), "/%25ZZ");
        assert_eq!(encode_request_target("/%2"), "/%252");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_arabic_query_and_space() {
        assert_eq!(
            encode_request_target("/بحث?q=مرحبا"),
            "/%D8%A8%D8%AD%D8%AB?q=%D9%85%D8%B1%D8%AD%D8%A8%D8%A7"
        );
        assert_eq!(encode_request_target("/a b"), "/a%20b");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn encode_target_fragment_stripped() {
        assert_eq!(encode_request_target("/p#frag"), "/p");
        assert_eq!(encode_request_target("/p?q=1#frag"), "/p?q=1");
        assert_eq!(encode_request_target(""), "/");
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
                        // إصلاح ترميز المسار: الخادم يرى المسار
                        // مرمّزا — فالرابط العربي الخام يصل هنا
                        // مرمّزا، والرابط المرمّز أصلا يصل كما هو.
                        // بلا الترميز كان المسار الخام يصل UTF-8
                        // خاما فيسقط في 404.
                        "/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7" => {
                            let b = "أداة من الخادم".as_bytes().to_vec();
                            (
                                "200 OK",
                                vec![format!("Content-Length: {}", b.len())],
                                b,
                            )
                        }
                        "/%D8%A8%D8%AD%D8%AB?q=%D9%85%D8%B1%D8%AD%D8%A8%D8%A7" => {
                            let b = "بحث ناجح".as_bytes().to_vec();
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

    // إصلاح ترميز المسار في اجلب: الرابط العربي الخام
    // يصل إلى الخادم مرمّزا (المسار والاستعلام)؛ والرابط
    // المرمّز أصلا يصل كما هو بلا ترميز مزدوج.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_arabic_path_encoded_on_wire() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        assert_eq!(
            fetch_ok(&format!("http://127.0.0.1:{}/أداة.نيفا", port)),
            "أداة من الخادم"
        );
        assert_eq!(
            fetch_ok(&format!(
                "http://127.0.0.1:{}/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7",
                port
            )),
            "أداة من الخادم"
        );
        assert_eq!(
            fetch_ok(&format!("http://127.0.0.1:{}/بحث?q=مرحبا", port)),
            "بحث ناجح"
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

    // Meeting 26: without the `https` feature, HTTPS is refused
    // before any traffic. With the feature, this same fetch
    // attempts a real TLS connection (see the TLS tests below).
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "https")))]
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

    // --- TLS tests (Meeting 26) ---
    // Fixtures in tests/fixtures/ (DER, generated once with
    // openssl — test CA + server cert signed by it + a
    // self-signed server cert; all SAN IP:127.0.0.1). The
    // keys are test keys only, never used in production.

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_fixture(name: &str) -> Vec<u8> {
        // include_bytes! is relative to this source file, so
        // the fixtures load no matter what the test process's
        // working directory is (fs::read would not).
        match name {
            "tls-ca-cert.der" => include_bytes!("../tests/fixtures/tls-ca-cert.der").to_vec(),
            "tls-server-cert.der" => {
                include_bytes!("../tests/fixtures/tls-server-cert.der").to_vec()
            }
            "tls-server-key.der" => {
                include_bytes!("../tests/fixtures/tls-server-key.der").to_vec()
            }
            "tls-selfsigned-cert.der" => {
                include_bytes!("../tests/fixtures/tls-selfsigned-cert.der").to_vec()
            }
            "tls-selfsigned-key.der" => {
                include_bytes!("../tests/fixtures/tls-selfsigned-key.der").to_vec()
            }
            _ => panic!("unknown TLS fixture: {}", name),
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_server_config(
        cert_der: &[u8],
        key_der: &[u8],
    ) -> std::sync::Arc<rustls::ServerConfig> {
        let cert = rustls::pki_types::CertificateDer::from(cert_der.to_vec());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(key_der.to_vec()),
        );
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("test TLS server cert");
        std::sync::Arc::new(config)
    }

    /// One canned response for the dual-protocol test server.
    /// Same paths as the plain test server, plus /upgrade
    /// (plain → HTTPS, same host:port) and /downgrade (HTTPS →
    /// plain, must be refused by the client).
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn dual_response(path: &str, port: u16) -> (&'static str, Vec<String>, Vec<u8>) {
        match path {
            "/hello" => {
                let b = "مرحبا من الخادم".as_bytes().to_vec();
                ("200 OK", vec![format!("Content-Length: {}", b.len())], b)
            }
            "/chunked" => (
                "200 OK",
                vec!["Transfer-Encoding: chunked".to_string()],
                b"5\r\nchunk\r\n5\r\ned-ok\r\n0\r\n\r\n".to_vec(),
            ),
            "/redirect" => (
                "302 Found",
                vec!["Location: /hello".to_string(), "Content-Length: 0".to_string()],
                Vec::new(),
            ),
            "/upgrade" => (
                "302 Found",
                vec![
                    format!("Location: https://127.0.0.1:{}/hello", port),
                    "Content-Length: 0".to_string(),
                ],
                Vec::new(),
            ),
            "/downgrade" => (
                "302 Found",
                vec![
                    format!("Location: http://127.0.0.1:{}/hello", port),
                    "Content-Length: 0".to_string(),
                ],
                Vec::new(),
            ),
            "/slow" => {
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
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn dual_handle_conn<S: std::io::Read + std::io::Write>(stream: &mut S, port: u16) {
        use std::io::{Read, Write};
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
        let (status, mut headers, body) = dual_response(&path, port);
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

    /// A test server that speaks both plain HTTP and TLS on
    /// ONE port: it peeks the first byte (0x16 = TLS
    /// ClientHello). Same port for both, so an HTTP→HTTPS
    /// upgrade redirect can target the same host:port.
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn dual_test_server(cert_der: Vec<u8>, key_der: Vec<u8>) -> u16 {
        use std::io::Read;
        use std::net::TcpListener;
        let config = tls_server_config(&cert_der, &key_der);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut first = [0u8; 1];
                let is_tls = matches!(stream.peek(&mut first), Ok(1) if first[0] == 0x16);
                if is_tls {
                    let conn = match rustls::ServerConnection::new(std::sync::Arc::clone(
                        &config,
                    )) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let mut tls = rustls::StreamOwned::new(conn, stream);
                    dual_handle_conn(&mut tls, port);
                } else {
                    dual_handle_conn(&mut stream, port);
                }
            }
        });
        port
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_ca_server() -> u16 {
        dual_test_server(
            tls_fixture("tls-server-cert.der"),
            tls_fixture("tls-server-key.der"),
        )
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_selfsigned_server() -> u16 {
        dual_test_server(
            tls_fixture("tls-selfsigned-cert.der"),
            tls_fixture("tls-selfsigned-key.der"),
        )
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_valid_cert_accepted() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let port = tls_ca_server();
        let body = fetch_ok(&format!("https://127.0.0.1:{}/hello", port));
        assert_eq!(body, "مرحبا من الخادم");
        clear_test_tls_roots();
        set_network_allowed(false);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_test_ca_not_trusted_without_injection() {
        // The test CA is not in Mozilla roots: without the
        // injected test root the same valid server cert must
        // be rejected — the injection is what makes it trusted.
        let _guard = NET_TEST_LOCK.lock().unwrap();
        clear_test_tls_roots();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = tls_ca_server();
        let err = fetch(&format!("https://127.0.0.1:{}/hello", port), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("شهادة الخادم غير موثوقة"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_self_signed_rejected() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        clear_test_tls_roots();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = tls_selfsigned_server();
        let err = fetch(&format!("https://127.0.0.1:{}/hello", port), 1, 1).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("شهادة الخادم غير موثوقة"),
            "got: {}",
            msg
        );
        // No raw rustls text ever reaches the user.
        assert!(!msg.contains("rustls"), "got: {}", msg);
        assert!(!msg.contains("certificate"), "got: {}", msg);
        set_network_allowed(false);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_http_upgrade_followed() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let port = tls_ca_server();
        let body = fetch_ok(&format!("http://127.0.0.1:{}/upgrade", port));
        assert_eq!(body, "مرحبا من الخادم");
        clear_test_tls_roots();
        set_network_allowed(false);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_https_downgrade_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let port = tls_ca_server();
        let err = fetch(
            &format!("https://127.0.0.1:{}/downgrade", port),
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("إعادة التوجيه من HTTPS إلى HTTP مرفوضة"),
            "got: {}",
            err
        );
        clear_test_tls_roots();
        set_network_allowed(false);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_timeout() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let port = tls_ca_server();
        // A 1 second timeout against the /slow endpoint (5s),
        // through TLS: the total deadline applies to the TLS
        // stream exactly as to the plain one.
        let err = fetch_native_with_timeout(
            &format!("https://127.0.0.1:{}/slow", port),
            1,
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("مهلة"), "got: {}", err);
        clear_test_tls_roots();
        set_network_allowed(false);
    }

    // --- HTTPS server tests (Meeting 27) ---
    //
    // The server loads cert/key from FILES (PEM, inside
    // FilePolicy), so the tests write the PEM fixtures to a
    // temp dir first. The handler is a stub HandlerCaller
    // (no engine): it answers by request path, so the
    // server plumbing (bind / TLS / parse / response) is
    // tested without a Niva program; the engine-level
    // handler (a real Niva function, both modes) is tested
    // in tests/integration.rs and the parity gate.

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    struct StubCaller {
        allowed: bool,
        dir: std::path::PathBuf,
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    impl crate::stdlib::HandlerCaller for StubCaller {
        fn call_handler(
            &mut self,
            _func: &Value,
            arg: Value,
            line: usize,
            col: usize,
        ) -> (RioResult<Value>, String) {
            let path = match &arg {
                Value::Dict(pairs) => match pairs.iter().find(|(k, _)| k == "مسار") {
                    Some((_, Value::Str(p))) => p.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            match path.as_str() {
                "/dict" => (
                    Ok(Value::Dict(vec![
                        ("حالة".to_string(), Value::Number(201.0)),
                        ("نص".to_string(), Value::Str("تم".to_string())),
                        (
                            "ترويسات".to_string(),
                            Value::Dict(vec![(
                                "X-Custom".to_string(),
                                Value::Str("نعم".to_string()),
                            )]),
                        ),
                    ])),
                    String::new(),
                ),
                "/err" => (
                    Err(RioError::runtime("قسمة على صفر", line, col)),
                    String::new(),
                ),
                "/print" => (
                    Ok(Value::Str("طبع".to_string())),
                    "من المعالج\n".to_string(),
                ),
                _ => (Ok(Value::Str(format!("مسار:{}", path))), String::new()),
            }
        }

        fn server_allowed(&self) -> bool {
            self.allowed
        }

        fn server_file_policy(&self) -> crate::permissions::FilePolicy {
            crate::permissions::FilePolicy::restricted(self.dir.clone())
        }

        fn server_cwd(&self) -> std::path::PathBuf {
            self.dir.clone()
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_pem(name: &str) -> &'static str {
        match name {
            "server-cert.pem" => include_str!("../tests/fixtures/server-cert.pem"),
            "server-key.pem" => include_str!("../tests/fixtures/server-key.pem"),
            "server-selfsigned-cert.pem" => {
                include_str!("../tests/fixtures/server-selfsigned-cert.pem")
            }
            "server-selfsigned-key.pem" => {
                include_str!("../tests/fixtures/server-selfsigned-key.pem")
            }
            _ => panic!("unknown server fixture: {}", name),
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-server-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "server-cert.pem",
            "server-key.pem",
            "server-selfsigned-cert.pem",
            "server-selfsigned-key.pem",
        ] {
            std::fs::write(dir.join(f), server_pem(f)).unwrap();
        }
        dir
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn reset_server_test_state() {
        SERVER_STOP.store(false, std::sync::atomic::Ordering::SeqCst);
        SERVER_TIMEOUT_OVERRIDE_SECS.store(0, std::sync::atomic::Ordering::SeqCst);
        LAST_SERVER_PORT.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn start_test_server(
        allowed: bool,
        dir: &std::path::Path,
        cert: &str,
        key: &str,
    ) -> (
        std::thread::JoinHandle<()>,
        std::sync::mpsc::Receiver<String>,
        u16,
    ) {
        reset_server_test_state();
        let caller = StubCaller {
            allowed,
            dir: dir.to_path_buf(),
        };
        let cert = cert.to_string();
        let key = key.to_string();
        // The serve result (RioResult<Value>) is not Send —
        // Value holds Rc — so the thread reports it as text
        // over a channel instead of returning it.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let handle = std::thread::spawn(move || {
            let mut caller = caller;
            let res = serve(
                &mut caller,
                0,
                &cert,
                &key,
                Value::Nil,
                "127.0.0.1",
                1,
                1,
            );
            let _ = tx.send(match res {
                Ok(_) => "ok".to_string(),
                Err(e) => e.to_string(),
            });
        });
        let mut port = 0;
        for _ in 0..100 {
            port = last_server_port();
            if port != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_ne!(port, 0, "the test server did not bind");
        (handle, rx, port)
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn stop_test_server(
        handle: std::thread::JoinHandle<()>,
        rx: std::sync::mpsc::Receiver<String>,
    ) {
        request_server_stop();
        let _ = handle.join();
        let outcome = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(outcome.as_deref(), Ok("ok"), "server outcome: {:?}", outcome);
        reset_server_test_state();
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn tls_client_config_for_tests() -> std::sync::Arc<rustls::ClientConfig> {
        let mut roots = rustls::RootCertStore::empty();
        let ca = rustls::pki_types::CertificateDer::from(tls_fixture("tls-ca-cert.der"));
        roots.add(ca).unwrap();
        std::sync::Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn https_raw(port: u16, request: &[u8]) -> Vec<u8> {
        use std::io::{Read, Write};
        let config = tls_client_config_for_tests();
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
        let conn = rustls::ClientConnection::new(config, name).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        tls.write_all(request).unwrap();
        let mut raw = Vec::new();
        tls.read_to_end(&mut raw).unwrap();
        raw
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn parse_http_response(raw: &[u8]) -> (u16, Vec<(String, String)>, String) {
        let text = String::from_utf8_lossy(raw).into_owned();
        let (head, body) = match text.split_once("\r\n\r\n") {
            Some((h, b)) => (h, b.to_string()),
            None => (text.as_str(), String::new()),
        };
        let mut lines = head.lines();
        let status: u16 = lines
            .next()
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| {
                l.split_once(':')
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        (status, headers, body)
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn https_get(port: u16, path: &str) -> (u16, Vec<(String, String)>, String) {
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            path
        );
        parse_http_response(&https_raw(port, req.as_bytes()))
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_binds_and_serves_text() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("text");
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let (status, _headers, body) = https_get(port, "/hello");
        assert_eq!(status, 200);
        assert_eq!(body, "مسار:/hello");
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_dict_response() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("dict");
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let (status, headers, body) = https_get(port, "/dict");
        assert_eq!(status, 201);
        assert_eq!(body, "تم");
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "X-Custom" && v == "نعم"),
            "headers: {:?}",
            headers
        );
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_handler_error_is_generic_500() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("err");
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let (status, _headers, body) = https_get(port, "/err");
        assert_eq!(status, 500);
        assert_eq!(body, "خطأ في معالج الطلب");
        assert!(
            !body.contains("قسمة على صفر"),
            "the real error leaked: {}",
            body
        );
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_permission_refused_without_flag() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("noflag");
        reset_server_test_state();
        let mut caller = StubCaller {
            allowed: false,
            dir: dir.clone(),
        };
        let err = serve(
            &mut caller,
            0,
            "server-cert.pem",
            "server-key.pem",
            Value::Nil,
            "127.0.0.1",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains(SERVER_PERMISSION_MARKER),
            "got: {}",
            err
        );
        assert!(err.to_string().contains("--اسمح-بالخادم"), "got: {}", err);
        assert!(!err.to_string().contains("rustls"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_bad_host_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("badhost");
        reset_server_test_state();
        let mut caller = StubCaller {
            allowed: true,
            dir: dir.clone(),
        };
        let err = serve(
            &mut caller,
            0,
            "server-cert.pem",
            "server-key.pem",
            Value::Nil,
            "example.com",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("127.0.0.1"),
            "got: {}",
            err
        );
        assert!(!err.to_string().contains("rustls"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_missing_cert_file() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("nocert");
        reset_server_test_state();
        let mut caller = StubCaller {
            allowed: true,
            dir: dir.clone(),
        };
        let err = serve(
            &mut caller,
            0,
            "nope.pem",
            "server-key.pem",
            Value::Nil,
            "127.0.0.1",
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("nope.pem"), "got: {}", err);
        assert!(!err.to_string().contains("rustls"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_bad_key_pair_error_is_arabic() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("badkey");
        reset_server_test_state();
        let mut caller = StubCaller {
            allowed: true,
            dir: dir.clone(),
        };
        // The self-signed key does not match the CA-signed cert.
        let err = serve(
            &mut caller,
            0,
            "server-cert.pem",
            "server-selfsigned-key.pem",
            Value::Nil,
            "127.0.0.1",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("غير صالح"),
            "got: {}",
            err
        );
        assert!(!err.to_string().contains("rustls"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn cert_is_self_signed_detection() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let ca_signed =
            rustls::pki_types::CertificateDer::from(tls_fixture("tls-server-cert.der"));
        assert!(!cert_is_self_signed(&ca_signed));
        let self_signed = rustls::pki_types::CertificateDer::from(tls_fixture(
            "tls-selfsigned-cert.der",
        ));
        assert!(cert_is_self_signed(&self_signed));
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_self_signed_rejected_by_client() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("selfsigned");
        let (handle, rx, port) = start_test_server(
            true,
            &dir,
            "server-selfsigned-cert.pem",
            "server-selfsigned-key.pem",
        );
        // The client trusts the test CA only: it must reject
        // the self-signed server (the client rule stands —
        // no client trusts a self-signed server by default).
        use std::io::Write;
        let config = tls_client_config_for_tests();
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
        let conn = rustls::ClientConnection::new(config, name).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        let res = tls.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(res.is_err(), "the handshake should have failed");
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_request_too_big() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("toobig");
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let req = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_SERVER_REQUEST_BYTES + 1
        );
        let (status, _headers, body) = parse_http_response(&https_raw(port, req.as_bytes()));
        assert_eq!(status, 413);
        assert_eq!(body, "الطلب أكبر من الحد");
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_timeout_closes_idle_connection() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("timeout");
        reset_server_test_state();
        SERVER_TIMEOUT_OVERRIDE_SECS.store(1, std::sync::atomic::Ordering::SeqCst);
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        // Open a TLS connection and send nothing: the server
        // must close it when the (overridden 1s) deadline fires.
        use std::io::Read;
        let config = tls_client_config_for_tests();
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
        let conn = rustls::ClientConnection::new(config, name).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        let mut buf = [0u8; 1];
        let res = tls.read(&mut buf);
        assert!(
            matches!(res, Ok(0)) || res.is_err(),
            "the idle connection should have closed, got: {:?}",
            res.map(|n| n)
        );
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    fn server_connection_cap() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("cap");
        let (handle, rx, port) = start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        // Hold 64 plain TCP connections: each is counted at
        // accept time, before any TLS handshake.
        let mut held = Vec::new();
        for _ in 0..MAX_SERVER_CONNECTIONS {
            held.push(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        // The 65th connection must be closed immediately, with
        // no response.
        let extra = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        extra
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut buf = [0u8; 1];
        use std::io::Read;
        let res = (&extra).read(&mut buf);
        assert!(
            matches!(res, Ok(0)) || res.is_err(),
            "the 65th connection should have been refused, got: {:?}",
            res.map(|n| n)
        );
        drop(held);
        drop(extra);
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- HTTP/2 tests (Meeting 27 §42) ---
    //
    // The engine client offers ["h2", "http/1.1"] and the
    // Niva server answers h2, so fetch() against the test
    // server negotiates h2 end to end. The raw h2 client
    // below (h2 + tokio-rustls, ALPN ["h2"]) proves the
    // wire protocol: concurrent streams on one connection
    // (multiplexing) and the 413 on an oversize body. The
    // HTTP/1.1 fallback is covered by the existing tests:
    // tls_valid_cert_accepted fetches from the HTTP/1.1-only
    // raw test server (the peer offers no ALPN), and
    // parity_server fetches with a no-ALPN rustls client.

    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    fn h2_client_config_for_tests() -> std::sync::Arc<rustls::ClientConfig> {
        let mut roots = rustls::RootCertStore::empty();
        let ca = rustls::pki_types::CertificateDer::from(tls_fixture("tls-ca-cert.der"));
        roots.add(ca).unwrap();
        let mut config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec()];
        std::sync::Arc::new(config)
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    fn h2_engine_client_fetches_from_niva_server() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("h2-client");
        let (handle, rx, port) =
            start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let body = fetch_ok(&format!("https://127.0.0.1:{}/hello", port));
        assert_eq!(body, "مسار:/hello");
        let dict = fetch_ok(&format!("https://127.0.0.1:{}/dict", port));
        assert_eq!(dict, "تم");
        let err = fetch(&format!("https://127.0.0.1:{}/err", port), 1, 1).unwrap_err();
        assert!(
            err.to_string().contains("الخادم رد برمز الحالة 500"),
            "got: {}",
            err
        );
        clear_test_tls_roots();
        set_network_allowed(false);
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One raw h2 GET on a shared connection: returns
    /// (status, headers, body).
    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    async fn h2_get_one(
        send: &mut h2::client::SendRequest<bytes::Bytes>,
        host: &str,
        path: &str,
    ) -> (u16, Vec<(String, String)>, String) {
        let uri = format!("https://{}{}", host, path);
        let req = http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", host)
            .body(())
            .unwrap();
        let (resp_fut, _stream) = send.send_request(req, true).unwrap();
        let resp = resp_fut.await.unwrap();
        let (parts, mut body_stream) = resp.into_parts();
        let status = parts.status.as_u16();
        let headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        let mut body = Vec::new();
        while let Some(chunk) = body_stream.data().await {
            let chunk = chunk.unwrap();
            let _ = body_stream.flow_control().release_capacity(chunk.len());
            body.extend_from_slice(&chunk);
        }
        (
            status,
            headers,
            String::from_utf8_lossy(&body).into_owned(),
        )
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    fn h2_server_multiplexes_streams() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("h2-mux");
        let (handle, rx, port) =
            start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let host = format!("127.0.0.1:{}", port);
        let results = run_on_runtime(async move {
            // One TLS + h2 connection, 8 streams at once.
            let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let connector = tokio_rustls::TlsConnector::from(h2_client_config_for_tests());
            let name =
                rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
            let tls = connector.connect(name, stream).await.unwrap();
            assert_eq!(
                tls.get_ref().1.alpn_protocol(),
                Some(b"h2".as_slice()),
                "the Niva server must negotiate h2"
            );
            let (send, conn) = h2::client::handshake(tls).await.unwrap();
            tokio::spawn(async move {
                let _ = conn.await;
            });
            let mut tasks = Vec::new();
            for i in 0..8 {
                let mut send = send.clone();
                let host = host.clone();
                tasks.push(tokio::spawn(async move {
                    let mut send = send.ready().await.unwrap();
                    h2_get_one(&mut send, &host, &format!("/s{}", i)).await
                }));
            }
            let mut out = Vec::new();
            for t in tasks {
                out.push(t.await.unwrap());
            }
            out
        })
        .expect("h2 mux runtime");
        assert_eq!(results.len(), 8);
        for (i, (status, headers, body)) in results.iter().enumerate() {
            assert_eq!(*status, 200, "stream {}: {:?}", i, results);
            assert_eq!(body, &format!("مسار:/s{}", i), "stream {}", i);
            assert!(
                headers
                    .iter()
                    .any(|(k, v)| k == "content-type"
                        && v == "text/plain; charset=utf-8"),
                "stream {} headers: {:?}",
                i,
                headers
            );
        }
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    fn h2_server_rejects_oversize_body() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("h2-413");
        let (handle, rx, port) =
            start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        let host = format!("127.0.0.1:{}", port);
        let status = run_on_runtime(async move {
            let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let connector = tokio_rustls::TlsConnector::from(h2_client_config_for_tests());
            let name =
                rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
            let tls = connector.connect(name, stream).await.unwrap();
            let (mut send, conn) = h2::client::handshake(tls).await.unwrap();
            tokio::spawn(async move {
                let _ = conn.await;
            });
            send = send.ready().await.unwrap();
            let req = http::Request::builder()
                .method("POST")
                .uri(format!("https://{}/hello", host))
                .header("host", host)
                .body(())
                .unwrap();
            let (resp_fut, mut send_stream) = send.send_request(req, false).unwrap();
            // Just over the 5 MB cap, in 16 KB chunks — the
            // h2 initial window is 65,535 bytes, so a chunk
            // must fit it (a 64 KB chunk would deadlock: the
            // client waits for capacity the server cannot
            // grant before reading anything). The server
            // stops releasing capacity at the cap and answers
            // 413; sending past that fails, which is expected
            // — the response is what matters.
            let chunk = bytes::Bytes::from(vec![b'x'; 16 * 1024]);
            let mut sent = 0usize;
            let target = MAX_SERVER_REQUEST_BYTES + 1;
            while sent < target {
                let n = (target - sent).min(chunk.len());
                send_stream.reserve_capacity(n);
                loop {
                    if send_stream.capacity() >= n {
                        break;
                    }
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        std::future::poll_fn(|cx| send_stream.poll_capacity(cx)),
                    )
                    .await
                    {
                        Ok(Some(Ok(_))) => continue,
                        _ => break,
                    }
                }
                if send_stream.capacity() < n {
                    break;
                }
                let last = sent + n >= target;
                if send_stream.send_data(chunk.slice(..n), last).is_err() {
                    break;
                }
                sent += n;
            }
            let resp = resp_fut.await.unwrap();
            resp.status().as_u16()
        })
        .expect("h2 413 runtime");
        assert_eq!(status, 413, "oversize h2 body must be 413");
        stop_test_server(handle, rx);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
