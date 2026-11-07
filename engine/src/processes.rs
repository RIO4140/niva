//! Process spawning for the `عمليات` native module
//! (catalog task 10, from `kernel/`): run a program, wait for
//! it, kill it, and the current process id. One implementation,
//! shared by both engines (interpreter + VM) through the stdlib
//! native functions — never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-01-OPERATIONS.md` §1): no shell ever; default =
//! program-directory only (mirrors FilePolicy); trusted
//! (--اسمح-بالكل) lifts the restriction to PATH + absolute
//! paths; a first token ending in `.نيفا` runs as a Niva
//! subprogram via the current niva binary; WASM = inert;
//! Android = spawn refused; spawn errors are Arabic only
//! (the OS English string never leaks).

use crate::error::{RioError, RioResult};
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use crate::permissions::FilePolicy;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::process::{Child, Command, Stdio};
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// Maximum live (spawned, not yet waited/killed) processes.
/// A runaway loop must not fork the machine to death.
pub const MAX_LIVE_PROCESSES: usize = 256;

/// Process-wide policy, synced from the evaluating engine at
/// the start of every evaluation (run / eval_one /
/// call_builtin) and on every انتقل — the same pattern as the
/// network gate (network.rs): the interpreter/VM owns the
/// per-instance state, the static is what the native functions
/// read at call time. Default is deny-all (empty program
/// directory refuses every spawn), so a caller that never
/// syncs (e.g. lib run_source) cannot spawn by accident.
struct ProcessPolicy {
    program_dir: PathBuf,
    trusted: bool,
    cwd: PathBuf,
}

static POLICY: LazyLock<Mutex<ProcessPolicy>> = LazyLock::new(|| {
    Mutex::new(ProcessPolicy {
        program_dir: PathBuf::new(),
        trusted: false,
        cwd: PathBuf::new(),
    })
});

/// Test-only serialization for the process-wide policy and
/// registry: cargo runs tests in parallel threads, and both
/// are single statics. Every test that touches either locks
/// this first (same pattern as NET_TEST_LOCK in network.rs).
#[cfg(test)]
pub(crate) static PROC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Sync the process-wide policy from the evaluating engine.
/// Called at the start of every evaluation and on every
/// انتقل (the session cwd changed).
pub fn set_process_policy(program_dir: PathBuf, trusted: bool, cwd: PathBuf) {
    if let Ok(mut policy) = POLICY.lock() {
        policy.program_dir = program_dir;
        policy.trusted = trusted;
        policy.cwd = cwd;
    }
}

/// Native only: on WASM/Android there is no spawn, so the
/// snapshot, the registry and the helpers are compiled out
/// there (otherwise cargo check on those targets warns
/// dead-code for every one of them).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn policy_snapshot() -> (PathBuf, bool, PathBuf) {
    match POLICY.lock() {
        Ok(p) => (p.program_dir.clone(), p.trusted, p.cwd.clone()),
        Err(_) => (PathBuf::new(), false, PathBuf::new()),
    }
}

/// Live spawned processes: id -> child handle. One table for
/// the whole OS process, shared by both engines (a Niva
/// program is one OS process, so it has one process table).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static REGISTRY: LazyLock<Mutex<HashMap<u64, Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Process ids start at 1 and never repeat in one run.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Test-only: the OS pid of a live spawned process, if any.
/// Linux only: its only caller is the /proc proof in
/// kill_really_kills (a wider gate warns dead-code on the
/// Windows target check, where no test calls it).
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn test_pid(id: u64) -> Option<u32> {
    REGISTRY.lock().ok()?.get(&id).map(|c| c.id())
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn err_spawn_not_found(display: &str, line: usize, col: usize) -> RioError {
    RioError::runtime(format!("البرنامج غير موجود: {}", display), line, col)
}

/// Unix only (like its only caller, the unix
/// check_executable): a wider gate warns dead-code on the
/// Windows target check, where the call site is compiled out.
#[cfg(all(unix, not(target_arch = "wasm32"), not(target_os = "android")))]
fn err_not_executable(display: &str, line: usize, col: usize) -> RioError {
    RioError::runtime(
        format!("الملف ليس برنامجا قابلا للتشغيل: {}", display),
        line,
        col,
    )
}

/// Map a spawn failure to Arabic by error KIND only — the OS
/// English string never reaches the user (same rule as the
/// file builtins' Arabic errors, but without their known
/// English leak — task 5's follow-up).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn err_spawn_failed(e: &std::io::Error, display: &str, line: usize, col: usize) -> RioError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => err_spawn_not_found(display, line, col),
        ErrorKind::PermissionDenied => RioError::runtime(
            format!("لا يمكن تشغيل البرنامج (إذن مرفوض): {}", display),
            line,
            col,
        ),
        _ => RioError::runtime(format!("تعذّر تشغيل البرنامج: {}", display), line, col),
    }
}

