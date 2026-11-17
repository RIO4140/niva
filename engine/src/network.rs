//! Network for Niva v1 (Meeting 22, DESIGN §16; server:
//! Meeting 27, DESIGN §42; POST: Meeting 29, DESIGN
//! §44): three stdlib functions, `شبكة.اجلب(url)` —
//! HTTP GET only, returns the response body as text —,
//! `شبكة.أرسل(رابط، جسم[، نوع_المحتوى])` — HTTP POST
//! only, same gates and limits as اجلب, returns the
//! response body as text — and `شبكة.اخدم منفذ شهادة
//! مفتاح معالج [مضيف]` — a ready HTTPS server with one
//! Niva handler.
//!
//! Permission: network is OFF by default. The gate lives here
//! (`set_network_allowed` / `network_allowed`); the interpreter
//! and the CLI set it, and `اجلب` refuses with a clear Arabic
//! error while it is off. The permission error message starts
//! with `PERMISSION_ERROR_MARKER`, which the interactive
//! consoles use to offer the first-use permission prompt.
//!
//! Platform split:
//! - Native (desktop / terminal / Android app): hand-rolled
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
//! h2c). No new Niva API: `اجلب` and `أرسل` and `اخدم`
//! negotiate h2 vs HTTP/1.1 transparently — the client offers
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
//!   After a POST, 301/302/303 re-issue as GET first and
//!   then follow the same redirect rules (including the
//!   §43 one-host allowlist), while 307/308 repeat the POST
//!   with the same body to the same host only — any other
//!   host is refused (Meeting 29, DESIGN §44).
//!   The IP check runs before the TLS handshake, so a
//!   certificate is never even offered to a refused address.
//!
//! Resource limits (§9): 30 second timeout, 5 MB maximum
//! response body, 5 MB maximum POST request body. Errors are
//! Arabic, in the engine's existing style (kind + message, no
//! stack traces to the user).

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
/// 5 MB, the adopted maximum POST request body (Meeting 29,
/// DESIGN §44 — the same cap as the response body).
pub const MAX_REQUEST_BYTES: usize = 5 * 1024 * 1024;
/// At most 5 same-host redirects (§7).
pub const MAX_REDIRECTS: usize = 5;

// ----------------------------------------------------------------
// إلغاء الجلب (المهمة 22 — شبكة.نزّل + إلغاء أمر معلق)
// ----------------------------------------------------------------

/// علم إلغاء الجلب: تضبطه حلقة انتظار الجلب وحدها
/// حين ترى سطر الإلغاء من التطبيق
/// (`\u{0001}نيفا-إلغاء` في stdin_pump)؛ ويُمسح في
/// بداية كل جلب/تنزيل؛ ويفحصه عامل الجلب بين
/// القراءات. بلا قفل — ذري واحد، والقراءة/الكتابة
/// بترتيب SeqCst.
#[cfg(not(target_arch = "wasm32"))]
static CANCEL_FETCH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn request_fetch_cancel() {
    CANCEL_FETCH.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(not(target_arch = "wasm32"))]
fn cancel_requested() -> bool {
    CANCEL_FETCH.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_fetch_cancel() {
    CANCEL_FETCH.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// خطأ الإلغاء العربي — يُرفع عند موقع الاستدعاء
/// (يُمسك بـ `حاول/ماعدا`).
#[cfg(not(target_arch = "wasm32"))]
fn cancel_error(line: usize, col: usize) -> RioError {
    RioError::runtime("أُلغي الأمر", line, col)
}

#[cfg(not(target_arch = "wasm32"))]
fn check_cancelled(line: usize, col: usize) -> RioResult<()> {
    if cancel_requested() {
        return Err(cancel_error(line, col));
    }
    Ok(())
}

/// مهلة قراءة واحدة أثناء جلب قابل للإلغاء: المتبقي
/// حتى المهلة الإجمالية مسقوفا بـ 500ms — فقراءة
/// متوقفة تماما تعيد WouldBlock كل نصف ثانية،
/// فيُفحص علم الإلغاء وتُعاد المحاولة. والمهلة
/// الإجمالية 30 ثانية ما زالت هي الحد — فالتقطير
/// البطيء (بايت كل 400ms) يستمر كما كان.
#[cfg(not(target_arch = "wasm32"))]
fn read_timeout(
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<std::time::Duration> {
    let left = time_left(deadline, line, col)?;
    Ok(left.min(std::time::Duration::from_millis(500)))
}

/// عند WouldBlock أثناء قراءة: فحص الإلغاء أولا، ثم
/// المهلة الإجمالية (انتهت ← خطأ المهلة نفسه)، ثم
/// إعادة المحاولة. تُستدعى قبل `io_timeout_or` —
/// فهو يحوّل WouldBlock إلى خطأ مهلة مباشرة.
#[cfg(not(target_arch = "wasm32"))]
fn on_read_would_block(
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<()> {
    check_cancelled(line, col)?;
    let _ = time_left(deadline, line, col)?;
    Ok(())
}

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
    #[cfg(target_arch = "wasm32")]
    {
        fetch_wasm(url, line, col)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // المهمة 22: العمل على خيط عامل (القيم Rc لا
        // تعبر الخيوط — فالعامل يرجع String والتحويل
        // إلى Value هنا على الخيط المنفذ)، والانتظار
        // يقرأ stdin أثناءه (سطر الإلغاء من التطبيق).
        let url = url.to_string();
        let text = run_cancellable(
            move || fetch_text_native(&url, TIMEOUT_SECS, line, col),
            line,
            col,
        )?;
        Ok(Value::Str(text))
    }
}

/// ترويسات المتصل (اجتماع 30، DESIGN §45): اسم
/// وقيمة نصا فقط. والاسم حرف رمزي (token) غير فارغ
/// — فاسم فيه مسافة أو نقطتان يكسر إطار الطلب على
/// السلك؛ والقيمة بلا CR/LF أبدا (حقن الترويسات).
/// تُفحص هنا قبل أي شبكة، قبل التوزيع على
/// المسارات — فنقطة واحدة تخدم HTTP/1.1 وHTTP/2
/// وWASM معا.
fn validate_caller_headers(
    headers: &[(String, String)],
    line: usize,
    col: usize,
) -> RioResult<()> {
    for (name, value) in headers {
        if name.is_empty() {
            return Err(RioError::runtime(
                "اسم ترويسة فارغ — الترويسة تحتاج اسما غير فارغ",
                line,
                col,
            ));
        }
        if !name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
                        | b'^' | b'_' | b'`' | b'|' | b'~'
                )
        }) {
            // بلا إظهار الاسم نفسه: قد يحمل أسطرا
            // جديدة فتلوث نص الخطأ.
            return Err(RioError::runtime(
                "اسم ترويسة غير صالح — الاسم حروف رمزية فقط",
                line,
                col,
            ));
        }
        if value.contains('\r') || value.contains('\n') {
            return Err(RioError::runtime(
                "قيمة ترويسة غير صالحة — لا يُسمح بأسطر جديدة في الترويسات",
                line,
                col,
            ));
        }
    }
    Ok(())
}

/// الإرسال (اجتماع 29، DESIGN §44؛ واجتماع 30،
/// §45): `شبكة.أرسل(رابط، جسم[، نوع_المحتوى[،
/// ترويسات]])` — HTTP POST، يرجع نص جسم الرد
/// كما في `اجلب`. النوع الافتراضي `application/json`؛
/// والوسيط الثالث يبدّله (لتدفق الجهاز:
/// `application/x-www-form-urlencoded`)؛ والوسيط
/// الرابع الاختياري قاموس ترويسات المتصل {اسم:
/// قيمة} نصا فقط (للمكالمات المصدّقة: Authorization
/// وAccept). والبوابات والحدود حرفيا كما في `اجلب`:
/// بوابة الشبكة نفسها بلا إذن جديد؛ ومهلة 30 ثانية
/// إجمالية؛ وسقف الرد 5 ميغابايت؛ وسقف جسم الطلب 5
/// ميغابايت (يُفحص قبل أي شبكة)؛ والتحقق الكامل من
/// سلسلة TLS؛ وفحص IP قبل المصافحة. والمحرك لا يضيف
/// أي ترويسة اعتماد من نفسه. وغير-2xx خطأ عربي
/// يسمّي الحالة كما في `اجلب`. WASM: جلب المتصفح
/// نفسه بطريقة POST.
pub fn post(
    url: &str,
    body: &str,
    content_type: &str,
    headers: &[(String, String)],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if !network_allowed() {
        return Err(RioError::runtime(
            "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول",
            line,
            col,
        ));
    }
    if body.len() > MAX_REQUEST_BYTES {
        return Err(RioError::runtime(
            "جسم الطلب أكبر من الحد الأقصى (5 ميغابايت)",
            line,
            col,
        ));
    }
    // اجتماع 30: ترويسات المتصل تُفحص قبل أي شبكة.
    validate_caller_headers(headers, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        post_wasm(url, body, content_type, headers, line, col)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // المهمة 22: كما في اجلب — العمل على خيط عامل
        // والانتظار يقرأ stdin أثناءه (سطر الإلغاء).
        let url = url.to_string();
        let body = body.to_string();
        let content_type = content_type.to_string();
        let headers = headers.to_vec();
        let text = run_cancellable(
            move || {
                post_text_native(
                    &url,
                    &body,
                    &content_type,
                    &headers,
                    TIMEOUT_SECS,
                    line,
                    col,
                )
            },
            line,
            col,
        )?;
        Ok(Value::Str(text))
    }
}

/// تنزيل ملف إلى القرص (المهمة 22): `شبكة.نزّل رابط
/// مسار` — يرجع عدد البايتات المنزّلة. تحت بوابة
/// الشبكة (كما اجلب حرفيا) وFilePolicy (كما
/// اكتب_ملف حرفيا — والمسار يُحلّ هنا قبل أي شبكة،
/// فخطأ خارج المجلد يرفض قبل أن يتحرك بايت واحد).
/// والجسم يتدفق بايتات خاما إلى الملف (بلا سقف
/// 5MB — سقفه حصة 100MB)؛ والملف الجزئي يُحذف عند
/// الإلغاء أو الخطأ. WASM: رسالة المتصفح المتبعة.
pub fn download(
    url: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if !network_allowed() {
        return Err(RioError::runtime(
            "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول",
            line,
            col,
        ));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (url, policy, cwd, path);
        Err(RioError::runtime(
            "هذه الدالة غير مدعومة في متصفح الويب",
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Meeting 50 §64: the download destination is a
        // write — refused under the downloads root.
        let resolved = policy.resolve_in_for_write(cwd, path, line, col)?;
        let display = policy.display_path(&resolved);
        let base = policy.base.clone();
        let url = url.to_string();
        let written = run_cancellable(
            move || {
                download_native_with_timeout(
                    &url, &resolved, &display, &base, TIMEOUT_SECS, line, col,
                )
            },
            line,
            col,
        )?;
        Ok(Value::Number(written as f64))
    }
}

// ----------------------------------------------------------------
// انتظار قابل للإلغاء (المهمة 22)
// ----------------------------------------------------------------

/// شغّل عمل جلب على خيط عامل وانتظر نتيجته مع قراءة
/// stdin أثناء الانتظار: سطر الإلغاء يضبط العلم
/// فيُجهض العامل (ثم «أُلغي الأمر»)؛ والأسطر العادية
/// تذهب إلى الطابور المعلق (للموجّه بعد الجلب)؛
/// ونهاية الإدخال تعني انتظار العامل مباشرة. والعمل
/// نفسه يرجع نوعا يعبر الخيوط (`String`/`u64` —
/// فالقيم Rc لا تعبرها، والتحويل إلى Value على
/// الخيط المنفذ).
#[cfg(not(target_arch = "wasm32"))]
fn run_cancellable<T: Send + 'static>(
    work: impl FnOnce() -> RioResult<T> + Send + 'static,
    line: usize,
    col: usize,
) -> RioResult<T> {
    use crate::stdin_pump::{
        is_cancel_line, next_channel_line_timeout, push_pending, RawLine,
    };
    clear_fetch_cancel();
    // إلغاء قديم في القناة (أُرسل قبل بدء الجلب)
    // يُتجاهل — فلا يلغي جلبا لم يبدأ بعد؛ وأسطر
    // عادية قديمة تذهب إلى الطابور المعلق.
    crate::stdin_pump::discard_stale_cancel_lines();
    let (tx, rx) = std::sync::mpsc::channel::<RioResult<T>>();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    loop {
        match next_channel_line_timeout(std::time::Duration::from_millis(100)) {
            RawLine::Line(l) => {
                if is_cancel_line(&l) {
                    request_fetch_cancel();
                    // انتظار العامل حتى يلاحظ العلم
                    // ويتوقف (≤500ms في المسار المتزامن،
                    // ≤100ms في h2) — ثم الخطأ العربي
                    // (والملف الجزئي حذفه العامل نفسه).
                    let _ = rx.recv();
                    return Err(cancel_error(line, col));
                } else {
                    push_pending(l);
                }
            }
            RawLine::Timeout => match rx.try_recv() {
                Ok(r) => return r,
                Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(RioError::runtime(
                        "تعذر تنفيذ طلب الشبكة",
                        line,
                        col,
                    ))
                }
            },
            RawLine::Eof => {
                // نهاية الإدخال الحقيقي — لكن القناة قد
                // تستقبل سطورا محقونة بعدها (اختبارا؛ وفي
                // الإنتاج لا سطور بعد EOF أصلا)، فلا
                // نحجب على العامل مباشرة: نستطلع العامل
                // ونواصل الحلقة (كما Timeout — فالاستطلاع
                // بشرائح 50ms لا يحجب طويلا).
                match rx.try_recv() {
                    Ok(r) => return r,
                    Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return Err(RioError::runtime(
                            "تعذر تنفيذ طلب الشبكة",
                            line,
                            col,
                        ))
                    }
                }
            }
        }
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

/// طريقة الطلب في نقطة التنفيذ الواحدة (اجتماع 29):
/// GET (اجلب/نزّل) أو POST (شبكة.أرسل).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FetchMethod {
    Get,
    Post,
}

/// ما يصدره `fetch_one_native` في محاولة واحدة: الطريقة
/// + جسم الطلب (فارغ في GET) + نوع المحتوى (فارغ في
/// GET) + ترويسات المتصل (فارغة في GET — بلا قناة
/// لها في `اجلب`). الحلقة تحمل نسخة قابلة للتعديل —
/// فبعد POST يتحول الطلب إلى GET عند 301/302/303،
/// ويبقى POST بالجسم نفسه عند 307/308.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct FetchRequest {
    method: FetchMethod,
    body: Vec<u8>,
    content_type: String,
    headers: Vec<(String, String)>,
    // لينكس8-طبقة1-نواة: طريقة مخصصة (PUT/PATCH/DELETE/HEAD/
    // OPTIONS/أي فعل) + خيارات التنفيذ الكاملة.
    custom_method: Option<String>,
    opts: ExecOpts,
}

#[cfg(not(target_arch = "wasm32"))]
impl FetchRequest {
    fn get() -> Self {
        FetchRequest {
            method: FetchMethod::Get,
            body: Vec::new(),
            content_type: String::new(),
            headers: Vec::new(),
            custom_method: None,
            opts: ExecOpts::default(),
        }
    }

    fn post(body: &[u8], content_type: &str, headers: &[(String, String)]) -> Self {
        FetchRequest {
            method: FetchMethod::Post,
            body: body.to_vec(),
            content_type: content_type.to_string(),
            headers: headers.to_vec(),
            custom_method: None,
            opts: ExecOpts::default(),
        }
    }

    // لينكس8-طبقة1-نواة: طلب بأي طريقة + خيارات.
    fn with_method(
        method: &str,
        body: &[u8],
        content_type: &str,
        headers: &[(String, String)],
        opts: ExecOpts,
    ) -> Self {
        let upper = method.to_ascii_uppercase();
        let (base, custom) = match upper.as_str() {
            "GET" => (FetchMethod::Get, None),
            "POST" => (FetchMethod::Post, None),
            _ => (FetchMethod::Post, Some(upper)),
        };
        FetchRequest {
            method: base,
            body: body.to_vec(),
            content_type: content_type.to_string(),
            headers: headers.to_vec(),
            custom_method: custom,
            opts,
        }
    }

    // لينكس8-طبقة1-نواة: الفعل كما يُرسل على السلك.
    fn method_str(&self) -> String {
        if let Some(m) = &self.custom_method {
            return m.clone();
        }
        if self.is_post() {
            "POST".to_string()
        } else {
            "GET".to_string()
        }
    }

    // لينكس8-طبقة1-نواة: هل تُرسل الطريقة جسما؟
    fn sends_body(&self) -> bool {
        if let Some(m) = &self.custom_method {
            return !self.body.is_empty()
                || matches!(m.as_str(), "POST" | "PUT" | "PATCH" | "DELETE");
        }
        self.is_post()
    }

    // لينكس8-طبقة1-نواة: هل تتحول إلى GET عند 301/302/303؟
    fn converts_to_get(&self) -> bool {
        if let Some(m) = &self.custom_method {
            return matches!(m.as_str(), "POST" | "PUT" | "PATCH" | "DELETE");
        }
        self.is_post()
    }

    // لينكس8-طبقة1-نواة: هل يحمل الطلب اعتمادا (ترويسة
    // تفويض أو كوكيز)؟ — لقاعدة التوجيه لمضيف موثوق فقط.
    fn has_auth(&self) -> bool {
        self.headers.iter().any(|(n, _)| {
            let l = n.to_ascii_lowercase();
            l == "authorization" || l == "cookie"
        })
    }

    /// بعد POST: 301/302/303 تُعاد GET (بلا جسم وبلا
    /// نوع محتوى) — كما في المتصفحات. وترويسات
    /// المتصل تبقى هنا؛ فإسقاطها عند التحويل لمضيف
    /// مختلف قرار الحلقة (§45)، لا قرار التحويل.
    fn to_get(&mut self) {
        self.method = FetchMethod::Get;
        self.custom_method = None;
        self.body.clear();
        self.content_type.clear();
    }

    /// اجتماع 30: عند التحويل تُسقط ترويسات المتصل
    /// لمضيف مختلف — فلا اعتماد يتسرب إلى مضيف آخر.
    fn drop_caller_headers(&mut self) {
        self.headers.clear();
    }

    fn is_post(&self) -> bool {
        self.method == FetchMethod::Post
    }
}

/// اجتماع 30: ترويسات المتصل تُعاد للمضيف المطابق
/// حرفيا فقط — المضيف بلا فرق حالة والمنفذ نفسه.
/// (المنفذ غير القياسي على المضيف نفسه مضيف مختلف،
/// كما في قاعدة الاجتماع 28 لمقارنة المضيفين.)
#[cfg(not(target_arch = "wasm32"))]
fn headers_survive_redirect(current: &ParsedUrl, next: &ParsedUrl) -> bool {
    next.host.to_ascii_lowercase() == current.host.to_ascii_lowercase()
        && next.port == current.port
}

/// النص الكامل للرد (اجلب): المصرف ذاكرة، والتحويل
/// إلى Value على المتصل (فالعامل يرجع String يعبر
/// الخيوط — والقيم Rc لا تعبرها).
#[cfg(not(target_arch = "wasm32"))]
fn fetch_text_native(
    url: &str,
    timeout_secs: u64,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let sink: Box<dyn BodySink + Send> = Box::new(MemorySink::new());
    let sink = fetch_loop(url, &FetchRequest::get(), timeout_secs, line, col, sink)?;
    Ok(sink.finish_string().unwrap_or_default())
}

/// النص الكامل للرد (شبكة.أرسل): كـ`fetch_text_native`
/// لكن بطلب POST — الجسم ونوع المحتوى وترويسات
/// المتصل تمر في الطلب عبر الحلقة (فيعيد التوجيه
/// إصدارها كما يقرر §44 و§45).
#[cfg(not(target_arch = "wasm32"))]
fn post_text_native(
    url: &str,
    body: &str,
    content_type: &str,
    headers: &[(String, String)],
    timeout_secs: u64,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let sink: Box<dyn BodySink + Send> = Box::new(MemorySink::new());
    let req = FetchRequest::post(body.as_bytes(), content_type, headers);
    let sink = fetch_loop(url, &req, timeout_secs, line, col, sink)?;
    Ok(sink.finish_string().unwrap_or_default())
}

#[cfg(test)]
fn fetch_native_with_timeout(
    url: &str,
    timeout_secs: u64,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    Ok(Value::Str(fetch_text_native(
        url,
        timeout_secs,
        line,
        col,
    )?))
}

/// حلقة الجلب فوق مصرف (إعادات التوجيه + الجسم):
/// اجلب يمرّر مصرف ذاكرة، ونزّل مصرف ملف. والجسم
/// نفسه لا يعبر هنا أبدا — المصرف يُرجع مع النتيجة
/// (أو يُسقَط عند الخطأ فيُغلق ملفه).
///
/// اجتماع 28 (§43): قائمة سماح التحويل — مضيف واحد
/// فقط. يُسمح بالتحويل إليه حين تتحقق كل الشروط
/// معا: المخطط https، والمضيف مطابق حرفيا (بلا نطاق
/// فرعي ولا لاحقة)، والمنفذ 443 (الافتراضي لـ https —
/// فمنفذ غير قياسي على المضيف نفسه يُعامَل مضيفا
/// مختلفا)، والطريقة GET. ولا ترويسة اعتماد أو كوكيز
/// تُرسل أصلا (الترويسات: Host وUser-Agent وAccept
/// فقط — وفي POST يضاف Content-Type وContent-Length)
/// فلا شيء يُمرَّر إلى المضيف المحوَّل إليه. وبعد
/// السماح تسري كل الحدود القائمة على المحاولة الجديدة
/// (المهلة الإجمالية، وسقف الجسم، والتحقق الكامل
/// من سلسلة TLS، وفحص IP قبل المصافحة) لأن الحلقة
/// تستدعي `fetch_one_native` نفسه من جديد. وأي مضيف
/// إضافي يحتاج اجتماعا جديدا — القائمة مضيف واحد.
///
/// اجتماع 29 (§44): التحويلات بعد POST — 301/302/303
/// (وأي 3xx آخر بحامل عنوان) تُعاد GET أولا ثم تسري
/// قواعد التحويل نفسها (الاجتماعان 26 و28 حرفيا،
/// ومنها قائمة السماح)؛ و307/308 يعيد POST بالجسم
/// نفسه للمضيف نفسه فقط — وأي مضيف مختلف يرفض
/// (قائمة السماح لا تسري على 307/308).
#[cfg(not(target_arch = "wasm32"))]
const REDIRECT_ALLOWLIST_HOST: &str = "release-assets.githubusercontent.com";

