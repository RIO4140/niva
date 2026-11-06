//! Pipes between processes for the `تواصل` native module
//! (catalog task 16, from `ipc/`): a parent spawns a
//! subprogram with piped stdin/stdout and exchanges lines
//! with it. One implementation, shared by both engines
//! (interpreter + VM) through the stdlib native functions —
//! never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-08-IPC.md` §1): the spawn policy is
//! `عمليات.أنشئ`'s own policy (program-directory only by
//! default, --اسمح-بالكل lifts it; no shell ever; a `.نيفا`
//! first token runs as a Niva subprogram via the current
//! niva binary) through the shared command builder in
//! processes.rs, so the two modules can never drift apart
//! on the spawn rules. Handles only, never a raw process
//! or pipe: `أنبوب` returns a numeric channel id, every
//! other call takes the id only; at most 64 live channels;
//! one message is one line of at most 64 KiB; `استقبل`
//! waits at most 30 seconds; `أغلق` closes stdin (EOF) and
//! waits at most 30 seconds, then kills; `أغلق`/`أوقف`
//! consume the id. WASM refuses every call with the flat
//! builtins' own message; on Android spawning is refused
//! like `عمليات.أنشئ` (the other calls then find no
//! channel, ever). Named channels and shared memory are NOT
//! here (later steps — see the report §1).

use crate::error::{RioError, RioResult};
#[cfg(not(target_arch = "wasm32"))]
use crate::network::TIMEOUT_SECS;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
/// Reader-only (open_pipe_native): on Android there is no
/// spawn, so the reader thread never exists and these
/// imports would warn dead-code on the aarch64 check.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::io::{BufRead, BufReader};
#[cfg(not(target_arch = "wasm32"))]
use std::process::{Child, ChildStdin};
/// Stdio::piped lives in open_pipe_native only (Android
/// refuses spawning, so no pipe is ever built there).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::process::Stdio;
/// The id counter lives in open_pipe_native only (same
/// reason as Stdio above).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{mpsc, Arc, LazyLock, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

/// One message is one line of at most 64 KiB — the same
/// chunk as a socket read (sockets.rs).
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// At most 64 live channels (the same cap as sockets):
/// a loop that opens channels without closing them cannot
/// exhaust the process's file descriptors or threads.
pub const MAX_LIVE_CHANNELS: usize = 64;

/// How long `استقبل` waits for a line, and how long
/// `أغلق` waits for the child to exit after EOF: the same
/// 30 seconds as the fetch and the sockets.
#[cfg(not(target_arch = "wasm32"))]
const TIMEOUT: Duration = Duration::from_secs(TIMEOUT_SECS);

/// How often the close-wait poll loop wakes up while
/// waiting for the child to exit (same pattern as the
/// sockets' accept poll).
#[cfg(not(target_arch = "wasm32"))]
const CLOSE_POLL: Duration = Duration::from_millis(50);

/// How many unread lines the reader thread may buffer
/// before it blocks: backpressure with bounded memory — a
/// chatty child cannot fill the parent's memory while the
/// parent never reads. Reader-only (open_pipe_native):
/// on Android no reader thread exists.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const READER_QUEUE: usize = 64;

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// A live channel: the child (its stdin here, its stdout
/// in the reader thread), and the line receiver. The child
/// is only waited on or killed after the entry leaves the
/// registry (`أغلق`/`أوقف` own it then), so no lock is
/// ever held across a wait; `أرسل`/`استقبل` clone the Arcs
/// under a short registry lock and never hold the registry
/// lock while blocking.
#[cfg(not(target_arch = "wasm32"))]
struct ChannelEntry {
    child: Child,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    rx: Arc<Mutex<mpsc::Receiver<String>>>,
}

/// Live channels: id -> entry. One table for the whole OS
/// process, shared by both engines (a Niva program is one
/// OS process, so it has one channel table). On Android the
/// table exists but stays empty forever (spawning is
/// refused there); on WASM it is compiled out.
#[cfg(not(target_arch = "wasm32"))]
static REGISTRY: LazyLock<Mutex<HashMap<u64, ChannelEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Channel ids start at 1 and never repeat in one run.
/// open_pipe_native only (Android never opens a channel).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Test-only serialization for the process-wide registry:
/// cargo runs tests in parallel threads, and the registry is
/// a single static. Every test that touches it locks this
/// first (same pattern as SOCK_TEST_LOCK in sockets.rs).
/// Tests that spawn also lock PROC_TEST_LOCK (in
/// processes.rs) first: the spawn policy is process-wide
/// and shared with the عمليات tests, so a policy set by a
/// parallel test would otherwise resolve the spawn against
/// the wrong directory.
#[cfg(test)]
pub(crate) static IPC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lock the test serialization lock, recovering from
/// poisoning: if an earlier test panicked while holding it,
/// the next test must still run and report its OWN result
/// instead of failing with PoisonError (CI 2026-11-06: one
/// macOS panic cascaded into four PoisonErrors that hid
/// every other test's verdict).
#[cfg(test)]
fn ipc_test_lock() -> std::sync::MutexGuard<'static, ()> {
    IPC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test-only: kill every live channel and drop the table.