/// Is this token a path (has a separator) or a bare name?
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn is_path_token(token: &str) -> bool {
    token.contains('/') || token.contains('\\')
}

/// Unix only: the resolved file must have an execute bit.
/// Windows has no such concept — the spawn itself decides.
#[cfg(all(unix, not(target_arch = "wasm32"), not(target_os = "android")))]
fn check_executable(path: &std::path::Path, display: &str, line: usize, col: usize) -> RioResult<()> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) if m.permissions().mode() & 0o111 != 0 => Ok(()),
        Ok(_) => Err(err_not_executable(display, line, col)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(err_spawn_not_found(display, line, col))
        }
        Err(e) => Err(err_spawn_failed(&e, display, line, col)),
    }
}

#[cfg(all(not(unix), not(target_arch = "wasm32"), not(target_os = "android")))]
fn check_executable(
    _path: &std::path::Path,
    _display: &str,
    _line: usize,
    _col: usize,
) -> RioResult<()> {
    Ok(())
}

/// Resolve symlinks in an executable path before the check
/// and the spawn: `/bin/true` is a symlink (to
/// `/usr/bin/true`) on macOS and on Linux, and the check +
/// spawn must see the real file, never the link (CI
/// 2026-11-06: the macOS runner refused `/bin/true` as
/// "not found" through the symlinked path). Falls back to
/// the given path when it does not exist, so the caller's
/// Arabic "not found" error stays exact. The display in
/// error messages still uses the program as the user typed
/// it — the canonical path never leaks out.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn canonical_executable(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Spawn a program: `عمليات.أنشئ "أمر وسائط"` — returns the
/// process id. The command is split on spaces (no shell, no
/// quotes); the first token is the program, the rest are args.
pub fn spawn_process(command: &str, line: usize, col: usize) -> RioResult<u64> {
    // WASM (the playground): no processes in a browser.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = command;
        return Err(RioError::runtime(
            "العمليات غير مدعومة في متصفح الويب",
            line,
            col,
        ));
    }
    // Android (the app): a public product with no shell and no
    // Linux environment — spawning arbitrary binaries from an
    // app process is a security hole, so it is refused.
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
        spawn_process_native(command, line, col)
    }
}

