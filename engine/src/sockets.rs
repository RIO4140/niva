//! Sockets for the `شبكة` native module (catalog task 13, from
//! `net/`): TCP/UDP sockets under the same network gate as
//! `اجلب`. One implementation, shared by both engines
//! (interpreter + VM) through the stdlib native functions —
//! never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-05-SOCKETS.md` §1): the gate is `اجلب`'s own gate
//! (`network_allowed`, off by default, `--اسمح-بالشبكة`); every
//! host is resolved once and checked with the same `ip_class`
//! rule as the fetch (§7) — a loopback / private / link-local /
//! multicast / unspecified address is refused with `اجلب`'s own
//! wording unless the host was explicitly allowed with
//! `--مضيف-محلي`; the timeout is the same 30 seconds and a
//! single read is one 64 KiB chunk (a stream read, not a
//! whole-body fetch); listeners bind 127.0.0.1 by default and
//! 0.0.0.0 only with an explicit host, exactly like `اخدم`.
//! Handles only, never a raw socket: `اتصل` / `مقبس` /
//! `استمع` return a numeric handle, and every other call takes
//! the handle only; at most 64 live sockets (the same cap as
//! the server's simultaneous connections); `أغلق` consumes the
//! handle. WASM refuses every socket call with the flat
//! builtins' own message. `شبكة.نزّل` is NOT here (task 22).

use crate::error::{RioError, RioResult};
use crate::network;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// One read is one stream chunk: 64 KiB. A peer that sends
/// more is read again — sockets are streams, not whole bodies.
pub const MAX_RECV_BYTES: usize = 64 * 1024;

/// At most 64 live sockets (the same cap as the server's
/// simultaneous connections): a loop that opens sockets
/// without closing them cannot exhaust the process's file
/// descriptors.
pub const MAX_LIVE_SOCKETS: usize = 64;

/// How long `اقبل` waits for a client, and the read/write
/// timeout on every connected stream: the same 30 seconds as
/// the fetch (§9).
const TIMEOUT: Duration = Duration::from_secs(network::TIMEOUT_SECS);

/// How often the accept poll loop wakes up while waiting.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// A live socket. TCP starts unconnected (`مقبس("tcp")`)
/// until `اتصل` connects it; a listener never sends or
/// receives — `اقبل` makes a connected socket from it.
enum SocketEntry {
    TcpUnconnected,
    TcpConnected(TcpStream),
    Listener(TcpListener),
    Udp(UdpSocket),
}

/// Live sockets: handle -> entry. One table for the whole OS
/// process, shared by both engines (a Niva program is one OS
/// process, so it has one socket table).
static REGISTRY: LazyLock<Mutex<HashMap<u64, SocketEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Socket handles start at 1 and never repeat in one run.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Test-only serialization for the process-wide registry:
/// cargo runs tests in parallel threads, and the registry is
/// a single static. Every test that touches it locks this
/// first (same pattern as MEM_TEST_LOCK in memory.rs).
#[cfg(test)]
pub(crate) static SOCK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lock the test serialization lock, recovering from
/// poisoning: if an earlier test panicked while holding it,
/// the next test must still run and report its OWN result
/// instead of failing with PoisonError (CI 2026-11-06: one
/// macOS panic cascaded into four PoisonErrors that hid
/// every other test's verdict).
#[cfg(test)]
fn sock_test_lock() -> std::sync::MutexGuard<'static, ()> {
    SOCK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test-only: drop every live socket and reset nothing else.
/// The caller must hold SOCK_TEST_LOCK (via sock_test_lock),
/// so no other sockets test can hold a handle at the time.
#[cfg(test)]
pub(crate) fn test_clear() {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.clear();
    }
}

/// A port must be 1..=65535 — except for a bind (`استمع` /
/// `اربط`), where 0 asks the OS for a free port. The error
/// carries the caller's own display name (same wording style
/// as `اخدم`'s).
fn check_port(
    port: i64,
    display: &str,
    allow_zero: bool,
    line: usize,
    col: usize,
) -> RioResult<u16> {
    let low = if allow_zero { 0 } else { 1 };
    if !(low..=65535).contains(&port) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع منفذا بين {} و 65535",
                display, low
            ),
            line,
            col,
        ));
    }
    Ok(port as u16)
}

/// Insert a new entry, enforcing the live-socket cap.
fn insert_entry(entry: SocketEntry, line: usize, col: usize) -> RioResult<u64> {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    match REGISTRY.lock() {
        Ok(mut reg) => {
            if reg.len() >= MAX_LIVE_SOCKETS {
                return Err(RioError::runtime(
                    "تجاوز عدد المقابس الحية الحد (٦٤) — أغلق بعض المقابس",
                    line,
                    col,
                ));
            }
            reg.insert(id, entry);
        }
        Err(_) => {
            return Err(RioError::runtime(
                "تعذّر تسجيل المقبس الجديد",
                line,
                col,
            ));
        }
    }
    Ok(id)
}