/// The caller must hold IPC_TEST_LOCK (via ipc_test_lock),
/// so no other ipc test can hold a handle at the time.
#[cfg(test)]
pub(crate) fn test_clear() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let entries: Vec<ChannelEntry> = match REGISTRY.lock() {
            Ok(mut reg) => reg.drain().map(|(_, e)| e).collect(),
            Err(_) => return,
        };
        for mut entry in entries {
            let _ = entry.child.kill();
            let _ = entry.child.wait();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn unknown_channel(id: u64, line: usize, col: usize) -> RioError {
    RioError::runtime(format!("قناة غير معروفة: {}", id), line, col)
}

/// Take a live channel out of the registry, or an Arabic
/// error if the id is unknown (already closed/stopped, or
/// never was).
#[cfg(not(target_arch = "wasm32"))]
fn take_channel(id: u64, line: usize, col: usize) -> RioResult<ChannelEntry> {
    match REGISTRY.lock() {
        Ok(mut reg) => reg.remove(&id).ok_or_else(|| unknown_channel(id, line, col)),
        Err(_) => Err(unknown_channel(id, line, col)),
    }
}

/// Clone the two Arcs of a live channel under a short
/// registry lock, or an Arabic error if the id is unknown.
/// The caller then works on the clones without holding the
/// registry lock (a send may block on a full pipe, a
/// receive may wait 30 seconds — neither may hold the
/// registry lock while doing so).
#[cfg(not(target_arch = "wasm32"))]
fn channel_arcs(
    id: u64,
    line: usize,
    col: usize,
) -> RioResult<(
    Arc<Mutex<Option<ChildStdin>>>,
    Arc<Mutex<mpsc::Receiver<String>>>,
)> {
    match REGISTRY.lock() {
        Ok(reg) => reg
            .get(&id)
            .map(|e| (Arc::clone(&e.stdin), Arc::clone(&e.rx)))
            .ok_or_else(|| unknown_channel(id, line, col)),
        Err(_) => Err(unknown_channel(id, line, col)),
    }
}

/// Open a pipe: `تواصل.أنبوب(أمر)` — spawn a subprogram
/// with piped stdin/stdout (stderr inherited, like
/// عمليات) and return the channel id. The spawn rules are
/// عمليات.أنشئ's own rules, through the shared builder.
pub fn open_pipe(command: &str, line: usize, col: usize) -> RioResult<u64> {
    // WASM (the playground): no processes in a browser.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = command;
        return Err(RioError::runtime(WASM_REFUSAL, line, col));
    }
    // Android (the app): a public product with no shell and
    // no Linux environment — spawning arbitrary binaries
    // from an app process is a security hole, so it is
    // refused (the same message as عمليات.أنشئ).
    #[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
    {
        let _ = command;
        return Err(RioError::runtime(
            "تشغيل برامج خارجية غير مدعوم في التطبيق",
            line,
            col,
        ));
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        open_pipe_native(command, line, col)
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn open_pipe_native(command: &str, line: usize, col: usize) -> RioResult<u64> {
    // The empty-command guard runs before the cap check, as
    // in عمليات (an empty command reports itself, not the
    // cap).
    if command.split_whitespace().next().is_none() {
        return Err(RioError::runtime(
            "الأمر فارغ — اكتب اسم برنامج",
            line,
            col,
        ));
    }
    // Live-channel cap (fast path): refuse before forking.
    if REGISTRY.lock().map(|r| r.len()).unwrap_or(0) >= MAX_LIVE_CHANNELS {
        return Err(RioError::runtime(
            "عدد القنوات المفتوحة تجاوز الحد (٦٤) — أغلق بعضها",
            line,
            col,
        ));
    }
    let (mut cmd, program_display) =
        crate::processes::build_spawn_command(command, line, col)?;
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::processes::err_spawn_failed(&e, &program_display, line, col))?;
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RioError::runtime(
                "تعذّر فتح قناة للبرنامج",
                line,
                col,
            ));
        }
    };
    let stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RioError::runtime(
                "تعذّر فتح قناة للبرنامج",
                line,
                col,
            ));
        }
    };
    // One reader thread per channel: lines from the child's
    // stdout into a bounded queue. It ends when the child
    // closes stdout (exits) or the receiver is gone; it
    // never touches the registry.
    let (tx, rx) = mpsc::sync_channel::<String>(READER_QUEUE);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF: the child closed stdout / exited.
                Ok(_) => {
                    // Strip one trailing newline (and a \r
                    // before it: a Windows child writes \r\n).
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    if tx.send(line.clone()).is_err() {
                        break; // Receiver gone: nobody reads.
                    }
                }
                Err(_) => break, // Read error: treat as EOF.
            }
        }
    });
    // Strict cap at insert time (two threads may both have
    // passed the fast path): kill the extra child at once.
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut reg = match REGISTRY.lock() {
            Ok(r) => r,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RioError::runtime(
                    "تعذّر تسجيل القناة الجديدة",
                    line,
                    col,
                ));
            }
        };
        if reg.len() >= MAX_LIVE_CHANNELS {
            drop(reg);
            let _ = child.kill();
            let _ = child.wait();
            return Err(RioError::runtime(
                "عدد القنوات المفتوحة تجاوز الحد (٦٤) — أغلق بعضها",
                line,
                col,
            ));
        }
        reg.insert(
            id,
            ChannelEntry {
                child,
                stdin: Arc::new(Mutex::new(Some(stdin))),
                rx: Arc::new(Mutex::new(rx)),
            },
        );
    }
    Ok(id)
}

