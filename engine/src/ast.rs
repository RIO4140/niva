//! AST: a clear abstract syntax tree for every statement and expression.
//! Each node carries its line + column so every later error (parse, type,
//! runtime) can point at the exact place in the Arabic source.

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
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
    In,
    NotIn,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Var(String),
    Index { target: Box<Expr>, index: Box<Expr> },
    // Phase 1A Step 4: classes, dot field assignment: ك.س = ٥
    Dot { target: Box<Expr>, name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    Ident(String),
    List(Vec<Expr>),
    Dict(Vec<(String, Expr)>),
    Binary { left: Box<Expr>, op: BinOp, right: Box<Expr> },
    Unary { op: UnOp, expr: Box<Expr> },
    // البند 73 (`:=` — الاجتماع 83 §98): تعبير إسناد —
    // اسم وحده لا هدف عام؛ يسنِد ثم ينتج القيمة.
    AssignExpr { name: String, value: Box<Expr> },
    // البندان 109–112 (الاجتماع 83 §98): الاستدعاء
    // يكتسب `kwargs` — معاملات مفتاحية `اسم = قيمة`
    // بترتيب كتابتها؛ والموضعية في `args` كما كانت.
    // البنود 145–147 (العموميات — الاجتماع 83 §98):
    // `type_args` معاملات نوع التخصيص (`اسم[نوع، ...]`)
    // بترتيب كتابتها؛ فارغة = بلا تخصيص صريح.
    Call { callee: Box<Expr>, args: Vec<Expr>, kwargs: Vec<(String, Expr)>, type_args: Vec<String> },
    Index { target: Box<Expr>, index: Box<Expr> },
    // Phase 1A Step 2: module.member. Step 4: object.member too.
    // Dot on any other value is a runtime type error.
    Dot { target: Box<Expr>, name: String },
    // Phase 1A Step 4: classes. أنشئ فئة(معاملات)
    // البندان 109–112: المنشئ يقبل معاملات مفتاحية
    // كالاستدعاء العادي (`أنشئ فئة(س = ١)`).
    // البنود 145–147: `type_args` معاملات نوع التخصيص
    // (`أنشئ فئة[نوع](...)`)؛ فارغة = بلا تخصيص صريح.
    Create { class: String, args: Vec<Expr>, kwargs: Vec<(String, Expr)>, type_args: Vec<String> },
    // Step 1 (Meeting 13): lambda, single expression,
    // implicit return. دالة_مجهولة(س): س + 1 — a first-class
    // Function value that captures lexically like a closure.
    // param_types[i]: Some(type) for params[i], None = untyped.
    Lambda { params: Vec<String>, param_types: Vec<Option<String>>, body: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: usize,
    pub col: usize,
}

impl Expr {
    pub fn new(kind: ExprKind, line: usize, col: usize) -> Self {
        Self { kind, line, col }
    }
}

/// Known type names for gradual typing (Phase 1A Step 3).
/// `أي` means Any / unknown: no static or runtime check.
pub const TYPE_NAMES: &[&str] = &[
    "عدد", "نص", "منطقي", "قائمة", "قاموس", "لاشيء", "دالة", "وحدة", "فئة", "كائن", "واجهة", "أي",
];

/// String/list method names (Meeting 14). Shared by the
/// interpreter (dispatch) and the semantic analyzer (static check).
pub const STR_METHODS: &[&str] = &["طول", "كبير", "صغير", "قص", "استبدل", "قسم", "يحتوي", "اقطع", "استبدل_أول"];
pub const LIST_METHODS: &[&str] = &["طول", "أضف", "أزل", "يحتوي"];

// البند 88 — مدير السياق (الاجتماع 88 §103):
// الاستدعاءات الداخلية التي يفكك إليها المحلل
// `عند` — بلا عقد AST جديدة وبلا لمس cgen.rs
// (سابقة `ولد` و`طابق`): `__عند_افحص(مدير)`
// يفحص الدالتين قبل أي استدعاء؛ و`__عند_خطأ()`
// يعيد قاموس الخطأ الممسوك أو لاشيء؛ و
// `__عند_أعد_رفع(خطأ)` يعيد رفع الخطأ نفسه من
// القاموس المحفوظ. البادئة `__` محجوزة داخليا.
pub const CTX_CHECK: &str = "__عند_افحص";
pub const CTX_ERROR: &str = "__عند_خطأ";
pub const CTX_RERAISE: &str = "__عند_أعد_رفع";

// ب6 — المولدات (الاجتماع 85 §100): أسماء الاستدعاءات
// الداخلية التي يفكك إليها المحلل `ولد` و`ولد من`
// والتعبير التوليدي — بلا عقد AST جديدة وبلا لمس
// cgen.rs (سابقة الصيغ الحرفية المفككة في ج6):
// `ولد قيمة` → `__ولد(قيمة)`، و`ولد من مولد` →
// `__ولد_من(مولد)`، و`(تعبير لاجل س في قابل اذا شرط)`
// → `__مولد_تعبير(دالة التعبير، دالة الشرط أو لاشيء،
// القابل)`. المفسر والآلة يعترضانها وقت التشغيل؛
// والمرآة C ترفضها بالرسالة القانونية. البادئة
// `__` محجوزة داخليا — لا يكتبها المستخدم عادة.
pub const YIELD_CALL: &str = "__ولد";
pub const YIELD_FROM_CALL: &str = "__ولد_من";
pub const GENEXPR_CALL: &str = "__مولد_تعبير";

/// ب6: هل يحوي جسم دالة استدعاء `ولد`/`ولد من`
/// (بعد التفكيك) — فالدالة مولد. لا ينزل إلى أجسام
/// دوال متداخلة أو لامدا أو طرق فئات: تلك مولدات
/// مستقلة بذاتها، لا تجعل الخارجي مولدا.
pub fn body_has_yield(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_yield)
}