/// Build the OS command for `عمليات.أنشئ` and `تواصل.أنبوب`
/// (shared — catalog task 16): the policy snapshot, the
/// empty-directory and empty-command guards, and the three
/// program-resolution branches (a `.نيفا` first token runs
/// as a Niva subprogram via the current niva binary; a path
/// token is resolved under the file rules, canonicalized and
/// checked executable; a bare name is a PATH search when
/// trusted, else a file in the session cwd). Stdio is NOT
/// set here — the caller sets it (inherit + null for
/// عمليات, piped for تواصل), so the two modules can never
/// drift apart on the spawn rules. Also returns the display
/// of the program token, for the spawn-failure error.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn build_spawn_command(
    command: &str,
    line: usize,
    col: usize,
) -> RioResult<(Command, String)> {
    let (program_dir, trusted, cwd) = policy_snapshot();
    if program_dir.as_os_str().is_empty() {
        return Err(RioError::runtime(
            "لا يمكن تشغيل برامج بلا مجلد برنامج",
            line,
            col,
        ));
    }
    let policy = if trusted {
        FilePolicy::trusted(program_dir.clone())
    } else {
        FilePolicy::restricted(program_dir.clone())
    };
    let display = |s: &str| policy.display_str(s);

    let mut parts = command.split_whitespace();
    let program = match parts.next() {
        Some(p) => p,
        None => {
            return Err(RioError::runtime(
                "الأمر فارغ — اكتب اسم برنامج",
                line,
                col,
            ))
        }
    };
    let args: Vec<&str> = parts.collect();
    let program_display = display(program);

    let mut cmd: Command;
    if program.ends_with(".نيفا") {
        // A Niva subprogram: resolved under the file rules,
        // then run with the current niva binary itself.
        let path = policy.resolve_in(&cwd, program, line, col)?;
        let exe = std::env::current_exe().map_err(|_| {
            RioError::runtime("تعذّر تحديد مسار ثنائي نيفا الحالي", line, col)
        })?;
        // Guard: a standalone binary refuses source files, so
        // spawning a subprogram from it would only produce a
        // confusing exit-1 — refuse with a clear Arabic error.
        let stem = exe
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if stem != "niva" && stem != "نيفا" {
            return Err(RioError::runtime(
                "تشغيل برنامج نيفا فرعي غير مدعوم من هذا الثنائي",
                line,
                col,
            ));
        }
        cmd = Command::new(exe);
        cmd.arg(path);
        cmd.args(&args);
    } else if is_path_token(program) {
        // A path: resolved under the file rules (relative to
        // the session cwd), then executed directly. The
        // canonical form is what gets checked and spawned
        // (symlinks resolved — see canonical_executable).
        let path = policy.resolve_in(&cwd, program, line, col)?;
        let path = canonical_executable(&path);
        check_executable(&path, &display(program), line, col)?;
        cmd = Command::new(path);
        cmd.args(&args);
    } else if trusted {
        // A bare name in trusted mode: PATH search by std.
        cmd = Command::new(program);
        cmd.args(&args);
    } else {
        // A bare name in restricted mode: an executable file
        // in the session cwd (which is inside program_dir).
        let path = policy.resolve_in(&cwd, program, line, col)?;
        let path = canonical_executable(&path);
        check_executable(&path, &display(program), line, col)?;
        cmd = Command::new(path);
        cmd.args(&args);
    }
    Ok((cmd, program_display))
}

/// Spawn a program: `عمليات.أنشئ "أمر وسائط"` — returns the
/// process id. The command is built by the shared builder
/// (same rules as `تواصل.أنبوب`); only the stdio differs
/// here (stdin null, stdout/stderr inherited).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn spawn_process_native(command: &str, line: usize, col: usize) -> RioResult<u64> {
    // The empty-command guard runs before the cap check, as
    // before the split (an empty command reports itself,
    // not the cap).
    if command.split_whitespace().next().is_none() {
        return Err(RioError::runtime(
            "الأمر فارغ — اكتب اسم برنامج",
            line,
            col,
        ));
    }
    // Live-process cap (fast path): refuse before forking.
    if REGISTRY.lock().map(|r| r.len()).unwrap_or(0) >= MAX_LIVE_PROCESSES {
        return Err(RioError::runtime(
            "عدد العمليات المفتوحة تجاوز الحد (٢٥٦) — انتظر بعضها أو أوقفها",
            line,
            col,
        ));
    }
    let (mut cmd, program_display) = build_spawn_command(command, line, col)?;
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let child = cmd
        .spawn()
        .map_err(|e| err_spawn_failed(&e, &program_display, line, col))?;

    // Strict cap at insert time (two threads may both have
    // passed the fast path): kill the extra child at once.
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut reg = match REGISTRY.lock() {
            Ok(r) => r,
            Err(_) => {
                let mut c = child;
                let _ = c.kill();
                let _ = c.wait();
                return Err(RioError::runtime(
                    "تعذّر تسجيل العملية الجديدة",
                    line,
                    col,
                ));
            }
        };
        if reg.len() >= MAX_LIVE_PROCESSES {
            drop(reg);
            let mut c = child;
            let _ = c.kill();
            let _ = c.wait();
            return Err(RioError::runtime(
                "عدد العمليات المفتوحة تجاوز الحد (٢٥٦) — انتظر بعضها أو أوقفها",
                line,
                col,
            ));
        }
        reg.insert(id, child);
    }
    Ok(id)
}

/// Take a live child out of the registry, or an Arabic error
/// if the id is unknown (already waited/killed, or never was).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn take_child(id: u64, line: usize, col: usize) -> RioResult<Child> {
    match REGISTRY.lock() {
        Ok(mut reg) => reg.remove(&id).ok_or_else(|| {
            RioError::runtime(format!("عملية غير معروفة: {}", id), line, col)
        }),
        Err(_) => Err(RioError::runtime(
            format!("عملية غير معروفة: {}", id),
            line,
            col,
        )),
    }
}

