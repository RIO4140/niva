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

/// المرحلة (هـ) — التحكم بالمهام (اجتماعا 64 و65
/// §79 و§80): جدول الوظائف فوق REGISTRY. الوظيفة =
/// عملية نظام واحدة من `عمليات.أنشئ` (أو `شغل`
/// كعملية ابن عبر الثنائي الحالي) — بلا مجموعات
/// عمليات وبلا حالة موقوفة (لا SIGTSTP في المحرك).
/// الجدول بيانات خالصة بلا مقابض نظام، فيعيش على
/// كل الأهداف (حتى حيث الإنشاء مرفوض فيبقى فارغا).
/// السقف نفسه سقف العمليات الحية (256).
pub const MAX_JOBS: usize = 256;

/// حالة وظيفة: ثلاث لا أربع — تعمل، وانتهت برمزها،
/// وأُوقفت (قُتلت). بلا «موقوفة» (اجتماع 64 §79).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Done(i32),
    Killed,
}

/// مدخل واحد في جدول الوظائف.
#[derive(Debug, Clone)]
pub struct JobInfo {
    pub number: u64,
    pub process_id: u64,
    pub command: String,
    pub status: JobStatus,
    pub notified: bool,
}

struct JobsState {
    jobs: Vec<JobInfo>,
    current: Option<u64>,
    previous: Option<u64>,
    next_number: u64,
    /// معرف آخر عملية خلفية (0 = لا خلفية بعد) —
    /// يضبطه كل إنشاء (`أنشئ` و`&`)، لا الانتظار.
    last_background: u64,
    /// رموز وظائف منتهية نُسيت بـ`أبعد` (كـbgp في
    /// باش): يبقى `انتظر %n` قادرا على إرجاعها.
    /// (مسار كامل: استيراد HashMap مبوَّب للأصلي وحده،
    /// والجدول بيانات خالصة على كل الأهداف.)
    saved: std::collections::HashMap<u64, i64>,
    /// معارف عمليات جارية نُسيت بـ`أبعد`: تبقى في
    /// REGISTRY ويجمعها موجّه الجمع عند الموجه التالي.
    detached: std::collections::HashSet<u64>,
}

impl JobsState {
    fn new() -> Self {
        JobsState {
            jobs: Vec::new(),
            current: None,
            previous: None,
            next_number: 1,
            last_background: 0,
            saved: std::collections::HashMap::new(),
            detached: std::collections::HashSet::new(),
        }
    }
}

static JOBS: LazyLock<Mutex<JobsState>> = LazyLock::new(|| Mutex::new(JobsState::new()));

/// أرقام مشرقية لأسطر الوظائف (كطباعة القيم).
pub(crate) fn eastern(n: u64) -> String {
    const D: [char; 10] = ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];
    n.to_string().chars().map(|c| c.to_digit(10).map(|d| D[d as usize]).unwrap_or(c)).collect()
}

/// أرقام مشرقية لرمز خروج قد يكون سالبا (-1 =
/// قُتلت بإشارة).
fn eastern_i64(n: i64) -> String {
    if n < 0 {
        format!("-{}", eastern(n.unsigned_abs()))
    } else {
        eastern(n as u64)
    }
}

/// سطر وظيفة واحد: `[n]+ الأمر — الحالة`
/// (والحالية `+` والسابقة `-` وغيرهما فراغ).
/// الحالة: تعمل، أو انتهت (رمز N)، أو أُوقفت.
pub fn format_job_line(job: &JobInfo, current: Option<u64>, previous: Option<u64>) -> String {
    let mark = if Some(job.number) == current {
        "+"
    } else if Some(job.number) == previous {
        "-"
    } else {
        " "
    };
    let status = match job.status {
        JobStatus::Running => "تعمل".to_string(),
        JobStatus::Done(code) => format!("انتهت (رمز {})", eastern_i64(code as i64)),
        JobStatus::Killed => "أُوقفت".to_string(),
    };
    format!("[{}]{} {} — {}", eastern(job.number), mark, job.command, status)
}

