//! Phase ك-2 (Meeting 25, task 7): the VM is now the DEFAULT
//! execution engine. The tree-walking interpreter remains only
//! as a hidden diagnostic/reference mode (`--مفسر`, never
//! documented as a way to run programs); the VM runs every
//! program and every REPL session by default.
//!
//! Design:
//! - Compiler: AST Stmt/Expr -> Vec<Instr>, with jump patching
//!   for إذا / بينما, and short-circuit jumps for و / أو.
//! - VM: stack machine, pc, variables map, output string.
//! - Supported: numbers, strings, booleans, لاشيء,
//!   variables, arithmetic, comparisons, و / أو (short-circuit),
//!   ليس, سالب, تعيين متغير, اطبع, إذا / وإلا إذا / وإلا,
//!   بينما, جملة تعبير (with Pop), and (slice 2) functions:
//!   top-level and nested دالة definitions (global table, no
//!   closures), direct calls by name, ارجع, recursion with
//!   the same depth limit as the interpreter (Meeting 15).
//!   Slice 3 adds: list/dict literals, index read (list,
//!   dict, string char), and index assignment on a plain
//!   variable (قائمة[١] = ٩٩, قاموس["مفتاح"] = قيمة).
//!   Lists/dicts are value types (cloned on assignment),
//!   exactly like the interpreter: ب = قائمة copies it,
//!   so mutating ب does not affect قائمة; only
//!   قائمة[١] = ... mutates the variable's own slot.
//!   Slice 4 adds: the 9 pure builtins (طول، مدى، مجموع،
//!   أدنى، أقصى، مطلق، عدد، نص، نوع), with the same
//!   messages and arity errors as the interpreter.
//!   Resolution matches the interpreter exactly: a
//!   user function of the same name wins (checked at
//!   compile time), and a variable of the same name
//!   wins at runtime (Op::Builtin checks the environment first
//!   and errors "القيمة {} ليست دالة", like the
//!   interpreter); only then does the builtin run.
//!   Slice 5 adds: full classes (فئة، أنشئ، نفس،
//!   حقول، طرق، وراثة، واجهات). Methods are compiled
//!   to bytecode and flattened at compile time (base
//!   methods copied, child overrides win) for the
//!   interface check; at runtime each method is a
//!   Function value on the class value itself (like
//!   the interpreter), so lookup walks the base chain
//!   and a method sees its defining scope. Base/
//!   interface checks run at class-definition time,
//!   like the interpreter. نفس is bound in the
//!   method's child VM. A field shadowing a method
//!   name wins, like the interpreter. BoundMethod
//!   values can be stored in a variable and called.
//!   Task 3 (phase ك-1) adds: methods capture their
//!   defining scope chain (an inherited method keeps
//!   its base's chain); builtin string/list methods
//!   (طول، كبير، صغير، قص، استبدل، قسم، يحتوي،
//!   أضف، أزل) via a dot-call op that intercepts the
//!   call before evaluating the dot, like the
//!   interpreter; and return-type checks on every
//!   VM call path, with the interpreter's messages.
//!   Slice 6 (phase ك-1, Meeting 24) adds: lexical
//!   closures and lambdas. Every دالة definition (top
//!   level or nested) and every دالة_مجهولة becomes a
//!   first-class Function value that captures its
//!   defining scope chain (shared Rc scopes, exactly
//!   like the interpreter's Meeting 11 semantics), so
//!   a returned closure still sees and mutates its
//!   defining environment, two closures from one
//!   factory call share state, and two from different
//!   calls do not. The VM's variables live in a scope
//!   chain (not one flat map), and assignment is
//!   lexical (nearest scope that already binds the
//!   name, else the innermost scope). Calls resolve
//!   environment-first: a closure value in scope wins
//!   over the compiled table, like the interpreter.
//!   Task 4 (phase ك-1) adds: modules and the stdlib in
//!   the VM. `استورد` / `صدّر` / dot access work like the
//!   interpreter: native modules first (never shadowed by
//!   a file), then a `.نيفا` file in the program
//!   directory, compiled and executed once per run, with
//!   cycle detection and export-only access. A loaded
//!   module's top-level functions and methods carry their
//!   module name, so each call pushes a fresh clone of
//!   the module scope — exactly like the interpreter, so
//!   a module function mutating a module global does NOT
//!   persist across calls (verified ١ ١ ٠). The four
//!   native modules (رياضيات، عشوائي، وقت، شبكة)
//!   are values like any other: شبكة.اجلب calls the
//!   same crate::network::fetch, so the process-wide
//!   gate (--اسمح-بالشبكة) and all limits (HTTP only,
//!   30s, 5 MB, no private/loopback) apply identically.
//! - NOT supported yet (compile error, clear message):
//!   complex index-assignment targets (قائمة[٠][١] = ...).
//!   Those remain tree-walking only for now.
//!   Task 5 (k-1.4) closed the file builtins +
//!   ادخل gap: they run in the VM, mirroring the
//!   interpreter exactly (same FilePolicy, same caps,
//!   same messages).
//! - Known VM gaps (documented, not new to a slice):
//!   A variable assigned AFTER a function definition,
//!   then called, runs the function in the VM but errors
//!   in the interpreter ONLY when the call happens before
//!   the definition statement has executed (forward
//!   call): after the definition has executed, the
//!   environment value wins and the VM matches the
//!   interpreter. A stored bound method (م = ص.قيمة;
//!   م()) runs in the VM but errors in the interpreter
//!   (its Ident-call path has no BoundMethod arm; the
//!   generic path does). The VM keeps its documented
//!   behavior; the interpreter fix is a language
//!   decision for a later task (مهمة 3). (مهمة 6،
//!   بوابة التكافؤ ك-1.5) رسائل شروط إذا/و/أو/ليس
//!   في VM تطابق المفسر الآن (CheckBool + رسالة ليس
//!   الخاصة)؛ وما زال اكسر داخل دالة يبلَّغ بموضع
//!   مختلف (الرسالة نفسها؛ والمحلل الدلالي يحسمه
//!   على مسار CLI أولا).
//! - GC: none beyond Rust ownership + Rc for values
//!   already in Value. A tracing GC is a later 1B slice, after
//!   the VM heap exists. This slice does NOT claim a GC.