/// Take a live entry out of the registry, or an Arabic error
/// if the handle is unknown (already closed, or never was).
fn take_entry(id: u64, line: usize, col: usize) -> RioResult<SocketEntry> {
    match REGISTRY.lock() {
        Ok(mut reg) => reg.remove(&id).ok_or_else(|| {
            RioError::runtime(format!("مقبس غير معروف: {}", id), line, col)
        }),
        Err(_) => Err(RioError::runtime(
            format!("مقبس غير معروف: {}", id),
            line,
            col,
        )),
    }
}

/// Put an entry back under its handle (used by `اقبل`: the
/// listener leaves the table while it waits, and returns to
/// it — under the same handle — whether a client arrived or
/// the wait timed out).
fn put_back_entry(id: u64, entry: SocketEntry) {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.insert(id, entry);
    }
}

/// A cloned TCP stream for I/O without holding the registry
/// lock while the call blocks (the clone shares the same
/// connection, timeouts included). Also used by the async
/// module (task 19), whose workers own the clone. Native
/// only: no caller exists on WASM, so it is compiled out
/// there (no dead-code warning).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clone_tcp(id: u64, line: usize, col: usize) -> RioResult<TcpStream> {
    let cloned = match REGISTRY.lock() {
        Ok(reg) => match reg.get(&id) {
            Some(SocketEntry::TcpConnected(s)) => s.try_clone().ok(),
            Some(SocketEntry::TcpUnconnected) => {
                return Err(RioError::runtime(
                    "المقبس غير متصل — اتصل أولا",
                    line,
                    col,
                ))
            }
            Some(SocketEntry::Listener(_)) => {
                return Err(RioError::runtime(
                    "المستمع لا يرسل ولا يستقبل — اقبل اتصالا أولا",
                    line,
                    col,
                ))
            }
            Some(SocketEntry::Udp(_)) => {
                return Err(RioError::runtime(
                    "مقبس UDP لا يستخدم أرسل واستقبل — استخدم أرسل_إلى واستقبل_من",
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!("مقبس غير معروف: {}", id),
                    line,
                    col,
                ))
            }
        },
        Err(_) => {
            return Err(RioError::runtime(
                format!("مقبس غير معروف: {}", id),
                line,
                col,
            ))
        }
    };
    cloned.ok_or_else(|| {
        RioError::runtime("تعذّر نسخ المقبس للقراءة أو الكتابة", line, col)
    })
}

/// A cloned UDP socket for I/O without holding the registry
/// lock while the call blocks.
fn clone_udp(id: u64, line: usize, col: usize) -> RioResult<UdpSocket> {
    let cloned = match REGISTRY.lock() {
        Ok(reg) => match reg.get(&id) {
            Some(SocketEntry::Udp(s)) => s.try_clone().ok(),
            Some(SocketEntry::TcpConnected(_)) | Some(SocketEntry::TcpUnconnected) => {
                return Err(RioError::runtime(
                    "مقبس TCP لا يستخدم أرسل_إلى واستقبل_من — استخدم أرسل واستقبل",
                    line,
                    col,
                ))
            }
            Some(SocketEntry::Listener(_)) => {
                return Err(RioError::runtime(
                    "المستمع لا يرسل ولا يستقبل — اقبل اتصالا أولا",
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!("مقبس غير معروف: {}", id),
                    line,
                    col,
                ))
            }
        },
        Err(_) => {
            return Err(RioError::runtime(
                format!("مقبس غير معروف: {}", id),
                line,
                col,
            ))
        }
    };
    cloned.ok_or_else(|| {
        RioError::runtime("تعذّر نسخ المقبس للقراءة أو الكتابة", line, col)
    })
}

/// Arm the 30-second read/write timeouts on a connected
/// stream (same timeout as the fetch).
fn arm_timeouts(stream: &TcpStream) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
}

/// Translate a connect failure the way the fetch does:
/// timeout first, then the plain refusal — no English OS
/// text ever reaches the user.
fn connect_error(host: &str, e: &std::io::Error, line: usize, col: usize) -> RioError {
    if e.kind() == std::io::ErrorKind::TimedOut {
        RioError::runtime("انتهت مهلة الاتصال (30 ثانية)", line, col)
    } else {
        RioError::runtime(format!("تعذر الاتصال بالمضيف {}", host), line, col)
    }
}

/// Translate a bind failure the way the server does:
/// permission first, then address-in-use, then generic.
fn bind_error_message(what: &str, host: &str, port: u16, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "تعذر ربط {} على {}:{} — المنفذ يحتاج صلاحيات مرتفعة",
            what, host, port
        ),
        std::io::ErrorKind::AddrInUse => {
            format!("تعذر ربط {} على {}:{} — المنفذ مستخدم بالفعل", what, host, port)
        }
        _ => format!("تعذر ربط {} على {}:{}", what, host, port),
    }
}