/// سجّل عملية منشأة وظيفة: رقم متسلسل جديد،
/// وتصير الحالية (والقديمة سابقة). عند الامتلاء
/// تُسقط أقدم منتهية مُبلَّغ عنها، وإلا رفض عربي.
pub fn register_job(process_id: u64, command: &str) -> RioResult<u64> {
    let mut state = match JOBS.lock() {
        Ok(s) => s,
        Err(_) => {
            return Err(RioError::runtime(
                "تعذّر تسجيل الوظيفة الجديدة",
                1,
                1,
            ))
        }
    };
    if state.jobs.len() >= MAX_JOBS {
        let drop_at = state
            .jobs
            .iter()
            .position(|j| matches!(j.status, JobStatus::Done(_)) && j.notified);
        match drop_at {
            Some(i) => {
                state.jobs.remove(i);
            }
            None => {
                return Err(RioError::runtime(
                    "عدد الوظائف تجاوز الحد (٢٥٦) — أبعد بعضها",
                    1,
                    1,
                ))
            }
        }
    }
    let number = state.next_number;
    state.next_number += 1;
    state.previous = state.current;
    state.current = Some(number);
    state.last_background = process_id;
    state.jobs.push(JobInfo {
        number,
        process_id,
        command: command.to_string(),
        status: JobStatus::Running,
        notified: false,
    });
    Ok(number)
}

/// لقطة من جدول الوظائف (نسخة للقراءة).
pub fn list_jobs() -> Vec<JobInfo> {
    match JOBS.lock() {
        Ok(s) => s.jobs.clone(),
        Err(_) => Vec::new(),
    }
}

/// الحالية والسابقة (لعلامتي `+` و`-`).
pub fn current_previous_jobs() -> (Option<u64>, Option<u64>) {
    match JOBS.lock() {
        Ok(s) => (s.current, s.previous),
        Err(_) => (None, None),
    }
}

/// معرف آخر عملية خلفية (0 = لا خلفية بعد).
pub fn last_background_id() -> u64 {
    match JOBS.lock() {
        Ok(s) => s.last_background,
        Err(_) => 0,
    }
}

/// اضبط معرف آخر خلفية (عند كل إنشاء).
pub fn set_last_background(id: u64) {
    if let Ok(mut s) = JOBS.lock() {
        s.last_background = id;
    }
}

/// طبّع أرقاما مشرقية (٠-٩) إلى لاتينية — فمواصفات
/// الوظائف ومعارف العمليات تقبل ما يطبعه الترمنال
/// (كالمعجم: أرقام مشرقية ولاتينية معا).
pub(crate) fn normalize_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
            other => other,
        })
        .collect()
}

/// حلّ مواصفة وظيفة (`%n` و`%%` و`%+` و`%-`
/// و`%اسم`، أو رقم عارٍ) إلى رقم وظيفة.
pub fn resolve_job_spec(spec: &str) -> Result<u64, String> {
    let spec = normalize_digits(spec.trim());
    let spec = spec.as_str();
    let state = match JOBS.lock() {
        Ok(s) => s,
        Err(_) => return Err(format!("وظيفة غير معروفة: {}", spec)),
    };
    if spec == "%" || spec == "%%" || spec == "%+" {
        return state
            .current
            .ok_or_else(|| "لا وظيفة حالية".to_string());
    }
    if spec == "%-" {
        return state
            .previous
            .ok_or_else(|| "لا وظيفة سابقة".to_string());
    }
    if let Some(rest) = spec.strip_prefix('%') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let n: u64 = rest.parse().map_err(|_| format!("وظيفة غير معروفة: {}", spec))?;
            if state.jobs.iter().any(|j| j.number == n) || state.saved.contains_key(&n) {
                return Ok(n);
            }
            return Err(format!("وظيفة غير معروفة: {}", spec));
        }
        if !rest.is_empty() {
            // بحث جزئي في نص الأمر — الأحدث أولا.
            if let Some(j) = state.jobs.iter().rev().find(|j| j.command.contains(rest)) {
                return Ok(j.number);
            }
            return Err(format!("وظيفة غير معروفة: {}", spec));
        }
        return Err(format!("وظيفة غير معروفة: {}", spec));
    }
    if !spec.is_empty() && spec.chars().all(|c| c.is_ascii_digit()) {
        let n: u64 = spec.parse().map_err(|_| format!("وظيفة غير معروفة: {}", spec))?;
        if state.jobs.iter().any(|j| j.number == n) || state.saved.contains_key(&n) {
            return Ok(n);
        }
    }
    Err(format!("وظيفة غير معروفة: {}", spec))
}

