//! Phase 1B VM (experimental, --vm flag only).
//!
//! This is NOT yet the default execution path. The tree-walking
//! interpreter remains default; the VM is a separate, testable
//! slice, to be expanded in later slices (lists, classes,
//! modules, GC) before it ever replaces the interpreter.
//!
//! Design:
//! - Compiler: AST Stmt/Expr -> Vec<Instr>, with jump patching
//!   for إذا / بينما, and short-circuit jumps for و / أو.
//! - VM: stack machine, pc, variables map, output string.
//! - Supported: numbers, strings, booleans, لاشيء,
//!   variables, arithmetic, comparisons, و / أو (short-circuit),
//!   ليس, سالب, تعيين متغير, اطبع, إذا / وإلا إذا / وإلا,
//!   بينما, جملة تعبير (with Pop), and (slice 2) functions:
//!   top-level and nested دالة definitions (global table, no
//!   closures), direct calls by name, ارجع, recursion with
//!   the same depth limit as the interpreter (Meeting 15).
//!   Slice 3 adds: list/dict literals, index read (list,
//!   dict, string char), and index assignment on a plain
//!   variable (قائمة[١] = ٩٩, قاموس["مفتاح"] = قيمة).
//!   Lists/dicts are value types (cloned on assignment),
//!   exactly like the interpreter: ب = قائمة copies it,
//!   so mutating ب does not affect قائمة; only
//!   قائمة[١] = ... mutates the variable's own slot.
//!   Slice 4 adds: the 9 pure builtins (طول، مدى، مجموع،
//!   أدنى، أقصى، مطلق، عدد، نص، نوع), with the same
//!   messages and arity errors as the interpreter.
//!   Resolution matches the interpreter exactly: a
//!   user function of the same name wins (checked at
//!   compile time), and a variable of the same name
//!   wins at runtime (Op::Builtin checks vars first
//!   and errors "القيمة {} ليست دالة", like the
//!   interpreter); only then does the builtin run.
//!   Slice 5 adds: full classes (فئة، أنشئ، نفس،
//!   حقول، طرق، وراثة، واجهات). Methods are compiled
//!   to bytecode and flattened at compile time (base
//!   methods copied, child overrides win), so method
//!   lookup is one map lookup. Base/interface checks
//!   run at class-definition time, like the
//!   interpreter. نفس is bound in the method's child
//!   VM. A field shadowing a method name wins, like
//!   the interpreter. BoundMethod values can be
//!   stored in a variable and called.
//! - NOT supported yet (compile error, clear message):
//!   modules, imports, exports, for, repeat, try,
//!   break, continue, file builtins (اقرأ_ملف،
//!   اكتب_ملف), ادخل, في (In), first-class plain
//!   functions, complex index-assignment targets
//!   (قائمة[٠][١] = ...). Those remain tree-walking
//!   only for now.
//! - Known VM gaps (documented, not new to a slice):
//!   gradual param types are not checked at runtime
//!   in the VM (functions or methods); a variable
//!   assigned AFTER a function definition, then
//!   called, runs the function in the VM but errors
//!   in the interpreter.
//! - GC: none beyond Rust ownership + Rc for values
//!   already in Value. A tracing GC is a later 1B slice, after
//!   the VM heap exists. This slice does NOT claim a GC.