#[cfg(not(target_arch = "wasm32"))]
fn redirect_allowlisted(next: &ParsedUrl) -> bool {
    next.scheme == "https"
        && next.host.to_ascii_lowercase() == REDIRECT_ALLOWLIST_HOST
        && next.port == 443
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_loop(
    url: &str,
    req: &FetchRequest,
    timeout_secs: u64,
    line: usize,
    col: usize,
    mut sink: Box<dyn BodySink + Send>,
) -> RioResult<Box<dyn BodySink + Send>> {
    let mut current = parse_url(url, line, col)?;
    // نسخة قابلة للتعديل: بعد POST قد يتحول الطلب
    // إلى GET (301/302/303) أو يبقى POST (307/308).
    let mut req = req.clone();
    // لينكس8-طبقة1-نواة: قياس التنزيل (مهلة نقل/حد سرعة/
    // حد حجم) — بلا حدود يعبر المصرف كما هو.
    let mut sink = Box::new(MeteredSink::new(
        sink,
        req.opts.stall_secs,
        req.opts.max_speed_down,
        req.opts.max_size,
        line,
        col,
    )) as Box<dyn BodySink + Send>;
    let mut redirects = 0usize;
    // Audit م-15: one total deadline for the whole fetch
    // (connect + headers + body + redirects) — a slow server
    // sending a byte every 29 seconds must not hang the
    // session forever.
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        check_cancelled(line, col)?;
        let (outcome, s) = fetch_one_native(&current, &req, deadline, line, col, sink)?;
        sink = s;
        match outcome {
            FetchOutcome::Body => return Ok(sink),
            FetchOutcome::Redirect(status, location) => {
                // لينكس8-طبقة1-نواة: بلا اتباع يُرجع الرد 3xx نفسه.
                if !req.opts.follow_redirects {
                    return Ok(sink);
                }
                let max_red = req.opts.max_redirects.min(MAX_REDIRECTS);
                if redirects >= max_red {
                    return Err(RioError::runtime(
                        format!("عدد إعادات التوجيه تجاوز الحد ({})", max_red),
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
                let cross_host = next.host.to_ascii_lowercase()
                    != current.host.to_ascii_lowercase()
                    || next.port != current.port;
                // لينكس8-طبقة1-نواة: بروتوكول التوجيه المقيد.
                if !req
                    .opts
                    .redirect_protocols
                    .iter()
                    .any(|p| p == &next.scheme)
                {
                    return Err(RioError::runtime(
                        format!(
                            "إعادة التوجيه إلى بروتوكول {} مرفوضة",
                            next.scheme
                        ),
                        line,
                        col,
                    ));
                }
                // لينكس8-طبقة1-نواة: لمضيف موثوق فقط — مع
                // اعتماد يُرفض التحويل لمضيف مختلف أصلا.
                if req.opts.trusted_only_redirect && cross_host && req.has_auth() {
                    return Err(RioError::runtime(
                        "إعادة التوجيه إلى مضيف غير موثوق مرفوضة",
                        line,
                        col,
                    ));
                }
                // اجتماع 29: بعد طريقة بجسم — 307/308 يعيد
                // الطريقة بالجسم نفسه للمضيف نفسه فقط؛ وكل
                // 3xx آخر (ومنها 301/302/303) يُعاد GET أولا
                // (إلا مع إبقاء الجسم) ثم تسري قواعد التحويل.
                if req.converts_to_get() && (status == 307 || status == 308) {
                    if cross_host {
                        return Err(RioError::runtime(
                            format!(
                                "إعادة التوجيه إلى مضيف مختلف ({}) مرفوضة",
                                next.host
                            ),
                            line,
                            col,
                        ));
                    }
                } else {
                    if req.converts_to_get() && !req.opts.keep_body_on_redirect {
                        req.to_get();
                    }
                    if cross_host && !redirect_allowlisted(&next) {
                        return Err(RioError::runtime(
                            format!(
                                "إعادة التوجيه إلى مضيف مختلف ({}) مرفوضة",
                                next.host
                            ),
                            line,
                            col,
                        ));
                    }
                }
                // اجتماع 30: عند التحويل تُعاد ترويسات
                // المتصل للمضيف المطابق حرفيا فقط
                // وتُسقط لغيره (307/308 لمضيف مختلف
                // مرفوض أصلا أعلاه، فلا يصل هنا بمضيف
                // مختلف إلا تحويل مسموح به — كالمسموح
                // في قائمة السماح — فيُسقط الاعتماد
                // عنه أيضا).
                if !headers_survive_redirect(&current, &next) {
                    req.drop_caller_headers();
                }
                // جسم 3xx — إن وُجد — لا يلوث الجسم
                // النهائي: المصرف يُعاد إلى الصفر.
                sink.reset(line, col)?;
                current = next;
            }
        }
    }
}

/// التنزيل إلى القرص (المهمة 22): يفتح الملف
/// (يستبدله) تحت قفل الكتابة المشترك — ففحص الحصة
/// والكتابة ذريّان (نمط `ملفات.اكتب` في المهمة 19)
/// — ويتدفق الجسم إليه؛ وعند أي خطأ (ومنها
/// الإلغاء) يُحذف الملف الجزئي (فالمصرف يُسقَط
/// عند الخطأ فيُغلق المقبض قبل الحذف). يرجع عدد
/// البايتات المنزّلة.
#[cfg(not(target_arch = "wasm32"))]
fn download_native_with_timeout(
    url: &str,
    resolved: &std::path::Path,
    display: &str,
    base: &std::path::Path,
    timeout_secs: u64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    // القفل لكل التدفق: كاتب متزامن آخر لا ينزلق
    // بين فحص الحصة والكتابة فيتجاوزها.
    let _write_guard = crate::files::FILE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // الحجم الحالي (قبل القطع) — فالنمو الصافي هو
    // ما يُحسب من الحصة (كما `ملفات.اكتب`).
    let existing = std::fs::metadata(resolved).map(|m| m.len()).unwrap_or(0);
    let mut budget = 10000usize;
    let used = crate::interpreter::Interpreter::dir_size(base, &mut budget);
    let limit = crate::interpreter::Interpreter::DISK_QUOTA_BYTES
        .saturating_sub(used)
        .saturating_add(existing);
    let file = std::fs::File::create(resolved).map_err(|_| {
        RioError::new(
            crate::error::ErrorKind::Io,
            format!("تعذر كتابة الملف {}", display),
            line,
            col,
        )
    })?;
    let sink: Box<dyn BodySink + Send> = Box::new(FileSink {
        file,
        written: 0,
        limit,
        display: display.to_string(),
        meta: None,
    });
    match fetch_loop(url, &FetchRequest::get(), timeout_secs, line, col, sink) {
        Ok(sink) => Ok(sink.finish_bytes()),
        Err(e) => {
            // الجزئي يُحذف (المقبض أُغلق بسقوط
            // المصرف عند الخطأ) — فلا ملف نصف مكتمل
            // باسم الهدف يبدو مكتملا.
            let _ = std::fs::remove_file(resolved);
            Err(e)
        }
    }
}

// ----------------------------------------------------------------
// مصرف جسم الرد (المهمة 22)
// ----------------------------------------------------------------

/// مصرف جسم الرد: اجلب يجمع في الذاكرة (سقف 5MB)،
/// ونزّل يكتب ملفا (سقف الحصة). القارئ واحد
/// (`read_response`) فيدفع القطع إلى المصرف — فلا
/// قارئين يتباعدان. `Send` لأن المصرف يعبر خيط
/// العامل (وخيط وقت تشغيل tokio في مسار h2).
#[cfg(not(target_arch = "wasm32"))]
trait BodySink: Send {
    /// ادفع قطعة (يفحص السقف ويرفع خطأه العربي).
    fn push(&mut self, chunk: &[u8], line: usize, col: usize) -> RioResult<()>;
    /// إعادة التهيئة عند إعادة توجيه.
    fn reset(&mut self, line: usize, col: usize) -> RioResult<()>;
    /// النص الكامل (مصرف الذاكرة فقط).
    fn finish_string(self: Box<Self>) -> Option<String>;
    /// البايتات المكتوبة (كل مصرف).
    fn finish_bytes(&self) -> u64;
    // لينكس8-طبقة1-نواة: بيانات الرد الأخيرة (رمز + رؤوس).
    fn set_meta(&mut self, _status: u16, _headers: Vec<(String, String)>) {}
    fn meta(&self) -> Option<(u16, Vec<(String, String)>)> {
        None
    }
    /// البايتات الخام (الذاكرة فقط؛ الملف يرجع فارغا).
    fn finish_raw(self: Box<Self>) -> Vec<u8> {
        Vec::new()
    }
}

/// مصرف الذاكرة (اجلب): السقف نفسه 5MB + 1 بايت
/// (البايت الزائد يثبت التجاوز بدلا من القطع الصامت).
#[cfg(not(target_arch = "wasm32"))]
struct MemorySink {
    buf: Vec<u8>,
    // لينكس8-طبقة1-نواة: بيانات الرد الأخيرة.
    meta: Option<(u16, Vec<(String, String)>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MemorySink {
    fn new() -> Self {
        MemorySink {
            buf: Vec::new(),
            meta: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BodySink for MemorySink {
    fn push(&mut self, chunk: &[u8], line: usize, col: usize) -> RioResult<()> {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > MAX_RESPONSE_BYTES + 1 {
            return Err(RioError::runtime(
                "الرد أكبر من الحد الأقصى (5 ميغابايت)",
                line,
                col,
            ));
        }
        Ok(())
    }

    fn reset(&mut self, _line: usize, _col: usize) -> RioResult<()> {
        self.buf.clear();
        Ok(())
    }

    fn finish_string(self: Box<Self>) -> Option<String> {
        Some(String::from_utf8_lossy(&self.buf).into_owned())
    }

    fn finish_bytes(&self) -> u64 {
        self.buf.len() as u64
    }

    fn finish_raw(self: Box<Self>) -> Vec<u8> {
        self.buf
    }

    // لينكس8-طبقة1-نواة: حفظ بيانات الرد وإرجاعها.
    fn set_meta(&mut self, status: u16, headers: Vec<(String, String)>) {
        self.meta = Some((status, headers));
    }

    fn meta(&self) -> Option<(u16, Vec<(String, String)>)> {
        self.meta.clone()
    }
}

/// مصرف الملف (نزّل): يكتب القطع خاما (بايتات كما
/// وصلت — لا تحويل UTF-8)، ويتتبع البايتات ضد الحد
/// المتبقي من الحصة (يُحسب مرة واحدة قبل التدفق —
/// فلا مسح مجلد لكل قطعة)؛ والخطأ رسالة الحصة نفسها.
#[cfg(not(target_arch = "wasm32"))]
struct FileSink {
    file: std::fs::File,
    written: u64,
    limit: u64,
    display: String,
    // لينكس8-طبقة1-نواة: بيانات الرد الأخيرة.
    meta: Option<(u16, Vec<(String, String)>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl BodySink for FileSink {
    fn push(&mut self, chunk: &[u8], line: usize, col: usize) -> RioResult<()> {
        use std::io::Write;
        if self.written.saturating_add(chunk.len() as u64) > self.limit {
            return Err(RioError::new(
                crate::error::ErrorKind::Io,
                "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة",
                line,
                col,
            ));
        }
        self.file.write_all(chunk).map_err(|_| {
            RioError::new(
                crate::error::ErrorKind::Io,
                format!("تعذر كتابة الملف {}", self.display),
                line,
                col,
            )
        })?;
        self.written += chunk.len() as u64;
        Ok(())
    }

    fn reset(&mut self, line: usize, col: usize) -> RioResult<()> {
        use std::io::Seek;
        self.file.seek(std::io::SeekFrom::Start(0)).map_err(|_| {
            RioError::new(
                crate::error::ErrorKind::Io,
                format!("تعذر كتابة الملف {}", self.display),
                line,
                col,
            )
        })?;
        self.file.set_len(0).map_err(|_| {
            RioError::new(
                crate::error::ErrorKind::Io,
                format!("تعذر كتابة الملف {}", self.display),
                line,
                col,
            )
        })?;
        self.written = 0;
        Ok(())
    }

    fn finish_string(self: Box<Self>) -> Option<String> {
        None
    }

    fn finish_bytes(&self) -> u64 {
        self.written
    }

    // لينكس8-طبقة1-نواة: حفظ بيانات الرد وإرجاعها.
    fn set_meta(&mut self, status: u16, headers: Vec<(String, String)>) {
        self.meta = Some((status, headers));
    }

    fn meta(&self) -> Option<(u16, Vec<(String, String)>)> {
        self.meta.clone()
    }
}

#[cfg(not(target_arch = "wasm32"))]
enum FetchOutcome {
    /// الجسم كاملا في المصرف (النص عند اجلب، والملف
    /// عند نزّل — لا يعبر هنا أبدا).
    Body,
    /// إعادة توجيه: رمز الحالة + العنوان. الرمز لازم
    /// للحلقة (اجتماع 29): 301/302/303 تُعاد GET،
    /// و307/308 يعيد POST للمضيف نفسه فقط.
    Redirect(u16, String),
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
    req: &FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
    mut sink: Box<dyn BodySink + Send>,
) -> RioResult<(FetchOutcome, Box<dyn BodySink + Send>)> {
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

    // لينكس8-وكيل: اتصال مباشر أو عبر وكيل (HTTP/SOCKS5).
    let (mut stream, absolute) = connect_target(url, req, deadline, line, col)?;
    let host_header = if url.port == 80 {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    // ترميز هدف الطلب قبل الإرسال: المسار العربي الخام
    // يرفضه المحلل الصارم (llhttp) برمز 400. والهدف
    // المرمّز نفسه يمر إلى HTTP/2 أدناه. وعبر وكيل HTTP
    // عادي يكون الهدف مطلقا (المخطط + المضيف + المسار).
    let target = if absolute {
        format!(
            "{}://{}{}",
            url.scheme,
            host_header,
            encode_request_target(&url.path)
        )
    } else {
        encode_request_target(&url.path)
    };
    // اجتماع 29: الطريقة من الطلب (GET أو POST)؛ وفي
    // POST يضاف نوع المحتوى وطول الجسم، والجسم نفسه
    // بايتات بعد سطر فارغ. والمدارة (Host وUser-Agent
    // وAccept) من المحرك وحده — لا ترويسة اعتماد
    // يضيفها المحرك من نفسه أبدا؛ وترويسات المتصل
    // (اجتماع 30) ملكه هو.
    // لينكس8-طبقة1-نواة: الطريقة من الطلب + إصدار HTTP من الخيارات.
    let method = req.method_str();
    let version = if req.opts.http_version == "1.0" { "HTTP/1.0" } else { "HTTP/1.1" };
    let user_agent = req
        .opts
        .user_agent
        .clone()
        .unwrap_or_else(|| "Niva/1.0".to_string());
    // لينكس8-طبقة1-أغلفة: الرؤوس المحذوفة تُسقط من المدارة أيضا.
    let removed = |name: &str| {
        req.opts
            .removed_headers
            .iter()
            .any(|h| h.eq_ignore_ascii_case(name))
    };
    let mut head = format!("{} {} {}\r\nHost: {}\r\n", method, target, version, host_header);
    if !removed("user-agent") {
        head.push_str(&format!("User-Agent: {}\r\n", user_agent));
    }
    if !removed("accept") {
        head.push_str("Accept: */*\r\n");
    }
    // لينكس8-طبقة1-نواة: رؤوس مدارة من الخيارات (إحالة/لغة).
    if !removed("referer") {
        if let Some(r) = &req.opts.referer {
            head.push_str(&format!("Referer: {}\r\n", r));
        }
    }
    if !removed("accept-language") {
        if let Some(l) = &req.opts.language {
            head.push_str(&format!("Accept-Language: {}\r\n", l));
        }
    }
    if !removed("accept-encoding") && req.opts.gzip {
        head.push_str("Accept-Encoding: gzip\r\n");
    }
    // اجتماع 30: ترويسات المتصل — والمدارة (Host
    // وContent-Length وContent-Type وConnection)
    // تغلب دائما فتُسقط نسخ المتصل منها؛ فلا ازدواج
    // على السلك ولا قيمة متصل تحل محل المدارة.
    for (name, value) in &req.headers {
        let lower = name.to_ascii_lowercase();
        // لينكس8-طبقة1-أغلفة: رأس محذوف لا يُرسل أبدا.
        if removed(&lower) {
            continue;
        }
        if lower == "host"
            || lower == "content-length"
            || lower == "content-type"
            || lower == "connection"
            || lower == "transfer-encoding"
            || (lower == "user-agent" && req.opts.user_agent.is_some())
            || (lower == "referer" && req.opts.referer.is_some())
            || (lower == "accept-language" && req.opts.language.is_some())
            || (lower == "accept-encoding" && req.opts.gzip)
        {
            continue;
        }
        head.push_str(&format!("{}: {}\r\n", name, value));
    }
    // لينكس8-طبقة1-نواة: الجسم — عادي (طول) أو متقطع (chunks).
    if req.sends_body() {
        head.push_str(&format!("Content-Type: {}\r\n", req.content_type));
        if req.opts.chunked {
            head.push_str("Transfer-Encoding: chunked\r\n");
        } else {
            head.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
        }
    }
    // لينكس8-وكيل: تفويض الوكيل لطلب HTTP العادي (أما
    // النفق فيرسله في CONNECT لا هنا).
    if req.opts.proxy.is_some() && !req.opts.tunnel_proxy && url.scheme == "http" {
        if let Some((u, p)) = &req.opts.proxy_auth {
            let has = req
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("proxy-authorization"));
            if !has {
                head.push_str(&format!(
                    "Proxy-Authorization: Basic {}\r\n",
                    b64_encode(format!("{}:{}", u, p).as_bytes())
                ));
            }
        }
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut request: Vec<u8> = head.into_bytes();
    if req.opts.chunked && req.sends_body() {
        // لينكس8-طبقة1-نواة: الإرسال المتقطع — قطع 8KB + قطعة صفرية.
        let mut off = 0usize;
        while off < req.body.len() {
            let n = (req.body.len() - off).min(8192);
            request.extend_from_slice(format!("{:x}\r\n", n).as_bytes());
            request.extend_from_slice(&req.body[off..off + n]);
            request.extend_from_slice(b"\r\n");
            off += n;
        }
        request.extend_from_slice(b"0\r\n\r\n");
    } else {
        request.extend_from_slice(&req.body);
    }
    // Meeting 26: HTTPS wraps the connected TcpStream in TLS
    // (after the §7 IP check above, before the handshake).
    // Meeting 27 §42: with the `http2` feature the handshake
    // runs through tokio-rustls on a dedicated thread and
    // ALPN decides h2 vs the HTTP/1.1 fallback.
    #[cfg(feature = "https")]
    if url.scheme == "https" {
        #[cfg(feature = "http2")]
        {
            return fetch_https_h2(stream, url, &request, req, &target, &host_header, deadline, line, col, sink);
        }
        #[cfg(not(feature = "http2"))]
        {
            let mut tls = tls_wrap(stream, &url.host, line, col)?;
            write_throttled(
                &mut tls,
                &request,
                req.opts.max_speed_up,
                &url.host,
                line,
                col,
            )?;
            let outcome = read_response(&mut tls, deadline, line, col, &mut *sink, req.opts.return_all_status)?;
            return Ok((outcome, sink));
        }
    }
    write_throttled(
        &mut stream,
        &request,
        req.opts.max_speed_up,
        &url.host,
        line,
        col,
    )?;
    let outcome = read_response(&mut stream, deadline, line, col, &mut *sink, req.opts.return_all_status)?;
    Ok((outcome, sink))
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
    request: &[u8],
    req: &FetchRequest,
    path: &str,
    host_header: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
    sink: Box<dyn BodySink + Send>,
) -> RioResult<(FetchOutcome, Box<dyn BodySink + Send>)> {
    use std::io::Write;
    let host = url.host.clone();
    let host_async = host.clone();
    // الهدف المرمّز من fetch_one_native (لا url.path الخام:
    // مكتبة http ترفض الـ URI غير ASCII أصلا).
    let path = path.to_string();
    let host_header = host_header.to_string();
    // الطلب الكامل (ترويسات + جسم) والطلب المنطقي
    // ينتقلان مملوكين إلى خيط وقت التشغيل (async move
    // ينقل القيم نفسها فيُستعار منها داخل المستقبل).
    let request = request.to_vec();
    let req = req.clone();
    // لينكس8-طبقة1-نواة: الخياران المستعملان بعد النقل
    // يُلتقطان قبله (فـ async move ينقل req كله).
    let max_speed_up = req.opts.max_speed_up;
    let return_all = req.opts.return_all_status;
    let outcome = run_on_runtime(async move {
        fetch_https_h2_async(stream, &host_async, &path, &host_header, &req, deadline, line, col, sink).await
    })
    .map_err(|_| RioError::runtime("تعذر تنفيذ طلب الشبكة", line, col))?;
    match outcome {
        Ok((HttpsOutcome::H2(outcome), sink)) => Ok((outcome, sink)),
        Ok((HttpsOutcome::Http1(std_stream, conn), mut sink)) => {
            // Back to blocking mode with the fetch timeouts:
            // from_std required non-blocking; the sync
            // StreamOwned path needs blocking + SO_RCVTIMEO.
            let _ = std_stream.set_nonblocking(false);
            let _ = std_stream.set_read_timeout(Some(time_left(deadline, line, col)?));
            let _ = std_stream.set_write_timeout(Some(time_left(deadline, line, col)?));
            let mut tls = rustls::StreamOwned::new(conn, std_stream);
            write_throttled(
                &mut tls,
                &request,
                max_speed_up,
                host.as_str(),
                line,
                col,
            )?;
            let outcome = read_response(&mut tls, deadline, line, col, &mut *sink, return_all)?;
            Ok((outcome, sink))
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
    req: &FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
    sink: Box<dyn BodySink + Send>,
) -> RioResult<(HttpsOutcome, Box<dyn BodySink + Send>)> {
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
    // المهمة 22: المصافحة قابلة للإلغاء أيضا —
    // انتظارها مع استطلاع العلم (لا انتظار مهلة
    // كاملة بلا مخرج).
    let tls = match await_cancellable(tokio::time::timeout(
        left,
        connector.connect(server_name, tcp),
    ))
    .await
    {
        Ok(Some(r)) => r
            .map_err(|_| RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col))?
            .map_err(|e| {
                io_timeout_or(
                    e,
                    "انتهت مهلة الاتصال (30 ثانية)",
                    "تعذر إنشاء اتصال HTTPS آمن",
                    line,
                    col,
                )
            })?,
        Ok(None) => return Err(cancel_error(line, col)),
        Err(()) => {
            return Err(RioError::runtime(
                "تعذر تنفيذ طلب الشبكة",
                line,
                col,
            ))
        }
    };
    if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
        // المهمة 22: التبادل قابل للإلغاء — انتظاره
        // مع استطلاع العلم؛ وعند الإلغاء تُجهض
        // المهمة (ووقت التشغيل يُسقط معها).
        let path = path.to_string();
        let host_header = host_header.to_string();
        let req = req.clone();
        let (outcome, sink) = match await_cancellable(fetch_h2(
            tls,
            path,
            host_header,
            req,
            deadline,
            line,
            col,
            sink,
        ))
        .await
        {
            Ok(Some(r)) => r?,
            Ok(None) => return Err(cancel_error(line, col)),
            Err(()) => {
                return Err(RioError::runtime(
                    "تعذر تنفيذ طلب الشبكة",
                    line,
                    col,
                ))
            }
        };
        Ok((HttpsOutcome::H2(outcome), sink))
    } else {
        let (tcp, conn) = tls.into_inner();
        let std_stream = tcp
            .into_std()
            .map_err(|_| RioError::runtime("تعذر بدء اتصال HTTPS", line, col))?;
        Ok((HttpsOutcome::Http1(std_stream, conn), sink))
    }
}

/// انتظار مستقبل مع استطلاع علم الإلغاء كل 100ms
/// (بديل `tokio::select!` — فخاصية macros في tokio
/// غير مفعّلة في الشجرة، فلا صندوق جديد): عند
/// الإلغاء تُجهض المهمة ويُرجع `Ok(None)`؛ وعند
/// انتهاء المهمة نتيجتها؛ وعند ذعرها `Err(())`
/// (كما `run_on_runtime` — «تعذر تنفيذ طلب الشبكة»).
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn await_cancellable<T: Send + 'static>(
    fut: impl std::future::Future<Output = T> + Send + 'static,
) -> Result<Option<T>, ()> {
    let handle = tokio::spawn(fut);
    loop {
        if handle.is_finished() {
            return match handle.await {
                Ok(v) => Ok(Some(v)),
                Err(e) if e.is_cancelled() => Ok(None),
                Err(_) => Err(()),
            };
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if cancel_requested() {
            handle.abort();
            return Ok(None);
        }
    }
}

/// One h2 request over an established TLS stream: handshake,
/// send GET or POST, read the response. The h2 connection future is
/// driven on a spawned task (the canonical h2 pattern); the
/// request runs on this task. Same result mapping as
/// read_response (2xx → Body, 3xx + Location → Redirect,
/// other statuses → Arabic error), same 5 MB cap, same
/// total deadline — every await is wrapped in the time left.
#[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
async fn fetch_h2(
    tls: tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    path: String,
    host_header: String,
    req: FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
    mut sink: Box<dyn BodySink + Send>,
) -> RioResult<(FetchOutcome, Box<dyn BodySink + Send>)> {
    // لاستطلاع مستقبل الرد بلا انتظار (رد مبكر أثناء
    // إرسال الجسم): Future::poll.
    use std::future::Future;
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
    // (المهمة 22: المسار والترويسة مملوكان هنا —
    // فالمستقبل يُنقَل إلى مهمة tokio فيستلزم 'static.)
    let uri = format!("https://{}{}", host_header, path);
    // اجتماع 29: الطريقة من الطلب؛ وفي POST يضاف نوع
    // المحتوى. والمدارة من المحرك وحده — لا ترويسة
    // اعتماد يضيفها المحرك من نفسه أبدا؛ وترويسات
    // المتصل (اجتماع 30) ملكه هو.
    // لينكس8-طبقة1-أغلفة: الرؤوس المحذوفة + المدارة كما H1.
    let removed_h2 = |name: &str| {
        req.opts
            .removed_headers
            .iter()
            .any(|h| h.eq_ignore_ascii_case(name))
    };
    let user_agent_h2 = req
        .opts
        .user_agent
        .clone()
        .unwrap_or_else(|| "Niva/1.0".to_string());
    let mut builder = http::Request::builder()
        .method(
            http::Method::from_bytes(req.method_str().as_bytes()).map_err(|_| {
                RioError::runtime("طريقة طلب غير صالحة", line, col)
            })?,
        )
        .uri(uri)
        .header("host", host_header.as_str());
    if !removed_h2("user-agent") {
        builder = builder.header("user-agent", user_agent_h2.as_str());
    }
    if !removed_h2("accept") {
        builder = builder.header("accept", "*/*");
    }
    if !removed_h2("referer") {
        if let Some(r) = &req.opts.referer {
            builder = builder.header("referer", r.as_str());
        }
    }
    if !removed_h2("accept-language") {
        if let Some(l) = &req.opts.language {
            builder = builder.header("accept-language", l.as_str());
        }
    }
    if !removed_h2("accept-encoding") && req.opts.gzip {
        builder = builder.header("accept-encoding", "gzip");
    }
    // اجتماع 30: ترويسات المتصل — والمدارة (host
    // وcontent-type) تغلب؛ وconnection ممنوعة في
    // HTTP/2 أصلا فتُسقط هي وcontent-length معهما.
    for (name, value) in &req.headers {
        let lower = name.to_ascii_lowercase();
        if removed_h2(&lower) {
            continue;
        }
        if lower == "host"
            || lower == "content-length"
            || lower == "content-type"
            || lower == "connection"
        {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_str());
    }
    if req.sends_body() {
        builder = builder.header("content-type", req.content_type.as_str());
    }
    let http_req = builder
        .body(())
        .map_err(|_| RioError::runtime("تعذر بناء طلب HTTP/2", line, col))?;
    // جسم فارغ (GET أو POST بلا جسم) يغلق الإرسال مع
    // الترويسات؛ وغيره يُرسل عبر دفق الإرسال أدناه.
    let end_of_stream = !req.is_post() || req.body.is_empty();
    let (resp_fut, mut send_stream) = send
        .send_request(http_req, end_of_stream)
        .map_err(|_| RioError::runtime("تعذر إرسال طلب HTTP/2", line, col))?;
    let mut resp_fut = std::pin::pin!(resp_fut);
    if req.sends_body() && !req.body.is_empty() {
        // إرسال الجسم في قطع 16KB تناسب نافذة التدفق
        // الأولية (65,535 بايت) — على نمط اختبار 413
        // في هذا الملف: حجز سعة، انتظارها بالمهلة،
        // ثم إرسال القطعة. والجسم مسقوف أصلا بـ5MB.
        let mut offset = 0usize;
        while offset < req.body.len() {
            // رد مبكر من الخادم (خطأ بلا قراءة الجسم):
            // نتوقف عن الإرسال ونأخذ الرد نفسه بدلا
            // من خطأ إرسال مضلل.
            let mut answered = false;
            std::future::poll_fn(|cx| {
                match resp_fut.as_mut().poll(cx) {
                    std::task::Poll::Ready(_) => {
                        answered = true;
                        std::task::Poll::Ready(())
                    }
                    std::task::Poll::Pending => std::task::Poll::Ready(()),
                }
            })
            .await;
            if answered {
                break;
            }
            let n = (req.body.len() - offset).min(16 * 1024);
            send_stream.reserve_capacity(n);
            loop {
                if send_stream.capacity() >= n {
                    break;
                }
                let left = time_left_async(deadline).ok_or_else(|| {
                    RioError::runtime("انتهت المهلة الإجمالية (30 ثانية)", line, col)
                })?;
                match tokio::time::timeout(
                    left,
                    std::future::poll_fn(|cx| send_stream.poll_capacity(cx)),
                )
                .await
                {
                    Ok(Some(Ok(_))) => continue,
                    _ => break,
                }
            }
            if send_stream.capacity() < n {
                return Err(RioError::runtime(
                    "انتهت مهلة إرسال الطلب",
                    line,
                    col,
                ));
            }
            let last = offset + n >= req.body.len();
            send_stream
                .send_data(
                    bytes::Bytes::copy_from_slice(&req.body[offset..offset + n]),
                    last,
                )
                .map_err(|_| RioError::runtime("تعذر إرسال طلب HTTP/2", line, col))?;
            offset += n;
        }
    }
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
    // لينكس8-طبقة1-نواة: بيانات الرد من ترويسات h2.
    let h2_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    sink.set_meta(status, h2_headers);
    // لينكس8-طبقة1-نواة: بيانات الرد من ترويسات h2.
    let h2_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    sink.set_meta(status, h2_headers);
    // لينكس8-طبقة1-نواة: بيانات الرد من ترويسات h2.
    let h2_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    sink.set_meta(status, h2_headers);
    // لينكس8-طبقة1-نواة: بيانات الرد من ترويسات h2.
    let h2_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    sink.set_meta(status, h2_headers);
    // لينكس8-طبقة1-نواة: بيانات الرد من ترويسات h2.
    let h2_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    sink.set_meta(status, h2_headers);
    // المهمة 22: القطع تُدفع إلى المصرف (الذاكرة
    // تسقف 5MB، والملف يسقف الحصة) — فلا جسم كامل
    // في الذاكرة عند التنزيل.
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
        sink.push(&chunk, line, col)?;
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه جسم عادي.
    if req.opts.return_all_status && !((300..400).contains(&status) && location.is_some()) {
        return Ok((FetchOutcome::Body, sink));
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه جسم عادي.
    if req.opts.return_all_status && !((300..400).contains(&status) && location.is_some()) {
        return Ok((FetchOutcome::Body, sink));
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه جسم عادي.
    if req.opts.return_all_status && !((300..400).contains(&status) && location.is_some()) {
        return Ok((FetchOutcome::Body, sink));
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه جسم عادي.
    if req.opts.return_all_status && !((300..400).contains(&status) && location.is_some()) {
        return Ok((FetchOutcome::Body, sink));
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه جسم عادي.
    if req.opts.return_all_status && !((300..400).contains(&status) && location.is_some()) {
        return Ok((FetchOutcome::Body, sink));
    }
    if (200..300).contains(&status) {
        Ok((FetchOutcome::Body, sink))
    } else if (300..400).contains(&status) {
        match location {
            Some(loc) => Ok((FetchOutcome::Redirect(status, loc), sink)),
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
    sink: &mut dyn BodySink,
    // لينكس8-طبقة1-نواة: صح = إرجاع كل الحالات بلا خطأ (نفذ).
    return_all: bool,
) -> RioResult<FetchOutcome> {
    const HEADER_CAP: usize = 64 * 1024;
    let mut head: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    let mut header_end = None;
    while head.len() < HEADER_CAP {
        let _ = stream.set_read_timeout(Some(read_timeout(deadline, line, col)?));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                head.push(byte[0]);
                // المهمة 22: فحص الإلغاء بعد كل بايت
                // ناجح أيضا — فخادم يرسل بايت كل 300ms
                // لا يصل إلى WouldBlock أبدا، وبلا هذا
                // الفحص لا يُلاحظ الإلغاء حتى النهاية.
                check_cancelled(line, col)?;
                if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
                    header_end = Some(head.len());
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // المهمة 22: مهلة القراءة مسقوفة (500ms)
                // فالتوقف يعيد هنا — فحص الإلغاء ثم
                // المهلة ثم إعادة المحاولة (لا خطأ مهلة
                // بعد نصف ثانية).
                on_read_would_block(deadline, line, col)?;
                continue;
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
    // لينكس8-طبقة1-نواة: كل الرؤوس (اسم صغير + قيمة مقصوصة).
    let mut all_headers: Vec<(String, String)> = Vec::new();
    for l in lines {
        if let Some((name, value)) = l.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            all_headers.push((name.clone(), value.to_string()));
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
    // لينكس8-طبقة1-نواة: بيانات الرد قبل قراءة الجسم (فتصل
    // حتى عند التوجيه — فالمصرف يُصفَّر لاحقا لا البيانات).
    sink.set_meta(status, all_headers);
    // The body: Content-Length, then chunked, then until EOF.
    // القطع تُدفع إلى المصرف (الذاكرة تسقف 5MB،
    // والملف يسقف الحصة) — فلا جسم كامل في الذاكرة
    // عند التنزيل.
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
                let _ = stream.set_read_timeout(Some(read_timeout(deadline, line, col)?));
                match stream.read(&mut buf[..want]) {
                    Ok(0) => {
                        return Err(RioError::runtime(
                            "رد غير صالح من الخادم (انقطع أثناء الجسم)",
                            line,
                            col,
                        ))
                    }
                    Ok(n) => {
                        sink.push(&buf[..n], line, col)?;
                        check_cancelled(line, col)?;
                        remaining -= n;
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        on_read_would_block(deadline, line, col)?;
                        continue;
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
            let _ = stream.set_read_timeout(Some(read_timeout(deadline, line, col)?));
            match stream.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => {
                    sink.push(&buf[..n], line, col)?;
                    check_cancelled(line, col)?;
                    remaining -= n;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    on_read_would_block(deadline, line, col)?;
                    continue;
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
            let _ = stream.set_read_timeout(Some(read_timeout(deadline, line, col)?));
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    sink.push(&buf[..n], line, col)?;
                    check_cancelled(line, col)?;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    on_read_would_block(deadline, line, col)?;
                    continue;
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
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    // لينكس8-طبقة1-نواة: إرجاع كل الحالات — كل غير-توجيه
    // (ومنها 3xx بلا عنوان و4xx/5xx) جسم عادي.
    if return_all && !((300..400).contains(&status) && location.is_some()) {
        return Ok(FetchOutcome::Body);
    }
    if (200..300).contains(&status) {
        Ok(FetchOutcome::Body)
    } else if (300..400).contains(&status) {
        match location {
            Some(loc) => Ok(FetchOutcome::Redirect(status, loc)),
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
        let _ = stream.set_read_timeout(Some(read_timeout(deadline, line, col)?));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                out.push(byte[0]);
                // المهمة 22: فحص الإلغاء بعد كل بايت
                // (كما في read_response — فالتقطير البطيء
                // لا يصل إلى WouldBlock).
                check_cancelled(line, col)?;
                if out.len() >= 2 && &out[out.len() - 2..] == b"\r\n" {
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // المهمة 22: كما في read_response — فحص
                // الإلغاء ثم المهلة ثم إعادة المحاولة.
                on_read_would_block(deadline, line, col)?;
                continue;
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
    Ok(Value::Str(wasm_request(
        "GET", url, None, None, None, line, col,
    )?))
}

/// POST في المتصفح (اجتماع 29): كـ`fetch_wasm` لكن
/// بطريقة POST وجسم ونوع محتوى — والمتصفح نفسه
/// يحوّل 301/302/303 إلى GET ويحفظ الطريقة في
/// 307/308؛ وفحص الرابط النهائي والترويسات كما في GET.
/// (اجتماع 30: ترويسات المتصل تمر هنا أيضا؛
/// وإعادة التوجيه في المتصفح يديرها المتصفح نفسه
/// — فلا إسقاط لمضيف مختلف من المحرك هنا.)
#[cfg(target_arch = "wasm32")]
fn post_wasm(
    url: &str,
    body: &str,
    content_type: &str,
    headers: &[(String, String)],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    Ok(Value::Str(wasm_request(
        "POST",
        url,
        Some(body),
        Some(content_type),
        Some(headers),
        line,
        col,
    )?))
}

/// طلب واحد عبر XMLHttpRequest المتزامن (WASM فقط):
/// الطريقة والجسم ونوع المحتوى وترويسات المتصل من
/// المتصل؛ والباقي (فحص الرابط النهائي، والحالة،
/// وسقف الرد) واحد في GET وPOST.
#[cfg(target_arch = "wasm32")]
fn wasm_request(
    method: &str,
    url: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    headers: Option<&[(String, String)]>,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let parsed = parse_url(url, line, col)?;
    // HTTPS works here (browser TLS); plain HTTP from an https
    // page is blocked by the browser's mixed-content policy,
    // and cross-origin targets need permissive CORS headers —
    // both are browser policies, reported as fetch failures.
    let xhr = web_sys::XmlHttpRequest::new().map_err(|_| {
        RioError::runtime("تعذر إنشاء طلب الشبكة في المتصفح", line, col)
    })?;
    // Synchronous: the interpreter cannot await a Promise.
    xhr.open_with_async(method, url, false).map_err(|_| {
        RioError::runtime(format!("رابط غير صالح: {}", url), line, col)
    })?;
    if let Some(ct) = content_type {
        xhr.set_request_header("Content-Type", ct).map_err(|_| {
            RioError::runtime("تعذر ضبط نوع محتوى الطلب", line, col)
        })?;
    }
    // اجتماع 30: ترويسات المتصل — والمدارة تغلب:
    // Content-Type مدارة بالوسيط أعلاه، وHost
    // وContent-Length وConnection ممنوعة في XHR
    // أصلا فتُسقط من القائمة.
    if let Some(headers) = headers {
        for (name, value) in headers {
            let lower = name.to_ascii_lowercase();
            if lower == "host"
                || lower == "content-length"
                || lower == "content-type"
                || lower == "connection"
            {
                continue;
            }
            xhr.set_request_header(name, value).map_err(|_| {
                RioError::runtime(
                    format!("تعذر ضبط ترويسة الطلب: {}", name),
                    line,
                    col,
                )
            })?;
        }
    }
    xhr.set_timeout(TIMEOUT_SECS as u32 * 1000);
    match body {
        Some(b) => xhr.send_with_opt_str(Some(b)),
        None => xhr.send(),
    }
    .map_err(|_| {
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
    Ok(text)
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

    // اجتماع 28: قائمة سماح التحويل — مضيف واحد فقط،
    // والمطابقة حرفية بعد تصغير حالة الأحرف.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn redirect_allowlist_matches_exact_host_only() {
        let yes = |u: &str| {
            redirect_allowlisted(&parse_url(u, 1, 1).unwrap())
        };
        // يقبل: المضيف حرفيا عبر https بالمنفذ الافتراضي.
        assert!(yes(
            "https://release-assets.githubusercontent.com/أداة.نيفا"
        ));
        // يقبل: المنفذ 443 صريحا (هو الافتراضي نفسه).
        assert!(yes(
            "https://release-assets.githubusercontent.com:443/أداة.نيفا"
        ));
        // يقبل: حالة الأحرف في المضيف لا تفرق (DNS).
        assert!(yes(
            "https://RELEASE-ASSETS.GITHUBUSERCONTENT.COM/أداة.نيفا"
        ));
        // يرفض: http لا https.
        assert!(!yes(
            "http://release-assets.githubusercontent.com/أداة.نيفا"
        ));
        // يرفض: نطاق فرعي.
        assert!(!yes(
            "https://sub.release-assets.githubusercontent.com/أداة.نيفا"
        ));
        // يرفض: لاحقة (مضيف آخر ينتهي بالاسم نفسه).
        assert!(!yes(
            "https://release-assets.githubusercontent.com.evil.com/أداة.نيفا"
        ));
        // يرفض: منفذ غير قياسي على المضيف نفسه.
        assert!(!yes(
            "https://release-assets.githubusercontent.com:8443/أداة.نيفا"
        ));
        // يرفض: مضيف آخر (ومنها مضيف الواجهة نفسه).
        assert!(!yes("https://example.com/أداة.نيفا"));
        assert!(!yes("https://github.com/مالك/مستودع"));
        assert!(!yes("https://api.github.com/repos/مالك/مستودع"));
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
                // المهمة 22: نقطتا نهاية متدفقتان —
                // لا تُبنيان دفعة واحدة؛ تُكتب الترويسات
                // أولا ثم يتدفق الجسم ببطء (لاختبار
                // الإلغاء أثناء الجسم).
                if path == "/drip" {
                    // 10 بايت، بايت كل 300ms — جلبها
                    // كاملا يستغرق ~3s؛ الإلغاء في
                    // المنتصف يقطعه.
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
                    );
                    for i in 0..10u8 {
                        let _ = stream.write_all(&[b'a' + i]);
                        let _ = stream.flush();
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                    continue;
                }
                if path == "/slow-body" {
                    // الترويسات فورا، والجسم بعد 5s —
                    // الإلغاء أثناء انتظار الجسم (قراءة
                    // WouldBlock).
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
                    );
                    let _ = stream.flush();
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let _ = stream.write_all(b"ok");
                    continue;
                }
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
                        // اجتماع 28: تحويل إلى المضيف المسموح
                        // حرفيا — البوابة تفتحه، والفشل بعده
                        // (DNS/اتصال/404) من الشبكة لا من البوابة.
                        "/redirect-allowlisted" => (
                            "302 Found",
                            vec![
                                "Location: https://release-assets.githubusercontent.com/12345678/niva-terminal-1.2.10.apk"
                                    .to_string(),
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
                        "/bytes" => {
                            // المهمة 22: 2048 بايت ثنائية
                            // حتمية — تحوي 0x00 وبايتات عالية
                            // (لا UTF-8 صالح) فيثبت التنزيل
                            // أنه ثنائي آمن.
                            let b: Vec<u8> =
                                (0..2048u32).map(|i| (i % 251) as u8).collect();
                            (
                                "200 OK",
                                vec![format!("Content-Length: {}", b.len())],
                                b,
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

    // اجتماع 29: خادم POST محاكٍ — يقرأ الطريقة
    // والترويسات والجسم (حتى Content-Length تماما)،
    // ويرد بحسب المسار والطريقة. منفصل عن test_server
    // (الذي لا يقرأ جسما) فلا يمس اختبارات GET القائمة.
    #[cfg(not(target_arch = "wasm32"))]
    fn post_test_server() -> u16 {
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
                // قراءة الترويسات (حتى \r\n\r\n أينما
                // وقع — فالجسم قد يصل في القطعة نفسها).
                let mut req: Vec<u8> = Vec::new();
                let mut buf = [0u8; 1024];
                let head_end = loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break None,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if let Some(p) =
                                req.windows(4).position(|w| w == b"\r\n\r\n")
                            {
                                break Some(p);
                            }
                            if req.len() > 16384 {
                                break None;
                            }
                        }
                        Err(_) => break None,
                    }
                };
                let Some(head_end) = head_end else {
                    continue;
                };
                let head =
                    String::from_utf8_lossy(&req[..head_end]).into_owned();
                let mut body: Vec<u8> = req[head_end + 4..].to_vec();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or("");
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("/").to_string();
                let mut content_length = 0usize;
                let mut content_type = String::new();
                // اجتماع 30: التقاط ترويسات المتصل
                // لفحصها (تفويض/مخصص) والمدارة (مضيف
                // وطول) — فيردّها /echo-headers كما وصلت.
                let mut authorization = String::new();
                let mut x_test = String::new();
                let mut host = String::new();
                for l in lines {
                    if let Some((name, value)) = l.split_once(':') {
                        match name.trim().to_ascii_lowercase().as_str() {
                            "content-length" => {
                                content_length =
                                    value.trim().parse().unwrap_or(0)
                            }
                            "content-type" => {
                                content_type = value.trim().to_string()
                            }
                            "authorization" => {
                                authorization = value.trim().to_string()
                            }
                            "x-test" => {
                                x_test = value.trim().to_string()
                            }
                            "host" => {
                                host = value.trim().to_string()
                            }
                            _ => {}
                        }
                    }
                }
                while body.len() < content_length {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => body.extend_from_slice(&buf[..n]),
                        Err(_) => break,
                    }
                }
                body.truncate(content_length);
                let body_text = String::from_utf8_lossy(&body).into_owned();
                let (status, mut headers, resp_body): (
                    &str,
                    Vec<String>,
                    Vec<u8>,
                ) = match (method.as_str(), path.as_str()) {
                    ("POST", "/echo") => (
                        "200 OK",
                        vec![],
                        format!(
                            "طريقة:POST نوع:{} جسم:{}",
                            content_type, body_text
                        )
                        .into_bytes(),
                    ),
                    ("GET", "/echo-get") => {
                        ("200 OK", vec![], "وصل GET".as_bytes().to_vec())
                    }
                    ("POST", "/echo-get") => {
                        ("200 OK", vec![], "وصل POST".as_bytes().to_vec())
                    }
                    // اجتماع 30: يردّ الترويسات كما
                    // وصلت (طريقة + تفويض + مخصص +
                    // نوع + مضيف + طول) — لفحص إرسال
                    // ترويسات المتصل وغلبة المدارة
                    // وبقائها عند التحويل للمضيف نفسه.
                    ("POST", "/echo-headers") | ("GET", "/echo-headers") => (
                        "200 OK",
                        vec![],
                        format!(
                            "طريقة:{} تفويض:{} مخصص:{} نوع:{} مضيف:{} طول:{}",
                            method,
                            authorization,
                            x_test,
                            content_type,
                            host,
                            content_length
                        )
                        .into_bytes(),
                    ),
                    ("POST", "/post-redirect-302-headers") => (
                        "302 Found",
                        vec![
                            "Location: /echo-headers".to_string(),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    ("POST", "/post-redirect-307-headers") => (
                        "307 Temporary Redirect",
                        vec![
                            "Location: /echo-headers".to_string(),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    ("POST", "/post-redirect-302") => (
                        "302 Found",
                        vec![
                            "Location: /echo-get".to_string(),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    ("POST", "/post-redirect-307") => (
                        "307 Temporary Redirect",
                        vec![
                            "Location: /echo".to_string(),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    ("POST", "/post-cross-307") => (
                        "307 Temporary Redirect",
                        vec![
                            format!("Location: http://localhost:{}/echo", port),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    // اجتماع 28: تحويل إلى المضيف المسموح
                    // حرفيا — البوابة تفتحه، والفشل بعده
                    // (DNS/اتصال/404) من الشبكة لا من البوابة.
                    ("POST", "/post-redirect-allowlisted") => (
                        "302 Found",
                        vec![
                            "Location: https://release-assets.githubusercontent.com/12345678/niva-terminal-1.2.10.apk"
                                .to_string(),
                            "Content-Length: 0".to_string(),
                        ],
                        Vec::new(),
                    ),
                    ("POST", "/missing") => (
                        "404 Not Found",
                        vec!["Content-Length: 0".to_string()],
                        Vec::new(),
                    ),
                    _ => (
                        "404 Not Found",
                        vec!["Content-Length: 0".to_string()],
                        Vec::new(),
                    ),
                };
                if !headers.iter().any(|h| h.starts_with("Content-Length")) {
                    headers.push(format!("Content-Length: {}", resp_body.len()));
                }
                headers.push("Connection: close".to_string());
                let mut resp = format!("HTTP/1.1 {}\r\n", status).into_bytes();
                for h in &headers {
                    resp.extend_from_slice(h.as_bytes());
                    resp.extend_from_slice(b"\r\n");
                }
                resp.extend_from_slice(b"\r\n");
                resp.extend_from_slice(&resp_body);
                let _ = stream.write_all(&resp);
            }
        });
        port
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn post_ok(url: &str, body: &str, ct: &str) -> String {
        post_ok_headers(url, body, ct, &[])
    }

    fn post_ok_headers(
        url: &str,
        body: &str,
        ct: &str,
        headers: &[(String, String)],
    ) -> String {
        match post(url, body, ct, headers, 1, 1).unwrap() {
            Value::Str(s) => s,
            other => panic!("expected text, got {:?}", other),
        }
    }

    #[cfg(test)]
    fn post_native_with_timeout(
        url: &str,
        body: &str,
        ct: &str,
        timeout_secs: u64,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        Ok(Value::Str(post_text_native(
            url,
            body,
            ct,
            &[],
            timeout_secs,
            line,
            col,
        )?))
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
    fn fetch_allowlisted_redirect_passes_gate() {
        // اجتماع 28: التحويل إلى المضيف المسموح يمر
        // البوابة — فالخطأ بعده ليس «مضيف مختلف» أبدا.
        // الفشل اللاحق يختلف بالبيئة (لا DNS حقيقي هنا):
        // على هذه الآلة مهلة اتصال (مهلة 2 ثانية في
        // الاختبار لا 30)؛ وبلا إنترنت «تعذر حل اسم
        // المضيف»؛ ومع إنترنت 404 من شبكة التوصيل
        // نفسها. والتأكيد الوحيد الحتمي هو أن البوابة
        // لم ترفض — فكل تلك الأخطاء من الشبكة لا منها.
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let err = fetch_native_with_timeout(
            &format!("http://127.0.0.1:{}/redirect-allowlisted", port),
            2,
            1,
            1,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("مضيف مختلف"),
            "البوابة رفضت المضيف المسموح — got: {}",
            msg
        );
        assert!(!msg.is_empty(), "got empty error");
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

    // اجتماع 29: شبكة.أرسل — POST كامل: الطريقة
    // والنوع الافتراضي والجسم في الرد.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_sends_method_and_default_content_type() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let out = post_ok(
            &format!("http://127.0.0.1:{}/echo", port),
            "{\"مفتاح\":1}",
            "application/json",
        );
        assert_eq!(
            out,
            "طريقة:POST نوع:application/json جسم:{\"مفتاح\":1}",
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_custom_content_type() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let out = post_ok(
            &format!("http://127.0.0.1:{}/echo", port),
            "a=1&b=2",
            "application/x-www-form-urlencoded",
        );
        assert_eq!(
            out,
            "طريقة:POST نوع:application/x-www-form-urlencoded جسم:a=1&b=2",
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_empty_body_ok() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let out = post_ok(
            &format!("http://127.0.0.1:{}/echo", port),
            "",
            "application/json",
        );
        assert_eq!(
            out,
            "طريقة:POST نوع:application/json جسم:",
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_gate_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(false);
        let err = post(
            "http://127.0.0.1:9/echo",
            "{}",
            "application/json",
            &[],
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains(PERMISSION_ERROR_MARKER),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_body_too_big_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let big = "x".repeat(MAX_REQUEST_BYTES + 1);
        let err = post(
            &format!("http://127.0.0.1:{}/echo", port),
            &big,
            "application/json",
            &[],
            1,
            1,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("جسم الطلب"), "got: {}", msg);
        assert!(msg.contains("5 ميغابايت"), "got: {}", msg);
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_302_becomes_get() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let out = post_ok(
            &format!("http://127.0.0.1:{}/post-redirect-302", port),
            "{\"مفتاح\":1}",
            "application/json",
        );
        assert_eq!(out, "وصل GET", "got: {}", out);
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_307_same_host_repeats_post() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let out = post_ok(
            &format!("http://127.0.0.1:{}/post-redirect-307", port),
            "{\"مفتاح\":1}",
            "application/json",
        );
        assert_eq!(
            out,
            "طريقة:POST نوع:application/json جسم:{\"مفتاح\":1}",
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_307_cross_host_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let err = post(
            &format!("http://127.0.0.1:{}/post-cross-307", port),
            "{}",
            "application/json",
            &[],
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("مضيف مختلف"),
            "got: {}",
            err
        );
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_302_allowlisted_passes_gate() {
        // اجتماع 28 + 29: تحويل 302 بعد POST إلى المضيف
        // المسموح يمر البوابة (كـGET) — فالخطأ بعده
        // ليس «مضيف مختلف» أبدا؛ والفشل اللاحق من
        // الشبكة (لا DNS حقيقي هنا) لا من البوابة.
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let err = post_native_with_timeout(
            &format!(
                "http://127.0.0.1:{}/post-redirect-allowlisted",
                port
            ),
            "{}",
            "application/json",
            2,
            1,
            1,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("مضيف مختلف"),
            "البوابة رفضت المضيف المسموح — got: {}",
            msg
        );
        assert!(!msg.is_empty(), "got empty error");
        set_network_allowed(false);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_non_2xx_names_status() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let err = post(
            &format!("http://127.0.0.1:{}/missing", port),
            "{}",
            "application/json",
            &[],
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("404"), "got: {}", err);
        set_network_allowed(false);
    }

    // اجتماع 30: ترويسات المتصل تُرسل كما هي —
    // التفويض والمخصص يصلان إلى الخادم.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_sends_caller_headers() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let headers = vec![
            ("Authorization".to_string(), "Bearer رمز".to_string()),
            ("X-Test".to_string(), "قيمة".to_string()),
        ];
        let out = post_ok_headers(
            &format!("http://127.0.0.1:{}/echo-headers", port),
            "{}",
            "application/json",
            &headers,
        );
        assert_eq!(
            out,
            format!(
                "طريقة:POST تفويض:Bearer رمز مخصص:قيمة نوع:application/json مضيف:127.0.0.1:{} طول:2",
                port
            ),
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    // اجتماع 30: المدارة تغلب دائما — نسخ المتصل
    // من Host وContent-Type وContent-Length
    // وConnection لا تصل إلى الخادم أبدا.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_managed_headers_win() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let headers = vec![
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("Host".to_string(), "evil.example".to_string()),
            ("Content-Length".to_string(), "999".to_string()),
            ("Connection".to_string(), "keep-alive".to_string()),
        ];
        let out = post_ok_headers(
            &format!("http://127.0.0.1:{}/echo-headers", port),
            "{}",
            "application/json",
            &headers,
        );
        assert_eq!(
            out,
            format!(
                "طريقة:POST تفويض: مخصص: نوع:application/json مضيف:127.0.0.1:{} طول:2",
                port
            ),
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    // اجتماع 30: CR/LF في اسم ترويسة أو قيمتها
    // يُرفض بخطأ عربي قبل أي شبكة — فحتى رابط
    // بلا خادم يعطي خطأ الترويسة لا خطأ الاتصال.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_headers_crlf_refused() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let bad_value = vec![(
            "X-Bad".to_string(),
            "حقن\r\nترويسة: x".to_string(),
        )];
        let err = post(
            "http://127.0.0.1:9/echo",
            "{}",
            "application/json",
            &bad_value,
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("ترويسة"), "got: {}", err);
        let bad_name = vec![("X-Bad\r\nName".to_string(), "x".to_string())];
        let err = post(
            "http://127.0.0.1:9/echo",
            "{}",
            "application/json",
            &bad_name,
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("ترويسة"), "got: {}", err);
        set_network_allowed(false);
    }

    // اجتماع 30: التحويل 302 للمضيف نفسه يحوّل
    // إلى GET ويُبقي ترويسات المتصل (التفويض يصل
    // إلى الهدف النهائي على المضيف نفسه).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_302_keeps_headers_same_host() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let headers = vec![(
            "Authorization".to_string(),
            "Bearer رمز".to_string(),
        )];
        let out = post_ok_headers(
            &format!(
                "http://127.0.0.1:{}/post-redirect-302-headers",
                port
            ),
            "{}",
            "application/json",
            &headers,
        );
        assert_eq!(
            out,
            format!(
                "طريقة:GET تفويض:Bearer رمز مخصص: نوع: مضيف:127.0.0.1:{} طول:0",
                port
            ),
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    // اجتماع 30: التحويل 307 للمضيف نفسه يعيد
    // POST بالجسم نفسه ويُبقي ترويسات المتصل.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn post_redirect_307_keeps_headers_same_host() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = post_test_server();
        let headers = vec![(
            "Authorization".to_string(),
            "Bearer رمز".to_string(),
        )];
        let out = post_ok_headers(
            &format!(
                "http://127.0.0.1:{}/post-redirect-307-headers",
                port
            ),
            "{}",
            "application/json",
            &headers,
        );
        assert_eq!(
            out,
            format!(
                "طريقة:POST تفويض:Bearer رمز مخصص: نوع:application/json مضيف:127.0.0.1:{} طول:2",
                port
            ),
            "got: {}",
            out
        );
        set_network_allowed(false);
    }

    // اجتماع 30: قاعدة بقاء الترويسات عند التحويل
    // — المضيف المطابق حرفيا (بلا فرق حالة) مع
    // المنفذ نفسه فقط؛ ومضيف أو منفذ مختلف يُسقطها.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn headers_survive_redirect_rule() {
        let a = parse_url("http://127.0.0.1:8080/a", 1, 1).unwrap();
        let same = parse_url("http://127.0.0.1:8080/b", 1, 1).unwrap();
        assert!(headers_survive_redirect(&a, &same));
        let case = parse_url("http://LOCALHOST:8080/b", 1, 1).unwrap();
        let lower = parse_url("http://localhost:8080/b", 1, 1).unwrap();
        assert!(headers_survive_redirect(&case, &lower));
        let other_host = parse_url("http://localhost:8080/b", 1, 1).unwrap();
        assert!(!headers_survive_redirect(&a, &other_host));
        let other_port = parse_url("http://127.0.0.1:9090/b", 1, 1).unwrap();
        assert!(!headers_survive_redirect(&a, &other_port));
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

    // --- المهمة 22: شبكة.نزّل + إلغاء أمر معلق ---

    fn download_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-download-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn expected_bytes() -> Vec<u8> {
        (0..2048u32).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn download_ok_binary_safe() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let dir = download_test_dir("ok");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        let v = download(
            &format!("http://127.0.0.1:{}/bytes", port),
            &policy,
            &dir,
            "ملف.بن",
            1,
            1,
        )
        .unwrap();
        assert!(matches!(v, Value::Number(n) if n == 2048.0), "got: {:?}", v);
        let got = std::fs::read(dir.join("ملف.بن")).unwrap();
        assert_eq!(got, expected_bytes());
        // غير UTF-8 صالح: أول بايت 0x00 موجود حرفيا.
        assert_eq!(got[0], 0u8);
        let _ = std::fs::remove_dir_all(&dir);
        set_network_allowed(false);
    }

    #[test]
    fn download_redirect_followed() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let dir = download_test_dir("redirect");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        download(
            &format!("http://127.0.0.1:{}/redirect", port),
            &policy,
            &dir,
            "معاد.بن",
            1,
            1,
        )
        .unwrap();
        let got = std::fs::read(dir.join("معاد.بن")).unwrap();
        assert_eq!(got, "مرحبا من الخادم".as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
        set_network_allowed(false);
    }

    #[test]
    fn download_replaces_existing_file() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let dir = download_test_dir("replace");
        std::fs::write(dir.join("قديم.بن"), "محتوى قديم طويل".as_bytes()).unwrap();
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        let v = download(
            &format!("http://127.0.0.1:{}/hello", port),
            &policy,
            &dir,
            "قديم.بن",
            1,
            1,
        )
        .unwrap();
        assert!(matches!(v, Value::Number(_)), "got: {:?}", v);
        let got = std::fs::read(dir.join("قديم.بن")).unwrap();
        assert_eq!(got, "مرحبا من الخادم".as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
        set_network_allowed(false);
    }

    #[test]
    fn download_http_error_leaves_no_file() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let dir = download_test_dir("error");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        let err = download(
            &format!("http://127.0.0.1:{}/status500", port),
            &policy,
            &dir,
            "فشل.بن",
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("500"), "got: {}", err);
        assert!(!dir.join("فشل.بن").exists());
        let _ = std::fs::remove_dir_all(&dir);
        set_network_allowed(false);
    }

    #[test]
    fn download_refused_without_network_flag() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(false);
        let dir = download_test_dir("gate");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        let err = download(
            "http://127.0.0.1:9/hello",
            &policy,
            &dir,
            "مغلق.بن",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("الشبكة مغلقة"),
            "got: {}",
            err
        );
        assert!(!dir.join("مغلق.بن").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_outside_policy_refused_before_network() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let dir = download_test_dir("policy");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        let err = download(
            "http://127.0.0.1:9/hello",
            &policy,
            &dir,
            "../خارج.بن",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("خارج مجلد"),
            "got: {}",
            err
        );
        set_network_allowed(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_cancelled_by_flag_deletes_partial() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        let dir = download_test_dir("cancel");
        let policy = crate::permissions::FilePolicy::restricted(dir.clone());
        clear_fetch_cancel();
        let setter = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            request_fetch_cancel();
        });
        let start = std::time::Instant::now();
        let err = download(
            &format!("http://127.0.0.1:{}/drip", port),
            &policy,
            &dir,
            "جزئي.بن",
            1,
            1,
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        let _ = setter.join();
        assert!(err.to_string().contains("أُلغي الأمر"), "got: {}", err);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "cancel took {:?}",
            elapsed
        );
        assert!(!dir.join("جزئي.بن").exists());
        clear_fetch_cancel();
        let _ = std::fs::remove_dir_all(&dir);
        set_network_allowed(false);
    }

    #[test]
    fn fetch_cancelled_by_flag_mid_body() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        clear_fetch_cancel();
        let setter = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            request_fetch_cancel();
        });
        let start = std::time::Instant::now();
        let err = fetch_text_native(
            &format!("http://127.0.0.1:{}/drip", port),
            30,
            1,
            1,
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        let _ = setter.join();
        assert!(err.to_string().contains("أُلغي الأمر"), "got: {}", err);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "cancel took {:?}",
            elapsed
        );
        clear_fetch_cancel();
        set_network_allowed(false);
    }

    #[test]
    fn fetch_cancelled_while_waiting_for_body() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        let port = test_server();
        clear_fetch_cancel();
        let setter = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            request_fetch_cancel();
        });
        let start = std::time::Instant::now();
        let err = fetch_text_native(
            &format!("http://127.0.0.1:{}/slow-body", port),
            30,
            1,
            1,
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        let _ = setter.join();
        assert!(err.to_string().contains("أُلغي الأمر"), "got: {}", err);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "cancel took {:?}",
            elapsed
        );
        clear_fetch_cancel();
        set_network_allowed(false);
    }

    #[test]
    fn cancellable_cancel_line_returns_cancel_error() {
        let _guard = crate::stdin_pump::pump_test_lock();
        let injector = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            crate::stdin_pump::inject_stdin_line(
                crate::stdin_pump::CANCEL_LINE.to_string() + "\n",
            );
        });
        let start = std::time::Instant::now();
        let r: RioResult<u64> = run_cancellable(
            || {
                loop {
                    if cancel_requested() {
                        return Ok(7);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            },
            1,
            1,
        );
        let elapsed = start.elapsed();
        let _ = injector.join();
        let err = r.unwrap_err();
        assert!(err.to_string().contains("أُلغي الأمر"), "got: {}", err);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "cancel took {:?}",
            elapsed
        );
        clear_fetch_cancel();
    }

    #[test]
    fn cancellable_pending_line_preserved() {
        let _guard = crate::stdin_pump::pump_test_lock();
        let injector = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(200));
            crate::stdin_pump::inject_stdin_line("سطر أثناء الجلب\n".to_string());
        });
        let r: RioResult<u64> = run_cancellable(
            || {
                std::thread::sleep(std::time::Duration::from_secs(1));
                Ok(9)
            },
            1,
            1,
        );
        let _ = injector.join();
        assert_eq!(r.unwrap(), 9);
        assert_eq!(
            crate::stdin_pump::stdin_line().as_deref(),
            Some("سطر أثناء الجلب\n")
        );
    }

    #[test]
    fn cancellable_stale_cancel_discarded() {
        let _guard = crate::stdin_pump::pump_test_lock();
        crate::stdin_pump::inject_stdin_line(
            crate::stdin_pump::CANCEL_LINE.to_string() + "\n",
        );
        let r: RioResult<u64> = run_cancellable(|| Ok(5), 1, 1);
        assert_eq!(r.unwrap(), 5);
    }

    #[test]
    fn file_sink_quota_limit() {
        let dir = download_test_dir("quota");
        let file = std::fs::File::create(dir.join("حصة.بن")).unwrap();
        let mut sink = FileSink {
            file,
            written: 0,
            limit: 10,
            display: "حصة.بن".to_string(),
            meta: None,
        };
        sink.push(b"12345", 1, 1).unwrap();
        let err = sink.push(b"123456", 1, 1).unwrap_err();
        assert!(err.to_string().contains("حصة التخزين"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
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
            // اجتماع 29: /echo-post يردّ طريقة الطلب
            // وجسمه كما وصل إلى المعالج — فيثبت أن
            // الخادم يستقبل أجسام POST (الحقل «جسم»
            // في قاموس الطلب) وأن عميل POST يصل إليه.
            let method = match &arg {
                Value::Dict(pairs) => match pairs.iter().find(|(k, _)| k == "طريقة") {
                    Some((_, Value::Str(m))) => m.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            let body = match &arg {
                Value::Dict(pairs) => match pairs.iter().find(|(k, _)| k == "جسم") {
                    Some((_, Value::Str(b))) => b.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            match path.as_str() {
                "/echo-post" => (
                    Ok(Value::Str(format!("طريقة:{} جسم:{}", method, body))),
                    String::new(),
                ),
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

    // اجتماع 29: عميل المحرك يرسل POST عبر HTTP/2 إلى
    // خادم نيفا نفسه (تفاوض ALPN على h2) — والمعالج
    // (/echo-post في StubCaller) يردّ الطريقة والجسم،
    // فيثبت مسار POST في HTTP/2 إرسالا واستقبالا.
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "http2"))]
    fn h2_engine_client_posts_to_niva_server() {
        let _guard = NET_TEST_LOCK.lock().unwrap();
        let dir = server_test_dir("h2-post");
        let (handle, rx, port) =
            start_test_server(true, &dir, "server-cert.pem", "server-key.pem");
        set_network_allowed(true);
        allow_local_host("127.0.0.1");
        set_test_tls_roots(vec![tls_fixture("tls-ca-cert.der")]);
        let out = post_ok(
            &format!("https://127.0.0.1:{}/echo-post", port),
            "{\"مفتاح\":1}",
            "application/json",
        );
        assert_eq!(out, "طريقة:POST جسم:{\"مفتاح\":1}", "got: {}", out);
        let form = post_ok(
            &format!("https://127.0.0.1:{}/echo-post", port),
            "code=abc&grant_type=device_code",
            "application/x-www-form-urlencoded",
        );
        assert_eq!(
            form, "طريقة:POST جسم:code=abc&grant_type=device_code",
            "got: {}",
            form
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

// ----- لينكس8-طبقة1-نواة: نواة الطلب (اجتماع 90 §105) -----

/// خيارات تنفيذ الطلب الكاملة — تُبنى من قاموس طلب نيفا
/// (exec_opts_from_dict) وتُحمل في FetchRequest.opts.
#[derive(Clone)]
pub struct ExecOpts {
    pub timeout_secs: u64,
    pub stall_secs: u64,
    pub retries: u32,
    pub retry_delay_ms: u64,
    pub max_retry_ms: u64,
    pub retry_on_refused: bool,
    pub retry_forever: bool,
    pub follow_redirects: bool,
    pub max_redirects: usize,
    pub redirect_protocols: Vec<String>,
    pub keep_body_on_redirect: bool,
    pub trusted_only_redirect: bool,
    pub verify_cert: bool,
    pub gzip: bool,
    pub check_status: bool,
    pub return_all_status: bool,
    pub allow_http: bool,
    pub http_version: String,
    pub chunked: bool,
    pub response_encoding: String,
    pub save_to: Option<String>,
    pub max_size: Option<u64>,
    pub no_overwrite: bool,
    pub skip_existing: bool,
    pub delete_on_fail: bool,
    pub preserve_mtime: bool,
    pub resume: bool,
    pub range_from: Option<u64>,
    pub range_to: Option<u64>,
    pub etag: Option<String>,
    pub if_modified_since: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub language: Option<String>,
    pub trace_file: Option<String>,
    pub silent: bool,
    pub verbose: bool,
    pub errors_only: bool,
    pub show_progress: bool,
    pub max_speed_down: Option<u64>,
    pub max_speed_up: Option<u64>,
    pub proxy: Option<String>,
    pub proxy_auth: Option<(String, String)>,
    pub no_proxy: Vec<String>,
    pub socks5: Option<String>,
    pub tunnel_proxy: bool,
    pub ca_file: Option<String>,
    pub ca_dir: Option<String>,
    pub client_cert: Option<(String, String)>,
    pub no_userinfo: bool,
    pub cookies: Vec<(String, String)>,
    pub auth: Option<AuthKind>,
    pub session_cookies: Vec<(String, String)>,
    pub ignore_session_cookies: bool,
    pub trace_text: bool,
    pub removed_headers: Vec<String>,
}

impl Default for ExecOpts {
    fn default() -> Self {
        ExecOpts {
            timeout_secs: 30,
            stall_secs: 0,
            retries: 1,
            retry_delay_ms: 0,
            max_retry_ms: 0,
            retry_on_refused: false,
            retry_forever: false,
            follow_redirects: true,
            max_redirects: 5,
            redirect_protocols: vec!["http".to_string(), "https".to_string()],
            keep_body_on_redirect: false,
            trusted_only_redirect: false,
            verify_cert: true,
            gzip: true,
            check_status: false,
            return_all_status: false,
            allow_http: false,
            http_version: "1.1".to_string(),
            chunked: false,
            response_encoding: "utf-8".to_string(),
            save_to: None,
            max_size: None,
            no_overwrite: false,
            skip_existing: false,
            delete_on_fail: true,
            preserve_mtime: false,
            resume: false,
            range_from: None,
            range_to: None,
            etag: None,
            if_modified_since: None,
            user_agent: None,
            referer: None,
            language: None,
            trace_file: None,
            silent: false,
            verbose: false,
            errors_only: false,
            show_progress: false,
            max_speed_down: None,
            max_speed_up: None,
            proxy: None,
            proxy_auth: None,
            no_proxy: Vec::new(),
            socks5: None,
            tunnel_proxy: false,
            ca_file: None,
            ca_dir: None,
            client_cert: None,
            no_userinfo: true,
            cookies: Vec::new(),
            auth: None,
            session_cookies: Vec::new(),
            ignore_session_cookies: false,
            trace_text: true,
            removed_headers: Vec::new(),
        }
    }
}

/// نوع المصادقة المحمول في قاموس الطلب (مفتاح «مصادقة»).
#[derive(Clone, Debug)]
pub enum AuthKind {
    Basic(String, String),
    Digest(String, String),
    Negotiate(String, String),
    Bearer(String),
    Amazon {
        key: String,
        secret: String,
        region: String,
        service: String,
    },
    HttpSig {
        key_id: String,
        key: String,
        algorithm: String,
    },
}

/// رد منفذ: رمز + بايتات الجسم + رؤوس (أسماء صغيرة).
pub struct NetResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub bytes: u64,
}

// ترميز Base64 صغير (للمصادقة الأساسية ومصادقة الوكيل) —
// بلا اعتماد جديد.
pub fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let h = crate::crypto::sha256(key);
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = ipad.to_vec();
    inner.extend_from_slice(msg);
    let ih = crate::crypto::sha256(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&ih);
    crate::crypto::sha256(&outer)
}

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

// ----- لينكس8: تحويل قاموس الخيارات + المنفذ -----

fn opt_get<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn opt_bad(display: &str, key: &str, line: usize, col: usize) -> RioError {
    RioError::typ(
        format!("الدالة {} {} غير صالح", display, key),
        line,
        col,
    )
}

fn opt_bool(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<bool>> {
    match opt_get(pairs, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(opt_bad(display, key, line, col)),
    }
}

fn opt_num(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<f64>> {
    match opt_get(pairs, key) {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(Some(*n)),
        Some(_) => Err(opt_bad(display, key, line, col)),
    }
}

fn opt_int(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<i64>> {
    match opt_num(pairs, key, display, line, col)? {
        None => Ok(None),
        Some(n) => {
            if n.fract() != 0.0 || n.abs() >= 9.0e15 {
                Err(opt_bad(display, key, line, col))
            } else {
                Ok(Some(n as i64))
            }
        }
    }
}

fn opt_str(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<String>> {
    match opt_get(pairs, key) {
        None => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(opt_bad(display, key, line, col)),
    }
}

fn opt_list_str(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<Vec<String>>> {
    match opt_get(pairs, key) {
        None => Ok(None),
        Some(Value::List(items)) => {
            let mut out = Vec::new();
            for it in items {
                match it {
                    Value::Str(s) => out.push(s.clone()),
                    _ => return Err(opt_bad(display, key, line, col)),
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(opt_bad(display, key, line, col)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn opt_dict_str(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<Vec<(String, String)>>> {
    match opt_get(pairs, key) {
        None => Ok(None),
        Some(Value::Dict(items)) => {
            let mut out = Vec::new();
            for (k, v) in items {
                match v {
                    Value::Str(s) => out.push((k.clone(), s.clone())),
                    _ => return Err(opt_bad(display, key, line, col)),
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(opt_bad(display, key, line, col)),
    }
}

/// قاموس طلب نيفا → خيارات تنفيذ. المفاتيح البنيوية
/// (رابط/طريقة/رؤوس/كوكيز/جسم/نوع_المحتوى) تُقرأ في
/// الغلاف لا هنا؛ وأي مفتاح غير معروف خطأ (حماية من
/// خطأ إملائي في اسم الخيار).
pub fn exec_opts_from_dict(
    pairs: &[(String, Value)],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<ExecOpts> {
    const KNOWN: &[&str] = &[
        "مهلة", "مهلة_النقل", "محاولات", "تأخير", "أقصى_زمن_المحاولات",
        "أعد_عند_الرفض", "أعد_بلا_نهاية", "اتباع_التوجيه", "أقصى_التوجيه",
        "بروتوكولات_التوجيه", "أبق_الجسم_بعد_التوجيه", "توجيه_لموثوق_فقط",
        "تحقق_الشهادة", "ضغط", "تحقق_من_الحالة", "اسمح_http", "إصدار_http",
        "إرسال_متقطع", "ترميز_الرد", "حفظ_في", "حد_الحجم", "منع_الكتابة",
        "تخط_الموجود", "حفظ_وقت_التعديل", "استئناف", "مدى", "وسم_الكيان",
        "شرط_زمني", "وكيل_المستخدم", "إحالة", "لغة", "تتبع_في", "تتبع_نصي",
        "صامت", "مفصل", "أخطاء_فقط", "أظهر_التقدم", "حد_سرعة_التنزيل",
        "حد_سرعة_الرفع", "وكيل", "مصادقة_الوكيل", "بلا_وكيل_ل", "وكيل_socks5",
        "نفق_الوكيل", "ملف_شهادات", "مجلد_شهادات", "شهادة_عميل",
        "منع_بيانات_الدخول", "رؤوس_محذوفة", "تجاهل_كوكيز_الجلسة",
        "مصادقة", "سجل_التنزيل", "إلحاق_السجل", "مجلد_التنزيل",
        "عمق_التكرار", "حوّل_الروابط", "احترام_الروبوتات", "البقاء_في_المضيف",
        "البقاء_في_المجلد", "أنواع_مقبولة", "أنواع_مرفوضة", "أنماط_مقبولة",
        "تتبع_الصور", "تتبع_css", "سطّح_المجلدات", "حد_الملفات",
        "بنية_المجلدات", "أنشئ_المجلدات", "أنشئ_مجلدات_ftp", "حذف_عند_الفشل",
    ];
    for (k, _) in pairs {
        if !KNOWN.contains(&k.as_str())
            && !matches!(
                k.as_str(),
                "رابط" | "طريقة" | "رؤوس" | "كوكيز" | "جسم" | "نوع_المحتوى"
            )
        {
            let safe = !k.is_empty()
                && k.chars()
                    .all(|c| c.is_alphanumeric() || c == '_');
            return Err(RioError::typ(
                if safe {
                    format!("الدالة {} خيار غير معروف: {}", display, k)
                } else {
                    format!("الدالة {} خيار غير معروف", display)
                },
                line,
                col,
            ));
        }
    }
    let mut o = ExecOpts::default();
    if let Some(v) = opt_num(pairs, "مهلة", display, line, col)? {
        if !(v > 0.0) || !v.is_finite() {
            return Err(opt_bad(display, "مهلة", line, col));
        }
        o.timeout_secs = v as u64;
    }
    if let Some(v) = opt_num(pairs, "مهلة_النقل", display, line, col)? {
        if !(v > 0.0) || !v.is_finite() {
            return Err(opt_bad(display, "مهلة_النقل", line, col));
        }
        o.stall_secs = v as u64;
    }
    if let Some(v) = opt_int(pairs, "محاولات", display, line, col)? {
        if v < 1 || v > 100 {
            return Err(opt_bad(display, "محاولات", line, col));
        }
        o.retries = v as u32;
    }
    if let Some(v) = opt_num(pairs, "تأخير", display, line, col)? {
        if v < 0.0 || !v.is_finite() {
            return Err(opt_bad(display, "تأخير", line, col));
        }
        o.retry_delay_ms = (v * 1000.0) as u64;
    }
    if let Some(v) = opt_num(pairs, "أقصى_زمن_المحاولات", display, line, col)? {
        if !(v > 0.0) || !v.is_finite() {
            return Err(opt_bad(display, "أقصى_زمن_المحاولات", line, col));
        }
        o.max_retry_ms = (v * 1000.0) as u64;
    }
    if let Some(v) = opt_bool(pairs, "أعد_عند_الرفض", display, line, col)? {
        o.retry_on_refused = v;
    }
    if let Some(v) = opt_bool(pairs, "أعد_بلا_نهاية", display, line, col)? {
        o.retry_forever = v;
    }
    if let Some(v) = opt_bool(pairs, "اتباع_التوجيه", display, line, col)? {
        o.follow_redirects = v;
    }
    if let Some(v) = opt_int(pairs, "أقصى_التوجيه", display, line, col)? {
        if v < 0 || v > 100 {
            return Err(opt_bad(display, "أقصى_التوجيه", line, col));
        }
        o.max_redirects = v as usize;
    }
    if let Some(v) = opt_list_str(pairs, "بروتوكولات_التوجيه", display, line, col)? {
        for p in &v {
            if p != "http" && p != "https" {
                return Err(opt_bad(display, "بروتوكولات_التوجيه", line, col));
            }
        }
        o.redirect_protocols = v;
    }
    if let Some(v) = opt_bool(pairs, "أبق_الجسم_بعد_التوجيه", display, line, col)? {
        o.keep_body_on_redirect = v;
    }
    if let Some(v) = opt_bool(pairs, "توجيه_لموثوق_فقط", display, line, col)? {
        o.trusted_only_redirect = v;
    }
    if let Some(v) = opt_bool(pairs, "تحقق_الشهادة", display, line, col)? {
        if !v {
            return Err(RioError::runtime(
                "تجاهل التحقق من الشهادة مرفوض — الشهادة تُتحقق دائما",
                line,
                col,
            ));
        }
        o.verify_cert = true;
    }
    if let Some(v) = opt_bool(pairs, "ضغط", display, line, col)? {
        o.gzip = v;
    }
    if let Some(v) = opt_bool(pairs, "تحقق_من_الحالة", display, line, col)? {
        o.check_status = v;
    }
    if let Some(v) = opt_bool(pairs, "اسمح_http", display, line, col)? {
        o.allow_http = v;
    }
    if let Some(v) = opt_str(pairs, "إصدار_http", display, line, col)? {
        if v != "1.0" && v != "1.1" {
            return Err(opt_bad(display, "إصدار_http", line, col));
        }
        o.http_version = v;
    }
    if let Some(v) = opt_bool(pairs, "إرسال_متقطع", display, line, col)? {
        o.chunked = v;
    }
    if let Some(v) = opt_str(pairs, "ترميز_الرد", display, line, col)? {
        if v != "utf-8" && v != "latin1" {
            return Err(opt_bad(display, "ترميز_الرد", line, col));
        }
        o.response_encoding = v;
    }
    if let Some(v) = opt_str(pairs, "حفظ_في", display, line, col)? {
        if v.is_empty() {
            return Err(opt_bad(display, "حفظ_في", line, col));
        }
        o.save_to = Some(v);
    }
    if let Some(v) = opt_int(pairs, "حد_الحجم", display, line, col)? {
        if v <= 0 {
            return Err(opt_bad(display, "حد_الحجم", line, col));
        }
        o.max_size = Some(v as u64);
    }
    if let Some(v) = opt_bool(pairs, "منع_الكتابة", display, line, col)? {
        o.no_overwrite = v;
    }
    if let Some(v) = opt_bool(pairs, "تخط_الموجود", display, line, col)? {
        o.skip_existing = v;
    }
    if let Some(v) = opt_bool(pairs, "حفظ_وقت_التعديل", display, line, col)? {
        o.preserve_mtime = v;
    }
    if let Some(v) = opt_bool(pairs, "استئناف", display, line, col)? {
        o.resume = v;
    }
    if let Some(v) = opt_get(pairs, "مدى") {
        match v {
            Value::Dict(d) => {
                let from = d
                    .iter()
                    .find(|(k, _)| k == "من")
                    .and_then(|(_, x)| match x {
                        Value::Number(n) => Some(*n as i64),
                        _ => None,
                    });
                let to = d
                    .iter()
                    .find(|(k, _)| k == "إلى")
                    .and_then(|(_, x)| match x {
                        Value::Number(n) => Some(*n as i64),
                        _ => None,
                    });
                match from {
                    Some(f) if f >= 0 && to.map(|t| t >= f).unwrap_or(true) => {
                        o.range_from = Some(f as u64);
                        o.range_to = to.map(|t| t as u64);
                    }
                    _ => return Err(opt_bad(display, "مدى", line, col)),
                }
            }
            _ => return Err(opt_bad(display, "مدى", line, col)),
        }
    }
    if let Some(v) = opt_str(pairs, "وسم_الكيان", display, line, col)? {
        o.etag = Some(v);
    }
    if let Some(v) = opt_str(pairs, "شرط_زمني", display, line, col)? {
        if v.is_empty() {
            return Err(opt_bad(display, "شرط_زمني", line, col));
        }
        o.if_modified_since = Some(v);
    }
    if let Some(v) = opt_str(pairs, "وكيل_المستخدم", display, line, col)? {
        o.user_agent = Some(v);
    }
    if let Some(v) = opt_str(pairs, "إحالة", display, line, col)? {
        o.referer = Some(v);
    }
    if let Some(v) = opt_str(pairs, "لغة", display, line, col)? {
        o.language = Some(v);
    }
    if let Some(v) = opt_str(pairs, "تتبع_في", display, line, col)? {
        o.trace_file = Some(v);
    }
    if let Some(v) = opt_bool(pairs, "تتبع_نصي", display, line, col)? {
        o.trace_text = v;
    }
    if let Some(v) = opt_list_str(pairs, "رؤوس_محذوفة", display, line, col)? {
        o.removed_headers = v;
    }
    if let Some(v) = opt_bool(pairs, "صامت", display, line, col)? {
        o.silent = v;
    }
    if let Some(v) = opt_bool(pairs, "مفصل", display, line, col)? {
        o.verbose = v;
    }
    if let Some(v) = opt_bool(pairs, "أخطاء_فقط", display, line, col)? {
        o.errors_only = v;
    }
    if let Some(v) = opt_bool(pairs, "أظهر_التقدم", display, line, col)? {
        o.show_progress = v;
    }
    if let Some(v) = opt_int(pairs, "حد_سرعة_التنزيل", display, line, col)? {
        if v <= 0 {
            return Err(opt_bad(display, "حد_سرعة_التنزيل", line, col));
        }
        o.max_speed_down = Some(v as u64);
    }
    if let Some(v) = opt_int(pairs, "حد_سرعة_الرفع", display, line, col)? {
        if v <= 0 {
            return Err(opt_bad(display, "حد_سرعة_الرفع", line, col));
        }
        o.max_speed_up = Some(v as u64);
    }
    if let Some(v) = opt_str(pairs, "وكيل", display, line, col)? {
        o.proxy = Some(v);
    }
    if let Some(v) = opt_get(pairs, "مصادقة_الوكيل") {
        match v {
            Value::Dict(d) => {
                let u = d.iter().find(|(k, _)| k == "مستخدم");
                let p = d.iter().find(|(k, _)| k == "كلمة_سر");
                match (u, p) {
                    (Some((_, Value::Str(u))), Some((_, Value::Str(p)))) => {
                        o.proxy_auth = Some((u.clone(), p.clone()));
                    }
                    _ => return Err(opt_bad(display, "مصادقة_الوكيل", line, col)),
                }
            }
            _ => return Err(opt_bad(display, "مصادقة_الوكيل", line, col)),
        }
    }
    if let Some(v) = opt_list_str(pairs, "بلا_وكيل_ل", display, line, col)? {
        o.no_proxy = v;
    }
    if let Some(v) = opt_str(pairs, "وكيل_socks5", display, line, col)? {
        o.socks5 = Some(v);
    }
    if let Some(v) = opt_bool(pairs, "نفق_الوكيل", display, line, col)? {
        o.tunnel_proxy = v;
    }
    if let Some(v) = opt_str(pairs, "ملف_شهادات", display, line, col)? {
        o.ca_file = Some(v);
    }
    if let Some(v) = opt_str(pairs, "مجلد_شهادات", display, line, col)? {
        o.ca_dir = Some(v);
    }
    if let Some(v) = opt_get(pairs, "شهادة_عميل") {
        match v {
            Value::Dict(d) => {
                let c = d.iter().find(|(k, _)| k == "شهادة");
                let k = d.iter().find(|(k, _)| k == "مفتاح");
                match (c, k) {
                    (Some((_, Value::Str(c))), Some((_, Value::Str(k)))) => {
                        o.client_cert = Some((c.clone(), k.clone()));
                    }
                    _ => return Err(opt_bad(display, "شهادة_عميل", line, col)),
                }
            }
            _ => return Err(opt_bad(display, "شهادة_عميل", line, col)),
        }
    }
    if let Some(v) = opt_bool(pairs, "منع_بيانات_الدخول", display, line, col)? {
        o.no_userinfo = v;
    }
    if let Some(v) = opt_bool(pairs, "تجاهل_كوكيز_الجلسة", display, line, col)? {
        o.ignore_session_cookies = v;
    }
    if let Some(v) = opt_get(pairs, "مصادقة") {
        match v {
            Value::Dict(d) => {
                let kind = d
                    .iter()
                    .find(|(k, _)| k == "نوع")
                    .and_then(|(_, x)| match x {
                        Value::Str(s) => Some(s.clone()),
                        _ => None,
                    });
                let gs = |k: &str| {
                    d.iter().find(|(kk, _)| kk == k).and_then(|(_, x)| match x {
                        Value::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                };
                match kind.as_deref() {
                    Some("أساسية") => match (gs("مستخدم"), gs("كلمة_سر")) {
                        (Some(u), Some(p)) => o.auth = Some(AuthKind::Basic(u, p)),
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    Some("هضمية") => match (gs("مستخدم"), gs("كلمة_سر")) {
                        (Some(u), Some(p)) => o.auth = Some(AuthKind::Digest(u, p)),
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    Some("تفاوضية") => match (gs("مستخدم"), gs("كلمة_سر")) {
                        (Some(u), Some(p)) => o.auth = Some(AuthKind::Negotiate(u, p)),
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    Some("حامل") => match gs("رمز") {
                        Some(t) => o.auth = Some(AuthKind::Bearer(t)),
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    Some("أمازون") => match (gs("مفتاح"), gs("سر")) {
                        (Some(k), Some(s)) => {
                            o.auth = Some(AuthKind::Amazon {
                                key: k,
                                secret: s,
                                region: gs("منطقة").unwrap_or_else(|| "us-east-1".to_string()),
                                service: gs("خدمة").unwrap_or_else(|| "s3".to_string()),
                            })
                        }
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    Some("توقيع") => match (gs("معرف"), gs("مفتاح")) {
                        (Some(id), Some(k)) => {
                            o.auth = Some(AuthKind::HttpSig {
                                key_id: id,
                                key: k,
                                algorithm: gs("خوارزمية")
                                    .unwrap_or_else(|| "hmac-sha256".to_string()),
                            })
                        }
                        _ => return Err(opt_bad(display, "مصادقة", line, col)),
                    },
                    _ => return Err(opt_bad(display, "مصادقة", line, col)),
                }
            }
            _ => return Err(opt_bad(display, "مصادقة", line, col)),
        }
    }
    if o.retry_forever && o.max_retry_ms == 0 {
        return Err(RioError::runtime(
            "إعادة المحاولة بلا نهاية تتطلب أقصى زمن للمحاولات",
            line,
            col,
        ));
    }
    Ok(o)
}

// ----- لينكس8: كتابة مخنوقة + مصرف مقاس -----

#[cfg(not(target_arch = "wasm32"))]
fn write_throttled<W: std::io::Write>(
    w: &mut W,
    data: &[u8],
    limit: Option<u64>,
    host: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    use std::io::Write;
    let mut send = |chunk: &[u8]| -> RioResult<()> {
        w.write_all(chunk).map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة إرسال الطلب",
                &format!("تعذر إرسال الطلب إلى {}", host),
                line,
                col,
            )
        })
    };
    match limit {
        None => send(data),
        Some(bps) => {
            let start = std::time::Instant::now();
            let mut sent: u64 = 0;
            for chunk in data.chunks(8192) {
                send(chunk)?;
                sent += chunk.len() as u64;
                let expected =
                    std::time::Duration::from_secs_f64(sent as f64 / bps as f64);
                let elapsed = start.elapsed();
                if expected > elapsed {
                    let mut rest = expected - elapsed;
                    while !rest.is_zero() {
                        let step = rest.min(std::time::Duration::from_millis(200));
                        std::thread::sleep(step);
                        check_cancelled(line, col)?;
                        rest = expected.saturating_sub(start.elapsed());
                    }
                }
            }
            Ok(())
        }
    }
}

/// مصرف يغلّف مصرفا آخر ويفرض: مهلة النقل (فجوة بين
/// دفعتين)، حد سرعة التنزيل، حد الحجم. بلا حدود يعبر
/// كما هو — فيمر جلب/نزّل العاديان بلا تغيير سلوك.
#[cfg(not(target_arch = "wasm32"))]
struct MeteredSink {
    inner: Box<dyn BodySink + Send>,
    stall_secs: u64,
    max_speed: Option<u64>,
    max_size: Option<u64>,
    started: std::time::Instant,
    last_push: Option<std::time::Instant>,
    total: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl MeteredSink {
    fn new(
        inner: Box<dyn BodySink + Send>,
        stall_secs: u64,
        max_speed: Option<u64>,
        max_size: Option<u64>,
        line: usize,
        col: usize,
    ) -> Self {
        MeteredSink {
            inner,
            stall_secs,
            max_speed,
            max_size,
            started: std::time::Instant::now(),
            last_push: None,
            total: 0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BodySink for MeteredSink {
    fn push(&mut self, chunk: &[u8], line: usize, col: usize) -> RioResult<()> {
        if self.stall_secs > 0 {
            if let Some(last) = self.last_push {
                if last.elapsed().as_secs() >= self.stall_secs {
                    return Err(RioError::runtime(
                        "انتهت مهلة النقل — توقف تدفق البيانات",
                        line,
                        col,
                    ));
                }
            }
        }
        if let Some(cap) = self.max_size {
            if self.total.saturating_add(chunk.len() as u64) > cap {
                return Err(RioError::runtime(
                    format!("تجاوز حد الحجم ({})", cap),
                    line,
                    col,
                ));
            }
        }
        if let Some(bps) = self.max_speed {
            let expected = std::time::Duration::from_secs_f64(
                (self.total + chunk.len() as u64) as f64 / bps as f64,
            );
            let elapsed = self.started.elapsed();
            if expected > elapsed {
                let mut rest = expected - elapsed;
                while !rest.is_zero() {
                    let step = rest.min(std::time::Duration::from_millis(200));
                    std::thread::sleep(step);
                    check_cancelled(line, col)?;
                    rest = expected.saturating_sub(self.started.elapsed());
                }
            }
        }
        self.inner.push(chunk, line, col)?;
        self.total += chunk.len() as u64;
        self.last_push = Some(std::time::Instant::now());
        Ok(())
    }

    fn reset(&mut self, line: usize, col: usize) -> RioResult<()> {
        self.inner.reset(line, col)?;
        self.total = 0;
        self.started = std::time::Instant::now();
        self.last_push = None;
        Ok(())
    }

    fn set_meta(&mut self, status: u16, headers: Vec<(String, String)>) {
        self.inner.set_meta(status, headers);
    }

    fn meta(&self) -> Option<(u16, Vec<(String, String)>)> {
        self.inner.meta()
    }

    fn finish_string(self: Box<Self>) -> Option<String> {
        self.inner.finish_string()
    }

    fn finish_bytes(&self) -> u64 {
        self.inner.finish_bytes()
    }

    fn finish_raw(self: Box<Self>) -> Vec<u8> {
        self.inner.finish_raw()
    }
}

// ----- لينكس8: فك ضغط DEFLATE/gzip (Rust خالص) + تواريخ HTTP -----

#[cfg(not(target_arch = "wasm32"))]
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// قارئ بتات (الأقل أولا — RFC 1951).
#[cfg(not(target_arch = "wasm32"))]
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> BitReader<'a> {
    fn bits(&mut self, n: u32) -> RioResult<u32> {
        let mut v: u32 = 0;
        for i in 0..n {
            let byte = self.pos / 8;
            if byte >= self.data.len() {
                return Err(RioError::runtime(
                    "بيانات مضغوطة غير صالحة",
                    0,
                    0,
                ));
            }
            let bit = (self.data[byte] >> (self.pos % 8)) & 1;
            v |= (bit as u32) << i;
            self.pos += 1;
        }
        Ok(v)
    }

    fn align_byte(&mut self) {
        self.pos = (self.pos + 7) / 8 * 8;
    }
}

/// جدول هوفمان القانوني (أطوال الشفرات → رموز).
#[cfg(not(target_arch = "wasm32"))]
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

#[cfg(not(target_arch = "wasm32"))]
fn huff_build(lengths: &[u8]) -> Huffman {
    let mut counts = [0u16; 16];
    for &l in lengths {
        counts[l as usize] += 1;
    }
    counts[0] = 0;
    let mut offs = [0u16; 16];
    for i in 1..16 {
        offs[i] = offs[i - 1] + counts[i - 1];
    }
    let mut symbols = vec![0u16; lengths.iter().filter(|&&l| l != 0).count()];
    for (sym, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbols[offs[l as usize] as usize] = sym as u16;
            offs[l as usize] += 1;
        }
    }
    Huffman { counts, symbols }
}

#[cfg(not(target_arch = "wasm32"))]
fn huff_decode(r: &mut BitReader, h: &Huffman) -> RioResult<u16> {
    let mut code: i32 = 0;
    let mut first: i32 = 0;
    let mut index: i32 = 0;
    for len in 1..16 {
        code |= r.bits(1)? as i32;
        let count = h.counts[len] as i32;
        if code - first < count {
            return Ok(h.symbols[(index + (code - first)) as usize]);
        }
        index += count;
        first = (first + count) << 1;
        code <<= 1;
    }
    Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0))
}

#[cfg(not(target_arch = "wasm32"))]
fn inflate(data: &[u8]) -> RioResult<Vec<u8>> {
    const MAX_OUT: usize = 100 * 1024 * 1024;
    let mut r = BitReader { data, pos: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let bfinal = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                r.align_byte();
                let start = r.pos / 8;
                if start + 4 > data.len() {
                    return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
                }
                let len = u16::from_le_bytes([data[start], data[start + 1]]) as usize;
                let nlen = u16::from_le_bytes([data[start + 2], data[start + 3]]) as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
                }
                if start + 4 + len > data.len() || out.len() + len > MAX_OUT {
                    return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
                }
                out.extend_from_slice(&data[start + 4..start + 4 + len]);
                r.pos = (start + 4 + len) * 8;
            }
            1 => {
                let mut lit_lengths = [0u8; 288];
                for (i, l) in lit_lengths.iter_mut().enumerate() {
                    *l = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let lit = huff_build(&lit_lengths);
                let dist_lengths = [5u8; 30];
                let dist = huff_build(&dist_lengths);
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            2 => {
                let hlit = r.bits(5)? as usize + 257;
                let hdist = r.bits(5)? as usize + 1;
                let hclen = r.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl_lengths = [0u8; 19];
                for i in 0..hclen {
                    cl_lengths[ORDER[i]] = r.bits(3)? as u8;
                }
                let cl = huff_build(&cl_lengths);
                let mut lengths = vec![0u8; hlit + hdist];
                let mut i = 0;
                while i < hlit + hdist {
                    let sym = huff_decode(&mut r, &cl)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return Err(RioError::runtime(
                                    "بيانات مضغوطة غير صالحة",
                                    0,
                                    0,
                                ));
                            }
                            let prev = lengths[i - 1];
                            let rep = 3 + r.bits(2)? as usize;
                            if i + rep > hlit + hdist {
                                return Err(RioError::runtime(
                                    "بيانات مضغوطة غير صالحة",
                                    0,
                                    0,
                                ));
                            }
                            for _ in 0..rep {
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            let rep = 3 + r.bits(3)? as usize;
                            if i + rep > hlit + hdist {
                                return Err(RioError::runtime(
                                    "بيانات مضغوطة غير صالحة",
                                    0,
                                    0,
                                ));
                            }
                            i += rep;
                        }
                        18 => {
                            let rep = 11 + r.bits(7)? as usize;
                            if i + rep > hlit + hdist {
                                return Err(RioError::runtime(
                                    "بيانات مضغوطة غير صالحة",
                                    0,
                                    0,
                                ));
                            }
                            i += rep;
                        }
                        _ => {
                            return Err(RioError::runtime(
                                "بيانات مضغوطة غير صالحة",
                                0,
                                0,
                            ))
                        }
                    }
                }
                if lengths[256] == 0 {
                    return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
                }
                let lit = huff_build(&lengths[..hlit]);
                let dist = huff_build(&lengths[hlit..]);
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            _ => return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0)),
        }
        if out.len() > MAX_OUT {
            return Err(RioError::runtime(
                "البيانات المفكوك ضغطها أكبر من الحد (100 ميغابايت)",
                0,
                0,
            ));
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn inflate_block(
    r: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> RioResult<()> {
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
        115, 131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
        1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
        12, 13, 13,
    ];
    loop {
        let sym = huff_decode(r, lit)?;
        if sym == 256 {
            return Ok(());
        }
        if sym < 256 {
            out.push(sym as u8);
        } else if sym <= 285 {
            let li = (sym - 257) as usize;
            let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li])? as usize;
            let dsym = huff_decode(r, dist)?;
            if dsym > 29 {
                return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
            }
            let di = dsym as usize;
            let d = DIST_BASE[di] as usize + r.bits(DIST_EXTRA[di])? as usize;
            if d == 0 || d > out.len() {
                return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
            }
            let start = out.len() - d;
            for i in 0..len {
                let b = out[start + i];
                out.push(b);
            }
        } else {
            return Err(RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0));
        }
        if out.len() > 100 * 1024 * 1024 {
            return Err(RioError::runtime(
                "البيانات المفكوك ضغطها أكبر من الحد (100 ميغابايت)",
                0,
                0,
            ));
        }
    }
}

/// فك gzip كامل (رأس + DEFLATE + تحقق CRC32/الحجم).
#[cfg(not(target_arch = "wasm32"))]
fn gunzip(data: &[u8]) -> RioResult<Vec<u8>> {
    let bad = || RioError::runtime("بيانات مضغوطة غير صالحة", 0, 0);
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err(bad());
    }
    let flg = data[3];
    let mut off = 10usize;
    if flg & 0x04 != 0 {
        if off + 2 > data.len() {
            return Err(bad());
        }
        let xlen = u16::from_le_bytes([data[off], data[off + 1]]) as usize;
        off += 2 + xlen;
        if off > data.len() {
            return Err(bad());
        }
    }
    if flg & 0x08 != 0 {
        while off < data.len() && data[off] != 0 {
            off += 1;
        }
        off += 1;
    }
    if flg & 0x10 != 0 {
        while off < data.len() && data[off] != 0 {
            off += 1;
        }
        off += 1;
    }
    if flg & 0x02 != 0 {
        off += 2;
    }
    if off + 8 > data.len() {
        return Err(bad());
    }
    let out = inflate(&data[off..data.len() - 8])?;
    let crc = u32::from_le_bytes([
        data[data.len() - 8],
        data[data.len() - 7],
        data[data.len() - 6],
        data[data.len() - 5],
    ]);
    let isize = u32::from_le_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if crc32_ieee(&out) != crc || out.len() as u32 != isize {
        return Err(RioError::runtime("فحص gzip غير مطابق", 0, 0));
    }
    Ok(out)
}

// ----- تواريخ HTTP (IMF-fixdate) -----

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn http_date_from_unix(secs: u64) -> String {
    const WEEK: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTH: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    let wd = WEEK[((days + 4).rem_euclid(7)) as usize];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        wd,
        d,
        MONTH[(m - 1) as usize],
        y,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// قراءة IMF-fixdate ("Wed, 21 Oct 2015 07:28:00 GMT").
#[cfg(not(target_arch = "wasm32"))]
fn parse_http_date(s: &str) -> Option<std::time::SystemTime> {
    const MONTH: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let p: Vec<&str> = s.split_whitespace().collect();
    if p.len() != 6 || p[5] != "GMT" {
        return None;
    }
    let d: u32 = p[1].parse().ok()?;
    let m = MONTH.iter().position(|&x| x == p[2])? as u32 + 1;
    let y: i64 = p[3].parse().ok()?;
    let t: Vec<&str> = p[4].split(':').collect();
    if t.len() != 3 {
        return None;
    }
    let hh: u64 = t[0].parse().ok()?;
    let mm: u64 = t[1].parse().ok()?;
    let ss: u64 = t[2].parse().ok()?;
    if hh > 23 || mm > 59 || ss > 60 || d == 0 || d > 31 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    let secs = days * 86400 + (hh * 3600 + mm * 60 + ss) as i64;
    if secs < 0 {
        return None;
    }
    Some(
        std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(secs as u64),
    )
}

// ----- لينكس8: المنفذ execute_request -----

#[cfg(not(target_arch = "wasm32"))]
fn sleep_retry(delay_ms: u64, line: usize, col: usize) -> RioResult<()> {
    let mut rest = std::time::Duration::from_millis(delay_ms);
    while !rest.is_zero() {
        let step = rest.min(std::time::Duration::from_millis(200));
        std::thread::sleep(step);
        check_cancelled(line, col)?;
        rest = rest.saturating_sub(step);
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn retryable_error(e: &RioError) -> bool {
    let m = format!("{}", e);
    m.contains("تعذر الاتصال")
        || m.contains("مهلة")
        || m.contains("تعذر حل اسم")
        || m.contains("تعذر قراءة")
        || m.contains("انقطع")
        || m.contains("رد غير صالح")
}

/// تنسيق أمازون للتاريخ (YYYYMMDD + YYYYMMDDTHHMMSSZ).
fn amz_datetime(secs: u64) -> (String, String) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    (
        format!("{:04}{:02}{:02}", y, m, d),
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            y,
            m,
            d,
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        ),
    )
}

/// رد المصادقة الهضمية MD5 من تحدٍ (WWW-Authenticate).
#[cfg(not(target_arch = "wasm32"))]
fn digest_response(
    challenge: &str,
    user: &str,
    pass: &str,
    method: &str,
    uri: &str,
) -> Option<String> {
    let get = |key: &str| -> Option<String> {
        let pat = format!("{}=", key);
        let i = challenge.find(&pat)?;
        let rest = &challenge[i + pat.len()..];
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"')?;
            Some(stripped[..end].to_string())
        } else {
            let end = rest.find(|c| c == ',' || c == ' ').unwrap_or(rest.len());
            Some(rest[..end].trim().to_string())
        }
    };
    let realm = get("realm")?;
    let nonce = get("nonce")?;
    let algorithm = get("algorithm").unwrap_or_else(|| "MD5".to_string());
    if algorithm.to_ascii_uppercase() != "MD5" {
        return None;
    }
    let qop_full = get("qop").unwrap_or_default();
    let qop = qop_full.split(',').next().unwrap_or("").trim();
    let opaque = get("opaque");
    let ha1 = crate::crypto::md5_hex(format!("{}:{}:{}", user, realm, pass).as_bytes());
    let ha2 = crate::crypto::md5_hex(format!("{}:{}", method, uri).as_bytes());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let cnonce = format!("{:x}", nanos);
    let nc = "00000001";
    let response = if qop == "auth" {
        crate::crypto::md5_hex(
            format!("{}:{}:{}:{}:{}:{}", ha1, nonce, nc, cnonce, qop, ha2).as_bytes(),
        )
    } else {
        crate::crypto::md5_hex(format!("{}:{}:{}", ha1, nonce, ha2).as_bytes())
    };
    let mut out = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\"",
        user, realm, nonce, uri, response
    );
    if qop == "auth" {
        out.push_str(&format!(
            ", algorithm=MD5, qop=auth, nc={}, cnonce=\"{}\"",
            nc, cnonce
        ));
    } else {
        out.push_str(", algorithm=MD5");
    }
    if let Some(o) = opaque {
        out.push_str(&format!(", opaque=\"{}\"", o));
    }
    Some(out)
}

#[cfg(not(target_arch = "wasm32"))]
struct SaveCtx {
    resolved: std::path::PathBuf,
    display: String,
    base: std::path::PathBuf,
    limit: u64,
    resume_from: u64,
}

#[cfg(not(target_arch = "wasm32"))]
fn execute_request_inner(
    url: &str,
    method: &str,
    body: &[u8],
    content_type: &str,
    headers: &[(String, String)],
    opts: &ExecOpts,
    save: Option<(&crate::permissions::FilePolicy, &std::path::Path, &str)>,
    line: usize,
    col: usize,
) -> RioResult<NetResponse> {
    // بيانات دخول في العنوان — تُستعمل مصادقة أساسية
    // فقط إن سمح الخيار (الافتراضي: منع — فيرفضها
    // محلل العناوين برسالة «رابط غير صالح»).
    let mut url_owned = url.to_string();
    let mut userinfo_auth: Option<(String, String)> = None;
    if !opts.no_userinfo {
        if let Some(sep) = url_owned.find("://") {
            let after = &url_owned[sep + 3..];
            let auth_end = after
                .find(|c| c == '/' || c == '?' || c == '#')
                .unwrap_or(after.len());
            let authority = &after[..auth_end];
            if let Some(at) = authority.rfind('@') {
                let userinfo = &authority[..at];
                let rest_auth = &authority[at + 1..];
                let (u, p) = match userinfo.split_once(':') {
                    Some((u, p)) => (u.to_string(), p.to_string()),
                    None => (userinfo.to_string(), String::new()),
                };
                userinfo_auth = Some((u, p));
                url_owned = format!(
                    "{}://{}{}",
                    &url_owned[..sep],
                    rest_auth,
                    &after[auth_end..]
                );
            }
        }
    }
    let parsed = parse_url(&url_owned, line, col)?;
    if parsed.scheme == "http" && !opts.allow_http && !local_host_allowed(&parsed.host) {
        return Err(RioError::runtime(
            "المخطط http غير مسموح — استخدم https أو فعّل اسمح_http",
            line,
            col,
        ));
    }
    validate_caller_headers(headers, line, col)?;
    let mut eff: Vec<(String, String)> = headers.to_vec();
    if opts.auth.is_none() {
        if let Some((u, p)) = &userinfo_auth {
            eff.push((
                "Authorization".to_string(),
                format!("Basic {}", b64_encode(format!("{}:{}", u, p).as_bytes())),
            ));
        }
    }
    // توقيع أمازون / توقيع HTTP — قبل الحلقة (التاريخ
    // صالح 15 دقيقة فيمضي مع المحاولات).
    if let Some(AuthKind::Amazon {
        key,
        secret,
        region,
        service,
    }) = &opts.auth
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (datestamp, amzdate) = amz_datetime(now);
        let payload_hash = crate::crypto::sha256_hex(body);
        let (uri, query) = match parsed.path.split_once('?') {
            Some((u, q)) => (u.to_string(), q.to_string()),
            None => (parsed.path.clone(), String::new()),
        };
        let host_hdr = if (parsed.scheme == "http" && parsed.port == 80)
            || (parsed.scheme == "https" && parsed.port == 443)
        {
            parsed.host.clone()
        } else {
            format!("{}:{}", parsed.host, parsed.port)
        };
        let canonical_headers = format!(
            "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            host_hdr, payload_hash, amzdate
        );
        let canonical_request = format!(
            "{}\n{}\n{}\n{}host;x-amz-content-sha256;x-amz-date\n{}",
            method.to_ascii_uppercase(),
            uri,
            query,
            canonical_headers,
            payload_hash
        );
        let scope = format!("{}/{}/{}/aws4_request", datestamp, region, service);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amzdate,
            scope,
            crate::crypto::sha256_hex(canonical_request.as_bytes())
        );
        let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), datestamp.as_bytes());
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, service.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex_bytes(&hmac_sha256(&k_signing, to_sign.as_bytes()));
        eff.push(("x-amz-date".to_string(), amzdate));
        eff.push(("x-amz-content-sha256".to_string(), payload_hash));
        eff.push((
            "Authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",
                key, scope, signature
            ),
        ));
    }
    if let Some(AuthKind::HttpSig {
        key_id,
        key,
        algorithm,
    }) = &opts.auth
    {
        if algorithm != "hmac-sha256" {
            return Err(RioError::runtime(
                "خوارزمية التوقيع غير مدعومة — المتاح hmac-sha256 فقط",
                line,
                col,
            ));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let date = http_date_from_unix(now);
        let host_hdr = if (parsed.scheme == "http" && parsed.port == 80)
            || (parsed.scheme == "https" && parsed.port == 443)
        {
            parsed.host.clone()
        } else {
            format!("{}:{}", parsed.host, parsed.port)
        };
        let target = format!(
            "{} {}",
            method.to_ascii_lowercase(),
            parsed.path
        );
        let signing = format!(
            "(request-target): {}\nhost: {}\ndate: {}",
            target, host_hdr, date
        );
        let sig = b64_encode(&hmac_sha256(key.as_bytes(), signing.as_bytes()));
        eff.push(("date".to_string(), date));
        eff.push((
            "Authorization".to_string(),
            format!(
                "Signature keyId=\"{}\",algorithm=\"hmac-sha256\",headers=\"(request-target) host date\",signature=\"{}\"",
                key_id, sig
            ),
        ));
    }
    match &opts.auth {
        Some(AuthKind::Basic(u, p)) => {
            eff.push((
                "Authorization".to_string(),
                format!("Basic {}", b64_encode(format!("{}:{}", u, p).as_bytes())),
            ));
        }
        Some(AuthKind::Bearer(t)) => {
            eff.push(("Authorization".to_string(), format!("Bearer {}", t)));
        }
        _ => {}
    }
    // إعداد الحفظ (قبل الخيط — فالحل خارج الخيط).
    let save_ctx: Option<SaveCtx> = match save {
        None => None,
        Some((policy, cwd, path_str)) => {
            let resolved = policy.resolve_in_for_write(cwd, path_str, line, col)?;
            let display = policy.display_path(&resolved);
            let base = policy.base.clone();
            let existing = std::fs::metadata(&resolved).map(|m| m.len()).ok();
            if opts.no_overwrite && existing.is_some() {
                return Err(RioError::runtime(
                    "الملف موجود — الكتابة مرفوضة",
                    line,
                    col,
                ));
            }
            if opts.skip_existing {
                if let Some(size) = existing {
                    return Ok(NetResponse {
                        status: 0,
                        body: Vec::new(),
                        headers: Vec::new(),
                        bytes: size,
                    });
                }
            }
            let resume_from = if opts.resume { existing.unwrap_or(0) } else { 0 };
            let mut budget = 10000usize;
            let used = crate::interpreter::Interpreter::dir_size(&base, &mut budget);
            let limit = crate::interpreter::Interpreter::DISK_QUOTA_BYTES
                .saturating_sub(used)
                .saturating_add(existing.unwrap_or(0));
            Some(SaveCtx {
                resolved,
                display,
                base,
                limit,
                resume_from,
            })
        }
    };
    // المدى / الشرط الزمني / وسم الكيان / الكوكيز.
    match (opts.range_from, opts.range_to) {
        (Some(f), Some(t)) => {
            eff.push(("Range".to_string(), format!("bytes={}-{}", f, t)));
        }
        (Some(f), None) => {
            eff.push(("Range".to_string(), format!("bytes={}-", f)));
        }
        (None, _) => {
            if let Some(sc) = &save_ctx {
                if sc.resume_from > 0 {
                    eff.push((
                        "Range".to_string(),
                        format!("bytes={}-", sc.resume_from),
                    ));
                }
            }
        }
    }
    if let Some(e) = &opts.etag {
        eff.push(("If-None-Match".to_string(), e.clone()));
    }
    if let Some(s) = &opts.if_modified_since {
        eff.push(("If-Modified-Since".to_string(), s.clone()));
    }
    let mut cookie_parts: Vec<String> = Vec::new();
    if !opts.ignore_session_cookies {
        for (k, v) in &opts.session_cookies {
            cookie_parts.push(format!("{}={}", k, v));
        }
    }
    for (k, v) in &opts.cookies {
        cookie_parts.push(format!("{}={}", k, v));
    }
    if !cookie_parts.is_empty() {
        eff.push(("Cookie".to_string(), cookie_parts.join("; ")));
    }
    let saved_path = save_ctx.as_ref().map(|s| s.resolved.clone());
    let delete_on_fail = opts.delete_on_fail;
    // لينكس8-طبقة1-أغلفة: صامت يلغي التقدم والتتبع،
    // وأخطاء-فقط تلغي التقدم؛ والمفصل يطبع الطلب والرد.
    let show_progress = opts.show_progress && !opts.silent && !opts.errors_only;
    let trace_on = !opts.silent && opts.trace_text;
    let verbose = opts.verbose;
    let trace_file = opts.trace_file.clone();
    let preserve_mtime = opts.preserve_mtime;
    let url2 = url_owned.clone();
    let path2 = parsed.path.clone();
    let method2 = method.to_string();
    let body2 = body.to_vec();
    let ct2 = content_type.to_string();
    let eff2 = eff.clone();
    let mut opts2 = opts.clone();
    opts2.return_all_status = true;
    if show_progress {
        eprintln!("جلب {} ...", url2);
    }
    // لينكس8-طبقة1-نواة: نسخ للاستعمال بعد الإغلاق
    // (فالإغلاق ينقل url2/method2).
    let url3 = url2.clone();
    let method3 = method2.clone();
    let result = run_cancellable(
        move || {
            let _write_guard = crate::files::FILE_WRITE_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let start = std::time::Instant::now();
            let total_attempts: u64 = if opts2.retry_forever {
                u64::MAX
            } else {
                opts2.retries as u64
            };
            let mut attempt: u64 = 0;
            let mut last_err: Option<RioError> = None;
            let mut digest_auth: Option<String> = None;
            loop {
                attempt += 1;
                check_cancelled(line, col)?;
                let (sink, is_file): (Box<dyn BodySink + Send>, bool) = match &save_ctx {
                    Some(sc) => {
                        let file = if sc.resume_from > 0 {
                            let f = std::fs::OpenOptions::new()
                                .write(true)
                                .create(true)
                                .append(true)
                                .open(&sc.resolved)
                                .map_err(|_| {
                                    RioError::new(
                                        crate::error::ErrorKind::Io,
                                        format!("تعذر كتابة الملف {}", sc.display),
                                        line,
                                        col,
                                    )
                                })?;
                            let _ = f.set_len(sc.resume_from);
                            f
                        } else {
                            std::fs::File::create(&sc.resolved).map_err(|_| {
                                RioError::new(
                                    crate::error::ErrorKind::Io,
                                    format!("تعذر كتابة الملف {}", sc.display),
                                    line,
                                    col,
                                )
                            })?
                        };
                        let written = if sc.resume_from > 0 {
                            sc.resume_from
                        } else {
                            0
                        };
                        (
                            Box::new(FileSink {
                                file,
                                written,
                                limit: sc.limit,
                                display: sc.display.clone(),
                                meta: None,
                            }),
                            true,
                        )
                    }
                    None => (Box::new(MemorySink::new()), false),
                };
                let mut headers_try = eff2.clone();
                if let Some(d) = &digest_auth {
                    headers_try.push(("Authorization".to_string(), d.clone()));
                }
                if verbose {
                    eprintln!("{} {}", method2, url2);
                }
                let freq =
                    FetchRequest::with_method(&method2, &body2, &ct2, &headers_try, opts2.clone());
                let outcome = fetch_loop(&url2, &freq, opts2.timeout_secs, line, col, sink);
                match outcome {
                    Ok(sink) => {
                        let (status, resp_headers) =
                            sink.meta().unwrap_or((0, Vec::new()));
                        if verbose {
                            eprintln!("{} {} -> {}", method2, url2, status);
                            for (n, v) in &resp_headers {
                                eprintln!("  {}: {}", n, v);
                            }
                        }
                        if status == 401 && digest_auth.is_none() {
                            if let Some(AuthKind::Digest(u, p)) = &opts2.auth {
                                if let Some(ch) = resp_headers
                                    .iter()
                                    .find(|(n, _)| n == "www-authenticate")
                                    .map(|(_, v)| v.clone())
                                {
                                    if let Some(h) = digest_response(
                                        &ch, u, p, &method2, &path2,
                                    ) {
                                        digest_auth = Some(h);
                                        continue;
                                    }
                                }
                            }
                            if let Some(AuthKind::Negotiate(..)) = &opts2.auth {
                                return Err(RioError::runtime(
                                    "المصادقة التفاوضية غير مدعومة في هذا البناء",
                                    line,
                                    col,
                                ));
                            }
                        }
                        if (status == 429 || (500..600).contains(&status))
                            && attempt < total_attempts
                        {
                            let over_time = !opts2.retry_forever
                                && opts2.max_retry_ms > 0
                                && start.elapsed().as_millis() as u64 >= opts2.max_retry_ms;
                            if !over_time {
                                sleep_retry(opts2.retry_delay_ms, line, col)?;
                                continue;
                            }
                        }
                        if opts2.check_status && !(200..300).contains(&status) {
                            return Err(RioError::runtime(
                                format!("الخادم رد برمز الحالة {}", status),
                                line,
                                col,
                            ));
                        }
                        let is_gzip = opts2.gzip
                            && resp_headers.iter().any(|(n, v)| {
                                n == "content-encoding"
                                    && v.to_ascii_lowercase().contains("gzip")
                            });
                        let bytes = sink.finish_bytes();
                        if is_file {
                            let mut nbytes = bytes;
                            if is_gzip {
                                if let Some(sc) = &save_ctx {
                                    if let Ok(raw) = std::fs::read(&sc.resolved) {
                                        if let Ok(dec) = gunzip(&raw) {
                                            nbytes = dec.len() as u64;
                                            let _ = std::fs::write(&sc.resolved, &dec);
                                        }
                                    }
                                }
                            }
                            return Ok(NetResponse {
                                status,
                                body: Vec::new(),
                                headers: resp_headers,
                                bytes: nbytes,
                            });
                        }
                        let raw = sink.finish_raw();
                        let raw = if is_gzip { gunzip(&raw)? } else { raw };
                        let nbytes = raw.len() as u64;
                        return Ok(NetResponse {
                            status,
                            body: raw,
                            headers: resp_headers,
                            bytes: nbytes,
                        });
                    }
                    Err(e) => {
                        let msg = format!("{}", e);
                        let is_refused = msg.contains("تعذر الاتصال");
                        if is_refused && !opts2.retry_on_refused {
                            return Err(e);
                        }
                        if !retryable_error(&e) {
                            return Err(e);
                        }
                        last_err = Some(e);
                        if attempt >= total_attempts {
                            break;
                        }
                        if !opts2.retry_forever
                            && opts2.max_retry_ms > 0
                            && start.elapsed().as_millis() as u64 >= opts2.max_retry_ms
                        {
                            break;
                        }
                        sleep_retry(opts2.retry_delay_ms, line, col)?;
                        continue;
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| {
                RioError::runtime("فشل الطلب", line, col)
            }))
        },
        line,
        col,
    );
    match result {
        Ok(resp) => {
            if show_progress {
                eprintln!("اكتمل {} — رمز {} ({} بايت)", url3, resp.status, resp.bytes);
            }
            if trace_on {
            if let Some(path) = &trace_file {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "{} {} -> {} ({} بايت)",
                        method3, url3, resp.status, resp.bytes
                    );
                }
            }
            }
            if preserve_mtime {
                if let Some(sp) = &saved_path {
                    if let Some((_, lm)) = resp
                        .headers
                        .iter()
                        .find(|(n, _)| n == "last-modified")
                    {
                        if let Some(t) = parse_http_date(lm) {
                            if let Ok(f) = std::fs::File::options().write(true).open(sp) {
                                let _ = f.set_times(
                                    std::fs::FileTimes::new().set_modified(t),
                                );
                            }
                        }
                    }
                }
            }
            Ok(resp)
        }
        Err(e) => {
            if delete_on_fail {
                if let Some(sp) = &saved_path {
                    let _ = std::fs::remove_file(sp);
                }
            }
            Err(e)
        }
    }
}

/// منفذ الطلب الموحد: طلب مبني + خيارات + حفظ اختياري.
/// يُرجع الرد كاملا (الرمز والجسم والرؤوس والبايتات).
pub fn execute_request(
    url: &str,
    method: &str,
    body: &[u8],
    content_type: &str,
    headers: &[(String, String)],
    opts: &ExecOpts,
    save: Option<(&crate::permissions::FilePolicy, &std::path::Path, &str)>,
    line: usize,
    col: usize,
) -> RioResult<NetResponse> {
    if !network_allowed() {
        return Err(RioError::runtime(
            "الشبكة مغلقة افتراضيا — استخدم --اسمح-بالشبكة للسماح بالشبكة، أو وافق عند أول استخدام في الكونسول",
            line,
            col,
        ));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (url, method, body, content_type, headers, opts, save);
        Err(RioError::runtime(
            "هذه الدالة غير مدعومة في متصفح الويب",
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        execute_request_inner(
            url,
            method,
            body,
            content_type,
            headers,
            opts,
            save,
            line,
            col,
        )
    }
}

// ----- لينكس8-وكيل: اتصال مباشر أو عبر وكيل -----

#[cfg(not(target_arch = "wasm32"))]
fn tcp_connect(
    addr: &std::net::SocketAddr,
    host: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<std::net::TcpStream> {
    let timeout = time_left(deadline, line, col)?;
    let stream = std::net::TcpStream::connect_timeout(addr, timeout).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col)
        } else {
            RioError::runtime(format!("تعذر الاتصال بالمضيف {}", host), line, col)
        }
    })?;
    let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
    let _ = stream.set_write_timeout(Some(time_left(deadline, line, col)?));
    Ok(stream)
}

/// حل اسم + فحص IP (§7) لمضيف ما — رسائل المباشر نفسها.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_and_check(
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

/// "مضيف:منفذ" أو "مخطط://مضيف:منفذ" (المنفذ افتراضي).
#[cfg(not(target_arch = "wasm32"))]
fn parse_proxy_hostport(spec: &str, default_port: u16) -> Option<(String, u16)> {
    let s = spec.trim();
    let s = match s.split_once("://") {
        Some((_, rest)) => rest,
        None => s,
    };
    let s = s.split('/').next().unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    if let Some(bracketed) = s.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        let port = after.strip_prefix(':').and_then(|p| p.parse::<u16>().ok()).unwrap_or(default_port);
        if host.is_empty() {
            return None;
        }
        return Some((host.to_string(), port));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => match p.parse::<u16>() {
            Ok(port) if port != 0 => Some((h.to_string(), port)),
            _ => None,
        },
        _ => Some((s.to_string(), default_port)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_until_headers(
    stream: &mut std::net::TcpStream,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<Vec<u8>> {
    use std::io::Read;
    let mut head: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while head.len() < 65536 {
        let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                head.push(byte[0]);
                check_cancelled(line, col)?;
                if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err(RioError::runtime(
                        "انتهت مهلة قراءة الرد (30 ثانية)",
                        line,
                        col,
                    ));
                }
                continue;
            }
            Err(_) => {
                return Err(RioError::runtime(
                    "تعذر قراءة الرد من الخادم",
                    line,
                    col,
                ))
            }
        }
    }
    Ok(head)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_exact_timeout(
    stream: &mut std::net::TcpStream,
    buf: &mut [u8],
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<()> {
    use std::io::Read;
    let _ = stream.set_read_timeout(Some(time_left(deadline, line, col)?));
    stream.read_exact(buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة قراءة الرد (30 ثانية)", line, col)
        } else {
            RioError::runtime("تعذر قراءة الرد من الوكيل", line, col)
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn http_proxy_connect(
    url: &ParsedUrl,
    spec: &str,
    req: &FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<(std::net::TcpStream, bool)> {
    use std::io::Write;
    let (phost, pport) = parse_proxy_hostport(spec, 8080)
        .ok_or_else(|| RioError::runtime("وكيل غير صالح", line, col))?;
    let paddr = resolve_and_check(&phost, pport, line, col)?;
    let mut stream = tcp_connect(&paddr, &phost, deadline, line, col)?;
    if url.scheme == "https" {
        if !req.opts.tunnel_proxy {
            return Err(RioError::runtime(
                "الوكيل يحتاج نفقا لـ HTTPS — فعّل نفق_الوكيل",
                line,
                col,
            ));
        }
        let mut text = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
            url.host, url.port, url.host, url.port
        );
        if let Some((u, p)) = &req.opts.proxy_auth {
            text.push_str(&format!(
                "Proxy-Authorization: Basic {}\r\n",
                b64_encode(format!("{}:{}", u, p).as_bytes())
            ));
        }
        text.push_str("\r\n");
        stream.write_all(text.as_bytes()).map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة إرسال الطلب",
                &format!("تعذر إرسال الطلب إلى {}", phost),
                line,
                col,
            )
        })?;
        let head = read_until_headers(&mut stream, deadline, line, col)?;
        let text = String::from_utf8_lossy(&head);
        let status: u16 = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if status != 200 {
            return Err(RioError::runtime(
                format!("الوكيل رفض النفق (رمز {})", status),
                line,
                col,
            ));
        }
        Ok((stream, false))
    } else {
        Ok((stream, true))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn socks5_connect(
    url: &ParsedUrl,
    spec: &str,
    req: &FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<std::net::TcpStream> {
    use std::io::Write;
    let (phost, pport) = parse_proxy_hostport(spec, 1080)
        .ok_or_else(|| RioError::runtime("وكيل غير صالح", line, col))?;
    let paddr = resolve_and_check(&phost, pport, line, col)?;
    let mut stream = tcp_connect(&paddr, &phost, deadline, line, col)?;
    let send = |stream: &mut std::net::TcpStream, data: &[u8]| -> RioResult<()> {
        stream.write_all(data).map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة إرسال الطلب",
                &format!("تعذر إرسال الطلب إلى {}", phost),
                line,
                col,
            )
        })
    };
    let greeting: Vec<u8> = if req.opts.proxy_auth.is_some() {
        vec![5, 2, 0, 2]
    } else {
        vec![5, 1, 0]
    };
    send(&mut stream, &greeting)?;
    let mut resp = [0u8; 2];
    read_exact_timeout(&mut stream, &mut resp, deadline, line, col)?;
    if resp[0] != 5 {
        return Err(RioError::runtime("رد غير صالح من الوكيل", line, col));
    }
    if resp[1] == 2 {
        let (u, p) = req.opts.proxy_auth.as_ref().unwrap();
        let mut auth = vec![1u8, u.len() as u8];
        auth.extend_from_slice(u.as_bytes());
        auth.push(p.len() as u8);
        auth.extend_from_slice(p.as_bytes());
        send(&mut stream, &auth)?;
        let mut aresp = [0u8; 2];
        read_exact_timeout(&mut stream, &mut aresp, deadline, line, col)?;
        if aresp[1] != 0 {
            return Err(RioError::runtime("الوكيل رفض المصادقة", line, col));
        }
    } else if resp[1] != 0 {
        return Err(RioError::runtime("الوكيل رفض الاتصال", line, col));
    }
    if url.host.len() > 255 {
        return Err(RioError::runtime("اسم مضيف طويل للوكيل", line, col));
    }
    let mut creq = vec![5u8, 1, 0];
    if let Ok(ip) = url.host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                creq.push(1);
                creq.extend_from_slice(&v4.octets());
            }
            std::net::IpAddr::V6(v6) => {
                creq.push(4);
                creq.extend_from_slice(&v6.octets());
            }
        }
    } else {
        creq.push(3);
        creq.push(url.host.len() as u8);
        creq.extend_from_slice(url.host.as_bytes());
    }
    creq.extend_from_slice(&url.port.to_be_bytes());
    send(&mut stream, &creq)?;
    let mut head4 = [0u8; 4];
    read_exact_timeout(&mut stream, &mut head4, deadline, line, col)?;
    if head4[0] != 5 {
        return Err(RioError::runtime("رد غير صالح من الوكيل", line, col));
    }
    if head4[1] != 0 {
        return Err(RioError::runtime(
            format!("الوكيل SOCKS5 رفض الاتصال (رمز {})", head4[1]),
            line,
            col,
        ));
    }
    let rest = match head4[3] {
        1 => 4 + 2,
        3 => {
            let mut l = [0u8; 1];
            read_exact_timeout(&mut stream, &mut l, deadline, line, col)?;
            l[0] as usize + 2
        }
        4 => 16 + 2,
        _ => return Err(RioError::runtime("رد غير صالح من الوكيل", line, col)),
    };
    let mut tail = vec![0u8; rest];
    read_exact_timeout(&mut stream, &mut tail, deadline, line, col)?;
    Ok(stream)
}

/// الاتصال بالهدف: مباشر، أو عبر وكيل HTTP (نفق لـ HTTPS،
/// وهدف مطلق لـ HTTP)، أو عبر SOCKS5. فحص IP (§7) يسري
/// على الوكيل نفسه عند التوكيل (فالوكيل يحل الهدف).
#[cfg(not(target_arch = "wasm32"))]
fn connect_target(
    url: &ParsedUrl,
    req: &FetchRequest,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<(std::net::TcpStream, bool)> {
    use std::net::ToSocketAddrs;
    let host_l = url.host.to_ascii_lowercase();
    let bypass = req.opts.no_proxy.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        host_l == p
            || (p.starts_with('.') && host_l.ends_with(p.as_str()))
            || host_l.ends_with(&format!(".{}", p))
    });
    if !bypass {
        if let Some(s5) = &req.opts.socks5 {
            let stream = socks5_connect(url, s5, req, deadline, line, col)?;
            return Ok((stream, false));
        }
        if let Some(px) = &req.opts.proxy {
            return http_proxy_connect(url, px, req, deadline, line, col);
        }
    }
    // مباشر (كما كان): DNS مرة واحدة ثم فحص IP ثم الاتصال.
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
    let stream = tcp_connect(&addr, &url.host, deadline, line, col)?;
    Ok((stream, false))
}

// ---------- لينكس8-طبقة3: FTP/FTPS/MQTT (اجتماع 90 §105) ----------
use std::io::{Read, Write};

/// تحليل رابط مضيف: مخطط://[مستخدم[:كلمة]@]مضيف[:منفذ][/مسار].
pub struct HostUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub user: Option<String>,
    pub pass: Option<String>,
}

pub fn parse_host_url(
    url: &str,
    scheme: &str,
    default_port: u16,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<HostUrl> {
    let rest = match url.split_once("://") {
        Some((s, r)) if s == scheme => r,
        _ => {
            return Err(RioError::typ(
                format!("الدالة {} تتوقع رابط {}", display, scheme),
                line,
                col,
            ))
        }
    };
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (rest, "/".to_string()),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, pass) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
            None => (Some(u.to_string()), None),
        },
        None => (None, None),
    };
    let (host, port) = if let Some(bracketed) = hostport.strip_prefix('[') {
        match bracketed.split_once(']') {
            Some((h, rest2)) => {
                let p = match rest2.strip_prefix(':') {
                    Some(ps) if !ps.is_empty() => ps.parse::<u16>().map_err(|_| {
                        RioError::typ(
                            format!("الدالة {} تتوقع منفذا صحيحا", display),
                            line,
                            col,
                        )
                    })?,
                    _ => default_port,
                };
                (h.to_string(), p)
            }
            None => {
                return Err(RioError::typ(
                    format!("الدالة {} تتوقع مضيفا صحيحا", display),
                    line,
                    col,
                ))
            }
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((h, ps)) => {
                if ps.is_empty() {
                    (h.to_string(), default_port)
                } else {
                    match ps.parse::<u16>() {
                        Ok(p) => (h.to_string(), p),
                        Err(_) => {
                            return Err(RioError::typ(
                                format!("الدالة {} تتوقع منفذا صحيحا", display),
                                line,
                                col,
                            ))
                        }
                    }
                }
            }
            None => (hostport.to_string(), default_port),
        }
    };
    if host.is_empty() {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع مضيفا غير فارغ", display),
            line,
            col,
        ));
    }
    Ok(HostUrl {
        host,
        port,
        path,
        user,
        pass,
    })
}

#[cfg(not(target_arch = "wasm32"))]
trait FtpIo: Read + Write {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Read + Write> FtpIo for T {}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_read_line<S: FtpIo>(
    s: &mut S,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut one = [0u8; 1];
    loop {
        let _ = time_left(deadline, line, col)?;
        match s.read(&mut one) {
            Ok(0) => return Err(RioError::runtime("انقطع الاتصال بالخادم", line, col)),
            Ok(_) => {
                if one[0] == b'\n' {
                    break;
                }
                buf.push(one[0]);
                if buf.len() > 4096 {
                    return Err(RioError::runtime("رد طويل جدا من الخادم", line, col));
                }
            }
            Err(e) => {
                return Err(io_timeout_or(
                    e,
                    "انتهت مهلة القراءة من الخادم",
                    "تعذر قراءة الرد من الخادم",
                    line,
                    col,
                ));
            }
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_read_reply<S: FtpIo>(
    s: &mut S,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<(u16, String)> {
    let first = ftp_read_line(s, deadline, line, col)?;
    let code: u16 = first
        .get(0..3)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| RioError::runtime("رد غير صالح من الخادم", line, col))?;
    let mut text = first.clone();
    if first.as_bytes().get(3) == Some(&b'-') {
        loop {
            let l = ftp_read_line(s, deadline, line, col)?;
            text.push('\n');
            text.push_str(&l);
            if l.len() >= 4 && l.as_bytes()[3] == b' ' && l.get(0..3) == Some(&first[0..3]) {
                break;
            }
        }
    }
    Ok((code, text))
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_cmd<S: FtpIo>(
    s: &mut S,
    cmd: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<(u16, String)> {
    let send = |s: &mut S, b: &[u8]| -> RioResult<()> {
        s.write_all(b).map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة الإرسال إلى الخادم",
                "تعذر إرسال الطلب إلى الخادم",
                line,
                col,
            )
        })
    };
    send(s, cmd.as_bytes())?;
    send(s, b"\r\n")?;
    s.flush().map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    ftp_read_reply(s, deadline, line, col)
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_expect(code: u16, want: &[u16], line: usize, col: usize) -> RioResult<()> {
    if want.contains(&code) {
        Ok(())
    } else {
        Err(RioError::runtime(
            format!("خادم FTP رد برمز {}", code),
            line,
            col,
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_parse_pasv(text: &str, line: usize, col: usize) -> RioResult<(String, u16)> {
    let bad = || RioError::runtime("رد غير صالح من الخادم", line, col);
    let start = text.find('(').ok_or_else(bad)?;
    let end = text[start..]
        .find(')')
        .map(|i| start + i)
        .ok_or_else(bad)?;
    let nums: Vec<u16> = text[start + 1..end]
        .split(',')
        .map(|p| p.trim().parse::<u16>())
        .collect::<Result<_, _>>()
        .map_err(|_| bad())?;
    if nums.len() != 6 {
        return Err(bad());
    }
    let host = format!("{}.{}.{}.{}", nums[0], nums[1], nums[2], nums[3]);
    let port = nums[4] * 256 + nums[5];
    Ok((host, port))
}

#[cfg(not(target_arch = "wasm32"))]
enum FtpOp {
    Retr,
    Stor(Vec<u8>),
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_transfer<S: FtpIo, D: FtpIo>(
    ctrl: &mut S,
    user: &str,
    pass: &str,
    path: &str,
    op: FtpOp,
    skip_greeting: bool,
    mut connect_data: impl FnMut(&str, u16, std::time::Instant, usize, usize) -> RioResult<D>,
    mut finish_data: impl FnMut(&mut D, usize, usize) -> RioResult<()>,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<Vec<u8>> {
    if !skip_greeting {
        let (code, _) = ftp_read_reply(ctrl, deadline, line, col)?;
        ftp_expect(code, &[220], line, col)?;
    }
    let (code, _) = ftp_cmd(ctrl, &format!("USER {}", user), deadline, line, col)?;
    if code == 331 {
        let (code, _) = ftp_cmd(ctrl, &format!("PASS {}", pass), deadline, line, col)?;
        ftp_expect(code, &[202, 230], line, col)?;
    } else {
        ftp_expect(code, &[202, 230], line, col)?;
    }
    let (code, _) = ftp_cmd(ctrl, "TYPE I", deadline, line, col)?;
    ftp_expect(code, &[200], line, col)?;
    let (code, text) = ftp_cmd(ctrl, "PASV", deadline, line, col)?;
    ftp_expect(code, &[227], line, col)?;
    let (dhost, dport) = ftp_parse_pasv(&text, line, col)?;
    let mut data = connect_data(&dhost, dport, deadline, line, col)?;
    let (is_retr, up): (bool, Vec<u8>) = match op {
        FtpOp::Retr => (true, Vec::new()),
        FtpOp::Stor(b) => (false, b),
    };
    let cmd = if is_retr {
        format!("RETR {}", path)
    } else {
        format!("STOR {}", path)
    };
    let (code, _) = ftp_cmd(ctrl, &cmd, deadline, line, col)?;
    ftp_expect(code, &[125, 150], line, col)?;
    let out = if is_retr {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let _ = time_left(deadline, line, col)?;
            match data.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    return Err(io_timeout_or(
                        e,
                        "انتهت مهلة القراءة من الخادم",
                        "تعذر قراءة الرد من الخادم",
                        line,
                        col,
                    ));
                }
            }
        }
        buf
    } else {
        data.write_all(&up).map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة الإرسال إلى الخادم",
                "تعذر إرسال الطلب إلى الخادم",
                line,
                col,
            )
        })?;
        data.flush().map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة الإرسال إلى الخادم",
                "تعذر إرسال الطلب إلى الخادم",
                line,
                col,
            )
        })?;
        finish_data(&mut data, line, col)?;
        up
    };
    drop(data);
    let (code, _) = ftp_read_reply(ctrl, deadline, line, col)?;
    ftp_expect(code, &[226, 250], line, col)?;
    let _ = ftp_cmd(ctrl, "QUIT", deadline, line, col);
    Ok(out)
}

pub fn ftp_fetch(url: &str, display: &str, line: usize, col: usize) -> RioResult<Vec<u8>> {
    require_network_allowed(line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (url, display);
        Err(RioError::runtime("FTP غير مدعوم على هذه المنصة", line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ftp_fetch_native(url, display, line, col)
    }
}

pub fn ftp_upload(
    url: &str,
    data: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    require_network_allowed(line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (url, data, display);
        Err(RioError::runtime("FTP غير مدعوم على هذه المنصة", line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ftp_upload_native(url, data, display, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_plain_data(
    dh: &str,
    dp: u16,
    dl: std::time::Instant,
    l: usize,
    c: usize,
) -> RioResult<std::net::TcpStream> {
    let a = resolve_and_check(dh, dp, l, c)?;
    tcp_connect(&a, dh, dl, l, c)
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_fetch_native(url: &str, display: &str, line: usize, col: usize) -> RioResult<Vec<u8>> {
    let u = parse_host_url(url, "ftp", 21, display, line, col)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let addr = resolve_and_check(&u.host, u.port, line, col)?;
    let mut ctrl = tcp_connect(&addr, &u.host, deadline, line, col)?;
    let user = u.user.clone().unwrap_or_else(|| "anonymous".to_string());
    let pass = u.pass.clone().unwrap_or_else(|| "niva@".to_string());
    ftp_transfer(
        &mut ctrl,
        &user,
        &pass,
        &u.path,
        FtpOp::Retr,
        false,
        ftp_plain_data,
        |d: &mut std::net::TcpStream, l, c| {
            d.shutdown(std::net::Shutdown::Write)
                .map_err(|_| RioError::runtime("تعذر إرسال الطلب إلى الخادم", l, c))
        },
        deadline,
        line,
        col,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn ftp_upload_native(
    url: &str,
    data: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    let u = parse_host_url(url, "ftp", 21, display, line, col)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let addr = resolve_and_check(&u.host, u.port, line, col)?;
    let mut ctrl = tcp_connect(&addr, &u.host, deadline, line, col)?;
    let user = u.user.clone().unwrap_or_else(|| "anonymous".to_string());
    let pass = u.pass.clone().unwrap_or_else(|| "niva@".to_string());
    let _ = ftp_transfer(
        &mut ctrl,
        &user,
        &pass,
        &u.path,
        FtpOp::Stor(data.to_vec()),
        false,
        ftp_plain_data,
        |d: &mut std::net::TcpStream, l, c| {
            d.shutdown(std::net::Shutdown::Write)
                .map_err(|_| RioError::runtime("تعذر إرسال الطلب إلى الخادم", l, c))
        },
        deadline,
        line,
        col,
    )?;
    Ok(())
}

pub fn ftps_fetch(url: &str, display: &str, line: usize, col: usize) -> RioResult<Vec<u8>> {
    require_network_allowed(line, col)?;
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    {
        ftps_fetch_native(url, display, line, col)
    }
    #[cfg(any(target_arch = "wasm32", not(feature = "https")))]
    {
        let _ = (url, display);
        Err(RioError::runtime("FTPS غير مدعوم في هذا البناء", line, col))
    }
}

pub fn ftps_upload(
    url: &str,
    data: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    require_network_allowed(line, col)?;
    #[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
    {
        ftps_upload_native(url, data, display, line, col)
    }
    #[cfg(any(target_arch = "wasm32", not(feature = "https")))]
    {
        let _ = (url, data, display);
        Err(RioError::runtime("FTPS غير مدعوم في هذا البناء", line, col))
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn ftps_tls_wrap(
    stream: std::net::TcpStream,
    host: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>> {
    let config = std::sync::Arc::new(tls_client_config());
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| {
            RioError::runtime(format!("اسم مضيف غير صالح لـ FTPS: {}", host), line, col)
        })?;
    let conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|_| RioError::runtime("تعذر بدء اتصال FTPS", line, col))?;
    let mut tls = rustls::StreamOwned::new(conn, stream);
    let left = time_left(deadline, line, col)?;
    let _ = tls.get_mut().set_read_timeout(Some(left));
    let _ = tls.get_mut().set_write_timeout(Some(left));
    Ok(tls)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn ftps_setup(
    url: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<(
    rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>,
    HostUrl,
    std::time::Instant,
)> {
    let u = parse_host_url(url, "ftps", 990, display, line, col)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let addr = resolve_and_check(&u.host, u.port, line, col)?;
    let mut plain = tcp_connect(&addr, &u.host, deadline, line, col)?;
    let (code, _) = ftp_read_reply(&mut plain, deadline, line, col)?;
    ftp_expect(code, &[220], line, col)?;
    let (code, _) = ftp_cmd(&mut plain, "AUTH TLS", deadline, line, col)?;
    ftp_expect(code, &[234], line, col)?;
    let mut ctrl = ftps_tls_wrap(plain, &u.host, deadline, line, col)?;
    let (code, _) = ftp_cmd(&mut ctrl, "PBSZ 0", deadline, line, col)?;
    ftp_expect(code, &[200], line, col)?;
    let (code, _) = ftp_cmd(&mut ctrl, "PROT P", deadline, line, col)?;
    ftp_expect(code, &[200], line, col)?;
    Ok((ctrl, u, deadline))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn ftps_fetch_native(url: &str, display: &str, line: usize, col: usize) -> RioResult<Vec<u8>> {
    let (mut ctrl, u, deadline) = ftps_setup(url, display, line, col)?;
    let user = u.user.clone().unwrap_or_else(|| "anonymous".to_string());
    let pass = u.pass.clone().unwrap_or_else(|| "niva@".to_string());
    ftp_transfer(
        &mut ctrl,
        &user,
        &pass,
        &u.path,
        FtpOp::Retr,
        true,
        |dh, dp, dl, l, c| {
            let a = resolve_and_check(dh, dp, l, c)?;
            let s = tcp_connect(&a, dh, dl, l, c)?;
            ftps_tls_wrap(s, dh, dl, l, c)
        },
        |d: &mut rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>, l, c| {
            d.conn.send_close_notify();
            d.flush().map_err(|e| {
                io_timeout_or(
                    e,
                    "انتهت مهلة الإرسال إلى الخادم",
                    "تعذر إرسال الطلب إلى الخادم",
                    l,
                    c,
                )
            })
        },
        deadline,
        line,
        col,
    )
}

#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
fn ftps_upload_native(
    url: &str,
    data: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    let (mut ctrl, u, deadline) = ftps_setup(url, display, line, col)?;
    let user = u.user.clone().unwrap_or_else(|| "anonymous".to_string());
    let pass = u.pass.clone().unwrap_or_else(|| "niva@".to_string());
    let _ = ftp_transfer(
        &mut ctrl,
        &user,
        &pass,
        &u.path,
        FtpOp::Stor(data.to_vec()),
        true,
        |dh, dp, dl, l, c| {
            let a = resolve_and_check(dh, dp, l, c)?;
            let s = tcp_connect(&a, dh, dl, l, c)?;
            ftps_tls_wrap(s, dh, dl, l, c)
        },
        |d: &mut rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>, l, c| {
            d.conn.send_close_notify();
            d.flush().map_err(|e| {
                io_timeout_or(
                    e,
                    "انتهت مهلة الإرسال إلى الخادم",
                    "تعذر إرسال الطلب إلى الخادم",
                    l,
                    c,
                )
            })
        },
        deadline,
        line,
        col,
    )?;
    Ok(())
}

// ---------- MQTT 3.1.1 (عميل متزامن، QoS 0) ----------
#[cfg(not(target_arch = "wasm32"))]
fn mqtt_len(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_packet(cmd: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![cmd];
    mqtt_len(body.len(), &mut out);
    out.extend_from_slice(body);
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_str(s: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_connect_packet() -> Vec<u8> {
    let mut body = Vec::new();
    mqtt_str("MQTT", &mut body);
    body.push(0x04);
    body.push(0x02);
    body.extend_from_slice(&60u16.to_be_bytes());
    mqtt_str("niva", &mut body);
    mqtt_packet(0x10, &body)
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_read_exact(
    s: &mut std::net::TcpStream,
    mut buf: &mut [u8],
    line: usize,
    col: usize,
) -> RioResult<()> {
    while !buf.is_empty() {
        match s.read(buf) {
            Ok(0) => return Err(RioError::runtime("انقطع الاتصال بالخادم", line, col)),
            Ok(n) => {
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e) => {
                return Err(io_timeout_or(
                    e,
                    "انتهت مهلة القراءة من الخادم",
                    "تعذر قراءة الرد من الخادم",
                    line,
                    col,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_read_packet(
    s: &mut std::net::TcpStream,
    until: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<(u8, Vec<u8>)> {
    // مهلة الشريحة = المتبقي حتى `until` بحد 30ث؛
    // والحزم صغيرة تصل كاملة — فانتهاء الشريحة في
    // منتصف حزمة لا يحدث عمليا على المضيف المحلي.
    let now = std::time::Instant::now();
    if now >= until {
        return Err(RioError::runtime("انتهت مهلة القراءة من الخادم", line, col));
    }
    let slice = until
        .duration_since(now)
        .min(std::time::Duration::from_secs(30));
    let _ = s.set_read_timeout(Some(slice));
    let mut h = [0u8; 1];
    mqtt_read_exact(s, &mut h, line, col)?;
    let cmd = h[0];
    let mut len: usize = 0;
    let mut mult = 1usize;
    loop {
        let mut b = [0u8; 1];
        mqtt_read_exact(s, &mut b, line, col)?;
        len += (b[0] as usize & 0x7F) * mult;
        mult *= 128;
        if b[0] & 0x80 == 0 {
            break;
        }
        if mult > 128 * 128 * 128 {
            return Err(RioError::runtime("رد غير صالح من الخادم", line, col));
        }
    }
    if len > 10 * 1024 * 1024 {
        return Err(RioError::runtime("رد طويل جدا من الخادم", line, col));
    }
    let mut body = vec![0u8; len];
    mqtt_read_exact(s, &mut body, line, col)?;
    Ok((cmd, body))
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_connect_stream(
    broker: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<(std::net::TcpStream, std::time::Instant)> {
    let u = parse_host_url(broker, "mqtt", 1883, display, line, col)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let addr = resolve_and_check(&u.host, u.port, line, col)?;
    let mut stream = tcp_connect(&addr, &u.host, deadline, line, col)?;
    stream
        .write_all(&mqtt_connect_packet())
        .map_err(|e| {
            io_timeout_or(
                e,
                "انتهت مهلة الإرسال إلى الخادم",
                "تعذر إرسال الطلب إلى الخادم",
                line,
                col,
            )
        })?;
    stream.flush().map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    let (cmd, body) = mqtt_read_packet(&mut stream, deadline, line, col)?;
    if cmd != 0x20 || body.len() != 2 || body[1] != 0 {
        return Err(RioError::runtime(
            format!(
                "وسيط MQTT رفض الاتصال (رمز {})",
                body.get(1).copied().unwrap_or(0)
            ),
            line,
            col,
        ));
    }
    Ok((stream, deadline))
}

pub fn mqtt_subscribe(
    broker: &str,
    topic: &str,
    timeout_secs: u64,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<String>> {
    require_network_allowed(line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (broker, topic, timeout_secs, display);
        Err(RioError::runtime("MQTT غير مدعوم على هذه المنصة", line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        mqtt_subscribe_native(broker, topic, timeout_secs, display, line, col)
    }
}

pub fn mqtt_publish(
    broker: &str,
    topic: &str,
    payload: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    require_network_allowed(line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (broker, topic, payload, display);
        Err(RioError::runtime("MQTT غير مدعوم على هذه المنصة", line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        mqtt_publish_native(broker, topic, payload, display, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_subscribe_native(
    broker: &str,
    topic: &str,
    timeout_secs: u64,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<String>> {
    let (mut stream, deadline) = mqtt_connect_stream(broker, display, line, col)?;
    let mut sub = Vec::new();
    mqtt_str(topic, &mut sub);
    sub.push(0x00);
    let mut pkt = vec![0x82];
    mqtt_len(sub.len() + 2, &mut pkt);
    pkt.extend_from_slice(&1u16.to_be_bytes());
    pkt.extend_from_slice(&sub);
    stream.write_all(&pkt).map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    stream.flush().map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    let (cmd, _) = mqtt_read_packet(&mut stream, deadline, line, col)?;
    if cmd != 0x90 {
        return Err(RioError::runtime("رد غير صالح من الخادم", line, col));
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut out: Vec<String> = Vec::new();
    let mut last_ping = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        if now >= until {
            break;
        }
        match mqtt_read_packet(&mut stream, until, line, col) {
            Ok((cmd, body)) => {
                if cmd == 0xD0 {
                    continue;
                }
                if cmd & 0xF0 == 0x30 && body.len() >= 2 {
                    let tlen = ((body[0] as usize) << 8) | body[1] as usize;
                    if body.len() >= 2 + tlen {
                        let payload = &body[2 + tlen..];
                        out.push(String::from_utf8_lossy(payload).into_owned());
                    }
                }
            }
            Err(_) => {
                let now2 = std::time::Instant::now();
                if now2 >= until {
                    break;
                }
                if now2.duration_since(last_ping).as_secs() >= 30 {
                    let _ = stream.write_all(&[0xC0, 0x00]);
                    let _ = stream.flush();
                    last_ping = now2;
                }
                continue;
            }
        }
    }
    let _ = stream.write_all(&[0xE0, 0x00]);
    let _ = stream.flush();
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn mqtt_publish_native(
    broker: &str,
    topic: &str,
    payload: &[u8],
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    let (mut stream, deadline) = mqtt_connect_stream(broker, display, line, col)?;
    let mut body = Vec::new();
    mqtt_str(topic, &mut body);
    body.extend_from_slice(payload);
    let pkt = mqtt_packet(0x30, &body);
    stream.write_all(&pkt).map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    stream.flush().map_err(|e| {
        io_timeout_or(
            e,
            "انتهت مهلة الإرسال إلى الخادم",
            "تعذر إرسال الطلب إلى الخادم",
            line,
            col,
        )
    })?;
    let _ = stream.write_all(&[0xE0, 0x00]);
    let _ = stream.flush();
    Ok(())
}

// ---------- لينكس8-طبقة4: توقيع مستقل (نسخة من كتلتي
// execute_request_inner؛ بلا مساس بالمسار الحي) ----------
pub fn sign_amazon_headers(
    method: &str,
    url: &str,
    body: &[u8],
    key: &str,
    secret: &str,
    region: &str,
    service: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<(String, String)>> {
    let parsed = parse_url(url, line, col)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (datestamp, amzdate) = amz_datetime(now);
    let payload_hash = crate::crypto::sha256_hex(body);
    let (uri, query) = match parsed.path.split_once('?') {
        Some((u, q)) => (u.to_string(), q.to_string()),
        None => (parsed.path.clone(), String::new()),
    };
    let host_hdr = if (parsed.scheme == "http" && parsed.port == 80)
        || (parsed.scheme == "https" && parsed.port == 443)
    {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    };
    let canonical_headers = format!(
        "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
        host_hdr, payload_hash, amzdate
    );
    let canonical_request = format!(
        "{}\n{}\n{}\n{}host;x-amz-content-sha256;x-amz-date\n{}",
        method.to_ascii_uppercase(),
        uri,
        query,
        canonical_headers,
        payload_hash
    );
    let scope = format!("{}/{}/{}/aws4_request", datestamp, region, service);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amzdate,
        scope,
        crate::crypto::sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), datestamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_bytes(&hmac_sha256(&k_signing, to_sign.as_bytes()));
    Ok(vec![
        ("x-amz-date".to_string(), amzdate),
        ("x-amz-content-sha256".to_string(), payload_hash),
        (
            "Authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",
                key, scope, signature
            ),
        ),
    ])
}

pub fn sign_http_sig(
    method: &str,
    url: &str,
    key_id: &str,
    key: &str,
    algorithm: &str,
    line: usize,
    col: usize,
) -> RioResult<(String, String)> {
    if algorithm != "hmac-sha256" {
        return Err(RioError::runtime(
            "خوارزمية التوقيع غير مدعومة — المتاح hmac-sha256 فقط",
            line,
            col,
        ));
    }
    let parsed = parse_url(url, line, col)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let date = http_date_from_unix(now);
    let host_hdr = if (parsed.scheme == "http" && parsed.port == 80)
        || (parsed.scheme == "https" && parsed.port == 443)
    {
        parsed.host.clone()
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    };
    let target = format!("{} {}", method.to_ascii_lowercase(), parsed.path);
    let signing = format!(
        "(request-target): {}\nhost: {}\ndate: {}",
        target, host_hdr, date
    );
    let sig = b64_encode(&hmac_sha256(key.as_bytes(), signing.as_bytes()));
    Ok((
        date,
        format!(
            "Signature keyId=\"{}\",algorithm=\"hmac-sha256\",headers=\"(request-target) host date\",signature=\"{}\"",
            key_id, sig
        ),
    ))
}

// ---------- لينكس8-طبقة5ب: فحص واتصال (اجتماع 90 §105) ----------

/// فحص اتصال TCP: صحيح إن نجح خلال المهلة، خطأ إن رُفض
/// أو انتهت المهلة. خطأ DNS ينتشر كخطأ عربي.
pub fn net_probe_tcp(host: &str, port: u16, timeout_ms: u64, display: &str, line: usize, col: usize) -> RioResult<bool> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (host, port, timeout_ms);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        require_network_allowed(line, col)?;
        let addr = resolve_and_check(host, port, line, col)?;
        let dur = std::time::Duration::from_millis(timeout_ms);
        Ok(std::net::TcpStream::connect_timeout(&addr, dur).is_ok())
    }
}

/// فحص منفذ UDP: "مغلق" عند ECONNREFUSED، "مفتوح" عند
/// وصول رد، "مفلتر" عند المهلة أو عند فشل الإرسال نفسه
/// (لا رد يمكن ملاحظته — منها EPERM في صندوق الرمل).
pub fn net_probe_udp(host: &str, port: u16, timeout_ms: u64, display: &str, line: usize, col: usize) -> RioResult<&'static str> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (host, port, timeout_ms);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        require_network_allowed(line, col)?;
        let addr = resolve_and_check(host, port, line, col)?;
        let sock = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|_| RioError::runtime("تعذر فتح مقبس UDP", line, col))?;
        sock.connect(addr)
            .map_err(|_| RioError::runtime("تعذر الاتصال بمقبس UDP", line, col))?;
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms)));
        match sock.send(&[0u8]) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return Ok("مغلق"),
            Err(_) => return Ok("مفلتر"),
        }
        let mut buf = [0u8; 512];
        match sock.recv(&mut buf) {
            Ok(_) => Ok("مفتوح"),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => Ok("مغلق"),
            Err(_) => Ok("مفلتر"),
        }
    }
}

// ---------- لينكس9-شبكة: صيغة نتيجة الفحص السطرية ----------
// الصيغة القانونية: سطر لكل منفذ مفحوص «منفذ: حالة»
// (الحالة: مفتوح/مغلق/مفلتر)؛ الأسطر الأخرى تُتجاهل.
pub fn net_scan_parse_ports(text: &str) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let Some((num, _rest)) = t.split_once(':') else { continue };
        if let Ok(p) = num.trim().parse::<u32>() {
            if (1..=65535).contains(&p) && !out.contains(&(p as u16)) {
                out.push(p as u16);
            }
        }
    }
    out.sort_unstable();
    out
}

fn net_scan_escape_xml(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn net_scan_to_xml(text: &str) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<فحص>\n");
    for line in text.lines() {
        let t = line.trim();
        let Some((num, state)) = t.split_once(':') else { continue };
        let Ok(p) = num.trim().parse::<u32>() else { continue };
        if !(1..=65535).contains(&p) { continue }
        out.push_str(&format!(
            "  <منفذ رقم=\"{}\" حالة=\"{}\" />\n",
            p,
            net_scan_escape_xml(state.trim())
        ));
    }
    out.push_str("</فحص>\n");
    out
}

pub fn net_scan_to_search(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let t = line.trim();
        let Some((num, state)) = t.split_once(':') else { continue };
        let Ok(p) = num.trim().parse::<u32>() else { continue };
        if !(1..=65535).contains(&p) { continue }
        out.push_str(&format!("{} {}\n", p, state.trim()));
    }
    out
}

// ---------- لينكس9-شبكة: نبضة UDP (363) ----------
// حياة المضيف = رد أو رفض ICMP؛ الصمت وحده «مفلتر» = غير حي.
pub fn net_probe_udp_ping(
    host: &str,
    port: u16,
    timeout_ms: u64,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    let state = net_probe_udp(host, port, timeout_ms, display, line, col)?;
    Ok(state != "مفلتر")
}

// ---------- لينكس9-شبكة: كشف الخدمة والإصدار (365) ----------
// تحليل البانر — المنطق نفسه حرفيا في مرآة C.
pub fn net_detect_service_parse(banner: &str) -> (String, String) {
    let first = banner.lines().next().unwrap_or("").trim();
    if let Some(rest) = first.strip_prefix("SSH-") {
        let ver: String = rest.split([' ', '\r', '\t']).next().unwrap_or("").chars().take(32).collect();
        return ("SSH".to_string(), ver);
    }
    if first.starts_with("220") {
        if first.contains("FTP") {
            return ("FTP".to_string(), first.chars().take(64).collect());
        }
        if first.contains("SMTP") || first.contains("ESMTP") {
            return ("SMTP".to_string(), first.chars().take(64).collect());
        }
        return ("غير معروفة".to_string(), first.chars().take(64).collect());
    }
    if let Some(rest) = first.strip_prefix("HTTP/") {
        let ver: String = rest.split([' ', '\r', '\t']).next().unwrap_or("").chars().take(16).collect();
        return ("HTTP".to_string(), format!("HTTP/{}", ver));
    }
    if first.is_empty() {
        return ("غير معروفة".to_string(), String::new());
    }
    ("غير معروفة".to_string(), first.chars().take(64).collect())
}

pub fn net_detect_service(
    host: &str,
    port: u16,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<(String, String)> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (host, port);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        require_network_allowed(line, col)?;
        let addr = resolve_and_check(host, port, line, col)?;
        let dur = std::time::Duration::from_millis(2000);
        let mut stream = std::net::TcpStream::connect_timeout(&addr, dur).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col)
            } else {
                RioError::runtime(format!("تعذر الاتصال بالمضيف {}", host), line, col)
            }
        })?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(1000)));
        let mut buf = [0u8; 512];
        let mut banner = String::new();
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                banner = String::from_utf8_lossy(&buf[..n]).into_owned();
            }
            _ => {
                // بلا بانر تلقائيا: جرّب طلب HEAD فيكشف HTTP.
                let _ = stream.write_all(b"HEAD / HTTP/1.0\r\n\r\n");
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(1000)));
                if let Ok(n) = stream.read(&mut buf) {
                    if n > 0 {
                        banner = String::from_utf8_lossy(&buf[..n]).into_owned();
                    }
                }
            }
        }
        Ok(net_detect_service_parse(&banner))
    }
}

// ---------- لينكس9-شبكة: استئناف فحص متوقف (374) ----------
// يقرأ ملف الفحص السطري، يفحص باقي المنافذ في [من، إلى]
// (السقف 1024 منفذا كافحص_نطاق)، يلحق النتائج بالملف،
// ويرجع قائمة المنافذ المفتوحة المكتشفة حديثا.
pub fn net_scan_resume(
    path: &str,
    host: &str,
    from: u16,
    to: u16,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<u16>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, host, from, to);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let old = std::fs::read_to_string(path).map_err(|_| {
            RioError::runtime(format!("ملف الفحص غير موجود: {}", path), line, col)
        })?;
        if to < from || (to - from + 1) > 1024 {
            return Err(RioError::runtime(
                "الدالة شبكة.أكمل_الفحص نطاق كبير جدا (الحد 1024)",
                line,
                col,
            ));
        }
        let scanned = net_scan_parse_ports(&old);
        let mut open: Vec<u16> = Vec::new();
        let mut extra = String::new();
        for p in from..=to {
            if scanned.contains(&p) {
                continue;
            }
            if net_probe_tcp(host, p, 200, display, line, col)? {
                open.push(p);
                extra.push_str(&format!("{}: مفتوح\n", p));
            } else {
                extra.push_str(&format!("{}: مغلق\n", p));
            }
        }
        let mut new_text = old;
        if !new_text.is_empty() && !new_text.ends_with('\n') {
            new_text.push('\n');
        }
        new_text.push_str(&extra);
        std::fs::write(path, new_text.as_bytes()).map_err(|_| {
            RioError::runtime(format!("الدالة {} تعذر كتابة الملف", display), line, col)
        })?;
        Ok(open)
    }
}

fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// نبض ICMP عبر مقبس ping غير المميز (SOCK_DGRAM):
/// لينكس فقط؛ فشل فتح المقبس (بلا صلاحية) يعني خطأ
/// لا خطأ عربي — النبض لم يصل.
pub fn net_probe_icmp(host: &str, timeout_ms: u64, display: &str, line: usize, col: usize) -> RioResult<bool> {
    #[cfg(any(target_arch = "wasm32", not(target_os = "linux")))]
    {
        let _ = (host, timeout_ms);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    {
        require_network_allowed(line, col)?;
        let addr = resolve_and_check(host, 0, line, col)?;
        let ip = match addr.ip() {
            std::net::IpAddr::V4(v4) => v4,
            _ => {
                return Err(RioError::runtime(
                    format!("الدالة {} النبض يدعم IPv4 فقط", display),
                    line,
                    col,
                ))
            }
        };
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP) };
        if fd < 0 {
            return Ok(false);
        }
        let id = (std::process::id() & 0xFFFF) as u16;
        let mut pkt = [0u8; 64];
        pkt[0] = 8;
        pkt[1] = 0;
        pkt[4] = (id >> 8) as u8;
        pkt[5] = (id & 0xFF) as u8;
        pkt[6] = 0;
        pkt[7] = 1;
        for (i, b) in b"niva".iter().enumerate() {
            pkt[8 + i] = *b;
        }
        let csum = icmp_checksum(&pkt);
        pkt[2] = (csum >> 8) as u8;
        pkt[3] = (csum & 0xFF) as u8;
        let sent = unsafe {
            let mut sa: libc::sockaddr_in = std::mem::zeroed();
            sa.sin_family = libc::AF_INET as libc::sa_family_t;
            sa.sin_port = 0;
            sa.sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(ip.octets()) };
            libc::sendto(
                fd,
                pkt.as_ptr() as *const libc::c_void,
                pkt.len(),
                0,
                &sa as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };
        if sent < 0 {
            unsafe { libc::close(fd) };
            return Ok(false);
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let pr = unsafe { libc::poll(&mut pfd, 1, timeout_ms as i32) };
        if pr <= 0 {
            unsafe { libc::close(fd) };
            return Ok(false);
        }
        let mut buf = [0u8; 512];
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
        unsafe { libc::close(fd) };
        if n < 20 {
            return Ok(false);
        }
        Ok(buf[0] == 0 && buf[4] == pkt[4] && buf[5] == pkt[5])
    }
}

/// تتبع المسار عبر UDP + IP_RECVERR (لينكس فقط):
/// كل قفزة (رقم، عنوان أو لا شيء عند المهلة، زمن بالمللي).
/// فشل الإرسال (منه EPERM) يُعامل كقفزة مهلة — لا رد
/// يمكن ملاحظته. يتوقف عند وصول ICMP «المنفذ لا يستجيب».
pub fn net_probe_trace(host: &str, max_hops: u32, display: &str, line: usize, col: usize) -> RioResult<Vec<(u32, Option<String>, u64)>> {
    #[cfg(any(target_arch = "wasm32", not(target_os = "linux")))]
    {
        let _ = (host, max_hops);
        Err(RioError::runtime(format!("{} غير مدعوم على هذه المنصة", display), line, col))
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    {
        require_network_allowed(line, col)?;
        let addr = resolve_and_check(host, 33434, line, col)?;
        let ip = match addr.ip() {
            std::net::IpAddr::V4(v4) => v4,
            _ => {
                return Err(RioError::runtime(
                    format!("الدالة {} التتبع يدعم IPv4 فقط", display),
                    line,
                    col,
                ))
            }
        };
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_UDP) };
        if fd < 0 {
            return Err(RioError::runtime("تعذر فتح مقبس التتبع", line, col));
        }
        let on: libc::c_int = 1;
        unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IP,
                libc::IP_RECVERR,
                &on as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
        let mut hops = Vec::new();
        let payload = [0u8; 32];
        for ttl in 1..=max_hops {
            let ttl_i = ttl as libc::c_int;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_IP,
                    libc::IP_TTL,
                    &ttl_i as *const libc::c_int as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
            let start = std::time::Instant::now();
            let dest_port = 33434 + ttl as u16;
            let sent = unsafe {
                let mut sa: libc::sockaddr_in = std::mem::zeroed();
                sa.sin_family = libc::AF_INET as libc::sa_family_t;
                sa.sin_port = dest_port.to_be();
                sa.sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(ip.octets()) };
                libc::sendto(
                    fd,
                    payload.as_ptr() as *const libc::c_void,
                    payload.len(),
                    0,
                    &sa as *const libc::sockaddr_in as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            };
            if sent < 0 {
                hops.push((ttl, None, start.elapsed().as_millis() as u64));
                continue;
            }
            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            let pr = unsafe { libc::poll(&mut pfd, 1, 1000) };
            if pr <= 0 {
                hops.push((ttl, None, start.elapsed().as_millis() as u64));
                continue;
            }
            let mut buf = [0u8; 512];
            let mut ctrl = [0u8; 512];
            let mut src: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
            let mut iov = libc::iovec { iov_base: buf.as_mut_ptr() as *mut libc::c_void, iov_len: buf.len() };
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_name = &mut src as *mut libc::sockaddr_storage as *mut libc::c_void;
            msg.msg_namelen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = ctrl.as_mut_ptr() as *mut libc::c_void;
            msg.msg_controllen = ctrl.len();
            let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
            let elapsed = start.elapsed().as_millis() as u64;
            if n < 0 {
                hops.push((ttl, None, elapsed));
                continue;
            }
            let sender: Option<String> = unsafe {
                if src.ss_family == libc::AF_INET as libc::sa_family_t {
                    let sin = &*(&src as *const libc::sockaddr_storage as *const libc::sockaddr_in);
                    let oct = sin.sin_addr.s_addr.to_ne_bytes();
                    Some(format!("{}.{}.{}.{}", oct[0], oct[1], oct[2], oct[3]))
                } else {
                    None
                }
            };
            let mut reached = false;
            unsafe {
                let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
                while !cmsg.is_null() {
                    if (*cmsg).cmsg_level == libc::IPPROTO_IP && (*cmsg).cmsg_type == libc::IP_RECVERR {
                        let ee = &*(libc::CMSG_DATA(cmsg) as *const libc::sock_extended_err);
                        if ee.ee_type == 3 && ee.ee_code == 3 {
                            reached = true;
                        }
                    }
                    cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
                }
            }
            hops.push((ttl, sender, elapsed));
            if reached {
                break;
            }
        }
        unsafe { libc::close(fd) };
        Ok(hops)
    }
}

/// لف تدفق TCP عادي بـ TLS (لاتصل_مشفر) — رسائل عامة بلا ذكر FTPS.
#[cfg(all(not(target_arch = "wasm32"), feature = "https"))]
pub(crate) fn tls_wrap_stream(
    stream: std::net::TcpStream,
    host: &str,
    deadline: std::time::Instant,
    line: usize,
    col: usize,
) -> RioResult<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>> {
    let config = std::sync::Arc::new(tls_client_config());
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| {
        RioError::runtime(format!("اسم مضيف غير صالح: {}", host), line, col)
    })?;
    let conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|_| RioError::runtime("تعذر بدء اتصال TLS", line, col))?;
    let mut tls = rustls::StreamOwned::new(conn, stream);
    let left = time_left(deadline, line, col)?;
    let _ = tls.get_mut().set_read_timeout(Some(left));
    let _ = tls.get_mut().set_write_timeout(Some(left));
    Ok(tls)
}