/// مدخل وظيفة برقمها (أو لا شيء).
pub fn job_by_number(number: u64) -> Option<JobInfo> {
    match JOBS.lock() {
        Ok(s) => s.jobs.iter().find(|j| j.number == number).cloned(),
        Err(_) => None,
    }
}

/// رمز محفوظ لوظيفة منتهية نُسيت بـ`أبعد`.
pub fn saved_job_code(number: u64) -> Option<i64> {
    match JOBS.lock() {
        Ok(s) => s.saved.get(&number).copied(),
        Err(_) => None,
    }
}

/// حدّث مدخل وظيفة مرتبطة بمعرف عملية بعد انتظار
/// مباشر (`عمليات.انتظر`): انتهت برمزها. والانتظار
/// نفسه إبلاغ — فتُضبط «أُبلغ عنها» حتى لا يعود
/// إشعار متأخر عن وظيفة عرف المستخدم انتهاءها.
pub(crate) fn sync_job_done(process_id: u64, code: i64) {
    if let Ok(mut s) = JOBS.lock() {
        if let Some(j) = s.jobs.iter_mut().find(|j| j.process_id == process_id) {
            if j.status == JobStatus::Running {
                j.status = JobStatus::Done(code as i32);
                j.notified = true;
            }
        }
    }
}

