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
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Index { target: Box<Expr>, index: Box<Expr> },
    // Phase 1A Step 2: module.member. Step 4: object.member too.
    // Dot on any other value is a runtime type error.
    Dot { target: Box<Expr>, name: String },
    // Phase 1A Step 4: classes. أنشئ فئة(معاملات)
    Create { class: String, args: Vec<Expr> },
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

pub fn is_known_type(name: &str) -> bool {
    TYPE_NAMES.contains(&name)
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
    FuncDef {
        name: String,
        params: Vec<String>,
        param_types: Vec<Option<String>>,
        return_type: Option<String>,
        body: Vec<Stmt>,
    },
    Return { value: Option<Expr> },
    Break,
    Continue,
    Pass,
    Try {
        try_body: Vec<Stmt>,
        except_body: Option<Vec<Stmt>>,
        finally_body: Option<Vec<Stmt>>,
    },
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