/// Wait for a process: `عمليات.انتظر معرف` — returns the exit
/// code, or -1 if the process was killed by a signal. The id
/// is consumed: waiting twice is an error.
pub fn wait_process(id: u64, line: usize, col: usize) -> RioResult<i64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        return Err(RioError::runtime(
            "العمليات غير مدعومة في متصفح الويب",
            line,
            col,
        ));
    }
    #[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
    {
        let _ = id;
        return Err(RioError::runtime(
            "تشغيل برامج خارجية غير مدعوم في التطبيق",
            line,
            col,
        ));
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        let mut child = take_child(id, line, col)?;
        match child.wait() {
            Ok(status) => Ok(status.code().map(|c| c as i64).unwrap_or(-1)),
            Err(_) => Err(RioError::runtime(
                format!("تعذّر انتظار العملية: {}", id),
                line,
                col,
            )),
        }
    }
}

/// Kill a process: `عمليات.أوقف معرف` — SIGKILL (or the
/// platform terminate), then reap it so no zombie is left.
/// Already finished is fine (nothing to do). The id is
/// consumed.
pub fn kill_process(id: u64, line: usize, col: usize) -> RioResult<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        return Err(RioError::runtime(
            "العمليات غير مدعومة في متصفح الويب",
            line,
            col,
        ));
    }
    #[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
    {
        let _ = id;
        return Err(RioError::runtime(
            "تشغيل برامج خارجية غير مدعوم في التطبيق",
            line,
            col,
        ));
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        let mut child = take_child(id, line, col)?;
        match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                child.kill().map_err(|_| {
                    RioError::runtime(format!("تعذّر إيقاف العملية: {}", id), line, col)
                })?;
                // Reap so no zombie is left behind.
                let _ = child.wait();
                Ok(())
            }
            Err(_) => Err(RioError::runtime(
                format!("تعذّر إيقاف العملية: {}", id),
                line,
                col,
            )),
        }
    }
}

