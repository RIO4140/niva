//! Values and the type system.
//! Niva is dynamically typed: a variable may hold any type and may be
//! reassigned to another type, but every operation checks types at runtime
//! and fails with an Arabic type error instead of silently coercing.
//! Types: عدد (Number), نص (String), منطقي (Bool), قائمة (List),
//! قاموس (Dict), لاشيء (Nil), دالة (Function).

use crate::ast::Stmt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Phase 1A Step 4: classes. Class and instance data live behind Rc
// so cloning an instance value shares its fields: a method that
// writes نفس.حقل = قيمة mutates the same fields the caller sees,
// without any write-back bookkeeping in the interpreter.
// ClassData methods map method name -> Function value.
#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: String,
    pub methods: HashMap<String, Value>,
    // Phase 1A Step 5: inheritance + interfaces.
    // base: parent class, if any (ورث). interfaces: names of
    // interfaces this class declares it implements (واجهة).
    // Method lookup walks base chain; interfaces are checked
    // at class definition time, not stored as methods.
    pub base: Option<Rc<ClassData>>,
    pub interfaces: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InterfaceData {
    pub name: String,
    // (method name, param count). v1 checks existence + arity only,
    // not param types or return types.
    pub methods: Vec<(String, usize)>,
}

impl ClassData {
    // Method lookup: own methods first, then base chain recursively.
    // Overriding works because own methods win.
    pub fn lookup_method(&self, name: &str) -> Option<Value> {
        if let Some(m) = self.methods.get(name) {
            return Some(m.clone());
        }
        if let Some(base) = &self.base {
            return base.lookup_method(name);
        }
        None
    }
}

#[derive(Debug)]
pub struct InstanceData {
    pub class: Rc<ClassData>,
    pub fields: RefCell<HashMap<String, Value>>,
}
// Value::Instance holds Rc<InstanceData>, so cloning a Value clones
// the Rc pointer and shares fields; InstanceData itself is not Clone.

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    List(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Nil,
    // module: Some(module_name) for a function defined at the top
    // level of a module, so calls to it can push that module's scope.
    // None for functions defined in the main program (and, in Step 2,
    // for nested functions defined inside a module function, which
    // rely on the outer call's module scope already on the stack).
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        module: Option<String>,
        // Phase 1A Step 3: gradual types, None = untyped (أي).
        param_types: Vec<Option<String>>,
        return_type: Option<String>,
        // Step 1 (Meeting 10/11): lexical closures. The shared
        // scope chain at definition time, as Rc pointers, so a
        // returned function still sees (and mutates) its defining
        // environment. None for module functions (module: Some),
        // which keep the Step 2 module-scope push instead, and
        // for synthetic VM function carriers.
        captured: Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
    },
    // Module: exports are the only names accessible from outside;
    // scope is the full top-level scope after module execution,
    // including private names, used for module function calls.
    Module {
        name: String,
        exports: HashMap<String, Value>,
        scope: HashMap<String, Value>,
    },
    // Class definition value: فئة اسم.
    Class(Rc<ClassData>),
    // Interface definition value: واجهة اسم.
    Interface(Rc<InterfaceData>),
    // Object instance: كائن من فئة. Rc-shared fields, see above.
    Instance(Rc<InstanceData>),
    // A method bound to an instance, produced by ك.طريقة evaluation.
    // Calling it binds نفس to the instance automatically.
    BoundMethod { func: Box<Value>, instance: Rc<InstanceData> },
    // A stdlib function implemented in Rust (Meeting 16). Its code
    // lives in the stdlib module; the value only carries the name
    // and the function pointer. type_name is "دالة", like a user
    // function, and the Call arm invokes the pointer directly.
    NativeFunction {
        name: String,
        func: crate::stdlib::NativeFn,
    },
}

// Captured environments compare by scope identity (same Rc
// pointers in the same order), not by deep value comparison:
// two closures from the same factory call share their scopes,
// two from different calls do not.
fn captured_eq(
    a: &Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
    b: &Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.len() == y.len()
                && x.iter().zip(y.iter()).all(|(p, q)| Rc::ptr_eq(p, q))
        }
        _ => false,
    }
}

// Manual PartialEq: Rc values compare by identity, not by deep
// field comparison, so == on classes/instances means same object.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Dict(a), Value::Dict(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (
                Value::Function { name: n1, params: p1, body: b1, module: m1, param_types: pt1, return_type: r1, captured: c1 },
                Value::Function { name: n2, params: p2, body: b2, module: m2, param_types: pt2, return_type: r2, captured: c2 },
            ) => n1 == n2 && p1 == p2 && b1 == b2 && m1 == m2 && pt1 == pt2 && r1 == r2
                && captured_eq(c1, c2),
            (
                Value::Module { name: n1, exports: e1, scope: s1 },
                Value::Module { name: n2, exports: e2, scope: s2 },
            ) => n1 == n2 && e1 == e2 && s1 == s2,
            (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
            (Value::Interface(a), Value::Interface(b)) => Rc::ptr_eq(a, b),
            (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
            (
                Value::BoundMethod { func: f1, instance: i1 },
                Value::BoundMethod { func: f2, instance: i2 },
            ) => f1 == f2 && Rc::ptr_eq(i1, i2),
            (
                Value::NativeFunction { name: n1, func: f1 },
                Value::NativeFunction { name: n2, func: f2 },
            ) => n1 == n2 && f1 == f2,
            _ => false,
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "عدد",
            Value::Str(_) => "نص",
            Value::Bool(_) => "منطقي",
            Value::List(_) => "قائمة",
            Value::Dict(_) => "قاموس",
            Value::Nil => "لاشيء",
            Value::Function { .. } => "دالة",
            Value::Module { .. } => "وحدة",
            Value::Class(_) => "فئة",
            Value::Interface(_) => "واجهة",
            Value::Instance(_) => "كائن",
            Value::BoundMethod { .. } => "دالة",
            Value::NativeFunction { .. } => "دالة",
        }
    }

    fn fmt_number(n: f64) -> String {
        if n.fract() == 0.0 && n.abs() < 9.0e15 {
            format!("{}", n as i64)
        } else {
            format!("{}", n)
        }
    }

    /// Display for `اطبع`: strings print raw, everything else prints
    /// in a Python-like form (lists/dicts with quoted string elements).
    pub fn fmt_value(&self) -> String {
        match self {
            Value::Number(n) => Self::fmt_number(*n),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => {
                if *b {
                    "صحيح".to_string()
                } else {
                    "خطأ".to_string()
                }
            }
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.fmt_in_collection()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Dict(pairs) => {
                let inner: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("\"{}\": {}", escape_str(k), v.fmt_in_collection()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Nil => "لاشيء".to_string(),
            Value::Function { name, .. } => format!("<دالة {}>", name),
            Value::Module { name, .. } => format!("<وحدة {}>", name),
            Value::Class(data) => format!("<فئة {}>", data.name),
            Value::Interface(data) => format!("<واجهة {}>", data.name),
            Value::Instance(data) => format!("<كائن {}>", data.class.name),
            Value::BoundMethod { func, .. } => match &**func {
                Value::Function { name, .. } => format!("<دالة {}>", name),
                _ => "<دالة>".to_string(),
            },
            Value::NativeFunction { name, .. } => format!("<دالة {}>", name),
        }
    }

    fn fmt_in_collection(&self) -> String {
        match self {
            Value::Str(s) => format!("\"{}\"", escape_str(s)),
            other => other.fmt_value(),
        }
    }
}

fn escape_str(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}
