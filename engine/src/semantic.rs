//! Semantic analyzer — Phase 1A Step 1.
//!
//! Runs AFTER parsing and BEFORE execution. It catches, with an Arabic
//! error carrying line + column, the mistakes that used to surface only
//! at runtime, after part of the program had already run:
//! - reading a variable that was never defined in any visible scope
//! - `ارجع` outside a function
//! - `اكسر` / `تابع` outside a loop (a loop inside a function does
//!   NOT count for a break inside a nested function, and vice versa)
//! - calling a known user function or builtin with the wrong number
//!   of arguments
//!
//! What it does NOT do yet (later Phase 1A steps, not this one):
//! - type checking (gradual types are Step 2, with annotation syntax)
//! - modules / imports (Step 3, with `استورد` / `صدّر`)
//! - dot access, classes, interfaces (Phase 1A spec in v1.3, not v1)
//!
//! Scope rules mirror the interpreter exactly, so the analyzer never
//! rejects a program the interpreter would accept, and never accepts
//! one the interpreter would reject for scope reasons:
//! - a function pushes a NEW scope; parameters live in it
//! - `إذا` / `بينما` / `لكل` / `كرر` / `حاول` do NOT push a scope:
//!   variables defined inside them stay visible after them, and a
//!   loop variable stays visible after the loop
//! - a function name is added to its enclosing scope BEFORE its body
//!   is analyzed, so recursion passes, but a call to a function
//!   defined LATER in the same scope fails, exactly as it would at
//!   runtime (the interpreter defines functions top-down)
//! - assignment defines a variable in the innermost scope, never in
//!   an outer scope, matching `define_local` in the interpreter

use crate::ast::{AssignTarget, Expr, ExprKind, Stmt, StmtKind};
use crate::error::{RioError, RioResult};
use std::collections::{HashMap, HashSet};

/// One scope: name -> Some(param_count) if the name is a known user
/// function in this scope, None if it is an ordinary variable.
type Scope = HashMap<String, Option<usize>>;

pub struct SemanticAnalyzer {
    scopes: Vec<Scope>,
    function_depth: usize,
    loop_depth: usize,
}

fn builtin_arity(name: &str) -> Option<(usize, usize)> {
    // (min, max) arguments, inclusive. None = not a builtin.
    match name {
        "طول" | "مجموع" | "أدنى" | "أقصى" | "مطلق" | "عدد" | "نص" | "نوع"
        | "اقرأ_ملف" => Some((1, 1)),
        "مدى" => Some((1, 3)),
        "اكتب_ملف" => Some((2, 2)),
        "ادخل" => Some((0, 1)),
        _ => None,
    }
}