/// The current process id: `عمليات.الحالية()` — works
/// everywhere except WASM (no processes in a browser).
/// On Android it returns the engine subprocess pid: a plain
/// number, harmless, so it is allowed there.
pub fn current_process_id(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(RioError::runtime(
            "العمليات غير مدعومة في متصفح الويب",
            line,
            col,
        ));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = line;
        let _ = col;
        Ok(std::process::id() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// اجتماع 32، السؤال التاسع: عدّاد محلي لأسماء
    /// مجلدات الاختبار — NEXT_ID معرّف خارج android
    /// فقط (واستيراد Ordering معه)، فاستخدامه في
    /// temp_dir كان يكسر اختبارات android.
    static TEST_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn test_policy(dir: &std::path::Path, trusted: bool) {
        set_process_policy(dir.to_path_buf(), trusted, dir.to_path_buf());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-proc-test-{}-{}-{}",
            std::process::id(),
            tag,
            TEST_DIR_ID.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Lock the test serialization lock, recovering from
    /// poisoning: if an earlier test panicked while holding
    /// it, the next test must still run and report its OWN
    /// result instead of failing with PoisonError (CI
    /// 2026-11-06: one macOS panic cascaded into four
    /// PoisonErrors that hid every other test's verdict).
    /// This is cascade-hardening only — the panicking test
    /// itself still fails loudly on its own line.
    fn proc_test_lock() -> std::sync::MutexGuard<'static, ()> {
        PROC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Unix fixture: an instantly-exiting program. `/bin/true`
    /// is a symlink on macOS (and on Linux); prefer it so the
    /// engine's symlink handling is exercised, and fall back
    /// to the real file `/usr/bin/true` when the link itself
    /// is missing on a runner image. The test's purpose is
    /// the process cap, not the OS layout — but if neither
    /// exists the test fails loudly instead of skipping.
    #[cfg(unix)]
    fn true_program() -> &'static str {
        if std::path::Path::new("/bin/true").exists() {
            "/bin/true"
        } else if std::path::Path::new("/usr/bin/true").exists() {
            "/usr/bin/true"
        } else {
            panic!("لا يوجد برنامج true على هذا النظام — لا /bin/true ولا /usr/bin/true");
        }
    }

    /// Same as true_program, for the instantly-failing one.
    #[cfg(unix)]
    fn false_program() -> &'static str {
        if std::path::Path::new("/bin/false").exists() {
            "/bin/false"
        } else if std::path::Path::new("/usr/bin/false").exists() {
            "/usr/bin/false"
        } else {
            panic!("لا يوجد برنامج false على هذا النظام — لا /bin/false ولا /usr/bin/false");
        }
    }

    #[test]
    fn spawn_true_wait_zero() {
        // Unix only: /bin/true + /bin/false are the instant
        // commands; Windows has no such binaries.
        #[cfg(unix)]
        {
            let _guard = proc_test_lock();
            let dir = temp_dir("true");
            test_policy(&dir, true);
            let id = spawn_process(true_program(), 1, 1).unwrap();
            assert_eq!(wait_process(id, 1, 1).unwrap(), 0);
            let id2 = spawn_process(false_program(), 1, 1).unwrap();
            assert_eq!(wait_process(id2, 1, 1).unwrap(), 1);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn spawn_missing_is_arabic_without_os_leak() {
        let _guard = proc_test_lock();
        let dir = temp_dir("missing");
        test_policy(&dir, false);
        let err = spawn_process("لاوجود", 1, 1).unwrap_err().to_string();
        assert!(err.contains("غير موجود"), "got: {}", err);
        assert!(!err.contains("os error"), "got: {}", err);
        assert!(!err.contains("No such file"), "got: {}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wait_and_kill_unknown_id_error() {
        let _guard = proc_test_lock();
        let dir = temp_dir("unknown");
        test_policy(&dir, true);
        let err = wait_process(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("عملية غير معروفة"), "got: {}", err);
        let err = kill_process(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("عملية غير معروفة"), "got: {}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_program_dir_refuses() {
        let _guard = proc_test_lock();
        set_process_policy(PathBuf::new(), false, PathBuf::new());
        let err = spawn_process("/bin/true", 1, 1).unwrap_err().to_string();
        assert!(err.contains("بلا مجلد برنامج"), "got: {}", err);
    }

    #[test]
    fn niva_subprogram_guard() {
        // In a unit test the current exe is the test binary,
        // not niva — the guard must refuse with Arabic.
        let _guard = proc_test_lock();
        let dir = temp_dir("sub");
        fs::write(dir.join("مساعد.نيفا"), "اطبع ١\n").unwrap();
        test_policy(&dir, false);
        let err = spawn_process("مساعد.نيفا", 1, 1).unwrap_err().to_string();
        assert!(err.contains("غير مدعوم من هذا الثنائي"), "got: {}", err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn current_pid_positive() {
        let pid = current_process_id(1, 1).unwrap();
        assert!(pid > 0);
        assert_eq!(pid, std::process::id() as u64);
    }

    #[test]
    fn kill_really_kills() {
        // Linux only: /proc proves the process is gone after
        // kill+reap (a mere registry removal would not).
        #[cfg(target_os = "linux")]
        {
            let _guard = proc_test_lock();
            let dir = temp_dir("kill");
            test_policy(&dir, true);
            let id = spawn_process("/bin/sleep 30", 1, 1).unwrap();
            let pid = test_pid(id).expect("live child has a pid");
            assert!(std::path::Path::new(&format!("/proc/{}", pid)).exists());
            kill_process(id, 1, 1).unwrap();
            assert!(
                !std::path::Path::new(&format!("/proc/{}", pid)).exists(),
                "sleep still alive after kill"
            );
            // The id is consumed: waiting again is an error.
            assert!(wait_process(id, 1, 1).is_err());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn live_cap_enforced() {
        // 256 live `true` processes (instant exits, but they
        // stay in the registry as zombies until waited on);
        // the 257th must be refused in Arabic.
        #[cfg(unix)]
        {
            let _guard = proc_test_lock();
            let dir = temp_dir("cap");
            test_policy(&dir, true);
            let mut ids = Vec::new();
            for _ in 0..MAX_LIVE_PROCESSES {
                ids.push(spawn_process(true_program(), 1, 1).unwrap());
            }
            let err = spawn_process(true_program(), 1, 1).unwrap_err().to_string();
            assert!(err.contains("تجاوز الحد"), "got: {}", err);
            for id in ids {
                let _ = wait_process(id, 1, 1);
            }
            // After reaping all, spawning works again.
            let id = spawn_process(true_program(), 1, 1).unwrap();
            assert_eq!(wait_process(id, 1, 1).unwrap(), 0);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