use crate::ast::{BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{RioError, RioResult};
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Op {
    Const(Value),
    Load(String),
    Store(String),
    Pop,
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
    Not,
    Neg,
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    Print(usize),
    Call(String, usize),
    Return,
    BuildList(usize),
    BuildDict(Vec<String>),
    Index,
    StoreIndex(String),
    Builtin(String, usize),
    DefineClass(VmClassDef),
    DefineInterface(String, Vec<(String, usize)>),
    Dot(String),
    StoreDot(String),
    Create(String, usize),
    CallBound(usize),
}

/// Slice 5: a compiled class. Methods are FLATTENED at
/// compile time: base methods copied in, child overrides
/// win, so runtime lookup is one map lookup and matches
/// the interpreter's base-chain walk (child wins).
#[derive(Debug, Clone)]
pub struct VmClassDef {
    pub name: String,
    pub base: Option<String>,
    pub interfaces: Vec<String>,
    pub methods: HashMap<String, FunctionDef>,
}

/// Slice 4: the 9 pure builtins the VM knows. File builtins
/// (اقرأ_ملف، اكتب_ملف) and ادخل are a later slice: they need
/// the file policy / stdin wired into the VM first.
const PURE_BUILTINS: [&str; 9] =
    ["طول", "مدى", "مجموع", "أدنى", "أقصى", "مطلق", "عدد", "نص", "نوع"];

fn is_pure_builtin(name: &str) -> bool {
    PURE_BUILTINS.contains(&name)
}

#[derive(Debug, Clone)]
pub struct Instr {
    pub op: Op,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub params: Vec<String>,
    pub code: Vec<Instr>,
}

#[derive(Debug, Clone)]
pub struct CompiledProgram {
    pub code: Vec<Instr>,
    pub functions: HashMap<String, FunctionDef>,
}

pub struct Compiler {
    pub code: Vec<Instr>,
    pub functions: HashMap<String, FunctionDef>,
    /// Compile-time class table, used only to flatten methods
    /// for inheritance. Never merged from sub-compilers: a class
    /// defined inside a function is local to that function,
    /// like in the interpreter.
    pub classes: HashMap<String, VmClassDef>,
}

impl Compiler {
    pub fn new() -> Self {
        Self { code: Vec::new(), functions: HashMap::new(), classes: HashMap::new() }
    }

    fn emit(&mut self, op: Op, line: usize, col: usize) -> usize {
        self.code.push(Instr { op, line, col });
        self.code.len() - 1
    }

    fn patch_jump(&mut self, idx: usize, target: usize) {
        match &mut self.code[idx].op {
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t) => *t = target,
            _ => {}
        }
    }

    pub fn compile_program(program: &[Stmt]) -> RioResult<CompiledProgram> {
        let mut c = Self::new();
        for stmt in program {
            c.compile_stmt(stmt)?;
        }
        Ok(CompiledProgram { code: c.code, functions: c.functions })
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation: _ } => {
                match target {
                    crate::ast::AssignTarget::Var(name) => {
                        self.compile_expr(value)?;
                        self.emit(Op::Store(name.clone()), stmt.line, stmt.col);
                        Ok(())
                    }
                    crate::ast::AssignTarget::Index { target: t, index: i } => {
                        // Slice 3: index assignment on a plain
                        // variable only, like the interpreter
                        // (complex targets are rejected there too).
                        // Evaluation order matches the interpreter:
                        // value first, then index. Stack at
                        // StoreIndex: [value, index].
                        let name = match &t.kind {
                            ExprKind::Ident(n) => n.clone(),
                            _ => {
                                return Err(RioError::parse(
                                    "VM: تعيين الفهرسة المعقدة غير مدعوم بعد في --vm، استخدم المفسر الافتراضي",
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        };
                        self.compile_expr(value)?;
                        self.compile_expr(i)?;
                        self.emit(Op::StoreIndex(name), stmt.line, stmt.col);
                        Ok(())
                    }
                    crate::ast::AssignTarget::Dot { target: t, name } => {
                        // Slice 5: field assignment on an instance.
                        // Evaluation order matches the interpreter:
                        // value first, then target. Stack at
                        // StoreDot: [value, target].
                        self.compile_expr(value)?;
                        self.compile_expr(t)?;
                        self.emit(Op::StoreDot(name.clone()), stmt.line, stmt.col);
                        Ok(())
                    }
                }
            }
            StmtKind::Print { args } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit(Op::Print(args.len()), stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::If { cond, then_body, elifs, else_body } => {
                // Chain: cond, JumpIfFalse -> next, then, Jump -> end,
                // elifs..., else, end. Patch all end-jumps at the end.
                let mut end_jumps: Vec<usize> = Vec::new();
                self.compile_expr(cond)?;
                let mut next_jump = self.emit(Op::JumpIfFalse(0), cond.line, cond.col);
                for s in then_body {
                    self.compile_stmt(s)?;
                }
                end_jumps.push(self.emit(Op::Jump(0), stmt.line, stmt.col));
                self.patch_jump(next_jump, self.code.len());
                for (econd, ebody) in elifs {
                    self.compile_expr(econd)?;
                    next_jump = self.emit(Op::JumpIfFalse(0), econd.line, econd.col);
                    for s in ebody {
                        self.compile_stmt(s)?;
                    }
                    end_jumps.push(self.emit(Op::Jump(0), stmt.line, stmt.col));
                    self.patch_jump(next_jump, self.code.len());
                }
                if let Some(body) = else_body {
                    for s in body {
                        self.compile_stmt(s)?;
                    }
                }
                let end = self.code.len();
                for j in end_jumps {
                    self.patch_jump(j, end);
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                let start = self.code.len();
                self.compile_expr(cond)?;
                let exit_jump = self.emit(Op::JumpIfFalse(0), cond.line, cond.col);
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.emit(Op::Jump(start), stmt.line, stmt.col);
                let end = self.code.len();
                self.patch_jump(exit_jump, end);
                Ok(())
            }
            StmtKind::ExprStmt { expr } => {
                self.compile_expr(expr)?;
                self.emit(Op::Pop, stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::FuncDef { name, params, body, .. } => {
                // Slice 2: compile the body with a fresh compiler,
                // merge its nested functions into this table (so
                // nested definitions are not lost), then register
                // this function. At runtime FuncDef emits nothing:
                // the function already lives in the table.
                let mut sub = Self::new();
                for s in body {
                    sub.compile_stmt(s)?;
                }
                for (n, f) in sub.functions {
                    self.functions.insert(n, f);
                }
                self.functions.insert(
                    name.clone(),
                    FunctionDef { params: params.clone(), code: sub.code },
                );
                Ok(())
            }
            StmtKind::ClassDef { name, base, interfaces, methods } => {
                // Slice 5: compile each method body with a fresh
                // sub-compiler (like FuncDef), merge nested
                // functions into this table, then flatten: base
                // methods first, own methods override. Base and
                // interface EXISTENCE are checked at runtime by
                // Op::DefineClass, like the interpreter.
                let mut flat: HashMap<String, FunctionDef> = HashMap::new();
                if let Some(bname) = base {
                    if let Some(bdef) = self.classes.get(bname) {
                        flat = bdef.methods.clone();
                    }
                }
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, body, .. } = &m.kind {
                        let mut sub = Self::new();
                        for s in body {
                            sub.compile_stmt(s)?;
                        }
                        for (n, f) in sub.functions {
                            self.functions.insert(n, f);
                        }
                        flat.insert(
                            mn.clone(),
                            FunctionDef { params: params.clone(), code: sub.code },
                        );
                    }
                }
                let def = VmClassDef {
                    name: name.clone(),
                    base: base.clone(),
                    interfaces: interfaces.clone(),
                    methods: flat,
                };
                self.classes.insert(name.clone(), def.clone());
                self.emit(Op::DefineClass(def), stmt.line, stmt.col);
                Ok(())
            }
            StmtKind::InterfaceDef { name, methods } => {
                self.emit(
                    Op::DefineInterface(name.clone(), methods.clone()),
                    stmt.line,
                    stmt.col,
                );
                Ok(())
            }
            StmtKind::Return { value } => {
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => {
                        self.emit(Op::Const(Value::Nil), stmt.line, stmt.col);
                    }
                }
                self.emit(Op::Return, stmt.line, stmt.col);
                Ok(())
            }
            _ => Err(RioError::parse(
                "VM: هذه الجملة غير مدعومة بعد في --vm، استخدم المفسر الافتراضي",
                stmt.line,
                stmt.col,
            )),
        }
    }

    fn compile_expr(&mut self, expr: &Expr) -> RioResult<()> {
        match &expr.kind {
            ExprKind::Number(n) => {
                self.emit(Op::Const(Value::Number(*n)), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Str(s) => {
                self.emit(Op::Const(Value::Str(s.clone())), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Bool(b) => {
                self.emit(Op::Const(Value::Bool(*b)), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Nil => {
                self.emit(Op::Const(Value::Nil), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Ident(name) => {
                self.emit(Op::Load(name.clone()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::List(elems) => {
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.emit(Op::BuildList(elems.len()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Dict(entries) => {
                // Keys are compile-time strings; values are
                // compiled in order and popped by BuildDict.
                let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
                for (_, v) in entries {
                    self.compile_expr(v)?;
                }
                self.emit(Op::BuildDict(keys), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Index { target, index } => {
                self.compile_expr(target)?;
                self.compile_expr(index)?;
                self.emit(Op::Index, expr.line, expr.col);
                Ok(())
            }
            ExprKind::Dot { target, name } => {
                self.compile_expr(target)?;
                self.emit(Op::Dot(name.clone()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Create { class, args } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.emit(Op::Create(class.clone(), args.len()), expr.line, expr.col);
                Ok(())
            }
            ExprKind::Unary { op, expr: inner } => {
                self.compile_expr(inner)?;
                match op {
                    UnOp::Neg => self.emit(Op::Neg, expr.line, expr.col),
                    UnOp::Not => self.emit(Op::Not, expr.line, expr.col),
                };
                Ok(())
            }
            ExprKind::Binary { left, op, right } => {
                // Short-circuit و / أو, preserving interpreter semantics.
                // JumpIfFalse/True pop the left value, so the false/
                // true target must push the short-circuit result.
                if *op == BinOp::And {
                    self.compile_expr(left)?;
                    let jf = self.emit(Op::JumpIfFalse(0), expr.line, expr.col);
                    self.compile_expr(right)?;
                    let jmp_end = self.emit(Op::Jump(0), expr.line, expr.col);
                    let false_target = self.code.len();
                    self.patch_jump(jf, false_target);
                    self.emit(Op::Const(Value::Bool(false)), expr.line, expr.col);
                    let end = self.code.len();
                    self.patch_jump(jmp_end, end);
                    return Ok(());
                }
                if *op == BinOp::Or {
                    self.compile_expr(left)?;
                    let jt = self.emit(Op::JumpIfTrue(0), expr.line, expr.col);
                    self.compile_expr(right)?;
                    let jmp_end = self.emit(Op::Jump(0), expr.line, expr.col);
                    let true_target = self.code.len();
                    self.patch_jump(jt, true_target);
                    self.emit(Op::Const(Value::Bool(true)), expr.line, expr.col);
                    let end = self.code.len();
                    self.patch_jump(jmp_end, end);
                    return Ok(());
                }
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let op = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Eq => Op::Eq,
                    BinOp::NotEq => Op::NotEq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::LtEq => Op::LtEq,
                    BinOp::GtEq => Op::GtEq,
                    BinOp::In | BinOp::NotIn => {
                        return Err(RioError::parse(
                            "VM: في غير مدعومة بعد",
                            expr.line,
                            expr.col,
                        ));
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.emit(op, expr.line, expr.col);
                Ok(())
            }
            ExprKind::Call { callee, args } => {
                // Slice 5: a method call (ك.طريقة(...)) compiles
                // the dot first (pushing a BoundMethod), then
                // CallBound. Other non-Ident callees are still
                // unsupported: they need first-class function
                // values in the VM, a later slice.
                if let ExprKind::Dot { .. } = &callee.kind {
                    self.compile_expr(callee)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.emit(Op::CallBound(args.len()), expr.line, expr.col);
                    return Ok(());
                }
                let name = match &callee.kind {
                    ExprKind::Ident(n) => n.clone(),
                    _ => {
                        return Err(RioError::parse(
                            "VM: الاستدعاء غير المباشر (طريقة أو متغير يحمل دالة) غير مدعوم بعد في --vm، استخدم المفسر الافتراضي",
                            expr.line,
                            expr.col,
                        ));
                    }
                };
                for a in args {
                    self.compile_expr(a)?;
                }
                // Slice 4: resolution order matches the
                // interpreter (user function first, then
                // builtin). A user function defined earlier
                // in the program wins over a builtin name.
                // A same-named variable is checked at
                // runtime by Op::Builtin itself.
                if self.functions.contains_key(&name) {
                    self.emit(Op::Call(name, args.len()), expr.line, expr.col);
                } else if is_pure_builtin(&name) {
                    self.emit(Op::Builtin(name, args.len()), expr.line, expr.col);
                } else {
                    self.emit(Op::Call(name, args.len()), expr.line, expr.col);
                }
                Ok(())
            }
            ExprKind::Lambda { .. } => Err(RioError::parse(
                "VM: الدالة المجهولة (lambda) غير مدعومة بعد في --vm، استخدم المفسر الافتراضي",
                expr.line,
                expr.col,
            )),
            _ => Err(RioError::parse(
                "VM: هذا التعبير غير مدعوم بعد في --vm، استخدم المفسر الافتراضي",
                expr.line,
                expr.col,
            )),
        }
    }
}

pub struct Vm {
    pub code: Vec<Instr>,
    pub pc: usize,
    pub stack: Vec<Value>,
    pub vars: HashMap<String, Value>,
    pub output: String,
    pub functions: Rc<HashMap<String, FunctionDef>>,
    /// Slice 5: classes defined at runtime so far. A plain map,
    /// cloned into child VMs: a class defined inside a method
    /// does not leak back to the caller, like the interpreter's
    /// local scope.
    pub classes: HashMap<String, VmClassDef>,
    pub return_value: Option<Value>,
    pub call_depth: usize,
}

impl Vm {
    pub fn new(code: Vec<Instr>, functions: HashMap<String, FunctionDef>) -> Self {
        Self {
            code,
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            output: String::new(),
            functions: Rc::new(functions),
            classes: HashMap::new(),
            return_value: None,
            call_depth: 0,
        }
    }

    fn child(&self, func: &FunctionDef, args: Vec<Value>) -> Vm {
        let mut vars = HashMap::new();
        for (name, val) in func.params.iter().zip(args.into_iter()) {
            vars.insert(name.clone(), val);
        }
        Vm {
            code: func.code.clone(),
            pc: 0,
            stack: Vec::new(),
            vars,
            output: String::new(),
            functions: Rc::clone(&self.functions),
            classes: self.classes.clone(),
            return_value: None,
            call_depth: self.call_depth + 1,
        }
    }

    /// Slice 5: a method's child VM: params bound, plus نفس
    /// bound to the instance, like the interpreter.
    fn child_method(
        &self,
        func: &FunctionDef,
        args: Vec<Value>,
        instance: Rc<crate::value::InstanceData>,
    ) -> Vm {
        let mut child = self.child(func, args);
        child
            .vars
            .insert("نفس".to_string(), Value::Instance(instance));
        child
    }

    /// Slice 5: run one method of a class on an instance.
    /// Arity and depth errors mirror the interpreter's
    /// call_method (param types are not checked in the VM,
    /// like plain functions). Child output is appended.
    fn call_method_vm(
        &mut self,
        class_name: &str,
        method_name: &str,
        instance: Rc<crate::value::InstanceData>,
        args: Vec<Value>,
        line: usize,
        col: usize,
    ) -> RioResult<Value> {
        let func = match self
            .classes
            .get(class_name)
            .and_then(|c| c.methods.get(method_name))
        {
            Some(f) => f.clone(),
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                        class_name, method_name
                    ),
                    line,
                    col,
                ));
            }
        };
        if func.params.len() != args.len() {
            return Err(RioError::runtime(
                format!(
                    "عدد المعاملات غير صحيح: الطريقة {} تتوقع {} لكن أعطيت {}",
                    method_name,
                    func.params.len(),
                    args.len()
                ),
                line,
                col,
            ));
        }
        if self.call_depth >= crate::interpreter::MAX_CALL_DEPTH {
            return Err(RioError::runtime(
                format!(
                    "تجاوز حد عمق الاستدعاء ({}) في VM",
                    crate::interpreter::MAX_CALL_DEPTH
                ),
                line,
                col,
            ));
        }
        let mut child_vm = self.child_method(&func, args, instance);
        match child_vm.run() {
            Ok(()) => {
                self.output.push_str(&child_vm.output);
                Ok(child_vm.return_value.unwrap_or(Value::Nil))
            }
            Err(e) => {
                self.output.push_str(&child_vm.output);
                Err(e)
            }
        }
    }

    fn pop(&mut self, line: usize, col: usize) -> RioResult<Value> {
        self.stack.pop().ok_or_else(|| {
            RioError::runtime("مكدس VM فارغ", line, col)
        })
    }

    fn pop_bool(&mut self, line: usize, col: usize) -> RioResult<bool> {
        match self.pop(line, col)? {
            Value::Bool(b) => Ok(b),
            other => Err(RioError::typ(
                format!("الشرط يجب أن يكون منطقيا، وليس {}", other.type_name()),
                line,
                col,
            )),
        }
    }

    /// Slice 4: the 9 pure builtins, mirroring the interpreter's
    /// call_builtin exactly (same messages, same arity errors).
    fn call_pure_builtin(
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
                    return Err(RioError::runtime("مدى تتوقع 1 إلى 3 معاملات", line, col));
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
                    if out.len() > crate::interpreter::MAX_LOOP_ITERATIONS {
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
                            RioError::typ(format!("لا يمكن تحويل النص إلى عدد: {}", s), line, col)
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
            _ => Err(RioError::runtime(
                format!("دالة مدمجة غير معروفة في VM: {}", name),
                line,
                col,
            )),
        }
    }

    pub fn run(&mut self) -> RioResult<()> {
        while self.pc < self.code.len() {
            let instr = self.code[self.pc].clone();
            let line = instr.line;
            let col = instr.col;
            self.pc += 1;
            match instr.op {
                Op::Const(v) => self.stack.push(v),
                Op::Load(name) => {
                    let v = self.vars.get(&name).cloned().ok_or_else(|| {
                        RioError::runtime(format!("متغير غير معرف: {}", name), line, col)
                    })?;
                    self.stack.push(v);
                }
                Op::Store(name) => {
                    let v = self.pop(line, col)?;
                    self.vars.insert(name, v);
                }
                Op::Pop => {
                    let _ = self.pop(line, col)?;
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    let (a, b) = match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => (*a, *b),
                        (Value::Str(a), Value::Str(b)) if matches!(instr.op, Op::Add) => {
                            self.stack.push(Value::Str(format!("{}{}", a, b)));
                            continue;
                        }
                        _ => {
                            return Err(RioError::typ(
                                format!("العملية الحسابية تتطلب عددين، وليس {} و {}", l.type_name(), r.type_name()),
                                line,
                                col,
                            ));
                        }
                    };
                    let v = match instr.op {
                        Op::Add => Value::Number(a + b),
                        Op::Sub => Value::Number(a - b),
                        Op::Mul => Value::Number(a * b),
                        Op::Div => {
                            if b == 0.0 {
                                return Err(RioError::runtime("قسمة على صفر", line, col));
                            }
                            Value::Number(a / b)
                        }
                        Op::Mod => {
                            if b == 0.0 {
                                return Err(RioError::runtime("باقي القسمة على صفر", line, col));
                            }
                            Value::Number(a % b)
                        }
                        _ => unreachable!(),
                    };
                    self.stack.push(v);
                }
                Op::Eq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    self.stack.push(Value::Bool(l == r));
                }
                Op::NotEq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    self.stack.push(Value::Bool(l != r));
                }
                Op::Lt | Op::Gt | Op::LtEq | Op::GtEq => {
                    let r = self.pop(line, col)?;
                    let l = self.pop(line, col)?;
                    let ord = match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
                        _ => {
                            return Err(RioError::typ(
                                format!("المقارنة تتطلب عددين أو نصين، وليس {} و {}", l.type_name(), r.type_name()),
                                line,
                                col,
                            ));
                        }
                    };
                    let ord = ord.ok_or_else(|| RioError::runtime("مقارنة غير صالحة", line, col))?;
                    let b = match instr.op {
                        Op::Lt => ord == std::cmp::Ordering::Less,
                        Op::Gt => ord == std::cmp::Ordering::Greater,
                        Op::LtEq => ord != std::cmp::Ordering::Greater,
                        Op::GtEq => ord != std::cmp::Ordering::Less,
                        _ => unreachable!(),
                    };
                    self.stack.push(Value::Bool(b));
                }
                Op::And => {
                    let r = self.pop_bool(line, col)?;
                    let l = self.pop_bool(line, col)?;
                    self.stack.push(Value::Bool(l && r));
                }
                Op::Or => {
                    let r = self.pop_bool(line, col)?;
                    let l = self.pop_bool(line, col)?;
                    self.stack.push(Value::Bool(l || r));
                }
                Op::Not => {
                    let b = self.pop_bool(line, col)?;
                    self.stack.push(Value::Bool(!b));
                }
                Op::Neg => {
                    match self.pop(line, col)? {
                        Value::Number(n) => self.stack.push(Value::Number(-n)),
                        other => {
                            return Err(RioError::typ(
                                format!("السالب يتطلب عددا، وليس {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Jump(target) => {
                    self.pc = target;
                }
                Op::JumpIfFalse(target) => {
                    let b = self.pop_bool(line, col)?;
                    if !b {
                        self.pc = target;
                    }
                }
                Op::JumpIfTrue(target) => {
                    let b = self.pop_bool(line, col)?;
                    if b {
                        self.pc = target;
                    }
                }
                Op::Print(n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الطباعة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let vals: Vec<Value> = self.stack.drain(start..).collect();
                    let parts: Vec<String> = vals.iter().map(|v| v.fmt_value()).collect();
                    self.output.push_str(&parts.join(" "));
                    self.output.push('\n');
                }
                Op::BuildList(n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند بناء القائمة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let items: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::List(items));
                }
                Op::BuildDict(keys) => {
                    let n = keys.len();
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند بناء القاموس", line, col));
                    }
                    let start = self.stack.len() - n;
                    let vals: Vec<Value> = self.stack.drain(start..).collect();
                    // Duplicate keys: later wins, like the interpreter.
                    let mut pairs: Vec<(String, Value)> = Vec::new();
                    for (k, v) in keys.iter().zip(vals.into_iter()) {
                        if let Some(pair) = pairs.iter_mut().find(|(kk, _)| kk == k) {
                            pair.1 = v;
                        } else {
                            pairs.push((k.clone(), v));
                        }
                    }
                    self.stack.push(Value::Dict(pairs));
                }
                Op::Index => {
                    let idx = self.pop(line, col)?;
                    let target = self.pop(line, col)?;
                    let result = match (&target, &idx) {
                        (Value::List(items), Value::Number(x)) => {
                            if x.fract() != 0.0 || *x < 0.0 {
                                return Err(RioError::typ(
                                    "فهرس القائمة يجب أن يكون عددا صحيحا",
                                    line,
                                    col,
                                ));
                            }
                            let i = *x as usize;
                            items.get(i).cloned().ok_or_else(|| {
                                RioError::runtime(
                                    format!("فهرس خارج النطاق: {} والقائمة طولها {}", i, items.len()),
                                    line,
                                    col,
                                )
                            })?
                        }
                        (Value::Dict(pairs), Value::Str(key)) => pairs
                            .iter()
                            .find(|(k, _)| k == key)
                            .map(|(_, v)| v.clone())
                            .ok_or_else(|| {
                                RioError::runtime(format!("مفتاح غير موجود: {}", key), line, col)
                            })?,
                        (Value::Str(s), Value::Number(x)) => {
                            if x.fract() != 0.0 || *x < 0.0 {
                                return Err(RioError::typ(
                                    "فهرس النص يجب أن يكون عددا صحيحا",
                                    line,
                                    col,
                                ));
                            }
                            let chars: Vec<char> = s.chars().collect();
                            chars
                                .get(*x as usize)
                                .map(|c| Value::Str(c.to_string()))
                                .ok_or_else(|| {
                                    RioError::runtime("فهرس خارج النطاق في النص", line, col)
                                })?
                        }
                        _ => {
                            return Err(RioError::typ(
                                format!(
                                    "لا يمكن الفهرسة على نوع {} بمفتاح من نوع {}",
                                    target.type_name(),
                                    idx.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    };
                    self.stack.push(result);
                }
                Op::StoreIndex(name) => {
                    // Stack: [value, index] (value compiled first,
                    // matching the interpreter's evaluation order).
                    let idx = self.pop(line, col)?;
                    let value = self.pop(line, col)?;
                    let slot = self.vars.get_mut(&name).ok_or_else(|| {
                        RioError::runtime(format!("متغير غير معرف: {}", name), line, col)
                    })?;
                    match slot {
                        Value::List(items) => {
                            let i = match idx {
                                Value::Number(x) if x.fract() == 0.0 && x >= 0.0 => x as usize,
                                other => {
                                    return Err(RioError::typ(
                                        format!("فهرس القائمة يجب أن يكون عددا صحيحا، وليس {}", other.type_name()),
                                        line,
                                        col,
                                    ));
                                }
                            };
                            if i >= items.len() {
                                return Err(RioError::runtime(
                                    format!("فهرس خارج النطاق: {} والقائمة طولها {}", i, items.len()),
                                    line,
                                    col,
                                ));
                            }
                            items[i] = value;
                        }
                        Value::Dict(pairs) => {
                            let key = match idx {
                                Value::Str(s) => s,
                                other => {
                                    return Err(RioError::typ(
                                        format!("مفتاح القاموس يجب أن يكون نصا، وليس {}", other.type_name()),
                                        line,
                                        col,
                                    ));
                                }
                            };
                            if let Some(pair) = pairs.iter_mut().find(|(k, _)| *k == key) {
                                pair.1 = value;
                            } else {
                                pairs.push((key, value));
                            }
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("لا يمكن تعيين عنصر في نوع {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Return => {
                    let v = if self.stack.is_empty() {
                        Value::Nil
                    } else {
                        self.pop(line, col)?
                    };
                    self.return_value = Some(v);
                    return Ok(());
                }
                Op::Builtin(name, n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند استدعاء دالة مدمجة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    // Resolution order, like the interpreter: a
                    // variable of the same name wins over the
                    // builtin, and calling it is a type error.
                    if let Some(v) = self.vars.get(&name) {
                        return Err(RioError::typ(
                            format!("القيمة {} ليست دالة", v.type_name()),
                            line,
                            col,
                        ));
                    }
                    let result = self.call_pure_builtin(&name, args, line, col)?;
                    self.stack.push(result);
                }
                Op::Call(name, n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الاستدعاء", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    // Slice 5: a BoundMethod held in a variable
                    // (م = ك.طريقة) is callable, like in the
                    // interpreter. A plain function value held in
                    // a variable is NOT (first-class functions
                    // are still unsupported in the VM).
                    if !self.functions.contains_key(&name) {
                        if let Some(Value::BoundMethod { func, instance }) =
                            self.vars.get(&name).cloned()
                        {
                            let (mname, class_name) = match &*func {
                                Value::Function { name: mn, .. } => {
                                    (mn.clone(), instance.class.name.clone())
                                }
                                _ => {
                                    return Err(RioError::runtime(
                                        "طريقة مربوطة غير صالحة في VM",
                                        line,
                                        col,
                                    ));
                                }
                            };
                            let result = self.call_method_vm(
                                &class_name,
                                &mname,
                                instance,
                                args,
                                line,
                                col,
                            )?;
                            self.stack.push(result);
                            continue;
                        }
                        if let Some(v) = self.vars.get(&name) {
                            if matches!(v, Value::Function { .. }) {
                                return Err(RioError::parse(
                                    "VM: استدعاء دالة من متغير غير مدعوم بعد في --vm، استخدم المفسر الافتراضي",
                                    line,
                                    col,
                                ));
                            }
                            return Err(RioError::typ(
                                format!("القيمة {} ليست دالة", v.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                    let func = match self.functions.get(&name) {
                        Some(f) => f.clone(),
                        None => {
                            return Err(RioError::runtime(
                                format!("دالة غير معرفة في VM: {}", name),
                                line,
                                col,
                            ));
                        }
                    };
                    if func.params.len() != n {
                        return Err(RioError::runtime(
                            format!(
                                "الدالة {} تتوقع {} معاملات لكن أعطيت {}",
                                name,
                                func.params.len(),
                                n
                            ),
                            line,
                            col,
                        ));
                    }
                    // Same depth limit as the interpreter (Meeting 15).
                    if self.call_depth >= crate::interpreter::MAX_CALL_DEPTH {
                        return Err(RioError::runtime(
                            format!(
                                "تجاوز حد عمق الاستدعاء ({}) في VM",
                                crate::interpreter::MAX_CALL_DEPTH
                            ),
                            line,
                            col,
                        ));
                    }
                    let mut child_vm = self.child(&func, args);
                    match child_vm.run() {
                        Ok(()) => {
                            self.output.push_str(&child_vm.output);
                            let ret = child_vm.return_value.unwrap_or(Value::Nil);
                            self.stack.push(ret);
                        }
                        Err(e) => {
                            // Preserve partial output, like the interpreter.
                            self.output.push_str(&child_vm.output);
                            return Err(e);
                        }
                    }
                }
                Op::DefineClass(def) => {
                    // Base first (must already be defined), then
                    // interface compliance, like the interpreter.
                    // Methods are already flattened in def.
                    let base_data = match &def.base {
                        Some(bname) => match self.vars.get(bname) {
                            Some(Value::Class(data)) => Some(Rc::clone(data)),
                            Some(other) => {
                                return Err(RioError::typ(
                                    format!("{} ليست فئة، بل {}", bname, other.type_name()),
                                    line,
                                    col,
                                ));
                            }
                            None => {
                                return Err(RioError::runtime(
                                    format!("فئة أب غير معرفة: {}", bname),
                                    line,
                                    col,
                                ));
                            }
                        },
                        None => None,
                    };
                    for iname in &def.interfaces {
                        let iface = match self.vars.get(iname) {
                            Some(Value::Interface(data)) => Rc::clone(data),
                            Some(other) => {
                                return Err(RioError::typ(
                                    format!("{} ليست واجهة، بل {}", iname, other.type_name()),
                                    line,
                                    col,
                                ));
                            }
                            None => {
                                return Err(RioError::runtime(
                                    format!("واجهة غير معرفة: {}", iname),
                                    line,
                                    col,
                                ));
                            }
                        };
                        for (mname, arity) in &iface.methods {
                            match def.methods.get(mname) {
                                Some(f) => {
                                    if f.params.len() != *arity {
                                        return Err(RioError::runtime(
                                            format!(
                                                "الفئة {} تنفذ طريقة {} من واجهة {} بعدد معاملات غير صحيح: تتوقع {} لكن وجد {}",
                                                def.name, mname, iname, arity, f.params.len()
                                            ),
                                            line,
                                            col,
                                        ));
                                    }
                                }
                                None => {
                                    return Err(RioError::runtime(
                                        format!(
                                            "الفئة {} لا تنفذ طريقة {} المطلوبة من واجهة {}",
                                            def.name, mname, iname
                                        ),
                                        line,
                                        col,
                                    ));
                                }
                            }
                        }
                    }
                    // The VM looks methods up in its own classes
                    // table (flattened), so ClassData carries an
                    // empty methods map; its name/base are used
                    // in error messages, like the interpreter's.
                    let class_data = Rc::new(crate::value::ClassData {
                        name: def.name.clone(),
                        methods: HashMap::new(),
                        base: base_data,
                        interfaces: def.interfaces.clone(),
                    });
                    self.vars
                        .insert(def.name.clone(), Value::Class(class_data));
                    self.classes.insert(def.name.clone(), def);
                }
                Op::DefineInterface(name, methods) => {
                    let iface = Value::Interface(Rc::new(crate::value::InterfaceData {
                        name: name.clone(),
                        methods: methods.clone(),
                    }));
                    self.vars.insert(name.clone(), iface);
                }
                Op::Dot(name) => {
                    let target = self.pop(line, col)?;
                    match &target {
                        Value::Instance(data) => {
                            // Field first, then method (bound to
                            // this instance). A field shadowing a
                            // method name wins, like the
                            // interpreter.
                            if let Some(v) = data.fields.borrow().get(&name) {
                                self.stack.push(v.clone());
                            } else if let Some(f) = self
                                .classes
                                .get(&data.class.name)
                                .and_then(|c| c.methods.get(&name))
                            {
                                // Synthetic Function value: only a
                                // carrier for the method name and
                                // params; the VM runs bytecode.
                                let func = Value::Function {
                                    name: name.clone(),
                                    params: f.params.clone(),
                                    body: Vec::new(),
                                    module: None,
                                    param_types: Vec::new(),
                                    return_type: None,
                                    captured: None,
                                };
                                self.stack.push(Value::BoundMethod {
                                    func: Box::new(func),
                                    instance: Rc::clone(data),
                                });
                            } else {
                                return Err(RioError::runtime(
                                    format!(
                                        "الكائن من فئة {} ليس له حقل أو طريقة باسم {}",
                                        data.class.name, name
                                    ),
                                    line,
                                    col,
                                ));
                            }
                        }
                        other => {
                            return Err(RioError::typ(
                                format!(
                                    "الوصول بالنقطة مدعوم على الوحدات والكائنات فقط، وليس على نوع {}",
                                    other.type_name()
                                ),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::StoreDot(name) => {
                    // Stack: [value, target] (value compiled
                    // first, matching the interpreter).
                    let target = self.pop(line, col)?;
                    let value = self.pop(line, col)?;
                    match target {
                        Value::Instance(data) => {
                            data.fields.borrow_mut().insert(name, value);
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("لا يمكن تعيين حقل في نوع {}", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
                Op::Create(class_name, n) => {
                    if self.stack.len() < n {
                        return Err(RioError::runtime("مكدس VM فارغ عند الإنشاء", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    let class_data = match self.vars.get(&class_name) {
                        Some(Value::Class(data)) => Rc::clone(data),
                        Some(other) => {
                            return Err(RioError::typ(
                                format!("{} ليست فئة، بل {}", class_name, other.type_name()),
                                line,
                                col,
                            ));
                        }
                        None => {
                            return Err(RioError::runtime(
                                format!("فئة غير معرفة: {}", class_name),
                                line,
                                col,
                            ));
                        }
                    };
                    let instance = Rc::new(crate::value::InstanceData {
                        class: Rc::clone(&class_data),
                        fields: std::cell::RefCell::new(HashMap::new()),
                    });
                    // Constructor: a method named أنشئ, if present
                    // (own or inherited via the flattened table),
                    // runs automatically with نفس bound.
                    let has_ctor = self
                        .classes
                        .get(&class_name)
                        .map(|c| c.methods.contains_key("أنشئ"))
                        .unwrap_or(false);
                    if has_ctor {
                        let result = self.call_method_vm(
                            &class_name,
                            "أنشئ",
                            Rc::clone(&instance),
                            args,
                            line,
                            col,
                        );
                        // The constructor's return value is
                        // discarded, like in the interpreter.
                        let _ = result?;
                    } else if !args.is_empty() {
                        return Err(RioError::runtime(
                            format!(
                                "الفئة {} ليس لها منشئ أنشئ، لكن أعطيت {} معاملات",
                                class_data.name,
                                args.len()
                            ),
                            line,
                            col,
                        ));
                    }
                    self.stack.push(Value::Instance(instance));
                }
                Op::CallBound(n) => {
                    if self.stack.len() < n + 1 {
                        return Err(RioError::runtime("مكدس VM فارغ عند استدعاء طريقة", line, col));
                    }
                    let start = self.stack.len() - n;
                    let args: Vec<Value> = self.stack.drain(start..).collect();
                    let bound = self.pop(line, col)?;
                    match bound {
                        Value::BoundMethod { func, instance } => {
                            let (mname, class_name) = match &*func {
                                Value::Function { name: mn, .. } => {
                                    (mn.clone(), instance.class.name.clone())
                                }
                                _ => {
                                    return Err(RioError::runtime(
                                        "طريقة مربوطة غير صالحة في VM",
                                        line,
                                        col,
                                    ));
                                }
                            };
                            let result = self.call_method_vm(
                                &class_name,
                                &mname,
                                instance,
                                args,
                                line,
                                col,
                            )?;
                            self.stack.push(result);
                        }
                        other => {
                            return Err(RioError::typ(
                                format!("القيمة {} ليست طريقة مربوطة", other.type_name()),
                                line,
                                col,
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;

    fn run_vm_src(src: &str) -> (String, String, bool) {
        let tokens = match tokenize(src) {
            Ok(t) => t,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let program = match Parser::new(tokens).parse_program() {
            Ok(p) => p,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let compiled = match Compiler::compile_program(&program) {
            Ok(c) => c,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let mut vm = Vm::new(compiled.code, compiled.functions);
        match vm.run() {
            Ok(()) => (vm.output, String::new(), true),
            Err(e) => (vm.output, e.to_string(), false),
        }
    }

    #[test]
    fn vm_arithmetic_and_variables() {
        let (out, _, ok) = run_vm_src("س = ٣ + ٤ * ٢\nاطبع س\nاطبع س * ٢\n");
        assert!(ok);
        assert_eq!(out, "١١\n٢٢\n");
    }

    #[test]
    fn vm_functions_and_recursion() {
        let (out, _, ok) = run_vm_src(
            "دالة جمع(أ، ب):\n    ارجع أ + ب\nدالة عاملي(ن):\n    إذا ن <= ١:\n        ارجع ١\n    ارجع ن * عاملي(ن - ١)\nاطبع جمع(٢، ٣)\nاطبع عاملي(٥)\n",
        );
        assert!(ok);
        assert_eq!(out, "٥\n١٢٠\n");
    }

    #[test]
    fn vm_lists_dicts_index_and_assignment() {
        let (out, _, ok) = run_vm_src(
            "قائمة = [١، ٢، ٣]\nاطبع قائمة[٠]\nقائمة[١] = ٩٩\nاطبع قائمة\nقاموس = {\"اسم\": \"محمود\"، \"عمر\": ٢٣}\nاطبع قاموس[\"اسم\"]\nقاموس[\"مدينة\"] = \"القاهرة\"\nاطبع قاموس[\"مدينة\"]\nاطبع \"محمود\"[٠]\n",
        );
        assert!(ok);
        assert_eq!(out, "١\n[١, ٩٩, ٣]\nمحمود\nالقاهرة\nم\n");
    }

    #[test]
    fn vm_list_copy_semantics_match_interpreter() {
        // ب = قائمة copies (value type), so mutating ب must
        // NOT affect قائمة; only قائمة[..] = .. mutates it.
        let (out, _, ok) = run_vm_src(
            "قائمة = [١، ٢]\nب = قائمة\nب[٠] = ٩٩\nاطبع قائمة\nاطبع ب\n",
        );
        assert!(ok);
        assert_eq!(out, "[١, ٢]\n[٩٩, ٢]\n");
    }

    #[test]
    fn vm_index_out_of_range_is_arabic_error() {
        let (out, err, ok) = run_vm_src("قائمة = [١]\nاطبع قائمة[٥]\n");
        assert!(!ok);
        assert!(err.contains("فهرس خارج النطاق"));
        assert_eq!(out, "");
    }

    #[test]
    fn vm_pure_builtins_match_interpreter() {
        let (out, _, ok) = run_vm_src(
            "قائمة = [٣، ١، ٢]\nاطبع طول(قائمة)\nاطبع مجموع(قائمة)\nاطبع أقصى(قائمة)\nاطبع أدنى(قائمة)\nاطبع مطلق(-٥)\nاطبع عدد(\"٤٢\")\nاطبع نص(٥)\nاطبع نوع(قائمة)\nاطبع مدى(٣)\nاطبع مدى(١، ٥، ٢)\nاطبع مدى(٥، ٠، -٢)\nاطبع طول(\"مرحبا\")\n",
        );
        assert!(ok);
        assert_eq!(
            out,
            "٣\n٦\n٣\n١\n٥\n٤٢\n٥\nقائمة\n[٠, ١, ٢]\n[١, ٣]\n[٥, ٣, ١]\n٥\n"
        );
    }

    #[test]
    fn vm_builtin_arity_and_type_errors() {
        let (_, err, ok) = run_vm_src("اطبع طول()\n");
        assert!(!ok);
        assert!(err.contains("تتوقع 1 معاملات"));
        let (_, err2, ok2) = run_vm_src("اطبع مجموع([١، \"أ\"])\n");
        assert!(!ok2);
        assert!(err2.contains("مجموع يتطلب قائمة أعداد"));
    }

    #[test]
    fn vm_user_function_shadows_builtin() {
        // A user function named طول wins over the builtin,
        // like in the interpreter.
        let (out, _, ok) = run_vm_src(
            "دالة طول(س):\n    ارجع ٩٩\nاطبع طول([١، ٢])\n",
        );
        assert!(ok);
        assert_eq!(out, "٩٩\n");
    }

    #[test]
    fn vm_variable_shadows_builtin() {
        // A variable named طول wins at runtime, and calling
        // it is a type error, like in the interpreter.
        let (_, err, ok) = run_vm_src("طول = ٥\nاطبع طول([١])\n");
        assert!(!ok);
        assert!(err.contains("ليست دالة"));
    }

    #[test]
    fn vm_classes_fields_and_methods() {
        let (out, _, ok) = run_vm_src(
            "فئة نقطة:\n    دالة أنشئ(س، ص):\n        نفس.س = س\n        نفس.ص = ص\n    دالة مجموع():\n        ارجع نفس.س + نفس.ص\n    دالة زد(مقدار):\n        نفس.س = نفس.س + مقدار\n        ارجع نفس.س\nك = أنشئ نقطة(٣، ٤)\nاطبع ك.س\nاطبع ك.مجموع()\nاطبع ك.زد(١٠)\nاطبع ك.س\n",
        );
        assert!(ok);
        assert_eq!(out, "٣\n٧\n١٣\n١٣\n");
    }

    #[test]
    fn vm_classes_inheritance_and_interfaces() {
        let (out, _, ok) = run_vm_src(
            "واجهة قابل_للطباعة:\n    دالة اطبع_نفسك():\n        مرر\nفئة شخص:\n    دالة أنشئ(اسم):\n        نفس.اسم = اسم\n    دالة عرّف():\n        ارجع نفس.اسم\n    دالة اطبع_نفسك():\n        اطبع نفس.اسم\nفئة طالب ورث شخص واجهة قابل_للطباعة:\n    دالة ادرس():\n        ارجع \"يدرس\"\n    دالة عرّف():\n        ارجع \"طالب: \" + نفس.اسم\nط = أنشئ طالب(\"محمود\")\nاطبع ط.عرّف()\nاطبع ط.ادرس()\nط.اطبع_نفسك()\n",
        );
        assert!(ok);
        assert_eq!(out, "طالب: محمود\nيدرس\nمحمود\n");
    }

    #[test]
    fn vm_class_field_shadows_method_and_bound_method_variable() {
        let (out, _, ok) = run_vm_src(
            "فئة صندوق:\n    دالة قيمة():\n        ارجع ٧\nص = أنشئ صندوق()\nص.قيمة = ٩٩\nاطبع ص.قيمة\nم = ص.قيمة\n",
        );
        assert!(ok);
        // Field shadows the method: ص.قيمة is data, not a method.
        assert_eq!(out, "٩٩\n");
        let (out2, _, ok2) = run_vm_src(
            "فئة صندوق:\n    دالة قيمة():\n        ارجع ٧\nص = أنشئ صندوق()\nم = ص.قيمة\nاطبع م()\n",
        );
        assert!(ok2);
        assert_eq!(out2, "٧\n");
    }

    #[test]
    fn vm_class_missing_interface_method_is_error() {
        // Note: و is the AND keyword, so it cannot be an
        // interface name; use a plain name here.
        let (_, err, ok) = run_vm_src(
            "واجهة قابلة:\n    دالة طريقة():\n        مرر\nفئة ك واجهة قابلة:\n    دالة غيرها():\n        ارجع ١\n",
        );
        assert!(!ok);
        assert!(err.contains("لا تنفذ طريقة طريقة المطلوبة"));
    }

    #[test]
    fn vm_function_locals_do_not_leak() {
        let (out, err, ok) = run_vm_src(
            "دالة زد(س):\n    نتيجة = س + ١\n    ارجع نتيجة\nاطبع زد(٤)\nاطبع نتيجة\n",
        );
        assert!(!ok, "local must not leak, got out={} err={}", out, err);
        assert!(err.contains("غير معرف"));
    }

    #[test]
    fn vm_if_and_while() {
        let (out, _, ok) = run_vm_src(
            "س = ٠\nبينما س < ٣:\n    س = س + ١\nاطبع س\nإذا س == ٣:\n    اطبع \"نعم\"\nوإلا:\n    اطبع \"لا\"\n",
        );
        assert!(ok);
        assert_eq!(out, "٣\nنعم\n");
    }
}
