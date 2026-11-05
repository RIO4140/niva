//! Values and the type system.
//! Rio is dynamically typed: a variable may hold any type and may be
//! reassigned to another type, but every operation checks types at runtime
//! and fails with an Arabic type error instead of silently coercing.
//! Types: عدد (Number), نص (String), منطقي (Bool), قائمة (List),
//! قاموس (Dict), لاشيء (Nil), دالة (Function).

use crate::ast::Stmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    List(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Nil,
    Function { name: String, params: Vec<String>, body: Vec<Stmt> },
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