/// حدّث مدخل وظيفة مرتبطة بمعرف عملية بعد قتل
/// مباشر (`عمليات.أوقف`): أُوقفت.
pub(crate) fn sync_job_killed(process_id: u64) {
    if let Ok(mut s) = JOBS.lock() {
        if let Some(j) = s.jobs.iter_mut().find(|j| j.process_id == process_id) {
            if j.status == JobStatus::Running {
                j.status = JobStatus::Killed;
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn reset_jobs_for_test() {
    if let Ok(mut s) = JOBS.lock() {
        *s = JobsState::new();
    }
}

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
/// `/usr/bin/true`) on Linux (usrmerge); and on the macOS
/// 26 runner `/bin/true` is absent entirely while
/// `/usr/bin/true` exists (CI 2026-11-06: 332/333; and
/// integration.rs `os_program` documents the same) — so
/// the check + spawn must see the real file, never a
/// missing link. Falls back to the given path when it
/// does not exist, so the caller's Arabic "not found"
/// error stays exact. The display in error messages
/// still uses the program as the user typed it — the
/// canonical path never leaks out.
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
        // (اجتماع 49 §63) الرمز النسبي يُحل مجلد
        // البرنامج أولا (إن وُجد الملف هناك) ثم cwd
        // (البيت) — مرآة `شغّل`: قبل الفصل كان cwd
        // هو مجلد البرنامج فوجد الشقيق تلقائيا.
        let path = if !program.starts_with('/')
            && program_dir.join(program).is_file()
        {
            policy.resolve_in(&program_dir, program, line, col)?
        } else {
            policy.resolve_in(&cwd, program, line, col)?
        };
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

/// ولّد الأمر مع محاولة ثانية عند فشل عابر: على مشغّل
/// مشترك مثقل قد يرفض fork/posix_spawn لحظيا (EAGAIN)
/// فيفشل توليد ابن يخرج فورا بينما ينجح توليد ابن طويل
/// قبله — فأعد المحاولة مرة واحدة بعد مهلة قصيرة.
/// الفشل الدائم (غير موجود، إذن مرفوض) لا يُعاد إطلاقا.
/// (ملاحظة: فشل `/bin/true` على macOS في الجولة
/// الأولى لم يكن عابرا أصلا — الملف غائب عن العدّاء؛
/// فأصلح الاختباران باستعمال `/bin/sleep 0` بدلا منه.
/// هذه المحاولة باقية متانة عامة للفشل العابر فقط.)
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn spawn_with_retry(cmd: &mut Command) -> std::io::Result<Child> {
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(e)
            if e.kind() != std::io::ErrorKind::NotFound
                && e.kind() != std::io::ErrorKind::PermissionDenied =>
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
            cmd.spawn()
        }
        Err(e) => Err(e),
    }
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
    let child = spawn_with_retry(&mut cmd)
        .map_err(|e| err_spawn_failed(&e, &program_display, line, col))?;
    let id = register_spawned_child(child, line, col)?;
    // كل إنشاء — أمامي أو خلفي — يضبط آخر خلفية.
    set_last_background(id);
    Ok(id)
}

/// سجّل عملية وليدة منشأة في REGISTRY مع السقف
/// الصارم عند الإدراج (خيطان قد يتجاوزا الفحص
/// السريع معا): الزائدة تُقتل فورا.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn register_spawned_child(mut child: Child, line: usize, col: usize) -> RioResult<u64> {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut reg = match REGISTRY.lock() {
            Ok(r) => r,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RioError::runtime(
                    "تعذّر تسجيل العملية الجديدة",
                    line,
                    col,
                ));
            }
        };
        if reg.len() >= MAX_LIVE_PROCESSES {
            drop(reg);
            let _ = child.kill();
            let _ = child.wait();
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

/// أنشئ برنامج نيفا كعملية ابن عبر الثنائي الحالي
/// (`شغل <ملف> &`): المخرج والخطأ موروثان كـ`أنشئ`.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn spawn_niva_file(path: &std::path::Path, line: usize, col: usize) -> RioResult<u64> {
    let exe = std::env::current_exe().map_err(|_| {
        RioError::runtime("تعذّر تحديد مسار ثنائي نيفا الحالي", line, col)
    })?;
    let stem = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if stem != "نيفا" && stem != "niva" {
        return Err(RioError::runtime(
            "تعذّر تحديد مسار ثنائي نيفا الحالي",
            line,
            col,
        ));
    }
    let mut cmd = Command::new(&exe);
    cmd.arg(path);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let child = spawn_with_retry(&mut cmd).map_err(|e| {
        RioError::runtime(format!("تعذّر تشغيل برنامج نيفا: {}", e), line, col)
    })?;
    let id = register_spawned_child(child, line, col)?;
    set_last_background(id);
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
            Ok(status) => {
                let code = status.code().map(|c| c as i64).unwrap_or(-1);
                // المرحلة (هـ): الانتظار المباشر يحدّث
                // مدخل الوظيفة المرتبط (إن وُجد).
                sync_job_done(id, code);
                Ok(code)
            }
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
            // انتهت وحدها قبل القتل: تُسجَّل منتهية
            // برمزها لا موقوفة.
            Ok(Some(status)) => {
                sync_job_done(id, status.code().map(|c| c as i64).unwrap_or(-1));
                Ok(())
            }
            Ok(None) => {
                child.kill().map_err(|_| {
                    RioError::runtime(format!("تعذّر إيقاف العملية: {}", id), line, col)
                })?;
                // Reap so no zombie is left behind.
                let _ = child.wait();
                // المرحلة (هـ): القتل المباشر يحدّث
                // مدخل الوظيفة المرتبط (إن وُجد).
                sync_job_killed(id);
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

/// المرحلة (هـ) — جمع غير حاجب لعملية واحدة:
/// إن انتهت أُخذت من REGISTRY وجُمعت، ورجع رمزها.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn reap_finished(id: u64) -> Option<i32> {
    let mut reg = match REGISTRY.lock() {
        Ok(r) => r,
        Err(_) => return None,
    };
    let child = reg.get_mut(&id)?;
    match child.try_wait() {
        Ok(Some(status)) => {
            let code = status.code().unwrap_or(-1);
            let mut child = reg.remove(&id).unwrap();
            let _ = child.wait();
            Some(code)
        }
        _ => None,
    }
}

/// انتظار وظيفة حاجب (`قدم` / `انتظر %n`):
/// يبقي الوظيفة برمزها. على منتهية يرجع رمزها
/// فورا؛ وعلى موقوفة خطأ؛ وعلى منسية منتهية يرجع
/// رمزها المحفوظ. لا يستهلكها (خلاف `عمليات.انتظر`).
pub fn wait_job(number: u64, line: usize, col: usize) -> RioResult<i64> {
    if let Some(code) = saved_job_code(number) {
        return Ok(code);
    }
    let pid = {
        let state = match JOBS.lock() {
            Ok(s) => s,
            Err(_) => {
                return Err(RioError::runtime(
                    format!("وظيفة غير معروفة: %{}", number),
                    line,
                    col,
                ))
            }
        };
        match state.jobs.iter().find(|j| j.number == number) {
            Some(j) => match j.status {
                JobStatus::Done(code) => return Ok(code as i64),
                JobStatus::Killed => {
                    return Err(RioError::runtime("الوظيفة أُوقفت", line, col))
                }
                JobStatus::Running => j.process_id,
            },
            None => {
                return Err(RioError::runtime(
                    format!("وظيفة غير معروفة: %{}", number),
                    line,
                    col,
                ))
            }
        }
    };
    // انتظار حاجب للعملية (يسجّل انتهاءها في الجدول).
    wait_process(pid, line, col)
}

/// انتظار الكل (`انتظر` بلا معامل): يرجع 0 دائما
/// (اجتماع 65 §80) — أخطاء الوظائف الفردية تُبتلع.
pub fn wait_all_jobs(line: usize, col: usize) -> i64 {
    let numbers: Vec<u64> = match JOBS.lock() {
        Ok(s) => s
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Running)
            .map(|j| j.number)
            .collect(),
        Err(_) => Vec::new(),
    };
    for n in numbers {
        let _ = wait_job(n, line, col);
    }
    0
}

