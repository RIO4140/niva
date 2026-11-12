//! Niva REPL (interactive terminal), Meeting 7.
//!
//! A REPL with RTL display, built on the compiler (VM) by
//! default since phase ك-2 (Meeting 25, task 7): every block
//! compiles to bytecode and runs on one persistent VM session
//! whose globals table survives across `feed()` calls
//! (DESIGN §39.3). The tree-walking interpreter stays only
//! as a hidden diagnostic mode (`Repl::new_interpreter`,
//! CLI `--مفسر`) — same session behavior, for comparison
//! and diagnosis, never documented as a way to run programs.
//!
//! Design:
//! - One engine (VM session or interpreter) + one
//!   SemanticAnalyzer persist across lines,
//!   so variables and functions defined earlier stay defined.
//!   The analyzer defines a name only after its value analyzes
//!   cleanly, so a failed line does not pollute later lines.
//! - Multi-line blocks are parser-driven: if parsing fails with
//!   "نهاية غير متوقعة داخل كتلة" or "توقع كتلة بمسافة بادئة
//!   بعد النقطتين", the input is incomplete and the REPL reads
//!   another line. An empty line while a block is open cancels it.
//! - A bare expression prints its value (fmt_value), like a
//!   Python REPL. Other statements run normally; their اطبع
//!   output is printed.
//! - Arabic prompt, Arabic help and errors. RTL display aid:
//!   :اعرض re-prints the session's lines wrapped in Unicode
//!   bidi isolates (RLI ... PDI), so a terminal renders each
//!   line as one isolated run. Full glyph shaping still depends
//!   on the terminal itself; the REPL does not pretend otherwise.
//! - std-only (no external crates): no arrow-key history in v1.
//! - Stateful line API: `Repl::feed` takes one line and returns
//!   `ReplOutput` (display text, next prompt, ended flag), holding
//!   the pending multi-line block inside the struct. The native
//!   `run_repl` loop and the WASM console both run on top of it,
//!   so the interactive behavior is identical everywhere.
//! - File policy is the same as file mode: restricted to the
//!   current working directory, unless --اسمح-بالكل was given.
//! - Commands (top-level only): :مساعدة :اعرض :امسح-المحفوظات
//!   :مسح :خروج (and plain خروج). Arabic only — no Latin
//!   aliases. :امسح-المحفوظات (task 2 of the Meeting 25 plan)
//!   clears only the :اعرض history lines — variables and
//!   functions stay — while :مسح resets the whole interpreter.
//! - Short terminal commands (owner's order 2026-11-05 ~22:02,
//!   corrected ~22:05): two-letter codes typed bare — like the
//!   :commands but WITHOUT the colon — with space-separated
//!   arguments and never a comma: ات (ls) مح (pwd) نم (cd)
//!   شم (mkdir) حم (rmdir) حف (rm) سم (cp) قم (mv) اف (cat).
//!   A REPL layer only: the line is intercepted before it is
//!   parsed as Niva code, and each code calls the file builtin
//!   that implements it. Full-word builtins (اعرض_الملفات...)
//!   stay as the implementation; the codes are what the
//!   terminal user types. Code rule: one easy letter from the
//!   first word + one easy letter from the second word.
//! - The prompt is Arabic-only punctuation: `نيفا: ` — the old
//!   `نيفا> ` mirrored to `نيفا<` in RTL display (bidi), and the
//!   continuation `.... ` was Latin dots; it is now `أكمل: `.

use crate::ast::{Expr, Stmt, StmtKind};
use crate::error::RioResult;
use crate::history::History;
use crate::interpreter::Interpreter;
use crate::lexer::{normalize_cli_word, tokenize};
use crate::parser::Parser;
use crate::redirect;
use crate::semantic::SemanticAnalyzer;
use crate::value::Value;
use crate::vm::VmSession;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Parser errors that mean "this input is incomplete, read more".
const INCOMPLETE_MARKERS: [&str; 2] =
    ["نهاية غير متوقعة داخل كتلة", "توقع كتلة بمسافة بادئة بعد النقطتين"];

/// Unicode bidi isolates, used by :اعرض for RTL display.
const RLI: char = '\u{2066}';
const PDI: char = '\u{2069}';

/// Block-continuation prompt: `> `, like any terminal
/// (owner's order 2026-11-05 ~23:02). The normal prompt is
/// dynamic — the current directory + ` $ ` (see
/// `Repl::prompt`): `~ $ ` at home, `~/dir $ ` below it,
/// RTL (owner's orders 2026-11-05 ~22:58 and ~23:03: the
/// prompt is never `نيفا: `).
pub const CONT_PROMPT: &str = "> ";

/// Structured control signals the REPL returns alongside its
/// display text (audit م-6): a host that wants them reads them
/// on a separate channel (stderr, only when the
/// `NIVA_CONTROL_CHANNEL=stderr` environment variable is set)
/// instead of matching visible text — a user program printing
/// the same words must never trigger them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplControl {
    None,
    /// This output asks for network permission (first use of
    /// اجلب with the gate closed).
    NetworkNeeded,
    /// This output asks for server permission (first use of
    /// اخدم with the gate closed). The network flag alone is
    /// never enough for the server (Meeting 27 §42.4).
    ServerNeeded,
    /// The session was cleared (:مسح) — the host clears its
    /// arrow-key history.
    SessionCleared,
    /// The settings were re-read (أعد-الإعدادات) — the host
    /// re-applies them live (Meeting 45 §59, item 19).
    SettingsReload,
}

/// The result of feeding one line to the REPL: the text to
/// display (stdout output, with any error text appended), the
/// prompt to show next, and whether the session ended (on
/// :خروج — EOF is reported by the caller's loop via `eof()`).
pub struct ReplOutput {
    pub output: String,
    pub prompt: String,
    pub ended: bool,
    /// True when this output is (or contains) the network
    /// permission error — the interactive console uses it to
    /// offer the first-use permission prompt (Meeting 22 §3.3).
    pub needs_network: bool,
}


pub enum ReplEval {
    Done { output: String },
    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): `code`
    /// رمز السطر — 2 لغوي/نحوي/دلالي، و1 وقت تشغيل،
    /// و127 وحدة مستوردة غائبة (§69 ممتد).
    Failed { output: String, error: String, code: u8 },
    /// الدفعة 1: السطر استدعى `خروج(رمز)` — الجلسة
    /// تنتهي برمزه؛ والمخرج ما طُبع قبل الخروج.
    Exited { output: String, code: u8 },
    Incomplete,
}

/// الدفعة 1 (§69 ممتد): خطأ وقت تشغيل → فشل عام (1)،
/// إلا وحدة مستوردة غائبة (ملف مطلوب غير موجود) → 127.
fn runtime_error_code(e: &crate::error::RioError) -> u8 {
    if e.is_module_not_found() {
        127
    } else {
        1
    }
}

/// ك-2 (مهمة 7): محرك الجلسة — VM افتراضيا، والمفسر وضع
/// تشخيص خفيا فقط. الواجهة مرآة واجهة المفسر التي يستخدمها
/// REPL، فيبقى منطق الجلسة واحدا في الوضعين.
enum Engine {
    Vm(VmSession),
    Interp(Interpreter),
}

impl Engine {
    fn run(&mut self, program: &[Stmt]) -> RioResult<()> {
        match self {
            Engine::Vm(s) => s.run(program),
            Engine::Interp(i) => i.run(program),
        }
    }

    fn eval_one(&mut self, expr: &Expr) -> RioResult<Value> {
        match self {
            Engine::Vm(s) => s.eval_one(expr),
            Engine::Interp(i) => i.eval_one(expr),
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        match self {
            Engine::Vm(s) => s.call_builtin(name, args, line, col),
            Engine::Interp(i) => i.call_builtin(name, args, line, col),
        }
    }

    fn current_dir_display(&self) -> String {
        match self {
            Engine::Vm(s) => s.current_dir_display(),
            Engine::Interp(i) => i.current_dir_display(),
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82):
    /// أسماء متغيرات/دوال الجلسة والمجلد الحالي —
    /// للإكمال في التفاعلي.
    fn session_variable_names(&self) -> Vec<String> {
        match self {
            Engine::Vm(s) => s.session_variable_names(),
            Engine::Interp(i) => i.session_variable_names(),
        }
    }

    fn session_function_names(&self) -> Vec<String> {
        match self {
            Engine::Vm(s) => s.session_function_names(),
            Engine::Interp(i) => i.session_function_names(),
        }
    }

    fn cwd_path(&self) -> std::path::PathBuf {
        match self {
            Engine::Vm(s) => s.cwd_path(),
            Engine::Interp(i) => i.cwd_path(),
        }
    }

    fn take_output(&mut self) -> String {
        match self {
            Engine::Vm(s) => s.take_output(),
            Engine::Interp(i) => std::mem::take(&mut i.output),
        }
    }

    fn set_trusted(&mut self, trusted: bool) {
        match self {
            Engine::Vm(s) => s.set_trusted(trusted),
            Engine::Interp(i) => i.set_trusted(trusted),
        }
    }

    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): يأخذ
    /// رمز الخروج المعلق مرة واحدة، من أي محرك.
    fn take_exit_code(&mut self) -> Option<u8> {
        match self {
            Engine::Vm(s) => s.take_exit_code(),
            Engine::Interp(i) => i.take_exit_code(),
        }
    }

    /// الدفعة 1: ضبط رمز آخر أمر في المحرك (تزامن
    /// REPL بعد كل سطر).
    fn set_last_exit_code(&mut self, code: u8) {
        match self {
            Engine::Vm(s) => s.set_last_exit_code(code),
            Engine::Interp(i) => i.set_last_exit_code(code),
        }
    }

    /// المرحلة (ج): ضبط رموز مراحل آخر أنبوب في المحرك
    /// (تزامن REPL بعد كل سطر، كـ`set_last_exit_code`).
    fn set_pipe_status(&mut self, codes: Vec<u8>) {
        match self {
            Engine::Vm(s) => s.set_pipe_status(codes),
            Engine::Interp(i) => i.set_pipe_status(codes),
        }
    }

    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root for this session — مرآة set_trusted
    /// حرفيا.
    fn set_downloads(&mut self, path: std::path::PathBuf) {
        match self {
            Engine::Vm(s) => s.set_downloads(path),
            Engine::Interp(i) => i.set_downloads(path),
        }
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// — مرآة set_trusted حرفيا.
    fn set_live_output(&mut self, live: bool) {
        match self {
            Engine::Vm(s) => s.set_live_output(live),
            Engine::Interp(i) => i.set_live_output(live),
        }
    }

    /// المرحلة (ج) — قارئ البث الحي (ليُستعاد بعد
    /// إطفائه المؤقت أثناء التوجيه/الأنبوب).
    fn live_output(&self) -> bool {
        match self {
            Engine::Vm(s) => s.live_output(),
            Engine::Interp(i) => i.live_output(),
        }
    }

    fn set_network_allowed(&mut self, allowed: bool) {
        match self {
            Engine::Vm(s) => s.set_network_allowed(allowed),
            Engine::Interp(i) => i.set_network_allowed(allowed),
        }
    }

    fn network_allowed(&self) -> bool {
        match self {
            Engine::Vm(s) => s.network_allowed(),
            Engine::Interp(i) => i.network_allowed(),
        }
    }

    fn set_server_allowed(&mut self, allowed: bool) {
        match self {
            Engine::Vm(s) => s.set_server_allowed(allowed),
            Engine::Interp(i) => i.set_server_allowed(allowed),
        }
    }

    fn server_allowed(&self) -> bool {
        match self {
            Engine::Vm(s) => s.server_allowed(),
            Engine::Interp(i) => i.server_allowed(),
        }
    }

    fn allow_local_network_host(&mut self, host: &str) {
        match self {
            Engine::Vm(s) => s.allow_local_network_host(host),
            Engine::Interp(i) => i.allow_local_network_host(host),
        }
    }

    fn reset(&mut self, trusted: bool, network_allowed: bool, server_allowed: bool, live_output: bool, downloads: Option<std::path::PathBuf>) {
        match self {
            Engine::Vm(s) => {
                s.reset();
                s.set_live_output(live_output);
                if let Some(d) = downloads {
                    s.set_downloads(d);
                }
            }
            Engine::Interp(i) => {
                *i = Interpreter::new();
                i.set_trusted(trusted);
                i.set_network_allowed(network_allowed);
                i.set_server_allowed(server_allowed);
                i.set_live_output(live_output);
                if let Some(d) = downloads {
                    i.set_downloads(d);
                }
            }
        }
    }
}

/// المرحلة (د) — الإكمال (اجتماع 67 §82): مستعار
/// جلسة — نص الاستبدال، وهل ينتهي بمسافة فيوسّع
/// الكلمة التالية أيضا.
struct AliasDef {
    value: String,
    expand_next: bool,
}

/// المرحلة (ج) — مستند هنا معلّق: السطر الذي بدأه
/// (`<< محدد`)، والمحدد، وهل تُحذف التبويبات البادئة،
/// والجسم المجموع حتى الآن (أسطر بيانات لا كود).
struct PendingHeredoc {
    line: String,
    delimiter: String,
    strip_tabs: bool,
    body: String,
}

/// المرحلة (ج) — نتيجة مرحلة واحدة: مخرجها (المعادل
/// للواصف 1) وخطأها (الواصف 2) ورمزها، وهل أنهت
/// الجلسة (`خروج`).
struct StageOutcome {
    stdout: String,
    stderr: String,
    code: u8,
    ended: bool,
}

/// المرحلة (ج) — مصدر مدخل مرحلة بعد حله: الطرفية،
/// أو نص (من أنبوب سابق أو سلسلة هنا أو مستند هنا)،
/// أو مغلق (فيعيد `ادخل` `""` فورا).
enum StageStdin {
    Terminal,
    Text(String),
    Closed,
}

/// المرحلة (ج) — قسّم نصا إلى أسطر مع إبقاء `\n`
/// في نهاية كل سطر (فالمضخة تعيد السطر كما كُتب).
fn split_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(|s| s.to_string()).collect()
}

/// المرحلة (د) — الإكمال (اجتماع 68 §83): مفاتيح
/// `niva.properties` الموجودة (المحرك + التطبيق) —
/// وحدها يقبلها `خيارات` للضبط؛ والمجهول خطأ.
const KNOWN_SETTINGS_KEYS: [&str; 24] = [
    // المحرك: الترحيب والمنع، والمحفوظات الستة،
    // والموجهات الخمسة.
    "الترحيب", "منع_المسح",
    "ملف_المحفوظات", "حد_المحفوظات", "حد_ملف_المحفوظات",
    "قواعد_المحفوظات", "تجاهل_المحفوظات", "صيغة_وقت_المحفوظات",
    "موجه_أول", "موجه_استمرار", "موجه_تتبع", "أمر_قبل_الموجه", "مهلة_الخمول",
    // التطبيق: الخط والألوان والمظهر وصف المفاتيح.
    "حجم_الخط", "ابقاء_الشاشة", "اظهار_صف_المفاتيح", "المظهر",
    "إخفاء_لوحة_المفاتيح_عند_البدء", "خط_مخصص", "ملء_الشاشة",
    "تعطيل_اختصارات_لوحة_المفاتيح", "لون_الخطأ", "لون_النجاح", "لون_المؤشر",
];

/// المرحلة (ج): اقرأ علما منطقيا من مدخلات الإعدادات
/// (`صح` = يعمل، وغيرها = متوقف؛ والغائب = متوقف).
fn settings_flag(entries: &[(String, String)], key: &str) -> bool {
    entries
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v == "صح")
        .unwrap_or(false)
}

pub struct Repl {
    engine: Engine,
    analyzer: SemanticAnalyzer,
    history: History,
    // Accumulated lines of a multi-line block still being typed.
    pending: String,
    // Remembered because Interpreter does not expose its file
    // policy for reading; reset() must restore the same flag.
    trusted: bool,
    // Meeting 22: same pattern for the network flag — the
    // interpreter holds the process-wide gate, and reset()
    // must restore the same value.
    network_allowed: bool,
    // Meeting 27: same pattern for the server flag — the
    // engine holds it per instance, and reset() must restore
    // the same value (the network flag alone never opens it).
    server_allowed: bool,
    // اجتماع 32، السؤال الثامن: البث الحي لمخرجات
    // `اطبع` — النمط نفسه: المحرك يحمله لكل نسخة،
    // وreset() يعيد القيمة نفسها (وإلا ضاع عند :مسح).
    live_output: bool,
    // Meeting 50 §64 (items 94/96): the read-only
    // downloads root — النمط نفسه: المحرك يحمله لكل
    // نسخة، وreset() يعيد القيمة نفسها.
    downloads: Option<std::path::PathBuf>,
    // مجلد البرنامج (جذر الاستيراد والتشغيل) — يُحفظ
    // لأن `شغّل` يشغّل الملف منه؛ والجلسة نفسها تبنيه
    // من مجلد العمل الجاري (new) أو من المعطى (tests).
    program_dir: std::path::PathBuf,
    // Audit م-6: the control signal of the last fed line
    // (None / NetworkNeeded / SessionCleared), read by
    // `last_control()` — a side channel so run_repl can emit
    // it on stderr without touching the display text.
    last_control: ReplControl,
    // اجتماع 45 §59 (البند 19): تأكيد أعد-الضبط معلّق —
    // السطر التالي «نعم» حرفيا ينفّذه، وأي سطر آخر يلغيه.
    pending_reset: bool,
    // الدفعة 7 — الموجهات والخطاف والمهلة (اجتماع 61 §76):
    // إعدادات الموجهات الثلاثة والخطاف والمهلة — من
    // `niva.properties` عند الإنشاء (ملف غائب = افتراضيات)،
    // وتبقى عبر :مسح (إعدادات لا حالة جلسة)؛ وحارس
    // التكرار للخطاف (استدعاء متداخل يُتخطى بصمت).
    prompts: crate::prompts::PromptSettings,
    in_prompt_hook: bool,
    // الدفعة 1 — رموز الخروج (اجتماع 54 §69): رمز آخر
    // سطر مكتمل (0 أولا)؛ ورمز الخروج الذي تنتهي به
    // الجلسة (صريح من :خروج، أو آخر رمز عند EOF)؛
    // ورمز السطر الجاري (يضبطه كل مسار ناتج نهائي،
    // وتقرأه feed() لتثبيته)؛ وعدد بادئات `عكس` في
    // السطر الجاري (فردي يقلب الرمز).
    last_exit_code: u8,
    quit_code: Option<u8>,
    pending_code: Option<u8>,
    invert_count: usize,
    // الدفعة 3 — التشغيل (اجتماع 57 §72، الميزة 53):
    // حارس ضد استدعاء الخطاف `عند_غياب_الامر` داخل
    // نفسه — غياب داخل الخطاف لا يستدعيه من جديد.
    in_not_found_hook: bool,
    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): رموز مراحل آخر أنبوب (أو رمز
    // السطر وحده بلا أنبوب) تقرأها `رموز_الأنبوب()`؛
    // ومفتاح `منع_المسح` (من الإعدادات، والافتراضي
    // خطأ)؛ وعلم `توقف_عند_الخطأ` (جلسة فقط، والافتراضي
    // خطأ)؛ ومستند هنا معلّق يجمع أسطره. (وكتم تسجيل
    // المراحل أثناء الأنبوب يتم بتعليق المحفوظات
    // `suspend_recording` القائم لا بعلم جديد — فالسطر
    // كله مدخل واحد لا مدخلات بعدد مراحله.)
    pipe_status: Vec<u8>,
    pipe_status_set: bool,
    noclobber: bool,
    stop_on_error: bool,
    pending_heredoc: Option<PendingHeredoc>,
    // المرحلة (د) — الإكمال (اجتماع 67 §82): مستعارات
    // الجلسة (اسم → قيمة وعلم المسافة الأخيرة) —
    // طبقة تفاعلية فقط، وتُمسح مع :مسح.
    aliases: HashMap<String, AliasDef>,
    // المرحلة (د) — الإكمال (اجتماع 68 §83): حارس
    // تشغيل ملف الخروج مرة واحدة في الجلسة.
    exit_file_ran: bool,
    // المرحلة (د) — الإكمال (اجتماع 68 §83): علما بدء
    // التشغيل التفاعلي — تخطي ملفات البدء، وملف بدء
    // إضافي يُنفَّذ بعدها.
    skip_startup: bool,
    startup_file: Option<std::path::PathBuf>,
}

impl Repl {
    /// الجلسة الافتراضية: على المترجم (VM).
    pub fn new() -> Self {
        let program_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        // الدفعة 6 (اجتماع 60 §75): إعدادات المحفوظات من
        // `niva.properties` عند الإنشاء (ملف غائب = افتراضيات).
        let (settings_entries, _) = read_settings(&program_dir);
        Self {
            engine: Engine::Vm(VmSession::new(program_dir.clone())),
            analyzer: SemanticAnalyzer::new(),
            history: History::from_settings(&program_dir, &settings_entries),
            prompts: crate::prompts::PromptSettings::from_entries(&settings_entries),
            in_prompt_hook: false,
            pending: String::new(),
            trusted: false,
            network_allowed: false,
            server_allowed: false,
            live_output: false,
            downloads: None,
            program_dir,
            last_control: ReplControl::None,
            pending_reset: false,
            last_exit_code: 0,
            quit_code: None,
            pending_code: None,
            invert_count: 0,
            in_not_found_hook: false,
            // المرحلة (ج): رموز الأنبوب فارغة أولا،
            // و`منع_المسح` من الإعدادات، وبلا توقف عند
            // الخطأ، وبلا مستند معلق.
            pipe_status: Vec::new(),
            pipe_status_set: false,
            noclobber: settings_flag(&settings_entries, "منع_المسح"),
            stop_on_error: false,
            pending_heredoc: None,
            aliases: HashMap::new(),
            exit_file_ran: false,
            skip_startup: false,
            startup_file: None,
        }
    }

    /// وضع التشخيص الخفي: الجلسة نفسها على مفسر الشجرة
    /// (CLI `--مفسر`) — للمقارنة والتشخيص فقط.
    pub fn new_interpreter() -> Self {
        let program_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        // الدفعة 6: إعدادات المحفوظات كما في new().
        let (settings_entries, _) = read_settings(&program_dir);
        Self {
            engine: Engine::Interp(Interpreter::new()),
            analyzer: SemanticAnalyzer::new(),
            history: History::from_settings(&program_dir, &settings_entries),
            prompts: crate::prompts::PromptSettings::from_entries(&settings_entries),
            in_prompt_hook: false,
            pending: String::new(),
            trusted: false,
            network_allowed: false,
            server_allowed: false,
            live_output: false,
            downloads: None,
            program_dir,
            last_control: ReplControl::None,
            pending_reset: false,
            last_exit_code: 0,
            quit_code: None,
            pending_code: None,
            invert_count: 0,
            in_not_found_hook: false,
            // المرحلة (ج): كما في new() — رموز الأنبوب
            // فارغة أولا، و`منع_المسح` من الإعدادات.
            pipe_status: Vec::new(),
            pipe_status_set: false,
            noclobber: settings_flag(&settings_entries, "منع_المسح"),
            stop_on_error: false,
            pending_heredoc: None,
            aliases: HashMap::new(),
            exit_file_ran: false,
            skip_startup: false,
            startup_file: None,
        }
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        self.engine.set_trusted(trusted);
    }

    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root for this session, live — no restart,
    /// no lost session state (mirror of set_trusted). The
    /// path must exist and be a directory; otherwise an
    /// Arabic error naming the problem, and nothing changes.
    pub fn set_downloads(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        if !path.is_dir() {
            return Err("مسار التنزيلات غير صالح".to_string());
        }
        self.downloads = Some(path.clone());
        self.engine.set_downloads(path);
        Ok(())
    }

    /// This session's downloads root, if storage was exposed.
    pub fn downloads(&self) -> Option<std::path::PathBuf> {
        self.downloads.clone()
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): علم
    /// `--بلا-بدء` — تخطي ملفات البدء والمقتطفات.
    pub fn set_skip_startup(&mut self, skip: bool) {
        self.skip_startup = skip;
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): ملف بدء
    /// إضافي من `--ملف-بدء <مسار>` — يُنفَّذ بعد ملفات
    /// البدء المعتادة.
    pub fn set_startup_file(&mut self, path: std::path::PathBuf) {
        self.startup_file = Some(path);
    }

    /// REPL rooted at a given directory (tests; the app and the
    /// CLI use the process working directory via new()).
    pub fn with_program_dir(program_dir: std::path::PathBuf) -> Self {
        let mut r = Self::new();
        r.engine = Engine::Vm(VmSession::new(program_dir.clone()));
        // الدفعة 6: المحفوظات على المجلد المعطى لا على
        // مجلد العمل (فملفها وإعداداتها منه).
        let settings_entries = read_settings(&program_dir).0;
        r.history = History::from_settings(&program_dir, &settings_entries);
        r.prompts = crate::prompts::PromptSettings::from_entries(&settings_entries);
        // المرحلة (ج): `منع_المسح` من إعدادات المجلد المعطى.
        r.noclobber = settings_flag(&settings_entries, "منع_المسح");
        r.program_dir = program_dir;
        r
    }

    /// Like with_program_dir, on the interpreter (diagnostic).
    pub fn with_program_dir_interpreter(program_dir: std::path::PathBuf) -> Self {
        let mut r = Self::new_interpreter();
        r.engine = Engine::Interp(Interpreter::with_program_dir(program_dir.clone()));
        // الدفعة 6: كما في with_program_dir.
        let settings_entries = read_settings(&program_dir).0;
        r.history = History::from_settings(&program_dir, &settings_entries);
        r.prompts = crate::prompts::PromptSettings::from_entries(&settings_entries);
        // المرحلة (ج): كما في with_program_dir.
        r.noclobber = settings_flag(&settings_entries, "منع_المسح");
        r.program_dir = program_dir;
        r
    }

    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.network_allowed = allowed;
        self.engine.set_network_allowed(allowed);
    }

    /// This session's own network permission (per-instance:
    /// one console's approval never leaks into another).
    pub fn network_allowed(&self) -> bool {
        self.engine.network_allowed()
    }

    /// Meeting 27: server capability (--اسمح-بالخادم).
    /// Default OFF; the network flag alone is not enough.
    pub fn set_server_allowed(&mut self, allowed: bool) {
        self.server_allowed = allowed;
        self.engine.set_server_allowed(allowed);
    }

    /// This session's own server permission (per-instance).
    pub fn server_allowed(&self) -> bool {
        self.engine.server_allowed()
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// (السطر يصل فور إنتاجه) — النمط نفسه كـ
    /// set_server_allowed؛ وreset() يعيده.
    pub fn set_live_output(&mut self, live: bool) {
        self.live_output = live;
        self.engine.set_live_output(live);
    }



    pub fn allow_local_network_host(&mut self, host: &str) {
        self.engine.allow_local_network_host(host);
    }

    /// رسالة الترحيب مفعّلة أم لا: تُقرأ من ملف الإعدادات
    /// الجديد (`~/إعدادات/niva.properties`، مع نقل القديم
    /// عند القراءة — اجتماع 45 §59، البند 36) — نفس صيغة
    /// التطبيق (سطور `مفتاح=قيمة`، `#` تعليق، فراغات
    /// تُهمل). المفتاح `الترحيب` بقيمة `لا` يوقفها؛
    /// وغياب الملف أو المفتاح أو أي قيمة أخرى يبقيها
    /// ظاهرة (الافتراضي: نعم). الملف قابل للتحرير من
    /// الترمنال نفسه.
    pub fn welcome_enabled(&self) -> bool {
        let (entries, _) = read_settings(&self.program_dir);
        for (key, value) in entries {
            if key == "الترحيب" {
                return value != "لا";
            }
        }
        true
    }

    /// محفوظات الجلسة (الدفعة 6: بنية `History` لا متجه
    /// سطور — نص + طابع لكل مدخل).
    pub fn history(&self) -> &History {
        &self.history
    }

    /// The control signal of the last fed line (audit م-6).
    pub fn last_control(&self) -> ReplControl {
        self.last_control
    }

    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): رمز آخر
    /// سطر مكتمل في هذه الجلسة.
    pub fn last_exit_code(&self) -> u8 {
        self.last_exit_code
    }

    /// الدفعة 1: رمز خروج عملية الجلسة — صريح من
    /// `:خروج [رمز]`، وإلا رمز آخر سطر مكتمل (EOF).
    pub fn quit_code(&self) -> u8 {
        self.quit_code.unwrap_or(self.last_exit_code)
    }

    /// The prompt to show now: the current directory,
    /// terminal style — `~ $ ` at home, `~/dir $ ` below it
    /// — or `> ` while a block is open (owner's orders
    /// 2026-11-05 ~22:58 and ~23:03: never `نيفا: `; `$`
    /// after the path, RTL display).
    /// الدفعة 7 (اجتماع 61 §76): القالبان من الإعدادات
    /// (`موجه_أول` و`موجه_استمرار`) ويُوسَّعان عند كل
    /// طباعة — فالمجلد والوقت والمستخدم أحياء.
    pub fn prompt(&self) -> String {
        let ctx = crate::prompts::PromptContext::current(&self.engine.current_dir_display());
        // المرحلة (ج): موجه الاستمرار أيضا أثناء جمع
        // مستند هنا (فالأسطر التالية جسمه لا سطر جديد).
        if self.pending.is_empty() && self.pending_heredoc.is_none() {
            crate::prompts::expand_prompt(&self.prompts.primary, &ctx)
        } else {
            crate::prompts::expand_prompt(&self.prompts.continuation, &ctx)
        }
    }

    /// إعدادات الموجهات والخطاف والمهلة لهذه الجلسة.
    pub fn prompts(&self) -> &crate::prompts::PromptSettings {
        &self.prompts
    }

    /// مهلة الخمول بالثواني (0 = بلا مهلة) — تقرأها
    /// حلقة `run_repl` قبل كل قراءة أمر.
    pub fn timeout_secs(&self) -> u64 {
        self.prompts.timeout_secs
    }

    /// نفّذ أمر ما قبل الموجه (الدفعة 7، الميزة 175)
    /// وأعد مخرجه ليُطبع قبل الموجه الأول — وللمضيفين
    /// (التطبيق وWASM) الطريقة نفسها: يستدعونها قبل
    /// كل موجه أول ويطبعون ما تعيده. بلا خطاف تعيد
    /// فراغا؛ وخطأ الخطاف يُطبع كما هو ولا يمنع الموجه
    /// ولا يمسّ رمز آخر سطر (يُحفظ `pending_code`
    /// ويُستعاد)؛ والاستدعاء المتداخل (خطاف يعيد
    /// دخوله) يُتخطى بصمت بعلم `in_prompt_hook`؛
    /// وسطور الخطاف لا تُسجَّل في المحفوظات (يُعلَّق
    /// التسجيل حول التنفيذ)؛ و`خروج()` داخل الخطاف لا ينهي
    /// الجلسة (مخرجه وحده يؤخذ).
    pub fn before_prompt(&mut self) -> String {
        if !self.prompts.has_hook() || self.in_prompt_hook {
            return String::new();
        }
        let hook = self.prompts.hook.clone();
        self.in_prompt_hook = true;
        let saved_code = self.pending_code.take();
        // سطور الخطاف لا تُسجَّل في المحفوظات — يُعلَّق
        // التسجيل حول التنفيذ (نمط المقتطفات المولَّدة
        // والخطاف القائم في §75)، فـ`eval` وحده يسجّل.
        self.history.suspend_recording();
        let (out, _ended) = self.eval_display(&hook);
        self.history.resume_recording();
        self.pending_code = saved_code;
        self.in_prompt_hook = false;
        // مع البث الحي يكون مخرج `اطبع` قد وصل فورا
        // أثناء التنفيذ ويعود فارغا من `eval` نفسه —
        // فلا مسح هنا (وإلا ابتُلع خطأ الخطاف).
        out
    }

    /// المرحلة (هـ) — الجمع والإشعار عند الموجه
    /// (اجتماع 65 §80): يجمع الوظائف الجارية التي
    /// انتهت (غير حاجب، عبر `try_wait`) ويعيد سطور
    /// الإشعار للمنتهية التي لم تُبلَّغ بعد — فارغة
    /// إن لم ينته شيء جديد. تُستدعى من حلقة `run_repl`
    /// قبل كل موجه أول، ومن معالجات عبارات المهام
    /// (فتحدَّث الحالات قبل الطباعة)، ومن الاختبارات.
    pub fn poll_jobs(&mut self) -> Vec<String> {
        crate::processes::poll_jobs()
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): اقتراحات
    /// الإكمال لجزء مكتوب — عبارات التفاعلي، والدوال
    /// المدمجة، والـ31 اختصارا (28 محركا + 3 تطبيقا:
    /// جل ومد واع)، وأسماء دوال الجلسة ومتغيراتها،
    /// وملفات المجلد الحالي (`/` بعد المجلدات)؛
    /// والتبويب يبقى مع التطبيق (الربط الجافا مؤجل).
    /// الجزء الفارغ يعيد كل المرشحات؛ والنتائج مزالة
    /// التكرار ومرتبة بايتيا.
    pub fn complete(&mut self, part: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // عبارات التفاعلي (الكلمة الأولى لكل عبارة
        // كاملة في try_full_phrase).
        for phrase in [
            "ابحث", "امسح", "حول", "ارجع", "كرر", "انتقل", "انسخ", "انقل", "موجود",
            "ادفع", "أخرج", "اعرض", "معلومات", "أعد-الإعدادات", "أعد-الضبط",
            "محفوظات", "حرر", "ملف_المحفوظات", "حد_المحفوظات", "حد_ملف_المحفوظات",
            "قواعد_المحفوظات", "تجاهل_المحفوظات", "صيغة_وقت_المحفوظات",
            "موجه_أول", "موجه_استمرار", "موجه_تتبع", "أمر_قبل_الموجه", "مهلة_الخمول",
            "منع_المسح", "توقف_عند_الخطأ", "وظائف", "قدم", "أخر", "اقتل", "انتظر",
            "أبعد", "مستعار", "مستعارات", "احذف_مستعار", "أكمل", "خيارات",
        ] {
            out.push(phrase.to_string());
        }
        // الدوال المدمجة.
        for name in crate::vm::all_builtin_names() {
            out.push(name.to_string());
        }
        // الاختصارات الـ31 (28 محركا + 3 تطبيقا).
        for code in [
            "ات", "مح", "شم", "حم", "حف", "اف", "شف", "عب", "عن", "عس", "عك", "عش",
            "حج", "شر", "اك", "ام", "تم", "عم", "اس", "اص", "رع", "تا", "وق", "نو",
            "اج", "عع", "نب", "اب", "جل", "مد", "اع",
        ] {
            out.push(code.to_string());
        }
        // أسماء دوال الجلسة ومتغيراتها.
        out.extend(self.engine.session_function_names());
        out.extend(self.engine.session_variable_names());
        // ملفات المجلد الحالي (`/` بعد المجلدات) —
        // والغائب أو غير المقروء يُتخطى بصمت.
        if let Ok(rd) = std::fs::read_dir(self.engine.cwd_path()) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    out.push(format!("{}/", name));
                } else {
                    out.push(name);
                }
            }
        }
        out.retain(|c| c.starts_with(part));
        out.sort();
        out.dedup();
        out
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): مسار
    /// `مصدر` الحرفي في سطر مستوى أعلى — أو لا شيء.
    /// يُستخدم لبذر المحلل فقط؛ فالمصدر داخل دالة أو
    /// شرط يُنفَّذ وقت استدعائه لا وقت التقييم.
    fn source_call_path(stmt: &crate::ast::Stmt) -> Option<String> {
        if let crate::ast::StmtKind::ExprStmt { expr } = &stmt.kind {
            if let crate::ast::ExprKind::Call { callee, args, kwargs: _, .. } = &expr.kind {
                if let crate::ast::ExprKind::Ident(name) = &callee.kind {
                    if name == "مصدر" && args.len() == 1 {
                        if let crate::ast::ExprKind::Str(path) = &args[0].kind {
                            return Some(path.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): بذر
    /// محلل الجلسة بتعريفات ملف `مصدر` — فالمحلل
    /// الدائم لا يرى إلا ما حُلل في التقييمات السابقة،
    /// وتعريفات مصدر وقت تشغيل فقط. يُحلَّل الملف (مع
    /// مصادره المتداخلة بعمق محدود) وتُتجاهل أخطاؤه —
    /// فخطأ وقت التشغيل الحقيقي يظهر عند تنفيذ مصدر
    /// نفسه. بلا بذر هنا يرفض التقييم التالي أسماء
    /// مصدر بـ«متغير غير معرف» رغم بقائها في الجلسة.
    /// البحث بالترتيب نفسه ($نيفا ثم ~ ثم المجلد
    /// الحالي)؛ والغائب أو غير المقروء يُتخطى بصمت.
    fn seed_source_file(&mut self, name: &str, depth: usize) {
        if depth > 8 {
            return;
        }
        let mut file_name = name.to_string();
        if !file_name.ends_with(".نيفا") {
            file_name.push_str(".نيفا");
        }
        let path = std::path::PathBuf::from(&file_name);
        let resolved = if path.is_absolute() {
            path
        } else {
            let cwd = self.engine.cwd_path();
            [
                self.program_dir.join(&file_name),
                self.program_dir.join("home").join(&file_name),
                cwd.join(&file_name),
            ]
            .into_iter()
            .find(|c| c.is_file())
            .unwrap_or_else(|| self.program_dir.join(&file_name))
        };
        let source = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(_) => return,
        };
        let tokens = match crate::lexer::tokenize_named(&source, &file_name) {
            Ok(t) => t,
            Err(_) => return,
        };
        let program = match crate::parser::Parser::named(tokens, &file_name).parse_program()
        {
            Ok(p) => p,
            Err(_) => return,
        };
        for stmt in &program {
            if let Some(inner) = Self::source_call_path(stmt) {
                self.seed_source_file(&inner, depth + 1);
            }
        }
        let _ = self.analyzer.analyze_program(&program);
    }

    /// Evaluate a complete snippet and render its result as
    /// display text: stdout output, with any error appended. An
    /// Incomplete result means the block never completed, so it
    /// is dropped with the cancellation message.
    /// الدفعة 1 (اجتماع 54 §69): تعيد (النص، انتهت؟) —
    /// وانتهت صحيحة حين استدعى المقتطف `خروج(رمز)`؛
    /// وتسجّل رمز السطر في pending_code (0 نجاح، أو
    /// رمز الفشل/الخروج) — والسطور غير المكتملة لا
    /// تمسّه، فيبقى رمز آخر سطر مكتمل.
    fn eval_display(&mut self, src: &str) -> (String, bool) {
        match self.eval(src) {
            ReplEval::Done { output } => {
                self.pending_code = Some(0);
                (output, false)
            }
            ReplEval::Failed { output, error, code } => {
                self.pending_code = Some(code);
                (format!("{}{}\n", output, error), false)
            }
            ReplEval::Exited { output, code } => {
                self.pending_code = Some(code);
                (output, true)
            }
            ReplEval::Incomplete => ("ألغيت الكتلة غير المكتملة.\n".to_string(), false),
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): ملف
    /// الخروج `~/خروج.نيفا` — عند الخروج التفاعلي
    /// الطبيعي فقط، مرة واحدة في الجلسة؛ وأخطاؤه
    /// تُطبع ولا تمنع الخروج؛ وخروج الإشعار (من التطبيق
    /// مباشرة بلا feed/eof) لا يشغّله أصلا.
    fn run_exit_file(&mut self) -> String {
        if self.exit_file_ran {
            return String::new();
        }
        self.exit_file_ran = true;
        let path = self.program_dir.join("home").join("خروج.نيفا");
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        // سطور ملف الخروج لا تُسجَّل في المحفوظات —
        // كسطور ملفات البدء والخطاف.
        self.history.suspend_recording();
        let (out, _ended) = self.eval_display(&source);
        self.history.resume_recording();
        out
    }

    /// Run a parsed command line and return its display output.
    /// The line must already parse as a command.
    fn handle_command(&mut self, line: &str) -> ReplOutput {
        match parse_command(line) {
            Some(ReplCommand::Help) => {
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: help_text().to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::Show) => {
                self.pending_code = Some(0);
                ReplOutput {
                    output: self.show_history(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::ClearHistory) => {
                // Task 2 (Meeting 25 plan): clear ONLY the
                // :اعرض lines. The interpreter, the analyzer
                // and the variables all stay — unlike :مسح,
                // which resets the whole session. No control
                // signal: the app intercepts this command
                // itself and clears its own arrow history.
                self.history.clear();
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: "مسحت محفوظات الجلسة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::Clear) => {
                self.reset();
                // Audit م-6: the host clears its arrow history.
                self.last_control = ReplControl::SessionCleared;
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: "بدأت جلسة جديدة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::AllowNetwork) => {
                // Audit م-2: flip the network flag inside the
                // running process — no restart, no lost session
                // state. The app sends this line itself when the
                // user allows the network; it never appears in
                // the terminal text or in :اعرض.
                self.set_network_allowed(true);
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::AllowServer) => {
                // Meeting 27: the same in-process flip for the
                // server flag — the network flag alone is never
                // enough. The app sends this line itself when
                // the user allows the server.
                self.set_server_allowed(true);
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::SetDownloads(path)) => {
                // Meeting 50 §64: set the read-only downloads
                // root inside the running process — no restart,
                // no lost session state. The app sends this line
                // itself when storage permission is granted; it
                // never appears in the terminal text or in
                // :اعرض. A bad path is an error line, and
                // nothing changes.
                let output = match self.set_downloads(path) {
                    // الدفعة 1: نجاح 0، ومسار سيئ 1.
                    Ok(()) => {
                        self.pending_code = Some(0);
                        String::new()
                    }
                    Err(msg) => {
                        self.pending_code = Some(1);
                        format!("{}\n", msg)
                    }
                };
                ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Some(ReplCommand::Quit(arg)) => {
                // الدفعة 1 (اجتماع 54 §69): بلا معامل →
                // رمز آخر سطر؛ ومع معامل صحيح → رمزه؛
                // والسيئ (غير عدد، أو خارج 0–255، أو
                // أكثر من معامل) خطأ برمز 2 والجلسة تستمر.
                match arg.as_deref() {
                    None => {
                        // رمز آخر سطر مكتمل — وإن سبقه في
                        // هذا السطر نفسه إكمال كتلة معلقة،
                        // فرمزه مسجَّل في pending_code لا
                        // في last_exit_code بعد (feed لم
                        // تثبّته بعد).
                        let code = self.pending_code.unwrap_or(self.last_exit_code);
                        self.quit_code = Some(code);
                        // الدفعة 1: رمز الخروج هو رمز آخر
                        // سطر — يُثبَّت كآخر رمز أيضا.
                        self.pending_code = Some(code);
                        ReplOutput {
                            output: String::new(),
                            prompt: self.prompt(),
                            ended: true,
                            needs_network: false,
                        }
                    }
                    Some(raw) => {
                        let parts: Vec<&str> = raw.split_whitespace().collect();
                        if parts.len() > 1 {
                            self.pending_code = Some(2);
                            return ReplOutput {
                                output: "خروج تتوقع معاملا واحدا على الأكثر\n".to_string(),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            };
                        }
                        match parse_exit_code_arg(parts[0]) {
                            Some(code) => {
                                self.quit_code = Some(code);
                                // الدفعة 1: رمز الخروج الصريح
                                // يُثبَّت كآخر رمز أيضا.
                                self.pending_code = Some(code);
                                ReplOutput {
                                    output: String::new(),
                                    prompt: self.prompt(),
                                    ended: true,
                                    needs_network: false,
                                }
                            }
                            None => {
                                self.pending_code = Some(2);
                                ReplOutput {
                                    output: "رمز الخروج يجب أن يكون عددا صحيحا بين ٠ و٢٥٥\n"
                                        .to_string(),
                                    prompt: self.prompt(),
                                    ended: false,
                                    needs_network: false,
                                }
                            }
                        }
                    }
                }
            }
            None => ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            },
        }
    }

    /// Try a short terminal command (ات، مح، نم، شم، حم، حف،
    /// سم، قم، اف). Returns Some(output) when the first word is
    /// a shortcut code — including an arity error — and None
    /// when the line is not a shortcut, so it falls through
    /// to normal code. Only at the top level (never inside a
    /// pending block, where the line is block content).
    fn try_shortcut(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let code = words.next()?;
        let def = shortcut_def(code)?;
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        if args.len() < def.min || args.len() > def.max {
            let want = if def.min == def.max {
                if def.min == 0 {
                    "بدون معاملات".to_string()
                } else {
                    format!("{} معاملات", def.min)
                }
            } else if def.max == usize::MAX {
                format!("{} معاملات على الأقل", def.min)
            } else {
                format!("{} إلى {} معاملات", def.min, def.max)
            };
            // الدفعة 1: خطأ عدد المعاملات → سوء استخدام (2).
            self.pending_code = Some(2);
            return Some(ReplOutput {
                output: format!("{} تتوقع {} لكن أعطيت {}\n", code, want, args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        // Successful shortcut lines join the session history,
        // like code lines do, so :اعرض shows what was typed.
        // Session commands (اس) are control, not
        // content — like the :commands, they are not recorded.
        if !matches!(def.kind, ShortcutKind::Command) {
            self.history.record(line);
        }
        match def.kind {
            ShortcutKind::Command => Some(self.handle_command(def.key)),
            ShortcutKind::Snippet => {
                let snippet = match def.key {
                    "عشوائي" => "استورد عشوائي\nعشوائي.عدد_عشوائي()".to_string(),
                    "وقت" => "استورد وقت\nوقت.الآن()".to_string(),
                    "نوم" => match parse_shortcut_number(&args[0]) {
                        Some(n) => format!("استورد وقت\nوقت.نوم({})", n),
                        None => {
                            // الدفعة 1: معامل غير عدد →
                            // سوء استخدام (2).
                            self.pending_code = Some(2);
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع عددا، وليس «{}»\n", code, args[0]),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    },
                    "اجلب" => {
                        let escaped = args[0].replace('\\', "\\\\").replace('"', "\\\"");
                        format!("استورد شبكة\nشبكة.اجلب(\"{}\")", escaped)
                    }
                    "الحالية" => "استورد عمليات\nعمليات.الحالية()".to_string(),
                    // The program + its arguments join into the
                    // one string عمليات.أنشئ takes (split on
                    // spaces, no shell) — no comma anywhere.
                    "أنشئ_برنامج" => {
                        let joined = args.join(" ");
                        let escaped = joined.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("استورد عمليات\nعمليات.أنشئ(\"{}\")", escaped)
                    }
                    "أوقف_برنامج" => match parse_shortcut_number(&args[0]) {
                        Some(n) => format!("استورد عمليات\nعمليات.أوقف({})", n),
                        None => {
                            // الدفعة 1: معامل غير عدد →
                            // سوء استخدام (2).
                            self.pending_code = Some(2);
                            return Some(ReplOutput {
                                output: format!("خطأ: {} تتوقع عددا، وليس «{}»\n", code, args[0]),
                                prompt: self.prompt(),
                                ended: false,
                                needs_network: false,
                            });
                        }
                    },
                    _ => unreachable!("unknown snippet key"),
                };
                // A snippet whose value is لاشيء prints nothing,
                // like the other shortcuts do (Nil -> empty).
                // الدفعة 6: المقتطف المولَّد لا يُسجَّل —
                // السطر المكتوب وحده في المحفوظات.
                self.history.suspend_recording();
                let (out, snippet_ended) = self.eval_display(&snippet);
                self.history.resume_recording();
                let out = if out == "لاشيء\n" { String::new() } else { out };
                Some(ReplOutput {
                    output: out,
                    prompt: self.prompt(),
                    ended: snippet_ended,
                    needs_network: false,
                })
            }
            ShortcutKind::RestJoined => {
                let values = vec![
                    Value::Str(args[0].clone()),
                    Value::Str(args[1..].join(" ")),
                ];
                match self.engine.call_builtin(def.key, values, 1, 1) {
                    Ok(value) => {
                        // الدفعة 1: نجاح 0.
                        self.pending_code = Some(0);
                        Some(ReplOutput {
                            output: format_shortcut_output(&value),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        })
                    }
                    Err(e) => {
                        // الدفعة 1: خطأ تشغيل 1.
                        self.pending_code = Some(1);
                        Some(ReplOutput {
                            output: format!("{}\n", e),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        })
                    }
                }
            }
            ShortcutKind::StrArgs => {
                let values: Vec<Value> = args.into_iter().map(Value::Str).collect();
                match self.engine.call_builtin(def.key, values, 1, 1) {
                    Ok(value) => {
                        // الدفعة 1: نجاح 0.
                        self.pending_code = Some(0);
                        Some(ReplOutput {
                            output: format_shortcut_output(&value),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        })
                    }
                    Err(e) => {
                        // الدفعة 1: خطأ تشغيل 1.
                        self.pending_code = Some(1);
                        Some(ReplOutput {
                            output: format!("{}\n", e),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        })
                    }
                }
            }
        }
    }

    /// Full-name terminal command (تثبيت): a whole Arabic
    /// word, not a two-letter code — so it never enters the
    /// 31-code table (the table stays 31, and the app has no
    /// code for it either). Like the Snippet shortcuts, it
    /// builds a builtin call and evaluates it; the typed line
    /// joins the history, like code lines do. Only at the top
    /// level, like try_shortcut — never inside a pending block.
    fn try_full_command(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let first = words.next()?;
        if first != "تثبيت" {
            return None;
        }
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        if args.len() != 1 {
            // الدفعة 1: خطأ عدد المعاملات → سوء استخدام (2).
            self.pending_code = Some(2);
            return Some(ReplOutput {
                output: format!("تثبيت تتوقع 1 معاملات لكن أعطيت {}\n", args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        self.history.record(line);
        let escaped = args[0].replace('\\', "\\\\").replace('"', "\\\"");
        let snippet = format!("تثبيت(\"{}\")", escaped);
        // A snippet whose value is لاشيء prints nothing,
        // like the other shortcuts do (Nil -> empty).
        // الدفعة 6: المقتطف المولَّد لا يُسجَّل (كالاختصارات).
        self.history.suspend_recording();
        let (out, snippet_ended) = self.eval_display(&snippet);
        self.history.resume_recording();
        let out = if out == "لاشيء\n" { String::new() } else { out };
        Some(ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: snippet_ended,
            needs_network: false,
        })
    }

    /// `حدث حزم` (أمر المالك 2026-11-10 ~03:07، قاعدة
    /// مؤكدة): أي مستخدم يكتبها في الموجه تُحدَّث حزم
    /// نيفا مباشرة عبر موزّع المدير — بلا برنامج ولا
    /// استيراد من المستخدم. العبارة تبني مقتطفا يستورد
    /// المدير (المدمج في المحرك؛ وملف على القرص يسبقه
    /// دائما) وينفّذ `أوامر.نفّذ(["حدث", "حزم"])` ويطبع
    /// نص النجاح أو رسالة الخطأ العربية. المقتطف
    /// المولَّد لا يُسجَّل (كالاختصارات)، والسطر المكتوب
    /// نفسه يُسجَّل كغيره.
    fn run_update_packages_phrase(&mut self) -> ReplOutput {
        self.history.suspend_recording();
        let snippet = "استورد أوامر\nنتيجة_حدث_حزم = أوامر.نفّذ([\"حدث\", \"حزم\"], لاشيء, لاشيء)\nإذا نتيجة_حدث_حزم[\"نجاح\"]:\n  اطبع نتيجة_حدث_حزم[\"نص\"]\nوإلا:\n  اطبع نتيجة_حدث_حزم[\"خطأ\"]";
        let (out, ended) = self.eval_display(snippet);
        self.history.resume_recording();
        ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended,
            needs_network: false,
        }
    }

    /// أمر تفاعلي `شغّل <اسم>` (اجتماع 43 §58 + تجاوز
    /// المالك «بلا تعقيد»): يشغّل ملف أداة من مجلد
    /// البرنامج كبرنامج — الاسم العاري بلا امتداد يُلحق
    /// به `.نيفا` تلقائيا (`شغّل مثال-أول`)، والاسم
    /// الكامل بالامتداد كما هو (`شغّل مثال-أول.نيفا`)،
    /// ولا خطأ على الصيغتين. والاسم القانوني بلا تشكيل
    /// (`شغل`) كما في باقي أوامر CLI — والصيغة المشكولة
    /// (`شغّل`) مقبولة توافقا فقط عبر normalize_cli_word
    /// (قاعدة المحلل نفسها). تفاعلي فقط — لا دالة
    /// برنامجية (من استدعاه من الكود يأخذ خطأ الاسم غير
    /// المعرف المعتاد). فقط في المستوى الأعلى، كـ
    /// try_full_command.
    /// مصدر الفهرس الافتراضي (كما في حزم — مخزن.نيفا):
    /// سطر «مصدر الفهرس» في `معلومات`.
    const DEFAULT_INDEX_URL: &str =
        "https://raw.githubusercontent.com/RIO4140/niva-index/main/فهرس.حزمة";

    /// `معلومات` (اجتماع 45 §59، البندان 18 و78): خمسة
    /// أسطر بالضبط، بلا نسخ للحافظة.
    fn info_lines(&self) -> String {
        // الوحدات المثبتة: ملفات .نيفا في جذر مجلد
        // البرنامج (ما يثبّته `تثبيت`) — لا وحدات المخزن
        // (لحزم أمرها الخاص).
        let installed = std::fs::read_dir(&self.program_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|x| x == "نيفا")
                            .unwrap_or(false)
                            && e.metadata().map(|m| m.is_file()).unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        let terminal = match terminal_version() {
            Some(t) => eastern_version(&t),
            None => "غير معروف".to_string(),
        };
        let network = if self.network_connected() {
            "متصلة"
        } else {
            "غير متصلة"
        };
        format!(
            "محرك نيفا {}\nترمنال التطبيق: {}\nالوحدات المثبتة: {}\nالشبكة: {}\nمصدر الفهرس: {}\n",
            version(),
            terminal,
            Value::Number(installed as f64).fmt_value(),
            network,
            Self::DEFAULT_INDEX_URL
        )
    }

    /// الشبكة متصلة أم لا: بلا سماح بالشبكة فالجواب
    /// «غير متصلة» حتما (بلا فحص)؛ ومع السماح فحص خفيف:
    /// اتصال TCP قصير (3 ثوان) بمضيف مصدر الفهرس — أي
    /// اتصال ناجح يعني «متصلة»، والفشل يعني انقطاعا.
    fn network_connected(&self) -> bool {
        if !self.network_allowed() {
            return false;
        }
        use std::net::ToSocketAddrs;
        let Ok(mut addrs) = ("raw.githubusercontent.com", 443).to_socket_addrs() else {
            return false;
        };
        let Some(addr) = addrs.next() else {
            return false;
        };
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)).is_ok()
    }

    /// `أعد-الإعدادات` (اجتماع 45 §59، البند 19): أعد
    /// قراءة niva.properties بلا إعادة تشغيل، وأرسل إشارة
    /// `إعدادات` للتطبيق ليعيد التطبيق الحي؛ وسطر تأكيد
    /// واحد يسمّي المفاتيح التي تحتاج إعادة تشغيل.
    fn reload_settings(&mut self) -> ReplOutput {
        let (entries, notices) = read_settings(&self.program_dir);
        // الدفعة 7 (اجتماع 61 §76): إعدادات الموجهات
        // والخطاف والمهلة تُعاد قراءتها حية مع باقي
        // الإعدادات — وتسري من الموجه التالي.
        self.prompts = crate::prompts::PromptSettings::from_entries(&entries);
        self.last_control = ReplControl::SettingsReload;
        // المفاتيح بلا مسار تطبيق حي (من قراءة loadSettings):
        // تُسمّى في الرسالة بأنها تحتاج إعادة تشغيل؛
        // والترحيب يسري من الجلسة الجديدة.
        let mut out = String::from(
            "أُعيدت قراءة الإعدادات وطُبّقت — وتحتاج إعادة تشغيل: إخفاء_لوحة_المفاتيح_عند_البدء، خط_مخصص، ملء_الشاشة؛ والترحيب يسري من الجلسة الجديدة\n",
        );
        for n in notices {
            out.push_str(&n);
            out.push('\n');
        }
        // الدفعة 1: أمر جلسة ناجح رمزه 0.
        self.pending_code = Some(0);
        ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// `أعد-الضبط` — الخطوة الأولى (اجتماع 45 §59، البند
    /// 19): رسالة تسمّي ما سيُمسح وما يبقى، وتطلب كتابة
    /// «نعم» حرفيا؛ وأي سطر آخر يلغي.
    fn ask_reset(&mut self) -> ReplOutput {
        self.pending_reset = true;
        // الدفعة 1: طلب تأكيد — السطر نفسه ناجح (0)؛
        // والتنفيذ الفعلي رمزه في do_reset.
        self.pending_code = Some(0);
        // الجلسات المحفوظة إن كانت في مجلد البرنامج
        // (session-* في الجذر) — تُسمّى بعددها.
        let sessions = std::fs::read_dir(&self.program_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("session-"))
                    .count()
            })
            .unwrap_or(0);
        let sessions_part = if sessions > 0 {
            format!(
                "، ومنها {} جلسة محفوظة",
                Value::Number(sessions as f64).fmt_value()
            )
        } else {
            String::new()
        };
        ReplOutput {
            output: format!(
                "سيُمسح مجلد البرنامج كاملا ما عدا البيت (~) — الوحدات المثبتة والملفات الداخلية{}؛ وتبقى ملفاتك في البيت (~) والإعدادات في ~/إعدادات/. اكتب نعم حرفيا للتأكيد، وأي شيء آخر يلغي.\n",
                sessions_part
            ),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// `أعد-الضبط` — الخطوة الثانية بعد «نعم»: امسح مجلد
    /// البرنامج كاملا ما عدا home/، وأعد بناء المشتقات في
    /// الخطوة نفسها (البند 52)، وأعد مجلد الجلسة إلى البيت.
    /// الإعدادات تعيش في home/ فلا تُمس.
    fn do_reset(&mut self) -> ReplOutput {
        let policy = crate::permissions::FilePolicy::restricted(self.program_dir.clone());
        let mut failed: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_name() == "home" {
                    continue;
                }
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if res.is_err() {
                    failed.push(policy.display_path(&path));
                }
            }
        }
        // (52) إعادة بناء المشتقات في الخطوة نفسها.
        let _ = std::fs::create_dir_all(self.program_dir.join("tmp"));
        let _ = std::fs::create_dir_all(self.program_dir.join(".نيفا"));
        // الجلسة تعود إلى البيت (مجلدها قد يكون في
        // المنطقة الممسوحة).
        let _ = self
            .engine
            .call_builtin("انتقل", vec![Value::Str("~".to_string())], 1, 1);
        // سلامة الإعدادات بعد المسح (تعيش في home/):
        // الملف غائب (افتراضي) أو مقروء = سليم.
        let settings_ok = match std::fs::read_to_string(&settings_path(&self.program_dir)) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        let mut out = if failed.is_empty() {
            "أُعيد ضبط مجلد البرنامج — وبقي البيت (~) كما هو، ومجلد الجلسة الآن البيت.".to_string()
        } else {
            format!(
                "أُعيد ضبط مجلد البرنامج جزئيا — تعذّر مسح: {}.",
                failed.join("، ")
            )
        };
        if !settings_ok {
            out.push_str(" تنبيه: ملف الإعدادات غير قابل للقراءة بعد المسح.");
        }
        out.push('\n');
        // الدفعة 1: نجاح كامل 0، وجزئي (تعذّر مسح) 1.
        self.pending_code = Some(if failed.is_empty() { 0 } else { 1 });
        ReplOutput {
            output: out,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    fn try_run_command(&mut self, line: &str) -> Option<ReplOutput> {
        let mut words = line.split_whitespace();
        let first = words.next()?;
        let word = normalize_cli_word(first);
        // الدفعة 3 (اجتماع 57 §72، الميزة 51): `استبدل`
        // تشغّل الأداة ثم تنهي الجلسة برمزها بلا عودة.
        let exec = word == "استبدل";
        if word != "شغل" && !exec {
            return None;
        }
        let args: Vec<String> = words.map(|w| w.to_string()).collect();
        let prompt = self.prompt();
        let bad = |msg: &str| ReplOutput {
            output: format!("{}\n", msg),
            prompt: prompt.clone(),
            ended: false,
            needs_network: false,
        };
        if args.len() != 1 {
            // الدفعة 1: وسائط خاطئة → سوء استخدام (2).
            self.pending_code = Some(2);
            return Some(bad(if exec {
                "استبدل تتوقع معاملا واحدا"
            } else {
                "شغّل تتوقع معاملا واحدا"
            }));
        }
        let name = &args[0];
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            // الدفعة 1: وسائط خاطئة → سوء استخدام (2).
            self.pending_code = Some(2);
            return Some(bad(if exec {
                "استبدل يقبل اسم ملف بلا مسار"
            } else {
                "شغّل يقبل اسم ملف بلا مسار"
            }));
        }
        self.history.record(line);
        Some(self.run_program_file(name, exec))
    }

    /// الخطاف (الدفعة 3، الميزة 53): عند غياب الأداة
    /// في `شغل`/`استبدل`، إن عرّف المستخدم
    /// `عند_غياب_الامر(اسم)` في الجلسة (ومنها
    /// ~/بدء.نيفا)، استدعها بالاسم كما كُتب؛ ومخرجها
    /// ورمزها هما مخرج السطر ورمزه (وخروج داخلها ينهي
    /// الجلسة برمزها، كأي سطر). الحارس يمنع التداخل.
    /// تعيد Some(...) حين نفّذت الخطاف، وNone حين لا
    /// خطاف معرّفا — فيتولى المتصل 127 العادية.
    fn try_not_found_hook(&mut self, name: &str) -> Option<ReplOutput> {
        if self.in_not_found_hook {
            return None;
        }
        if !self.analyzer.is_defined("عند_غياب_الامر") {
            return None;
        }
        // الاسم يبنى داخل مقتطف نصي — علامة اقتباس أو
        // سطر جديد فيه تكسره، فيُتخطى الخطاف إلى 127.
        if name.contains('"') || name.contains('\n') || name.contains('\r') {
            return None;
        }
        self.in_not_found_hook = true;
        let before = self.history.len();
        // الاستدعاء ملفوف في `اذا صحيح:` — فالاستدعاء
        // المجرد تعبير أخير يطبع eval قيمة إرجاعه
        // (`لاشيء`) ويبتلع ما طبعه الخطاف إلى السطر
        // التالي؛ أما كبيان داخل الشرط فيُشغَّل لأثره
        // فيكون مخرج السطر ما طبعه الخطاف وحده.
        let (output, ended) = self.eval_display(&format!(
            "اذا صحيح:\n    عند_غياب_الامر(\"{}\")",
            name
        ));
        // eval دفع المقتطف إلى المحفوظات — الخطاف لا
        // يظهر في :اعرض ولا في أسهم المحفوظات.
        self.history.truncate(before);
        self.in_not_found_hook = false;
        Some(ReplOutput {
            output,
            prompt: self.prompt(),
            ended,
            needs_network: false,
        })
    }

    /// اقرأ ملف `.نيفا` — من مجلد البرنامج أولا (المثبَّت)،
    /// ثم من البيت (اجتماع 45 §59، البند 35: المثبَّت
    /// يفوز عند التعارض) — وشغّله كبرنامج في جلسة جديدة
    /// على محرك التفاعلي الحالي (افتراضي أو مفسر) — جلسة
    /// التفاعلي نفسها لا تُمس ولا يتسرب شيء في أي اتجاه.
    /// المخرجات تُبث حية (أو تُجمع في ناتج الإرجاع حين
    /// يكون البث معطلا كما في الاختبارات)؛ وخطأ وقت
    /// التشغيل يُطبع بالعربية ويعود التحكم إلى الموجه
    /// بلا خروج من التفاعلي.
    /// الدفعة 3 (اجتماع 57 §72، الميزة 39): البحث عبر
    /// find_tool_file — المجلد وغير المقروء في `$نيفا`
    /// يُتخطيان إلى البيت، وأول عائق يحدد الرسالة
    /// والرمز حين لا يوجد تنفيذي. والغائب يستدعي
    /// الخطاف عند تعريفه. `exec` (الميزة 51) ينهي
    /// الجلسة برمز الأداة عند وجودها فقط.
    fn run_program_file(&mut self, name: &str, exec: bool) -> ReplOutput {
        let file_name = if name.ends_with(".نيفا") {
            name.to_string()
        } else {
            format!("{}.نيفا", name)
        };
        let lookup = crate::files::find_tool_file(&self.program_dir, &file_name);
        let path = match lookup {
            crate::files::ToolLookup::Found { path, .. } => path,
            crate::files::ToolLookup::IsDir => {
                // الدفعة 1 (اجتماع 54 §69): مجلد كبرنامج
                // 126 برسالة تميّزه.
                self.pending_code = Some(126);
                return ReplOutput {
                    output: format!("{} مجلد وليس ملفا\n", file_name),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            crate::files::ToolLookup::Unreadable => {
                // الدفعة 1: ملف غير مقروء 126.
                self.pending_code = Some(126);
                return ReplOutput {
                    output: format!("تعذر قراءة الملف: {}\n", file_name),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            crate::files::ToolLookup::Missing => {
                // الدفعة 3: الخطاف عند الغياب فقط.
                if let Some(hook_out) = self.try_not_found_hook(name) {
                    return hook_out;
                }
                // الدفعة 1: الغائب 127.
                self.pending_code = Some(127);
                return ReplOutput {
                    output: format!(
                        "الملف {} غير موجود — بحثت في $نيفا وفي ~\n",
                        file_name
                    ),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
        };
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                // الدفعة 1: ملف غير مقروء 126.
                self.pending_code = Some(126);
                return ReplOutput {
                    output: format!("تعذر قراءة الملف: {}\n", file_name),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
        };
        // الدفعة 1: خطأ لغوي/نحوي/دلالي → سوء استخدام (2).
        let mut fail = |e: String| {
            self.pending_code = Some(2);
            ReplOutput {
                output: format!("{}\n", e),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            }
        };
        let tokens = match tokenize(&source) {
            Ok(t) => t,
            Err(e) => return fail(e.to_string()),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return fail(e.to_string()),
        };
        // محلل دلالي جديد للبرنامج — لا محلل الجلسة —
        // فتعريفات البرنامج لا تتسرب إلى التفاعلي.
        let mut analyzer = SemanticAnalyzer::new();
        if let Err(e) = analyzer.analyze_program(&program) {
            return fail(e.to_string());
        }
        // جلسة جديدة على محرك التفاعلي الحالي — بأعلام
        // الجلسة نفسها (موثوق/شبكة/خادم/بث حي)، فالبرنامج
        // يعمل كما يعمل `نيفا <ملف>` بالأعلام نفسها.
        let (output, code) = match &self.engine {
            Engine::Vm(_) => {
                let mut session = VmSession::new(self.program_dir.clone());
                session.set_trusted(self.trusted);
                session.set_network_allowed(self.network_allowed);
                session.set_server_allowed(self.server_allowed);
                session.set_live_output(self.live_output);
                session.set_source_file(&file_name);
                match session.run(&program) {
                    // الدفعة 1: رمز خروج البرنامج الفرعي،
                    // أو 0 حين انتهى بلا خروج.
                    Ok(()) => (
                        session.take_output(),
                        session.take_exit_code().unwrap_or(0),
                    ),
                    Err(e) => {
                        let mut out = session.take_output();
                        out.push_str(&format!("{}\n", e));
                        // الدفعة 1 (§69 ممتد): خطأ وقت تشغيل
                        // غير ممسوك في البرنامج الفرعي → 1،
                        // إلا وحدة غائبة → 127.
                        (out, runtime_error_code(&e))
                    }
                }
            }
            Engine::Interp(_) => {
                let mut interp =
                    Interpreter::with_program_dir(self.program_dir.clone());
                interp.set_trusted(self.trusted);
                interp.set_network_allowed(self.network_allowed);
                interp.set_server_allowed(self.server_allowed);
                interp.set_live_output(self.live_output);
                interp.set_source_file(&file_name);
                match interp.run(&program) {
                    Ok(()) => (
                        std::mem::take(&mut interp.output),
                        interp.take_exit_code().unwrap_or(0),
                    ),
                    Err(e) => {
                        let mut out = std::mem::take(&mut interp.output);
                        out.push_str(&format!("{}\n", e));
                        // الدفعة 1 (§69 ممتد): وحدة غائبة
                        // 127، وخطأ تشغيل آخر → 1.
                        (out, runtime_error_code(&e))
                    }
                }
            }
        };
        self.pending_code = Some(code);
        if exec {
            // الميزة 51: الاستبدال ينهي الجلسة برمز
            // الأداة — بلا عودة إلى الموجه.
            self.quit_code = Some(code);
        }
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: exec,
            needs_network: false,
        }
    }

    /// ملف البدء والمقتطفات (اجتماع 45 §59، البنود 44 و45
    /// و46 و111): `~/بدء.نيفا` ثم `~/مقتطفات/*.نيفا`
    /// أبجديا، عند دخول التفاعلي فقط — لا في تشغيل ملف
    /// ولا في `شغّل`. الخطأ في ملف لا يمنع الدخول أبدا:
    /// يُطبع الخطأ باسم الملف، ويكمل الباقي، ثم يظهر
    /// الموجه. التعريفات تبقى في الجلسة.
    fn run_startup_files(&mut self) -> String {
        let home = self.program_dir.join("home");
        let mut out = String::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        // المرحلة (د) — الإكمال (اجتماع 68 §83):
        // `--بلا-بدء` يتخطى بدء.نيفا والمقتطفات؛
        // وملف `--ملف-بدء` الصريح يبقى ولو معه.
        if !self.skip_startup {
            let start = home.join("بدء.نيفا");
            if start.is_file() {
                files.push(("~/بدء.نيفا".to_string(), start));
            }
            let snippets = home.join("مقتطفات");
            if let Ok(entries) = std::fs::read_dir(&snippets) {
                let mut names: Vec<(String, PathBuf)> = entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|x| x == "نيفا")
                            .unwrap_or(false)
                            && e.metadata().map(|m| m.is_file()).unwrap_or(false)
                    })
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        (format!("~/مقتطفات/{}", name), e.path())
                    })
                    .collect();
                names.sort_by(|a, b| a.0.cmp(&b.0));
                files.extend(names);
            }
        }
        // المرحلة (د) — الإكمال (اجتماع 68 §83): ملف
        // البدء الإضافي من `--ملف-بدء` — بعد المعتادة،
        // والنسبي نسبة إلى مجلد البرنامج؛ والغائب
        // يُبلَّغ في الحلقة كغيره ولا يمنع الدخول.
        if let Some(extra) = self.startup_file.clone() {
            let path = if extra.is_absolute() {
                extra.clone()
            } else {
                self.program_dir.join(&extra)
            };
            let display = extra.to_string_lossy().to_string();
            files.push((display, path));
        }
        // الدفعة 6: سطور ملفات البدء ليست سطورا مكتوبة —
        // لا تُسجَّل في المحفوظات (تُعلَّق أثناء الحلقة كلها).
        self.history.suspend_recording();
        for (display, path) in files {
            match std::fs::read_to_string(&path) {
                Ok(source) => match self.eval(&source) {
                    ReplEval::Done { output } => out.push_str(&output),
                    ReplEval::Failed { output, error, code } => {
                        // الدفعة 1: رمز الفشل يُثبَّت
                        // كآخر رمز، ولا يمنع باقي الملفات.
                        self.last_exit_code = code;
                        self.engine.set_last_exit_code(code);
                        out.push_str(&output);
                        out.push_str(&format!("خطأ في {}: {}\n", display, error));
                    }
                    ReplEval::Exited { output, code } => {
                        // الدفعة 1: خروج في ملف بدء —
                        // يُثبَّت رمزه ويكمل باقي الملفات.
                        self.last_exit_code = code;
                        self.engine.set_last_exit_code(code);
                        out.push_str(&output);
                    }
                    ReplEval::Incomplete => {
                        out.push_str(&format!("خطأ في {}: الملف غير مكتمل\n", display));
                    }
                },
                Err(_) => {
                    out.push_str(&format!("تعذّر قراءة الملف: {}\n", display));
                }
            }
        }
        self.history.resume_recording();
        out
    }

    /// استدعاء دالة مدمجة بمعاملات نصية، لعبارات كاملة:
    /// مثل ذراع StrArgs في try_shortcut — بلا مقتطف
    /// وبلا فصلة في أي مكان.
    fn call_str_builtin(&mut self, key: &str, args: Vec<String>) -> ReplOutput {
        let values: Vec<Value> = args.into_iter().map(Value::Str).collect();
        match self.engine.call_builtin(key, values, 1, 1) {
            Ok(value) => {
                // الدفعة 1: نجاح 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: format_shortcut_output(&value),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Err(e) => {
                // الدفعة 1: خطأ تشغيل 1.
                self.pending_code = Some(1);
                ReplOutput {
                    output: format!("{}\n", e),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
        }
    }

    /// استدعاء مدمج بمعامل واحد يُحلَّل عددا أولا (أرقام
    /// عربية + ٫) فيمرَّر عددا، وإلا مرَّر النص كما هو —
    /// سلوك حع القديم في «حول إلى عدد»: عدد(نص) لا تقبل
    /// ٫، فبدون هذا التحويل ينكسر «حول إلى عدد ٣٫٥»
    /// الذي كان يعمل. (إصلاح من التحقق المستقل 2026-11-08.)
    fn call_num_or_str_builtin(&mut self, key: &str, arg: String) -> ReplOutput {
        let value = match parse_shortcut_number(&arg) {
            Some(n) => Value::Number(n),
            None => Value::Str(arg),
        };
        match self.engine.call_builtin(key, vec![value], 1, 1) {
            Ok(value) => {
                // الدفعة 1: نجاح 0.
                self.pending_code = Some(0);
                ReplOutput {
                    output: format_shortcut_output(&value),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            Err(e) => {
                // الدفعة 1: خطأ تشغيل 1.
                self.pending_code = Some(1);
                ReplOutput {
                    output: format!("{}\n", e),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
        }
    }

    /// العبارات الكاملة (2026-11-08، أمر المالك «اكتب دول
    /// كاملين»): 6 عبارات محرك بدلا من أكوادها المحذوفة
    /// — بم/بع/مس/ال/كا هنا، وحع هنا («حول إلى عدد»)؛
    /// ومع/اد/اق/مى بلا عبارة: دوالها العادية
    /// مجموع/أدنى/أقصى/مدى تكفي في الكود. وأوامر كاملة
    /// بكلمة واحدة (2026-11-08 ~20:00، أمر المالك:
    /// كلمة واحدة 4–5 حروف بلا ألف في النصف لا تحتاج
    /// اختصارا): انتقل/انسخ/انقل/موجود بدلا من أكوادها
    /// المحذوفة نم/سم/قم/وج — وعر/اط خرجا بلا بديل هنا
    /// لأن :اعرض واطبع يعملان أصلا بصيغتهما الكاملة.
    /// مطابقة بادئة للسطر كله: الكلمات الأولى هي العبارة
    /// للسطر كله: الكلمات الأولى هي العبارة والباقي
    /// معاملات مفصولة بمسافة بلا فصلة أبدا، وعدد
    /// المعاملات ورسائل الخطأ بنفس أسلوب الاختصارات
    /// («تتوقع N معاملات لكن أعطيت M»). «حول إلى عدد»
    /// بلا شدة على الواو (قاعدة بلا-تشكيل). تُفحص قبل
    /// line_is_code (كـ parse_command) فلا يبتلعها اسم
    /// معرّف من المستخدم: «ارجع للبيت» لأن ارجع كلمة
    /// مفتاحية، و«كرر الأخير» لأن كرر اسم موجود.
    /// «امسح الجلسة» محرك لا تطبيق: التطبيق يمررها
    /// إلى المحرك كما هي ولا يعترضها.
    fn try_full_phrase(&mut self, line: &str) -> Option<ReplOutput> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (phrase, nwords, min, max) = match words.as_slice() {
            ["ابحث", "في", "ملف", ..] => ("ابحث في ملف", 3, 2, 2),
            ["ابحث", "عن", "ملف", ..] => ("ابحث عن ملف", 3, 1, 1),
            ["امسح", "الجلسة", ..] => ("امسح الجلسة", 2, 0, 0),
            ["حول", "إلى", "عدد", ..] => ("حول إلى عدد", 3, 1, 1),
            ["ارجع", "للبيت", ..] => ("ارجع للبيت", 2, 0, 0),
            ["كرر", "الأخير", ..] => ("كرر الأخير", 2, 0, 0),
            ["انتقل", ..] => ("انتقل", 1, 0, 2),
            // اجتماع 59 §74 — الدفعة 5 (أدوات الملفات):
            // عبارات النسخ والحذف والروابط الكاملة —
            // قبل «انسخ» الأقصر حتى لا تبتلعها.
            ["انسخ", "تكراري", ..] => ("انسخ تكراري", 2, 2, 2),
            ["انسخ", "محدث", ..] => ("انسخ محدث", 2, 2, 2),
            ["انسخ", "أرشيف", ..] => ("انسخ أرشيف", 2, 2, 2),
            ["انسخ", "خفيف", ..] => ("انسخ خفيف", 2, 2, 2),
            ["احذف", "تكراري", ..] => ("احذف تكراري", 2, 1, 1),
            ["أنشئ", "رابط", "صلب", ..] => ("أنشئ رابط صلب", 3, 2, 2),
            ["أنشئ", "رابط", "نسبي", ..] => ("أنشئ رابط نسبي", 3, 2, 2),
            ["انسخ", ..] => ("انسخ", 1, 2, 2),
            ["انقل", ..] => ("انقل", 1, 2, 2),
            ["موجود", ..] => ("موجود", 1, 1, 1),
            // اجتماع 56 §71 — الدفعة 2 (التنقل): عبارات
            // المكدس الكاملة بكلمتين، بلا فصلة أبدا؛
            // والمعامل الواحد فيها مسار أو رقم نصي.
            ["ادفع", "مجلد", ..] => ("ادفع مجلد", 2, 0, 1),
            ["أخرج", "مجلد", ..] => ("أخرج مجلد", 2, 0, 1),
            ["اعرض", "المكدس", ..] => ("اعرض المكدس", 2, 0, 0),
            ["امسح", "المكدس", ..] => ("امسح المكدس", 2, 0, 0),
            // اجتماع 45 §59 (البنود 18 و19 و78): أوامر
            // كاملة بكلمة واحدة، بلا معاملات.
            ["معلومات", ..] => ("معلومات", 1, 0, 0),
            ["أعد-الإعدادات", ..] => ("أعد-الإعدادات", 1, 0, 0),
            ["أعد-الضبط", ..] => ("أعد-الضبط", 1, 0, 0),
            // الدفعة 6 (اجتماع 60 §75): عبارات المحفوظات
            // (153 و154 و149 و155 و156) — بلا فصلة أبدا،
            // وبلا أكواد حرفين جديدة (جدول الـ31 كما هو).
            ["محفوظات", ..] => ("محفوظات", 1, 0, usize::MAX),
            ["حرر", ..] => ("حرر", 1, 2, 3),
            ["ملف_المحفوظات", ..] => ("ملف_المحفوظات", 1, 0, 1),
            ["حد_المحفوظات", ..] => ("حد_المحفوظات", 1, 0, 1),
            ["حد_ملف_المحفوظات", ..] => ("حد_ملف_المحفوظات", 1, 0, 1),
            ["قواعد_المحفوظات", ..] => ("قواعد_المحفوظات", 1, 0, usize::MAX),
            ["تجاهل_المحفوظات", ..] => ("تجاهل_المحفوظات", 1, 0, usize::MAX),
            ["صيغة_وقت_المحفوظات", ..] => ("صيغة_وقت_المحفوظات", 1, 0, usize::MAX),
            // الدفعة 7 (اجتماع 61 §76): عبارات الموجهات
            // والخطاف والمهلة (174–176) — بلا فصلة أبدا،
            // وبلا أكواد حرفين جديدة (جدول الـ31 كما هو).
            // القالب يُبنى بربط الكلمات بمسافة واحدة
            // (فالمسافات المتعددة أو الأخيرة الدقيقة
            // تُضبط من niva.properties مباشرة).
            ["موجه_أول", ..] => ("موجه_أول", 1, 0, usize::MAX),
            ["موجه_استمرار", ..] => ("موجه_استمرار", 1, 0, usize::MAX),
            ["موجه_تتبع", ..] => ("موجه_تتبع", 1, 0, usize::MAX),
            ["أمر_قبل_الموجه", ..] => ("أمر_قبل_الموجه", 1, 0, usize::MAX),
            ["مهلة_الخمول", ..] => ("مهلة_الخمول", 1, 0, 1),
            // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
            // و63 §77 و§78): عبارتا `منع_المسح`
            // و`توقف_عند_الخطأ` القارئتان/الضابطتان —
            // بلا فصلة أبدا، كعبارات الدفعتين 6 و7.
            ["منع_المسح", ..] => ("منع_المسح", 1, 0, 1),
            ["توقف_عند_الخطأ", ..] => ("توقف_عند_الخطأ", 1, 0, 1),
            // المرحلة (هـ) — التحكم بالمهام (اجتماعا 64
            // و65 §79 و§80): عبارات الوظائف — بلا فصلة
            // أبدا. ملاحظة التهجئة: الاجتماع كتب
            // «قدّم» و«أخّر» بالشدة، وقاعدة المالك
            // الدائمة (بلا تشكيل في أي اسم أمر) تقتضي
            // «قدم» و«أخر» بلا شدة — فهما المنفذان.
            ["وظائف", ..] => ("وظائف", 1, 0, 1),
            ["قدم", ..] => ("قدم", 1, 0, 1),
            ["أخر", ..] => ("أخر", 1, 0, 1),
            ["اقتل", ..] => ("اقتل", 1, 0, 1),
            ["انتظر", ..] => ("انتظر", 1, 0, 1),
            ["أبعد", ..] => ("أبعد", 1, 0, 1),
            // المرحلة (د) — الإكمال (اجتماعا 67 و68
            // §82 و§83): عبارات المستعارات والإكمال
            // والخيارات — بلا فصلة أبدا.
            ["مستعار", ..] => ("مستعار", 1, 2, usize::MAX),
            ["مستعارات", ..] => ("مستعارات", 1, 0, 0),
            ["احذف_مستعار", ..] => ("احذف_مستعار", 1, 1, 1),
            ["أكمل", ..] => ("أكمل", 1, 0, 1),
            ["خيارات", ..] => ("خيارات", 1, 0, 2),
            // أمر المالك 2026-11-10 ~03:07 (قاعدة مؤكدة):
            // `حدث حزم` أمر مباشر في الموجه لأي مستخدم —
            // بلا معاملات، وأي كلمة بعدها خطأ عدد معاملات.
            ["حدث", "حزم", ..] => ("حدث حزم", 2, 0, 0),
            _ => return None,
        };
        let args: Vec<String> = words[nwords..].iter().map(|w| w.to_string()).collect();
        if args.len() < min || args.len() > max {
            let want = if min == max {
                if min == 0 {
                    "بدون معاملات".to_string()
                } else {
                    format!("{} معاملات", min)
                }
            } else if max == usize::MAX {
                format!("{} معاملات على الأقل", min)
            } else {
                format!("{} إلى {} معاملات", min, max)
            };
            // الدفعة 1: خطأ عدد المعاملات → سوء استخدام (2).
            self.pending_code = Some(2);
            return Some(ReplOutput {
                output: format!("{} تتوقع {} لكن أعطيت {}\n", phrase, want, args.len()),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        match phrase {
            "ابحث في ملف" => {
                self.history.record(line);
                Some(self.call_str_builtin("ابحث_في_ملف", args))
            }
            "ابحث عن ملف" => {
                self.history.record(line);
                Some(self.call_str_builtin("ابحث_عن", args))
            }
            "امسح الجلسة" => Some(self.handle_command(":مسح")),
            "انتقل" | "انسخ" | "انقل" | "موجود" => {
                self.history.record(line);
                Some(self.call_str_builtin(phrase, args))
            }
            // اجتماع 59 §74 — الدفعة 5 (أدوات الملفات):
            // العبارات الكاملة إلى دوالها المدمجة.
            "انسخ تكراري" => {
                self.history.record(line);
                Some(self.call_str_builtin("انسخ_تكراري", args))
            }
            "انسخ محدث" => {
                self.history.record(line);
                Some(self.call_str_builtin("انسخ_محدث", args))
            }
            "انسخ أرشيف" => {
                self.history.record(line);
                Some(self.call_str_builtin("انسخ_أرشيف", args))
            }
            "انسخ خفيف" => {
                self.history.record(line);
                Some(self.call_str_builtin("انسخ_خفيف", args))
            }
            "احذف تكراري" => {
                self.history.record(line);
                Some(self.call_str_builtin("احذف_تكراري", args))
            }
            "أنشئ رابط صلب" => {
                self.history.record(line);
                Some(self.call_str_builtin("أنشئ_رابط_صلب", args))
            }
            "أنشئ رابط نسبي" => {
                self.history.record(line);
                Some(self.call_str_builtin("أنشئ_رابط_نسبي", args))
            }
            // اجتماع 56 §71 — الدفعة 2 (التنقل): عبارات
            // المكدس إلى دوالها المدمجة بالاسم نفسه.
            "ادفع مجلد" => {
                self.history.record(line);
                Some(self.call_str_builtin("ادفع_مجلد", args))
            }
            "أخرج مجلد" => {
                self.history.record(line);
                Some(self.call_str_builtin("أخرج_مجلد", args))
            }
            "اعرض المكدس" => {
                self.history.record(line);
                Some(self.call_str_builtin("اعرض_المكدس", args))
            }
            "امسح المكدس" => {
                self.history.record(line);
                Some(self.call_str_builtin("امسح_المكدس", args))
            }
            "معلومات" => {
                self.history.record(line);
                // الدفعة 1: أمر جلسة ناجح رمزه 0.
                self.pending_code = Some(0);
                Some(ReplOutput {
                    output: self.info_lines(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                })
            }
            // أمر المالك 2026-11-10 ~03:07 (قاعدة مؤكدة):
            // `حدث حزم` أمر مباشر في الموجه — يحدّث حزم
            // نيفا عبر موزّع المدير، بلا برنامج ولا استيراد.
            "حدث حزم" => {
                self.history.record(line);
                Some(self.run_update_packages_phrase())
            }
            "أعد-الإعدادات" => {
                self.history.record(line);
                Some(self.reload_settings())
            }
            "أعد-الضبط" => {
                self.history.record(line);
                Some(self.ask_reset())
            }
            // الدفعة 6 (اجتماع 60 §75): أمر المحفوظات
            // وفروعه (153 و150 و157) — السطر المكتوب
            // نفسه يُسجَّل في المحفوظات كغيره.
            "محفوظات" => {
                self.history.record(line);
                Some(self.history_command(args))
            }
            // الدفعة 6: تحرير أمر سابق وإعادة تنفيذه (154).
            "حرر" => {
                self.history.record(line);
                Some(self.edit_command(args))
            }
            // الدفعة 6: إعدادات المحفوظات (149 و155 و156).
            "ملف_المحفوظات" => {
                self.history.record(line);
                Some(self.history_file_setting(args))
            }
            "حد_المحفوظات" => {
                self.history.record(line);
                Some(self.history_limit_setting(args, true))
            }
            "حد_ملف_المحفوظات" => {
                self.history.record(line);
                Some(self.history_limit_setting(args, false))
            }
            "قواعد_المحفوظات" => {
                self.history.record(line);
                Some(self.history_control_setting(args))
            }
            "تجاهل_المحفوظات" => {
                self.history.record(line);
                Some(self.history_ignore_setting(args))
            }
            "صيغة_وقت_المحفوظات" => {
                self.history.record(line);
                Some(self.history_time_setting(args))
            }
            // الدفعة 7 (اجتماع 61 §76): عبارات الموجهات
            // والخطاف والمهلة (174–176) — السطر المكتوب
            // نفسه يُسجَّل في المحفوظات كغيره.
            "موجه_أول" => {
                self.history.record(line);
                Some(self.prompt_template_setting(args, crate::prompts::KEY_PRIMARY, "موجه أول"))
            }
            "موجه_استمرار" => {
                self.history.record(line);
                Some(self.prompt_template_setting(args, crate::prompts::KEY_CONTINUATION, "موجه استمرار"))
            }
            "موجه_تتبع" => {
                self.history.record(line);
                Some(self.prompt_template_setting(args, crate::prompts::KEY_TRACE, "موجه تتبع"))
            }
            "أمر_قبل_الموجه" => {
                self.history.record(line);
                Some(self.prompt_hook_setting(args))
            }
            "مهلة_الخمول" => {
                self.history.record(line);
                Some(self.prompt_timeout_setting(args))
            }
            "منع_المسح" => {
                self.history.record(line);
                Some(self.noclobber_setting(args))
            }
            "توقف_عند_الخطأ" => {
                self.history.record(line);
                Some(self.stop_on_error_setting(args))
            }
            // المرحلة (هـ) — التحكم بالمهام (اجتماعا 64
            // و65 §79 و§80): عبارات الوظائف. كل معالج
            // يجمع المنتهية أولا (فالحالات محدَّثة)،
            // والسطر نفسه يُسجَّل في المحفوظات كغيره.
            "وظائف" => {
                self.history.record(line);
                Some(self.jobs_list(args))
            }
            "قدم" => {
                self.history.record(line);
                Some(self.jobs_foreground(args))
            }
            "أخر" => {
                self.history.record(line);
                Some(self.jobs_background_confirm(args))
            }
            "اقتل" => {
                self.history.record(line);
                Some(self.jobs_kill(args))
            }
            "انتظر" => {
                self.history.record(line);
                Some(self.jobs_wait(args))
            }
            "أبعد" => {
                self.history.record(line);
                Some(self.jobs_disown(args))
            }
            // المرحلة (د) — الإكمال (اجتماعا 67 و68
            // §82 و§83): معالجات المستعارات والإكمال
            // والخيارات.
            "مستعار" => {
                self.history.record(line);
                let trailing = line.ends_with(' ');
                Some(self.alias_define(args, trailing))
            }
            "مستعارات" => {
                self.history.record(line);
                self.pending_code = Some(0);
                Some(ReplOutput {
                    output: self.alias_list(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                })
            }
            "احذف_مستعار" => {
                self.history.record(line);
                Some(self.alias_delete(args))
            }
            "أكمل" => {
                self.history.record(line);
                let part = args.first().cloned().unwrap_or_default();
                let candidates = self.complete(&part);
                self.pending_code = Some(0);
                let output = if candidates.is_empty() {
                    "لا توجد اقتراحات\n".to_string()
                } else {
                    format!("{}\n", candidates.join("\n"))
                };
                Some(ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                })
            }
            "خيارات" => {
                self.history.record(line);
                Some(self.options_command(args))
            }
            "حول إلى عدد" => {
                self.history.record(line);
                // المعامل الواحد مضمون بفحص العدد أعلاه.
                let arg = args.into_iter().next().unwrap();
                Some(self.call_num_or_str_builtin("عدد", arg))
            }
            "ارجع للبيت" => {
                self.history.record(line);
                // البيت "~" في كل مكان في نيفا (سياسة
                // الملفات تحله إلى مجلد البرنامج).
                Some(self.call_str_builtin("انتقل", vec!["~".to_string()]))
            }
            "كرر الأخير" => {
                // كرر الأخير (!!): إعادة تنفيذ آخر سطر في
                // المحفوظات — آخر سطر ليس «كرر الأخير»
                // نفسها أبدا، ولا سطر «كا» قديم من جلسة
                // قبل الحذف؛ وبلا سطر سابق: الخطأ العربي
                // القصير نفسه. العبارة نفسها لا تُسجَّل:
                // السطر المعاد تنفيذه يسجّل نفسه في مساره.
                // الدفعة 6: إعادة التنفيذ بلا توسيع — فالسطر
                // السابق يُنفَّذ حرفيا كما كُتب (ولو بدأ بـ`!`
                // بعد إضافته عبر `محفوظات أضف`).
                let last = self
                    .history
                    .entries()
                    .iter()
                    .rev()
                    .find(|e| {
                        let w: Vec<&str> = e.text.split_whitespace().collect();
                        w != ["كرر", "الأخير"] && w.first() != Some(&"كا")
                    })
                    .map(|e| e.text.clone());
                match last {
                    Some(prev) => Some(self.feed_inner_no_expand(&prev)),
                    None => {
                        // الدفعة 1: بلا سطر سابق — خطأ
                        // حالة لا سوء استخدام (1).
                        self.pending_code = Some(1);
                        Some(ReplOutput {
                            output: "خطأ: لا يوجد سطر سابق لإعادة تنفيذه\n".to_string(),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        })
                    }
                }
            }
            _ => unreachable!("unknown full phrase"),
        }
    }

    /// Feed one typed line and get what to display next. The
    /// wrapper flags the network-permission error so an
    /// interactive console can offer the first-use prompt.
    pub fn feed(&mut self, line: &str) -> ReplOutput {
        self.last_control = ReplControl::None;
        // الدفعة 1 — رموز الخروج (اجتماع 54 §69): رمز
        // السطر يبدأ فارغا؛ وكل مسار ناتج نهائي يضبطه
        // (0 نجاح، 1 فشل عام، 2 سوء استخدام، 126/127،
        // أو رمز خروج)؛ والسطور الفارغة وغير المكتملة
        // لا تمسّه، فيبقى رمز آخر سطر مكتمل.
        self.pending_code = None;
        let mut out = self.feed_inner(line);
        // الدفعة 6: الجلسة انتهت — ألحق الجديد بملف
        // المحفوظات (بأي طريق: :خروج أو خروج() أو
        // استبدل)؛ وبلا ملف لا حفظ ولا تحذير؛ والفشل
        // تحذير لا منع للخروج.
        if out.ended && self.history.file_path().is_some() {
            if let Err(msg) = self.history.append_file_on_exit() {
                out.output
                    .push_str(&format!("تعذّر حفظ المحفوظات: {}\n", msg));
            }
        }
        // المرحلة (د) — الإكمال (اجتماع 68 §83): ملف
        // الخروج عند انتهاء الجلسة (مرة واحدة، ولو
        // أُعيدت التغذية بعد الانتهاء).
        if out.ended {
            let exit_out = self.run_exit_file();
            out.output.push_str(&exit_out);
        }
        // خذ رمز السطر (أو أبقِ الأخير)، وطبّق `عكس`
        // إن كان عدد بادئاتها فرديا والجلسة لم تنتهِ
        // (0 ↔ 1)، ثم احفظه وزامن المحرك — فيقرأه
        // `رمز_الخروج()` في السطر التالي.
        if let Some(code) = self.pending_code.take() {
            let code = if self.invert_count % 2 == 1 && !out.ended {
                if code == 0 { 1 } else { 0 }
            } else {
                code
            };
            self.last_exit_code = code;
            self.engine.set_last_exit_code(code);
            // المرحلة (ج) — `توقف_عند_الخطأ` (الميزة 79):
            // أي سطر ينتهي برمز غير 0 ينهي الجلسة بذلك
            // الرمز — لا سطور التوجيه وحدها؛ والخطأ
            // الممسوك بـ`حاول/ماعدا` رمزه 0 فلا ينهي.
            if self.stop_on_error && code != 0 && !out.ended {
                out.ended = true;
            }
            // المرحلة (ج): رموز الأنبوب — السطر العادي
            // رمزه وحده؛ وسطر الأنبوب ضبطها منفذه أصلا
            // (فلا تُستبدل برمزه الأخير وحده).
            if !self.pipe_status_set {
                self.pipe_status = vec![code];
            }
            self.pipe_status_set = false;
            let status = self.pipe_status.clone();
            self.engine.set_pipe_status(status);
        }
        self.invert_count = 0;
        out.needs_network = out
            .output
            .contains(crate::network::PERMISSION_ERROR_MARKER);
        if out.needs_network {
            // Audit م-6: the host offers the permission prompt
            // on this structured signal, not on the text.
            self.last_control = ReplControl::NetworkNeeded;
        }
        // Meeting 27: the same structured signal for the
        // server-permission error — the host offers the
        // first-use server prompt on it, not on the text.
        if out
            .output
            .contains(crate::network::SERVER_PERMISSION_MARKER)
        {
            self.last_control = ReplControl::ServerNeeded;
        }
        out
    }

    /// Audit م-12: a line that parses as Niva code runs as
    /// code — shortcut codes must not shadow names. The check
    /// is a trial semantic analysis on a CLONE of the
    /// analyzer: parse-only is not enough (a shortcut-shaped
    /// line can parse as code yet fail analysis while its
    /// name is undefined). One line of code is normally one statement,
    /// so a multi-statement line is shortcut-shaped. A bare
    /// reference to a shortcut-named variable falls through to
    /// the shortcut only while no such variable exists; once
    /// defined, the name is code. The clone keeps the trial
    /// side-effect free (the real eval analyzes again).
    fn line_is_code(&self, line: &str) -> bool {
        let tokens = match tokenize(line) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(_) => return false,
        };
        // One line of Niva code is normally one statement. A
        // line that parses as several statements (e.g. "نم
        // جديد" — two bare names) is shortcut-shaped: let the
        // shortcut layer own it, and fall back to eval after.
        if program.len() != 1 {
            return false;
        }
        let mut trial = self.analyzer.clone();
        trial.analyze_program(&program).is_ok()
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): وسّع
    /// الكلمة الأولى من السطر بمستعارها مرة واحدة —
    /// ولا شيء حين لا مستعار، أو حين قيمة المستعار
    /// تبدأ باسمه نفسه (حارس الاستدعاء الذاتي).
    /// القيمة المنتهية بمسافة توسّع الكلمة التالية
    /// أيضا مرة واحدة (بحارسها نفسه).
    fn expand_alias_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        let first_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let first = &trimmed[..first_end];
        let def = self.aliases.get(first)?;
        let value_first = def.value.split_whitespace().next().unwrap_or("");
        if value_first == first {
            return None;
        }
        let rest = trimmed[first_end..].trim_start();
        let mut out = def.value.clone();
        if def.expand_next && !rest.is_empty() {
            let next_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let next = &rest[..next_end];
            if let Some(ndef) = self.aliases.get(next) {
                let nfirst = ndef.value.split_whitespace().next().unwrap_or("");
                if nfirst != next {
                    out.push(' ');
                    out.push_str(&ndef.value);
                    let after = rest[next_end..].trim_start();
                    if !after.is_empty() {
                        out.push(' ');
                        out.push_str(after);
                    }
                    return Some(out);
                }
            }
        }
        if !rest.is_empty() {
            out.push(' ');
            out.push_str(rest);
        }
        Some(out)
    }

    /// تأكيد أعد-الضبط المعلّق (اجتماع 45 §59، البند 19):
    /// السطر التالي «نعم» حرفيا ينفّذ المسح، وأي سطر آخر
    /// يلغي ويُعالج عاديا (مع سطر الإلغاء قبله).
    fn feed_inner(&mut self, line: &str) -> ReplOutput {
        self.feed_inner_flag(line, true)
    }

    /// إعادة تنفيذ بلا توسيع (الدفعة 6): `كرر الأخير`
    /// و`حرر` والتوسيع الناجح ينفذون السطر حرفيا كما كُتب.
    fn feed_inner_no_expand(&mut self, line: &str) -> ReplOutput {
        self.feed_inner_flag(line, false)
    }

    fn feed_inner_flag(&mut self, line: &str, expand: bool) -> ReplOutput {
        if self.pending_reset {
            self.pending_reset = false;
            if line.trim() == "نعم" {
                self.history.record(line);
                return self.do_reset();
            }
            let mut out = self.feed_inner_impl(line, expand);
            out.output = format!("أُلغي إعادة الضبط.\n{}", out.output);
            return out;
        }
        self.feed_inner_impl(line, expand)
    }

    /// The exact logic of the old `run_repl` loop, moved into
    /// the struct so the native loop and the WASM console share
    /// it: an empty line while a block is pending cancels it; a
    /// command line never belongs to the block; an indented line
    /// only waits; only a line at indent 0 can finish the block.
    fn feed_inner_impl(&mut self, line: &str, expand: bool) -> ReplOutput {
        let line = line.trim_end_matches(['\n', '\r']).to_string();
        // المرحلة (ج) — جمع مستند هنا: أي سطر أثناء
        // الجمع جسم (بيانات لا كود ولا محفوظات) إلا
        // سطر المحدد نفسه، فينفَّذ السطر المخزون بمدخله
        // الجسم. قبل الكتلة المعلقة (فالجسم ليس كودا).
        if let Some(mut h) = self.pending_heredoc.take() {
            let content = if h.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            if content == h.delimiter {
                let stored = h.line.clone();
                let body = std::mem::take(&mut h.body);
                return self.run_heredoc_line(&stored, body);
            }
            h.body.push_str(content);
            h.body.push('\n');
            self.pending_heredoc = Some(h);
            return ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        if !self.pending.is_empty() {
            if line.trim().is_empty() {
                self.pending.clear();
                return ReplOutput {
                    output: "ألغيت الكتلة غير المكتملة.\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            if parse_command(&line).is_some() {
                let pending = std::mem::take(&mut self.pending);
                let (mut output, block_ended) = self.eval_display(&pending);
                let cmd = self.handle_command(&line);
                output.push_str(&cmd.output);
                return ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: cmd.ended || block_ended,
                    needs_network: false,
                };
            }
            if line.starts_with([' ', '\t']) {
                self.pending.push('\n');
                self.pending.push_str(&line);
                return ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            self.pending.push('\n');
            self.pending.push_str(&line);
            let pending = std::mem::take(&mut self.pending);
            return match self.eval(&pending) {
                ReplEval::Incomplete => {
                    self.pending = pending;
                    ReplOutput {
                        output: String::new(),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }
                }
                ReplEval::Done { output } => {
                    self.pending_code = Some(0);
                    ReplOutput {
                        output,
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }
                }
                // الدفعة 1: الكتلة خرجت بـ`خروج(رمز)` —
                // الجلسة تنتهي برمزه.
                ReplEval::Exited { output, code } => {
                    self.pending_code = Some(code);
                    ReplOutput {
                        output,
                        prompt: self.prompt(),
                        ended: true,
                        needs_network: false,
                    }
                }
                ReplEval::Failed { output, error, code } => {
                    self.pending_code = Some(code);
                    ReplOutput {
                        output: format!("{}{}\n", output, error),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    }
                }
            };
        }
        // الدفعة 1 — `عكس` (اجتماع 54 §69، مقابل `!`):
        // بادئة قبل سطر واحد تقلب رمزه (0 ↔ 1)؛
        // وتكرارها زوجي = الهوية. بعد فحص الكتلة
        // المعلقة (فالسطر داخل كتلة محتوى كود لا
        // بادئة) وقبل parse_command.
        if let Some((count, rest)) = strip_invert_prefix(&line) {
            if rest.trim().is_empty() {
                self.pending_code = Some(2);
                return ReplOutput {
                    output: "عكس تتوقع أمرا بعدها\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            if parse_command(rest).is_some_and(|c| matches!(c, ReplCommand::Quit(_)))
                || starts_with_exit_call(rest)
            {
                self.pending_code = Some(2);
                return ReplOutput {
                    output: "عكس لا تناسب الخروج\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            let history_len = self.history.len();
            let out = self.feed_inner_impl(rest, expand);
            if !self.pending.is_empty() {
                // الباقي بدأ كتلة متعددة الأسطر — تُلغى،
                // و`عكس` عليها مرفوضة برمز 2.
                self.pending.clear();
                self.pending_code = Some(2);
                return ReplOutput {
                    output: "عكس لا تناسب الكتلة متعددة الأسطر\n".to_string(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                };
            }
            // المحفوظات تسجّل السطر الأصلي كاملا
            // (ببادئاته)، لا الباقي وحده — فقط حين
            // دفع الاستدعاء الباقي نفسه (فلا يمسّ
            // مدخل أقدم مطابق حين فشل الباقي).
            // الدفعة 6: استبدال نص آخر مدخل مع بقاء طابعه.
            if self.history.len() == history_len + 1
                && self.history.last_text() == Some(rest)
            {
                self.history.replace_last_text(&line);
            }
            self.invert_count = count;
            return out;
        }
        // الدفعة 6 (اجتماع 60 §75، الميزتان 151 و152):
        // توسيع التاريخ والاستبدال السريع — أول السطر
        // فقط (بعد أي بادئة `عكس`)، وقبل أي مسار آخر.
        // `!` وحدها خطأ معجمي و`^` حرف غير معروف، فسطر
        // يبدأ بأي منهما لا يكون كود نيفا صالحا أبدا.
        // الناتج يُطبع ثم يُنفَّذ بلا توسيع ثانٍ (تمريرة
        // واحدة)؛ والخطأ يلغي السطر بلا تنفيذ وبلا تسجيل.
        if expand {
            let trimmed = line.trim_start();
            if trimmed.starts_with('!') || trimmed.starts_with('^') {
                let expanded = if trimmed.starts_with('!') {
                    self.history.expand(trimmed)
                } else {
                    self.history.quick_subst_line(trimmed)
                };
                return match expanded {
                    Ok(text) => {
                        if self.live_output {
                            // مع البث الحي يُطبع الصدى فورا
                            // قبل التنفيذ (وإلا سبقته مخرجات
                            // `اطبع` الحية فيظهر بعدها).
                            use std::io::Write;
                            println!("{}", text);
                            let _ = std::io::stdout().flush();
                            self.feed_inner_no_expand(&text)
                        } else {
                            let mut out = self.feed_inner_no_expand(&text);
                            out.output = format!("{}\n{}", text, out.output);
                            out
                        }
                    }
                    Err(msg) => {
                        self.pending_code = Some(1);
                        ReplOutput {
                            output: format!("{}\n", msg),
                            prompt: self.prompt(),
                            ended: false,
                            needs_network: false,
                        }
                    }
                };
            }
        }
        // المرحلة (د) — الإكمال (اجتماع 67 §82): توسيع
        // المستعارات — الكلمة الأولى مرة واحدة، بلا
        // توسيع ثانٍ (إعادة التغذية بلا توسيع)؛ وسطور
        // الكتل المعلقة والتواريخ (`!` و`^`) فوق لا
        // تمر من هنا.
        if expand {
            if let Some(expanded) = self.expand_alias_line(&line) {
                return self.feed_inner_no_expand(&expanded);
            }
        }
        if line.trim().is_empty() {
            return ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        if parse_command(&line).is_some() {
            return self.handle_command(&line);
        }
        // المرحلة (ج) — اعتراض التوجيه والأنابيب: بعد
        // `عكس` وتوسيع التاريخ وقبل العبارات الكاملة
        // والاختصارات؛ فـ`ات > خ.txt` يوجَّه مخرج
        // الاختصار، و`محفوظات > خ.txt` مخرج العبارة.
        // يعيد None حين لا عمل توجيه في السطر فيمر كما هو.
        if let Some(out) = self.try_redirect_line(&line) {
            return out;
        }
        // العبارات الكاملة (2026-11-08): قبل line_is_code
        // كـ parse_command — فلا يبتلعها اسم معرّف؛
        // وتثبيت يبقى في موضعه بعد line_is_code بلا تغيير.
        if let Some(out) = self.try_full_phrase(&line) {
            return out;
        }
        // Audit م-12 (code first): a line that parses as Niva
        // code runs as code — shortcut codes never shadow
        // names. Only when the line is not code does the
        // shortcut layer get its turn.
        if !self.line_is_code(&line) {
            if let Some(out) = self.try_shortcut(&line) {
                return out;
            }
            if let Some(out) = self.try_full_command(&line) {
                return out;
            }
            if let Some(out) = self.try_run_command(&line) {
                return out;
            }
        }
        match self.eval(&line) {
            ReplEval::Incomplete => {
                self.pending.push_str(&line);
                ReplOutput {
                    output: String::new(),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            ReplEval::Done { output } => {
                self.pending_code = Some(0);
                ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
            // الدفعة 1: السطر خرج بـ`خروج(رمز)` — الجلسة
            // تنتهي برمزه.
            ReplEval::Exited { output, code } => {
                self.pending_code = Some(code);
                ReplOutput {
                    output,
                    prompt: self.prompt(),
                    ended: true,
                    needs_network: false,
                }
            }
            ReplEval::Failed { output, error, code } => {
                self.pending_code = Some(code);
                ReplOutput {
                    output: format!("{}{}\n", output, error),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
        }
    }

    /// EOF (Ctrl-D): the loop prints a newline and exits cleanly.
    /// الدفعة 1 (اجتماع 54 §69): يخرج برمز آخر سطر
    /// مكتمل، كـباش.
    pub fn eof(&mut self) -> ReplOutput {
        self.quit_code = Some(self.last_exit_code);
        // الدفعة 6: حفظ المحفوظات عند EOF كما عند :خروج.
        let mut output = "\n".to_string();
        if self.history.file_path().is_some() {
            if let Err(msg) = self.history.append_file_on_exit() {
                output.push_str(&format!("تعذّر حفظ المحفوظات: {}\n", msg));
            }
        }
        // المرحلة (د) — الإكمال (اجتماع 68 §83): ملف
        // الخروج عند EOF أيضا (مرة واحدة).
        output.push_str(&self.run_exit_file());
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: true,
            needs_network: false,
        }
    }

    /// Evaluate one complete snippet (possibly multi-line).
    /// Incomplete input returns Incomplete and changes nothing.
    pub fn eval(&mut self, src: &str) -> ReplEval {
        let tokens = match tokenize(src) {
            Ok(t) => t,
            Err(e) => {
                return ReplEval::Failed {
                    output: String::new(),
                    error: e.to_string(),
                    // الدفعة 1: خطأ لغوي → سوء استخدام (2).
                    code: 2,
                }
            }
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                if INCOMPLETE_MARKERS.iter().any(|m| msg.contains(m)) {
                    return ReplEval::Incomplete;
                }
                return ReplEval::Failed {
                    output: String::new(),
                    error: msg,
                    // الدفعة 1: خطأ نحوي → سوء استخدام (2).
                    code: 2,
                };
            }
        };
        // المرحلة (د) — الإكمال (اجتماع 68 §83): بذر
        // محلل الجلسة بتعريفات أي `مصدر` حرفي في سطور
        // المستوى الأعلى — قبل تحليل البرنامج نفسه،
        // فتبقى أسماؤه صالحة في التقييمات التالية.
        for stmt in &program {
            if let Some(path) = Self::source_call_path(stmt) {
                self.seed_source_file(&path, 0);
            }
        }
        if let Err(e) = self.analyzer.analyze_program(&program) {
            return ReplEval::Failed {
                output: String::new(),
                error: e.to_string(),
                // الدفعة 1: خطأ دلالي → سوء استخدام (2).
                code: 2,
            };
        }
        // Complete input: record it before running, so a runtime
        // error does not erase it from the session history.
        // الدفعة 6: نقطة التسجيل الوحيدة (قواعد + أنماط + حد).
        self.history.record(src);
        // A bare expression prints its value — including a trailing
        // expression after other statements (a function definition
        // followed by its call, the common REPL flow). Run the
        // preceding statements first, then evaluate the trailing
        // expression once and print it.
        if let Some(StmtKind::ExprStmt { expr }) = program.last().map(|s| &s.kind) {
            let (head, _) = program.split_at(program.len() - 1);
            if let Err(e) = self.engine.run(head) {
                return ReplEval::Failed {
                    output: self.engine.take_output(),
                    error: e.to_string(),
                    // الدفعة 1: خطأ وقت تشغيل → فشل عام (1).
                    code: runtime_error_code(&e),
                };
            }
            let printed = self.engine.take_output();
            // الدفعة 1: خروج في السطور السابقة ينهي
            // الجلسة — والتعبير الأخير لا يُقيَّم.
            if let Some(code) = self.engine.take_exit_code() {
                return ReplEval::Exited {
                    output: printed,
                    code,
                };
            }
            return match self.engine.eval_one(expr) {
                Ok(v) => {
                    // المرحلة (ج): ما طُبعه التعبير نفسه
                    // أثناء تقييمه (كـ`وزع` يمرّر مدخله، أو
                    // دالة تطبع داخلها) يلحق بالمطبوع قبله
                    // — وإلا يُسقط من ناتج السطر ويتسرب
                    // للسطر التالي.
                    let during = self.engine.take_output();
                    // قيمة فارغة (كقيمة تثبيت-الكل) لا تطبع
                    // سطرا فارغا بعدها — المخرج يبقى ما طُبع
                    // فقط، بلا `\n` زائدة.
                    let value = v.fmt_value();
                    let output = if value.is_empty() {
                        format!("{}{}", printed, during)
                    } else {
                        format!("{}{}{}\n", printed, during, value)
                    };
                    // الدفعة 1: خروج في التعبير نفسه.
                    if let Some(code) = self.engine.take_exit_code() {
                        return ReplEval::Exited { output, code };
                    }
                    ReplEval::Done { output }
                }
                Err(e) => ReplEval::Failed {
                    // المرحلة (ج): كما في النجاح — ما طُبع
                    // أثناء التقييم قبل الخطأ يبقى في الناتج.
                    output: format!("{}{}", printed, self.engine.take_output()),
                    error: e.to_string(),
                    // الدفعة 1: خطأ وقت تشغيل → فشل عام (1).
                    code: runtime_error_code(&e),
                },
            };
        }
        match self.engine.run(&program) {
            Ok(()) => {
                let output = self.engine.take_output();
                // الدفعة 1: خروج في البرنامج ينهي الجلسة.
                if let Some(code) = self.engine.take_exit_code() {
                    return ReplEval::Exited { output, code };
                }
                ReplEval::Done { output }
            }
            Err(e) => ReplEval::Failed {
                output: self.engine.take_output(),
                error: e.to_string(),
                // الدفعة 1: خطأ وقت تشغيل → فشل عام (1).
                code: runtime_error_code(&e),
            },
        }
    }

    /// عرض المحفوظات مرقما (الدفعة 6، الميزة 153):
    /// الرقم 1 = الأقدم؛ والطابع قبل النص حين ضُبطت
    /// الصيغة؛ والكتلة مدخل واحد (سطرها الأول يحمل
    /// الرقم والتاليات بمسافة بادئة). و`:اعرض` يبقى
    /// بلا أرقام وبلا طوابع.
    pub fn show_history_numbered(&self, count: Option<usize>) -> String {
        let mut out = String::from("المحفوظات:\n");
        let total = self.history.len();
        let start = match count {
            Some(n) if n < total => total - n,
            _ => 0,
        };
        for (i, entry) in self.history.entries().iter().enumerate().skip(start) {
            let num = Value::Number((i + 1) as f64).fmt_value();
            let time_part = match self.history.display_time(entry) {
                Some(t) => format!("{}  ", t),
                None => String::new(),
            };
            let indent = " ".repeat(num.chars().count() + 2 + time_part.chars().count());
            for (j, part) in entry.text.lines().enumerate() {
                if j == 0 {
                    out.push_str(&num);
                    out.push_str("  ");
                    out.push_str(&time_part);
                } else {
                    out.push_str(&indent);
                }
                out.push(RLI);
                out.push_str(part);
                out.push(PDI);
                out.push('\n');
            }
        }
        out
    }

    /// السابق (الدفعة 6، الميزة 157): بدائية للمضيفين
    /// (السهم لأعلى / `ق`) — من الأحدث إلى الأقدم.
    pub fn history_prev(&mut self) -> Option<String> {
        self.history.history_prev()
    }

    /// التالي (الدفعة 6، الميزة 157): بدائية للمضيفين
    /// (السهم لأسفل / `ت`) — وبعد الأحدث `None`.
    pub fn history_next(&mut self) -> Option<String> {
        self.history.history_next()
    }

    /// البحث العكسي (الدفعة 6، الميزة 157): أحدث مدخل
    /// يحتوي النص، مع رقمه (1-based من الأقدم).
    pub fn history_search_reverse(&self, query: &str) -> Option<(usize, String)> {
        self.history
            .search_reverse(query)
            .map(|(n, e)| (n, e.text.clone()))
    }

    /// حمّل ملف المحفوظات عند دخول التفاعلي (الدفعة 6،
    /// الميزة 150): تُستدعى من `run_repl` بعد ملفات
    /// البدء. تعيد تحذيرا عند الفشل، وNone عند النجاح
    /// (بما فيه ملف غائب = بلا مدخلات).
    pub fn load_history_file(&mut self) -> Option<String> {
        if self.history.file_path().is_none() {
            return None;
        }
        match self.history.load_file() {
            Ok(_) => None,
            Err(msg) => Some(format!("تحذير: {}", msg)),
        }
    }

    /// أمر المحفوظات (الدفعة 6، الميزة 153 + فروع الملف
    /// 150 + البحث 157): `محفوظات [فرع ...]`.
    fn history_command(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = self.history_command_inner(args);
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// منطق `محفوظات` الخالص: (النص، رمز السطر).
    fn history_command_inner(&mut self, args: Vec<String>) -> (String, u8) {
        let num = |n: i64| Value::Number(n as f64).fmt_value();
        let num_usize = |n: usize| Value::Number(n as f64).fmt_value();
        match args.first().map(|s| s.as_str()) {
            None => (self.show_history_numbered(None), 0),
            Some("امسح") => {
                if args.len() > 1 {
                    return (
                        format!("محفوظات امسح تتوقع بدون معاملات لكن أعطيت {}\n", args.len() - 1),
                        2,
                    );
                }
                self.history.clear();
                ("مسحت محفوظات الجلسة.\n".to_string(), 0)
            }
            Some("احذف") => {
                if args.len() != 2 {
                    return (
                        "محفوظات احذف تتوقع رقما واحدا\n".to_string(),
                        2,
                    );
                }
                let spec = &args[1];
                // مدى `أول-آخر` (أرقام موجبة)، أو رقم واحد
                // (والسالب من النهاية، -1 = الأخير).
                if spec.matches('-').count() == 1 && !spec.starts_with('-') {
                    let mut parts = spec.splitn(2, '-');
                    let first = parts.next().unwrap_or("");
                    let last = parts.next().unwrap_or("");
                    let (a, b) = match (
                        crate::history::parse_history_number(first),
                        crate::history::parse_history_number(last),
                    ) {
                        (Some(a), Some(b)) if a > 0 && b > 0 => (a, b),
                        _ => {
                            return (
                                "محفوظات احذف تتوقع رقما أو مدى مثل ٢-٤\n".to_string(),
                                2,
                            );
                        }
                    };
                    if a > b {
                        return ("محفوظات احذف تتوقع الأول أصغر من الآخر أو مساويا له\n".to_string(), 2);
                    }
                    let (Some(fi), Some(li)) = (self.history.resolve(a), self.history.resolve(b))
                    else {
                        return (
                            "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات\n".to_string(),
                            1,
                        );
                    };
                    self.history.delete_range(fi, li);
                    (
                        format!("حذفت الأوامر من {} إلى {}.\n", num(a), num(b)),
                        0,
                    )
                } else {
                    let n = match crate::history::parse_history_number(spec) {
                        Some(n) => n,
                        None => {
                            return (
                                "محفوظات احذف تتوقع رقما واحدا\n".to_string(),
                                2,
                            );
                        }
                    };
                    match self.history.resolve(n) {
                        Some(i) => {
                            self.history.delete(i);
                            (format!("حذفت الأمر رقم {}.\n", num(n)), 0)
                        }
                        None => (
                            "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات\n".to_string(),
                            1,
                        ),
                    }
                }
            }
            Some("أضف") => {
                if args.len() < 2 {
                    return ("محفوظات أضف تتوقع نصا\n".to_string(), 2);
                }
                // إضافة بلا تنفيذ — وتمر بقواعد الحفظ
                // (فالمكرر أو الروتيني قد يُتجاهل صامتا).
                let text = args[1..].join(" ");
                self.history.record(&text);
                (String::new(), 0)
            }
            Some("اطبع") => {
                if args.len() < 2 {
                    return ("محفوظات اطبع تتوقع توسيعا\n".to_string(), 2);
                }
                // التوسيع يستثني سطر الأمر نفسه (سُجَّل
                // قبل التنفيذ — والتوسيع في باش يحدث وقت
                // القراءة قبل التسجيل).
                let expansion = args[1..].join(" ");
                match self.history.expand_excluding_last(&expansion) {
                    Ok(text) => (format!("{}\n", text), 0),
                    Err(msg) => (format!("{}\n", msg), 1),
                }
            }
            Some("ابحث") => {
                if args.len() < 2 {
                    return ("محفوظات ابحث تتوقع نصا\n".to_string(), 2);
                }
                let query = args[1..].join(" ");
                match self.history.search_reverse_excluding_last(&query) {
                    Some((n, entry)) => {
                        let mut out = String::new();
                        out.push_str(&num_usize(n));
                        out.push_str("  ");
                        if let Some(t) = self.history.display_time(entry) {
                            out.push_str(&t);
                            out.push_str("  ");
                        }
                        out.push(RLI);
                        out.push_str(&entry.text);
                        out.push(PDI);
                        out.push('\n');
                        (out, 0)
                    }
                    None => (
                        format!("خطأ: لا يوجد أمر يحتوي «{}» في المحفوظات\n", query),
                        1,
                    ),
                }
            }
            Some("ألحق") => {
                if args.len() > 1 {
                    return (
                        format!("محفوظات ألحق تتوقع بدون معاملات لكن أعطيت {}\n", args.len() - 1),
                        2,
                    );
                }
                match self.history.append_file() {
                    Ok(0) => ("لا جديد للإلحاق بملف المحفوظات.\n".to_string(), 0),
                    Ok(n) => (
                        format!("أُلحق {} إلى ملف المحفوظات.\n", num_usize(n)),
                        0,
                    ),
                    Err(msg) => (format!("{}\n", msg), 1),
                }
            }
            Some("احفظ") => {
                if args.len() > 1 {
                    return (
                        format!("محفوظات احفظ تتوقع بدون معاملات لكن أعطيت {}\n", args.len() - 1),
                        2,
                    );
                }
                match self.history.save_file() {
                    Ok(()) => ("حُفظت المحفوظات في الملف.\n".to_string(), 0),
                    Err(msg) => (format!("{}\n", msg), 1),
                }
            }
            Some("اقرأ") => {
                if args.len() > 1 {
                    return (
                        format!("محفوظات اقرأ تتوقع بدون معاملات لكن أعطيت {}\n", args.len() - 1),
                        2,
                    );
                }
                match self.history.load_file() {
                    Ok(n) => (
                        format!("قُرئت {} من ملف المحفوظات.\n", num_usize(n)),
                        0,
                    ),
                    Err(msg) => (format!("{}\n", msg), 1),
                }
            }
            Some("اقرأ_جديد") => {
                if args.len() > 1 {
                    return (
                        format!("محفوظات اقرأ_جديد تتوقع بدون معاملات لكن أعطيت {}\n", args.len() - 1),
                        2,
                    );
                }
                match self.history.load_new_file() {
                    Ok(0) => ("لا جديد في ملف المحفوظات.\n".to_string(), 0),
                    Ok(n) => (
                        format!("قُرئت {} جديدة من ملف المحفوظات.\n", num_usize(n)),
                        0,
                    ),
                    Err(msg) => (format!("{}\n", msg), 1),
                }
            }
            Some(other) => {
                // معامل واحد عددا = عرض آخر N.
                if args.len() == 1 {
                    if let Some(n) = crate::history::parse_history_number(other) {
                        if n < 0 {
                            return (
                                "محفوظات تتوقع عددا موجبا لعرض آخر N\n".to_string(),
                                2,
                            );
                        }
                        return (self.show_history_numbered(Some(n as usize)), 0);
                    }
                }
                (
                    format!("فرع محفوظات غير معروف: {}\n", other),
                    2,
                )
            }
        }
    }

    /// تحرير أمر سابق وإعادة تنفيذه (الدفعة 6، الميزة 154
    /// — `fc -s` بلفظ عربي): `حرر <قديم> <جديد>` على
    /// الأخير، و`حرر <رقم> <قديم> <جديد>` على مدخل مرقم.
    /// الناتج يُطبع ثم يُنفَّذ حرفيا (بلا توسيع ثانٍ).
    fn edit_command(&mut self, args: Vec<String>) -> ReplOutput {
        let fail = |msg: String, code: u8, repl: &mut Self| {
            repl.pending_code = Some(code);
            ReplOutput {
                output: format!("{}\n", msg),
                prompt: repl.prompt(),
                ended: false,
                needs_network: false,
            }
        };
        // بلا معامل رقم: آخر أمر قبل سطر `حرر` نفسه
        // (فسطر الأمر سُجَّل قبل التنفيذ)؛ وبرقم: المدخل
        // المرقم — إلا سطر `حرر` نفسه (فيعيد استدعاء
        // نفسه بلا نهاية).
        let (index, old, new) = match args.len() {
            2 => (self.history.len().checked_sub(2), args[0].clone(), args[1].clone()),
            3 => {
                let n = match crate::history::parse_history_number(&args[0]) {
                    Some(n) => n,
                    None => {
                        return fail(
                            "حرر تتوقع رقما صحيحا أولا".to_string(),
                            2,
                            self,
                        );
                    }
                };
                let index = match self.history.resolve(n) {
                    Some(i) => i,
                    None => {
                        return fail(
                            "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات".to_string(),
                            1,
                            self,
                        );
                    }
                };
                if index == self.history.len().saturating_sub(1) {
                    return fail(
                        "خطأ: لا يمكن تحرير أمر حرر نفسه".to_string(),
                        1,
                        self,
                    );
                }
                (Some(index), args[1].clone(), args[2].clone())
            }
            _ => {
                return fail(
                    "حرر تتوقع قديما وجديدا، أو رقما ثم قديما وجديدا".to_string(),
                    2,
                    self,
                );
            }
        };
        let Some(index) = index else {
            return fail(
                "خطأ: لا يوجد أمر سابق للتحرير".to_string(),
                1,
                self,
            );
        };
        let text = match self.history.edit_entry(index, &old, &new) {
            Ok(t) => t,
            Err(msg) => return fail(msg, 1, self),
        };
        // حارس التكرار الذاتي: ناتج التحرير لا يكون
        // أمر تحرير (فيعيد استدعاء نفسه بلا نهاية).
        let first_word = text.split_whitespace().next().unwrap_or("");
        if first_word == "حرر" {
            return fail(
                "خطأ: لا يمكن أن يكون ناتج التحرير أمر حرر".to_string(),
                1,
                self,
            );
        }
        if self.live_output {
            use std::io::Write;
            println!("{}", text);
            let _ = std::io::stdout().flush();
            self.feed_inner_no_expand(&text)
        } else {
            let mut out = self.feed_inner_no_expand(&text);
            out.output = format!("{}\n{}", text, out.output);
            out
        }
    }

    /// اكتب مفتاحا واحدا في `niva.properties` (الدفعة 6):
    /// يحفظ باقي السطور والتعليقات؛ والسطر الغائب يُلحق
    /// في النهاية. تعيد تحذيرا عند الفشل (ولا تمنع
    /// الضبط الحي في الذاكرة).
    fn persist_history_setting(&self, key: &str, value: &str) -> Option<String> {
        let path = settings_path(&self.program_dir);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let mut out = String::new();
        let mut found = false;
        for raw in current.lines() {
            let line = raw.trim();
            if !line.is_empty() && !line.starts_with('#') {
                if let Some(eq) = line.find('=') {
                    if line[..eq].trim() == key {
                        out.push_str(key);
                        out.push('=');
                        out.push_str(value);
                        out.push('\n');
                        found = true;
                        continue;
                    }
                }
            }
            out.push_str(raw);
            out.push('\n');
        }
        if !found {
            out.push_str(key);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return Some("تعذّر إنشاء مجلد الإعدادات".to_string());
            }
        }
        match crate::install::write_atomic(&path, out.as_bytes()) {
            Ok(()) => None,
            Err(_) => Some("تعذّر كتابة ملف الإعدادات".to_string()),
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): تعريف
    /// مستعار — الاسم كلمة واحدة، والقيمة باقي الكلمات
    /// مربوطة بمسافة واحدة؛ والمسافة الأخيرة في السطر
    /// (trailing) تُسجَّل علما فيوسّع التعريف الكلمة
    /// التالية أيضا. أسماء عبارات إدارة المستعارات
    /// نفسها والأوامر (`:`) محجوزة (فهي تدير الطبقة
    /// — وحارس إضافي مع حارس الاستدعاء الذاتي).
    fn alias_define(&mut self, args: Vec<String>, expand_next: bool) -> ReplOutput {
        let name = args[0].clone();
        let value = args[1..].join(" ");
        if ["مستعار", "مستعارات", "احذف_مستعار", "أكمل", "خيارات"]
            .contains(&name.as_str())
            || name.starts_with(':')
        {
            self.pending_code = Some(1);
            return ReplOutput {
                output: format!("الاسم محجوز ولا يصلح مستعارا: {}\n", name),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        self.aliases.insert(
            name.clone(),
            AliasDef {
                value: value.clone(),
                expand_next,
            },
        );
        self.pending_code = Some(0);
        ReplOutput {
            output: format!("مستعار {} {}\n", name, value),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): قائمة
    /// المستعارات بصيغة قابلة لإعادة الاستخدام
    /// («مستعار <اسم> <قيمة>» في كل سطر؛ والمسافة
    /// الأخيرة تُحفظ حين كان العلم مضبوطا).
    fn alias_list(&self) -> String {
        if self.aliases.is_empty() {
            return "لا توجد مستعارات\n".to_string();
        }
        let mut names: Vec<&String> = self.aliases.keys().collect();
        names.sort();
        let mut out = String::new();
        for name in names {
            let def = &self.aliases[name];
            out.push_str(&format!("مستعار {} {}", name, def.value));
            if def.expand_next {
                out.push(' ');
            }
            out.push('\n');
        }
        out
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82): حذف
    /// مستعار واحد، أو «الكل» لمسح الطبقة كلها؛
    /// والغائب خطأ يسمّي الاسم.
    fn alias_delete(&mut self, args: Vec<String>) -> ReplOutput {
        let name = args[0].clone();
        if name == "الكل" {
            let had = !self.aliases.is_empty();
            self.aliases.clear();
            self.pending_code = Some(0);
            return ReplOutput {
                output: if had {
                    "حُذفت كل المستعارات\n".to_string()
                } else {
                    "لا توجد مستعارات\n".to_string()
                },
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            };
        }
        if self.aliases.remove(&name).is_some() {
            self.pending_code = Some(0);
            ReplOutput {
                output: format!("حُذف المستعار: {}\n", name),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            }
        } else {
            self.pending_code = Some(1);
            ReplOutput {
                output: format!("مستعار غير موجود: {}\n", name),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            }
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): عبارة
    /// `خيارات` — بلا معاملات تعرض كل المفاتيح
    /// الحالية بصيغة «مفتاح=قيمة»؛ ومعامل واحد يقرأ
    /// مفتاحا؛ ومعاملان يضبطانه (فيُحفظ في
    /// `niva.properties` ويُطبَّق حيا حيث له مسار حي
    /// — الموجهات ومنع_المسح وحدا المحفوظات؛ وباقي
    /// المفاتيح يسري من الجلسة الجديدة أو إعادة
    /// التشغيل). المفتاح المجهول خطأ يسمّيه.
    fn options_command(&mut self, args: Vec<String>) -> ReplOutput {
        let done = |out: String, code: u8, slf: &mut Self| ReplOutput {
            output: out,
            prompt: slf.prompt(),
            ended: false,
            needs_network: false,
        };
        match args.len() {
            0 => {
                let (entries, _) = read_settings(&self.program_dir);
                self.pending_code = Some(0);
                let output = if entries.is_empty() {
                    "لا توجد إعدادات\n".to_string()
                } else {
                    entries
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join("\n")
                        + "\n"
                };
                done(output, 0, self)
            }
            1 => {
                let key = args[0].clone();
                if !KNOWN_SETTINGS_KEYS.contains(&key.as_str()) {
                    self.pending_code = Some(1);
                    return done(format!("مفتاح غير معروف: {}\n", key), 1, self);
                }
                let (entries, _) = read_settings(&self.program_dir);
                match entries.iter().find(|(k, _)| *k == key) {
                    Some((_, v)) => {
                        self.pending_code = Some(0);
                        done(format!("{}={}\n", key, v), 0, self)
                    }
                    None => {
                        self.pending_code = Some(1);
                        done(format!("المفتاح غير مضبوط: {}\n", key), 1, self)
                    }
                }
            }
            _ => {
                let key = args[0].clone();
                let value = args[1].clone();
                if !KNOWN_SETTINGS_KEYS.contains(&key.as_str()) {
                    self.pending_code = Some(1);
                    return done(format!("مفتاح غير معروف: {}\n", key), 1, self);
                }
                // حدا المحفوظات عددان صحيحان غير سالبين
                // — يُفحصان قبل الحفظ كعبارتهما الخاصة.
                if key == crate::history::KEY_MAX_MEMORY
                    || key == crate::history::KEY_MAX_FILE
                {
                    match crate::history::parse_history_number(&value) {
                        Some(n) if n >= 0 => {
                            let n = n as usize;
                            if key == crate::history::KEY_MAX_MEMORY {
                                self.history.set_max_memory(n);
                            } else if let Err(msg) = self.history.set_max_file(n) {
                                self.pending_code = Some(1);
                                return done(format!("{}\n", msg), 1, self);
                            }
                        }
                        _ => {
                            self.pending_code = Some(2);
                            return done(
                                format!("{} تتوقع عددا صحيحا غير سالب\n", key),
                                2,
                                self,
                            );
                        }
                    }
                }
                if let Some(w) = self.persist_history_setting(&key, &value) {
                    self.pending_code = Some(1);
                    return done(format!("تعذّر حفظ الإعداد: {}\n", w), 1, self);
                }
                // التطبيق الحي: الموجهات ومنع_المسح هنا؛
                // وباقي المفاتيح (ومنها الترحيب ومفاتيح
                // التطبيق) تسري من الجلسة الجديدة.
                let (entries, _) = read_settings(&self.program_dir);
                self.prompts = crate::prompts::PromptSettings::from_entries(&entries);
                self.noclobber = settings_flag(&entries, "منع_المسح");
                self.last_control = ReplControl::SettingsReload;
                self.pending_code = Some(0);
                let mut output = format!("{}={}\n", key, value);
                if !matches!(
                    key.as_str(),
                    "موجه_أول"
                        | "موجه_استمرار"
                        | "موجه_تتبع"
                        | "أمر_قبل_الموجه"
                        | "مهلة_الخمول"
                        | "منع_المسح"
                        | "ملف_المحفوظات"
                        | "حد_المحفوظات"
                        | "حد_ملف_المحفوظات"
                        | "قواعد_المحفوظات"
                        | "تجاهل_المحفوظات"
                        | "صيغة_وقت_المحفوظات"
                ) {
                    output.push_str("يسري من الجلسة الجديدة\n");
                }
                done(output, 0, self)
            }
        }
    }

    /// إعداد ملف المحفوظات (149): `ملف_المحفوظات [مسار]`
    /// — بلا معامل تعرض الحالي؛ و`بلا` تعني بلا ملف.
    fn history_file_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = match args.first().map(|s| s.as_str()) {
            None => match self.history.file_path() {
                Some(p) => (
                    format!(
                        "ملف المحفوظات: {}\n",
                        crate::history::display_history_file(&self.program_dir, p)
                    ),
                    0,
                ),
                None => ("ملف المحفوظات: بلا ملف\n".to_string(), 0),
            },
            Some("بلا") => {
                self.history.set_file(None);
                let warn = self.persist_history_setting(crate::history::KEY_FILE, "");
                let mut out = "ملف المحفوظات: بلا ملف\n".to_string();
                if let Some(w) = warn {
                    out.push_str(&format!("تحذير: {}\n", w));
                }
                (out, 0)
            }
            Some(value) => {
                let resolved =
                    crate::history::resolve_history_file(&self.program_dir, value);
                if resolved.is_absolute() && !resolved.starts_with(&self.program_dir) {
                    (
                        "خطأ: مسار ملف المحفوظات يجب أن يكون داخل مجلد البرنامج\n".to_string(),
                        2,
                    )
                } else {
                    let shown =
                        crate::history::display_history_file(&self.program_dir, &resolved);
                    self.history.set_file(Some(resolved));
                    let warn = self.persist_history_setting(crate::history::KEY_FILE, &shown);
                    let mut out = format!("ملف المحفوظات: {}\n", shown);
                    if let Some(w) = warn {
                        out.push_str(&format!("تحذير: {}\n", w));
                    }
                    (out, 0)
                }
            }
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد حد المحفوظات (149): `حد_المحفوظات [عدد]`
    /// للذاكرة، و`حد_ملف_المحفوظات [عدد]` للملف.
    fn history_limit_setting(&mut self, args: Vec<String>, memory: bool) -> ReplOutput {
        let (name, key, current) = if memory {
            (
                "حد المحفوظات",
                crate::history::KEY_MAX_MEMORY,
                self.history.settings().max_memory,
            )
        } else {
            (
                "حد ملف المحفوظات",
                crate::history::KEY_MAX_FILE,
                self.history.settings().max_file,
            )
        };
        let (output, code) = match args.first().map(|s| s.as_str()) {
            None => (
                format!(
                    "{}: {}\n",
                    name,
                    Value::Number(current as f64).fmt_value()
                ),
                0,
            ),
            Some(raw) => match crate::history::parse_history_number(raw) {
                Some(n) if n >= 0 => {
                    let n = n as usize;
                    let mut out = format!(
                        "{}: {}\n",
                        name,
                        Value::Number(n as f64).fmt_value()
                    );
                    let mut code = 0;
                    if memory {
                        self.history.set_max_memory(n);
                    } else if let Err(msg) = self.history.set_max_file(n) {
                        out = format!("{}\n", msg);
                        code = 1;
                    }
                    if code == 0 {
                        if let Some(w) = self.persist_history_setting(key, &n.to_string()) {
                            out.push_str(&format!("تحذير: {}\n", w));
                        }
                    }
                    (out, code)
                }
                _ => (
                    format!("{} تتوقع عددا صحيحا غير سالب\n", name),
                    2,
                ),
            },
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد قواعد الحفظ (155): `قواعد_المحفوظات [قيم...]`
    /// — بلا معاملات تعرض الحالية.
    fn history_control_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = if args.is_empty() {
            let cur = self.history.settings().control_string();
            (
                format!(
                    "قواعد المحفوظات: {}\n",
                    if cur.is_empty() { "بلا قواعد" } else { &cur }
                ),
                0,
            )
        } else {
            let joined = args.join(" ");
            match crate::history::parse_control(&joined) {
                Ok((d, sp, e)) => {
                    self.history.set_control(d, sp, e);
                    let stored = self.history.settings().control_string();
                    let mut out = format!(
                        "قواعد المحفوظات: {}\n",
                        if stored.is_empty() { "بلا قواعد" } else { &stored }
                    );
                    if let Some(w) =
                        self.persist_history_setting(crate::history::KEY_CONTROL, &stored)
                    {
                        out.push_str(&format!("تحذير: {}\n", w));
                    }
                    (out, 0)
                }
                Err(msg) => (format!("{}\n", msg), 2),
            }
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد أنماط التجاهل (156): `تجاهل_المحفوظات [أنماط...]`
    /// — بلا معاملات تعرض الحالية؛ و`بلا` تمسحها.
    fn history_ignore_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = if args.is_empty() {
            let cur = self.history.settings().ignore_string();
            (
                format!(
                    "تجاهل المحفوظات: {}\n",
                    if cur.is_empty() { "بلا أنماط" } else { &cur }
                ),
                0,
            )
        } else {
            let patterns = crate::history::parse_patterns(&args.join(" "));
            self.history.set_patterns(patterns);
            let stored = self.history.settings().ignore_string();
            let mut out = format!(
                "تجاهل المحفوظات: {}\n",
                if stored.is_empty() { "بلا أنماط" } else { &stored }
            );
            if let Some(w) = self.persist_history_setting(crate::history::KEY_IGNORE, &stored) {
                out.push_str(&format!("تحذير: {}\n", w));
            }
            (out, 0)
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد صيغة الطابع (156): `صيغة_وقت_المحفوظات [صيغة]`
    /// — بلا معاملات تعرض الحالية؛ و`بلا` تمسحها.
    fn history_time_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = if args.is_empty() {
            match self.history.settings().time_format.as_deref() {
                Some(f) => (format!("صيغة وقت المحفوظات: {}\n", f), 0),
                None => ("صيغة وقت المحفوظات: بلا صيغة\n".to_string(), 0),
            }
        } else {
            let joined = args.join(" ");
            if joined == "بلا" {
                self.history.set_time_format(None);
                let mut out = "صيغة وقت المحفوظات: بلا صيغة\n".to_string();
                if let Some(w) =
                    self.persist_history_setting(crate::history::KEY_TIME_FORMAT, "")
                {
                    out.push_str(&format!("تحذير: {}\n", w));
                }
                (out, 0)
            } else {
                match crate::history::check_time_format(&joined) {
                    Ok(()) => {
                        self.history.set_time_format(Some(joined.clone()));
                        let mut out =
                            format!("صيغة وقت المحفوظات: {}\n", joined);
                        if let Some(w) = self.persist_history_setting(
                            crate::history::KEY_TIME_FORMAT,
                            &joined,
                        ) {
                            out.push_str(&format!("تحذير: {}\n", w));
                        }
                        (out, 0)
                    }
                    Err(msg) => (format!("{}\n", msg), 2),
                }
            }
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد قالب موجه (174): `موجه_أول [قالب...]` أو
    /// `موجه_استمرار` أو `موجه_تتبع` — بلا معاملات تعرض
    /// القالب الحالي؛ وبمعاملات يُضبط القالب الجديد
    /// (الكلمات مربوطة بمسافة واحدة) ويُحفظ في
    /// `niva.properties` ويسري من الموجه التالي.
    fn prompt_template_setting(
        &mut self,
        args: Vec<String>,
        key: &str,
        label: &str,
    ) -> ReplOutput {
        let (output, code) = if args.is_empty() {
            let current = match key {
                crate::prompts::KEY_PRIMARY => &self.prompts.primary,
                crate::prompts::KEY_CONTINUATION => &self.prompts.continuation,
                _ => &self.prompts.trace,
            };
            (format!("{}: {}\n", label, current), 0)
        } else {
            let joined = args.join(" ");
            match key {
                crate::prompts::KEY_PRIMARY => self.prompts.primary = joined.clone(),
                crate::prompts::KEY_CONTINUATION => {
                    self.prompts.continuation = joined.clone()
                }
                _ => self.prompts.trace = joined.clone(),
            }
            let mut out = format!("{}: {}\n", label, joined);
            if let Some(w) = self.persist_history_setting(key, &joined) {
                out.push_str(&format!("تحذير: {}\n", w));
            }
            (out, 0)
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد أمر ما قبل الموجه (175): `أمر_قبل_الموجه
    /// [مقتطف...]` — بلا معاملات يعرض الحالي (أو «بلا»)؛
    /// وبقيمة `بلا` يُلغى الخطاف؛ وبمقتطف يُضبط ويُحفظ
    /// ويسري من الموجه التالي.
    fn prompt_hook_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = if args.is_empty() {
            if self.prompts.has_hook() {
                (format!("أمر قبل الموجه: {}\n", self.prompts.hook), 0)
            } else {
                ("أمر قبل الموجه: بلا\n".to_string(), 0)
            }
        } else {
            let joined = args.join(" ");
            if joined == "بلا" {
                self.prompts.hook.clear();
                let mut out = "أمر قبل الموجه: بلا\n".to_string();
                if let Some(w) =
                    self.persist_history_setting(crate::prompts::KEY_HOOK, "")
                {
                    out.push_str(&format!("تحذير: {}\n", w));
                }
                (out, 0)
            } else {
                self.prompts.hook = joined.clone();
                let mut out = format!("أمر قبل الموجه: {}\n", joined);
                if let Some(w) =
                    self.persist_history_setting(crate::prompts::KEY_HOOK, &joined)
                {
                    out.push_str(&format!("تحذير: {}\n", w));
                }
                (out, 0)
            }
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// إعداد مهلة الخمول (176): `مهلة_الخمول [ثوان]` —
    /// بلا معامل تعرض الحالية (أو «بلا مهلة»)؛ و0 أو
    /// `بلا` تلغيها؛ والقيمة السيئة (غير عدد أو سالبة)
    /// مرفوضة بخطأ ويبقى القديم.
    fn prompt_timeout_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = match args.first().map(|s| s.as_str()) {
            None => {
                if self.prompts.has_timeout() {
                    (
                        format!(
                            "مهلة الخمول: {} ثوان\n",
                            crate::time::eastern(&self.prompts.timeout_secs.to_string())
                        ),
                        0,
                    )
                } else {
                    ("مهلة الخمول: بلا مهلة\n".to_string(), 0)
                }
            }
            Some(value) => match crate::prompts::parse_timeout(value) {
                Some(n) => {
                    self.prompts.timeout_secs = n;
                    let mut out = if n == 0 {
                        "مهلة الخمول: بلا مهلة\n".to_string()
                    } else {
                        format!(
                            "مهلة الخمول: {} ثوان\n",
                            crate::time::eastern(&n.to_string())
                        )
                    };
                    if let Some(w) = self
                        .persist_history_setting(crate::prompts::KEY_TIMEOUT, &n.to_string())
                    {
                        out.push_str(&format!("تحذير: {}\n", w));
                    }
                    (out, 0)
                }
                None => (
                    "مهلة_الخمول تتوقع عددا صحيحا غير سالب\n".to_string(),
                    2,
                ),
            },
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// المرحلة (ج) — `منع_المسح [صح|خطأ]` (اجتماع 62
    /// §77، الميزة 64): بلا معامل تعرض الحالي؛ وبمعامل
    /// تضبطه وتكتبه في الإعدادات (فيبقى عبر الجلسات —
    /// نمط `persist_history_setting` نفسه)؛ والقيمة
    /// السيئة مرفوضة بخطأ 2 ويبقى القديم.
    fn noclobber_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = match args.first().map(|s| s.as_str()) {
            None => (
                format!(
                    "منع_المسح: {}\n",
                    if self.noclobber { "صح" } else { "خطأ" }
                ),
                0,
            ),
            Some("صح") | Some("خطأ") => {
                let value = args[0].as_str();
                self.noclobber = value == "صح";
                let mut out = format!("منع_المسح: {}\n", value);
                if let Some(w) = self.persist_history_setting("منع_المسح", value) {
                    out.push_str(&format!("تحذير: {}\n", w));
                }
                (out, 0)
            }
            Some(_) => ("منع_المسح تتوقع صح أو خطأ\n".to_string(), 2),
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// المرحلة (ج) — `توقف_عند_الخطأ [صح|خطأ]` (اجتماع
    /// 63 §78، الميزة 79): بلا معامل تعرض الحالي؛
    /// وبمعامل تضبط علم الجلسة وحده (بلا حفظ في الملف —
    /// فوضع ينهي الجلسات لا يُحفظ فيفاجئ جلسة قادمة)؛
    /// والقيمة السيئة مرفوضة بخطأ 2 ويبقى القديم.
    fn stop_on_error_setting(&mut self, args: Vec<String>) -> ReplOutput {
        let (output, code) = match args.first().map(|s| s.as_str()) {
            None => (
                format!(
                    "توقف_عند_الخطأ: {}\n",
                    if self.stop_on_error { "صح" } else { "خطأ" }
                ),
                0,
            ),
            Some("صح") | Some("خطأ") => {
                let value = args[0].as_str();
                self.stop_on_error = value == "صح";
                (format!("توقف_عند_الخطأ: {}\n", value), 0)
            }
            Some(_) => ("توقف_عند_الخطأ تتوقع صح أو خطأ\n".to_string(), 2),
        };
        self.pending_code = Some(code);
        ReplOutput {
            output,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    // المرحلة (هـ) — التحكم بالمهام (اجتماعا 64
    // و65 §79 و§80): معالجات عبارات الوظائف الست.
    // كل معالج يجمع المنتهية أولا (فالحالات محدَّثة)
    // ويلحق سطور الإشعار بمخرجه. رمز السطر: 0 نجاح،
    // و1 خطأ وظيفة (غائبة/منتهية/مُوقفة)، و2 معامل
    // تصفية غريب؛ و`قدم` و`انتظر %n` يرجعان رمز
    // الوظيفة نفسها كرمز السطر (-1 لقُتلت بإشارة
    // يصير 255 التفافا كرمز u8).

    /// مخرج عبارة مهام: سطور الإشعار أولا ثم المخرج
    /// الخاص، مع رمز معلق.
    fn jobs_out(&mut self, notes: Vec<String>, output: String, code: u8) -> ReplOutput {
        let mut full = String::new();
        for n in notes {
            full.push_str(&n);
            full.push('\n');
        }
        full.push_str(&output);
        self.pending_code = Some(code);
        ReplOutput {
            output: full,
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// حلّ معامل عبارة مهام (بلا معامل = الحالية).
    fn jobs_resolve(&self, args: &[String]) -> Result<u64, String> {
        let spec = args.first().map(|s| s.as_str()).unwrap_or("%");
        crate::processes::resolve_job_spec(spec)
    }

    /// `وظائف [جارية|منتهية]`: سطر لكل وظيفة
    /// (`[n]+ الأمر — الحالة`)، أو لا شيء بلا وظائف.
    fn jobs_list(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        let filter = args.first().map(|s| s.as_str());
        match filter {
            None | Some("جارية") | Some("منتهية") => {}
            Some(_) => {
                return self.jobs_out(notes, "وظائف تتوقع جارية أو منتهية\n".to_string(), 2);
            }
        }
        let (current, previous) = crate::processes::current_previous_jobs();
        let mut out = String::new();
        for job in crate::processes::list_jobs() {
            let show = match filter {
                None => true,
                Some("جارية") => job.status == crate::processes::JobStatus::Running,
                Some("منتهية") => job.status != crate::processes::JobStatus::Running,
                Some(_) => unreachable!(),
            };
            if show {
                out.push_str(&crate::processes::format_job_line(&job, current, previous));
                out.push('\n');
            }
        }
        self.jobs_out(notes, out, 0)
    }

    /// `قدم [%n]`: انتظار حاجب يبقي الوظيفة برمزها
    /// (بلا معامل = الحالية؛ وعلى منتهية يرجع رمزها
    /// فورا؛ وعلى مُوقفة خطأ).
    fn jobs_foreground(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        let number = match self.jobs_resolve(&args) {
            Ok(n) => n,
            Err(m) => return self.jobs_out(notes, format!("{}\n", m), 1),
        };
        match crate::processes::wait_job(number, 1, 1) {
            Ok(code) => self.jobs_out(notes, String::new(), code as u8),
            Err(e) => self.jobs_out(notes, format!("{}\n", e), 1),
        }
    }

    /// `أخر [%n]`: تأكيد — على جارية تطبع
    /// `[n]+ الأمر &` وترجع 0؛ وعلى منتهية أو مُوقفة
    /// خطأ «لا وظيفة موقوفة لتستأنف»؛ ولا يغيّر
    /// `آخر_خلفية`.
    fn jobs_background_confirm(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        let number = match self.jobs_resolve(&args) {
            Ok(n) => n,
            Err(m) => return self.jobs_out(notes, format!("{}\n", m), 1),
        };
        let (current, previous) = crate::processes::current_previous_jobs();
        match crate::processes::job_by_number(number) {
            Some(job) if job.status == crate::processes::JobStatus::Running => {
                let mark = if Some(number) == current {
                    "+"
                } else if Some(number) == previous {
                    "-"
                } else {
                    " "
                };
                let out = format!(
                    "[{}]{} {} &\n",
                    crate::processes::eastern(number),
                    mark,
                    job.command
                );
                self.jobs_out(notes, out, 0)
            }
            Some(_) => self.jobs_out(notes, "لا وظيفة موقوفة لتستأنف\n".to_string(), 1),
            None => self.jobs_out(
                notes,
                format!("وظيفة غير معروفة: %{}\n", number),
                1,
            ),
        }
    }

    /// `اقتل [%n|معرف]`: بالقوة القصوى وحده بلا
    /// اختيار إشارة (بلا معامل = الحالية؛ وعلى منتهية
    /// خطأ «الوظيفة منتهية»). المعامل العاري يُحل
    /// مواصفة وظيفة أولا ثم معرف عملية.
    fn jobs_kill(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        let spec = args.first().map(|s| s.as_str()).unwrap_or("%");
        if let Ok(number) = crate::processes::resolve_job_spec(spec) {
            return match crate::processes::kill_job(number, 1, 1) {
                Ok(()) => self.jobs_out(notes, String::new(), 0),
                Err(e) => self.jobs_out(notes, format!("{}\n", e), 1),
            };
        }
        if let Ok(id) = crate::processes::normalize_digits(spec).parse::<u64>() {
            return match crate::processes::kill_process(id, 1, 1) {
                Ok(()) => self.jobs_out(notes, String::new(), 0),
                Err(_) => self.jobs_out(
                    notes,
                    format!("وظيفة غير معروفة: {}\n", spec),
                    1,
                ),
            };
        }
        self.jobs_out(notes, format!("وظيفة غير معروفة: {}\n", spec), 1)
    }

    /// `انتظر [%n|أي]`: بلا معامل = الكل ويرجع 0؛
    /// و`%n` يرجع رمزها؛ و`أي` يرجع رمز أول منتهية
    /// ويطبع رقمها.
    fn jobs_wait(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        match args.first().map(|s| s.as_str()) {
            None => {
                crate::processes::wait_all_jobs(1, 1);
                self.jobs_out(notes, String::new(), 0)
            }
            Some("أي") => match crate::processes::wait_any_job(1, 1) {
                Ok((number, code)) => {
                    let out = format!("{}\n", crate::processes::eastern(number));
                    self.jobs_out(notes, out, code as u8)
                }
                Err(e) => self.jobs_out(notes, format!("{}\n", e), 1),
            },
            Some(spec) => {
                let number = match crate::processes::resolve_job_spec(spec) {
                    Ok(n) => n,
                    Err(m) => return self.jobs_out(notes, format!("{}\n", m), 1),
                };
                match crate::processes::wait_job(number, 1, 1) {
                    Ok(code) => self.jobs_out(notes, String::new(), code as u8),
                    Err(e) => self.jobs_out(notes, format!("{}\n", e), 1),
                }
            }
        }
    }

    /// `أبعد [%n|الكل]`: ينسى الوظيفة (بلا معامل =
    /// الحالية)؛ والمنتهية يُحفظ رمزها حتى يبقى
    /// `انتظر %n` قادرا على إرجاعه.
    fn jobs_disown(&mut self, args: Vec<String>) -> ReplOutput {
        let notes = self.poll_jobs();
        match args.first().map(|s| s.as_str()) {
            Some("الكل") => {
                crate::processes::disown_all_jobs();
                self.jobs_out(notes, String::new(), 0)
            }
            _ => {
                let number = match self.jobs_resolve(&args) {
                    Ok(n) => n,
                    Err(m) => return self.jobs_out(notes, format!("{}\n", m), 1),
                };
                match crate::processes::disown_job(number, 1, 1) {
                    Ok(()) => self.jobs_out(notes, String::new(), 0),
                    Err(e) => self.jobs_out(notes, format!("{}\n", e), 1),
                }
            }
        }
    }

    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): المنفذ على مستوى السطر. يُدعى
    // من `feed_inner_impl` بعد الأوامر وقبل العبارات
    // الكاملة؛ فيعيد `None` حين لا عمل توجيه في السطر
    // فيمر كما هو.
    fn try_redirect_line(&mut self, line: &str) -> Option<ReplOutput> {
        let parsed = match redirect::parse_line(line) {
            Ok(p) => p,
            Err(e) => {
                // المرحلة (ج) — الغموض الوحيد: عامل بلا
                // هدف (كقالب موجه ينتهي بـ`>` — «موجه_أول
                // جاهز>»). فغياب الهدف قد يكون قيمة
                // عبارة حرة لا توجيها ناقصا، فيمر السطر
                // للعبارات والكود (فيرفضه محلل اللغة
                // برمز 2 إن لم تطالب به عبارة)؛ أما عامل
                // بهدف حاضر فتوجيه بلا غموض فيبقى خطؤه هنا.
                if e.message == "توجيه بلا هدف" {
                    return None;
                }
                self.pending_code = Some(e.code);
                return Some(ReplOutput {
                    output: format!("{}\n", e.message),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                });
            }
        };
        if !parsed.has_work() {
            return None;
        }
        // مستند هنا: خزّن السطر وانتظر الجسم (يُسجَّل
        // السطر وحده مدخلا؛ وأسطر الجسم بيانات لا
        // تُسجَّل).
        if let Some((_, _, h)) = parsed.heredoc() {
            self.pending_heredoc = Some(PendingHeredoc {
                line: line.to_string(),
                delimiter: h.delimiter.clone(),
                strip_tabs: h.strip_tabs,
                body: String::new(),
            });
            self.history.record(line);
            return Some(ReplOutput {
                output: String::new(),
                prompt: self.prompt(),
                ended: false,
                needs_network: false,
            });
        }
        Some(self.run_parsed_line(&parsed, line, None))
    }

    /// المرحلة (ج) — نفّذ السطر المخزون لمستند هنا
    /// بعد اكتمال جسمه.
    fn run_heredoc_line(&mut self, line: &str, body: String) -> ReplOutput {
        match redirect::parse_line(line) {
            Ok(parsed) => self.run_parsed_line(&parsed, line, Some(body)),
            Err(e) => {
                self.pending_code = Some(e.code);
                ReplOutput {
                    output: format!("{}\n", e.message),
                    prompt: self.prompt(),
                    ended: false,
                    needs_network: false,
                }
            }
        }
    }

    /// المرحلة (ج) — نفّذ سطرا موجَّها/مأنبوبا: سجّله
    /// مدخلا واحدا، وأطفئ البث الحي (فالمخرج الملتقط
    // هو ما يُوجَّه لا ما يصل الطرفية مباشرة)، وعلّق
    // تسجيل المحفوظات (فالمراحل لا تُسجَّل منفردة)،
    // ثم استعد بعد التنفيذ.
    fn run_parsed_line(
        &mut self,
        parsed: &redirect::ParsedLine,
        line: &str,
        heredoc_body: Option<String>,
    ) -> ReplOutput {
        self.history.record(line);
        let old_live = self.engine.live_output();
        self.engine.set_live_output(false);
        self.history.suspend_recording();
        let out = self.run_segments(parsed, heredoc_body);
        self.history.resume_recording();
        self.engine.set_live_output(old_live);
        out
    }

    /// المرحلة (ج) — فشل توجيه: السطر كله يفشل قبل
    /// أي تنفيذ، بالرمز 1، ونص عربي، وبلا تنفيذ جزئي.
    fn redirect_fail(&mut self, message: String) -> ReplOutput {
        self.pipe_status = vec![1];
        self.pipe_status_set = true;
        self.pending_code = Some(1);
        ReplOutput {
            output: format!("{}\n", message),
            prompt: self.prompt(),
            ended: false,
            needs_network: false,
        }
    }

    /// المرحلة (ج) — نفّذ قطاعات السطر: أولا تحقق
    /// مسبق من كل التوجيهات بالترتيب (قراءة الملفات
    // وإنشاء/مسح ملفات الكتابة) — فأي فشل يفشل السطر
    // كله قبل أي تنفيذ؛ ثم التنفيذ مع الدوائر القصيرة
    // (`&&` تتخطى حين الرمز غير 0، و`||` حين 0).
    /// وتُبنى رموز الأنبوب من آخر قطاع منفَّذ، ويُضبط
    /// رمز السطر النهائي، وينهي `توقف_عند_الخطأ`
    /// الجلسة برمز الفشل حين يعمل.
    /// المرحلة (هـ) — هل نص المرحلة استدعاء `شغل`؟
    /// (كلمة أولى `شغل` ثم اسم ملف.)
    fn is_run_call(text: &str) -> bool {
        let mut words = text.split_whitespace();
        matches!(words.next(), Some("شغل"))
    }

    /// المرحلة (هـ) — هل نص المرحلة استدعاء إنشاء
    /// عملية (`عمليات.أنشئ` أو `أنشئ_عملية`)؟
    fn is_spawn_call(text: &str) -> bool {
        let t = text.trim_start();
        t.starts_with("عمليات.أنشئ") || t.starts_with("أنشئ_عملية")
    }

    /// المرحلة (هـ) — تحقق من صلاحية مقطع خلفي قبل
    /// تنفيذه (اجتماع 64 §79): مرحلة واحدة بلا أنبوب
    /// وبلا توجيه، ونصها `شغل <ملف>` أو إنشاء عملية؛
    /// وغيره خطأ صيغة رمز 2 بالرسالة نفسها.
    fn validate_background_segment(seg: &redirect::Segment) -> Result<(), String> {
        const MSG: &str = "الخلفية لعمليات النظام وشغل فقط";
        if seg.stages.len() != 1 {
            return Err(MSG.to_string());
        }
        let stage = &seg.stages[0];
        if stage.has_redirect() {
            return Err(MSG.to_string());
        }
        let text = stage.text.trim();
        if Self::is_run_call(text) || Self::is_spawn_call(text) {
            Ok(())
        } else {
            Err(MSG.to_string())
        }
    }

    /// المرحلة (هـ) — نفّذ مقطعا خلفيا صالحا، ويعيد
    /// (سطر الإنشاء `[n] id` أو رسالة الخطأ، الرمز).
    /// رمز السطر بعد الخلفية 0 دائما عند النجاح.
    fn run_background_segment(&mut self, seg: &redirect::Segment) -> (String, u8) {
        let text = seg.stages[0].text.trim();
        if Self::is_run_call(text) {
            return self.run_background_tool(text);
        }
        match self.eval_background_expr(text) {
            Ok(id) => match crate::processes::register_job(id, text) {
                Ok(n) => (
                    format!(
                        "[{}] {}\n",
                        crate::processes::eastern(n),
                        crate::processes::eastern(id)
                    ),
                    0,
                ),
                Err(e) => (format!("{}\n", e), 1),
            },
            Err((msg, code)) => (format!("{}\n", msg), code),
        }
    }

    /// المرحلة (هـ) — `شغل <ملف> &`: حلّ الملف كما
    /// في الأمامي (أخطاء 126/127 نفسها) ثم أنشئ
    /// برنامج نيفا كعملية ابن عبر الثنائي الحالي.
    fn run_background_tool(&mut self, text: &str) -> (String, u8) {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() != 2 {
            return ("شغّل تتوقع معاملا واحدا\n".to_string(), 2);
        }
        let name = words[1];
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return ("شغّل يقبل اسم ملف بلا مسار\n".to_string(), 2);
        }
        let file_name = if name.ends_with(".نيفا") {
            name.to_string()
        } else {
            format!("{}.نيفا", name)
        };
        let path = match crate::files::find_tool_file(&self.program_dir, &file_name) {
            crate::files::ToolLookup::Found { path, .. } => path,
            crate::files::ToolLookup::IsDir => {
                return (format!("{} مجلد وليس ملفا\n", file_name), 126);
            }
            crate::files::ToolLookup::Unreadable => {
                return (format!("تعذر قراءة الملف: {}\n", file_name), 126);
            }
            crate::files::ToolLookup::Missing => {
                return (
                    format!("الملف {} غير موجود — بحثت في $نيفا وفي ~\n", file_name),
                    127,
                );
            }
        };
        // الإنشاء نفسه أصلي فقط (WASM وأندرويد
        // يرفضانه كما يرفض `أنشئ`).
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            let _ = path;
            #[cfg(target_arch = "wasm32")]
            return ("العمليات غير مدعومة في متصفح الويب\n".to_string(), 1);
            #[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
            return (
                "تشغيل برامج خارجية غير مدعوم في التطبيق\n".to_string(),
                1,
            );
        }
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            match crate::processes::spawn_niva_file(&path, 1, 1) {
                Ok(id) => match crate::processes::register_job(id, text) {
                    Ok(n) => (
                        format!(
                            "[{}] {}\n",
                            crate::processes::eastern(n),
                            crate::processes::eastern(id)
                        ),
                        0,
                    ),
                    Err(e) => (format!("{}\n", e), 1),
                },
                Err(e) => (format!("{}\n", e), 1),
            }
        }
    }

    /// المرحلة (هـ) — قيّم تعبير إنشاء واحد
    /// (`عمليات.أنشئ "..."` أو `أنشئ_عملية ...`)
    /// وخذ معرفه عددا — بلا تسجيل محفوظات (فالسطر
    /// كله سُجّل مدخلا واحدا) وبلا طباعة قيمة.
    /// خطأ تحليلي/دلالي رمز 2، ووقت تشغيل برمزه.
    fn eval_background_expr(&mut self, text: &str) -> Result<u64, (String, u8)> {
        let tokens = match tokenize(text) {
            Ok(t) => t,
            Err(e) => return Err((e.to_string(), 2)),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                if INCOMPLETE_MARKERS.iter().any(|m| msg.contains(m)) {
                    return Err((
                        "الكتلة متعددة الأسطر لا تناسب الخلفية".to_string(),
                        2,
                    ));
                }
                return Err((msg, 2));
            }
        };
        if let Err(e) = self.analyzer.analyze_program(&program) {
            return Err((e.to_string(), 2));
        }
        let expr = match program.last().map(|s| &s.kind) {
            Some(StmtKind::ExprStmt { expr }) if program.len() == 1 => expr,
            _ => {
                return Err((
                    "الخلفية لعمليات النظام وشغل فقط".to_string(),
                    2,
                ))
            }
        };
        match self.engine.eval_one(expr) {
            Ok(Value::Number(n)) if n >= 0.0 && n.fract() == 0.0 => Ok(n as u64),
            Ok(other) => Err((
                format!("أنشئ ترجع معرفا عدديا، وليس {}", other.type_name()),
                1,
            )),
            Err(e) => Err((e.to_string(), runtime_error_code(&e))),
        }
    }

    fn run_segments(
        &mut self,
        parsed: &redirect::ParsedLine,
        heredoc_body: Option<String>,
    ) -> ReplOutput {
        // (0) المرحلة (هـ) — تحقق مسبق من المقاطع
        // الخلفية قبل أي تجهيز ملفات: مقطع خلفي غير
        // صالح يفشل السطر كله برمز 2 بلا تنفيذ جزئي
        // وبلا إنشاء/مسح ملفات توجيه.
        for (idx, segment) in parsed.segments.iter().enumerate() {
            if idx + 1 < parsed.segments.len()
                && parsed.segments[idx + 1].op == redirect::SegOp::Background
                && !segment.stages.is_empty()
            {
                if let Err(m) = Self::validate_background_segment(segment) {
                    self.pipe_status = vec![2];
                    self.pipe_status_set = true;
                    self.pending_code = Some(2);
                    return ReplOutput {
                        output: format!("{}\n", m),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    };
                }
            }
        }
        // (1) التحقق المسبق بالترتيب: حلّ المدخل
        // (ملف القراءة يُقرأ هنا كاملا)، وإنشاء/مسح
        // ملفات الكتابة (فالأمر الفاشل يمسح ملفه كما
        // في باش). ومفاتيح خريطة القيم: المسار + الإلحاق
        // + القراءة-الكتابة، فـ`&>` (وجهتان لملف واحد)
        // يُنشأ مرة واحدة.
        let mut read_cache: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut prepared: std::collections::HashSet<(String, bool, bool)> =
            std::collections::HashSet::new();
        for segment in &parsed.segments {
            for stage in &segment.stages {
                // مدخل المرحلة: ملف يُقرأ، أو جسم مستند
                // هنا، أو سلسلة هنا (بنصها + `\n`)، أو
                // مغلق (لا قراءة).
                match &stage.stdin {
                    redirect::StdinSrc::File(path) => {
                        if !read_cache.contains_key(path) {
                            match self.redirect_read(path) {
                                Ok(text) => {
                                    read_cache.insert(path.clone(), text);
                                }
                                Err(m) => return self.redirect_fail(m),
                            }
                        }
                    }
                    redirect::StdinSrc::Text(_) | redirect::StdinSrc::Closed => {}
                    redirect::StdinSrc::Terminal => {}
                }
                // ملفات الكتابة (الخرج والخطأ): أُنشئ أو
                // امسح أو اقرأ القديم (للقراءة-الكتابة).
                for dst in [&stage.stdout, &stage.stderr] {
                    if let redirect::OutDst::File {
                        path,
                        append,
                        read_write,
                        force,
                    } = dst
                    {
                        // الإلحاق والقراءة-الكتابة يحتاجان
                        // القديم بعد التنفيذ: يُقرأ هنا قبل
                        // أي مسح؛ والغائب قديمه فارغ (يُنشأ
                        // في التجهيز لا يفشل).
                        if (*read_write || *append) && !read_cache.contains_key(path) {
                            if self.redirect_exists(path) {
                                match self.redirect_read(path) {
                                    Ok(text) => {
                                        read_cache.insert(path.clone(), text);
                                    }
                                    Err(m) => return self.redirect_fail(m),
                                }
                            } else {
                                read_cache.insert(path.clone(), String::new());
                            }
                        }
                        let key = (path.clone(), *append, *read_write);
                        if prepared.contains(&key) {
                            continue;
                        }
                        prepared.insert(key);
                        if let Err(m) =
                            self.prepare_out_file(path, *append, *read_write, *force)
                        {
                            return self.redirect_fail(m);
                        }
                    }
                }
            }
        }
        // (2) التنفيذ.
        let mut line_output = String::new();
        let mut final_code: u8 = 0;
        let mut ended = false;
        let mut last_codes: Vec<u8> = Vec::new();
        let n_segments = parsed.segments.len();
        for (seg_idx, segment) in parsed.segments.iter().enumerate() {
            // الدائرة القصيرة: تخطَّ القطاعات اللاحقة
            // حين لا يمر الشرط.
            match segment.op {
                redirect::SegOp::First => {}
                redirect::SegOp::And if final_code != 0 => continue,
                redirect::SegOp::Or if final_code == 0 => continue,
                _ => {}
            }
            // المرحلة (هـ) — علامة خلفية فارغة أخيرة
            // (`شغل x &`): لا شيء لتنفيذه — المقطع
            // قبلها جرى خلفيا — وتُتخطى بلا أثر في
            // الرموز (فـ`last_codes` يبقى من قبله).
            if segment.stages.is_empty() {
                continue;
            }
            // المرحلة (هـ) — المقطع التالي رابطه خلفية
            // يعني هذا المقطع خلفي: ينفَّذ في الخلفية
            // (سطر `[n] id`) ويرجع 0 دائما، ثم يأتي
            // التالي فورا حسب رابطه.
            let background = seg_idx + 1 < n_segments
                && parsed.segments[seg_idx + 1].op == redirect::SegOp::Background;
            if background {
                let (text, code) = self.run_background_segment(segment);
                line_output.push_str(&text);
                final_code = code;
                last_codes = vec![code];
                continue;
            }
            let mut pipe_text: Option<String> = None;
            let mut codes: Vec<u8> = Vec::new();
            for (idx, stage) in segment.stages.iter().enumerate() {
                // مدخل المرحلة: الأولى من التوجيه،
                // واللاحقة من نص أنبوب سابقتها.
                let stdin = if idx == 0 {
                    // سلسلة هنا (نصها + `\n`)، ثم جسم
                    // المستند هنا، ثم ملف القراءة (مقروء
                    // مسبقا)، ثم المغلق، ثم الطرفية.
                    if let Some(text) = &stage.here_string {
                        StageStdin::Text(format!("{}\n", text))
                    } else if stage.heredoc.is_some() {
                        StageStdin::Text(heredoc_body.clone().unwrap_or_default())
                    } else {
                        match &stage.stdin {
                            redirect::StdinSrc::Terminal => StageStdin::Terminal,
                            redirect::StdinSrc::Closed => StageStdin::Closed,
                            redirect::StdinSrc::File(path) => StageStdin::Text(
                                read_cache.get(path).cloned().unwrap_or_default(),
                            ),
                            redirect::StdinSrc::Text(t) => StageStdin::Text(t.clone()),
                        }
                    }
                } else {
                    match pipe_text.take() {
                        Some(t) => StageStdin::Text(t),
                        None => StageStdin::Closed,
                    }
                };
                let outcome = self.run_stage(&stage.text, stdin);
                codes.push(outcome.code);
                final_code = outcome.code;
                // كتابة المخرجات إلى ملفاتها (بعد تنفيذ
                // المرحلة لا قبله — فالمحتوى من التنفيذ).
                let old_content = match &stage.stdout {
                    redirect::OutDst::File { path, .. } => {
                        read_cache.get(path).cloned().unwrap_or_default()
                    }
                    _ => String::new(),
                };
                if let Some(m) = self.apply_out_sinks(stage, &old_content, &outcome.stdout, &outcome.stderr) {
                    self.pipe_status = vec![1];
                    self.pipe_status_set = true;
                    self.pending_code = Some(1);
                    return ReplOutput {
                        output: format!("{}\n", m),
                        prompt: self.prompt(),
                        ended: false,
                        needs_network: false,
                    };
                }
                // نص الأنبوب للتالية: مخرجها حين وجهتها
                // أنبوب، ويُلحق خطأها حين وجهته أنبوب.
                let mut next = String::new();
                if stage.stdout == redirect::OutDst::Pipe {
                    next.push_str(&outcome.stdout);
                }
                if stage.stderr == redirect::OutDst::Pipe {
                    next.push_str(&outcome.stderr);
                }
                // ما بقي للطرفية (أو للواصفين) من أي
                // مرحلة — لا الأخيرة وحدها — يظهر في
                // مخرج السطر: الخرج أولا ثم الخطأ (فخطأ
                // مرحلة وسطى موجَّه للطرفية يظهر كما في
                // باش، لا يُفقد).
                match &stage.stdout {
                    redirect::OutDst::Terminal
                    | redirect::OutDst::Fd(1)
                    | redirect::OutDst::Fd(2) => {
                        line_output.push_str(&outcome.stdout);
                    }
                    _ => {}
                }
                match &stage.stderr {
                    redirect::OutDst::Terminal
                    | redirect::OutDst::Fd(1)
                    | redirect::OutDst::Fd(2) => {
                        line_output.push_str(&outcome.stderr);
                    }
                    _ => {}
                }
                let is_last = idx + 1 == segment.stages.len();
                if !is_last {
                    pipe_text = Some(next);
                }
                if outcome.ended {
                    ended = true;
                    break;
                }
            }
            last_codes = codes;
            if ended {
                break;
            }
        }
        self.pipe_status = if last_codes.is_empty() {
            vec![final_code]
        } else {
            last_codes
        };
        self.pipe_status_set = true;
        self.pending_code = Some(final_code);
        // `توقف_عند_الخطأ`: أي سطر ينتهي برمز غير 0
        // ينهي الجلسة بذلك الرمز (إلا حين أنهى السطر
        // الجلسة أصلا).
        if self.stop_on_error && final_code != 0 && !ended {
            ended = true;
        }
        ReplOutput {
            output: line_output,
            prompt: self.prompt(),
            ended,
            needs_network: false,
        }
    }

    /// المرحلة (ج) — نفّذ مرحلة واحدة بمدخل محلول:
    /// ضع المدخل في المضخة حول التنفيذ (نص يُقسَّم
    /// أسطر، ومغلق قائمة فارغة = نهاية فورا، وطرفية
    // بلا قائمة)، وامسحه بعدها دائما فلا يتسرب لسطر
    // قادم. ويمسح أي رمز معلق قبلها فلا يُقرأ قديم.
    fn run_stage(&mut self, text: &str, stdin: StageStdin) -> StageOutcome {
        self.pending_code = None;
        match stdin {
            StageStdin::Terminal => crate::stdin_pump::clear_redirect_stdin(),
            StageStdin::Text(t) => {
                crate::stdin_pump::set_redirect_stdin(split_lines(&t))
            }
            StageStdin::Closed => crate::stdin_pump::set_redirect_stdin(Vec::new()),
        }
        let out = self.run_stage_inner(text);
        crate::stdin_pump::clear_redirect_stdin();
        out
    }

    /// المرحلة (ج) — نفّذ نص مرحلة عبر مسار الإرسال
    /// نفسه للسطر العادي (أوامر، ثم عبارات كاملة،
    // ثم اختصارات وأوامر كاملة و`شغل`، ثم كود) مع
    // `عكس` قبلها. والفرق: الناتج يُفصل مخرجا
    // وخطأ ورمزا (فلا يُطبع هنا)؛ والكتلة متعددة
    // الأسطر مرفوضة (رمز 2) لأنها لا تناسب مرحلة
    // في سطر.
    fn run_stage_inner(&mut self, text: &str) -> StageOutcome {
        let text = text.trim();
        let empty = || StageOutcome {
            stdout: String::new(),
            stderr: String::new(),
            code: 0,
            ended: false,
        };
        if text.is_empty() {
            return empty();
        }
        // `عكس` قبل المرحلة: تُنفَّذ البقية ويُقلب
        // رمزها حين التكرار فردي (كما في feed_inner_impl).
        if let Some((count, rest)) = strip_invert_prefix(text) {
            if rest.is_empty() {
                return StageOutcome {
                    stdout: String::new(),
                    stderr: "عكس تتوقع أمرا بعدها\n".to_string(),
                    code: 2,
                    ended: false,
                };
            }
            if starts_with_exit_call(rest) {
                return StageOutcome {
                    stdout: String::new(),
                    stderr: "عكس لا تناسب الخروج\n".to_string(),
                    code: 2,
                    ended: false,
                };
            }
            let mut outcome = self.run_stage_inner(rest);
            if count % 2 == 1 && !outcome.ended {
                outcome.code = if outcome.code == 0 { 1 } else { 0 };
            }
            return outcome;
        }
        if let Some(cmd) = parse_command(text) {
            let _ = cmd;
            let out = self.handle_command(text);
            let code = self.pending_code.take().unwrap_or(0);
            // فصل الناتج: نجاح = مخرج، وفشل = خطأ.
            let (stdout, stderr) = if code == 0 {
                (out.output, String::new())
            } else {
                (String::new(), out.output)
            };
            return StageOutcome {
                stdout,
                stderr,
                code,
                ended: out.ended,
            };
        }
        // فصل الناتج: نجاح = مخرج، وفشل = خطأ.
        let split = |out: ReplOutput, code: u8| {
            let (stdout, stderr) = if code == 0 {
                (out.output, String::new())
            } else {
                (String::new(), out.output)
            };
            StageOutcome {
                stdout,
                stderr,
                code,
                ended: out.ended,
            }
        };
        if let Some(out) = self.try_full_phrase(text) {
            let code = self.pending_code.take().unwrap_or(0);
            return split(out, code);
        }
        if !self.line_is_code(text) {
            if let Some(out) = self.try_shortcut(text) {
                let code = self.pending_code.take().unwrap_or(0);
                return split(out, code);
            }
            if let Some(out) = self.try_full_command(text) {
                let code = self.pending_code.take().unwrap_or(0);
                return split(out, code);
            }
            if let Some(out) = self.try_run_command(text) {
                let code = self.pending_code.take().unwrap_or(0);
                return split(out, code);
            }
        }
        match self.eval(text) {
            ReplEval::Done { output } => StageOutcome {
                stdout: output,
                stderr: String::new(),
                code: 0,
                ended: false,
            },
            ReplEval::Failed { output, error, code } => StageOutcome {
                stdout: output,
                stderr: format!("{}\n", error),
                code,
                ended: false,
            },
            ReplEval::Exited { output, code } => StageOutcome {
                stdout: output,
                stderr: String::new(),
                code,
                ended: true,
            },
            ReplEval::Incomplete => StageOutcome {
                stdout: String::new(),
                stderr: "الكتلة متعددة الأسطر لا تناسب الأنبوب أو التوجيه\n".to_string(),
                code: 2,
                ended: false,
            },
        }
    }

    /// المرحلة (ج) — اقرأ ملف التوجيه عبر الدالة
    /// نفسها (`اقرأ_ملف`: سياسة الملفات والحد 5م).
    fn redirect_read(&mut self, path: &str) -> Result<String, String> {
        match self.engine.call_builtin(
            "اقرأ_ملف",
            vec![crate::value::Value::Str(path.to_string())],
            1,
            1,
        ) {
            Ok(crate::value::Value::Str(text)) => Ok(text),
            Ok(_) => Err(format!("لا يمكن قراءة الملف: {}", path)),
            Err(e) => Err(format!("{}", e)),
        }
    }

    /// المرحلة (ج) — اكتب ملف التوجيه عبر الدالة
    /// نفسها (`اكتب_ملف`: سياسة الملفات والحصة
    /// والكتابة الذرية).
    fn redirect_write(&mut self, path: &str, content: &str) -> Result<(), String> {
        match self.engine.call_builtin(
            "اكتب_ملف",
            vec![
                crate::value::Value::Str(path.to_string()),
                crate::value::Value::Str(content.to_string()),
            ],
            1,
            1,
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("{}", e)),
        }
    }

    /// المرحلة (ج) — هل المسار موجود (عبر `موجود`)؟
    fn redirect_exists(&mut self, path: &str) -> bool {
        matches!(
            self.engine.call_builtin(
                "موجود",
                vec![crate::value::Value::Str(path.to_string())],
                1,
                1,
            ),
            Ok(crate::value::Value::Bool(true))
        )
    }

    /// المرحلة (ج) — نوع المسار (عبر `نوع_المسار`):
    /// «ملف» أو «مجلد» أو «غير موجود».
    fn redirect_kind(&mut self, path: &str) -> String {
        match self.engine.call_builtin(
            "نوع_المسار",
            vec![crate::value::Value::Str(path.to_string())],
            1,
            1,
        ) {
            Ok(crate::value::Value::Str(kind)) => kind,
            _ => "غير موجود".to_string(),
        }
    }

    /// المرحلة (ج) — جهّز ملف كتابة قبل التنفيذ:
    /// `منع_المسح` يرفض `>` و`&>` (بلا قوة) على ملف
    // موجود؛ والإلحاق يمر بلا مسح؛ والقراءة-الكتابة
    // تمر بلا مسح (وملفها الغائب يُنشأ فارغا)؛
    // والكتابة العادية تُنشأ/تُمسح الآن (فالأمر
    // الفاشل يمسح ملفه كما في باش).
    fn prepare_out_file(
        &mut self,
        path: &str,
        append: bool,
        read_write: bool,
        force: bool,
    ) -> Result<(), String> {
        if self.noclobber && !force && !append && !read_write {
            if self.redirect_kind(path) == "ملف" {
                return Err(format!(
                    "منع_المسح يمنع الكتابة على ملف موجود: {} (استخدم >| للقوة)",
                    path
                ));
            }
        }
        if append || read_write {
            if !self.redirect_exists(path) {
                self.redirect_write(path, "")?;
            }
            return Ok(());
        }
        self.redirect_write(path, "")
    }

    /// المرحلة (ج) — ابنِ محتوى ملف كتابة بعد التنفيذ:
    /// الكتابة العادية = النص؛ والإلحاق = القديم +
    // النص؛ والقراءة-الكتابة = النص فوق القديم من
    // البداية مع بقاء ذيله (بالأحرف لا بالبايتات).
    fn write_out_file(
        &mut self,
        path: &str,
        append: bool,
        read_write: bool,
        old_content: &str,
        text: &str,
    ) -> Result<(), String> {
        let content = if read_write {
            let rest: String = old_content.chars().skip(text.chars().count()).collect();
            format!("{}{}", text, rest)
        } else if append {
            format!("{}{}", old_content, text)
        } else {
            text.to_string()
        };
        self.redirect_write(path, &content)
    }

    /// المرحلة (ج) — طبّق وجهتي مرحلة بعد تنفيذها:
    /// الوجهتان لملف واحد = كتابة مدمجة واحدة (الخرج
    // ثم الخطأ)؛ وإلا كل وجهة على حدة؛ والوجهة
    // `Fd(1)` أو `Fd(2)` أو الطرفية بلا كتابة (يظهر
    // نصها في مخرج السطر). وتعيد رسالة خطأ أو لا شيء.
    fn apply_out_sinks(
        &mut self,
        stage: &redirect::Stage,
        old_content: &str,
        stdout_text: &str,
        stderr_text: &str,
    ) -> Option<String> {
        let file_of = |dst: &redirect::OutDst| -> Option<(String, bool, bool, bool)> {
            if let redirect::OutDst::File {
                path,
                append,
                read_write,
                force,
            } = dst
            {
                Some((path.clone(), *append, *read_write, *force))
            } else {
                None
            }
        };
        let out_file = file_of(&stage.stdout);
        let err_file = file_of(&stage.stderr);
        match (&out_file, &err_file) {
            (Some((path, append, read_write, _)), Some((path2, _, _, _)))
                if path == path2 =>
            {
                let combined = format!("{}{}", stdout_text, stderr_text);
                if let Err(m) =
                    self.write_out_file(path, *append, *read_write, old_content, &combined)
                {
                    return Some(m);
                }
            }
            _ => {
                if let Some((path, append, read_write, _)) = &out_file {
                    if let Err(m) = self.write_out_file(
                        path,
                        *append,
                        *read_write,
                        old_content,
                        stdout_text,
                    ) {
                        return Some(m);
                    }
                }
                if let Some((path, append, read_write, _)) = &err_file {
                    if let Err(m) = self.write_out_file(
                        path,
                        *append,
                        *read_write,
                        old_content,
                        stderr_text,
                    ) {
                        return Some(m);
                    }
                }
            }
        }
        None
    }

    /// Session history, each line wrapped in bidi isolates so a
    /// terminal renders it as one isolated run (RTL display aid).
    pub fn show_history(&self) -> String {
        let mut out = String::from("ما كتبته في هذه الجلسة:\n");
        for entry in self.history.entries() {
            for part in entry.text.lines() {
                out.push(RLI);
                out.push_str(part);
                out.push(PDI);
                out.push('\n');
            }
        }
        out
    }

    pub fn reset(&mut self) {
        let trusted = self.trusted;
        let network_allowed = self.network_allowed;
        let server_allowed = self.server_allowed;
        let live_output = self.live_output;
        let downloads = self.downloads.clone();
        self.engine
            .reset(trusted, network_allowed, server_allowed, live_output, downloads);
        self.analyzer = SemanticAnalyzer::new();
        self.history.clear();
        self.pending.clear();
        // المرحلة (د) — الإكمال (اجتماع 67 §82):
        // المستعارات حالة جلسة — تُمسح مع :مسح؛
        // وحارس ملف الخروج يعود للجلسة الجديدة.
        self.aliases.clear();
        self.exit_file_ran = false;
        // الدفعة 1: جلسة جديدة — رموز الخروج من الصفر.
        self.last_exit_code = 0;
        self.quit_code = None;
        self.pending_code = None;
        self.invert_count = 0;
        self.in_not_found_hook = false;
    }
}

/// نص الإصدار المجرد وحده (مثال «١٫٠٫٠») — المصدر
/// الوحيد لرقم الإصدار؛ واللافتة تُبنى منه، فلا يفترقان
/// أبدا (اجتماع 32، السؤال السادس: `إصدار_المحرك()`).
pub fn version() -> &'static str {
    "١٫٠٫٠"
}

/// The greeting printed when a REPL session starts — the same
/// text the WASM console shows on load. Minimal by the owner's
/// no-explanation rule (2026-11-05): name + version only, like
/// بيئة الطرفية/Linux — no teaching, no how-to, no docs links.
/// نسخة الترمنال المضيف (بلاغ 2026-11-08: `اص` يعرض
/// نسخة المحرك ١٫٠٫٠ لا نسخة الترمنال الحقيقية).
/// التطبيق يمرّر نسخته في `NIVA_TERMINAL_VERSION`
/// (مثلا "1.2.11") عند تشغيل المحرك؛ والمحرك يقبلها
/// أرقاما ونقاطا لاتينية فقط، وإلا تجاهلها.
fn terminal_version() -> Option<String> {
    let raw = std::env::var("NIVA_TERMINAL_VERSION").ok()?;
    let raw = raw.trim();
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some(raw.to_string())
}

/// أرقام لاتينية ونقطة عشرية إلى أرقام مشرقية وفاصلة
/// عشرية عربية (مثلا "1.2.11" ← «١٫٢٫١١»).
fn eastern_version(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '0'..='9' => {
                char::from_u32('٠' as u32 + (c as u32 - '0' as u32)).unwrap_or(c)
            }
            '.' => '٫',
            other => other,
        })
        .collect()
}

/// اللافتة لنسخة ترمنال معطاة (دالة خالصة للاختبار):
/// بلا ترمنال — لافتة المحرك كما هي؛ ومعه — «ترمنال
/// نيفا X (محرك Y)» فيبقى رقم المحرك ظاهرا ولا يضيع.
/// `إصدار_المحرك()` لا يمسّه هذا أبدا (نسخة اللغة
/// للتوافق، لا نسخة التطبيق).
pub fn banner_for(terminal: Option<&str>) -> String {
    match terminal {
        Some(t) => format!(
            "ترمنال نيفا {} (محرك {})",
            eastern_version(t),
            version()
        ),
        None => format!("محرك نيفا {}", version()),
    }
}

pub fn banner() -> String {
    banner_for(terminal_version().as_deref())
}

/// رسالة الترحيب: تُطبع بعد اللافتة عند بدء التفاعلي
/// فقط — لا في تشغيل ملف ولا مع `--نسخه`. عربية كاملة
/// مختصرة: الوثائق والإبلاغ والأوامر اليومية. بلا سطر
/// مجتمع (لا قناة مخصصة لنيفا بعد — يُضاف حين يحددها
/// المالك، ولا يُختلق رابط).
pub fn welcome_text() -> String {
    "أهلا بك في نيفا\n\nالوثائق: https://github.com/RIO4140/niva\nبلّغ عن مشكلة: https://github.com/RIO4140/niva/issues\n\nأوامر يومية:\n ات — اعرض الملفات\n شغل <اسم> — شغّل ملف\n تثبيت <رابط> — ثبّت أداة\n حدث حزم — حدّث حزم نيفا\n تن — حدّث التطبيق".to_string()
}

/// `:مساعدة` output: a BARE command list — terminal convention,
/// not teaching. No explanations, no examples, no notes.
pub fn help_text() -> &'static str {
    ":مساعدة\n:اعرض\nمحفوظات [أمر]\nحرر <قديم> <جديد>\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:تنزيلات <مسار>\n:خروج [رمز]\nمعلومات\nأعد-الإعدادات\nأعد-الضبط\nمستعار <اسم> <نص>\nمستعارات\nاحذف_مستعار <اسم>\nأكمل [جزء]\nخيارات [مفتاح] [قيمة]\n"
}

pub enum ReplCommand {
    Help,
    Show,
    /// Task 2 (Meeting 25 plan): clear only the :اعرض
    /// history lines — variables and functions stay. The
    /// app's own :امسح-المحفوظات (v1.1.6) clears the app's
    /// arrow-key history; the engine honors the same name
    /// here for its own history (desktop / بيئة الطرفية / WASM).
    ClearHistory,
    Clear,
    /// Audit م-2: allow the network inside the running
    /// process (no restart). Sent by the host app on approval.
    AllowNetwork,
    /// Meeting 27: allow the server inside the running
    /// process (no restart). Sent by the host app on approval.
    AllowServer,
    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root inside the running process (no
    /// restart). Sent by the host app when storage
    /// permission is granted.
    SetDownloads(std::path::PathBuf),
    /// الدفعة 1 (اجتماع 54 §69): `:خروج [رمز]` — بلا
    /// معامل تنتهي الجلسة برمز آخر سطر، ومع معامل
    /// صحيح 0–255 تنتهي به؛ والمعامل السيئ خطأ
    /// برمز 2 والجلسة تستمر.
    Quit(Option<String>),
}

/// Short terminal commands: code -> (builtin, min args, max
/// args). The single place to change the codes if the owner
/// corrects them; the builtins are the implementation.
/// Rule (owner 2026-11-05 ~22:05): a code is one easy letter
/// from the first word + one easy letter from the second word
/// — ات = اعرض ملفات، حم = احذف مجلد.
/// How a shortcut's space-separated arguments are handled.
enum ShortcutKind {
    /// All arguments as strings (file builtins).
    StrArgs,
    /// First argument is a path, the rest joined with spaces
    /// as one string (اكتب_ملف content may hold spaces).
    RestJoined,
    /// Evaluate a constructed stdlib snippet (رياضيات /
    /// عشوائي / وقت / شبكة / عمليات).
    Snippet,
    /// A REPL command (:مساعدة / :اعرض / :مسح / :خروج).
    Command,
}

/// One row of the 31-shortcut table
/// (~/workspace/android-app/SHORTCUTS-31.md): the code, the
/// builtin or snippet key, the argument count range, and the
/// kind. App-level codes (جلسات، مسح الشاشة، ...) are NOT
/// here: the app intercepts them before sending the line to
/// the engine, so they never reach this table.
struct ShortcutDef {
    key: &'static str,
    min: usize,
    max: usize,
    kind: ShortcutKind,
}

/// هل هذا الرمز اختصار طرفي معروف؟ الدفعة 3
/// (الميزة 44): طبقة «اختصار» في وصف الأمر —
/// مصدر الحقيقة الوحيد جدول `shortcut_def` نفسه.
pub(crate) fn is_shortcut_code(code: &str) -> bool {
    shortcut_def(code).is_some()
}

fn shortcut_def(code: &str) -> Option<ShortcutDef> {
    Some(match code {
        // --- ملفات ومجلدات (18) ---
        // المرحلة (د) — الإكمال (اجتماع 69 §84): `ات`
        // تتسع إلى 0..N معاملات (مسار وأعلام).
        "ات" => ShortcutDef { key: "اعرض_الملفات", min: 0, max: usize::MAX, kind: ShortcutKind::StrArgs },
        "مح" => ShortcutDef { key: "المجلد_الحالي", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "شم" => ShortcutDef { key: "أنشئ_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حم" => ShortcutDef { key: "احذف_مجلد", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "حف" => ShortcutDef { key: "احذف_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "اف" => ShortcutDef { key: "اقرأ_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شف" => ShortcutDef { key: "أنشئ_ملف", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عب" => ShortcutDef { key: "اعرض_البداية", min: 1, max: 2, kind: ShortcutKind::StrArgs },
        "عن" => ShortcutDef { key: "اعرض_النهاية", min: 1, max: 2, kind: ShortcutKind::StrArgs },
        "عس" => ShortcutDef { key: "عد_الأسطر", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عك" => ShortcutDef { key: "عد_الكلمات", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عش" => ShortcutDef { key: "اعرض_الشجرة", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        "حج" => ShortcutDef { key: "حجم_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "شر" => ShortcutDef { key: "أنشئ_رابط", min: 2, max: 2, kind: ShortcutKind::StrArgs },
        "اك" => ShortcutDef { key: "اكتب_ملف", min: 2, max: usize::MAX, kind: ShortcutKind::RestJoined },
        "ام" => ShortcutDef { key: "نوع_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "تم" => ShortcutDef { key: "تفاصيل_المسار", min: 1, max: 1, kind: ShortcutKind::StrArgs },
        "عم" => ShortcutDef { key: "عد_الملفات", min: 0, max: 1, kind: ShortcutKind::StrArgs },
        // --- أوامر الجلسة (1) ---
        "اس" => ShortcutDef { key: ":مساعدة", min: 0, max: 0, kind: ShortcutKind::Command },
        // --- إصدار المحرك (1) ---
        "اص" => ShortcutDef { key: "النسخة", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        // --- نص وأعداد (1) ---
        "رع" => ShortcutDef { key: "عشوائي", min: 0, max: 0, kind: ShortcutKind::Snippet },
        // --- وقت وشبكة (4) ---
        "تا" => ShortcutDef { key: "التاريخ_الآن", min: 0, max: 0, kind: ShortcutKind::StrArgs },
        "وق" => ShortcutDef { key: "وقت", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نو" => ShortcutDef { key: "نوم", min: 1, max: 1, kind: ShortcutKind::Snippet },
        "اج" => ShortcutDef { key: "اجلب", min: 1, max: 1, kind: ShortcutKind::Snippet },
        // --- عمليات (3) ---
        "عع" => ShortcutDef { key: "الحالية", min: 0, max: 0, kind: ShortcutKind::Snippet },
        "نب" => ShortcutDef { key: "أنشئ_برنامج", min: 1, max: usize::MAX, kind: ShortcutKind::Snippet },
        "اب" => ShortcutDef { key: "أوقف_برنامج", min: 1, max: 1, kind: ShortcutKind::Snippet },
        _ => return None,
    })
}

/// Parse a shortcut argument as a number: Arabic digits
/// (٠-٩) and the Arabic decimal separator (٫) are cleaned
/// first, like the عدد builtin does.
fn parse_shortcut_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
            '٫' => '.',
            other => other,
        })
        .collect();
    cleaned.parse::<f64>().ok()
}

/// Display a shortcut result the terminal way: a file list
/// prints one name per line, a path prints alone, a لاشيء
/// result prints nothing.
fn format_shortcut_output(value: &Value) -> String {
    match value {
        Value::List(items) => {
            let mut out = String::new();
            for item in items {
                out.push_str(&item.fmt_value());
                out.push('\n');
            }
            out
        }
        Value::Nil => String::new(),
        other => format!("{}\n", other.fmt_value()),
    }
}

pub fn parse_command(line: &str) -> Option<ReplCommand> {
    let trimmed = line.trim();
    // Meeting 50 §64: `:تنزيلات <مسار>` carries its
    // argument, so it cannot match exactly — strip the
    // prefix first; an empty path is not a command.
    if let Some(rest) = trimmed.strip_prefix(":تنزيلات ") {
        let path = rest.trim();
        if path.is_empty() {
            return None;
        }
        return Some(ReplCommand::SetDownloads(std::path::PathBuf::from(path)));
    }
    // الدفعة 1 (اجتماع 54 §69): `:خروج [رمز]` و`خروج
    // [رمز]` — معامل واحد اختياري (يُفحص في
    // handle_command)؛ و`خروج(٥)` كود لا أمر،
    // و`خروجية` اسم آخر لا أمر.
    for prefix in [":خروج", "خروج"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            if rest.is_empty() {
                return Some(ReplCommand::Quit(None));
            }
            if rest.starts_with(char::is_whitespace) {
                let arg = rest.trim();
                if !arg.is_empty() {
                    return Some(ReplCommand::Quit(Some(arg.to_string())));
                }
            }
        }
    }
    match trimmed {
        ":مساعدة" => Some(ReplCommand::Help),
        ":اعرض" => Some(ReplCommand::Show),
        ":امسح-المحفوظات" => Some(ReplCommand::ClearHistory),
        ":مسح" => Some(ReplCommand::Clear),
        ":اسمح-بالشبكة" => Some(ReplCommand::AllowNetwork),
        ":اسمح-بالخادم" => Some(ReplCommand::AllowServer),
        _ => None,
    }
}

/// الدفعة 1 — `عكس` (اجتماع 54 §69): تجريد كل بادئات
/// `عكس` المتتالية من أول السطر (كلمة كاملة يليها
/// فراغ أو نهاية)؛ تعيد (العدد، الباقي) أو لا شيء
/// حين لا يبدأ السطر بها.
fn strip_invert_prefix(line: &str) -> Option<(usize, &str)> {
    let mut rest = line;
    let mut count = 0;
    loop {
        let trimmed = rest.trim_start();
        let Some(after) = trimmed.strip_prefix("عكس") else {
            break;
        };
        // حد الكلمة: بعدها فراغ أو نهاية — فـ`عكسية`
        // ليست بادئة.
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            count += 1;
            rest = after;
        } else {
            break;
        }
    }
    if count == 0 {
        None
    } else {
        Some((count, rest.trim_start()))
    }
}

/// الدفعة 1: هل يبدأ السطر باستدعاء `خروج` ككود
/// (`خروج` وحدها أو `خروج(`)؟ — و`خروج_شيء` اسم
/// آخر لا يُحسب. (`خروج <معامل>` أمر يمسكه
/// parse_command قبله.)
fn starts_with_exit_call(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(after) = trimmed.strip_prefix("خروج") else {
        return false;
    };
    after.is_empty() || after.starts_with('(')
}

/// الدفعة 1: حلّل معامل `:خروج` رمزا صحيحا 0–255
/// (أرقام عربية أو غربية)؛ وإلا لا شيء — فيرفض
/// الأمر برمز 2 والجلسة تستمر.
fn parse_exit_code_arg(s: &str) -> Option<u8> {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10).unwrap(),
            other => other,
        })
        .collect();
    match cleaned.parse::<i64>() {
        Ok(n) if (0..=255).contains(&n) => Some(n as u8),
        _ => None,
    }
}

/// مجلد الإعدادات (اجتماع 45 §59، البندان 36 و37):
/// `~/إعدادات/` — مرئي في البيت، يحرره المستخدم،
/// لا مخفي.
pub(crate) fn settings_dir(program_dir: &Path) -> PathBuf {
    program_dir.join("home").join("إعدادات")
}

/// ملف الإعدادات: `~/إعدادات/niva.properties`.
pub(crate) fn settings_path(program_dir: &Path) -> PathBuf {
    settings_dir(program_dir).join("niva.properties")
}

/// المسار القديم قبل الفصل (في جذر مجلد البرنامج).
fn legacy_settings_path(program_dir: &Path) -> PathBuf {
    program_dir.join("niva.properties")
}

/// مفاتيح متقاعدة سبق تحويلها (البند 75): (قديم، جديد).
/// لا مفتاح أُعيدت تسميته بعد حتى الآن — الجدول فارغ
/// عمدا، والآلية (migrate_retired_keys + إعادة الكتابة)
/// جاهزة للتحويلة الأولى حين يأمر بها المالك.
const RETIRED_KEYS: &[(&str, &str)] = &[];

/// حوّل المفاتيح المتقاعدة في نص ملف إعدادات إلى
/// أسمائها الجديدة (البند 75): دالة خالصة للاختبار.
/// تعيد النص الجديد وسطور الإشعار («المفتاح القديم X
/// صار Y») — سطر واحد لكل تحويلة.
pub(crate) fn migrate_retired_keys(
    content: &str,
    table: &[(&str, &str)],
) -> (String, Vec<String>) {
    if table.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let mut out = String::new();
    let mut notices = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        let Some(eq) = line.find('=') else {
            out.push_str(raw);
            out.push('\n');
            continue;
        };
        let key = line[..eq].trim();
        match table.iter().find(|(old, _)| *old == key) {
            Some((old, new)) => {
                out.push_str(new);
                out.push('=');
                out.push_str(line[eq + 1..].trim());
                out.push('\n');
                notices.push(format!("المفتاح القديم {} صار {}", old, new));
            }
            None => {
                out.push_str(raw);
                out.push('\n');
            }
        }
    }
    (out, notices)
}

/// إدخالات ملف إعدادات: أزواج (مفتاح، قيمة) بلا
/// تعليقات ولا سطور تالفة.
fn parse_settings_entries(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        out.push((
            line[..eq].trim().to_string(),
            line[eq + 1..].trim().to_string(),
        ));
    }
    out
}

/// اقرأ ملف الإعدادات (اجتماع 45 §59، البندان 36 و75):
/// الجديد أولا (`~/إعدادات/niva.properties`)؛ وإن غاب
/// والقديم موجود نُقل إليه (وإن تعذّر النقل قُرئ القديم
/// مع إشعار بإعادة المحاولة لاحقا)؛ والمفاتيح المتقاعدة
/// تتحول عند القراءة ويُعاد كتابة الملف بها. تعيد
/// (الإدخالات، الإشعارات).
pub(crate) fn read_settings(program_dir: &Path) -> (Vec<(String, String)>, Vec<String>) {
    let mut notices = Vec::new();
    let path = settings_path(program_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => {
            let (migrated, mut n) = migrate_retired_keys(&t, RETIRED_KEYS);
            let converted = !n.is_empty();
            notices.append(&mut n);
            if converted {
                let _ = crate::install::write_atomic(&path, migrated.as_bytes());
            }
            migrated
        }
        Err(_) => {
            let legacy = legacy_settings_path(program_dir);
            match std::fs::read_to_string(&legacy) {
                Ok(old) => {
                    let (migrated, mut n) = migrate_retired_keys(&old, RETIRED_KEYS);
                    notices.append(&mut n);
                    let _ = std::fs::create_dir_all(settings_dir(program_dir));
                    match crate::install::write_atomic(&path, migrated.as_bytes()) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&legacy);
                            migrated
                        }
                        Err(_) => {
                            notices.push(
                                "تعذّر نقل ملف الإعدادات إلى ~/إعدادات/niva.properties — سأستخدم الملف القديم وأعيد المحاولة لاحقا"
                                    .to_string(),
                            );
                            old
                        }
                    }
                }
                Err(_) => return (Vec::new(), notices),
            }
        }
    };
    let mut entries = parse_settings_entries(&text);
    // (41) قيمة بلا بايت فارغ: القيمة غير الصالحة تُسقط
    // مع إشعار يسمّي المفتاح، بلا إيقاف للجلسة.
    entries.retain(|(key, value)| {
        if crate::env::valid_value(value) {
            true
        } else {
            notices.push(format!("قيمة المفتاح {} غير صالحة — أُسقطت", key));
            false
        }
    });
    (entries, notices)
}

/// قفل النقل (البند 50): ملف فارغ عادي باسم المهمة
/// تحت `<program_dir>/.نيفا/` — يُنشأ فقط بعد النجاح
/// الكامل للنقل.
const MOVE_LOCK_NAME: &str = "نقل-البيت.قفل";

/// (اجتماع 49 §63) علامات شجرة مصدر — مجلد ترمنال
/// لن يحمل أيا منها أبدا.
const SOURCE_CHECKOUT_MARKERS: &[&str] = &[
    "Cargo.toml",
    ".git",
    "package.json",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Makefile",
    "CMakeLists.txt",
];

/// (اجتماع 49 §63) هل هذا المجلد شجرة مصدر لا مجلد
/// ترمنال؟ — وجود أي علامة من القائمة يكفي.
fn is_source_checkout(program_dir: &Path) -> bool {
    SOURCE_CHECKOUT_MARKERS
        .iter()
        .any(|m| program_dir.join(m).exists())
}

/// أسماء الجذر التي لا تنتقل إلى البيت أبدا (اجتماع 45
/// §59، البند 42 + استثناءان موثقان في CATALOG-43):
/// - home/ و.نيفا/ وtmp/ وniva.properties وملفات
///   الجلسات (session-*) — استثناءات الاجتماع الأربعة.
/// - `إعدادات/` — مجلد الإعدادات الجديد نفسه.
/// - `*.نيفا` — ملفات البرامج المثبتة: `استورد` يبحث
///   في مجلد البرنامج وحده، فنقلها يكسر الاستيراد.
/// - `*.نيفاب` — مصنوع مبني: منتج بناء لا ملف قديم
///   سائب، فتشغيله لا يرحّله.
/// - `بيان.حزمة` و`قفل.حزمة` — بيان المشروع وقفله:
///   يُقرآن من جذر مجلد البرنامج (`اقرأ_مشروع` و
///   `locked_version`)، فنقلهما يكسر الاستيراد
///   المقيَّد (المفسر يأخذ الأحدث بدلا من المقيَّد).
/// - `حول-نيفا.md` — وثيقة يولّدها التطبيق ويعيد
///   كتابتها في الجذر.
fn stays_at_root(name: &str) -> bool {
    matches!(
        name,
        "home" | ".نيفا" | "tmp" | "إعدادات" | "niva.properties" | "حول-نيفا.md"
            | "بيان.حزمة" | "قفل.حزمة"
    ) || name.starts_with("session-")
        || name.ends_with(".نيفا")
        || name.ends_with(".نيفاب")
}

/// إنشاء مجلدات الجلسة وفحص الكتابة فقط (اجتماع 45
/// §59، البنود 42 و51): home/ وtmp/ و~/إعدادات/
/// و.نيفا/، ثم ملف اختبار في tmp/ (خطأ قاتل يسمّي
/// المجلد). بلا نقل وبلا قفل — وهذا ما يحتاجه
/// المستقل (مجلد الملف التنفيذي ليس مجلد ترمنال،
/// والنقل فيه يحرّك الملف التنفيذي نفسه).
pub fn ensure_dirs(program_dir: &Path) -> Result<(), String> {
    let policy = crate::permissions::FilePolicy::restricted(program_dir.to_path_buf());
    let home = program_dir.join("home");
    let tmp = program_dir.join("tmp");
    let internal = program_dir.join(".نيفا");
    // المجلدات أولا — بلا رسالة للمستخدم إن نجحت.
    for dir in [&home, &tmp, &internal, &settings_dir(program_dir)] {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!(
                "تعذّر إنشاء مجلد {}: {}",
                policy.display_path(dir),
                e
            )
        })?;
    }
    // (51) فحص الكتابة: ملف اختبار في tmp/ — والخطأ
    // قاتل يسمّي المجلد، ولا يبدأ الدخول.
    let probe = tmp.join(".نيفا-اختبار-كتابة");
    match std::fs::write(&probe, "اختبار") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(_) => {
            return Err(format!(
                "مجلد الملفات غير قابل للكتابة: {} — تحقق من صلاحيات المجلد ثم أعد المحاولة",
                policy.display_path(&tmp)
            ));
        }
    }
    Ok(())
}

/// تجهيز أول تشغيل (اجتماع 45 §59، البنود 35 و36 و42
/// و50 و51): أنشئ home/ وtmp/ و~/إعدادات/ و.نيفا/؛
/// وافحص قابلية كتابة مجلد الملفات بملف اختبار في tmp/
/// (خطأ قاتل يسمّي المجلد)؛ وانقل ملفات الجذر القديمة
/// إلى البيت مرة واحدة (بقفل)؛ وانقل الإعدادات القديمة
/// إلى ~/إعدادات/. تعيد إشعارات تُطبع بعد اللافتة، أو
/// رسالة قاتلة تمنع بدء الدخول.
pub fn ensure_first_run(program_dir: &Path) -> Result<Vec<String>, String> {
    let mut notices = Vec::new();
    let policy = crate::permissions::FilePolicy::restricted(program_dir.to_path_buf());
    let home = program_dir.join("home");
    // هل كان البيت موجودا قبل هذا التشغيل؟ — يُفحص
    // قبل إنشائه، لأن القفل مع بيت أُعيد إنشاؤه الآن
    // حالة غير متسقة (البند 50).
    let home_existed = home.is_dir();
    // المجلدات وفحص الكتابة أولا.
    ensure_dirs(program_dir)?;
    // (36) نقل الإعدادات القديمة — read_settings يفعله
    // عند القراءة؛ وهنا نفعله مبكرا ليظهر التخطيط
    // الجديد فورا.
    let (_, mut settings_notices) = read_settings(program_dir);
    notices.append(&mut settings_notices);
    // (35/42/50) نقل البيت مرة واحدة، بقفل.
    let lock = program_dir.join(".نيفا").join(MOVE_LOCK_NAME);
    if lock.exists() {
        // القفل موجود لكن البيت لم يكن موجودا قبل هذا
        // التشغيل (أُعيد إنشاؤه فارغا أعلاه): حالة غير
        // متسقة — الخطأ يسمّي القفل بصيغته القصيرة
        // ويطلب حذفه يدويا لإعادة المحاولة.
        if !home_existed {
            return Err(format!(
                "البيت غير موجود رغم وجود القفل {} — احذف الملف يدويا لإعادة المحاولة",
                format!(".نيفا/{}", MOVE_LOCK_NAME)
            ));
        }
        return Ok(notices);
    }
    // (اجتماع 46 §60) مجلد مشروع: محتوياته هي
    // المشروع نفسه لا ملفات قديمة سائبة — يُتخطى
    // النقل كاملا (نقل `مصدر/` أو `حزم/` يكسر
    // البرنامج الجاري واستيراده)، ويُنشأ القفل بصمت.
    let is_project = program_dir.join("بيان.حزمة").is_file()
        || program_dir.join("حزم").is_dir()
        || program_dir.join("مصدر/رئيسي.نيفا").is_file();
    if is_project {
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    // (اجتماع 49 §63) حارس مجلد المصدر: مجلد فيه علامة
    // شجرة مصدر ليس مجلد ترمنال أبدا — فالنقل يُتخطى
    // كاملا ويُنشأ القفل بصمت (الترحيل «غير منطبق»).
    // بلا هذا الحارس نقلت سلسلة batch2e شجرة مصدر
    // المحرك كاملة إلى home/ (اختبار REPL بلا مجلد
    // مؤقت ورث جذر المحرك فصار مجلد برنامج).
    if is_source_checkout(program_dir) {
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    let mut to_move: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(program_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if stays_at_root(&name) {
                continue;
            }
            to_move.push((entry.path(), name));
        }
    }
    if to_move.is_empty() {
        // لا شيء لينقل: القفل يثبّت الاكتمال.
        let _ = std::fs::write(&lock, "");
        return Ok(notices);
    }
    let mut moved_names: Vec<String> = Vec::new();
    for (path, name) in &to_move {
        let dest = home.join(name);
        if dest.exists() {
            // هدف بنفس الاسم في البيت: لا نكتب فوقه —
            // الحالة غير متسقة، بلا قفل، وتُعاد المحاولة
            // تلقائيا في المرة التالية.
            return Err(format!(
                "تعذّر نقل {} إلى البيت — ملف بنفس الاسم موجود في ~ — ستعاد المحاولة تلقائيا في المرة التالية",
                policy.display_path(path)
            ));
        }
        if let Err(e) = std::fs::rename(path, &dest) {
            return Err(format!(
                "تعذّر نقل {} إلى البيت ({}) — ستعاد المحاولة تلقائيا في المرة التالية",
                policy.display_path(path),
                e
            ));
        }
        moved_names.push(name.clone());
    }
    // النجاح الكامل وحده ينشئ القفل.
    let _ = std::fs::write(&lock, "");
    // رسالة واحدة واضحة بأسماء المنقولات.
    let shown: Vec<&str> = moved_names.iter().take(5).map(|s| s.as_str()).collect();
    let mut msg = format!("نُقلت ملفاتك القديمة إلى البيت (~): {}", shown.join("، "));
    if moved_names.len() > 5 {
        msg.push_str(&format!("، و {} أخرى", moved_names.len() - 5));
    }
    notices.push(msg);
    Ok(notices)
}

/// The interactive loop: read lines from stdin, feed them to
/// the stateful `Repl`, print what it returns. Returns SUCCESS
/// on :خروج or EOF. The session runs on the compiler (VM) by
/// default; `use_interpreter` (CLI `--مفسر`) runs it on the
/// tree-walking interpreter instead, for diagnosis only.
pub fn run_repl(
    trusted: bool,
    network_allowed: bool,
    server_allowed: bool,
    local_hosts: &[String],
    use_interpreter: bool,
    downloads: Option<std::path::PathBuf>,
    // المرحلة (د) — الإكمال (اجتماع 68 §83): علما بدء
    // التشغيل التفاعلي.
    skip_startup: bool,
    startup_file: Option<std::path::PathBuf>,
) -> ExitCode {
    let mut repl = if use_interpreter {
        Repl::new_interpreter()
    } else {
        Repl::new()
    };
    repl.set_trusted(trusted);
    repl.set_network_allowed(network_allowed);
    repl.set_server_allowed(server_allowed);
    repl.set_skip_startup(skip_startup);
    if let Some(f) = startup_file {
        repl.set_startup_file(f);
    }
    // Meeting 50 §64: the downloads root, when the
    // launcher passed one (validated in main.rs).
    if let Some(d) = downloads {
        let _ = repl.set_downloads(d);
    }
    // اجتماع 32، السؤال الثامن: REPL التفاعلي يبث
    // `اطبع` حية — السطر يصل فور إنتاجه لا بعد الخروج.
    // (WASM لا يفعّله أبدا: يعرض result.output كما كان.)
    repl.set_live_output(true);
    for host in local_hosts {
        repl.allow_local_network_host(host);
    }
    // اجتماع 45 §59: تجهيز أول تشغيل (مجلدات + نقل
    // البيت + الإعدادات) — والخطأ القاتل يمنع الدخول.
    let notices = match ensure_first_run(&repl.program_dir) {
        Ok(n) => n,
        Err(msg) => {
            eprintln!("{}", msg);
            return ExitCode::FAILURE;
        }
    };
    println!("{}", banner());
    for n in &notices {
        println!("{}", n);
    }
    if repl.welcome_enabled() {
        println!("{}", welcome_text());
    }
    // ملف البدء والمقتطفات (البنود 44–46 و111) — الخطأ
    // فيهما لا يمنع الدخول.
    let startup = repl.run_startup_files();
    if !startup.is_empty() {
        print!("{}", startup);
    }
    // الدفعة 6 (الميزة 150): تحميل ملف المحفوظات عند
    // الدخول — بعد ملفات البدء (فسطورها لا تُسجَّل).
    if let Some(warn) = repl.load_history_file() {
        println!("{}", warn);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        // المرحلة (هـ) — الجمع والإشعار عند الموجه
        // (اجتماع 65 §80): وظائف انتهت منذ الموجه
        // السابق تُجمَع وتُبلَّغ هنا قبل الموجه (لا
        // أثناء تنفيذ سطر) — ثم أمر ما قبل الموجه.
        for note in repl.poll_jobs() {
            println!("{}", note);
        }
        // الدفعة 7 (اجتماع 61 §76، الميزة 175): أمر ما
        // قبل الموجه ينفذ قبل كل موجه أول — بما فيه
        // الأول — ومخرجه يُطبع قبل الموجه مباشرة.
        let hook_out = repl.before_prompt();
        if !hook_out.is_empty() {
            print!("{}", hook_out);
        }
        print!("{}", repl.prompt());
        if out.flush().is_err() {
            return ExitCode::FAILURE;
        }
        // مهمة 22 (شبكة.نزّل): القراءة عبر مضخة stdin
        // لا مباشرة — خيط المضخة وحده يملك stdin،
        // والمستهلكون (ومنهم `ادخل` داخل feed وانتظار
        // الجلب) يأخذون الأسطر من قناتها. قفل المهمة 20
        // (لكل قراءة لا لكل الحلقة) صار غير ذي موضوع
        // هنا: لا قفل إطلاقا على هذا الخيط بعد اليوم.
        // وسطور الإلغاء تُتخطى هنا بصمت (بلا جلب جارٍ
        // لا أثر لها)؛ وEOF يخرج نظيفا كما كان.
        // الدفعة 7 (الميزة 176): بمهلة خمول مضبوطة
        // تُقرأ بمهلة — وإن انتهت بلا إدخال طُبعت رسالة
        // الانتهاء وخرجت الجلسة كما عند EOF تماما
        // (حفظ المحفوظات، ورمز آخر سطر).
        let line = if repl.timeout_secs() == 0 {
            match crate::stdin_pump::stdin_line() {
                Some(l) => l,
                None => {
                    // EOF (Ctrl-D): exit cleanly.
                    // الدفعة 1 (اجتماع 54 §69): برمز آخر
                    // سطر مكتمل، لا 0 دائما.
                    let result = repl.eof();
                    print!("{}", result.output);
                    return ExitCode::from(repl.quit_code());
                }
            }
        } else {
            match crate::stdin_pump::stdin_line_timeout(
                std::time::Duration::from_secs(repl.timeout_secs()),
            ) {
                crate::stdin_pump::TimeoutLine::Line(l) => l,
                crate::stdin_pump::TimeoutLine::Eof => {
                    let result = repl.eof();
                    print!("{}", result.output);
                    return ExitCode::from(repl.quit_code());
                }
                crate::stdin_pump::TimeoutLine::Timeout => {
                    println!("{}", crate::prompts::TIMEOUT_MESSAGE);
                    let result = repl.eof();
                    print!("{}", result.output);
                    return ExitCode::from(repl.quit_code());
                }
            }
        };
        let result = repl.feed(&line);
        // Audit م-6: structured control signals on stderr,
        // only when the host opts in (NIVA_CONTROL_CHANNEL
        // =stderr) — the desktop/بيئة الطرفية REPL never shows them.
        // The \u{0001} prefix keeps a control line distinct
        // from any other stderr text (panics included).
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            match repl.last_control() {
                ReplControl::NetworkNeeded => eprintln!("\u{0001}نيفا-تحكم:شبكة"),
                ReplControl::ServerNeeded => eprintln!("\u{0001}نيفا-تحكم:خادم"),
                ReplControl::SessionCleared => eprintln!("\u{0001}نيفا-تحكم:مسح"),
                // اجتماع 45 §59 (البند 19): إعادة قراءة
                // الإعدادات — التطبيق يعيد التطبيق الحي.
                ReplControl::SettingsReload => eprintln!("\u{0001}نيفا-تحكم:إعدادات"),
                ReplControl::None => {}
            }
        }
        print!("{}", result.output);
        if result.ended {
            // الدفعة 1 (اجتماع 54 §69): رمز الخروج
            // الصريح من :خروج، أو رمز آخر سطر من
            // خروج() — لا 0 دائما.
            return ExitCode::from(repl.quit_code());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done_out(r: ReplEval) -> String {
        match r {
            ReplEval::Done { output } => output,
            ReplEval::Failed { output, error, code } => {
                panic!(
                    "expected Done, got Failed: output={} error={} code={}",
                    output, error, code
                )
            }
            ReplEval::Exited { output, code } => {
                panic!("expected Done, got Exited: output={} code={}", output, code)
            }
            ReplEval::Incomplete => panic!("expected Done, got Incomplete"),
        }
    }

    fn is_failed(r: ReplEval) -> bool {
        matches!(r, ReplEval::Failed { .. })
    }

    #[test]
    fn batch6_history_expansion_echo_not_duplicated_when_live() {
        // الدفعة 6: مع البث الحي يخرج صدى التوسيع
        // والتحرير إلى stdout فورا قبل التنفيذ، فلا
        // يبقى في ناتج feed (وإلا تكرر)؛ وبلا بث حي
        // يبقى في الناتج أولا كما تثبته اختبارات
        // `batch6_history_expansion_forms`.
        let mut r = Repl::new();
        r.set_live_output(true);
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("!!").output, "");
        assert_eq!(r.feed("حرر س ص").output, "");
        // التعبير العاري يطبع قيمته عبر المسار العادي
        // (البث الحي لـ`اطبع` وحده) فيبقى في الناتج.
        assert_eq!(r.feed("ص").output, "٥\n");
        r.set_live_output(false);
        assert_eq!(r.feed("!!").output, "ص\n٥\n");
    }

    #[test]
    fn repl_bare_expression_prints_value() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("١ + ٢")), "٣\n");
        assert_eq!(done_out(r.eval("\"مرحبا\"")), "مرحبا\n");
        assert_eq!(done_out(r.eval("[١، ٢]")), "[١, ٢]\n");
    }

    #[test]
    fn repl_live_output_streams_and_survives_reset() {
        // اجتماع 32، السؤال الثامن: مع البث الحي لا
        // يُحتفظ بمخرجات `اطبع` في ناتج eval (وصلت
        // للمخرجات فورا)؛ و:مسح يعيد العلم نفسه.
        let mut r = Repl::new();
        r.set_live_output(true);
        assert_eq!(done_out(r.eval("اطبع ١")), "");
        r.reset();
        assert_eq!(done_out(r.eval("اطبع ٢")), "");
        // وبدونه تعود المخرجات كما كانت.
        r.set_live_output(false);
        assert_eq!(done_out(r.eval("اطبع ٣")), "٣\n");
    }

    #[test]
    fn repl_trailing_expression_prints_after_block() {
        let mut r = Repl::new();
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب\nجمع(٢، ٣)")),
            "٥\n"
        );
        // A trailing call after a definition prints its value.
        assert_eq!(done_out(r.eval("س = ٧\nس * ٢")), "١٤\n");
    }

    // Simulate run_repl's line feeding: an indented line while a
    // block is pending only waits; a line at indent 0 evaluates
    // the accumulated block. This is the regression test for
    // nested function definitions (closures) in the REPL: the
    // block must reach eval as one piece, not line by line.
    fn feed_lines(repl: &mut Repl, lines: &[&str]) -> Vec<String> {
        let mut pending = String::new();
        let mut outputs = Vec::new();
        for line in lines {
            if !pending.is_empty() {
                if line.starts_with([' ', '\t']) {
                    pending.push('\n');
                    pending.push_str(line);
                    continue;
                }
                pending.push('\n');
                pending.push_str(line);
                match repl.eval(&pending) {
                    ReplEval::Incomplete => continue,
                    ReplEval::Done { output } => outputs.push(output),
                    ReplEval::Failed { output, error, .. } => {
                        outputs.push(format!("{}{}", output, error))
                    }
                    ReplEval::Exited { output, .. } => outputs.push(output),
                }
                pending.clear();
                continue;
            }
            match repl.eval(line) {
                ReplEval::Incomplete => pending.push_str(line),
                ReplEval::Done { output } => outputs.push(output),
                ReplEval::Failed { output, error, .. } => {
                    outputs.push(format!("{}{}", output, error))
                }
                ReplEval::Exited { output, .. } => outputs.push(output),
            }
        }
        outputs
    }

    #[test]
    fn repl_nested_function_definition_completes_as_one_block() {
        let mut r = Repl::new();
        let outs = feed_lines(
            &mut r,
            &[
                "دالة اصنع_عداد():",
                "    ع = ٠",
                "    دالة زد():",
                "        ع = ع + ١",
                "        ارجع ع",
                "    ارجع زد",
                "أ = اصنع_عداد()",
                "أ()",
                "أ()",
            ],
        );
        // The definition block runs silently; the two calls print ١, ٢.
        assert_eq!(outs, vec!["", "١\n", "٢\n"]);
    }

    #[test]
    fn repl_lambda_define_and_call() {
        // Meeting 13: a lambda is a single-line expression, so
        // the REPL needs no block continuation for it.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("مضاعف = دالة_مجهولة(س): س * ٢")), "");
        assert_eq!(done_out(r.eval("مضاعف(٥)")), "١٠\n");
        assert_eq!(done_out(r.eval("مضاعف")), "<دالة مجهولة>\n");
    }

    #[test]
    fn repl_list_methods_mutate_and_print() {
        // Meeting 14: method calls are expressions, so the REPL
        // handles them like any other line.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("ق = [١، ٢]")), "");
        assert_eq!(done_out(r.eval("ق.أضف(٣)")), "لاشيء\n");
        assert_eq!(done_out(r.eval("ق")), "[١, ٢, ٣]\n");
        assert_eq!(done_out(r.eval("\"مرحبا\".طول()")), "٥\n");
    }

    #[test]
    fn repl_stdlib_import_and_call() {
        // Meeting 16: native modules load in the REPL like any
        // other import — the REPL keeps the same interpreter.
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("استورد رياضيات")), "");
        assert_eq!(done_out(r.eval("رياضيات.جذر(١٦)")), "٤\n");
        assert_eq!(done_out(r.eval("رياضيات.باي")), "٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n");
    }

    #[test]
    fn repl_variables_and_functions_persist() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        assert_eq!(done_out(r.eval("س * ٢")), "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الإلحاق
    // والتجميد يعبران أسطر REPL — الجلسة تحتفظ بالحالة.
    #[test]
    fn repl_append_and_freeze_persist() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("ن = \"أ\"")), "");
        assert_eq!(done_out(r.eval("ن += \"ب\"")), "");
        assert_eq!(done_out(r.eval("اطبع ن")), "أب\n");
        assert_eq!(done_out(r.eval("جمد(\"ن\")")), "لاشيء\n");
        assert_eq!(
            done_out(r.eval("اطبع افحص_متغير(\"ن\")")),
            "متغير ن: نص = \"أب\" مجمد\n"
        );
        match r.eval("ن = \"ج\"") {
            ReplEval::Failed { error, .. } => {
                assert!(error.contains("متغير مجمد"), "got: {}", error)
            }
            ReplEval::Done { output } => {
                panic!("expected Failed, got Done: {}", output)
            }
            ReplEval::Exited { output, code } => {
                panic!("expected Failed, got Exited: {} {}", output, code)
            }
            ReplEval::Incomplete => panic!("expected Failed, got Incomplete"),
        }
        assert_eq!(done_out(r.eval("اطبع ن")), "أب\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): المرجع
    // الاسمي والحذف يعبران أسطر REPL.
    #[test]
    fn repl_nameref_and_delete_persist() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("ص = ١٠")), "");
        assert_eq!(done_out(r.eval("مرجع(\"ر\", \"ص\")")), "لاشيء\n");
        assert_eq!(done_out(r.eval("ر = ٢٠")), "");
        assert_eq!(done_out(r.eval("اطبع ص")), "٢٠\n");
        assert_eq!(done_out(r.eval("احذف_متغير(\"ر\")")), "لاشيء\n");
        assert_eq!(done_out(r.eval("اطبع ص")), "٢٠\n");
        assert_eq!(done_out(r.eval("احذف_متغير(\"ص\")")), "لاشيء\n");
        match r.eval("اطبع ص") {
            ReplEval::Failed { error, .. } => {
                assert!(error.contains("متغير غير معرف"), "got: {}", error)
            }
            ReplEval::Done { output } => {
                panic!("expected Failed, got Done: {}", output)
            }
            ReplEval::Exited { output, code } => {
                panic!("expected Failed, got Exited: {} {}", output, code)
            }
            ReplEval::Incomplete => panic!("expected Failed, got Incomplete"),
        }
    }

    #[test]
    fn repl_incomplete_block_keeps_waiting() {
        let mut r = Repl::new();
        assert!(matches!(r.eval("دالة جمع(أ، ب):"), ReplEval::Incomplete));
        assert!(matches!(r.eval("إذا س:"), ReplEval::Incomplete));
        assert!(matches!(r.eval("س = ٥"), ReplEval::Done { .. }));
    }

    #[test]
    fn repl_error_does_not_kill_session() {
        let mut r = Repl::new();
        assert!(is_failed(r.eval("غيرمعرف")));
        assert_eq!(done_out(r.eval("س = ١")), "");
        assert_eq!(done_out(r.eval("س")), "١\n");
    }

    #[test]
    fn repl_print_output_and_reset() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("اطبع ٥")), "٥\n");
        assert_eq!(done_out(r.eval("س = ٥")), "");
        r.reset();
        assert!(is_failed(r.eval("س")));
        assert!(r.history().is_empty());
    }

    #[test]
    fn repl_history_shows_with_bidi_isolates() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        let shown = r.show_history();
        assert!(shown.contains("س = ٥"));
        assert!(shown.contains(RLI));
        assert!(shown.contains(PDI));
    }

    #[test]
    fn repl_commands_parse() {
        assert!(matches!(parse_command(":مساعدة"), Some(ReplCommand::Help)));
        assert!(matches!(parse_command(":اعرض"), Some(ReplCommand::Show)));
        assert!(matches!(
            parse_command(":امسح-المحفوظات"),
            Some(ReplCommand::ClearHistory)
        ));
        assert!(matches!(parse_command(":مسح"), Some(ReplCommand::Clear)));
        assert!(matches!(parse_command(":خروج"), Some(ReplCommand::Quit(_))));
        assert!(matches!(parse_command("خروج"), Some(ReplCommand::Quit(_))));
        // No Latin aliases: :help / :quit / exit are not commands.
        assert!(parse_command(":help").is_none());
        assert!(parse_command("exit").is_none());
        assert!(parse_command("س = ٥").is_none());
    }

    // Task 2 (Meeting 25 plan): :امسح-المحفوظات clears
    // only the :اعرض history lines — variables and functions
    // survive it, unlike :مسح which resets the interpreter.
    #[test]
    fn repl_clear_history_keeps_variables() {
        let mut r = Repl::new();
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("س * ٢").output, "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
        // :اعرض shows the session lines.
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(o.output.contains("س * ٢"), "got: {}", o.output);
        // The new command clears the history only.
        let o = r.feed(":امسح-المحفوظات");
        assert_eq!(o.output, "مسحت محفوظات الجلسة.\n");
        assert!(!o.ended);
        assert!(r.history().is_empty());
        // :اعرض is now just the header — the old lines are gone.
        let o = r.feed(":اعرض");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(!o.output.contains("س * ٢"), "got: {}", o.output);
        // Variables and functions survived the clear.
        assert_eq!(r.feed("اطبع س").output, "٥\n");
        assert_eq!(r.feed("س").output, "٥\n");
        assert_eq!(r.feed("جمع(٢، ٣)").output, "٥\n");
        // The command line itself is never recorded: a new
        // line appears in :اعرض, the command does not.
        assert_eq!(r.feed("ص = ٧").output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("ص = ٧"), "got: {}", o.output);
        assert!(!o.output.contains("امسح-المحفوظات"), "got: {}", o.output);
    }

    // الدفعة 6 (اجتماع 60 §75): توسيع التاريخ (151)
    // والاستبدال السريع (152) عبر feed.
    #[test]
    fn batch6_history_expansion_forms() {
        let mut r = Repl::new();
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("اطبع س").output, "٥\n");
        assert_eq!(r.feed("!!").output, "اطبع س\n٥\n");
        assert_eq!(r.feed("!١").output, "س = ٥\n");
        assert_eq!(r.feed("!اطبع").output, "اطبع س\n٥\n");
        assert_eq!(r.feed("!?٥؟").output, "س = ٥\n");
        assert_eq!(r.feed("!?س").output, "س = ٥\n");
        let o = r.feed("!٩");
        assert!(o.output.contains("لا يوجد أمر بهذا الرقم"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("!");
        assert!(o.output.contains("يتوقع حدثا بعد !"), "got: {}", o.output);
        // الاستبدال السريع على آخر أمر، ويُسجَّل الناتج.
        assert_eq!(r.feed("ص = ٧").output, "");
        assert_eq!(r.feed("^ص^س^").output, "س = ٧\n");
        assert_eq!(r.feed("س").output, "٧\n");
        let o = r.feed("^غائب^س^");
        assert!(o.output.contains("غير موجود في آخر أمر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        // `!` داخل كود لا يتوسع، و`كرر الأخير` كما كان.
        assert_eq!(r.feed("اطبع \"!\"").output, "!\n");
        assert_eq!(r.feed("كرر الأخير").output, "!\n");
    }

    // الدفعة 6: أمر المحفوظات (153) وفروعه عبر feed.
    #[test]
    fn batch6_history_command_branches() {
        let dir = std::env::temp_dir().join(format!("niva-batch6-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("اطبع س").output, "٥\n");
        let o = r.feed("محفوظات");
        assert!(o.output.contains("١"), "got: {}", o.output);
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(o.output.contains("اطبع س"), "got: {}", o.output);
        let o = r.feed("محفوظات ٢");
        // السطر نفسه يُسجَّل قبل العرض (كـباش) فآخر
        // مدخلين هما سطرا `محفوظات` أنفسهما.
        assert!(o.output.contains("محفوظات ٢"), "got: {}", o.output);
        assert!(!o.output.contains("اطبع س"), "got: {}", o.output);
        assert_eq!(r.feed("محفوظات أضف يدوي = ١").output, "");
        let o = r.feed("محفوظات ابحث يدوي");
        assert!(o.output.contains("يدوي = ١"), "got: {}", o.output);
        let o = r.feed("محفوظات اطبع !!") ;
        assert!(o.output.contains("محفوظات ابحث يدوي"), "got: {}", o.output);
        let o = r.feed("محفوظات احذف ١");
        assert!(o.output.contains("حذفت الأمر رقم ١"), "got: {}", o.output);
        assert!(!r.history().texts().contains(&"س = ٥".to_string()));
        let o = r.feed("محفوظات احذف ٩٩");
        assert!(o.output.contains("لا يوجد أمر بهذا الرقم"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("محفوظات غريب");
        assert!(o.output.contains("فرع محفوظات غير معروف"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 6: فروع الملف (150) — حفظ وإلحاق وقراءة
    // وتعاون بين جلستين، والحفظ عند الخروج.
    #[test]
    fn batch6_history_file_branches() {
        let dir = std::env::temp_dir().join(format!("niva-batch6-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("اطبع س").output, "٥\n");
        let o = r.feed("محفوظات احفظ");
        assert!(o.output.contains("حُفظت المحفوظات"), "got: {}", o.output);
        let path = dir.join("home").join("محفوظات");
        assert!(path.is_file(), "missing {:?}", path);
        // جلسة ثانية تحمّل الملف، وتقرأ الجديد وحده.
        let mut r2 = Repl::with_program_dir(dir.clone());
        assert!(r2.history().is_empty());
        assert!(r2.load_history_file().is_none());
        assert!(r2.history().texts().contains(&"اطبع س".to_string()));
        let o = r2.feed("محفوظات اقرأ_جديد");
        assert!(o.output.contains("لا جديد"), "got: {}", o.output);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("#0\nخارجي = ٩\n");
        std::fs::write(&path, text).unwrap();
        let o = r2.feed("محفوظات اقرأ_جديد");
        assert!(o.output.contains("قُرئت ١ جديدة"), "got: {}", o.output);
        assert_eq!(r2.history().last_text(), Some("خارجي = ٩"));
        // الحفظ عند الخروج يلحق الجديد وحده.
        assert_eq!(r2.feed("ص = ٨").output, "");
        let o = r2.feed(":خروج");
        assert!(o.ended);
        let mut r3 = Repl::with_program_dir(dir.clone());
        assert!(r3.load_history_file().is_none());
        assert!(r3.history().texts().contains(&"ص = ٨".to_string()));
        assert!(r3.history().texts().contains(&"خارجي = ٩".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 6: نقطة التسجيل الواحدة (السؤال الحادي عشر)
    // — المقتطف المولَّد مدخل واحد لا ثلاثة، والكتلة
    // مدخل واحد لا أسطرا.
    #[test]
    fn batch6_history_single_entry_per_typed_line() {
        let mut r = Repl::new();
        assert_eq!(r.feed("رع").output.len() > 0, true);
        let shown = r.show_history();
        assert!(shown.contains("رع"), "got: {}", shown);
        assert!(!shown.contains("استورد عشوائي"), "got: {}", shown);
        assert_eq!(r.history().len(), 1);
        let before = r.history().len();
        // السطر المُسنَّد وحده ينتظر؛ والسطر على مسافة
        // الصفر يكمل الكتلة فيُقيَّم المقتطف كله دفعة واحدة.
        assert_eq!(r.feed("إذا صحيح:").output, "");
        assert_eq!(r.feed("    اطبع ١").output, "");
        assert_eq!(r.feed("س = ٥").output, "١\n");
        assert_eq!(r.history().len(), before + 1);
        let numbered = r.show_history_numbered(None);
        assert_eq!(numbered.matches("إذا صحيح:").count(), 1);
    }

    // الدفعة 6: سطور ملفات البدء لا تُسجَّل (السؤال
    // الحادي عشر) — والتعريفات تبقى في الجلسة.
    #[test]
    fn batch6_history_startup_not_recorded() {
        let dir = std::env::temp_dir().join(format!("niva-batch6-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("بدء.نيفا"), "بدء_س = ٩\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let out = r.run_startup_files();
        assert_eq!(out, "");
        assert!(r.history().is_empty());
        assert_eq!(r.feed("اطبع بدء_س").output, "٩\n");
        assert_eq!(r.history().texts(), vec!["اطبع بدء_س".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 6: تحرير أمر سابق وإعادة تنفيذه (154).
    #[test]
    fn batch6_history_edit_command() {
        let mut r = Repl::new();
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("حرر س ص").output, "ص = ٥\n");
        assert_eq!(r.feed("ص").output, "٥\n");
        assert_eq!(r.feed("حرر ١ س ع").output, "ع = ٥\n");
        assert_eq!(r.feed("ع").output, "٥\n");
        let o = r.feed("حرر غائب جديد");
        assert!(o.output.contains("غير موجود في هذا الأمر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("حرر ٩٩ أ ب");
        assert!(o.output.contains("لا يوجد أمر بهذا الرقم"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
    }

    // الدفعة 6: عبارات إعدادات المحفوظات (149 و155 و156).
    #[test]
    fn batch6_history_settings_phrases() {
        let dir = std::env::temp_dir().join(format!("niva-batch6-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("حد_المحفوظات");
        assert!(o.output.contains("٥٠٠"), "got: {}", o.output);
        let o = r.feed("حد_المحفوظات غريب");
        assert!(o.output.contains("تتوقع عددا صحيحا غير سالب"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 2);
        assert_eq!(r.feed("أول = ١").output, "");
        assert_eq!(r.feed("ثان = ٢").output, "");
        assert_eq!(r.feed("ثال = ٣").output, "");
        assert_eq!(r.feed("راب = ٤").output, "");
        assert_eq!(r.feed("خام = ٥").output, "");
        let o = r.feed("حد_المحفوظات ٣");
        assert!(o.output.contains("٣"), "got: {}", o.output);
        assert_eq!(
            r.history().texts(),
            vec!["راب = ٤".to_string(), "خام = ٥".to_string(), "حد_المحفوظات ٣".to_string()]
        );
        // القواعد: المكرر يُتجاهل بعد ضبطها (والحد ٣
        // قائم فالطول لا ينمو — يُفحص العدد لا الطول).
        assert_eq!(r.feed("قواعد_المحفوظات تجاهل_المكرر").output, "قواعد المحفوظات: تجاهل_المكرر\n");
        assert_eq!(r.feed("ساد = ٦").output, "");
        assert_eq!(r.feed("ساد = ٦").output, "");
        assert_eq!(
            r.history().texts().iter().filter(|t| t.as_str() == "ساد = ٦").count(),
            1
        );
        let o = r.feed("قواعد_المحفوظات غريب");
        assert!(o.output.contains("قاعدة محفوظات غير معروفة"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 2);
        // الأنماط: `ات` لا تُسجَّل بعد ضبطها.
        assert_eq!(
            r.feed("تجاهل_المحفوظات ات").output,
            "تجاهل المحفوظات: ات\n"
        );
        let len = r.history().len();
        let _ = r.feed("ات");
        assert_eq!(r.history().len(), len);
        // الصيغة: الطابع يظهر في `محفوظات`، والسيئة مرفوضة.
        assert_eq!(
            r.feed("صيغة_وقت_المحفوظات %F %T").output,
            "صيغة وقت المحفوظات: %F %T\n"
        );
        let o = r.feed("محفوظات");
        assert!(o.output.contains("-"), "got: {}", o.output);
        let o = r.feed("صيغة_وقت_المحفوظات %Q");
        assert!(o.output.contains("رمز وقت غير معروف"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 2);
        // الملف: العرض ثم الضبط ثم الكتابة في الإعدادات.
        let o = r.feed("ملف_المحفوظات");
        assert!(o.output.contains("~/محفوظات"), "got: {}", o.output);
        let o = r.feed("ملف_المحفوظات ~/جديد");
        assert!(o.output.contains("~/جديد"), "got: {}", o.output);
        let props = std::fs::read_to_string(dir.join("home").join("إعدادات").join("niva.properties")).unwrap();
        assert!(props.contains("ملف_المحفوظات=~/جديد"), "got: {}", props);
        assert!(props.contains("حد_المحفوظات=3"), "got: {}", props);
        let o = r.feed("ملف_المحفوظات بلا");
        assert!(o.output.contains("بلا ملف"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 7 (اجتماع 61 §76): عبارات الموجهات
    // الثلاث (174) — العرض والضبط والحفظ في الملف
    // والتحميل منه، والاستمرار المخصص داخل الكتلة.
    #[test]
    fn batch7_prompt_phrases() {
        let dir = std::env::temp_dir().join(format!("niva-batch7-prompt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        // الافتراضيات: قالب اليوم حرفيا.
        assert_eq!(r.feed("موجه_أول").output, "موجه أول: {المجلد} $ \n");
        assert_eq!(r.feed("موجه_استمرار").output, "موجه استمرار: > \n");
        assert_eq!(r.feed("موجه_تتبع").output, "موجه تتبع: + \n");
        // الضبط: القالب الجديد يسري من الموجه التالي.
        assert_eq!(r.feed("موجه_أول جاهز>").output, "موجه أول: جاهز>\n");
        assert_eq!(r.prompt(), "جاهز>");
        assert_eq!(r.feed("موجه_استمرار >>").output, "موجه استمرار: >>\n");
        // داخل كتلة معلقة يظهر موجه الاستمرار المخصص.
        let o = r.feed("إذا صحيح:");
        assert_eq!(o.prompt, ">>");
        assert_eq!(r.feed("").output, "ألغيت الكتلة غير المكتملة.\n");
        // موجه التتبع يُخزَّن وحده (بلا عرض تتبّع).
        assert_eq!(r.feed("موجه_تتبع >>>").output, "موجه تتبع: >>>\n");
        assert_eq!(r.feed("موجه_تتبع").output, "موجه تتبع: >>>\n");
        // الحفظ في niva.properties حرفيا.
        let props = std::fs::read_to_string(
            dir.join("home").join("إعدادات").join("niva.properties"),
        )
        .unwrap();
        assert!(props.contains("موجه_أول=جاهز>"), "got: {}", props);
        assert!(props.contains("موجه_استمرار=>>"), "got: {}", props);
        assert!(props.contains("موجه_تتبع=>>>"), "got: {}", props);
        // جلسة جديدة على المجلد نفسه تحمل القوالب.
        let r2 = Repl::with_program_dir(dir.clone());
        assert_eq!(r2.prompts().primary, "جاهز>");
        assert_eq!(r2.prompts().continuation, ">>");
        assert_eq!(r2.prompts().trace, ">>>");
        assert_eq!(r2.prompt(), "جاهز>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 7: تحميل الموجهات من الملف عند الإنشاء
    // (set_if_not) — الغائب يأخذ الافتراضي، والحاضر
    // يفوز، والمهلة السيئة تُسقط إلى 0.
    #[test]
    fn batch7_prompt_settings_file() {
        let dir = std::env::temp_dir().join(format!("niva-batch7-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let settings = dir.join("home").join("إعدادات");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(
            settings.join("niva.properties"),
            "موجه_أول={المستخدم}@نيفا $ \nمهلة_الخمول=غريب\n",
        )
        .unwrap();
        // القيمة تُقرأ مقصوصة المسافات من الملف
        // (parse_settings_entries) — فالمسافة الأخيرة
        // تُضبط من العبارة لا من الملف.
        let r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.prompts().primary, "{المستخدم}@نيفا $");
        assert_eq!(r.prompts().continuation, "> ");
        assert_eq!(r.prompts().timeout_secs, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 7 (اجتماع 61 §76): أمر ما قبل الموجه
    // (175) — ينفذ قبل كل موجه أول في نفس الجلسة،
    // وخطؤه لا يمنع الموجه ولا يمسّ رمز آخر سطر،
    // وسطوره لا تُسجَّل، ولا ينفذ داخل كتلة معلقة.
    #[test]
    fn batch7_hook_runs_before_prompt() {
        let dir = std::env::temp_dir().join(format!("niva-batch7-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("أمر_قبل_الموجه").output, "أمر قبل الموجه: بلا\n");
        assert_eq!(r.before_prompt(), "");
        assert_eq!(
            r.feed("أمر_قبل_الموجه اطبع \"قبل\"").output,
            "أمر قبل الموجه: اطبع \"قبل\"\n"
        );
        assert_eq!(r.before_prompt(), "قبل\n");
        // قبل كل موجه أول — يعيد التنفيذ كل مرة.
        assert_eq!(r.before_prompt(), "قبل\n");
        // في نفس الجلسة: الخطاف يرى المتغيرات ويكتبها.
        assert_eq!(r.feed("أمر_قبل_الموجه س = ٩").output, "أمر قبل الموجه: س = ٩\n");
        assert_eq!(r.before_prompt(), "");
        assert_eq!(r.feed("س").output, "٩\n");
        // خطأ الخطاف يُطبع ولا يمسّ رمز آخر سطر.
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.last_exit_code(), 0);
        assert_eq!(r.feed("أمر_قبل_الموجه دالة_غائبة()").output, "أمر قبل الموجه: دالة_غائبة()\n");
        let err = r.before_prompt();
        assert!(err.contains("غير معرف") || err.contains("غائبة"), "got: {}", err);
        assert_eq!(r.last_exit_code(), 0);
        // سطور الخطاف لا تُسجَّل — سطر العبارة وحده يُسجَّل.
        assert!(!r.history().texts().iter().any(|t| t == "اطبع \"قبل\""));
        assert!(!r.history().texts().iter().any(|t| t == "س = ٩"));
        // الحارس: الاستدعاء المتداخل يُتخطى بصمت.
        r.in_prompt_hook = true;
        assert_eq!(r.before_prompt(), "");
        r.in_prompt_hook = false;
        // الإلغاء بـ`بلا`.
        assert_eq!(r.feed("أمر_قبل_الموجه بلا").output, "أمر قبل الموجه: بلا\n");
        assert_eq!(r.before_prompt(), "");
        // لا ينفذ داخل كتلة معلقة (موجه استمرار لا أول).
        assert_eq!(r.feed("أمر_قبل_الموجه اطبع \"داخل\"").output, "أمر قبل الموجه: اطبع \"داخل\"\n");
        let o = r.feed("إذا صحيح:");
        assert_eq!(o.output, "");
        assert_eq!(r.feed("").output, "ألغيت الكتلة غير المكتملة.\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 7 (اجتماع 61 §76): مهلة الخمول (176) —
    // العرض والضبط والرفض والإلغاء.
    #[test]
    fn batch7_timeout_phrase() {
        let dir = std::env::temp_dir().join(format!("niva-batch7-timeout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("مهلة_الخمول").output, "مهلة الخمول: بلا مهلة\n");
        assert_eq!(r.timeout_secs(), 0);
        assert_eq!(r.feed("مهلة_الخمول ٦٠").output, "مهلة الخمول: ٦٠ ثوان\n");
        assert_eq!(r.timeout_secs(), 60);
        assert_eq!(r.feed("مهلة_الخمول").output, "مهلة الخمول: ٦٠ ثوان\n");
        let o = r.feed("مهلة_الخمول غريب");
        assert_eq!(o.output, "مهلة_الخمول تتوقع عددا صحيحا غير سالب\n");
        assert_eq!(r.last_exit_code(), 2);
        assert_eq!(r.timeout_secs(), 60);
        let o = r.feed("مهلة_الخمول -٥");
        assert_eq!(o.output, "مهلة_الخمول تتوقع عددا صحيحا غير سالب\n");
        assert_eq!(r.timeout_secs(), 60);
        assert_eq!(r.feed("مهلة_الخمول بلا").output, "مهلة الخمول: بلا مهلة\n");
        assert_eq!(r.timeout_secs(), 0);
        assert_eq!(r.feed("مهلة_الخمول 0").output, "مهلة الخمول: بلا مهلة\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 6: بدائيات التنقل والبحث (157) على Repl.
    #[test]
    fn batch6_history_nav_methods() {
        let mut r = Repl::new();
        assert_eq!(r.feed("أ = ١").output, "");
        assert_eq!(r.feed("اطبع أ").output, "١\n");
        assert_eq!(r.feed("ب = ٢").output, "");
        assert_eq!(r.history_prev().as_deref(), Some("ب = ٢"));
        assert_eq!(r.history_prev().as_deref(), Some("اطبع أ"));
        assert_eq!(r.history_prev().as_deref(), Some("أ = ١"));
        assert_eq!(r.history_prev().as_deref(), Some("أ = ١"));
        assert_eq!(r.history_next().as_deref(), Some("اطبع أ"));
        assert_eq!(r.history_next().as_deref(), Some("ب = ٢"));
        assert_eq!(r.history_next(), None);
        let (n, text) = r.history_search_reverse("اطبع").unwrap();
        assert_eq!(n, 2);
        assert_eq!(text, "اطبع أ");
        assert!(r.history_search_reverse("غائب").is_none());
    }

    // الدفعة 6: تكافؤ وضع المفسر — التوسيع والتحرير
    // والعرض المرقم تعمل فيه كما في VM.
    #[test]
    fn batch6_history_interpreter_parity() {
        let dir = std::env::temp_dir().join(format!("niva-batch6-interp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Repl::with_program_dir_interpreter(dir.clone());
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("!!").output, "س = ٥\n");
        assert_eq!(r.feed("حرر س ص").output, "ص = ٥\n");
        assert_eq!(r.feed("ص").output, "٥\n");
        let o = r.feed("محفوظات");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        assert!(o.output.contains("ص = ٥"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The stateful feed API (console): the prototype session,
    // line by line, with the exact prompts at each step. Only
    // compiled with the prototype feature enabled.
    #[test]
    #[cfg(feature = "prototype-rtl")]
    fn repl_feed_prototype_session() {
        let mut r = Repl::new();
        let o = r.feed("٥ = س");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        let o = r.feed("٣ = ص");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("اطبع س + ص");
        assert_eq!(o.output, "٨\n");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("إذا س أكبر من ص:");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع \"س أكبر\"");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("٧ = ع");
        assert_eq!(o.output, "س أكبر\n");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("اطبع ع * ٢");
        assert_eq!(o.output, "١٤\n");
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
    }

    // Public build (no prototype feature): the reversed form is
    // rejected with the normal error, and the normal form runs
    // the same session. This is the behavior the console ships.
    #[test]
    #[cfg(not(feature = "prototype-rtl"))]
    fn repl_feed_reversed_assignment_rejected() {
        let mut r = Repl::new();
        let o = r.feed("٥ = س");
        assert!(o.output.contains("هدف التعيين غير صالح"), "got: {}", o.output);
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives; the normal form runs the session.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("ص = ٣");
        assert_eq!(o.output, "");
        let o = r.feed("اطبع س + ص");
        assert_eq!(o.output, "٨\n");
        let o = r.feed("إذا س أكبر من ص:");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع \"س أكبر\"");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("ع = ٧");
        assert_eq!(o.output, "س أكبر\n");
        let o = r.feed("اطبع ع * ٢");
        assert_eq!(o.output, "١٤\n");
    }

    #[test]
    fn repl_feed_empty_line_cancels_block() {
        let mut r = Repl::new();
        let o = r.feed("دالة جمع(أ، ب):");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("");
        assert_eq!(o.output, "ألغيت الكتلة غير المكتملة.\n");
        assert_eq!(o.prompt, "~ $ ");
        // The cancelled definition never took effect.
        assert!(is_failed(r.eval("جمع(٢، ٣)")));
    }

    #[test]
    fn repl_feed_command_while_block_pending() {
        let mut r = Repl::new();
        let o = r.feed("إذا س:");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed(":مساعدة");
        assert_eq!(
            o.output,
            "ألغيت الكتلة غير المكتملة.\n:مساعدة\n:اعرض\nمحفوظات [أمر]\nحرر <قديم> <جديد>\n:امسح-المحفوظات\n:مسح\n:اسمح-بالشبكة\n:اسمح-بالخادم\n:تنزيلات <مسار>\n:خروج [رمز]\nمعلومات\nأعد-الإعدادات\nأعد-الضبط\nمستعار <اسم> <نص>\nمستعارات\nاحذف_مستعار <اسم>\nأكمل [جزء]\nخيارات [مفتاح] [قيمة]\n"
        );
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
    }

    #[test]
    fn repl_feed_quit_and_clear() {
        let mut r = Repl::new();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        let o = r.feed(":مسح");
        assert_eq!(o.output, "بدأت جلسة جديدة.\n");
        assert!(!o.ended);
        assert!(is_failed(r.eval("س")));
        let o = r.feed(":خروج");
        assert!(o.ended);
        assert_eq!(o.output, "");
        let mut r2 = Repl::new();
        let o = r2.feed("خروج");
        assert!(o.ended);
        // Latin `exit` is not a command any more.
        let mut r3 = Repl::new();
        let o = r3.feed("exit");
        assert!(!o.ended);
    }

    // الدفعة 1 — رموز الخروج (اجتماع 54 §69).
    #[test]
    fn repl_exit_code_after_success_and_failure() {
        let mut r = Repl::new();
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        assert_eq!(r.last_exit_code(), 0);
        let o = r.feed("رمز_الخروج()");
        assert_eq!(o.output, "٠\n");
        assert_eq!(r.last_exit_code(), 0);
        // خطأ وقت تشغيل (قسمة على صفر) → 1.
        let o = r.feed("١ / ٠");
        assert!(o.output.contains("قسمة على صفر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("رمز_الخروج()");
        assert_eq!(o.output, "١\n");
        assert_eq!(r.last_exit_code(), 0);
        // اسم غير معرف → خطأ دلالي (2).
        let o = r.feed("غيرمعرف");
        assert!(o.output.contains("غير معرف"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 2);
        // نوع غير متطابق وقت التشغيل → فشل عام (1).
        let o = r.feed("١ + \"نص\"");
        assert!(!o.output.is_empty());
        assert_eq!(r.last_exit_code(), 1);
    }

    #[test]
    fn repl_missing_module_is_127() {
        // الدفعة 1 (§69 ممتد): وحدة مستوردة غائبة ملف
        // مطلوب غير موجود → 127 لا 1.
        let mut r = Repl::new();
        let o = r.feed("استورد لاوجود");
        assert!(o.output.contains("وحدة غير موجودة"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 127);
        let o = r.feed("رمز_الخروج()");
        assert_eq!(o.output, "١٢٧\n");
    }

    #[test]
    fn repl_run_missing_and_dir_codes() {
        let dir = std::env::temp_dir().join(format!("niva-repl-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("مجلد.نيفا")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 127);
        let o = r.feed("رمز_الخروج()");
        assert_eq!(o.output, "١٢٧\n");
        let o = r.feed("شغل مجلد");
        assert!(o.output.contains("مجلد وليس ملفا"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 126);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_run_program_exit_code() {
        let dir = std::env::temp_dir().join(format!("niva-repl-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("يخرج.نيفا"), "اطبع \"داخل\"\nخروج(٥)\nاطبع \"بعد\"\n").unwrap();
        std::fs::write(dir.join("عادي.نيفا"), "اطبع \"تم\"\n").unwrap();
        std::fs::write(dir.join("فاشل.نيفا"), "س = ١ / ٠\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغل يخرج");
        assert_eq!(o.output, "داخل\n");
        assert_eq!(r.last_exit_code(), 5);
        let o = r.feed("شغل عادي");
        assert_eq!(o.output, "تم\n");
        assert_eq!(r.last_exit_code(), 0);
        let o = r.feed("شغل فاشل");
        assert!(o.output.contains("قسمة على صفر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        // محرك المفسر (التشخيص) يعطي الرموز نفسها.
        let mut ri = Repl::with_program_dir_interpreter(dir.clone());
        let o = ri.feed("شغل يخرج");
        assert_eq!(o.output, "داخل\n");
        assert_eq!(ri.last_exit_code(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_run_shadowed_tool_skips_to_next() {
        // الدفعة 3 (اجتماع 57 §72، الميزة 39): المجلد
        // وغير المقروء في `$نيفا` يُتخطيان إلى البيت.
        let dir = std::env::temp_dir()
            .join(format!("niva-repl-shadow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        // مجلد في `$نيفا` + ملف في البيت → يعمل ملف البيت.
        std::fs::create_dir_all(dir.join("أداة.نيفا")).unwrap();
        std::fs::write(
            dir.join("home").join("أداة.نيفا"),
            "اطبع \"من البيت\"\n",
        )
        .unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغل أداة");
        assert_eq!(o.output, "من البيت\n", "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 0);
        // مجلد في المكانين معا → 126.
        std::fs::create_dir_all(dir.join("home").join("مجلد.نيفا")).unwrap();
        std::fs::create_dir_all(dir.join("مجلد.نيفا")).unwrap();
        let o = r.feed("شغل مجلد");
        assert!(o.output.contains("مجلد وليس ملفا"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 126);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn repl_run_unreadable_tool_skips_to_next() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir()
            .join(format!("niva-repl-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let blocked = dir.join("مقفل.نيفا");
        std::fs::write(&blocked, "اطبع \"محجوب\"\n").unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000))
            .unwrap();
        std::fs::write(
            dir.join("home").join("مقفل.نيفا"),
            "اطبع \"من البيت\"\n",
        )
        .unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغل مقفل");
        assert_eq!(o.output, "من البيت\n", "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 0);
        // غير مقروء في المكانين معا → 126.
        let blocked_home = dir.join("home").join("مقفل.نيفا");
        std::fs::set_permissions(
            &blocked_home,
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let o = r.feed("شغل مقفل");
        assert!(o.output.contains("تعذر قراءة الملف"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 126);
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644))
            .unwrap();
        std::fs::set_permissions(
            &blocked_home,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_exec_ends_session() {
        // الدفعة 3 (اجتماع 57 §72، الميزة 51): `استبدل`
        // تنهي الجلسة برمز الأداة بلا عودة.
        let dir = std::env::temp_dir()
            .join(format!("niva-repl-exec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("يخرج.نيفا"),
            "اطبع \"داخل\"\nخروج(٥)\nاطبع \"بعد\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("فاشل.نيفا"), "س = ١ / ٠\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("استبدل يخرج");
        assert_eq!(o.output, "داخل\n", "got: {}", o.output);
        assert!(o.ended, "exec must end the session");
        assert_eq!(r.quit_code(), 5);
        assert_eq!(r.last_exit_code(), 5);
        // محرك المفسر يعطي النهاية نفسها.
        let mut ri = Repl::with_program_dir_interpreter(dir.clone());
        let o = ri.feed("استبدل يخرج");
        assert_eq!(o.output, "داخل\n", "got: {}", o.output);
        assert!(o.ended);
        assert_eq!(ri.quit_code(), 5);
        // الأداة الفاشلة تنهي الجلسة برمزها 1.
        let mut r2 = Repl::with_program_dir(dir.clone());
        let o = r2.feed("استبدل فاشل");
        assert!(o.output.contains("قسمة على صفر"), "got: {}", o.output);
        assert!(o.ended);
        assert_eq!(r2.quit_code(), 1);
        // الغائب لا ينهي الجلسة: 127 ويستمر الموجه.
        let mut r3 = Repl::with_program_dir(dir.clone());
        let o = r3.feed("استبدل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert!(!o.ended);
        assert_eq!(r3.last_exit_code(), 127);
        // وسائط خاطئة 2، والجلسة تستمر.
        let o = r3.feed("استبدل");
        assert_eq!(r3.last_exit_code(), 2);
        assert!(!o.ended);
        let o = r3.feed("استبدل أ ب");
        assert_eq!(r3.last_exit_code(), 2);
        assert!(!o.ended);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_not_found_hook() {
        // الدفعة 3 (اجتماع 57 §72، الميزة 53): الخطاف
        // يُستدعى عند الغياب فقط، ومخرجه ورمزه مخرج
        // السطر ورمزه، ولا يظهر استدعاؤه في المحفوظات.
        let dir = std::env::temp_dir()
            .join(format!("niva-repl-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("موجود.نيفا"), "اطبع \"تم\"\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // بلا خطاف: 127 العادية.
        let o = r.feed("شغل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 127);
        // عرّف الخطاف (كتلة معلقة تُغلق بسطر كود صامت
        // — فالسطر غير المُزاح يُلحق بالكتلة ويُقيَّم
        // ككود، لا كأمر طرفي).
        let o = r.feed("دالة عند_غياب_الامر(اسم):");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع اسم");
        assert_eq!(o.output, "");
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        // الغياب يستدعي الخطاف.
        let o = r.feed("شغل غائب");
        assert_eq!(o.output, "غائب\n", "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 0);
        assert!(!o.ended);
        // الوجود لا يستدعيه.
        let o = r.feed("شغل موجود");
        assert_eq!(o.output, "تم\n", "got: {}", o.output);
        // `استبدل` الغائب يستدعيه أيضا، بلا إنهاء.
        let o = r.feed("استبدل غائب");
        assert_eq!(o.output, "غائب\n", "got: {}", o.output);
        assert!(!o.ended);
        assert_eq!(r.last_exit_code(), 0);
        // استدعاء الخطاف نفسه لا يظهر في المحفوظات
        // (تعريفه يظهر — وهو سطر كتبه المستخدم فعلا).
        let o = r.feed(":اعرض");
        assert!(
            !o.output.contains("عند_غياب_الامر(\""),
            "got: {}",
            o.output
        );
        // خطأ داخل الخطاف → 1.
        let o = r.feed("دالة عند_غياب_الامر(اسم):");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    س = ١ / ٠");
        assert_eq!(o.output, "");
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        let o = r.feed("شغل غائب");
        assert!(o.output.contains("قسمة على صفر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        // غياب داخل الخطاف نفسه → 127 العادية بلا
        // استدعاء متداخل (وإلا طُبع «غائب2» سطرا).
        let o = r.feed("دالة عند_غياب_الامر(اسم):");
        assert_eq!(o.prompt, CONT_PROMPT);
        let o = r.feed("    اطبع اسم");
        assert_eq!(o.output, "");
        let o = r.feed("    استبدل_الجلسة(\"غائب2\")");
        assert_eq!(o.output, "");
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        let o = r.feed("شغل غائب3");
        assert!(o.output.starts_with("غائب3\n"), "got: {}", o.output);
        assert!(!o.output.contains("غائب2\n"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 127);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_invert_flips_code() {
        let mut r = Repl::new();
        let o = r.feed("عكس شغل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 0);
        let o = r.feed("عكس س = ١");
        assert_eq!(o.output, "");
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("عكس عكس شغل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 127);
        // المحفوظات تسجّل السطر الأصلي كاملا.
        assert_eq!(
            r.history().last_text(),
            Some("عكس عكس شغل غائب")
        );
    }

    #[test]
    fn repl_invert_rejects_exit_and_block() {
        let mut r = Repl::new();
        let o = r.feed("عكس");
        assert_eq!(o.output, "عكس تتوقع أمرا بعدها\n");
        assert_eq!(r.last_exit_code(), 2);
        assert!(!o.ended);
        let o = r.feed("عكس :خروج");
        assert_eq!(o.output, "عكس لا تناسب الخروج\n");
        assert_eq!(r.last_exit_code(), 2);
        let o = r.feed("عكس خروج(٥)");
        assert_eq!(o.output, "عكس لا تناسب الخروج\n");
        assert_eq!(r.last_exit_code(), 2);
        assert!(!o.ended);
        let o = r.feed("عكس دالة جمع(أ، ب):");
        assert_eq!(o.output, "عكس لا تناسب الكتلة متعددة الأسطر\n");
        assert_eq!(r.last_exit_code(), 2);
        // الجلسة مستمرة بعد كل رفض.
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        assert_eq!(r.last_exit_code(), 0);
    }

    #[test]
    fn repl_quit_with_code() {
        let mut r = Repl::new();
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        let o = r.feed("خروج ٧");
        assert!(o.ended);
        assert_eq!(r.quit_code(), 7);
        assert_eq!(r.last_exit_code(), 7);
        let mut r = Repl::new();
        let o = r.feed("١ / ٠");
        assert!(o.output.contains("قسمة على صفر"), "got: {}", o.output);
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed(":خروج");
        assert!(o.ended);
        assert_eq!(r.quit_code(), 1);
        assert_eq!(r.last_exit_code(), 1);
        let mut r = Repl::new();
        let o = r.feed(":خروج ٣");
        assert!(o.ended);
        assert_eq!(r.quit_code(), 3);
        assert_eq!(r.last_exit_code(), 3);
    }

    #[test]
    fn repl_quit_bad_code_stays() {
        let mut r = Repl::new();
        let o = r.feed("خروج س");
        assert!(!o.ended);
        assert_eq!(r.last_exit_code(), 2);
        let o = r.feed("خروج ٣٠٠");
        assert!(!o.ended);
        assert_eq!(r.last_exit_code(), 2);
        let o = r.feed("خروج ٧ ٨");
        assert!(!o.ended);
        assert_eq!(r.last_exit_code(), 2);
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        assert_eq!(r.last_exit_code(), 0);
    }

    #[test]
    fn repl_exit_call_ends_session() {
        let mut r = Repl::new();
        let o = r.feed("خروج(٥)");
        assert!(o.ended);
        assert_eq!(r.quit_code(), 5);
        assert_eq!(r.last_exit_code(), 5);
        // خروج() بلا معامل = رمز آخر أمر.
        let mut r = Repl::new();
        let o = r.feed("١ / ٠");
        assert_eq!(r.last_exit_code(), 1);
        let o = r.feed("خروج()");
        assert!(o.ended);
        assert_eq!(r.quit_code(), 1);
        assert_eq!(r.last_exit_code(), 1);
    }

    #[test]
    fn repl_eof_exits_with_last_code() {
        let mut r = Repl::new();
        let o = r.feed("١ / ٠");
        assert_eq!(r.last_exit_code(), 1);
        let o = r.eof();
        assert!(o.ended);
        assert_eq!(r.quit_code(), 1);
    }

    #[test]
    fn repl_feed_error_appears_in_output() {
        let mut r = Repl::new();
        let o = r.feed("غيرمعرف");
        assert!(o.output.contains("غير معرف"));
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives the error.
        let o = r.feed("س = ١");
        assert_eq!(o.output, "");
        let o = r.feed("س");
        assert_eq!(o.output, "١\n");
    }

    // Meeting 22: the network flag in the REPL — off by
    // default, on with set_network_allowed; feed() flags the
    // permission error so a console can offer the first-use
    // prompt. The fetch itself is exercised against the local
    // test server in network.rs; here the gate and the flag
    // are what is tested (no server needed).
    #[test]
    fn repl_network_gate_off_by_default() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        assert!(!o.needs_network);
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(o.needs_network);
        assert_eq!(o.prompt, "~ $ ");
        assert!(!o.ended);
        // The session survives the refused fetch.
        assert_eq!(done_out(r.eval("س = ١")), "");
        assert_eq!(done_out(r.eval("س")), "١\n");
    }

    // Meeting 26: without the `https` feature, HTTPS is refused
    // before any traffic, even allowed. With the feature, the
    // same fetch attempts a real connection (see the variant
    // below) — so this test is the no-feature one.
    #[test]
    #[cfg(not(feature = "https"))]
    fn repl_network_allowed_flag_sticks_after_reset() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        r.set_network_allowed(true);
        // HTTPS is refused before any traffic, even allowed.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"https://example.com/\")");
        assert!(o.output.contains("HTTPS غير مدعوم"), "got: {}", o.output);
        assert!(!o.needs_network, "an HTTPS refusal is not a permission ask");
        r.reset();
        // reset() restores the flag: still allowed afterwards.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"https://example.com/\")");
        assert!(o.output.contains("HTTPS غير مدعوم"), "got: {}", o.output);
        crate::network::set_network_allowed(false);
    }

    // Meeting 26: with the `https` feature, the flag-sticks
    // check uses a plain HTTP fetch to a closed local port:
    // the connection is refused at once, so the error is a
    // connection error, not a permission ask.
    #[test]
    #[cfg(feature = "https")]
    fn repl_network_allowed_flag_sticks_after_reset() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        crate::network::allow_local_host("127.0.0.1");
        let mut r = Repl::new();
        r.set_network_allowed(true);
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("تعذر الاتصال"), "got: {}", o.output);
        assert!(!o.needs_network, "a connection error is not a permission ask");
        r.reset();
        // reset() restores the flag: still allowed afterwards.
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("تعذر الاتصال"), "got: {}", o.output);
        crate::network::set_network_allowed(false);
    }

    // Sessions (owner's order 2026-11-05 ~21:03, overruling
    // Meeting 23 on the multiplexer core): the network flag is
    // per-instance — one console's approval never leaks into
    // another, and switching back to an approved console keeps
    // its approval without asking again.
    #[test]
    fn repl_network_flag_is_per_instance() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r1 = Repl::new();
        let mut r2 = Repl::new();
        // r1 allows; r2 is still gated.
        r1.set_network_allowed(true);
        assert!(r1.network_allowed());
        assert!(!r2.network_allowed());
        let o = r2.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r2.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(o.needs_network);
        // r1 passes the gate (the fetch itself fails on the
        // refused connection / loopback refusal — but NOT on
        // the permission error).
        let o = r1.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r1.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        // r2 allows too; switching back to r1 keeps its approval.
        r2.set_network_allowed(true);
        let o = r2.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        let o = r1.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        crate::network::set_network_allowed(false);
    }

    #[test]
    fn repl_banner_text() {
        // Minimal banner (owner's no-explanation rule): name +
        // version only, no teaching, no how-to.
        assert_eq!(banner(), "محرك نيفا ١٫٠٫٠");
    }

    // رسالة الترحيب: عربية مختصرة — الوثائق والإبلاغ
    // والأوامر اليومية فقط، وبلا سطر مجتمع (لا قناة بعد).
    #[test]
    fn repl_welcome_text() {
        let text = welcome_text();
        assert!(text.starts_with("أهلا بك في نيفا"), "got: {}", text);
        assert!(text.contains("https://github.com/RIO4140/niva"), "got: {}", text);
        assert!(
            text.contains("https://github.com/RIO4140/niva/issues"),
            "got: {}",
            text
        );
        for cmd in ["ات", "شغل", "تثبيت", "حدث حزم", "تن"] {
            assert!(text.contains(cmd), "got: {}", text);
        }
        assert!(!text.contains("المجتمع"), "got: {}", text);
    }

    // مفتاح `الترحيب` في niva.properties: `لا` يوقفها،
    // وغياب الملف أو المفتاح أو أي قيمة أخرى يبقيها.
    #[test]
    fn repl_welcome_enabled_by_properties() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        let settings_dir = home.join("إعدادات");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let r = Repl::with_program_dir(dir.clone());
        assert!(r.welcome_enabled());
        // المسار الجديد: ~/إعدادات/niva.properties.
        std::fs::write(settings_dir.join("niva.properties"), "# تعليق\nالترحيب=لا\n").unwrap();
        assert!(!r.welcome_enabled());
        std::fs::write(settings_dir.join("niva.properties"), "الترحيب = نعم\n").unwrap();
        assert!(r.welcome_enabled());
        std::fs::write(settings_dir.join("niva.properties"), "المظهر=فاتح\n").unwrap();
        assert!(r.welcome_enabled());
        // المسار القديم (جذر مجلد البرنامج) يُنقل عند
        // القراءة (اجتماع 45 §59، البند 36).
        let _ = std::fs::remove_file(settings_dir.join("niva.properties"));
        std::fs::write(dir.join("niva.properties"), "الترحيب=لا\n").unwrap();
        assert!(!r.welcome_enabled());
        assert!(settings_dir.join("niva.properties").is_file());
        assert!(!dir.join("niva.properties").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_banner_terminal_version() {
        // بلاغ 2026-11-08: `اص` في الترمنال يعرض نسخة
        // المحرك لا نسخة الترمنال — اللافتة مع نسخة
        // ترمنال تسمّي الترمنال أولا وتبقي المحرك.
        assert_eq!(
            banner_for(Some("1.2.11")),
            "ترمنال نيفا ١٫٢٫١١ (محرك ١٫٠٫٠)"
        );
        assert_eq!(
            banner_for(Some("2.0")),
            "ترمنال نيفا ٢٫٠ (محرك ١٫٠٫٠)"
        );
        assert_eq!(banner_for(None), "محرك نيفا ١٫٠٫٠");
    }

    // Arabic terminal shortcuts in the REPL (owner's order
    // 2026-11-05): they run like any other builtin here.
    // The test runs with the engine crate dir as the working
    // directory, so Cargo.toml is one of the listed files.
    // Full-word file builtins in the REPL (the shortcut
    // layer calls these as its implementation).
    #[test]
    fn repl_file_shortcuts_work() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("المجلد_الحالي()");
        assert!(!o.ended);
        // Home displays as `~` (owner's order 2026-11-05
        // ~22:38), never the absolute internal path.
        assert_eq!(o.output.trim(), "~", "got: {}", o.output);
        let o = r.feed("اعرض_الملفات()");
        assert!(!o.ended);
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Short terminal commands (owner's order 2026-11-05 ~22:02):
    // two-letter codes, space-separated args, no comma ever.
    fn shortcut_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("niva-shortcut-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // اجتماع 45 §59: الجلسة تبدأ في البيت — أنشئه
        // ليكون مجلد العمل موجودا للاختبارات.
        std::fs::create_dir_all(dir.join("home")).unwrap();
        dir
    }

    #[test]
    fn repl_shortcut_list_and_pwd() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        std::fs::write(home.join(".مخفي"), "سر").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe = home.join("شغّل");
            std::fs::write(&exe, "نص").unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink("أ.txt", home.join("رابط")).unwrap();
        }
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("مح");
        assert!(!o.ended);
        // Home displays as `~` (owner's order 2026-11-05
        // ~22:38), never the absolute internal path.
        assert_eq!(o.output.trim(), "~", "got: {}", o.output);
        let o = r.feed("ات");
        assert!(!o.ended);
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        assert!(o.output.contains("فرعي/"), "got: {}", o.output);
        // مثل `ls` الافتراضي: المخفي لا يظهر.
        assert!(!o.output.contains(".مخفي"), "got: {}", o.output);
        #[cfg(unix)]
        {
            // علامات `ls -F`: `*` للتنفيذي و`@` للرابط.
            assert!(o.output.contains("شغّل*"), "got: {}", o.output);
            assert!(o.output.contains("رابط@"), "got: {}", o.output);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // قيمة فارغة (كقيمة تثبيت-الكل) لا تطبع سطرا فارغا
    // بعدها — الموجّه يأتي مباشرة.
    #[test]
    fn repl_empty_value_prints_nothing() {
        let mut r = Repl::new();
        let o = r.feed("\"\"");
        assert!(!o.ended);
        assert_eq!(o.output, "", "got: {:?}", o.output);
        let o = r.feed("٢ + ٢");
        assert_eq!(o.output, "٤\n", "got: {:?}", o.output);
    }

    #[test]
    fn repl_shortcut_cd_mkdir_rmdir() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شم جديد");
        assert!(!o.ended);
        // الدفعة 5 (اجتماع 59 §74، الميزة 211):
        // `أنشئ_مجلد` ترجع قائمة ما أُنشئ،
        // والاختصار يطبعها سطرا لكل عنصر.
        assert_eq!(o.output, "جديد\n", "got: {}", o.output);
        assert!(home.join("جديد").is_dir());
        let o = r.feed("انتقل جديد");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("جديد"), "got: {}", o.output);
        let o = r.feed("انتقل ..");
        assert_eq!(o.output, "");
        let o = r.feed("حم جديد");
        assert_eq!(o.output, "");
        assert!(!home.join("جديد").exists());
        // اجتماع 45 §59: من البيت `..` تصل إلى مجلد
        // البرنامج ($نيفا) — مسموحة؛ والثانية تتجاوزه
        // فتُرفض.
        let o = r.feed("انتقل ..");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "$نيفا $ ");
        let o = r.feed("انتقل ..");
        assert!(o.output.contains("خارج مجلد البرنامج"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_cd_bare_goes_parent_step_by_step() {
        // أمر المالك المباشر 2026-11-09 (يلغي §66 جزئيا):
        // بلا معاملات → الأب مجلدا واحدا كل مرة؛ و`-`
        // اسم عادي غير موجود؛ و`..` كما كان.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("أ/ب/ج")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انتقل أ");
        assert_eq!(o.output, "");
        let o = r.feed("انتقل ب");
        assert_eq!(o.output, "");
        let o = r.feed("انتقل ج");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("ج"), "got: {}", o.output);
        // بلا معاملات من عمق 3 → الأب مجلدا واحدا كل مرة.
        let o = r.feed("انتقل");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("ب") && !o.output.contains("ج"), "got: {}", o.output);
        let o = r.feed("انتقل");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert!(o.output.contains("أ") && !o.output.contains("ب"), "got: {}", o.output);
        let o = r.feed("انتقل");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert_eq!(o.output, "~\n", "got: {}", o.output);
        // من البيت بلا معاملات → مجلد البرنامج.
        let o = r.feed("انتقل");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "$نيفا $ ");
        // ومن مجلد البرنامج بلا معاملات → رفض.
        let o = r.feed("انتقل");
        assert!(o.output.contains("خارج مجلد البرنامج"), "got: {}", o.output);
        // `-` اسم عادي غير موجود.
        let o = r.feed("انتقل -");
        assert!(o.output.contains("المجلد غير موجود: -"), "got: {}", o.output);
        // `..` كما كان.
        let mut r2 = Repl::with_program_dir(dir.clone());
        let o = r2.feed("انتقل أ");
        assert_eq!(o.output, "");
        let o = r2.feed("انتقل ..");
        assert_eq!(o.output, "");
        let o = r2.feed("مح");
        assert_eq!(o.output, "~\n", "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_nav56_stack_phrases() {
        // اجتماع 56 §71 — الدفعة 2 (التنقل): عبارات
        // المكدس الأربع في REPL.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("أ")).unwrap();
        std::fs::create_dir_all(home.join("ب")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("ادفع مجلد أ");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("ادفع مجلد ~/ب");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("اعرض المكدس");
        assert!(o.output.contains("~/ب"), "got: {}", o.output);
        assert!(o.output.contains("~/أ"), "got: {}", o.output);
        assert!(o.output.contains("~"), "got: {}", o.output);
        let o = r.feed("ادفع مجلد");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("مح");
        assert_eq!(o.output, "~/أ\n", "got: {}", o.output);
        let o = r.feed("أخرج مجلد");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("مح");
        assert_eq!(o.output, "~/ب\n", "got: {}", o.output);
        let o = r.feed("امسح المكدس");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("اعرض المكدس");
        assert_eq!(o.output, "~/ب\n", "got: {}", o.output);
        // انتقل بمعاملين (العلم) من العبارة.
        let o = r.feed("انتقل ~ فيزيائي");
        assert_eq!(o.output, "", "got: {}", o.output);
        let o = r.feed("مح");
        assert_eq!(o.output, "~\n", "got: {}", o.output);
        let o = r.feed("انتقل أ غريب");
        assert!(o.output.contains("علم انتقل غير معروف: غريب"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_show_file_and_rm_file() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص الملف").unwrap();
        std::fs::create_dir(home.join("مجلد")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // اف = اعرض ملف (cat).
        let o = r.feed("اف أ.txt");
        assert!(!o.ended);
        assert!(o.output.contains("نص الملف"), "got: {}", o.output);
        // حم is dir-only: a file is an error.
        let o = r.feed("حم أ.txt");
        assert!(o.output.contains("ليس مجلدا"), "got: {}", o.output);
        // حف is file-only: a dir is an error.
        let o = r.feed("حف مجلد");
        assert!(o.output.contains("ليس ملفا"), "got: {}", o.output);
        let o = r.feed("حف أ.txt");
        assert_eq!(o.output, "");
        assert!(!home.join("أ.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_copy_move_space_separated() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انسخ أ.txt ب.txt");
        assert!(!o.ended);
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "نص");
        let o = r.feed("انقل ب.txt ج.txt");
        assert_eq!(o.output, "");
        assert!(!home.join("ب.txt").exists());
        assert!(home.join("ج.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_mkdir_p_creates_parents() {
        // الاجتماع 53 §67 (الميزة 210): اختصار `شم`
        // يمر عبر الدالة نفسها — فينشئ الآباء.
        let dir = shortcut_dir();
        let home = dir.join("home");
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شم أ/ب/ج");
        assert!(!o.ended);
        // الدفعة 5 (اجتماع 59 §74، الميزة 211):
        // القائمة بالترتيب سطرا لكل عنصر، والموجود
        // قائمة فارغة فلا مخرجات.
        assert_eq!(o.output, "أ\nأ/ب\nأ/ب/ج\n", "got: {}", o.output);
        assert!(home.join("أ/ب/ج").is_dir());
        let o = r.feed("شم أ/ب/ج");
        assert_eq!(o.output, "", "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_move_dir_into_itself_refused() {
        // الميزة 204: عبارة `انقل` تمر عبر الدالة
        // نفسها — فرفض نقل مجلد داخل نفسه يصلها.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مجلد/داخلي")).unwrap();
        std::fs::write(home.join("مجلد/داخلي/أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انقل مجلد مجلد/فرعي");
        assert!(o.output.contains("لا يمكن نقل المجلد داخل نفسه"), "got: {}", o.output);
        assert_eq!(
            std::fs::read_to_string(home.join("مجلد/داخلي/أ.txt")).unwrap(),
            "نص"
        );
        let o = r.feed("انقل مجلد منقول");
        assert_eq!(o.output, "");
        assert!(!home.join("مجلد").exists());
        assert!(home.join("منقول/داخلي/أ.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_copy_onto_itself_refused() {
        // الميزة 202: عبارة `انسخ` تمر عبر الدالة
        // المدمجة نفسها — فالرفض يشملها، والملف سليم.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انسخ أ.txt أ.txt");
        assert!(!o.ended);
        assert!(
            o.output.contains("لا يمكن نسخ الملف على نفسه"),
            "got: {}",
            o.output
        );
        assert_eq!(std::fs::read_to_string(home.join("أ.txt")).unwrap(), "نص");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repl_shortcut_wrong_args_and_not_a_shortcut() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        // Wrong arg counts are short Arabic errors; the session survives.
        // أمر المالك المباشر 2026-11-09: `انتقل` بلا
        // معاملات → الأب (لا خطأ) — من البيت إلى مجلد البرنامج.
        let o = r.feed("انتقل");
        assert_eq!(o.output, "");
        let o = r.feed("مح");
        assert_eq!(o.output, "$نيفا\n", "got: {}", o.output);
        let o = r.feed("انتقل أ ب");
        // اجتماع 56 §71: `انتقل` صارت 0–2 معاملات —
        // فالثاني هنا علم غير معروف (لا خطأ عدد).
        assert!(o.output.contains("علم انتقل غير معروف: ب"), "got: {}", o.output);
        let o = r.feed("انتقل أ ب ج");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("انسخ أ.txt");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("ات زيادة");
        // اجتماع 69 §84: `ات` صارت 0..N معاملات —
        // فالمعامل هنا مسار، والغائب خطأ عربي يسمّيه
        // (لا خطأ عدد).
        assert!(
            o.output.contains("المسار غير موجود: زيادة"),
            "got: {}",
            o.output
        );
        // Not a shortcut: falls through to normal code.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("س");
        assert_eq!(o.output, "٥\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // أوامر كاملة بكلمة واحدة (2026-11-08 ~20:00، قاعدة
    // 4–5 حروف بلا ألف في النصف): انتقل/انسخ/انقل/موجود
    // بدلا من نم/سم/قم/وج — والأكواد المحذوفة نفسها لم
    // تعد تعمل (لا اختصار ولا أمر)؛ وعر/اط خرجا لأن
    // :اعرض واطبع يعملان أصلا بصيغتهما الكاملة.
    #[test]
    fn repl_full_word_commands_replace_removed_codes() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("انتقل ..");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("نم فرعي");
        assert!(!o.output.is_empty(), "removed code must not silently work");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("انسخ أ.txt ب.txt");
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "نص");
        let o = r.feed("انقل ب.txt ج.txt");
        assert_eq!(o.output, "");
        assert!(home.join("ج.txt").exists());
        let o = r.feed("موجود أ.txt");
        assert_eq!(o.output, "صحيح\n");
        let o = r.feed("موجود غائب.txt");
        assert_eq!(o.output, "خطأ\n");
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("عر");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        let o = r.feed("اط مرحبا");
        assert_ne!(o.output, "مرحبا\n", "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The 31-shortcut table (owner's correction 2026-11-05
    // ~22:11, superseding the 100): exactly 31 codes, all
    // present, no collisions. The 3 app-level codes (جل،
    // مد، اع) are NOT here — the app intercepts them before
    // the line reaches the engine.
    // Change 2026-11-06 (owner's whole-table no-alif rule):
    // خج/طن/طع/جذ/قو left the engine table and جد left the
    // app table; عع/نب/اب/تم/ال/كا joined the engine table
    // — 44 engine codes + 6 app codes = 50.
    // Change 2026-11-08 (owner's «اكتب دول كاملين»):
    // بم/بع/مس/مع/اد/اق/حع/مى/ال/كا left the engine table
    // and مش/شب/سش left the app table, to their full
    // phrases — 34 engine codes + 3 app codes = 37.
    // Change 2026-11-08 ~20:00 (owner's 4–5-letter rule):
    // نم/سم/قم/وج/عر/اط left the engine table, to their
    // full commands — 28 engine codes + 3 app codes = 31.
    #[test]
    fn shortcut_table_is_thirty_one_unique_codes() {
        let codes = [
            "ات", "مح", "شم", "حم", "حف", "اف", "شف", "عب", "عن", "عس",
            "عك", "عش", "حج", "شر", "اك", "ام", "تم", "عم", "اس",
            "اص", "رع", "تا", "وق", "نو", "اج",
            "عع", "نب", "اب",
        ];
        assert_eq!(codes.len(), 28, "engine codes (31 total - 3 app-level)");
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(shortcut_def(code).is_some(), "missing code: {}", code);
            assert!(seen.insert(code), "duplicate code: {}", code);
        }
        // The 9 original codes stay inside the 31 — except
        // نم/سم/قم, removed by the 4–5-letter rule.
        for code in ["ات", "مح", "شم", "حم", "حف", "اف"] {
            assert!(shortcut_def(code).is_some(), "original code lost: {}", code);
        }
        // App-level codes must NOT be engine shortcuts.
        for code in ["جل", "مد", "اع"] {
            assert!(shortcut_def(code).is_none(), "app code leaked into engine: {}", code);
        }
        // The removed codes are gone from both tables.
        for code in [
            "خج", "طن", "طع", "جذ", "قو", "جد",
            "بم", "بع", "مس", "مع", "اد", "اق", "حع", "مى", "ال", "كا",
            "مش", "شب", "سش",
            "نم", "سم", "قم", "وج", "عر", "اط",
        ] {
            assert!(shortcut_def(code).is_none(), "removed code still present: {}", code);
        }
    }

    // New file builtins through the shortcuts (space-separated,
    // no comma): touch / head / tail / wc / grep / find / tree /
    // du / ln / test / file-type / count / write.
    #[test]
    fn repl_shortcut_new_file_builtins() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "ألف\nباء\nجيم\nدال\nهاء\nواو").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        std::fs::write(home.join("فرعي/ملف-داخلي.txt"), "داخل").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شف جديد.txt");
        assert_eq!(o.output, "");
        assert_eq!(std::fs::read_to_string(home.join("جديد.txt")).unwrap(), "");
        let o = r.feed("عب ملف.txt");
        assert_eq!(o.output, "ألف\nباء\nجيم\nدال\nهاء\n");
        let o = r.feed("عب ملف.txt ٢");
        assert_eq!(o.output, "ألف\nباء\n");
        let o = r.feed("عن ملف.txt ٢");
        assert_eq!(o.output, "هاء\nواو\n");
        let o = r.feed("عس ملف.txt");
        assert_eq!(o.output, "٦\n");
        let o = r.feed("عك ملف.txt");
        assert_eq!(o.output, "٦\n");
        let o = r.feed("ابحث في ملف ألف ملف.txt");
        assert_eq!(o.output, "ألف\n");
        let o = r.feed("ابحث عن ملف ملف");
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        assert!(o.output.contains("فرعي/ملف-داخلي.txt"), "got: {}", o.output);
        let o = r.feed("عش");
        assert!(o.output.contains("فرعي/"), "got: {}", o.output);
        assert!(o.output.contains("  ملف-داخلي.txt"), "got: {}", o.output);
        let expected = std::fs::metadata(home.join("ملف.txt")).unwrap().len() as f64;
        let o = r.feed("حج ملف.txt");
        assert_eq!(parse_shortcut_number(o.output.trim()), Some(expected));
        let o = r.feed("موجود ملف.txt");
        assert_eq!(o.output, "صحيح\n");
        let o = r.feed("موجود غائب.txt");
        assert_eq!(o.output, "خطأ\n");
        let o = r.feed("ام فرعي");
        assert_eq!(o.output, "مجلد\n");
        let o = r.feed("ام غائب.txt");
        assert_eq!(o.output, "غير موجود\n");
        // عم before شر: ملف.txt + فرعي + جديد.txt = 3.
        let o = r.feed("عم");
        assert_eq!(o.output, "٣\n");
        let o = r.feed("شر ملف.txt رابط.txt");
        assert_eq!(o.output, "");
        assert_eq!(
            std::fs::read_to_string(home.join("رابط.txt")).unwrap(),
            "ألف\nباء\nجيم\nدال\nهاء\nواو"
        );
        // اك: path + the rest joined with spaces.
        let o = r.feed("اك ملف٢.txt مرحبا بالعالم");
        assert_eq!(o.output, "");
        assert_eq!(
            std::fs::read_to_string(home.join("ملف٢.txt")).unwrap(),
            "مرحبا بالعالم"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Text and numbers: random stays a shortcut; echo left
    // the table 2026-11-08 ~20:00 (owner's 4–5-letter
    // rule) — اطبع is the full command. Sum, min, max,
    // convert and range left the table 2026-11-08
    // (owner's «اكتب دول كاملين») — their plain builtins
    // مجموع/أدنى/أقصى/عدد/مدى take a list (مجموع/أدنى/
    // أقصى) or comma-separated args in normal code.
    // (طن/طع/جذ/قو left the table 2026-11-06 under the
    // no-alif rule; their builtins — طول/مطلق/
    // رياضيات.جذر/رياضيات.قوة — still work in programs.)
    #[test]
    fn repl_shortcut_numbers_and_text() {
        let mut r = Repl::new();
        assert_eq!(r.feed("مجموع([١، ٢، ٣])").output, "٦\n");
        assert_eq!(r.feed("أدنى([٥، ٢، ٨])").output, "٢\n");
        assert_eq!(r.feed("أقصى([٥، ٢، ٨])").output, "٨\n");
        assert_eq!(r.feed("عدد(\"٥\")").output, "٥\n");
        assert_eq!(r.feed("مدى(١، ٥)").output, "[١, ٢, ٣, ٤]\n");
        let o = r.feed("رع");
        let n = parse_shortcut_number(o.output.trim());
        assert!(matches!(n, Some(v) if (0.0..1.0).contains(&v)), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا بالعالم\"").output, "مرحبا بالعالم\n");
        // A non-number where a number is expected: short error.
        let o = r.feed("عدد(\"نص\")");
        assert!(o.output.contains("لا يمكن تحويل"), "got: {}", o.output);
    }

    // The 2026-11-06 replacements (owner's whole-table
    // no-alif rule): عع/نب/اب/تم — each wired to a
    // real action. (ال/كا left the table 2026-11-08 to
    // their full phrases «ارجع للبيت» و«كرر الأخير».)
    // The نب/اب error paths assume the serial
    // runner (CI runs --test-threads=1): the snippet's
    // `استورد عمليات` resets the global process policy
    // right before the spawn, so no other test can
    // interleave — exactly the guarantee the processes
    // tests get from their lock.
    #[test]
    fn repl_shortcut_replacements() {
        // عع: the current process id — a positive number.
        let mut r = Repl::new();
        let o = r.feed("عع");
        let pid = parse_shortcut_number(o.output.trim());
        assert!(matches!(pid, Some(v) if v > 0.0), "got: {}", o.output);
        // نب/اب: wiring only — no universal program exists
        // on all three CI runners for a happy path (the
        // spawn/kill happy paths are covered in
        // processes.rs). A missing program surfaces the
        // Arabic spawn error; an unknown id surfaces the
        // Arabic kill error.
        let o = r.feed("نب لاوجود");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        let o = r.feed("اب ٩٩٩٩٩٩");
        assert!(o.output.contains("عملية غير معروفة"), "got: {}", o.output);
        // ارجع للبيت: from a subdir back home — the prompt
        // returns to "~ $ ".
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("ارجع للبيت");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        // تم: details of a file, a dir, and a missing path.
        std::fs::write(home.join("ملف.txt"), "abc").unwrap();
        let o = r.feed("تم ملف.txt");
        assert!(o.output.contains("\"نوع\": \"ملف\""), "got: {}", o.output);
        assert!(o.output.contains("\"حجم\": ٣"), "got: {}", o.output);
        let o = r.feed("تم فرعي");
        assert!(o.output.contains("\"نوع\": \"مجلد\""), "got: {}", o.output);
        let o = r.feed("تم غائب.txt");
        assert!(o.output.contains("\"نوع\": \"غير موجود\""), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
        // كرر الأخير: rerun the last line (!!). Empty history
        // is a short Arabic error; it never reruns itself.
        let mut r = Repl::new();
        let o = r.feed("كرر الأخير");
        assert!(o.output.contains("لا يوجد سطر سابق"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("س = ٥").output, "");
        assert_eq!(r.feed("كرر الأخير").output, "");
        // The removed codes' builtins still work as code.
        assert_eq!(done_out(r.eval("طول(\"مرحبا\")")), "٥\n");
        assert_eq!(done_out(r.eval("مطلق(٠ - ٥)")), "٥\n");
        assert_eq!(done_out(r.eval("استورد رياضيات")), "");
        assert_eq!(done_out(r.eval("رياضيات.جذر(١٦)")), "٤\n");
        assert_eq!(done_out(r.eval("رياضيات.قوة(٢، ٣)")), "٨\n");
    }

    // تثبيت أدوات نيفا (أمر الاسم الكامل، بلا كود حرفين):
    // من مسار محلي إلى مجلد البرنامج، ثم الاستيراد —
    // في الوضعين (المترجم والمفسر التشخيصي).
    #[test]
    fn repl_full_command_install_local() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let home = dir.join("home");
            std::fs::create_dir(home.join("مصادر")).unwrap();
            std::fs::write(home.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n")
                .unwrap();
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            let o = r.feed("تثبيت مصادر/أداة.نيفا");
            assert!(!o.ended);
            assert_eq!(o.output, "ثُبّتت الوحدة أداة\n", "got: {}", o.output);
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٤٢\n"
            );
            // والوحدة المثبّتة تُستورد فورا.
            assert_eq!(r.feed("استورد أداة").output, "");
            assert_eq!(r.feed("أداة.قيمة").output, "٤٢\n");
            // الثانية مرفوضة: بلا كتابة صامتة فوق موجود.
            let o = r.feed("تثبيت مصادر/أداة.نيفا");
            assert!(
                o.output.contains("مثبّتة أصلا"),
                "got: {}",
                o.output
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٤٢\n"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // تثبيت: الرفوض — الوسائط، وليس .نيفا، والخروج عن
    // المجلد، والمجلد، والغائب، واسم الوحدة الأصلية،
    // والمخطط غير http/https، والشبكة المغلقة (البوابة
    // قبل الشبكة نفسها). والجدول يبقى 37: تثبيت ليس
    // كود اختصار أبدا.
    #[test]
    fn repl_full_command_install_rejections() {
        assert!(shortcut_def("تثبيت").is_none());
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("مصادر")).unwrap();
        std::fs::write(home.join("مصادر/نص.txt"), "نص").unwrap();
        std::fs::write(home.join("مصادر/أداة.نيفا"), "صدر قيمة\nقيمة = ١\n").unwrap();
        std::fs::write(home.join("مصادر/رياضيات.نيفا"), "صدر س\nس = ١\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // العارية وحدها كود (اسم مدمج معروف تحليليا) —
        // فترد «متغير غير معرف» وقت التشغيل، كأي دالة
        // مدمجة عارية (طول) — لا خطأ وسائط.
        let o = r.feed("تثبيت");
        assert!(
            o.output.contains("متغير غير معرف: تثبيت"),
            "got: {}",
            o.output
        );
        let o = r.feed("تثبيت أ ب");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/نص.txt");
        assert!(o.output.contains(".نيفا"), "got: {}", o.output);
        // اجتماع 45 §59: من البيت الخروج من القاعدة
        // يلزم `../..` لا `..` وحده.
        let o = r.feed("تثبيت ../../خارج.نيفا");
        assert!(
            o.output.contains("خارج مجلد البرنامج"),
            "got: {}",
            o.output
        );
        let o = r.feed("تثبيت مصادر");
        assert!(o.output.contains("ليس ملفا"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/غائب.نيفا");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        let o = r.feed("تثبيت مصادر/رياضيات.نيفا");
        assert!(o.output.contains("وحدة أصلية"), "got: {}", o.output);
        let o = r.feed("تثبيت ftp://مثال.مصر/أداة.نيفا");
        assert!(o.output.contains("http وhttps"), "got: {}", o.output);
        // الشبكة مغلقة افتراضيا: البوابة، لا الشبكة.
        let o = r.feed("تثبيت http://127.0.0.1:9/أداة.نيفا");
        assert!(o.needs_network, "got: {}", o.output);
        assert!(
            o.output.contains(crate::network::PERMISSION_ERROR_MARKER),
            "got: {}",
            o.output
        );
        assert!(!dir.join("أداة.نيفا").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // `حدث حزم` (أمر المالك 2026-11-10 ~03:07، قاعدة
    // مؤكدة): أمر مباشر في الموجه لأي مستخدم — في
    // الوضعين، ومن مجلد مؤقت بلا ملفات مدير (فالمدمج
    // هو الذي يُحمَّل). التنزيل يفشل هنا (الشبكة مغلقة
    // في الجلسة، والشهادة مرفوضة على هذه الآلة أصلا)
    // فيرجع الخطأ العربي النظيف من خطوة التنزيل؛ وإن
    // نجح التنزيل في بيئة أخرى فالنص الآخر يثبت
    // الوصول نفسه — لا «متغير غير معرف» في الحالين.
    #[test]
    fn repl_hadath_huzam_phrase() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            let o = r.feed("حدث حزم");
            assert!(
                o.output.contains("تعذّر تنزيل الفهرس")
                    || o.output.contains("جارٍ تحديث قائمة الحزم"),
                "got: {}",
                o.output
            );
            assert!(
                !o.output.contains("متغير غير معرف"),
                "got: {}",
                o.output
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // المدير المدمج وحده: `استورد أوامر` من مجلد مؤقت
    // بلا ملفات يحمّل المدمج، ومساعدته تسرد `حدث حزم`.
    #[test]
    fn repl_embedded_manager_import() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            let o = r.feed("استورد أوامر");
            assert!(
                !o.output.contains("وحدة غير موجودة"),
                "got: {}",
                o.output
            );
            let o = r.feed("اطبع أوامر.استعمال()");
            assert!(o.output.contains("حدث حزم"), "got: {}", o.output);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // `حدث حزم` بلا معاملات فقط: أي كلمة زائدة خطأ
    // عدد معاملات بأسلوب العبارات.
    #[test]
    fn repl_hadath_huzam_args() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("حدث حزم زيادة");
        assert!(
            o.output.contains("تتوقع بدون معاملات"),
            "got: {}",
            o.output
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // `حدث` وحدها بلا `حزم` بعدها ليست عبارة — تبقى
    // كودا عاديا فيرد «متغير غير معرف» كما قبل.
    #[test]
    fn repl_hadath_bare_still_code() {
        let dir = shortcut_dir();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("حدث");
        assert!(
            o.output.contains("متغير غير معرف: حدث"),
            "got: {}",
            o.output
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// خادم HTTP محلي صغير لاختبار التثبيت من رابط: يرد
    /// بمحتوى ثابت على أول طلب ثم ينتهي (اتصال واحد —
    /// بلا keep-alive، كقاعدة المحرك).
    fn install_test_server(body: &'static str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // يقبل حتى 4 طلبات (الاختبار ينزّل مرتين:
            // رابط خام + رابط مرمّز) ثم ينتهي.
            for _ in 0..4 {
                let Ok((mut s, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        port
    }

    // تثبيت من رابط: خادم محلي، شبكة مسموحة، مضيف محلي
    // مسموح — في الوضعين. يفترض التشغيل المتسلسل (CI
    // --test-threads=1): العلم العام يُعاد ضبطه في النهاية.
    #[test]
    fn repl_full_command_install_url() {
        for interpreter in [false, true] {
            let dir = shortcut_dir();
            let port = install_test_server("صدر قيمة\nقيمة = ٧\n");
            let mut r = if interpreter {
                Repl::with_program_dir_interpreter(dir.clone())
            } else {
                Repl::with_program_dir(dir.clone())
            };
            r.set_network_allowed(true);
            r.allow_local_network_host("127.0.0.1");
            let o = r.feed(&format!(
                "تثبيت http://127.0.0.1:{}/أداة.نيفا",
                port
            ));
            assert!(!o.ended);
            assert_eq!(o.output, "ثُبّتت الوحدة أداة\nبصمة الملف: 3d5a2de9551ce2a171b674bd2360a7ed0d355a5d33e15b7893a4e361bd68678f\n", "got: {}", o.output);
            assert_eq!(
                std::fs::read_to_string(dir.join("أداة.نيفا")).unwrap(),
                "صدر قيمة\nقيمة = ٧\n"
            );
            assert_eq!(r.feed("استورد أداة").output, "");
            assert_eq!(r.feed("أداة.قيمة").output, "٧\n");
            // والرابط المرمّز بالنسب المئوية (الصيغة التي
            // تصل بها الروابط العربية عادة) — الاسم بعد
            // الفك «أداة» أيضا، والتثبيت الثاني مرفوض.
            let o = r.feed(&format!(
                "تثبيت http://127.0.0.1:{}/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7",
                port
            ));
            assert!(o.output.contains("مثبّتة أصلا"), "got: {}", o.output);
            r.set_network_allowed(false);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // Time, version, and the session commands through the
    // shortcuts; fetch with the network closed (the default)
    // hits the permission gate, not the network.
    #[test]
    fn repl_shortcut_time_version_fetch_commands() {
        let mut r = Repl::new();
        let o = r.feed("تا");
        let date = o.output.trim_end().to_string();
        assert_eq!(date.len(), 19, "got: {}", date);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[10..11], " ");
        let o = r.feed("اص");
        assert_eq!(o.output, "محرك نيفا ١٫٠٫٠\n");
        let o = r.feed("وق");
        assert!(parse_shortcut_number(o.output.trim()).is_some(), "got: {}", o.output);
        let o = r.feed("نو ٠");
        assert_eq!(o.output, "");
        let o = r.feed("اج https://example.com");
        assert!(o.needs_network, "got: {}", o.output);
        assert!(
            o.output.contains(crate::network::PERMISSION_ERROR_MARKER),
            "got: {}",
            o.output
        );
        // Session commands: help, show history, clear, quit.
        let o = r.feed("اس");
        assert!(o.output.contains(":خروج"), "got: {}", o.output);
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed(":اعرض");
        assert!(o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed("امسح الجلسة");
        assert!(!o.ended);
        let o = r.feed(":اعرض");
        assert!(!o.output.contains("س = ٥"), "got: {}", o.output);
        let o = r.feed(":خروج");
        assert!(o.ended);
    }

    // Owner's orders 2026-11-05 ~22:58 and ~23:03: the prompt
    // is the current directory, terminal style — `~ $ ` at
    // home, `~/dir $ ` below it, `> ` while a block is open —
    // never `نيفا: `.
    #[test]
    fn repl_prompt_is_directory_and_dollar() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("س = ٥");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("انتقل ..");
        assert_eq!(o.prompt, "~ $ ");
        let o = r.feed("دالة جمع(أ، ب):");
        assert_eq!(o.prompt, "> ");
        let o = r.feed("");
        assert_eq!(o.prompt, "~ $ ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-12: shortcut codes never shadow Niva names —
    // `ات = ٥` runs as code, and once defined `ات` prints its
    // value; in a fresh session `ات` is still the file list.
    #[test]
    fn repl_shortcut_names_do_not_shadow_code() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // Undefined: the shortcut owns the bare name.
        let o = r.feed("ات");
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        // An assignment to the same name is code, not a
        // shortcut (the old code errored here).
        let o = r.feed("ات = ٥");
        assert_eq!(o.output, "", "got: {}", o.output);
        // Now defined: the bare name prints its value.
        let o = r.feed("ات");
        assert_eq!(o.output, "٥\n", "got: {}", o.output);
        // The real listing is still reachable by its full name.
        let o = r.feed("اعرض_الملفات()");
        assert!(o.output.contains("أ.txt"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Audit م-2: `:اسمح-بالشبكة` flips the network flag
    // inside the running process — no restart, no lost state.
    #[test]
    fn repl_allow_network_command_flips_flag_in_process() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(o.output.contains("الشبكة مغلقة افتراضيا"), "got: {}", o.output);
        assert!(o.needs_network);
        // The control command: no output, no restart.
        let o = r.feed(":اسمح-بالشبكة");
        assert_eq!(o.output, "");
        assert!(!o.ended);
        // The same fetch now passes the gate (it fails on the
        // refused connection instead) — and the session state
        // survived: س is still defined.
        let o = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            !o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert!(!o.needs_network);
        let o = r.feed("س");
        assert_eq!(o.output, "٥\n");
        crate::network::set_network_allowed(false);
    }

    // Audit م-6: the structured control signal of the last
    // line — NetworkNeeded on a refused fetch, SessionCleared
    // on :مسح, None otherwise.
    #[test]
    fn repl_last_control_signals() {
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        crate::network::set_network_allowed(false);
        let mut r = Repl::new();
        let _ = r.feed("س = ٥");
        assert_eq!(r.last_control(), ReplControl::None);
        let _ = r.feed("استورد شبكة");
        let _ = r.feed("شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert_eq!(r.last_control(), ReplControl::NetworkNeeded);
        let _ = r.feed("امسح الجلسة");
        assert_eq!(r.last_control(), ReplControl::SessionCleared);
        let _ = r.feed("س = ٦");
        assert_eq!(r.last_control(), ReplControl::None);
        crate::network::set_network_allowed(false);
    }

    // Audit م-16: وقت.نوم caps at 60 seconds.
    #[test]
    fn repl_sleep_caps_at_sixty() {
        let mut r = Repl::new();
        let o = r.feed("نو ٦١");
        assert!(o.output.contains("60 ثانية"), "got: {}", o.output);
        let o = r.feed("نو ٠");
        assert_eq!(o.output, "");
    }

    // ك-2 (مهمة 7): وضع التشخيص الخفي — الجلسة نفسها على
    // مفسر الشجرة (Repl::new_interpreter): المتغيرات تبقى،
    // والتعبير الأخير يُطبع، والشبكة تُفتح من داخل الجلسة.
    #[test]
    fn repl_allow_server_command_flips_flag_in_process() {
        // (اجتماع 27، س4): :اسمح-بالخادم يقلب علم الخادم
        // للجلسة نفسها، بلا إعادة تشغيل؛ و:اسمح-بالخادم
        // لا يفتح الشبكة العامة (العلمان مستقلان).
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        let mut r = Repl::new();
        let o = r.feed("استورد شبكة");
        assert_eq!(o.output, "");
        // المعالج دالة مجهولة كمعامل — القيمة نفسها
        // لا تهم: فحص الإذن يأتي قبل أي شيء آخر.
        let line = "شبكة.اخدم(١٨٤٤٣، \"server-cert.pem\"، \"server-key.pem\"، دالة_مجهولة(طلب): \"x\")";
        let o = r.feed(line);
        assert!(
            o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::ServerNeeded);
        assert!(!o.needs_network, "the server gate must not set the network signal");
        let o = r.feed(":اسمح-بالخادم");
        assert_eq!(o.output, "");
        assert!(!o.ended);
        // الاستدعاء نفسه يتجاوز البوابة الآن — يفشل على
        // ملف الشهادة الغائب (مع الميزة) أو على غياب
        // دعم TLS (بلا الميزة) بدلا من رفض الإذن —
        // وبقيت الجلسة حية بعد الأمر.
        let o = r.feed(line);
        assert!(
            !o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        #[cfg(feature = "https")]
        let expected = "تعذر قراءة ملف الشهادة";
        #[cfg(not(feature = "https"))]
        let expected = "الخادم يحتاج HTTPS";
        assert!(
            o.output.contains(expected),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::None);
        let o = r.feed("اطبع ٥");
        assert_eq!(o.output, "٥\n");
    }

    #[test]
    fn repl_server_and_network_flags_are_independent() {
        // (اجتماع 27، س4): علم الخادم وحده لا يفتح
        // الشبكة العامة (العلمان مستقلان).
        let _guard = crate::network::NET_TEST_LOCK.lock().unwrap();
        let mut r = Repl::new();
        let o = r.feed(":اسمح-بالخادم");
        assert_eq!(o.output, "");
        // الشبكة العامة ما زالت مغلقة: اجلب مرفوض.
        let o = r.feed("استورد شبكة\nاطبع شبكة.اجلب(\"http://127.0.0.1:9/\")");
        assert!(
            o.output.contains("الشبكة مغلقة افتراضيا"),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::NetworkNeeded);
        // والخادم في الوقت نفسه مفتوح: الاستدعاء يتجاوز
        // البوابة ويفشل على ملف الشهادة الغائب (مع
        // الميزة) أو على غياب دعم TLS (بلا الميزة) فقط.
        let o = r.feed("شبكة.اخدم(١٨٤٤٣، \"server-cert.pem\"، \"server-key.pem\"، دالة_مجهولة(طلب): \"x\")");
        assert!(
            !o.output.contains("الخادم مغلق افتراضيا"),
            "got: {}",
            o.output
        );
        #[cfg(feature = "https")]
        let expected = "تعذر قراءة ملف الشهادة";
        #[cfg(not(feature = "https"))]
        let expected = "الخادم يحتاج HTTPS";
        assert!(
            o.output.contains(expected),
            "got: {}",
            o.output
        );
        assert_eq!(r.last_control(), ReplControl::None);
    }

    #[test]
    fn repl_interpreter_mode_session_works() {
        let mut r = Repl::new_interpreter();
        assert_eq!(done_out(r.eval("س = ٥")), "");
        assert_eq!(done_out(r.eval("س * ٢")), "١٠\n");
        assert_eq!(
            done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب")),
            ""
        );
        assert_eq!(done_out(r.eval("جمع(٢، ٣)")), "٥\n");
        assert!(!r.network_allowed());
        let _ = r.feed(":اسمح-بالشبكة");
        assert!(r.network_allowed());
        r.reset();
        assert!(is_failed(r.eval("س")));
    }

    // ك-2 (مهمة 7): الدوال العادية تعمل على محرك VM —
    // عدد/مجموع تمر عبر VmSession::call_builtin مباشرة.
    // (حع/مع left the table 2026-11-08 — plain calls now.)
    #[test]
    fn repl_vm_shortcut_calls_builtin() {
        let mut r = Repl::new();
        assert_eq!(r.feed("عدد(\"٥\")").output, "٥\n");
        assert_eq!(r.feed("مجموع([١، ٢، ٣])").output, "٦\n");
        assert_eq!(r.feed("مح").output, "~\n");
    }

    // العبارات الكاملة (2026-11-08، أمر المالك «اكتب دول
    // كاملين»): المحرك الست — بحثان ومسح جلسة وتحويل
    // ورجوع للبيت وتكرار — مسارات سعيدة ورفوض وسائط.
    #[test]
    fn repl_full_phrases() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::write(home.join("ملف.txt"), "ألف\nباء\n").unwrap();
        std::fs::create_dir(home.join("فرعي")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // ابحث في ملف: نمط + اسم.
        let o = r.feed("ابحث في ملف ألف ملف.txt");
        assert_eq!(o.output, "ألف\n");
        // ابحث عن ملف: اسم.
        let o = r.feed("ابحث عن ملف ملف");
        assert!(o.output.contains("ملف.txt"), "got: {}", o.output);
        // حول إلى عدد: نص (بلا شدة على الواو).
        assert_eq!(r.feed("حول إلى عدد ٥").output, "٥\n");
        // الفاصلة العشرية العربية ٫ تعمل كما كانت في حع
        // القديم (المعامل يُحلَّل عددا أولا قبل التمرير).
        assert_eq!(r.feed("حول إلى عدد ٣٫٥").output, "٣٫٥\n");
        // نص ليس عددا: خطأ عربي قصير.
        let o = r.feed("حول إلى عدد نص");
        assert!(o.output.contains("لا يمكن تحويل"), "got: {}", o.output);
        // ارجع للبيت: من فرعي إلى البيت.
        let o = r.feed("انتقل فرعي");
        assert_eq!(o.prompt, "~/فرعي $ ");
        let o = r.feed("ارجع للبيت");
        assert_eq!(o.output, "");
        assert_eq!(o.prompt, "~ $ ");
        // امسح الجلسة: تعيد ضبط المحرك كـ :مسح تماما.
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        let o = r.feed("امسح الجلسة");
        assert!(!o.ended);
        let o = r.feed("س");
        assert!(o.output.contains("متغير غير معرف"), "got: {}", o.output);
        // كرر الأخير: بلا سطر سابق خطأ قصير؛ وبعده يعيد
        // تنفيذ آخر سطر، ولا يعيد تنفيذ نفسه أبدا.
        let o = r.feed("كرر الأخير");
        assert!(o.output.contains("لا يوجد سطر سابق"), "got: {}", o.output);
        assert_eq!(r.feed("اطبع \"مرحبا\"").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        assert_eq!(r.feed("كرر الأخير").output, "مرحبا\n");
        // رفوض الوسائط: بنفس أسلوب الاختصارات.
        let o = r.feed("ابحث في ملف ألف");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("ابحث في ملف ألف ملف.txt زيادة");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 3"), "got: {}", o.output);
        let o = r.feed("ابحث عن ملف");
        assert!(o.output.contains("تتوقع 1 معاملات لكن أعطيت 0"), "got: {}", o.output);
        let o = r.feed("حول إلى عدد");
        assert!(o.output.contains("تتوقع 1 معاملات لكن أعطيت 0"), "got: {}", o.output);
        let o = r.feed("امسح الجلسة زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("ارجع للبيت زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("كرر الأخير زيادة");
        assert!(o.output.contains("تتوقع بدون معاملات لكن أعطيت 1"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
    // العبارات السبع الكاملة بلا فصلة.
    #[test]
    fn repl_full_phrases_batch5() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        // «ب» تُكتب أولا ثم «أ»، فـ«أ» أحدث منها.
        std::fs::write(home.join("ب.txt"), "قديم").unwrap();
        std::fs::write(home.join("أ.txt"), "محتوى").unwrap();
        // الكتابتان المتتاليتان قد تتشاركان الطابع
        // الزمني على نظام ملفات الجهاز — ثبّت وقت
        // «ب» قديما صراحة ليكون الترتيب حتميا.
        {
            let t = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1577836800);
            crate::files::set_file_times(&home.join("ب.txt"), t).unwrap();
        }
        std::fs::create_dir_all(home.join("مجلد/فرعي")).unwrap();
        std::fs::write(home.join("مجلد/فرعي/ج.txt"), "ج").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // نسخ تكراري: بلا مخرجات، والشجرة منسوخة.
        let o = r.feed("انسخ تكراري مجلد نسخة");
        assert_eq!(o.output, "", "got: {}", o.output);
        assert_eq!(
            std::fs::read_to_string(home.join("نسخة/فرعي/ج.txt")).unwrap(),
            "ج"
        );
        // نسخ محدث: «أ» أحدث من «ب» (كُتبت بعدها).
        let o = r.feed("انسخ محدث أ.txt ب.txt");
        assert_eq!(o.output, "صحيح\n", "got: {}", o.output);
        assert_eq!(std::fs::read_to_string(home.join("ب.txt")).unwrap(), "محتوى");
        // نسخ خفيف: طريقة نصية من الثلاثة.
        let o = r.feed("انسخ خفيف أ.txt خفيف.txt");
        assert!(
            ["مستنسخ\n", "متناثر\n", "عادي\n"].contains(&o.output.as_str()),
            "got: {}",
            o.output
        );
        assert_eq!(
            std::fs::read_to_string(home.join("خفيف.txt")).unwrap(),
            "محتوى"
        );
        // رابط صلب ثم نسبي.
        let o = r.feed("أنشئ رابط صلب أ.txt صلب.txt");
        assert_eq!(o.output, "", "got: {}", o.output);
        assert_eq!(
            std::fs::read_to_string(home.join("صلب.txt")).unwrap(),
            "محتوى"
        );
        let o = r.feed("أنشئ رابط نسبي أ.txt رابط.txt");
        assert_eq!(o.output, "", "got: {}", o.output);
        assert_eq!(
            std::fs::read_link(home.join("رابط.txt")).unwrap(),
            std::path::PathBuf::from("أ.txt")
        );
        // حذف تكراري: بلا مخرجات، والمجلد ذهب.
        let o = r.feed("احذف تكراري نسخة");
        assert_eq!(o.output, "", "got: {}", o.output);
        assert!(!home.join("نسخة").exists());
        // خطأ عدد المعاملات لكل عبارة.
        let o = r.feed("انسخ تكراري مجلد");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("انسخ محدث أ.txt");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("انسخ أرشيف مجلد");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("انسخ خفيف أ.txt");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("احذف تكراري");
        assert!(o.output.contains("تتوقع 1 معاملات لكن أعطيت 0"), "got: {}", o.output);
        let o = r.feed("أنشئ رابط صلب أ.txt");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let o = r.feed("أنشئ رابط نسبي أ.txt");
        assert!(o.output.contains("تتوقع 2 معاملات لكن أعطيت 1"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // الأكواد المحذوفة (2026-11-08) لا تنفذ اختصارا أبدا
    // بعد الحذف: تسقط إلى الكود وتخطئ كاسم غير معرف،
    // بلا فعل — فـ«بم» لا تبحث، و«مس» لا تمسح الجلسة،
    // و«كا» لا تكرر.
    #[test]
    fn repl_removed_codes_do_not_execute() {
        let dir = shortcut_dir();
        std::fs::write(dir.join("ملف.txt"), "ألف\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("س = ٥");
        assert_eq!(o.output, "");
        for line in [
            "بم ألف ملف.txt",
            "بع ملف",
            "مس",
            "مع ١ ٢",
            "اد ٥ ٢",
            "اق ٥ ٢",
            "حع ٥",
            "مى ١ ٥",
            "ال",
            "كا",
        ] {
            let o = r.feed(line);
            assert!(
                o.output.contains("متغير غير معرف"),
                "line {}: got: {}",
                line,
                o.output
            );
            assert!(!o.ended, "line {} ended the session", line);
        }
        // و«مس» لم تمسح الجلسة: س ما زالت ٥.
        assert_eq!(r.feed("س").output, "٥\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ك-2 (مهمة 7): إغلاق معرّف في سطر يبقى قابلا للاستدعاء
    // في سطر لاحق — الجلسة الدائمة على VM تحتفظ بالبيئة
    // وسجل الإغلاقات عبر التحميلات.
    #[test]
    fn repl_vm_closure_persists_across_feeds() {
        let mut r = Repl::new();
        assert_eq!(
            done_out(r.eval("دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد")),
            ""
        );
        assert_eq!(done_out(r.eval("أ = اصنع_عداد()")), "");
        assert_eq!(done_out(r.eval("أ()")), "١\n");
        assert_eq!(done_out(r.eval("أ()")), "٢\n");
    }

    /// مجلد مؤقت لاختبارات `شغّل` — ملفات فقط، ويُمسح
    /// في نهاية كل اختبار.
    fn run_test_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-run-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// برنامج مثال بشرطة في اسمه يستورد وحدة: العرض
    /// المصغر لتدفق NivaDB المستهدف.
    fn write_dashed_example(dir: &std::path::Path) {
        std::fs::write(dir.join("قاعدة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n").unwrap();
        std::fs::write(
            dir.join("مثال-أول.نيفا"),
            "استورد قاعدة\nاطبع قاعدة.قيمة\nمتغير_البرنامج = ٥\n",
        )
        .unwrap();
    }

    // اجتماع 43 §58 + تجاوز المالك «بلا تعقيد»: `شغّل`
    // بالاسم العاري وبالاسم الكامل، بملف بشرطة يستورد
    // وحدة — والجلسة معزولة (متغير البرنامج لا يتسرب)
    // والتفاعلي حي بعد التشغيل.
    #[test]
    fn repl_run_command_dashed_name_and_isolation() {
        let dir = run_test_dir("dash");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        // الاسم العاري بلا امتداد (الإلحاق التلقائي).
        let o = r.feed("شغّل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        assert!(!o.ended);
        // والصيغة القانونية بلا تشكيل تعمل أيضا.
        let o = r.feed("شغل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        // متغير البرنامج لا يتسرب إلى التفاعلي.
        let o = r.feed("متغير_البرنامج");
        assert!(
            o.output.contains("متغير غير معرف"),
            "got: {}",
            o.output
        );
        // والاسم الكامل بالامتداد يشغّل أيضا.
        let o = r.feed("شغّل مثال-أول.نيفا");
        assert_eq!(o.output, "٤٢\n");
        // والتفاعلي حي بعد التشغيلين.
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: `شغّل` على المفسر أيضا — جلسة
    // جديدة على محرك التفاعلي الحالي لا على الافتراضي.
    #[test]
    fn repl_run_command_interpreter_mode() {
        let dir = run_test_dir("interp");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir_interpreter(dir.clone());
        let o = r.feed("شغّل مثال-أول");
        assert_eq!(o.output, "٤٢\n");
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 45 §59 + تجاوز المالك: الأخطاء العربية
    // القصيرة — ملف غائب، صفر معاملات، معاملات زائدة،
    // اسم فيه مسار — والتفاعلي حي بعد كل خطأ.
    #[test]
    fn repl_run_command_errors() {
        let dir = run_test_dir("err");
        write_dashed_example(&dir);
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغّل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert!(o.output.contains("غائب.نيفا"), "got: {}", o.output);
        let o = r.feed("شغّل");
        assert!(
            o.output.contains("معاملا واحدا"),
            "got: {}",
            o.output
        );
        let o = r.feed("شغّل أ ب");
        assert!(
            o.output.contains("معاملا واحدا"),
            "got: {}",
            o.output
        );
        let o = r.feed("شغّل ../غائب.نيفا");
        assert!(o.output.contains("بلا مسار"), "got: {}", o.output);
        let o = r.feed("شغّل مجلد/ملف.نيفا");
        assert!(o.output.contains("بلا مسار"), "got: {}", o.output);
        assert_eq!(r.feed("١ + ١").output, "٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: خطأ وقت التشغيل في البرنامج يُطبع
    // بالعربية (بعد مخرجاته الجزئية) ويعود التحكم إلى
    // الموجه بلا خروج من التفاعلي.
    #[test]
    fn repl_run_command_runtime_error_returns_to_prompt() {
        let dir = run_test_dir("rterr");
        std::fs::write(dir.join("خاطئ.نيفا"), "اطبع ١\nس = ١ / ٠\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("شغّل خاطئ");
        assert!(o.output.starts_with("١\n"), "got: {}", o.output);
        assert!(o.output.contains("خطأ"), "got: {}", o.output);
        assert!(!o.ended);
        assert_eq!(r.feed("٢ + ٢").output, "٤\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // اجتماع 43 §58: `شغّل` تفاعلي فقط — لا دالة
    // برنامجية: من استدعاه من الكود يأخذ خطأ الاسم غير
    // المعرف المعتاد.
    #[test]
    fn repl_run_command_not_a_builtin() {
        let mut r = Repl::new();
        let failed = match r.eval("شغّل(\"x.نيفا\")") {
            ReplEval::Failed { error, .. } => error,
            ReplEval::Done { .. } => panic!("expected Failed, got Done"),
            ReplEval::Incomplete => panic!("expected Failed, got Incomplete"),
            ReplEval::Exited { .. } => panic!("expected Failed, got Exited"),
        };
        assert!(
            failed.contains("متغير غير معرف"),
            "got: {}",
            failed
        );
        // المحلل يجرّد الشدة فيبلغ الاسم بلا شدة (درس
        // المهمة 24): «شغل» لا «شغّل».
        assert!(failed.contains("شغل"), "got: {}", failed);
    }

    // اجتماع 45 §59 — الدفعة 2: معلومات، أعد-الإعدادات،
    // أعد-الضبط، النقل والقفل، الترحيل، البدء والمقتطفات،
    // والبحث في المكانين.

    #[test]
    fn batch2_info_is_exactly_five_lines() {
        let dir = shortcut_dir();
        // وحدتان مثبّتتان في جذر مجلد البرنامج.
        std::fs::write(dir.join("أداة.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("أداة٢.نيفا"), "صدر س\nس = ٢\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed("معلومات");
        let lines: Vec<&str> = o.output.lines().collect();
        assert_eq!(lines.len(), 5, "got: {}", o.output);
        assert_eq!(lines[0], "محرك نيفا ١٫٠٫٠");
        assert_eq!(lines[1], "ترمنال التطبيق: غير معروف");
        assert_eq!(lines[2], "الوحدات المثبتة: ٢");
        // الشبكة مغلقة افتراضيا في الاختبار: غير متصلة.
        assert_eq!(lines[3], "الشبكة: غير متصلة");
        assert_eq!(
            lines[4],
            "مصدر الفهرس: https://raw.githubusercontent.com/RIO4140/niva-index/main/فهرس.حزمة"
        );
        // بلا معاملات.
        let o = r.feed("معلومات زيادة");
        assert!(o.output.contains("تتوقع"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_reload_settings_signals_and_names_restart_keys() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(home.join("إعدادات/niva.properties"), "المظهر=فاتح\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.last_control(), ReplControl::None);
        let o = r.feed("أعد-الإعدادات");
        assert!(o.output.contains("أُعيدت قراءة الإعدادات"), "got: {}", o.output);
        assert!(o.output.contains("إخفاء_لوحة_المفاتيح_عند_البدء"), "got: {}", o.output);
        assert!(o.output.contains("خط_مخصص"), "got: {}", o.output);
        assert!(o.output.contains("ملء_الشاشة"), "got: {}", o.output);
        assert!(o.output.contains("الترحيب يسري من الجلسة الجديدة"), "got: {}", o.output);
        assert_eq!(r.last_control(), ReplControl::SettingsReload);
        // سطر التأكيد واحد (بلا إشعارات إضافية هنا).
        assert_eq!(o.output.lines().count(), 1, "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_reset_asks_confirms_and_keeps_home() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(home.join("إعدادات/niva.properties"), "المظهر=فاتح\n").unwrap();
        std::fs::write(home.join("ملف-مستخدم.txt"), "ملفي").unwrap();
        std::fs::write(dir.join("أداة.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("session-1.txt"), "جلسة").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // الطلب: رسالة تسمّي المسح والبقاء والجلسة.
        let o = r.feed("أعد-الضبط");
        assert!(o.output.contains("سيُمسح"), "got: {}", o.output);
        assert!(o.output.contains("جلسة محفوظة"), "got: {}", o.output);
        assert!(o.output.contains("تبقى ملفاتك في البيت"), "got: {}", o.output);
        // أي سطر آخر يلغي — والسطر نفسه يُعالج عاديا.
        let o = r.feed("١ + ١");
        assert!(o.output.starts_with("أُلغي إعادة الضبط."), "got: {}", o.output);
        assert!(o.output.contains("٢"), "got: {}", o.output);
        assert!(dir.join("أداة.نيفا").exists());
        // الطلب ثم «نعم»: المسح يتم.
        let _ = r.feed("أعد-الضبط");
        let o = r.feed("نعم");
        assert!(o.output.contains("أُعيد ضبط مجلد البرنامج"), "got: {}", o.output);
        assert!(!dir.join("أداة.نيفا").exists());
        assert!(!dir.join("session-1.txt").exists());
        // البيت والإعدادات باقيان، والمشتقات أُعيد بناؤها.
        assert!(home.join("ملف-مستخدم.txt").exists());
        assert!(home.join("إعدادات/niva.properties").exists());
        assert!(dir.join("tmp").is_dir());
        assert!(dir.join(".نيفا").is_dir());
        // مجلد الجلسة عاد إلى البيت.
        assert_eq!(r.feed("مح").output.trim(), "~");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_source_checkout_skips_home_move() {
        // اجتماع 49 §63: مجلد مصدر (Cargo.toml) لا يُنقل
        // منه شيء — الحارس يمنع كارثة batch2e (ترحيل
        // شجرة المصدر كاملة إلى home/).
        let dir = shortcut_dir();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/مهم.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        assert!(!notices.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices);
        assert!(dir.join("Cargo.toml").exists());
        assert!(dir.join("src/مهم.rs").exists());
        assert!(dir.join("ملاحظة.txt").exists());
        // القفل يُنشأ بصمت رغم التخطي.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_project_dir_skips_home_move() {
        // اجتماع 46 §60: مجلد مشروع لا يُنقل — لا
        // `مصدر/` ولا `حزم/` ولا `بيان.حزمة`.
        let dir = shortcut_dir();
        std::fs::create_dir_all(dir.join("مصدر")).unwrap();
        std::fs::write(dir.join("مصدر/رئيسي.نيفا"), "اطبع \"ناجح\"\n").unwrap();
        std::fs::create_dir_all(dir.join("حزم/حزمة_صغيرة")).unwrap();
        std::fs::write(dir.join("بيان.حزمة"), "اسم: مشروعي\n").unwrap();
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        assert!(!notices.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices);
        assert!(dir.join("مصدر/رئيسي.نيفا").exists());
        assert!(dir.join("حزم/حزمة_صغيرة").is_dir());
        assert!(dir.join("بيان.حزمة").exists());
        assert!(dir.join("ملاحظة.txt").exists());
        // القفل يُنشأ بصمت رغم التخطي.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_first_run_moves_old_files_with_exclusions() {
        let dir = shortcut_dir();
        // ملفات قديمة في الجذر (قبل الفصل).
        std::fs::write(dir.join("ملاحظة.txt"), "ملاحظة").unwrap();
        std::fs::write(dir.join("قديم.نيفا"), "صدر س\nس = ١\n").unwrap();
        std::fs::write(dir.join("niva.properties"), "الترحيب=لا\n").unwrap();
        std::fs::write(dir.join("session-1.txt"), "جلسة").unwrap();
        std::fs::create_dir(dir.join("tmp")).unwrap();
        let notices = ensure_first_run(&dir).unwrap();
        // رسالة النقل تسمّي المنقول.
        assert!(notices.iter().any(|n| n.contains("ملاحظة.txt")), "got: {:?}", notices);
        assert!(dir.join("home/ملاحظة.txt").exists());
        // الاستثناءات بقيت في الجذر.
        assert!(dir.join("قديم.نيفا").exists());
        assert!(dir.join("session-1.txt").exists());
        assert!(dir.join("tmp").is_dir());
        // الإعدادات انتقلت إلى ~/إعدادات/.
        assert!(dir.join("home/إعدادات/niva.properties").exists());
        assert!(!dir.join("niva.properties").exists());
        // القفل وُجد، والتشغيل الثاني بلا نقل ولا رسالة نقل.
        assert!(dir.join(".نيفا/نقل-البيت.قفل").is_file());
        let notices2 = ensure_first_run(&dir).unwrap();
        assert!(!notices2.iter().any(|n| n.contains("نُقلت")), "got: {:?}", notices2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_move_lock_inconsistent_names_lock() {
        // البند 50: قفل موجود والبيت غائب — الخطأ يسمّي
        // القفل بصيغته القصيرة ويطلب حذفه يدويا.
        let dir = shortcut_dir();
        let home = dir.join("home");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(dir.join(".نيفا")).unwrap();
        std::fs::write(dir.join(".نيفا/نقل-البيت.قفل"), "").unwrap();
        let err = ensure_first_run(&dir).unwrap_err();
        assert!(err.contains(".نيفا/نقل-البيت.قفل"), "got: {}", err);
        assert!(err.contains("احذف الملف يدويا"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_retired_key_migration_is_pure_and_named() {
        // البند 75: الآلية خالصة — بجدول عينة، إذ لا
        // مفتاح أُعيدت تسميته بعد حتى الآن.
        let (out, notices) = migrate_retired_keys(
            "# تعليق\nمفتاح_قديم=قيمة\nمفتاح_آخر=١\n",
            &[("مفتاح_قديم", "مفتاح_جديد")],
        );
        assert_eq!(out, "# تعليق\nمفتاح_جديد=قيمة\nمفتاح_آخر=١\n");
        assert_eq!(notices, vec!["المفتاح القديم مفتاح_قديم صار مفتاح_جديد"]);
        // بلا جدول: النص كما هو بلا إشعارات.
        let (out, notices) = migrate_retired_keys("أ=١\n", &[]);
        assert_eq!(out, "أ=١\n");
        assert!(notices.is_empty());
    }

    #[test]
    fn batch2_settings_value_validation_drops_nul() {
        // البند 41: قيمة بلا بايت فارغ — القيمة غير
        // الصالحة تُسقط مع إشعار يسمّي المفتاح.
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("إعدادات")).unwrap();
        std::fs::write(
            home.join("إعدادات/niva.properties"),
            "المظهر=فاتح\nتالف=قبل\0بعد\n",
        )
        .unwrap();
        let (entries, notices) = read_settings(&dir);
        assert!(entries.iter().any(|(k, v)| k == "المظهر" && v == "فاتح"));
        assert!(!entries.iter().any(|(k, _)| k == "تالف"));
        assert!(notices.iter().any(|n| n.contains("تالف")), "got: {:?}", notices);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_startup_and_snippets_run_but_never_block() {
        let dir = shortcut_dir();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("مقتطفات")).unwrap();
        std::fs::write(home.join("بدء.نيفا"), "س = ٤٢\nاطبع \"بدء\"\n").unwrap();
        std::fs::write(home.join("مقتطفات/أ.نيفا"), "اطبع \"أ\"\n").unwrap();
        std::fs::write(home.join("مقتطفات/ب.نيفا"), "س = ١ / ٠\n").unwrap();
        std::fs::write(home.join("مقتطفات/ملاحظة.txt"), "ليس برنامجا\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let out = r.run_startup_files();
        // بدء ثم المقتطفات أبجديا؛ والملف التالف لا يمنع
        // الدخول: خطؤه يُطبع باسمه ويكمل الباقي.
        assert!(out.contains("بدء\n"), "got: {}", out);
        assert!(out.contains("أ\n"), "got: {}", out);
        assert!(out.contains("خطأ في ~/مقتطفات/ب.نيفا"), "got: {}", out);
        assert!(!out.contains("ملاحظة"), "got: {}", out);
        // تعريفات البدء بقيت في الجلسة.
        assert_eq!(r.feed("س").output, "٤٢\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch2_run_searches_program_dir_then_home() {
        let dir = run_test_dir("twoplace");
        // في المكانين معا: المثبَّت (الجذر) يفوز.
        std::fs::write(dir.join("مثال.نيفا"), "اطبع \"جذر\"\n").unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home/مثال.نيفا"), "اطبع \"بيت\"\n").unwrap();
        // في البيت وحده: يُوجد.
        std::fs::write(dir.join("home/بيتي.نيفا"), "اطبع \"بيتي\"\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("شغّل مثال").output, "جذر\n");
        assert_eq!(r.feed("شغّل بيتي").output, "بيتي\n");
        // الغائب يسمّي المكانين.
        let o = r.feed("شغّل غائب");
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        assert!(o.output.contains("$نيفا"), "got: {}", o.output);
        assert!(o.output.contains("~"), "got: {}", o.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): كل اختبار على مجلد نظيف،
    // والجلسة تبدأ في `home` (اجتماع 45 §59) فتُنشأ
    // — وملفات التوجيه النسبية تُكتب هناك، والقراءة
    // الراجعة من القرص مباشرة حيث يثبت المحتوى.
    fn redirect_dir(tag: &str) -> std::path::PathBuf {
        let dir = run_test_dir(tag);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        dir
    }

    fn redirect_file(dir: &std::path::Path, name: &str) -> String {
        std::fs::read_to_string(dir.join("home").join(name)).unwrap()
    }

    #[test]
    fn stage_redirect_write_append_read() {
        let dir = redirect_dir("redir-write");
        let mut r = Repl::with_program_dir(dir.clone());
        // `>` ينشئ ويمسح؛ و`>>` يلحق؛ و`<` يقرأ.
        assert_eq!(r.feed("اطبع \"أ\" > خ.txt").output, "");
        assert_eq!(r.last_exit_code(), 0);
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\n");
        assert_eq!(r.feed("اطبع \"ب\" > خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "ب\n");
        assert_eq!(r.feed("اطبع \"ج\" >> خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "ب\nج\n");
        // القراءة: مدخل الملف كاملا لمخرج عار.
        assert_eq!(
            r.feed("اقرأ_الإدخال() < خ.txt").output,
            "ب\nج\n\n"
        );
        assert_eq!(r.last_exit_code(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_redirect_read_write() {
        let dir = redirect_dir("redir-rw");
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("اطبع \"قديم\" > خ.txt").output, "");
        // `<>`: كتابة فوق القديم من البداية مع بقاء
        // ذيله (بالأحرف) — «قديم\n» خمسة أحرف،
        // و«جد\n» ثلاثة، فيبقى «م\n».
        assert_eq!(r.feed("اطبع \"جد\" <> خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "جد\nم\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_redirect_stderr_and_dup_order() {
        let dir = redirect_dir("redir-err");
        let mut r = Repl::with_program_dir(dir.clone());
        // خطأ وحده إلى ملف (نص الخطأ الكامل كما
        // يظهر في السطر العادي: «خطأ وقت التشغيل:
        // فشل (السطر 1، العمود 4)»).
        assert_eq!(r.feed("فشل() 2> خ.txt").output, "");
        assert_eq!(r.last_exit_code(), 1);
        assert!(redirect_file(&dir, "خ.txt").contains("فشل"));
        // الترتيب يحسم: `> ملف 2>&1` يجمع الاثنين
        // في الملف.
        assert_eq!(r.feed("فشل() > م.txt 2>&1").output, "");
        assert!(redirect_file(&dir, "م.txt").contains("فشل"));
        // والعكس يبقي الخطأ على الطرفية.
        let o = r.feed("فشل() 2>&1 > م2.txt");
        assert!(o.output.contains("فشل"), "got: {}", o.output);
        assert!(o.output.contains("خطأ وقت التشغيل"), "got: {}", o.output);
        assert_eq!(redirect_file(&dir, "م2.txt"), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_redirect_both() {
        let dir = redirect_dir("redir-both");
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("فشل() &> خ.txt").output, "");
        assert!(redirect_file(&dir, "خ.txt").contains("فشل"));
        assert_eq!(r.feed("اطبع \"أ\" &> خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\n");
        assert_eq!(r.feed("اطبع \"ب\" &>> خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\nب\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_pipe_two_three_and_read_all() {
        let dir = redirect_dir("redir-pipe");
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("اطبع \"أ\" | ادخل()").output, "أ\n");
        assert_eq!(r.feed("اطبع \"أب\" | ادخل() | ادخل()").output, "أب\n");
        // `اقرأ_الإدخال` يصرّف المدخل كله دفعة واحدة.
        assert_eq!(r.feed("اطبع \"أب\" | س = اقرأ_الإدخال()").output, "");
        assert_eq!(r.feed("طول(س)").output, "٣\n");
        // والمفسر (التشخيص) يوافق VM.
        let dir2 = redirect_dir("redir-pipe-interp");
        let mut ri = Repl::with_program_dir_interpreter(dir2.clone());
        assert_eq!(ri.feed("اطبع \"أ\" | ادخل()").output, "أ\n");
        assert_eq!(ri.feed("اطبع \"أب\" | س = اقرأ_الإدخال()").output, "");
        assert_eq!(ri.feed("طول(س)").output, "٣\n");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn stage_pipe_status() {
        let mut r = Repl::new();
        assert_eq!(r.feed("اطبع \"أ\" | ادخل()").output, "أ\n");
        assert_eq!(r.feed("رموز_الأنبوب()").output, "[٠, ٠]\n");
        // فشل الأولى يبقى في القائمة؛ والثانية تنجح
        // على مدخل فارغ فلا تطبع شيئا (القيمة الفارغة
        // لا تطبع سطرا).
        assert_eq!(
            r.feed("فشل() | ادخل()").output,
            "خطأ وقت التشغيل: فشل (السطر 1، العمود 4)\n"
        );
        assert_eq!(r.feed("رموز_الأنبوب()").output, "[١, ٠]\n");
        // والسطر العادي رمزه وحده.
        assert_eq!(r.feed("اطبع \"أ\"").output, "أ\n");
        assert_eq!(r.feed("رموز_الأنبوب()").output, "[٠]\n");
    }

    #[test]
    fn stage_short_circuits_and_semicolon() {
        let mut r = Repl::new();
        // `نجاح()` تعبير عار فيطبع قيمته (`صح`) كما
        // يطبع أي تعبير عار — والدائرة القصيرة بعده
        // تعمل على رمزه.
        assert_eq!(r.feed("نجاح() && اطبع \"نعم\"").output, "صحيح\nنعم\n");
        assert_eq!(r.last_exit_code(), 0);
        // `فشل` يتخطى ما بعد `&&`، وخطأه وحده يظهر
        // (بنصه الكامل).
        assert_eq!(
            r.feed("فشل() && اطبع \"لا\"").output,
            "خطأ وقت التشغيل: فشل (السطر 1، العمود 4)\n"
        );
        assert_eq!(r.last_exit_code(), 1);
        assert_eq!(
            r.feed("فشل() || اطبع \"نعم\"").output,
            "خطأ وقت التشغيل: فشل (السطر 1، العمود 4)\nنعم\n"
        );
        assert_eq!(r.last_exit_code(), 0);
        assert_eq!(r.feed("نجاح() || اطبع \"لا\"").output, "صحيح\n");
        // `؛` فاصل جمل قائم (بلا بناء جديد).
        assert_eq!(r.feed("اطبع ١؛ اطبع ٢").output, "١\n٢\n");
    }

    #[test]
    fn stage_tee_builtin() {
        let dir = redirect_dir("redir-tee");
        let mut r = Repl::with_program_dir(dir.clone());
        // يمرّر المدخل كما هو، ويكتبه في كل ملف.
        assert_eq!(
            r.feed("اطبع \"أ\" | وزع(\"خ.txt\")").output,
            "أ\n"
        );
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\n");
        assert_eq!(
            r.feed("اطبع \"ب\" | وزع(\"أ.txt\"، \"ب.txt\")").output,
            "ب\n"
        );
        assert_eq!(redirect_file(&dir, "أ.txt"), "ب\n");
        assert_eq!(redirect_file(&dir, "ب.txt"), "ب\n");
        // `"ألحق"` في الآخر يلحق بدلا من المسح.
        assert_eq!(
            r.feed("اطبع \"ج\" | وزع(\"خ.txt\"، \"ألحق\")").output,
            "ج\n"
        );
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\nج\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_heredoc_and_here_string() {
        let mut r = Repl::new();
        // مستند هنا: أسطر الجسم بيانات حتى المحدد.
        assert_eq!(r.feed("ادخل() << نهاية").output, "");
        assert_eq!(r.feed("أول سطر").output, "");
        assert_eq!(r.feed("نهاية").output, "أول سطر\n");
        // `<<-` يحذف التبويبات البادئة من الجسم
        // ومن سطر المحدد نفسه.
        assert_eq!(r.feed("ادخل() <<- نهاية").output, "");
        assert_eq!(r.feed("\tمسحوب").output, "");
        assert_eq!(r.feed("\tنهاية").output, "مسحوب\n");
        // سلسلة هنا: نصها + `\n` مدخلا في السطر نفسه.
        assert_eq!(r.feed("ادخل() <<< \"نص هنا\"").output, "نص هنا\n");
    }

    #[test]
    fn stage_noclobber_setting() {
        let dir = redirect_dir("redir-noclobber");
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("اطبع \"قديم\" > خ.txt").output, "");
        assert_eq!(r.feed("منع_المسح").output, "منع_المسح: خطأ\n");
        assert_eq!(r.feed("منع_المسح صح").output, "منع_المسح: صح\n");
        // `>` على ملف موجود مرفوضة، والملف باق كما هو.
        let o = r.feed("اطبع \"جديد\" > خ.txt");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("منع_المسح"), "got: {}", o.output);
        assert_eq!(redirect_file(&dir, "خ.txt"), "قديم\n");
        // `>|` قوة وحدها، و`>>` بلا حماية.
        assert_eq!(r.feed("اطبع \"جديد\" >| خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "جديد\n");
        assert_eq!(r.feed("اطبع \"ألحق\" >> خ.txt").output, "");
        assert_eq!(redirect_file(&dir, "خ.txt"), "جديد\nألحق\n");
        // والقيمة تبقى عبر جلسة جديدة على المجلد نفسه.
        let mut r2 = Repl::with_program_dir(dir.clone());
        assert_eq!(r2.feed("منع_المسح").output, "منع_المسح: صح\n");
        // والقيمة السيئة مرفوضة برمز 2.
        assert_eq!(r2.feed("منع_المسح ربما").output, "منع_المسح تتوقع صح أو خطأ\n");
        assert_eq!(r2.last_exit_code(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_stop_on_error_setting() {
        let mut r = Repl::new();
        assert_eq!(
            r.feed("توقف_عند_الخطأ").output,
            "توقف_عند_الخطأ: خطأ\n"
        );
        assert_eq!(
            r.feed("توقف_عند_الخطأ صح").output,
            "توقف_عند_الخطأ: صح\n"
        );
        // النجاح لا ينهي؛ والفشل ينهي برمزه.
        let o = r.feed("اطبع \"أ\"");
        assert_eq!(o.output, "أ\n");
        assert!(!o.ended);
        let o = r.feed("فشل()");
        assert_eq!(
            o.output,
            "خطأ وقت التشغيل: فشل (السطر 1، العمود 4)\n"
        );
        assert!(o.ended);
        assert_eq!(r.last_exit_code(), 1);
    }

    #[test]
    fn stage_stop_on_error_ignores_caught_error() {
        let mut r = Repl::new();
        assert_eq!(r.feed("توقف_عند_الخطأ صح").output, "توقف_عند_الخطأ: صح\n");
        // خطأ ممسوك بـ`حاول/ماعدا` ليس فشل سطر.
        assert_eq!(r.feed("حاول:").output, "");
        assert_eq!(r.feed("    فشل()").output, "");
        assert_eq!(r.feed("ماعدا:").output, "");
        assert_eq!(r.feed("    اطبع \"ممسوك\"").output, "");
        let o = r.feed("س = ١");
        assert_eq!(o.output, "ممسوك\n");
        assert!(!o.ended);
        assert_eq!(r.last_exit_code(), 0);
        assert_eq!(r.feed("س").output, "١\n");
    }

    #[test]
    fn stage_redirect_error_fails_before_execution() {
        let dir = redirect_dir("redir-prefail");
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("س = ٧").output, "");
        // ملف القراءة الغائب يفشل السطر كله قبل
        // التنفيذ — فالإسناد لا يحدث.
        let o = r.feed("س = ٩ < غائب.txt");
        assert_eq!(r.last_exit_code(), 1);
        assert!(!o.output.is_empty(), "got empty error");
        assert_eq!(r.feed("س").output, "٧\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_comparisons_unchanged() {
        let mut r = Repl::new();
        // `>` و`<` مع عدد أو كلمة عارية مقارنة لا توجيه.
        assert_eq!(r.feed("س = ٥ > ٣").output, "");
        assert_eq!(r.feed("س").output, "صحيح\n");
        assert_eq!(r.feed("٥ > ٣").output, "صحيح\n");
        assert_eq!(r.feed("٥ < ٩").output, "صحيح\n");
    }

    #[test]
    fn stage_run_command_in_pipe_and_history_once() {
        let dir = redirect_dir("redir-run");
        std::fs::write(dir.join("مثال.نيفا"), "اطبع \"من-الملف\"\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("شغّل مثال | ادخل()").output, "من-الملف\n");
        // سطر الأنبوب مدخل واحد في المحفوظات لا مدخلين.
        let count = r
            .history()
            .texts()
            .iter()
            .filter(|t| *t == "شغّل مثال | ادخل()")
            .count();
        assert_eq!(count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_invert_before_redirect_and_dev_names() {
        let dir = redirect_dir("redir-invert-dev");
        let mut r = Repl::with_program_dir(dir.clone());
        // `عكس` قبل سطر موجَّه: التوجيه يحدث، والرمز
        // يُقلب (0 يصير 1).
        assert_eq!(r.feed("عكس اطبع \"أ\" > خ.txt").output, "");
        assert_eq!(r.last_exit_code(), 1);
        assert_eq!(redirect_file(&dir, "خ.txt"), "أ\n");
        // `/dev/null`: كتابة تُهمل، وقراءة نهاية فورا
        // (فـ`ادخل` عندها قيمة فارغة لا تطبع سطرا).
        assert_eq!(r.feed("اطبع \"أ\" > /dev/null").output, "");
        assert_eq!(r.last_exit_code(), 0);
        assert_eq!(r.feed("ادخل() < /dev/null").output, "");
        // `/dev/tcp` شبكة عبر التوجيه مرفوضة (رمز 1).
        let o = r.feed("اطبع \"أ\" > /dev/tcp/127.0.0.1/80");
        assert_eq!(r.last_exit_code(), 1);
        assert!(!o.output.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_rejected_operators() {
        let mut r = Repl::new();
        // واصف فوق 2 مرفوض.
        let o = r.feed("اطبع \"أ\" 3> خ.txt");
        assert_eq!(r.last_exit_code(), 2);
        assert!(o.output.contains("واصف غير معروف"), "got: {}", o.output);
        // `&` وحده خلفية مؤجلة لاجتماع المهام.
        let o = r.feed("اطبع \"أ\" &");
        assert_eq!(r.last_exit_code(), 2);
        assert!(o.output.contains("الخلفية"), "got: {}", o.output);
    }

    // المرحلة (هـ) — التحكم بالمهام (اجتماعا 64 و65
    // §79 و§80): اختبارات REPL للخلفية والعبارات.
    // القفل العام للعمليات + إعادة ضبط الجدول في كل
    // اختبار (فالجدول عملية-واسع والاختبارات تتشاركه)؛
    // وسياسة موثوقة على مجلد مؤقت كاختبارات processes.

    fn jobs_test_dir(tag: &str) -> std::path::PathBuf {
        let dir = redirect_dir(tag);
        crate::processes::set_process_policy(dir.clone(), true, dir.clone());
        dir
    }

    fn jobs_test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::processes::PROC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// تدفق خلفية كامل على محرك واحد (VM أو مفسر):
    /// إنشاء نوم، وقوائم وتصفية، وتأكيد أخر، وقتل،
    /// ونسيان، ومواصفات غائبة.
    #[cfg(unix)]
    fn jobs_background_flow(r: &mut Repl) {
        // جلسة موثوقة (--اسمح-بالكل): /bin خارج مجلد
        // البرنامج فيرفضه فحص الملفات بلا ثقة.
        r.set_trusted(true);
        // `عمليات` وحدة تُستورد أولا (كالأمامي: بلا
        // استيراد «متغير غير معرف: عمليات»).
        let o = r.feed("استورد عمليات");
        assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
        let o = r.feed("عمليات.أنشئ \"/bin/sleep 30\" &");
        assert_eq!(r.last_exit_code(), 0);
        assert!(o.output.starts_with("[١] "), "got: {}", o.output);
        let o = r.feed("وظائف");
        assert!(o.output.contains("[١]+"), "got: {}", o.output);
        assert!(o.output.contains("تعمل"), "got: {}", o.output);
        assert!(r.feed("وظائف جارية").output.contains("تعمل"));
        assert_eq!(r.feed("وظائف منتهية").output, "");
        // أخر على جارية: تأكيد بالسطر نفسه.
        let o = r.feed("أخر %1");
        assert_eq!(r.last_exit_code(), 0);
        assert!(o.output.contains("[١]+"), "got: {}", o.output);
        assert!(o.output.contains("&"), "got: {}", o.output);
        // المواصفة بالاسم الجزئي (في نص الأمر نفسه).
        let o = r.feed("أخر %sleep");
        assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
        // اقتل: تنهيها؛ ووظائف تراها موقوفة.
        let o = r.feed("اقتل %1");
        assert_eq!(r.last_exit_code(), 0);
        let o = r.feed("وظائف");
        assert!(o.output.contains("أُوقفت"), "got: {}", o.output);
        // قدم على موقوفة: خطأ؛ واقتل عليها: خطأ.
        let o = r.feed("قدم %1");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("الوظيفة أُوقفت"), "got: {}", o.output);
        let o = r.feed("اقتل %1");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("الوظيفة أُوقفت"), "got: {}", o.output);
        // أخر على موقوفة: خطأ الاستئناف.
        let o = r.feed("أخر %1");
        assert!(o.output.contains("لا وظيفة موقوفة لتستأنف"), "got: {}", o.output);
        // أبعد: تنسى؛ ووظائف فارغة.
        assert_eq!(r.feed("أبعد %1").output, "");
        assert_eq!(r.feed("وظائف").output, "");
        // مواصفة غائبة، وبلا وظائف: قدم بلا حالية.
        let o = r.feed("قدم %99");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("وظيفة غير معروفة"), "got: {}", o.output);
        let o = r.feed("قدم");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("لا وظيفة حالية"), "got: {}", o.output);
    }

    #[test]
    fn jobs_background_flow_both_engines() {
        #[cfg(unix)]
        {
            for interp in [false, true] {
                let _guard = jobs_test_lock();
                crate::processes::reset_jobs_for_test();
                let tag = if interp { "jobs-bg-interp" } else { "jobs-bg-vm" };
                let dir = jobs_test_dir(tag);
                let mut r = if interp {
                    Repl::with_program_dir_interpreter(dir.clone())
                } else {
                    Repl::with_program_dir(dir.clone())
                };
                jobs_background_flow(&mut r);
                let _ = std::fs::remove_dir_all(&dir);
                crate::processes::reset_jobs_for_test();
            }
        }
    }

    /// true في الخلفية: انتظار أي وقدم وآخر_خلفية
    /// وحية ووظائف() — على المحركين.
    #[test]
    fn jobs_background_true_wait_and_functions() {
        #[cfg(unix)]
        {
            for interp in [false, true] {
                let _guard = jobs_test_lock();
                crate::processes::reset_jobs_for_test();
                let tag = if interp {
                    "jobs-true-interp"
                } else {
                    "jobs-true-vm"
                };
                let dir = jobs_test_dir(tag);
                let mut r = if interp {
                    Repl::with_program_dir_interpreter(dir.clone())
                } else {
                    Repl::with_program_dir(dir.clone())
                };
                // جلسة موثوقة (--اسمح-بالكل): /bin خارج
                // مجلد البرنامج فيرفضه فحص الملفات بلا ثقة.
                r.set_trusted(true);
                let o = r.feed("استورد عمليات");
                assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
                // برنامج ينتهي فورا برمز ٠: /bin/sleep 0 —
                // فـ/bin/true غائب عن عدّاء macOS 26
                // بينما /usr/bin/true موجود (ipc.rs:
                // recv_eof_after_child_exit يوثق ذلك)،
                // وsleep 0 موجود على يونكس كله وينتهي
                // فورا كما true تماما.
                let o = r.feed("عمليات.أنشئ \"/bin/sleep 0\" &");
                assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
                assert!(o.output.starts_with("[١] "), "got: {}", o.output);
                // انتظر أي: يرجع رمز أول منتهية ويطبع
                // رقمها (والإشعار قد يسبقه في المخرج
                // نفسه إن انتهت قبل جمع المعالج — فلا
                // تأكيد عليه هنا؛ والإشعار مرة واحدة
                // يثبته اختبار `jobs_poll_notifies_once`).
                let o = r.feed("انتظر أي");
                assert_eq!(r.last_exit_code(), 0);
                assert!(o.output.ends_with("١\n"), "got: {}", o.output);
                // قدم على المنتهية: رمزها فورا بلا انتظار.
                let o = r.feed("قدم %1");
                assert_eq!(r.last_exit_code(), 0);
                assert_eq!(o.output, "");
                // عمليات.وظائف(): قائمة قواميس — المنتهية
                // برمز ٠ (استدعاء مباشر بلا حاول: يثبت
                // حل الاسم في الجدول).
                let o = r.feed("عمليات.وظائف()");
                assert!(o.output.contains("انتهت"), "got: {}", o.output);
                assert!(o.output.contains("٠"), "got: {}", o.output);
                // عمليات.آخر_خلفية(): معرف موجب.
                let o = r.feed("عمليات.آخر_خلفية()");
                let id = parse_shortcut_number(o.output.trim());
                assert!(matches!(id, Some(v) if v > 0.0), "got: {}", o.output);
                // عمليات.حية(معرف): المنتهية خطأ،
                // والغائبة خطأ.
                let o = r.feed(&format!("عمليات.حية({})", id.unwrap() as u64));
                assert_eq!(o.output, "خطأ\n", "got: {}", o.output);
                let o = r.feed("عمليات.حية(٩٩٩٩٩٩)");
                assert_eq!(o.output, "خطأ\n", "got: {}", o.output);
                // عمليات.انتظر_الكل(): ٠ بلا جارية.
                let o = r.feed("عمليات.انتظر_الكل()");
                assert_eq!(o.output, "٠\n", "got: {}", o.output);
                let _ = std::fs::remove_dir_all(&dir);
                crate::processes::reset_jobs_for_test();
            }
        }
    }

    /// الخلفية غير الصالحة والأخطاء — بلا إنشاء:
    /// كود عادٍ، وأنبوب، وتوجيه، وشغل غائب (127)،
    /// وانتظر بلا وظائف، ومعامل تصفية غريب.
    #[test]
    fn jobs_background_invalid_and_errors() {
        let _guard = jobs_test_lock();
        crate::processes::reset_jobs_for_test();
        let dir = jobs_test_dir("jobs-bg-invalid");
        let mut r = Repl::with_program_dir(dir.clone());
        // كود عادٍ بخلفية: خطأ 2 بالرسالة نفسها.
        let o = r.feed("اطبع \"أ\" &");
        assert_eq!(r.last_exit_code(), 2);
        assert!(
            o.output.contains("الخلفية لعمليات النظام وشغل فقط"),
            "got: {}",
            o.output
        );
        // أنبوب خلفي: خطأ 2.
        let o = r.feed("عمليات.أنشئ \"/bin/true\" | عد_الكلمات &");
        assert_eq!(r.last_exit_code(), 2);
        assert!(
            o.output.contains("الخلفية لعمليات النظام وشغل فقط"),
            "got: {}",
            o.output
        );
        // توجيه خلفي: خطأ 2 — وملف التوجيه لا يُنشأ
        // (فالتحقق قبل تجهيز الملفات).
        let o = r.feed("عمليات.أنشئ \"/bin/true\" > خ.txt &");
        assert_eq!(r.last_exit_code(), 2);
        assert!(!dir.join("خ.txt").exists());
        // شغل غائب بخلفية: 127 كالأمامي.
        let o = r.feed("شغل غائب &");
        assert_eq!(r.last_exit_code(), 127);
        assert!(o.output.contains("غير موجود"), "got: {}", o.output);
        // وظائف بلا وظائف: فارغة ورمز 0.
        assert_eq!(r.feed("وظائف").output, "");
        assert_eq!(r.last_exit_code(), 0);
        // انتظر بلا معامل = الكل: 0.
        assert_eq!(r.feed("انتظر").output, "");
        assert_eq!(r.last_exit_code(), 0);
        // انتظر أي بلا جارية: خطأ.
        let o = r.feed("انتظر أي");
        assert_eq!(r.last_exit_code(), 1);
        assert!(o.output.contains("لا وظائف جارية"), "got: {}", o.output);
        // معامل تصفية غريب: خطأ 2.
        let o = r.feed("وظائف غريب");
        assert_eq!(r.last_exit_code(), 2);
        assert!(o.output.contains("وظائف تتوقع جارية أو منتهية"), "got: {}", o.output);
        // الجمع عند الموجه: بلا وظائف بلا إشعار.
        assert!(r.poll_jobs().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        crate::processes::reset_jobs_for_test();
    }

    /// الإشعار عند الموجه: true في الخلفية تنتهي،
    /// و`poll_jobs()` يعيد سطرها مرة واحدة فقط.
    #[test]
    fn jobs_poll_notifies_once() {
        #[cfg(unix)]
        {
            let _guard = jobs_test_lock();
            crate::processes::reset_jobs_for_test();
            let dir = jobs_test_dir("jobs-poll");
            let mut r = Repl::with_program_dir(dir.clone());
            // جلسة موثوقة (--اسمح-بالكل): /bin خارج
            // مجلد البرنامج فيرفضه فحص الملفات بلا ثقة.
            r.set_trusted(true);
            let o = r.feed("استورد عمليات");
            assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
            // /bin/sleep 0 لا /bin/true: true غائب عن
            // عدّاء macOS 26 (انظر الاختبار السابق).
            let o = r.feed("عمليات.أنشئ \"/bin/sleep 0\" &");
            assert_eq!(r.last_exit_code(), 0, "got: {}", o.output);
            // انتظر خروجها ثم اجمع: سطر واحد يحمل
            // «انتهت (رمز ٠)»، ثم الجمع التالي فراغ.
            let mut first: Vec<String> = Vec::new();
            for _ in 0..200 {
                let notes = r.poll_jobs();
                if !notes.is_empty() {
                    first = notes;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(first.len(), 1, "got: {:?}", first);
            assert!(first[0].contains("انتهت (رمز ٠)"), "got: {}", first[0]);
            assert!(r.poll_jobs().is_empty());
            // الوظيفة نفسها منتهية برمز ٠ في القائمة.
            let o = r.feed("وظائف");
            assert!(o.output.contains("انتهت (رمز ٠)"), "got: {}", o.output);
            let _ = std::fs::remove_dir_all(&dir);
            crate::processes::reset_jobs_for_test();
        }
    }

    // المرحلة (د) — الإكمال (اجتماع 67 §82): طبقة
    // المستعارات — التعريف والقائمة والحذف والتوسيع
    // مرة واحدة، والمسافة الأخيرة توسّع الكلمة التالية،
    // وحارس الذاتية يمنع الدوران.
    #[test]
    fn stage_completion_aliases() {
        let mut r = Repl::new();
        assert_eq!(r.feed("مستعار س اطبع ٧").output, "مستعار س اطبع ٧\n");
        assert_eq!(r.feed("س").output, "٧\n");
        assert_eq!(r.feed("مستعارات").output, "مستعار س اطبع ٧\n");
        assert_eq!(
            r.feed("مستعار مستعار اطبع ١").output,
            "الاسم محجوز ولا يصلح مستعارا: مستعار\n"
        );
        assert_eq!(r.feed("احذف_مستعار س").output, "حُذف المستعار: س\n");
        assert_eq!(r.feed("مستعارات").output, "لا توجد مستعارات\n");
        assert_eq!(
            r.feed("احذف_مستعار س").output,
            "مستعار غير موجود: س\n"
        );
        // المسافة الأخيرة: الكلمة التالية تتوسع أيضا.
        assert_eq!(r.feed("مستعار خمسة ٥").output, "مستعار خمسة ٥\n");
        assert_eq!(r.feed("مستعار ص اطبع ").output, "مستعار ص اطبع\n");
        assert_eq!(r.feed("ص خمسة").output, "٥\n");
        assert_eq!(
            r.feed("مستعارات").output,
            "مستعار خمسة ٥\nمستعار ص اطبع \n"
        );
        // حارس الذاتية: لا دوران، بل خطأ لا تعليق.
        assert_eq!(r.feed("مستعار د د").output, "مستعار د د\n");
        assert!(is_failed(r.eval("د")));
        assert!(r.feed("د").output.contains("متغير غير معرف"));
    }

    // المرحلة (د) — الإكمال (اجتماع 67 §82): الإكمال —
    // دوال مدمجة واختصارات ودوال الجلسة وملفات المجلد
    // الحالي؛ والغائب بلا اقتراحات.
    #[test]
    fn stage_completion_complete() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-repl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("أ.txt"), "نص").unwrap();
        std::fs::create_dir_all(dir.join("home").join("مجلد")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        // «اطبع» كلمة مفتاحية لا اسم قابل للإكمال —
        // فلا اقتراح لها (نطاق الاجتماع 67 §82).
        assert!(r.complete("اط").is_empty());
        let c = r.complete("اع");
        assert!(c.contains(&"اعرض_الملفات".to_string()), "got: {:?}", c);
        assert!(c.contains(&"اعرض_المتغيرات".to_string()), "got: {:?}", c);
        assert!(c.contains(&"اعرض".to_string()), "got: {:?}", c);
        let c = r.complete("بيئة_أ");
        assert_eq!(c, vec!["بيئة_أو".to_string()], "got: {:?}", c);
        let c = r.complete("فها");
        assert_eq!(c, vec!["فهارس".to_string()], "got: {:?}", c);
        let c = r.complete("مست");
        assert!(c.contains(&"مستعار".to_string()), "got: {:?}", c);
        assert!(c.contains(&"مستعارات".to_string()), "got: {:?}", c);
        let c = r.complete("ات");
        assert!(c.contains(&"ات".to_string()), "got: {:?}", c);
        assert!(r.feed("أكمل اع").output.contains("اعرض_الملفات"));
        assert_eq!(r.feed("أكمل ززز").output, "لا توجد اقتراحات\n");
        assert_eq!(done_out(r.eval("دالة جمع(أ، ب):\n    ارجع أ + ب\n")), "");
        let c = r.complete("جم");
        assert_eq!(c, vec!["جمد".to_string(), "جمع".to_string()], "got: {:?}", c);
        let c = r.complete("أ.");
        assert_eq!(c, vec!["أ.txt".to_string()], "got: {:?}", c);
        let c = r.complete("مجل");
        assert_eq!(c, vec!["مجلد/".to_string()], "got: {:?}", c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): خيارات —
    // قراءة وضبط ومفتاح مجهول؛ والضبط يحفظ ويقرأ من
    // الجلسة الجديدة.
    #[test]
    fn stage_completion_options() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-opt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(r.feed("خيارات").output, "لا توجد إعدادات\n");
        assert_eq!(
            r.feed("خيارات الترحيب").output,
            "المفتاح غير مضبوط: الترحيب\n"
        );
        assert_eq!(
            r.feed("خيارات مفتاح_وهمي").output,
            "مفتاح غير معروف: مفتاح_وهمي\n"
        );
        assert_eq!(
            r.feed("خيارات الترحيب مرحبا").output,
            "الترحيب=مرحبا\nيسري من الجلسة الجديدة\n"
        );
        assert_eq!(r.feed("خيارات الترحيب").output, "الترحيب=مرحبا\n");
        assert_eq!(
            r.feed("خيارات حد_المحفوظات ١٠٠").output,
            "حد_المحفوظات=١٠٠\n"
        );
        assert!(r
            .feed("خيارات حد_المحفوظات سالب")
            .output
            .contains("تتوقع عددا صحيحا غير سالب"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): ملف
    // الخروج — يعمل عند :خروج مرة واحدة فقط، وعند
    // EOF كذلك؛ وغيابه صمت.
    #[test]
    fn stage_completion_exit_file() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("خروج.نيفا"), "اطبع \"وداعا\"\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let o = r.feed(":خروج");
        assert!(o.ended, "expected ended");
        assert!(o.output.contains("وداعا"), "got: {}", o.output);
        let o2 = r.eof();
        assert!(!o2.output.contains("وداعا"), "got: {}", o2.output);
        let mut r2 = Repl::with_program_dir(dir.clone());
        let _ = std::fs::remove_file(dir.join("home").join("خروج.نيفا"));
        let o3 = r2.eof();
        assert!(o3.ended);
        assert!(!o3.output.contains("وداعا"), "got: {}", o3.output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): علما
    // البدء — --بلا-بدء يتخطى بدء.نيفا والمقتطفات،
    // و--ملف-بدء يضيف ملفه بعدهما ولو مع التخطي.
    #[test]
    fn stage_completion_startup_flags() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home").join("مقتطفات")).unwrap();
        std::fs::write(dir.join("home").join("بدء.نيفا"), "س = ٧\n").unwrap();
        std::fs::write(
            dir.join("home").join("مقتطفات").join("أ.نيفا"),
            "ص = ٨\n",
        )
        .unwrap();
        std::fs::write(dir.join("إضافي.نيفا"), "ع = ٩\n").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        let _ = r.run_startup_files();
        assert_eq!(done_out(r.eval("س")), "٧\n");
        assert_eq!(done_out(r.eval("ص")), "٨\n");
        let mut r2 = Repl::with_program_dir(dir.clone());
        r2.set_skip_startup(true);
        r2.set_startup_file(std::path::PathBuf::from("إضافي.نيفا"));
        let _ = r2.run_startup_files();
        assert_eq!(done_out(r2.eval("ع")), "٩\n");
        assert!(is_failed(r2.eval("س")));
        let mut r3 = Repl::with_program_dir(dir.clone());
        r3.set_skip_startup(true);
        let _ = r3.run_startup_files();
        assert!(is_failed(r3.eval("س")));
        assert!(is_failed(r3.eval("ص")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماعا 66–69 §81–§84):
    // الدوال الجديدة تمر عبر المحلل في واجهة الطرفية
    // نفسها (اختبار غير باهت: بلا حاول، واسم غير
    // مسجل في semantic.rs يفشل هنا).
    #[test]
    fn stage_completion_new_builtins_resolve_in_repl() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-repl2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(
            dir.join("home").join("تعريفات.نيفا"),
            "س = ٤٢\nدالة ضاعف(ن):\n    ارجع ن * ٢\n",
        )
        .unwrap();
        std::fs::write(dir.join("home").join("أ.txt"), "نص").unwrap();
        let mut r = Repl::with_program_dir(dir.clone());
        assert_eq!(
            r.feed("اطبع بيئة_أو(\"اسم_غير_موجود_تماماً\"، \"بديل\")").output,
            "بديل\n"
        );
        assert_eq!(
            r.feed("اطبع \"أبجد\".اقطع(١، ٢)").output,
            "بج\n"
        );
        assert_eq!(r.feed("اطبع فهارس([١، ٢])").output, "[٠, ١]\n");
        assert_eq!(
            r.feed("اطبع أزل_لاحقة(\"تقرير.txt\"، \".txt\")").output,
            "تقرير\n"
        );
        assert_eq!(r.feed("مصدر(\"تعريفات.نيفا\")").output, "لاشيء\n");
        assert_eq!(done_out(r.eval("س")), "٤٢\n");
        assert_eq!(done_out(r.eval("ضاعف(٣)")), "٦\n");
        assert_eq!(
            r.feed("اطبع اعرض_الملفات()").output,
            "[\"أ.txt\", \"تعريفات.نيفا\"]\n"
        );
        assert_eq!(r.feed("ات").output, "أ.txt\nتعريفات.نيفا\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
