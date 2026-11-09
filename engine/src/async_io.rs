//! Async I/O primitives for the `غير_متزامن` native module
//! (catalog task 19, from io_uring/): submit a file or
//! socket operation and collect its result later — the
//! submit returns at once (a worker thread does the I/O),
//! so a program — and the REPL — never blocks on it. One
//! implementation, shared by both engines (interpreter +
//! VM) through the stdlib native functions — never
//! implemented twice.
//!
//! Design (written before the code — see
//! `CATALOG-11-ASYNC.md` §1): one worker thread per
//! submitted operation; a loop is a registry of pending
//! operations plus a results channel (the تواصل pattern);
//! no io_uring (no raw syscall ever), no tokio runtime,
//! no new dependencies — std only. File paths resolve
//! through the session FilePolicy at submit time; socket
//! operations check the network gate at submit time and
//! clone the socket then. I/O failures arrive as result
//! dicts (with خطأ), never as raised errors — the submit
//! already returned.

use crate::error::{RioError, RioResult};
use crate::permissions::FilePolicy;
use crate::value::Value;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{mpsc, Arc, LazyLock, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// At most 64 live loops (the channels/sockets cap).
pub const MAX_LIVE_LOOPS: usize = 64;
/// At most 64 pending operations per loop (submitted −
/// collected); enforced at submit, so a worker never
/// blocks sending its result.
pub const MAX_PENDING_PER_LOOP: usize = 64;

/// The wait for the first result (the تواصل/fetch 30s).
#[cfg(not(target_arch = "wasm32"))]
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// The results channel capacity (the تواصل reader queue).
#[cfg(not(target_arch = "wasm32"))]
const RESULTS_QUEUE: usize = 64;

/// One completed operation: its id, its kind, and either
/// a payload (text for a read/receive, byte count for a
/// write/send) or an Arabic error message.
#[cfg(not(target_arch = "wasm32"))]
enum AsyncPayload {
    Text(String),
    Bytes(u64),
}

#[cfg(not(target_arch = "wasm32"))]
struct AsyncResult {
    op: u64,
    kind: &'static str,
    outcome: Result<AsyncPayload, String>,
}

/// A live loop: the results sender (workers hold clones),
/// the results receiver, and how many operations were
/// submitted but not yet collected.
#[cfg(not(target_arch = "wasm32"))]
struct LoopEntry {
    tx: mpsc::SyncSender<AsyncResult>,
    rx: Arc<Mutex<mpsc::Receiver<AsyncResult>>>,
    pending: usize,
}

/// Live loops: id -> entry. One table for the whole OS
/// process, shared by both engines (a Niva program is one
/// OS process, so it has one loop table). On WASM it is
/// compiled out.
#[cfg(not(target_arch = "wasm32"))]
static LOOPS: LazyLock<Mutex<HashMap<u64, LoopEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Loop ids and operation ids start at 1 and never repeat
/// in one run.
#[cfg(not(target_arch = "wasm32"))]
static NEXT_LOOP_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(not(target_arch = "wasm32"))]
static NEXT_OP_ID: AtomicU64 = AtomicU64::new(1);