fn stmt_has_yield(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Assign { value, .. } => expr_has_yield(value),
        StmtKind::Print { args } => args.iter().any(expr_has_yield),
        StmtKind::If { cond, then_body, elifs, else_body } => {
            expr_has_yield(cond)
                || then_body.iter().any(stmt_has_yield)
                || elifs.iter().any(|(c, b)| expr_has_yield(c) || b.iter().any(stmt_has_yield))
                || else_body.as_ref().map_or(false, |b| b.iter().any(stmt_has_yield))
        }
        StmtKind::While { cond, body } | StmtKind::RepeatWhile { cond, body } => {
            expr_has_yield(cond) || body.iter().any(stmt_has_yield)
        }
        StmtKind::For { iterable, body, .. } => {
            expr_has_yield(iterable) || body.iter().any(stmt_has_yield)
        }
        StmtKind::RepeatCount { count, body } => {
            expr_has_yield(count) || body.iter().any(stmt_has_yield)
        }
        StmtKind::Return { value } => value.as_ref().map_or(false, expr_has_yield),
        StmtKind::Try { try_body, except_body, else_body, finally_body } => {
            try_body.iter().any(stmt_has_yield)
                || except_body.as_ref().map_or(false, |b| b.iter().any(stmt_has_yield))
                || else_body.as_ref().map_or(false, |b| b.iter().any(stmt_has_yield))
                || finally_body.as_ref().map_or(false, |b| b.iter().any(stmt_has_yield))
        }
        StmtKind::ExprStmt { expr } => expr_has_yield(expr),
        StmtKind::Decorated { def, .. } => stmt_has_yield(def),
        // دالة/فئة/واجهة متداخلة: لا نزول — جسمها
        // مستقل، ويُفحص وحده عند تعريفه.
        StmtKind::FuncDef { .. } | StmtKind::ClassDef { .. } | StmtKind::InterfaceDef { .. } => false,
        // إعلانات النطاق واسم النوع: بلا جسم ولا تعبير.
        StmtKind::GlobalDecl { .. } | StmtKind::NonlocalDecl { .. } | StmtKind::TypeAlias { .. } => false,
        StmtKind::Break | StmtKind::Continue | StmtKind::Pass | StmtKind::Import { .. } | StmtKind::Export { .. } => false,
    }
}

/// البنود 77 و78 (الاجتماع 83 §98): اجمع إعلانات
/// `عام` و`نطاق` في جسم دالة (أسماء عامة، أسماء
/// محيطة) — نزولا في الكتل المتداخلة (اذا/بينما/
/// لكل/كرر/حاول/والا) لا في أجسام دوال أو فئات
/// متداخلة (نطاقات مستقلة). تُحسَم مرة واحدة عند
/// تعريف الدالة، فيسري الإعلان على الجسم كله ولو
/// جاء بعد الإسناد — كدلالة بايثون.
pub fn collect_scope_decls(body: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut globals: Vec<String> = Vec::new();
    let mut nonlocals: Vec<String> = Vec::new();
    collect_scope_decls_into(body, &mut globals, &mut nonlocals);
    (globals, nonlocals)
}

