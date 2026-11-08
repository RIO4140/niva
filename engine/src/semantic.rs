//! Semantic analyzer — Phase 1A Steps 1-3.
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
//! - (Step 3, gradual types) assigning a known-typed value to a
//!   variable, parameter, or return declared with a different type.
//!   Unknown (untyped / أي) never errors statically; it is checked
//!   at runtime by the interpreter, so gradual code is not falsely
//!   rejected.
//!
//! What it does NOT do yet (later Phase 1A steps, not this one):
//! - classes, interfaces (Phase 1A spec in v1.3, not v1)
//!
//! Phase 1A Step 2 adds modules to this analyzer for the MAIN
//! program only: an Import defines its bound names, an Export is a
//! no-op, and a Dot access analyzes its target but does NOT check
//! the member name (member existence and export status are checked
//! at runtime, when the module file is actually loaded).
//! Module FILES themselves are not semantically analyzed in Step 2:
//! they are parsed and executed by the interpreter's loader, with
//! the module name added to any runtime error. A two-pass module
//! analysis (so a module function may reference a module global
//! defined later in the file) is a later step, not this one.
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

#[derive(Debug, Clone)]
struct VarInfo {
    // Some(n) if a known user function with n params, None otherwise.
    arity: Option<usize>,
    // Declared type for a variable (annotation), None = untyped.
    // For a module binding, Some("وحدة").
    declared_type: Option<String>,
    // For a known user function only.
    param_types: Vec<Option<String>>,
    return_type: Option<String>,
}

impl VarInfo {
    fn var() -> Self {
        Self { arity: None, declared_type: None, param_types: vec![], return_type: None }
    }
    fn var_typed(t: &str) -> Self {
        Self { arity: None, declared_type: Some(t.to_string()), param_types: vec![], return_type: None }
    }
    fn func(arity: usize, param_types: Vec<Option<String>>, return_type: Option<String>) -> Self {
        Self { arity: Some(arity), declared_type: None, param_types, return_type }
    }
}

/// One scope: name -> info about that name in this scope.
type Scope = HashMap<String, VarInfo>;

// Audit م-12: Clone so the REPL can trial-analyze a line on
// a copy (code-first check) without touching the real state.
#[derive(Debug, Clone)]
pub struct SemanticAnalyzer {
    scopes: Vec<Scope>,
    function_depth: usize,
    loop_depth: usize,
    // Stack of declared return types for enclosing functions.
    return_type_stack: Vec<Option<String>>,
}

fn builtin_arity(name: &str) -> Option<(usize, usize)> {
    // (min, max) arguments, inclusive. None = not a builtin.
    match name {
        "طول" | "مجموع" | "أدنى" | "أقصى" | "مطلق" | "عدد" | "نص" | "نوع"
        | "اقرأ_ملف" | "انتقل" | "أنشئ_مجلد" | "احذف" | "احذف_مجلد" | "احذف_ملف"
        | "أنشئ_ملف" | "عد_الأسطر" | "عد_الكلمات" | "ابحث_عن" | "حجم_المسار"
        | "موجود" | "نوع_المسار" | "تفاصيل_المسار" | "تثبيت" | "بيئة" => Some((1, 1)),
        "مدى" => Some((1, 3)),
        "اكتب_ملف" | "انسخ" | "انقل" | "أنشئ_رابط" | "ابحث_في_ملف" => Some((2, 2)),
        "ادخل" => Some((0, 1)),
        "اعرض_الملفات" | "المجلد_الحالي" | "التاريخ_الآن" | "النسخة" | "إصدار_المحرك" => {
            Some((0, 0))
        }
        "عد_الملفات" | "اعرض_الشجرة" => Some((0, 1)),
        "اعرض_البداية" | "اعرض_النهاية" => Some((1, 2)),
        _ => None,
    }
}

fn builtin_return_type(name: &str) -> Option<&'static str> {
    match name {
        "طول" | "مجموع" | "مطلق" | "عدد" | "عد_الأسطر" | "عد_الكلمات" | "حجم_المسار"
        | "عد_الملفات" => Some("عدد"),
        "نص" | "نوع" | "اقرأ_ملف" | "ادخل" | "المجلد_الحالي" | "اعرض_البداية"
        | "اعرض_النهاية" | "التاريخ_الآن" | "النسخة" | "إصدار_المحرك" | "نوع_المسار"
        | "تثبيت" | "بيئة" => Some("نص"),
        "مدى" | "اعرض_الملفات" | "ابحث_في_ملف" | "ابحث_عن" | "اعرض_الشجرة" => Some("قائمة"),
        "اكتب_ملف" | "انتقل" | "أنشئ_مجلد" | "احذف" | "احذف_مجلد" | "احذف_ملف"
        | "انسخ" | "انقل" | "أنشئ_ملف" | "أنشئ_رابط" => Some("لاشيء"),
        "موجود" => Some("منطقي"),
        "تفاصيل_المسار" => Some("قاموس"),
        // أدنى / أقصى depend on input type: unknown in Step 3.
        _ => None,
    }
}