/// Create a loop: `غير_متزامن.أنشئ_حلقة()` — returns the
/// loop id.
pub fn create_loop(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = line;
        let _ = col;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        create_loop_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn create_loop_native(line: usize, col: usize) -> RioResult<u64> {
    let mut loops = LOOPS.lock().map_err(|_| {
        RioError::runtime("تعذّر إنشاء الحلقة", line, col)
    })?;
    if loops.len() >= MAX_LIVE_LOOPS {
        return Err(RioError::runtime(
            "بلغ عدد الحلقات الحد الأقصى (64 حلقة)",
            line,
            col,
        ));
    }
    let (tx, rx) = mpsc::sync_channel::<AsyncResult>(RESULTS_QUEUE);
    let id = NEXT_LOOP_ID.fetch_add(1, Ordering::SeqCst);
    loops.insert(
        id,
        LoopEntry {
            tx,
            rx: Arc::new(Mutex::new(rx)),
            pending: 0,
        },
    );
    Ok(id)
}

/// Reserve one pending slot in a loop and hand out the
/// operation id plus a results sender for the worker.
/// The slot is taken under the registry lock together
/// with the check, so the cap can never be overrun.
#[cfg(not(target_arch = "wasm32"))]
fn reserve_op(
    loop_id: u64,
    line: usize,
    col: usize,
) -> RioResult<(u64, mpsc::SyncSender<AsyncResult>)> {
    let mut loops = LOOPS.lock().map_err(|_| {
        RioError::runtime(format!("حلقة غير معروفة: {}", loop_id), line, col)
    })?;
    let entry = loops.get_mut(&loop_id).ok_or_else(|| {
        RioError::runtime(format!("حلقة غير معروفة: {}", loop_id), line, col)
    })?;
    if entry.pending >= MAX_PENDING_PER_LOOP {
        return Err(RioError::runtime(
            "الحلقة ممتلئة (64 عملية معلقة)",
            line,
            col,
        ));
    }
    entry.pending += 1;
    let op = NEXT_OP_ID.fetch_add(1, Ordering::SeqCst);
    Ok((op, entry.tx.clone()))
}

/// Give a reserved slot back (the worker could not start).
#[cfg(not(target_arch = "wasm32"))]
fn release_op(loop_id: u64) {
    if let Ok(mut loops) = LOOPS.lock() {
        if let Some(entry) = loops.get_mut(&loop_id) {
            entry.pending = entry.pending.saturating_sub(1);
        }
    }
}

/// The worker side of a file read: open the resolved path,
/// seek to the offset, read to the end under the same 5 MiB
/// cap as `اقرأ_ملف` (files.rs wording, no OS text).
#[cfg(not(target_arch = "wasm32"))]
fn read_file_worker(
    path: &std::path::Path,
    display: &str,
    offset: u64,
) -> Result<AsyncPayload, String> {
    let cap = crate::interpreter::Interpreter::MAX_READ_BYTES;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Err(format!("تعذر قراءة الملف {}", display)),
    };
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Err(format!("تعذر قراءة الملف {}", display));
    }
    let mut buf = Vec::new();
    if (&mut f)
        .take((cap + 1) as u64)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Err(format!("تعذر قراءة الملف {}", display));
    }
    if buf.len() > cap {
        return Err(format!(
            "الملف {} أكبر من الحد الأقصى (5 ميغابايت)",
            display
        ));
    }
    match String::from_utf8(buf) {
        Ok(text) => Ok(AsyncPayload::Text(text)),
        Err(_) => Err(format!("الملف {} ليس نصا صالحا (UTF-8)", display)),
    }
}

