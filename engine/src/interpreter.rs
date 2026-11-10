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
use crate::navigation::TryDir;
use crate::parser::Parser;
use crate::value::{ClassData, InstanceData, InterfaceData, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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
    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): `خروج(رمز)`
    /// ينهي البرنامج كله بالرمز — يتجاوز `ماعدا` (ليست
    /// خطأ) ويشغّل `أخيرا` في طريقه للخارج.
    Exit(u8),
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
    // اجتماع 56 §71 — الدفعة 2 (التنقل): مكدس
    // المجلدات (الأول = الأحدث المدفوع)، وقائمة بحث
    // `CDPATH` (فارغة افتراضيا = بلا قيمة افتراضية)،
    // وراية الوضع الفيزيائي العام للجلسة (الافتراضي
    // منطقي).
    dir_stack: Vec<PathBuf>,
    cdpath: Vec<String>,
    physical_mode: bool,
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
    // الدفعة 1 — رموز الخروج (اجتماع 54 §69): خروج
    // معلق طلبه `خروج(رمز)` (يُستهلك عند أول حد جملة
    // فيتحول إلى Flow::Exit)؛ ورمز آخر أمر مكتمل (0
    // أولا؛ و1 بعد خطأ ممسوك بـ`ماعدا`) تقرأه
    // `رمز_الخروج()`.
    exit_code: Option<u8>,
    last_exit_code: u8,
    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): رموز مراحل آخر أنبوب (أو رمز
    // السطر وحده بلا أنبوب) تقرأها `رموز_الأنبوب()`.
    pipe_status: Vec<u8>,
    // الدفعة 4 — المتغيرات (اجتماع 58 §73):
    // أسماء المتغيرات المجمدة نهائيا (بلا فك)،
    // وخريطة المراجع الاسمية (اسم ← هدف)، وبدء
    // الجلسة (لـ`بيئة("ثواني")`).
    frozen: HashSet<String>,
    namerefs: HashMap<String, String>,
    started: crate::time::SessionClock,
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
            // اجتماع 45 §59: الجلسة تبدأ في البيت
            // (`program_dir/home`)، لا في مجلد البرنامج.
            cwd: program_dir.join("home"),
            loaded_modules: HashMap::new(),
            loading_stack: Vec::new(),
            file_policy: crate::permissions::FilePolicy::restricted(program_dir),
            network_allowed: false,
            server_allowed: false,
            source_file: None,
            live_output: false,
            module_files: HashMap::new(),
            exit_code: None,
            last_exit_code: 0,
            pipe_status: Vec::new(),
            dir_stack: Vec::new(),
            cdpath: Vec::new(),
            physical_mode: false,
            frozen: HashSet::new(),
            namerefs: HashMap::new(),
            started: crate::time::SessionClock::now(),
        }
    }

    pub fn with_program_dir(program_dir: PathBuf) -> Self {
        let mut interp = Self::new();
        interp.program_dir = program_dir.clone();
        // اجتماع 45 §59: الجلسة تبدأ في البيت.
        interp.cwd = program_dir.join("home");
        interp.file_policy = crate::permissions::FilePolicy::restricted(program_dir);
        interp
    }

    /// Resolve a user path against this session's current
    /// directory (انتقل), under the file policy.
    /// اجتماع 56 §71: توسعة `~+` و`~+N` أولا — فهي في
    /// كل مسارات الجلسة، كما توسعة التلدة في باش.
    fn resolve_cwd(&self, path: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        match self.expand_tilde_plus(path, line, col)? {
            Some(expanded) => self.file_policy.resolve_in(&self.cwd, &expanded, line, col),
            None => self.file_policy.resolve_in(&self.cwd, path, line, col),
        }
    }

    /// Like `resolve_cwd`, but a write is refused under the
    /// downloads root (Meeting 50 §64, items 94/96).
    fn resolve_cwd_for_write(&self, path: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        match self.expand_tilde_plus(path, line, col)? {
            Some(expanded) => self
                .file_policy
                .resolve_in_for_write(&self.cwd, &expanded, line, col),
            None => self.file_policy.resolve_in_for_write(&self.cwd, path, line, col),
        }
    }

    /// ضبط المجلد الحالي بعد نجاح انتقال، مع مزامنة
    /// سياسة العمليات حالا (مهمة 10: لا يبقى `أنشئ`
    /// على المجلد القديم بقية هذا التقييم).
    fn set_cwd(&mut self, new: PathBuf) {
        self.cwd = new;
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.file_policy.trusted,
            self.cwd.clone(),
        );
    }

    /// عرض مسار منشأ لإعلان `mkdir -v` (الميزة 211،
    /// الاجتماع 59 §74): نسبي إلى المجلد الحالي إن
    /// كان تحته، وإلا بصيغة العرض القائمة (`~` و
    /// `$نيفا`).
    fn display_created(&self, path: &std::path::Path) -> String {
        if let Ok(rel) = path.strip_prefix(&self.cwd) {
            if !rel.as_os_str().is_empty() {
                return crate::permissions::slash_display(rel);
            }
        }
        self.file_policy
            .display_str(&path.to_string_lossy())
    }

    /// بعد حركة مكدس (تبديل/تدوير/إخراج): المجلد الجديد
    /// من المكدس يُحلّ فيزيائيا إن كان الوضع العام
    /// فيزيائيا، ثم تُزامَن سياسة العمليات.
    fn after_stack_move(&mut self) {
        if self.physical_mode {
            if let Some(canon) = crate::navigation::canonical_dir(&self.cwd) {
                self.cwd = canon;
            }
        }
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.file_policy.trusted,
            self.cwd.clone(),
        );
    }

    /// المسار الفيزيائي للمجلد الحالي (حل الروابط)،
    /// ويسقط إلى المنطقي عند الفشل.
    fn physical_cwd(&self) -> PathBuf {
        crate::navigation::canonical_dir(&self.cwd).unwrap_or_else(|| self.cwd.clone())
    }

    /// عرض المجلد الحالي حسب الوضع: علم صريح، وإلا
    /// الوضع العام للجلسة (فيزيائي إن كان مفعّلا).
    fn display_current(&self, mode: Option<crate::navigation::NavMode>) -> String {
        let physical = match mode {
            Some(m) => m.is_physical(),
            None => self.physical_mode,
        };
        if physical {
            self.file_policy.display_path(&self.physical_cwd())
        } else {
            self.file_policy.display_path(&self.cwd)
        }
    }

    /// توسعة `~+` و`~+N` (اجتماع 56 §71، السؤال
    /// الثالث): `~+` و`~+0` = الحالي، و`~+N` (N ≥ 1)
    /// = عنصر المكدس N من الأحدث. ترجع المسار المطلق
    /// للنظام + الباقي بعد البادئة، أو `None` إن لم
    /// تكن الصيغة `~+`. الفهرس الخارج خطأ عربي.
    /// `~-` ليست هنا: محذوفة بأمر المالك فتبقى حرفية.
    fn expand_tilde_plus(&self, path: &str, line: usize, col: usize) -> RioResult<Option<String>> {
        let (idx, tail) = match crate::navigation::parse_tilde_plus(path) {
            Some(v) => v,
            None => return Ok(None),
        };
        let base: PathBuf = if idx == 0 {
            self.cwd.clone()
        } else {
            match crate::navigation::resolve_stack_index(idx - 1, self.dir_stack.len()) {
                Some(pos) => self.dir_stack[pos].clone(),
                None => {
                    return Err(RioError::runtime(
                        format!(
                            "عنصر المكدس غير موجود: {}",
                            Value::Number(idx as f64).fmt_value()
                        ),
                        line,
                        col,
                    ))
                }
            }
        };
        let mut out = base.to_string_lossy().to_string();
        out.push_str(tail);
        Ok(Some(out))
    }

    /// محاولة واحدة على مسار محلول: منطقيا (بيانات
    /// وصفية + تطبيع معجمي) أو فيزيائيا
    /// (`canonicalize`: الروابط ثم `..` على الحقيقي).
    fn try_resolved(&self, resolved: PathBuf, mode: crate::navigation::NavMode) -> TryDir {
        if mode.is_physical() {
            return match crate::navigation::canonical_dir(&resolved) {
                Some(canon) => TryDir::Ok(canon),
                None => {
                    if resolved.exists() {
                        TryDir::NotDir
                    } else {
                        TryDir::NotFound
                    }
                }
            };
        }
        match std::fs::metadata(&resolved) {
            Ok(m) if m.is_dir() => TryDir::Ok(Self::normalize_lexical(&resolved)),
            Ok(_) => TryDir::NotDir,
            Err(_) => TryDir::NotFound,
        }
    }

    /// محاولة مرشح نصي واحد: حلّه عبر السياسة ثم
    /// `try_resolved`. خطأ السياسة (خروج من النطاق)
    /// يُحفَظ نوعا مستقلا ليُعرض لا ليُبتلع. وبوابة
    /// الوجود (إصلاح انهيار `..` المعجمي): يُرفض
    /// المرشح إن غاب مساره الموصول كما كُتب قبل
    /// أي تطبيع — فـ`مشاريع/..` بلا `مشاريع` موجودة
    /// ليست الحالي نفسه.
    fn try_candidate(&self, raw: &str, mode: crate::navigation::NavMode, line: usize, col: usize) -> TryDir {
        match self.resolve_cwd(raw, line, col) {
            Ok(resolved) => {
                let gate_raw = match self.expand_tilde_plus(raw, line, col) {
                    Ok(Some(e)) => e,
                    Ok(None) => raw.to_string(),
                    Err(e) => return TryDir::Policy(e),
                };
                match self.file_policy.raw_joined_state(&self.cwd, &gate_raw) {
                    Some(true) => self.try_resolved(resolved, mode),
                    Some(false) => TryDir::NotDir,
                    None => TryDir::NotFound,
                }
            }
            Err(e) => TryDir::Policy(e),
        }
    }

    /// تصحيح إملاء المكون الأخير (الميزة 17): خطأ
    /// داميراو واحد، ومرشح وحيد فقط (الغموض = لا
    /// تصحيح). يرجع المسار المصحح بعد فحص الوضع،
    /// أو `None`.
    fn spell_correct(&self, raw: &str, mode: crate::navigation::NavMode, line: usize, col: usize) -> Option<PathBuf> {
        if raw.is_empty() || raw.ends_with('/') {
            return None;
        }
        let (parent, last) = match raw.rfind('/') {
            Some(i) => (&raw[..i], &raw[i + 1..]),
            None => ("", raw),
        };
        if last.is_empty() || last == "." || last == ".." {
            return None;
        }
        let parent_path = if parent.is_empty() {
            self.cwd.clone()
        } else {
            match self.resolve_cwd(parent, line, col) {
                Ok(p) => p,
                Err(_) => return None,
            }
        };
        let entries = std::fs::read_dir(&parent_path).ok()?;
        let mut found: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let child = parent_path.join(&name);
            if !child.is_dir() {
                continue;
            }
            if crate::navigation::edit_distance_one(last, &name) {
                if found.is_some() {
                    return None;
                }
                found = Some(child);
            }
        }
        let corrected = found?;
        match self.try_resolved(corrected, mode) {
            TryDir::Ok(p) => Some(p),
            _ => None,
        }
    }

    /// سلسلة محاولات `انتقل` كاملة (اجتماع 56 §71،
    /// السؤال السادس): مطلق، ثم عناصر `CDPATH`
    /// بالترتيب، ثم المجلد الحالي، ثم المحاولة
    /// الحرفية، ثم قيمة المتغير، ثم تصحيح الإملاء.
    /// النجاح يضبط `cwd`؛ والفشل يرجع خطأ السياسة
    /// للمسار المباشر أولا، ثم «ليس مجلدا»، ثم
    /// «المجلد غير موجود» — برسائل اليوم نفسها.
    fn change_dir(&mut self, raw: &str, flag: Option<crate::navigation::NavMode>, line: usize, col: usize) -> RioResult<()> {
        use crate::navigation::NavMode;
        let mode = match flag {
            Some(m) => m,
            None => {
                if self.physical_mode {
                    NavMode::Physical
                } else {
                    NavMode::Logical
                }
            }
        };
        let is_absolute = raw == "~"
            || raw.starts_with("~/")
            || raw == "$نيفا"
            || raw.starts_with("$نيفا/")
            || raw.starts_with('/');
        let mut saw_not_dir = false;
        let mut direct_policy: Option<RioError> = None;
        if is_absolute {
            match self.try_candidate(raw, mode, line, col) {
                TryDir::Ok(p) => {
                    self.set_cwd(p);
                    return Ok(());
                }
                TryDir::NotDir => saw_not_dir = true,
                TryDir::Policy(e) => direct_policy = Some(e),
                TryDir::NotFound => {}
            }
        } else {
            // POSIX (مثبت بفحوص باش 5.2 الحية): لا
            // `CDPATH` حين يكون المكون الأول `.` أو
            // `..` (`.` و`..` و`./...` و`../...`) —
            // فـ`..` تذهب إلى الأب دائما ولو وُجد عنصر
            // `CDPATH` يجعل «عنصر/..» ينهار إلى الحالي.
            if crate::navigation::cdpath_applies(raw) {
                for entry in self.cdpath.clone() {
                    let cand = if entry.is_empty() {
                        raw.to_string()
                    } else {
                        format!("{}/{}", entry, raw)
                    };
                    match self.try_candidate(&cand, mode, line, col) {
                        TryDir::Ok(p) => {
                            self.set_cwd(p);
                            return Ok(());
                        }
                        TryDir::NotDir => saw_not_dir = true,
                        TryDir::Policy(_) => {}
                        TryDir::NotFound => {}
                    }
                }
            }
            match self.try_candidate(raw, mode, line, col) {
                TryDir::Ok(p) => {
                    self.set_cwd(p);
                    return Ok(());
                }
                TryDir::NotDir => saw_not_dir = true,
                TryDir::Policy(e) => direct_policy = Some(e),
                TryDir::NotFound => {}
            }
        }
        // المحاولة الحرفية (الميزة 13): النص كما كُتب
        // بلا تطبيع معجمي، والنجاح من المسار الحقيقي —
        // بشرط أن يكون مجلدا فعلا لا ملفا.
        if let Ok(canon) = self.file_policy.resolve_literal_in(&self.cwd, raw, line, col) {
            if canon.is_dir() {
                self.set_cwd(canon);
                return Ok(());
            }
        }
        // اسم متغير صالح (الميزة 16): قيمته النصية
        // مجلد — مطلقا أو نسبة إلى الحالي فقط.
        if crate::env::valid_name(raw) {
            if let Some(Value::Str(val)) = self.lookup(raw) {
                match self.try_candidate(&val, mode, line, col) {
                    TryDir::Ok(p) => {
                        self.set_cwd(p);
                        return Ok(());
                    }
                    TryDir::NotDir => saw_not_dir = true,
                    TryDir::Policy(_) => {}
                    TryDir::NotFound => {}
                }
            }
        }
        // تصحيح الإملاء (الميزة 17).
        if let Some(corrected) = self.spell_correct(raw, mode, line, col) {
            self.set_cwd(corrected);
            return Ok(());
        }
        if let Some(e) = direct_policy {
            return Err(e);
        }
        if saw_not_dir {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("ليس مجلدا: {}", self.file_policy.display_str(raw)),
                line,
                col,
            ));
        }
        Err(RioError::new(
            ErrorKind::Io,
            format!("المجلد غير موجود: {}", self.file_policy.display_str(raw)),
            line,
            col,
        ))
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

    /// المرحلة (د) — الإكمال (اجتماع 69 §84): حجم
    /// بصيغة بشرية (1024، لاحقة ك/م/ج) — بلا لاحقة
    /// دون الكيلو؛ والأرقام عربية ككل المخرجات،
    /// والكسر برقم عشري واحد (بلا ‎٫٠‎ حين يصح).
    pub(crate) fn human_size(n: u64) -> String {
        const UNITS: [&str; 4] = ["", " ك", " م", " ج"];
        let mut v = n as f64;
        let mut u = 0usize;
        while v >= 1024.0 && u < 3 {
            v /= 1024.0;
            u += 1;
        }
        if u == 0 {
            return Value::Number(n as f64).fmt_value();
        }
        let rounded = (v * 10.0).round() / 10.0;
        format!("{}{}", Value::Number(rounded).fmt_value(), UNITS[u])
    }

    /// المرحلة (د) — الإكمال (اجتماع 69 §84): اسم
    /// المالك من ‎/etc/passwd‎ بالمعرف الرقمي (يونكس
    /// فقط؛ و«لا شيء» حين يغيب الملف أو الاسم).
    #[cfg(unix)]
    pub(crate) fn passwd_name(uid: u32) -> Option<String> {
        let text = std::fs::read_to_string("/etc/passwd").ok()?;
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 {
                if let Ok(u) = fields[2].parse::<u32>() {
                    if u == uid {
                        return Some(fields[0].to_string());
                    }
                }
            }
        }
        None
    }

    /// المرحلة (د) — الإكمال (اجتماع 69 §84): اسم
    /// المجموعة من ‎/etc/group‎ بالمعرف الرقمي (يونكس
    /// فقط؛ و«لا شيء» حين يغيب الملف أو الاسم).
    #[cfg(unix)]
    pub(crate) fn group_name(gid: u32) -> Option<String> {
        let text = std::fs::read_to_string("/etc/group").ok()?;
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 {
                if let Ok(g) = fields[2].parse::<u32>() {
                    if g == gid {
                        return Some(fields[0].to_string());
                    }
                }
            }
        }
        None
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

    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root for this session. The caller
    /// validates that the path exists and is a directory.
    pub fn set_downloads(&mut self, path: std::path::PathBuf) {
        self.file_policy.set_downloads(path);
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

    /// الدفعة 1 — رموز الخروج: رمز الخروج المعلق (يأخذه
    /// المتصل مرة واحدة — main.rs أو REPL — فيصير لا
    /// شيء بعده)، ورمز آخر أمر، وضبطه من REPL بعد كل
    /// سطر (فالجلسة التفاعلية هي «الأمر» الذي يُقاس).
    pub fn take_exit_code(&mut self) -> Option<u8> {
        self.exit_code.take()
    }

    pub fn last_exit_code(&self) -> u8 {
        self.last_exit_code
    }

    pub fn set_last_exit_code(&mut self, code: u8) {
        self.last_exit_code = code;
    }

    /// المرحلة (ج): ضبط رموز مراحل آخر أنبوب (تزامن
    /// REPL بعد كل سطر، كـ`set_last_exit_code`).
    pub fn set_pipe_status(&mut self, codes: Vec<u8>) {
        self.pipe_status = codes;
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
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): حلّ
    // المرجع الاسمي إلى هدفه النهائي (سلسلة بحد
    // 32 — الدورة مستحيلة أصلا لأن `مرجع()` يرفضها،
    // والحد حارس لا مسار عادي).
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): عملية
    // `+` المشتركة بين التعبير والإلحاق — عددان
    // ونصان وقائمتان كما كانت، وقاموسان بالدمج
    // (اليمين يفوز عند تكرار المفتاح).
    pub(crate) fn add_values(l: &Value, r: &Value, line: usize, col: usize) -> RioResult<Value> {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            (Value::List(a), Value::List(b)) => {
                let mut out = a.clone();
                out.extend(b.iter().cloned());
                Ok(Value::List(out))
            }
            (Value::Dict(a), Value::Dict(b)) => {
                let mut out: Vec<(String, Value)> = a.clone();
                for (k, v) in b {
                    if let Some(pair) = out.iter_mut().find(|(kk, _)| kk == k) {
                        pair.1 = v.clone();
                    } else {
                        out.push((k.clone(), v.clone()));
                    }
                }
                Ok(Value::Dict(out))
            }
            _ => Err(RioError::typ(
                format!("لا يمكن جمع {} و {}", l.type_name(), r.type_name()),
                line,
                col,
            )),
        }
    }

    fn resolve_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        for _ in 0..32 {
            match self.namerefs.get(&current) {
                Some(target) => current = target.clone(),
                None => break,
            }
        }
        current
    }

    fn assign_local(
        &mut self,
        name: &str,
        value: Value,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // الكتابة عبر مرجع تصل هدفه، والكتابة في
        // مجمد مرفوضة قبل أي تغيير.
        let resolved = self.resolve_name(name);
        if self.frozen.contains(&resolved) {
            return Err(RioError::runtime(
                format!("متغير مجمد: {}", name),
                line,
                col,
            ));
        }
        if let Some(i) = self.lookup_scope_index(&resolved) {
            self.scopes[i]
                .borrow_mut()
                .insert(resolved, value);
        } else {
            self.define_local(&resolved, value);
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        let resolved = self.resolve_name(name);
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.borrow().get(&resolved) {
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
                // الدفعة 1: خروج المستوى الأعلى ينهي
                // البرنامج بنجاح برمزه — لا خطأ؛ والرمز
                // يُعاد إلى الراية (أخذها مغلّف exec_stmt
                // عند التحويل) فيأخذه المتصل.
                Flow::Exit(code) => {
                    self.exit_code = Some(code);
                    return Ok(());
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

    /// الدفعة 1 — رموز الخروج: بعد كل جملة، إن طلب
    /// `خروج(رمز)` أثناء تقييمها (في تعبيرها أو في دالة
    /// استدعتها) صارت الجملة خروجا بالرمز — فيتوقف
    /// تنفيذ الكتلة حالا. والراية تُؤخذ هنا فتكتمل
    /// `أخيرا` الجارية (فهي جمل لاحقة براية فارغة)
    /// ولا تُقطع في منتصفها.
    fn exec_stmt(&mut self, stmt: &Stmt) -> RioResult<Flow> {
        let flow = self.exec_stmt_inner(stmt)?;
        if let Some(code) = self.exit_code.take() {
            return Ok(Flow::Exit(code));
        }
        Ok(flow)
    }

    fn exec_stmt_inner(&mut self, stmt: &Stmt) -> RioResult<Flow> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation, append } => {
                let v = self.eval_expr(value)?;
                // Phase 1A Step 3 (runtime part): an annotated
                // assignment is checked at runtime too, so gradual
                // cases (value from an untyped variable) and module
                // files (not semantically analyzed) are still caught.
                // `أي` means no check. Static checking for the main
                // program happens earlier, in the analyzer.
                if let Some(ann) = annotation {
                    // البند 8: الفحص بنيوي (قائمة[عدد] تفحص عناصرها).
                    if !v.matches_declared_type(ann) {
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
                        // الدفعة 4 — المتغيرات (اجتماع 58
                        // §73): الإلحاق يجمع القديم مع
                        // الجديد بعملية `+` نفسها؛ والغائب
                        // يُنشأ من الهوية (0 أو "" أو []
                        // أو {}) كما في باش.
                        if *append {
                            let old = match self.lookup(name) {
                                Some(old) => old,
                                None => match &v {
                                    Value::Number(_) => Value::Number(0.0),
                                    Value::Str(_) => Value::Str(String::new()),
                                    Value::List(_) => Value::List(Vec::new()),
                                    Value::Dict(_) => Value::Dict(Vec::new()),
                                    other => {
                                        return Err(RioError::typ(
                                            format!(
                                                "لا يمكن الإلحاق بقيمة {}",
                                                other.type_name()
                                            ),
                                            stmt.line,
                                            stmt.col,
                                        ));
                                    }
                                },
                            };
                            let joined =
                                Self::add_values(&old, &v, stmt.line, stmt.col)?;
                            self.assign_local(name, joined, stmt.line, stmt.col)?;
                        } else {
                            // Step 1 (Meeting 11): lexical assignment.
                            self.assign_local(name, v, stmt.line, stmt.col)?;
                        }
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
                        Flow::Exit(code) => return Ok(Flow::Exit(code)),
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
                        Flow::Exit(code) => return Ok(Flow::Exit(code)),
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Decorated { decorators, def } => {
                // البند 8 — الزخارف: نفّذ التعريف أولا، ثم
                // طبّق الزخارف من الأسفل إلى الأعلى: كل زخرفة
                // تُستدعى بالمعرف الحالي، ونتيجتها تحل محله
                // في النطاق نفسه (على طريقة بايثون).
                self.exec_stmt(def)?;
                let name = match &def.kind {
                    StmtKind::FuncDef { name, .. } | StmtKind::ClassDef { name, .. } => {
                        name.clone()
                    }
                    _ => {
                        return Err(RioError::runtime(
                            "الزخرفة يجب أن تسبق تعريف دالة أو فئة",
                            stmt.line,
                            stmt.col,
                        ));
                    }
                };
                let mut value = self.lookup(&name).ok_or_else(|| {
                    RioError::runtime(
                        format!("تعريف مفقود بعد الزخرفة: {}", name),
                        stmt.line,
                        stmt.col,
                    )
                })?;
                for dec in decorators.iter().rev() {
                    let call = Expr::new(
                        ExprKind::Call {
                            callee: Box::new(dec.clone()),
                            args: vec![Expr::new(
                                ExprKind::Ident(name.clone()),
                                def.line,
                                def.col,
                            )],
                        },
                        dec.line,
                        dec.col,
                    );
                    value = self.eval_expr(&call)?;
                    self.check_not_frozen(&name, stmt.line, stmt.col)?;
                    self.define_local(&name, value.clone());
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
                // الدفعة 4 — المتغيرات (اجتماع 58 §73):
                // إعادة تعريف اسم مجمد مرفوضة.
                self.check_not_frozen(name, stmt.line, stmt.col)?;
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
        // المدير المدمج (أمر المالك 2026-11-10 ~03:07):
        // ملاذ أخير بعد فشل بحث القرص كله — فالعبارة
        // `حدث حزم` تعمل لأي مستخدم بلا تثبيت مسبق.
        // اجتماع 32، السؤال الخامس: ملف الوحدة هو الذي
        // تُسنَد إليه أخطاؤها — العرض بشرطة مائلة كما
        // يراه المستخدم؛ والبادئة «في وحدة X: » تبقى
        // توافقا مع الرسائل القديمة. والمدمج يُسنَد
        // إلى «مدمج/<الوحدة>.نيفا».
        let (source, display) = match found.found {
            Some(p) => {
                let src = Self::read_file_capped(&p, module, line, col)?;
                let disp = crate::permissions::slash_display(&p);
                (src, disp)
            }
            None => match crate::packages::مصدر_مدمج(module) {
                Some(src) => (src.to_string(), format!("مدمج/{}.نيفا", module)),
                None => {
                    let searched = found
                        .searched
                        .iter()
                        .map(|p| crate::permissions::slash_display(p))
                        .collect::<Vec<_>>()
                        .join("؛ ");
                    return Err(RioError::module_not_found(module, &searched, line, col));
                }
            },
        };
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
                Flow::Exit(code) => return Ok(Flow::Exit(code)),
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
                    // الدفعة 1: الخطأ الممسوك رمز خروج 1 —
                    // يُضبط قبل جسم ماعدا، فتقرأه
                    // `رمز_الخروج()` داخله.
                    self.last_exit_code = 1;
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

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): فحص
    // التجميد لتعريف دالة — إعادة تعريف اسم مجمد
    // مرفوضة كإعادة الإسناد.
    fn check_not_frozen(&self, name: &str, line: usize, col: usize) -> RioResult<()> {
        let resolved = self.resolve_name(name);
        if self.frozen.contains(&resolved) {
            return Err(RioError::runtime(
                format!("متغير مجمد: {}", name),
                line,
                col,
            ));
        }
        Ok(())
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
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // الفهرسة عبر مرجع تصل هدفه، والفهرسة في
        // مجمد مرفوضة قبل أي تغيير.
        let name = self.resolve_name(&name);
        if self.frozen.contains(&name) {
            return Err(RioError::runtime(
                format!("متغير مجمد: {}", name),
                line,
                col,
            ));
        }
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
            BinOp::Add => Self::add_values(&l, &r, line, col),
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
                if !arg.matches_declared_type(pt) {
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
                    if !v.matches_declared_type(rt) {
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
            // الدفعة 1: خروج داخل دالة ينهي البرنامج كله
            // — لا قيمة ترجع؛ والراية تُستعاد ليحسمها حد
            // الجملة عند المتصل.
            Flow::Exit(code) => {
                self.exit_code = Some(code);
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
                if !arg.matches_declared_type(pt) {
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
                    if !v.matches_declared_type(rt) {
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
            // الدفعة 1: كـ call_user — خروج داخل طريقة
            // ينهي البرنامج كله، والراية تُستعاد للمتصل.
            Flow::Exit(code) => {
                self.exit_code = Some(code);
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
                    // المرحلة (د) — الإكمال (اجتماع 66
                    // §81): قصّ بالحروف لا بالبايتات —
                    // البداية السالبة من النهاية، والطول
                    // اختياري (بلا طول = حتى النهاية)؛
                    // والخارج عن النطاق يُقصّ بصمت.
                    "اقطع" => {
                        if args.len() < 1 || args.len() > 2 {
                            return Err(RioError::runtime(
                                format!(
                                    "الطريقة اقطع تتوقع 1 أو 2 معاملات لكن أعطيت {}",
                                    args.len()
                                ),
                                line, col,
                            ));
                        }
                        let start_num = match &args[0] {
                            Value::Number(n) => *n,
                            other => {
                                return Err(RioError::typ(
                                    format!(
                                        "طريقة اقطع تتوقع عددا للبداية، وليس {}",
                                        other.type_name()
                                    ),
                                    line, col,
                                ))
                            }
                        };
                        let len_num = match args.get(1) {
                            None => None,
                            Some(Value::Number(n)) => Some(*n),
                            Some(other) => {
                                return Err(RioError::typ(
                                    format!(
                                        "طريقة اقطع تتوقع عددا للطول، وليس {}",
                                        other.type_name()
                                    ),
                                    line, col,
                                ))
                            }
                        };
                        let chars: Vec<char> = s.chars().collect();
                        let total = chars.len();
                        let start = if start_num < 0.0 {
                            total.saturating_sub((-start_num) as usize)
                        } else {
                            (start_num as usize).min(total)
                        };
                        let end = match len_num {
                            None => total,
                            Some(l) if l <= 0.0 => start,
                            Some(l) => start.saturating_add(l as usize).min(total),
                        };
                        Ok(Value::Str(chars[start..end].iter().collect()))
                    }
                    // المرحلة (د) — الإكمال (اجتماع 66
                    // §81): استبدال أول ظهور فقط (خلاف
                    // `استبدل` التي تستبدل الكل).
                    "استبدل_أول" => {
                        arity(2)?;
                        let (old, new) = match (&args[0], &args[1]) {
                            (Value::Str(a), Value::Str(b)) => (a.clone(), b.clone()),
                            _ => {
                                return Err(RioError::typ(
                                    "طريقة استبدل_أول تتوقع نصين: القديم والجديد",
                                    line, col,
                                ))
                            }
                        };
                        Ok(Value::Str(s.replacen(old.as_str(), new.as_str(), 1)))
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
                        self.write_back_list(target, items)?;
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
                        self.write_back_list(target, items)?;
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
    fn write_back_list(
        &mut self,
        target: &Expr,
        items: Vec<Value>,
    ) -> RioResult<()> {
        if let ExprKind::Ident(name) = &target.kind {
            // الدفعة 4 — المتغيرات (اجتماع 58 §73):
            // الكتابة في قائمة مجمدة مرفوضة كالإسناد.
            self.assign_local(name, Value::List(items), target.line, target.col)?;
        }
        Ok(())
    }

    /// Call a builtin by name with pre-built values. Crate-visible
    /// so the REPL shortcut layer (ات، مك، رح...) can invoke the
    /// file builtins directly, without re-parsing a call string
    /// (which would need commas — the shortcuts never use them).
    /// شغّل ملف أداة في جلسة جديدة (الدفعة 3 —
    /// التشغيل، الاجتماع 57 §72، الميزة 51):
    /// مرآة `run_program_file` في التفاعلي — نفس
    /// البحث (الميزة 39) ونفس الرموز (0 نجاح، 1
    /// فشل تشغيل، 2 خطأ صيغة، 127 وحدة غائبة).
    /// الغياب والمجلد وغير المقروء أخطاء تُرجع
    /// (فيمسكها `ماعدا`)؛ والغائب بعلامة الملف
    /// المطلوب فيرجع المتصل 127. أما الأداة
    /// الموجودة فمخرجها ورمزها يُرجعان دائما —
    /// والمتصل يضبط راية الخروج بهما فلا عودة.
    fn exec_tool(
        &mut self,
        file_name: &str,
        line: usize,
        col: usize,
    ) -> RioResult<(String, u8)> {
        use crate::files::ToolLookup;
        let path = match crate::files::find_tool_file(&self.program_dir, file_name) {
            ToolLookup::Found { path, .. } => path,
            ToolLookup::IsDir => {
                return Err(RioError::runtime(
                    format!("{} مجلد وليس ملفا", file_name),
                    line,
                    col,
                ))
            }
            ToolLookup::Unreadable => {
                return Err(RioError::runtime(
                    format!("تعذر قراءة الملف: {}", file_name),
                    line,
                    col,
                ))
            }
            ToolLookup::Missing => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!(
                        "الملف {} غير موجود — بحثت في $نيفا وفي ~",
                        file_name
                    ),
                    line,
                    col,
                )
                .with_module_not_found())
            }
        };
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                return Err(RioError::runtime(
                    format!("تعذر قراءة الملف: {}", file_name),
                    line,
                    col,
                ))
            }
        };
        let mut fail = |e: String| (format!("{}\n", e), 2u8);
        let tokens = match crate::lexer::tokenize(&source) {
            Ok(t) => t,
            Err(e) => return Ok(fail(e.to_string())),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return Ok(fail(e.to_string())),
        };
        let mut analyzer = crate::semantic::SemanticAnalyzer::new();
        if let Err(e) = analyzer.analyze_program(&program) {
            return Ok(fail(e.to_string()));
        }
        let mut sub = Interpreter::with_program_dir(self.program_dir.clone());
        sub.set_trusted(self.file_policy.trusted);
        sub.set_network_allowed(self.network_allowed);
        sub.set_server_allowed(self.server_allowed);
        sub.set_live_output(self.live_output);
        sub.set_source_file(file_name);
        match sub.run(&program) {
            Ok(()) => Ok((
                std::mem::take(&mut sub.output),
                sub.take_exit_code().unwrap_or(0),
            )),
            Err(e) => {
                let mut out = std::mem::take(&mut sub.output);
                out.push_str(&format!("{}\n", e));
                // الدفعة 1 (§69 ممتد): وحدة غائبة
                // 127، وخطأ تشغيل آخر → 1.
                let code = if e.is_module_not_found() { 127 } else { 1 };
                Ok((out, code))
            }
        }
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): قيمة
    // متغير بيئة ديناميكي، أو `None` إن كان الاسم
    // ثابتا (جدول `env.rs`) — يشارك فيه ذراعا
    // `بيئة` و`اعرض_البيئة`. `line` سطر الاستدعاء
    // نفسه (لـ`رقم_السطر`).
    fn env_dynamic(&self, name: &str, line: usize, col: usize) -> Option<RioResult<String>> {
        match name {
            "المجلد_الحالي" => Some(Ok(self.display_current(None))),
            "المستخدم" => Some(crate::env::username().map_err(|m| RioError::runtime(m, line, col))),
            "اسم_المضيف" => Some(crate::env::hostname().map_err(|m| RioError::runtime(m, line, col))),
            "عشوائي" => Some(Ok(Value::Number(crate::env::random_0_32767() as f64).fmt_value())),
            "ثواني" => Some(Ok(Value::Number(self.started.elapsed_secs() as f64).fmt_value())),
            "رقم_السطر" => Some(Ok(Value::Number(line as f64).fmt_value())),
            _ => None,
        }
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): قائمة
    // «الاسم=القيمة» لكل الأسماء المقروءة الأربعة
    // عشر، بالترتيب الثابت في `env::ALL_NAMES`.
    // على منصة لا تدعم `المستخدم`/`اسم_المضيف`
    // تظهر القيمة «غير مدعوم» بدلا من فشل العرض
    // كله (أما `بيئة()` المباشرة فتخطئ صراحة).
    fn env_display_list(&self, line: usize, col: usize) -> RioResult<Vec<Value>> {
        let mut out = Vec::new();
        for name in crate::env::ALL_NAMES {
            let value = match self.env_dynamic(name, line, col) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    if name == "المستخدم" || name == "اسم_المضيف" {
                        let _ = e;
                        "غير مدعوم".to_string()
                    } else {
                        return Err(e);
                    }
                }
                None => crate::env::lookup(name, crate::repl::version())
                    .unwrap_or_else(|| "?".to_string()),
            };
            out.push(Value::Str(format!("{}={}", name, value)));
        }
        Ok(out)
    }

    // المرحلة (د) — الإكمال (اجتماع 66 §81): قراءة
    // بيئة لينة بلا خطأ — ديناميكية أو ثابتة؛
    // والمجهول أو غير المقروء «لا شيء» (لا خطأ).
    fn env_lookup_soft(&self, name: &str, line: usize, col: usize) -> Option<String> {
        match self.env_dynamic(name, line, col) {
            Some(Ok(v)) => Some(v),
            Some(Err(_)) => None,
            None => crate::env::lookup(name, crate::repl::version()),
        }
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): حلّ
    // مسار `مصدر` — المطلق كما هو (بسياسة الملفات)،
    // وإلا في `$نيفا` ثم `~` ثم المجلد الحالي؛
    // واللاحقة `.نيفا` تُلحق حين تغيب. (ملاحظة:
    // `شغل` لا يبحث في المجلد الحالي أصلا — الترتيب
    // هنا قرار الاجتماع الصريح لا ترتيب `شغل`.)
    fn resolve_source_file(
        &self,
        name: &str,
        line: usize,
        col: usize,
    ) -> RioResult<(std::path::PathBuf, String)> {
        let mut file_name = name.to_string();
        if !file_name.ends_with(".نيفا") {
            file_name.push_str(".نيفا");
        }
        let path = std::path::PathBuf::from(&file_name);
        if path.is_absolute() {
            let resolved = self.resolve_cwd(&file_name, line, col)?;
            return Ok((resolved, file_name));
        }
        for cand in [
            self.program_dir.join(&file_name),
            self.program_dir.join("home").join(&file_name),
            self.cwd.join(&file_name),
        ] {
            if std::fs::metadata(&cand).map(|m| m.is_file()).unwrap_or(false) {
                return Ok((cand, file_name));
            }
        }
        Err(RioError::new(
            ErrorKind::Io,
            format!("الملف غير موجود: {}", name),
            line,
            col,
        ))
    }

    /// المرحلة (د) — الإكمال (اجتماعا 66 و67 §81
    /// و§82): أسماء متغيرات الجلسة (النطاق الخارجي
    /// وحده) — بلا دوال ولا وحدات — مرتبة؛ للإكمال.
    pub fn session_variable_names(&self) -> Vec<String> {
        let mut names: Vec<String> = match self.scopes.first() {
            Some(scope) => scope
                .borrow()
                .iter()
                .filter(|(_, v)| {
                    !matches!(
                        v,
                        Value::Function { .. }
                            | Value::NativeFunction { .. }
                            | Value::Module { .. }
                    )
                })
                .map(|(k, _)| k.clone())
                .collect(),
            None => Vec::new(),
        };
        names.sort();
        names
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): أسماء
    /// دوال الجلسة المعرفة (النطاق الخارجي وحده) —
    /// مرتبة؛ للإكمال ولـ`اعرض_الدوال`.
    pub fn session_function_names(&self) -> Vec<String> {
        let mut names: Vec<String> = match self.scopes.first() {
            Some(scope) => scope
                .borrow()
                .iter()
                .filter(|(_, v)| matches!(v, Value::Function { .. }))
                .map(|(k, _)| k.clone())
                .collect(),
            None => Vec::new(),
        };
        names.sort();
        names
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): المجلد
    /// الحالي الحقيقي للجلسة (لا صيغة العرض)؛ للإكمال.
    pub fn cwd_path(&self) -> std::path::PathBuf {
        self.cwd.clone()
    }

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
                | "اقرأ_الإدخال"
                | "وزع"
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
                // الدفعة 3 — التشغيل (اجتماع 57 §72):
                // وصف الأمر ومكان الأداة والاستبدال
                // تقرأ مجلد الأدوات فهي خاملة هنا أيضا.
                | "نوع_الامر"
                | "مكان_الاداة"
                | "استبدل_الجلسة"
                // الدفعة 5 — أدوات الملفات (اجتماع 59
                // §74): دوال ملفات فهي خاملة هنا أيضا.
                | "انسخ_تكراري"
                | "انسخ_محدث"
                | "انسخ_أرشيف"
                | "انسخ_خفيف"
                | "احذف_تكراري"
                | "أنشئ_رابط_صلب"
                | "أنشئ_رابط_نسبي"
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
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
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
                        // اجتماع 45 §59 (47): كتابة ذرية.
                        crate::install::write_atomic(&resolved, content.as_bytes())
                            .map(|_| Value::Nil)
                            .map_err(|e| {
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
                // المرحلة (د) — الإكمال (اجتماع 69 §84):
                // مسار اختياري أولا، وأعلام نصية اختيارية:
                // أظهر_المخفية (بلا . و.. أبدا)، بلا_ترتيب،
                // عكسي، طويل (نوع+حجم+وقت+اسم — بلا مالك
                // ولا سطر total)، بشري (1024)، بالوقت،
                // وقت_الوصول، وقت_التغيير، مجلد_نفسه. أي
                // نص آخر مسار؛ ومساران خطأ.
                let mut show_hidden = false;
                let mut no_sort = false;
                let mut reverse = false;
                let mut long = false;
                let mut human = false;
                let mut time_key = 0u8;
                let mut self_dir = false;
                let mut path_arg: Option<String> = None;
                for arg in &args {
                    match arg {
                        Value::Str(s) => match s.as_str() {
                            "أظهر_المخفية" => show_hidden = true,
                            "بلا_ترتيب" => no_sort = true,
                            "عكسي" => reverse = true,
                            "طويل" => long = true,
                            "بشري" => human = true,
                            "بالوقت" => time_key = 1,
                            "وقت_الوصول" => time_key = 2,
                            "وقت_التغيير" => time_key = 3,
                            "مجلد_نفسه" => self_dir = true,
                            other => {
                                if path_arg.is_some() {
                                    return Err(RioError::runtime(
                                        "اعرض_الملفات تتوقع مسارا واحدا على الأكثر",
                                        line, col,
                                    ));
                                }
                                path_arg = Some(other.to_string());
                            }
                        },
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "اعرض_الملفات تتوقع نصوصا، وليس {}",
                                    other.type_name()
                                ),
                                line, col,
                            ))
                        }
                    }
                }
                let dir = self.resolve_cwd(path_arg.as_deref().unwrap_or("."), line, col)?;
                // المرحلة (د) — الإكمال (اجتماع 69 §84):
                // الغائب خطأ عربي نظيف يسمّي المسار (لا
                // خطأ نظام خام)؛ والملف المفرد يُعرض وحده
                // مثل `ls ملف`.
                let dir_meta = std::fs::metadata(&dir).map_err(|_| {
                    RioError::new(
                        ErrorKind::Io,
                        format!(
                            "المسار غير موجود: {}",
                            path_arg.as_deref().unwrap_or(".")
                        ),
                        line, col,
                    )
                })?;
                // Audit م-14: cap the listing at the first 1000
                // names (sorted) and count the rest instead of
                // building a list of the whole directory.
                const LIST_CAP: usize = 1000;
                // كل مدخل ببياناته (للطويل والترتيب بالوقت).
                struct ListEntry {
                    name: String,
                    is_dir: bool,
                    is_link: bool,
                    is_exec: bool,
                    size: u64,
                    mtime: u64,
                    atime: u64,
                    ctime: u64,
                }
                let meta_times = |m: &std::fs::Metadata| -> (u64, u64, u64) {
                    let mtime = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let atime = m
                        .accessed()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(mtime);
                    #[cfg(unix)]
                    let ctime = {
                        use std::os::unix::fs::MetadataExt;
                        if m.ctime() >= 0 {
                            m.ctime() as u64
                        } else {
                            mtime
                        }
                    };
                    #[cfg(not(unix))]
                    let ctime = mtime;
                    (mtime, atime, ctime)
                };
                let mut entries: Vec<ListEntry> = Vec::new();
                let mut total = 0usize;
                if self_dir || !dir_meta.is_dir() {
                    let m = dir_meta;
                    let (mtime, atime, ctime) = meta_times(&m);
                    let is_link = std::fs::symlink_metadata(&dir)
                        .ok()
                        .map(|sm| sm.file_type().is_symlink())
                        .unwrap_or(false);
                    #[cfg(unix)]
                    let is_exec = {
                        use std::os::unix::fs::PermissionsExt;
                        m.permissions().mode() & 0o111 != 0
                    };
                    #[cfg(not(unix))]
                    let is_exec = false;
                    total = 1;
                    entries.push(ListEntry {
                        name: path_arg.clone().unwrap_or_else(|| ".".to_string()),
                        is_dir: m.is_dir(),
                        is_link,
                        is_exec,
                        size: m.len(),
                        mtime,
                        atime,
                        ctime,
                    });
                } else {
                    let rd = std::fs::read_dir(&dir).map_err(|e| {
                        RioError::new(
                            ErrorKind::Io,
                            format!("تعذر عرض المجلد: {}", e),
                            line, col,
                        )
                    })?;
                    for entry in rd {
                        let entry = entry.map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر عرض المجلد: {}", e),
                                line, col,
                            )
                        })?;
                        let name = entry.file_name().to_string_lossy().to_string();
                        // مثل `ls` الافتراضي: الملفات المخفية
                        // لا تظهر ولا تُحسب في العدد ولا تشغل
                        // مكانا في السقف؛ و`.` و`..` لا يردهما
                        // read_dir أصلا، فلا يظهران أبدا حتى
                        // مع العلم.
                        if !show_hidden && name.starts_with('.') {
                            continue;
                        }
                        total += 1;
                        if entries.len() >= LIST_CAP {
                            continue;
                        }
                        let file_type = entry.file_type().ok();
                        let is_dir = file_type.as_ref().map(|t| t.is_dir()).unwrap_or(false);
                        let is_link =
                            file_type.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
                        // علامات `ls -F`: `/` للمجلد و`@`
                        // للرابط و`*` للتنفيذي (يونكس فقط —
                        // الفحص مكفوف على غير يونكس).
                        #[cfg(unix)]
                        let is_exec = {
                            use std::os::unix::fs::PermissionsExt;
                            entry
                                .metadata()
                                .map(|m| m.permissions().mode() & 0o111 != 0)
                                .unwrap_or(false)
                        };
                        #[cfg(not(unix))]
                        let is_exec = false;
                        let (size, mtime, atime, ctime) = match entry.metadata() {
                            Ok(m) => {
                                let (mt, at, ct) = meta_times(&m);
                                (m.len(), mt, at, ct)
                            }
                            Err(_) => (0, 0, 0, 0),
                        };
                        entries.push(ListEntry {
                            name,
                            is_dir,
                            is_link,
                            is_exec,
                            size,
                            mtime,
                            atime,
                            ctime,
                        });
                    }
                }
                // الترتيب: بالوقت المختار (الأحدث أولا،
                // والاسم فاصلا) أو بالاسم بايتيا؛ وبلا_ترتيب
                // يبقي ترتيب القراءة؛ وعكسي يقلب الناتج كله.
                if !no_sort {
                    if time_key == 0 {
                        entries.sort_by(|a, b| a.name.cmp(&b.name));
                    } else {
                        let key = |e: &ListEntry| match time_key {
                            1 => e.mtime,
                            2 => e.atime,
                            _ => e.ctime,
                        };
                        entries.sort_by(|a, b| key(b).cmp(&key(a)).then(a.name.cmp(&b.name)));
                    }
                }
                if reverse {
                    entries.reverse();
                }
                let mut out: Vec<Value> = Vec::new();
                for e in entries {
                    if long {
                        let kind = if e.is_link {
                            "رابط"
                        } else if e.is_dir {
                            "مجلد"
                        } else {
                            "ملف"
                        };
                        let size = if human {
                            Self::human_size(e.size)
                        } else {
                            Value::Number(e.size as f64).fmt_value()
                        };
                        out.push(Value::Str(format!(
                            "{} {} {} {}",
                            kind,
                            size,
                            Self::format_unix_utc(e.mtime),
                            e.name
                        )));
                    } else if e.is_dir {
                        out.push(Value::Str(format!("{}/", e.name)));
                    } else if e.is_link {
                        out.push(Value::Str(format!("{}@", e.name)));
                    } else if e.is_exec {
                        out.push(Value::Str(format!("{}*", e.name)));
                    } else {
                        out.push(Value::Str(e.name));
                    }
                }
                if total > LIST_CAP {
                    // Arabic digits, like every other number the
                    // terminal shows.
                    let more = Value::Number((total - LIST_CAP) as f64).fmt_value();
                    out.push(Value::Str(format!("…و {} أخرى", more)));
                }
                Ok(Value::List(out))
            }
            "المجلد_الحالي" => {
                // اجتماع 56 §71 (الميزتان 9 و12): 0 أو 1
                // معاملات — بلا معامل حسب الوضع العام
                // للجلسة، و«منطقي»/«فيزيائي» صريحا.
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                let mode = match args.first() {
                    None => None,
                    Some(Value::Str(s)) => match crate::navigation::parse_pwd_flag(s) {
                        Some(m) => Some(m),
                        None => {
                            return Err(RioError::runtime(
                                format!(
                                    "علم المجلد_الحالي غير معروف: {} — العلم الصالح: منطقي أو فيزيائي",
                                    s
                                ),
                                line,
                                col,
                            ))
                        }
                    },
                    Some(other) => {
                        return Err(RioError::typ(
                            format!("المجلد_الحالي يتطلب علما نصيا، وليس {}", other.type_name()),
                            line,
                            col,
                        ))
                    }
                };
                Ok(Value::Str(self.display_current(mode)))
            }
            "بيئة" => {
                arity(1)?;
                // اجتماع 45 §59 (39–41): قراءة فقط، الثمانية
                // فقط — الاسم غير الصالح أو المجهول خطأ
                // يسمّي الاسم.
                match &args[0] {
                    Value::Str(name) => {
                        // اجتماع 56 §71 (الميزة 4): كشف
                        // `PWD` — `بيئة("المجلد_الحالي")`
                        // قراءة ديناميكية بصيغة العرض حسب
                        // الوضع الحالي؛ والدفعة 4 (اجتماع
                        // 58 §73): باقي الديناميكية
                        // (`المستخدم` و`اسم_المضيف` و`عشوائي`
                        // و`ثواني` و`رقم_السطر`) بالمساعد
                        // نفسه — كلها حالة جلسة لا جدول
                        // الثمانية الثابتة في `env.rs`.
                        if let Some(v) = self.env_dynamic(name, line, col) {
                            return v.map(Value::Str);
                        }
                        match crate::env::lookup_checked(name, crate::repl::version()) {
                            Ok(v) => Ok(Value::Str(v)),
                            Err(msg) => Err(RioError::runtime(msg, line, col)),
                        }
                    }
                    other => Err(RioError::runtime(
                        format!("بيئة تتوقع نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // المرحلة (د) — الإكمال (اجتماع 66 §81):
            // بديل البيئة: المجهول أو الفارغ → البديل؛
            // والبديل المشروط: الموجود غير الفارغ →
            // البديل، وإلا نص فارغ.
            "بيئة_أو" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(name), Value::Str(default)) => {
                        match self.env_lookup_soft(name, line, col) {
                            Some(v) if !v.is_empty() => Ok(Value::Str(v)),
                            _ => Ok(Value::Str(default.clone())),
                        }
                    }
                    _ => Err(RioError::typ(
                        "بيئة_أو تتوقع نصين: الاسم والبديل",
                        line,
                        col,
                    )),
                }
            }
            "بيئة_إن_وجد" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(name), Value::Str(alt)) => {
                        match self.env_lookup_soft(name, line, col) {
                            Some(v) if !v.is_empty() => Ok(Value::Str(alt.clone())),
                            _ => Ok(Value::Str(String::new())),
                        }
                    }
                    _ => Err(RioError::typ(
                        "بيئة_إن_وجد تتوقع نصين: الاسم والبديل",
                        line,
                        col,
                    )),
                }
            }
            // المرحلة (د) — الإكمال (اجتماع 66 §81):
            // إزالة البادئة/اللاحقة الحرفية فقط — بلا
            // أنماط؛ والغائبة تترك النص كما هو.
            "أزل_بادئة" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Str(prefix)) => Ok(Value::Str(
                        s.strip_prefix(prefix.as_str())
                            .unwrap_or(s.as_str())
                            .to_string(),
                    )),
                    _ => Err(RioError::typ(
                        "أزل_بادئة تتوقع نصين: النص والبادئة",
                        line,
                        col,
                    )),
                }
            }
            "أزل_لاحقة" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Str(suffix)) => Ok(Value::Str(
                        s.strip_suffix(suffix.as_str())
                            .unwrap_or(s.as_str())
                            .to_string(),
                    )),
                    _ => Err(RioError::typ(
                        "أزل_لاحقة تتوقع نصين: النص واللاحقة",
                        line,
                        col,
                    )),
                }
            }
            // المرحلة (د) — الإكمال (اجتماع 66 §81):
            // فهارس القائمة (0..الطول-1).
            "فهارس" => {
                arity(1)?;
                match &args[0] {
                    Value::List(items) => Ok(Value::List(
                        (0..items.len())
                            .map(|i| Value::Number(i as f64))
                            .collect(),
                    )),
                    other => Err(RioError::typ(
                        format!("فهارس تتوقع قائمة، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // المرحلة (د) — الإكمال (اجتماع 66 §81):
            // متغيرات الجلسة «الاسم=القيمة» (النطاق
            // الخارجي وحده) — مرتبة؛ والمجمد يُعلَّم،
            // والمرجع الاسمي يُعرض هدفه.
            "اعرض_المتغيرات" => {
                arity(0)?;
                let mut names = self.session_variable_names();
                names.sort();
                let mut out = Vec::new();
                for name in names {
                    if let Some(target) = self.namerefs.get(name.as_str()) {
                        out.push(Value::Str(format!(
                            "{}=مرجع إلى {}",
                            name, target
                        )));
                        continue;
                    }
                    let resolved = self.resolve_name(&name);
                    if let Some(v) = self.lookup(&resolved) {
                        let val = match &v {
                            Value::Str(s) => format!("\"{}\"", s),
                            other => other.fmt_value(),
                        };
                        let mut entry = format!("{}={}", name, val);
                        if self.frozen.contains(&resolved) {
                            entry.push_str(" مجمد");
                        }
                        out.push(Value::Str(entry));
                    }
                }
                Ok(Value::List(out))
            }
            // المرحلة (د) — الإكمال (اجتماع 67 §82):
            // أسماء دوال الجلسة مرتبة.
            "اعرض_الدوال" => {
                arity(0)?;
                Ok(Value::List(
                    self.session_function_names()
                        .into_iter()
                        .map(Value::Str)
                        .collect(),
                ))
            }
            // المرحلة (د) — الإكمال (اجتماع 67 §82):
            // حذف دالة معرفة من أقرب ربط؛ والغائبة أو
            // غير الدالة خطأ يسمّي الاسم.
            "احذف_دالة" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        if self.namerefs.contains_key(name.as_str()) {
                            return Err(RioError::runtime(
                                format!("ليست دالة: {}", name),
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_name(name);
                        if self.frozen.contains(&resolved) {
                            return Err(RioError::runtime(
                                format!("دالة مجمدة: {}", name),
                                line,
                                col,
                            ));
                        }
                        let mut found = false;
                        let mut is_function = false;
                        for scope in self.scopes.iter().rev() {
                            if let Some(v) = scope.borrow().get(&resolved) {
                                found = true;
                                is_function = matches!(v, Value::Function { .. });
                                break;
                            }
                        }
                        if !found {
                            return Err(RioError::runtime(
                                format!("دالة غير موجودة: {}", name),
                                line,
                                col,
                            ));
                        }
                        if !is_function {
                            return Err(RioError::runtime(
                                format!("ليست دالة: {}", name),
                                line,
                                col,
                            ));
                        }
                        for scope in self.scopes.iter().rev() {
                            if scope.borrow_mut().remove(&resolved).is_some() {
                                break;
                            }
                        }
                        Ok(Value::Nil)
                    }
                    other => Err(RioError::typ(
                        format!("احذف_دالة تتوقع نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // المرحلة (د) — الإكمال (اجتماع 68 §83):
            // تنفيذ ملف .نيفا في الجلسة الحالية نفسها —
            // فتبقى تعريفاته (دالة أو متغير أو مرجع) بعد
            // عودته. تحليل نحوي فقط بلا تحليل دلالي
            // مسبق (فالملف يشارك متغيرات الجلسة)؛
            // وأخطاء الأسماء تظهر وقت التشغيل. دورة
            // الاستدعاء (ملف يستدعي نفسه) خطأ صريح.
            "مصدر" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let (resolved, display) =
                            self.resolve_source_file(path, line, col)?;
                        if self.loading_stack.contains(&display) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                format!("الملف يُستدعى دوريا: {}", path),
                                line,
                                col,
                            ));
                        }
                        let source = Self::read_file_capped(&resolved, &display, line, col)?;
                        let tokens = crate::lexer::tokenize_named(&source, &display)?;
                        let program =
                            crate::parser::Parser::named(tokens, &display).parse_program()?;
                        self.loading_stack.push(display.clone());
                        let old_file = self.source_file.clone();
                        self.source_file = Some(display.clone());
                        let result = self.run(&program);
                        self.source_file = old_file;
                        self.loading_stack.pop();
                        result?;
                        Ok(Value::Nil)
                    }
                    other => Err(RioError::typ(
                        format!("مصدر يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // الدفعة 4 — المتغيرات (اجتماع 58 §73):
            // التجميد والحذف والفحص والمرجع وعرض
            // البيئة ومفاتيح القاموس.
            "جمد" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        // المجمد يلزم أن يكون مربوطا
                        // أصلا؛ والمرجع الاسمي يُجمَّد
                        // هدفه لا اسمه.
                        let resolved = self.resolve_name(name);
                        if self.lookup(&resolved).is_none() {
                            return Err(RioError::runtime(
                                format!("متغير غير معرف: {}", name),
                                line,
                                col,
                            ));
                        }
                        self.frozen.insert(resolved);
                        Ok(Value::Nil)
                    }
                    other => Err(RioError::typ(
                        format!("جمد يتطلب نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "احذف_متغير" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        // المجمد لا يُحذف أبدا (كباش)؛
                        // والمرجع الاسمي يُحذف باسمه لا
                        // هدفه؛ والغائب نجاح صامت.
                        if self.namerefs.contains_key(name.as_str()) {
                            self.namerefs.remove(name.as_str());
                            return Ok(Value::Nil);
                        }
                        let resolved = self.resolve_name(name);
                        if self.frozen.contains(&resolved) {
                            return Err(RioError::runtime(
                                format!("متغير مجمد: {}", name),
                                line,
                                col,
                            ));
                        }
                        for scope in self.scopes.iter().rev() {
                            if scope.borrow_mut().remove(&resolved).is_some() {
                                break;
                            }
                        }
                        Ok(Value::Nil)
                    }
                    other => Err(RioError::typ(
                        format!(
                            "احذف_متغير يتطلب نصا، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            "افحص_متغير" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        if let Some(target) = self.namerefs.get(name.as_str()) {
                            return Ok(Value::Str(format!(
                                "متغير {}: مرجع إلى {}",
                                name, target
                            )));
                        }
                        let resolved = self.resolve_name(name);
                        match self.lookup(&resolved) {
                            Some(v) => {
                                let val = match &v {
                                    Value::Str(s) => format!("\"{}\"", s),
                                    other => other.fmt_value(),
                                };
                                let mut out = format!(
                                    "متغير {}: {} = {}",
                                    name,
                                    v.type_name(),
                                    val
                                );
                                if self.frozen.contains(&resolved) {
                                    out.push_str(" مجمد");
                                }
                                Ok(Value::Str(out))
                            }
                            None => Err(RioError::runtime(
                                format!("متغير غير معرف: {}", name),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!(
                            "افحص_متغير يتطلب نصا، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            "مرجع" => {
                arity(2)?;
                let (new_name, target) = match (&args[0], &args[1]) {
                    (Value::Str(a), Value::Str(b)) => (a.clone(), b.clone()),
                    (Value::Str(_), other) => {
                        return Err(RioError::typ(
                            format!("مرجع يتطلب نصين، وليس {}", other.type_name()),
                            line,
                            col,
                        ));
                    }
                    (other, _) => {
                        return Err(RioError::typ(
                            format!("مرجع يتطلب نصين، وليس {}", other.type_name()),
                            line,
                            col,
                        ));
                    }
                };
                if !crate::env::valid_name(&new_name) || !crate::env::valid_name(&target) {
                    return Err(RioError::runtime(
                        format!("اسم مرجع غير صالح: {} أو {}", new_name, target),
                        line,
                        col,
                    ));
                }
                if self.lookup(&new_name).is_some() || self.namerefs.contains_key(new_name.as_str()) {
                    return Err(RioError::runtime(
                        format!("الاسم مستخدم: {}", new_name),
                        line,
                        col,
                    ));
                }
                if self.lookup(&target).is_none() {
                    return Err(RioError::runtime(
                        format!("متغير غير معرف: {}", target),
                        line,
                        col,
                    ));
                }
                // الدورة: حلّ سلسلة الهدف — إن عادت
                // إلى الاسم الجديد فهو مرجع دائري.
                let mut current = target.clone();
                for _ in 0..32 {
                    if current == new_name {
                        return Err(RioError::runtime(
                            "مرجع دائري".to_string(),
                            line,
                            col,
                        ));
                    }
                    match self.namerefs.get(&current) {
                        Some(next) => current = next.clone(),
                        None => break,
                    }
                }
                self.namerefs.insert(new_name, target);
                Ok(Value::Nil)
            }
            "اعرض_البيئة" => {
                arity(0)?;
                Ok(Value::List(self.env_display_list(line, col)?))
            }
            "مفاتيح" => {
                arity(1)?;
                match &args[0] {
                    Value::Dict(pairs) => Ok(Value::List(
                        pairs
                            .iter()
                            .map(|(k, _)| Value::Str(k.clone()))
                            .collect(),
                    )),
                    other => Err(RioError::typ(
                        format!("مفاتيح يتطلب قاموسا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            // الدفعة 1 — رموز الخروج (اجتماع 54 §69).
            "رمز_الخروج" => {
                arity(0)?;
                Ok(Value::Number(self.last_exit_code as f64))
            }
            // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
            // و63 §77 و§78): رموز مراحل آخر أنبوب.
            "رموز_الأنبوب" => {
                arity(0)?;
                Ok(Value::List(
                    self.pipe_status
                        .iter()
                        .map(|c| Value::Number(*c as f64))
                        .collect(),
                ))
            }
            // المرحلة (ج): اقرأ كل الإدخال الموجَّه حتى
            // نهايته (نصا واحدا)؛ وبلا توجيه يقرأ من
            // الطرفية حتى EOF كـ`cat`.
            "اقرأ_الإدخال" => {
                arity(0)?;
                let mut text = String::new();
                while let Some(line) = crate::stdin_pump::stdin_line() {
                    text.push_str(&line);
                }
                Ok(Value::Str(text))
            }
            // المرحلة (ج): أمران فارغان لا يفعلان شيئا
            // إلا رمزهما — حاملا الدوائر القصيرة.
            "نجاح" => {
                arity(0)?;
                Ok(Value::Bool(true))
            }
            "فشل" => {
                arity(0)?;
                Err(RioError::runtime("فشل", line, col))
            }
            // المرحلة (ج): `tee` بلفظ عربي — اقرأ كل
            // الإدخال الموجَّه، واكتبه في كل ملف مسمى
            // (مسح افتراضيا، و"ألحق" كمعامل أخير للإلحاق)،
            // واطبعه كما هو إلى المخرج. الكتابة عبر
            // `اكتب_ملف` نفسها (السياسة والحصة والذرية).
            "وزع" => {
                if args.is_empty() {
                    return Err(RioError::runtime(
                        "وزع تتوقع مسارا واحدا على الأقل",
                        line,
                        col,
                    ));
                }
                let mut append = false;
                let mut paths: Vec<String> = Vec::new();
                for (idx, a) in args.iter().enumerate() {
                    match a {
                        Value::Str(s) if idx + 1 == args.len() && s == "ألحق" => {
                            append = true;
                        }
                        Value::Str(s) => paths.push(s.clone()),
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "وزع تتوقع مسارات نصية، وليس {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    }
                }
                if paths.is_empty() {
                    return Err(RioError::runtime(
                        "وزع تتوقع مسارا واحدا على الأقل",
                        line,
                        col,
                    ));
                }
                let mut text = String::new();
                while let Some(l) = crate::stdin_pump::stdin_line() {
                    text.push_str(&l);
                }
                for p in &paths {
                    let content = if append {
                        let old = match self.call_builtin(
                            "موجود",
                            vec![Value::Str(p.clone())],
                            line,
                            col,
                        ) {
                            Ok(Value::Bool(true)) => {
                                match self.call_builtin(
                                    "اقرأ_ملف",
                                    vec![Value::Str(p.clone())],
                                    line,
                                    col,
                                ) {
                                    Ok(Value::Str(s)) => s,
                                    Ok(_) => String::new(),
                                    Err(e) => return Err(e),
                                }
                            }
                            Ok(_) => String::new(),
                            Err(e) => return Err(e),
                        };
                        format!("{}{}", old, text)
                    } else {
                        text.clone()
                    };
                    self.call_builtin(
                        "اكتب_ملف",
                        vec![Value::Str(p.clone()), Value::Str(content)],
                        line,
                        col,
                    )?;
                }
                if self.live_output {
                    use std::io::Write;
                    print!("{}", text);
                    let _ = std::io::stdout().flush();
                } else {
                    self.output.push_str(&text);
                }
                // المرحلة (ج): نص فارغ لا `لاشيء` — فقيمة
                // التعبير العار الفارغة لا تطبع سطرا،
                // فتبقى التمريرة وحدها في المخرج.
                Ok(Value::Str(String::new()))
            }
            "اسم_البرنامج" => {
                arity(0)?;
                // في التفاعلي بلا ملف: الهوية نفسها «نيفا».
                Ok(Value::Str(
                    self.source_file.clone().unwrap_or_else(|| "نيفا".to_string()),
                ))
            }
            "خروج" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                let code = match args.first() {
                    // بلا معامل: رمز آخر أمر، كـ exit في باش.
                    None => self.last_exit_code,
                    Some(v) => match crate::value::exit_code_number(v) {
                        Some(n) => n,
                        None => {
                            return Err(RioError::runtime(
                                "رمز الخروج يجب أن يكون عددا صحيحا بين ٠ و٢٥٥",
                                line,
                                col,
                            ))
                        }
                    },
                };
                // الراية وحدها هنا؛ حد الجملة (exec_stmt)
                // يحوّلها إلى Flow::Exit، فتشغّل `أخيرا`
                // في طريقها ولا يمسكها `ماعدا`.
                self.exit_code = Some(code);
                Ok(Value::Nil)
            }
            // الدفعة 3 — التشغيل (اجتماع 57 §72):
            // وصف الأمر ومكان الأداة والاستبدال.
            "نوع_الامر" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        let desc = crate::command_names::describe_command(
                            name,
                            |n| {
                                matches!(
                                    self.lookup(n),
                                    Some(Value::Function { .. })
                                )
                            },
                            |f| crate::files::find_tool_file(&self.program_dir, f),
                        );
                        Ok(Value::Str(desc.to_string()))
                    }
                    other => Err(RioError::typ(
                        format!("نوع_الامر يتطلب نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "مكان_الاداة" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        if name.contains('/')
                            || name.contains('\\')
                            || name.contains("..")
                        {
                            return Err(RioError::runtime(
                                "مكان_الاداة يقبل اسم ملف بلا مسار",
                                line,
                                col,
                            ));
                        }
                        let file_name = if name.ends_with(".نيفا") {
                            name.clone()
                        } else {
                            format!("{}.نيفا", name)
                        };
                        match crate::files::find_tool_file(&self.program_dir, &file_name) {
                            crate::files::ToolLookup::Found { path, .. } => {
                                Ok(Value::Str(
                                    path.to_string_lossy().into_owned(),
                                ))
                            }
                            // غائب أو مجلد أو غير مقروء:
                            // لا ملف أداة — لاشيء لا خطأ،
                            // فيصح الفحص قبل التشغيل.
                            _ => Ok(Value::Nil),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("مكان_الاداة يتطلب نصا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "استبدل_الجلسة" => {
                arity(1)?;
                let name = match &args[0] {
                    Value::Str(s) => s.clone(),
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "استبدل_الجلسة يتطلب نصا، وليس {}",
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                };
                if name.contains('/') || name.contains('\\') || name.contains("..") {
                    return Err(RioError::runtime(
                        "استبدل_الجلسة يقبل اسم ملف بلا مسار",
                        line,
                        col,
                    ));
                }
                let file_name = if name.ends_with(".نيفا") {
                    name
                } else {
                    format!("{}.نيفا", name)
                };
                // غياب الأداة خطأ يُمسك (127 بعلامة
                // الملف المطلوب الغائب)؛ أما الأداة
                // الموجودة فتُشغَّل ثم تنتهي الجلسة
                // برمزها بلا عودة (الراية كـ`خروج`).
                let (out, code) = self.exec_tool(&file_name, line, col)?;
                self.output.push_str(&out);
                self.exit_code = Some(code);
                Ok(Value::Nil)
            }
            "انتقل" => {
                // اجتماع 56 §71 — الدفعة 2 (التنقل): 0 أو
                // 2 معاملات — بلا معاملات → الأب `..`
                // (أمر المالك المباشر 2026-11-09، يلغي
                // §66 جزئيا)؛ و`-` اسم عادي غير موجود؛
                // والمعامل الثاني علم نصي اختياري
                // («منطقي»/«فيزيائي»/«منطقي_متحقق»/
                // «فيزيائي_متحقق»)؛ والصامت في كل الصور
                // (يرجع لاشيء دائما — والمجلد الجديد في
                // الموجه).
                if args.len() > 2 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 2 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                let dir = match args.first() {
                    None => "..",
                    Some(Value::Str(s)) => s.as_str(),
                    Some(other) => {
                        return Err(RioError::typ(
                            format!("انتقل يتطلب مسارا نصيا، وليس {}", other.type_name()),
                            line,
                            col,
                        ))
                    }
                };
                let flag = match args.get(1) {
                    None => None,
                    Some(Value::Str(s)) => match crate::navigation::parse_mode_flag(s) {
                        Some(m) => Some(m),
                        None => {
                            return Err(RioError::runtime(
                                format!(
                                    "علم انتقل غير معروف: {} — العلم الصالح: منطقي أو فيزيائي أو منطقي_متحقق أو فيزيائي_متحقق",
                                    s
                                ),
                                line,
                                col,
                            ))
                        }
                    },
                    Some(other) => {
                        return Err(RioError::typ(
                            format!("علم انتقل يتطلب نصا، وليس {}", other.type_name()),
                            line,
                            col,
                        ))
                    }
                };
                self.change_dir(dir, flag, line, col)?;
                Ok(Value::Nil)
            }
            // اجتماع 56 §71 — الدفعة 2 (التنقل): مكدس
            // المجلدات (الميزات 19–22 و26). الدفع
            // والإخراج يمران عبر `انتقل` نفسها فيرثان
            // كل قواعدها (`CDPATH` والأعلام والتصحيح
            // ورسائل الخطأ).
            "ادفع_مجلد" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                match args.first() {
                    // بلا معاملات: تبديل الحالي مع أعلى
                    // عنصر في المكدس (مقابل `pushd` بلا
                    // معاملات).
                    None => {
                        if self.dir_stack.is_empty() {
                            return Err(RioError::runtime("المكدس فارغ", line, col));
                        }
                        let top = self.dir_stack[0].clone();
                        self.dir_stack[0] = self.cwd.clone();
                        self.cwd = top;
                        self.after_stack_move();
                        Ok(Value::Nil)
                    }
                    Some(Value::Str(s)) => {
                        // نص رقمي يسمّي عنصرا صالحا في
                        // المكدس = تدوير (صيغة عبارة
                        // REPL التي تمرر نصوصا)؛ وإلا
                        // فهو مسار يُدفع.
                        if let Some(idx) = crate::navigation::parse_arabic_int(s) {
                            if let Some(pos) =
                                crate::navigation::resolve_stack_index(idx, self.dir_stack.len())
                            {
                                crate::navigation::rotate_stack(
                                    &mut self.cwd,
                                    &mut self.dir_stack,
                                    pos,
                                );
                                self.after_stack_move();
                                return Ok(Value::Nil);
                            }
                        }
                        let old = self.cwd.clone();
                        self.change_dir(s, None, line, col)?;
                        self.dir_stack.insert(0, old);
                        Ok(Value::Nil)
                    }
                    Some(v) => match crate::navigation::parse_stack_index(v) {
                        Some(idx) => {
                            let pos = match crate::navigation::resolve_stack_index(
                                idx,
                                self.dir_stack.len(),
                            ) {
                                Some(p) => p,
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "عنصر المكدس غير موجود: {}",
                                            Value::Number(idx as f64).fmt_value()
                                        ),
                                        line,
                                        col,
                                    ))
                                }
                            };
                            crate::navigation::rotate_stack(
                                &mut self.cwd,
                                &mut self.dir_stack,
                                pos,
                            );
                            self.after_stack_move();
                            Ok(Value::Nil)
                        }
                        None => Err(RioError::typ(
                            format!(
                                "ادفع_مجلد يتطلب مسارا نصيا أو رقما، وليس {}",
                                v.type_name()
                            ),
                            line,
                            col,
                        )),
                    },
                }
            }
            "أخرج_مجلد" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                match args.first() {
                    // بلا معاملات: إخراج الأعلى والانتقال
                    // إليه؛ والفشل لا يُفقد العنصر.
                    None => {
                        if self.dir_stack.is_empty() {
                            return Err(RioError::runtime("المكدس فارغ", line, col));
                        }
                        let target = self.dir_stack[0].clone();
                        match std::fs::metadata(&target) {
                            Ok(m) if m.is_dir() => {
                                self.dir_stack.remove(0);
                                self.cwd = Self::normalize_lexical(&target);
                                self.after_stack_move();
                                Ok(Value::Nil)
                            }
                            Ok(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!(
                                    "ليس مجلدا: {}",
                                    self.file_policy.display_path(&target)
                                ),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!(
                                    "المجلد غير موجود: {}",
                                    self.file_policy.display_path(&target)
                                ),
                                line,
                                col,
                            )),
                        }
                    }
                    // برقم: حذف العنصر بلا انتقال.
                    Some(v) => match crate::navigation::parse_stack_index(v) {
                        Some(idx) => {
                            let pos = match crate::navigation::resolve_stack_index(
                                idx,
                                self.dir_stack.len(),
                            ) {
                                Some(p) => p,
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "عنصر المكدس غير موجود: {}",
                                            Value::Number(idx as f64).fmt_value()
                                        ),
                                        line,
                                        col,
                                    ))
                                }
                            };
                            self.dir_stack.remove(pos);
                            Ok(Value::Nil)
                        }
                        None => Err(RioError::typ(
                            format!(
                                "أخرج_مجلد يتطلب رقما، وليس {}",
                                v.type_name()
                            ),
                            line,
                            col,
                        )),
                    },
                }
            }
            "اعرض_المكدس" => {
                arity(0)?;
                // الحالي أولا بصيغة العرض (`~` و`~/...`)،
                // ثم عناصر المكدس من الأحدث إلى الأقدم.
                let mut out = vec![Value::Str(self.file_policy.display_path(&self.cwd))];
                for p in &self.dir_stack {
                    out.push(Value::Str(self.file_policy.display_path(p)));
                }
                Ok(Value::List(out))
            }
            "امسح_المكدس" => {
                arity(0)?;
                self.dir_stack.clear();
                Ok(Value::Nil)
            }
            // اجتماع 56 §71 (الميزتان 8 و179): قائمة
            // بحث `CDPATH` — بلا معاملات ترجع القائمة
            // الحالية، وبقائمة نصوص واحدة تضبطها وترجع
            // الجديدة. الافتراضي قائمة فارغة.
            "مسار_بحث_المجلدات" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                match args.first() {
                    None => Ok(Value::List(
                        self.cdpath.iter().cloned().map(Value::Str).collect(),
                    )),
                    Some(Value::List(items)) => {
                        let mut new: Vec<String> = Vec::new();
                        for it in items {
                            match it {
                                Value::Str(s) => new.push(s.clone()),
                                other => {
                                    return Err(RioError::typ(
                                        format!(
                                            "مسار_بحث_المجلدات يتطلب قائمة نصوص، وليس عنصرا {}",
                                            other.type_name()
                                        ),
                                        line,
                                        col,
                                    ))
                                }
                            }
                        }
                        self.cdpath = new;
                        Ok(Value::List(
                            self.cdpath.iter().cloned().map(Value::Str).collect(),
                        ))
                    }
                    Some(other) => Err(RioError::typ(
                        format!(
                            "مسار_بحث_المجلدات يتطلب قائمة نصوص، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            // اجتماع 56 §71 (الميزة 12): الوضع الفيزيائي
            // العام للجلسة — بلا معاملات يرجع الحالي،
            // وبقيمة منطقية يضبطه ويرجع الجديد.
            // الافتراضي منطقي.
            "الوضع_الفيزيائي" => {
                if args.len() > 1 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة {} تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                            name,
                            args.len()
                        ),
                        line,
                        col,
                    ));
                }
                match args.first() {
                    None => Ok(Value::Bool(self.physical_mode)),
                    Some(Value::Bool(b)) => {
                        self.physical_mode = *b;
                        Ok(Value::Bool(self.physical_mode))
                    }
                    Some(other) => Err(RioError::typ(
                        format!(
                            "الوضع_الفيزيائي يتطلب قيمة منطقية، وليس {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    )),
                }
            }
            "أنشئ_مجلد" => {
                // الاجتماع 59 §74 (الميزة 211): معامل وضع
                // اختياري + إرجاع قائمة ما أُنشئ فعلا.
                if args.is_empty() || args.len() > 2 {
                    return Err(RioError::runtime(
                        "أنشئ_مجلد تتوقع 1 أو 2 معاملات",
                        line,
                        col,
                    ));
                }
                let dir = match &args[0] {
                    Value::Str(d) => d.clone(),
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "أنشئ_مجلد يتطلب مسارا نصيا، وليس {}",
                                other.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                };
                if dir.is_empty() {
                    return Err(RioError::new(
                        ErrorKind::Io,
                        "أنشئ_مجلد يتطلب مسارا غير فارغ",
                        line,
                        col,
                    ));
                }
                let mode = if args.len() == 2 {
                    match &args[1] {
                        Value::Str(m) => match crate::files::parse_octal_mode(m) {
                            Some(v) => Some(v),
                            None => {
                                return Err(RioError::new(
                                    ErrorKind::Io,
                                    "الوضع يجب أن يكون ثمانيا مثل 755",
                                    line,
                                    col,
                                ));
                            }
                        },
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "أنشئ_مجلد يتطلب وضعا نصيا، وليس {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    }
                } else {
                    None
                };
                let resolved = self.resolve_cwd_for_write(&dir, line, col)?;
                // المكونات الناقصة قبل الإنشاء — لإعلان
                // `-v` كقيمة (القائمة الفارغة: موجود أصلا).
                let missing = crate::files::missing_components(&resolved);
                std::fs::create_dir_all(&resolved).map_err(|e| {
                    RioError::new(
                        ErrorKind::Io,
                        format!(
                            "تعذر إنشاء المجلد {}: {}",
                            self.file_policy.display_str(&dir),
                            e
                        ),
                        line,
                        col,
                    )
                })?;
                // الوضع على الأخير وحده، وإن كان منشأ
                // حديثا فقط (الموجود لا يمس — كما في
                // `mkdir -m -p`).
                if let Some(m) = mode {
                    if missing.last().map(|p| p == &resolved).unwrap_or(false) {
                        crate::files::set_mode(&resolved, m).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!(
                                    "تعذر ضبط وضع المجلد {}: {}",
                                    self.file_policy.display_str(&dir),
                                    e
                                ),
                                line,
                                col,
                            )
                        })?;
                    }
                }
                let created: Vec<Value> = missing
                    .iter()
                    .map(|p| Value::Str(self.display_created(p)))
                    .collect();
                Ok(Value::List(created))
            }
            "احذف" => {
                // المرحلة (د) — الإكمال (اجتماع 69 §84):
                // علم ثان اختياري «تجاهل_الغائب» (الغائب
                // نجاح صامت).
                if args.len() < 1 || args.len() > 2 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة احذف تتوقع 1 أو 2 معاملات لكن أعطيت {}",
                            args.len()
                        ),
                        line, col,
                    ));
                }
                let ignore_missing = match args.get(1) {
                    None => false,
                    Some(Value::Str(s)) if s == "تجاهل_الغائب" => true,
                    Some(Value::Str(s)) => {
                        return Err(RioError::runtime(
                            format!(
                                "علم احذف غير معروف: {} — العلم الصالح: تجاهل_الغائب",
                                s
                            ),
                            line, col,
                        ));
                    }
                    Some(other) => {
                        return Err(RioError::typ(
                            format!("احذف تتوقع علما نصيا، وليس {}", other.type_name()),
                            line, col,
                        ));
                    }
                };
                match &args[0] {
                    Value::Str(path) => {
                        // الاجتماع 59 §74 (الميزة 208):
                        // حارس الجذر قبل أي حل ساندبوكس.
                        if let Some(msg) = crate::files::root_refusal_text(path) {
                            return Err(RioError::new(ErrorKind::Io, msg, line, col));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd, path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => {
                                std::fs::remove_dir(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    if crate::files::is_dir_not_empty_error(&e) {
                                        RioError::new(
                                            ErrorKind::Io,
                                            format!(
                                                "المجلد غير فارغ: {} — استخدم احذف_تكراري",
                                                self.file_policy.display_str(path)
                                            ),
                                            line,
                                            col,
                                        )
                                    } else {
                                        RioError::new(
                                            ErrorKind::Io,
                                            format!("تعذر حذف {}: {}", self.file_policy.display_str(path), e),
                                            line,
                                            col,
                                        )
                                    }
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
                            Err(_) => {
                                // المرحلة (د) — الإكمال (اجتماع
                                // 69 §84): «تجاهل_الغائب» — الغائب
                                // نجاح صامت؛ وبلا العلم يبقى الخطأ
                                // الصريح.
                                if ignore_missing {
                                    Ok(Value::Nil)
                                } else {
                                    Err(RioError::new(
                                        ErrorKind::Io,
                                        format!(
                                            "المسار غير موجود: {}",
                                            self.file_policy.display_str(path)
                                        ),
                                        line,
                                        col,
                                    ))
                                }
                            }
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
                        if let Some(msg) = crate::files::root_refusal_text(path) {
                            return Err(RioError::new(ErrorKind::Io, msg, line, col));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd, path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        match std::fs::metadata(&resolved) {
                            Ok(m) if m.is_dir() => {
                                std::fs::remove_dir(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    if crate::files::is_dir_not_empty_error(&e) {
                                        RioError::new(
                                            ErrorKind::Io,
                                            format!(
                                                "المجلد غير فارغ: {} — استخدم احذف_تكراري",
                                                self.file_policy.display_str(path)
                                            ),
                                            line,
                                            col,
                                        )
                                    } else {
                                        RioError::new(
                                            ErrorKind::Io,
                                            format!("تعذر حذف المجلد {}: {}", self.file_policy.display_str(path), e),
                                            line,
                                            col,
                                        )
                                    }
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
                        if let Some(msg) = crate::files::root_refusal_text(path) {
                            return Err(RioError::new(ErrorKind::Io, msg, line, col));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd, path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
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
            "احذف_تكراري" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        if let Some(msg) = crate::files::root_refusal_text(path) {
                            return Err(RioError::new(ErrorKind::Io, msg, line, col));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd, path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        match std::fs::symlink_metadata(&resolved) {
                            Ok(_) => crate::files::remove_tree(&resolved)
                                .map(|_| Value::Nil)
                                .map_err(|e| {
                                    RioError::new(
                                        ErrorKind::Io,
                                        format!(
                                            "تعذر حذف {}: {}",
                                            self.file_policy.display_str(path),
                                            e
                                        ),
                                        line,
                                        col,
                                    )
                                }),
                            Err(_) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.display_str(path)),
                                line,
                                col,
                            )),
                        }
                    }
                    other => Err(RioError::typ(
                        format!("احذف_تكراري يتطلب مسارا نصيا، وليس {}", other.type_name()),
                        line,
                        col,
                    )),
                }
            }
            "انسخ" => {
                // المرحلة (د) — الإكمال (اجتماع 69 §84):
                // علم ثالث اختياري «لا_تكتب» (لا كتابة
                // فوق موجود).
                if args.len() < 2 || args.len() > 3 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة المدمجة انسخ تتوقع 2 أو 3 معاملات لكن أعطيت {}",
                            args.len()
                        ),
                        line, col,
                    ));
                }
                let no_clobber = match args.get(2) {
                    None => false,
                    Some(Value::Str(s)) if s == "لا_تكتب" => true,
                    Some(Value::Str(s)) => {
                        return Err(RioError::runtime(
                            format!(
                                "علم انسخ غير معروف: {} — العلم الصالح: لا_تكتب",
                                s
                            ),
                            line, col,
                        ));
                    }
                    Some(other) => {
                        return Err(RioError::typ(
                            format!("انسخ تتوقع علما نصيا، وليس {}", other.type_name()),
                            line, col,
                        ));
                    }
                };
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        // cp into a directory keeps the file name.
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الاجتماع 59 §74 (الميزة 196): نسخ
                        // مجلد بلا خيار تكراري مرفوض برسالة
                        // صريحة تسمي البديل — لا خطأ إدخال/
                        // إخراج مبهم.
                        if std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                        {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري",
                                line,
                                col,
                            ));
                        }
                        // الميزة 202: نسخ الملف على نفسه كان
                        // يفرّغه بصمت (`std::fs::copy` على
                        // المسار نفسه يرجع `Ok(0)`) — ارفض
                        // قبل أي كتابة، والملف يبقى سليما.
                        if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
                        // المرحلة (د) — الإكمال (اجتماع 69
                        // §84): «لا_تكتب» — الهدف الموجود
                        // يُتخطى بصمت بلا كتابة ولا حصة.
                        if no_clobber && std::fs::metadata(&dst_path).is_ok() {
                            return Ok(Value::Nil);
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
            "انسخ_تكراري" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        // cp into a directory keeps the file name.
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الاجتماع 59 §74 (الميزة 197):
                        // الملف كـ`انسخ`، والمجلد شجرة
                        // عبر `copy_tree` (من 204).
                        let src_is_dir = std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        if src_is_dir {
                            if crate::files::dst_inside_src(&src_path, &dst_path) {
                                return Err(RioError::new(
                                    ErrorKind::Io,
                                    "لا يمكن نسخ المجلد داخل نفسه",
                                    line,
                                    col,
                                ));
                            }
                        } else if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
                        let extra = match std::fs::symlink_metadata(&src_path) {
                            Ok(m) if m.is_dir() => crate::files::tree_size(&src_path),
                            Ok(m) => {
                                let dst_existing =
                                    std::fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
                                m.len().saturating_sub(dst_existing)
                            }
                            Err(_) => 0,
                        };
                        self.check_disk_quota(extra, line, col)?;
                        crate::files::copy_tree(&src_path, &dst_path)
                            .map(|_| Value::Nil)
                            .map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.display_str(src),
                                        e
                                    ),
                                    line,
                                    col,
                                )
                            })
                    }
                    _ => Err(RioError::typ("انسخ_تكراري يتطلب مسارين نصيين", line, col)),
                }
            }
            "انسخ_محدث" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الاجتماع 59 §74 (الميزة 199):
                        // المجلد مصدرا مرفوض برسالة 196 نفسها.
                        if std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                        {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري",
                                line,
                                col,
                            ));
                        }
                        if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
                        // ينسخ فقط حين يكون المصدر أحدث
                        // من الهدف (المساوي يتخطى كما في
                        // `cp -u`)، وينسخ إن غاب الهدف.
                        let src_mtime = std::fs::metadata(&src_path)
                            .and_then(|m| m.modified())
                            .ok();
                        let dst_mtime = std::fs::metadata(&dst_path)
                            .and_then(|m| m.modified())
                            .ok();
                        if let (Some(s), Some(d)) = (src_mtime, dst_mtime) {
                            if s <= d {
                                return Ok(Value::Bool(false));
                            }
                        }
                        let src_len =
                            std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
                        let dst_existing =
                            std::fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
                        let extra = src_len.saturating_sub(dst_existing);
                        self.check_disk_quota(extra, line, col)?;
                        std::fs::copy(&src_path, &dst_path)
                            .map(|_| Value::Bool(true))
                            .map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.display_str(src),
                                        e
                                    ),
                                    line,
                                    col,
                                )
                            })
                    }
                    _ => Err(RioError::typ("انسخ_محدث يتطلب مسارين نصيين", line, col)),
                }
            }
            "انسخ_أرشيف" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الاجتماع 59 §74 (الميزة 200):
                        // تكراري مع حفظ الروابط والأذونات
                        // والأوقات.
                        let src_is_dir = std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        if src_is_dir {
                            if crate::files::dst_inside_src(&src_path, &dst_path) {
                                return Err(RioError::new(
                                    ErrorKind::Io,
                                    "لا يمكن نسخ المجلد داخل نفسه",
                                    line,
                                    col,
                                ));
                            }
                        } else if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
                        let extra = match std::fs::symlink_metadata(&src_path) {
                            Ok(m) if m.is_dir() => crate::files::tree_size(&src_path),
                            Ok(m) => {
                                let dst_existing =
                                    std::fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
                                m.len().saturating_sub(dst_existing)
                            }
                            Err(_) => 0,
                        };
                        self.check_disk_quota(extra, line, col)?;
                        crate::files::copy_archive(&src_path, &dst_path)
                            .map(|_| Value::Nil)
                            .map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.display_str(src),
                                        e
                                    ),
                                    line,
                                    col,
                                )
                            })
                    }
                    _ => Err(RioError::typ("انسخ_أرشيف يتطلب مسارين نصيين", line, col)),
                }
            }
            "انسخ_خفيف" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الاجتماع 59 §74 (الميزة 201):
                        // ملف فقط — المجلد برسالة 196.
                        if std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                        {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري",
                                line,
                                col,
                            ));
                        }
                        if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
                        let src_len =
                            std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
                        let dst_existing =
                            std::fs::metadata(&dst_path).map(|m| m.len()).unwrap_or(0);
                        let extra = src_len.saturating_sub(dst_existing);
                        self.check_disk_quota(extra, line, col)?;
                        crate::files::copy_light(&src_path, &dst_path)
                            .map(|m| Value::Str(m.as_str().to_string()))
                            .map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.display_str(src),
                                        e
                                    ),
                                    line,
                                    col,
                                )
                            })
                    }
                    _ => Err(RioError::typ("انسخ_خفيف يتطلب مسارين نصيين", line, col)),
                }
            }
            "انقل" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(src), Value::Str(dst)) => {
                        let src_path = self.resolve_cwd_for_write(src, line, col)?;
                        let mut dst_path = self.resolve_cwd_for_write(dst, line, col)?;
                        // mv into a directory keeps the file name.
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        // الميزة 204: نقل مجلد داخل نفسه أو
                        // داخل فرع منه مرفوض قبل أي كتابة
                        // (كما في `mv`)؛ والملف على نفسه
                        // يبقى بلا أثر (إعادة تسمية على النفس).
                        if std::fs::metadata(&src_path).map(|m| m.is_dir()).unwrap_or(false)
                            && crate::files::dst_inside_src(&src_path, &dst_path)
                        {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "لا يمكن نقل المجلد داخل نفسه",
                                line,
                                col,
                            ));
                        }
                        match std::fs::rename(&src_path, &dst_path) {
                            Ok(()) => Ok(Value::Nil),
                            Err(e) if crate::files::is_cross_device_error(&e) => {
                                // الميزة 204: عبر الأنظمة — نسخ
                                // ثم حذف المصدر بعد نجاح النسخ.
                                let extra = match std::fs::symlink_metadata(&src_path) {
                                    Ok(m) if m.is_dir() => crate::files::tree_size(&src_path),
                                    Ok(m) => {
                                        let dst_existing = std::fs::metadata(&dst_path)
                                            .map(|m| m.len())
                                            .unwrap_or(0);
                                        m.len().saturating_sub(dst_existing)
                                    }
                                    Err(_) => 0,
                                };
                                // الحصة على نمو مجلد البرنامج
                                // وحده؛ فالهدف خارجه بلا حصة.
                                if dst_path.starts_with(&self.program_dir) {
                                    self.check_disk_quota(extra, line, col)?;
                                }
                                match crate::files::move_fallback(&src_path, &dst_path) {
                                    Ok(()) => Ok(Value::Nil),
                                    Err(crate::files::MoveError::Copy(err)) => Err(RioError::new(
                                        ErrorKind::Io,
                                        format!(
                                            "تعذر نقل {}: {}",
                                            self.file_policy.display_str(src),
                                            err
                                        ),
                                        line,
                                        col,
                                    )),
                                    Err(crate::files::MoveError::Remove(err)) => Err(RioError::new(
                                        ErrorKind::Io,
                                        format!(
                                            "نُسخ {} إلى {} لكن تعذر حذف المصدر: {}",
                                            self.file_policy.display_str(src),
                                            self.file_policy.display_str(dst),
                                            err
                                        ),
                                        line,
                                        col,
                                    )),
                                }
                            }
                            Err(e) => Err(RioError::new(
                                ErrorKind::Io,
                                format!("تعذر نقل {}: {}", self.file_policy.display_str(src), e),
                                line,
                                col,
                            )),
                        }
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
                            // اجتماع 45 §59 (47): كتابة ذرية.
                            crate::install::write_atomic(&resolved, b"").map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!("تعذر إنشاء الملف {}: {}", self.file_policy.display_str(path), e),
                                    line,
                                    col,
                                )
                            })?;
                        } else {
                            // الاجتماع 59 §74 (الميزة 215):
                            // الموجود يحدَّث وقته بلا مساس
                            // بالمحتوى (`touch`)؛ والحل
                            // للكتابة لأن تحديث الوقت كتابة.
                            let resolved_w = self.resolve_cwd_for_write(path, line, col)?;
                            crate::files::touch_times(&resolved_w).map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!("تعذر تحديث وقت {}: {}", self.file_policy.display_str(path), e),
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
            "أنشئ_رابط_صلب" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(target), Value::Str(link)) => {
                        let target_path = self.resolve_cwd(target, line, col)?;
                        let link_path = self.resolve_cwd(link, line, col)?;
                        // الاجتماع 59 §74 (الميزة 212):
                        // الصلب يشترط هدفا موجودا، ويرفض
                        // المجلد صراحة (كما في `ln`).
                        match std::fs::metadata(&target_path) {
                            Ok(m) if m.is_dir() => {
                                return Err(RioError::new(
                                    ErrorKind::Io,
                                    "لا يمكن إنشاء رابط صلب لمجلد",
                                    line,
                                    col,
                                ));
                            }
                            Ok(_) => {}
                            Err(_) => {
                                return Err(RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "الهدف غير موجود: {}",
                                        self.file_policy.display_str(target)
                                    ),
                                    line,
                                    col,
                                ));
                            }
                        }
                        std::fs::hard_link(&target_path, &link_path)
                            .map(|_| Value::Nil)
                            .map_err(|e| {
                                RioError::new(
                                    ErrorKind::Io,
                                    format!(
                                        "تعذر إنشاء الرابط الصلب {}: {}",
                                        self.file_policy.display_str(link),
                                        e
                                    ),
                                    line,
                                    col,
                                )
                            })
                    }
                    _ => Err(RioError::typ("أنشئ_رابط_صلب يتطلب مسارين نصيين", line, col)),
                }
            }
            "أنشئ_رابط_نسبي" => {
                arity(2)?;
                match (&args[0], &args[1]) {
                    (Value::Str(target), Value::Str(link)) => {
                        // الاجتماع 59 §74 (الميزة 213):
                        // النسبي يحسب من موقع الرابط لا من
                        // الجلسة — يحل الطرفين إلى مطلقين
                        // ثم يحسب النسبي بينهما، ويخزن النص
                        // النسبي حرفيا؛ ويقبل هدفا غائبا
                        // كالرمزي.
                        let target_abs = self.resolve_cwd(target, line, col)?;
                        let link_path = self.resolve_cwd(link, line, col)?;
                        let link_dir = link_path
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| self.cwd.clone());
                        // النسبي يُحسب داخل كل فرع منصة
                        // (على WASM بلا استعمال فيحذّر).
                        #[cfg(unix)]
                        let res = std::os::unix::fs::symlink(
                            &crate::files::relative_path(&link_dir, &target_abs),
                            &link_path,
                        );
                        #[cfg(windows)]
                        let res = {
                            let rel = crate::files::relative_path(&link_dir, &target_abs);
                            match std::fs::metadata(&target_abs) {
                            Ok(m) if m.is_dir() => std::os::windows::fs::symlink_dir(
                                &rel,
                                &link_path,
                            ),
                            _ => std::os::windows::fs::symlink_file(
                                &rel,
                                &link_path,
                            ),
                            }
                        };
                        #[cfg(not(any(unix, windows)))]
                        let res: std::io::Result<()> = {
                            // WASM: بلا روابط رمزية — المتغيران
                            // محلولان للتحقق من النوع فقط،
                            // فيُستهلكان هنا صراحة بلا تحذير.
                            let _ = (&target_abs, &link_dir);
                            Err(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "إنشاء الروابط غير مدعوم على هذه المنصة",
                            ))
                        };
                        res.map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر إنشاء الرابط {}: {}", self.file_policy.display_str(link), e),
                                line,
                                col,
                            )
                        })
                    }
                    _ => Err(RioError::typ("أنشئ_رابط_نسبي يتطلب مسارين نصيين", line, col)),
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
                        let meta = std::fs::metadata(&resolved).ok();
                        let smeta = std::fs::symlink_metadata(&resolved).ok();
                        let kind = match &meta {
                            Some(m) if m.is_dir() => "مجلد",
                            Some(_) => "ملف",
                            None => "غير موجود",
                        };
                        let mut budget: usize = 10000;
                        let size = Self::dir_size(&resolved, &mut budget);
                        let modified = meta
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok()
                            })
                            .map(|d| Self::format_unix_utc(d.as_secs()))
                            .unwrap_or_else(|| "—".to_string());
                        // المرحلة (د) — الإكمال (اجتماع 69
                        // §84): أذونات رمزية (d/l/- + تسعة
                        // أحرف rwx — من بيانات الرابط نفسه
                        // لا هدفه)، والمالك والمجموعة
                        // بالاسم من ‎/etc/passwd‎
                        // و‎/etc/group‎ على لينكس/أندرويد
                        // (الرقم حين لا اسم، و«—» على غير
                        // يونكس)، والوصول والتغيير، والعقدة
                        // والروابط (يونكس فقط)، وهدف الرابط
                        // (أو لاشيء حين ليس رابطا).
                        let is_link = smeta
                            .as_ref()
                            .map(|m| m.file_type().is_symlink())
                            .unwrap_or(false);
                        #[cfg(unix)]
                        let perms = {
                            use std::os::unix::fs::PermissionsExt;
                            match smeta.as_ref().or(meta.as_ref()) {
                                Some(m) => {
                                    let mode = m.permissions().mode();
                                    let type_c = if m.is_dir() {
                                        'd'
                                    } else if m.file_type().is_symlink() {
                                        'l'
                                    } else {
                                        '-'
                                    };
                                    let bit = |b: u32, c: char| {
                                        if mode & b != 0 { c } else { '-' }
                                    };
                                    format!(
                                        "{}{}{}{}{}{}{}{}{}{}",
                                        type_c,
                                        bit(0o400, 'r'), bit(0o200, 'w'), bit(0o100, 'x'),
                                        bit(0o040, 'r'), bit(0o020, 'w'), bit(0o010, 'x'),
                                        bit(0o004, 'r'), bit(0o002, 'w'), bit(0o001, 'x'),
                                    )
                                }
                                None => "—".to_string(),
                            }
                        };
                        #[cfg(not(unix))]
                        // الاجتماع 70 §85: رموز مشتقة من
                        // سمة القراءة فقط — بلا `x` أبدا
                        // (لا بت تنفيذ في القياسية على
                        // ويندوز، ولا تخمين من الامتداد).
                        let perms = match smeta.as_ref().or(meta.as_ref()) {
                            Some(m) => {
                                let type_c = if m.is_dir() {
                                    'd'
                                } else if m.file_type().is_symlink() {
                                    'l'
                                } else {
                                    '-'
                                };
                                let triple =
                                    if m.permissions().readonly() { "r--" } else { "rw-" };
                                format!("{}{}{}{}", type_c, triple, triple, triple)
                            }
                            None => "—".to_string(),
                        };
                        #[cfg(unix)]
                        let (owner, group) = {
                            use std::os::unix::fs::MetadataExt;
                            match meta.as_ref() {
                                Some(m) => {
                                    let uid = m.uid();
                                    let gid = m.gid();
                                    let owner = Self::passwd_name(uid)
                                        .unwrap_or_else(|| uid.to_string());
                                    let group = Self::group_name(gid)
                                        .unwrap_or_else(|| gid.to_string());
                                    (owner, group)
                                }
                                None => ("—".to_string(), "—".to_string()),
                            }
                        };
                        #[cfg(not(unix))]
                        // الاجتماع 70 §85: المالك اسم
                        // المستخدم الحالي من `USERNAME`
                        // (تقريب موثق لا مالك ACL)؛
                        // والمجموعة بلا مقابل فتبقى «—».
                        let (owner, group) = (
                            std::env::var("USERNAME")
                                .unwrap_or_else(|_| "—".to_string()),
                            "—".to_string(),
                        );
                        let accessed = meta
                            .as_ref()
                            .and_then(|m| m.accessed().ok())
                            .and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok()
                            })
                            .map(|d| Self::format_unix_utc(d.as_secs()))
                            .unwrap_or_else(|| "—".to_string());
                        #[cfg(unix)]
                        let changed = {
                            use std::os::unix::fs::MetadataExt;
                            match meta.as_ref() {
                                Some(m) if m.ctime() >= 0 => {
                                    Self::format_unix_utc(m.ctime() as u64)
                                }
                                _ => "—".to_string(),
                            }
                        };
                        #[cfg(not(unix))]
                        let changed = "—".to_string();
                        #[cfg(unix)]
                        let (inode, links) = {
                            use std::os::unix::fs::MetadataExt;
                            match meta.as_ref() {
                                Some(m) => (
                                    Value::Number(m.ino() as f64),
                                    Value::Number(m.nlink() as f64),
                                ),
                                None => (
                                    Value::Str("—".to_string()),
                                    Value::Str("—".to_string()),
                                ),
                            }
                        };
                        #[cfg(not(unix))]
                        // الاجتماع 70 §85: الروابط العدد ١
                        // دائما (صحيح للملف العادي؛
                        // والروابط الصلبة النادرة تُحسب ١)؛
                        // والعقدة بلا مقابل فتبقى «—».
                        let (inode, links) = (
                            Value::Str("—".to_string()),
                            Value::Number(1.0),
                        );
                        let target = if is_link {
                            match std::fs::read_link(&resolved) {
                                Ok(p) => Value::Str(p.to_string_lossy().to_string()),
                                Err(_) => Value::Nil,
                            }
                        } else {
                            Value::Nil
                        };
                        Ok(Value::Dict(vec![
                            ("نوع".to_string(), Value::Str(kind.to_string())),
                            ("حجم".to_string(), Value::Number(size as f64)),
                            ("معدل".to_string(), Value::Str(modified)),
                            ("أذونات".to_string(), Value::Str(perms)),
                            ("مالك".to_string(), Value::Str(owner)),
                            ("مجموعة".to_string(), Value::Str(group)),
                            ("وصل".to_string(), Value::Str(accessed)),
                            ("غيّر".to_string(), Value::Str(changed)),
                            ("عقدة".to_string(), inode),
                            ("روابط".to_string(), links),
                            ("هدف".to_string(), target),
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
                    Value::Str(source) => {
                        // باعث سطور التثبيت: يسلك فرع `اطبع`
                        // نفسه (المفسر ~622) — حي مع البث الحي
                        // فيطبع فورا مع تفريغ، وإلا يُلحق
                        // بمخزن المخرجات فيحفظ الترتيب.
                        let live = self.live_output;
                        let output = &mut self.output;
                        let mut emit = move |line: &str| {
                            if live {
                                use std::io::Write;
                                print!("{}\n", line);
                                let _ = std::io::stdout().flush();
                            } else {
                                output.push_str(line);
                                output.push('\n');
                            }
                        };
                        crate::install::install_module(
                            &self.file_policy,
                            &self.cwd,
                            &self.program_dir,
                            source,
                            line,
                            col,
                            &mut emit,
                        )
                    }
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
        // اجتماع 45 §59: الجلسة تبدأ في البيت — أنشئه
        // ليكون مجلد العمل موجودا للاختبارات.
        std::fs::create_dir_all(dir.join("home")).unwrap();
        dir
    }

    #[test]
    fn shortcuts_list_pwd_cd_mkdir() {
        let dir = shortcuts_dir();
        // اجتماع 45 §59: ملفات العمل في البيت (مجلد
        // الجلسة الحالي)، لا في جذر مجلد البرنامج.
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
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
        assert!(home.join("جديد").is_dir());
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
        // اجتماع 45 §59: من البيت `..` تصل إلى مجلد
        // البرنامج ($نيفا) — مسموحة؛ والثانية تتجاوزه
        // فتُرفض.
        let (_, res) = run_in(&mut interp, "انتقل(\"..\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "$نيفا", "got: {}", out);
        let (_, res) = run_in(&mut interp, "انتقل(\"..\")");
        assert!(res.is_err(), "expected escape to be rejected");
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "$نيفا", "got: {}", out);
        let (_, res) = run_in(&mut interp, "انتقل(\"~\")");
        assert!(res.is_ok(), "{:?}", res);
        // Escaping the base is rejected (restricted mode):
        // من البيت يلزم `../..` للخروج.
        let (_, res) = run_in(&mut interp, "احذف(\"../../خارج.txt\")");
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_mkdir_p_creates_parents() {
        // الاجتماع 53 §67 (الميزة 210): `أنشئ_مجلد`
        // بسلوك `mkdir -p` دائما — ينشئ كل الآباء
        // الناقصين، وينجح صامتا على الموجود.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // إنشاء متداخل عميق (3 مستويات) دفعة واحدة.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"أ/ب/ج\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(home.join("أ/ب/ج").is_dir());
        // الموجود أصلا ينجح صامتا — بلا خطأ.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"أ/ب/ج\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"أ\")");
        assert!(res.is_ok(), "{:?}", res);
        // المسار الفارغ خطأ عربي قصير.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"\")");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("مسارا غير فارغ"), "got: {}", msg);
        // ملف في الطريق خطأ — لا يُنشأ مجلد باسمه.
        std::fs::write(home.join("ملف.txt"), "نص").unwrap();
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"ملف.txt/داخل\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"ملف.txt\")");
        assert!(res.is_err());
        // الخروج من الساندبوكس مرفوض — بلا إنشاء خارجي.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"../../خارج\")");
        assert!(res.is_err());
        assert!(!dir.parent().unwrap().join("خارج").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn shortcuts_mkdir_p_symlink_escape_refused() {
        // الاجتماع 53 §67: رابط رمزي هارب في منتصف
        // المسار يُرفض قبل أي إنشاء.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        let outside =
            std::env::temp_dir().join(format!("niva-mkdir-p-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("رابط")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"رابط/أ/ب\")");
        assert!(res.is_err());
        assert!(!outside.join("أ").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    // اجتماع 56 §71 — الدفعة 2 (التنقل): المكدس
    // وCDPATH والمنطقي/الفيزيائي والمتغير والتصحيح
    // وكشف PWD.
    fn nav56_dir() -> std::path::PathBuf {
        let dir = shortcuts_dir();
        let home = dir.join("home");
        for name in ["أ", "ب", "ج", "مشاريع"] {
            std::fs::create_dir_all(home.join(name)).unwrap();
        }
        std::fs::create_dir_all(home.join("مشاريع/هدف")).unwrap();
        std::fs::create_dir_all(home.join("أ/هدف")).unwrap();
        std::fs::write(home.join("ملف.txt"), "نص").unwrap();
        dir
    }

    #[test]
    fn nav56_stack_push_pop_swap_rotate_show_clear() {
        let dir = nav56_dir();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // فارغ أولا: العرض الحالي وحده، والإخراج
        // والتبديل خطأ «المكدس فارغ».
        let (out, res) = run_in(&mut interp, "س = اعرض_المكدس()\nاطبع طول(س)\nاطبع س[٠]");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١\n~\n", "got: {}", out);
        let (_, res) = run_in(&mut interp, "أخرج_مجلد()");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("المكدس فارغ"));
        let (_, res) = run_in(&mut interp, "ادفع_مجلد()");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("المكدس فارغ"));
        // دفع أ ثم ب (بمسارين مطلقين — فالثاني شقيق
        // الأول لا ابن له): الحالي ~/ب، والمكدس [~/أ، ~].
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(\"أ\")\nادفع_مجلد(\"~/ب\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(
            &mut interp,
            "س = اعرض_المكدس()\nاطبع طول(س)\nاطبع س[٠]\nاطبع س[١]\nاطبع س[٢]",
        );
        assert_eq!(out, "٣\n~/ب\n~/أ\n~\n", "got: {}", out);
        // تبديل بلا معاملات: الحالي ~/أ والأعلى ~/ب.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد()");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(
            &mut interp,
            "اطبع المجلد_الحالي()\nس = اعرض_المكدس()\nاطبع س[١]",
        );
        assert_eq!(out, "~/أ\n~/ب\n", "got: {}", out);
        // تدوير إلى العنصر ١ (~): الحالي ~، والقديم
        // ~/أ يدخل مكانه في المكدس.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(١)");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(
            &mut interp,
            "اطبع المجلد_الحالي()\nس = اعرض_المكدس()\nاطبع س[٠]\nاطبع س[١]\nاطبع س[٢]",
        );
        assert_eq!(out, "~\n~\n~/ب\n~/أ\n", "got: {}", out);
        // تدوير سالب (-١ = الأقدم ~/أ): الحالي ~/أ.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(-١)");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/أ", "got: {}", out);
        // إخراج بلا معاملات: الأعلى (~/ب) يصير الحاليا.
        let (_, res) = run_in(&mut interp, "أخرج_مجلد()");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/ب", "got: {}", out);
        // إخراج برقم بلا انتقال: حذف العنصر ٠ (~)
        // والمجلد الحالي يبقى ~/ب.
        let (_, res) = run_in(&mut interp, "أخرج_مجلد(٠)");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(
            &mut interp,
            "اطبع المجلد_الحالي()\nس = اعرض_المكدس()\nاطبع طول(س)",
        );
        assert_eq!(out, "~/ب\n١\n", "got: {}", out);
        // الفهرس الخارج خطأ مسمّى.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(٥)");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("عنصر المكدس غير موجود"));
        let (_, res) = run_in(&mut interp, "أخرج_مجلد(٥)");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("عنصر المكدس غير موجود"));
        // المسح: المكدس فارغ بعده، والدفع الفاشل لا
        // يدفع شيئا.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(\"غائب\")");
        assert!(res.is_err());
        let (out, _) = run_in(
            &mut interp,
            "امسح_المكدس()\nس = اعرض_المكدس()\nاطبع طول(س)",
        );
        assert_eq!(out, "١\n", "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nav56_tilde_plus_forms() {
        let dir = nav56_dir();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(\"أ\")\nادفع_مجلد(\"~/ب\")");
        assert!(res.is_ok(), "{:?}", res);
        // ~+1 = الأعلى (~/أ)؛ و~+2 = التالي (~).
        let (_, res) = run_in(&mut interp, "انتقل(\"~+1\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/أ", "got: {}", out);
        let (_, res) = run_in(&mut interp, "انتقل(\"~+2\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        // ~+ و~+0 = الحالي (بلا أثر).
        let (_, res) = run_in(&mut interp, "انتقل(\"~+\")\nانتقل(\"~+0\")");
        assert!(res.is_ok(), "{:?}", res);
        // الفهرس الخارج خطأ مسمّى؛ و~- حرفي غير موجود.
        let (_, res) = run_in(&mut interp, "انتقل(\"~+9\")");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("عنصر المكدس غير موجود"));
        let (_, res) = run_in(&mut interp, "انتقل(\"~-\")");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("المجلد غير موجود: ~-"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nav56_cdpath_order_empty_entry_and_absolute() {
        let dir = nav56_dir();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // بلا CDPATH: «هدف» غير موجود في الحالي أصلا —
        // مع أن ~/أ/هدف و~/مشاريع/هدف موجودان.
        let (_, res) = run_in(&mut interp, "انتقل(\"هدف\")");
        assert!(res.is_err());
        // الافتراضي قائمة فارغة.
        let (out, res) = run_in(&mut interp, "اطبع طول(مسار_بحث_المجلدات())");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "٠", "got: {}", out);
        // CDPATH = [مشاريع]: يجد ~/مشاريع/هدف.
        let (_, res) = run_in(&mut interp, "مسار_بحث_المجلدات([\"مشاريع\"])");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "انتقل(\"هدف\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/مشاريع/هدف", "got: {}", out);
        // المطلق يغلب CDPATH: ~/أ/هدف مباشرة.
        let (_, res) = run_in(&mut interp, "انتقل(\"~/أ/هدف\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/أ/هدف", "got: {}", out);
        // انحدار (إصلاح انهيار `..` المعجمي): مع
        // CDPATH = [مشاريع]، `..` من ~/مشاريع/هدف
        // تذهب إلى ~/مشاريع — لا تبقى في مكانها
        // (مرشح «مشاريع/..» المنهار معجميا إلى الحالي
        // نفسه مرفوض ببوابة الوجود).
        let (_, res) = run_in(
            &mut interp,
            "مسار_بحث_المجلدات([\"مشاريع\"])\nانتقل(\"~/مشاريع/هدف\")\nانتقل(\"..\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/مشاريع", "got: {}", out);
        // POSIX (فحوص باش 5.2 الحية): `..` و`../..`
        // و`./..` لا تُستشار فيها `CDPATH` إطلاقا —
        // ولو كان مرشحها موجودا وينهار إلى غير الأب:
        // مع CDPATH = [~/أ]، `../..` من ~/مشاريع/هدف
        // → ~ (لا $نيفا التي يعطيها «~/أ/../..»)،
        // و`./..` → ~/مشاريع (لا ~ التي يعطيها
        // «~/أ/./..»).
        let (_, res) = run_in(
            &mut interp,
            "مسار_بحث_المجلدات([\"~/أ\"])\nانتقل(\"~/مشاريع/هدف\")\nانتقل(\"../..\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"~/مشاريع/هدف\")\nانتقل(\"./..\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/مشاريع", "got: {}", out);
        // CDPATH يغلب الحالي: من البيت، «هدف» في
        // ~/أ/هدف قبل ~/هدف.
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"~\")\nمسار_بحث_المجلدات([\"أ\"])\nانتقل(\"هدف\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/أ/هدف", "got: {}", out);
        // العنصر الفارغ = الحالي: أنشئ ~/هدف أولا، ثم
        // «هدف» من البيت → ~/هدف.
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"~\")\nأنشئ_مجلد(\"هدف\")\nمسار_بحث_المجلدات([\"\"])\nانتقل(\"هدف\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/هدف", "got: {}", out);
        // نوع القيمة: غير قائمة خطأ، وعنصر غير نصي خطأ.
        let (_, res) = run_in(&mut interp, "مسار_بحث_المجلدات(\"مشاريع\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "مسار_بحث_المجلدات([٥])");
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn nav56_logical_vs_physical_and_dotdot() {
        let dir = nav56_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("حقيقي/فرعي")).unwrap();
        std::os::unix::fs::symlink(home.join("حقيقي"), home.join("رابط")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // منطقيا: المجلد يمر عبر الرابط.
        let (_, res) = run_in(&mut interp, "انتقل(\"رابط/فرعي\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/رابط/فرعي", "got: {}", out);
        // فيزيائيا: الرابط محلول إلى الحقيقي (العرض
        // قد يطوّل المسار المطلق على ماك حيث /tmp
        // رابط — فالفحص بالاحتواء لا بالمساواة).
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي(\"فيزيائي\")");
        assert!(out.contains("حقيقي/فرعي"), "got: {}", out);
        assert!(!out.contains("رابط"), "got: {}", out);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي(\"منطقي\")");
        assert_eq!(out.trim_end(), "~/رابط/فرعي", "got: {}", out);
        // .. منطقيا من ~/رابط/فرعي → ~/رابط.
        let (_, res) = run_in(&mut interp, "انتقل(\"..\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/رابط", "got: {}", out);
        // .. فيزيائيا من ~/رابط/فرعي → ~/حقيقي (حل
        // الروابط أولا ثم حذف المكون السابق).
        let (_, res) = run_in(&mut interp, "انتقل(\"فرعي\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "انتقل(\"..\"، \"فيزيائي\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        // دقيق (لا احتواء): البقاء في ~/حقيقي/فرعي
        // — كما كان قبل إصلاح انهيار `..` المعجمي —
        // يفشل هذا التأكيد (ينتهي بـ«فرعي»).
        assert!(out.trim_end().ends_with("حقيقي"), "got: {}", out);
        assert!(!out.contains("فرعي"), "got: {}", out);
        assert!(!out.contains("رابط"), "got: {}", out);
        // «فيزيائي_متحقق» كالفيزيائي؛ والعلم الغريب خطأ.
        let (_, res) = run_in(&mut interp, "انتقل(\"..\"، \"فيزيائي_متحقق\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "انتقل(\"أ\"، \"غريب\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("علم انتقل غير معروف: غريب"));
        // الوضع العام: فيزيائي → القراءة والانتقال
        // فيزيائيان افتراضا؛ ومنطقي → يعود المنطقي.
        let (_, res) = run_in(&mut interp, "انتقل(\"~/رابط/فرعي\")\nاطبع الوضع_الفيزيائي()");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "الوضع_الفيزيائي(صحيح)");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert!(out.contains("حقيقي/فرعي"), "got: {}", out);
        assert!(!out.contains("رابط"), "got: {}", out);
        let (out, _) = run_in(&mut interp, "اطبع الوضع_الفيزيائي()");
        assert_eq!(out.trim_end(), "صحيح", "got: {}", out);
        let (_, res) = run_in(&mut interp, "الوضع_الفيزيائي(خطأ)");
        assert!(res.is_ok(), "{:?}", res);
        // إيقاف الوضع يعيد العرض المنطقي للمجلد المخزن
        // نفسه (~/رابط/فرعي) — فالوضع لا يعيد كتابة
        // المجلد المخزن، بل طريقة عرضه وانتقالاته.
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/رابط/فرعي", "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nav56_cdable_vars_spell_and_pwd() {
        let dir = nav56_dir();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // كشف PWD: بيئة("المجلد_الحالي") يتبع كل تنقل.
        let (out, res) = run_in(&mut interp, "اطبع بيئة(\"المجلد_الحالي\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "~", "got: {}", out);
        let (_, res) = run_in(&mut interp, "انتقل(\"أ\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع بيئة(\"المجلد_الحالي\")");
        assert_eq!(out.trim_end(), "~/أ", "got: {}", out);
        // اسم متغير صالح: قيمته النصية هي المجلد.
        let (_, res) = run_in(&mut interp, "انتقل(\"~\")\nوجهتي = \"مشاريع\"\nانتقل(\"وجهتي\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/مشاريع", "got: {}", out);
        // الاسم الحرفي الموجود يغلب قيمة المتغير.
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"~\")\nأنشئ_مجلد(\"وجهتي\")\nوجهتي = \"ب\"\nانتقل(\"وجهتي\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/وجهتي", "got: {}", out);
        // القيمة غير النصية ليست مجلدا: «رقم» غائب.
        let (_, res) = run_in(&mut interp, "انتقل(\"~\")\nرقم = ٥\nانتقل(\"رقم\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("المجلد غير موجود: رقم"));
        // تصحيح إملاء: «مشاري» خطأ حرف واحد من «مشاريع».
        let (_, res) = run_in(&mut interp, "انتقل(\"مشاري\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع المجلد_الحالي()");
        assert_eq!(out.trim_end(), "~/مشاريع", "got: {}", out);
        // غامض: «مجلد٣» على بعد خطوة واحدة من «مجلد»
        // و«مجلد٢» معا — بلا تصحيح.
        let (_, res) = run_in(
            &mut interp,
            "انتقل(\"~\")\nأنشئ_مجلد(\"مجلد\")\nأنشئ_مجلد(\"مجلد٢\")\nانتقل(\"مجلد٣\")",
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("المجلد غير موجود: مجلد٣"));
        // بعيد بأكثر من خطوة: بلا تصحيح.
        let (_, res) = run_in(&mut interp, "انتقل(\"مشاريععع\")");
        assert!(res.is_err());
        // PWD بعد دفع مكدس؛ ولا OLDPWD ولا ذاكرة سابقة.
        let (_, res) = run_in(&mut interp, "ادفع_مجلد(\"ب\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, _) = run_in(&mut interp, "اطبع بيئة(\"المجلد_الحالي\")");
        assert_eq!(out.trim_end(), "~/ب", "got: {}", out);
        let (_, res) = run_in(&mut interp, "اطبع بيئة(\"المجلد_السابق\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("متغير بيئة غير معروف"));
        let (_, res) = run_in(&mut interp, "انتقل(\"-\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("المجلد غير موجود: -"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 45 §59 (البنود 39–41): بيئة() تقرأ الثمانية
    // فقط، والاسم غير الصالح أو المجهول خطأ يسمّيه.
    #[test]
    fn env_builtin_reads_eight_only() {
        let dir = shortcuts_dir();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        for (name, want) in [
            ("البيت", "~\n"),
            ("مجلد_البرنامج", "$نيفا\n"),
            ("المسار", "$نيفا\n"),
            ("إصدار_نيفا", "١٫٠٫٠\n"),
            ("الطرفية", "نيفا\n"),
            ("اللغة", "ar_EG.UTF-8\n"),
            ("المجلد_المؤقت", "$نيفا/tmp\n"),
            ("الألوان", "truecolor\n"),
        ] {
            let (out, res) = run_in(&mut interp, &format!("اطبع بيئة(\"{}\")", name));
            assert!(res.is_ok(), "{:?}", res);
            assert_eq!(out, want, "got: {} for {}", out, name);
        }
        // بلا تمرير لبيئة النظام: الاسم اللاتيني غير صالح.
        let (_, res) = run_in(&mut interp, "اطبع بيئة(\"PATH\")");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("غير صالح"), "got: {}", msg);
        // اسم صحيح النحو لكن مجهول.
        let (_, res) = run_in(&mut interp, "اطبع بيئة(\"بيت\")");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("غير معروف"), "got: {}", msg);
        // اسم يبدأ برقم: غير صالح.
        let (_, res) = run_in(&mut interp, "اطبع بيئة(\"١بيت\")");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("غير صالح"), "got: {}", msg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=` يلحق
    // لا يستبدل — نص وعدد وقائمة وقاموس، والغائب ينشأ
    // من الهوية كما في باش.
    #[test]
    fn append_assigns_text_number_list_dict_undefined() {
        let (out, res) = run_src(
            "ن = \"أهلا\"\nن += \" وسهلا\"\nاطبع ن\nع = ٥\nع += ٣\nاطبع ع\nق = [١، ٢]\nق += [٣]\nاطبع ق\nخ = {\"أ\": ١}\nخ += {\"ب\": ٢}\nاطبع خ\nغ += ٧\nاطبع غ\nج += \"س\"\nاطبع ج\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            "أهلا وسهلا\n٨\n[١, ٢, ٣]\n{\"أ\": ١, \"ب\": ٢}\n٧\nس\n"
        );
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): المجمد يرفض
    // إعادة الإسناد والإلحاق والحذف.
    #[test]
    fn frozen_variable_rejects_reassign_append_delete() {
        let mut interp = Interpreter::new();
        let (_, res) = run_in(&mut interp, "س = ٥\nجمد(\"س\")\n");
        assert!(res.is_ok(), "{:?}", res);
        for src in ["س = ٦\n", "س += ١\n", "احذف_متغير(\"س\")\n"] {
            let (_, res) = run_in(&mut interp, src);
            assert!(res.is_err(), "expected error for {}", src);
            let msg = format!("{:?}", res.unwrap_err());
            assert!(msg.contains("متغير مجمد"), "got: {}", msg);
        }
        let (out, res) = run_in(&mut interp, "اطبع س\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٥\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الحذف يزيل
    // الربط (القراءة بعده خطأ)، والغائب نجاح صامت.
    #[test]
    fn delete_variable_unbinds_and_unknown_is_silent() {
        let mut interp = Interpreter::new();
        let (_, res) = run_in(&mut interp, "ص = ١\nاحذف_متغير(\"ص\")\n");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "اطبع ص\n");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("متغير غير معرف"), "got: {}", msg);
        let (_, res) = run_in(&mut interp, "احذف_متغير(\"غائب\")\n");
        assert!(res.is_ok(), "{:?}", res);
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الديناميكية
    // — عشوائي يتغير، وثواني غير سالبة، ورقم_السطر سطر
    // الاستدعاء نفسه.
    #[test]
    fn dynamic_vars_random_seconds_lineno() {
        let mut interp = Interpreter::new();
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..20 {
            let (out, res) = run_in(&mut interp, "اطبع بيئة(\"عشوائي\")\n");
            assert!(res.is_ok(), "{:?}", res);
            seen.push(out);
        }
        assert!(
            seen.iter().collect::<std::collections::HashSet<_>>().len() >= 2,
            "random never changed in 20 reads"
        );
        let (out, res) = run_src("اطبع بيئة(\"ثواني\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٠\n");
        let (out, res) = run_src("اطبع بيئة(\"رقم_السطر\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١\n");
        let (out, res) = run_src("س = ١\nاطبع بيئة(\"رقم_السطر\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٢\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): المستخدم
    // واسم_المضيف غير فارغين على لينكس/أندرويد.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn env_user_and_host_nonempty() {
        let (out, res) = run_src("اطبع بيئة(\"المستخدم\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!out.trim().is_empty());
        let (out, res) = run_src("اطبع بيئة(\"اسم_المضيف\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!out.trim().is_empty());
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): القاموس هو
    // المصفوفة الترابطية — مفاتيح بترتيب الإدراج ودمج `+=`.
    #[test]
    fn assoc_dict_keys_and_merge() {
        let (out, res) = run_src(
            "خ = {}\nخ[\"أ\"] = ١\nخ[\"ب\"] = ٢\nاطبع مفاتيح(خ)\nاطبع طول(خ)\nاطبع \"أ\" في خ\nخ += {\"ج\": ٣، \"أ\": ٩}\nاطبع خ\nاطبع مفاتيح(خ)\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            "[\"أ\", \"ب\"]\n٢\nصحيح\n{\"أ\": ٩, \"ب\": ٢, \"ج\": ٣}\n[\"أ\", \"ب\", \"ج\"]\n"
        );
        let (_, res) = run_src("خ = {}\nاطبع خ[\"غ\"]\n");
        assert!(res.is_err());
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): عرض البيئة —
    // 14 سطرا بالترتيب الثابت.
    #[test]
    fn env_display_fourteen_in_fixed_order() {
        let (out, res) = run_src(
            "ق = اعرض_البيئة()\nاطبع طول(ق)\nاطبع ق[٠]\nاطبع ق[٤]\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "١٤\nالبيت=~\nالطرفية=نيفا\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الفحص
    // والمرجع الاسمي — الكتابة عبر المرجع تصل الهدف،
    // والحذف يزيل الاسم وحده.
    #[test]
    fn inspect_and_nameref_write_through() {
        let (out, res) = run_src(
            "س = ٥\nاطبع افحص_متغير(\"س\")\nجمد(\"س\")\nاطبع افحص_متغير(\"س\")\nص = ١٠\nمرجع(\"ر\", \"ص\")\nر = ٢٠\nاطبع ص\nاطبع افحص_متغير(\"ر\")\nاحذف_متغير(\"ر\")\nاطبع ص\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            "متغير س: عدد = ٥\nمتغير س: عدد = ٥ مجمد\n٢٠\nمتغير ر: مرجع إلى ص\n٢٠\n"
        );
        let (_, res) = run_src("مرجع(\"ر\", \"غائب\")\n");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("متغير غير معرف"), "got: {}", msg);
        // الهدف نفسه اسم جديد غير معرّف: غير معرف لا دائري.
        let (_, res) = run_src("مرجع(\"د\", \"د\")\n");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("متغير غير معرف"), "got: {}", msg);
        // الاسم الجديد مستخدم أصلا.
        let (_, res) = run_src("س = ١\nمرجع(\"س\", \"س\")\n");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("الاسم مستخدم"), "got: {}", msg);
        // المعامل الثاني غير نص: يسمّي نوعه هو لا الأول.
        let (_, res) = run_src("س = ١\nمرجع(\"ر\", ٥)\n");
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("وليس عدد"), "got: {}", msg);
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الإلحاق
    // للفهرسة مرفوض تحليلا.
    #[test]
    fn append_index_rejected_at_parse() {
        let toks = tokenize("ق = [١]\nق[٠] += ١\n").unwrap();
        let res = Parser::new(toks).parse_program();
        assert!(res.is_err());
        let msg = format!("{:?}", res.unwrap_err());
        assert!(msg.contains("الإلحاق يدعم المتغير وحده"), "got: {}", msg);
    }
    // their call syntax is on hold pending the owner's comma
    // decision (2026-11-05 «مفيش حاجه اسمها فصلة») — their
    // bodies are implemented, their tests wait for the ruling.
    #[test]
    fn shortcuts_delete_file_and_empty_dir() {
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "احذف(\"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("أ.txt").exists());
        // احذف on a missing path errors; on a non-empty dir errors.
        let (_, res) = run_in(&mut interp, "احذف(\"غيرموجود.txt\")");
        assert!(res.is_err());
        std::fs::create_dir(home.join("ممتلئ")).unwrap();
        std::fs::write(home.join("ممتلئ/داخل.txt"), "x").unwrap();
        let (_, res) = run_in(&mut interp, "احذف(\"ممتلئ\")");
        assert!(res.is_err());
        // An empty dir deletes fine.
        let (_, res) = run_in(&mut interp, "أنشئ_مجلد(\"فارغ\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "احذف(\"فارغ\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("فارغ").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_cd_affects_read() {
        // After انتقل, اقرأ_ملف resolves against the new
        // directory — cd is real, not cosmetic. (The اكتب_ملف
        // half of this check is on hold with the comma ruling:
        // it is a two-argument call.)
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("داخل")).unwrap();
        std::fs::write(home.join("داخل/ملف.txt"), "من الداخل").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انتقل(\"داخل\")");
        assert!(res.is_ok(), "{:?}", res);
        let (out, res) = run_in(&mut interp, "اطبع اقرأ_ملف(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("من الداخل"), "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_cd_bare_goes_parent() {
        // أمر المالك المباشر 2026-11-09 (يلغي §66 جزئيا):
        // `انتقل()` بلا معاملات → الأب مجلدا واحدا كل
        // مرة؛ و`انتقل("-")` اسم غير موجود.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("أ/ب")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انتقل(\"أ\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "انتقل(\"ب\")");
        assert!(res.is_ok(), "{:?}", res);
        // بلا معاملات من عمق 2 → الأب ثم البيت.
        let (out, res) = run_in(&mut interp, "انتقل()\nاطبع المجلد_الحالي()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("~/أ\n"), "got: {}", out);
        let (out, res) = run_in(&mut interp, "انتقل()\nاطبع المجلد_الحالي()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("~\n"), "got: {}", out);
        // من البيت بلا معاملات → مجلد البرنامج.
        let (out, res) = run_in(&mut interp, "انتقل()\nاطبع المجلد_الحالي()");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("$نيفا\n"), "got: {}", out);
        // ومن مجلد البرنامج بلا معاملات → رفض.
        let (_, res) = run_in(&mut interp, "انتقل()");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("خارج مجلد البرنامج"));
        // `-` اسم عادي غير موجود.
        let (_, res) = run_in(&mut interp, "انتقل(\"-\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("المجلد غير موجود: -"));
        // اجتماع 56 §71: معاملان — الثاني علم نصي؛
        // و«ب» علم غير معروف.
        let (_, res) = run_in(&mut interp, "انتقل(\"أ\"، \"ب\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("علم انتقل غير معروف: ب"));
        // ثلاثة معاملات → خطأ العدد الجديد (0 أو 2).
        let (_, res) = run_in(&mut interp, "انتقل(\"أ\"، \"منطقي\"، \"زائد\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("تتوقع 0 أو 2 معاملات"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_copy_onto_itself_refused() {
        // الميزة 202: `انسخ` على النفس كانت تفرّغ الملف
        // بصمت — الآن رفض قبل أي كتابة، والملف سليم.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "محتوى مهم").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // الاسم نفسه.
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \"أ.txt\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن نسخ الملف على نفسه"));
        assert_eq!(
            std::fs::read_to_string(home.join("أ.txt")).unwrap(),
            "محتوى مهم"
        );
        // `./` و`..` والمسار المطلق داخل المجلد.
        let (_, res) = run_in(&mut interp, "انسخ(\"./أ.txt\"، \"أ.txt\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \"فرعي/../أ.txt\")");
        assert!(res.is_err());
        let abs = home.join("أ.txt").to_string_lossy().to_string();
        let src = format!("انسخ(\"{}\", \"أ.txt\")", abs);
        let (_, res) = run_in(&mut interp, &src);
        assert!(res.is_err());
        // الرابط الرمزي لنفس الملف، في الاتجاهين.
        let (_, res) = run_in(&mut interp, "أنشئ_رابط(\"أ.txt\"، \"رابط.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        let (_, res) = run_in(&mut interp, "انسخ(\"رابط.txt\"، \"أ.txt\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \"رابط.txt\")");
        assert!(res.is_err());
        assert_eq!(
            std::fs::read_to_string(home.join("أ.txt")).unwrap(),
            "محتوى مهم"
        );
        // النسخ إلى مجلد يحمل الاسم نفسه (إلحاق الاسم).
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \".\")");
        assert!(res.is_err());
        // النسخ إلى اسم مختلف يعمل، وملفان مختلفان يعملان.
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("ب.txt")).unwrap(),
            "محتوى مهم"
        );
        std::fs::write(home.join("ج.txt"), "آخر").unwrap();
        let (_, res) = run_in(&mut interp, "انسخ(\"أ.txt\"، \"ج.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("ج.txt")).unwrap(),
            "محتوى مهم"
        );
        // `انقل` على النفس بلا أثر: إعادة تسمية على
        // النفس — تنجح والملف سليم.
        let (_, res) = run_in(&mut interp, "انقل(\"أ.txt\"، \"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("أ.txt")).unwrap(),
            "محتوى مهم"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 5 — أدوات الملفات (اجتماع 59 §74).
    fn batch5_set_mtime(path: &std::path::Path, secs: u64) {
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        // فتح قراءة يكفي لـ`set_times` على يونكس — ويعمل
        // على المجلدات أيضا (الكتابة ترفضها EISDIR)؛
        // وعلى ويندوز يفتح المساعد بحق السمة وحده
        // (وإلا Access Denied، os error 5).
        crate::files::set_file_times(path, t).unwrap();
    }

    #[test]
    fn batch5_copy_dir_refused() {
        // الميزة 196: `انسخ` على مجلد برسالة صريحة
        // تسمي البديل، والمجلد يبقى سليما.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مجلد/فرعي")).unwrap();
        std::fs::write(home.join("مجلد/فرعي/أ.txt"), "محتوى مهم").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انسخ(\"مجلد\"، \"نسخة\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري"));
        assert!(!home.join("نسخة").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("مجلد/فرعي/أ.txt")).unwrap(),
            "محتوى مهم"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch5_recursive_copy() {
        // الميزة 197: نسخ تكراري يحفظ الشجرة
        // والروابط الرمزية روابط، والهدف داخل
        // المصدر مرفوض قبل أي كتابة.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مجلد/فرعي")).unwrap();
        std::fs::write(home.join("مجلد/أ.txt"), "أ").unwrap();
        std::fs::write(home.join("مجلد/فرعي/ب.txt"), "ب").unwrap();
        std::os::unix::fs::symlink("أ.txt", home.join("مجلد/رابط.txt")).unwrap();
        std::os::unix::fs::symlink("مجلد/أ.txt", home.join("خارج.txt")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انسخ_تكراري(\"مجلد\"، \"نسخة\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("نسخة/أ.txt")).unwrap(),
            "أ"
        );
        assert_eq!(
            std::fs::read_to_string(home.join("نسخة/فرعي/ب.txt")).unwrap(),
            "ب"
        );
        assert!(std::fs::symlink_metadata(home.join("نسخة/رابط.txt"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_link(home.join("نسخة/رابط.txt")).unwrap(),
            std::path::PathBuf::from("أ.txt")
        );
        // ملف واحد ينسخ كـ`انسخ`.
        let (_, res) = run_in(&mut interp, "انسخ_تكراري(\"مجلد/أ.txt\"، \"ج.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(home.join("ج.txt")).unwrap(), "أ");
        // الهدف داخل المصدر مرفوض، والمصدر سليم.
        let (_, res) = run_in(&mut interp, "انسخ_تكراري(\"مجلد\"، \"مجلد/داخل\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن نسخ المجلد داخل نفسه"));
        assert!(!home.join("مجلد/داخل").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch5_copy_updated() {
        // الميزة 199: الأحدث ينسخ ويرجع صحيحا،
        // والأقدم والمساوي يتخطيان ويرجعان خطأ،
        // والغائب ينسخ؛ والمجلد مصدرا مرفوض.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "جديد").unwrap();
        std::fs::write(home.join("ب.txt"), "قديم").unwrap();
        batch5_set_mtime(&home.join("ب.txt"), 1577836800);
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "صحيح", "got: {}", out);
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "جديد");
        // المصدر أقدم: يتخطى.
        batch5_set_mtime(&home.join("أ.txt"), 1577836800);
        let (out, res) = run_in(&mut interp, "اطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "خطأ", "got: {}", out);
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "جديد");
        // المساوي: يتخطى.
        let t = std::fs::metadata(home.join("ب.txt")).unwrap().modified().unwrap();
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(home.join("أ.txt"))
            .unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_accessed(t)
                .set_modified(t),
        )
        .unwrap();
        drop(f);
        let (out, res) = run_in(&mut interp, "اطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "خطأ", "got: {}", out);
        // الهدف الغائب: ينسخ.
        let (out, res) = run_in(&mut interp, "اطبع انسخ_محدث(\"أ.txt\"، \"ج.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "صحيح", "got: {}", out);
        assert_eq!(std::fs::read_to_string(home.join("ج.txt")).unwrap(), "جديد");
        // المجلد مصدرا: رسالة 196 نفسها.
        std::fs::create_dir(home.join("مجلد")).unwrap();
        let (_, res) = run_in(&mut interp, "انسخ_محدث(\"مجلد\"، \"د.txt\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن نسخ المجلد بلا خيار تكراري"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch5_copy_archive() {
        // الميزة 200: الأرشيف يحفظ الأذونات
        // والأوقات والروابط الرمزية والصلبة.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مجلد/فرعي")).unwrap();
        std::fs::write(home.join("مجلد/أ.txt"), "أ").unwrap();
        std::fs::write(home.join("مجلد/فرعي/ب.txt"), "ب").unwrap();
        std::os::unix::fs::symlink("أ.txt", home.join("مجلد/رابط.txt")).unwrap();
        std::fs::hard_link(home.join("مجلد/أ.txt"), home.join("مجلد/صلب.txt")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            home.join("مجلد/أ.txt"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        batch5_set_mtime(&home.join("مجلد/أ.txt"), 1577836800);
        batch5_set_mtime(&home.join("مجلد/فرعي"), 1577836800);
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "انسخ_أرشيف(\"مجلد\"، \"نسخة\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("نسخة/أ.txt")).unwrap(),
            "أ"
        );
        assert_eq!(
            std::fs::metadata(home.join("نسخة/أ.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            std::fs::metadata(home.join("نسخة/أ.txt"))
                .unwrap()
                .modified()
                .unwrap(),
            std::fs::metadata(home.join("مجلد/أ.txt"))
                .unwrap()
                .modified()
                .unwrap()
        );
        assert_eq!(
            std::fs::metadata(home.join("نسخة/فرعي"))
                .unwrap()
                .modified()
                .unwrap(),
            std::fs::metadata(home.join("مجلد/فرعي"))
                .unwrap()
                .modified()
                .unwrap()
        );
        assert!(std::fs::symlink_metadata(home.join("نسخة/رابط.txt"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_link(home.join("نسخة/رابط.txt")).unwrap(),
            std::path::PathBuf::from("أ.txt")
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(home.join("نسخة/أ.txt")).unwrap().ino(),
            std::fs::metadata(home.join("نسخة/صلب.txt")).unwrap().ino()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch5_copy_light() {
        // الميزة 201: المحتوى متطابق والطريقة نص
        // من الثلاثة — مستنسخ أو متناثر أو عادي.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "محتوى للنسخ الخفيف").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع انسخ_خفيف(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(
            ["مستنسخ", "متناثر", "عادي"].contains(&out.trim_end()),
            "got: {}",
            out
        );
        assert_eq!(
            std::fs::read_to_string(home.join("ب.txt")).unwrap(),
            "محتوى للنسخ الخفيف"
        );
        // ملف متناثر: ثقب كبير ثم ذيل — المحتوى
        // والحجم متطابقان بعد النسخ.
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::File::create(home.join("متناثر.bin")).unwrap();
            f.set_len(1024 * 1024).unwrap();
            // «نهاية» ١٠ بايتات — تنتهي عند ١ ميجا تماما.
            f.seek(SeekFrom::End(-10)).unwrap();
            f.write_all("نهاية".as_bytes()).unwrap();
        }
        let (out, res) = run_in(
            &mut interp,
            "اطبع انسخ_خفيف(\"متناثر.bin\"، \"متناثر2.bin\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert!(
            ["مستنسخ", "متناثر", "عادي"].contains(&out.trim_end()),
            "got: {}",
            out
        );
        assert_eq!(
            std::fs::metadata(home.join("متناثر2.bin")).unwrap().len(),
            1024 * 1024
        );
        assert_eq!(
            std::fs::read(home.join("متناثر2.bin")).unwrap(),
            std::fs::read(home.join("متناثر.bin")).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch5_recursive_delete() {
        // الميزة 206: حذف تكراري للشجرة كاملة،
        // والرابط يزال رابطا لا هدفه؛ و`احذف`
        // و`احذف_مجلد` على غير الفارغ برسالة عربية.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("شجرة/فرعي")).unwrap();
        std::fs::write(home.join("شجرة/أ.txt"), "أ").unwrap();
        std::fs::write(home.join("شجرة/فرعي/ب.txt"), "ب").unwrap();
        std::fs::write(home.join("خارج.txt"), "خارج").unwrap();
        std::os::unix::fs::symlink("../خارج.txt", home.join("شجرة/رابط.txt")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // غير الفارغ: رسالة عربية تسمي البديل.
        let (_, res) = run_in(&mut interp, "احذف(\"شجرة\")");
        assert!(res.is_err());
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("المجلد غير فارغ"), "got: {}", msg);
        assert!(msg.contains("احذف_تكراري"), "got: {}", msg);
        assert!(home.join("شجرة/أ.txt").exists());
        let (_, res) = run_in(&mut interp, "احذف_مجلد(\"شجرة\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("المجلد غير فارغ"));
        // التكراري يحذف الشجرة ويترك الهدف الخارجي.
        let (_, res) = run_in(&mut interp, "احذف_تكراري(\"شجرة\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("شجرة").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("خارج.txt")).unwrap(),
            "خارج"
        );
        // على ملف ورابط يعمل كالحذف العادي.
        let (_, res) = run_in(&mut interp, "احذف_تكراري(\"خارج.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("خارج.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch5_root_guard() {
        // الميزة 208: `/` و`.` و`..` مرفوضة في
        // مداخل الحذف الأربعة — نصا وعبر رابط.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("أ")).unwrap();
        std::os::unix::fs::symlink("/", home.join("جذر")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        for call in [
            "احذف(\"/\")",
            "احذف(\".\")",
            "احذف(\"..\")",
            "احذف(\"أ/..\")",
            "احذف(\"أ/.\")",
            "احذف_مجلد(\"/\")",
            "احذف_ملف(\"/\")",
            "احذف_تكراري(\"/\")",
        ] {
            let (_, res) = run_in(&mut interp, call);
            assert!(res.is_err(), "{}", call);
            let msg = res.unwrap_err().to_string();
            assert!(
                msg.contains("لا يمكن حذف الجذر: /") || msg.contains("لا يمكن حذف . أو .."),
                "{} got: {}",
                call,
                msg
            );
        }
        // الرابط إلى الجذر: رسالة الجذر الصريحة،
        // لا رسالة الساندبوكس العامة.
        let (_, res) = run_in(&mut interp, "احذف(\"جذر\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن حذف الجذر: /"));
        assert!(home.join("جذر").exists());
        let (_, res) = run_in(&mut interp, "احذف_تكراري(\"جذر\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن حذف الجذر: /"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch5_mkdir_mode_and_list() {
        // الميزة 211: وضع ثماني على الأخير وحده،
        // وقائمة ما أُنشئ بالترتيب، والموجود فارغة،
        // وغير الثماني خطأ عربي.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع أنشئ_مجلد(\"أ/ب/ج\"، \"750\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "[\"أ\", \"أ/ب\", \"أ/ب/ج\"]", "got: {}", out);
        assert!(home.join("أ/ب/ج").is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(home.join("أ/ب/ج"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o750
            );
        }
        // الموجود: قائمة فارغة، والوضع لا يمس.
        let (out, res) = run_in(&mut interp, "اطبع أنشئ_مجلد(\"أ/ب/ج\"، \"700\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "[]", "got: {}", out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(home.join("أ/ب/ج"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o750
            );
        }
        // بلا وضع: قائمة المنشأ كما قبل.
        let (out, res) = run_in(&mut interp, "اطبع أنشئ_مجلد(\"س/ص\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out.trim_end(), "[\"س\", \"س/ص\"]", "got: {}", out);
        // غير الثماني: خطأ عربي.
        for bad in ["\"888\"", "\"75a\"", "\"75\"", "\"07755\""] {
            let src = format!("أنشئ_مجلد(\"فاشل\"، {})", bad);
            let (_, res) = run_in(&mut interp, &src);
            assert!(res.is_err(), "{}", src);
            assert!(res
                .unwrap_err()
                .to_string()
                .contains("الوضع يجب أن يكون ثمانيا مثل 755"));
        }
        assert!(!home.join("فاشل").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch5_hard_link() {
        // الميزة 212: رابط صلب يشارك العقدة،
        // والغائب والمجلد مرفوضان صراحة.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "مشترك").unwrap();
        std::fs::create_dir(home.join("مجلد")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "أنشئ_رابط_صلب(\"أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "مشترك");
        std::fs::write(home.join("ب.txt"), "بعد التعديل").unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("أ.txt")).unwrap(),
            "بعد التعديل"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(home.join("أ.txt")).unwrap().ino(),
                std::fs::metadata(home.join("ب.txt")).unwrap().ino()
            );
            assert!(std::fs::metadata(home.join("أ.txt")).unwrap().nlink() >= 2);
        }
        let (_, res) = run_in(&mut interp, "أنشئ_رابط_صلب(\"غائب.txt\"، \"ج.txt\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("الهدف غير موجود"));
        let (_, res) = run_in(&mut interp, "أنشئ_رابط_صلب(\"مجلد\"، \"د.txt\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن إنشاء رابط صلب لمجلد"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch5_relative_link() {
        // الميزة 213: النسبي يحسب من موقع الرابط
        // ويخزن حرفيا، ويبقى صالحا بعد نقل الشجرة.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("شجرة/أ/ب")).unwrap();
        std::fs::write(home.join("شجرة/أ/ب/ملف.txt"), "داخل").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(
            &mut interp,
            "أنشئ_رابط_نسبي(\"شجرة/أ/ب/ملف.txt\"، \"شجرة/أ/رابط.txt\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_link(home.join("شجرة/أ/رابط.txt")).unwrap(),
            std::path::PathBuf::from("ب/ملف.txt")
        );
        assert_eq!(
            std::fs::read_to_string(home.join("شجرة/أ/رابط.txt")).unwrap(),
            "داخل"
        );
        // نقل الشجرة كلها: الرابط يبقى صالحا.
        let (_, res) = run_in(&mut interp, "انقل(\"شجرة\"، \"شجرة2\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("شجرة2/أ/رابط.txt")).unwrap(),
            "داخل"
        );
        // هدف غائب يقبل كالرمزي.
        let (_, res) = run_in(
            &mut interp,
            "أنشئ_رابط_نسبي(\"غائب.txt\"، \"رابط-غائب.txt\")",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_link(home.join("رابط-غائب.txt")).unwrap(),
            std::path::PathBuf::from("غائب.txt")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch5_touch() {
        // الميزة 215: `أنشئ_ملف` على الموجود
        // تحدث وقته بلا مساس بالمحتوى، وعلى
        // الغائب تنشئه فارغا كما قبل.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "محتوى ثابت").unwrap();
        batch5_set_mtime(&home.join("أ.txt"), 1577836800);
        let before = std::fs::metadata(home.join("أ.txt"))
            .unwrap()
            .modified()
            .unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            std::fs::read_to_string(home.join("أ.txt")).unwrap(),
            "محتوى ثابت"
        );
        let after = std::fs::metadata(home.join("أ.txt"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(after > before, "before {:?} after {:?}", before, after);
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"جديد.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(home.join("جديد.txt")).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_move_dir_into_itself_refused() {
        // الميزة 204: نقل مجلد داخل نفسه أو داخل فرع
        // منه مرفوض قبل أي كتابة، والمجلد يبقى سليما؛
        // والنقل العادي (ملف ومجلد) يعمل كما كان.
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مجلد/داخلي")).unwrap();
        std::fs::write(home.join("مجلد/داخلي/أ.txt"), "محتوى مهم").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // داخل نفسه بالاسم نفسه (إلحاق الاسم بعد كون
        // الهدف مجلدا يجعل الهدف «مجلد/مجلد»).
        let (_, res) = run_in(&mut interp, "انقل(\"مجلد\"، \"مجلد\")");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("لا يمكن نقل المجلد داخل نفسه"));
        // داخل فرع منه.
        let (_, res) = run_in(&mut interp, "انقل(\"مجلد\"، \"مجلد/فرعي\")");
        assert!(res.is_err());
        // بصيغة ./ أيضا.
        let (_, res) = run_in(&mut interp, "انقل(\"./مجلد\"، \"مجلد/فرعي\")");
        assert!(res.is_err());
        assert_eq!(
            std::fs::read_to_string(home.join("مجلد/داخلي/أ.txt")).unwrap(),
            "محتوى مهم"
        );
        // النقل العادي يعمل: مجلد ثم ملف.
        let (_, res) = run_in(&mut interp, "انقل(\"مجلد\"، \"منقول\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("مجلد").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("منقول/داخلي/أ.txt")).unwrap(),
            "محتوى مهم"
        );
        let (_, res) = run_in(&mut interp, "انقل(\"منقول/داخلي/أ.txt\"، \"ب.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("منقول/داخلي/أ.txt").exists());
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "محتوى مهم");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_rmdir_rmfile_distinction() {
        let dir = shortcuts_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("مجلد")).unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // احذف_مجلد is dir-only: a file is an error.
        let (_, res) = run_in(&mut interp, "احذف_مجلد(\"أ.txt\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "احذف_مجلد(\"مجلد\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("مجلد").exists());
        // احذف_ملف is file-only: a dir is an error.
        std::fs::create_dir(home.join("مجلد٢")).unwrap();
        let (_, res) = run_in(&mut interp, "احذف_ملف(\"مجلد٢\")");
        assert!(res.is_err());
        let (_, res) = run_in(&mut interp, "احذف_ملف(\"أ.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert!(!home.join("أ.txt").exists());
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
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "ألف\nباء\nجيم\nدال\nهاء\nواو").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        std::fs::write(home.join("فرعي/داخلي.txt"), "داخل").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        // أنشئ_ملف (touch): creates an empty file, and is a
        // no-op when the file already exists.
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"جديد.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(home.join("جديد.txt")).unwrap(), "");
        let (_, res) = run_in(&mut interp, "أنشئ_ملف(\"ملف.txt\")");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(std::fs::read_to_string(home.join("ملف.txt")).unwrap(), "ألف\nباء\nجيم\nدال\nهاء\nواو");
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
        let expected = std::fs::metadata(home.join("ملف.txt")).unwrap().len();
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
        let home = dir.join("home");
        let big = vec![b'x'; Interpreter::MAX_READ_BYTES + 1];
        std::fs::write(home.join("ضخم.txt"), &big).unwrap();
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
        std::fs::write(home.join("تمام.txt"), &ok).unwrap();
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
        let home = dir.join("home");
        for i in 0..1005 {
            std::fs::write(home.join(format!("ملف-{:04}.txt", i)), "").unwrap();
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

    // الدفعة 1 — رموز الخروج (اجتماع 54 §69).
    fn run_interp(src: &str) -> (Interpreter, Result<(), RioError>) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = Interpreter::new();
        let res = interp.run(&prog);
        (interp, res)
    }

    #[test]
    fn exit_code_starts_zero() {
        let (mut interp, res) = run_interp("اطبع رمز_الخروج()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "٠\n");
        assert_eq!(interp.last_exit_code(), 0);
        assert_eq!(interp.take_exit_code(), None);
    }

    #[test]
    fn exit_code_one_after_caught_error() {
        let (mut interp, res) = run_interp(
            "حاول:\n    س = غير_معرف\nماعدا:\n    اطبع رمز_الخروج()\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "١\n");
        assert_eq!(interp.last_exit_code(), 1);
    }

    #[test]
    fn exit_code_not_reset_by_successful_try() {
        let (mut interp, res) = run_interp(
            "حاول:\n    س = ١\nماعدا:\n    اطبع \"خطأ\"\nاطبع رمز_الخروج()\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "٠\n");
        assert_eq!(interp.last_exit_code(), 0);
    }

    #[test]
    fn exit_with_code_ends_program() {
        let (mut interp, res) = run_interp("اطبع \"قبل\"\nخروج(٥)\nاطبع \"بعد\"\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "قبل\n");
        assert_eq!(interp.take_exit_code(), Some(5));
    }

    #[test]
    fn exit_without_arg_uses_last_code() {
        let (mut interp, res) = run_interp(
            "حاول:\n    س = غير_معرف\nماعدا:\n    خروج()\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "");
        assert_eq!(interp.take_exit_code(), Some(1));
    }

    #[test]
    fn exit_rejects_bad_codes() {
        for src in [
            "خروج(-١)\n",
            "خروج(٢٥٦)\n",
            "خروج(٣٫٥)\n",
            "خروج(\"نص\")\n",
        ] {
            let (_, res) = run_interp(src);
            let err = res.unwrap_err();
            assert!(
                err.to_string()
                    .contains("رمز الخروج يجب أن يكون عددا صحيحا بين ٠ و٢٥٥"),
                "src {}: got: {}",
                src,
                err
            );
        }
        let (_, res) = run_interp("خروج(١، ٢)\n");
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("0 أو 1 معاملات"),
            "arity must name the 0-or-1 range"
        );
    }

    #[test]
    fn program_name_defaults_and_set() {
        let (mut interp, res) = run_interp("اطبع اسم_البرنامج()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "نيفا\n");
        let mut interp2 = Interpreter::new();
        interp2.set_source_file("برنامج.نيفا");
        let toks = tokenize("اطبع اسم_البرنامج()\n").unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        assert!(interp2.run(&prog).is_ok());
        assert_eq!(interp2.output, "برنامج.نيفا\n");
    }

    // الدفعة 3 — التشغيل (اجتماع 57 §72): مساعد
    // تشغيل مصدر في مجلد مؤقت بملفاته (ومنها البيت).
    fn batch3_dir(files: &[(&str, &str)]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "niva-batch3-interp-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, src).unwrap();
        }
        dir
    }

    fn batch3_run(
        src: &str,
        dir: &std::path::Path,
    ) -> (String, Result<(), RioError>, Option<u8>) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = Interpreter::with_program_dir(dir.to_path_buf());
        let res = interp.run(&prog);
        let code = interp.take_exit_code();
        (interp.output.clone(), res, code)
    }

    #[test]
    fn batch3_type_command_layers() {
        let dir = batch3_dir(&[
            ("أداة.نيفا", "اطبع \"أداة\"\n"),
            ("home/بيت.نيفا", "اطبع \"بيت\"\n"),
        ]);
        let (out, res, _) = batch3_run(
            "دالة عرفتها():\n    ارجع ١\nاطبع نوع_الامر(\"شغل\")\nاطبع نوع_الامر(\"تن\")\nاطبع نوع_الامر(\"ات\")\nاطبع نوع_الامر(\"اذا\")\nاطبع نوع_الامر(\"عرفتها\")\nاطبع نوع_الامر(\"انتقل\")\nاطبع نوع_الامر(\"ادفع مجلد\")\nاطبع نوع_الامر(\"أداة\")\nاطبع نوع_الامر(\"بيت\")\nاطبع نوع_الامر(\"غائب\")\n",
            &dir,
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            "امر طرفي\nامر طرفي\nاختصار\nكلمة محجوزة\nدالة معرفة\nدالة مدمجة\nدالة مدمجة\nاداة في $نيفا\nاداة في ~\nغير موجود\n",
            "got: {}",
            out
        );
        // غير النص خطأ نوع.
        let (_, res, _) = batch3_run("اطبع نوع_الامر(١)\n", &dir);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("يتطلب نصا"),
            "type error must name the text requirement"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch3_tool_location() {
        let dir = batch3_dir(&[
            ("أداة.نيفا", "اطبع \"أداة\"\n"),
            ("home/بيت.نيفا", "اطبع \"بيت\"\n"),
        ]);
        std::fs::create_dir_all(dir.join("مجلد.نيفا")).unwrap();
        let (out, res, _) = batch3_run(
            "اطبع مكان_الاداة(\"أداة\")\nاطبع مكان_الاداة(\"بيت\")\nاطبع مكان_الاداة(\"غائب\")\nاطبع مكان_الاداة(\"مجلد\")\n",
            &dir,
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            format!(
                "{}\n{}\nلاشيء\nلاشيء\n",
                dir.join("أداة.نيفا").display(),
                dir.join("home").join("بيت.نيفا").display()
            ),
            "got: {}",
            out
        );
        // الاسم المساري مرفوض كـ`شغل`.
        let (_, res, _) = batch3_run("اطبع مكان_الاداة(\"أ/ب\")\n", &dir);
        assert!(res.is_err());
        assert!(
            res.unwrap_err().to_string().contains("بلا مسار"),
            "path-shaped name must be rejected"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch3_exec_session_replaces() {
        let dir = batch3_dir(&[
            ("يخرج.نيفا", "اطبع \"داخل\"\nخروج(٧)\nاطبع \"بعد\"\n"),
            ("فاشل.نيفا", "س = ١ / ٠\n"),
        ]);
        // الاستبدال بلا عودة: السطور بعدها لا تعمل،
        // ورمز الأداة رمز البرنامج.
        let (out, res, code) = batch3_run(
            "اطبع \"قبل\"\nاستبدل_الجلسة(\"يخرج\")\nاطبع \"بعد\"\n",
            &dir,
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "قبل\nداخل\n", "got: {}", out);
        assert_eq!(code, Some(7));
        // كـ`خروج`: لا يمسكها `ماعدا`.
        let (out, res, code) = batch3_run(
            "حاول:\n    استبدل_الجلسة(\"يخرج\")\nماعدا:\n    اطبع \"ممسوك\"\nاطبع \"بعد\"\n",
            &dir,
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "داخل\n", "got: {}", out);
        assert_eq!(code, Some(7));
        // الأداة الفاشلة: مخرجها ورمزها 1، بلا عودة.
        let (out, res, code) = batch3_run(
            "استبدل_الجلسة(\"فاشل\")\nاطبع \"بعد\"\n",
            &dir,
        );
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("قسمة على صفر"), "got: {}", out);
        assert!(!out.contains("بعد"), "got: {}", out);
        assert_eq!(code, Some(1));
        // الأداة الغائبة: خطأ يُمسك بعلامة 127.
        let (_, res, _) = batch3_run("استبدل_الجلسة(\"غائب\")\n", &dir);
        let err = res.unwrap_err();
        assert!(err.is_module_not_found(), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exit_inside_function_and_loop() {
        let (mut interp, res) = run_interp(
            "دالة اخرج():\n    خروج(٧)\nاخرج()\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "");
        assert_eq!(interp.take_exit_code(), Some(7));
        let (mut interp, res) = run_interp(
            "س = ٠\nبينما س < ٣:\n    س = س + ١\n    خروج(٧)\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "");
        assert_eq!(interp.take_exit_code(), Some(7));
    }

    #[test]
    fn exit_skips_except_runs_finally() {
        let (mut interp, res) = run_interp(
            "حاول:\n    خروج(٥)\nماعدا:\n    اطبع \"ماعدا\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "أخيرا\n");
        assert_eq!(interp.take_exit_code(), Some(5));
    }

    #[test]
    fn exit_inside_function_runs_caller_finally() {
        let (mut interp, res) = run_interp(
            "دالة اخرج():\n    خروج(٣)\nحاول:\n    اخرج()\nماعدا:\n    اطبع \"ماعدا\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(interp.output, "أخيرا\n");
        assert_eq!(interp.take_exit_code(), Some(3));
    }

    // المرحلة (د) — الإكمال (اجتماع 66 §81): بديلا
    // البيئة — المعروف غير الفارغ قيمته، والمجهول
    // البديل (أو الفراغ للمشروط).
    #[test]
    fn stage_completion_env_defaults() {
        let (out, res) = run_src(
            "اطبع بيئة_أو(\"المسار\"، \"بديل\")\nاطبع بيئة_أو(\"اسم_غير_موجود_تماماً\"، \"بديل\")\nاطبع بيئة_إن_وجد(\"المسار\"، \"موجود\")\nاطبع بيئة_إن_وجد(\"اسم_غير_موجود_تماماً\"، \"موجود\")\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "$نيفا\nبديل\nموجود\n\nبعد\n");
    }

    // المرحلة (د) — الإكمال (اجتماع 66 §81): إزالة
    // البادئة/اللاحقة الحرفية — والغائبة تترك النص.
    #[test]
    fn stage_completion_strip_prefix_suffix() {
        let (out, res) = run_src(
            "اطبع أزل_بادئة(\"تقرير.txt\"، \"تقرير.\")\nاطبع أزل_لاحقة(\"تقرير.txt\"، \".txt\")\nاطبع أزل_بادئة(\"تقرير.txt\"، \"xx\")\nاطبع أزل_لاحقة(\"تقرير.txt\"، \"yy\")\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "txt\nتقرير\nتقرير.txt\nتقرير.txt\n");
    }

    // المرحلة (د) — الإكمال (اجتماع 66 §81): فهارس
    // القائمة، والقصّ بالحروف (موجب وسالب وطول
    // وخارج النطاق)، واستبدال أول ظهور فقط.
    #[test]
    fn stage_completion_indices_and_string_methods() {
        let (out, res) = run_src(
            "اطبع فهارس([١٠، ٢٠، ٣٠])\nاطبع \"أبجد\".اقطع(١، ٢)\nاطبع \"أبجد\".اقطع(-٢)\nاطبع \"أبجد\".اقطع(١)\nاطبع \"أبجد\".اقطع(٩، ٢)\nاطبع \"أ-أ-أ\".استبدل_أول(\"أ\"، \"ب\")\nاطبع \"أ-أ-أ\".استبدل(\"أ\"، \"ب\")\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(
            out,
            "[٠, ١, ٢]\nبج\nجد\nبجد\n\nب-أ-أ\nب-ب-ب\n"
        );
    }

    // المرحلة (د) — الإكمال (اجتماعا 66 و67 §81
    // و§82): عرض المتغيرات والدوال وحذف الدالة —
    // والغائبة وغير الدالة خطأ يسمّي الاسم.
    #[test]
    fn stage_completion_session_vars_and_functions() {
        let (out, res) = run_src(
            "س = ٥\nنص = \"أ\"\nدالة جمع(أ، ب):\n    ارجع أ + ب\nاطبع اعرض_المتغيرات()\nاطبع اعرض_الدوال()\nاحذف_دالة(\"جمع\")\nاطبع اعرض_الدوال()\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "[\"س=٥\", \"نص=\\\"أ\\\"\"]\n[\"جمع\"]\n[]\n");
        let (_, res2) = run_src("احذف_دالة(\"غائبة\")\n");
        assert!(res2.is_err());
        assert!(res2.unwrap_err().to_string().contains("دالة غير موجودة: غائبة"));
        let (_, res3) = run_src("س = ٥\nاحذف_دالة(\"س\")\n");
        assert!(res3.is_err());
        assert!(res3.unwrap_err().to_string().contains("ليست دالة: س"));
    }

    // المرحلة (د) — الإكمال (اجتماع 69 §84): أعلام
    // عرض الملفات — المخفية والعكسي والطويل والبشري
    // والمسار ومجلد_نفسه.
    #[test]
    fn stage_completion_list_flags() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::create_dir_all(dir.join("home").join("فرعي")).unwrap();
        std::fs::write(dir.join("home").join("أ.txt"), "نص").unwrap();
        std::fs::write(dir.join("home").join("ب.txt"), "نص أطول هنا").unwrap();
        std::fs::write(dir.join("home").join(".مخفي"), "خ").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "[\"أ.txt\", \"ب.txt\", \"فرعي/\"]\n", "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات(\"أظهر_المخفية\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains(".مخفي"), "got: {}", out);
        assert!(!out.contains("\".\""), "got: {}", out);
        assert!(!out.contains("\"..\""), "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات(\"عكسي\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "[\"فرعي/\", \"ب.txt\", \"أ.txt\"]\n", "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات(\"أ.txt\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "[\"أ.txt\"]\n", "got: {}", out);
        let (_, res2) = run_in(&mut interp, "اطبع اعرض_الملفات(\"غائب\")\n");
        assert!(res2.is_err());
        assert!(res2
            .unwrap_err()
            .to_string()
            .contains("المسار غير موجود: غائب"));
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات(\"فرعي\"، \"مجلد_نفسه\"، \"طويل\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("مجلد "), "got: {}", out);
        assert!(out.contains("فرعي"), "got: {}", out);
        let (out, res) = run_in(&mut interp, "اطبع اعرض_الملفات(\"طويل\"، \"بشري\")\n");
        assert!(res.is_ok(), "{:?}", res);
        assert!(out.contains("ملف "), "got: {}", out);
        let (_, res2) = run_in(&mut interp, "اطبع اعرض_الملفات(\"أ\"، \"ب\")\n");
        assert!(res2.is_err());
        assert!(res2
            .unwrap_err()
            .to_string()
            .contains("مسارا واحدا على الأكثر"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 69 §84): حقول
    // تفاصيل_المسار الموسعة — وهدف الرابط.
    #[test]
    fn stage_completion_path_details_extended() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-details-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("ملف.txt"), "نص").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(&mut interp, "اطبع تفاصيل_المسار(\"ملف.txt\")\n");
        assert!(res.is_ok(), "{:?}", res);
        for key in [
            "\"نوع\": \"ملف\"",
            "\"حجم\": ٤",
            "\"معدل\": ",
            "\"أذونات\": \"-",
            "\"مالك\": ",
            "\"مجموعة\": ",
            "\"وصل\": ",
            "\"غيّر\": ",
            "\"عقدة\": ",
            "\"روابط\": ١",
            "\"هدف\": لاشيء",
        ] {
            assert!(out.contains(key), "missing {} in: {}", key, out);
        }
        assert!(!out.contains("\"مالك\": \"—\""), "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 69 §84): نسخ
    // «لا_تكتب» يتخطى الموجود بصمت، وحذف
    // «تجاهل_الغائب» ينجح صامتا؛ وبلا العلمين يبقى
    // السلوك القديم.
    #[test]
    fn stage_completion_copy_no_clobber_and_delete_ignore_missing() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-flags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("مصدر.txt"), "جديد").unwrap();
        std::fs::write(dir.join("home").join("هدف.txt"), "قديم").unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(
            &mut interp,
            "انسخ(\"مصدر.txt\"، \"هدف.txt\"، \"لا_تكتب\")\nاطبع اقرأ_ملف(\"هدف.txt\")\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "قديم\n", "got: {}", out);
        let (out, res) = run_in(
            &mut interp,
            "انسخ(\"مصدر.txt\"، \"هدف.txt\")\nاطبع اقرأ_ملف(\"هدف.txt\")\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "جديد\n", "got: {}", out);
        let (out, res) = run_in(&mut interp, "احذف(\"غائب.txt\"، \"تجاهل_الغائب\")\nاطبع \"تم\"\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "تم\n", "got: {}", out);
        let (_, res2) = run_in(&mut interp, "احذف(\"غائب.txt\")\n");
        assert!(res2.is_err());
        assert!(res2.unwrap_err().to_string().contains("المسار غير موجود"));
        let (_, res3) = run_in(&mut interp, "انسخ(\"مصدر.txt\"، \"هدف.txt\"، \"علم_غريب\")\n");
        assert!(res3.is_err());
        assert!(res3.unwrap_err().to_string().contains("علم انسخ غير معروف"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): مصدر —
    // التعريفات تبقى بعد عودته، والبحث في $نيفا ثم ~
    // ثم المجلد الحالي، والغائب والدورة خطأ صريح.
    #[test]
    fn stage_completion_source_file() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(
            dir.join("home").join("تعريفات.نيفا"),
            "س = ٤٢\nدالة ضاعف(ن):\n    ارجع ن * ٢\n",
        )
        .unwrap();
        let mut interp = Interpreter::with_program_dir(dir.clone());
        let (out, res) = run_in(
            &mut interp,
            "مصدر(\"تعريفات.نيفا\")\nاطبع س\nاطبع ضاعف(٣)\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(out, "٤٢\n٦\n", "got: {}", out);
        let (_, res2) = run_in(&mut interp, "مصدر(\"غائب\")\n");
        assert!(res2.is_err());
        assert!(res2.unwrap_err().to_string().contains("الملف غير موجود"));
        std::fs::write(dir.join("home").join("دوري.نيفا"), "مصدر(\"دوري.نيفا\")\n").unwrap();
        let (_, res3) = run_in(&mut interp, "مصدر(\"دوري.نيفا\")\n");
        assert!(res3.is_err());
        assert!(res3.unwrap_err().to_string().contains("يُستدعى دوريا"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