/// Create a socket: `شبكة.مقبس("tcp")` or `شبكة.مقبس("udp")`
/// — returns the socket handle. A TCP socket starts
/// unconnected (connect it with `اتصل`); a UDP socket is
/// bound at once to an ephemeral loopback port, so
/// `أرسل_إلى` works immediately and `اربط` rebinds it to a
/// chosen port. The kind strings are protocol identifiers,
/// like the `http` in a URL.
pub fn create_socket(kind: &str, line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = kind;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        create_socket_native(kind, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn create_socket_native(kind: &str, line: usize, col: usize) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    match kind {
        "tcp" => insert_entry(SocketEntry::TcpUnconnected, line, col),
        "udp" => {
            let sock = UdpSocket::bind("127.0.0.1:0").map_err(|e| {
                RioError::runtime(
                    bind_error_message("المقبس", "127.0.0.1", 0, &e),
                    line,
                    col,
                )
            })?;
            let _ = sock.set_read_timeout(Some(TIMEOUT));
            let _ = sock.set_write_timeout(Some(TIMEOUT));
            insert_entry(SocketEntry::Udp(sock), line, col)
        }
        _ => Err(RioError::runtime(
            "نوع المقبس يجب أن يكون tcp أو udp",
            line,
            col,
        )),
    }
}

/// Open a TCP connection: `شبكة.اتصل(مضيف، منفذ)` —
/// creates and connects a socket, returns its handle.
pub fn connect_new(host: &str, port: i64, line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (host, port);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        connect_new_native(host, port, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn connect_new_native(host: &str, port: i64, line: usize, col: usize) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    let port = check_port(port, "شبكة.اتصل", false, line, col)?;
    let addr = network::resolve_checked(host, port, line, col)?;
    let stream = TcpStream::connect_timeout(&addr, TIMEOUT)
        .map_err(|e| connect_error(host, &e, line, col))?;
    arm_timeouts(&stream);
    insert_entry(SocketEntry::TcpConnected(stream), line, col)
}

/// Connect an existing socket: `شبكة.اتصل(معرف، مضيف،
/// منفذ)` — connects a socket made by `مقبس("tcp")` and
/// returns the same handle. An already-connected socket, a
/// listener, or a UDP socket is an Arabic error.
pub fn connect_existing(
    id: u64,
    host: &str,
    port: i64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, host, port);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        connect_existing_native(id, host, port, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn connect_existing_native(
    id: u64,
    host: &str,
    port: i64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    // The socket must exist and be an unconnected TCP socket
    // before any address is resolved: an unknown handle is
    // reported as such, not as a connection failure.
    let is_unconnected = match REGISTRY.lock() {
        Ok(reg) => match reg.get(&id) {
            Some(SocketEntry::TcpUnconnected) => true,
            Some(SocketEntry::TcpConnected(_)) => {
                return Err(RioError::runtime(
                    "المقبس متصل بالفعل",
                    line,
                    col,
                ))
            }
            Some(SocketEntry::Listener(_)) => {
                return Err(RioError::runtime(
                    "المستمع لا يتصل — اقبل اتصالا أولا",
                    line,
                    col,
                ))
            }
            Some(SocketEntry::Udp(_)) => {
                return Err(RioError::runtime(
                    "مقبس UDP لا يتصل — استخدم أرسل_إلى",
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!("مقبس غير معروف: {}", id),
                    line,
                    col,
                ))
            }
        },
        Err(_) => {
            return Err(RioError::runtime(
                format!("مقبس غير معروف: {}", id),
                line,
                col,
            ))
        }
    };
    debug_assert!(is_unconnected);
    let port = check_port(port, "شبكة.اتصل", false, line, col)?;
    let addr = network::resolve_checked(host, port, line, col)?;
    let stream = TcpStream::connect_timeout(&addr, TIMEOUT)
        .map_err(|e| connect_error(host, &e, line, col))?;
    arm_timeouts(&stream);
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.insert(id, SocketEntry::TcpConnected(stream));
    }
    Ok(id)
}

/// Listen for TCP connections: `شبكة.استمع(منفذ)` binds
/// 127.0.0.1; `شبكة.استمع(منفذ، مضيف)` binds the given
/// host, which must be 127.0.0.1 or 0.0.0.0 (the server's
/// own rule). Port 0 asks the OS for a free port — read it
/// back with `المنفذ`. Returns the listener handle.
pub fn listen(
    port: i64,
    host: Option<&str>,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (port, host);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        listen_native(port, host, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn listen_native(
    port: i64,
    host: Option<&str>,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    let port = check_port(port, "شبكة.استمع", true, line, col)?;
    let host = match host {
        None => "127.0.0.1",
        Some(h) if h == "127.0.0.1" || h == "0.0.0.0" => h,
        Some(_) => {
            return Err(RioError::runtime(
                "الخادم يربط محليا فقط: المضيف 127.0.0.1 أو 0.0.0.0",
                line,
                col,
            ))
        }
    };
    let listener = TcpListener::bind((host, port)).map_err(|e| {
        RioError::runtime(
            bind_error_message("المستمع", host, port, &e),
            line,
            col,
        )
    })?;
    insert_entry(SocketEntry::Listener(listener), line, col)
}

/// Accept one client: `شبكة.اقبل(معرف_مستمع)` — waits up
/// to 30 seconds for a client, then returns the new
/// connected socket's handle. The listener stays live under
/// its own handle either way.
pub fn accept(id: u64, line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        accept_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn accept_native(id: u64, line: usize, col: usize) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    let entry = take_entry(id, line, col)?;
    let listener = match entry {
        SocketEntry::Listener(l) => l,
        SocketEntry::TcpConnected(_) | SocketEntry::TcpUnconnected => {
            put_back_entry(id, entry);
            return Err(RioError::runtime(
                "اقبل يعمل على مستمع فقط — استخدم استمع أولا",
                line,
                col,
            ));
        }
        SocketEntry::Udp(_) => {
            put_back_entry(id, entry);
            return Err(RioError::runtime(
                "اقبل يعمل على مستمع TCP فقط — مقبس UDP لا يقبل اتصالات",
                line,
                col,
            ));
        }
    };
    // Non-blocking poll with a 30-second deadline: a plain
    // blocking accept would hang the program with no way
    // back when no client ever arrives.
    let _ = listener.set_nonblocking(true);
    let deadline = Instant::now() + TIMEOUT;
    let outcome: RioResult<TcpStream> = loop {
        match listener.accept() {
            Ok((stream, _)) => break Ok(stream),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    break Err(RioError::runtime(
                        "انتهت مهلة القبول (30 ثانية) — لم يصل أي عميل",
                        line,
                        col,
                    ));
                }
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(_) => {
                break Err(RioError::runtime(
                    "تعذر قبول الاتصال",
                    line,
                    col,
                ))
            }
        }
    };
    let _ = listener.set_nonblocking(false);
    put_back_entry(id, SocketEntry::Listener(listener));
    let stream = outcome?;
    arm_timeouts(&stream);
    insert_entry(SocketEntry::TcpConnected(stream), line, col)
}

/// Bind a UDP socket: `شبكة.اربط(معرف، منفذ)` — binds the
/// socket to 127.0.0.1 on the given port (a UDP server's
/// fixed port). A UDP socket made by `مقبس("udp")` already
/// has an ephemeral port; rebinding replaces it.
pub fn bind_udp(id: u64, port: i64, line: usize, col: usize) -> RioResult<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, port);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        bind_udp_native(id, port, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn bind_udp_native(id: u64, port: i64, line: usize, col: usize) -> RioResult<()> {
    network::require_network_allowed(line, col)?;
    let port = check_port(port, "شبكة.اربط", true, line, col)?;
    let is_udp = match REGISTRY.lock() {
        Ok(reg) => match reg.get(&id) {
            Some(SocketEntry::Udp(_)) => true,
            Some(_) => {
                return Err(RioError::runtime(
                    "اربط يعمل على مقبس UDP فقط",
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!("مقبس غير معروف: {}", id),
                    line,
                    col,
                ))
            }
        },
        Err(_) => {
            return Err(RioError::runtime(
                format!("مقبس غير معروف: {}", id),
                line,
                col,
            ))
        }
    };
    debug_assert!(is_udp);
    let sock = UdpSocket::bind(("127.0.0.1", port)).map_err(|e| {
        RioError::runtime(
            bind_error_message("المقبس", "127.0.0.1", port, &e),
            line,
            col,
        )
    })?;
    let _ = sock.set_read_timeout(Some(TIMEOUT));
    let _ = sock.set_write_timeout(Some(TIMEOUT));
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.insert(id, SocketEntry::Udp(sock));
    }
    Ok(())
}

/// Send text on a connected TCP socket: `شبكة.أرسل(معرف،
/// نص)` — returns the bytes sent.
pub fn send(id: u64, text: &str, line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, text);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        send_native(id, text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn send_native(id: u64, text: &str, line: usize, col: usize) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    let mut stream = clone_tcp(id, line, col)?;
    stream.write_all(text.as_bytes()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الإرسال (30 ثانية)", line, col)
        } else {
            RioError::runtime("تعذر الإرسال على المقبس", line, col)
        }
    })?;
    Ok(text.len() as u64)
}