/// Submit a file read: `غير_متزامن.اقرأ(حلقة، مسار،
/// موضع)` — returns the operation id at once. The path
/// resolves through the session FilePolicy here, at
/// submit time, so a policy error raises synchronously.
pub fn submit_read(
    policy: &FilePolicy,
    cwd: &std::path::Path,
    loop_id: u64,
    path: &str,
    offset: u64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (policy, cwd, loop_id, path, offset);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        submit_read_native(policy, cwd, loop_id, path, offset, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn submit_read_native(
    policy: &FilePolicy,
    cwd: &std::path::Path,
    loop_id: u64,
    path: &str,
    offset: u64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    let resolved = policy.resolve_in(cwd, path, line, col)?;
    let display = policy.display_str(path);
    let (op, tx) = reserve_op(loop_id, line, col)?;
    let spawn = std::thread::Builder::new().spawn(move || {
        let outcome = read_file_worker(&resolved, &display, offset);
        let _ = tx.send(AsyncResult {
            op,
            kind: "قراءة",
            outcome,
        });
    });
    if spawn.is_err() {
        release_op(loop_id);
        return Err(RioError::runtime(
            "تعذّر بدء العملية غير المتزامنة",
            line,
            col,
        ));
    }
    Ok(op)
}

/// The worker side of a file write: quota check + write,
/// both under the shared file-write lock (files.rs), so
/// the quota can never be overrun by concurrent writers.
/// Same 100 MiB quota and wording as `اكتب_ملف`.
#[cfg(not(target_arch = "wasm32"))]
fn write_file_worker(
    path: &std::path::Path,
    display: &str,
    base: &std::path::Path,
    text: &str,
    line: usize,
    col: usize,
) -> Result<AsyncPayload, String> {
    let _guard = crate::files::FILE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let existing = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let extra = (text.len() as u64).saturating_sub(existing);
    if let Err(e) = crate::files::check_disk_quota(base, extra, line, col) {
        return Err(e.message);
    }
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)
    {
        Ok(f) => f,
        Err(_) => return Err(format!("تعذر كتابة الملف {}", display)),
    };
    match f.write_all(text.as_bytes()) {
        Ok(()) => Ok(AsyncPayload::Bytes(text.len() as u64)),
        Err(_) => Err(format!("تعذر كتابة الملف {}", display)),
    }
}

/// Submit a file write: `غير_متزامن.اكتب(حلقة، مسار،
/// نص)` — replaces the file whole, returns the operation
/// id at once. The path resolves through the session
/// FilePolicy here, at submit time.
pub fn submit_write(
    policy: &FilePolicy,
    cwd: &std::path::Path,
    loop_id: u64,
    path: &str,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (policy, cwd, loop_id, path, text);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        submit_write_native(policy, cwd, loop_id, path, text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn submit_write_native(
    policy: &FilePolicy,
    cwd: &std::path::Path,
    loop_id: u64,
    path: &str,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    let resolved = policy.resolve_in_for_write(cwd, path, line, col)?;
    let display = policy.display_str(path);
    let base: PathBuf = policy.base.clone();
    let text_owned = text.to_string();
    let (op, tx) = reserve_op(loop_id, line, col)?;
    let spawn = std::thread::Builder::new().spawn(move || {
        let outcome = write_file_worker(&resolved, &display, &base, &text_owned, line, col);
        let _ = tx.send(AsyncResult {
            op,
            kind: "كتابة",
            outcome,
        });
    });
    if spawn.is_err() {
        release_op(loop_id);
        return Err(RioError::runtime(
            "تعذّر بدء العملية غير المتزامنة",
            line,
            col,
        ));
    }
    Ok(op)
}

/// The worker side of a socket send: one write_all on the
/// cloned stream (same wording as `شبكة.أرسل`).
#[cfg(not(target_arch = "wasm32"))]
fn send_worker(
    mut stream: std::net::TcpStream,
    text: String,
) -> Result<AsyncPayload, String> {
    match stream.write_all(text.as_bytes()) {
        Ok(()) => Ok(AsyncPayload::Bytes(text.len() as u64)),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::TimedOut {
                Err("انتهت مهلة الإرسال (30 ثانية)".to_string())
            } else {
                Err("تعذر الإرسال على المقبس".to_string())
            }
        }
    }
}

/// Submit a socket send: `غير_متزامن.أرسل(حلقة،
/// معرف_مقبس، نص)` — returns the operation id at once.
/// The gate is checked here, at submit time, and the
/// socket is cloned here, so a bad socket id, an
/// unconnected socket, a listener, or UDP raises
/// synchronously with `شبكة`'s own messages.
pub fn submit_send(
    loop_id: u64,
    socket_id: u64,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (loop_id, socket_id, text);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        submit_send_native(loop_id, socket_id, text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn submit_send_native(
    loop_id: u64,
    socket_id: u64,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    crate::network::require_network_allowed(line, col)?;
    let stream = crate::sockets::clone_tcp(socket_id, line, col)?;
    let text_owned = text.to_string();
    let (op, tx) = reserve_op(loop_id, line, col)?;
    let spawn = std::thread::Builder::new().spawn(move || {
        let outcome = send_worker(stream, text_owned);
        let _ = tx.send(AsyncResult {
            op,
            kind: "إرسال",
            outcome,
        });
    });
    if spawn.is_err() {
        release_op(loop_id);
        return Err(RioError::runtime(
            "تعذّر بدء العملية غير المتزامنة",
            line,
            col,
        ));
    }
    Ok(op)
}

/// The worker side of a socket receive: one read of up to
/// 64 KiB on the cloned stream (same wording as
/// `شبكة.استقبل` — a clean peer close is an Arabic error,
/// never an empty string).
#[cfg(not(target_arch = "wasm32"))]
fn recv_worker(
    mut stream: std::net::TcpStream,
) -> Result<AsyncPayload, String> {
    let mut buf = vec![0u8; crate::sockets::MAX_RECV_BYTES];
    match stream.read(&mut buf) {
        Ok(0) => Err("أغلق الطرف الآخر الاتصال".to_string()),
        Ok(n) => {
            buf.truncate(n);
            match String::from_utf8(buf) {
                Ok(text) => Ok(AsyncPayload::Text(text)),
                Err(_) => {
                    Err("البيانات المستقبلة ليست نصا صالحا (UTF-8)".to_string())
                }
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::TimedOut {
                Err("انتهت مهلة الاستقبال (30 ثانية)".to_string())
            } else {
                Err("تعذر الاستقبال من المقبس".to_string())
            }
        }
    }
}

/// Submit a socket receive: `غير_متزامن.استقبل(حلقة،
/// معرف_مقبس)` — returns the operation id at once. The
/// gate is checked and the socket cloned here, at submit
/// time (same as `أرسل`).
pub fn submit_recv(
    loop_id: u64,
    socket_id: u64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (loop_id, socket_id);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        submit_recv_native(loop_id, socket_id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn submit_recv_native(
    loop_id: u64,
    socket_id: u64,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    crate::network::require_network_allowed(line, col)?;
    let stream = crate::sockets::clone_tcp(socket_id, line, col)?;
    let (op, tx) = reserve_op(loop_id, line, col)?;
    let spawn = std::thread::Builder::new().spawn(move || {
        let outcome = recv_worker(stream);
        let _ = tx.send(AsyncResult {
            op,
            kind: "استقبال",
            outcome,
        });
    });
    if spawn.is_err() {
        release_op(loop_id);
        return Err(RioError::runtime(
            "تعذّر بدء العملية غير المتزامنة",
            line,
            col,
        ));
    }
    Ok(op)
}

/// One result as a Niva dict: success is {المعرف،
/// النوع، المحتوى} (text for a read/receive, byte count
/// for a write/send); failure is {المعرف، النوع، خطأ}.
#[cfg(not(target_arch = "wasm32"))]
fn result_value(r: AsyncResult) -> Value {
    let id = Value::Number(r.op as f64);
    let kind = Value::Str(r.kind.to_string());
    match r.outcome {
        Ok(AsyncPayload::Text(text)) => Value::Dict(vec![
            ("المعرف".to_string(), id),
            ("النوع".to_string(), kind),
            ("المحتوى".to_string(), Value::Str(text)),
        ]),
        Ok(AsyncPayload::Bytes(n)) => Value::Dict(vec![
            ("المعرف".to_string(), id),
            ("النوع".to_string(), kind),
            ("المحتوى".to_string(), Value::Number(n as f64)),
        ]),
        Err(msg) => Value::Dict(vec![
            ("المعرف".to_string(), id),
            ("النوع".to_string(), kind),
            ("خطأ".to_string(), Value::Str(msg)),
        ]),
    }
}

/// Collect every result that is ready on this loop's
/// receiver: wait at most `timeout` for the first one,
/// then drain the rest without waiting. Used by
/// `انتظر_النتائج` (30s) and by the unit tests (short).
#[cfg(not(target_arch = "wasm32"))]
fn collect_results(
    rx_arc: &Arc<Mutex<mpsc::Receiver<AsyncResult>>>,
    timeout: Duration,
) -> Vec<AsyncResult> {
    let mut collected: Vec<AsyncResult> = Vec::new();
    let rx = match rx_arc.lock() {
        Ok(g) => g,
        Err(_) => return collected,
    };
    match rx.recv_timeout(timeout) {
        Ok(first) => {
            collected.push(first);
            while let Ok(next) = rx.try_recv() {
                collected.push(next);
            }
        }
        Err(_) => {}
    }
    collected
}

/// Wait for results: `غير_متزامن.انتظر_النتائج(حلقة)` —
/// waits at most 30 seconds for the first completed
/// operation, then returns every ready result as a list
/// of dicts (empty when nothing completed in time — not
/// an error). Each result is delivered once.
pub fn wait_results(loop_id: u64, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = loop_id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wait_results_native(loop_id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_results_native(loop_id: u64, line: usize, col: usize) -> RioResult<Value> {
    let rx_arc = {
        let loops = LOOPS.lock().map_err(|_| {
            RioError::runtime(format!("حلقة غير معروفة: {}", loop_id), line, col)
        })?;
        match loops.get(&loop_id) {
            Some(entry) => Arc::clone(&entry.rx),
            None => {
                return Err(RioError::runtime(
                    format!("حلقة غير معروفة: {}", loop_id),
                    line,
                    col,
                ))
            }
        }
    };
    let collected = collect_results(&rx_arc, WAIT_TIMEOUT);
    let n = collected.len();
    if n > 0 {
        if let Ok(mut loops) = LOOPS.lock() {
            if let Some(entry) = loops.get_mut(&loop_id) {
                entry.pending = entry.pending.saturating_sub(n);
            }
        }
    }
    Ok(Value::List(
        collected.into_iter().map(result_value).collect(),
    ))
}

/// Test-only serialization for the process-wide loop
/// registry: cargo runs tests in parallel threads, and
/// the registry is a single static. Every test that
/// touches it locks this first (same pattern as
/// IPC_TEST_LOCK in ipc.rs).
#[cfg(test)]
pub(crate) static ASYNC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Test-only: drop every loop (the registry is
/// process-wide and shared by the unit tests). Workers
/// still running send into a dropped receiver and their
/// send fails silently — no panic.
#[cfg(test)]
pub(crate) fn test_clear() {
    if let Ok(mut loops) = LOOPS.lock() {
        loops.clear();
    }
}

/// Test-only: how many operations are still pending in a
/// loop (submitted − collected).
#[cfg(test)]
pub(crate) fn test_pending(loop_id: u64) -> Option<usize> {
    LOOPS
        .lock()
        .ok()
        .and_then(|loops| loops.get(&loop_id).map(|e| e.pending))
}

/// Test-only: the receiver of a loop, for short-timeout
/// collection without waiting the public 30 seconds.
#[cfg(test)]
fn test_receiver(
    loop_id: u64,
) -> Option<Arc<Mutex<mpsc::Receiver<AsyncResult>>>> {
    LOOPS
        .lock()
        .ok()
        .and_then(|loops| loops.get(&loop_id).map(|e| Arc::clone(&e.rx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU64 as Counter;

    /// Lock the test serialization lock, recovering from
    /// poisoning (same pattern as ipc.rs).
    fn async_test_lock() -> std::sync::MutexGuard<'static, ()> {
        ASYNC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A fresh temp dir per test (same pattern as ipc.rs:
    /// the counter lives here, ungated, so this helper
    /// compiles on every target).
    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: Counter = Counter::new(1);
        let dir = std::env::temp_dir().join(format!(
            "niva-async-test-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The policy + cwd of a fresh temp dir (restricted to
    /// it, like a program folder).
    fn policy_in(dir: &std::path::Path) -> (FilePolicy, std::path::PathBuf) {
        (
            FilePolicy::restricted(dir.to_path_buf()),
            dir.to_path_buf(),
        )
    }

    /// The dict of one result, as (kind, content-or-error).
    /// Panics on a wrong shape — the test then reports its
    /// own failure, not a silent mismatch.
    fn result_shape(v: &Value) -> (String, Option<String>, Option<String>) {
        match v {
            Value::Dict(pairs) => {
                let get = |k: &str| {
                    pairs
                        .iter()
                        .find(|(key, _)| key == k)
                        .map(|(_, val)| val.clone())
                };
                let kind = match get("النوع") {
                    Some(Value::Str(s)) => s,
                    _ => panic!("result has no النوع: {:?}", v),
                };
                let content = match get("المحتوى") {
                    Some(Value::Str(s)) => Some(s),
                    Some(Value::Number(n)) => Some(format!("{}", n as i64)),
                    _ => None,
                };
                let error = match get("خطأ") {
                    Some(Value::Str(s)) => Some(s),
                    _ => None,
                };
                (kind, content, error)
            }
            other => panic!("expected a result dict, got {:?}", other),
        }
    }

    /// Collect until `want` results arrived (each public
    /// wait returns at least one while operations are
    /// outstanding, so this never waits the full 30s on a
    /// live loop — workers finish in milliseconds).
    fn collect_all(loop_id: u64, want: usize) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        while out.len() < want {
            match wait_results(loop_id, 1, 1) {
                Ok(Value::List(items)) => out.extend(items),
                Ok(other) => panic!("wait_results returned {:?}", other),
                Err(e) => panic!("wait_results failed: {}", e.message),
            }
        }
        out
    }

    #[test]
    fn loop_ids_are_distinct_and_positive() {
        let _guard = async_test_lock();
        test_clear();
        let a = create_loop(1, 1).unwrap();
        let b = create_loop(1, 1).unwrap();
        assert!(a >= 1);
        assert!(b > a);
    }

    #[test]
    fn read_roundtrip() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("read");
        let (policy, cwd) = policy_in(&dir);
        fs::write(dir.join("رسالة.txt"), "مرحبا من غير_متزامن").unwrap();
        let lp = create_loop(1, 1).unwrap();
        let op = submit_read(&policy, &cwd, lp, "رسالة.txt", 0, 1, 1).unwrap();
        assert!(op >= 1);
        let results = collect_all(lp, 1);
        let (kind, content, error) = result_shape(&results[0]);
        assert_eq!(kind, "قراءة");
        assert_eq!(content.as_deref(), Some("مرحبا من غير_متزامن"));
        assert!(error.is_none());
        assert_eq!(test_pending(lp), Some(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_from_offset_and_past_end() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("offset");
        let (policy, cwd) = policy_in(&dir);
        fs::write(dir.join("أ.txt"), "أبجد").unwrap();
        let lp = create_loop(1, 1).unwrap();
        // «أبجد» is 8 UTF-8 bytes: offset 4 skips «أب».
        let _ = submit_read(&policy, &cwd, lp, "أ.txt", 4, 1, 1).unwrap();
        let _ = submit_read(&policy, &cwd, lp, "أ.txt", 100, 1, 1).unwrap();
        let results = collect_all(lp, 2);
        let by_kind: Vec<&Value> = results.iter().collect();
        assert_eq!(by_kind.len(), 2);
        // Both are reads; the past-end one is an empty text.
        let mut texts: Vec<String> = Vec::new();
        for r in &results {
            let (_, content, error) = result_shape(r);
            assert!(error.is_none());
            texts.push(content.unwrap());
        }
        assert!(texts.contains(&"جد".to_string()));
        assert!(texts.contains(&"".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_roundtrip_then_file_holds_text() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("write");
        let (policy, cwd) = policy_in(&dir);
        let lp = create_loop(1, 1).unwrap();
        let op = submit_write(&policy, &cwd, lp, "جديد.txt", "نص جديد", 1, 1).unwrap();
        assert!(op >= 1);
        let results = collect_all(lp, 1);
        let (kind, content, error) = result_shape(&results[0]);
        assert_eq!(kind, "كتابة");
        assert_eq!(content.as_deref(), Some("13"));
        assert!(error.is_none());
        assert_eq!(
            fs::read_to_string(dir.join("جديد.txt")).unwrap(),
            "نص جديد"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_a_result_error_not_a_raise() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("missing");
        let (policy, cwd) = policy_in(&dir);
        let lp = create_loop(1, 1).unwrap();
        let _ = submit_read(&policy, &cwd, lp, "مفقود.txt", 0, 1, 1).unwrap();
        let results = collect_all(lp, 1);
        let (kind, _, error) = result_shape(&results[0]);
        assert_eq!(kind, "قراءة");
        let msg = error.expect("missing file must fail");
        assert!(msg.contains("تعذر قراءة الملف"), "got: {}", msg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_utf8_is_a_result_error() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("utf8");
        let (policy, cwd) = policy_in(&dir);
        fs::write(dir.join("خام.bin"), [0xFFu8, 0xFE, 0x41]).unwrap();
        let lp = create_loop(1, 1).unwrap();
        let _ = submit_read(&policy, &cwd, lp, "خام.bin", 0, 1, 1).unwrap();
        let results = collect_all(lp, 1);
        let (_, _, error) = result_shape(&results[0]);
        let msg = error.expect("invalid UTF-8 must fail");
        assert!(msg.contains("ليس نصا صالحا"), "got: {}", msg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversize_file_is_a_result_error() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("big");
        let (policy, cwd) = policy_in(&dir);
        let cap = crate::interpreter::Interpreter::MAX_READ_BYTES;
        let big = vec![b'x'; cap + 1];
        fs::write(dir.join("كبير.txt"), &big).unwrap();
        let lp = create_loop(1, 1).unwrap();
        let _ = submit_read(&policy, &cwd, lp, "كبير.txt", 0, 1, 1).unwrap();
        let results = collect_all(lp, 1);
        let (_, _, error) = result_shape(&results[0]);
        let msg = error.expect("oversize must fail");
        assert!(msg.contains("أكبر من الحد الأقصى"), "got: {}", msg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_loop_raises_at_submit() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("unknown");
        let (policy, cwd) = policy_in(&dir);
        let err = submit_read(&policy, &cwd, 999999, "أ.txt", 0, 1, 1)
            .unwrap_err()
            .message;
        assert!(err.contains("حلقة غير معروفة"), "got: {}", err);
        let err = wait_results(999999, 1, 1).unwrap_err().message;
        assert!(err.contains("حلقة غير معروفة"), "got: {}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn policy_refusal_raises_at_submit() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("policy");
        let (policy, cwd) = policy_in(&dir);
        let lp = create_loop(1, 1).unwrap();
        let err = submit_read(&policy, &cwd, lp, "../خارج.txt", 0, 1, 1)
            .unwrap_err()
            .message;
        assert!(
            err.contains("خارج مجلد البرنامج") || err.contains("مجلد البرنامج"),
            "got: {}",
            err
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loop_full_refuses_the_65th() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("full");
        let (policy, cwd) = policy_in(&dir);
        fs::write(dir.join("أ.txt"), "س").unwrap();
        let lp = create_loop(1, 1).unwrap();
        for _ in 0..MAX_PENDING_PER_LOOP {
            submit_read(&policy, &cwd, lp, "أ.txt", 0, 1, 1).unwrap();
        }
        let err = submit_read(&policy, &cwd, lp, "أ.txt", 0, 1, 1)
            .unwrap_err()
            .message;
        assert!(err.contains("الحلقة ممتلئة"), "got: {}", err);
        let _ = collect_all(lp, MAX_PENDING_PER_LOOP);
        assert_eq!(test_pending(lp), Some(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_collect_is_empty_without_waiting() {
        let _guard = async_test_lock();
        test_clear();
        let dir = temp_dir("drain");
        let (policy, cwd) = policy_in(&dir);
        fs::write(dir.join("أ.txt"), "س").unwrap();
        let lp = create_loop(1, 1).unwrap();
        let _ = submit_read(&policy, &cwd, lp, "أ.txt", 0, 1, 1).unwrap();
        let _ = collect_all(lp, 1);
        // The loop is drained: a short-timeout collect finds
        // nothing — no 30-second wait in the test.
        let rx = test_receiver(lp).unwrap();
        let rest = collect_results(&rx, Duration::from_millis(50));
        assert!(rest.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn socket_send_recv_roundtrip() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let _guard = async_test_lock();
        test_clear();
        let net = crate::network::NET_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let _sock = crate::sockets::SOCK_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::sockets::test_clear();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf) {
                    if n > 0 {
                        let _ = stream.write_all(&buf[..n]);
                    }
                }
            }
        });
        let sock = crate::sockets::connect_new("127.0.0.1", port as i64, 1, 1).unwrap();
        let lp = create_loop(1, 1).unwrap();
        let _ = submit_send(lp, sock, "مرحبا", 1, 1).unwrap();
        let _ = submit_recv(lp, sock, 1, 1).unwrap();
        let results = collect_all(lp, 2);
        assert_eq!(results.len(), 2);
        let mut saw_send = false;
        let mut saw_recv = false;
        for r in &results {
            let (kind, content, error) = result_shape(r);
            assert!(error.is_none(), "kind {} failed", kind);
            if kind == "إرسال" {
                saw_send = true;
                assert_eq!(content.as_deref(), Some("10"));
            } else if kind == "استقبال" {
                saw_recv = true;
                assert_eq!(content.as_deref(), Some("مرحبا"));
            }
        }
        assert!(saw_send && saw_recv);
        let _ = crate::sockets::close_socket(sock, 1, 1);
        server.join().unwrap();
        drop(net);
        drop(_sock);
    }

    #[test]
    fn submit_recv_returns_at_once_while_server_is_silent() {
        use std::net::TcpListener;
        let _guard = async_test_lock();
        test_clear();
        let net = crate::network::NET_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let _sock = crate::sockets::SOCK_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::sockets::test_clear();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // The server accepts and then says nothing for 2
        // seconds — a blocking receive would hang here.
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_secs(2));
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        });
        let sock = crate::sockets::connect_new("127.0.0.1", port as i64, 1, 1).unwrap();
        let lp = create_loop(1, 1).unwrap();
        let start = std::time::Instant::now();
        let _ = submit_recv(lp, sock, 1, 1).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "submit blocked the caller"
        );
        // The pending operation is still outstanding — the
        // loop is not drained, so do not wait for it here;
        // the server thread ends on its own in 2 seconds.
        let _ = crate::sockets::close_socket(sock, 1, 1);
        server.join().unwrap();
        drop(net);
        drop(_sock);
    }
}
