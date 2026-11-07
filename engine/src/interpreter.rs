//! Tree-walking interpreter.
//!
//! Scope rules (tested in tests/integration.rs):
//! - The global scope is the bottom scope.
//! - A function call pushes a NEW scope. Parameters and variables
//!   defined for the first time inside a function live in that scope
//!   only and disappear when it returns.
//! - Reading a variable searches from the innermost scope outward to global.
//! - Step 1 (Meeting 11, 2026-11-05): LEXICAL ASSIGNMENT. Assigning to a
//!   name that already exists in an outer scope updates that binding
//!   (walking outward); only a name that exists nowhere is defined
//!   locally. This is what makes closures stateful (the counter
//!   pattern). A variable defined for the first time inside a function
//!   still never escapes it. (Index assignment to an existing
//!   list/dict mutates that object itself, wherever it lives.)
//! - A function value captures its defining scope chain (shared Rc
//!   scopes), so a returned function still sees and mutates its
//!   defining environment: lexical closures.
//! - `إذا` / `بينما` / `لكل` / `كرر` do NOT create a scope: variables
//!   assigned inside them stay visible after the block in the same scope.
//!   In particular a loop variable is still visible after the loop.
//!   Loop variables are always defined in the current scope
//!   (define_local), never assigned outward to a same-named global.
//!
//! Type rules: dynamic typing, runtime checks, no silent coercion
//! (except the explicit عدد() / نص() conversions). Conditions must be
//! منطقي (Bool).

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{ErrorKind, RioError, RioResult};
use crate::lexer;
use crate::parser::Parser;
use crate::value::{ClassData, InstanceData, InterfaceData, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::io::Write;
use std::path::PathBuf;

pub(crate) const MAX_LOOP_ITERATIONS: usize = 1_000_000;
// P0 fix (2026-11-05, v1.2 final review): a hard call-depth limit for
// the tree-walking interpreter, so unbounded recursion returns an
// Arabic Niva error instead of overflowing the Rust stack and aborting
// the process. 200 is chosen empirically, not arbitrarily: measured
// 2026-11-05, the DEBUG build overflows at ~220-250 nested calls
// (unoptimised frames, 8 MB main stack), while the RELEASE build
// survives 1000+. A 1000 limit would therefore still crash under
// `cargo test` (debug). 200 is safe in BOTH builds, with margin.
// The VM phase (Phase 1B) will use its own explicit call stack,
// not the Rust stack, and may raise this limit with a Niva error,
// never with a crash.
// Meeting 15: measured depth cap, shared by the interpreter and
// the VM. On a 256 MB thread the worst measured shape
// (debug, 10 locals + wide expression) is safe past 700, so 500
// holds ~30% margin; release ceilings are far higher. The cap is
// enforced as a clean Niva error, never a stack overflow.
// Meeting 17 (WASM): the browser stack is far smaller than the
// native 256 MB thread (the playground builds with a 16 MB WASM
// stack), so the WASM cap is measured separately.
// Measured 2026-11-05 in Node (V8, the same engine Chromium
// uses), Meeting-15 extreme shape (10 locals + wide expression),
// cap lifted: largest completing N = 1500, smallest failing
// N = 1531 (a JS RangeError, not a WASM trap). Max executing
// depth = N+1, so the ceiling is depth ~1501. Cap 1000 holds
// ~33% margin on the worst shape — the same margin rule as
// Meeting 15. Depth beyond the cap is a clean Niva error,
// never a browser crash.
#[cfg(target_arch = "wasm32")]
pub(crate) const MAX_CALL_DEPTH: usize = 1000;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_CALL_DEPTH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Normal,
    Return,
    Break,
    Continue,
}

// We carry a return value alongside Flow::Return via a separate field on
// the interpreter instead of stuffing it into Flow, to keep Flow Copy.
pub struct Interpreter {
    // Shared scopes: each scope is an Rc so a function value can
    // capture its defining chain (lexical closures, Step 1) and
    // later calls still share it with the defining environment.
    scopes: Vec<Rc<RefCell<HashMap<String, Value>>>>,
    pub output: String,
    return_value: Option<Value>,
    call_depth: usize,
    // Phase 1A Step 2: modules.
    // program_dir: directory of the main program file, used to resolve
    // module names (module name = file stem, identifier only, so no
    // path traversal is possible through a module name).
    program_dir: PathBuf,
    // module name -> Module value, loaded once per run.
    loaded_modules: HashMap<String, Value>,
    // Modules currently being loaded (for cycle detection).
    loading_stack: Vec<String>,
    // Meeting 4: file capability policy. Default restricted to
    // program_dir; --اسمح-بالكل (trusted) removes the restriction.
    file_policy: crate::permissions::FilePolicy,
    // Arabic terminal shortcuts (owner's order 2026-11-05):
    // the session's current directory, changed by انتقل.
    // Absolute and lexically normalized; starts at
    // program_dir. Relative paths in the file builtins resolve
    // against it (via resolve_cwd), so cd affects اقرأ_ملف
    // and the rest exactly like in a real terminal.
    cwd: PathBuf,
    // Meeting 22, per-instance network flag (sessions): each
    // interpreter owns its own permission. The process-wide gate
    // in network.rs is the fetch-time source of truth, and it is
    // re-synced from this flag at the start of every top-level
    // evaluation (run / eval_one) — evaluation is synchronous
    // and single-threaded, so the gate always reflects the
    // interpreter that is currently evaluating, and one REPL
    // console's approval never leaks into another.
    network_allowed: bool,
    // Meeting 27: the server flag, same per-instance pattern
    // as the network flag — but it is read through the
    // HandlerCaller context (no process-wide static), because
    // native functions now receive the engine as context.
    server_allowed: bool,
    // اجتماع 32، السؤال الخامس: الملف الجاري تنفيذه
    // (الملف الرئيسي، أو ملف الوحدة أثناء تحميلها
    // وتنفيذ دوالها) — تُسنَد إليه أخطاء التشغيل عند
    // خروج run(). لا شيء لإدخال REPL ونص بلا ملف.
    source_file: Option<String>,
    // اجتماع 32، السؤال الثامن: بث حي — حين يكون صحيحا
    // يكتب `اطبع` السطر إلى المخرجات القياسية فور إنتاجه
    // (مع تفريغ) ولا يحتفظ به في output؛ فالمحتوى
    // النهائي مطابق حرفيا ولا يتضاعف عند طباعة main له.
    // افتراضيا خطأ: lib والاختبارات تقرأ output كما كانت.
    live_output: bool,
    // اجتماع 32، السؤال الخامس: اسم وحدة ← ملفها
    // (العرض بشرطة مائلة) — لإسناد أخطاء أجسام دوال
    // الوحدة إلى ملفها عند استدعائها من ملف آخر.
    module_files: HashMap<String, String>,
}