fn collect_scope_decls_into(
    body: &[Stmt],
    globals: &mut Vec<String>,
    nonlocals: &mut Vec<String>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::GlobalDecl { names } => {
                for n in names {
                    if !globals.contains(n) {
                        globals.push(n.clone());
                    }
                }
            }
            StmtKind::NonlocalDecl { names } => {
                for n in names {
                    if !nonlocals.contains(n) {
                        nonlocals.push(n.clone());
                    }
                }
            }
            StmtKind::If { then_body, elifs, else_body, .. } => {
                collect_scope_decls_into(then_body, globals, nonlocals);
                for (_, b) in elifs {
                    collect_scope_decls_into(b, globals, nonlocals);
                }
                if let Some(b) = else_body {
                    collect_scope_decls_into(b, globals, nonlocals);
                }
            }
            StmtKind::While { body, .. } | StmtKind::RepeatWhile { cond: _, body } => {
                collect_scope_decls_into(body, globals, nonlocals);
            }
            StmtKind::For { body, .. } | StmtKind::RepeatCount { body, .. } => {
                collect_scope_decls_into(body, globals, nonlocals);
            }
            StmtKind::Try { try_body, except_body, else_body, finally_body } => {
                collect_scope_decls_into(try_body, globals, nonlocals);
                if let Some(b) = except_body {
                    collect_scope_decls_into(b, globals, nonlocals);
                }
                if let Some(b) = else_body {
                    collect_scope_decls_into(b, globals, nonlocals);
                }
                if let Some(b) = finally_body {
                    collect_scope_decls_into(b, globals, nonlocals);
                }
            }
            // دالة/فئة متداخلة: نطاق مستقل — لا نزول.
            _ => {}
        }
    }
}

fn expr_has_yield(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call { callee, args, kwargs, .. } => {
            if let ExprKind::Ident(name) = &callee.kind {
                if name == YIELD_CALL || name == YIELD_FROM_CALL {
                    return true;
                }
            }
            expr_has_yield(callee)
                || args.iter().any(expr_has_yield)
                || kwargs.iter().any(|(_, v)| expr_has_yield(v))
        }
        ExprKind::Binary { left, right, .. } => expr_has_yield(left) || expr_has_yield(right),
        ExprKind::Unary { expr: inner, .. } => expr_has_yield(inner),
        // البند 73: الإسناد داخل تعبير — قيمته قد تحوي ولدا.
        ExprKind::AssignExpr { value, .. } => expr_has_yield(value),
        ExprKind::List(elems) => elems.iter().any(expr_has_yield),
        ExprKind::Dict(entries) => entries.iter().any(|(_, v)| expr_has_yield(v)),
        ExprKind::Index { target, index } => expr_has_yield(target) || expr_has_yield(index),
        ExprKind::Dot { target, .. } => expr_has_yield(target),
        ExprKind::Create { args, kwargs, .. } => {
            args.iter().any(expr_has_yield)
                || kwargs.iter().any(|(_, v)| expr_has_yield(v))
        }
        // لامدا: جسمها تعبير مستقل — لا يُحسب على الخارجي.
        ExprKind::Lambda { .. } => false,
        ExprKind::Number(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Nil | ExprKind::Ident(_) => false,
    }
}

pub fn is_known_type(name: &str) -> bool {
    TYPE_NAMES.contains(&name)
}

/// معامل نوع واحد في تعريف عام (البنود 145–147 —
/// الاجتماع 83 §98): `ت` وحده، أو `ت: حد`، أو
/// `ت = افتراضي`، أو `ت: حد = افتراضي`. الحد والافتراضي
/// عرضان نصيان قانونيان لنوع (كما يعرضهما TypeExpr).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<String>,
    pub default: Option<String>,
}

impl TypeParam {
    pub fn new(name: String, bound: Option<String>, default: Option<String>) -> Self {
        Self { name, bound, default }
    }

    /// الترميز النصي لجدول واحد في cgen: `ت` أو `ت:حد`
    /// أو `ت=افتراضي` أو `ت:حد=افتراضي` — فالعرض القانوني
    /// للنوع بلا `:` ولا `=` ولا `؛` أبدا.
    pub fn encode(&self) -> String {
        let mut s = self.name.clone();
        if let Some(b) = &self.bound {
            s.push(':');
            s.push_str(b);
        }
        if let Some(d) = &self.default {
            s.push('=');
            s.push_str(d);
        }
        s
    }
}

