//! Tree-walking interpreter.
//!
//! Scope rules (documented in DESIGN.md, tested in tests/integration.rs):
//! - The global scope is the bottom scope.
//! - A function call pushes a NEW scope. Parameters and assignments inside
//!   a function live in that scope only and disappear when it returns.
//! - Reading a variable searches from the innermost scope outward to global.
//! - Assignment inside a function creates/updates a LOCAL variable, never
//!   a global scalar of the same name. (Index assignment to an existing
//!   list/dict mutates that object itself, wherever it lives.)
//! - `إذا` / `بينما` / `لكل` / `كرر` do NOT create a scope: variables
//!   assigned inside them stay visible after the block in the same scope.
//!   In particular a loop variable is still visible after the loop.
//!
//! Type rules: dynamic typing, runtime checks, no silent coercion
//! (except the explicit عدد() / نص() conversions). Conditions must be
//! منطقي (Bool). See DESIGN.md for the full table.

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{ErrorKind, RioError, RioResult};
use crate::value::Value;
use std::collections::HashMap;
use std::io::Write;

const MAX_LOOP_ITERATIONS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Normal,
    Return,
    Break,
    Continue,
}

// We carry a return value alongside Flow::Return via a separate field on
// the interpreter instead of stuffing it into Flow, to keep Flow Copy.
pub struct Interpreter {
    scopes: Vec<HashMap<String, Value>>,
    pub output: String,
    return_value: Option<Value>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            output: String::new(),
            return_value: None,
        }
    }

    fn define_local(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn lookup_scope_index(&self, name: &str) -> Option<usize> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return Some(i);
            }
        }
        None
    }

    pub fn run(&mut self, program: &[Stmt]) -> RioResult<()> {
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

    fn exec_stmt(&mut self, stmt: &Stmt) -> RioResult<Flow> {
        match &stmt.kind {
            StmtKind::Assign { target, value } => {
                let v = self.eval_expr(value)?;
                match target {
                    AssignTarget::Var(name) => {
                        self.define_local(name, v);
                    }
                    AssignTarget::Index { target: t, index: i } => {
                        self.assign_index(t, i, v, stmt.line, stmt.col)?;
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Print { args } => {
                let mut parts = Vec::new();
                for arg in args {
                    parts.push(self.eval_expr(arg)?.fmt_value());
                }
                self.output.push_str(&parts.join(" "));
                self.output.push('\n');
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
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::FuncDef { name, params, body } => {
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                self.define_local(name, func);
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
                let _ = self.eval_expr(expr)?;
                Ok(Flow::Normal)
            }
        }
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
        let slot = self.scopes[scope_idx].get_mut(&name).unwrap();
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
                            other => Err(RioError::typ(
                                format!("القيمة {} ليست دالة", other.type_name()),
                                callee.line,
                                callee.col,
                            )),
                        };
                    }
                    return self.call_builtin(name, arg_values, expr.line, expr.col);
                }
                let callee_value = self.eval_expr(callee)?;
                match callee_value {
                    Value::Function { .. } => {
                        self.call_user(callee_value, arg_values, expr.line, expr.col)
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
            BinOp::Add => match (&l, &r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::List(a), Value::List(b)) => {
                    let mut out = a.clone();
                    out.extend(b.iter().cloned());
                    Ok(Value::List(out))
                }
                _ => Err(RioError::typ(
                    format!("لا يمكن جمع {} و {}", l.type_name(), r.type_name()),
                    line,
                    col,
                )),
            },
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
        let (name, params, body) = match func {
            Value::Function { name, params, body } => (name, params, body),
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
        let mut scope = HashMap::new();
        for (p, v) in params.iter().zip(args.into_iter()) {
            scope.insert(p.clone(), v);
        }
        self.scopes.push(scope);
        let result = self.exec_block(&body);
        self.scopes.pop();
        match result? {
            Flow::Return => Ok(self.return_value.take().unwrap_or(Value::Nil)),
            Flow::Normal => Ok(Value::Nil),
            Flow::Break => Err(RioError::runtime("اكسر خارج حلقة", line, col)),
            Flow::Continue => Err(RioError::runtime("تابع خارج حلقة", line, col)),
        }
    }

    fn call_builtin(
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
                    Value::Str(path) => std::fs::read_to_string(path).map(Value::Str).map_err(|e| {
                        RioError::new(ErrorKind::Io, format!("تعذر قراءة الملف {}: {}", path, e), line, col)
                    }),
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
                        std::fs::write(path, content).map(|_| Value::Nil).map_err(|e| {
                            RioError::new(
                                ErrorKind::Io,
                                format!("تعذر كتابة الملف {}: {}", path, e),
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
                let mut line_input = String::new();
                std::io::stdin()
                    .read_line(&mut line_input)
                    .map_err(|e| RioError::new(ErrorKind::Io, format!("تعذر قراءة الإدخال: {}", e), line, col))?;
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

    #[test]
    fn function_scope_is_local() {
        let (out, res) = run_src(
            "دالة جمع(أ، ب):\n    نتيجة = أ + ب\n    ارجع نتيجة\nاطبع جمع(٢، ٣)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "5\n");
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
        assert_eq!(out, "0\n1\n2\n2\n");
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
}