/// انتظار أي وظيفة (`انتظر أي` / `عمليات.انتظر_أي`):
/// أول جارية تنتهي — يرجع (رقمها، رمزها).
pub fn wait_any_job(line: usize, col: usize) -> RioResult<(u64, i64)> {
    #[cfg(target_arch = "wasm32")]
    {
        return Err(RioError::runtime(
            "العمليات غير مدعومة في متصفح الويب",
            line,
            col,
        ));
    }
    #[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
    {
        return Err(RioError::runtime(
            "تشغيل برامج خارجية غير مدعوم في التطبيق",
            line,
            col,
        ));
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    loop {
        {
            let mut state = match JOBS.lock() {
                Ok(s) => s,
                Err(_) => {
                    return Err(RioError::runtime("تعذّر الوصول لجدول الوظائف", line, col))
                }
            };
            let running: Vec<(u64, u64)> = state
                .jobs
                .iter()
                .filter(|j| j.status == JobStatus::Running)
                .map(|j| (j.number, j.process_id))
                .collect();
            if running.is_empty() {
                return Err(RioError::runtime("لا وظائف جارية", line, col));
            }
            for (number, pid) in running {
                if let Some(code) = reap_finished(pid) {
                    if let Some(j) = state.jobs.iter_mut().find(|j| j.number == number) {
                        // الانتظار نفسه إبلاغ (كـsync_job_done).
                        j.status = JobStatus::Done(code);
                        j.notified = true;
                    }
                    return Ok((number, code as i64));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// قتل وظيفة بالقوة القصوى (`اقتل`): على منتهية
/// خطأ «الوظيفة منتهية»، وعلى موقوفة «الوظيفة
/// أُوقفت». لا اختيار إشارة (اجتماع 65 §80).
pub fn kill_job(number: u64, line: usize, col: usize) -> RioResult<()> {
    let pid = {
        let state = match JOBS.lock() {
            Ok(s) => s,
            Err(_) => {
                return Err(RioError::runtime(
                    format!("وظيفة غير معروفة: %{}", number),
                    line,
                    col,
                ))
            }
        };
        match state.jobs.iter().find(|j| j.number == number) {
            Some(j) => match j.status {
                JobStatus::Done(_) => {
                    return Err(RioError::runtime("الوظيفة منتهية", line, col))
                }
                JobStatus::Killed => {
                    return Err(RioError::runtime("الوظيفة أُوقفت", line, col))
                }
                JobStatus::Running => j.process_id,
            },
            None => {
                return Err(RioError::runtime(
                    format!("وظيفة غير معروفة: %{}", number),
                    line,
                    col,
                ))
            }
        }
    };
    kill_process(pid, line, col)
}

/// هل العملية حية؟ (`عمليات.حية` — مقابل `kill -0`).
pub fn is_alive(id: u64) -> bool {
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        let _ = id;
        return false;
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        match REGISTRY.lock() {
            Ok(mut reg) => match reg.get_mut(&id) {
                Some(child) => matches!(child.try_wait(), Ok(None)),
                None => false,
            },
            Err(_) => false,
        }
    }
}

/// انسَ وظيفة (`أبعد`): المنتهية يُحفظ رمزها
/// (ليبقى `انتظر %n` قادرا)، والجارية تُنقل إلى
/// «منفصلة» يجمعها موجّه الجمع عند الموجه التالي.
pub fn disown_job(number: u64, line: usize, col: usize) -> RioResult<()> {
    let mut state = match JOBS.lock() {
        Ok(s) => s,
        Err(_) => {
            return Err(RioError::runtime(
                format!("وظيفة غير معروفة: %{}", number),
                line,
                col,
            ))
        }
    };
    let idx = match state.jobs.iter().position(|j| j.number == number) {
        Some(i) => i,
        None => {
            return Err(RioError::runtime(
                format!("وظيفة غير معروفة: %{}", number),
                line,
                col,
            ))
        }
    };
    let job = state.jobs.remove(idx);
    match job.status {
        JobStatus::Done(code) => {
            state.saved.insert(number, code as i64);
        }
        JobStatus::Running => {
            state.detached.insert(job.process_id);
        }
        JobStatus::Killed => {}
    }
    refresh_current_previous(&mut state);
    Ok(())
}

/// انسَ كل الوظائف (`أبعد الكل`).
pub fn disown_all_jobs() {
    if let Ok(mut s) = JOBS.lock() {
        let jobs = std::mem::take(&mut s.jobs);
        for job in jobs {
            match job.status {
                JobStatus::Done(code) => {
                    s.saved.insert(job.number, code as i64);
                }
                JobStatus::Running => {
                    s.detached.insert(job.process_id);
                }
                JobStatus::Killed => {}
            }
        }
        s.current = None;
        s.previous = None;
    }
}

/// أعد حساب الحالية والسابقة بعد نسيان: الأعلى
/// رقما في الجدول حالية، والتي تليه سابقة.
fn refresh_current_previous(state: &mut JobsState) {
    let mut numbers: Vec<u64> = state.jobs.iter().map(|j| j.number).collect();
    numbers.sort_unstable();
    state.current = numbers.pop();
    state.previous = numbers.pop();
}

/// الجمع غير الحاجب عند الموجه (`poll_jobs`):
/// يجمع كل جارية انتهت ويعيد سطور الإشعار
/// (`[n]+ الأمر — انتهت (رمز ٠)`) للمنتهية التي
/// لم تُبلَّغ بعد؛ ويجمع المنفصلة بلا إشعار.
/// على WASM وأندرويد لا عمليات فيُرجع فارغا.
pub fn poll_jobs() -> Vec<String> {
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        return Vec::new();
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        let mut notes = Vec::new();
        let mut state = match JOBS.lock() {
            Ok(s) => s,
            Err(_) => return notes,
        };
        let (current, previous) = (state.current, state.previous);
        for i in 0..state.jobs.len() {
            if state.jobs[i].status != JobStatus::Running {
                continue;
            }
            let pid = state.jobs[i].process_id;
            if let Some(code) = reap_finished(pid) {
                state.jobs[i].status = JobStatus::Done(code);
                if !state.jobs[i].notified {
                    state.jobs[i].notified = true;
                    notes.push(format_job_line(&state.jobs[i], current, previous));
                }
            }
        }
        // المنفصلة: جمع بلا إشعار.
        let detached: Vec<u64> = state.detached.iter().copied().collect();
        for pid in detached {
            if reap_finished(pid).is_some() {
                state.detached.remove(&pid);
            }
        }
        notes
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

    /// Unix fixture: an instantly-exiting program.
    /// `/bin/true` is a symlink (usrmerge) on Linux; on
    /// the macOS 26 runner it is absent entirely while
    /// `/usr/bin/true` exists — so prefer the link, then
    /// fall back to the real file, and fail loudly when
    /// neither exists. The test's purpose is the process
    /// cap, not the OS layout — but if neither exists the
    /// test fails loudly instead of skipping.
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

    // المرحلة (هـ) — جدول الوظائف: منطق خالص
    // بمعارف وهمية (بلا عمليات حقيقية).
    #[test]
    fn jobs_table_register_resolve() {
        let _guard = proc_test_lock();
        reset_jobs_for_test();
        let n1 = register_job(999_001, "نوم طويل").unwrap();
        let n2 = register_job(999_002, "اطبع مرحبا").unwrap();
        assert_eq!((n1, n2), (1, 2));
        assert_eq!(last_background_id(), 999_002);
        let jobs = list_jobs();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].status, JobStatus::Running);
        let (cur, prev) = current_previous_jobs();
        assert_eq!((cur, prev), (Some(2), Some(1)));
        assert_eq!(resolve_job_spec("1").unwrap(), 1);
        assert_eq!(resolve_job_spec("%1").unwrap(), 1);
        // الأرقام المشرقية كما يطبعها الترمنال.
        assert_eq!(resolve_job_spec("%١").unwrap(), 1);
        assert_eq!(resolve_job_spec("٢").unwrap(), 2);
        assert_eq!(resolve_job_spec("%").unwrap(), 2);
        assert_eq!(resolve_job_spec("%%").unwrap(), 2);
        assert_eq!(resolve_job_spec("%+").unwrap(), 2);
        assert_eq!(resolve_job_spec("%-").unwrap(), 1);
        assert_eq!(resolve_job_spec("%مرحبا").unwrap(), 2);
        assert!(resolve_job_spec("%غائب").is_err());
        assert!(resolve_job_spec("99").is_err());
        assert!(resolve_job_spec("").is_err());
        let line1 = format_job_line(&jobs[0], cur, prev);
        assert!(line1.contains("[١]-"), "got: {}", line1);
        assert!(line1.contains("نوم طويل"), "got: {}", line1);
        assert!(line1.contains("تعمل"), "got: {}", line1);
        let line2 = format_job_line(&jobs[1], cur, prev);
        assert!(line2.contains("[٢]+"), "got: {}", line2);
        // السقف: 256 وظيفة — الـ257 تُرفض (كلها
        // جارية فلا أقدم منتهية مُبلَّغ عنها تُسقط).
        for i in 3..=256 {
            register_job(999_000 + i, "وظيفة").unwrap();
        }
        let err = register_job(999_999, "زائدة").unwrap_err().to_string();
        assert!(err.contains("تجاوز الحد"), "got: {}", err);
        reset_jobs_for_test();
    }

    /// المرحلة (هـ) — انتظر خروج عملية بلا استهلاك
    /// (لاختبارات الجمع: `true` قد لا تكون خرجت بعد
    /// حين يُستدعى الجمع بعد ميكروثوانٍ من الإنشاء —
    /// فالجمع بلا انتظار يراها جارية).
    #[cfg(unix)]
    fn wait_for_exit(id: u64) {
        for _ in 0..200 {
            if !is_alive(id) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("العملية {} لم تخرج", id);
    }

    // المرحلة (هـ) — انتظار وجمع وقتل ونسيان
    // بعمليات حقيقية (يونكس فقط).
    #[test]
    fn jobs_wait_poll_kill_disown() {
        #[cfg(unix)]
        {
            let _guard = proc_test_lock();
            reset_jobs_for_test();
            let dir = temp_dir("jobs");
            test_policy(&dir, true);
            // true تنتهي فورا: الجمع يبلّغ عنها مرة واحدة.
            let id = spawn_process(true_program(), 1, 1).unwrap();
            assert_eq!(last_background_id(), id);
            let n1 = register_job(id, "برنامج صحيح").unwrap();
            assert_eq!(n1, 1);
            wait_for_exit(id);
            let notes = poll_jobs();
            assert_eq!(notes.len(), 1, "got: {:?}", notes);
            assert!(notes[0].contains("انتهت (رمز ٠)"), "got: {}", notes[0]);
            assert!(poll_jobs().is_empty());
            assert_eq!(wait_job(n1, 1, 1).unwrap(), 0);
            // sleep جارية: حية، ثم تُقتل.
            let id2 = spawn_process("/bin/sleep 30", 1, 1).unwrap();
            let n2 = register_job(id2, "نوم").unwrap();
            assert!(is_alive(id2));
            kill_job(n2, 1, 1).unwrap();
            assert!(!is_alive(id2));
            let err = wait_job(n2, 1, 1).unwrap_err().to_string();
            assert!(err.contains("الوظيفة أُوقفت"), "got: {}", err);
            let err = kill_job(n2, 1, 1).unwrap_err().to_string();
            assert!(err.contains("الوظيفة أُوقفت"), "got: {}", err);
            let err = kill_job(n1, 1, 1).unwrap_err().to_string();
            assert!(err.contains("الوظيفة منتهية"), "got: {}", err);
            // النسيان: المنتهية يُحفظ رمزها، والجارية
            // تُنسى، والمجهولة خطأ.
            disown_job(n1, 1, 1).unwrap();
            assert_eq!(wait_job(n1, 1, 1).unwrap(), 0);
            assert_eq!(list_jobs().len(), 1);
            disown_job(n2, 1, 1).unwrap();
            assert!(list_jobs().is_empty());
            assert!(disown_job(99, 1, 1).is_err());
            // بلا جارية: wait_any خطأ لا تعليق.
            let err = wait_any_job(1, 1).unwrap_err().to_string();
            assert!(err.contains("لا وظائف جارية"), "got: {}", err);
            // wait_any: أول جارية تنتهي هي true.
            let id3 = spawn_process(true_program(), 1, 1).unwrap();
            let n3 = register_job(id3, "صحيح ثان").unwrap();
            let id4 = spawn_process("/bin/sleep 30", 1, 1).unwrap();
            let n4 = register_job(id4, "نوم ثان").unwrap();
            assert_eq!(wait_any_job(1, 1).unwrap(), (n3, 0));
            assert_eq!(wait_job(n3, 1, 1).unwrap(), 0);
            kill_job(n4, 1, 1).unwrap();
            let _ = fs::remove_dir_all(&dir);
            reset_jobs_for_test();
        }
    }

    // المرحلة (هـ) — منسية جارية: الجمع يجمعها
    // عند الموجه بلا إشعار (والانتظار المباشر بعدها
    // يجدها مستهلكة — أي جُمعت فعلا).
    #[test]
    fn jobs_disowned_running_reaped_by_poll() {
        #[cfg(unix)]
        {
            let _guard = proc_test_lock();
            reset_jobs_for_test();
            let dir = temp_dir("jobs-disown");
            test_policy(&dir, true);
            let id = spawn_process(true_program(), 1, 1).unwrap();
            let n = register_job(id, "صحيح منسي").unwrap();
            disown_job(n, 1, 1).unwrap();
            assert!(list_jobs().is_empty());
            wait_for_exit(id);
            assert!(poll_jobs().is_empty());
            assert!(wait_process(id, 1, 1).is_err());
            let _ = fs::remove_dir_all(&dir);
            reset_jobs_for_test();
        }
    }

    // المرحلة (هـ) — الانتظار والقتل المباشران
    // (`عمليات.انتظر` / `عمليات.أوقف`) يحدّثان
    // مدخل الوظيفة المرتبط.
    #[test]
    fn jobs_sync_from_direct_wait_kill() {
        #[cfg(unix)]
        {
            let _guard = proc_test_lock();
            reset_jobs_for_test();
            let dir = temp_dir("jobs-sync");
            test_policy(&dir, true);
            let id = spawn_process(true_program(), 1, 1).unwrap();
            let n1 = register_job(id, "صحيح مباشر").unwrap();
            assert_eq!(wait_process(id, 1, 1).unwrap(), 0);
            let job = job_by_number(n1).unwrap();
            assert_eq!(job.status, JobStatus::Done(0));
            let id2 = spawn_process("/bin/sleep 30", 1, 1).unwrap();
            let n2 = register_job(id2, "نوم مباشر").unwrap();
            kill_process(id2, 1, 1).unwrap();
            let job = job_by_number(n2).unwrap();
            assert_eq!(job.status, JobStatus::Killed);
            let _ = fs::remove_dir_all(&dir);
            reset_jobs_for_test();
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