/// تمثيل نوعي حقيقي (البند 8 — الاجتماع 72 §87):
/// النوع اليوم لم يعد نصا يُقارَن حرفيا، بل بنية —
/// اسم مسمى، أو نوع عام مطبَّق `اسم[نوع، نوع]`
/// (مثل `قائمة[عدد]` و`قاموس[نص، عدد]`)، تميزه
/// `parse_type_name` بنيويا لا نصا، فيكفي أساسا
/// للبند 4 (الأنواع العامة) القادم. العرض القانوني
/// يفصل المعاملات بفاصلة عربية `،`، ويُحلَّل من جديد
/// بالدالة `parse` نفسها — فالنص المخزّن في التعليقات
/// هو هذا العرض دائما.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Generic { name: String, args: Vec<TypeExpr> },
}

impl TypeExpr {
    /// العرض القانوني: الاسم وحده، أو `اسم[نوع، نوع]`.
    pub fn display(&self) -> String {
        match self {
            TypeExpr::Named(n) => n.clone(),
            TypeExpr::Generic { name, args } => {
                let inner: Vec<String> = args.iter().map(|a| a.display()).collect();
                format!("{}[{}]", name, inner.join("، "))
            }
        }
    }

    /// الاسم الأساسي: المسمى نفسه، أو اسم العام.
    pub fn base_name(&self) -> &str {
        match self {
            TypeExpr::Named(n) => n,
            TypeExpr::Generic { name, .. } => name,
        }
    }

    /// هل هذا النوع `أي` (بلا فحص)؟
    pub fn is_any(&self) -> bool {
        matches!(self, TypeExpr::Named(n) if n == "أي")
    }

    /// هل يطابق نوع القيمة الأساسي هذا النوع المعلن
    /// (بلا فحص عناصر)؟ العام يطابقه أساسه: `قائمة`
    /// تطابق `قائمة[عدد]` ستاتيكيا، وفحص العناصر
    /// وقت التشغيل وحده.
    pub fn matches_base(&self, actual: &str) -> bool {
        if self.is_any() || actual == "أي" {
            return true;
        }
        self.base_name() == actual
    }

    /// هل يطابق النوع المعلن `actual` (نص نوع قد يكون
    /// عاما هو الآخر) هذا النوع؟ مطابقة بنيوية كاملة:
    /// الاسمان المسميان متساويان؛ والعام يطابقه أساس
    /// عار (`قائمة` تطابق `قائمة[عدد]`) أو عام مثله
    /// بمعاملات مطابقة واحدا واحدا (`قائمة[عدد]` لا
    /// تطابق `قائمة[نص]`).
    pub fn matches_declared(&self, actual: &str) -> bool {
        if self.is_any() || actual == "أي" {
            return true;
        }
        let Some(a) = TypeExpr::parse(actual) else {
            return self.base_name() == actual;
        };
        match (self, &a) {
            (TypeExpr::Named(e), TypeExpr::Named(x)) => e == x,
            (TypeExpr::Named(e), TypeExpr::Generic { name, .. }) => e == name,
            (TypeExpr::Generic { name, .. }, TypeExpr::Named(x)) => name == x,
            (
                TypeExpr::Generic { name: en, args: ea },
                TypeExpr::Generic { name: xn, args: xa },
            ) => {
                en == xn
                    && ea.len() == xa.len()
                    && ea.iter().zip(xa.iter()).all(|(e, x)| e.matches_declared(&x.display()))
            }
        }
    }

    /// تحليل نص نوع مخزّن (العرض القانوني) إلى بنية.
    /// لا شيء خارج العرض القانوني: فاصلة عربية أو
    /// لاتينية، وفراغات تُتخطى، وتداخل مسموح.
    pub fn parse(text: &str) -> Option<TypeExpr> {
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0usize;
        let t = Self::parse_at(&chars, &mut pos)?;
        Self::skip_ws(&chars, &mut pos);
        if pos != chars.len() {
            None
        } else {
            Some(t)
        }
    }

    fn skip_ws(chars: &[char], pos: &mut usize) {
        while *pos < chars.len() && (chars[*pos] == ' ' || chars[*pos] == '\t') {
            *pos += 1;
        }
    }