/// Receive text on a connected TCP socket:
/// `شبكة.استقبل(معرف)` — one stream read, up to 64 KiB.
/// A clean close from the peer is an Arabic error, not an
/// empty string (the two must never be confused).
pub fn recv(id: u64, line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        recv_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn recv_native(id: u64, line: usize, col: usize) -> RioResult<String> {
    network::require_network_allowed(line, col)?;
    let mut stream = clone_tcp(id, line, col)?;
    let mut buf = vec![0u8; MAX_RECV_BYTES];
    let n = stream.read(&mut buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الاستقبال (30 ثانية)", line, col)
        } else {
            RioError::runtime("تعذر الاستقبال من المقبس", line, col)
        }
    })?;
    if n == 0 {
        return Err(RioError::runtime(
            "أغلق الطرف الآخر الاتصال",
            line,
            col,
        ));
    }
    buf.truncate(n);
    String::from_utf8(buf).map_err(|_| {
        RioError::runtime(
            "البيانات المستقبلة ليست نصا صالحا (UTF-8)",
            line,
            col,
        )
    })
}

/// Send a UDP datagram: `شبكة.أرسل_إلى(معرف، مضيف، منفذ،
/// نص)` — returns the bytes sent.
pub fn send_to(
    id: u64,
    host: &str,
    port: i64,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, host, port, text);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        send_to_native(id, host, port, text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn send_to_native(
    id: u64,
    host: &str,
    port: i64,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    network::require_network_allowed(line, col)?;
    let port = check_port(port, "شبكة.أرسل_إلى", false, line, col)?;
    let addr = network::resolve_checked(host, port, line, col)?;
    let sock = clone_udp(id, line, col)?;
    let n = sock.send_to(text.as_bytes(), addr).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الإرسال (30 ثانية)", line, col)
        } else {
            RioError::runtime("تعذر الإرسال على المقبس", line, col)
        }
    })?;
    Ok(n as u64)
}