/// Send a line: `تواصل.أرسل(معرف، نص)` — write the text
/// plus one newline to the child's stdin, and return the
/// bytes written (text + newline). The message is checked
/// before the id is looked up, so a bad message reports
/// itself even for an unknown id.
pub fn send(id: u64, text: &str, line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, text);
        return Err(RioError::runtime(WASM_REFUSAL, line, col));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        send_native(id, text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn send_native(id: u64, text: &str, line: usize, col: usize) -> RioResult<u64> {
    if text.contains('\n') || text.contains('\r') {
        return Err(RioError::runtime(
            "الرسالة يجب أن تكون سطرا واحدا بلا أسطر جديدة",
            line,
            col,
        ));
    }
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(RioError::runtime(
            "الرسالة أكبر من الحد (64 كيبيبايت)",
            line,
            col,
        ));
    }
    let (stdin_arc, _) = channel_arcs(id, line, col)?;
    let mut guard = match stdin_arc.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err(RioError::runtime(
                format!("تعذّر الوصول إلى القناة: {}", id),
                line,
                col,
            ));
        }
    };
    let stdin = match guard.as_mut() {
        Some(s) => s,
        None => return Err(unknown_channel(id, line, col)),
    };
    let bytes = text.as_bytes();
    let written = match stdin.write_all(bytes).and_then(|_| stdin.write_all(b"\n")).and_then(|_| stdin.flush()) {
        Ok(()) => (bytes.len() + 1) as u64,
        Err(e) => {
            use std::io::ErrorKind;
            return Err(match e.kind() {
                ErrorKind::BrokenPipe => RioError::runtime(
                    "أغلق الطرف الآخر الاتصال",
                    line,
                    col,
                ),
                _ => RioError::runtime(
                    format!("تعذّر الإرسال إلى القناة: {}", id),
                    line,
                    col,
                ),
            });
        }
    };
    Ok(written)
}