use crate::ast::{BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{RioError, RioResult};
use crate::navigation::TryDir;
use crate::value::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// سياق فحص منطقي — رسالة الخطأ تختلف باختلاف
/// الموضع، كما في المفسر تماما: «شرط إذا...» و«شرط
/// وإلا إذا...» و«و تتطلب منطقيا» و«أو تتطلب منطقيا»
/// (مهمة 6، بوابة التكافؤ ك-1.5).
#[derive(Debug, Clone, Copy)]
pub enum BoolCtx {
    If,
    Elif,
    And,
    Or,
}

impl BoolCtx {
    fn message(&self, type_name: &str) -> String {
        match self {
            BoolCtx::If => format!("شرط إذا يجب أن يكون منطقيا، وليس {}", type_name),
            BoolCtx::Elif => format!("شرط وإلا إذا يجب أن يكون منطقيا، وليس {}", type_name),
            BoolCtx::And => format!("و تتطلب منطقيا، وليس {}", type_name),
            BoolCtx::Or => format!("أو تتطلب منطقيا، وليس {}", type_name),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Op {
    Const(Value),
    Load(String),
    Store(String),
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`
    // (إلحاق بقيمة قديمة أو هوية الغائب).
    StoreAppend(String),
    Pop,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Not,
    Neg,
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    Print(usize),
    /// استدعاء بالاسم: `callee_line/callee_col` موضع
    /// الاسم المستدعى نفسه — خطأا «استخدم أنشئ» و«القيمة
    /// ليست دالة» يبلَّغان به، كما في المفسر؛ وباقي
    /// أخطاء الاستدعاء (المعاملات والإرجاع) بموضع
    /// تعليمة الاستدعاء نفسه (موضع القوس).
    Call { name: String, nargs: usize, callee_line: usize, callee_col: usize },
    Return,
    BuildList(usize),
    BuildDict(Vec<String>),
    /// فهرسة قراءة: `idx_line/idx_col` موضع تعبير
    /// الفهرس نفسه — أخطاء الفهرس (خارج النطاق، مفتاح
    /// غائب، نوع فهرس خاطئ) تبلَّغ به، كما في المفسر؛
    /// أما خطأ «لا يمكن الفهرسة» فيبلَّغ بموضع تعليمة
    /// الفهرسة نفسها (موضع القوس المربع).
    Index { idx_line: usize, idx_col: usize },
    /// تعيين فهرسة: `idx_line/idx_col` موضع تعبير
    /// الفهرس (أخطاء الفهرس)، و`tgt_line/tgt_col`
    /// موضع المتغير الهدف (متغير غير معرف، وتعيين
    /// عنصر في نوع غير قابل للفهرسة)، كما في المفسر.
    StoreIndex { name: String, idx_line: usize, idx_col: usize, tgt_line: usize, tgt_col: usize },
    /// دالة مدمجة بالاسم: `callee_line/callee_col`
    /// موضع الاسم — خطأا «استخدم أنشئ» و«القيمة ليست
    /// دالة» حين يظلل الاسم قيمة مربوطة يبلَّغان به،
    /// كما في المفسر؛ وباقي الأخطاء بموضع التعليمة.
    Builtin { name: String, nargs: usize, callee_line: usize, callee_col: usize },
    DefineClass(VmClassDef),
    DefineInterface(String, Vec<(String, usize)>),
    Dot(String),
    StoreDot(String),
    Create(String, usize),
    /// مهمة 3 (ك-1.3): استدعاء طريقة عبر نقطة مباشرة
    /// (ك.طريقة(...)). يسحب الهدف ثم المعاملات (المعاملات
    /// تُقيَّم أولا، كما في المفسر): نص/قائمة ← طريقة
    /// مدمجة؛ كائن ← حقل أو طريقة؛ غيرهما ← خطأ الوصول
    /// بالنقطة نفسه في المفسر. `var` اسم المتغير الهدف
    /// حين يكون معرّفا عاديا، لتكتب طرق القوائم المعدِّلة
    /// (أضف/أزل) رجعيا إليه؛ وغيابه يعني هدفا غير متغير
    /// فيرفض التعديل برسالة المفسر نفسها.
    CallDotMethod { name: String, var: Option<String>, nargs: usize },
    /// Phase ك-1: create a closure value from the defining
    /// scope chain and push it (a Lambda expression).
    MakeClosure(ClosureDef),
    /// Phase ك-1: create a closure value and define it in
    /// the innermost scope (a دالة definition statement).
    DefineClosure(ClosureDef),
    /// Phase ك-1: call the value on top of the stack (any
    /// non-Ident, non-Dot callee: a variable holding a
    /// closure, a call result, a parenthesised lambda).
    /// Stack at entry: [arg1, ..., argN, callee].
    /// `callee_line/callee_col` موضع تعبير المستدعى
    /// نفسه — خطأ «القيمة ليست دالة» يبلَّغ به، كما في
    /// المفسر؛ وباقي أخطاء الاستدعاء بموضع التعليمة.
    CallValue { nargs: usize, callee_line: usize, callee_col: usize },
    /// مهمة 6 (بوابة التكافؤ ك-1.5): فحص منطقي بسياق
    /// المفسر — يسحب القيمة؛ إن كانت منطقية أعادها،
    /// وإلا أخطأ برسالة السياق (إذا/وإلا إذا/و/أو).
    /// تُصدَر قبل كل قفز شرطي، فيبقى القفز نفسه
    /// دفاعيا على pop_bool العام.
    CheckBool(BoolCtx),
    /// مهمة 6 (بوابة التكافؤ ك-1.5): فحص التعيين
    /// المعلن نوعه وقت التشغيل، كما في المفسر — ينظر
    /// إلى أعلى المكدس دون سحبه؛ فإن خالف نوع القيمة
    /// النوع المعلن أخطأ «نوع غير متطابق: المتغير ...
    /// معلن ...، لكن القيمة ...». `أي` تعني لا فحص
    /// (لا تُصدَر التعليمة أصلا).
    CheckAssignType { name: String, expected: String },
    /// مهمة 4 (ك-1.3): استيراد وحدة وقت التشغيل، كما في
    /// المفسر: الوحدات الأصلية أولا (لا يظللها ملف
    /// بنفس الاسم أبدا)، ثم ملف `.نيفا` في مجلد
    /// البرنامج (يُترجَم ويُنفَّذ مرة واحدة، مع كشف
    /// الدورات)، ثم ربط الاسم المختصر أو الأسماء
    /// المطلوبة (تصدير فقط). `صدّر` نفسه لا تعليمة
    /// له — لا-عملية وقت التشغيل، كما في المفسر.
    Import { module: String, alias: Option<String>, names: Option<Vec<String>> },
    // --- ك-1.2: التحكم في التدفق ---
    /// قفز شرطي لحلقة: يسحب الشرط؛ إن لم يكن منطقيا فخطأ
    /// «شرط الحلقة يجب أن يكون منطقيا» (رسالة المفسر نفسها)،
    /// وإن كان خطأ فاقفز إلى الهدف (تعليمة LoopEnd).
    JumpIfFalseLoop(usize),
    /// يجهّز حلقة «لكل»: يسحب التكرارية، يتحقق أنها قائمة أو
    /// نص («لا يمكن التكرار على نوع {}» وإلا)، ويدفع حالة
    /// الحلقة (عناصر مادّية = لقطة كما في المفسر).
    /// continue_pc هو التعليمة التالية مباشرة (ForNext).
    ForSetup { var: String, end_pc: usize },
    /// رأس حلقة «لكل»: إن نفدت العناصر فاقفز إلى LoopEnd؛
    /// وإلا عرّف المتغير بالعنصر التالي وتابع.
    ForNext(usize),
    /// يجهّز «كرر N مرات»: يسحب العدد، يتحقق أنه عدد صحيح
    /// غير سالب ولا يتجاوز ١٬٠٠٠٬٠٠٠ (رسائل المفسر نفسها).
    RepeatCountSetup { end_pc: usize },
    /// رأس «كرر N مرات»: إن نفد العدد فاقفز إلى LoopEnd؛
    /// وإلا أنقص واحدا وتابع.
    RepeatCountNext(usize),
    /// يجهّز حلقة «بينما»/«كرر طالما»: يدفع حالة الحلقة
    /// (عداد التكرارات 0). continue_pc هو التعليمة التالية
    /// مباشرة (بداية تقييم الشرط).
    WhileBegin { end_pc: usize },
    /// يزيد عداد تكرارات حلقة «بينما»/«كرر طالما» واحدا؛ وإن
    /// تجاوز ١٬٠٠٠٬٠٠٠ فخطأ «تم تجاوز حد التكرار، هل الحلقة
    /// لا تنتهي؟» (حد المفسر نفسه).
    WhileTick,
    /// نهاية حلقة: يسحب حالة الحلقة العليا (لا شيء إن كانت
    /// فارغة — دفاعيا).
    LoopEnd,
    /// اكسر: يفكّ معالجات حاول الداخلية أولا (يشغّل أخيرا)،
    /// ثم يقفز إلى LoopEnd الحلقة العليا.
    Break,
    /// تابع: يفكّ معالجات حاول الداخلية أولا، ثم يقفز إلى
    /// رأس الحلقة العليا.
    Continue,
    /// يبدأ منطقة حاول: يدفع معالجا (ماعدا/أخيرا/بعد).
    TryBegin { except_pc: Option<usize>, finally_pc: Option<usize>, after_pc: usize },
    /// نهاية جسم حاول أو جسم ماعدا: يسحب المعالج الأعلى،
    /// ويقفز إلى أخيرا (إن وُجد) أو إلى بعد.
    EndTryBlock { finally_pc: Option<usize>, after_pc: usize },
    /// نهاية جسم أخيرا: ينفّذ ما كان معلقا (متابعة، أو إرجاع،
    /// أو إعادة رفع خطأ، أو اكسر/تابع).
    EndFinally,
    /// «في»: يفحص العضو في قائمة أو نص أو قاموس (دلالة المفسر).
    In,
    /// «ليس في»: نفي «في».
    NotIn,
}

/// Phase ك-1: a compiled closure. `id` keys the bytecode in
/// the compiler/VM closure registry (unique per FuncDef /
/// Lambda in the program); `body` is the AST body carried in
/// the Value, exactly like the interpreter's Function value,
/// so printing and comparison match the interpreter.
#[derive(Debug, Clone)]
pub struct ClosureDef {
    pub id: u64,
    pub name: String,
    pub params: Vec<String>,
    pub param_types: Vec<Option<String>>,
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
}

/// Phase ك-1: unique closure ids, process-wide. A static
/// atomic is the simplest dependency-free source; ids never
/// appear in user-visible output, so their exact values do
/// not matter.
static NEXT_CLOSURE_ID: AtomicU64 = AtomicU64::new(1);

fn next_closure_id() -> u64 {
    NEXT_CLOSURE_ID.fetch_add(1, Ordering::Relaxed)
}

/// مهمة 8 (ك-2.2): بعد تحميل مصنوع بايت كود مبني في
/// عملية أخرى، ارفع عدّاد معرّفات الإغلاقات فوق أكبر
/// معرّف فيه، فلا يصطدم إغلاق وحدة تُترجَم وقت التشغيل
/// (load_module يدمج سجلها في السجل المشترك) بإغلاق
/// مصنوع — العدّاد يبدأ من 1 في كل عملية، فبدون هذا
/// الرفع يكتب إغلاق الوحدة فوق إغلاق المصنوع بالمعرّف
/// نفسه، ويعمل البرنامج خطأ بصمت.
pub fn reserve_closure_ids_above(max_id: u64) {
    NEXT_CLOSURE_ID.fetch_max(max_id.saturating_add(1), Ordering::Relaxed);
}

/// Slice 5: a compiled class. Methods are FLATTENED at
/// compile time: base methods copied in, child overrides
/// win, so runtime lookup is one map lookup and matches
/// the interpreter's base-chain walk (child wins).
/// مهمة 3: `own_method_ids` معرّفات بايت كود الطرق
/// الخاصة بهذه الفئة وحدها (لا الموروثة) في سجل
/// الإغلاقات — تُسجَّل وقت الترجمة، فيُبنى منها وقت
/// التشغيل قيمة دالة تحمل سلسلة النطاق المعرِّف، كما
/// في المفسر (الموروثة تأتي من قيمة الفئة الأب
/// بسلسلتها، فتبقى سلسلة الأساس للطريقة الموروثة).
#[derive(Debug, Clone)]
pub struct VmClassDef {
    pub name: String,
    pub base: Option<String>,
    pub interfaces: Vec<String>,
    pub methods: HashMap<String, FunctionDef>,
    pub own_method_ids: HashMap<String, u64>,
}

/// Slice 4: the 9 pure builtins the VM knows.
/// مهمة 5 (ك-1.4): file builtins (اقرأ_ملف، اكتب_ملف،
/// …) and ادخل — they need the file policy / cwd wired
/// into the VM (see Vm::file_policy / Vm::cwd), and run
/// through call_file_builtin, mirroring the
/// interpreter's call_builtin arms exactly.
const PURE_BUILTINS: [&str; 9] =
    ["طول", "مدى", "مجموع", "أدنى", "أقصى", "مطلق", "عدد", "نص", "نوع"];

fn is_pure_builtin(name: &str) -> bool {
    PURE_BUILTINS.contains(&name)
}

/// المرحلة (د) — الإكمال (اجتماع 67 §82): كل أسماء
/// الدوال المدمجة (النقية + الملفات) — للإكمال في
/// التفاعلي.
pub(crate) fn all_builtin_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = PURE_BUILTINS.to_vec();
    names.extend(FILE_BUILTINS.iter().copied());
    names
}

/// مهمة 5: every file builtin the interpreter knows
/// (the 11 terminal shortcuts + the 15 added for the 50
/// terminal shortcuts + تفاصيل_المسار + تثبيت + ادخل +
/// الدفعة 1: رمز_الخروج + اسم_البرنامج + خروج +
/// اجتماع 56 §71 — الدفعة 2: المكدس الستة +
/// الدفعة 3 — التشغيل (اجتماع 57 §72): وصف الأمر
/// ومكان الأداة والاستبدال). والدفعة 4 — المتغيرات
/// (اجتماع 58 §73): التجميد والحذف والفحص
/// والمرجع وعرض البيئة ومفاتيح القاموس. والدفعة 5 —
/// أدوات الملفات (اجتماع 59 §74): النسخ التكراري
/// والمحدَّث والأرشيفي والخفيف والحذف التكراري
/// والرابط الصلب والنسبي. The compiler emits
/// Op::Builtin for these, like the pure ones; the
/// runtime env-first check in Op::Builtin still applies
/// (a bound name wins over the builtin).
const FILE_BUILTINS: [&str; 70] = [
    "اقرأ_ملف", "اكتب_ملف", "اعرض_الملفات", "المجلد_الحالي", "انتقل", "أنشئ_مجلد",
    "احذف", "احذف_مجلد", "احذف_ملف", "انسخ", "انقل", "أنشئ_ملف", "اعرض_البداية",
    "اعرض_النهاية", "عد_الأسطر", "عد_الكلمات", "ابحث_في_ملف", "ابحث_عن",
    "اعرض_الشجرة", "حجم_المسار", "أنشئ_رابط", "التاريخ_الآن", "النسخة", "موجود",
    "نوع_المسار", "عد_الملفات", "تفاصيل_المسار", "تثبيت", "ادخل", "إصدار_المحرك",
    "بيئة", "رمز_الخروج", "اسم_البرنامج", "خروج",
    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): قراءة الإدخال الموجَّه، و`tee`،
    // وأمرا النجاح والفشل، وقائمة رموز الأنبوب.
    "اقرأ_الإدخال", "وزع", "نجاح", "فشل", "رموز_الأنبوب",
    "ادفع_مجلد", "أخرج_مجلد", "اعرض_المكدس", "امسح_المكدس", "مسار_بحث_المجلدات",
    "الوضع_الفيزيائي",
    "نوع_الامر", "مكان_الاداة", "استبدل_الجلسة",
    "جمد", "احذف_متغير", "افحص_متغير", "مرجع", "اعرض_البيئة", "مفاتيح",
    "انسخ_تكراري", "انسخ_محدث", "انسخ_أرشيف", "انسخ_خفيف", "احذف_تكراري",
    "أنشئ_رابط_صلب", "أنشئ_رابط_نسبي",
    // المرحلة (د) — الإكمال (اجتماعات 66–69 §81–
    // §84): بديلا البيئة، وإزالتا البادئة/اللاحقة،
    // والفهارس، وعرض المتغيرات والدوال، وحذف الدالة،
    // والمصدر. (بلا هذه القائمة يصدر المترجم Op::Call
    // فيخطئ التشغيل «دالة غير معرفة في VM» — تنبيه
    // الدرس الدائم: كل اسم جديد هنا أولا.)
    "بيئة_أو", "بيئة_إن_وجد", "أزل_بادئة", "أزل_لاحقة", "فهارس",
    "اعرض_المتغيرات", "اعرض_الدوال", "احذف_دالة", "مصدر",
];

fn is_file_builtin(name: &str) -> bool {
    FILE_BUILTINS.contains(&name)
}

#[derive(Debug, Clone)]
pub struct Instr {
    pub op: Op,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub params: Vec<String>,
    /// مهمة 3: أنواع المعاملات ونوع الإرجاع المعلنان،
    /// ليُفحصا وقت التشغيل في VM كما في المفسر (كانتا
    /// تُحفظان في ClosureDef فقط، فلا يراهما مسار
    /// الاستدعاء بالجدول ولا الطرق).
    pub param_types: Vec<Option<String>>,
    pub return_type: Option<String>,
    pub code: Vec<Instr>,
}

#[derive(Debug, Clone)]
pub struct CompiledProgram {
    pub code: Vec<Instr>,
    pub functions: HashMap<String, FunctionDef>,
    /// Phase ك-1: compiled closure bodies by id (the registry
    /// Value::Function.vm_id keys into). The name-keyed
    /// `functions` table stays as well: it answers the
    /// compile-time Call-vs-Builtin decision, and it is the
    /// fallback for calls whose name is not bound in the
    /// environment (forward calls and calls from method
    /// bodies, which have no captured chain yet).
    pub closures: HashMap<u64, FunctionDef>,
}

pub struct Compiler {
    pub code: Vec<Instr>,
    pub functions: HashMap<String, FunctionDef>,
    /// Phase ك-1: closure bodies by id, merged up from
    /// sub-compilers exactly like `functions`.
    pub closures: HashMap<u64, FunctionDef>,
    /// Compile-time class table, used only to flatten methods
    /// for inheritance. Never merged from sub-compilers: a class
    /// defined inside a function is local to that function,
    /// like in the interpreter.
    pub classes: HashMap<String, VmClassDef>,
    /// ك-1.2: عمق الحلقات وقت الترجمة في هذا المترجم.
    /// اكسر/تابع خارج حلقة خطأ ترجمة («اكسر خارج حلقة» /
    /// «تابع خارج حلقة» — رسالة المفسر نفسها). المترجمات
    /// الفرعية تبدأ من 0، فاكسر داخل جسم دالة خطأ حتى لو
    /// كانت الدالة نفسها داخل حلقة — كما في المحلل الدلالي.
    loop_depth: usize,
}

impl Compiler {
    pub fn new() -> Self {
        Self { code: Vec::new(), functions: HashMap::new(), closures: HashMap::new(), classes: HashMap::new(), loop_depth: 0 }
    }

    /// A sub-compiler for a nested body: it inherits the user
    /// function names known so far, so a closure calling an
    /// outer user function that shadows a builtin still
    /// compiles to Op::Call, not Op::Builtin. Use
    /// merge_sub afterwards to bring its tables back.
    /// ك-1.2: loop_depth يبدأ من 0 دائما — اكسر/تابع داخل
    /// جسم دالة/طريقة/لامدا خطأ ترجمة.
    fn sub_compiler(&self) -> Self {
        Self {
            code: Vec::new(),
            functions: self.functions.clone(),
            closures: HashMap::new(),
            classes: HashMap::new(),
            loop_depth: 0,
        }
    }

    /// Merge a sub-compiler's tables back into this one.
    /// (The sub was seeded with this compiler's function
    /// names, so merging them back is idempotent for old
    /// entries and adds the new ones.)
    fn merge_sub(&mut self, sub: Compiler) {
        for (n, f) in sub.functions {
            self.functions.insert(n, f);
        }
        for (id, f) in sub.closures {
            self.closures.insert(id, f);
        }
    }

    fn emit(&mut self, op: Op, line: usize, col: usize) -> usize {
        self.code.push(Instr { op, line, col });
        self.code.len() - 1
    }

    /// مهمة 6 (بوابة التكافؤ ك-1.5): يصدر فحص نوع
    /// التعيين المعلن (إن وُجد ولم يكن «أي») بعد ترجمة
    /// القيمة مباشرة، وقبل ترجمة الفهرس/الهدف — ترتيب
    /// المفسر نفسه (القيمة ثم الفحص ثم التقييم ثم
    /// التعيين). `name` اسم المتغير، أو سلسلة فارغة
    /// لغير المتغير العادي (رسالة المفسر نفسها).
    fn emit_assign_type_check(
        &mut self,
        annotation: &Option<String>,
        name: &str,
        line: usize,
        col: usize,
    ) {
        if let Some(ann) = annotation {
            if ann != "أي" {
                self.emit(
                    Op::CheckAssignType { name: name.to_string(), expected: ann.clone() },
                    line,
                    col,
                );
            }
        }
    }

    fn patch_jump(&mut self, idx: usize, target: usize) {
        match &mut self.code[idx].op {
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t)
            | Op::JumpIfFalseLoop(t) | Op::ForNext(t) | Op::RepeatCountNext(t) => *t = target,
            _ => {}
        }
    }

    /// ك-1.2: يضبط end_pc لتعليمة تجهيز حلقة
    /// (ForSetup / RepeatCountSetup / WhileBegin).
    fn patch_end(&mut self, idx: usize, end: usize) {
        match &mut self.code[idx].op {
            Op::ForSetup { end_pc, .. } | Op::RepeatCountSetup { end_pc } | Op::WhileBegin { end_pc } => {
                *end_pc = end;
            }
            _ => {}
        }
    }

    /// ك-1.2: يضبط أهداف تعليمة TryBegin بعد ترجمة الأجسام.
    fn patch_try_begin(
        &mut self,
        idx: usize,
        except_pc: Option<usize>,
        finally_pc: Option<usize>,
        after_pc: usize,
    ) {
        if let Op::TryBegin { except_pc: e, finally_pc: f, after_pc: a } =
            &mut self.code[idx].op
        {
            *e = except_pc;
            *f = finally_pc;
            *a = after_pc;
        }
    }

    /// ك-1.2: يضبط هدفي تعليمة EndTryBlock بعد ترجمة الأجسام.
    fn patch_end_try_block(&mut self, idx: usize, finally_pc: Option<usize>, after_pc: usize) {
        if let Op::EndTryBlock { finally_pc: f, after_pc: a } = &mut self.code[idx].op {
            *f = finally_pc;
            *a = after_pc;
        }
    }

    pub fn compile_program(program: &[Stmt]) -> RioResult<CompiledProgram> {
        let mut c = Self::new();
        for stmt in program {
            c.compile_stmt(stmt)?;
        }
        Ok(CompiledProgram { code: c.code, functions: c.functions, closures: c.closures })
    }

    /// ك-2 (مهمة 7): ترجمة تعبير واحد إلى برنامج يترك قيمته
    /// على المكدس (بلا Pop) — لتقييم التعبير الأخير في REPL
    /// وطباعة قيمته، كما يفعل eval_one في المفسر. الإغلاقات
    /// داخل التعبير (لامدا) تُسجَّل في سجل البرنامج، فيدمجها
    /// `Vm::load` في السجل المشترك للجلسة.
    pub fn compile_expr_program(expr: &Expr) -> RioResult<CompiledProgram> {
        let mut c = Self::new();
        c.compile_expr(expr)?;
        Ok(CompiledProgram { code: c.code, functions: c.functions, closures: c.closures })
    }

    /// ك-1.2: ترجمة حلقة «بينما» و«كرر طالما» (دلالة واحدة
    /// في المفسر: exec_while). الشكل:
    ///   WhileBegin -> cond -> JumpIfFalseLoop -> LoopEnd
    ///   WhileTick -> body -> Jump(cond) -> LoopEnd
    /// WhileTick بعد الشرط مباشرة: العداد يزيد بعد ثبوت
    /// الشرط صحيحا، وقبل الجسم — كما في المفسر تماما
    /// (فلا يتجاوز الجسم ١٬٠٠٠٬٠٠٠ تنفيذ).
    fn compile_while(&mut self, cond: &Expr, body: &[Stmt], stmt: &Stmt) -> RioResult<()> {
        self.loop_depth += 1;
        let begin_idx = self.emit(Op::WhileBegin { end_pc: 0 }, stmt.line, stmt.col);
        let start = self.code.len();
        self.compile_expr(cond)?;
        let exit_jump = self.emit(Op::JumpIfFalseLoop(0), cond.line, cond.col);
        self.emit(Op::WhileTick, cond.line, cond.col);
        for s in body {
            self.compile_stmt(s)?;
        }
        self.emit(Op::Jump(start), stmt.line, stmt.col);
        let end_idx = self.emit(Op::LoopEnd, stmt.line, stmt.col);
        self.patch_jump(exit_jump, end_idx);
        self.patch_end(begin_idx, end_idx);
        self.loop_depth -= 1;
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation, append } => {
                match target {
                    crate::ast::AssignTarget::Var(name) => {
                        self.compile_expr(value)?;
                        self.emit_assign_type_check(annotation, name, stmt.line, stmt.col);
                        // الدفعة 4 — المتغيرات (اجتماع 58
                        // §73): الإلحاق تعليمة مستقلة
                        // (القديم أو الهوية وقت التشغيل).
                        if *append {
                            self.emit(Op::StoreAppend(name.clone()), stmt.line, stmt.col);
                        } else {
                            self.emit(Op::Store(name.clone()), stmt.line, stmt.col);
                        }
                        Ok(())
                    }
                    crate::ast::AssignTarget::Index { target: t, index: i } => {
                        // Slice 3: index assignment on a plain
                        // variable only, like the interpreter
                        // (complex targets are rejected there too).
                        // Evaluation order matches the interpreter:
                        // value first, then the type check, then
                        // index. Stack at StoreIndex: [value, index].
                        let name = match &t.kind {
                            ExprKind::Ident(n) => n.clone(),
                            _ => {
                                return Err(RioError::parse(
                                    "الآلة الافتراضية: تعيين الفهرسة المعقدة غير مدعوم بعد، استخدم المفسر الافتراضي",
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        };
                        self.compile_expr(value)?;
                        self.emit_assign_type_check(annotation, "", stmt.line, stmt.col);
                        self.compile_expr(i)?;
                        self.emit(
                            Op::StoreIndex {
                                name,
                                idx_line: i.line,
                                idx_col: i.col,
                                tgt_line: t.line,
                                tgt_col: t.col,
                            },
                            stmt.line,
                            stmt.col,
                        );
                        Ok(())
                    }
                    crate::ast::AssignTarget::Dot { target: t, name } => {
                        // Slice 5: field assignment on an instance.
                        // Evaluation order matches the interpreter:
                        // value first, then the type check, then
                        // target. Stack at StoreDot: [value, target].
                        self.compile_expr(value)?;
                        self.emit_assign_type_check(annotation, "", stmt.line, stmt.col);
                        self.compile_expr(t)?;
                        self.emit(Op::StoreDot(name.clone()), stmt.line, stmt.col);
                        Ok(())
                    }
                }
            }
            StmtKind::Print { args } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit(Op::Print(args.len()), stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::If { cond, then_body, elifs, else_body } => {
                // Chain: cond, JumpIfFalse -> next, then, Jump -> end,
                // elifs..., else, end. Patch all end-jumps at the end.
                let mut end_jumps: Vec<usize> = Vec::new();
                self.compile_expr(cond)?;
                self.emit(Op::CheckBool(BoolCtx::If), cond.line, cond.col);
                let mut next_jump = self.emit(Op::JumpIfFalse(0), cond.line, cond.col);
                for s in then_body {
                    self.compile_stmt(s)?;
                }
                end_jumps.push(self.emit(Op::Jump(0), stmt.line, stmt.col));
                self.patch_jump(next_jump, self.code.len());
                for (econd, ebody) in elifs {
                    self.compile_expr(econd)?;
                    self.emit(Op::CheckBool(BoolCtx::Elif), econd.line, econd.col);
                    next_jump = self.emit(Op::JumpIfFalse(0), econd.line, econd.col);
                    for s in ebody {
                        self.compile_stmt(s)?;
                    }
                    end_jumps.push(self.emit(Op::Jump(0), stmt.line, stmt.col));
                    self.patch_jump(next_jump, self.code.len());
                }
                if let Some(body) = else_body {
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                }
                let end = self.code.len();
                for j in end_jumps {
                    self.patch_jump(j, end);
                }
                Ok(())
            }
            StmtKind::While { cond, body } => self.compile_while(cond, body, stmt),
            StmtKind::RepeatWhile { cond, body } => self.compile_while(cond, body, stmt),
            StmtKind::RepeatCount { count, body } => {
                // ك-1.2: العدد يُقيَّم مرة واحدة قبل الحلقة،
                // والتحقق (عدد صحيح غير سالب ≤ ١٬٠٠٠٬٠٠٠)
                // عند وقت التشغيل في RepeatCountSetup.
                self.loop_depth += 1;
                self.compile_expr(count)?;
                let setup_idx = self.emit(
                    Op::RepeatCountSetup { end_pc: 0 },
                    count.line,
                    count.col,
                );
                let next_idx = self.emit(Op::RepeatCountNext(0), stmt.line, stmt.col);
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.emit(Op::Jump(next_idx), stmt.line, stmt.col);
                let end_idx = self.emit(Op::LoopEnd, stmt.line, stmt.col);
                self.patch_end(setup_idx, end_idx);
                self.patch_jump(next_idx, end_idx);
                self.loop_depth -= 1;
                Ok(())
            }
            StmtKind::For { var, iterable, body } => {
                // ك-1.2: التكرارية تُقيَّم مرة واحدة، والمواد
                // مادّية (لقطة) في ForSetup — كما في المفسر.
                self.loop_depth += 1;
                self.compile_expr(iterable)?;
                let setup_idx = self.emit(
                    Op::ForSetup { var: var.clone(), end_pc: 0 },
                    iterable.line,
                    iterable.col,
                );
                let next_idx = self.emit(Op::ForNext(0), stmt.line, stmt.col);
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.emit(Op::Jump(next_idx), stmt.line, stmt.col);
                let end_idx = self.emit(Op::LoopEnd, stmt.line, stmt.col);
                self.patch_end(setup_idx, end_idx);
                self.patch_jump(next_idx, end_idx);
                self.loop_depth -= 1;
                Ok(())
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    return Err(RioError::runtime("اكسر خارج حلقة", stmt.line, stmt.col));
                }
                self.emit(Op::Break, stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    return Err(RioError::runtime("تابع خارج حلقة", stmt.line, stmt.col));
                }
                self.emit(Op::Continue, stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::Try { try_body, except_body, finally_body } => {
                // ك-1.2: ترتيب التعليمات في التدفق:
                //   TryBegin -> جسم حاول -> EndTryBlock
                //   -> جسم أخيرا -> EndFinally
                //   -> جسم ماعدا -> EndTryBlock
                //   -> بعد
                // EndTryBlock في نهاية جسم حاول يقفز إلى أخيرا
                // (إن وُجد) أو إلى بعد؛ وEndTryBlock في نهاية
                // جسم ماعدا يفعل الشيء نفسه. أهداف TryBegin
                // وEndTryBlock تُرقَّع بعد ترجمة الأجسام.
                let begin_idx = self.emit(
                    Op::TryBegin { except_pc: None, finally_pc: None, after_pc: 0 },
                    stmt.line,
                    stmt.col,
                );
                for s in try_body {
                    self.compile_stmt(s)?;
                }
                let end_try_idx = self.emit(
                    Op::EndTryBlock { finally_pc: None, after_pc: 0 },
                    stmt.line,
                    stmt.col,
                );
                let finally_idx = match finally_body {
                    Some(fin) => {
                        let idx = self.code.len();
                        for s in fin {
                            self.compile_stmt(s)?;
                        }
                        self.emit(Op::EndFinally, stmt.line, stmt.col);
                        Some(idx)
                    }
                    None => None,
                };
                let mut end_exc_idx = None;
                let except_idx = match except_body {
                    Some(exc) => {
                        let idx = self.code.len();
                        for s in exc {
                            self.compile_stmt(s)?;
                        }
                        end_exc_idx = Some(self.emit(
                            Op::EndTryBlock { finally_pc: finally_idx, after_pc: 0 },
                            stmt.line,
                            stmt.col,
                        ));
                        Some(idx)
                    }
                    None => None,
                };
                let after = self.code.len();
                self.patch_try_begin(begin_idx, except_idx, finally_idx, after);
                self.patch_end_try_block(end_try_idx, finally_idx, after);
                if let Some(i) = end_exc_idx {
                    self.patch_end_try_block(i, finally_idx, after);
                }
                Ok(())
            }
            StmtKind::ExprStmt { expr } => {
                self.compile_expr(expr)?;
                self.emit(Op::Pop, stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::FuncDef { name, params, param_types, return_type, body } => {
                // Phase ك-1: every دالة definition becomes a
                // closure value at runtime (DefineClosure), so
                // it captures its defining scope chain exactly
                // like the interpreter (Meeting 11). The body
                // is compiled with a sub-compiler, its code
                // registered under a fresh id, and the name
                // table entry kept as well (compile-time
                // Call-vs-Builtin decision + fallback for
                // calls whose name is not bound in the env).
                // The outer definition wins over a nested
                // same-named one in the table, as before.
                let mut sub = self.sub_compiler();
                for s in body {
                    sub.compile_stmt(s)?;
                }
                let code = std::mem::take(&mut sub.code);
                self.merge_sub(sub);
                let id = next_closure_id();
                let def = FunctionDef {
                    params: params.clone(),
                    param_types: param_types.clone(),
                    return_type: return_type.clone(),
                    code,
                };
                self.functions.insert(name.clone(), def.clone());
                self.closures.insert(id, def);
                self.emit(
                    Op::DefineClosure(ClosureDef {
                        id,
                        name: name.clone(),
                        params: params.clone(),
                        param_types: param_types.clone(),
                        return_type: return_type.clone(),
                        body: body.clone(),
                    }),
                    stmt.line,
                    stmt.col,
                );
                Ok(())
            }
            StmtKind::ClassDef { name, base, interfaces, methods } => {
                // Slice 5: compile each method body with a fresh
                // sub-compiler (like FuncDef), merge nested
                // functions into this table, then flatten: base
                // methods first, own methods override. Base and
                // interface EXISTENCE are checked at runtime by
                // Op::DefineClass, like the interpreter.
                let mut flat: HashMap<String, FunctionDef> = HashMap::new();
                if let Some(bname) = base {
                    if let Some(bdef) = self.classes.get(bname) {
                        flat = bdef.methods.clone();
                    }
                }
                // مهمة 3: معرّفات بايت كود الطرق الخاصة بهذه
                // الفئة وحدها في سجل الإغلاقات (تُسجَّل وقت
                // الترجمة)، لتُبنى منها وقت التشغيل قيم
                // دوال تحمل سلسلة النطاق المعرِّف.
                let mut own_ids: HashMap<String, u64> = HashMap::new();
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, param_types, return_type, body } = &m.kind {
                        let mut sub = self.sub_compiler();
                        for s in body {
                            sub.compile_stmt(s)?;
                        }
                        let code = std::mem::take(&mut sub.code);
                        self.merge_sub(sub);
                        let fdef = FunctionDef {
                            params: params.clone(),
                            param_types: param_types.clone(),
                            return_type: return_type.clone(),
                            code,
                        };
                        let id = next_closure_id();
                        self.closures.insert(id, fdef.clone());
                        flat.insert(mn.clone(), fdef);
                        own_ids.insert(mn.clone(), id);
                    }
                }
                let def = VmClassDef {
                    name: name.clone(),
                    base: base.clone(),
                    interfaces: interfaces.clone(),
                    methods: flat,
                    own_method_ids: own_ids,
                };
                self.classes.insert(name.clone(), def.clone());
                self.emit(Op::DefineClass(def), stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::InterfaceDef { name, methods } => {
                self.emit(
                    Op::DefineInterface(name.clone(), methods.clone()),
                    stmt.line,
                    stmt.col,
                );
                Ok(())
            }
            StmtKind::Return { value } => {
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => {
                        self.emit(Op::Const(Value::Nil), stmt.line, stmt.col);
                    }
                }
                self.emit(Op::Return, stmt.line, stmt.col);
                Ok(())
            }
            // مهمة 4 (ك-1.3): الاستيراد تعليمة وقت تشغيل
            // (تحميل الوحدة يحدث حين يُنفَّذ السطر، كما
            // في المفسر)؛ والتصدير لا-عملية (تُجمَع
            // أسماؤه وقت تحميل الوحدة فقط).
            StmtKind::Import { module, alias, names } => {
                self.emit(
                    Op::Import { module: module.clone(), alias: alias.clone(), names: names.clone() },
                    stmt.line,
                    stmt.col,
                );
                Ok(())
            }
            StmtKind::Export { .. } => Ok(()),
            _ => Err(RioError::parse(
                "الآلة الافتراضية: هذه الجملة غير مدعومة بعد، استخدم المفسر الافتراضي",
                stmt.line,
                stmt.col,
            )),
        }
    }

    fn compile_expr(&mut self, expr: &Expr) -> RioResult<()> {
        match &expr.kind {
            ExprKind::Number(n) => {
                self.emit(Op::Const(Value::Number(*n)), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Str(s) => {
                self.emit(Op::Const(Value::Str(s.clone())), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Bool(b) => {
                self.emit(Op::Const(Value::Bool(*b)), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Nil => {
                self.emit(Op::Const(Value::Nil), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Ident(name) => {
                self.emit(Op::Load(name.clone()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::List(elems) => {
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.emit(Op::BuildList(elems.len()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Dict(entries) => {
                // Keys are compile-time strings; values are
                // compiled in order and popped by BuildDict.
                let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
                for (_, v) in entries {
                    self.compile_expr(v)?;
                }
                self.emit(Op::BuildDict(keys), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Index { target, index } => {
                self.compile_expr(target)?;
                self.compile_expr(index)?;
                self.emit(
                    Op::Index { idx_line: index.line, idx_col: index.col },
                    expr.line,
                    expr.col,
                );
                Ok(())
            }
            ExprKind::Dot { target, name } => {
                self.compile_expr(target)?;
                self.emit(Op::Dot(name.clone()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Create { class, args } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit(Op::Create(class.clone(), args.len()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Unary { op, expr: inner } => {
                self.compile_expr(inner)?;
                match op {
                    UnOp::Neg => self.emit(Op::Neg, expr.line, expr.col),
                    UnOp::Not => self.emit(Op::Not, expr.line, expr.col),
                };
                Ok(())
            }
            ExprKind::Binary { left, op, right } => {
                // Short-circuit و / أو, preserving interpreter semantics.
                // JumpIfFalse/True pop the left value, so the false/
                // true target must push the short-circuit result.
                if *op == BinOp::And {
                    self.compile_expr(left)?;
                    self.emit(Op::CheckBool(BoolCtx::And), left.line, left.col);
                    let jf = self.emit(Op::JumpIfFalse(0), expr.line, expr.col);
                    self.compile_expr(right)?;
                    self.emit(Op::CheckBool(BoolCtx::And), right.line, right.col);
                    let jmp_end = self.emit(Op::Jump(0), expr.line, expr.col);
                    let false_target = self.code.len();
                    self.patch_jump(jf, false_target);
                    self.emit(Op::Const(Value::Bool(false)), expr.line, expr.col);
                    let end = self.code.len();
                    self.patch_jump(jmp_end, end);
                    return Ok(());
                }
                if *op == BinOp::Or {
                    self.compile_expr(left)?;
                    self.emit(Op::CheckBool(BoolCtx::Or), left.line, left.col);
                    let jt = self.emit(Op::JumpIfTrue(0), expr.line, expr.col);
                    self.compile_expr(right)?;
                    self.emit(Op::CheckBool(BoolCtx::Or), right.line, right.col);
                    let jmp_end = self.emit(Op::Jump(0), expr.line, expr.col);
                    let true_target = self.code.len();
                    self.patch_jump(jt, true_target);
                    self.emit(Op::Const(Value::Bool(true)), expr.line, expr.col);
                    let end = self.code.len();
                    self.patch_jump(jmp_end, end);
                    return Ok(());
                }
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let op = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Eq => Op::Eq,
                    BinOp::NotEq => Op::NotEq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::LtEq => Op::LtEq,
                    BinOp::GtEq => Op::GtEq,
                    BinOp::In => Op::In,
                    BinOp::NotIn => Op::NotIn,
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.emit(op, expr.line, expr.col);
                Ok(())
            }
            ExprKind::Call { callee, args } => {
                // مهمة 3: استدعاء عبر نقطة (ك.طريقة(...))
                // يُترجَم إلى CallDotMethod: المعاملات أولا
                // ثم الهدف (ترتيب تقييم المفسر نفسه)،
                // فيعترض الاستدعاءُ الهدفَ قبل تقييم النقطة
                // كقيمة — كما يفعل المفسر مع Dot callee.
                // `var` اسم الهدف حين يكون معرّفا عاديا،
                // لتكتب طرق القوائم المعدِّلة رجعيا إليه.
                if let ExprKind::Dot { target, name } = &callee.kind {
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.compile_expr(target)?;
                    let var = if let ExprKind::Ident(v) = &target.kind {
                        Some(v.clone())
                    } else {
                        None
                    };
                    self.emit(
                        Op::CallDotMethod { name: name.clone(), var, nargs: args.len() },
                        expr.line,
                        expr.col,
                    );
                    return Ok(());
                }
                // Phase ك-1: any other callee (a variable holding
                // a closure, a call result, a parenthesised
                // lambda) compiles the args first, then the
                // callee (the interpreter's evaluation order),
                // then CallValue dispatches on the value.
                if !matches!(&callee.kind, ExprKind::Ident(_)) {
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.compile_expr(callee)?;
                    self.emit(
                        Op::CallValue {
                            nargs: args.len(),
                            callee_line: callee.line,
                            callee_col: callee.col,
                        },
                        expr.line,
                        expr.col,
                    );
                    return Ok(());
                }
                // Non-Ident, non-Dot callees were handled above.
                let name = if let ExprKind::Ident(n) = &callee.kind {
                    n.clone()
                } else {
                    unreachable!("non-Ident callee handled above")
                };
                for a in args {
                    self.compile_expr(a)?;
                }
                // Slice 4: resolution order matches the
                // interpreter (user function first, then
                // builtin). A user function defined earlier
                // in the program wins over a builtin name.
                // A same-named variable is checked at
                // runtime by Op::Builtin itself.
                if self.functions.contains_key(&name) {
                    self.emit(
                        Op::Call {
                            name,
                            nargs: args.len(),
                            callee_line: callee.line,
                            callee_col: callee.col,
                        },
                        expr.line,
                        expr.col,
                    );
                } else if is_pure_builtin(&name) || is_file_builtin(&name) {
                    self.emit(
                        Op::Builtin {
                            name,
                            nargs: args.len(),
                            callee_line: callee.line,
                            callee_col: callee.col,
                        },
                        expr.line,
                        expr.col,
                    );
                } else {
                    self.emit(
                        Op::Call {
                            name,
                            nargs: args.len(),
                            callee_line: callee.line,
                            callee_col: callee.col,
                        },
                        expr.line,
                        expr.col,
                    );
                }
                Ok(())
            }
            ExprKind::Lambda { params, param_types, body } => {
                // Phase ك-1: compile the single body expression
                // with a sub-compiler, wrap it as an implicit
                // Return (exactly like the interpreter), register
                // the code under a fresh id, and emit MakeClosure
                // (the value captures the defining env at runtime).
                let mut sub = self.sub_compiler();
                sub.compile_expr(body)?;
                sub.emit(Op::Return, expr.line, expr.col);
                let code = std::mem::take(&mut sub.code);
                self.merge_sub(sub);
                let id = next_closure_id();
                let def = FunctionDef {
                    params: params.clone(),
                    param_types: param_types.clone(),
                    return_type: None,
                    code,
                };
                self.closures.insert(id, def);
                self.emit(
                    Op::MakeClosure(ClosureDef {
                        id,
                        name: "مجهولة".to_string(),
                        params: params.clone(),
                        param_types: param_types.clone(),
                        return_type: None,
                        body: vec![Stmt::new(
                            StmtKind::Return { value: Some((**body).clone()) },
                            expr.line,
                            expr.col,
                        )],
                    }),
                    expr.line,
                    expr.col,
                );
                Ok(())
            }
        }
    }
}

/// ك-1.2: حالة حلقة واحدة قيد التنفيذ. المواد (items)
/// مادّية عند الدخول — لقطة كما في المفسر: تعديل القائمة
/// داخل الجسم لا يغيّر التكرار. end_pc هو تعليمة LoopEnd،
/// وcontinue_pc هو رأس الحلقة (ForNext / RepeatCountNext /
/// بداية تقييم شرط بينما).
#[derive(Debug, Clone)]
enum LoopState {
    For { items: Vec<Value>, index: usize, var: String, end_pc: usize, continue_pc: usize },
    Count { remaining: usize, end_pc: usize, continue_pc: usize },
    While { iterations: usize, end_pc: usize, continue_pc: usize },
}

/// ك-1.2: ما يجب أن يفعله جسم أخيرا عند اكتماله الطبيعي.
/// Raise يحمل الخطأ المعلق: إن أخطأ جسم أخيرا نفسه أثناء
/// تنفيذ مسار خطأ، يُتجاهل خطؤه ويُعاد رفع هذا الخطأ
/// (المفسر: `let _ = self.exec_block(fin);` في مساري
/// لا-ماعدا وخطأ-ماعدا).
#[derive(Debug, Clone)]
enum FlowResume {
    After(usize),
    Raise(RioError),
    Break { end_pc: usize },
    Continue { target_pc: usize },
    Return,
    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): `خروج(رمز)`
    /// ينهي البرنامج كله — يهرب من كل معالج كـ Return،
    /// ويشغّل `أخيرا` في طريقه، ولا يمسكه `ماعدا`.
    Exit(u8),
    /// اكسر/تابع بلا حلقة محيطة: تُشغَّل أخيرا أولا، ثم
    /// يُرفع الخطأ بالرسالة المخزنة.
    NoLoop(&'static str),
}

/// ك-1.2: جسم أخيرا قيد التنفيذ مع ما كان معلقا له.
#[derive(Debug, Clone)]
struct FinallyCtx {
    resume: FlowResume,
}

/// ك-1.2: منطقة حاول قيد التنفيذ. stack_depth وloop_depth
/// وfinally_depth أعماق لحظة TryBegin: عند الإمساك بالخطأ
/// تُحذف القيم الجزئية من جسم حاول الفاشل، وتُهجر الحلقات
/// المدخولة داخله، وتُعرف أخيرا التي هرب منها الخطأ.
#[derive(Debug, Clone)]
struct Handler {
    except_pc: Option<usize>,
    finally_pc: Option<usize>,
    after_pc: usize,
    stack_depth: usize,
    loop_depth: usize,
    finally_depth: usize,
}

/// ك-1.2: ما تعيده خطوة تنفيذ واحدة: متابعة، أو اكتمال
/// الدالة (ارجع).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepFlow {
    Next,
    Returned,
}

/// ك-1.2: ما قرره unwind_flow: متابعة التنفيذ (قفز إلى
/// أخيرا أو إلى هدف التدفق)، أو اكتمال الارجع.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnwindOutcome {
    Continue,
    ReturnNow,
}

pub struct Vm {
    pub code: Vec<Instr>,
    pub pc: usize,
    pub stack: Vec<Value>,
    /// Phase ك-1: the scope chain, exactly like the
    /// interpreter's: each scope is shared (Rc) so a closure
    /// value can capture its defining chain and later calls
    /// still share it with the defining environment.
    pub env: Vec<Rc<RefCell<HashMap<String, Value>>>>,
    pub output: String,
    pub functions: Rc<HashMap<String, FunctionDef>>,
    /// Phase ك-1: compiled closure bodies by id (the registry
    /// Value::Function.vm_id keys into). Shared with child VMs.
    /// مهمة 4: RefCell لأن تحميل وحدة يدمج سجل إغلاقاتها
    /// في السجل المشترك — وإلا فدالة وحدة حُمّلت داخل
    /// استدعاء لا يراها المتصل بعد عودته (المفسر
    /// يشارك loaded_modules؛ والسجل هنا نظيره).
    pub closures: Rc<RefCell<HashMap<u64, FunctionDef>>>,
    /// مهمة 4 (ك-1.3): مجلد البرنامج الرئيسي — تُحلّ
    /// أسماء الوحدات الملفية نسبة إليه، كما في المفسر.
    pub program_dir: std::path::PathBuf,
    /// مهمة 5 (ك-1.4): سياسة الملفات (مقيّدة بمجلد
    /// البرنامج افتراضيا، أو موثوقة بـ --اسمح-بالكل)
    /// ومجلد الجلسة الحالي (يغيّره انتقل) — مشتركان
    /// (Rc) مع كل VM فرعية، كما أن المفسر الواحد
    /// يشارك cwd وfile_policy في كل استدعاءاته، فيبقى
    /// تغيير انتقل داخل دالة ظاهرا للمتصل بعد عودتها.
    pub file_policy: Rc<RefCell<crate::permissions::FilePolicy>>,
    pub cwd: Rc<RefCell<std::path::PathBuf>>,
    /// اجتماع 56 §71 — الدفعة 2 (التنقل): مكدس
    /// المجلدات وقائمة بحث `CDPATH` وراية الوضع
    /// الفيزيائي العام — مشتركة مع كل VM فرعية كـ`cwd`
    /// (فلا يضيع المكدس داخل دالة).
    pub dir_stack: Rc<RefCell<Vec<std::path::PathBuf>>>,
    pub cdpath: Rc<RefCell<Vec<String>>>,
    pub physical_mode: Rc<RefCell<bool>>,
    /// مهمة 4: الوحدات المحمّلة مرة واحدة في هذا التشغيل
    /// (اسم ← قيمة الوحدة)، مشتركة مع كل VM فرعية.
    pub loaded_modules: Rc<RefCell<HashMap<String, Value>>>,
    /// مهمة 4: الوحدات قيد التحميل الآن (كشف الدورات)،
    /// مشتركة مع كل VM فرعية.
    pub loading_stack: Rc<RefCell<Vec<String>>>,
    pub return_value: Option<Value>,
    pub call_depth: usize,
    /// Meeting 27: the server flag (خادم HTTPS). Same
    /// per-instance pattern as the interpreter's — read
    /// through the HandlerCaller context when a native
    /// function runs; inherited by child VMs so a handler
    /// calling اخدم again sees the same permission.
    pub server_allowed: bool,
    /// اجتماع 32، السؤال الخامس: الملف الجاري تنفيذه —
    /// تُسنَد إليه أخطاء التشغيل عند خروج run()؛
    /// وترثه VM الفرعية (دالة) من أمها، وتستبدله
    /// وحدة ملف بملفها.
    pub source_file: Option<String>,
    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// — كالمفسر حرفيا؛ وترثه VM الفرعية من أمها.
    pub live_output: bool,
    /// اجتماع 32، السؤال الخامس: اسم وحدة ← ملفها —
    /// مشتركة (Rc) مع كل VM فرعية كـ loaded_modules،
    /// لإسناد أخطاء أجسام دوال الوحدة إلى ملفها.
    pub module_files: Rc<RefCell<HashMap<String, String>>>,
    /// ك-1.2: حالات الحلقات قيد التنفيذ (الأعلى هي الأعمق).
    loop_states: Vec<LoopState>,
    /// ك-1.2: معالجات حاول قيد التنفيذ (الأعلى هي الأعمق).
    handlers: Vec<Handler>,
    /// ك-1.2: أجسام أخيرا قيد التنفيذ (الأعلى هي الأعمق).
    finally_stack: Vec<FinallyCtx>,
    /// ك-1.2: التدفق المعلق أثناء فكّ أخيرا (اكسر/تابع/ارجع).
    pending_flow: Option<FlowResume>,
    /// ك-1.2: قيمة الارجع المعلقة أثناء فكّ أخيرا.
    pending_return: Option<Value>,
    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): خروج معلق
    /// طلبه `خروج(رمز)` (يأخذه المتصل مرة واحدة)، ورمز
    /// آخر أمر مكتمل (0 أولا؛ و1 بعد خطأ ممسوك بـ`ماعدا`).
    exit_code: Option<u8>,
    last_exit_code: u8,
    // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
    // و63 §77 و§78): رموز مراحل آخر أنبوب تقرأها
    // `رموز_الأنبوب()` — مرآة المفسر.
    pipe_status: Vec<u8>,
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): أسماء
    // المتغيرات المجمدة نهائيا، وخريطة المراجع
    // الاسمية، وبدء الجلسة (لـ`بيئة("ثواني")`) —
    // مشتركة (Rc) مع كل VM فرعية كـ`cwd`، فلا
    // يضيع التجميد داخل دالة.
    frozen: Rc<RefCell<HashSet<String>>>,
    namerefs: Rc<RefCell<HashMap<String, String>>>,
    started: Rc<std::time::Instant>,
}

impl Vm {
    pub fn new(compiled: CompiledProgram, program_dir: std::path::PathBuf) -> Self {
        Self {
            code: compiled.code,
            pc: 0,
            stack: Vec::new(),
            env: vec![Rc::new(RefCell::new(HashMap::new()))],
            output: String::new(),
            functions: Rc::new(compiled.functions),
            closures: Rc::new(RefCell::new(compiled.closures)),
            program_dir: program_dir.clone(),
            file_policy: Rc::new(RefCell::new(
                crate::permissions::FilePolicy::restricted(program_dir.clone()),
            )),
            // اجتماع 45 §59: الجلسة تبدأ في البيت.
            cwd: Rc::new(RefCell::new(program_dir.join("home"))),
            // اجتماع 56 §71 — الدفعة 2 (التنقل).
            dir_stack: Rc::new(RefCell::new(Vec::new())),
            cdpath: Rc::new(RefCell::new(Vec::new())),
            physical_mode: Rc::new(RefCell::new(false)),
            loaded_modules: Rc::new(RefCell::new(HashMap::new())),
            loading_stack: Rc::new(RefCell::new(Vec::new())),
            return_value: None,
            call_depth: 0,
            server_allowed: false,
            source_file: None,
            live_output: false,
            module_files: Rc::new(RefCell::new(HashMap::new())),
            loop_states: Vec::new(),
            handlers: Vec::new(),
            finally_stack: Vec::new(),
            pending_flow: None,
            pending_return: None,
            exit_code: None,
            last_exit_code: 0,
            pipe_status: Vec::new(),
            frozen: Rc::new(RefCell::new(HashSet::new())),
            namerefs: Rc::new(RefCell::new(HashMap::new())),
            started: Rc::new(std::time::Instant::now()),
        }
    }

    /// Look a name up innermost-first, like the interpreter.
    fn lookup(&self, name: &str) -> Option<Value> {
        let resolved = self.resolve_name(name);
        for scope in self.env.iter().rev() {
            if let Some(v) = scope.borrow().get(&resolved) {
                return Some(v.clone());
            }
        }
        None
    }

    /// Define in the innermost scope (دالة definitions,
    /// classes, interfaces), like the interpreter's
    /// define_local.
    fn define(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.env.last() {
            scope.borrow_mut().insert(name.to_string(), value);
        }
    }

    /// مهمة 5: الوضع الموثوق (--اسمح-بالكل)، مرآة
    /// Interpreter::set_trusted — يزيل قيد مجلد البرنامج.
    /// يُستدعى من main.rs في مسار VM كما في مسار المفسر.
    pub fn set_trusted(&mut self, trusted: bool) {
        self.file_policy.borrow_mut().trusted = trusted;
    }

    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root for this session — مرآة
    /// Interpreter::set_downloads.
    pub fn set_downloads(&mut self, path: std::path::PathBuf) {
        self.file_policy.borrow_mut().set_downloads(path);
    }

    /// Meeting 27: server capability (--اسمح-بالخادم).
    /// Default OFF; the network flag alone is not enough.
    /// Read through the HandlerCaller context when a native
    /// function runs; inherited by child VMs.
    pub fn set_server_allowed(&mut self, allowed: bool) {
        self.server_allowed = allowed;
    }

    /// This VM's own server permission.
    pub fn server_allowed(&self) -> bool {
        self.server_allowed
    }

    /// اجتماع 32، السؤال الخامس: ملف المصدر الجاري
    /// تنفيذه — تُسنَد إليه أخطاء التشغيل. يضبطه المتصل
    /// العلوي (main.rs) للملف الرئيسي؛ وتحميل الوحدة
    /// يبدّله في نسختها.
    pub fn set_source_file(&mut self, file: &str) {
        self.source_file = Some(file.to_string());
    }

    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): يأخذ
    /// رمز الخروج المعلق مرة واحدة (منتهية كـ Take).
    pub fn take_exit_code(&mut self) -> Option<u8> {
        self.exit_code.take()
    }

    /// الدفعة 1: رمز الخروج المعلق بلا أخذ.
    pub fn exit_code(&self) -> Option<u8> {
        self.exit_code
    }

    /// الدفعة 1: رمز آخر أمر مكتمل.
    pub fn last_exit_code(&self) -> u8 {
        self.last_exit_code
    }

    /// الدفعة 1: ضبط رمز آخر أمر (تزامن REPL للمحرك
    /// بعد كل سطر، كالمفسر).
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

    /// ك-2 (مهمة 7): حمّل برنامجا مترجما جديدا على نسخة VM
    /// دائمة (جلسة REPL): يستبدل الكود وجدول الدوال، ويدمج
    /// سجل الإغلاقات في السجل المشترك، ويصفّر عدّاد البرنامج
    /// والمكدس وعمق الاستدعاء وحالات الحلقات والمعالجات —
    /// ويُبقي البيئة (العموميات) والوحدات المحمّلة وسياسة
    /// الملفات ومجلد الجلسة. خطأ الترجمة يحدث قبل التحميل
    /// أبدا، فسطر فاشل لا يلوّث الجلسة.
    pub fn load(&mut self, compiled: CompiledProgram) {
        for (id, def) in compiled.closures {
            self.closures.borrow_mut().insert(id, def);
        }
        self.code = compiled.code;
        self.functions = Rc::new(compiled.functions);
        self.pc = 0;
        self.stack.clear();
        self.return_value = None;
        // الدفعة 1: برنامج جديد بلا خروج معلق — أما رمز
        // آخر أمر فيبقى (تزامنه REPL بعد كل سطر).
        self.exit_code = None;
        self.call_depth = 0;
        self.loop_states.clear();
        self.handlers.clear();
        self.finally_stack.clear();
        self.pending_flow = None;
        self.pending_return = None;
    }

    /// ك-2 (مهمة 7): خذ القيمة العليا من المكدس بعد تشغيل
    /// برنامج تعبير (compile_expr_program) — قيمة التعبير
    /// الذي يطبعه REPL. لا شيء على المكدس (لا يحدث عادة)
    /// يعني لاشيء، لا خطأ.
    pub fn pop_result(&mut self) -> Option<Value> {
        self.stack.pop()
    }

    /// ك-2 (مهمة 7): استدعاء دالة مدمجة مباشرة، مرآة
    /// Interpreter::call_builtin الذي يستدعيه REPL
    /// للاختصارات — إرسال مباشر بلا فحص بيئة (فحص البيئة
    /// يخص مسار الاستدعاء في الكود، Op::Builtin، لا هذا
    /// المسار)، فالاختصار يتصرف في الوضعين بالطريقة نفسها.
    /// خالصة → call_pure_builtin؛ ملف/ادخل →
    /// call_file_builtin؛ غير ذلك خطأ «دالة غير معرفة»
    /// برسالة المفسر نفسها.
    pub fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        if is_pure_builtin(name) {
            self.call_pure_builtin(name, args, line, col)
        } else if is_file_builtin(name) {
            self.call_file_builtin(name, args, line, col)
        } else {
            Err(RioError::runtime(
                format!("دالة غير معرفة: {}", name),
                line,
                col,
            ))
        }
    }

    /// مهمة 5: حلّ مسار مستخدم نسبة إلى مجلد الجلسة
    /// الحالي (انتقل)، تحت سياسة الملفات — مرآة
    /// Interpreter::resolve_cwd تماما.
    /// اجتماع 56 §71: توسعة `~+` و`~+N` أولا — فهي في
    /// كل مسارات الجلسة، كما توسعة التلدة في باش.
    fn resolve_cwd(&self, path: &str, line: usize, col: usize) -> RioResult<std::path::PathBuf> {
        match self.expand_tilde_plus(path, line, col)? {
            Some(expanded) => self.file_policy.borrow().resolve_in(&self.cwd.borrow(), &expanded, line, col),
            None => self
                .file_policy
                .borrow()
                .resolve_in(&self.cwd.borrow(), path, line, col),
        }
    }

    /// Like `resolve_cwd`, but a write is refused under the
    /// downloads root (Meeting 50 §64, items 94/96) — مرآة
    /// Interpreter::resolve_cwd_for_write تماما.
    fn resolve_cwd_for_write(&self, path: &str, line: usize, col: usize) -> RioResult<std::path::PathBuf> {
        match self.expand_tilde_plus(path, line, col)? {
            Some(expanded) => self.file_policy.borrow().resolve_in_for_write(
                &self.cwd.borrow(),
                &expanded,
                line,
                col,
            ),
            None => self
                .file_policy
                .borrow()
                .resolve_in_for_write(&self.cwd.borrow(), path, line, col),
        }
    }

    /// اجتماع 56 §71 — الدفعة 2 (التنقل): مرآة مساعدات
    /// المفسر نفسها (الحالة هنا `Rc<RefCell<...>>`
    /// مشتركة مع VM الفرعية، فالأساليب `&self`).

    /// الدفعة 4 — المتغيرات (اجتماع 58 §73): قيمة
    /// متغير بيئة ديناميكي، أو `None` إن كان الاسم
    /// ثابتا — مرآة مساعد المفسر نفسه.
    fn env_dynamic(&self, name: &str, line: usize, col: usize) -> Option<RioResult<String>> {
        match name {
            "المجلد_الحالي" => Some(Ok(self.display_current(None))),
            "المستخدم" => Some(crate::env::username().map_err(|m| RioError::runtime(m, line, col))),
            "اسم_المضيف" => Some(crate::env::hostname().map_err(|m| RioError::runtime(m, line, col))),
            "عشوائي" => Some(Ok(Value::Number(crate::env::random_0_32767() as f64).fmt_value())),
            "ثواني" => Some(Ok(Value::Number(self.started.elapsed().as_secs() as f64).fmt_value())),
            "رقم_السطر" => Some(Ok(Value::Number(line as f64).fmt_value())),
            _ => None,
        }
    }

    /// الدفعة 4 — المتغيرات (اجتماع 58 §73): قائمة
    /// «الاسم=القيمة» للأربعة عشر اسما المقروءة —
    /// مرآة مساعد المفسر نفسه.
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
    // بيئة لينة بلا خطأ — مرآة مساعد المفسر نفسه.
    fn env_lookup_soft(&self, name: &str, line: usize, col: usize) -> Option<String> {
        match self.env_dynamic(name, line, col) {
            Some(Ok(v)) => Some(v),
            Some(Err(_)) => None,
            None => crate::env::lookup(name, crate::repl::version()),
        }
    }

    // المرحلة (د) — الإكمال (اجتماع 68 §83): حلّ
    // مسار `مصدر` — مرآة مساعد المفسر نفسه (المطلق
    // كما هو بسياسة الملفات، وإلا `$نيفا` ثم `~` ثم
    // المجلد الحالي؛ واللاحقة `.نيفا` تُلحق حين تغيب).
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
            self.cwd.borrow().join(&file_name),
        ] {
            if std::fs::metadata(&cand).map(|m| m.is_file()).unwrap_or(false) {
                return Ok((cand, file_name));
            }
        }
        Err(RioError::new(
            crate::error::ErrorKind::Io,
            format!("الملف غير موجود: {}", name),
            line,
            col,
        ))
    }

    /// المرحلة (د) — الإكمال (اجتماعا 66 و67 §81
    /// و§82): أسماء متغيرات الجلسة (النطاق الخارجي
    /// وحده) — بلا دوال ولا وحدات — مرتبة؛ للإكمال.
    /// مرآة مساعد المفسر نفسه.
    pub fn session_variable_names(&self) -> Vec<String> {
        let mut names: Vec<String> = match self.env.first() {
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
    /// مرتبة؛ للإكمال ولـ`اعرض_الدوال`. مرآة مساعد
    /// المفسر نفسه.
    pub fn session_function_names(&self) -> Vec<String> {
        let mut names: Vec<String> = match self.env.first() {
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
        self.cwd.borrow().clone()
    }

    fn set_cwd(&self, new: std::path::PathBuf) {
        *self.cwd.borrow_mut() = new;
        crate::processes::set_process_policy(
            self.file_policy.borrow().base.clone(),
            self.file_policy.borrow().trusted,
            self.cwd.borrow().clone(),
        );
    }

    fn after_stack_move(&self) {
        if *self.physical_mode.borrow() {
            let cur = self.cwd.borrow().clone();
            if let Some(canon) = crate::navigation::canonical_dir(&cur) {
                *self.cwd.borrow_mut() = canon;
            }
        }
        crate::processes::set_process_policy(
            self.file_policy.borrow().base.clone(),
            self.file_policy.borrow().trusted,
            self.cwd.borrow().clone(),
        );
    }

    fn physical_cwd(&self) -> std::path::PathBuf {
        let cur = self.cwd.borrow().clone();
        crate::navigation::canonical_dir(&cur).unwrap_or(cur)
    }

    /// عرض مسار منشأ لإعلان `mkdir -v` (الميزة 211،
    /// الاجتماع 59 §74): نسبي إلى المجلد الحالي إن
    /// كان تحته، وإلا بصيغة العرض القائمة (`~` و
    /// `$نيفا`).
    fn display_created(&self, path: &std::path::Path) -> String {
        let cwd = self.cwd.borrow();
        if let Ok(rel) = path.strip_prefix(cwd.as_path()) {
            if !rel.as_os_str().is_empty() {
                return crate::permissions::slash_display(rel);
            }
        }
        self.file_policy
            .borrow()
            .display_str(&path.to_string_lossy())
    }

    fn display_current(&self, mode: Option<crate::navigation::NavMode>) -> String {
        let physical = match mode {
            Some(m) => m.is_physical(),
            None => *self.physical_mode.borrow(),
        };
        if physical {
            self.file_policy.borrow().display_path(&self.physical_cwd())
        } else {
            self.file_policy.borrow().display_path(&self.cwd.borrow())
        }
    }

    fn expand_tilde_plus(&self, path: &str, line: usize, col: usize) -> RioResult<Option<String>> {
        let (idx, tail) = match crate::navigation::parse_tilde_plus(path) {
            Some(v) => v,
            None => return Ok(None),
        };
        let base: std::path::PathBuf = if idx == 0 {
            self.cwd.borrow().clone()
        } else {
            match crate::navigation::resolve_stack_index(idx - 1, self.dir_stack.borrow().len()) {
                Some(pos) => self.dir_stack.borrow()[pos].clone(),
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

    fn try_resolved(&self, resolved: std::path::PathBuf, mode: crate::navigation::NavMode) -> TryDir {
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
            Ok(m) if m.is_dir() => TryDir::Ok(
                crate::interpreter::Interpreter::normalize_lexical(&resolved),
            ),
            Ok(_) => TryDir::NotDir,
            Err(_) => TryDir::NotFound,
        }
    }

    fn try_candidate(&self, raw: &str, mode: crate::navigation::NavMode, line: usize, col: usize) -> TryDir {
        match self.resolve_cwd(raw, line, col) {
            Ok(resolved) => {
                // بوابة الوجود (إصلاح انهيار `..`
                // المعجمي — مرآة المفسر): يُرفض المرشح
                // إن غاب مساره الموصول كما كُتب قبل
                // أي تطبيع.
                let gate_raw = match self.expand_tilde_plus(raw, line, col) {
                    Ok(Some(e)) => e,
                    Ok(None) => raw.to_string(),
                    Err(e) => return TryDir::Policy(e),
                };
                match self
                    .file_policy
                    .borrow()
                    .raw_joined_state(&self.cwd.borrow(), &gate_raw)
                {
                    Some(true) => self.try_resolved(resolved, mode),
                    Some(false) => TryDir::NotDir,
                    None => TryDir::NotFound,
                }
            }
            Err(e) => TryDir::Policy(e),
        }
    }

    fn spell_correct(&self, raw: &str, mode: crate::navigation::NavMode, line: usize, col: usize) -> Option<std::path::PathBuf> {
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
            self.cwd.borrow().clone()
        } else {
            match self.resolve_cwd(parent, line, col) {
                Ok(p) => p,
                Err(_) => return None,
            }
        };
        let entries = std::fs::read_dir(&parent_path).ok()?;
        let mut found: Option<std::path::PathBuf> = None;
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

    fn change_dir(&self, raw: &str, flag: Option<crate::navigation::NavMode>, line: usize, col: usize) -> RioResult<()> {
        use crate::navigation::NavMode;
        let mode = match flag {
            Some(m) => m,
            None => {
                if *self.physical_mode.borrow() {
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
            // POSIX (مثبت بفحوص باش 5.2 الحية — مرآة
            // المفسر): لا `CDPATH` حين يكون المكون
            // الأول `.` أو `..`.
            if crate::navigation::cdpath_applies(raw) {
                for entry in self.cdpath.borrow().clone() {
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
        if let Ok(canon) = self.file_policy.borrow().resolve_literal_in(
            &self.cwd.borrow(),
            raw,
            line,
            col,
        ) {
            if canon.is_dir() {
                self.set_cwd(canon);
                return Ok(());
            }
        }
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
        if let Some(corrected) = self.spell_correct(raw, mode, line, col) {
            self.set_cwd(corrected);
            return Ok(());
        }
        if let Some(e) = direct_policy {
            return Err(e);
        }
        if saw_not_dir {
            return Err(RioError::new(
                crate::error::ErrorKind::Io,
                format!("ليس مجلدا: {}", self.file_policy.borrow().display_str(raw)),
                line,
                col,
            ));
        }
        Err(RioError::new(
            crate::error::ErrorKind::Io,
            format!("المجلد غير موجود: {}", self.file_policy.borrow().display_str(raw)),
            line,
            col,
        ))
    }

    /// مهمة 5: حصة التخزين (100 ميغابايت لمجلد البرنامج)
    /// — مرآة Interpreter::check_disk_quota، بالرسالة نفسها.
    fn check_disk_quota(&self, extra_bytes: u64, line: usize, col: usize) -> RioResult<()> {
        let mut budget = 10000usize;
        let used = crate::interpreter::Interpreter::dir_size(&self.program_dir, &mut budget);
        if used.saturating_add(extra_bytes) > crate::interpreter::Interpreter::DISK_QUOTA_BYTES {
            return Err(RioError::new(
                crate::error::ErrorKind::Io,
                "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة",
                line,
                col,
            ));
        }
        Ok(())
    }

    /// Lexical assignment (Meeting 11): assign to the nearest
    /// scope that already binds the name; define locally only
    /// when the name exists nowhere. Matches the
    /// interpreter's assign_local exactly, so closures mutate
    /// their captured variables and functions can update outer
    /// variables.
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): حلّ
    // المرجع الاسمي إلى هدفه النهائي، كالمفسر.
    fn resolve_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        for _ in 0..32 {
            let next = self.namerefs.borrow().get(&current).cloned();
            match next {
                Some(target) => current = target,
                None => break,
            }
        }
        current
    }

    fn check_not_frozen(&self, name: &str, line: usize, col: usize) -> RioResult<()> {
        let resolved = self.resolve_name(name);
        if self.frozen.borrow().contains(&resolved) {
            return Err(RioError::runtime(
                format!("متغير مجمد: {}", name),
                line,
                col,
            ));
        }
        Ok(())
    }

    fn assign(&mut self, name: &str, value: Value, line: usize, col: usize) -> RioResult<()> {
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // الكتابة عبر مرجع تصل هدفه، والكتابة في
        // مجمد مرفوضة قبل أي تغيير.
        let resolved = self.resolve_name(name);
        if self.frozen.borrow().contains(&resolved) {
            return Err(RioError::runtime(
                format!("متغير مجمد: {}", name),
                line,
                col,
            ));
        }
        for scope in self.env.iter().rev() {
            if scope.borrow().contains_key(&resolved) {
                scope.borrow_mut().insert(resolved, value);
                return Ok(());
            }
        }
        self.define(&resolved, value);
        Ok(())
    }

    /// The scope index holding a name (innermost first), for
    /// in-place slot mutation (index assignment).
    fn lookup_scope_index(&self, name: &str) -> Option<usize> {
        for (i, scope) in self.env.iter().enumerate().rev() {
            if scope.borrow().contains_key(name) {
                return Some(i);
            }
        }
        None
    }

    /// Phase ك-1: build a closure value from a ClosureDef,
    /// capturing the current scope chain (shared Rc scopes).
    fn make_closure_value(&self, def: &ClosureDef) -> Value {
        Value::Function {
            name: def.name.clone(),
            params: def.params.clone(),
            body: def.body.clone(),
            module: None,
            param_types: def.param_types.clone(),
            return_type: def.return_type.clone(),
            captured: Some(self.env.clone()),
            vm_id: Some(def.id),
        }
    }

    fn child(&self, func: &FunctionDef, args: Vec<Value>) -> Vm {
        let mut scope = HashMap::new();
        for (name, val) in func.params.iter().zip(args.into_iter()) {
            scope.insert(name.clone(), val);
        }
        Vm {
            code: func.code.clone(),
            pc: 0,
            stack: Vec::new(),
            env: vec![Rc::new(RefCell::new(scope))],
            output: String::new(),
            functions: Rc::clone(&self.functions),
            closures: Rc::clone(&self.closures),
            program_dir: self.program_dir.clone(),
            file_policy: Rc::clone(&self.file_policy),
            cwd: Rc::clone(&self.cwd),
            // اجتماع 56 §71: الدالة تشارك المكدس
            // و`CDPATH` والوضع الفيزيائي مع متصلها.
            dir_stack: Rc::clone(&self.dir_stack),
            cdpath: Rc::clone(&self.cdpath),
            physical_mode: Rc::clone(&self.physical_mode),
            loaded_modules: Rc::clone(&self.loaded_modules),
            // المرحلة (ج): الدالة ترث رموز أنبوب متصلها
            // (فـ`رموز_الأنبوب()` داخل دالة ترى الحالية).
            pipe_status: self.pipe_status.clone(),
            loading_stack: Rc::clone(&self.loading_stack),
            return_value: None,
            call_depth: self.call_depth + 1,
            server_allowed: self.server_allowed,
            // اجتماع 32: الدالة ترث ملف أمها وبثها الحي —
            // فإن كانت دالة وحدة عيّن مسار الاستدعاء
            // (call_closure_value) ملف الوحدة بعد الإنشاء.
            source_file: self.source_file.clone(),
            live_output: self.live_output,
            module_files: Rc::clone(&self.module_files),
            // ك-1.2: نسخة الدالة تبدأ بلا حلقات ولا معالجات —
            // اكسر داخل جسم دالة لا يكسر حلقة المتصل أبدا.
            loop_states: Vec::new(),
            handlers: Vec::new(),
            finally_stack: Vec::new(),
            pending_flow: None,
            pending_return: None,
            // الدفعة 1: الدالة ترث رمز آخر أمر من متصلها،
            // كالمفسر (نسخة واحدة) حرفيا.
            exit_code: None,
            last_exit_code: self.last_exit_code,
            // الدفعة 4 — المتغيرات (اجتماع 58 §73):
            // الدالة تشارك المجمدات والمراجع وبدء
            // الجلسة مع متصلها.
            frozen: Rc::clone(&self.frozen),
            namerefs: Rc::clone(&self.namerefs),
            started: Rc::clone(&self.started),
        }
    }

    /// مهمة 4 (ك-1.3): نسخة لتنفيذ المستوى الأعلى لوحدة
    /// ملف: نطاق عام جديد فارغ، وجدول دوال الوحدة
    /// نفسها (لا جدول المتصل — نطاق الوحدة معزول)،
    /// والسجل المشترك للإغلاقات والوحدات والمكدس
    /// التحميلي مشترك. عمق الاستدعاء لا يزيد: تحميل
    /// الوحدة ليس استدعاء دالة، كما في المفسر.
    fn child_module(&self, compiled: CompiledProgram) -> Vm {
        Vm {
            code: compiled.code,
            pc: 0,
            stack: Vec::new(),
            env: vec![Rc::new(RefCell::new(HashMap::new()))],
            output: String::new(),
            functions: Rc::new(compiled.functions),
            closures: Rc::clone(&self.closures),
            program_dir: self.program_dir.clone(),
            file_policy: Rc::clone(&self.file_policy),
            cwd: Rc::clone(&self.cwd),
            // اجتماع 56 §71: الوحدة تشارك المكدس
            // و`CDPATH` والوضع الفيزيائي مع أمها.
            dir_stack: Rc::clone(&self.dir_stack),
            cdpath: Rc::clone(&self.cdpath),
            physical_mode: Rc::clone(&self.physical_mode),
            loaded_modules: Rc::clone(&self.loaded_modules),
            // المرحلة (ج): الوحدة ترث رموز أنبوب أمها.
            pipe_status: self.pipe_status.clone(),
            loading_stack: Rc::clone(&self.loading_stack),
            return_value: None,
            call_depth: self.call_depth,
            server_allowed: self.server_allowed,
            // اجتماع 32: نسخة الوحدة ترث بث الأم الحي؛
            // وملفها يُعيَّن بعد الإنشاء في load_module.
            source_file: self.source_file.clone(),
            live_output: self.live_output,
            module_files: Rc::clone(&self.module_files),
            loop_states: Vec::new(),
            handlers: Vec::new(),
            finally_stack: Vec::new(),
            pending_flow: None,
            pending_return: None,
            // الدفعة 1: كـ child — الوحدة ترث رمز آخر أمر.
            exit_code: None,
            last_exit_code: self.last_exit_code,
            // الدفعة 4 — المتغيرات (اجتماع 58 §73):
            // الوحدة تشارك المجمدات والمراجع وبدء
            // الجلسة مع أمها.
            frozen: Rc::clone(&self.frozen),
            namerefs: Rc::clone(&self.namerefs),
            started: Rc::clone(&self.started),
        }
    }

    /// المرحلة (د) — الإكمال (اجتماع 68 §83): نسخة
    /// لتنفيذ ملف `مصدر` — كـ child_module تماما إلا
    /// أنها تشارك سلسلة نطاقات المتصل نفسها (فتعريفات
    /// المستوى الأعلى تهبط في نطاق الجلسة وتبقى بعد
    /// عودته)، وتدمج جدول دوال الملف المترجم مع جدول
    /// المتصل. كود المتصل ومكدسه لا يُمسان أبدا (فلا
    /// يكسر `load` السطر الخارجي الجاري تقييمه).
    fn child_source(&self, compiled: CompiledProgram) -> Vm {
        let mut functions = (*self.functions).clone();
        functions.extend(compiled.functions);
        Vm {
            code: compiled.code,
            pc: 0,
            stack: Vec::new(),
            env: self.env.clone(),
            output: String::new(),
            functions: Rc::new(functions),
            closures: Rc::clone(&self.closures),
            program_dir: self.program_dir.clone(),
            file_policy: Rc::clone(&self.file_policy),
            cwd: Rc::clone(&self.cwd),
            dir_stack: Rc::clone(&self.dir_stack),
            cdpath: Rc::clone(&self.cdpath),
            physical_mode: Rc::clone(&self.physical_mode),
            loaded_modules: Rc::clone(&self.loaded_modules),
            pipe_status: self.pipe_status.clone(),
            loading_stack: Rc::clone(&self.loading_stack),
            return_value: None,
            call_depth: self.call_depth,
            server_allowed: self.server_allowed,
            source_file: self.source_file.clone(),
            live_output: self.live_output,
            module_files: Rc::clone(&self.module_files),
            loop_states: Vec::new(),
            handlers: Vec::new(),
            finally_stack: Vec::new(),
            pending_flow: None,
            pending_return: None,
            exit_code: None,
            last_exit_code: self.last_exit_code,
            frozen: Rc::clone(&self.frozen),
            namerefs: Rc::clone(&self.namerefs),
            started: Rc::clone(&self.started),
        }
    }

    /// Phase ك-1: a closure call runs in its captured
    /// environment: the captured chain, plus a fresh param
    /// scope on top — exactly like the interpreter's
    /// call_user.
    fn child_closure(
        &self,
        func: &FunctionDef,
        args: Vec<Value>,
        captured: Vec<Rc<RefCell<HashMap<String, Value>>>>,
    ) -> Vm {
        let mut child = self.child(func, args);
        let mut env = captured;
        env.append(&mut child.env);
        child.env = env;
        child
    }

    /// مهمة 3: فحص أنواع المعاملات برسائل المفسر نفسها؛
    /// `kind` هي «الدالة» أو «الطريقة».
    fn check_param_types(
        kind: &str,
        name: &str,
        params: &[String],
        param_types: &[Option<String>],
        args: &[Value],
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        for (i, arg) in args.iter().enumerate() {
            if let Some(Some(pt)) = param_types.get(i) {
                if pt != "أي" && arg.type_name() != pt {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: معامل {} في {} {} معلن {}، لكن القيمة {}",
                            params[i], kind, name, pt, arg.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        Ok(())
    }

    /// مهمة 3: فحص نوع الإرجاع برسائل المفسر نفسها: إن
    /// رجعت الدالة/الطريقة قيمة من نوع غير المعلن فخطأ؛
    /// وإن لم ترجع شيئا والمعلن ليس لاشيء ولا أي فخطأ.
    fn check_return_type(
        kind: &str,
        name: &str,
        return_type: &Option<String>,
        returned: bool,
        v: &Value,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        if let Some(rt) = return_type {
            if rt != "أي" {
                if returned {
                    if v.type_name() != rt {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: {} {} معلنة أن ترجع {}، لكن رجعت {}",
                                kind, name, rt, v.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                } else if rt != "لاشيء" {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: {} {} معلنة أن ترجع {}، لكن لم ترجع شيئا",
                            kind, name, rt
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        Ok(())
    }

    /// مهمة 3: هل نفذت الدالة ارجع؟ `return_value`
    /// معيَّنة تعني ارجع (حتى لو القيمة لاشيء)؛ وغيابها
    /// يعني انتهاء الجسم بلا ارجع — كما في المفسر
    /// Flow::Return مقابل Flow::Normal.
    fn child_return(child: &Vm) -> (bool, Value) {
        match &child.return_value {
            Some(v) => (true, v.clone()),
            None => (false, Value::Nil),
        }
    }

    /// مهمة 4 (ك-1.3): سلسلة النطاق لدالة/طريقة وحدة
    /// (module: Some): نسخة جديدة من نطاق الوحدة
    /// المخزَّن — لا السلسلة الملتقَطة — كما في المفسر
    /// تماما (call_user/call_method تدفعان نسخة من
    /// نطاق الوحدة في كل استدعاء). فإن لم تكن الدالة
    /// لوحدة، أو لم تكن الوحدة محمّلة (دفاعيا فقط)،
    /// فلا شيء — ويستخدم المتصل السلسلة الملتقَطة.
    fn module_scope_chain(
        &self,
        module: &Option<String>,
    ) -> Option<Vec<Rc<RefCell<HashMap<String, Value>>>>> {
        let name = module.as_ref()?;
        let scope = match self.loaded_modules.borrow().get(name) {
            Some(Value::Module { scope, .. }) => scope.clone(),
            _ => return None,
        };
        Some(vec![Rc::new(RefCell::new(scope))])
    }

    /// مهمة 4: قراءة ملف وحدة بسقف 5 ميغابايت، كما في
    /// المفسر حرفيا (السقف يُفرَض أثناء القراءة،
    /// والملف الأكبر أو غير UTF-8 خطأ عربي نظيف).
    fn read_module_file(
        &self,
        path: &std::path::Path,
        display: &str,
        line: usize,
        col: usize,
    ) -> RioResult<String> {
        use std::io::Read;
        let cap = crate::interpreter::Interpreter::MAX_READ_BYTES;
        let mut f = std::fs::File::open(path).map_err(|e| {
            RioError::new(
                crate::error::ErrorKind::Io,
                format!("تعذر قراءة الملف {}: {}", display, e),
                line,
                col,
            )
        })?;
        let mut buf = Vec::new();
        (&mut f)
            .take((cap + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| {
                RioError::new(
                    crate::error::ErrorKind::Io,
                    format!("تعذر قراءة الملف {}: {}", display, e),
                    line,
                    col,
                )
            })?;
        if buf.len() > cap {
            return Err(RioError::new(
                crate::error::ErrorKind::Io,
                format!("الملف {} أكبر من الحد الأقصى (5 ميغابايت)", display),
                line,
                col,
            ));
        }
        String::from_utf8(buf).map_err(|_| {
            RioError::new(
                crate::error::ErrorKind::Io,
                format!("الملف {} ليس نصا صالحا (UTF-8)", display),
                line,
                col,
            )
        })
    }

    /// مهمة 4 (ك-1.3): تحميل وحدة بالاسم، مرة واحدة في
    /// التشغيل — مرآة load_module في المفسر: الوحدات
    /// الأصلية أولا (لا يظللها ملف أبدا)، ثم ترتيب
    /// الاستيراد في التصميم §8 (`حزم/<اسم>/`، ثم مجلد
    /// البرنامج، ثم المخزن العام — قفل ثم أحدث)، مع
    /// كشف الدورات وتصدير فقط، وتوسيم دوال المستوى
    /// الأعلى باسم الوحدة.
    fn load_module(&mut self, module: &str, line: usize, col: usize) -> RioResult<Value> {
        if let Some(v) = self.loaded_modules.borrow().get(module) {
            return Ok(v.clone());
        }
        if let Some(v) = crate::stdlib::native_module(module) {
            self.loaded_modules
                .borrow_mut()
                .insert(module.to_string(), v.clone());
            return Ok(v);
        }
        if self.loading_stack.borrow().contains(&module.to_string()) {
            let mut cycle = self.loading_stack.borrow().clone();
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
        // مرآة المفسر حرفيا (نفس الدالة المشتركة).
        let found =
            crate::packages::find_module_file(&self.program_dir, module).map_err(|msg| {
                RioError::new(crate::error::ErrorKind::Io, msg, line, col)
            })?;
        // المدير المدمج (أمر المالك 2026-11-10 ~03:07):
        // ملاذ أخير بعد فشل بحث القرص كله — كالمفسر
        // حرفيا، فالعبارة `حدث حزم` تعمل لأي مستخدم
        // بلا تثبيت مسبق.
        // اجتماع 32، السؤال الخامس: ملف الوحدة هو الذي
        // تُسنَد إليه أخطاؤها — كالمفسر حرفيا.
        let (source, display) = match found.found {
            Some(p) => {
                let src = self.read_module_file(&p, module, line, col)?;
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
            .borrow_mut()
            .insert(module.to_string(), display.clone());
        let tokens = crate::lexer::tokenize_named(&source, &display).map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        let mut parser = crate::parser::Parser::named(tokens, &display);
        let program = parser.parse_program().map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        // Collect export names from top-level Export statements
        // only — only exported names are accessible outside.
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
        let compiled = Compiler::compile_program(&program).map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        // Merge the module's closures into the shared registry
        // BEFORE running it, so the module's own code (and any
        // later caller) finds every body by id.
        self.closures
            .borrow_mut()
            .extend(compiled.closures.clone());
        self.loading_stack.borrow_mut().push(module.to_string());
        let mut child = self.child_module(compiled);
        // اجتماع 32: نسخة الوحدة تنفذ بملفها، فتُسنَد
        // أخطاء مستواها الأعلى إليه (البث الحي موروث).
        child.source_file = Some(display.clone());
        let exec_result = child.run();
        let module_scope = child
            .env
            .first()
            .map(|s| s.borrow().clone())
            .unwrap_or_default();
        // Partial output survives even when the module fails,
        // like the interpreter's single shared output.
        self.output.push_str(&child.output);
        // الدفعة 1: خروج أو رمز آخر أمر من الوحدة يصعدان
        // إلى المستورد (كالمفسر — نسخة واحدة).
        if let Some(code) = child.exit_code {
            self.exit_code = Some(code);
        }
        self.last_exit_code = child.last_exit_code;
        self.loading_stack.borrow_mut().pop();
        exec_result.map_err(|e| {
            e.with_prefix(&format!("في وحدة {}: ", module))
                .with_file_if_none(Some(display.clone()))
        })?;
        // Mark top-level functions with the module name, like
        // the interpreter: a Function whose captured chain is
        // exactly the module scope (length 1) is a top-level
        // definition. Re-exported functions are re-marked
        // with THIS module's name — the interpreter does the
        // same (its calls then push this module's scope).
        let mut full_scope = module_scope;
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
        }
        let mod_value = Value::Module {
            name: module.to_string(),
            exports,
            scope: full_scope,
        };
        self.loaded_modules
            .borrow_mut()
            .insert(module.to_string(), mod_value.clone());
        Ok(mod_value)
    }

    /// Phase ك-1: call a closure value (a Value::Function
    /// with vm_id). Arity, param types and depth mirror the
    /// interpreter's call_user (same messages); the VM depth
    /// message keeps its "في VM" suffix, like Op::Call.
    /// مهمة 3: نوع الإرجاع يُفحص هنا أيضا، برسائل
    /// المفسر نفسها.
    fn call_closure_value(
        &mut self,
        func: Value,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let (name, params, param_types, return_type, captured, vm_id, module) = match func {
            Value::Function { name, params, param_types, return_type, captured, vm_id, module, .. } => {
                (name, params, param_types, return_type, captured, vm_id, module)
            }
            _ => unreachable!(),
        };
        let id = match vm_id {
            Some(id) => id,
            None => {
                return Err(RioError::runtime(
                    format!("القيمة {} ليست دالة قابلة للاستدعاء في VM", name),
                    line,
                    col,
                ));
            }
        };
        let def = match self.closures.borrow().get(&id) {
            Some(d) => d.clone(),
            None => {
                return Err(RioError::runtime(
                    format!("دالة غير معرفة في VM: {}", name),
                    line,
                    col,
                ));
            }
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
        Self::check_param_types("الدالة", &name, &params, &param_types, &args, line, col)?;
        // Same depth limit as the interpreter (Meeting 15).
        if self.call_depth >= crate::interpreter::MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    crate::interpreter::MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        // A closure without a captured chain (defensive: the
        // VM always captures) runs in an empty top scope.
        // مهمة 4: دالة وحدة تعمل في نسخة نطاق الوحدة
        // (لا سلسلتها الملتقَطة) — انظر module_scope_chain.
        let captured = match self.module_scope_chain(&module) {
            Some(chain) => chain,
            None => captured.unwrap_or_default(),
        };
        let mut child_vm = self.child_closure(&def, args, captured);
        // اجتماع 32، السؤال الخامس: دالة وحدة تُنفَّذ
        // بملف وحدتها — فإن كانت بلا وحدة ورثت ملف أمها
        // (child_closure) كما هو.
        if let Some(m) = &module {
            if let Some(f) = self.module_files.borrow().get(m).cloned() {
                child_vm.source_file = Some(f);
            }
        }
        match child_vm.run() {
            Ok(()) => {
                self.output.push_str(&child_vm.output);
                // الدفعة 1: خروج أو رمز آخر أمر من داخل
                // الدالة يصعدان إلى المتصل (كالمفسر).
                if let Some(code) = child_vm.exit_code {
                    self.exit_code = Some(code);
                }
                self.last_exit_code = child_vm.last_exit_code;
                let (returned, v) = Self::child_return(&child_vm);
                Self::check_return_type("الدالة", &name, &return_type, returned, &v, line, col)?;
                Ok(v)
            }
            Err(e) => {
                // Preserve partial output, like the interpreter.
                self.output.push_str(&child_vm.output);
                Err(e)
            }
        }
    }

    /// مهمة 3: استدعاء طريقة مربوطة بقيمتها (لا باسم
    /// فئتها): الفحص والدلالة كـ call_method في المفسر —
    /// عدد المعاملات وأنواعها وحد العمق، ثم التشغيل في
    /// سلسلة النطاق المعرِّف الملتقَطة + نطاق المعاملات
    /// + نفس، ثم فحص نوع الإرجاع برسائل المفسر نفسها.
    /// (كانت الطريقة تُبحث باسم فئتها في جدول VM
    /// فلا ترى نطاق تعريفها أصلا، ولا تُفحص أنواعها.)
    fn call_method_value(
        &mut self,
        func: Value,
        instance: Rc<crate::value::InstanceData>,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let (name, params, param_types, return_type, captured, vm_id, module) = match func {
            Value::Function { name, params, param_types, return_type, captured, vm_id, module, .. } => {
                (name, params, param_types, return_type, captured, vm_id, module)
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
        Self::check_param_types("الطريقة", &name, &params, &param_types, &args, line, col)?;
        if self.call_depth >= crate::interpreter::MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    crate::interpreter::MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        let id = match vm_id {
            Some(id) => id,
            None => {
                return Err(RioError::runtime(
                    format!("القيمة {} ليست دالة قابلة للاستدعاء في VM", name),
                    line,
                    col,
                ));
            }
        };
        let def = match self.closures.borrow().get(&id) {
            Some(d) => d.clone(),
            None => {
                return Err(RioError::runtime(
                    format!("دالة غير معرفة في VM: {}", name),
                    line,
                    col,
                ));
            }
        };
        // The method runs in its captured chain (like a
        // closure), with نفس bound in the param scope —
        // exactly like the interpreter's call_method.
        // مهمة 4: طريقة وحدة تعمل في نسخة نطاق الوحدة
        // (لا سلسلتها الملتقَطة) — انظر module_scope_chain.
        let captured = match self.module_scope_chain(&module) {
            Some(chain) => chain,
            None => captured.unwrap_or_default(),
        };
        let mut child_vm = self.child_closure(&def, args, captured);
        child_vm.define("نفس", Value::Instance(instance));
        // اجتماع 32، السؤال الخامس: كـ call_closure_value —
        // طريقة وحدة تُنفَّذ بملف وحدتها.
        if let Some(m) = &module {
            if let Some(f) = self.module_files.borrow().get(m).cloned() {
                child_vm.source_file = Some(f);
            }
        }
        match child_vm.run() {
            Ok(()) => {
                self.output.push_str(&child_vm.output);
                // الدفعة 1: كـ call_closure_value — خروج
                // أو رمز آخر أمر من داخل الطريقة يصعدان.
                if let Some(code) = child_vm.exit_code {
                    self.exit_code = Some(code);
                }
                self.last_exit_code = child_vm.last_exit_code;
                let (returned, v) = Self::child_return(&child_vm);
                Self::check_return_type("الطريقة", &name, &return_type, returned, &v, line, col)?;
                Ok(v)
            }
            Err(e) => {
                self.output.push_str(&child_vm.output);
                Err(e)
            }
        }
    }

    /// مهمة 3: استدعاء قيمة دالة عامة، كما في المفسر:
    /// الدالة ← call_user، والطريقة المربوطة ←
    /// call_method، والأصلية ← المؤشر مباشرة.
    fn call_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        match callee {
            Value::Function { .. } => self.call_closure_value(callee, args, line, col),
            Value::BoundMethod { func, instance } => {
                self.call_method_value(*func, instance, args, line, col)
            }
            Value::NativeFunction { func, .. } => func(self, &args, line, col),
            other => Err(RioError::typ(
                format!("القيمة {} ليست دالة", other.type_name()),
                line,
                col,
            )),
        }
    }

    /// مهمة 3: طرق النصوص والقوائم المدمجة في VM، مطابقة
    /// لـ call_builtin_method في المفسر (الرسائل نفسها):
    /// نصوص خالصة ترجع جديدا؛ وقوائم أضف/أزل تعدِّل
    /// وتكتبان رجعيا في متغير عادي عبر التعيين المعجمي،
    /// وغير المتغير العادي خطأ قبل أي تعديل.
    fn call_builtin_method_vm(
        &mut self,
        target: Value,
        name: &str,
        var: Option<&str>,
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
        match &target {
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
                    // §81): قصّ بالحروف — مرآة طريقة
                    // المفسر نفسها.
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
                    // §81): استبدال أول ظهور فقط — مرآة
                    // طريقة المفسر نفسها.
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
                        let var = match var {
                            Some(v) => v,
                            None => {
                                return Err(RioError::runtime(
                                    "طريقة معدلة للقائمة (أضف/أزل) تحتاج متغيرا عاديا، لا نتيجة تعبير",
                                    line, col,
                                ))
                            }
                        };
                        items.push(args[0].clone());
                        self.assign(var, Value::List(items), line, col)?;
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
                        let var = match var {
                            Some(v) => v,
                            None => {
                                return Err(RioError::runtime(
                                    "طريقة معدلة للقائمة (أضف/أزل) تحتاج متغيرا عاديا، لا نتيجة تعبير",
                                    line, col,
                                ))
                            }
                        };
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
                        self.assign(var, Value::List(items), line, col)?;
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

    fn pop(&mut self, line: usize, col: usize) -> RioResult<Value> {
        self.stack.pop().ok_or_else(|| {
            RioError::runtime("مكدس VM فارغ", line, col)
        })
    }

    fn pop_bool(&mut self, line: usize, col: usize) -> RioResult<bool> {
        match self.pop(line, col)? {
            Value::Bool(b) => Ok(b),
            other => Err(RioError::typ(
                format!("الشرط يجب أن يكون منطقيا، وليس {}", other.type_name()),
                line,
                col,
            )),
        }
    }

    /// Slice 4: the 9 pure builtins, mirroring the interpreter's
    /// call_builtin exactly (same messages, same arity errors).
    fn call_pure_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
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
                    return Err(RioError::runtime("مدى تتوقع 1 إلى 3 معاملات", line, col));
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
                    if out.len() > crate::interpreter::MAX_LOOP_ITERATIONS {
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
                            RioError::typ(format!("لا يمكن تحويل النص إلى عدد: {}", s), line, col)
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
            _ => Err(RioError::runtime(
                format!("دالة مدمجة غير معروفة في VM: {}", name),
                line,
                col,
            )),
        }
    }

    /// شغّل ملف أداة في جلسة جديدة (الدفعة 3 —
    /// التشغيل، الاجتماع 57 §72، الميزة 51):
    /// مرآة `exec_tool` في المفسر — نفس البحث
    /// (الميزة 39) ونفس الرموز. راية الشبكة تُقرأ
    /// من البوابة العامة (تزامنها الجلسة قبل كل
    /// استدعاء مدمجة، فتعكس الجلسة الحالية)؛ وباقي
    /// الأعلام من حقول VM نفسها.
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
                    crate::error::ErrorKind::Io,
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
        let program = match crate::parser::Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return Ok(fail(e.to_string())),
        };
        let mut analyzer = crate::semantic::SemanticAnalyzer::new();
        if let Err(e) = analyzer.analyze_program(&program) {
            return Ok(fail(e.to_string()));
        }
        let mut session = VmSession::new(self.program_dir.clone());
        session.set_trusted(self.file_policy.borrow().trusted);
        session.set_network_allowed(crate::network::network_allowed());
        session.set_server_allowed(self.server_allowed);
        session.set_live_output(self.live_output);
        session.set_source_file(file_name);
        match session.run(&program) {
            Ok(()) => Ok((
                session.take_output(),
                session.take_exit_code().unwrap_or(0),
            )),
            Err(e) => {
                let mut out = session.take_output();
                out.push_str(&format!("{}\n", e));
                // الدفعة 1 (§69 ممتد): وحدة غائبة
                // 127، وخطأ تشغيل آخر → 1.
                let code = if e.is_module_not_found() { 127 } else { 1 };
                Ok((out, code))
            }
        }
    }

    /// مهمة 5 (ك-1.4): دوال الملفات + ادخل في VM —
    /// مرآة أذرع call_builtin في المفسر نفسها (الرسائل
    /// والأعداد القصوى والقواعد نفسها)، عبر المساعدات
    /// المشتركة في Interpreter (قراءة محدودة 5 ميغابايت،
    /// بحث، شجرة، حجم، عدّ، تطبيع). الاختلاف الوحيد:
    /// resolve_cwd / check_disk_quota نسختان محليتان
    /// في VM تستخدمان file_policy وcwd وprogram_dir
    /// الخاصين بها.
    fn call_file_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        // Meeting 17 (WASM): كما في المفسر — لا نظام ملفات
        // ولا stdin في المتصفح، فهذه الدوال خاملة بخطأ
        // عربي نظيف بدلا من لمس std::fs / stdin إطلاقا.
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
            "اقرأ_ملف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let display = self.file_policy.borrow().display_str(path);
                        crate::interpreter::Interpreter::read_file_capped(
                            &resolved, &display, line, col,
                        )
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
                        // counts its size difference).
                        let existing =
                            std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
                        let extra = (content.len() as u64).saturating_sub(existing);
                        self.check_disk_quota(extra, line, col)?;
                        // اجتماع 45 §59 (47): كتابة ذرية.
                        crate::install::write_atomic(&resolved, content.as_bytes())
                            .map(|_| Value::Nil)
                            .map_err(|e| {
                                RioError::new(
                                    crate::error::ErrorKind::Io,
                                    format!("تعذر كتابة الملف {}: {}", self.file_policy.borrow().display_str(path), e),
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
            "اعرض_الملفات" => {
                // المرحلة (د) — الإكمال (اجتماع 69 §84):
                // مسار اختياري وأعلام نصية — مرآة ذراع
                // المفسر نفسه.
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
                        crate::error::ErrorKind::Io,
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
                            crate::error::ErrorKind::Io,
                            format!("تعذر عرض المجلد: {}", e),
                            line, col,
                        )
                    })?;
                    for entry in rd {
                        let entry = entry.map_err(|e| {
                            RioError::new(
                                crate::error::ErrorKind::Io,
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
                            crate::interpreter::Interpreter::human_size(e.size)
                        } else {
                            Value::Number(e.size as f64).fmt_value()
                        };
                        out.push(Value::Str(format!(
                            "{} {} {} {}",
                            kind,
                            size,
                            crate::interpreter::Interpreter::format_unix_utc(e.mtime),
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
                    let more = Value::Number((total - LIST_CAP) as f64).fmt_value();
                    out.push(Value::Str(format!("…و {} أخرى", more)));
                }
                Ok(Value::List(out))
            }
            "المجلد_الحالي" => {
                // اجتماع 56 §71 (الميزتان 9 و12) — مرآة
                // ذراع المفسر نفسه: 0 أو 1 معاملات،
                // وبلا معامل حسب الوضع العام للجلسة.
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
                // فقط — مرآة ذراع المفسر نفسه.
                match &args[0] {
                    Value::Str(name) => {
                        // اجتماع 56 §71 (الميزة 4): كشف
                        // `PWD` — `بيئة("المجلد_الحالي")`
                        // قراءة ديناميكية بصيغة العرض حسب
                        // الوضع الحالي؛ والدفعة 4 (اجتماع
                        // 58 §73): باقي الديناميكية
                        // بالمساعد نفسه.
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
            // بديل البيئة والبديل المشروط — مرآة أذرع
            // المفسر نفسها.
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
            // إزالة البادئة/اللاحقة الحرفية فقط — مرآة
            // أذرع المفسر نفسها.
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
            // متغيرات الجلسة «الاسم=القيمة» — مرآة ذراع
            // المفسر نفسه (المجمد يُعلَّم، والمرجع
            // الاسمي يُعرض هدفه).
            "اعرض_المتغيرات" => {
                arity(0)?;
                let mut names = self.session_variable_names();
                names.sort();
                let mut out = Vec::new();
                for name in names {
                    if let Some(target) = self.namerefs.borrow().get(name.as_str()) {
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
                        if self.frozen.borrow().contains(&resolved) {
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
            // حذف دالة معرفة من أقرب ربط — مرآة ذراع
            // المفسر نفسه.
            "احذف_دالة" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        if self.namerefs.borrow().contains_key(name.as_str()) {
                            return Err(RioError::runtime(
                                format!("ليست دالة: {}", name),
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_name(name);
                        if self.frozen.borrow().contains(&resolved) {
                            return Err(RioError::runtime(
                                format!("دالة مجمدة: {}", name),
                                line,
                                col,
                            ));
                        }
                        let mut found = false;
                        let mut is_function = false;
                        for scope in self.env.iter().rev() {
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
                        for scope in self.env.iter().rev() {
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
            // في نسخة مصدر تشارك النطاقات (فتبقى
            // التعريفات بعد عودته)؛ والمخرج ورمز الخروج
            // يصعدان إلى المتصل كالوحدة؛ والدورة خطأ صريح.
            "مصدر" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let (resolved, display) =
                            self.resolve_source_file(path, line, col)?;
                        if self.loading_stack.borrow().contains(&display) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("الملف يُستدعى دوريا: {}", path),
                                line,
                                col,
                            ));
                        }
                        let source = crate::interpreter::Interpreter::read_file_capped(
                            &resolved, &display, line, col,
                        )?;
                        let tokens = crate::lexer::tokenize_named(&source, &display)?;
                        let program =
                            crate::parser::Parser::named(tokens, &display).parse_program()?;
                        let compiled = Compiler::compile_program(&program)?;
                        self.closures
                            .borrow_mut()
                            .extend(compiled.closures.clone());
                        self.loading_stack.borrow_mut().push(display.clone());
                        let mut child = self.child_source(compiled);
                        child.source_file = Some(display.clone());
                        let result = child.run();
                        self.loading_stack.borrow_mut().pop();
                        self.output.push_str(&child.output);
                        if let Some(code) = child.exit_code {
                            self.exit_code = Some(code);
                        }
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
            // البيئة ومفاتيح القاموس — مرآة أذرع
            // المفسر نفسها.
            "جمد" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(name) => {
                        let resolved = self.resolve_name(name);
                        if self.lookup(&resolved).is_none() {
                            return Err(RioError::runtime(
                                format!("متغير غير معرف: {}", name),
                                line,
                                col,
                            ));
                        }
                        self.frozen.borrow_mut().insert(resolved);
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
                        if self.namerefs.borrow().contains_key(name.as_str()) {
                            self.namerefs.borrow_mut().remove(name.as_str());
                            return Ok(Value::Nil);
                        }
                        let resolved = self.resolve_name(name);
                        if self.frozen.borrow().contains(&resolved) {
                            return Err(RioError::runtime(
                                format!("متغير مجمد: {}", name),
                                line,
                                col,
                            ));
                        }
                        for scope in self.env.iter().rev() {
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
                        if let Some(target) =
                            self.namerefs.borrow().get(name.as_str()).cloned()
                        {
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
                                if self.frozen.borrow().contains(&resolved) {
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
                if self.lookup(&new_name).is_some()
                    || self.namerefs.borrow().contains_key(new_name.as_str())
                {
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
                let mut current = target.clone();
                for _ in 0..32 {
                    if current == new_name {
                        return Err(RioError::runtime(
                            "مرجع دائري".to_string(),
                            line,
                            col,
                        ));
                    }
                    match self.namerefs.borrow().get(&current).cloned() {
                        Some(next) => current = next,
                        None => break,
                    }
                }
                self.namerefs.borrow_mut().insert(new_name, target);
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
            // الدفعة 1 — رموز الخروج (اجتماع 54 §69)،
            // مرآة أذرع المفسر نفسها. `خروج` تضبط الراية
            // فقط وترجع لاشيء؛ وحلقة run_inner تفكّها
            // (تشغّل أخيرا ولا يمسكها ماعدا).
            "رمز_الخروج" => {
                arity(0)?;
                Ok(Value::Number(self.last_exit_code as f64))
            }
            // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
            // و63 §77 و§78)، مرآة أذرع المفسر نفسها.
            "رموز_الأنبوب" => {
                arity(0)?;
                Ok(Value::List(
                    self.pipe_status
                        .iter()
                        .map(|c| Value::Number(*c as f64))
                        .collect(),
                ))
            }
            "اقرأ_الإدخال" => {
                arity(0)?;
                let mut text = String::new();
                while let Some(line) = crate::stdin_pump::stdin_line() {
                    text.push_str(&line);
                }
                Ok(Value::Str(text))
            }
            "نجاح" => {
                arity(0)?;
                Ok(Value::Bool(true))
            }
            "فشل" => {
                arity(0)?;
                Err(RioError::runtime("فشل", line, col))
            }
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
                self.exit_code = Some(code);
                Ok(Value::Nil)
            }
            // الدفعة 3 — التشغيل (اجتماع 57 §72):
            // وصف الأمر ومكان الأداة والاستبدال —
            // مرآة أذرع المفسر نفسها.
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
                            // لا ملف أداة — لاشيء لا خطأ.
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
                // اجتماع 56 §71 — الدفعة 2 (التنقل): مرآة
                // ذراع المفسر نفسه — 0 أو 2 معاملات؛ بلا
                // معاملات → الأب (`..`)؛ و`-` اسم عادي
                // غير موجود؛ والصامت في كل الصور.
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
            // المجلدات — مرآة أذرع المفسر نفسها.
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
                    None => {
                        if self.dir_stack.borrow().is_empty() {
                            return Err(RioError::runtime("المكدس فارغ", line, col));
                        }
                        let top = self.dir_stack.borrow()[0].clone();
                        self.dir_stack.borrow_mut()[0] = self.cwd.borrow().clone();
                        *self.cwd.borrow_mut() = top;
                        self.after_stack_move();
                        Ok(Value::Nil)
                    }
                    Some(Value::Str(s)) => {
                        if let Some(idx) = crate::navigation::parse_arabic_int(s) {
                            if let Some(pos) = crate::navigation::resolve_stack_index(
                                idx,
                                self.dir_stack.borrow().len(),
                            ) {
                                {
                                    let mut cwd = self.cwd.borrow_mut();
                                    let mut stack = self.dir_stack.borrow_mut();
                                    crate::navigation::rotate_stack(&mut cwd, &mut stack, pos);
                                }
                                self.after_stack_move();
                                return Ok(Value::Nil);
                            }
                        }
                        let old = self.cwd.borrow().clone();
                        self.change_dir(s, None, line, col)?;
                        self.dir_stack.borrow_mut().insert(0, old);
                        Ok(Value::Nil)
                    }
                    Some(v) => match crate::navigation::parse_stack_index(v) {
                        Some(idx) => {
                            let pos = match crate::navigation::resolve_stack_index(
                                idx,
                                self.dir_stack.borrow().len(),
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
                            {
                                let mut cwd = self.cwd.borrow_mut();
                                let mut stack = self.dir_stack.borrow_mut();
                                crate::navigation::rotate_stack(&mut cwd, &mut stack, pos);
                            }
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
                    None => {
                        if self.dir_stack.borrow().is_empty() {
                            return Err(RioError::runtime("المكدس فارغ", line, col));
                        }
                        let target = self.dir_stack.borrow()[0].clone();
                        match std::fs::metadata(&target) {
                            Ok(m) if m.is_dir() => {
                                self.dir_stack.borrow_mut().remove(0);
                                *self.cwd.borrow_mut() =
                                    crate::interpreter::Interpreter::normalize_lexical(&target);
                                self.after_stack_move();
                                Ok(Value::Nil)
                            }
                            Ok(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!(
                                    "ليس مجلدا: {}",
                                    self.file_policy.borrow().display_path(&target)
                                ),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!(
                                    "المجلد غير موجود: {}",
                                    self.file_policy.borrow().display_path(&target)
                                ),
                                line,
                                col,
                            )),
                        }
                    }
                    Some(v) => match crate::navigation::parse_stack_index(v) {
                        Some(idx) => {
                            let pos = match crate::navigation::resolve_stack_index(
                                idx,
                                self.dir_stack.borrow().len(),
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
                            self.dir_stack.borrow_mut().remove(pos);
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
                let mut out = vec![Value::Str(
                    self.file_policy.borrow().display_path(&self.cwd.borrow()),
                )];
                for p in self.dir_stack.borrow().iter() {
                    out.push(Value::Str(self.file_policy.borrow().display_path(p)));
                }
                Ok(Value::List(out))
            }
            "امسح_المكدس" => {
                arity(0)?;
                self.dir_stack.borrow_mut().clear();
                Ok(Value::Nil)
            }
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
                        self.cdpath.borrow().iter().cloned().map(Value::Str).collect(),
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
                        *self.cdpath.borrow_mut() = new;
                        Ok(Value::List(
                            self.cdpath.borrow().iter().cloned().map(Value::Str).collect(),
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
                    None => Ok(Value::Bool(*self.physical_mode.borrow())),
                    Some(Value::Bool(b)) => {
                        *self.physical_mode.borrow_mut() = *b;
                        Ok(Value::Bool(*self.physical_mode.borrow()))
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
                        crate::error::ErrorKind::Io,
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
                                    crate::error::ErrorKind::Io,
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
                let missing = crate::files::missing_components(&resolved);
                std::fs::create_dir_all(&resolved).map_err(|e| {
                    RioError::new(
                        crate::error::ErrorKind::Io,
                        format!(
                            "تعذر إنشاء المجلد {}: {}",
                            self.file_policy.borrow().display_str(&dir),
                            e
                        ),
                        line,
                        col,
                    )
                })?;
                if let Some(m) = mode {
                    if missing.last().map(|p| p == &resolved).unwrap_or(false) {
                        crate::files::set_mode(&resolved, m).map_err(|e| {
                            RioError::new(
                                crate::error::ErrorKind::Io,
                                format!(
                                    "تعذر ضبط وضع المجلد {}: {}",
                                    self.file_policy.borrow().display_str(&dir),
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
                // علم ثان اختياري «تجاهل_الغائب» — مرآة
                // ذراع المفسر نفسه.
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
                        if let Some(msg) = crate::files::root_refusal_text(path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                msg,
                                line,
                                col,
                            ));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd.borrow(), path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                            crate::error::ErrorKind::Io,
                                            format!(
                                                "المجلد غير فارغ: {} — استخدم احذف_تكراري",
                                                self.file_policy.borrow().display_str(path)
                                            ),
                                            line,
                                            col,
                                        )
                                    } else {
                                        RioError::new(
                                            crate::error::ErrorKind::Io,
                                            format!("تعذر حذف {}: {}", self.file_policy.borrow().display_str(path), e),
                                            line,
                                            col,
                                        )
                                    }
                                })
                            }
                            Ok(_) => {
                                std::fs::remove_file(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        crate::error::ErrorKind::Io,
                                        format!("تعذر حذف {}: {}", self.file_policy.borrow().display_str(path), e),
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
                                        crate::error::ErrorKind::Io,
                                        format!("المسار غير موجود: {}", self.file_policy.borrow().display_str(path)),
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
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                msg,
                                line,
                                col,
                            ));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd.borrow(), path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                            crate::error::ErrorKind::Io,
                                            format!(
                                                "المجلد غير فارغ: {} — استخدم احذف_تكراري",
                                                self.file_policy.borrow().display_str(path)
                                            ),
                                            line,
                                            col,
                                        )
                                    } else {
                                        RioError::new(
                                            crate::error::ErrorKind::Io,
                                            format!("تعذر حذف المجلد {}: {}", self.file_policy.borrow().display_str(path), e),
                                            line,
                                            col,
                                        )
                                    }
                                })
                            }
                            Ok(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("ليس مجلدا: {}", self.file_policy.borrow().display_str(path)),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.borrow().display_str(path)),
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
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                msg,
                                line,
                                col,
                            ));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd.borrow(), path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        match std::fs::metadata(&resolved) {
                            Ok(m) if !m.is_dir() => {
                                std::fs::remove_file(&resolved).map(|_| Value::Nil).map_err(|e| {
                                    RioError::new(
                                        crate::error::ErrorKind::Io,
                                        format!("تعذر حذف الملف {}: {}", self.file_policy.borrow().display_str(path), e),
                                        line,
                                        col,
                                    )
                                })
                            }
                            Ok(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("ليس ملفا: {}", self.file_policy.borrow().display_str(path)),
                                line,
                                col,
                            )),
                            Err(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("المسار غير موجود: {}", self.file_policy.borrow().display_str(path)),
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
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                msg,
                                line,
                                col,
                            ));
                        }
                        // فحص قانوني قبل الحل: الرابط الرمزي
                        // إلى الجذر يرفضه الساندبوكس برسالة
                        // عامة، والقرار يطلب رسالة الجذر.
                        if crate::files::delete_target_is_root(&self.cwd.borrow(), path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن حذف الجذر: /",
                                line,
                                col,
                            ));
                        }
                        let resolved = self.resolve_cwd_for_write(path, line, col)?;
                        if crate::files::is_filesystem_root(&resolved) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                        crate::error::ErrorKind::Io,
                                        format!(
                                            "تعذر حذف {}: {}",
                                            self.file_policy.borrow().display_str(path),
                                            e
                                        ),
                                        line,
                                        col,
                                    )
                                }),
                            Err(_) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!(
                                    "المسار غير موجود: {}",
                                    self.file_policy.borrow().display_str(path)
                                ),
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
                // علم ثالث اختياري «لا_تكتب» — مرآة ذراع
                // المفسر نفسه.
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
                                crate::error::ErrorKind::Io,
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
                                crate::error::ErrorKind::Io,
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
                                crate::error::ErrorKind::Io,
                                format!("تعذر نسخ {}: {}", self.file_policy.borrow().display_str(src), e),
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
                        if dst_path.is_dir() {
                            if let Some(name) = src_path.file_name() {
                                dst_path.push(name);
                            }
                        }
                        let src_is_dir = std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        if src_is_dir {
                            if crate::files::dst_inside_src(&src_path, &dst_path) {
                                return Err(RioError::new(
                                    crate::error::ErrorKind::Io,
                                    "لا يمكن نسخ المجلد داخل نفسه",
                                    line,
                                    col,
                                ));
                            }
                        } else if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.borrow().display_str(src),
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
                        if std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                        {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري",
                                line,
                                col,
                            ));
                        }
                        if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن نسخ الملف على نفسه",
                                line,
                                col,
                            ));
                        }
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
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.borrow().display_str(src),
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
                        let src_is_dir = std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        if src_is_dir {
                            if crate::files::dst_inside_src(&src_path, &dst_path) {
                                return Err(RioError::new(
                                    crate::error::ErrorKind::Io,
                                    "لا يمكن نسخ المجلد داخل نفسه",
                                    line,
                                    col,
                                ));
                            }
                        } else if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.borrow().display_str(src),
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
                        if std::fs::metadata(&src_path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                        {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                "لا يمكن نسخ المجلد بلا خيار تكراري — استخدم انسخ_تكراري",
                                line,
                                col,
                            ));
                        }
                        if crate::files::paths_same_file(&src_path, &dst_path) {
                            return Err(RioError::new(
                                crate::error::ErrorKind::Io,
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
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "تعذر نسخ {}: {}",
                                        self.file_policy.borrow().display_str(src),
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
                                crate::error::ErrorKind::Io,
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
                                        crate::error::ErrorKind::Io,
                                        format!(
                                            "تعذر نقل {}: {}",
                                            self.file_policy.borrow().display_str(src),
                                            err
                                        ),
                                        line,
                                        col,
                                    )),
                                    Err(crate::files::MoveError::Remove(err)) => Err(RioError::new(
                                        crate::error::ErrorKind::Io,
                                        format!(
                                            "نُسخ {} إلى {} لكن تعذر حذف المصدر: {}",
                                            self.file_policy.borrow().display_str(src),
                                            self.file_policy.borrow().display_str(dst),
                                            err
                                        ),
                                        line,
                                        col,
                                    )),
                                }
                            }
                            Err(e) => Err(RioError::new(
                                crate::error::ErrorKind::Io,
                                format!("تعذر نقل {}: {}", self.file_policy.borrow().display_str(src), e),
                                line,
                                col,
                            )),
                        }
                    }
                    _ => Err(RioError::typ("انقل يتطلب مسارين نصيين", line, col)),
                }
            }
            "أنشئ_ملف" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        if !resolved.exists() {
                            // اجتماع 45 §59 (47): كتابة ذرية.
                            crate::install::write_atomic(&resolved, b"").map_err(|e| {
                                RioError::new(
                                    crate::error::ErrorKind::Io,
                                    format!("تعذر إنشاء الملف {}: {}", self.file_policy.borrow().display_str(path), e),
                                    line,
                                    col,
                                )
                            })?;
                        } else {
                            // الاجتماع 59 §74 (الميزة 215):
                            // الموجود يحدَّث وقته بلا مساس
                            // بالمحتوى (`touch`).
                            let resolved_w = self.resolve_cwd_for_write(path, line, col)?;
                            crate::files::touch_times(&resolved_w).map_err(|e| {
                                RioError::new(
                                    crate::error::ErrorKind::Io,
                                    format!("تعذر تحديث وقت {}: {}", self.file_policy.borrow().display_str(path), e),
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
                    crate::interpreter::Interpreter::count_arg(&args[1], line, col)?
                } else {
                    5
                };
                let resolved = self.resolve_cwd(&path, line, col)?;
                let display = self.file_policy.borrow().display_str(&path);
                let content = crate::interpreter::Interpreter::read_file_capped(
                    &resolved, &display, line, col,
                )?;
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
                        let display = self.file_policy.borrow().display_str(path);
                        let content = crate::interpreter::Interpreter::read_file_capped(
                            &resolved, &display, line, col,
                        )?;
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
                        let display = self.file_policy.borrow().display_str(path);
                        let content = crate::interpreter::Interpreter::read_file_capped(
                            &resolved, &display, line, col,
                        )?;
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
                        let display = self.file_policy.borrow().display_str(path);
                        let content = crate::interpreter::Interpreter::read_file_capped(
                            &resolved, &display, line, col,
                        )?;
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
                        let cwd = self.cwd.borrow().clone();
                        let mut out: Vec<String> = Vec::new();
                        crate::interpreter::Interpreter::find_files(&cwd, &cwd, query, 0, &mut out);
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
                    self.cwd.borrow().clone()
                };
                let mut out: Vec<String> = Vec::new();
                crate::interpreter::Interpreter::tree_lines(&dir, "", 0, &mut out);
                Ok(Value::List(out.into_iter().map(Value::Str).collect()))
            }
            "حجم_المسار" => {
                arity(1)?;
                match &args[0] {
                    Value::Str(path) => {
                        let resolved = self.resolve_cwd(path, line, col)?;
                        let mut budget: usize = 10000;
                        Ok(Value::Number(
                            crate::interpreter::Interpreter::dir_size(&resolved, &mut budget)
                                as f64,
                        ))
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
                                crate::error::ErrorKind::Io,
                                format!("تعذر إنشاء الرابط {}: {}", self.file_policy.borrow().display_str(link), e),
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
                        match std::fs::metadata(&target_path) {
                            Ok(m) if m.is_dir() => {
                                return Err(RioError::new(
                                    crate::error::ErrorKind::Io,
                                    "لا يمكن إنشاء رابط صلب لمجلد",
                                    line,
                                    col,
                                ));
                            }
                            Ok(_) => {}
                            Err(_) => {
                                return Err(RioError::new(
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "الهدف غير موجود: {}",
                                        self.file_policy.borrow().display_str(target)
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
                                    crate::error::ErrorKind::Io,
                                    format!(
                                        "تعذر إنشاء الرابط الصلب {}: {}",
                                        self.file_policy.borrow().display_str(link),
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
                        let target_abs = self.resolve_cwd(target, line, col)?;
                        let link_path = self.resolve_cwd(link, line, col)?;
                        let link_dir = link_path
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| self.cwd.borrow().clone());
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
                                crate::error::ErrorKind::Io,
                                format!("تعذر إنشاء الرابط {}: {}", self.file_policy.borrow().display_str(link), e),
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
                Ok(Value::Str(
                    crate::interpreter::Interpreter::format_unix_utc(secs),
                ))
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
            // تفاصيل المسار (stat) — مرآة ذراع المفسر نفسه:
            // نوعه وحجمه وتاريخ آخر تعديل، قاموس واحد.
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
                        let size = crate::interpreter::Interpreter::dir_size(
                            &resolved, &mut budget,
                        );
                        let modified = meta
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok()
                            })
                            .map(|d| {
                                crate::interpreter::Interpreter::format_unix_utc(
                                    d.as_secs(),
                                )
                            })
                            .unwrap_or_else(|| "—".to_string());
                        // المرحلة (د) — الإكمال (اجتماع 69
                        // §84): الحقول الموسعة — مرآة ذراع
                        // المفسر نفسه.
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
                                    let owner =
                                        crate::interpreter::Interpreter::passwd_name(uid)
                                            .unwrap_or_else(|| uid.to_string());
                                    let group =
                                        crate::interpreter::Interpreter::group_name(gid)
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
                            .map(|d| {
                                crate::interpreter::Interpreter::format_unix_utc(
                                    d.as_secs(),
                                )
                            })
                            .unwrap_or_else(|| "—".to_string());
                        #[cfg(unix)]
                        let changed = {
                            use std::os::unix::fs::MetadataExt;
                            match meta.as_ref() {
                                Some(m) if m.ctime() >= 0 => {
                                    crate::interpreter::Interpreter::format_unix_utc(
                                        m.ctime() as u64,
                                    )
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
                        // نفسه (الآلة ~3544) — حي مع البث الحي
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
                            &self.file_policy.borrow(),
                            &self.cwd.borrow(),
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
                    self.cwd.borrow().clone()
                };
                let count = std::fs::read_dir(&dir)
                    .map(|entries| entries.count())
                    .map_err(|e| {
                        RioError::new(crate::error::ErrorKind::Io, format!("تعذر عرض المجلد: {}", e), line, col)
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

    pub fn run(&mut self) -> RioResult<()> {
        self.run_inner()
            .map_err(|e| e.with_file_if_none(self.source_file.clone()))
    }

    fn run_inner(&mut self) -> RioResult<()> {
        while self.pc < self.code.len() {
            let idx = self.pc;
            let instr = self.code[idx].clone();
            self.pc += 1;
            match self.step(idx, instr) {
                // الدفعة 1: خروج معلق (من `خروج()` المدمجة
                // أو من VM فرعية عادت) يُفكّ هنا — يهرب
                // من كل معالج إلا إلى `أخيرا`، ثم ينهي
                // التشغيل برمزه؛ وخطوة أخيرا نفسها تعود
                // إلى هنا فتُستأنف.
                Ok(StepFlow::Next) => {
                    if let Some(code) = self.exit_code {
                        self.pending_flow = Some(FlowResume::Exit(code));
                        match self.unwind_flow(1, 1)? {
                            // قفز إلى أخيرا: الراية تُمسح
                            // لأن FinallyCtx يحمل الخروج
                            // الآن — وإلا أعاد إطلاق نفسه
                            // داخل جسم أخيرا قبل تنفيذه.
                            UnwindOutcome::Continue => {
                                self.exit_code = None;
                            }
                            UnwindOutcome::ReturnNow => return Ok(()),
                        }
                    }
                }
                Ok(StepFlow::Returned) => return Ok(()),
                // خطأ اعترضه on_error وعولج (أعاد التموضع
                // إلى ماعدا/أخيرا) فتواصل الحلقة؛ وإن لم
                // يمسكه معالج فـ `?` تعيده إلى المتصل.
                Err(e) => self.on_error(e)?,
            }
        }
        Ok(())
    }

    /// ك-1.2: خطوة تنفيذ واحدة (تعليمة واحدة). فُصلت عن
    /// run() لأن حاول/ماعدا/أخيرا يحتاج اعتراض الأخطاء
    /// وإعادة التموضع — وrun() وحدها هي من تعترض.
    /// `idx` هو رقم التعليمة في التدفق (تحتاجه تعليمات
    /// تجهيز الحلقات لحساب continue_pc = idx + 1).
    fn step(&mut self, idx: usize, instr: Instr) -> RioResult<StepFlow> {
        let line = instr.line;
        let col = instr.col;
        match instr.op {
                Op::Const(v) => self.stack.push(v),
                Op::Load(name) => {
                    // Phase ك-1: walk the scope chain
                    // innermost-first (a closure value bound in
                    // an outer scope is found here).
                    let v = self.lookup(&name).ok_or_else(|| {
                        RioError::runtime(format!("متغير غير معرف: {}", name), line, col)
                    })?;
                    self.stack.push(v);
                }
                Op::Store(name) => {
                    let v = self.pop(line, col)?;
                    // Phase ك-1: lexical assignment (Meeting 11)
                    // — a closure storing an outer variable mutates
                    // the captured scope, not a local shadow.
                    self.assign(&name, v, line, col)?;
                }
                Op::StoreAppend(name) => {
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): الإلحاق — القديم + الجديد
                    // بعملية `+` نفسها، والغائب من
                    // الهوية كما في باش.
                    let v = self.pop(line, col)?;
                    let old = match self.lookup(&name) {
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
                                    line,
                                    col,
                                ));
                            }
                        },
                    };
                    let joined = crate::interpreter::Interpreter::add_values(
                        &old, &v, line, col,
                    )?;
                    self.assign(&name, joined, line, col)?;
                }
                Op::Pop => {
                    // اجتماع 32، السؤال الثاني (2): عبارة
                    // مجردة تنتج قيمة دالة خطأ عربي يسمي
                    // الدالة — مرآة فحص ExprStmt في المفسر
                    // حرفيا (نفس الرسالة، نفس الاسم من
                    // القيمة نفسها)، فيتطابق الوضعان.
                    let v = self.pop(line, col)?;
                    if let Value::Function { name, .. } | Value::NativeFunction { name, .. } =
                        &v
                    {
                        return Err(RioError::runtime(
                            format!(
                                "دالة بلا استدعاء: {} — استدعها بالأقواس أو أسندها إلى متغير",
                                name
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): الجمع (ومنها دمج القواميس)
                    // مساعد المفسر نفسه، فتتطابق الرسائل.
                    if matches!(instr.op, Op::Add) {
                        let joined =
                            crate::interpreter::Interpreter::add_values(&l, &r, line, col)?;
                        self.stack.push(joined);
                        return Ok(StepFlow::Next);
                    }
                    let (a, b) = match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => (*a, *b),
                        (Value::Str(a), Value::Str(b)) if matches!(instr.op, Op::Add) => {
                            self.stack.push(Value::Str(format!("{}{}", a, b)));
                            return Ok(StepFlow::Next);
                        }
                        // مهمة 6: جمع قائمتين يدمجهما، كما
                        // في المفسر (ذراع الجمع وحده).
                        (Value::List(a), Value::List(b)) if matches!(instr.op, Op::Add) => {
                            let mut out = a.clone();
                            out.extend(b.iter().cloned());
                            self.stack.push(Value::List(out));
                            return Ok(StepFlow::Next);
                        }
                        _ => {
                            // مهمة 6: رسالة الجمع في المفسر
                            // خاصة («لا يمكن جمع ...»)، وباقي
                            // العمليات بالرسالة العامة نفسها.
                            let msg = if matches!(instr.op, Op::Add) {
                                format!("لا يمكن جمع {} و {}", l.type_name(), r.type_name())
                            } else {
                                format!(
                                    "العملية الحسابية تتطلب عددين، وليس {} و {}",
                                    l.type_name(),
                                    r.type_name()
                                )
                            };
                            return Err(RioError::typ(msg, line, col));
                        }
                    };
                    let v = match instr.op {
                        Op::Add => Value::Number(a + b),
                        Op::Sub => Value::Number(a - b),
                        Op::Mul => Value::Number(a * b),
                        Op::Div => {
                            if b == 0.0 {
                                return Err(RioError::runtime("قسمة على صفر", line, col));
                            }
                            Value::Number(a / b)
                        }
                        Op::Mod => {
                            if b == 0.0 {
                                return Err(RioError::runtime("باقي القسمة على صفر", line, col));
                            }
                            Value::Number(a % b)
                        }
                        _ => unreachable!(),
                    };
                    self.stack.push(v);
                }
                Op::Eq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    self.stack.push(Value::Bool(l == r));
                }
                Op::NotEq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    self.stack.push(Value::Bool(l != r));
                }
                Op::Lt | Op::Gt | Op::LtEq | Op::GtEq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    let ord = match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
                        _ => {
                            return Err(RioError::typ(
                                format!("المقارنة تتطلب عددين أو نصين، وليس {} و {}", l.type_name(), r.type_name()),
                                line,
                                col,
                            ));
                        }
                    };
                    let ord = ord.ok_or_else(|| RioError::runtime("مقارنة غير صالحة", line, col))?;
                    let b = match instr.op {
                        Op::Lt => ord == std::cmp::Ordering::Less,
                        Op::Gt => ord == std::cmp::Ordering::Greater,
                        Op::LtEq => ord != std::cmp::Ordering::Greater,
                        Op::GtEq => ord != std::cmp::Ordering::Less,
                        _ => unreachable!(),
                    };
                    self.stack.push(Value::Bool(b));
                }
                Op::And => {
                    let r = self.pop_bool(line, col)?;
                    let l = self.pop_bool(line, col)?;
                    self.stack.push(Value::Bool(l && r));
                }
                Op::Or => {
                    let r = self.pop_bool(line, col)?;
                    let l = self.pop_bool(line, col)?;
                    self.stack.push(Value::Bool(l || r));
                }
                Op::Not => {
                    // مهمة 6: رسالة «ليس» في المفسر خاصة
                    // («ليس تتطلب منطقيا»)، لا العامة.
                    match self.pop(line, col)? {
                        Value::Bool(b) => self.stack.push(Value::Bool(!b)),
                        other => {
                            return Err(RioError::typ(
                                format!("ليس تتطلب منطقيا، وليس {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Neg => {
                    match self.pop(line, col)? {
                        Value::Number(n) => self.stack.push(Value::Number(-n)),
                        other => {
                            return Err(RioError::typ(
                                format!("السالب يتطلب عددا، وليس {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Jump(target) => {
                    self.pc = target;
                }
                Op::JumpIfFalse(target) => {
                    let b = self.pop_bool(line, col)?;
                    if !b {
                        self.pc = target;
                    }
                }
                Op::JumpIfTrue(target) => {
                    let b = self.pop_bool(line, col)?;
                    if b {
                        self.pc = target;
                    }
                }
                // مهمة 6 (بوابة التكافؤ ك-1.5): فحص منطقي
                // بسياق المفسر — تُصدَر قبل كل قفز شرطي
                // (إذا/وإلا إذا/و/أو)؛ تسحب القيمة فإن كانت
                // منطقية أعادتها، وإلا أخطأت برسالة السياق.
                Op::CheckBool(ctx) => match self.pop(line, col)? {
                    Value::Bool(b) => self.stack.push(Value::Bool(b)),
                    other => {
                        return Err(RioError::typ(ctx.message(&other.type_name()), line, col));
                    }
                },
                // مهمة 6 (بوابة التكافؤ ك-1.5): فحص التعيين
                // المعلن نوعه وقت التشغيل — ينظر إلى أعلى
                // المكدس دون سحبه (القيمة ما زالت للتعيين
                // التالي)؛ فإن خالف نوعها المعلن أخطأ
                // برسالة المفسر نفسها.
                Op::CheckAssignType { name, expected } => {
                    let actual = match self.stack.last() {
                        Some(v) => v.type_name(),
                        None => {
                            return Err(RioError::runtime(
                                "مكدس VM فارغ عند فحص نوع التعيين",
                                line,
                                col,
                            ));
                        }
                    };
                    if actual != expected.as_str() {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: المتغير {} معلن {}، لكن القيمة {}",
                                name, expected, actual
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Op::Print(n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الطباعة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let vals: Vec<Value> = self.stack.drain(start..).collect();
                    let parts: Vec<String> = vals.iter().map(|v| v.fmt_value()).collect();
                    let line_out = format!("{}\n", parts.join(" "));
                    // اجتماع 32، السؤال الثامن: بث حي —
                    // كالمفسر حرفيا (فور الإنتاج، بلا احتفاظ).
                    if self.live_output {
                        use std::io::Write;
                        print!("{}", line_out);
                        let _ = std::io::stdout().flush();
                    } else {
                        self.output.push_str(&line_out);
                    }
                }
                Op::BuildList(n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند بناء القائمة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let items: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::List(items));
                }
                Op::BuildDict(keys) => {
                    let n = keys.len();
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند بناء القاموس", line, col));
                    }
                    let start = self.stack.len() - n;
                    let vals: Vec<Value> = self.stack.drain(start..).collect();
                    // Duplicate keys: later wins, like the interpreter.
                    let mut pairs: Vec<(String, Value)> = Vec::new();
                    for (k, v) in keys.iter().zip(vals.into_iter()) {
                        if let Some(pair) = pairs.iter_mut().find(|(kk, _)| kk == k) {
                            pair.1 = v;
                        } else {
                            pairs.push((k.clone(), v));
                        }
                    }
                    self.stack.push(Value::Dict(pairs));
                }
                Op::Index { idx_line, idx_col } => {
                    let idx = self.pop(line, col)?;
                    let target = self.pop(line, col)?;
                    let result = match (&target, &idx) {
                        (Value::List(items), Value::Number(x)) => {
                            if x.fract() != 0.0 || *x < 0.0 {
                                return Err(RioError::typ(
                                    "فهرس القائمة يجب أن يكون عددا صحيحا",
                                    idx_line,
                                    idx_col,
                                ));
                            }
                            let i = *x as usize;
                            items.get(i).cloned().ok_or_else(|| {
                                RioError::runtime(
                                    format!("فهرس خارج النطاق: {} والقائمة طولها {}", i, items.len()),
                                    idx_line,
                                    idx_col,
                                )
                            })?
                        }
                        (Value::Dict(pairs), Value::Str(key)) => pairs
                            .iter()
                            .find(|(k, _)| k == key)
                            .map(|(_, v)| v.clone())
                            .ok_or_else(|| {
                                RioError::runtime(
                                    format!("مفتاح غير موجود: {}", key),
                                    idx_line,
                                    idx_col,
                                )
                            })?,
                        (Value::Str(s), Value::Number(x)) => {
                            if x.fract() != 0.0 || *x < 0.0 {
                                return Err(RioError::typ(
                                    "فهرس النص يجب أن يكون عددا صحيحا",
                                    idx_line,
                                    idx_col,
                                ));
                            }
                            let chars: Vec<char> = s.chars().collect();
                            chars
                                .get(*x as usize)
                                .map(|c| Value::Str(c.to_string()))
                                .ok_or_else(|| {
                                    RioError::runtime(
                                        "فهرس خارج النطاق في النص",
                                        idx_line,
                                        idx_col,
                                    )
                                })?
                        }
                        _ => {
                            return Err(RioError::typ(
                                format!(
                                    "لا يمكن الفهرسة على نوع {} بمفتاح من نوع {}",
                                    target.type_name(),
                                    idx.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    };
                    self.stack.push(result);
                }
                Op::StoreIndex { name, idx_line, idx_col, tgt_line, tgt_col } => {
                    // Stack: [value, index] (value compiled first,
                    // matching the interpreter's evaluation order).
                    // Phase ك-1: mutate the slot in the scope
                    // where the name is bound (the slot lives in
                    // a shared Rc scope, so a closure mutating a
                    // captured list/dict mutates the original).
                    let idx = self.pop(line, col)?;
                    let value = self.pop(line, col)?;
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): الفهرسة عبر مرجع تصل هدفه،
                    // والفهرسة في مجمد مرفوضة.
                    let name = self.resolve_name(&name);
                    self.check_not_frozen(&name, tgt_line, tgt_col)?;
                    let si = self.lookup_scope_index(&name).ok_or_else(|| {
                        RioError::runtime(
                            format!("متغير غير معرف: {}", name),
                            tgt_line,
                            tgt_col,
                        )
                    })?;
                    let scope = Rc::clone(&self.env[si]);
                    let mut borrowed = scope.borrow_mut();
                    let slot = borrowed
                        .get_mut(&name)
                        .expect("name was just found in this scope");
                    match slot {
                        Value::List(items) => {
                            let i = match idx {
                                Value::Number(x) if x.fract() == 0.0 && x >= 0.0 => x as usize,
                                other => {
                                    return Err(RioError::typ(
                                        format!("فهرس القائمة يجب أن يكون عددا صحيحا، وليس {}", other.type_name()),
                                        idx_line,
                                        idx_col,
                                    ));
                                }
                            };
                            if i >= items.len() {
                                return Err(RioError::runtime(
                                    format!("فهرس خارج النطاق: {} والقائمة طولها {}", i, items.len()),
                                    idx_line,
                                    idx_col,
                                ));
                            }
                            items[i] = value;
                        }
                        Value::Dict(pairs) => {
                            let key = match idx {
                                Value::Str(s) => s,
                                other => {
                                    return Err(RioError::typ(
                                        format!("مفتاح القاموس يجب أن يكون نصا، وليس {}", other.type_name()),
                                        idx_line,
                                        idx_col,
                                    ));
                                }
                            };
                            if let Some(pair) = pairs.iter_mut().find(|(k, _)| *k == key) {
                                pair.1 = value;
                            } else {
                                pairs.push((key, value));
                            }
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("لا يمكن تعيين عنصر في نوع {}", other.type_name()),
                                tgt_line,
                                tgt_col,
                            ));
                        }
                    }
                }
                Op::Return => {
                    // ك-1.2: ارجع يفكّ معالجات حاول أولا
                    // (يشغّل أخيرا)، ثم يكمل الارجع. القيمة
                    // تُحفظ في pending_return أثناء الفكّ.
                    let v = if self.stack.is_empty() {
                        Value::Nil
                    } else {
                        self.pop(line, col)?
                    };
                    self.pending_return = Some(v);
                    self.pending_flow = Some(FlowResume::Return);
                    match self.unwind_flow(line, col)? {
                        UnwindOutcome::Continue => return Ok(StepFlow::Next),
                        UnwindOutcome::ReturnNow => {
                            // perform_flow وضعت القيمة في
                            // return_value — لا تُؤخذ مرة أخرى
                            // (أخذها ثانية يعيدها لاشيء).
                            return Ok(StepFlow::Returned);
                        }
                    }
                }
                Op::Builtin { name, nargs: n, callee_line, callee_col } => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند استدعاء دالة مدمجة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    // Resolution order, like the interpreter: a
                    // name bound in the environment wins over the
                    // builtin. مهمة 4: إن كانت القيمة المربوطة
                    // دالة (مستخدمة أو مربوطة أو أصلية) فتُستدعى
                    // — ملفات الوحدات لا يحللها المحلل
                    // الدلالي، فدالة وحدة تظلل اسما مدمجا
                    // وتُستدعى قبل تعريفها تصل هنا؛ وإن كانت
                    // فئة فخطأ «استخدم أنشئ»؛ وغير ذلك خطأ نوع.
                    if let Some(v) = self.lookup(&name) {
                        match v {
                            Value::Function { .. }
                            | Value::BoundMethod { .. }
                            | Value::NativeFunction { .. } => {
                                let result = self.call_value(v, args, line, col)?;
                                self.stack.push(result);
                                return Ok(StepFlow::Next);
                            }
                            Value::Class(data) => {
                                return Err(RioError::typ(
                                    format!(
                                        "استخدم أنشئ لإنشاء كائن من فئة {}، مثل أنشئ {}()",
                                        data.name, data.name
                                    ),
                                    callee_line,
                                    callee_col,
                                ));
                            }
                            other => {
                                return Err(RioError::typ(
                                    format!("القيمة {} ليست دالة", other.type_name()),
                                    callee_line,
                                    callee_col,
                                ));
                            }
                        }
                    }
                    // مهمة 5: ملف/ادخل → call_file_builtin؛
                    // خالصة → call_pure_builtin.
                    let result = if is_pure_builtin(&name) {
                        self.call_pure_builtin(&name, args, line, col)?
                    } else {
                        self.call_file_builtin(&name, args, line, col)?
                    };
                    self.stack.push(result);
                }
                Op::Call { name, nargs: n, callee_line, callee_col } => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الاستدعاء", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    // Phase ك-1: env-first, like the interpreter.
                    // A closure value bound in the environment
                    // wins over the compiled table, so nested
                    // and returned closures (and shadowing) call
                    // their own captured definition.
                    if let Some(v) = self.lookup(&name) {
                        match v {
                            Value::BoundMethod { func, instance } => {
                                let result = self.call_method_value(
                                    *func, instance, args, line, col,
                                )?;
                                self.stack.push(result);
                                return Ok(StepFlow::Next);
                            }
                            Value::Function { .. } => {
                                let result =
                                    self.call_closure_value(v, args, line, col)?;
                                self.stack.push(result);
                                return Ok(StepFlow::Next);
                            }
                            // مهمة 4: دالة أصلية مستوردة بالاسم
                            // (من رياضيات استورد جذر) — تُستدعى
                            // بالمؤشر مباشرة، كما في المفسر.
                            Value::NativeFunction { func, .. } => {
                                let result = func(self, &args, line, col)?;
                                self.stack.push(result);
                                return Ok(StepFlow::Next);
                            }
                            // مهمة 4: استدعاء فئة بالاسم خطأ
                            // «استخدم أنشئ»، برسالة المفسر نفسها
                            // (ملفات الوحدات لا يحللها المحلل
                            // الدلالي، فيصل هذا الخطأ للتشغيل).
                            // مهمة 6: الموضع موضع الاسم المستدعى
                            // نفسه، لا موضع القوس.
                            Value::Class(data) => {
                                return Err(RioError::typ(
                                    format!(
                                        "استخدم أنشئ لإنشاء كائن من فئة {}، مثل أنشئ {}()",
                                        data.name, data.name
                                    ),
                                    callee_line,
                                    callee_col,
                                ));
                            }
                            _ => {
                                return Err(RioError::typ(
                                    format!("القيمة {} ليست دالة", v.type_name()),
                                    callee_line,
                                    callee_col,
                                ));
                            }
                        }
                    }
                    // The name is NOT bound in the environment:
                    // the compiled table answers (forward calls,
                    // and calls from method bodies, which have no
                    // captured chain yet — decision A, §39).
                    let func = match self.functions.get(&name) {
                        Some(f) => f.clone(),
                        None => {
                            return Err(RioError::runtime(
                                format!("دالة غير معرفة في VM: {}", name),
                                line,
                                col,
                            ));
                        }
                    };
                    if func.params.len() != n {
                        return Err(RioError::runtime(
                            format!(
                                "الدالة {} تتوقع {} معاملات لكن أعطيت {}",
                                name,
                                func.params.len(),
                                n
                            ),
                            line,
                            col,
                        ));
                    }
                    // مهمة 3: أنواع المعاملات كما في المفسر
                    // (كانت تُفحص لإغلاقات القيم فقط).
                    Self::check_param_types(
                        "الدالة",
                        &name,
                        &func.params,
                        &func.param_types,
                        &args,
                        line,
                        col,
                    )?;
                    // Same depth limit as the interpreter (Meeting 15).
                    if self.call_depth >= crate::interpreter::MAX_CALL_DEPTH {
                        return Err(RioError::runtime(
                            format!(
                                "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                                crate::interpreter::MAX_CALL_DEPTH
                            ),
                            line,
                            col,
                        ));
                    }
                    let mut child_vm = self.child(&func, args);
                    match child_vm.run() {
                        Ok(()) => {
                            self.output.push_str(&child_vm.output);
                            // الدفعة 1: خروج أو رمز آخر أمر
                            // من داخل الدالة يصعدان إلى المتصل.
                            if let Some(code) = child_vm.exit_code {
                                self.exit_code = Some(code);
                            }
                            self.last_exit_code = child_vm.last_exit_code;
                            let (returned, ret) = Self::child_return(&child_vm);
                            // مهمة 3: نوع الإرجاع كما في المفسر.
                            Self::check_return_type(
                                "الدالة",
                                &name,
                                &func.return_type,
                                returned,
                                &ret,
                                line,
                                col,
                            )?;
                            self.stack.push(ret);
                        }
                        Err(e) => {
                            // Preserve partial output, like the interpreter.
                            self.output.push_str(&child_vm.output);
                            return Err(e);
                        }
                    }
                }
                Op::DefineClass(def) => {
                    // Base first (must already be defined), then
                    // interface compliance, like the interpreter.
                    // Methods are already flattened in def.
                    let base_data = match &def.base {
                        Some(bname) => match self.lookup(bname) {
                            Some(Value::Class(data)) => Some(Rc::clone(&data)),
                            Some(other) => {
                                return Err(RioError::typ(
                                    format!("{} ليست فئة، بل {}", bname, other.type_name()),
                                    line,
                                    col,
                                ));
                            }
                            None => {
                                return Err(RioError::runtime(
                                    format!("فئة أب غير معرفة: {}", bname),
                                    line,
                                    col,
                                ));
                            }
                        },
                        None => None,
                    };
                    for iname in &def.interfaces {
                        let iface = match self.lookup(iname) {
                            Some(Value::Interface(data)) => Rc::clone(&data),
                            Some(other) => {
                                return Err(RioError::typ(
                                    format!("{} ليست واجهة، بل {}", iname, other.type_name()),
                                    line,
                                    col,
                                ));
                            }
                            None => {
                                return Err(RioError::runtime(
                                    format!("واجهة غير معرفة: {}", iname),
                                    line,
                                    col,
                                ));
                            }
                        };
                        for (mname, arity) in &iface.methods {
                            match def.methods.get(mname) {
                                Some(f) => {
                                    if f.params.len() != *arity {
                                        return Err(RioError::runtime(
                                            format!(
                                                "الفئة {} تنفذ طريقة {} من واجهة {} بعدد معاملات غير صحيح: تتوقع {} لكن وجد {}",
                                                def.name, mname, iname, arity, f.params.len()
                                            ),
                                            line,
                                            col,
                                        ));
                                    }
                                }
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الفئة {} لا تنفذ طريقة {} المطلوبة من واجهة {}",
                                            def.name, mname, iname
                                        ),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                    }
                    // مهمة 3: الطرق قيم دوال على الفئة نفسها،
                    // كما في المفسر: الموروثة تأتي من قيمة
                    // الفئة الأب (بسلسلة نطاق تعريفها)،
                    // والخاصة تُبنى هنا بسلسلة التعريف
                    // الحالية، فيراها جسم الطريقة ويغيّرها.
                    let mut methods: HashMap<String, Value> = HashMap::new();
                    if let Some(base) = &base_data {
                        methods = base.methods.clone();
                    }
                    for (mname, id) in &def.own_method_ids {
                        if let Some(fdef) = def.methods.get(mname) {
                            methods.insert(
                                mname.clone(),
                                Value::Function {
                                    name: mname.clone(),
                                    params: fdef.params.clone(),
                                    body: Vec::new(),
                                    // مهمة 4: طريقة فئة عُرّفت في
                                    // وحدة تحمل اسمها (كما في
                                    // المفسر: mod_name من مكدس
                                    // التحميل)، فيدفع استدعاؤها
                                    // نسخة نطاق الوحدة.
                                    module: self.loading_stack.borrow().last().cloned(),
                                    param_types: fdef.param_types.clone(),
                                    return_type: fdef.return_type.clone(),
                                    captured: Some(self.env.clone()),
                                    vm_id: Some(*id),
                                },
                            );
                        }
                    }
                    let class_data = Rc::new(crate::value::ClassData {
                        name: def.name.clone(),
                        methods,
                        base: base_data,
                        interfaces: def.interfaces.clone(),
                    });
                    self.define(&def.name, Value::Class(class_data));
                }
                Op::DefineInterface(name, methods) => {
                    let iface = Value::Interface(Rc::new(crate::value::InterfaceData {
                        name: name.clone(),
                        methods: methods.clone(),
                    }));
                    self.define(&name, iface);
                }
                Op::Dot(name) => {
                    let target = self.pop(line, col)?;
                    match &target {
                        // مهمة 4 (ك-1.3): الوصول بالنقطة إلى
                        // وحدة يقرأ التصدير فقط، برسالة
                        // المفسر نفسها للاسم غير المصدَّر.
                        Value::Module { name: mod_name, exports, .. } => {
                            match exports.get(&name) {
                                Some(v) => self.stack.push(v.clone()),
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                            name, mod_name, name, mod_name
                                        ),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                        Value::Instance(data) => {
                            // Field first, then method (bound to
                            // this instance). A field shadowing a
                            // method name wins, like the
                            // interpreter. The method comes from
                            // the class value itself (lookup walks
                            // the base chain), so a class defined
                            // inside a function still answers —
                            // مهمة 3.
                            if let Some(v) = data.fields.borrow().get(&name) {
                                self.stack.push(v.clone());
                            } else if let Some(func) = data.class.lookup_method(&name) {
                                self.stack.push(Value::BoundMethod {
                                    func: Box::new(func),
                                    instance: Rc::clone(data),
                                });
                            } else {
                                return Err(RioError::runtime(
                                    format!(
                                        "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                                        data.class.name, name
                                    ),
                                    line,
                                    col,
                                ));
                            }
                        }
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::StoreDot(name) => {
                    // Stack: [value, target] (value compiled
                    // first, matching the interpreter).
                    let target = self.pop(line, col)?;
                    let value = self.pop(line, col)?;
                    match target {
                        Value::Instance(data) => {
                            data.fields.borrow_mut().insert(name, value);
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("لا يمكن تعيين حقل في نوع {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Create(class_name, n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الإنشاء", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    let class_data = match self.lookup(&class_name) {
                        Some(Value::Class(data)) => Rc::clone(&data),
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست فئة، بل {}", class_name, other.type_name()),
                                line,
                                col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("فئة غير معرفة: {}", class_name),
                                line,
                                col,
                            ));
                        }
                    };
                    let instance = Rc::new(crate::value::InstanceData {
                        class: Rc::clone(&class_data),
                        fields: std::cell::RefCell::new(HashMap::new()),
                    });
                    // Constructor: a method named أنشئ, if present
                    // (own or inherited via the base chain),
                    // runs automatically with نفس bound.
                    if let Some(func) = class_data.lookup_method("أنشئ") {
                        let result = self.call_method_value(
                            func,
                            Rc::clone(&instance),
                            args,
                            line,
                            col,
                        );
                        // The constructor's return value is
                        // discarded, like in the interpreter.
                        let _ = result?;
                    } else if !args.is_empty() {
                        return Err(RioError::runtime(
                            format!(
                                "الفئة {} ليس لها منشئ أنشئ، لكن أعطيت {} معاملات",
                                class_data.name,
                                args.len()
                            ),
                            line,
                            col,
                        ));
                    }
                    self.stack.push(Value::Instance(instance));
                }
                Op::CallDotMethod { name, var, nargs } => {
                    // مهمة 3: استدعاء عبر نقطة مباشرة. المكدس
                    // عند الدخول: [معامل1، ...، معاملN،
                    // الهدف] (المعاملات قُيّمت أولا، كما في
                    // المفسر). نص/قائمة ← طريقة مدمجة؛ كائن
                    // ← حقل أولا ثم طريقة؛ غيرهما ← خطأ
                    // الوصول بالنقطة نفسه في المفسر.
                    if self.stack.len() < nargs + 1 {
                        return Err(RioError::runtime("مكدس VM فارغ عند استدعاء طريقة", line, col));
                    }
                    let target = self.pop(line, col)?;
                    let start = self.stack.len() - nargs;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    match target {
                        Value::Str(_) | Value::List(_) => {
                            let result = self.call_builtin_method_vm(
                                target,
                                &name,
                                var.as_deref(),
                                args,
                                line,
                                col,
                            )?;
                            self.stack.push(result);
                        }
                        Value::Instance(data) => {
                            // Field first (a field holding a
                            // callable wins), then method — like
                            // the interpreter's Dot. The field
                            // borrow ends before the call (a
                            // method may write the same fields).
                            let field = data.fields.borrow().get(&name).cloned();
                            if let Some(callee) = field {
                                let result =
                                    self.call_value(callee, args, line, col)?;
                                self.stack.push(result);
                            } else if let Some(func) = data.class.lookup_method(&name) {
                                let result = self.call_method_value(
                                    func,
                                    Rc::clone(&data),
                                    args,
                                    line,
                                    col,
                                )?;
                                self.stack.push(result);
                            } else {
                                return Err(RioError::runtime(
                                    format!(
                                        "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                                        data.class.name, name
                                    ),
                                    line,
                                    col,
                                ));
                            }
                        }
                        // مهمة 4 (ك-1.3): استدعاء عبر نقطة
                        // وحدة: الاسم من التصدير فقط (رسالة
                        // المفسر نفسها للغائب)، ثم استدعاء
                        // القيمة — دالة وحدة، أو دالة أصلية
                        // (رياضيات.جذر)، أو خطأ «ليست دالة».
                        Value::Module { name: mod_name, exports, .. } => {
                            match exports.get(&name) {
                                Some(callee) => {
                                    let result = self.call_value(
                                        callee.clone(),
                                        args,
                                        line,
                                        col,
                                    )?;
                                    self.stack.push(result);
                                }
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                            name, mod_name, name, mod_name
                                        ),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::MakeClosure(def) => {
                    // Phase ك-1: a Lambda expression — push the
                    // closure value (capturing the current scope
                    // chain) onto the stack.
                    let v = self.make_closure_value(&def);
                    self.stack.push(v);
                }
                Op::DefineClosure(def) => {
                    // Phase ك-1: a دالة definition — create the
                    // closure value and define it in the
                    // innermost scope, like the interpreter's
                    // define_local. The value is NOT left on the
                    // stack (a definition is a statement).
                    let v = self.make_closure_value(&def);
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): إعادة تعريف اسم مجمد مرفوضة.
                    self.check_not_frozen(&def.name, line, col)?;
                    self.define(&def.name, v);
                }
                Op::CallValue { nargs: n, callee_line, callee_col } => {
                    // Phase ك-1: first-class callee. Stack at
                    // entry: [arg1, ..., argN, callee] (args were
                    // compiled first, matching the interpreter).
                    if self.stack.len() < n + 1 {
                        return Err(RioError::runtime("مكدس VM فارغ عند الاستدعاء", line, col));
                    }
                    let callee = self.pop(line, col)?;
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    // مهمة 6: المستدعى غير الدالة خطأ بموضع
                    // تعبير المستدعى نفسه، كما في المفسر — قبل
                    // call_value، فهي تبلِّغ بموضع التعليمة.
                    if !matches!(
                        callee,
                        Value::Function { .. }
                            | Value::BoundMethod { .. }
                            | Value::NativeFunction { .. }
                    ) {
                        return Err(RioError::typ(
                            format!("القيمة {} ليست دالة", callee.type_name()),
                            callee_line,
                            callee_col,
                        ));
                    }
                    let result = self.call_value(callee, args, line, col)?;
                    self.stack.push(result);
                }
                Op::Import { module, alias, names } => {
                    // مهمة 4 (ك-1.3): تحميل الوحدة وقت
                    // التنفيذ، كما في المفسر: ربط الوحدة
                    // (أو مختصرها)، أو ربط كل اسم مطلوب
                    // من التصدير فقط.
                    let mod_value = self.load_module(&module, line, col)?;
                    match &names {
                        Some(names) => {
                            let exports = match &mod_value {
                                Value::Module { exports, .. } => exports.clone(),
                                _ => HashMap::new(),
                            };
                            for name in names {
                                match exports.get(name) {
                                    Some(v) => self.define(name, v.clone()),
                                    None => {
                                        return Err(RioError::runtime(
                                            format!(
                                                "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                                name, module, name, module
                                            ),
                                            line,
                                            col,
                                        ));
                                    }
                                }
                            }
                        }
                        None => {
                            let bind = alias.clone().unwrap_or_else(|| module.clone());
                            self.define(&bind, mod_value);
                        }
                    }
                }
                Op::JumpIfFalseLoop(target) => {
                    let b = self.pop_bool_loop(line, col)?;
                    if !b {
                        self.pc = target;
                    }
                }
                Op::ForSetup { var, end_pc } => {
                    let iterable = self.pop(line, col)?;
                    let items: Vec<Value> = match iterable {
                        Value::List(items) => items,
                        Value::Str(s) => {
                            s.chars().map(|c| Value::Str(c.to_string())).collect()
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("لا يمكن التكرار على نوع {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    };
                    self.loop_states.push(LoopState::For {
                        items,
                        index: 0,
                        var: var.clone(),
                        end_pc,
                        continue_pc: idx + 1,
                    });
                }
                Op::ForNext(target) => {
                    let next = match self.loop_states.last_mut() {
                        Some(LoopState::For { items, index, var, .. }) => {
                            if *index >= items.len() {
                                None
                            } else {
                                let item = items[*index].clone();
                                let name = var.clone();
                                *index += 1;
                                Some((name, item))
                            }
                        }
                        _ => None,
                    };
                    match next {
                        Some((name, item)) => self.define(&name, item),
                        None => self.pc = target,
                    }
                }
                Op::RepeatCountSetup { end_pc } => {
                    let count = self.pop(line, col)?;
                    let n: i64 = match count {
                        Value::Number(x) => {
                            if x.fract() != 0.0 || x < 0.0 {
                                return Err(RioError::typ(
                                    "عدد مرات التكرار يجب أن يكون عددا صحيحا غير سالب",
                                    line,
                                    col,
                                ));
                            }
                            x as i64
                        }
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "عدد مرات التكرار يجب أن يكون عددا، وليس {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    };
                    if n as usize > crate::interpreter::MAX_LOOP_ITERATIONS {
                        return Err(RioError::runtime(
                            "عدد مرات التكرار كبير جدا",
                            line,
                            col,
                        ));
                    }
                    self.loop_states.push(LoopState::Count {
                        remaining: n as usize,
                        end_pc,
                        continue_pc: idx + 1,
                    });
                }
                Op::RepeatCountNext(target) => {
                    let done = match self.loop_states.last_mut() {
                        Some(LoopState::Count { remaining, .. }) => {
                            if *remaining == 0 {
                                true
                            } else {
                                *remaining -= 1;
                                false
                            }
                        }
                        _ => true,
                    };
                    if done {
                        self.pc = target;
                    }
                }
                Op::WhileBegin { end_pc } => {
                    self.loop_states.push(LoopState::While {
                        iterations: 0,
                        end_pc,
                        continue_pc: idx + 1,
                    });
                }
                Op::WhileTick => {
                    let over = match self.loop_states.last_mut() {
                        Some(LoopState::While { iterations, .. }) => {
                            *iterations += 1;
                            *iterations > crate::interpreter::MAX_LOOP_ITERATIONS
                        }
                        _ => false,
                    };
                    if over {
                        return Err(RioError::runtime(
                            "تم تجاوز حد التكرار، هل الحلقة لا تنتهي؟",
                            line,
                            col,
                        ));
                    }
                }
                Op::LoopEnd => {
                    let _ = self.loop_states.pop();
                }
                Op::Break => {
                    let end_pc = match self.loop_states.last() {
                        Some(LoopState::For { end_pc, .. })
                        | Some(LoopState::Count { end_pc, .. })
                        | Some(LoopState::While { end_pc, .. }) => *end_pc,
                        None => usize::MAX,
                    };
                    if end_pc == usize::MAX {
                        self.pending_flow = Some(FlowResume::NoLoop("اكسر خارج حلقة"));
                    } else {
                        self.pending_flow = Some(FlowResume::Break { end_pc });
                    }
                    match self.unwind_flow(line, col)? {
                        UnwindOutcome::Continue => {}
                        UnwindOutcome::ReturnNow => {
                            return Ok(StepFlow::Returned);
                        }
                    }
                }
                Op::Continue => {
                    let target_pc = match self.loop_states.last() {
                        Some(LoopState::For { continue_pc, .. })
                        | Some(LoopState::Count { continue_pc, .. })
                        | Some(LoopState::While { continue_pc, .. }) => *continue_pc,
                        None => usize::MAX,
                    };
                    if target_pc == usize::MAX {
                        self.pending_flow = Some(FlowResume::NoLoop("تابع خارج حلقة"));
                    } else {
                        self.pending_flow =
                            Some(FlowResume::Continue { target_pc });
                    }
                    match self.unwind_flow(line, col)? {
                        UnwindOutcome::Continue => {}
                        UnwindOutcome::ReturnNow => {
                            return Ok(StepFlow::Returned);
                        }
                    }
                }
                Op::TryBegin { except_pc, finally_pc, after_pc } => {
                    self.handlers.push(Handler {
                        except_pc,
                        finally_pc,
                        after_pc,
                        stack_depth: self.stack.len(),
                        loop_depth: self.loop_states.len(),
                        finally_depth: self.finally_stack.len(),
                    });
                }
                Op::EndTryBlock { finally_pc, after_pc } => {
                    let _ = self.handlers.pop();
                    match finally_pc {
                        Some(fin_pc) => {
                            self.finally_stack.push(FinallyCtx {
                                resume: FlowResume::After(after_pc),
                            });
                            self.pc = fin_pc;
                        }
                        None => {
                            self.pc = after_pc;
                        }
                    }
                }
                Op::EndFinally => {
                    // نهاية جسم أخيرا: نفّذ ما كان معلقا.
                    // (السياق يُدفع دائما قبل دخول أخيرا —
                    // من EndTryBlock أو من on_error؛ فإن غاب
                    // دفاعيا، فلا شيء معلق.)
                    if let Some(ctx) = self.finally_stack.pop() {
                        self.pending_flow = Some(ctx.resume);
                        match self.unwind_flow(line, col)? {
                            UnwindOutcome::Continue => {}
                            UnwindOutcome::ReturnNow => {
                                return Ok(StepFlow::Returned);
                            }
                        }
                    }
                }
                Op::In | Op::NotIn => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    let found = match (&l, &r) {
                        (item, Value::List(items)) => items.iter().any(|x| x == item),
                        (Value::Str(needle), Value::Str(hay)) => {
                            hay.contains(needle.as_str())
                        }
                        (Value::Str(key), Value::Dict(pairs)) => {
                            pairs.iter().any(|(k, _)| k == key)
                        }
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
                    let result = if matches!(instr.op, Op::In) { found } else { !found };
                    self.stack.push(Value::Bool(result));
                }
            }
        Ok(StepFlow::Next)
    }

    /// ك-1.2: يسحب شرط حلقة مع رسالة المفسر («شرط الحلقة
    /// يجب أن يكون منطقيا») — غير رسالة pop_bool العامة.
    fn pop_bool_loop(&mut self, line: usize, col: usize) -> RioResult<bool> {
        match self.pop(line, col)? {
            Value::Bool(b) => Ok(b),
            other => Err(RioError::typ(
                format!("شرط الحلقة يجب أن يكون منطقيا، وليس {}", other.type_name()),
                line,
                col,
            )),
        }
    }

    /// ك-1.2: اعتراض خطأ وقت تشغيل: يبحث عن أقرب معالج
    /// حاول يمسكه (الأعمق أولا). إن وُجد، يعيد التموضع
    /// إلى جسم ماعدا أو أخيرا ويعيد Ok(()) لتواصل run()؛
    /// وإن لم يوجد، يعيد الخطأ لينتقل إلى المتصل.
    /// قاعدة التبديل (من المفسر): الخطأ الذي يهرب من جسم
    /// أخيرا يُتجاهل إن كان ذلك الجسم نفسه دخل من خطأ —
    /// الخطأ الأصلي (الأبعد) هو الذي يُرفع.
    fn on_error(&mut self, mut e: RioError) -> RioResult<()> {
        while let Some(h) = self.handlers.pop() {
            // الخطأ هرب من كل جسم أخيرا بدأ بعد هذا المعالج:
            // اسحبها كلها؛ وإن كان أحدها يحمل خطأ معلقا
            // (Raise)، فالخطأ الأصلي هو الذي يُرفع (آخر
            // ما يُسحب — أي الأبعد — يفوز).
            while self.finally_stack.len() > h.finally_depth {
                match self.finally_stack.pop() {
                    Some(FinallyCtx { resume: FlowResume::Raise(orig) }) => e = orig,
                    _ => {}
                }
            }
            if let Some(except_pc) = h.except_pc {
                // ماعدا موجود: احمِ أخيرا بحارس (فإن أخطأ
                // جسم ماعدا نفسه، شُغّل أخيرا ثم أُعيد رفع
                // خطئه).
                // الدفعة 1: الخطأ الممسوك رمز خروج 1 —
                // يُضبط قبل جسم ماعدا، فتقرأه
                // `رمز_الخروج()` داخله.
                self.last_exit_code = 1;
                self.handlers.push(Handler {
                    except_pc: None,
                    finally_pc: h.finally_pc,
                    after_pc: h.after_pc,
                    stack_depth: h.stack_depth,
                    loop_depth: h.loop_depth,
                    finally_depth: h.finally_depth,
                });
                self.stack.truncate(h.stack_depth);
                self.loop_states.truncate(h.loop_depth);
                self.pc = except_pc;
                return Ok(());
            }
            if let Some(fin_pc) = h.finally_pc {
                // بلا ماعدا: شغّل أخيرا ثم أعد رفع الخطأ.
                self.finally_stack.push(FinallyCtx { resume: FlowResume::Raise(e) });
                self.stack.truncate(h.stack_depth);
                self.loop_states.truncate(h.loop_depth);
                self.pc = fin_pc;
                return Ok(());
            }
            // دفاعيا: معالج بلا ماعدا ولا أخيرا (المحلل
            // يمنعه) — واصل البحث للخارج.
        }
        // لا معالج: اسحب كل أخيرا المتبقية بنفس قاعدة
        // التبديل، ثم أعد رفع الخطأ النهائي.
        while let Some(ctx) = self.finally_stack.pop() {
            if let FlowResume::Raise(orig) = ctx.resume {
                e = orig;
            }
        }
        Err(e)
    }

    /// ك-1.2: يفكّ معالجات حاول التي يهرب منها التدفق
    /// المعلق (اكسر/تابع/ارجع)، مشغّلا أخيرا لكل منها
    /// بالترتيب من الأعمق إلى الخارج. قاعدة الهروب:
    /// ارجع يهرب من كل معالج؛ اكسر/تابع يهربان من معالج
    /// بدأ بعد دخول الحلقة المستهدفة فقط (أي أن الحلقة
    /// خارج حاول)، فمعالج داخل الحلقة لا يُفكّ.
    fn unwind_flow(&mut self, line: usize, col: usize) -> RioResult<UnwindOutcome> {
        let flow = match self.pending_flow.take() {
            Some(f) => f,
            None => return Ok(UnwindOutcome::Continue),
        };
        loop {
            let handler = match self.handlers.last() {
                Some(h) => h.clone(),
                None => return self.perform_flow(flow, line, col),
            };
            let escapes = match &flow {
                FlowResume::Return | FlowResume::NoLoop(_) | FlowResume::Exit(_) => true,
                FlowResume::Break { .. } | FlowResume::Continue { .. } => {
                    handler.loop_depth >= self.loop_states.len()
                }
                FlowResume::After(_) | FlowResume::Raise(_) => false,
            };
            if !escapes {
                return self.perform_flow(flow, line, col);
            }
            let h = self.handlers.pop().expect("handler was just seen");
            if let Some(fin_pc) = h.finally_pc {
                self.finally_stack.push(FinallyCtx { resume: flow });
                self.pc = fin_pc;
                return Ok(UnwindOutcome::Continue);
            }
            // بلا أخيرا: واصل فكّ المعالجات الخارجية.
        }
    }

    /// ك-1.2: ينفّذ تدفقا معلقا بعد أن فُكّت كل معالجات
    /// حاول التي يهرب منها (أو لم يكن هناك معالج).
    fn perform_flow(
        &mut self,
        flow: FlowResume,
        line: usize,
        col: usize,
    ) -> RioResult<UnwindOutcome> {
        match flow {
            FlowResume::After(pc) => {
                self.pc = pc;
                Ok(UnwindOutcome::Continue)
            }
            FlowResume::Raise(e) => Err(e),
            FlowResume::Break { end_pc } => {
                // القفز إلى LoopEnd؛ هي من تسحب حالة الحلقة.
                self.pc = end_pc;
                Ok(UnwindOutcome::Continue)
            }
            FlowResume::Continue { target_pc } => {
                self.pc = target_pc;
                Ok(UnwindOutcome::Continue)
            }
            FlowResume::Return => {
                self.return_value = self.pending_return.take();
                Ok(UnwindOutcome::ReturnNow)
            }
            // الدفعة 1: خروج معلق — يُثبَّت كرمز خروج
            // البرنامج وينتهي التشغيل (بنجاح برمزه).
            FlowResume::Exit(code) => {
                self.exit_code = Some(code);
                Ok(UnwindOutcome::ReturnNow)
            }
            FlowResume::NoLoop(msg) => Err(RioError::runtime(msg, line, col)),
        }
    }
}

/// ك-2 (مهمة 7): جلسة REPL دائمة على المترجم — نسخة VM
/// واحدة يبقى جدول عمومياتها (والوحدات المحمّلة وسياسة
/// الملفات ومجلد الجلسة) عبر استدعاءات feed()، وكل كتلة
/// تُترجم وتُحمَّل عليها (DESIGN §39.3). مرآة واجهة المفسر
/// التي يستخدمها REPL (run / eval_one / call_builtin /
/// المجلد / الإخراج / التصفير)، فتبديل المحرك في repl.rs
/// لا يغيّر منطق الجلسة.
pub struct VmSession {
    vm: Vm,
    program_dir: std::path::PathBuf,
    network_allowed: bool,
    server_allowed: bool,
}

impl VmSession {
    pub fn new(program_dir: std::path::PathBuf) -> Self {
        let empty = CompiledProgram {
            code: Vec::new(),
            functions: HashMap::new(),
            closures: HashMap::new(),
        };
        Self {
            vm: Vm::new(empty, program_dir.clone()),
            program_dir,
            network_allowed: false,
            server_allowed: false,
        }
    }

    /// شغّل برنامجا كاملا على الجلسة الدائمة: ترجمة ثم
    /// تحميل ثم تشغيل. خطأ الترجمة يسبق التحميل، فسطر
    /// فاشل لا يلوّث الجلسة.
    pub fn run(&mut self, program: &[Stmt]) -> RioResult<()> {
        // Per-instance network flag (sessions): same sync as
        // Interpreter::run — the gate always reflects the
        // session that is currently evaluating.
        crate::network::set_network_allowed(self.network_allowed);
        // Meeting 27: the server flag, same per-instance
        // pattern — synced onto the VM before evaluation.
        self.vm.set_server_allowed(self.server_allowed);
        // مهمة 10: سياسة العمليات نفس النمط.
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.vm.file_policy.borrow().trusted,
            self.vm.cwd.borrow().clone(),
        );
        let compiled = Compiler::compile_program(program)?;
        self.vm.load(compiled);
        self.vm.run()
    }

    /// قيّم تعبيرا واحدا وأعد قيمته (للتعبير الأخير في REPL).
    pub fn eval_one(&mut self, expr: &Expr) -> RioResult<Value> {
        crate::network::set_network_allowed(self.network_allowed);
        self.vm.set_server_allowed(self.server_allowed);
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.vm.file_policy.borrow().trusted,
            self.vm.cwd.borrow().clone(),
        );
        let compiled = Compiler::compile_expr_program(expr)?;
        self.vm.load(compiled);
        self.vm.run()?;
        Ok(self.vm.pop_result().unwrap_or(Value::Nil))
    }

    /// استدعاء دالة مدمجة مباشرة (اختصارات REPL).
    pub fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        crate::network::set_network_allowed(self.network_allowed);
        self.vm.set_server_allowed(self.server_allowed);
        crate::processes::set_process_policy(
            self.program_dir.clone(),
            self.vm.file_policy.borrow().trusted,
            self.vm.cwd.borrow().clone(),
        );
        self.vm.call_builtin(name, args, line, col)
    }

    /// الدفعة 1 — رموز الخروج (اجتماع 54 §69): يأخذ
    /// رمز الخروج المعلق مرة واحدة، مرآة المفسر.
    pub fn take_exit_code(&mut self) -> Option<u8> {
        self.vm.take_exit_code()
    }

    /// الدفعة 1: رمز آخر أمر مكتمل.
    pub fn last_exit_code(&self) -> u8 {
        self.vm.last_exit_code()
    }

    /// الدفعة 1: ضبط رمز آخر أمر (تزامن REPL).
    pub fn set_last_exit_code(&mut self, code: u8) {
        self.vm.set_last_exit_code(code);
    }

    /// المرحلة (ج): ضبط رموز مراحل آخر أنبوب.
    pub fn set_pipe_status(&mut self, codes: Vec<u8>) {
        self.vm.set_pipe_status(codes);
    }

    /// المجلد الحالي في صيغة العرض (~، ~/...) — للموجّه.
    pub fn current_dir_display(&self) -> String {
        self.vm
            .file_policy
            .borrow()
            .display_path(&self.vm.cwd.borrow())
    }

    /// المرحلة (د) — الإكمال (اجتماع 67 §82):
    /// أسماء متغيرات/دوال الجلسة والمجلد الحالي —
    /// تفويض إلى الآلة، للإكمال في التفاعلي.
    pub fn session_variable_names(&self) -> Vec<String> {
        self.vm.session_variable_names()
    }

    pub fn session_function_names(&self) -> Vec<String> {
        self.vm.session_function_names()
    }

    pub fn cwd_path(&self) -> std::path::PathBuf {
        self.vm.cwd_path()
    }

    /// خذ مخرجات التشغيل الأخير (اطبع) وصفّرها.
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.vm.output)
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.vm.set_trusted(trusted);
    }

    /// Meeting 50 §64 (items 94/96): set the read-only
    /// downloads root for this session — مرآة set_trusted.
    pub fn set_downloads(&mut self, path: std::path::PathBuf) {
        self.vm.set_downloads(path);
    }

    /// Meeting 22: same per-instance pattern as the
    /// interpreter — the flag lives on this session and is
    /// synced to the process-wide gate at the start of run /
    /// eval_one / call_builtin, so one console's approval
    /// never leaks into another console.
    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.network_allowed = allowed;
        crate::network::set_network_allowed(allowed);
    }

    /// This session's own network permission.
    pub fn network_allowed(&self) -> bool {
        self.network_allowed
    }

    /// Meeting 27: server capability (--اسمح-بالخادم).
    /// Same per-instance pattern as the network flag.
    pub fn set_server_allowed(&mut self, allowed: bool) {
        self.server_allowed = allowed;
        self.vm.set_server_allowed(allowed);
    }

    /// اجتماع 32، السؤال الخامس: ملف المصدر الجاري —
    /// يُمرَّر إلى VM الجلسة (load لا يمسه).
    pub fn set_source_file(&mut self, file: &str) {
        self.vm.set_source_file(file);
    }

    /// اجتماع 32، السؤال الثامن: بث حي لمخرجات `اطبع`
    /// — يُمرَّر إلى VM الجلسة (load لا يمسه).
    pub fn set_live_output(&mut self, live: bool) {
        self.vm.set_live_output(live);
    }

    /// المرحلة (ج) — البث الحي مفعّل؟ (مرآة الضابط).
    pub fn live_output(&self) -> bool {
        self.vm.live_output()
    }

    /// This session's own server permission.
    pub fn server_allowed(&self) -> bool {
        self.server_allowed
    }

    pub fn allow_local_network_host(&mut self, host: &str) {
        crate::network::allow_local_host(host);
    }

    /// جلسة جديدة: VM جديدة فارغة بنفس مجلد البرنامج،
    /// مع استعادة الموثوقية والشبكة (:مسح).
    pub fn reset(&mut self) {
        let trusted = self.vm.file_policy.borrow().trusted;
        let downloads = self.vm.file_policy.borrow().downloads.clone();
        let network_allowed = self.network_allowed;
        let server_allowed = self.server_allowed;
        let empty = CompiledProgram {
            code: Vec::new(),
            functions: HashMap::new(),
            closures: HashMap::new(),
        };
        self.vm = Vm::new(empty, self.program_dir.clone());
        self.vm.set_trusted(trusted);
        // Meeting 50 §64: the downloads root survives
        // :مسح like the other session capabilities.
        if let Some(d) = downloads {
            self.vm.set_downloads(d);
        }
        self.network_allowed = network_allowed;
        self.server_allowed = server_allowed;
        self.vm.set_server_allowed(server_allowed);
    }
}

// --- سياق الدوال الأصلية (خادم HTTPS، اجتماع 27) ---
//
// المترجم يستدعي المعالج عبر call_value نفسه: دالة
// مستخدم → نسخة VM فرعية (تنفيذ معزول: مكدس وعمق
// ومخرجات محلية، تُدمَج مخرجاتها في مخرجات المتصل
// بعد العودة)، وطريقة مربوطة → call_method_value،
// ودالة أصلية → استدعاء مباشر. البيئة المعجمية
// الملتقَطة مشتركة بالتصميم (الإغلاقات)، أما حالة
// التنفيذ فمعزولة لكل طلب — القراءة الصادقة لعبارة
// «لا حالة نيفا مشتركة بين الاتصالات» في §42.
impl crate::stdlib::HandlerCaller for Vm {
    fn call_handler(
        &mut self,
        func: &Value,
        arg: Value,
        line: usize,
        col: usize,
    ) -> (RioResult<Value>, String) {
        let before = self.output.len();
        let res = self.call_value(func.clone(), vec![arg], line, col);
        let produced = self.output[before..].to_string();
        (res, produced)
    }

    fn server_allowed(&self) -> bool {
        self.server_allowed
    }

    fn server_file_policy(&self) -> crate::permissions::FilePolicy {
        self.file_policy.borrow().clone()
    }

    fn server_cwd(&self) -> std::path::PathBuf {
        self.cwd.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;

    fn run_vm_src(src: &str) -> (String, String, bool) {
        run_vm_src_dir(
            src,
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )
    }

    /// مهمة 4: مثل run_vm_src لكن بمجلد برنامج صريح،
    /// لاختبارات الوحدات الملفية (تُحلّ نسبة إليه).
    fn run_vm_src_dir(src: &str, dir: &std::path::Path) -> (String, String, bool) {
        let tokens = match tokenize(src) {
            Ok(t) => t,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let compiled = match Compiler::compile_program(&program) {
            Ok(c) => c,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let mut vm = Vm::new(compiled, dir.to_path_buf());
        match vm.run() {
            Ok(()) => (vm.output, String::new(), true),
            Err(e) => (vm.output, e.to_string(), false),
        }
    }

    /// مهمة 4: شغّل برنامجا رئيسيا مع ملفات وحدات في مجلد
    /// مؤقت فريد (متوازي-آمن)، وأعد (مخرجات VM، خطؤها،
    /// نجاحها). المجلد يُمسح بعد التشغيل.
    fn run_vm_src_with_files(
        main_src: &str,
        files: &[(&str, &str)],
    ) -> (String, String, bool) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "niva-vm-modules-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            std::fs::write(dir.join(name), src).unwrap();
        }
        let result = run_vm_src_dir(main_src, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// مهمة 4: شغّل البرنامج نفسه على المفسر (المرجع)
    /// وعلى VM في مجلد الوحدات نفسه، وأعد (المخرجات،
    /// الخطأ، النجاح) لكل منهما — لمطابقة الوحدات.
    fn run_both_with_files(
        main_src: &str,
        files: &[(&str, &str)],
    ) -> ((String, String, bool), (String, String, bool)) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "niva-vm-modules-both-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            std::fs::write(dir.join(name), src).unwrap();
        }
        let toks = tokenize(main_src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = crate::interpreter::Interpreter::with_program_dir(dir.clone());
        let ires = interp.run(&prog);
        let iout = (
            interp.output.clone(),
            match &ires {
                Ok(()) => String::new(),
                Err(e) => e.to_string(),
            },
            ires.is_ok(),
        );
        let vout = run_vm_src_dir(main_src, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        (iout, vout)
    }

    /// مهمة 4: مطابقة وحدات — نفس البرنامج والملفات في
    /// الوضعين: نفس المخرجات ونفس النجاح، ويساوي المتوقع.
    fn assert_module_parity(main_src: &str, files: &[(&str, &str)], expected: &str) {
        let ((iout, ierr, iok), (vout, verr, vok)) = run_both_with_files(main_src, files);
        assert!(iok, "interpreter failed on: {} (err={})", main_src, ierr);
        assert!(vok, "VM failed on: {} (err={})", main_src, verr);
        assert_eq!(iout, vout, "interpreter vs VM differ on: {}", main_src);
        assert_eq!(vout, expected);
    }

    /// مهمة 4: مطابقة أخطاء وحدات — نفس الخطأ في
    /// الوضعين (نفس الرسالة)، ونفس الفشل.
    fn assert_module_error_parity(main_src: &str, files: &[(&str, &str)], needle: &str) {
        let ((iout, ierr, iok), (vout, verr, vok)) = run_both_with_files(main_src, files);
        assert!(!iok, "interpreter unexpectedly passed on: {}", main_src);
        assert!(!vok, "VM unexpectedly passed on: {}", main_src);
        assert_eq!(iout, vout, "interpreter vs VM output differ on: {}", main_src);
        assert!(ierr.contains(needle), "interpreter err: {}", ierr);
        assert!(verr.contains(needle), "VM err: {}", verr);
    }

    #[test]
    fn vm_arithmetic_and_variables() {
        let (out, _, ok) = run_vm_src("س = ٣ + ٤ * ٢\nاطبع س\nاطبع س * ٢\n");
        assert!(ok);
        assert_eq!(out, "١١\n٢٢\n");
    }

    #[test]
    fn vm_functions_and_recursion() {
        let (out, _, ok) = run_vm_src(
            "دالة جمع(أ، ب):\n    ارجع أ + ب\nدالة عاملي(ن):\n    إذا ن <= ١:\n        ارجع ١\n    ارجع ن * عاملي(ن - ١)\nاطبع جمع(٢، ٣)\nاطبع عاملي(٥)\n",
        );
        assert!(ok);
        assert_eq!(out, "٥\n١٢٠\n");
    }

    #[test]
    fn vm_lists_dicts_index_and_assignment() {
        let (out, _, ok) = run_vm_src(
            "قائمة = [١، ٢، ٣]\nاطبع قائمة[٠]\nقائمة[١] = ٩٩\nاطبع قائمة\nقاموس = {\"اسم\": \"محمود\"، \"عمر\": ٢٣}\nاطبع قاموس[\"اسم\"]\nقاموس[\"مدينة\"] = \"القاهرة\"\nاطبع قاموس[\"مدينة\"]\nاطبع \"محمود\"[٠]\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n[١, ٩٩, ٣]\nمحمود\nالقاهرة\nم\n");
    }

    #[test]
    fn vm_list_copy_semantics_match_interpreter() {
        // ب = قائمة copies (value type), so mutating ب must
        // NOT affect قائمة; only قائمة[..] = .. mutates it.
        let (out, _, ok) = run_vm_src(
            "قائمة = [١، ٢]\nب = قائمة\nب[٠] = ٩٩\nاطبع قائمة\nاطبع ب\n",
        );
        assert!(ok);
        assert_eq!(out, "[١, ٢]\n[٩٩, ٢]\n");
    }

    #[test]
    fn vm_index_out_of_range_is_arabic_error() {
        let (out, err, ok) = run_vm_src("قائمة = [١]\nاطبع قائمة[٥]\n");
        assert!(!ok);
        assert!(err.contains("فهرس خارج النطاق"));
        assert_eq!(out, "");
    }

    #[test]
    fn vm_pure_builtins_match_interpreter() {
        let (out, _, ok) = run_vm_src(
            "قائمة = [٣، ١، ٢]\nاطبع طول(قائمة)\nاطبع مجموع(قائمة)\nاطبع أقصى(قائمة)\nاطبع أدنى(قائمة)\nاطبع مطلق(-٥)\nاطبع عدد(\"٤٢\")\nاطبع نص(٥)\nاطبع نوع(قائمة)\nاطبع مدى(٣)\nاطبع مدى(١، ٥، ٢)\nاطبع مدى(٥، ٠، -٢)\nاطبع طول(\"مرحبا\")\n",
        );
        assert!(ok);
        assert_eq!(
            out,
            "٣\n٦\n٣\n١\n٥\n٤٢\n٥\nقائمة\n[٠, ١, ٢]\n[١, ٣]\n[٥, ٣, ١]\n٥\n"
        );
    }

    #[test]
    fn vm_builtin_arity_and_type_errors() {
        let (_, err, ok) = run_vm_src("اطبع طول()\n");
        assert!(!ok);
        assert!(err.contains("تتوقع 1 معاملات"));
        let (_, err2, ok2) = run_vm_src("اطبع مجموع([١، \"أ\"])\n");
        assert!(!ok2);
        assert!(err2.contains("مجموع يتطلب قائمة أعداد"));
    }

    #[test]
    fn vm_user_function_shadows_builtin() {
        // A user function named طول wins over the builtin,
        // like in the interpreter.
        let (out, _, ok) = run_vm_src(
            "دالة طول(س):\n    ارجع ٩٩\nاطبع طول([١، ٢])\n",
        );
        assert!(ok);
        assert_eq!(out, "٩٩\n");
    }

    #[test]
    fn vm_variable_shadows_builtin() {
        // A variable named طول wins at runtime, and calling
        // it is a type error, like in the interpreter.
        let (_, err, ok) = run_vm_src("طول = ٥\nاطبع طول([١])\n");
        assert!(!ok);
        assert!(err.contains("ليست دالة"));
    }

    #[test]
    fn vm_classes_fields_and_methods() {
        let (out, _, ok) = run_vm_src(
            "فئة نقطة:\n    دالة أنشئ(س، ص):\n        نفس.س = س\n        نفس.ص = ص\n    دالة مجموع():\n        ارجع نفس.س + نفس.ص\n    دالة زد(مقدار):\n        نفس.س = نفس.س + مقدار\n        ارجع نفس.س\nك = أنشئ نقطة(٣، ٤)\nاطبع ك.س\nاطبع ك.مجموع()\nاطبع ك.زد(١٠)\nاطبع ك.س\n",
        );
        assert!(ok);
        assert_eq!(out, "٣\n٧\n١٣\n١٣\n");
    }

    #[test]
    fn vm_classes_inheritance_and_interfaces() {
        let (out, _, ok) = run_vm_src(
            "واجهة قابل_للطباعة:\n    دالة اطبع_نفسك():\n        مرر\nفئة شخص:\n    دالة أنشئ(اسم):\n        نفس.اسم = اسم\n    دالة عرّف():\n        ارجع نفس.اسم\n    دالة اطبع_نفسك():\n        اطبع نفس.اسم\nفئة طالب ورث شخص واجهة قابل_للطباعة:\n    دالة ادرس():\n        ارجع \"يدرس\"\n    دالة عرّف():\n        ارجع \"طالب: \" + نفس.اسم\nط = أنشئ طالب(\"محمود\")\nاطبع ط.عرّف()\nاطبع ط.ادرس()\nط.اطبع_نفسك()\n",
        );
        assert!(ok);
        assert_eq!(out, "طالب: محمود\nيدرس\nمحمود\n");
    }

    #[test]
    fn vm_class_field_shadows_method_and_bound_method_variable() {
        let (out, _, ok) = run_vm_src(
            "فئة صندوق:\n    دالة قيمة():\n        ارجع ٧\nص = أنشئ صندوق()\nص.قيمة = ٩٩\nاطبع ص.قيمة\nم = ص.قيمة\n",
        );
        assert!(ok);
        // Field shadows the method: ص.قيمة is data, not a method.
        assert_eq!(out, "٩٩\n");
        let (out2, _, ok2) = run_vm_src(
            "فئة صندوق:\n    دالة قيمة():\n        ارجع ٧\nص = أنشئ صندوق()\nم = ص.قيمة\nاطبع م()\n",
        );
        assert!(ok2);
        assert_eq!(out2, "٧\n");
    }

    #[test]
    fn vm_method_sees_defining_scope() {
        // مهمة 3: الطريقة ترى المتغير العام (سلسلة النطاق
        // المعرِّف) — كان خطأ «متغير غير معرف» في VM.
        let (out, _, ok) = run_vm_src(
            "س = ٥\nفئة عداد:\n    دالة اقرأ():\n        ارجع س\nع = أنشئ عداد()\nاطبع ع.اقرأ()\n",
        );
        assert!(ok);
        assert_eq!(out, "٥\n");
    }

    #[test]
    fn vm_method_closes_over_enclosing_locals() {
        // مهمة 3: فئة معرَّفة داخل دالة، وطريقتها تغلق
        // على متغيرات الدالة — وكان الاستدعاء يفشل لأن
        // الفئة ليست في جدول VM أصلا.
        let (out, _, ok) = run_vm_src(
            "دالة اصنع():\n    س = ٧\n    فئة ص:\n        دالة اقرأ():\n            ارجع س\n    ارجع أنشئ ص()\nع = اصنع()\nاطبع ع.اقرأ()\n",
        );
        assert!(ok);
        assert_eq!(out, "٧\n");
    }

    #[test]
    fn vm_method_mutates_captured_variable() {
        // التعيين المعجمي عبر سلسلة النطاق الملتقَطة.
        let (out, _, ok) = run_vm_src(
            "دالة اصنع():\n    س = ٠\n    فئة عداد:\n        دالة زد():\n            س = س + ١\n            ارجع س\n    ارجع أنشئ عداد()\nع = اصنع()\nاطبع ع.زد()\nاطبع ع.زد()\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n٢\n");
    }

    #[test]
    fn vm_inherited_method_keeps_base_scope() {
        // الطريقة الموروثة ترى نطاق تعريف الفئة الأب،
        // لا نطاق تعريف الفئة الابن — كما في المفسر.
        let (out, _, ok) = run_vm_src(
            "دالة اصنع_أساس():\n    س = ١١\n    فئة أساس:\n        دالة اقرأ(): عدد:\n            ارجع س\n    ارجع أساس\nأساس = اصنع_أساس()\nفئة فرعي ورث أساس:\n    دالة زد():\n        ارجع ١\nع = أنشئ فرعي()\nاطبع ع.اقرأ()\nاطبع ع.زد()\n",
        );
        assert!(ok);
        assert_eq!(out, "١١\n١\n");
    }

    #[test]
    fn vm_function_return_type_checked() {
        // مهمة 3: نوع الإرجاع يُفحص في VM كما في المفسر.
        let (_, err, ok) = run_vm_src(
            "دالة احصل(): عدد:\n    ارجع \"نص\"\nاطبع احصل()\n",
        );
        assert!(!ok);
        assert!(
            err.contains("الدالة احصل معلنة أن ترجع عدد، لكن رجعت نص"),
            "err: {}",
            err
        );
    }

    #[test]
    fn vm_method_return_type_checked() {
        let (_, err, ok) = run_vm_src(
            "فئة ص:\n    دالة قيمة(): عدد:\n        ارجع \"نص\"\nع = أنشئ ص()\nاطبع ع.قيمة()\n",
        );
        assert!(!ok);
        assert!(
            err.contains("الطريقة قيمة معلنة أن ترجع عدد، لكن رجعت نص"),
            "err: {}",
            err
        );
    }

    #[test]
    fn vm_missing_return_with_declared_type_is_error() {
        let (_, err, ok) = run_vm_src(
            "دالة احصل(): عدد:\n    مرر\nاطبع احصل()\n",
        );
        assert!(!ok);
        assert!(
            err.contains("الدالة احصل معلنة أن ترجع عدد، لكن لم ترجع شيئا"),
            "err: {}",
            err
        );
        let (_, err2, ok2) = run_vm_src(
            "فئة ص:\n    دالة قيمة(): عدد:\n        مرر\nع = أنشئ ص()\nاطبع ع.قيمة()\n",
        );
        assert!(!ok2);
        assert!(
            err2.contains("الطريقة قيمة معلنة أن ترجع عدد، لكن لم ترجع شيئا"),
            "err: {}",
            err2
        );
    }

    #[test]
    fn vm_return_type_any_and_matching_pass() {
        let (out, _, ok) = run_vm_src(
            "دالة نقل(ق: أي): أي:\n    ارجع ق\nاطبع نقل(٧)\nاطبع نقل(\"أ\")\nدالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\nاطبع جمع(٣، ٤)\n",
        );
        assert!(ok);
        assert_eq!(out, "٧\nأ\n٧\n");
    }

    #[test]
    fn vm_method_param_type_checked() {
        let (_, err, ok) = run_vm_src(
            "فئة ص:\n    دالة خذ(س: عدد):\n        ارجع س\nع = أنشئ ص()\nاطبع ع.خذ(\"نص\")\n",
        );
        assert!(!ok);
        assert!(
            err.contains("معامل س في الطريقة خذ معلن عدد، لكن القيمة نص"),
            "err: {}",
            err
        );
    }

    #[test]
    fn vm_table_call_param_type_checked() {
        // مسار الجدول (استدعاء أمامي): الأنواع تُفحص
        // فيه أيضا بعد مهمة 3.
        let (_, err, ok) = run_vm_src(
            "اطبع جمع(\"نص\")\nدالة جمع(أ: عدد): عدد:\n    ارجع أ\n",
        );
        assert!(!ok);
        assert!(
            err.contains("معامل أ في الدالة جمع معلن عدد، لكن القيمة نص"),
            "err: {}",
            err
        );
    }

    #[test]
    fn vm_builtin_string_methods() {
        // مهمة 3: طرق النصوص المدمجة في VM.
        let (out, _, ok) = run_vm_src(
            "اطبع \"مرحبا\".طول()\nاطبع \"Hello\".كبير()\nاطبع \"Hello\".صغير()\nاطبع \"  محمود  \".قص()\nاطبع \"أهلا محمود\".استبدل(\"محمود\"، \"سارة\")\nاطبع \"أ، ب، ج\".قسم(\"، \")\nاطبع \"مرحبا\".يحتوي(\"رحب\")\n",
        );
        assert!(ok);
        assert_eq!(
            out,
            "٥\nHELLO\nhello\nمحمود\nأهلا سارة\n[\"أ\", \"ب\", \"ج\"]\nصحيح\n"
        );
    }

    #[test]
    fn vm_builtin_list_methods_write_back() {
        // مهمة 3: أضف/أزل تكتبان رجعيا في المتغير.
        let (out, _, ok) = run_vm_src(
            "ق = [١، ٢، ٣]\nاطبع ق.طول()\nق.أضف(٤)\nاطبع ق\nاطبع ق.أزل()\nاطبع ق\nاطبع ق.يحتوي(٢)\n",
        );
        assert!(ok);
        assert_eq!(out, "٣\n[١, ٢, ٣, ٤]\n٤\n[١, ٢, ٣]\nصحيح\n");
    }

    #[test]
    fn vm_builtin_list_mutation_needs_plain_variable() {
        let (_, err, ok) = run_vm_src("اطبع [١، ٢].أضف(٣)\n");
        assert!(!ok);
        assert!(err.contains("تحتاج متغيرا عاديا"), "err: {}", err);
        let (_, err2, ok2) = run_vm_src("ق = []\nاطبع ق.أزل()\n");
        assert!(!ok2);
        assert!(
            err2.contains("لا يمكن الإزالة من قائمة فارغة"),
            "err: {}",
            err2
        );
    }

    #[test]
    fn vm_dot_call_evaluates_args_first() {
        // مهمة 3: ترتيب تقييم المفسر — المعاملات أولا
        // ثم الهدف (كان العكس في VM لاستدعاءات النقطة).
        let (out, _, ok) = run_vm_src(
            "دالة سجل(س):\n    اطبع س\n    ارجع س\nدالة اصنع():\n    اطبع \"هدف\"\n    ارجع أنشئ ص()\nفئة ص:\n    دالة قيمة(س):\n        ارجع س\nاطبع اصنع().قيمة(سجل(\"معامل\"))\n",
        );
        assert!(ok);
        assert_eq!(out, "معامل\nهدف\nمعامل\n");
    }

    #[test]
    fn vm_forward_call_from_method_body_documented() {
        // قيد موثَّق (قرار أ، §39): الاستدعاء من داخل
        // جسم طريقة يجيب من الجدول المترجَم، فينجح
        // الاستدعاء الأمامي في VM بينما يرفضه المفسر.
        // مسار CLI لا يصل إلى VM هنا أصلا (المحلل
        // الدلالي يرفض أولا)، فيرى المستخدم خطأ المفسر.
        let (out, _, ok) = run_vm_src(
            "فئة ص:\n    دالة ناد():\n        ارجع لاحقة()\nع = أنشئ ص()\nاطبع ع.ناد()\nدالة لاحقة():\n    ارجع ٤٢\n",
        );
        assert!(ok);
        assert_eq!(out, "٤٢\n");
    }

    #[test]
    fn vm_class_missing_interface_method_is_error() {
        // Note: و is the AND keyword, so it cannot be an
        // interface name; use a plain name here.
        let (_, err, ok) = run_vm_src(
            "واجهة قابلة:\n    دالة طريقة():\n        مرر\nفئة ك واجهة قابلة:\n    دالة غيرها():\n        ارجع ١\n",
        );
        assert!(!ok);
        assert!(err.contains("لا تنفذ طريقة طريقة المطلوبة"));
    }

    #[test]
    fn vm_function_locals_do_not_leak() {
        let (out, err, ok) = run_vm_src(
            "دالة زد(س):\n    نتيجة = س + ١\n    ارجع نتيجة\nاطبع زد(٤)\nاطبع نتيجة\n",
        );
        assert!(!ok, "local must not leak, got out={} err={}", out, err);
        assert!(err.contains("غير معرف"));
    }

    #[test]
    fn vm_if_and_while() {
        let (out, _, ok) = run_vm_src(
            "س = ٠\nبينما س < ٣:\n    س = س + ١\nاطبع س\nإذا س == ٣:\n    اطبع \"نعم\"\nوإلا:\n    اطبع \"لا\"\n",
        );
        assert!(ok);
        assert_eq!(out, "٣\nنعم\n");
    }

    // Phase ك-1 (Meeting 24): closures + lambdas in the VM.
    // These mirror the interpreter tests (Meeting 11/13) with
    // identical sources and expected outputs; the interpreter
    // is the reference — any disagreement means the VM is
    // wrong.

    /// Run the same source on the interpreter (the reference).
    fn run_interp_src(src: &str) -> (String, bool) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = crate::interpreter::Interpreter::new();
        let res = interp.run(&prog);
        (interp.output, res.is_ok())
    }

    /// Parity: the same program gives the same output and the
    /// same success on the interpreter and on the VM.
    fn assert_parity(src: &str, expected: &str) {
        let (iout, iok) = run_interp_src(src);
        let (vout, verr, vok) = run_vm_src(src);
        assert!(iok, "interpreter failed on: {}", src);
        assert!(vok, "VM failed on: {} (err={})", src, verr);
        assert_eq!(iout, vout, "interpreter vs VM differ on: {}", src);
        assert_eq!(vout, expected);
    }

    #[test]
    fn vm_closure_returned_sees_outer_variable() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع():\n    س = ١٠\n    دالة احصل():\n        ارجع س\n    ارجع احصل\nد = اصنع()\nاطبع د()\n",
        );
        assert!(ok);
        assert_eq!(out, "١٠\n");
    }

    #[test]
    fn vm_closure_counter_shares_state() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع أ()\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n٢\n٣\n");
    }

    #[test]
    fn vm_closure_two_counters_are_independent() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nب = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع ب()\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n٢\n١\n");
    }

    #[test]
    fn vm_closure_sees_later_mutation_of_outer() {
        let (out, _, ok) = run_vm_src(
            "س = ١\nدالة احصل():\n    ارجع س\nس = ٢\nاطبع احصل()\n",
        );
        assert!(ok);
        assert_eq!(out, "٢\n");
    }

    #[test]
    fn vm_closure_param_shadows_captured() {
        let (out, _, ok) = run_vm_src(
            "س = ١٠\nدالة احصل(س):\n    ارجع س\nاطبع احصل(٥)\nاطبع س\n",
        );
        assert!(ok);
        assert_eq!(out, "٥\n١٠\n");
    }

    #[test]
    fn vm_lexical_assignment_mutates_outer() {
        let (out, _, ok) = run_vm_src(
            "س = ١٠\nدالة غير():\n    س = ٢٠\nغير()\nاطبع س\n",
        );
        assert!(ok);
        assert_eq!(out, "٢٠\n");
    }

    #[test]
    fn vm_closure_recursion_still_works() {
        let (out, _, ok) = run_vm_src(
            "دالة فيب(ن):\n    إذا ن <= ١:\n        ارجع ن\n    ارجع فيب(ن - ١) + فيب(ن - ٢)\nاطبع فيب(١٠)\n",
        );
        assert!(ok);
        assert_eq!(out, "٥٥\n");
    }

    #[test]
    fn vm_lambda_basic_value_and_call() {
        let (out, _, ok) = run_vm_src(
            "مضاعف = دالة_مجهولة(س): س * 2\nاطبع مضاعف(5)\nاطبع مضاعف\n",
        );
        assert!(ok);
        assert_eq!(out, "١٠\n<دالة مجهولة>\n");
    }

    #[test]
    fn vm_lambda_passed_as_argument() {
        let (out, _, ok) = run_vm_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س): س * 3، 4)\n",
        );
        assert!(ok);
        assert_eq!(out, "١٢\n");
    }

    #[test]
    fn vm_lambda_factory_captures_param() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع_مضاعف(ن):\n    ارجع دالة_مجهولة(س): س * ن\nمضاعف3 = اصنع_مضاعف(3)\nاطبع مضاعف3(4)\n",
        );
        assert!(ok);
        assert_eq!(out, "١٢\n");
    }

    #[test]
    fn vm_lambda_captures_outer_variable() {
        let (out, _, ok) = run_vm_src(
            "س = 10\nاحصل = دالة_مجهولة(): س\nاطبع احصل()\n",
        );
        assert!(ok);
        assert_eq!(out, "١٠\n");
    }

    #[test]
    fn vm_lambda_typed_param_checked_at_runtime() {
        let (_, err, ok) = run_vm_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س: عدد): س + 1، \"نص\")\n",
        );
        assert!(!ok);
        assert!(err.contains("نوع غير متطابق"));
    }

    #[test]
    fn vm_lambda_immediate_call_via_parens() {
        let (out, _, ok) = run_vm_src(
            "د = (دالة_مجهولة(س): س + 1)(10)\nاطبع د\n",
        );
        assert!(ok);
        assert_eq!(out, "١١\n");
    }

    #[test]
    fn vm_lambda_currying_nested_lambda() {
        let (out, _, ok) = run_vm_src(
            "مركب = دالة_مجهولة(س): دالة_مجهولة(ص): س + ص\nاطبع مركب(3)(4)\n",
        );
        assert!(ok);
        assert_eq!(out, "٧\n");
    }

    #[test]
    fn vm_nested_closures_three_levels() {
        let (out, _, ok) = run_vm_src(
            "دالة خارجي(أ):\n    دالة وسط(ب):\n        دالة داخلي(ج):\n            ارجع أ + ب + ج\n        ارجع داخلي\n    ارجع وسط\nاطبع خارجي(1)(2)(3)\n",
        );
        assert!(ok);
        assert_eq!(out, "٦\n");
    }

    #[test]
    fn vm_closure_in_list() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع(ن):\n    ارجع دالة_مجهولة(): ن\nق = [اصنع(1)، اصنع(2)]\nاطبع ق[0]()\nاطبع ق[1]()\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n٢\n");
    }

    #[test]
    fn vm_two_closures_share_one_factory_state() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع():\n    ع = 0\n    دالة زد():\n        ع = ع + 1\n        ارجع ع\n    دالة احصل():\n        ارجع ع\n    ارجع [زد، احصل]\nزوج = اصنع()\nاطبع زوج[0]()\nاطبع زوج[0]()\nاطبع زوج[1]()\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n٢\n٢\n");
    }

    #[test]
    fn vm_lambda_returned_from_function() {
        let (out, _, ok) = run_vm_src(
            "دالة اصنع_مضاعف(ن):\n    ارجع دالة_مجهولة(س): س * ن\nم = اصنع_مضاعف(5)\nاطبع م(6)\n",
        );
        assert!(ok);
        assert_eq!(out, "٣٠\n");
    }

    #[test]
    fn vm_user_function_shadows_builtin_name() {
        // Like the interpreter: a user function wins over a
        // builtin of the same name.
        let (out, _, ok) = run_vm_src(
            "دالة طول(س):\n    ارجع 99\nاطبع طول(\"نص\")\n",
        );
        assert!(ok);
        assert_eq!(out, "٩٩\n");
    }

    #[test]
    fn vm_parity_counter_program() {
        assert_parity(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع أ()\n",
            "١\n٢\n٣\n",
        );
    }

    #[test]
    fn vm_parity_lambda_program() {
        assert_parity(
            "ضاعف = دالة_مجهولة(س): س * 2\nاطبع ضاعف(21)\nدالة طبق(د، ق):\n    ارجع د(ق)\nاطبع طبق(دالة_مجهولة(س): س + 10، 5)\nاطبع طبق(ضاعف، 100)\n",
            "٤٢\n١٥\n٢٠٠\n",
        );
    }

    #[test]
    fn vm_parity_nested_closures() {
        assert_parity(
            "دالة خارجي(أ):\n    دالة وسط(ب):\n        دالة داخلي(ج):\n            ارجع أ + ب + ج\n        ارجع داخلي\n    ارجع وسط\nاطبع خارجي(1)(2)(3)\n",
            "٦\n",
        );
    }

    #[test]
    fn vm_parity_closure_in_list_and_shared_state() {
        assert_parity(
            "دالة اصنع():\n    ع = 0\n    دالة زد():\n        ع = ع + 1\n        ارجع ع\n    دالة احصل():\n        ارجع ع\n    ارجع [زد، احصل]\nزوج = اصنع()\nاطبع زوج[0]()\nاطبع زوج[0]()\nاطبع زوج[1]()\n",
            "١\n٢\n٢\n",
        );
    }

    #[test]
    fn vm_forward_call_quirk_documented() {
        // KNOWN QUIRK (recorded, not fixed in ك-1): the VM
        // compiles the whole program before running it, so a
        // call to a function defined LATER succeeds here,
        // while the interpreter (via the semantic analyzer,
        // which runs first on the CLI path) rejects it. The
        // CLI path never reaches the VM for such a program,
        // so users see the interpreter's error either way.
        let (out, _, ok) = run_vm_src(
            "اطبع لاحق()\nدالة لاحق():\n    ارجع ٤٢\n",
        );
        assert!(ok);
        assert_eq!(out, "٤٢\n");
        let (_, iok) = run_interp_src(
            "اطبع لاحق()\nدالة لاحق():\n    ارجع ٤٢\n",
        );
        assert!(!iok, "interpreter must reject the forward call");
    }

    // ك-1.2: التحكم في التدفق في VM — اختبارات مطابقة مع
    // المفسر (المرجع). كل اختبار مطابقة يشغّل المصدر نفسه
    // في الوضعين ويقارن المخرجات؛ واختبارات الخطأ تقارن
    // نص الخطأ العربي كاملا (بما فيه السطر والعمود).

    /// Run the same source on the interpreter, returning the
    /// error text too (for error-parity tests).
    fn run_interp_src_err(src: &str) -> (String, String, bool) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = crate::interpreter::Interpreter::new();
        match interp.run(&prog) {
            Ok(()) => (interp.output, String::new(), true),
            Err(e) => (interp.output, e.to_string(), false),
        }
    }

    /// Error parity: both modes fail with the SAME Arabic
    /// error text (and the same partial output).
    fn assert_parity_err(src: &str) {
        let (iout, ierr, iok) = run_interp_src_err(src);
        let (vout, verr, vok) = run_vm_src(src);
        assert!(!iok, "interpreter unexpectedly succeeded on: {}", src);
        assert!(!vok, "VM unexpectedly succeeded on: {}", src);
        assert_eq!(iout, vout, "partial output differs on: {}", src);
        assert_eq!(
            ierr, verr,
            "error text differs on: {}\ninterp: {}\nvm: {}",
            src, ierr, verr
        );
    }

    #[test]
    fn vm_control_for_list_and_var_persists() {
        assert_parity(
            "مجموع = ٠\nلكل رقم في [١، ٢، ٣]:\n    مجموع = مجموع + رقم\nاطبع مجموع\nاطبع رقم\n",
            "٦\n٣\n",
        );
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): تكافؤ `+=`
    // على نص وعدد وقائمة وقاموس والغائب.
    #[test]
    fn vm_append_parity() {
        assert_parity(
            "ن = \"أهلا\"\nن += \" وسهلا\"\nاطبع ن\nع = ٥\nع += ٣\nاطبع ع\nق = [١، ٢]\nق += [٣]\nاطبع ق\nخ = {\"أ\": ١}\nخ += {\"ب\": ٢}\nاطبع خ\nغ += ٧\nاطبع غ\n",
            "أهلا وسهلا\n٨\n[١, ٢, ٣]\n{\"أ\": ١, \"ب\": ٢}\n٧\n",
        );
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): التجميد
    // والحذف والمرجع والفحص — نفس الرسائل في الوضعين.
    #[test]
    fn vm_freeze_delete_nameref_parity() {
        assert_parity(
            "س = ٥\nاطبع افحص_متغير(\"س\")\nجمد(\"س\")\nاطبع افحص_متغير(\"س\")\nص = ١٠\nمرجع(\"ر\", \"ص\")\nر = ٢٠\nاطبع ص\nاطبع افحص_متغير(\"ر\")\nاحذف_متغير(\"ر\")\nاطبع ص\nاحذف_متغير(\"ص\")\n",
            "متغير س: عدد = ٥\nمتغير س: عدد = ٥ مجمد\n٢٠\nمتغير ر: مرجع إلى ص\n٢٠\n",
        );
        assert_parity_err("س = ٥\nجمد(\"س\")\nس = ٦\n");
        assert_parity_err("س = ٥\nجمد(\"س\")\nس += ١\n");
        assert_parity_err("س = ٥\nجمد(\"س\")\nاحذف_متغير(\"س\")\n");
        assert_parity_err("ص = ١\nاحذف_متغير(\"ص\")\nاطبع ص\n");
        assert_parity_err("س = ١\nمرجع(\"ر\", ٥)\n");
    }

    // الدفعة 4 — المتغيرات (اجتماع 58 §73): الترابطية
    // وعرض البيئة — نفس المخرجات في الوضعين.
    #[test]
    fn vm_assoc_and_env_display_parity() {
        assert_parity(
            "خ = {}\nخ[\"أ\"] = ١\nخ[\"ب\"] = ٢\nاطبع مفاتيح(خ)\nخ += {\"ج\": ٣، \"أ\": ٩}\nاطبع خ\nاطبع مفاتيح(خ)\nق = اعرض_البيئة()\nاطبع طول(ق)\nاطبع ق[٠]\nاطبع ق[٤]\n",
            "[\"أ\", \"ب\"]\n{\"أ\": ٩, \"ب\": ٢, \"ج\": ٣}\n[\"أ\", \"ب\", \"ج\"]\n١٤\nالبيت=~\nالطرفية=نيفا\n",
        );
    }

    #[test]
    fn vm_control_for_string() {
        assert_parity(
            "لكل حرف في \"محمود\":\n    اطبع حرف\n",
            "م\nح\nم\nو\nد\n",
        );
    }

    #[test]
    fn vm_control_for_empty_list() {
        assert_parity("لكل س في []:\n    اطبع س\nاطبع \"بعد\"\n", "بعد\n");
    }

    #[test]
    fn vm_control_for_break() {
        assert_parity(
            "لكل رقم في [١، ٢، ٣، ٤]:\n    إذا رقم == ٣:\n        اكسر\n    اطبع رقم\nاطبع \"انتهى\"\n",
            "١\n٢\nانتهى\n",
        );
    }

    #[test]
    fn vm_control_for_continue() {
        assert_parity(
            "لكل رقم في [١، ٢، ٣، ٤]:\n    إذا رقم == ٢:\n        تابع\n    اطبع رقم\n",
            "١\n٣\n٤\n",
        );
    }

    #[test]
    fn vm_control_nested_loops_break_continue() {
        assert_parity(
            "لكل أ في [١، ٢]:\n    لكل ب في [١، ٢، ٣]:\n        إذا ب == ٢:\n            تابع\n        إذا أ == ٢ و ب == ٣:\n            اكسر\n        اطبع أ * ١٠ + ب\n",
            "١١\n١٣\n٢١\n",
        );
    }

    #[test]
    fn vm_control_for_var_does_not_leak_from_function() {
        assert_parity(
            "س = ١٠\nدالة د():\n    لكل س في [١، ٢]:\n        مرر\n    ارجع س\nاطبع د()\nاطبع س\n",
            "٢\n١٠\n",
        );
    }

    #[test]
    fn vm_control_for_iterates_snapshot() {
        // The iterable is materialised once: mutating the
        // list inside the body does not change the iteration.
        assert_parity(
            "قائمة = [١، ٢، ٣]\nلكل رقم في قائمة:\n    قائمة[٠] = ٩٩\n    اطبع رقم\nاطبع قائمة\n",
            "١\n٢\n٣\n[٩٩, ٢, ٣]\n",
        );
    }

    #[test]
    fn vm_control_for_non_iterable_is_error() {
        assert_parity_err("لكل س في ٥:\n    اطبع س\n");
    }

    #[test]
    fn vm_control_repeat_count() {
        assert_parity(
            "عداد = ٠\nكرر ٣ مرات:\n    عداد = عداد + ١\n    اطبع عداد\n",
            "١\n٢\n٣\n",
        );
    }

    #[test]
    fn vm_control_repeat_count_zero() {
        assert_parity("كرر ٠ مرات:\n    اطبع \"لا\"\nاطبع \"بعد\"\n", "بعد\n");
    }

    #[test]
    fn vm_control_repeat_while() {
        assert_parity(
            "عداد = ٠\nكرر طالما عداد < ٣:\n    عداد = عداد + ١\nاطبع عداد\n",
            "٣\n",
        );
    }

    #[test]
    fn vm_control_repeat_count_negative_is_error() {
        assert_parity_err("كرر -١ مرات:\n    اطبع \"لا\"\n");
    }

    #[test]
    fn vm_control_repeat_count_fraction_is_error() {
        assert_parity_err("كرر 3 / 2 مرات:\n    اطبع \"لا\"\n");
    }

    #[test]
    fn vm_control_repeat_count_non_number_is_error() {
        assert_parity_err("كرر \"أ\" مرات:\n    اطبع \"لا\"\n");
    }

    #[test]
    fn vm_control_repeat_count_too_big_is_error() {
        assert_parity_err("كرر ١٠٠٠٠٠١ مرات:\n    اطبع \"لا\"\n");
    }

    #[test]
    fn vm_control_while_condition_must_be_bool() {
        assert_parity_err("بينما ٥:\n    اطبع \"لا\"\n");
    }

    #[test]
    fn vm_control_while_iteration_limit() {
        // The 1,000,000-iteration cap, identical in both
        // modes (same message, same line/col).
        assert_parity_err(
            "عداد = ٠\nبينما عداد < ٢٠٠٠٠٠٠:\n    عداد = عداد + ١\n",
        );
    }

    #[test]
    fn vm_control_while_break_continue() {
        assert_parity(
            "عداد = ٠\nبينما عداد < ١٠:\n    عداد = عداد + ١\n    إذا عداد == ٢:\n        تابع\n    إذا عداد == ٥:\n        اكسر\n    اطبع عداد\nاطبع \"بعد\"\n",
            "١\n٣\n٤\nبعد\n",
        );
    }

    #[test]
    fn vm_control_break_outside_loop_is_error() {
        assert_parity_err("اكسر\n");
    }

    #[test]
    fn vm_control_continue_outside_loop_is_error() {
        assert_parity_err("تابع\n");
    }

    #[test]
    fn vm_control_break_inside_function_is_error() {
        // Same message in both modes. The POSITION differs by
        // design: the VM rejects it at compile time (the اكسر
        // statement's line), the interpreter at runtime (the
        // call's line) — and on the CLI path the semantic
        // analyzer rejects it before either runs.
        let src = "عداد = ٠\nبينما عداد < ١:\n    عداد = عداد + ١\n    دالة د():\n        اكسر\n    د()\n";
        let (_, ierr, iok) = run_interp_src_err(src);
        let (_, verr, vok) = run_vm_src(src);
        assert!(!iok, "interpreter unexpectedly succeeded");
        assert!(!vok, "VM unexpectedly succeeded");
        assert!(ierr.contains("اكسر خارج حلقة"), "interp err: {}", ierr);
        assert!(verr.contains("اكسر خارج حلقة"), "vm err: {}", verr);
    }

    #[test]
    fn vm_control_try_except_finally_on_error() {
        assert_parity(
            "حاول:\n    اطبع ١٠ / ٠\nماعدا:\n    اطبع \"خطأ\"\nأخيرا:\n    اطبع \"انتهى\"\nاطبع \"بعد\"\n",
            "خطأ\nانتهى\nبعد\n",
        );
    }

    #[test]
    fn vm_control_try_finally_on_normal_path() {
        assert_parity(
            "حاول:\n    اطبع \"أ\"\nماعدا:\n    اطبع \"خطأ\"\nأخيرا:\n    اطبع \"ب\"\nاطبع \"بعد\"\n",
            "أ\nب\nبعد\n",
        );
    }

    #[test]
    fn vm_control_try_except_only() {
        assert_parity(
            "حاول:\n    اطبع ١٠ / ٠\nماعدا:\n    اطبع \"أُمسك\"\nاطبع \"بعد\"\n",
            "أُمسك\nبعد\n",
        );
    }

    #[test]
    fn vm_control_try_finally_only_on_error() {
        assert_parity_err(
            "حاول:\n    اطبع ١٠ / ٠\nأخيرا:\n    اطبع \"انتهى\"\n",
        );
    }

    #[test]
    fn vm_control_try_finally_only_normal() {
        assert_parity("حاول:\n    اطبع \"أ\"\nأخيرا:\n    اطبع \"ب\"\n", "أ\nب\n");
    }

    #[test]
    fn vm_control_try_break_runs_finally() {
        assert_parity(
            "لكل رقم في [١، ٢، ٣]:\n    حاول:\n        إذا رقم == ٢:\n            اكسر\n        اطبع رقم\n    أخيرا:\n        اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
            "١\nأخيرا\nأخيرا\nبعد\n",
        );
    }

    #[test]
    fn vm_control_try_continue_runs_finally() {
        assert_parity(
            "لكل رقم في [١، ٢، ٣]:\n    حاول:\n        إذا رقم == ٢:\n            تابع\n        اطبع رقم\n    أخيرا:\n        اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
            "١\nأخيرا\nأخيرا\n٣\nأخيرا\nبعد\n",
        );
    }

    #[test]
    fn vm_control_try_return_runs_finally() {
        assert_parity(
            "دالة د():\n    حاول:\n        ارجع ١\n    أخيرا:\n        اطبع \"أخيرا\"\nاطبع د()\n",
            "أخيرا\n١\n",
        );
    }

    #[test]
    fn vm_control_try_finally_return_wins() {
        assert_parity(
            "دالة د():\n    حاول:\n        ارجع ١\n    أخيرا:\n        ارجع ٢\nاطبع د()\n",
            "٢\n",
        );
    }

    #[test]
    fn vm_control_try_error_in_except_runs_finally() {
        assert_parity_err(
            "حاول:\n    اطبع ١٠ / ٠\nماعدا:\n    اطبع ١ / ٠\nأخيرا:\n    اطبع \"أخيرا\"\n",
        );
    }

    #[test]
    fn vm_control_try_error_in_finally_on_normal_path() {
        assert_parity_err(
            "حاول:\n    اطبع \"أ\"\nأخيرا:\n    اطبع ١ / ٠\n",
        );
    }

    #[test]
    fn vm_control_try_error_in_finally_ignored_on_error_path() {
        // The finally's own error is ignored on the error
        // path: the ORIGINAL error (line 2) propagates.
        assert_parity_err(
            "حاول:\n    اطبع ١٠ / ٠\nأخيرا:\n    اطبع ١ / ٠\n",
        );
    }

    #[test]
    fn vm_control_try_nested_finally_error_swapped() {
        // The inner finally's error is ignored; the original
        // error is caught by the outer except.
        assert_parity(
            "حاول:\n    حاول:\n        اطبع ١٠ / ٠\n    أخيرا:\n        اطبع ١ / ٠\nماعدا:\n    اطبع \"أُمسك\"\n",
            "أُمسك\n",
        );
    }

    #[test]
    fn vm_control_try_catches_function_error() {
        assert_parity(
            "دالة د():\n    ارجع ١٠ / ٠\nحاول:\n    اطبع د()\nماعدا:\n    اطبع \"أُمسك\"\n",
            "أُمسك\n",
        );
    }

    #[test]
    fn vm_control_try_inside_loop_break_keeps_try() {
        // The loop is INSIDE the try, so the break does not
        // unwind the try: the code after the loop still runs.
        assert_parity(
            "حاول:\n    لكل رقم في [١، ٢، ٣]:\n        إذا رقم == ٢:\n            اكسر\n        اطبع رقم\n    اطبع \"بعد الحلقة\"\nماعدا:\n    اطبع \"خطأ\"\n",
            "١\nبعد الحلقة\n",
        );
    }

    #[test]
    fn vm_control_try_return_inside_loop_runs_finally() {
        assert_parity(
            "دالة د():\n    لكل رقم في [١، ٢، ٣]:\n        حاول:\n            إذا رقم == ٢:\n                ارجع رقم\n            اطبع رقم\n        أخيرا:\n            اطبع \"أخيرا\"\n    ارجع ٠\nاطبع د()\n",
            "١\nأخيرا\nأخيرا\n٢\n",
        );
    }

    #[test]
    fn vm_control_in_operator() {
        assert_parity(
            "قائمة = [١، ٢، ٣]\nاطبع ٢ في قائمة\nاطبع ٩٩ في قائمة\nاطبع ٩٩ ليس في قائمة\nاطبع \"مح\" في \"محمود\"\nاطبع \"ز\" في \"محمود\"\nقاموس = {\"اسم\": \"محمود\"}\nاطبع \"اسم\" في قاموس\nاطبع \"عمر\" في قاموس\n",
            "صحيح\nخطأ\nصحيح\nصحيح\nخطأ\nصحيح\nخطأ\n",
        );
    }

    #[test]
    fn vm_control_in_wrong_right_type_is_error() {
        assert_parity_err("اطبع ٥ في \"نص\"\n");
    }

    // مهمة 4 (ك-1.3): الوحدات والمكتبة القياسية في VM.
    // كل اختبار يقارن المفسر (المرجع) بـ VM على الملفات
    // نفسها — أي خلاف يعني أن VM مخطئة.

    const VM_CALC_MODULE: &str = "صدّر اجمع، الثابت، احصل\nالثابت = ١٠\nدالة مساعد(س):\n    ارجع س * ٢\nدالة اجمع(أ، ب):\n    ارجع مساعد(أ) + ب + الثابت\nدالة احصل():\n    ارجع الثابت\nسر = ٩٩٩\n";

    #[test]
    fn vm_modules_file_import_dot_from_alias() {
        // Exported function uses a private helper (مساعد) and
        // a module global (الثابت); both visible in the call.
        assert_module_parity(
            "استورد حساب\nاطبع حساب.اجمع(٣، ٤)\nاطبع حساب.احصل()\nمن حساب استورد اجمع\nاطبع اجمع(١، ٢)\nاستورد حساب باسم ح\nاطبع ح.احصل()\nاطبع حساب\n",
            &[("حساب.نيفا", VM_CALC_MODULE)],
            "٢٠\n١٠\n١٤\n١٠\n<وحدة حساب>\n",
        );
    }

    #[test]
    fn vm_modules_private_name_rejected() {
        assert_module_error_parity(
            "استورد حساب\nاطبع حساب.سر\n",
            &[("حساب.نيفا", VM_CALC_MODULE)],
            "غير مصدر",
        );
        assert_module_error_parity(
            "من حساب استورد سر\n",
            &[("حساب.نيفا", VM_CALC_MODULE)],
            "غير مصدر",
        );
    }

    #[test]
    fn vm_modules_missing_and_cycle_errors() {
        assert_module_error_parity("استورد لاوجود\n", &[], "وحدة غير موجودة");
        assert_module_error_parity(
            "استورد أ\n",
            &[
                ("أ.نيفا", "صدّر من_أ\nاستورد ب\nدالة من_أ():\n    ارجع ١\n"),
                ("ب.نيفا", "صدّر من_ب\nاستورد أ\nدالة من_ب():\n    ارجع ٢\n"),
            ],
            "استيراد دائري",
        );
    }

    #[test]
    fn vm_modules_load_once() {
        assert_module_parity(
            "استورد مرة\nاستورد مرة\nمن مرة استورد قيمة\nاطبع قيمة\n",
            &[("مرة.نيفا", "صدّر قيمة\nاطبع \"تحميل مرة\"\nقيمة = ٧\n")],
            "تحميل مرة\n٧\n",
        );
    }

    #[test]
    fn vm_modules_nested_import() {
        // A module importing another module: the inner call
        // resolves through the outer module's scope.
        assert_module_parity(
            "استورد خارج\nاطبع خارج.ضاعف(٢١)\n",
            &[
                (
                    "داخل.نيفا",
                    "صدّر ضعف\nدالة ضعف(س):\n    ارجع س * ٢\n",
                ),
                (
                    "خارج.نيفا",
                    "صدّر ضاعف\nاستورد داخل\nدالة ضاعف(س):\n    ارجع داخل.ضعف(س) + ١\n",
                ),
            ],
            "٤٣\n",
        );
    }

    #[test]
    fn vm_modules_import_inside_function() {
        // Import inside a function body loads once and binds
        // in that call's scope; a second call reuses it.
        assert_module_parity(
            "دالة استخدم():\n    استورد حساب\n    ارجع حساب.احصل()\nاطبع استخدم()\nاطبع استخدم()\n",
            &[("حساب.نيفا", VM_CALC_MODULE)],
            "١٠\n١٠\n",
        );
    }

    #[test]
    fn vm_modules_global_mutation_does_not_persist() {
        // Module functions run in a fresh clone of the module
        // scope per call (like the interpreter): a mutation
        // of a module global is gone on the next call.
        // Verified on the interpreter first: ١ ١ ٠.
        assert_module_parity(
            "استورد عداد\nاطبع عداد.زد()\nاطبع عداد.زد()\nاطبع عداد.اقرأ()\n",
            &[(
                "عداد.نيفا",
                "صدّر زد، اقرأ\nعداد = ٠\nدالة زد():\n    عداد = عداد + ١\n    ارجع عداد\nدالة اقرأ():\n    ارجع عداد\n",
            )],
            "١\n١\n٠\n",
        );
    }

    #[test]
    fn vm_modules_class_method_sees_module_globals() {
        // A class defined in a module: its methods see the
        // module globals, and a mutation does not persist
        // across calls either (module-scope clone per call).
        assert_module_parity(
            "استورد وحدة\nك = وحدة.صانع()\nاطبع ك.زد()\nاطبع ك.زد()\nاطبع وحدة.اقرأ()\n",
            &[(
                "وحدة.نيفا",
                "صدّر صانع، اقرأ\nعداد = ٠\nفئة عداد_فئة:\n    دالة زد():\n        عداد = عداد + ١\n        ارجع عداد\nدالة صانع():\n    ارجع أنشئ عداد_فئة()\nدالة اقرأ():\n    ارجع عداد\n",
            )],
            "١\n١\n٠\n",
        );
    }

    #[test]
    fn vm_modules_reexported_function_uses_reexporting_module_scope() {
        // Re-export re-marks the function with THIS module's
        // name (like the interpreter): its body resolves names
        // in the re-exporting module's scope, so عام_ب is
        // undefined there — same error in both modes.
        assert_module_error_parity(
            "استورد أ\nاطبع أ.شيء()\n",
            &[
                (
                    "ب.نيفا",
                    "صدّر شيء\nعام_ب = ١\nدالة شيء():\n    ارجع عام_ب\n",
                ),
                ("أ.نيفا", "صدّر شيء\nعام_أ = ٢\nمن ب استورد شيء\n"),
            ],
            "متغير غير معرف: عام_ب",
        );
    }

    #[test]
    fn vm_modules_builtin_name_shadowed_in_module() {
        // A module function shadowing a builtin name, called
        // before its definition point in the file: the
        // interpreter calls the user function (lookup at call
        // time); the VM must do the same, not the builtin.
        assert_module_parity(
            "استورد وحدة\nاطبع وحدة.استخدم()\n",
            &[(
                "وحدة.نيفا",
                "صدّر استخدم\nدالة استخدم():\n    ارجع طول([١، ٢])\nدالة طول(ق):\n    ارجع ٩٩\n",
            )],
            "٩٩\n",
        );
    }

    #[test]
    fn vm_stdlib_native_math_time_parity() {
        // Deterministic stdlib surface: math + time + a
        // from-imported native function. Random parts are
        // range-checked (not exact) so both modes agree.
        assert_parity(
            "استورد رياضيات\nاستورد وقت\nاطبع رياضيات.جذر(16)\nاطبع رياضيات.قوة(2، 10)\nاطبع رياضيات.أرضية(3.7)\nاطبع رياضيات.سقف(3.2)\nاطبع رياضيات.باي\nاطبع رياضيات.هـ\nمن رياضيات استورد جذر\nاطبع جذر(25)\nاطبع وقت.الآن() > 1700000000\n",
            "٤\n١٠٢٤\n٣\n٤\n٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥\n٥\nصحيح\n",
        );
        assert_parity(
            "استورد عشوائي\nاطبع عشوائي.عدد_عشوائي() >= 0\nاطبع عشوائي.عدد_عشوائي() < 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) >= 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) <= 6\nاطبع عشوائي.اختيار([10، 20، 30]) في [10، 20، 30]\n",
            "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\n",
        );
    }

    #[test]
    fn vm_stdlib_native_not_shadowed_by_file() {
        // A same-named .نيفا file in the program directory
        // must never shadow a native module (Meeting 16).
        let (out, _, ok) = run_vm_src_with_files(
            "استورد رياضيات\nاطبع رياضيات.جذر(16)\n",
            &[("رياضيات.نيفا", "صدّر س\nس = 999\n")],
        );
        assert!(ok);
        assert_eq!(out, "٤\n");
        let ((iout, _, iok), _) = run_both_with_files(
            "استورد رياضيات\nاطبع رياضيات.جذر(16)\n",
            &[("رياضيات.نيفا", "صدّر س\nس = 999\n")],
        );
        assert!(iok);
        assert_eq!(iout, "٤\n");
    }

    #[test]
    fn vm_modules_module_value_prints_and_dot_on_non_module() {
        // Printing a module value, and dot access on a
        // non-module/non-instance is the interpreter's type
        // error in both modes.
        assert_module_parity("استورد حساب\nاطبع حساب\n", &[("حساب.نيفا", VM_CALC_MODULE)], "<وحدة حساب>\n");
        assert_module_error_parity(
            "س = ٥\nاطبع س.شيء\n",
            &[],
            "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط",
        );
    }

    /// مهمة 5 (ك-1.4): شغّل البرنامج نفسه على المفسر
    /// وعلى VM في مجلدين مؤقتين متطابقين (ملفات بذر
    /// واحدة)، وأعد (مخرجات، خطأ، نجاح، لقطة المجلد)
    /// لكل منهما. مجلدان منفصلان لأن البرنامج قد يكتب
    /// أو يحذف — تشغيل الوضعين في المجلد نفسه يجعل
    /// الثاني يرى آثار الأول فيكذب التكافؤ.
    fn run_both_file_dirs(
        main_src: &str,
        files: &[(&str, &str)],
        trusted: bool,
    ) -> (
        (String, String, bool, Vec<(String, String)>),
        (String, String, bool, Vec<(String, String)>),
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("niva-vm-files-{}-{}", std::process::id(), n));
        let dir_i = base.join("مفسر");
        let dir_v = base.join("vm");
        for dir in [&dir_i, &dir_v] {
            std::fs::create_dir_all(dir).unwrap();
            // اجتماع 45 §59: الجلسة تبدأ في البيت — أنشئه
            // وبذور الملفات فيه (مجلد العمل الحالي).
            std::fs::create_dir_all(dir.join("home")).unwrap();
            for (name, src) in files {
                let path = dir.join("home").join(name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, src).unwrap();
            }
        }
        let snapshot = |dir: &std::path::Path| -> Vec<(String, String)> {
            let mut out: Vec<(String, String)> = Vec::new();
            let mut stack = vec![dir.to_path_buf()];
            while let Some(d) = stack.pop() {
                let mut entries: Vec<_> = std::fs::read_dir(&d)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .collect();
                entries.sort();
                for p in entries {
                    if p.is_dir() {
                        stack.push(p);
                    } else if p.is_file() {
                        let rel = p.strip_prefix(dir).unwrap();
                        out.push((
                            crate::permissions::slash_display(rel),
                            std::fs::read_to_string(&p).unwrap_or_default(),
                        ));
                    }
                }
            }
            out.sort();
            out
        };
        let toks = tokenize(main_src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp = crate::interpreter::Interpreter::with_program_dir(dir_i.clone());
        if trusted {
            interp.set_trusted(true);
        }
        let ires = interp.run(&prog);
        let iout = (
            interp.output.clone(),
            match &ires {
                Ok(()) => String::new(),
                Err(e) => e.to_string(),
            },
            ires.is_ok(),
            snapshot(&dir_i),
        );
        let vout = {
            let toks = tokenize(main_src).unwrap();
            let prog = Parser::new(toks).parse_program().unwrap();
            let compiled = Compiler::compile_program(&prog).unwrap();
            let mut vm = Vm::new(compiled, dir_v.clone());
            if trusted {
                vm.set_trusted(true);
            }
            let res = vm.run();
            (
                vm.output.clone(),
                match &res {
                    Ok(()) => String::new(),
                    Err(e) => e.to_string(),
                },
                res.is_ok(),
                snapshot(&dir_v),
            )
        };
        let _ = std::fs::remove_dir_all(&base);
        (iout, vout)
    }

    /// مهمة 5: تكافؤ ملفات — نفس المخرجات والنجاح
    /// والمتوقع، ونفس محتويات المجلد بعد التشغيل.
    fn assert_file_parity(main_src: &str, files: &[(&str, &str)], expected: &str) {
        let ((iout, ierr, iok, isnap), (vout, verr, vok, vsnap)) =
            run_both_file_dirs(main_src, files, false);
        assert!(iok, "interpreter failed on: {} (err={})", main_src, ierr);
        assert!(vok, "VM failed on: {} (err={})", main_src, verr);
        assert_eq!(iout, vout, "interpreter vs VM output differ on: {}", main_src);
        assert_eq!(vout, expected);
        assert_eq!(isnap, vsnap, "final directory contents differ on: {}", main_src);
    }

    /// مهمة 5: تكافؤ أخطاء ملفات — نفس الفشل، ونفس
    /// رسالة الخطأ حرفيا، ونفس المخرجات قبل الخطأ.
    fn assert_file_error_parity(main_src: &str, files: &[(&str, &str)], needle: &str) {
        let ((iout, ierr, iok, _), (vout, verr, vok, _)) =
            run_both_file_dirs(main_src, files, false);
        assert!(!iok, "interpreter unexpectedly passed on: {}", main_src);
        assert!(!vok, "VM unexpectedly passed on: {}", main_src);
        assert_eq!(iout, vout, "outputs differ on: {}", main_src);
        assert_eq!(ierr, verr, "error messages differ on: {}", main_src);
        assert!(ierr.contains(needle), "error: {}", ierr);
    }

    #[test]
    fn vm_files_write_read_roundtrip() {
        assert_file_parity(
            "اكتب_ملف(\"ملاحظة.txt\"، \"مرحبا من محرك نيفا\")\nاطبع اقرأ_ملف(\"ملاحظة.txt\")\nاطبع موجود(\"ملاحظة.txt\")\nاطبع نوع_المسار(\"ملاحظة.txt\")\n",
            &[],
            "مرحبا من محرك نيفا\nصحيح\nملف\n",
        );
    }

    // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
    // تكافؤ مفسر/VM للنسخ والحذف والروابط والوضع.
    #[test]
    fn vm_batch5_recursive_copy_mkdir_list() {
        assert_file_parity(
            "أنشئ_مجلد(\"مجلد/فرعي\")\nاكتب_ملف(\"مجلد/أ.txt\"، \"أ\")\nاكتب_ملف(\"مجلد/فرعي/ب.txt\"، \"ب\")\nانسخ_تكراري(\"مجلد\"، \"نسخة\")\nاطبع اقرأ_ملف(\"نسخة/أ.txt\")\nاطبع اقرأ_ملف(\"نسخة/فرعي/ب.txt\")\nاطبع أنشئ_مجلد(\"س/ص/ع\"، \"750\")\n",
            &[],
            "أ\nب\n[\"س\", \"س/ص\", \"س/ص/ع\"]\n",
        );
    }

    #[test]
    fn vm_batch5_copy_updated() {
        // الغائب ينسخ دائما (صحيح)، والثانية
        // تتخطى دائما (خطأ) لأن الهدف صار أحدث
        // من المصدر أو مساويا له بعد النسخ — بلا
        // اعتماد على دقة طابع الوقت بين كتابتين.
        assert_file_parity(
            "اكتب_ملف(\"أ.txt\"، \"قديم\")\nاطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")\nاطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")\nاطبع اقرأ_ملف(\"ب.txt\")\n",
            &[],
            "صحيح\nخطأ\nقديم\n",
        );
    }

    #[test]
    #[cfg(unix)]
    fn vm_batch5_copy_archive_links() {
        assert_file_parity(
            "أنشئ_مجلد(\"مجلد\")\nاكتب_ملف(\"مجلد/أ.txt\"، \"أ\")\nأنشئ_رابط(\"مجلد/أ.txt\"، \"مجلد/رابط.txt\")\nأنشئ_رابط_صلب(\"مجلد/أ.txt\"، \"مجلد/صلب.txt\")\nانسخ_أرشيف(\"مجلد\"، \"نسخة\")\nاطبع اقرأ_ملف(\"نسخة/رابط.txt\")\nاطبع اقرأ_ملف(\"نسخة/صلب.txt\")\nاطبع اقرأ_ملف(\"نسخة/أ.txt\")\n",
            &[],
            "أ\nأ\nأ\n",
        );
    }

    #[test]
    fn vm_batch5_copy_light_touch_delete() {
        assert_file_parity(
            "اكتب_ملف(\"أ.txt\"، \"ثابت\")\nانسخ_خفيف(\"أ.txt\"، \"ب.txt\")\nاطبع اقرأ_ملف(\"ب.txt\")\nأنشئ_ملف(\"أ.txt\")\nاطبع اقرأ_ملف(\"أ.txt\")\nأنشئ_مجلد(\"شجرة/فرعي\")\nاكتب_ملف(\"شجرة/فرعي/ج.txt\"، \"ج\")\nاحذف_تكراري(\"شجرة\")\nاطبع موجود(\"شجرة\")\nاطبع موجود(\"أ.txt\")\n",
            &[],
            "ثابت\nثابت\nخطأ\nصحيح\n",
        );
    }

    #[test]
    #[cfg(unix)]
    fn vm_batch5_relative_link_survives_move() {
        assert_file_parity(
            "أنشئ_مجلد(\"شجرة/أ/ب\")\nاكتب_ملف(\"شجرة/أ/ب/ملف.txt\"، \"داخل\")\nأنشئ_رابط_نسبي(\"شجرة/أ/ب/ملف.txt\"، \"شجرة/أ/رابط.txt\")\nاطبع اقرأ_ملف(\"شجرة/أ/رابط.txt\")\nانقل(\"شجرة\"، \"شجرة2\")\nاطبع اقرأ_ملف(\"شجرة2/أ/رابط.txt\")\n",
            &[],
            "داخل\nداخل\n",
        );
    }

    #[test]
    fn vm_batch5_file_errors() {
        assert_file_error_parity(
            "انسخ(\"مجلد\"، \"نسخة\")\n",
            &[("مجلد/أ.txt", "أ")],
            "لا يمكن نسخ المجلد بلا خيار تكراري",
        );
        assert_file_error_parity("احذف(\"/\")\n", &[], "لا يمكن حذف الجذر: /");
        assert_file_error_parity("احذف(\".\")\n", &[], "لا يمكن حذف . أو ..");
        assert_file_error_parity("احذف_تكراري(\"/\")\n", &[], "لا يمكن حذف الجذر: /");
        assert_file_error_parity(
            "أنشئ_مجلد(\"أ\"، \"888\")\n",
            &[],
            "الوضع يجب أن يكون ثمانيا مثل 755",
        );
        assert_file_error_parity(
            "أنشئ_رابط_صلب(\"غائب.txt\"، \"ب.txt\")\n",
            &[],
            "الهدف غير موجود",
        );
        assert_file_error_parity(
            "احذف(\"مجلد\")\n",
            &[("مجلد/أ.txt", "أ")],
            "المجلد غير فارغ",
        );
    }

    #[test]
    fn vm_files_create_exists_kind_delete() {
        assert_file_parity(
            "أنشئ_ملف(\"جديد.txt\")\nاطبع موجود(\"جديد.txt\")\nاطبع نوع_المسار(\"جديد.txt\")\nاطبع نوع_المسار(\"غائب.txt\")\nاطبع موجود(\"غائب.txt\")\nاحذف_ملف(\"جديد.txt\")\nاطبع موجود(\"جديد.txt\")\n",
            &[],
            "صحيح\nملف\nغير موجود\nخطأ\nخطأ\n",
        );
    }

    #[test]
    fn vm_files_mkdir_cd_pwd_list() {
        assert_file_parity(
            "أنشئ_مجلد(\"فرعي\")\nاطبع نوع_المسار(\"فرعي\")\nانتقل(\"فرعي\")\nاطبع المجلد_الحالي()\nاكتب_ملف(\"داخل.txt\"، \"داخل\")\nاطبع اعرض_الملفات()\nاطبع عد_الملفات()\n",
            &[],
            "مجلد\n~/فرعي\n[\"داخل.txt\"]\n١\n",
        );
    }

    #[test]
    fn vm_files_cd_persists_across_function_call() {
        // انتقل داخل دالة يبقى بعد عودتها (cwd مشترك
        // مع كل VM فرعية، كما في المفسر الواحد).
        assert_file_parity(
            "أنشئ_مجلد(\"فرعي\")\nدالة اذهب():\n    انتقل(\"فرعي\")\nاذهب()\nاطبع المجلد_الحالي()\nانتقل(\"..\")\nاطبع المجلد_الحالي()\n",
            &[],
            "~/فرعي\n~\n",
        );
    }

    #[test]
    fn vm_files_cd_bare_goes_parent() {
        // أمر المالك المباشر 2026-11-09 (يلغي §66 جزئيا):
        // تكافؤ المفسر والآلة الافتراضية — `انتقل()`
        // بلا معاملات → الأب مجلدا واحدا كل مرة.
        assert_file_parity(
            "أنشئ_مجلد(\"أ\")\nانتقل(\"أ\")\nانتقل()\nاطبع المجلد_الحالي()\nانتقل()\nاطبع المجلد_الحالي()\n",
            &[],
            "~\n$نيفا\n",
        );
    }

    #[test]
    fn vm_files_cd_dash_not_found() {
        // أمر المالك المباشر 2026-11-09 (يلغي §66 جزئيا):
        // تكافؤ الخطأ — `-` اسم عادي غير موجود.
        assert_file_error_parity("انتقل(\"-\")\n", &[], "المجلد غير موجود: -");
    }

    #[test]
    fn vm_nav56_stack_parity() {
        // اجتماع 56 §71 — الدفعة 2 (التنقل): المكدس
        // (دفع/تبديل/إخراج/عرض) — تكافؤ المفسر والآلة
        // الافتراضية.
        assert_file_parity(
            "أنشئ_مجلد(\"أ\")\nأنشئ_مجلد(\"ب\")\nادفع_مجلد(\"أ\")\nادفع_مجلد(\"~/ب\")\nس = اعرض_المكدس()\nاطبع طول(س)\nاطبع س[٠]\nاطبع س[١]\nاطبع س[٢]\nادفع_مجلد()\nاطبع المجلد_الحالي()\nأخرج_مجلد()\nاطبع المجلد_الحالي()\nأخرج_مجلد(٠)\nاطبع طول(اعرض_المكدس())\n",
            &[],
            "٣\n~/ب\n~/أ\n~\n~/أ\n~/ب\n١\n",
        );
    }

    #[test]
    fn vm_nav56_cdpath_and_spell_parity() {
        // اجتماع 56 §71: CDPATH وتصحيح الإملاء وكشف
        // PWD — تكافؤ المفسر والآلة الافتراضية.
        assert_file_parity(
            "أنشئ_مجلد(\"مشاريع/هدف\")\nمسار_بحث_المجلدات([\"مشاريع\"])\nانتقل(\"هدف\")\nاطبع المجلد_الحالي()\nانتقل(\"~\")\nانتقل(\"مشاري\")\nاطبع المجلد_الحالي()\nاطبع بيئة(\"المجلد_الحالي\")\n",
            &[],
            "~/مشاريع/هدف\n~/مشاريع\n~/مشاريع\n",
        );
    }

    #[test]
    fn vm_nav56_cdable_var_and_tilde_plus_parity() {
        // اجتماع 56 §71: اسم المتغير كمجلد و`~+N` —
        // تكافؤ المفسر والآلة الافتراضية.
        assert_file_parity(
            "أنشئ_مجلد(\"أ\")\nأنشئ_مجلد(\"ب\")\nوجهتي = \"أ\"\nانتقل(\"وجهتي\")\nاطبع المجلد_الحالي()\nادفع_مجلد(\"~/ب\")\nانتقل(\"~+1\")\nاطبع المجلد_الحالي()\n",
            &[],
            "~/أ\n~/أ\n",
        );
    }

    #[test]
    fn vm_nav56_cdpath_dotdot_regression_parity() {
        // انحدار (إصلاح انهيار `..` المعجمي): مع
        // CDPATH مضبوطا، `..` تذهب إلى الأب فعلا —
        // منطقيا وفيزيائيا — لا تبقى في مكانها.
        // وPOSIX: `../..` لا تُستشار فيها `CDPATH`
        // (مع CDPATH = [~/أ]، `../..` من ~/مشاريع/هدف
        // → ~ لا $نيفا).
        assert_file_parity(
            "أنشئ_مجلد(\"أ\")\nأنشئ_مجلد(\"مشاريع/هدف\")\nمسار_بحث_المجلدات([\"مشاريع\"])\nانتقل(\"هدف\")\nانتقل(\"..\")\nاطبع المجلد_الحالي()\nانتقل(\"هدف\")\nانتقل(\"..\"، \"فيزيائي\")\nاطبع المجلد_الحالي()\nمسار_بحث_المجلدات([\"~/أ\"])\nانتقل(\"~/مشاريع/هدف\")\nانتقل(\"../..\")\nاطبع المجلد_الحالي()\n",
            &[],
            "~/مشاريع\n~/مشاريع\n~\n",
        );
    }

    #[test]
    fn vm_nav56_flag_error_parity() {
        // اجتماع 56 §71: العلم الغريب — تكافؤ رسالة
        // الخطأ حرفيا بين المفسر والآلة الافتراضية.
        assert_file_error_parity(
            "انتقل(\"أ\"، \"غريب\")\n",
            &[("أ/بذرة", "")],
            "علم انتقل غير معروف: غريب",
        );
    }

    #[test]
    fn vm_nav56_stack_empty_error_parity() {
        // اجتماع 56 §71: المكدس الفارغ — تكافؤ رسالة
        // الخطأ حرفيا.
        assert_file_error_parity("أخرج_مجلد()\n", &[], "المكدس فارغ");
    }

    #[test]
    fn vm_files_head_tail_count_search() {
        let seed = "ألف\nباء\nجيم\nدال\nهاء\n";
        assert_file_parity(
            "اطبع اعرض_البداية(\"نص.txt\")\nاطبع اعرض_البداية(\"نص.txt\"، ٢)\nاطبع اعرض_النهاية(\"نص.txt\"، ٢)\nاطبع عد_الأسطر(\"نص.txt\")\nاطبع عد_الكلمات(\"نص.txt\")\nاطبع ابحث_في_ملف(\"باء\"، \"نص.txt\")\n",
            &[("نص.txt", seed)],
            "ألف\nباء\nجيم\nدال\nهاء\nألف\nباء\nدال\nهاء\n٥\n٥\n[\"باء\"]\n",
        );
    }

    #[test]
    fn vm_files_find_tree_size() {
        assert_file_parity(
            "اطبع ابحث_عن(\"ملف\")\nاطبع اعرض_الشجرة()\nاطبع عد_الملفات()\nاطبع حجم_المسار(\"ملف.txt\")\nاطبع حجم_المسار(\"فرعي\")\n",
            &[("ملف.txt", "abc"), ("فرعي/ملف٢.txt", "xy")],
            "[\"فرعي/ملف٢.txt\", \"ملف.txt\"]\n[\"فرعي/\", \"  ملف٢.txt\", \"ملف.txt\"]\n٢\n٣\n٢\n",
        );
    }

    #[test]
    fn vm_files_copy_move_and_delete_dir() {
        assert_file_parity(
            "اكتب_ملف(\"أصل.txt\"، \"محتوى\")\nانسخ(\"أصل.txt\"، \"نسخة.txt\")\nاطبع اقرأ_ملف(\"نسخة.txt\")\nانقل(\"نسخة.txt\"، \"منقول.txt\")\nاطبع موجود(\"نسخة.txt\")\nاطبع اقرأ_ملف(\"منقول.txt\")\nاحذف(\"منقول.txt\")\nاطبع موجود(\"منقول.txt\")\nأنشئ_مجلد(\"سلة\")\nاحذف_مجلد(\"سلة\")\nاطبع موجود(\"سلة\")\n",
            &[],
            "محتوى\nخطأ\nمحتوى\nخطأ\nخطأ\n",
        );
    }

    #[test]
    fn vm_files_mkdir_p_creates_parents() {
        // الاجتماع 53 §67 (الميزة 210): إنشاء متداخل
        // ونجاح صامت على الموجود، والخروج من
        // الساندبوكس مرفوض — في الوضعين معا.
        assert_file_parity(
            "أنشئ_مجلد(\"أ/ب/ج\")\nاطبع نوع_المسار(\"أ/ب/ج\")\nأنشئ_مجلد(\"أ/ب/ج\")\nاطبع \"موجود-صامت\"\nحاول:\n    أنشئ_مجلد(\"../../خارج\")\nماعدا:\n    اطبع \"رفض\"\n",
            &[],
            "مجلد\nموجود-صامت\nرفض\n",
        );
    }

    #[test]
    fn vm_files_copy_onto_itself_refused() {
        // الميزة 202: نسخ الملف على نفسه يرفض قبل أي
        // كتابة، والملف يبقى سليما — في الوضعين معا.
        assert_file_parity(
            "اكتب_ملف(\"أ.txt\"، \"محتوى مهم\")\nحاول:\n    انسخ(\"أ.txt\"، \"أ.txt\")\nماعدا:\n    اطبع \"رفض\"\nاطبع اقرأ_ملف(\"أ.txt\")\nحاول:\n    انسخ(\"./أ.txt\"، \"أ.txt\")\nماعدا:\n    اطبع \"رفض\"\nاطبع اقرأ_ملف(\"أ.txt\")\nانسخ(\"أ.txt\"، \"ب.txt\")\nاطبع اقرأ_ملف(\"ب.txt\")\n",
            &[],
            "رفض\nمحتوى مهم\nرفض\nمحتوى مهم\nمحتوى مهم\n",
        );
    }

    #[test]
    #[cfg(unix)]
    fn vm_files_copy_onto_itself_via_symlink_refused() {
        // الميزة 202 عبر الرابط الرمزي: الهدف يحل إلى
        // الملف نفسه فيُرفض، والملف يبقى سليما.
        assert_file_parity(
            "اكتب_ملف(\"هدف.txt\"، \"هدف\")\nأنشئ_رابط(\"هدف.txt\"، \"رابط.txt\")\nحاول:\n    انسخ(\"رابط.txt\"، \"هدف.txt\")\nماعدا:\n    اطبع \"رفض\"\nاطبع اقرأ_ملف(\"هدف.txt\")\n",
            &[],
            "رفض\nهدف\n",
        );
    }

    #[test]
    fn vm_files_move_dir_into_itself_refused() {
        // الميزة 204: رفض نقل مجلد داخل نفسه في
        // الوضعين بالرسالة نفسها حرفيا.
        assert_file_error_parity(
            "أنشئ_مجلد(\"مجلد\")\nاكتب_ملف(\"مجلد/أ.txt\"، \"محتوى مهم\")\nانقل(\"مجلد\"، \"مجلد/فرعي\")\n",
            &[],
            "لا يمكن نقل المجلد داخل نفسه",
        );
    }

    #[test]
    fn vm_files_move_dir_matches_interpreter() {
        // الميزة 204: نقل مجلد عادي يعمل في الوضعين،
        // والمخرجات واللقطة النهائية متطابقان.
        assert_file_parity(
            "أنشئ_مجلد(\"مجلد/داخلي\")\nاكتب_ملف(\"مجلد/داخلي/أ.txt\"، \"محتوى مهم\")\nانقل(\"مجلد\"، \"منقول\")\nاطبع اقرأ_ملف(\"منقول/داخلي/أ.txt\")\nاطبع موجود(\"مجلد\")\n",
            &[],
            "محتوى مهم\nخطأ\n",
        );
    }

    #[test]
    #[cfg(unix)]
    fn vm_files_symlink_unix() {
        assert_file_parity(
            "اكتب_ملف(\"هدف.txt\"، \"هدف\")\nأنشئ_رابط(\"هدف.txt\"، \"رابط.txt\")\nاطبع اقرأ_ملف(\"رابط.txt\")\n",
            &[],
            "هدف\n",
        );
    }

    #[test]
    fn vm_env_builtin_matches_interpreter() {
        // اجتماع 45 §59 (البنود 39–41): بيئة() في الوضعين
        // بقيمها نفسها.
        assert_file_parity(
            "اطبع بيئة(\"البيت\")\nاطبع بيئة(\"مجلد_البرنامج\")\nاطبع بيئة(\"إصدار_نيفا\")\nاطبع بيئة(\"الألوان\")\n",
            &[],
            "~\n$نيفا\n١٫٠٫٠\ntruecolor\n",
        );
    }

    #[test]
    fn vm_files_version_and_date() {
        // النسخة: نص ثابت — تكافؤ حرفي. التاريخ_الآن:
        // ثواني UTC؛ قد تعبر الثانية بين التشغيلين،
        // فيُقارن التاريخ (أول ١٠ أحرف) فقط.
        let ((iout, ierr, iok, _), (vout, verr, vok, _)) =
            run_both_file_dirs("اطبع النسخة()\nاطبع التاريخ_الآن()\n", &[], false);
        assert!(iok, "interpreter failed (err={})", ierr);
        assert!(vok, "VM failed (err={})", verr);
        let ilines: Vec<&str> = iout.lines().collect();
        let vlines: Vec<&str> = vout.lines().collect();
        assert_eq!(ilines.len(), 2);
        assert_eq!(vlines.len(), 2);
        assert_eq!(ilines[0], "محرك نيفا ١٫٠٫٠");
        assert_eq!(vlines[0], ilines[0]);
        assert_eq!(
            ilines[1].chars().take(10).collect::<String>(),
            vlines[1].chars().take(10).collect::<String>(),
            "date differs: {} vs {}",
            ilines[1],
            vlines[1]
        );
    }

    #[test]
    fn vm_files_user_function_shadows_builtin() {
        // دالة مستخدمة بالاسم نفسه تفوز على المدمجة
        // في الوضعين (فحص وقت التجميع في VM).
        assert_file_parity(
            "دالة اقرأ_ملف(س):\n    ارجع \"ظل\"\nاطبع اقرأ_ملف(\"أي.txt\")\n",
            &[],
            "ظل\n",
        );
    }

    #[test]
    fn vm_files_arity_and_type_errors() {
        assert_file_error_parity("اطبع اقرأ_ملف()\n", &[], "تتوقع 1 معاملات");
        assert_file_error_parity("اطبع اقرأ_ملف(٥)\n", &[], "يتطلب مسارا نصيا");
        assert_file_error_parity("اطبع اكتب_ملف(\"أ.txt\")\n", &[], "تتوقع 2 معاملات");
        assert_file_error_parity("اطبع اقرأ_ملف(\"غائب.txt\")\n", &[], "تعذر قراءة الملف");
    }

    #[test]
    fn vm_files_outside_dir_refused_and_trusted() {
        // رفض خارج المجلد: الرسالة والخروج نفسهما في
        // الوضعين؛ و--اسمح-بالكل يفتح القراءة في الوضعين.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("niva-vm-outside-{}-{}", std::process::id(), n));
        let dir_i = base.join("مفسر");
        let dir_v = base.join("vm");
        for dir in [&dir_i, &dir_v] {
            std::fs::create_dir_all(dir).unwrap();
            // اجتماع 45 §59: الجلسة تبدأ في البيت — أنشئه،
            // فالوضع الموثوق يمرر المسار إلى النظام كما هو.
            std::fs::create_dir_all(dir.join("home")).unwrap();
        }
        std::fs::write(base.join("سر.txt"), "سرّي\n").unwrap();
        // اجتماع 45 §59: الجلسة تبدأ في البيت — الخروج
        // من القاعدة يلزم `../..` لا `..` وحده.
        let src = "اطبع اقرأ_ملف(\"../../سر.txt\")\n";
        let run = |dir: &std::path::Path, trusted: bool, use_vm: bool| -> (String, String, bool) {
            let toks = tokenize(src).unwrap();
            let prog = Parser::new(toks).parse_program().unwrap();
            if use_vm {
                let compiled = Compiler::compile_program(&prog).unwrap();
                let mut vm = Vm::new(compiled, dir.to_path_buf());
                if trusted {
                    vm.set_trusted(true);
                }
                let res = vm.run();
                (
                    vm.output.clone(),
                    match &res {
                        Ok(()) => String::new(),
                        Err(e) => e.to_string(),
                    },
                    res.is_ok(),
                )
            } else {
                let mut interp =
                    crate::interpreter::Interpreter::with_program_dir(dir.to_path_buf());
                if trusted {
                    interp.set_trusted(true);
                }
                let res = interp.run(&prog);
                (
                    interp.output.clone(),
                    match &res {
                        Ok(()) => String::new(),
                        Err(e) => e.to_string(),
                    },
                    res.is_ok(),
                )
            }
        };
        let (iout, ierr, iok) = run(&dir_i, false, false);
        let (vout, verr, vok) = run(&dir_v, false, true);
        assert!(!iok, "interpreter unexpectedly passed");
        assert!(!vok, "VM unexpectedly passed");
        assert_eq!(iout, vout);
        assert_eq!(ierr, verr, "refusal messages differ");
        assert!(ierr.contains("خارج مجلد البرنامج"), "error: {}", ierr);
        let (iout, _, iok) = run(&dir_i, true, false);
        let (vout, _, vok) = run(&dir_v, true, true);
        assert!(iok, "trusted interpreter failed");
        assert!(vok, "trusted VM failed");
        // الملف نفسه ينتهي بسطر جديد، واطبع تضيف سطرا آخر.
        assert_eq!(iout, "سرّي\n\n");
        assert_eq!(vout, iout);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// اجتماع 32: ابنِ VM من مصدر نصي لضبط علميها
    /// (البث الحي وملف المصدر) قبل التشغيل.
    fn vm_from_src(src: &str) -> Result<Vm, RioError> {
        let tokens = tokenize(src)?;
        let program = Parser::new(tokens).parse_program()?;
        let compiled = Compiler::compile_program(&program)?;
        Ok(Vm::new(
            compiled,
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        ))
    }

    #[test]
    fn vm_engine_version_bare_and_banner() {
        // اجتماع 32، السؤال السادس — مرآة اختبار المفسر:
        // إصدار_المحرك() ترجع النسخة المجردة؛ والنسخة()
        // تبقى اللافتة.
        let (out, _, ok) = run_vm_src("اطبع إصدار_المحرك()\nاطبع النسخة()\n");
        assert!(ok);
        assert_eq!(out, "١٫٠٫٠\nمحرك نيفا ١٫٠٫٠\n");
        // بلا وسائط فقط: وسائط خطأ عدد.
        let (_, _, ok) = run_vm_src("اطبع إصدار_المحرك(١)\n");
        assert!(!ok);
    }

    #[test]
    fn vm_bare_function_reference_is_an_error() {
        // اجتماع 32، السؤال الثاني (2) — مرآة اختبار
        // المفسر: مرجع دالة كعبارة مجردة خطأ يسمي الدالة.
        let (_, err, ok) = run_vm_src("دالة س():\n    ارجع ١\nس\n");
        assert!(!ok);
        assert!(err.contains("دالة بلا استدعاء: س"), "err: {}", err);
        // دالة مستوردة مجردة (من وحدة أصلية).
        let (_, err, ok) = run_vm_src("من رياضيات استورد جذر\nجذر\n");
        assert!(!ok);
        assert!(err.contains("دالة بلا استدعاء: جذر"), "err: {}", err);
        // دالة وحدة أصلية مجردة.
        let (_, err, ok) = run_vm_src("استورد جهاز\nجهاز.أزل_تنبيه\n");
        assert!(!ok);
        assert!(
            err.contains("دالة بلا استدعاء: أزل_تنبيه"),
            "err: {}",
            err
        );
        // الإسناد والتمرير والطباعة تبقى صالحة.
        let (out, _, ok) =
            run_vm_src("من رياضيات استورد جذر\nس = جذر\nاطبع نوع(س)\nاطبع جذر\n");
        assert!(ok);
        assert_eq!(out, "دالة\n<دالة جذر>\n");
        // استدعاء مجرد يرجع غير دالة: قيمته تُتجاهل.
        let (out, _, ok) = run_vm_src("دالة أعط():\n    ارجع ٥\nأعط()\n");
        assert!(ok);
        assert_eq!(out, "");
        // استدعاء مجرد يرجع دالة: خطأ أيضا (قيمة دالة
        // في عبارة مجردة).
        let (_, err, ok) = run_vm_src(
            "دالة مصنع():\n    ارجع دالة_مجهولة(س): س\nمصنع()\n",
        );
        assert!(!ok);
        assert!(
            err.contains("دالة بلا استدعاء: مجهولة"),
            "err: {}",
            err
        );
    }

    #[test]
    fn vm_live_output_streams_without_retaining() {
        // اجتماع 32، السؤال الثامن — مرآة اختبار المفسر:
        // مع البث الحي لا يُحتفظ بالسطر في output؛
        // وبدونه يُحتفظ كما كان.
        let mut vm = vm_from_src("اطبع ١\nاطبع ٢\n").unwrap();
        vm.set_live_output(true);
        assert!(vm.run().is_ok());
        assert_eq!(vm.output, "");
        let mut vm = vm_from_src("اطبع ١\nاطبع ٢\n").unwrap();
        vm.set_live_output(false);
        assert!(vm.run().is_ok());
        assert_eq!(vm.output, "١\n٢\n");
    }

    #[test]
    fn vm_runtime_error_names_source_file() {
        // اجتماع 32، السؤال الخامس — مرآة اختبار المفسر:
        // خطأ التشغيل يسمي الملف المضبوط.
        let mut vm = vm_from_src("س = ١ / ٠\n").unwrap();
        vm.set_source_file("برنامج.نيفا");
        let res = vm.run();
        assert!(res.is_err());
        let e = res.unwrap_err();
        assert_eq!(e.file.as_deref(), Some("برنامج.نيفا"));
        assert!(e.to_string().contains("الملف برنامج.نيفا"));
        assert!(e.to_string().contains("قسمة على صفر"));
    }

    #[test]
    fn vm_module_errors_name_module_file() {
        // اجتماع 32، السؤال الخامس — مرآة اختبار المفسر:
        // خطأ في وحدة — في مستواها الأعلى أو داخل دالة
        // منها تُستدعى من الرئيسي — يُسنَد إلى ملف الوحدة.
        let (_, err, ok) = run_vm_src_with_files(
            "استورد م\nاطبع م.اقسم(١)\n",
            &[("م.نيفا", "صدر اقسم\nدالة اقسم(س):\n    ارجع س / ٠\n")],
        );
        assert!(!ok);
        assert!(err.contains("قسمة على صفر"), "got: {}", err);
        assert!(err.contains("م.نيفا"), "got: {}", err);
        // والخطأ في المستوى الأعلى للوحدة نفسه — مع
        // بادئة زمن التحميل «في وحدة ع: ».
        let (_, err, ok) =
            run_vm_src_with_files("استورد ع\n", &[("ع.نيفا", "س = ١ / ٠\n")]);
        assert!(!ok);
        assert!(err.contains("قسمة على صفر"), "got: {}", err);
        assert!(err.contains("في وحدة ع: "), "got: {}", err);
        assert!(err.contains("ع.نيفا"), "got: {}", err);
    }

    // الدفعة 1 — رموز الخروج (اجتماع 54 §69): مرآة
    // اختبارات المفسر نفسها — تكافؤ مفسر/VM.
    fn run_vm_full(src: &str) -> (Vm, Result<(), RioError>) {
        let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let tokens = tokenize(src).unwrap();
        let program = Parser::new(tokens).parse_program().unwrap();
        let compiled = Compiler::compile_program(&program).unwrap();
        let mut vm = Vm::new(compiled, dir);
        let res = vm.run();
        (vm, res)
    }

    #[test]
    fn vm_exit_code_starts_zero() {
        let (mut vm, res) = run_vm_full("اطبع رمز_الخروج()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "٠\n");
        assert_eq!(vm.last_exit_code(), 0);
        assert_eq!(vm.take_exit_code(), None);
    }

    #[test]
    fn vm_exit_code_one_after_caught_error() {
        let (mut vm, res) = run_vm_full(
            "حاول:\n    س = غير_معرف\nماعدا:\n    اطبع رمز_الخروج()\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "١\n");
        assert_eq!(vm.last_exit_code(), 1);
    }

    #[test]
    fn vm_exit_with_code_ends_program() {
        let (mut vm, res) = run_vm_full("اطبع \"قبل\"\nخروج(٥)\nاطبع \"بعد\"\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "قبل\n");
        assert_eq!(vm.take_exit_code(), Some(5));
    }

    #[test]
    fn vm_exit_without_arg_uses_last_code() {
        let (mut vm, res) = run_vm_full(
            "حاول:\n    س = غير_معرف\nماعدا:\n    خروج()\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "");
        assert_eq!(vm.take_exit_code(), Some(1));
    }

    #[test]
    fn vm_exit_rejects_bad_codes() {
        for src in [
            "خروج(-١)\n",
            "خروج(٢٥٦)\n",
            "خروج(٣٫٥)\n",
            "خروج(\"نص\")\n",
        ] {
            let (_, res) = run_vm_full(src);
            let err = res.unwrap_err();
            assert!(
                err.to_string()
                    .contains("رمز الخروج يجب أن يكون عددا صحيحا بين ٠ و٢٥٥"),
                "src {}: got: {}",
                src,
                err
            );
        }
        let (_, res) = run_vm_full("خروج(١، ٢)\n");
        assert!(res.is_err());
    }

    #[test]
    fn vm_program_name_defaults_and_set() {
        let (mut vm, res) = run_vm_full("اطبع اسم_البرنامج()\n");
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "نيفا\n");
        let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let tokens = tokenize("اطبع اسم_البرنامج()\n").unwrap();
        let program = Parser::new(tokens).parse_program().unwrap();
        let compiled = Compiler::compile_program(&program).unwrap();
        let mut vm2 = Vm::new(compiled, dir);
        vm2.set_source_file("برنامج.نيفا");
        assert!(vm2.run().is_ok());
        assert_eq!(vm2.output, "برنامج.نيفا\n");
    }

    #[test]
    fn vm_exit_inside_function_and_loop() {
        let (mut vm, res) = run_vm_full(
            "دالة اخرج():\n    خروج(٧)\nاخرج()\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "");
        assert_eq!(vm.take_exit_code(), Some(7));
        let (mut vm, res) = run_vm_full(
            "س = ٠\nبينما س < ٣:\n    س = س + ١\n    خروج(٧)\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "");
        assert_eq!(vm.take_exit_code(), Some(7));
    }

    #[test]
    fn vm_exit_skips_except_runs_finally() {
        let (mut vm, res) = run_vm_full(
            "حاول:\n    خروج(٥)\nماعدا:\n    اطبع \"ماعدا\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "أخيرا\n");
        assert_eq!(vm.take_exit_code(), Some(5));
    }

    #[test]
    fn vm_exit_inside_function_runs_caller_finally() {
        let (mut vm, res) = run_vm_full(
            "دالة اخرج():\n    خروج(٣)\nحاول:\n    اخرج()\nماعدا:\n    اطبع \"ماعدا\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
        );
        assert!(res.is_ok(), "{:?}", res);
        assert_eq!(vm.output, "أخيرا\n");
        assert_eq!(vm.take_exit_code(), Some(3));
    }

    // الدفعة 3 — التشغيل (اجتماع 57 §72): مجلد مؤقت
    // بملفاته، وتشغيل مصدر على VM مع رمز الخروج.
    fn batch3_vm_dir(files: &[(&str, &str)]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "niva-batch3-vm-{}-{}",
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

    fn batch3_vm_run(
        src: &str,
        dir: &std::path::Path,
    ) -> (String, Result<(), RioError>, Option<u8>) {
        let tokens = tokenize(src).unwrap();
        let program = Parser::new(tokens).parse_program().unwrap();
        let compiled = Compiler::compile_program(&program).unwrap();
        let mut vm = Vm::new(compiled, dir.to_path_buf());
        let res = vm.run();
        let out = std::mem::take(&mut vm.output);
        let code = vm.take_exit_code();
        (out, res, code)
    }

    fn batch3_interp_run(
        src: &str,
        dir: &std::path::Path,
    ) -> (String, Result<(), RioError>, Option<u8>) {
        let toks = tokenize(src).unwrap();
        let prog = Parser::new(toks).parse_program().unwrap();
        let mut interp =
            crate::interpreter::Interpreter::with_program_dir(dir.to_path_buf());
        let res = interp.run(&prog);
        let code = interp.take_exit_code();
        (interp.output.clone(), res, code)
    }

    #[test]
    fn batch3_vm_type_command_layers_parity() {
        let ((iout, ierr, iok), (vout, verr, vok)) = run_both_with_files(
            "دالة عرفتها():\n    ارجع ١\nاطبع نوع_الامر(\"شغل\")\nاطبع نوع_الامر(\"ات\")\nاطبع نوع_الامر(\"اذا\")\nاطبع نوع_الامر(\"عرفتها\")\nاطبع نوع_الامر(\"انتقل\")\nاطبع نوع_الامر(\"ادفع مجلد\")\nاطبع نوع_الامر(\"أداة\")\nاطبع نوع_الامر(\"غائب\")\n",
            &[("أداة.نيفا", "اطبع \"أداة\"\n")],
        );
        assert!(iok, "interpreter failed: {}", ierr);
        assert!(vok, "VM failed: {}", verr);
        assert_eq!(iout, vout, "interpreter vs VM differ");
        assert_eq!(
            vout,
            "امر طرفي\nاختصار\nكلمة محجوزة\nدالة معرفة\nدالة مدمجة\nدالة مدمجة\nاداة في $نيفا\nغير موجود\n",
            "got: {}",
            vout
        );
    }

    #[test]
    fn batch3_vm_tool_location_parity() {
        let ((iout, ierr, iok), (vout, verr, vok)) = run_both_with_files(
            "اطبع مكان_الاداة(\"أداة\")\nاطبع مكان_الاداة(\"غائب\")\n",
            &[("أداة.نيفا", "اطبع \"أداة\"\n")],
        );
        assert!(iok, "interpreter failed: {}", ierr);
        assert!(vok, "VM failed: {}", verr);
        assert_eq!(iout, vout, "interpreter vs VM differ");
        assert!(vout.contains("أداة.نيفا"), "got: {}", vout);
        assert!(vout.contains("لاشيء"), "got: {}", vout);
    }

    #[test]
    fn batch3_vm_exec_session_replaces_parity() {
        let dir = batch3_vm_dir(&[
            ("يخرج.نيفا", "اطبع \"داخل\"\nخروج(٧)\nاطبع \"بعد\"\n"),
            ("فاشل.نيفا", "س = ١ / ٠\n"),
        ]);
        // الاستبدال بلا عودة في المحركين معا.
        let src = "اطبع \"قبل\"\nاستبدل_الجلسة(\"يخرج\")\nاطبع \"بعد\"\n";
        let (iout, ires, icode) = batch3_interp_run(src, &dir);
        let (vout, vres, vcode) = batch3_vm_run(src, &dir);
        assert!(ires.is_ok(), "{:?}", ires);
        assert!(vres.is_ok(), "{:?}", vres);
        assert_eq!(iout, "قبل\nداخل\n", "interp got: {}", iout);
        assert_eq!(vout, "قبل\nداخل\n", "VM got: {}", vout);
        assert_eq!(icode, Some(7));
        assert_eq!(vcode, Some(7));
        // كـ`خروج`: يشغّل `أخيرا` ولا يمسكه `ماعدا`.
        let src2 = "حاول:\n    استبدل_الجلسة(\"يخرج\")\nماعدا:\n    اطبع \"ممسوك\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n";
        let (iout, _, icode) = batch3_interp_run(src2, &dir);
        let (vout, _, vcode) = batch3_vm_run(src2, &dir);
        assert_eq!(iout, "داخل\nأخيرا\n", "interp got: {}", iout);
        assert_eq!(vout, "داخل\nأخيرا\n", "VM got: {}", vout);
        assert_eq!(icode, Some(7));
        assert_eq!(vcode, Some(7));
        // الأداة الفاشلة: رمزها 1 في المحركين.
        let src3 = "استبدل_الجلسة(\"فاشل\")\nاطبع \"بعد\"\n";
        let (iout, _, icode) = batch3_interp_run(src3, &dir);
        let (vout, _, vcode) = batch3_vm_run(src3, &dir);
        assert!(iout.contains("قسمة على صفر"), "interp got: {}", iout);
        assert!(vout.contains("قسمة على صفر"), "VM got: {}", vout);
        assert_eq!(icode, Some(1));
        assert_eq!(vcode, Some(1));
        // الأداة الغائبة: 127 بعلامة الملف المطلوب.
        let (_, ires, _) = batch3_interp_run("استبدل_الجلسة(\"غائب\")\n", &dir);
        let (_, vres, _) = batch3_vm_run("استبدل_الجلسة(\"غائب\")\n", &dir);
        assert!(ires.unwrap_err().is_module_not_found());
        assert!(vres.unwrap_err().is_module_not_found());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // المرحلة (د) — الإكمال (اجتماعات 66–69 §81–
    // §84): مرآة اختبارات المفسر على VM — التكافؤ
    // بين المحركين لكل ما بُني في المرحلة.
    #[test]
    fn stage_completion_vm_env_and_strings() {
        let (out, err, ok) = run_vm_src(
            "اطبع بيئة_أو(\"المسار\"، \"بديل\")\nاطبع بيئة_أو(\"اسم_غير_موجود_تماماً\"، \"بديل\")\nاطبع بيئة_إن_وجد(\"المسار\"، \"موجود\")\nاطبع بيئة_إن_وجد(\"اسم_غير_موجود_تماماً\"، \"موجود\")\nاطبع أزل_بادئة(\"تقرير.txt\"، \"تقرير.\")\nاطبع أزل_لاحقة(\"تقرير.txt\"، \".txt\")\nاطبع فهارس([١٠، ٢٠، ٣٠])\nاطبع \"أبجد\".اقطع(١، ٢)\nاطبع \"أبجد\".اقطع(-٢)\nاطبع \"أ-أ-أ\".استبدل_أول(\"أ\"، \"ب\")\n",
        );
        assert!(ok, "err: {}", err);
        assert_eq!(
            out,
            "$نيفا\nبديل\nموجود\n\ntxt\nتقرير\n[٠, ١, ٢]\nبج\nجد\nب-أ-أ\n"
        );
    }

    #[test]
    fn stage_completion_vm_session_vars_and_functions() {
        let (out, err, ok) = run_vm_src(
            "س = ٥\nنص = \"أ\"\nدالة جمع(أ، ب):\n    ارجع أ + ب\nاطبع اعرض_المتغيرات()\nاطبع اعرض_الدوال()\nاحذف_دالة(\"جمع\")\nاطبع اعرض_الدوال()\n",
        );
        assert!(ok, "err: {}", err);
        assert_eq!(out, "[\"س=٥\", \"نص=\\\"أ\\\"\"]\n[\"جمع\"]\n[]\n");
        let (_, err, ok) = run_vm_src("احذف_دالة(\"غائبة\")\n");
        assert!(!ok);
        assert!(err.contains("دالة غير موجودة: غائبة"), "got: {}", err);
        let (_, err, ok) = run_vm_src("س = ٥\nاحذف_دالة(\"س\")\n");
        assert!(!ok);
        assert!(err.contains("ليست دالة: س"), "got: {}", err);
    }

    #[test]
    fn stage_completion_vm_list_and_details_and_flags() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-vm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("home").join("أ.txt"), "نص").unwrap();
        std::fs::write(dir.join("home").join("ب.txt"), "نص أطول").unwrap();
        std::fs::write(dir.join("home").join(".مخفي"), "خ").unwrap();
        let (out, err, ok) = run_vm_src_dir("اطبع اعرض_الملفات()\n", &dir);
        assert!(ok, "err: {}", err);
        assert_eq!(out, "[\"أ.txt\", \"ب.txt\"]\n", "got: {}", out);
        let (out, err, ok) = run_vm_src_dir("اطبع اعرض_الملفات(\"أ.txt\")\n", &dir);
        assert!(ok, "err: {}", err);
        assert_eq!(out, "[\"أ.txt\"]\n", "got: {}", out);
        let (_, err, ok) = run_vm_src_dir("اطبع اعرض_الملفات(\"غائب\")\n", &dir);
        assert!(!ok);
        assert!(err.contains("المسار غير موجود: غائب"), "got: {}", err);
        let (out, err, ok) =
            run_vm_src_dir("اطبع اعرض_الملفات(\"أظهر_المخفية\")\n", &dir);
        assert!(ok, "err: {}", err);
        assert!(out.contains(".مخفي"), "got: {}", out);
        let (out, err, ok) = run_vm_src_dir(
            "اطبع تفاصيل_المسار(\"أ.txt\")\n",
            &dir,
        );
        assert!(ok, "err: {}", err);
        for key in [
            "\"نوع\": \"ملف\"",
            "\"حجم\": ٤",
            "\"أذونات\": \"-",
            "\"مالك\": ",
            "\"روابط\": ١",
            "\"هدف\": لاشيء",
        ] {
            assert!(out.contains(key), "missing {} in: {}", key, out);
        }
        let (out, err, ok) = run_vm_src_dir(
            "انسخ(\"أ.txt\"، \"ب.txt\"، \"لا_تكتب\")\nاطبع اقرأ_ملف(\"ب.txt\")\nاحذف(\"غائب.txt\"، \"تجاهل_الغائب\")\nاطبع \"تم\"\n",
            &dir,
        );
        assert!(ok, "err: {}", err);
        assert_eq!(out, "نص أطول\nتم\n", "got: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_completion_vm_source_file() {
        let dir = std::env::temp_dir()
            .join(format!("niva-stage-d-vm-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(
            dir.join("home").join("تعريفات.نيفا"),
            "س = ٤٢\nدالة ضاعف(ن):\n    ارجع ن * ٢\n",
        )
        .unwrap();
        let (out, err, ok) = run_vm_src_dir(
            "مصدر(\"تعريفات.نيفا\")\nاطبع س\nاطبع ضاعف(٣)\n",
            &dir,
        );
        assert!(ok, "err: {}", err);
        assert_eq!(out, "٤٢\n٦\n", "got: {}", out);
        let (_, err, ok) = run_vm_src_dir("مصدر(\"غائب\")\n", &dir);
        assert!(!ok);
        assert!(err.contains("الملف غير موجود"), "got: {}", err);
        std::fs::write(dir.join("home").join("دوري.نيفا"), "مصدر(\"دوري.نيفا\")\n").unwrap();
        let (_, err, ok) = run_vm_src_dir("مصدر(\"دوري.نيفا\")\n", &dir);
        assert!(!ok);
        assert!(err.contains("يُستدعى دوريا"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