/// Receive a UDP datagram: `شبكة.استقبل_من(معرف)` —
/// returns (text, sender host, sender port). A socket made
/// by `مقبس("udp")` already has an ephemeral port, so a
/// reply arrives without `اربط`; `اربط` is only for a
/// server that wants a fixed port.
pub fn recv_from(id: u64, line: usize, col: usize) -> RioResult<(String, String, u16)> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        recv_from_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn recv_from_native(id: u64, line: usize, col: usize) -> RioResult<(String, String, u16)> {
    network::require_network_allowed(line, col)?;
    let sock = clone_udp(id, line, col)?;
    let mut buf = vec![0u8; MAX_RECV_BYTES];
    let (n, src) = sock.recv_from(&mut buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            RioError::runtime("انتهت مهلة الاستقبال (30 ثانية)", line, col)
        } else {
            RioError::runtime("تعذر الاستقبال من المقبس", line, col)
        }
    })?;
    buf.truncate(n);
    let text = String::from_utf8(buf).map_err(|_| {
        RioError::runtime(
            "البيانات المستقبلة ليست نصا صالحا (UTF-8)",
            line,
            col,
        )
    })?;
    Ok((text, src.ip().to_string(), src.port()))
}

/// The socket's local port: `شبكة.المنفذ(معرف)` — the
/// port a listener bound (read it back after `استمع(0)`)
/// or the ephemeral port of a connected socket.
pub fn local_port(id: u64, line: usize, col: usize) -> RioResult<u16> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        local_port_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn local_port_native(id: u64, line: usize, col: usize) -> RioResult<u16> {
    network::require_network_allowed(line, col)?;
    match REGISTRY.lock() {
        Ok(reg) => match reg.get(&id) {
            Some(SocketEntry::TcpConnected(s)) => s
                .local_addr()
                .map(|a| a.port())
                .map_err(|_| RioError::runtime("تعذّر قراءة منفذ المقبس", line, col)),
            Some(SocketEntry::TcpUnconnected) => Err(RioError::runtime(
                "المقبس غير متصل — اتصل أولا",
                line,
                col,
            )),
            Some(SocketEntry::Listener(l)) => l
                .local_addr()
                .map(|a| a.port())
                .map_err(|_| RioError::runtime("تعذّر قراءة منفذ المقبس", line, col)),
            Some(SocketEntry::Udp(s)) => s
                .local_addr()
                .map(|a| a.port())
                .map_err(|_| RioError::runtime("تعذّر قراءة منفذ المقبس", line, col)),
            None => Err(RioError::runtime(
                format!("مقبس غير معروف: {}", id),
                line,
                col,
            )),
        },
        Err(_) => Err(RioError::runtime(
            format!("مقبس غير معروف: {}", id),
            line,
            col,
        )),
    }
}