/// Receive a line: `تواصل.استقبل(معرف)` — one line from
/// the child's stdout, without the newline. Waits at most
/// 30 seconds; a child that closed stdout (exited) is an
/// Arabic error, never an empty string.
pub fn recv(id: u64, line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        return Err(RioError::runtime(WASM_REFUSAL, line, col));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        recv_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn recv_native(id: u64, line: usize, col: usize) -> RioResult<String> {
    let (_, rx_arc) = channel_arcs(id, line, col)?;
    let rx = match rx_arc.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err(RioError::runtime(
                format!("تعذّر الوصول إلى القناة: {}", id),
                line,
                col,
            ));
        }
    };
    match rx.recv_timeout(TIMEOUT) {
        Ok(line_text) => Ok(line_text),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(RioError::runtime(
            "انتهت مهلة الاستقبال (30 ثانية) — لم تصل رسالة",
            line,
            col,
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(RioError::runtime(
            "أغلق الطرف الآخر الاتصال",
            line,
            col,
        )),
    }
}

/// Close a channel: `تواصل.أغلق(معرف)` — close the child's
/// stdin (EOF), wait at most 30 seconds for it to exit,
/// and return its exit code (-1 if killed by a signal, or
/// if it ignored EOF and had to be killed). The id is
/// consumed: closing twice is an error.
pub fn close(id: u64, line: usize, col: usize) -> RioResult<i64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        return Err(RioError::runtime(WASM_REFUSAL, line, col));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        close_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn close_native(id: u64, line: usize, col: usize) -> RioResult<i64> {
    let mut entry = take_channel(id, line, col)?;
    // EOF: drop the child's stdin.
    if let Ok(mut stdin) = entry.stdin.lock() {
        *stdin = None;
    }
    // Poll for the exit (same pattern as the sockets'
    // accept poll): a child that exits on EOF returns its
    // code; one that ignores EOF is killed after 30s.
    let start = Instant::now();
    loop {
        match entry.child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().map(|c| c as i64).unwrap_or(-1)),
            Ok(None) => {
                if start.elapsed() >= TIMEOUT {
                    let _ = entry.child.kill();
                    let _ = entry.child.wait();
                    return Ok(-1);
                }
                std::thread::sleep(CLOSE_POLL);
            }
            Err(_) => {
                return Err(RioError::runtime(
                    format!("تعذّر انتظار القناة: {}", id),
                    line,
                    col,
                ));
            }
        }
    }
}

