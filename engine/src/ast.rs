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