    fn parse_at(chars: &[char], pos: &mut usize) -> Option<TypeExpr> {
        Self::skip_ws(chars, pos);
        let mut name = String::new();
        while *pos < chars.len() {
            let c = chars[*pos];
            if c == '[' || c == ']' || c == '،' || c == ',' || c == ' ' || c == '\t' {
                break;
            }
            name.push(c);
            *pos += 1;
        }
        if name.is_empty() {
            return None;
        }
        Self::skip_ws(chars, pos);
        if *pos < chars.len() && chars[*pos] == '[' {
            *pos += 1;
            let mut args = Vec::new();
            loop {
                args.push(Self::parse_at(chars, pos)?);
                Self::skip_ws(chars, pos);
                if *pos < chars.len() && (chars[*pos] == '،' || chars[*pos] == ',') {
                    *pos += 1;
                    continue;
                }
                break;
            }
            Self::skip_ws(chars, pos);
            if *pos >= chars.len() || chars[*pos] != ']' {
                return None;
            }
            *pos += 1;
            Some(TypeExpr::Generic { name, args })
        } else {
            Some(TypeExpr::Named(name))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    // annotation: Some(type) only for a Var target, e.g. س: عدد = ٥.
    // None = untyped (gradual: أي).
    Assign {
        target: AssignTarget,
        value: Expr,
        annotation: Option<String>,
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // `+=` (إلحاق) مقابل `=` (استبدال).
        append: bool,
    },
    Print { args: Vec<Expr> },
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    While { cond: Expr, body: Vec<Stmt> },
    For { var: String, iterable: Expr, body: Vec<Stmt> },
    RepeatCount { count: Expr, body: Vec<Stmt> },
    RepeatWhile { cond: Expr, body: Vec<Stmt> },
    // param_types[i]: Some(type) for params[i], None = untyped.
    // return_type: Some(type), None = untyped (أي).
    // البنود 109–112 (الاجتماع 83 §98): `defaults[i]`
    // قيمة افتراضية لمعامل params[i] (تُقيَّم وقت التعريف)
    // أو لا شيء؛ و`vararg` اسم جامع الزائد الموضعي
    // (`*اسم`) أو لا شيء؛ و`kwarg` اسم جامع الزائد
    // المفتاحي (`**اسم`) أو لا شيء. الافتراضيات متتالية
    // من النهاية (يحسمها المحلل).
    FuncDef {
        name: String,
        params: Vec<String>,
        param_types: Vec<Option<String>>,
        return_type: Option<String>,
        defaults: Vec<Option<Expr>>,
        vararg: Option<String>,
        kwarg: Option<String>,
        // البنود 145–147 (العموميات — الاجتماع 83 §98):
        // معاملات النوع المعلنة `[ت: حد = افتراضي، ...]`؛
        // فارغة = دالة غير عامة كما كانت.
        type_params: Vec<TypeParam>,
        body: Vec<Stmt>,
    },
    Return { value: Option<Expr> },
    Break,
    Continue,
    Pass,
    // البند 98 (`والا` بعد حاول — الاجتماع 83 §98):
    // `else_body` كتلة تنفذ عند نجاح المحاولة بلا خطأ،
    // قبل `finally`؛ وخطأ داخلها يمسكه `ماعدا` نفسه
    // (موثق — كالمسار الناجح في المولد الأصلي).
    Try {
        try_body: Vec<Stmt>,
        except_body: Option<Vec<Stmt>>,
        else_body: Option<Vec<Stmt>>,
        finally_body: Option<Vec<Stmt>>,
    },
    // البند 77 (`عام` — الاجتماع 83 §98): إعلان أسماء
    // تُكتَب في نطاق البرنامج الجاري وحده (لا نطاق وحدة
    // أخرى)؛ وبلا إعلان يكتب الإسناد لأقرب ربط كما كان.
    GlobalDecl { names: Vec<String> },
    // البند 78 (`نطاق` — الاجتماع 83 §98): إعلان أسماء
    // تُكتَب في أقرب نطاق محيط يربطها (لا المحلي ولا
    // العام)؛ وغياب أي ربط محيط خطأ عربي صريح.
    NonlocalDecl { names: Vec<String> },
    // البند 104 (`نوع` — الاجتماع 83 §98): اسم مستعار
    // لنوع — `target` العرض النصي الموسَّع للنوع؛
    // يُوسَّع في المحلل قبل أن يراه أي محرك، فالجملة
    // نفسها بلا أثر زمن-تشغيل في المحركات الثلاثة.
    TypeAlias { name: String, target: String },
    ExprStmt { expr: Expr },
    // Phase 1A Step 2: modules.
    // Import forms:
    //   استورد وحدة                  -> module, alias=None, names=None
    //   استورد وحدة باسم مختصر      -> module, alias=Some, names=None
    //   من وحدة استورد اسم1، اسم2  -> module, alias=None, names=Some
    Import {
        module: String,
        alias: Option<String>,
        names: Option<Vec<String>>,
    },
    // صدّر اسم1، اسم2 : declares names public for this module file.
    // Only exported names are accessible via import (strict rule).
    Export { names: Vec<String> },
    // البند 8 — الزخارف (الاجتماع 72 §87): `@اسم` في بداية
    // سطر قبل تعريف دالة أو فئة فقط. الزخرفة دالة تُستدعى
    // بالدالة/الفئة المعرفة وقت التعريف، ونتيجتها تحل
    // محلها (على طريقة بايثون). زخارف متعددة متتالية
    // تُطبَّق من الأسفل إلى الأعلى. `@` في أي موضع آخر
    // ليس زخرفة: داخل تعبير خطأ نحوي، وقبل غير تعريف
    // خطأ نحوي، وداخل نص حرفي حرف عادي.
    Decorated { decorators: Vec<Expr>, def: Box<Stmt> },
    // Phase 1A Step 4: simple classes, extended in Step 5 with
    // inheritance and interfaces. فئة اسم [ورث أب] [واجهة و1، و2]:
    // + indented block of دالة methods only.
    // Methods are FuncDef statements; نفس is auto-bound to the
    // instance inside a method body, and is NOT a method parameter.
    // A method named أنشئ, if present, is the constructor, called
    // automatically by أنشئ اسم(معاملات).
    ClassDef {
        name: String,
        base: Option<String>,
        interfaces: Vec<String>,
        // البنود 145–147 (العموميات — الاجتماع 83 §98):
        // معاملات النوع المعلنة `[ت: حد = افتراضي، ...]`؛
        // فارغة = فئة غير عامة كما كانت.
        type_params: Vec<TypeParam>,
        methods: Vec<Stmt>,
    },
    // Phase 1A Step 5: interfaces. واجهة اسم: + indented block of
    // دالة signatures only, each body must be مرر (no implementation).
    // Each signature is stored as (method name, param count).
    InterfaceDef {
        name: String,
        methods: Vec<(String, usize)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
    pub col: usize,
}

impl Stmt {
    pub fn new(kind: StmtKind, line: usize, col: usize) -> Self {
        Self { kind, line, col }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_expr_parse_display_roundtrip() {
        // البند 8: النوع العام بنية لا نصا، والعرض
        // القانوني يُحلَّل من جديد إلى البنية نفسها.
        let t = TypeExpr::parse("قائمة[عدد]").unwrap();
        assert_eq!(t, TypeExpr::Generic {
            name: "قائمة".to_string(),
            args: vec![TypeExpr::Named("عدد".to_string())],
        });
        assert_eq!(t.display(), "قائمة[عدد]");
        let nested = TypeExpr::parse("قائمة[قائمة[عدد]]").unwrap();
        assert_eq!(nested.display(), "قائمة[قائمة[عدد]]");
        let dict = TypeExpr::parse("قاموس[نص، عدد]").unwrap();
        assert_eq!(dict.display(), "قاموس[نص، عدد]");
        assert!(TypeExpr::parse("قائمة[").is_none());
        assert!(TypeExpr::parse("").is_none());
        assert!(TypeExpr::parse("عدد[عدد]").is_some());
    }

    #[test]
    fn type_expr_matches_declared_structural() {
        // أساس عار يطابق العام؛ وعام يطابق عاما مثله
        // بمعاملاته؛ وعامان بمعاملين مختلفين لا يتطابقان.
        let generic = TypeExpr::parse("قائمة[عدد]").unwrap();
        assert!(generic.matches_declared("قائمة"));
        assert!(generic.matches_declared("قائمة[عدد]"));
        assert!(!generic.matches_declared("قائمة[نص]"));
        assert!(!generic.matches_declared("نص"));
        let named = TypeExpr::parse("عدد").unwrap();
        assert!(named.matches_declared("عدد"));
        assert!(!named.matches_declared("نص"));
    }
}