impl Interpreter {
    pub fn new() -> Self {
        let program_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            scopes: vec![Rc::new(RefCell::new(HashMap::new()))],
            output: String::new(),
            return_value: None,
            call_depth: 0,
            program_dir: program_dir.clone(),
            cwd: program_dir.clone(),
            loaded_modules: HashMap::new(),
            loading_stack: Vec::new(),
            file_policy: crate::permissions::FilePolicy::restricted(program_dir),
            network_allowed: false,
            server_allowed: false,
            source_file: None,
            live_output: false,
            module_files: HashMap::new(),
        }
    }

    pub fn with_program_dir(program_dir: PathBuf) -> Self {
        let mut interp = Self::new();
        interp.program_dir = program_dir.clone();
        interp.cwd = program_dir.clone();
        interp.file_policy = crate::permissions::FilePolicy::restricted(program_dir);
        interp
    }

    /// Resolve a user path against this session's current
    /// directory (انتقل), under the file policy.
    fn resolve_cwd(&self, path: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        self.file_policy.resolve_in(&self.cwd, path, line, col)
    }

    /// Parse a line-count argument: a whole number, or a text
    /// of (Arabic or Western) digits, like the عدد builtin.
    pub(crate) fn count_arg(v: &Value, line: usize, col: usize) -> RioResult<usize> {
        match v {
            Value::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as usize),
            Value::Str(s) => {
                let cleaned: String = s
                    .trim()
                    .chars()
                    .map(|c| match c {
                        '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
                        other => other,
                    })
                    .collect();
                cleaned.parse::<usize>().map_err(|_| {
                    RioError::typ(format!("العدد غير صالح: {}", s), line, col)
                })
            }
            other => Err(RioError::typ(
                format!("العدد يتطلب رقما، وليس {}", other.type_name()),
                line,
                col,
            )),
        }
    }

    /// Recursive file-name search (ابحث_عن): substring match
    /// on the entry name, depth at most 4, at most 100 hits.
    /// Symlinked directories are not followed (DirEntry
    /// file_type does not follow links), so no cycles.
    pub(crate) fn find_files(dir: &std::path::Path, base: &std::path::Path, query: &str, depth: usize, out: &mut Vec<String>) {
        if out.len() >= 100 || depth > 4 {
            return;
        }
        // Audit م-14: stream the entries (no whole-directory
        // collect + sort — the caller sorts the final hits).
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if out.len() >= 100 {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if name.contains(query) {
                    // Paths the language returns use `/` on
                    // every platform (slash_display): a raw
                    // to_string_lossy() shows `\` on Windows
                    // for paths the OS joined (CI 2026-11-06).
                    let full = e.path();
                    let rel = full.strip_prefix(base).unwrap_or(&full);
                    out.push(crate::permissions::slash_display(rel));
                }
                if is_dir {
                    Self::find_files(&e.path(), base, query, depth + 1, out);
                }
            }
        }
    }

    /// Recursive tree listing (اعرض_الشجرة): indented names,
    /// depth at most 3, at most 200 lines.
    pub(crate) fn tree_lines(dir: &std::path::Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        if out.len() >= 200 || depth > 3 {
            return;
        }
        // Audit م-14: collect at most the remaining room
        // (never the whole directory) before sorting.
        if let Ok(entries) = std::fs::read_dir(dir) {
            let room = 200usize.saturating_sub(out.len());
            let mut entries: Vec<_> = entries.flatten().take(room).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                if out.len() >= 200 {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    out.push(format!("{}{}/", prefix, name));
                    Self::tree_lines(&e.path(), &format!("{}  ", prefix), depth + 1, out);
                } else {
                    out.push(format!("{}{}", prefix, name));
                }
            }
        }
    }

    /// Recursive size in bytes (حجم_المسار): `budget` caps the
    /// walked entries at 10000 so a huge tree cannot hang the
    /// REPL. Symlinks are not followed (symlink_metadata).
    pub(crate) fn dir_size(path: &std::path::Path, budget: &mut usize) -> u64 {
        if *budget == 0 {
            return 0;
        }
        *budget -= 1;
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() => {
                let mut total = 0u64;
                if let Ok(entries) = std::fs::read_dir(path) {
                    for e in entries.flatten() {
                        total += Self::dir_size(&e.path(), budget);
                        if *budget == 0 {
                            break;
                        }
                    }
                }
                total
            }
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }

    /// Unix seconds -> "YYYY-MM-DD HH:MM:SS" in UTC.
    /// Thin wrapper over the shared civil math in
    /// `crate::time` (the single source of it).
    pub(crate) fn format_unix_utc(secs: u64) -> String {
        let days = (secs / 86400) as i64;
        let secs_of_day = secs % 86400;
        let (h, m, s) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
        let (y, mo, d) = crate::time::civil_from_days(days);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
    }

    /// Lexical normalization (no filesystem access): resolve
    /// "." and ".." in a path, for storing a clean cwd after
    /// انتقل. Same walk as the policy's normalize().
    pub(crate) fn normalize_lexical(path: &std::path::Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                std::path::Component::RootDir => out.push("/"),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::Normal(s) => out.push(s),
                std::path::Component::Prefix(p) => out.push(p.as_os_str()),
            }
        }
        out
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.file_policy.trusted = trusted;
    }

    /// Meeting 22: network capability. Default OFF; the CLI
    /// --اسمح-بالشبكة flag (or the WASM console's first-use
    /// approval) turns it on. Independent of file trust: it
    /// does not change file permissions. Per-instance: the flag
    /// lives on this interpreter and is synced to the
    /// process-wide gate at the start of run / eval_one, so one
    /// console's approval never leaks into another console.
    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.network_allowed = allowed;
        crate::network::set_network_allowed(allowed);
    }

    /// This interpreter's own network permission.
    pub fn network_allowed(&self) -> bool {
        self.network_allowed
    }

    /// Meeting 27: server capability. Default OFF; the CLI
    /// --اسمح-بالخادم flag (or the console's first-use
    /// approval, :اسمح-بالخادم) turns it on. The network flag
    /// alone is NOT enough to serve. Per-instance, like the
    /// network flag; read through the HandlerCaller context
    /// when a native function runs.
    pub fn set_server_allowed(&mut self, allowed: bool) {
        self.server_allowed = allowed;
    }

    /// This interpreter's own server permission.
    pub fn server_allowed(&self) -> bool {
        self.server_allowed
    }

    /// Explicitly authorize a local hostname for fetching (the
    /// only way a loopback/private address can be reached — see
    /// network.rs). Used by the CLI --مضيف-محلي flag.
    pub fn allow_local_network_host(&mut self, host: &str) {
        crate::network::allow_local_host(host);
    }

    /// اجتماع 32، السؤال الخامس: ملف المصدر الجاري
    /// تنفيذه — تُسنَد إليه أخطاء التشغيل. يضبطه المتصل
    /// العلوي (main.rs) للملف الرئيسي؛ وتحميل الوحدة
    /// يبدّله مؤقتا أثناء تنفيذها ثم يعيده.
    pub fn set_source_file(&mut self, file: &str) {
        self.source_file = Some(file.to_string());
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// (السطر يصل فور إنتاجه). افتراضيا خطأ.
    pub fn set_live_output(&mut self, live: bool) {
        self.live_output = live;
    }

    /// البث الحي مفعّل؟
    pub fn live_output(&self) -> bool {
        self.live_output
    }

    fn define_local(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last() {
            scope.borrow_mut().insert(name.to_string(), value);
        }
    }

    // Step 1 (Meeting 11): lexical assignment. Assign to the
    // nearest scope that already binds the name (walking outward,
    // so closures mutate their captured variables and functions
    // can update outer variables); define locally only when the
    // name exists nowhere. Used for plain `س = قيمة` only:
    // loop variables and definitions stay define_local.
    fn assign_local(&mut self, name: &str, value: Value) {
        if let Some(i) = self.lookup_scope_index(name) {
            self.scopes[i].borrow_mut().insert(name.to_string(), value);
        } else {
            self.define_local(name, value);
        }
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.borrow().get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    /// REPL support (audit م-12): is this name bound in any
    /// visible scope (variable, function, or module)?
    /// (Kept as a public helper; the code-first check now
    /// trial-analyzes instead, which subsumes this.)
    #[allow(dead_code)]
    pub fn is_defined(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// Terminal prompt (owner's order 2026-11-05 ~23:03):
    /// the session directory in display form (`~`, `~/...`).
    pub fn current_dir_display(&self) -> String {
        self.file_policy.display_path(&self.cwd)
    }

    /// Audit م-13: read a whole file with a hard size cap
    /// (5 MB). The size is enforced while reading (take
    /// cap+1), not only by a metadata check, so a file that
    /// grows between the check and the read cannot overflow
    /// the process. Over-size and non-UTF-8 files are clean
    /// Arabic errors, never a crash.
    pub const MAX_READ_BYTES: usize = 5 * 1024 * 1024;

    pub(crate) fn read_file_capped(
        path: &std::path::Path,
        display: &str,
        line: usize,
        col: usize,
    ) -> RioResult<String> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| {
            RioError::new(
                ErrorKind::Io,
                format!("تعذر قراءة الملف {}: {}", display, e),
                line,
                col,
            )
        })?;
        let mut buf = Vec::new();
        (&mut f)
            .take((Self::MAX_READ_BYTES + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| {
                RioError::new(
                    ErrorKind::Io,
                    format!("تعذر قراءة الملف {}: {}", display, e),
                    line,
                    col,
                )
            })?;
        if buf.len() > Self::MAX_READ_BYTES {
            return Err(RioError::new(
                ErrorKind::Io,
                format!(
                    "الملف {} أكبر من الحد الأقصى (5 ميغابايت)",
                    display
                ),
                line,
                col,
            ));
        }
        String::from_utf8(buf).map_err(|_| {
            RioError::new(
                ErrorKind::Io,
                format!("الملف {} ليس نصا صالحا (UTF-8)", display),
                line,
                col,
            )
        })
    }

    /// Audit م-20 (scoped): disk quota for the program dir
    /// (100 MB total). A write that would push the dir past
    /// the quota is refused with a clean Arabic error, so a
    /// loop cannot fill the device. The walk is budget-capped
    /// (dir_size), like حجم_المسار.
    pub const DISK_QUOTA_BYTES: u64 = 100 * 1024 * 1024;

    fn check_disk_quota(&self, extra_bytes: u64, line: usize, col: usize) -> RioResult<()> {
        let mut budget = 10000usize;
        let used = Self::dir_size(&self.program_dir, &mut budget);
        if used.saturating_add(extra_bytes) > Self::DISK_QUOTA_BYTES {
            return Err(RioError::new(
                ErrorKind::Io,
                "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة",
                line,
                col,
            ));
        }
        Ok(())
    }

    fn lookup_scope_index(&self, name: &str) -> Option<usize> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.borrow().contains_key(name) {
                return Some(i);
            }
        }
        None
    }

    /// REPL support: evaluate one expression and return its value.
    /// Used for bare-expression lines, which the REPL prints.
    pub fn eval_one(&mut self, expr: &Expr) -> RioResult<Value> {
        // Per-instance network flag (sessions): sync the
        // process-wide gate to this interpreter before any
        // اجلب can run inside this evaluation.
        crate::network::set_network_allowed(self.network_allowed);
        // مهمة 10: سياسة العمليات نفس النمط — تُزامَن من
        // حالة هذه النسخة (مجلد البرنامج + موثوق + cwd).
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.file_policy.trusted,
            self.cwd.clone(),
        );
        self.eval_expr(expr)
    }

    pub fn run(&mut self, program: &[Stmt]) -> RioResult<()> {
        self.run_inner(program)
            .map_err(|e| e.with_file_if_none(self.source_file.clone()))
    }

    fn run_inner(&mut self, program: &[Stmt]) -> RioResult<()> {
        // Per-instance network flag (sessions): same sync as
        // eval_one — the gate always reflects the interpreter
        // that is currently evaluating.
        crate::network::set_network_allowed(self.network_allowed);
        // مهمة 10: سياسة العمليات نفس النمط.
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.file_policy.trusted,
            self.cwd.clone(),
        );
        for stmt in program {
            match self.exec_stmt(stmt)? {
                Flow::Normal => {}
                Flow::Return => {
                    return Err(RioError::runtime("ارجع خارج دالة", stmt.line, stmt.col));
                }
                Flow::Break => {
                    return Err(RioError::runtime("اكسر خارج حلقة", stmt.line, stmt.col));
                }
                Flow::Continue => {
                    return Err(RioError::runtime("تابع خارج حلقة", stmt.line, stmt.col));
                }
            }
        }
        Ok(())
    }

    fn exec_block(&mut self, body: &[Stmt]) -> RioResult<Flow> {
        for stmt in body {
            let flow = self.exec_stmt(stmt)?;
            if flow != Flow::Normal {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> RioResult<Flow> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation } => {
                let v = self.eval_expr(value)?;
                // Phase 1A Step 3 (runtime part): an annotated
                // assignment is checked at runtime too, so gradual
                // cases (value from an untyped variable) and module
                // files (not semantically analyzed) are still caught.
                // `أي` means no check. Static checking for the main
                // program happens earlier, in the analyzer.
                if let Some(ann) = annotation {
                    if ann != "أي" && v.type_name() != ann {
                        let var_name = match target {
                            AssignTarget::Var(n) => n.clone(),
                            _ => String::new(),
                        };
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: المتغير {} معلن {}، لكن القيمة {}",
                                var_name,
                                ann,
                                v.type_name()
                            ),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                match target {
                    AssignTarget::Var(name) => {
                        // Step 1 (Meeting 11): lexical assignment.
                        self.assign_local(name, v);
                    }
                    AssignTarget::Index { target: t, index: i } => {
                        self.assign_index(t, i, v, stmt.line, stmt.col)?;
                    }
                    AssignTarget::Dot { target: t, name } => {
                        self.assign_dot(t, name, v, stmt.line, stmt.col)?;
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Print { args } => {
                let mut parts = Vec::new();
                for arg in args {
                    parts.push(self.eval_expr(arg)?.fmt_value());
                }
                let line = format!("{}\n", parts.join(" "));
                // اجتماع 32، السؤال الثامن: بث حي — السطر
                // يصل فور إنتاجه ولا يُحتفظ به في output،
                // فيبقى المحتوى النهائي مطابقا حرفيا.
                if self.live_output {
                    use std::io::Write;
                    print!("{}", line);
                    let _ = std::io::stdout().flush();
                } else {
                    self.output.push_str(&line);
                }
                Ok(Flow::Normal)
            }
            StmtKind::If { cond, then_body, elifs, else_body } => {
                let c = self.eval_expr(cond)?;
                let c = match c {
                    Value::Bool(b) => b,
                    other => {
                        return Err(RioError::typ(
                            format!("شرط إذا يجب أن يكون منطقيا، وليس {}", other.type_name()),
                            cond.line,
                            cond.col,
                        ));
                    }
                };
                if c {
                    return self.exec_block(then_body);
                }
                for (econd, ebody) in elifs {
                    let ec = self.eval_expr(econd)?;
                    match ec {
                        Value::Bool(true) => return self.exec_block(ebody),
                        Value::Bool(false) => continue,
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "شرط وإلا إذا يجب أن يكون منطقيا، وليس {}",
                                    other.type_name()
                                ),
                                econd.line,
                                econd.col,
                            ));
                        }
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_block(body);
                }
                Ok(Flow::Normal)
            }
            StmtKind::While { cond, body } => self.exec_while(cond, body),
            StmtKind::RepeatWhile { cond, body } => self.exec_while(cond, body),
            StmtKind::RepeatCount { count, body } => {
                let n = self.eval_expr(count)?;
                let n = match n {
                    Value::Number(x) => {
                        if x.fract() != 0.0 || x < 0.0 {
                            return Err(RioError::typ(
                                "عدد مرات التكرار يجب أن يكون عددا صحيحا غير سالب",
                                count.line,
                                count.col,
                            ));
                        }
                        x as usize
                    }
                    other => {
                        return Err(RioError::typ(
                            format!("عدد مرات التكرار يجب أن يكون عددا، وليس {}", other.type_name()),
                            count.line,
                            count.col,
                        ));
                    }
                };
                if n > MAX_LOOP_ITERATIONS {
                    return Err(RioError::runtime(
                        "عدد مرات التكرار كبير جدا",
                        count.line,
                        count.col,
                    ));
                }
                for _ in 0..n {
                    match self.exec_block(body)? {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return => return Ok(Flow::Return),
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::For { var, iterable, body } => {
                let iter = self.eval_expr(iterable)?;
                let items: Vec<Value> = match iter {
                    Value::List(items) => items,
                    Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    other => {
                        return Err(RioError::typ(
                            format!("لا يمكن التكرار على نوع {}", other.type_name()),
                            iterable.line,
                            iterable.col,
                        ));
                    }
                };
                for item in items {
                    self.define_local(var, item);
                    match self.exec_block(body)? {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return => return Ok(Flow::Return),
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::FuncDef { name, params, param_types, return_type, body } => {
                // Step 1: lexical closure — capture the defining
                // scope chain (shared Rc scopes). Module top-level
                // functions are marked with their module name after
                // module execution and then use the Step 2 module
                // scope push instead of this captured chain.
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    module: None,
                    param_types: param_types.clone(),
                    return_type: return_type.clone(),
                    captured: Some(self.scopes.clone()),
                    vm_id: None,
                };
                self.define_local(name, func);
                Ok(Flow::Normal)
            }
            StmtKind::ClassDef { name, base, interfaces, methods } => {
                // Resolve base class first (must already be defined).
                let base_data: Option<Rc<ClassData>> = match base {
                    Some(bname) => match self.lookup(bname) {
                        Some(Value::Class(data)) => Some(data),
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست فئة، بل {}", bname, other.type_name()),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("فئة أب غير معرفة: {}", bname),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                    },
                    None => None,
                };
                // Methods defined in a module file carry that module
                // name, so their bodies can read module globals,
                // like top-level module functions in Step 2.
                let mod_name = self.loading_stack.last().cloned();
                // Step 1: methods capture the defining scope chain
                // too (a method defined inside a function can close
                // over its locals); module methods use the Step 2
                // module push at call time instead.
                let captured = self.scopes.clone();
                let mut map: HashMap<String, Value> = HashMap::new();
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, param_types, return_type, body } = &m.kind {
                        map.insert(mn.clone(), Value::Function {
                            name: mn.clone(),
                            params: params.clone(),
                            body: body.clone(),
                            module: mod_name.clone(),
                            param_types: param_types.clone(),
                            return_type: return_type.clone(),
                            captured: Some(captured.clone()),
                            vm_id: None,
                        });
                    }
                }
                let class_data = Rc::new(ClassData {
                    name: name.clone(),
                    methods: map,
                    base: base_data,
                    interfaces: interfaces.clone(),
                });
                // Interface compliance: every required method must
                // exist (own or inherited) with the right arity.
                // v1 checks existence + arity only, not param types.
                for iname in interfaces {
                    let iface = match self.lookup(iname) {
                        Some(Value::Interface(data)) => data,
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست واجهة، بل {}", iname, other.type_name()),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("واجهة غير معرفة: {}", iname),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                    };
                    for (mname, arity) in &iface.methods {
                        match class_data.lookup_method(mname) {
                            Some(Value::Function { params, .. }) => {
                                if params.len() != *arity {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الفئة {} تنفذ طريقة {} من واجهة {} بعدد معاملات غير صحيح: تتوقع {} لكن وجد {}",
                                            name, mname, iname, arity, params.len()
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                            _ => {
                                return Err(RioError::runtime(
                                    format!(
                                        "الفئة {} لا تنفذ طريقة {} المطلوبة من واجهة {}",
                                        name, mname, iname
                                    ),
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        }
                    }
                }
                let class = Value::Class(class_data);
                self.define_local(name, class);
                Ok(Flow::Normal)
            }
            StmtKind::InterfaceDef { name, methods } => {
                let iface = Value::Interface(Rc::new(InterfaceData {
                    name: name.clone(),
                    methods: methods.clone(),
                }));
                self.define_local(name, iface);
                Ok(Flow::Normal)
            }
            StmtKind::Import { module, alias, names } => {
                let mod_value = self.load_module(module, stmt.line, stmt.col)?;
                match names {
                    Some(names) => {
                        // من وحدة استورد اسم1، اسم2 : bind each name.
                        let exports = match &mod_value {
                            Value::Module { exports, .. } => exports.clone(),
                            _ => HashMap::new(),
                        };
                        for name in names {
                            match exports.get(name) {
                                Some(v) => self.define_local(name, v.clone()),
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                            name, module, name, module
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                        }
                    }
                    None => {
                        // استورد وحدة [باسم مختصر] : bind module value.
                        let bind = alias.clone().unwrap_or_else(|| module.clone());
                        self.define_local(&bind, mod_value);
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Export { .. } => {
                // Export is collected at module load time (see
                // load_module) from top-level statements, before
                // execution. At runtime it is a no-op, including in
                // the main program (the main program is not a module).
                Ok(Flow::Normal)
            }
            StmtKind::Return { value } => {
                let v = match value {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Nil,
                };
                self.return_value = Some(v);
                Ok(Flow::Return)
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
            StmtKind::Pass => Ok(Flow::Normal),
            StmtKind::Try { try_body, except_body, finally_body } => {
                self.exec_try(try_body, except_body, finally_body)
            }
            StmtKind::ExprStmt { expr } => {
                let v = self.eval_expr(expr)?;
                // اجتماع 32، السؤال الثاني (2): عبارة
                // مجردة تنتج قيمة دالة (مرجع دالة، أو
                // نتيجة استدعاء ترجع دالة) خطأ عربي يسمي
                // الدالة — فالصمت هنا فخ لا ميزة. الفحص
                // على القيمة لا على شكل التعبير، ليطابق
                // VM حرفيا (Pop لا يرى التعبير)؛ أما
                // الاستدعاء المجرد الذي يرجع غير دالة
                // فتبقى قيمته الإرجاعية تُتجاهل كما كانت.
                if let Value::Function { name, .. } | Value::NativeFunction { name, .. } = &v
                {
                    return Err(RioError::runtime(
                        format!(
                            "دالة بلا استدعاء: {} — استدعها بالأقواس أو أسندها إلى متغير",
                            name
                        ),
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(Flow::Normal)
            }
        }
    }

    /// Load a module by name (file stem), once per run.
    /// Returns a Module value. On cycle, missing file, or lex/parse
    /// failure, returns an Arabic Niva error naming the module.
    fn load_module(&mut self, module: &str, line: usize, col: usize) -> RioResult<Value> {
        if let Some(v) = self.loaded_modules.get(module) {
            return Ok(v.clone());
        }
        // Step 2 (Meeting 16): native stdlib modules come first,
        // so `استورد رياضيات` works anywhere with no file, and a
        // same-named file in the program directory never shadows
        // a native module. Cached like a file module below.
        if let Some(v) = crate::stdlib::native_module(module) {
            self.loaded_modules.insert(module.to_string(), v.clone());
            return Ok(v);
        }
        if self.loading_stack.contains(&module.to_string()) {
            let mut cycle = self.loading_stack.clone();
            cycle.push(module.to_string());
            return Err(RioError::runtime(
                format!("استيراد دائري: {}", cycle.join(" ← ")),
                line,
                col,
            ));
        }
        // مهمة الخطوة 4 من «حزم» (التصميم §8): ترتيب
        // الاستيراد بعد الأصلية — `حزم/<اسم>/`، ثم مجلد
        // البرنامج، ثم المخزن العام (قفل ثم أحدث).
        let found = crate::packages::find_module_file(&self.program_dir, module).map_err(|msg| {
            RioError::new(ErrorKind::Io, msg, line, col)
        })?;
        let path = match found.found {
            Some(p) => p,
            None => {
                let searched = found
                    .searched
                    .iter()
                    .map(|p| crate::permissions::slash_display(p))
                    .collect::<Vec<_>>()
                    .join("؛ ");
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!("وحدة غير موجودة: {} (بحثت في {})", module, searched),
                    line,
                    col,
                ));
            }
        };
        let source = Self::read_file_capped(&path, module, line, col)?;
        // اجتماع 32، السؤال الخامس: ملف الوحدة هو الذي
        // تُسنَد إليه أخطاؤها — العرض بشرطة مائلة كما
        // يراه المستخدم؛ والبادئة «في وحدة X: » تبقى
        // توافقا مع الرسائل القديمة.
        let display = crate::permissions::slash_display(&path);
        self.module_files
            .insert(module.to_string(), display.clone());
        let tokens = lexer::tokenize_named(&source, &display).map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        let mut parser = Parser::named(tokens, &display);
        let program = parser.parse_program().map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        // Collect export names from top-level Export statements only.
        // Strict rule: only exported names are accessible from outside.
        // A module with no Export statement exports nothing.
        let mut export_names: Vec<String> = Vec::new();
        for stmt in &program {
            if let StmtKind::Export { names } = &stmt.kind {
                for n in names {
                    if !export_names.contains(n) {
                        export_names.push(n.clone());
                    }
                }
            }
        }
        // Execute the module in a fresh global scope, isolated from
        // the caller's scopes. loaded_modules / loading_stack are
        // shared, so nested imports and cycle detection work.
        // اجتماع 32: أثناء تنفيذ الوحدة source_file هو
        // ملفها، فتُسنَد أخطاء مستواها الأعلى إليه؛ ثم يُعاد
        // ملف المتصل (استيراد متداخل يعيد ملف وحدته هو).
        self.loading_stack.push(module.to_string());
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![Rc::new(RefCell::new(HashMap::new()))]);
        let saved_return = self.return_value.take();
        let saved_file = self.source_file.replace(display.clone());
        let exec_result = self.run(&program);
        self.source_file = saved_file;
        let module_scope = if self.scopes.is_empty() {
            HashMap::new()
        } else {
            self.scopes[0].borrow().clone()
        };
        self.scopes = saved_scopes;
        self.return_value = saved_return;
        self.loading_stack.pop();
        exec_result.map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        // Mark top-level functions in the module scope with their
        // module name, so calls to them push this module's scope.
        // Functions defined inside module functions keep module=None
        // and rely on the outer call's module scope (Step 2 limit).
        // Step 1: a closure stored at module top level (its
        // captured chain is longer than the module scope alone)
        // is NOT marked — its captured environment, not the module
        // scope, must be restored at call time.
        let mut full_scope = module_scope.clone();
        for v in full_scope.values_mut() {
            if let Value::Function { module: m, captured, .. } = v {
                let top_level = match captured {
                    Some(chain) => chain.len() == 1,
                    None => true,
                };
                if top_level {
                    *m = Some(module.to_string());
                }
            }
        }
        let mut exports: HashMap<String, Value> = HashMap::new();
        for name in &export_names {
            if let Some(v) = full_scope.get(name) {
                exports.insert(name.clone(), v.clone());
            }
            // An exported name that was never defined is simply not
            // present in exports; importing it then fails with the
            // helpful "غير مصدر" message, naming the module.
        }
        let mod_value = Value::Module {
            name: module.to_string(),
            exports,
            scope: full_scope,
        };
        self.loaded_modules.insert(module.to_string(), mod_value.clone());
        Ok(mod_value)
    }

    fn exec_while(&mut self, cond: &Expr, body: &[Stmt]) -> RioResult<Flow> {
        let mut iterations = 0usize;
        loop {
            let c = self.eval_expr(cond)?;
            let c = match c {
                Value::Bool(b) => b,
                other => {
                    return Err(RioError::typ(
                        format!("شرط الحلقة يجب أن يكون منطقيا، وليس {}", other.type_name()),
                        cond.line,
                        cond.col,
                    ));
                }
            };
            if !c {
                break;
            }
            iterations += 1;
            if iterations > MAX_LOOP_ITERATIONS {
                return Err(RioError::runtime(
                    "تم تجاوز حد التكرار، هل الحلقة لا تنتهي؟",
                    cond.line,
                    cond.col,
                ));
            }
            match self.exec_block(body)? {
                Flow::Normal => {}
                Flow::Break => break,
                Flow::Continue => continue,
                Flow::Return => return Ok(Flow::Return),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_try(
        &mut self,
        try_body: &[Stmt],
        except_body: &Option<Vec<Stmt>>,
        finally_body: &Option<Vec<Stmt>>,
    ) -> RioResult<Flow> {
        match self.exec_block(try_body) {
            Ok(flow) => {
                if let Some(fin) = finally_body {
                    let fflow = self.exec_block(fin)?;
                    if fflow != Flow::Normal {
                        return Ok(fflow);
                    }
                }
                Ok(flow)
            }
            Err(e) => {
                if let Some(exc) = except_body {
                    let eflow = match self.exec_block(exc) {
                        Ok(f) => f,
                        Err(e2) => {
                            if let Some(fin) = finally_body {
                                let _ = self.exec_block(fin);
                            }
                            return Err(e2);
                        }
                    };
                    if let Some(fin) = finally_body {
                        let fflow = self.exec_block(fin)?;
                        if fflow != Flow::Normal {
                            return Ok(fflow);
                        }
                    }
                    Ok(eflow)
                } else {
                    if let Some(fin) = finally_body {
                        let _ = self.exec_block(fin);
                    }
                    Err(e)
                }
            }
        }
    }

    // Phase 1A Step 4: dot field assignment: ك.س = قيمة.
    // The target is evaluated to an Instance; because instance
    // fields live behind Rc, mutating the cloned Rc value mutates
    // the same fields the caller sees, for any target form
    // (variable, نفس, chained dot, list/dict element).
    fn assign_dot(
        &mut self,
        target: &Expr,
        name: &str,
        value: Value,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        let t = self.eval_expr(target)?;
        match t {
            Value::Instance(data) => {
                data.fields.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            other => Err(RioError::typ(
                format!("لا يمكن تعيين حقل في نوع {}", other.type_name()),
                line,
                col,
            )),
        }
    }

    fn assign_index(
        &mut self,
        target: &Expr,
        index: &Expr,
        value: Value,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        let name = match &target.kind {
            ExprKind::Ident(n) => n.clone(),
            _ => {
                return Err(RioError::parse(
                    "تعيين الفهرسة المعقدة غير مدعوم في المحرك v1",
                    line,
                    col,
                ));
            }
        };
        let idx = self.eval_expr(index)?;
        let scope_idx = match self.lookup_scope_index(&name) {
            Some(i) => i,
            None => {
                return Err(RioError::runtime(
                    format!("متغير غير معرف: {}", name),
                    target.line,
                    target.col,
                ));
            }
        };
        let mut scope = self.scopes[scope_idx].borrow_mut();
        let slot = scope.get_mut(&name).unwrap();
        match slot {
            Value::List(items) => {
                let i = match idx {
                    Value::Number(x) if x.fract() == 0.0 && x >= 0.0 => x as usize,
                    other => {
                        return Err(RioError::typ(
                            format!("فهرس القائمة يجب أن يكون عددا صحيحا، وليس {}", other.type_name()),
                            index.line,
                            index.col,
                        ));
                    }
                };
                if i >= items.len() {
                    return Err(RioError::runtime(
                        format!("فهرس خارج النطاق: {} والقائمة طولها {}", i, items.len()),
                        index.line,
                        index.col,
                    ));
                }
                items[i] = value;
                Ok(())
            }
            Value::Dict(pairs) => {
                let key = match idx {
                    Value::Str(s) => s,
                    other => {
                        return Err(RioError::typ(
                            format!("مفتاح القاموس يجب أن يكون نصا، وليس {}", other.type_name()),
                            index.line,
                            index.col,
                        ));
                    }
                };
                if let Some(pair) = pairs.iter_mut().find(|(k, _)| *k == key) {
                    pair.1 = value;
                } else {
                    pairs.push((key, value));
                }
                Ok(())
            }
            other => Err(RioError::typ(
                format!("لا يمكن تعيين عنصر في نوع {}", other.type_name()),
                target.line,
                target.col,
            )),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> RioResult<Value> {
        match &expr.kind {
            ExprKind::Number(n) => Ok(Value::Number(*n)),
            ExprKind::Str(s) => Ok(Value::Str(s.clone())),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Nil => Ok(Value::Nil),
            ExprKind::Ident(name) => match self.lookup(name) {
                Some(v) => Ok(v),
                None => Err(RioError::runtime(
                    format!("متغير غير معرف: {}", name),
                    expr.line,
                    expr.col,
                )),
            },
            ExprKind::List(elems) => {
                let mut out = Vec::new();
                for e in elems {
                    out.push(self.eval_expr(e)?);
                }
                Ok(Value::List(out))
            }
            ExprKind::Dict(entries) => {
                let mut out: Vec<(String, Value)> = Vec::new();
                for (k, v) in entries {
                    let val = self.eval_expr(v)?;
                    if let Some(pair) = out.iter_mut().find(|(kk, _)| kk == k) {
                        pair.1 = val;
                    } else {
                        out.push((k.clone(), val));
                    }
                }
                Ok(Value::Dict(out))
            }
            ExprKind::Unary { op, expr: inner } => {
                let v = self.eval_expr(inner)?;
                match op {
                    UnOp::Neg => match v {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        other => Err(RioError::typ(
                            format!("السالب يتطلب عددا، وليس {}", other.type_name()),
                            expr.line,
                            expr.col,
                        )),
                    },
                    UnOp::Not => match v {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        other => Err(RioError::typ(
                            format!("ليس تتطلب منطقيا، وليس {}", other.type_name()),
                            expr.line,
                            expr.col,
                        )),
                    },
                }
            }
            ExprKind::Binary { left, op, right } => self.eval_binary(left, op, right, expr.line, expr.col),
            ExprKind::Call { callee, args } => {
                let mut arg_values = Vec::new();
                for a in args {
                    arg_values.push(self.eval_expr(a)?);
                }
                // Ident callee: user function first, then builtin.
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(v) = self.lookup(name) {
                        return match v {
                            Value::Function { .. } => {
                                self.call_user(v, arg_values, expr.line, expr.col)
                            }
                            // Step 2 (Meeting 16): a stdlib function
                            // imported by name (من رياضيات استورد جذر).
                            Value::NativeFunction { func, .. } => {
                                func(self, &arg_values, expr.line, expr.col)
                            }
                            // Meeting 26 (§41.2): a stored bound
                            // method is a value like any other —
                            // the general path below has this arm;
                            // the name path was missing it.
                            Value::BoundMethod { func, instance } => {
                                self.call_method(*func, instance, arg_values, expr.line, expr.col)
                            }
                            Value::Class(data) => Err(RioError::typ(
                                format!(
                                    "استخدم أنشئ لإنشاء كائن من فئة {}، مثل أنشئ {}()",
                                    data.name, data.name
                                ),
                                callee.line,
                                callee.col,
                            )),
                            other => Err(RioError::typ(
                                format!("القيمة {} ليست دالة", other.type_name()),
                                callee.line,
                                callee.col,
                            )),
                        };
                    }
                    return self.call_builtin(name, arg_values, expr.line, expr.col);
                }
                // Meeting 14: string/list methods. A Dot callee on
                // a Str/List target dispatches to a builtin method;
                // the target expression is kept so mutating list
                // methods can write back to a plain variable.
                // Other targets fall through to the generic Dot
                // evaluation below (modules, objects).
                if let ExprKind::Dot { target, name } = &callee.kind {
                    let target_value = self.eval_expr(target)?;
                    if matches!(target_value, Value::Str(_) | Value::List(_)) {
                        return self.call_builtin_method(
                            target,
                            target_value,
                            name,
                            arg_values,
                            expr.line,
                            expr.col,
                        );
                    }
                }
                let callee_value = self.eval_expr(callee)?;
                match callee_value {
                    Value::Function { .. } => {
                        self.call_user(callee_value, arg_values, expr.line, expr.col)
                    }
                    Value::NativeFunction { func, .. } => {
                        func(self, &arg_values, expr.line, expr.col)
                    }
                    Value::BoundMethod { func, instance } => {
                        self.call_method(*func, instance, arg_values, expr.line, expr.col)
                    }
                    other => Err(RioError::typ(
                        format!("القيمة {} ليست دالة", other.type_name()),
                        callee.line,
                        callee.col,
                    )),
                }
            }
            ExprKind::Index { target, index } => {
                let t = self.eval_expr(target)?;
                let i = self.eval_expr(index)?;
                match (&t, &i) {
                    (Value::List(items), Value::Number(x)) => {
                        if x.fract() != 0.0 || *x < 0.0 {
                            return Err(RioError::typ(
                                "فهرس القائمة يجب أن يكون عددا صحيحا",
                                index.line,
                                index.col,
                            ));
                        }
                        let idx = *x as usize;
                        items.get(idx).cloned().ok_or_else(|| {
                            RioError::runtime(
                                format!("فهرس خارج النطاق: {} والقائمة طولها {}", idx, items.len()),
                                index.line,
                                index.col,
                            )
                        })
                    }
                    (Value::Dict(pairs), Value::Str(key)) => pairs
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            RioError::runtime(
                                format!("مفتاح غير موجود: {}", key),
                                index.line,
                                index.col,
                            )
                        }),
                    (Value::Str(s), Value::Number(x)) => {
                        if x.fract() != 0.0 || *x < 0.0 {
                            return Err(RioError::typ(
                                "فهرس النص يجب أن يكون عددا صحيحا",
                                index.line,
                                index.col,
                            ));
                        }
                        let chars: Vec<char> = s.chars().collect();
                        chars
                            .get(*x as usize)
                            .map(|c| Value::Str(c.to_string()))
                            .ok_or_else(|| {
                                RioError::runtime(
                                    "فهرس خارج النطاق في النص",
                                    index.line,
                                    index.col,
                                )
                            })
                    }
                    _ => Err(RioError::typ(
                        format!(
                            "لا يمكن الفهرسة على نوع {} بمفتاح من نوع {}",
                            t.type_name(),
                            i.type_name()
                        ),
                        expr.line,
                        expr.col,
                    )),
                }
            }
            ExprKind::Dot { target, name } => {
                let t = self.eval_expr(target)?;
                match &t {
                    Value::Module { name: mod_name, exports, .. } => {
                        exports.get(name).cloned().ok_or_else(|| {
                            RioError::runtime(
                                format!(
                                    "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                    name, mod_name, name, mod_name
                                ),
                                expr.line,
                                expr.col,
                            )
                        })
                    }
                    Value::Instance(data) => {
                        // Field first, then method (bound to this
                        // instance). A field shadowing a method name
                        // wins, so ك.س can be data, not a method.
                        if let Some(v) = data.fields.borrow().get(name) {
                            Ok(v.clone())
                        } else if let Some(m) = data.class.lookup_method(name) {
                            Ok(Value::BoundMethod {
                                func: Box::new(m.clone()),
                                instance: Rc::clone(data),
                            })
                        } else {
                            Err(RioError::runtime(
                                format!(
                                    "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                                    data.class.name, name
                                ),
                                expr.line,
                                expr.col,
                            ))
                        }
                    }
                    other => Err(RioError::typ(
                        format!(
                            "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع {}",
                            other.type_name()
                        ),
                        expr.line,
                        expr.col,
                    )),
                }
            }
            ExprKind::Lambda { params, param_types, body } => {
                // Meeting 13: single-expression lambda, implicit
                // return. Stored as a Function whose body is one
                // Return statement, so call_user handles it exactly
                // like a named closure: lexical capture, param
                // types, depth limit, return value.
                let func = Value::Function {
                    name: "مجهولة".to_string(),
                    params: params.clone(),
                    body: vec![Stmt::new(
                        StmtKind::Return { value: Some((**body).clone()) },
                        expr.line,
                        expr.col,
                    )],
                    module: None,
                    param_types: param_types.clone(),
                    return_type: None,
                    captured: Some(self.scopes.clone()),
                    vm_id: None,
                };
                Ok(func)
            }
            ExprKind::Create { class, args } => {
                let mut arg_values = Vec::new();
                for a in args {
                    arg_values.push(self.eval_expr(a)?);
                }
                let class_value = match self.lookup(class) {
                    Some(v) => v,
                    None => {
                        return Err(RioError::runtime(
                            format!("فئة غير معرفة: {}", class),
                            expr.line,
                            expr.col,
                        ));
                    }
                };
                let class_data = match class_value {
                    Value::Class(data) => data,
                    other => {
                        return Err(RioError::typ(
                            format!("{} ليست فئة، بل {}", class, other.type_name()),
                            expr.line,
                            expr.col,
                        ));
                    }
                };
                let instance = Rc::new(InstanceData {
                    class: Rc::clone(&class_data),
                    fields: RefCell::new(HashMap::new()),
                });
                // Constructor: a method named أنشئ, if present,
                // is called automatically with نفس bound.
                if let Some(ctor) = class_data.lookup_method("أنشئ") {
                    let ctor = ctor.clone();
                    self.call_method(ctor, Rc::clone(&instance), arg_values, expr.line, expr.col)?;
                } else if !arg_values.is_empty() {
                    return Err(RioError::runtime(
                        format!(
                            "الفئة {} ليس لها منشئ أنشئ، لكن أعطيت {} معاملات",
                            class_data.name,
                            arg_values.len()
                        ),
                        expr.line,
                        expr.col,
                    ));
                }
                Ok(Value::Instance(instance))
            }
        }
    }

    fn eval_binary(
        &mut self,
        left: &Expr,
        op: &BinOp,
        right: &Expr,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        // Short-circuit logic.
        if *op == BinOp::And {
            let l = self.eval_expr(left)?;
            let lb = match l {
                Value::Bool(b) => b,
                other => {
                    return Err(RioError::typ(
                        format!("و تتطلب منطقيا، وليس {}", other.type_name()),
                        left.line,
                        left.col,
                    ));
                }
            };
            if !lb {
                return Ok(Value::Bool(false));
            }
            let r = self.eval_expr(right)?;
            return match r {
                Value::Bool(b) => Ok(Value::Bool(b)),
                other => Err(RioError::typ(
                    format!("و تتطلب منطقيا، وليس {}", other.type_name()),
                    right.line,
                    right.col,
                )),
            };
        }
        if *op == BinOp::Or {
            let l = self.eval_expr(left)?;
            let lb = match l {
                Value::Bool(b) => b,
                other => {
                    return Err(RioError::typ(
                        format!("أو تتطلب منطقيا، وليس {}", other.type_name()),
                        left.line,
                        left.col,
                    ));
                }
            };
            if lb {
                return Ok(Value::Bool(true));
            }
            let r = self.eval_expr(right)?;
            return match r {
                Value::Bool(b) => Ok(Value::Bool(b)),
                other => Err(RioError::typ(
                    format!("أو تتطلب منطقيا، وليس {}", other.type_name()),
                    right.line,
                    right.col,
                )),
            };
        }

        let l = self.eval_expr(left)?;
        let r = self.eval_expr(right)?;
        match op {
            BinOp::Eq => Ok(Value::Bool(l == r)),
            BinOp::NotEq => Ok(Value::Bool(l != r)),
            BinOp::In | BinOp::NotIn => {
                let found = match (&l, &r) {
                    (item, Value::List(items)) => items.iter().any(|x| x == item),
                    (Value::Str(needle), Value::Str(hay)) => hay.contains(needle.as_str()),
                    (Value::Str(key), Value::Dict(pairs)) => pairs.iter().any(|(k, _)| k == key),
                    _ => {
                        return Err(RioError::typ(
                            format!(
                                "في تتطلب قائمة أو نصا أو قاموسا على اليمين، وليس {}",
                                r.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                };
                Ok(Value::Bool(if *op == BinOp::In { found } else { !found }))
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                let ord = match (&l, &r) {
                    (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                    (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
                    _ => {
                        return Err(RioError::typ(
                            format!(
                                "المقارنة تتطلب عددين أو نصين، وليس {} و {}",
                                l.type_name(),
                                r.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                };
                let ord = ord.ok_or_else(|| {
                    RioError::runtime("مقارنة غير صالحة", line, col)
                })?;
                let b = match op {
                    BinOp::Lt => ord == std::cmp::Ordering::Less,
                    BinOp::Gt => ord == std::cmp::Ordering::Greater,
                    BinOp::LtEq => ord != std::cmp::Ordering::Greater,
                    BinOp::GtEq => ord != std::cmp::Ordering::Less,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(b))
            }
            BinOp::Add => match (&l, &r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::List(a), Value::List(b)) => {
                    let mut out = a.clone();
                    out.extend(b.iter().cloned());
                    Ok(Value::List(out))
                }
                _ => Err(RioError::typ(
                    format!("لا يمكن جمع {} و {}", l.type_name(), r.type_name()),
                    line,
                    col,
                )),
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let (a, b) = match (&l, &r) {
                    (Value::Number(a), Value::Number(b)) => (*a, *b),
                    _ => {
                        return Err(RioError::typ(
                            format!(
                                "العملية الحسابية تتطلب عددين، وليس {} و {}",
                                l.type_name(),
                                r.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                };
                match op {
                    BinOp::Sub => Ok(Value::Number(a - b)),
                    BinOp::Mul => Ok(Value::Number(a * b)),
                    BinOp::Div => {
                        if b == 0.0 {
                            Err(RioError::runtime("قسمة على صفر", line, col))
                        } else {
                            Ok(Value::Number(a / b))
                        }
                    }
                    BinOp::Mod => {
                        if b == 0.0 {
                            Err(RioError::runtime("باقي القسمة على صفر", line, col))
                        } else {
                            Ok(Value::Number(a % b))
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    fn call_user(
        &mut self,
        func: Value,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let (name, params, body, module, param_types, return_type, captured) = match func {
            Value::Function { name, params, body, module, param_types, return_type, captured, vm_id: _ } => {
                (name, params, body, module, param_types, return_type, captured)
            }
            _ => unreachable!(),
        };
        if args.len() != params.len() {
            return Err(RioError::runtime(
                format!(
                    "عدد المعاملات غير صحيح: الدالة {} تتوقع {} لكن أعطيت {}",
                    name,
                    params.len(),
                    args.len()
                ),
                line,
                col,
            ));
        }
        // Phase 1A Step 3 (runtime part): param types, `أي` = no check.
        for (i, arg) in args.iter().enumerate() {
            if let Some(Some(pt)) = param_types.get(i) {
                if pt != "أي" && arg.type_name() != pt {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: معامل {} في الدالة {} معلن {}، لكن القيمة {}",
                            params[i],
                            name,
                            pt,
                            arg.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        // P0: refuse a new call at the limit, before pushing a scope
        // or a Rust frame for it, so the error is a Niva error, not a
        // stack overflow. Depth is decremented on every exit path,
        // including errors, so a caught error does not leak depth.
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        // اجتماع 32، السؤال الحادي عشر: عزل إطارات
        // الاستدعاء — كل استدعاء دالة يجري في إطار
        // محليات خاص به، مطابقا لدلالة المترجم (VM):
        // سلسلة المتصل تُحفظ وتُستبدل بسلسلة الدالة
        // المعجمية وحدها، ثم تُستعاد بعد الاستدعاء —
        // فلا يرى المستدعى محليات المتصل أبدا، ولا
        // يسمّم اسما حرا منها. دالة وحدة: نسخة جديدة
        // من نطاق الوحدة المخزَّن (فلا يستمر تغيير
        // نطاق الوحدة — كما في VM حرفيا). إغلاق:
        // سلسلته الملتقَطة. دالة بلا سلسلة ملتقَطة
        // (دفاعيا فقط — كل القيم تحمل واحدة): نطاق
        // المعاملات وحده، كما في VM.
        let base: Vec<Rc<RefCell<HashMap<String, Value>>>> = if let Some(mod_name) = &module {
            match self.loaded_modules.get(mod_name) {
                Some(Value::Module { scope, .. }) => {
                    vec![Rc::new(RefCell::new(scope.clone()))]
                }
                _ => match &captured {
                    Some(chain) => chain.clone(),
                    None => vec![Rc::new(RefCell::new(HashMap::new()))],
                },
            }
        } else {
            match &captured {
                Some(chain) => chain.clone(),
                None => vec![Rc::new(RefCell::new(HashMap::new()))],
            }
        };
        let saved_scopes = std::mem::replace(&mut self.scopes, base);
        let mut scope = HashMap::new();
        for (p, v) in params.iter().zip(args.into_iter()) {
            scope.insert(p.clone(), v);
        }
        self.scopes.push(Rc::new(RefCell::new(scope)));
        self.call_depth += 1;
        let result = self.exec_block(&body);
        // اجتماع 32، السؤال الخامس: خطأ داخل جسم دالة
        // وحدة يُسنَد إلى ملف الوحدة لا ملف المتصل.
        // (دالة بلا وحدة — رئيسية أو لامدا — بلا إسناد هنا؛
        // فيسنَدها run() الجاري إلى ملفه هو.)
        let result = match &module {
            Some(m) => {
                result.map_err(|e| e.with_file_if_none(self.module_files.get(m).cloned()))
            }
            None => result,
        };
        self.scopes.pop();
        self.scopes = saved_scopes;
        self.call_depth -= 1;
        match result? {
            Flow::Return => {
                let v = self.return_value.take().unwrap_or(Value::Nil);
                // Phase 1A Step 3 (runtime part): return type.
                if let Some(rt) = &return_type {
                    if rt != "أي" && v.type_name() != rt {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الدالة {} معلنة أن ترجع {}، لكن رجعت {}",
                                name,
                                rt,
                                v.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(v)
            }
            Flow::Normal => {
                // No explicit return: value is لاشيء. If a return
                // type is declared and is not لاشيء / أي, that is
                // a mismatch, caught here (not silently Nil).
                if let Some(rt) = &return_type {
                    if rt != "أي" && rt != "لاشيء" {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الدالة {} معلنة أن ترجع {}، لكن لم ترجع شيئا",
                                name, rt
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(Value::Nil)
            }
            Flow::Break => Err(RioError::runtime("اكسر خارج حلقة", line, col)),
            Flow::Continue => Err(RioError::runtime("تابع خارج حلقة", line, col)),
        }
    }

    // Phase 1A Step 4: call a method bound to an instance.
    // نفس is bound to the instance in the method scope, after
    // parameters, so نفس always refers to the instance even if
    // a parameter were (incorrectly) named نفس. Method params
    // are the user arguments only; نفس is NOT a parameter.
    fn call_method(
        &mut self,
        func: Value,
        instance: Rc<InstanceData>,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let (name, params, body, module, param_types, return_type, captured) = match func {
            Value::Function { name, params, body, module, param_types, return_type, captured, vm_id: _ } => {
                (name, params, body, module, param_types, return_type, captured)
            }
            _ => unreachable!(),
        };
        if args.len() != params.len() {
            return Err(RioError::runtime(
                format!(
                    "عدد المعاملات غير صحيح: الطريقة {} تتوقع {} لكن أعطيت {}",
                    name,
                    params.len(),
                    args.len()
                ),
                line,
                col,
            ));
        }
        for (i, arg) in args.iter().enumerate() {
            if let Some(Some(pt)) = param_types.get(i) {
                if pt != "أي" && arg.type_name() != pt {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: معامل {} في الطريقة {} معلن {}، لكن القيمة {}",
                            params[i],
                            name,
                            pt,
                            arg.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        // اجتماع 32، السؤال الحادي عشر: كـ call_user —
        // عزل إطار الاستدعاء: سلسلة المتصل تُحفظ
        // وتُستبدل بسلسلة الطريقة المعجمية وحدها —
        // نسخة نطاق الوحدة لطريقة وحدة، وسلسلتها
        // الملتقَطة لغيرها — ثم تُستعاد بعد الاستدعاء.
        let base: Vec<Rc<RefCell<HashMap<String, Value>>>> = if let Some(mod_name) = &module {
            match self.loaded_modules.get(mod_name) {
                Some(Value::Module { scope, .. }) => {
                    vec![Rc::new(RefCell::new(scope.clone()))]
                }
                _ => match &captured {
                    Some(chain) => chain.clone(),
                    None => vec![Rc::new(RefCell::new(HashMap::new()))],
                },
            }
        } else {
            match &captured {
                Some(chain) => chain.clone(),
                None => vec![Rc::new(RefCell::new(HashMap::new()))],
            }
        };
        let saved_scopes = std::mem::replace(&mut self.scopes, base);
        let mut scope = HashMap::new();
        for (p, v) in params.iter().zip(args.into_iter()) {
            scope.insert(p.clone(), v);
        }
        scope.insert("نفس".to_string(), Value::Instance(Rc::clone(&instance)));
        self.scopes.push(Rc::new(RefCell::new(scope)));
        self.call_depth += 1;
        let result = self.exec_block(&body);
        // اجتماع 32، السؤال الخامس: كـ call_user — خطأ
        // داخل جسم طريقة وحدة يُسنَد إلى ملف الوحدة.
        let result = match &module {
            Some(m) => {
                result.map_err(|e| e.with_file_if_none(self.module_files.get(m).cloned()))
            }
            None => result,
        };
        self.scopes.pop();
        self.scopes = saved_scopes;
        self.call_depth -= 1;
        match result? {
            Flow::Return => {
                let v = self.return_value.take().unwrap_or(Value::Nil);
                if let Some(rt) = &return_type {
                    if rt != "أي" && v.type_name() != rt {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الطريقة {} معلنة أن ترجع {}، لكن رجعت {}",
                                name,
                                rt,
                                v.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(v)
            }
            Flow::Normal => {
                if let Some(rt) = &return_type {
                    if rt != "أي" && rt != "لاشيء" {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الطريقة {} معلنة أن ترجع {}، لكن لم ترجع شيئا",
                                name, rt
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(Value::Nil)
            }
            Flow::Break => Err(RioError::runtime("اكسر خارج حلقة", line, col)),
            Flow::Continue => Err(RioError::runtime("تابع خارج حلقة", line, col)),
        }
    }

    // Meeting 14: string/list methods. String methods are pure
    // (they return new strings). List methods أضف/أزل mutate and
    // write back to a plain variable only (Meeting 11 lexical
    // assignment); a non-variable target is a clear runtime error.
    fn call_builtin_method(
        &mut self,
        target: &Expr,
        target_value: Value,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let arity = |expected: usize| -> RioResult<()> {
            if args.len() != expected {
                Err(RioError::runtime(
                    format!(
                        "الطريقة {} تتوقع {} معاملات لكن أعطيت {}",
                        name, expected, args.len()
                    ),
                    line, col,
                ))
            } else {
                Ok(())
            }
        };
        match &target_value {
            Value::Str(s) => {
                let s = s.clone();
                match name {
                    // طول counts characters, like the طول builtin,
                    // never bytes (Arabic text must count letters).
                    "طول" => {
                        arity(0)?;
                        Ok(Value::Number(s.chars().count() as f64))
                    }
                    "كبير" => {
                        arity(0)?;
                        Ok(Value::Str(s.to_uppercase()))
                    }
                    "صغير" => {
                        arity(0)?;
                        Ok(Value::Str(s.to_lowercase()))
                    }
                    "قص" => {
                        arity(0)?;
                        Ok(Value::Str(s.trim().to_string()))
                    }
                    "استبدل" => {
                        arity(2)?;
                        let (old, new) = match (&args[0], &args[1]) {
                            (Value::Str(a), Value::Str(b)) => (a.clone(), b.clone()),
                            _ => {
                                return Err(RioError::typ(
                                    "طريقة استبدل تتوقع نصين: القديم والجديد",
                                    line, col,
                                ))
                            }
                        };
                        Ok(Value::Str(s.replace(old.as_str(), new.as_str())))
                    }
                    "قسم" => {
                        arity(1)?;
                        let sep = match &args[0] {
                            Value::Str(sep) => sep.clone(),
                            _ => {
                                return Err(RioError::typ(
                                    "طريقة قسم تتوقع نصا كفاصل",
                                    line, col,
                                ))
                            }
                        };
                        Ok(Value::List(
                            s.split(sep.as_str())
                                .map(|p| Value::Str(p.to_string()))
                                .collect(),
                        ))
                    }
                    "يحتوي" => {
                        arity(1)?;
                        match &args[0] {
                            Value::Str(sub) => Ok(Value::Bool(s.contains(sub.as_str()))),
                            _ => Err(RioError::typ(
                                "طريقة يحتوي على نص تتوقع نصا",
                                line, col,
                            )),
                        }
                    }
                    _ => Err(RioError::typ(
                        format!("طريقة غير معروفة لنوع نص: {}", name),
                        line, col,
                    )),
                }
            }
            Value::List(items) => {
                let mut items = items.clone();
                match name {
                    "طول" => {
                        arity(0)?;
                        Ok(Value::Number(items.len() as f64))
                    }
                    "يحتوي" => {
                        arity(1)?;
                        Ok(Value::Bool(items.iter().any(|x| *x == args[0])))
                    }
                    "أضف" => {
                        arity(1)?;
                        self.require_plain_var_target(target, line, col)?;
                        items.push(args[0].clone());
                        self.write_back_list(target, items);
                        Ok(Value::Nil)
                    }
                    "أزل" => {
                        if args.len() > 1 {
                            return Err(RioError::runtime(
                                format!(
                                    "الطريقة أزل تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                                    args.len()
                                ),
                                line, col,
                            ));
                        }
                        self.require_plain_var_target(target, line, col)?;
                        let removed = if args.is_empty() {
                            items.pop().ok_or_else(|| {
                                RioError::runtime("لا يمكن الإزالة من قائمة فارغة", line, col)
                            })?
                        } else {
                            let idx = match &args[0] {
                                Value::Number(x) if x.fract() == 0.0 && *x >= 0.0 => *x as usize,
                                _ => {
                                    return Err(RioError::typ(
                                        "طريقة أزل تتوقع فهرسا عددا صحيحا",
                                        line, col,
                                    ))
                                }
                            };
                            if idx >= items.len() {
                                return Err(RioError::runtime(
                                    format!(
                                        "فهرس خارج النطاق: {} والقائمة طولها {}",
                                        idx,
                                        items.len()
                                    ),
                                    line, col,
                                ));
                            }
                            items.remove(idx)
                        };
                        self.write_back_list(target, items);
                        Ok(removed)
                    }
                    _ => Err(RioError::typ(
                        format!("طريقة غير معروفة لنوع قائمة: {}", name),
                        line, col,
                    )),
                }
            }
            _ => unreachable!(),
        }
    }

    /// Mutating list methods (أضف/أزل) write back only to a plain
    /// variable target. Anything else (a call result, an index, a
    /// field) has nowhere to write back to, so it is an error
    /// before any mutation happens.
    fn require_plain_var_target(
        &self,
        target: &Expr,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        match &target.kind {
            ExprKind::Ident(_) => Ok(()),
            _ => Err(RioError::runtime(
                "طريقة معدلة للقائمة (أضف/أزل) تحتاج متغيرا عاديا، لا نتيجة تعبير",
                line, col,
            )),
        }
    }

    /// Write a mutated list back through lexical assignment
    /// (Meeting 11): the nearest existing binding is updated, so
    /// a list captured by a closure is updated in place too.
    /// Caller already checked the target is a plain variable.
    fn write_back_list(&mut self, target: &Expr, items: Vec<Value>) {
        if let ExprKind::Ident(name) = &target.kind {
            self.assign_local(name, Value::List(items));
        }
    }

    /// Call a builtin by name with pre-built values. Crate-visible
    /// so the REPL shortcut layer (ات، مك، رح...) can invoke the
    /// file builtins directly, without re-parsing a call string
    /// (which would need commas — the shortcuts never use them).
    pub(crate) fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        // Meeting 17 (WASM): in the browser there is no filesystem
        // and no stdin, and user code is untrusted — these three
        // builtins are inert, with a clean Arabic error, instead
        // of touching std::fs / stdin at all.
        #[cfg(target_arch = "wasm32")]
        if matches!(
            name,
            "اقرأ_ملف"
                | "اكتب_ملف"
                | "ادخل"
                | "اعرض_الملفات"
                | "المجلد_الحالي"
                | "انتقل"
                | "أنشئ_مجلد"
                | "احذف"
                | "احذف_مجلد"
                | "احذف_ملف"
                | "انسخ"
                | "انقل"
                | "أنشئ_ملف"
                | "اعرض_البداية"
                | "اعرض_النهاية"
                | "عد_الأسطر"
                | "عد_الكلمات"
                | "ابحث_في_ملف"
                | "ابحث_عن"
                | "اعرض_الشجرة"
                | "حجم_المسار"
                | "أنشئ_رابط"
                | "موجود"
                | "نوع_المسار"
                | "عد_الملفات"
                | "تفاصيل_المسار"
                | "تثبيت"
        ) {
            return Err(RioError::runtime(
                "هذه الدالة غير مدعومة في متصفح الويب",
                line,
                col,
            ));
        }
        let arity = |expected: usize| -> RioResult<()> {
            if args.len() != expected {
                Err(RioError::runtime(
                    format!(
                        "الدالة المدمجة {} تتوقع {} معاملات لكن أعطيت {}",
                        name,
                        expected,
                        args.len()
                    ),
                    line,
                    col,
                ))
            } else {
                Ok(())
            }
        };
        match name {
            "طول" => {
                arity(1)?;
                Ok(match &args[0] {
                    Value::List(items) => Value::Number(items.len() as f64),
                    Value::Str(s) => Value::Number(s.chars().count() as f64),
                    Value::Dict(pairs) => Value::Number(pairs.len() as f64),
                    other => {
                        return Err(RioError::typ(
                            format!("طول يتطلب قائمة أو نصا أو قاموسا، وليس {}", other.type_name()),
                            line,
                            col,
                        ));
                    }
                })
            }
            "مدى" => {
                if args.is_empty() || args.len() > 3 {
                    return Err(RioError::runtime(
                        "مدى تتوقع 1 إلى 3 معاملات",
                        line,
                        col,
                    ));
                }
                let ints: Vec<i64> = args
                    .iter()
                    .map(|v| match v {
                        Value::Number(x) if x.fract() == 0.0 => Ok(*x as i64),
                        other => Err(RioError::typ(
                            format!("مدى يتطلب أعدادا صحيحة، وليس {}", other.type_name()),
                            line,
                            col,
                        )),
                    })
                    .collect::<RioResult<Vec<i64>>>()?;
                let (start, end, step) = match ints.len() {
                    1 => (0, ints[0], 1),
                    2 => (ints[0], ints[1], 1),
                    _ => (ints[0], ints[1], ints[2]),
                };
                if step == 0 {
                    return Err(RioError::runtime("خطوة مدى لا يمكن أن تكون صفرا", line, col));
                }
                let mut out = Vec::new();
                let mut cur = start;
                while (step > 0 && cur < end) || (step < 0 && cur > end) {
                    out.push(Value::Number(cur as f64));
                    cur += step;
                    if out.len() > MAX_LOOP_ITERATIONS {
                        return Err(RioError::runtime("نطاق مدى كبير جدا", line, col));
                    }
                }
                Ok(Value::List(out))
            }
            "مجموع" => {
                arity(1)?;
                match &args[0] {
                    Value::List(items) => {
                        let mut sum = 0.0;
                        for item in items {
                            match item {
                                Value::Number(n) => sum += n,
                                other => {
                                    return Err(RioError::typ(
                                        format!("مجموع يتطلب قائمة أعداد، وجد {}", other.type_name()),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                        Ok(Value::Number(sum))
                    }
                    other => Err(RioError::typ(
                        format!("مجموع يتطلب قائمة، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "أدنى" | "أقصى" => {
                arity(1)?;
                match &args[0] {
                    Value::List(items) => {
                        if items.is_empty() {
                            return Err(RioError::runtime("القائمة فارغة", line, col));
                        }
                        let mut nums = Vec::new();
                        for item in items {
                            match item {
                                Value::Number(n) => nums.push(*n),
                                other => {
                                    return Err(RioError::typ(
                                        format!("أدنى/أقصى يتطلب قائمة أعداد، وجد {}", other.type_name()),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                        let result = if name == "أدنى" {
                            nums.iter().cloned().fold(f64::INFINITY, f64::min)
                        } else {
                            nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                        };
                        Ok(Value::Number(result))
                    }
                    other => Err(RioError::typ(
                        format!("أدنى/أقصى يتطلب قائمة، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "مطلق" => {
                arity(1)?;
                match &args[0] {
                    Value::Number(n) => Ok(Value::Number(n.abs())),
                    other => Err(RioError::typ(
                        format!("مطلق يتطلب عددا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "عدد" => {
                arity(1)?;
                match &args[0] {
                    Value::Number(n) => Ok(Value::Number(*n)),
                    Value::Bool(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
                    Value::Str(s) => {
                        let cleaned: String = s
                            .trim()
                            .chars()
                            .map(|c| match c {
                                '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
                                other => other,
                            })
                            .collect();
                        cleaned.parse::<f64>().map(Value::Number).map_err(|_| {
                            RioError::typ(
                                format!("لا يمكن تحويل النص إلى عدد: {}", s),
                                line,
                                col,
                            )
                        })
                    }
                    other => Err(RioError::typ(
                        format!("لا يمكن تحويل نوع {} إلى عدد", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "نص" => {
                arity(1)?;
                Ok(Value::Str(args[0].fmt_value()))
            }
            "نوع" => {
                arity(1)?;
                Ok(Value::Str(args[0].type_name().to_string()))
            }
            "اقرأ_ملف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let display = self.file_policy.display_str(path);
                        Self::read_file_capped(&resolved, &display, line, col)
                            .map(Value::Str)
                    }
                    other => Err(RioError::typ(
                        format!("اقرأ_ملف يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "اكتب_ملف" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(path), Value::Str(content)) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        // Audit م-20: refuse a write that would
                        // push the program dir past its quota
                        // (net growth only — rewriting a file
                        // counts its size difference). The check
                        // and the write are atomic under the
                        // shared file-write lock (task 19): an
                        // async write worker can no longer slip
                        // between this check and this write.
                        let _write_guard = crate::files::FILE_WRITE_LOCK
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        let existing =
                            std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
                        let extra = (content.len() as u64).saturating_sub(existing);
                        self.check_disk_quota(extra, line, col)?;
                        std::fs::write(&resolved, content).map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر كتابة الملف {}: {}", self.file_policy.display_str(path), e),
                                line,
                                col,
                            )
                        })
                    }
                    _ => Err(RioError::typ(
                        "اكتب_ملف يتطلب مسارا ونصا",
                        line,
                        col,
                    )),
                }
            }
            // Arabic terminal shortcuts (owner's order
            // 2026-11-05): ls / pwd / cd / mkdir / rm / cp / mv
            // as builtins, so they work in programs and in the
            // REPL alike. All respect the file policy (the
            // session cwd, inside program_dir unless trusted).
            "اعرض_الملفات" => {
                arity(0)?;
                let dir = self.resolve_cwd(".", line, col)?;
                let entries = std::fs::read_dir(&dir).map_err(|e| {
                    RioError::new(ErrorKind::Io, format!("تعذر عرض المجلد: {}", e), line, col)
                })?;
                // Audit م-14: cap the listing at the first 1000
                // names (sorted) and count the rest instead of
                // building a list of the whole directory.
                const LIST_CAP: usize = 1000;
                let mut names: Vec<String> = Vec::new();
                let mut total = 0usize;
                for entry in entries {
                    let entry = entry.map_err(|e| {
                        RioError::new(ErrorKind::Io, format!("تعذر عرض المجلد: {}", e), line, col)
                    })?;
                    total += 1;
                    if names.len() < LIST_CAP {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if is_dir {
                            names.push(format!("{}/", name));
                        } else {
                            names.push(name);
                        }
                    }
                }
                names.sort();
                let mut out: Vec<Value> = names.into_iter().map(Value::Str).collect();
                if total > LIST_CAP {
                    // Arabic digits, like every other number the
                    // terminal shows.
                    let more = Value::Number((total - LIST_CAP) as f64).fmt_value();
                    out.push(Value::Str(format!("…و {} أخرى", more)));
                }
                Ok(Value::List(out))
            }
            "المجلد_الحالي" => {
                arity(0)?;
                // Terminal display (owner's order 2026-11-05
                // ~22:38): home shows as `~`, below it `~/...`
                // — never the absolute internal path.
                Ok(Value::Str(self.file_policy.display_path(&self.cwd)))
            }
            "انتقل" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(dir) => {
                        let resolved = self.resolve_cwd(dir, line, col)?;
                        match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => {
                                self.cwd = Self::normalize_lexical(&resolved);
                                // مهمة 10: تغيّر مجلد الجلسة —
                                // أعد مزامنة سياسة العمليات حالا،
                                // فلا يبقى `أنشئ` على المجلد القديم
                                // بقية هذا التقييم.
                                crate::processes::set_process_policy(
                                    self.program_dir.clone(),
                                    self.file_policy.trusted,
                                    self.cwd.clone(),
                                );
                                Ok(Value::Nil)
                            }
                            Ok(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("ليس مجلدا: {}", self.file_policy.display_str(dir)),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("المجلد غير موجود: {}", self.file_policy.display_str(dir)),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("انتقل يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "أنشئ_مجلد" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(dir) => {
                        let resolved = self.resolve_cwd(dir, line, col)?;
                        std::fs::create_dir(&resolved).map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر إنشاء المجلد {}: {}", self.file_policy.display_str(dir), e),
                                line,
                                col,
                            )
                        })
                    }
                    other => Err(RioError::typ(
                        format!("أنشئ_مجلد يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "احذف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => {
                                std::fs::remove_dir(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        ErrorKind::Io,
                                        format!("تعذر حذف {}: {}", self.file_policy.display_str(path), e),
                                        line,
                                        col,
                                    )
                                })
                            }
                            Ok(_) => {
                                std::fs::remove_file(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        ErrorKind::Io,
                                        format!("تعذر حذف {}: {}", self.file_policy.display_str(path), e),
                                        line,
                                        col,
                                    )
                                })
                            }
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("احذف يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "احذف_مجلد" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => {
                                std::fs::remove_dir(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        ErrorKind::Io,
                                        format!("تعذر حذف المجلد {}: {}", self.file_policy.display_str(path), e),
                                        line,
                                        col,
                                    )
                                })
                            }
                            Ok(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("ليس مجلدا: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("احذف_مجلد يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "احذف_ملف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        match std::fs::metadata(&resolved) {
                            Ok(m) if !m.is_dir() => {
                                std::fs::remove_file(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        ErrorKind::Io,
                                        format!("تعذر حذف الملف {}: {}", self.file_policy.display_str(path), e),
                                        line,
                                        col,
                                    )
                                })
                            }
                            Ok(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("ليس ملفا: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("احذف_ملف يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "انسخ" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd(dst, line, col)?;
                        // cp into a directory keeps the file name.
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // Audit م-20: same quota as اكتب_ملف.
                        let src_len =
                            std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
                        let dst_existing =
                            std::fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
                        let extra = src_len.saturating_sub(dst_existing);
                        self.check_disk_quota(extra, line, col)?;
                        std::fs::copy(&src_path, &dst_path).map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر نسخ {}: {}", self.file_policy.display_str(src), e),
                                line,
                                col,
                            )
                        })
                    }
                    _ => Err(RioError::typ("انسخ يتطلب مسارين نصيين", line, col)),
                }
            }
            "انقل" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd(dst, line, col)?;
                        // mv into a directory keeps the file name.
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        std::fs::rename(&src_path, &dst_path).map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر نقل {}: {}", self.file_policy.display_str(src), e),
                                line,
                                col,
                            )
                        })
                    }
                    _ => Err(RioError::typ("انقل يتطلب مسارين نصيين", line, col)),
                }
            }
            // More file builtins for the 50 terminal shortcuts
            // (owner's order 2026-11-05 ~22:11): touch / head /
            // tail / wc / grep / find / tree / du / ln / date /
            // version / test / file-type / count.
            "أنشئ_ملف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        if !resolved.exists() {
                            std::fs::write(&resolved, "").map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!("تعذر إنشاء الملف {}: {}", self.file_policy.display_str(path), e),
                                    line,
                                    col,
                                )
                            })?;
                        }
                        Ok(Value::Nil)
                    }
                    other => Err(RioError::typ(
                        format!("أنشئ_ملف يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "اعرض_البداية" | "اعرض_النهاية" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(RioError::runtime(
                        format!("{} تتوقع 1 أو 2 معاملات", name),
                        line,
                        col,
                    ));
                }
                let path = match &args[0] {
                    Value::Str(p) => p.clone(),
                    other => {
                        return Err(RioError::typ(
                            format!("{} يتطلب مسارا نصيا، وليس {}", name, other.type_name()),
                            line,
                            col,
                        ));
                    }
                };
                let count = if args.len() == 2 {
                    Self::count_arg(&args[1], line, col)?
                } else {
                    5
                };
                let resolved = self.resolve_cwd(&path, line, col)?;
                let display = self.file_policy.display_str(&path);
                let content = Self::read_file_capped(&resolved, &display, line, col)?;
                let lines: Vec<&str> = content.lines().collect();
                let out = if name == "اعرض_البداية" {
                    lines.iter().take(count).cloned().collect::<Vec<_>>().join("\n")
                } else {
                    let skip = lines.len().saturating_sub(count);
                    lines.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n")
                };
                Ok(Value::Str(out))
            }
            "عد_الأسطر" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let display = self.file_policy.display_str(path);
                        let content = Self::read_file_capped(&resolved, &display, line, col)?;
                        Ok(Value::Number(content.lines().count() as f64))
                    }
                    other => Err(RioError::typ(
                        format!("عد_الأسطر يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "عد_الكلمات" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let display = self.file_policy.display_str(path);
                        let content = Self::read_file_capped(&resolved, &display, line, col)?;
                        Ok(Value::Number(content.split_whitespace().count() as f64))
                    }
                    other => Err(RioError::typ(
                        format!("عد_الكلمات يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "ابحث_في_ملف" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(pattern), Value::Str(path)) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let display = self.file_policy.display_str(path);
                        let content = Self::read_file_capped(&resolved, &display, line, col)?;
                        let hits: Vec<Value> = content
                            .lines()
                            .filter(|l| l.contains(pattern.as_str()))
                            .take(100)
                            .map(|l| Value::Str(l.to_string()))
                            .collect();
                        Ok(Value::List(hits))
                    }
                    _ => Err(RioError::typ("ابحث_في_ملف يتطلب نصا ومسارا", line, col)),
                }
            }
            "ابحث_عن" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(query) => {
                        let mut out: Vec<String> = Vec::new();
                        Self::find_files(&self.cwd, &self.cwd, query, 0, &mut out);
                        out.sort();
                        Ok(Value::List(out.into_iter().map(Value::Str).collect()))
                    }
                    other => Err(RioError::typ(
                        format!("ابحث_عن يتطلب نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "اعرض_الشجرة" => {
                if args.len() > 1 {
                    return Err(RioError::runtime("اعرض_الشجرة تتوقع 0 أو 1 معاملات", line, col));
                }
                let dir = if let Some(Value::Str(d)) = args.first() {
                    self.resolve_cwd(d, line, col)?
                } else {
                    self.cwd.clone()
                };
                let mut out: Vec<String> = Vec::new();
                Self::tree_lines(&dir, "", 0, &mut out);
                Ok(Value::List(out.into_iter().map(Value::Str).collect()))
            }
            "حجم_المسار" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let mut budget: usize = 10000;
                        Ok(Value::Number(Self::dir_size(&resolved, &mut budget) as f64))
                    }
                    other => Err(RioError::typ(
                        format!("حجم_المسار يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "أنشئ_رابط" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(target), Value::Str(link)) => {
                        let target_path = self.resolve_cwd(target, line, col)?;
                        let link_path = self.resolve_cwd(link, line, col)?;
                        #[cfg(unix)]
                        let res = std::os::unix::fs::symlink(&target_path, &link_path);
                        // Windows يفرّق بين رابط الملف ورابط المجلد:
                        // يُختار حسب نوع الهدف، والهدف الغائب يُربط
                        // كملف (الحالة الشائعة). الرفض عند غياب
                        // الامتياز يصل كخطأ إدخال/إخراج عربي كغيره.
                        #[cfg(windows)]
                        let res = match std::fs::metadata(&target_path) {
                            Ok(m) if m.is_dir() => std::os::windows::fs::symlink_dir(
                                &target_path,
                                &link_path,
                            ),
                            _ => std::os::windows::fs::symlink_file(
                                &target_path,
                                &link_path,
                            ),
                        };
                        #[cfg(not(any(unix, windows)))]
                        let res: std::io::Result<()> = Err(std::io::Error::new(
                            std::io::ErrorKind::Unsupported,
                            "إنشاء الروابط غير مدعوم على هذه المنصة",
                        ));
                        res.map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر إنشاء الرابط {}: {}", self.file_policy.display_str(link), e),
                                line,
                                col,
                            )
                        })
                    }
                    _ => Err(RioError::typ("أنشئ_رابط يتطلب مسارين نصيين", line, col)),
                }
            }
            "التاريخ_الآن" => {
                arity(0)?;
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Ok(Value::Str(Self::format_unix_utc(secs)))
            }
            "النسخة" => {
                arity(0)?;
                Ok(Value::Str(crate::repl::banner().to_string()))
            }
            // اجتماع 32، السؤال السادس: النسخة المجردة
            // وحدها (مثال «١٫٠٫٠»)؛ و`النسخة()` تبقى
            // اللافتة كما هي بلا كسر لكود «حزم».
            "إصدار_المحرك" => {
                arity(0)?;
                Ok(Value::Str(crate::repl::version().to_string()))
            }
            "موجود" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        Ok(Value::Bool(std::fs::metadata(&resolved).is_ok()))
                    }
                    other => Err(RioError::typ(
                        format!("موجود يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "نوع_المسار" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let kind = match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => "مجلد",
                            Ok(_) => "ملف",
                            Err(_) => "غير موجود",
                        };
                        Ok(Value::Str(kind.to_string()))
                    }
                    other => Err(RioError::typ(
                        format!("نوع_المسار يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // تفاصيل المسار (stat): نوعه وحجمه وتاريخ آخر
            // تعديل — قاموس واحد، لا خطأ على الغائب (مثل
            // نوع_المسار: «غير موجود»، حجم ٠، بلا تاريخ).
            "تفاصيل_المسار" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let kind = match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => "مجلد",
                            Ok(_) => "ملف",
                            Err(_) => "غير موجود",
                        };
                        let mut budget: usize = 10000;
                        let size = Self::dir_size(&resolved, &mut budget);
                        let modified = std::fs::metadata(&resolved)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok()
                            })
                            .map(|d| Self::format_unix_utc(d.as_secs()))
                            .unwrap_or_else(|| "—".to_string());
                        Ok(Value::Dict(vec![
                            ("نوع".to_string(), Value::Str(kind.to_string())),
                            ("حجم".to_string(), Value::Number(size as f64)),
                            ("معدل".to_string(), Value::Str(modified)),
                        ]))
                    }
                    other => Err(RioError::typ(
                        format!(
                            "تفاصيل_المسار يتطلب مسارا نصيا، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            // تثبيت أدوات نيفا (أمر الاسم الكامل، بلا كود
            // حرفين): من رابط أو مسار محلي إلى مجلد البرنامج.
            // التنفيذ المشترك في crate::install — لا تيرمكس
            // ولا مدير حزم ولا ثنائيات أجنبية أبدا.
            "تثبيت" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(source) => crate::install::install_module(
                        &self.file_policy,
                        &self.cwd,
                        &self.program_dir,
                        source,
                        line,
                        col,
                    ),
                    other => Err(RioError::typ(
                        format!(
                            "تثبيت يتطلب مصدرا نصيا، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            "عد_الملفات" => {
                if args.len() > 1 {
                    return Err(RioError::runtime("عد_الملفات تتوقع 0 أو 1 معاملات", line, col));
                }
                let dir = if let Some(Value::Str(d)) = args.first() {
                    self.resolve_cwd(d, line, col)?
                } else {
                    self.cwd.clone()
                };
                let count = std::fs::read_dir(&dir)
                    .map(|entries| entries.count())
                    .map_err(|e| {
                        RioError::new(ErrorKind::Io, format!("تعذر عرض المجلد: {}", e), line, col)
                    })?;
                Ok(Value::Number(count as f64))
            }
            "ادخل" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        "ادخل تتوقع 0 أو 1 معاملات",
                        line,
                        col,
                    ));
                }
                if let Some(Value::Str(prompt)) = args.first() {
                    print!("{}", prompt);
                    let _ = std::io::stdout().flush();
                } else if let Some(other) = args.first() {
                    return Err(RioError::typ(
                        format!("ادخل يتطلب نصا للسؤال، وليس {}", other.type_name()),
                        line,
                        col,
                    ));
                }
                // مهمة 22: القراءة عبر مضخة stdin (لا
                // مباشرة) — خيط المضخة وحده يملك stdin؛
                // وEOF يرجع نصا فارغا كما كان (read_line
                // حين تعيد 0).
                let line_input = crate::stdin_pump::stdin_line().unwrap_or_default();
                let trimmed = line_input.trim_end_matches(['\n', '\r']).to_string();
                Ok(Value::Str(trimmed))
            }
            _ => Err(RioError::runtime(
                format!("دالة غير معرفة: {}", name),
                line,
                col,
            )),
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

// --- سياق الدوال الأصلية (خادم HTTPS، اجتماع 27) ---
//
// المفسر يستدعي المعالج عبر مسار الاستدعاء العادي
// نفسه (دالة مستخدم / طريقة مربوطة / دالة أصلية)،
// فيلتقط المعالج البيئة المعجمية التي أُنشئ فيها
// (الإغلاقات تعمل)، وتنفيذ كل طلب معزول: النطاقات
// والمكدس والعمق محلية لكل استدعاء (call_user يبدّل
// النطاقات ويعيدها)، والمخرجات تُجمَع في self.output
// فنرجع دلتا المخرجات بعد كل استدعاء.
impl crate::stdlib::HandlerCaller for Interpreter {
    fn call_handler(
        &mut self,
        func: &Value,
        arg: Value,
        line: usize,
        col: usize,
    ) -> (RioResult<Value>, String) {
        let before = self.output.len();
        let res = match func.clone() {
            Value::Function { .. } => self.call_user(func.clone(), vec![arg], line, col),
            Value::BoundMethod { func: f, instance } => {
                self.call_method(*f, instance, vec![arg], line, col)
            }
            Value::NativeFunction { func: f, .. } => f(self, &[arg], line, col),
            other => Err(RioError::typ(
                format!("القيمة {} ليست دالة", other.type_name()),
                line,
                col,
            )),
        };
        let produced = self.output[before..].to_string();
        (res, produced)
    }

    fn server_allowed(&self) -> bool {
        self.server_allowed
    }

    fn server_file_policy(&self) -> crate::permissions::FilePolicy {
        self.file_policy.clone()
    }

    fn server_cwd(&self) -> std::path::PathBuf {
        self.cwd.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;

    fn run_src(src: &str) -> (String, Result<(), RioError>) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = Interpreter::new();
        let res = interp.run(&prog);
        (interp.output, res)
    }

    /// Run source in an existing interpreter (keeps its cwd),
    /// clearing the output first so each call's output is its own.
    fn run_in(interp: &mut Interpreter, src: &str) -> (String, Result<(), RioError>) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        interp.output.clear();
        let res = interp.run(&prog);
        (interp.output.clone(), res)
    }

    #[test]
    fn function_scope_is_local() {
        let (out, res) = run_src(
            "دالة جمع(أ، ب):\n    نتيجة = أ + ب\n    ارجع نتيجة\nاطبع جمع(٢، ٣)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٥\n");
        // Local variable must NOT be visible outside the function.
        let (_, res2) = run_src(
            "دالة جمع(أ، ب):\n    نتيجة = أ + ب\n    ارجع نتيجة\nاطبع جمع(٢، ٣)\nاطبع نتيجة\n",
        );
        assert!(res2.is_err());
        assert!(res2.unwrap_err().to_string().contains("متغير غير معرف"));
    }

    #[test]
    fn loop_variable_visible_after_loop() {
        let (out, res) = run_src("لكل رقم في مدى(٣):\n    اطبع رقم\nاطبع رقم\n");
        assert!(res.is_ok());
        assert_eq!(out, "٠\n١\n٢\n٢\n");
    }

    #[test]
    fn type_error_string_plus_number() {
        let (_, res) = run_src("اطبع \"أ\" + ١\n");
        let err = res.unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Type);
    }

    #[test]
    fn if_condition_must_be_bool() {
        let (_, res) = run_src("إذا ١:\n    اطبع ١\n");
        assert!(res.is_err());
    }

    // Step 1 (Meeting 10/11): lexical closures + lexical assignment.
    #[test]
    fn closure_returned_sees_outer_variable() {
        let (out, res) = run_src(
            "دالة اصنع():\n    س = ١٠\n    دالة احصل():\n        ارجع س\n    ارجع احصل\nد = اصنع()\nاطبع د()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١٠\n");
    }

    #[test]
    fn closure_counter_shares_state() {
        let (out, res) = run_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع أ()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١\n٢\n٣\n");
    }

    #[test]
    fn closure_two_counters_are_independent() {
        let (out, res) = run_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nب = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع ب()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١\n٢\n١\n");
    }

    #[test]
    fn closure_sees_later_mutation_of_outer() {
        let (out, res) = run_src(
            "س = ١\nدالة احصل():\n    ارجع س\nس = ٢\nاطبع احصل()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٢\n");
    }

    #[test]
    fn closure_param_shadows_captured() {
        let (out, res) = run_src(
            "س = ١٠\nدالة احصل(س):\n    ارجع س\nاطبع احصل(٥)\nاطبع س\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٥\n١٠\n");
    }

    #[test]
    fn lexical_assignment_mutates_outer() {
        let (out, res) = run_src(
            "س = ١٠\nدالة غير():\n    س = ٢٠\nغير()\nاطبع س\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٢٠\n");
    }

    #[test]
    fn closure_recursion_still_works() {
        let (out, res) = run_src(
            "دالة فيب(ن):\n    إذا ن <= ١:\n        ارجع ن\n    ارجع فيب(ن - ١) + فيب(ن - ٢)\nاطبع فيب(١٠)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٥٥\n");
    }

    // Step 1 (Meeting 13): lambdas — دالة_مجهولة(س): س + 1.
    #[test]
    fn lambda_basic_value_and_call() {
        let (out, res) = run_src(
            "مضاعف = دالة_مجهولة(س): س * 2\nاطبع مضاعف(5)\nاطبع مضاعف\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١٠\n<دالة مجهولة>\n");
    }

    #[test]
    fn lambda_passed_as_argument() {
        let (out, res) = run_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س): س * 3، 4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١٢\n");
    }

    #[test]
    fn lambda_factory_captures_param() {
        let (out, res) = run_src(
            "دالة اصنع_مضاعف(ن):\n    ارجع دالة_مجهولة(س): س * ن\nمضاعف3 = اصنع_مضاعف(3)\nاطبع مضاعف3(4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١٢\n");
    }

    #[test]
    fn lambda_captures_outer_variable() {
        let (out, res) = run_src(
            "س = 10\nاحصل = دالة_مجهولة(): س\nاطبع احصل()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١٠\n");
    }

    #[test]
    fn lambda_typed_param_checked_at_runtime() {
        let (_, res) = run_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س: عدد): س + 1، \"نص\")\n",
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("نوع غير متطابق"));
    }

    #[test]
    fn lambda_immediate_call_via_parens() {
        let (out, res) = run_src(
            "د = (دالة_مجهولة(س): س + 1)(10)\nاطبع د\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "١١\n");
    }

    #[test]
    fn lambda_currying_nested_lambda() {
        let (out, res) = run_src(
            "مركب = دالة_مجهولة(س): دالة_مجهولة(ص): س + ص\nاطبع مركب(3)(4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٧\n");
    }

    // Step 1 (Meeting 14): string/list methods.
    #[test]
    fn str_methods_basic() {
        let (out, res) = run_src(
            "اطبع \"مرحبا\".طول()\nاطبع \"Hello\".كبير()\nاطبع \"Hello\".صغير()\nاطبع \"  محمود  \".قص()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٥\nHELLO\nhello\nمحمود\n");
    }

    #[test]
    fn str_replace_split_contains() {
        let (out, res) = run_src(
            "اطبع \"أهلا محمود\".استبدل(\"محمود\"، \"سارة\")\nاطبع \"أ، ب\".قسم(\"، \")\nاطبع \"مرحبا\".يحتوي(\"رحب\")\nاطبع \"مرحبا\".يحتوي(\"وداعا\")\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "أهلا سارة\n[\"أ\", \"ب\"]\nصحيح\nخطأ\n");
    }

    #[test]
    fn list_append_writes_back() {
        let (out, res) = run_src(
            "ق = [1، 2]\nق.أضف(3)\nاطبع ق\nاطبع ق.طول()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "[١, ٢, ٣]\n٣\n");
    }

    #[test]
    fn list_pop_last_and_index() {
        let (out, res) = run_src(
            "ق = [1، 2، 3]\nاطبع ق.أزل()\nاطبع ق.أزل(0)\nاطبع ق\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "٣\n١\n[٢]\n");
    }

    #[test]
    fn list_method_unknown_is_runtime_error() {
        // Untyped variable: the analyzer cannot know the type,
        // so the unknown method fails at runtime, not analysis.
        let (_, res) = run_src("ق = [1]\nاطبع ق.غير_موجود()\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("طريقة غير معروفة لنوع قائمة"));
    }

    #[test]
    fn list_append_on_call_result_is_error() {
        let (_, res) = run_src(
            "دالة اصنع():\n    ارجع [1، 2]\nاصنع().أضف(3)\n",
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("تحتاج متغيرا عاديا"));
    }

    #[test]
    fn list_pop_from_empty_is_error() {
        let (_, res) = run_src("ق = []\nاطبع ق.أزل()\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("قائمة فارغة"));
    }

    // Step 2 (Meeting 16): initial stdlib — native modules.
    #[test]
    fn stdlib_math_basic() {
        let (out, res) = run_src(
            "استورد رياضيات\nاطبع رياضيات.جذر(16)\nاطبع رياضيات.قوة(2، 10)\nاطبع رياضيات.جيب(0)\nاطبع رياضيات.أرضية(3.7)\nاطبع رياضيات.سقف(3.2)\nاطبع رياضيات.باي\nاطبع رياضيات.هـ\nاطبع رياضيات.لوغاريتم(رياضيات.هـ)\n",
        );
        assert!(res.is_ok());
        assert_eq!(
            out,
            "٤\n١٠٢٤\n٠\n٣\n٤\n٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥\n١\n"
        );
    }

    #[test]
    fn stdlib_math_errors() {
        let (_, res) = run_src("استورد رياضيات\nاطبع رياضيات.جذر(-1)\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("جذر عدد سالب"));
        let (_, res) = run_src("استورد رياضيات\nاطبع رياضيات.جذر(1، 2)\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("تتوقع 1 معاملات"));
    }

    #[test]
    fn stdlib_from_import_binds_native_function() {
        let (out, res) = run_src("من رياضيات استورد جذر\nاطبع جذر(25)\nاطبع نوع(جذر)\n");
        assert!(res.is_ok());
        assert_eq!(out, "٥\nدالة\n");
    }

    #[test]
    fn engine_version_bare_and_banner() {
        // اجتماع 32، السؤال السادس: إصدار_المحرك()
        // ترجع النسخة المجردة؛ والنسخة() تبقى اللافتة.
        let (out, res) = run_src("اطبع إصدار_المحرك()\nاطبع النسخة()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١٫٠٫٠\nمحرك نيفا ١٫٠٫٠\n");
        // بلا وسائط فقط: وسائط خطأ عدد.
        let (_, res) = run_src("اطبع إصدار_المحرك(١)\n");
        assert!(res.is_err());
    }

    #[test]
    fn bare_function_reference_is_an_error() {
        // اجتماع 32، السؤال الثاني (2): مرجع دالة
        // كعبارة مجردة خطأ يسمي الدالة.
        let (_, res) = run_src("دالة س():\n    ارجع ١\nس\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("دالة بلا استدعاء: س"));
        // دالة مستوردة مجردة (من وحدة أصلية).
        let (_, res) = run_src("من رياضيات استورد جذر\nجذر\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("دالة بلا استدعاء: جذر"));
        // دالة وحدة أصلية مجردة.
        let (_, res) = run_src("استورد جهاز\nجهاز.أزل_تنبيه\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("دالة بلا استدعاء: أزل_تنبيه"));
        // الإسناد والتمرير والطباعة تبقى صالحة.
        // (ملاحظة: المدمجات المسطحة مثل `طول` ليست قيما
        // أصلا — لا تُحل إلا في موضع استدعاء — فسلوكها
        // «متغير غير معرف» قائم قبل هذا الإصلاح ولا يمسه.)
        let (out, res) = run_src("من رياضيات استورد جذر\nس = جذر\nاطبع نوع(س)\nاطبع جذر\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "دالة\n<دالة جذر>\n");
        // استدعاء مجرد يرجع غير دالة: قيمته تُتجاهل.
        let (out, res) = run_src("دالة أعط():\n    ارجع ٥\nأعط()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "");
        // استدعاء مجرد يرجع دالة: خطأ أيضا (قيمة دالة
        // في عبارة مجردة) — اتساقا مع VM.
        let (_, res) = run_src(
            "دالة مصنع():\n    ارجع دالة_مجهولة(س): س\nمصنع()\n",
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("دالة بلا استدعاء: مجهولة"));
    }

    #[test]
    fn live_output_streams_without_retaining() {
        // اجتماع 32، السؤال الثامن: مع البث الحي لا
        // يُحتفظ بالسطر في output (يصل للمخرجات فورا)؛
        // وبدونه يُحتفظ كما كان.
        let mut interp = Interpreter::new();
        interp.set_live_output(true);
        let (_, res) = run_in(&mut interp, "اطبع ١\nاطبع ٢\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "");
        interp.set_live_output(false);
        let (out, res) = run_in(&mut interp, "اطبع ١\nاطبع ٢\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١\n٢\n");
    }

    #[test]
    fn runtime_error_names_source_file() {
        // اجتماع 32، السؤال الخامس: خطأ التشغيل يسمي
        // الملف المضبوط بـ set_source_file.
        let mut interp = Interpreter::new();
        interp.set_source_file("برنامج.نيفا");
        let (_, res) = run_in(&mut interp, "س = ١ / ٠\n");
        assert!(res.is_err());
        let e = res.unwrap_err();
        assert_eq!(e.file.as_deref(), Some("برنامج.نيفا"));
        assert!(e.to_string().contains("الملف برنامج.نيفا"));
        assert!(e.to_string().contains("قسمة على صفر"));
    }

    #[test]
    fn module_errors_name_module_file() {
        // اجتماع 32، السؤال الخامس: خطأ في وحدة — في
        // مستواها الأعلى أو داخل دالة منها تُستدعى من
        // الرئيسي — يُسنَد إلى ملف الوحدة لا الرئيسي.
        let dir = shortcuts_dir();
        std::fs::write(
            dir.join("م.نيفا"),
            "صدر اقسم\nدالة اقسم(س):\n    ارجع س / ٠\n",
        )
        .unwrap();
        std::fs::write(dir.join("رئيسي.نيفا"), "استورد م\nاطبع م.اقسم(١)\n").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        interp.set_source_file("رئيسي.نيفا");
        let (_, res) = run_in(&mut interp, "استورد م\nاطبع م.اقسم(١)\n");
        assert!(res.is_err());
        let e = res.unwrap_err();
        let shown = e.to_string();
        // البادئة «في وحدة م: » لزمن التحميل وحده؛ أما
        // خطأ جسم دالة تُستدعى بعد التحميل فيُسنَد
        // باسمه في العرض — وهذا ما يشترطه الحكم.
        assert!(shown.contains("قسمة على صفر"), "got: {}", shown);
        assert!(
            shown.contains("الملف") && shown.contains("م.نيفا"),
            "got: {}",
            shown
        );
        assert!(
            e.file.as_deref().map(|f| f.contains("م.نيفا")).unwrap_or(false),
            "file: {:?}",
            e.file
        );
        // والخطأ في المستوى الأعلى للوحدة نفسه.
        std::fs::write(dir.join("ع.نيفا"), "س = ١ / ٠\n").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "استورد ع\n");
        assert!(res.is_err());
        let e = res.unwrap_err();
        assert!(
            e.file.as_deref().map(|f| f.contains("ع.نيفا")).unwrap_or(false),
            "file: {:?}",
            e.file
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stdlib_random_ranges() {
        // Random values are never asserted exactly — only ranges
        // and types, so the test is deterministic.
        let (out, res) = run_src(
            "استورد عشوائي\nاطبع عشوائي.عدد_عشوائي() >= 0\nاطبع عشوائي.عدد_عشوائي() < 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) >= 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) <= 6\nاطبع عشوائي.اختيار([10، 20]) في [10، 20]\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\n");
    }

    #[test]
    fn stdlib_random_errors() {
        let (_, res) = run_src("استورد عشوائي\nاطبع عشوائي.اختيار([])\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("قائمة فارغة"));
        let (_, res) = run_src("استورد عشوائي\nاطبع عشوائي.صحيح_عشوائي(6، 1)\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("الحد الأدنى أكبر"));
    }

    #[test]
    fn stdlib_time_now_is_number() {
        let (out, res) = run_src("استورد وقت\nاطبع وقت.الآن() > 1700000000\n");
        assert!(res.is_ok());
        assert_eq!(out, "صحيح\n");
    }

    // Arabic terminal shortcuts (owner's order 2026-11-05):
    // ls / pwd / cd / mkdir / rm / cp / mv as builtins.
    fn shortcuts_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("niva-shortcuts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn shortcuts_list_pwd_cd_mkdir() {
        let dir = shortcuts_dir();
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(dir.join("فرعي")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // المجلد_الحالي() = home, displayed as `~` (owner's
        // order 2026-11-05 ~22:38 — terminal paths like any
        // Unix terminal, never the absolute internal path).
        let (out, res) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        // اعرض_الملفات() lists the file and the dir (with /).
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("أ.txt"), "got: {}", out);
        assert!(out.contains("فرعي/"), "got: {}", out);
        // أنشئ_مجلد + انتقل + .. back to the base.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"جديد\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(dir.join("جديد").is_dir());
        let (_, res) = run_in(&mut interp, "انتقل(\"جديد\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/جديد", "got: {}", out);
        // `~` jumps home, and `~/...` reads a file from home.
        let (_, res) = run_in(&mut interp, "انتقل(\"~\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع اقرأ_ملف(\"~/أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "نص", "got: {}", out);
        // A path error never leaks the internal Android path.
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"/data/data/com.rio4140.niva/files/خارج\")",
        );
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(!msg.contains("com.rio4140.niva"), "got: {}", msg);
        assert!(!msg.contains("/data/"), "got: {}", msg);
        let (_, res) = run_in(&mut interp, "انتقل(\"..\")");
        assert!(res.is_err(), "expected escape to be rejected");
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        // Escaping the base is rejected (restricted mode).
        let (_, res) = run_in(&mut interp, "احذف(\"../خارج.txt\")");
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // احذف only: انسخ / انقل are two-argument builtins and
    // their call syntax is on hold pending the owner's comma
    // decision (2026-11-05 «مفيش حاجه اسمها فصلة») — their
    // bodies are implemented, their tests wait for the ruling.
    #[test]
    fn shortcuts_delete_file_and_empty_dir() {
        let dir = shortcuts_dir();
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "احذف(\"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!dir.join("أ.txt").exists());
        // احذف on a missing path errors; on a non-empty dir errors.
        let (_, res) = run_in(&mut interp, "احذف(\"غيرموجود.txt\")");
        assert!(res.is_err());
        std::fs::create_dir(dir.join("ممتلئ")).unwrap();
        std::fs::write(dir.join("ممتلئ/داخل.txt"), "x").unwrap();
        let (_, res) = run_in(&mut interp, "احذف(\"ممتلئ\")");
        assert!(res.is_err());
        // An empty dir deletes fine.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"فارغ\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "احذف(\"فارغ\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!dir.join("فارغ").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_cd_affects_read() {
        // After انتقل, اقرأ_ملف resolves against the new
        // directory — cd is real, not cosmetic. (The اكتب_ملف
        // half of this check is on hold with the comma ruling:
        // it is a two-argument call.)
        let dir = shortcuts_dir();
        std::fs::create_dir(dir.join("داخل")).unwrap();
        std::fs::write(dir.join("داخل/ملف.txt"), "من الداخل").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انتقل(\"داخل\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, res) = run_in(&mut interp, "اطبع اقرأ_ملف(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("من الداخل"), "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_rmdir_rmfile_distinction() {
        let dir = shortcuts_dir();
        std::fs::write(dir.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(dir.join("مجلد")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // احذف_مجلد is dir-only: a file is an error.
        let (_, res) = run_in(&mut interp, "احذف_مجلد(\"أ.txt\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "احذف_مجلد(\"مجلد\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!dir.join("مجلد").exists());
        // احذف_ملف is file-only: a dir is an error.
        std::fs::create_dir(dir.join("مجلد٢")).unwrap();
        let (_, res) = run_in(&mut interp, "احذف_ملف(\"مجلد٢\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "احذف_ملف(\"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!dir.join("أ.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_wrong_args_error() {
        let mut interp = Interpreter::new();
        let (_, res) = run_in(&mut interp, "اعرض_الملفات(١)");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "انتقل(٥)");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "انتقل(\"غيرموجود\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "انسخ(\"أ\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "احذف(٥)");
        assert!(res.is_err());
    }

    // New file builtins for the 50 shortcuts (owner's order
    // 2026-11-05 ~22:11): touch / head / tail / wc / find /
    // tree / du / test / file-type / count / date / version.
    // Only 0- and 1-argument calls are tested here: the
    // two-argument ones (ابحث_في_ملف، أنشئ_رابط، عب/عن مع
    // عدد) are covered by the REPL shortcut tests, which call
    // them space-separated with no comma, per the owner's
    // no-comma rule.
    #[test]
    fn shortcuts_new_file_builtins() {
        let dir = shortcuts_dir();
        std::fs::write(dir.join("ملف.txt"), "ألف\nباء\nجيم\nدال\nهاء\nواو").unwrap();
        std::fs::create_dir(dir.join("فرعي")).unwrap();
        std::fs::write(dir.join("فرعي/داخلي.txt"), "داخل").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // أنشئ_ملف (touch): creates an empty file, and is a
        // no-op when the file already exists.
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"جديد.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(dir.join("جديد.txt")).unwrap(), "");
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(dir.join("ملف.txt")).unwrap(), "ألف\nباء\nجيم\nدال\nهاء\nواو");
        // اعرض_البداية / اعرض_النهاية default to 5 lines.
        let (out, res) = run_in(&mut interp, "اطبع اعرض_البداية(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "ألف\nباء\nجيم\nدال\nهاء\n");
        let (out, res) = run_in(&mut interp, "اطبع اعرض_النهاية(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "باء\nجيم\nدال\nهاء\nواو\n");
        // عد_الأسطر / عد_الكلمات.
        let (out, res) = run_in(&mut interp, "اطبع عد_الأسطر(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٦\n");
        let (out, res) = run_in(&mut interp, "اطبع عد_الكلمات(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٦\n");
        // ابحث_عن (find by name substring, recursive): the
        // name "داخلي.txt" does not contain "ملف", so only the
        // top-level file matches that query.
        let (out, res) = run_in(&mut interp, "اطبع ابحث_عن(\"ملف\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("ملف.txt"), "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع ابحث_عن(\"داخلي\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("فرعي/داخلي.txt"), "got: {}", out);
        // اعرض_الشجرة (tree, indented).
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الشجرة()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("فرعي/"), "got: {}", out);
        assert!(out.contains("  داخلي.txt"), "got: {}", out);
        // حجم_المسار (du): bytes, equals the real file size.
        let (out, res) = run_in(&mut interp, "اطبع حجم_المسار(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        let expected = std::fs::metadata(dir.join("ملف.txt")).unwrap().len();
        assert_eq!(out.trim(), crate::value::Value::Number(expected as f64).fmt_value());
        // موجود / نوع_المسار.
        let (out, res) = run_in(&mut interp, "اطبع موجود(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "صحيح\n");
        let (out, res) = run_in(&mut interp, "اطبع موجود(\"غائب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "خطأ\n");
        let (out, res) = run_in(&mut interp, "اطبع نوع_المسار(\"فرعي\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "مجلد\n");
        let (out, res) = run_in(&mut interp, "اطبع نوع_المسار(\"غائب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "غير موجود\n");
        // عد_الملفات (count entries): ملف.txt + فرعي + جديد.txt.
        let (out, res) = run_in(&mut interp, "اطبع عد_الملفات()");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٣\n");
        // التاريخ_الآن: "YYYY-MM-DD HH:MM:SS" (19 chars).
        let (out, res) = run_in(&mut interp, "اطبع التاريخ_الآن()");
        assert!(res.is_ok(), "{:?}", res);
        let date = out.trim_end().to_string();
        assert_eq!(date.len(), 19, "got: {}", date);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        assert_eq!(&date[10..11], " ");
        assert_eq!(&date[13..14], ":");
        assert_eq!(&date[16..17], ":");
        // النسخة: the banner, name + version only.
        let (out, res) = run_in(&mut interp, "اطبع النسخة()");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "محرك نيفا ١٫٠٫٠\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-13: reading a file larger than 5 MB is refused
    // with a clean Arabic error — never an OOM.
    #[test]
    fn read_file_capped_refuses_oversize() {
        let dir = shortcuts_dir();
        let big = vec![b'x'; Interpreter::MAX_READ_BYTES + 1];
        std::fs::write(dir.join("ضخم.txt"), &big).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "اطبع اقرأ_ملف(\"ضخم.txt\")");
        assert!(res.is_err(), "oversize read must fail");
        let err = res.unwrap_err();
        assert!(
            err.to_string().contains("أكبر من الحد الأقصى"),
            "got: {}",
            err
        );
        // Just under the cap still reads (one line, no newline).
        let ok = vec![b'x'; Interpreter::MAX_READ_BYTES];
        std::fs::write(dir.join("تمام.txt"), &ok).unwrap();
        let (out, res) = run_in(&mut interp, "اطبع عد_الأسطر(\"تمام.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-14: اعرض_الملفات caps at 1000 names and reports
    // how many more exist — a huge directory cannot build a
    // giant list.
    #[test]
    fn list_files_caps_at_1000_with_marker() {
        let dir = shortcuts_dir();
        for i in 0..1005 {
            std::fs::write(dir.join(format!("ملف-{:04}.txt", i)), "").unwrap();
        }
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("…و ٥ أخرى"), "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-20 (scoped): the disk quota refuses a write
    // once the program dir is full. A sparse file stands in
    // for 100 MB of content (metadata length counts, no real
    // disk is used).
    #[test]
    fn disk_quota_refuses_write_when_full() {
        let dir = shortcuts_dir();
        let f = std::fs::File::create(dir.join("كبير.bin")).unwrap();
        f.set_len(Interpreter::DISK_QUOTA_BYTES + 1).unwrap();
        drop(f);
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "اطبع اكتب_ملف(\"جديد.txt\"، \"نص\")");
        assert!(res.is_err(), "write past the quota must fail");
        let err = res.unwrap_err();
        assert!(err.to_string().contains("حصة التخزين"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
