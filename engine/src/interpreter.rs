//! Tree-walking interpreter.
//!
//! Scope rules (documented in DESIGN.md, tested in tests/integration.rs):
//! - The global scope is the bottom scope.
//! - A function call pushes a NEW scope. Parameters and variables
//!   defined for the first time inside a function live in that scope
//!   only and disappear when it returns.
//! - Reading a variable searches from the innermost scope outward to global.
//! - Step 1 (Meeting 11, 2026-11-05): LEXICAL ASSIGNMENT. Assigning to a
//!   name that already exists in an outer scope updates that binding
//!   (walking outward); only a name that exists nowhere is defined
//!   locally. This is what makes closures stateful (the counter
//!   pattern). A variable defined for the first time inside a function
//!   still never escapes it. (Index assignment to an existing
//!   list/dict mutates that object itself, wherever it lives.)
//! - A function value captures its defining scope chain (shared Rc
//!   scopes), so a returned function still sees and mutates its
//!   defining environment: lexical closures.
//! - `إذا` / `بينما` / `لكل` / `كرر` do NOT create a scope: variables
//!   assigned inside them stay visible after the block in the same scope.
//!   In particular a loop variable is still visible after the loop.
//!   Loop variables are always defined in the current scope
//!   (define_local), never assigned outward to a same-named global.
//!
//! Type rules: dynamic typing, runtime checks, no silent coercion
//! (except the explicit عدد() / نص() conversions). Conditions must be
//! منطقي (Bool). See DESIGN.md for the full table.

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{ErrorKind, RioError, RioResult};
use crate::lexer;
use crate::parser::Parser;
use crate::value::{ClassData, InstanceData, InterfaceData, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::io::Write;
use std::path::PathBuf;

pub(crate) const MAX_LOOP_ITERATIONS: usize = 1_000_000;
// P0 fix (2026-11-05, v1.2 final review): a hard call-depth limit for
// the tree-walking interpreter, so unbounded recursion returns an
// Arabic Niva error instead of overflowing the Rust stack and aborting
// the process. 200 is chosen empirically, not arbitrarily: measured
// 2026-11-05, the DEBUG build overflows at ~220-250 nested calls
// (unoptimised frames, 8 MB main stack), while the RELEASE build
// survives 1000+. A 1000 limit would therefore still crash under
// `cargo test` (debug). 200 is safe in BOTH builds, with margin.
// The VM phase (Phase 1B) will use its own explicit call stack,
// not the Rust stack, and may raise this limit with a Niva error,
// never with a crash.
// Meeting 15: measured depth cap, shared by the interpreter and
// the VM. On a 256 MB thread the worst measured shape
// (debug, 10 locals + wide expression) is safe past 700, so 500
// holds ~30% margin; release ceilings are far higher. The cap is
// enforced as a clean Niva error, never a stack overflow.
// Meeting 17 (WASM): the browser stack is far smaller than the
// native 256 MB thread (the playground builds with a 16 MB WASM
// stack), so the WASM cap is measured separately.
// Measured 2026-11-05 in Node (V8, the same engine Chromium
// uses), Meeting-15 extreme shape (10 locals + wide expression),
// cap lifted: largest completing N = 1500, smallest failing
// N = 1531 (a JS RangeError, not a WASM trap). Max executing
// depth = N+1, so the ceiling is depth ~1501. Cap 1000 holds
// ~33% margin on the worst shape — the same margin rule as
// Meeting 15. Depth beyond the cap is a clean Niva error,
// never a browser crash.
#[cfg(target_arch = "wasm32")]
pub(crate) const MAX_CALL_DEPTH: usize = 1000;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_CALL_DEPTH: usize = 500;

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
    // Shared scopes: each scope is an Rc so a function value can
    // capture its defining chain (lexical closures, Step 1) and
    // later calls still share it with the defining environment.
    scopes: Vec<Rc<RefCell<HashMap<String, Value>>>>,
    pub output: String,
    return_value: Option<Value>,
    call_depth: usize,
    // Phase 1A Step 2: modules.
    // program_dir: directory of the main program file, used to resolve
    // module names (module name = file stem, identifier only, so no
    // path traversal is possible through a module name).
    program_dir: PathBuf,
    // module name -> Module value, loaded once per run.
    loaded_modules: HashMap<String, Value>,
    // Modules currently being loaded (for cycle detection).
    loading_stack: Vec<String>,
    // Meeting 4: file capability policy. Default restricted to
    // program_dir; --allow-all (trusted) removes the restriction.
    file_policy: crate::permissions::FilePolicy,
}

