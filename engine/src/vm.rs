//! Phase 1B slice 1: Bytecode VM (experimental, --vm flag only).
//!
//! This is NOT yet the default execution path. The tree-walking
//! interpreter remains default; the VM is a separate, testable
//! slice for core statements and expressions, to be expanded
//! in later slices (functions, classes, modules, GC) before it
//! ever replaces the interpreter.
//!
//! Design:
//! - Compiler: AST Stmt/Expr -> Vec<Instr>, with jump patching
//!   for إذا / بينما, and short-circuit jumps for و / أو.
//! - VM: stack machine, pc, variables map, output string.
//! - Supported in slice 1: numbers, strings, booleans, لاشيء,
//!   variables, arithmetic, comparisons, و / أو (short-circuit),
//!   ليس, سالب, تعيين متغير, اطبع, إذا / وإلا إذا / وإلا,
//!   بينما, جملة تعبير (with Pop).
//! - NOT supported in slice 1 (compile error, clear message):
//!   lists, dicts, index, dot, create, calls, functions, classes,
//!   modules, imports, exports, for, repeat, try, return, break,
//!   continue. Those remain tree-walking only for now.
//! - GC in slice 1: none beyond Rust ownership + Rc for values
//!   already in Value. A tracing GC is a later 1B slice, after
//!   the VM heap exists. This slice does NOT claim a GC.

use crate::ast::{BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{RioError, RioResult};
use crate::value::Value;
use std::collections::HashMap;

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
}

#[derive(Debug, Clone)]
pub struct Instr {
    pub op: Op,
    pub line: usize,
    pub col: usize,
}

pub struct Compiler {
    pub code: Vec<Instr>,
}

impl Compiler {
    pub fn new() -> Self {
        Self { code: Vec::new() }
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

    pub fn compile_program(program: &[Stmt]) -> RioResult<Vec<Instr>> {
        let mut c = Self::new();
        for stmt in program {
            c.compile_stmt(stmt)?;
        }
        Ok(c.code)
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            StmtKind::Assign { target, value, annotation: _ } => {
                // Slice 1: variable assignment only.
                match target {
                    crate::ast::AssignTarget::Var(name) => {
                        self.compile_expr(value)?;
                        self.emit(Op::Store(name.clone()), stmt.line, stmt.col);
                        Ok(())
                    }
                    _ => Err(RioError::parse(
                        "VM slice 1: تعيين الفهرسة والنقطة غير مدعوم بعد",
                        stmt.line,
                        stmt.col,
                    )),
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
            _ => Err(RioError::parse(
                "VM slice 1: هذه الجملة غير مدعومة بعد في --vm، استخدم المفسر الافتراضي",
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
                            "VM slice 1: في غير مدعومة بعد",
                            expr.line,
                            expr.col,
                        ));
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.emit(op, expr.line, expr.col);
                Ok(())
            }
            _ => Err(RioError::parse(
                "VM slice 1: هذا التعبير غير مدعوم بعد في --vm، استخدم المفسر الافتراضي",
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
}

impl Vm {
    pub fn new(code: Vec<Instr>) -> Self {
        Self { code, pc: 0, stack: Vec::new(), vars: HashMap::new(), output: String::new() }
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
        let code = match Compiler::compile_program(&program) {
            Ok(c) => c,
            Err(e) => return (String::new(), e.to_string(), false),
        };
        let mut vm = Vm::new(code);
        match vm.run() {
            Ok(()) => (vm.output, String::new(), true),
            Err(e) => (vm.output, e.to_string(), false),
        }
    }

    #[test]
    fn vm_arithmetic_and_variables() {
        let (out, _, ok) = run_vm_src("س = ٣ + ٤ * ٢\nاطبع س\nاطبع س * ٢\n");
        assert!(ok);
        assert_eq!(out, "11\n22\n");
    }

    #[test]
    fn vm_if_and_while() {
        let (out, _, ok) = run_vm_src(
            "س = ٠\nبينما س < ٣:\n    س = س + ١\nاطبع س\nإذا س == ٣:\n    اطبع \"نعم\"\nوإلا:\n    اطبع \"لا\"\n",
        );
        assert!(ok);
        assert_eq!(out, "3\nنعم\n");
    }
}