fn is_builtin(name: &str) -> bool {
    builtin_arity(name).is_some()
}

/// Static type check helper: if both types are known and differ,
/// and neither is `أي`, return a semantic type-mismatch error.
/// Unknown on either side passes (gradual typing).
fn check_type_match(
    expected: &str,
    actual: Option<String>,
    what: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    if expected == "أي" {
        return Ok(());
    }
    if let Some(a) = actual {
        if a != "أي" && a != expected {
            return Err(RioError::semantic(
                format!("نوع غير متطابق: {} معلن {}، لكن القيمة {}", what, expected, a),
                line,
                col,
            ));
        }
    }
    Ok(())
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            function_depth: 0,
            loop_depth: 0,
            return_type_stack: vec![],
        }
    }

    pub fn analyze_program(&mut self, program: &[Stmt]) -> RioResult<()> {
        for stmt in program {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn define(&mut self, name: &str, info: VarInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), info);
        }
    }

    fn lookup(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn is_defined(&self, name: &str) -> bool {
        self.lookup(name).is_some() || is_builtin(name)
    }

    /// Static type of an expression, if known. None = unknown (أي).
    /// Only declared types, literals, and known function/builtin
    /// returns are known; everything else (index, dot, binary with
    /// unknown operands, untyped variables) is unknown and passes.
    fn infer_type(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Number(_) => Some("عدد".to_string()),
            ExprKind::Str(_) => Some("نص".to_string()),
            ExprKind::Bool(_) => Some("منطقي".to_string()),
            ExprKind::Nil => Some("لاشيء".to_string()),
            ExprKind::List(_) => Some("قائمة".to_string()),
            ExprKind::Dict(_) => Some("قاموس".to_string()),
            ExprKind::Ident(name) => {
                if let Some(info) = self.lookup(name) {
                    if let Some(t) = info.declared_type {
                        return Some(t);
                    }
                    if info.arity.is_some() {
                        return Some("دالة".to_string());
                    }
                    return None;
                }
                // Builtin name used as a value (not a call): a function.
                if is_builtin(name) {
                    return Some("دالة".to_string());
                }
                None
            }
            ExprKind::Call { callee, .. } => {
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(info) = self.lookup(name) {
                        if let Some(rt) = info.return_type {
                            return Some(rt);
                        }
                        if info.arity.is_some() {
                            return None;
                        }
                    }
                    if let Some(rt) = builtin_return_type(name) {
                        return Some(rt.to_string());
                    }
                }
                None
            }
            ExprKind::Binary { left, op, right } => {
                use crate::ast::BinOp;
                match op {
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
                    | BinOp::And | BinOp::Or | BinOp::In | BinOp::NotIn => {
                        Some("منطقي".to_string())
                    }
                    BinOp::Add => {
                        let lt = self.infer_type(left)?;
                        let rt = self.infer_type(right)?;
                        if lt == rt && (lt == "عدد" || lt == "نص" || lt == "قائمة") {
                            Some(lt)
                        } else {
                            None
                        }
                    }
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        let lt = self.infer_type(left)?;
                        let rt = self.infer_type(right)?;
                        if lt == "عدد" && rt == "عدد" {
                            Some("عدد".to_string())
                        } else {
                            None
                        }
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                use crate::ast::UnOp;
                match op {
                    UnOp::Neg => {
                        if self.infer_type(inner)? == "عدد" {
                            Some("عدد".to_string())
                        } else {
                            None
                        }
                    }
                    UnOp::Not => {
                        if self.infer_type(inner)? == "منطقي" {
                            Some("منطقي".to_string())
                        } else {
                            None
                        }
                    }
                }
            }
            ExprKind::Index { .. } | ExprKind::Dot { .. } => None,
            ExprKind::Create { .. } => Some("كائن".to_string()),
            ExprKind::Lambda { .. } => Some("دالة".to_string()),
        }
    }

    fn analyze_block(&mut self, body: &[Stmt]) -> RioResult<()> {
        for stmt in body {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation } => {
                // Value first, matching runtime evaluation order.
                self.analyze_expr(value)?;
                let value_type = self.infer_type(value);
                match target {
                    AssignTarget::Var(name) => {
                        // Step 1 (Meeting 11): lexical assignment,
                        // matching the interpreter's assign_local.
                        // An assignment targets the nearest existing
                        // binding (any enclosing scope), not a fresh
                        // local; only a name that exists nowhere is
                        // defined in the current scope. Declared-type
                        // checks use the found binding's type.
                        let existing_decl = self
                            .lookup(name)
                            .and_then(|info| info.declared_type.clone());
                        if let Some(decl) = &existing_decl {
                            check_type_match(
                                decl,
                                value_type.clone(),
                                &format!("المتغير {}", name),
                                stmt.line,
                                stmt.col,
                            )?;
                        }
                        if let Some(ann) = annotation {
                            check_type_match(
                                ann,
                                value_type,
                                &format!("المتغير {}", name),
                                stmt.line,
                                stmt.col,
                            )?;
                            // An annotation on an existing binding
                            // must agree with its declared type.
                            if let Some(decl) = &existing_decl {
                                if decl != ann {
                                    return Err(RioError::semantic(
                                        format!(
                                            "المتغير {} معلن {} في نطاق خارجي، ولا يمكن إعادة إعلانه {}",
                                            name, decl, ann
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                            // No existing binding: the annotation
                            // declares a new local.
                            if self.lookup(name).is_none() {
                                self.define(name, VarInfo::var_typed(ann));
                            }
                        } else if self.lookup(name).is_none() {
                            self.define(name, VarInfo::var());
                        }
                    }
                    AssignTarget::Dot { target: t, name: _ } => {
                        // Field assignment: target must be analyzable
                        // (a variable, نفس inside a method, or a dot
                        // chain). Field existence/type is runtime.
                        self.analyze_expr(t)?;
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
                self.define(var, VarInfo::var());
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
            StmtKind::FuncDef { name, params, param_types, return_type, body } => {
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
                self.define(
                    name,
                    VarInfo::func(params.len(), param_types.clone(), return_type.clone()),
                );
                self.scopes.push(HashMap::new());
                for (p, pt) in params.iter().zip(param_types.iter()) {
                    match pt {
                        Some(t) => self.define(p, VarInfo::var_typed(t)),
                        None => self.define(p, VarInfo::var()),
                    }
                }
                self.function_depth += 1;
                self.return_type_stack.push(return_type.clone());
                let saved_loop = self.loop_depth;
                self.loop_depth = 0;
                let r = self.analyze_block(body);
                self.loop_depth = saved_loop;
                self.return_type_stack.pop();
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
                    if let Some(Some(rt)) = self.return_type_stack.last() {
                        let vt = self.infer_type(v);
                        check_type_match(
                            rt,
                            vt,
                            &format!("قيمة ارجع"),
                            stmt.line,
                            stmt.col,
                        )?;
                    }
                } else {
                    // Return without a value: type لاشيء.
                    if let Some(Some(rt)) = self.return_type_stack.last() {
                        check_type_match(
                            rt,
                            Some("لاشيء".to_string()),
                            &format!("قيمة ارجع"),
                            stmt.line,
                            stmt.col,
                        )?;
                    }
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
            StmtKind::ClassDef { name, base, interfaces, methods } => {
                // Base and interfaces must already be defined
                // (top-down, like functions). Compliance itself
                // is checked at runtime, where method maps exist.
                if let Some(b) = base {
                    if !self.is_defined(b) {
                        return Err(RioError::semantic(
                            format!("فئة أب غير معرفة: {}", b),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                for iname in interfaces {
                    if !self.is_defined(iname) {
                        return Err(RioError::semantic(
                            format!("واجهة غير معرفة: {}", iname),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                // Class name is defined BEFORE its methods, so a
                // method body may create instances of its own class.
                // Its static type is فئة.
                self.define(name, VarInfo::var_typed("فئة"));
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, param_types, return_type, body } = &m.kind {
                        let mut seen = HashSet::new();
                        for p in params {
                            if !seen.insert(p) {
                                return Err(RioError::semantic(
                                    format!("معامل مكرر في الطريقة {}: {}", mn, p),
                                    m.line,
                                    m.col,
                                ));
                            }
                        }
                        // Method scope: params + نفس (auto-bound to
                        // the instance, type كائن). نفس is NOT a param.
                        self.scopes.push(HashMap::new());
                        for (p, pt) in params.iter().zip(param_types.iter()) {
                            match pt {
                                Some(t) => self.define(p, VarInfo::var_typed(t)),
                                None => self.define(p, VarInfo::var()),
                            }
                        }
                        self.define("نفس", VarInfo::var_typed("كائن"));
                        self.function_depth += 1;
                        self.return_type_stack.push(return_type.clone());
                        let saved_loop = self.loop_depth;
                        self.loop_depth = 0;
                        let r = self.analyze_block(body);
                        self.loop_depth = saved_loop;
                        self.return_type_stack.pop();
                        self.function_depth -= 1;
                        self.scopes.pop();
                        r?;
                    }
                }
                Ok(())
            }
            StmtKind::InterfaceDef { name, .. } => {
                // Interface name is defined, type واجهة.
                // Its methods have no body to analyze (مرر only).
                self.define(name, VarInfo::var_typed("واجهة"));
                Ok(())
            }
            StmtKind::Import { module, alias, names } => {
                match names {
                    Some(names) => {
                        // من وحدة استورد اسم1، اسم2 : each name is
                        // defined, with unknown arity and type (it may
                        // be a value, not a function; runtime checks).
                        for n in names {
                            self.define(n, VarInfo::var());
                        }
                    }
                    None => {
                        // استورد وحدة [باسم مختصر] : bind module value,
                        // whose type is وحدة.
                        let bind = alias.clone().unwrap_or_else(|| module.clone());
                        self.define(&bind, VarInfo::var_typed("وحدة"));
                    }
                }
                Ok(())
            }
            StmtKind::Export { .. } => Ok(()),
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
                let mut known_param_types: Option<Vec<Option<String>>> = None;
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(info) = self.lookup(name) {
                        if let Some(n) = info.arity {
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
                            known_param_types = Some(info.param_types.clone());
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
                // Phase 1A Step 3: param type check for known
                // functions, after args are analyzed. Unknown arg
                // types pass (gradual); runtime checks them.
                if let Some(pts) = known_param_types {
                    if let ExprKind::Ident(name) = &callee.kind {
                        for (arg, pt) in args.iter().zip(pts.iter()) {
                            if let Some(pt) = pt {
                                let at = self.infer_type(arg);
                                check_type_match(
                                    pt,
                                    at,
                                    &format!("معامل في الدالة {}", name),
                                    arg.line,
                                    arg.col,
                                )?;
                            }
                        }
                    }
                }
                Ok(())
            }
            ExprKind::Index { target, index } => {
                self.analyze_expr(target)?;
                self.analyze_expr(index)?;
                Ok(())
            }
            ExprKind::Create { class, args } => {
                // Class must be defined (top-down, like functions).
                // Its type is checked at runtime, so a variable
                // holding a class passes here (gradual).
                if !self.is_defined(class) {
                    return Err(RioError::semantic(
                        format!("فئة غير معرفة: {}", class),
                        expr.line,
                        expr.col,
                    ));
                }
                for a in args {
                    self.analyze_expr(a)?;
                }
                Ok(())
            }
            ExprKind::Dot { target, name } => {
                // Member existence / export status are runtime
                // checks (module file is loaded at runtime).
                self.analyze_expr(target)?;
                // Meeting 14: static check for string/list methods
                // when the target type is known (gradual: unknown
                // types pass, and are checked at runtime instead).
                if let Some(t) = self.infer_type(target) {
                    if t == "نص" && !crate::ast::STR_METHODS.contains(&name.as_str()) {
                        return Err(RioError::semantic(
                            format!("طريقة غير معروفة لنوع نص: {}", name),
                            expr.line,
                            expr.col,
                        ));
                    }
                    if t == "قائمة" && !crate::ast::LIST_METHODS.contains(&name.as_str()) {
                        return Err(RioError::semantic(
                            format!("طريقة غير معروفة لنوع قائمة: {}", name),
                            expr.line,
                            expr.col,
                        ));
                    }
                }
                Ok(())
            }
            ExprKind::Lambda { params, param_types, body } => {
                // Meeting 13: like a FuncDef body, but no name
                // and no declared return type. Duplicate params
                // are a definition error; the body sees the
                // enclosing scopes (lexical capture).
                let mut seen = HashSet::new();
                for p in params {
                    if !seen.insert(p) {
                        return Err(RioError::semantic(
                            format!("معامل مكرر في الدالة المجهولة: {}", p),
                            expr.line,
                            expr.col,
                        ));
                    }
                }
                self.scopes.push(HashMap::new());
                for (p, pt) in params.iter().zip(param_types.iter()) {
                    match pt {
                        Some(t) => self.define(p, VarInfo::var_typed(t)),
                        None => self.define(p, VarInfo::var()),
                    }
                }
                self.function_depth += 1;
                self.return_type_stack.push(None);
                let saved_loop = self.loop_depth;
                self.loop_depth = 0;
                let r = self.analyze_expr(body);
                self.loop_depth = saved_loop;
                self.return_type_stack.pop();
                self.function_depth -= 1;
                self.scopes.pop();
                r
            }
        }
    }
}