/// Close a socket: `شبكة.أغلق(معرف)` — the handle is
/// consumed, so closing twice is an error.
pub fn close_socket(id: u64, line: usize, col: usize) -> RioResult<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        close_socket_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn close_socket_native(id: u64, line: usize, col: usize) -> RioResult<()> {
    network::require_network_allowed(line, col)?;
    take_entry(id, line, col)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Does this machine allow a UDP loopback send? This VM's
    /// sandbox blocks UDP sendto with EPERM (proven 2026-11-06
    /// with a plain Python socket: bind ok, sendto EPERM) —
    /// GitHub CI runners allow it. Tests that need a UDP send
    /// probe first and skip honestly here instead of failing
    /// on a sandbox rule that has nothing to do with Niva.
    fn udp_loopback_works() -> bool {
        let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
            return false;
        };
        let Ok(port) = sock.local_addr().map(|a| a.port()) else {
            return false;
        };
        sock.send_to(b"p", ("127.0.0.1", port)).is_ok()
    }

    /// Open the gate and allow loopback for the test, holding
    /// both the network-policy lock and the registry lock (in
    /// that order, everywhere) so no other test can move the
    /// gate or the registry mid-test. The caller holds the
    /// guards for the whole test; the registry is cleared.
    fn gate_open() -> (
        std::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let net = crate::network::NET_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let sock = sock_test_lock();
        test_clear();
        (net, sock)
    }

    /// Close the gate (the registry is cleared too). The
    /// loopback allow-list is NOT cleared — it only grows —
    /// so a test that needs a loopback refusal must use an
    /// address no other test ever allowed (127.0.0.3 below).
    fn gate_closed() -> (
        std::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let net = crate::network::NET_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::network::set_network_allowed(false);
        let sock = sock_test_lock();
        test_clear();
        (net, sock)
    }

    /// A one-shot TCP echo server on the given loopback
    /// address: accepts one client, echoes one message back,
    /// closes. Returns the port it bound.
    fn tcp_echo_server(addr: &str) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind((addr, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                if let Ok(n) = stream.read(&mut buf) {
                    if n > 0 {
                        let _ = stream.write_all(&buf[..n]);
                    }
                }
            }
        });
        (port, handle)
    }

    /// A one-shot UDP echo server: echoes one datagram back.
    fn udp_echo_server() -> (u16, std::thread::JoinHandle<()>) {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            if let Ok((n, src)) = sock.recv_from(&mut buf) {
                let _ = sock.send_to(&buf[..n], src);
            }
        });
        (port, handle)
    }

    #[test]
    fn create_close_roundtrip() {
        let _g = gate_open();
        let tcp = create_socket("tcp", 1, 1).unwrap();
        assert!(tcp > 0);
        let udp = create_socket("udp", 1, 1).unwrap();
        assert_ne!(udp, tcp);
        // A UDP socket already has an ephemeral local port.
        assert!(local_port(udp, 1, 1).unwrap() > 0);
        // An unconnected TCP socket has no local port yet.
        let err = local_port(tcp, 1, 1).unwrap_err().to_string();
        assert!(err.contains("غير متصل"), "got: {}", err);
        close_socket(tcp, 1, 1).unwrap();
        close_socket(udp, 1, 1).unwrap();
    }

    #[test]
    fn bad_kind_is_arabic() {
        let _g = gate_open();
        for bad in ["", "TCP", "tcps", "خيط"] {
            let err = create_socket(bad, 1, 1).unwrap_err().to_string();
            assert!(err.contains("tcp أو udp"), "got: {}", err);
        }
    }

    #[test]
    fn unknown_and_double_close_error() {
        let _g = gate_open();
        let err = close_socket(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("مقبس غير معروف"), "got: {}", err);
        let id = create_socket("udp", 1, 1).unwrap();
        close_socket(id, 1, 1).unwrap();
        let err = close_socket(id, 1, 1).unwrap_err().to_string();
        assert!(err.contains("مقبس غير معروف"), "got: {}", err);
        let err = send(id, "x", 1, 1).unwrap_err().to_string();
        assert!(err.contains("مقبس غير معروف"), "got: {}", err);
    }

    #[test]
    fn closed_gate_refuses() {
        let _g = gate_closed();
        let err = create_socket("tcp", 1, 1).unwrap_err().to_string();
        assert!(err.contains("الشبكة مغلقة افتراضيا"), "got: {}", err);
        let err = connect_new("127.0.0.1", 9, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("الشبكة مغلقة افتراضيا"), "got: {}", err);
        let err = listen(0, None, 1, 1).unwrap_err().to_string();
        assert!(err.contains("الشبكة مغلقة افتراضيا"), "got: {}", err);
    }

    #[test]
    fn private_refused() {
        let _g = gate_open();
        let err = connect_new("10.1.2.3", 80, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مرفوض"), "got: {}", err);
        assert!(err.contains("خاص"), "got: {}", err);
        let id = create_socket("udp", 1, 1).unwrap();
        let err = send_to(id, "192.168.1.1", 9, "x", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مرفوض"), "got: {}", err);
        assert!(err.contains("خاص"), "got: {}", err);
        close_socket(id, 1, 1).unwrap();
    }

    #[test]
    fn loopback_refused_without_allow() {
        // 127.0.0.3: loopback, and no test ever allows it —
        // the refusal must not depend on test order.
        let _g = gate_open();
        let err = connect_new("127.0.0.3", 9, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مرفوض"), "got: {}", err);
        assert!(err.contains("حلقي"), "got: {}", err);
    }

    #[test]
    fn tcp_echo_roundtrip() {
        let _g = gate_open();
        let (port, server) = tcp_echo_server("127.0.0.1");
        let id = connect_new("127.0.0.1", port as i64, 1, 1).unwrap();
        // The 30-second read timeout is armed on the stream.
        {
            let reg = REGISTRY.lock().unwrap();
            match reg.get(&id) {
                Some(SocketEntry::TcpConnected(s)) => {
                    assert_eq!(
                        s.read_timeout().unwrap(),
                        Some(Duration::from_secs(30))
                    );
                }
                _ => panic!("expected a connected TCP socket"),
            }
        }
        let sent = send(id, "مرحبا", 1, 1).unwrap();
        assert_eq!(sent, "مرحبا".len() as u64);
        assert_eq!(recv(id, 1, 1).unwrap(), "مرحبا");
        assert!(local_port(id, 1, 1).unwrap() > 0);
        close_socket(id, 1, 1).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn connect_existing_roundtrip() {
        let _g = gate_open();
        let (port, server) = tcp_echo_server("127.0.0.1");
        let id = create_socket("tcp", 1, 1).unwrap();
        // An unconnected socket refuses to send first.
        let err = send(id, "x", 1, 1).unwrap_err().to_string();
        assert!(err.contains("غير متصل"), "got: {}", err);
        let back = connect_existing(id, "127.0.0.1", port as i64, 1, 1).unwrap();
        assert_eq!(back, id);
        // Already connected: a second connect is an error.
        let err = connect_existing(id, "127.0.0.1", port as i64, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("متصل بالفعل"), "got: {}", err);
        send(id, "سلام", 1, 1).unwrap();
        assert_eq!(recv(id, 1, 1).unwrap(), "سلام");
        close_socket(id, 1, 1).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn listen_accept_roundtrip() {
        let _g = gate_open();
        let listener = listen(0, None, 1, 1).unwrap();
        let port = local_port(listener, 1, 1).unwrap();
        assert!(port > 0);
        // A client thread connects, asks, and reads the reply.
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all("سؤال".as_bytes()).unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap();
            String::from_utf8(buf[..n].to_vec()).unwrap()
        });
        let conn = accept(listener, 1, 1).unwrap();
        assert_ne!(conn, listener);
        assert_eq!(recv(conn, 1, 1).unwrap(), "سؤال");
        send(conn, "جواب", 1, 1).unwrap();
        assert_eq!(client.join().unwrap(), "جواب");
        close_socket(conn, 1, 1).unwrap();
        close_socket(listener, 1, 1).unwrap();
    }

    #[test]
    fn udp_echo_roundtrip() {
        if !udp_loopback_works() {
            eprintln!("تخطَّى: UDP loopback محظور في هذا الصندوق (EPERM) — يعمل في CI");
            return;
        }
        let _g = gate_open();
        let (port, server) = udp_echo_server();
        let id = create_socket("udp", 1, 1).unwrap();
        let sent = send_to(id, "127.0.0.1", port as i64, "حزمة", 1, 1).unwrap();
        assert_eq!(sent, "حزمة".len() as u64);
        let (text, host, from_port) = recv_from(id, 1, 1).unwrap();
        assert_eq!(text, "حزمة");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(from_port, port);
        close_socket(id, 1, 1).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn udp_bind_fixed_port() {
        let _g = gate_open();
        // Pick a free port, then bind the Niva socket to it.
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let id = create_socket("udp", 1, 1).unwrap();
        bind_udp(id, port as i64, 1, 1).unwrap();
        assert_eq!(local_port(id, 1, 1).unwrap(), port);
        // A datagram sent to the fixed port arrives — needs a
        // UDP send, which this sandbox blocks (see
        // udp_loopback_works): skip that half honestly here.
        if !udp_loopback_works() {
            eprintln!("تخطَّى: UDP loopback محظور في هذا الصندوق (EPERM) — يعمل في CI");
            close_socket(id, 1, 1).unwrap();
            return;
        }
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to("نداء".as_bytes(), ("127.0.0.1", port))
            .unwrap();
        let (text, _, _) = recv_from(id, 1, 1).unwrap();
        assert_eq!(text, "نداء");
        close_socket(id, 1, 1).unwrap();
    }

    #[test]
    fn wrong_kind_errors_are_arabic() {
        let _g = gate_open();
        let listener = listen(0, None, 1, 1).unwrap();
        let err = send(listener, "x", 1, 1).unwrap_err().to_string();
        assert!(err.contains("المستمع لا يرسل"), "got: {}", err);
        // NOTE: accept() on a real listener waits up to 30
        // seconds for a client, so it is NOT called here —
        // the non-listener kind check below covers the error
        // path, and listen_accept_roundtrip covers success.
        let udp = create_socket("udp", 1, 1).unwrap();
        let err = send(udp, "x", 1, 1).unwrap_err().to_string();
        assert!(err.contains("أرسل_إلى"), "got: {}", err);
        let err = send_to(listener, "127.0.0.1", 9, "x", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("المستمع لا يرسل"), "got: {}", err);
        let err = bind_udp(listener, 9, 1, 1).unwrap_err().to_string();
        assert!(err.contains("UDP فقط"), "got: {}", err);
        let tcp = create_socket("tcp", 1, 1).unwrap();
        let err = accept(tcp, 1, 1).unwrap_err().to_string();
        assert!(err.contains("مستمع فقط"), "got: {}", err);
        let err = connect_existing(udp, "127.0.0.1", 9, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("UDP لا يتصل"), "got: {}", err);
        close_socket(listener, 1, 1).unwrap();
        close_socket(udp, 1, 1).unwrap();
        close_socket(tcp, 1, 1).unwrap();
    }

    #[test]
    fn bad_port_refused() {
        let _g = gate_open();
        for bad in [0, -1, 65536, 100000] {
            let err = connect_new("127.0.0.1", bad, 1, 1)
                .unwrap_err()
                .to_string();
            assert!(err.contains("منفذا بين 1 و 65535"), "got: {}", err);
        }
        let err = listen(70000, None, 1, 1).unwrap_err().to_string();
        assert!(err.contains("منفذا بين 0 و 65535"), "got: {}", err);
        let id = create_socket("udp", 1, 1).unwrap();
        // Port 0 is valid for a bind (ephemeral) — -1 is not.
        bind_udp(id, 0, 1, 1).unwrap();
        let err = bind_udp(id, -1, 1, 1).unwrap_err().to_string();
        assert!(err.contains("منفذا بين 0 و 65535"), "got: {}", err);
        close_socket(id, 1, 1).unwrap();
    }

    #[test]
    fn listen_bad_host_refused() {
        let _g = gate_open();
        let err = listen(0, Some("8.8.8.8"), 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("يربط محليا فقط"), "got: {}", err);
        // 0.0.0.0 with an explicit host is allowed.
        let id = listen(0, Some("0.0.0.0"), 1, 1).unwrap();
        assert!(local_port(id, 1, 1).unwrap() > 0);
        close_socket(id, 1, 1).unwrap();
    }

    #[test]
    fn live_cap_enforced() {
        let _g = gate_open();
        let mut ids = Vec::new();
        for _ in 0..MAX_LIVE_SOCKETS {
            ids.push(create_socket("udp", 1, 1).unwrap());
        }
        let err = create_socket("udp", 1, 1).unwrap_err().to_string();
        assert!(err.contains("تجاوز عدد المقابس"), "got: {}", err);
        // After closing one, creating works again.
        close_socket(ids.pop().unwrap(), 1, 1).unwrap();
        let id = create_socket("udp", 1, 1).unwrap();
        ids.push(id);
        for id in ids {
            close_socket(id, 1, 1).unwrap();
        }
    }

    #[test]
    fn wasm_refusal() {
        // Compiled on the host, executed only on wasm32.
        #[cfg(target_arch = "wasm32")]
        {
            let _g = gate_open();
            for err in [
                create_socket("tcp", 1, 1).unwrap_err(),
                connect_new("127.0.0.1", 9, 1, 1).unwrap_err(),
                listen(0, None, 1, 1).unwrap_err(),
                accept(1, 1, 1).unwrap_err(),
                bind_udp(1, 9, 1, 1).unwrap_err(),
                send(1, "x", 1, 1).unwrap_err(),
                recv(1, 1, 1).unwrap_err(),
                send_to(1, "127.0.0.1", 9, "x", 1, 1).unwrap_err(),
                recv_from(1, 1, 1).unwrap_err(),
                local_port(1, 1, 1).unwrap_err(),
                close_socket(1, 1, 1).unwrap_err(),
            ] {
                assert!(
                    err.to_string().contains("غير مدعومة في متصفح الويب"),
                    "got: {}",
                    err
                );
            }
        }
    }
}
