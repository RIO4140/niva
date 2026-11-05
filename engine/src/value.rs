//! Values and the type system.
//! Rio is dynamically typed: a variable may hold any type and may be
//! reassigned to another type, but every operation checks types at runtime
//! and fails with an Arabic type error instead of silently coercing.
//! Types: عدد (Number), نص (String), منطقي (Bool), قائمة (List),
//! قاموس (Dict), لاشيء (Nil), دالة (Function).

use crate::ast::Stmt;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
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
    },
    // Module: exports are the only names accessible from outside;
    // scope is the full top-level scope after module execution,
    // including private names, used for module function calls.
    Module {
        name: String,
        exports: HashMap<String, Value>,
        scope: HashMap<String, Value>,
    },
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
