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
    // Phase 1A Step 2: module.member (and later object.member).
    // In Step 2, Dot is only valid on a Module value; on any other
    // value it is a runtime type error.
    Dot { target: Box<Expr>, name: String },
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
    "عدد", "نص", "منطقي", "قائمة", "قاموس", "لاشيء", "دالة", "وحدة", "أي",
];

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