/// Stop a channel: `تواصل.أوقف(معرف)` — kill the child
/// at once and reap it, so no zombie is left. Already
/// finished is fine (nothing to do). The id is consumed.
pub fn stop(id: u64, line: usize, col: usize) -> RioResult<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        return Err(RioError::runtime(WASM_REFUSAL, line, col));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        stop_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn stop_native(id: u64, line: usize, col: usize) -> RioResult<()> {
    let mut entry = take_channel(id, line, col)?;
    match entry.child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            entry.child.kill().map_err(|_| {
                RioError::runtime(format!("تعذّر إيقاف القناة: {}", id), line, col)
            })?;
            let _ = entry.child.wait();
            Ok(())
        }
        Err(_) => Err(RioError::runtime(
            format!("تعذّر إيقاف القناة: {}", id),
            line,
            col,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_policy(dir: &std::path::Path, trusted: bool) {
        crate::processes::set_process_policy(dir.to_path_buf(), trusted, dir.to_path_buf());
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        // The counter lives here (ungated), so this helper
        // compiles on every target — unlike the processes.rs
        // helper, whose NEXT_ID/Ordering are cfg(not(android))
        // while the helper itself is not (aarch64 --tests
        // failure, pre-existing, task 14).
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "niva-ipc-test-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Both locks, always in this order (processes first,
    /// then ipc): the spawn policy is process-wide and
    /// shared with the عمليات tests.
    fn test_locks() -> (
        std::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let proc = crate::processes::PROC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let ipc = ipc_test_lock();
        (proc, ipc)
    }

    #[test]
    fn pipe_cat_echo_unix() {
        // Unix only: /bin/cat echoes stdin to stdout line
        // by line; Windows has no such binary (the
        // integration tests cover the Niva-subprogram echo
        // on all three platforms).
        #[cfg(unix)]
        {
            let (_proc, _ipc) = test_locks();
            test_clear();
            let dir = temp_dir("cat");
            test_policy(&dir, true);
            let id = open_pipe("/bin/cat", 1, 1).unwrap();
            assert_eq!(send(id, "مرحبا", 1, 1).unwrap(), 11);
            assert_eq!(recv(id, 1, 1).unwrap(), "مرحبا");
            assert_eq!(close(id, 1, 1).unwrap(), 0);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn send_recv_unknown_id_error() {
        let (_proc, _ipc) = test_locks();
        test_clear();
        let err = send(999_999, "x", 1, 1).unwrap_err().to_string();
        assert!(err.contains("قناة غير معروفة"), "got: {}", err);
        assert!(!err.contains("os error"), "got: {}", err);
        let err = recv(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("قناة غير معروفة"), "got: {}", err);
        let err = close(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("قناة غير معروفة"), "got: {}", err);
        let err = stop(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("قناة غير معروفة"), "got: {}", err);
    }

    #[test]
    fn message_limits_are_arabic() {
        let (_proc, _ipc) = test_locks();
        test_clear();
        // Checked before the id lookup: an unknown id still
        // reports the message error, not the id error.
        let long = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let err = send(999_999, &long, 1, 1).unwrap_err().to_string();
        assert!(err.contains("أكبر من الحد"), "got: {}", err);
        let err = send(999_999, "سطر\nثان", 1, 1).unwrap_err().to_string();
        assert!(err.contains("سطرا واحدا"), "got: {}", err);
        let err = send(999_999, "سطر\rثان", 1, 1).unwrap_err().to_string();
        assert!(err.contains("سطرا واحدا"), "got: {}", err);
    }

    #[test]
    fn close_consumes_id() {
        #[cfg(unix)]
        {
            let (_proc, _ipc) = test_locks();
            test_clear();
            let dir = temp_dir("consume");
            test_policy(&dir, true);
            let id = open_pipe("/bin/cat", 1, 1).unwrap();
            assert_eq!(close(id, 1, 1).unwrap(), 0);
            assert!(send(id, "x", 1, 1).is_err());
            assert!(recv(id, 1, 1).is_err());
            assert!(close(id, 1, 1).is_err());
            assert!(stop(id, 1, 1).is_err());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn recv_eof_after_child_exit() {
        #[cfg(unix)]
        {
            let (_proc, _ipc) = test_locks();
            test_clear();
            let dir = temp_dir("eof");
            test_policy(&dir, true);
            // /bin/true exits at once with no output: the
            // reader thread hits EOF, so recv reports the
            // clean close in Arabic, never an empty string.
            let id = open_pipe("/bin/true", 1, 1).unwrap();
            let err = recv(id, 1, 1).unwrap_err().to_string();
            assert!(err.contains("أغلق الطرف الآخر الاتصال"), "got: {}", err);
            assert_eq!(close(id, 1, 1).unwrap(), 0);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn stop_kills_child() {
        // Linux only: /proc proves the process is gone
        // after stop (a mere registry removal would not).
        #[cfg(target_os = "linux")]
        {
            let (_proc, _ipc) = test_locks();
            test_clear();
            let dir = temp_dir("stop");
            test_policy(&dir, true);
            let id = open_pipe("/bin/sleep 30", 1, 1).unwrap();
            let pid = {
                let reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
                // The test pid helper reads the live child's
                // OS pid through the registry entry.
                reg.get(&id).map(|e| e.child.id())
            }
            .expect("live channel has a pid");
            assert!(std::path::Path::new(&format!("/proc/{}", pid)).exists());
            stop(id, 1, 1).unwrap();
            assert!(
                !std::path::Path::new(&format!("/proc/{}", pid)).exists(),
                "sleep still alive after stop"
            );
            // The id is consumed: closing again is an error.
            assert!(close(id, 1, 1).is_err());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn niva_subprogram_guard() {
        // In a unit test the current exe is the test binary,
        // not niva — the guard must refuse with Arabic.
        let (_proc, _ipc) = test_locks();
        test_clear();
        let dir = temp_dir("sub");
        fs::write(dir.join("مساعد.نيفا"), "اطبع ١\n").unwrap();
        test_policy(&dir, false);
        let err = open_pipe("مساعد.نيفا", 1, 1).unwrap_err().to_string();
        assert!(err.contains("غير مدعوم من هذا الثنائي"), "got: {}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_cap_enforced() {
        // 64 live `cat` channels; the 65th must be refused
        // in Arabic. Idle cats cost one thread each — cheap.
        #[cfg(unix)]
        {
            let (_proc, _ipc) = test_locks();
            test_clear();
            let dir = temp_dir("cap");
            test_policy(&dir, true);
            let mut ids = Vec::new();
            for _ in 0..MAX_LIVE_CHANNELS {
                ids.push(open_pipe("/bin/cat", 1, 1).unwrap());
            }
            let err = open_pipe("/bin/cat", 1, 1).unwrap_err().to_string();
            assert!(err.contains("تجاوز الحد"), "got: {}", err);
            for id in ids {
                let _ = close(id, 1, 1);
            }
            // After closing all, opening works again.
            let id = open_pipe("/bin/cat", 1, 1).unwrap();
            assert_eq!(close(id, 1, 1).unwrap(), 0);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