fn is_builtin(name: &str) -> bool {
    builtin_arity(name).is_some()
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            function_depth: 0,
            loop_depth: 0,
        }
    }

    pub fn analyze_program(&mut self, program: &[Stmt]) -> RioResult<()> {
        for stmt in program {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn define(&mut self, name: &str, func_arity: Option<usize>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), func_arity);
        }
    }

    fn lookup(&self, name: &str) -> Option<Option<usize>> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(*v);
            }
        }
        None
    }

    fn is_defined(&self, name: &str) -> bool {
        self.lookup(name).is_some() || is_builtin(name)
    }

    fn analyze_block(&mut self, body: &[Stmt]) -> RioResult<()> {
        for stmt in body {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            StmtKind::Assign { target, value } => {
                // Value first, matching runtime evaluation order.
                self.analyze_expr(value)?;
                match target {
                    AssignTarget::Var(name) => {
                        self.define(name, None);
                    }
                    AssignTarget::Index { target: t, index: i } => {
                        // Narrowed v1.3 Target: only Ident[Expr] is
                        // valid here; anything else is a semantic error
                        // (the interpreter also rejects it at runtime).
                        match &t.kind {
                            ExprKind::Ident(name) => {
                                if !self.is_defined(name) {
                                    return Err(RioError::semantic(
                                        format!("متغير غير معرف: {}", name),
                                        t.line,
                                        t.col,
                                    ));
                                }
                                self.analyze_expr(i)?;
                            }
                            _ => {
                                return Err(RioError::semantic(
                                    "هدف التعيين غير صالح",
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }
            StmtKind::Print { args } => {
                for a in args {
                    self.analyze_expr(a)?;
                }
                Ok(())
            }
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                self.analyze_expr(cond)?;
                self.analyze_block(then_body)?;
                for (c, b) in elifs {
                    self.analyze_expr(c)?;
                    self.analyze_block(b)?;
                }
                if let Some(b) = else_body {
                    self.analyze_block(b)?;
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                self.analyze_expr(cond)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::For {
                var,
                iterable,
                body,
            } => {
                self.analyze_expr(iterable)?;
                // Loop variable stays in the CURRENT scope, not a new
                // one, and stays visible after the loop.
                self.define(var, None);
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::RepeatCount { count, body } => {
                self.analyze_expr(count)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::RepeatWhile { cond, body } => {
                self.analyze_expr(cond)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::FuncDef { name, params, body } => {
                // Duplicate parameters are a definition error.
                let mut seen = HashSet::new();
                for p in params {
                    if !seen.insert(p) {
                        return Err(RioError::semantic(
                            format!("معامل مكرر في الدالة {}: {}", name, p),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                // Define the function BEFORE its body, so recursion
                // passes, and so later calls in the enclosing scope
                // pass. Calls before this definition fail, as at runtime.
                self.define(name, Some(params.len()));
                self.scopes.push(HashMap::new());
                for p in params {
                    self.define(p, None);
                }
                self.function_depth += 1;
                let saved_loop = self.loop_depth;
                self.loop_depth = 0;
                let r = self.analyze_block(body);
                self.loop_depth = saved_loop;
                self.function_depth -= 1;
                self.scopes.pop();
                r
            }
            StmtKind::Return { value } => {
                if self.function_depth == 0 {
                    return Err(RioError::semantic(
                        "ارجع خارج دالة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                if let Some(v) = value {
                    self.analyze_expr(v)?;
                }
                Ok(())
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    return Err(RioError::semantic(
                        "اكسر خارج حلقة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(())
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    return Err(RioError::semantic(
                        "تابع خارج حلقة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::Try {
                try_body,
                except_body,
                finally_body,
            } => {
                self.analyze_block(try_body)?;
                if let Some(b) = except_body {
                    self.analyze_block(b)?;
                }
                if let Some(b) = finally_body {
                    self.analyze_block(b)?;
                }
                Ok(())
            }
            StmtKind::ExprStmt { expr } => self.analyze_expr(expr),
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) -> RioResult<()> {
        match &expr.kind {
            ExprKind::Number(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Nil => Ok(()),
            ExprKind::Ident(name) => {
                if self.is_defined(name) {
                    Ok(())
                } else {
                    Err(RioError::semantic(
                        format!("متغير غير معرف: {}", name),
                        expr.line,
                        expr.col,
                    ))
                }
            }
            ExprKind::List(items) => {
                for it in items {
                    self.analyze_expr(it)?;
                }
                Ok(())
            }
            ExprKind::Dict(pairs) => {
                for (_, v) in pairs {
                    self.analyze_expr(v)?;
                }
                Ok(())
            }
            ExprKind::Binary { left, op, right } => {
                // `و` / `أو` short-circuit at runtime, but both sides
                // are still in scope, so both are analyzed.
                let _ = op;
                self.analyze_expr(left)?;
                self.analyze_expr(right)?;
                Ok(())
            }
            ExprKind::Unary { op: _, expr: e } => self.analyze_expr(e),
            ExprKind::Call { callee, args } => {
                // Arity check for direct calls to known functions.
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(Some(n)) = self.lookup(name) {
                        if args.len() != n {
                            return Err(RioError::semantic(
                                format!(
                                    "عدد المعاملات غير صحيح: الدالة {} تتوقع {} لكن أعطيت {}",
                                    name,
                                    n,
                                    args.len()
                                ),
                                expr.line,
                                expr.col,
                            ));
                        }
                    } else if let Some((min, max)) = builtin_arity(name) {
                        if args.len() < min || args.len() > max {
                            return Err(RioError::semantic(
                                format!(
                                    "عدد المعاملات غير صحيح: الدالة {} تتوقع {} إلى {} لكن أعطيت {}",
                                    name, min, max, args.len()
                                ),
                                expr.line,
                                expr.col,
                            ));
                        }
                    } else if !self.is_defined(name) {
                        return Err(RioError::semantic(
                            format!("متغير غير معرف: {}", name),
                            callee.line,
                            callee.col,
                        ));
                    }
                    // If the callee is a variable holding a function
                    // (alias), arity is unknown at analysis time and
                    // is checked at runtime, not here.
                } else {
                    self.analyze_expr(callee)?;
                }
                for a in args {
                    self.analyze_expr(a)?;
                }
                Ok(())
            }
            ExprKind::Index { target, index } => {
                self.analyze_expr(target)?;
                self.analyze_expr(index)?;
                Ok(())
            }
        }
    }
}