impl Interpreter {
    pub fn new() -> Self {
        let program_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            scopes: vec![Rc::new(RefCell::new(HashMap::new()))],
            output: String::new(),
            return_value: None,
            call_depth: 0,
            program_dir: program_dir.clone(),
            loaded_modules: HashMap::new(),
            loading_stack: Vec::new(),
            file_policy: crate::permissions::FilePolicy::restricted(program_dir),
        }
    }

    pub fn with_program_dir(program_dir: PathBuf) -> Self {
        let mut interp = Self::new();
        interp.program_dir = program_dir.clone();
        interp.file_policy = crate::permissions::FilePolicy::restricted(program_dir);
        interp
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.file_policy.trusted = trusted;
    }

    fn define_local(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last() {
            scope.borrow_mut().insert(name.to_string(), value);
        }
    }

    // Step 1 (Meeting 11): lexical assignment. Assign to the
    // nearest scope that already binds the name (walking outward,
    // so closures mutate their captured variables and functions
    // can update outer variables); define locally only when the
    // name exists nowhere. Used for plain `س = قيمة` only:
    // loop variables and definitions stay define_local.
    fn assign_local(&mut self, name: &str, value: Value) {
        if let Some(i) = self.lookup_scope_index(name) {
            self.scopes[i].borrow_mut().insert(name.to_string(), value);
        } else {
            self.define_local(name, value);
        }
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.borrow().get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn lookup_scope_index(&self, name: &str) -> Option<usize> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.borrow().contains_key(name) {
                return Some(i);
            }
        }
        None
    }

    /// REPL support: evaluate one expression and return its value.
    /// Used for bare-expression lines, which the REPL prints.
    pub fn eval_one(&mut self, expr: &Expr) -> RioResult<Value> {
        self.eval_expr(expr)
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
            StmtKind::Assign { target, value, annotation } => {
                let v = self.eval_expr(value)?;
                // Phase 1A Step 3 (runtime part): an annotated
                // assignment is checked at runtime too, so gradual
                // cases (value from an untyped variable) and module
                // files (not semantically analyzed) are still caught.
                // `أي` means no check. Static checking for the main
                // program happens earlier, in the analyzer.
                if let Some(ann) = annotation {
                    if ann != "أي" && v.type_name() != ann {
                        let var_name = match target {
                            AssignTarget::Var(n) => n.clone(),
                            _ => String::new(),
                        };
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: المتغير {} معلن {}، لكن القيمة {}",
                                var_name,
                                ann,
                                v.type_name()
                            ),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                match target {
                    AssignTarget::Var(name) => {
                        // Step 1 (Meeting 11): lexical assignment.
                        self.assign_local(name, v);
                    }
                    AssignTarget::Index { target: t, index: i } => {
                        self.assign_index(t, i, v, stmt.line, stmt.col)?;
                    }
                    AssignTarget::Dot { target: t, name } => {
                        self.assign_dot(t, name, v, stmt.line, stmt.col)?;
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
            StmtKind::FuncDef { name, params, param_types, return_type, body } => {
                // Step 1: lexical closure — capture the defining
                // scope chain (shared Rc scopes). Module top-level
                // functions are marked with their module name after
                // module execution and then use the Step 2 module
                // scope push instead of this captured chain.
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    module: None,
                    param_types: param_types.clone(),
                    return_type: return_type.clone(),
                    captured: Some(self.scopes.clone()),
                };
                self.define_local(name, func);
                Ok(Flow::Normal)
            }
            StmtKind::ClassDef { name, base, interfaces, methods } => {
                // Resolve base class first (must already be defined).
                let base_data: Option<Rc<ClassData>> = match base {
                    Some(bname) => match self.lookup(bname) {
                        Some(Value::Class(data)) => Some(data),
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست فئة، بل {}", bname, other.type_name()),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("فئة أب غير معرفة: {}", bname),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                    },
                    None => None,
                };
                // Methods defined in a module file carry that module
                // name, so their bodies can read module globals,
                // like top-level module functions in Step 2.
                let mod_name = self.loading_stack.last().cloned();
                // Step 1: methods capture the defining scope chain
                // too (a method defined inside a function can close
                // over its locals); module methods use the Step 2
                // module push at call time instead.
                let captured = self.scopes.clone();
                let mut map: HashMap<String, Value> = HashMap::new();
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, param_types, return_type, body } = &m.kind {
                        map.insert(mn.clone(), Value::Function {
                            name: mn.clone(),
                            params: params.clone(),
                            body: body.clone(),
                            module: mod_name.clone(),
                            param_types: param_types.clone(),
                            return_type: return_type.clone(),
                            captured: Some(captured.clone()),
                        });
                    }
                }
                let class_data = Rc::new(ClassData {
                    name: name.clone(),
                    methods: map,
                    base: base_data,
                    interfaces: interfaces.clone(),
                });
                // Interface compliance: every required method must
                // exist (own or inherited) with the right arity.
                // v1 checks existence + arity only, not param types.
                for iname in interfaces {
                    let iface = match self.lookup(iname) {
                        Some(Value::Interface(data)) => data,
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست واجهة، بل {}", iname, other.type_name()),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("واجهة غير معرفة: {}", iname),
                                stmt.line,
                                stmt.col,
                            ));
                        }
                    };
                    for (mname, arity) in &iface.methods {
                        match class_data.lookup_method(mname) {
                            Some(Value::Function { params, .. }) => {
                                if params.len() != *arity {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الفئة {} تنفذ طريقة {} من واجهة {} بعدد معاملات غير صحيح: تتوقع {} لكن وجد {}",
                                            name, mname, iname, arity, params.len()
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                            _ => {
                                return Err(RioError::runtime(
                                    format!(
                                        "الفئة {} لا تنفذ طريقة {} المطلوبة من واجهة {}",
                                        name, mname, iname
                                    ),
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        }
                    }
                }
                let class = Value::Class(class_data);
                self.define_local(name, class);
                Ok(Flow::Normal)
            }
            StmtKind::InterfaceDef { name, methods } => {
                let iface = Value::Interface(Rc::new(InterfaceData {
                    name: name.clone(),
                    methods: methods.clone(),
                }));
                self.define_local(name, iface);
                Ok(Flow::Normal)
            }
            StmtKind::Import { module, alias, names } => {
                let mod_value = self.load_module(module, stmt.line, stmt.col)?;
                match names {
                    Some(names) => {
                        // من وحدة استورد اسم1، اسم2 : bind each name.
                        let exports = match &mod_value {
                            Value::Module { exports, .. } => exports.clone(),
                            _ => HashMap::new(),
                        };
                        for name in names {
                            match exports.get(name) {
                                Some(v) => self.define_local(name, v.clone()),
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                            name, module, name, module
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                        }
                    }
                    None => {
                        // استورد وحدة [باسم مختصر] : bind module value.
                        let bind = alias.clone().unwrap_or_else(|| module.clone());
                        self.define_local(&bind, mod_value);
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Export { .. } => {
                // Export is collected at module load time (see
                // load_module) from top-level statements, before
                // execution. At runtime it is a no-op, including in
                // the main program (the main program is not a module).
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

    /// Load a module by name (file stem), once per run.
    /// Returns a Module value. On cycle, missing file, or lex/parse
    /// failure, returns an Arabic Niva error naming the module.
    fn load_module(&mut self, module: &str, line: usize, col: usize) -> RioResult<Value> {
        if let Some(v) = self.loaded_modules.get(module) {
            return Ok(v.clone());
        }
        // Step 2 (Meeting 16): native stdlib modules come first,
        // so `استورد رياضيات` works anywhere with no file, and a
        // same-named file in the program directory never shadows
        // a native module. Cached like a file module below.
        if let Some(v) = crate::stdlib::native_module(module) {
            self.loaded_modules.insert(module.to_string(), v.clone());
            return Ok(v);
        }
        if self.loading_stack.contains(&module.to_string()) {
            let mut cycle = self.loading_stack.clone();
            cycle.push(module.to_string());
            return Err(RioError::runtime(
                format!("استيراد دائري: {}", cycle.join(" ← ")),
                line,
                col,
            ));
        }
        let path = self.program_dir.join(format!("{}.niva", module));
        let source = std::fs::read_to_string(&path).map_err(|_| {
            RioError::new(
                ErrorKind::Io,
                format!(
                    "وحدة غير موجودة: {} (بحثت في {})",
                    module,
                    path.display()
                ),
                line,
                col,
            )
        })?;
        let tokens = lexer::tokenize(&source).map_err(|e| {
            RioError::new(
                e.kind,
                format!("في وحدة {}: {}", module, e.message),
                e.line,
                e.col,
            )
        })?;
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().map_err(|e| {
            RioError::new(
                e.kind,
                format!("في وحدة {}: {}", module, e.message),
                e.line,
                e.col,
            )
        })?;
        // Collect export names from top-level Export statements only.
        // Strict rule: only exported names are accessible from outside.
        // A module with no Export statement exports nothing.
        let mut export_names: Vec<String> = Vec::new();
        for stmt in &program {
            if let StmtKind::Export { names } = &stmt.kind {
                for n in names {
                    if !export_names.contains(n) {
                        export_names.push(n.clone());
                    }
                }
            }
        }
        // Execute the module in a fresh global scope, isolated from
        // the caller's scopes. loaded_modules / loading_stack are
        // shared, so nested imports and cycle detection work.
        self.loading_stack.push(module.to_string());
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![Rc::new(RefCell::new(HashMap::new()))]);
        let saved_return = self.return_value.take();
        let exec_result = self.run(&program);
        let module_scope = if self.scopes.is_empty() {
            HashMap::new()
        } else {
            self.scopes[0].borrow().clone()
        };
        self.scopes = saved_scopes;
        self.return_value = saved_return;
        self.loading_stack.pop();
        exec_result.map_err(|e| {
            RioError::new(
                e.kind,
                format!("في وحدة {}: {}", module, e.message),
                e.line,
                e.col,
            )
        })?;
        // Mark top-level functions in the module scope with their
        // module name, so calls to them push this module's scope.
        // Functions defined inside module functions keep module=None
        // and rely on the outer call's module scope (Step 2 limit).
        // Step 1: a closure stored at module top level (its
        // captured chain is longer than the module scope alone)
        // is NOT marked — its captured environment, not the module
        // scope, must be restored at call time.
        let mut full_scope = module_scope.clone();
        for v in full_scope.values_mut() {
            if let Value::Function { module: m, captured, .. } = v {
                let top_level = match captured {
                    Some(chain) => chain.len() == 1,
                    None => true,
                };
                if top_level {
                    *m = Some(module.to_string());
                }
            }
        }
        let mut exports: HashMap<String, Value> = HashMap::new();
        for name in &export_names {
            if let Some(v) = full_scope.get(name) {
                exports.insert(name.clone(), v.clone());
            }
            // An exported name that was never defined is simply not
            // present in exports; importing it then fails with the
            // helpful "غير مصدر" message, naming the module.
        }
        let mod_value = Value::Module {
            name: module.to_string(),
            exports,
            scope: full_scope,
        };
        self.loaded_modules.insert(module.to_string(), mod_value.clone());
        Ok(mod_value)
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

    // Phase 1A Step 4: dot field assignment: ك.س = قيمة.
    // The target is evaluated to an Instance; because instance
    // fields live behind Rc, mutating the cloned Rc value mutates
    // the same fields the caller sees, for any target form
    // (variable, نفس, chained dot, list/dict element).
    fn assign_dot(
        &mut self,
        target: &Expr,
        name: &str,
        value: Value,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        let t = self.eval_expr(target)?;
        match t {
            Value::Instance(data) => {
                data.fields.borrow_mut().insert(name.to_string(), value);
                Ok(())
            }
            other => Err(RioError::typ(
                format!("لا يمكن تعيين حقل في نوع {}", other.type_name()),
                line,
                col,
            )),
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
        let mut scope = self.scopes[scope_idx].borrow_mut();
        let slot = scope.get_mut(&name).unwrap();
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
                            // Step 2 (Meeting 16): a stdlib function
                            // imported by name (من رياضيات استورد جذر).
                            Value::NativeFunction { func, .. } => {
                                func(&arg_values, expr.line, expr.col)
                            }
                            Value::Class(data) => Err(RioError::typ(
                                format!(
                                    "استخدم أنشئ لإنشاء كائن من فئة {}، مثل أنشئ {}()",
                                    data.name, data.name
                                ),
                                callee.line,
                                callee.col,
                            )),
                            other => Err(RioError::typ(
                                format!("القيمة {} ليست دالة", other.type_name()),
                                callee.line,
                                callee.col,
                            )),
                        };
                    }
                    return self.call_builtin(name, arg_values, expr.line, expr.col);
                }
                // Meeting 14: string/list methods. A Dot callee on
                // a Str/List target dispatches to a builtin method;
                // the target expression is kept so mutating list
                // methods can write back to a plain variable.
                // Other targets fall through to the generic Dot
                // evaluation below (modules, objects).
                if let ExprKind::Dot { target, name } = &callee.kind {
                    let target_value = self.eval_expr(target)?;
                    if matches!(target_value, Value::Str(_) | Value::List(_)) {
                        return self.call_builtin_method(
                            target,
                            target_value,
                            name,
                            arg_values,
                            expr.line,
                            expr.col,
                        );
                    }
                }
                let callee_value = self.eval_expr(callee)?;
                match callee_value {
                    Value::Function { .. } => {
                        self.call_user(callee_value, arg_values, expr.line, expr.col)
                    }
                    Value::NativeFunction { func, .. } => {
                        func(&arg_values, expr.line, expr.col)
                    }
                    Value::BoundMethod { func, instance } => {
                        self.call_method(*func, instance, arg_values, expr.line, expr.col)
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
            ExprKind::Dot { target, name } => {
                let t = self.eval_expr(target)?;
                match &t {
                    Value::Module { name: mod_name, exports, .. } => {
                        exports.get(name).cloned().ok_or_else(|| {
                            RioError::runtime(
                                format!(
                                    "الاسم {} غير مصدر من وحدة {}. أضف صدّر {} في وحدة {}",
                                    name, mod_name, name, mod_name
                                ),
                                expr.line,
                                expr.col,
                            )
                        })
                    }
                    Value::Instance(data) => {
                        // Field first, then method (bound to this
                        // instance). A field shadowing a method name
                        // wins, so ك.س can be data, not a method.
                        if let Some(v) = data.fields.borrow().get(name) {
                            Ok(v.clone())
                        } else if let Some(m) = data.class.lookup_method(name) {
                            Ok(Value::BoundMethod {
                                func: Box::new(m.clone()),
                                instance: Rc::clone(data),
                            })
                        } else {
                            Err(RioError::runtime(
                                format!(
                                    "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                                    data.class.name, name
                                ),
                                expr.line,
                                expr.col,
                            ))
                        }
                    }
                    other => Err(RioError::typ(
                        format!(
                            "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع {}",
                            other.type_name()
                        ),
                        expr.line,
                        expr.col,
                    )),
                }
            }
            ExprKind::Lambda { params, param_types, body } => {
                // Meeting 13: single-expression lambda, implicit
                // return. Stored as a Function whose body is one
                // Return statement, so call_user handles it exactly
                // like a named closure: lexical capture, param
                // types, depth limit, return value.
                let func = Value::Function {
                    name: "مجهولة".to_string(),
                    params: params.clone(),
                    body: vec![Stmt::new(
                        StmtKind::Return { value: Some((**body).clone()) },
                        expr.line,
                        expr.col,
                    )],
                    module: None,
                    param_types: param_types.clone(),
                    return_type: None,
                    captured: Some(self.scopes.clone()),
                };
                Ok(func)
            }
            ExprKind::Create { class, args } => {
                let mut arg_values = Vec::new();
                for a in args {
                    arg_values.push(self.eval_expr(a)?);
                }
                let class_value = match self.lookup(class) {
                    Some(v) => v,
                    None => {
                        return Err(RioError::runtime(
                            format!("فئة غير معرفة: {}", class),
                            expr.line,
                            expr.col,
                        ));
                    }
                };
                let class_data = match class_value {
                    Value::Class(data) => data,
                    other => {
                        return Err(RioError::typ(
                            format!("{} ليست فئة، بل {}", class, other.type_name()),
                            expr.line,
                            expr.col,
                        ));
                    }
                };
                let instance = Rc::new(InstanceData {
                    class: Rc::clone(&class_data),
                    fields: RefCell::new(HashMap::new()),
                });
                // Constructor: a method named أنشئ, if present,
                // is called automatically with نفس bound.
                if let Some(ctor) = class_data.lookup_method("أنشئ") {
                    let ctor = ctor.clone();
                    self.call_method(ctor, Rc::clone(&instance), arg_values, expr.line, expr.col)?;
                } else if !arg_values.is_empty() {
                    return Err(RioError::runtime(
                        format!(
                            "الفئة {} ليس لها منشئ أنشئ، لكن أعطيت {} معاملات",
                            class_data.name,
                            arg_values.len()
                        ),
                        expr.line,
                        expr.col,
                    ));
                }
                Ok(Value::Instance(instance))
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
        let (name, params, body, module, param_types, return_type, captured) = match func {
            Value::Function { name, params, body, module, param_types, return_type, captured } => {
                (name, params, body, module, param_types, return_type, captured)
            }
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
        // Phase 1A Step 3 (runtime part): param types, `أي` = no check.
        for (i, arg) in args.iter().enumerate() {
            if let Some(Some(pt)) = param_types.get(i) {
                if pt != "أي" && arg.type_name() != pt {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: معامل {} في الدالة {} معلن {}، لكن القيمة {}",
                            params[i],
                            name,
                            pt,
                            arg.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        // P0: refuse a new call at the limit, before pushing a scope
        // or a Rust frame for it, so the error is a Niva error, not a
        // stack overflow. Depth is decremented on every exit path,
        // including errors, so a caught error does not leak depth.
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        // Step 1: a closure call runs in its captured environment:
        // save the caller's stack, replace it with the captured
        // chain, push the parameter scope, restore after the call.
        // Module functions (module: Some) keep the Step 2
        // module-scope push on the caller's stack instead.
        let mut pushed_module_scope = false;
        let mut saved_scopes: Option<Vec<Rc<RefCell<HashMap<String, Value>>>>> = None;
        if let Some(mod_name) = &module {
            if let Some(Value::Module { scope, .. }) = self.loaded_modules.get(mod_name) {
                let scope = scope.clone();
                self.scopes.push(Rc::new(RefCell::new(scope)));
                pushed_module_scope = true;
            }
        } else if let Some(captured) = &captured {
            saved_scopes = Some(std::mem::replace(&mut self.scopes, captured.clone()));
        }
        let mut scope = HashMap::new();
        for (p, v) in params.iter().zip(args.into_iter()) {
            scope.insert(p.clone(), v);
        }
        self.scopes.push(Rc::new(RefCell::new(scope)));
        self.call_depth += 1;
        let result = self.exec_block(&body);
        self.scopes.pop();
        if pushed_module_scope {
            self.scopes.pop();
        }
        if let Some(saved) = saved_scopes {
            self.scopes = saved;
        }
        self.call_depth -= 1;
        match result? {
            Flow::Return => {
                let v = self.return_value.take().unwrap_or(Value::Nil);
                // Phase 1A Step 3 (runtime part): return type.
                if let Some(rt) = &return_type {
                    if rt != "أي" && v.type_name() != rt {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الدالة {} معلنة أن ترجع {}، لكن رجعت {}",
                                name,
                                rt,
                                v.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(v)
            }
            Flow::Normal => {
                // No explicit return: value is لاشيء. If a return
                // type is declared and is not لاشيء / أي, that is
                // a mismatch, caught here (not silently Nil).
                if let Some(rt) = &return_type {
                    if rt != "أي" && rt != "لاشيء" {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الدالة {} معلنة أن ترجع {}، لكن لم ترجع شيئا",
                                name, rt
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(Value::Nil)
            }
            Flow::Break => Err(RioError::runtime("اكسر خارج حلقة", line, col)),
            Flow::Continue => Err(RioError::runtime("تابع خارج حلقة", line, col)),
        }
    }

    // Phase 1A Step 4: call a method bound to an instance.
    // نفس is bound to the instance in the method scope, after
    // parameters, so نفس always refers to the instance even if
    // a parameter were (incorrectly) named نفس. Method params
    // are the user arguments only; نفس is NOT a parameter.
    fn call_method(
        &mut self,
        func: Value,
        instance: Rc<InstanceData>,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let (name, params, body, module, param_types, return_type, captured) = match func {
            Value::Function { name, params, body, module, param_types, return_type, captured } => {
                (name, params, body, module, param_types, return_type, captured)
            }
            _ => unreachable!(),
        };
        if args.len() != params.len() {
            return Err(RioError::runtime(
                format!(
                    "عدد المعاملات غير صحيح: الطريقة {} تتوقع {} لكن أعطيت {}",
                    name,
                    params.len(),
                    args.len()
                ),
                line,
                col,
            ));
        }
        for (i, arg) in args.iter().enumerate() {
            if let Some(Some(pt)) = param_types.get(i) {
                if pt != "أي" && arg.type_name() != pt {
                    return Err(RioError::typ(
                        format!(
                            "نوع غير متطابق: معامل {} في الطريقة {} معلن {}، لكن القيمة {}",
                            params[i],
                            name,
                            pt,
                            arg.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}). أوقف الاستدعاء الذاتي أو قلل عمقه",
                    MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        // Step 1: like call_user — module methods keep the Step 2
        // module-scope push; other methods run in their captured
        // environment (the scope chain at class definition time).
        let mut pushed_module_scope = false;
        let mut saved_scopes: Option<Vec<Rc<RefCell<HashMap<String, Value>>>>> = None;
        if let Some(mod_name) = &module {
            if let Some(Value::Module { scope, .. }) = self.loaded_modules.get(mod_name) {
                let scope = scope.clone();
                self.scopes.push(Rc::new(RefCell::new(scope)));
                pushed_module_scope = true;
            }
        } else if let Some(captured) = &captured {
            saved_scopes = Some(std::mem::replace(&mut self.scopes, captured.clone()));
        }
        let mut scope = HashMap::new();
        for (p, v) in params.iter().zip(args.into_iter()) {
            scope.insert(p.clone(), v);
        }
        scope.insert("نفس".to_string(), Value::Instance(Rc::clone(&instance)));
        self.scopes.push(Rc::new(RefCell::new(scope)));
        self.call_depth += 1;
        let result = self.exec_block(&body);
        self.scopes.pop();
        if pushed_module_scope {
            self.scopes.pop();
        }
        if let Some(saved) = saved_scopes {
            self.scopes = saved;
        }
        self.call_depth -= 1;
        match result? {
            Flow::Return => {
                let v = self.return_value.take().unwrap_or(Value::Nil);
                if let Some(rt) = &return_type {
                    if rt != "أي" && v.type_name() != rt {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الطريقة {} معلنة أن ترجع {}، لكن رجعت {}",
                                name,
                                rt,
                                v.type_name()
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(v)
            }
            Flow::Normal => {
                if let Some(rt) = &return_type {
                    if rt != "أي" && rt != "لاشيء" {
                        return Err(RioError::typ(
                            format!(
                                "نوع غير متطابق: الطريقة {} معلنة أن ترجع {}، لكن لم ترجع شيئا",
                                name, rt
                            ),
                            line,
                            col,
                        ));
                    }
                }
                Ok(Value::Nil)
            }
            Flow::Break => Err(RioError::runtime("اكسر خارج حلقة", line, col)),
            Flow::Continue => Err(RioError::runtime("تابع خارج حلقة", line, col)),
        }
    }

    // Meeting 14: string/list methods. String methods are pure
    // (they return new strings). List methods أضف/أزل mutate and
    // write back to a plain variable only (Meeting 11 lexical
    // assignment); a non-variable target is a clear runtime error.
    fn call_builtin_method(
        &mut self,
        target: &Expr,
        target_value: Value,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let arity = |expected: usize| -> RioResult<()> {
            if args.len() != expected {
                Err(RioError::runtime(
                    format!(
                        "الطريقة {} تتوقع {} معاملات لكن أعطيت {}",
                        name, expected, args.len()
                    ),
                    line, col,
                ))
            } else {
                Ok(())
            }
        };
        match &target_value {
            Value::Str(s) => {
                let s = s.clone();
                match name {
                    // طول counts characters, like the طول builtin,
                    // never bytes (Arabic text must count letters).
                    "طول" => {
                        arity(0)?;
                        Ok(Value::Number(s.chars().count() as f64))
                    }
                    "كبير" => {
                        arity(0)?;
                        Ok(Value::Str(s.to_uppercase()))
                    }
                    "صغير" => {
                        arity(0)?;
                        Ok(Value::Str(s.to_lowercase()))
                    }
                    "قص" => {
                        arity(0)?;
                        Ok(Value::Str(s.trim().to_string()))
                    }
                    "استبدل" => {
                        arity(2)?;
                        let (old, new) = match (&args[0], &args[1]) {
                            (Value::Str(a), Value::Str(b)) => (a.clone(), b.clone()),
                            _ => {
                                return Err(RioError::typ(
                                    "طريقة استبدل تتوقع نصين: القديم والجديد",
                                    line, col,
                                ))
                            }
                        };
                        Ok(Value::Str(s.replace(old.as_str(), new.as_str())))
                    }
                    "قسم" => {
                        arity(1)?;
                        let sep = match &args[0] {
                            Value::Str(sep) => sep.clone(),
                            _ => {
                                return Err(RioError::typ(
                                    "طريقة قسم تتوقع نصا كفاصل",
                                    line, col,
                                ))
                            }
                        };
                        Ok(Value::List(
                            s.split(sep.as_str())
                                .map(|p| Value::Str(p.to_string()))
                                .collect(),
                        ))
                    }
                    "يحتوي" => {
                        arity(1)?;
                        match &args[0] {
                            Value::Str(sub) => Ok(Value::Bool(s.contains(sub.as_str()))),
                            _ => Err(RioError::typ(
                                "طريقة يحتوي على نص تتوقع نصا",
                                line, col,
                            )),
                        }
                    }
                    _ => Err(RioError::typ(
                        format!("طريقة غير معروفة لنوع نص: {}", name),
                        line, col,
                    )),
                }
            }
            Value::List(items) => {
                let mut items = items.clone();
                match name {
                    "طول" => {
                        arity(0)?;
                        Ok(Value::Number(items.len() as f64))
                    }
                    "يحتوي" => {
                        arity(1)?;
                        Ok(Value::Bool(items.iter().any(|x| *x == args[0])))
                    }
                    "أضف" => {
                        arity(1)?;
                        self.require_plain_var_target(target, line, col)?;
                        items.push(args[0].clone());
                        self.write_back_list(target, items);
                        Ok(Value::Nil)
                    }
                    "أزل" => {
                        if args.len() > 1 {
                            return Err(RioError::runtime(
                                format!(
                                    "الطريقة أزل تتوقع 0 أو 1 معاملات لكن أعطيت {}",
                                    args.len()
                                ),
                                line, col,
                            ));
                        }
                        self.require_plain_var_target(target, line, col)?;
                        let removed = if args.is_empty() {
                            items.pop().ok_or_else(|| {
                                RioError::runtime("لا يمكن الإزالة من قائمة فارغة", line, col)
                            })?
                        } else {
                            let idx = match &args[0] {
                                Value::Number(x) if x.fract() == 0.0 && *x >= 0.0 => *x as usize,
                                _ => {
                                    return Err(RioError::typ(
                                        "طريقة أزل تتوقع فهرسا عددا صحيحا",
                                        line, col,
                                    ))
                                }
                            };
                            if idx >= items.len() {
                                return Err(RioError::runtime(
                                    format!(
                                        "فهرس خارج النطاق: {} والقائمة طولها {}",
                                        idx,
                                        items.len()
                                    ),
                                    line, col,
                                ));
                            }
                            items.remove(idx)
                        };
                        self.write_back_list(target, items);
                        Ok(removed)
                    }
                    _ => Err(RioError::typ(
                        format!("طريقة غير معروفة لنوع قائمة: {}", name),
                        line, col,
                    )),
                }
            }
            _ => unreachable!(),
        }
    }

    /// Mutating list methods (أضف/أزل) write back only to a plain
    /// variable target. Anything else (a call result, an index, a
    /// field) has nowhere to write back to, so it is an error
    /// before any mutation happens.
    fn require_plain_var_target(
        &self,
        target: &Expr,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        match &target.kind {
            ExprKind::Ident(_) => Ok(()),
            _ => Err(RioError::runtime(
                "طريقة معدلة للقائمة (أضف/أزل) تحتاج متغيرا عاديا، لا نتيجة تعبير",
                line, col,
            )),
        }
    }

    /// Write a mutated list back through lexical assignment
    /// (Meeting 11): the nearest existing binding is updated, so
    /// a list captured by a closure is updated in place too.
    /// Caller already checked the target is a plain variable.
    fn write_back_list(&mut self, target: &Expr, items: Vec<Value>) {
        if let ExprKind::Ident(name) = &target.kind {
            self.assign_local(name, Value::List(items));
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        // Meeting 17 (WASM): in the browser there is no filesystem
        // and no stdin, and user code is untrusted — these three
        // builtins are inert, with a clean Arabic error, instead
        // of touching std::fs / stdin at all.
        #[cfg(target_arch = "wasm32")]
        if matches!(name, "اقرأ_ملف" | "اكتب_ملف" | "ادخل") {
            return Err(RioError::runtime(
                "هذه الدالة غير مدعومة في متصفح الويب",
                line,
                col,
            ));
        }
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
                    Value::Str(path) => {
                        let resolved = self.file_policy.resolve(path, line, col)?;
                        std::fs::read_to_string(&resolved).map(Value::Str).map_err(|e| {
                            RioError::new(ErrorKind::Io, format!("تعذر قراءة الملف {}: {}", path, e), line, col)
                        })
                    }
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
                        let resolved = self.file_policy.resolve(path, line, col)?;
                        std::fs::write(&resolved, content).map(|_| Value::Nil).map_err(|e| {
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

    // Step 1 (Meeting 10/11): lexical closures + lexical assignment.
    #[test]
    fn closure_returned_sees_outer_variable() {
        let (out, res) = run_src(
            "دالة اصنع():\n    س = ١٠\n    دالة احصل():\n        ارجع س\n    ارجع احصل\nد = اصنع()\nاطبع د()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "10\n");
    }

    #[test]
    fn closure_counter_shares_state() {
        let (out, res) = run_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع أ()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "1\n2\n3\n");
    }

    #[test]
    fn closure_two_counters_are_independent() {
        let (out, res) = run_src(
            "دالة اصنع_عداد():\n    ع = ٠\n    دالة زد():\n        ع = ع + ١\n        ارجع ع\n    ارجع زد\nأ = اصنع_عداد()\nب = اصنع_عداد()\nاطبع أ()\nاطبع أ()\nاطبع ب()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "1\n2\n1\n");
    }

    #[test]
    fn closure_sees_later_mutation_of_outer() {
        let (out, res) = run_src(
            "س = ١\nدالة احصل():\n    ارجع س\nس = ٢\nاطبع احصل()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "2\n");
    }

    #[test]
    fn closure_param_shadows_captured() {
        let (out, res) = run_src(
            "س = ١٠\nدالة احصل(س):\n    ارجع س\nاطبع احصل(٥)\nاطبع س\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "5\n10\n");
    }

    #[test]
    fn lexical_assignment_mutates_outer() {
        let (out, res) = run_src(
            "س = ١٠\nدالة غير():\n    س = ٢٠\nغير()\nاطبع س\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "20\n");
    }

    #[test]
    fn closure_recursion_still_works() {
        let (out, res) = run_src(
            "دالة فيب(ن):\n    إذا ن <= ١:\n        ارجع ن\n    ارجع فيب(ن - ١) + فيب(ن - ٢)\nاطبع فيب(١٠)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "55\n");
    }

    // Step 1 (Meeting 13): lambdas — دالة_مجهولة(س): س + 1.
    #[test]
    fn lambda_basic_value_and_call() {
        let (out, res) = run_src(
            "مضاعف = دالة_مجهولة(س): س * 2\nاطبع مضاعف(5)\nاطبع مضاعف\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "10\n<دالة مجهولة>\n");
    }

    #[test]
    fn lambda_passed_as_argument() {
        let (out, res) = run_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س): س * 3، 4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "12\n");
    }

    #[test]
    fn lambda_factory_captures_param() {
        let (out, res) = run_src(
            "دالة اصنع_مضاعف(ن):\n    ارجع دالة_مجهولة(س): س * ن\nمضاعف3 = اصنع_مضاعف(3)\nاطبع مضاعف3(4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "12\n");
    }

    #[test]
    fn lambda_captures_outer_variable() {
        let (out, res) = run_src(
            "س = 10\nاحصل = دالة_مجهولة(): س\nاطبع احصل()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "10\n");
    }

    #[test]
    fn lambda_typed_param_checked_at_runtime() {
        let (_, res) = run_src(
            "دالة طبق(د، س):\n    ارجع د(س)\nاطبع طبق(دالة_مجهولة(س: عدد): س + 1، \"نص\")\n",
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("نوع غير متطابق"));
    }

    #[test]
    fn lambda_immediate_call_via_parens() {
        let (out, res) = run_src(
            "د = (دالة_مجهولة(س): س + 1)(10)\nاطبع د\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "11\n");
    }

    #[test]
    fn lambda_currying_nested_lambda() {
        let (out, res) = run_src(
            "مركب = دالة_مجهولة(س): دالة_مجهولة(ص): س + ص\nاطبع مركب(3)(4)\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "7\n");
    }

    // Step 1 (Meeting 14): string/list methods.
    #[test]
    fn str_methods_basic() {
        let (out, res) = run_src(
            "اطبع \"مرحبا\".طول()\nاطبع \"Hello\".كبير()\nاطبع \"Hello\".صغير()\nاطبع \"  محمود  \".قص()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "5\nHELLO\nhello\nمحمود\n");
    }

    #[test]
    fn str_replace_split_contains() {
        let (out, res) = run_src(
            "اطبع \"أهلا محمود\".استبدل(\"محمود\"، \"سارة\")\nاطبع \"أ، ب\".قسم(\"، \")\nاطبع \"مرحبا\".يحتوي(\"رحب\")\nاطبع \"مرحبا\".يحتوي(\"وداعا\")\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "أهلا سارة\n[\"أ\", \"ب\"]\nصحيح\nخطأ\n");
    }

    #[test]
    fn list_append_writes_back() {
        let (out, res) = run_src(
            "ق = [1، 2]\nق.أضف(3)\nاطبع ق\nاطبع ق.طول()\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "[1, 2, 3]\n3\n");
    }

    #[test]
    fn list_pop_last_and_index() {
        let (out, res) = run_src(
            "ق = [1، 2، 3]\nاطبع ق.أزل()\nاطبع ق.أزل(0)\nاطبع ق\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "3\n1\n[2]\n");
    }

    #[test]
    fn list_method_unknown_is_runtime_error() {
        // Untyped variable: the analyzer cannot know the type,
        // so the unknown method fails at runtime, not analysis.
        let (_, res) = run_src("ق = [1]\nاطبع ق.غير_موجود()\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("طريقة غير معروفة لنوع قائمة"));
    }

    #[test]
    fn list_append_on_call_result_is_error() {
        let (_, res) = run_src(
            "دالة اصنع():\n    ارجع [1، 2]\nاصنع().أضف(3)\n",
        );
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("تحتاج متغيرا عاديا"));
    }

    #[test]
    fn list_pop_from_empty_is_error() {
        let (_, res) = run_src("ق = []\nاطبع ق.أزل()\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("قائمة فارغة"));
    }

    // Step 2 (Meeting 16): initial stdlib — native modules.
    #[test]
    fn stdlib_math_basic() {
        let (out, res) = run_src(
            "استورد رياضيات\nاطبع رياضيات.جذر(16)\nاطبع رياضيات.قوة(2، 10)\nاطبع رياضيات.جيب(0)\nاطبع رياضيات.أرضية(3.7)\nاطبع رياضيات.سقف(3.2)\nاطبع رياضيات.باي\nاطبع رياضيات.هـ\nاطبع رياضيات.لوغاريتم(رياضيات.هـ)\n",
        );
        assert!(res.is_ok());
        assert_eq!(
            out,
            "4\n1024\n0\n3\n4\n3.141592653589793\n2.718281828459045\n1\n"
        );
    }

    #[test]
    fn stdlib_math_errors() {
        let (_, res) = run_src("استورد رياضيات\nاطبع رياضيات.جذر(-1)\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("جذر عدد سالب"));
        let (_, res) = run_src("استورد رياضيات\nاطبع رياضيات.جذر(1، 2)\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("تتوقع 1 معاملات"));
    }

    #[test]
    fn stdlib_from_import_binds_native_function() {
        let (out, res) = run_src("من رياضيات استورد جذر\nاطبع جذر(25)\nاطبع نوع(جذر)\n");
        assert!(res.is_ok());
        assert_eq!(out, "5\nدالة\n");
    }

    #[test]
    fn stdlib_random_ranges() {
        // Random values are never asserted exactly — only ranges
        // and types, so the test is deterministic.
        let (out, res) = run_src(
            "استورد عشوائي\nاطبع عشوائي.عدد_عشوائي() >= 0\nاطبع عشوائي.عدد_عشوائي() < 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) >= 1\nاطبع عشوائي.صحيح_عشوائي(1، 6) <= 6\nاطبع عشوائي.اختيار([10، 20]) في [10، 20]\n",
        );
        assert!(res.is_ok());
        assert_eq!(out, "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\n");
    }

    #[test]
    fn stdlib_random_errors() {
        let (_, res) = run_src("استورد عشوائي\nاطبع عشوائي.اختيار([])\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("قائمة فارغة"));
        let (_, res) = run_src("استورد عشوائي\nاطبع عشوائي.صحيح_عشوائي(6، 1)\n");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("الحد الأدنى أكبر"));
    }

    #[test]
    fn stdlib_time_now_is_number() {
        let (out, res) = run_src("استورد وقت\nاطبع وقت.الآن() > 1700000000\n");
        assert!(res.is_ok());
        assert_eq!(out, "صحيح\n");
    }
}
