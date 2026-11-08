//! Niva engine as a library.
//!
//! The binary (main.rs) is a thin CLI over this: it reads a file
//! and prints the result. The WASM playground calls `run_source`
//! directly with source text, no filesystem involved.
//!
//! `run_source` follows the CLI path exactly: tokenize, parse,
//! semantic analysis BEFORE any execution, then compile once
//! and run on the bytecode VM (the default engine since
//! phase ك-2, task 7). It returns the printed output plus
//! the result; partial output is returned even on error, like
//! the CLI (which prints partial output, then the Arabic error).

pub mod artifact;
pub mod async_io;
pub mod ast;
pub mod cgen;
pub mod crypto;
pub mod device;
pub mod env;
pub mod error;
pub mod files;
pub mod install;
pub mod interpreter;
pub mod ipc;
pub mod lexer;
pub mod memory;
pub mod network;
pub mod packages;
pub mod parser;
pub mod permissions;
pub mod processes;
pub mod repl;
pub mod security;
pub mod stdin_pump;
pub mod semantic;
pub mod sockets;
pub mod stdlib;
pub mod system;
pub mod time;
pub mod value;
pub mod vm;

/// Run Niva source text on the compiler (bytecode VM) — the
/// default engine since phase ك-2 (task 7). The program
/// directory is empty: in the browser there is no filesystem,
/// so file modules resolve to nothing and the file builtins
/// are gated off on wasm32 (see `call_builtin`).
/// Network is OFF for a one-shot run: there is no flag here,
/// so the gate is reset explicitly — a previous console
/// session's permission never leaks into a later run_source
/// call in the same (WASM) instance.
pub fn run_source(source: &str) -> (String, Result<(), error::RioError>) {
    network::set_network_allowed(false);
    // مهمة 10: سياسة العمليات تُصفَّر صراحة أيضا — مجلد
    // برنامج فارغ يرفض كل تشغيل (لا تسرب من جلسة سابقة
    // في النسخة نفسها).
    processes::set_process_policy(std::path::PathBuf::new(), false, std::path::PathBuf::new());
    let toks = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => return (String::new(), Err(e)),
    };
    let program = match parser::Parser::new(toks).parse_program() {
        Ok(p) => p,
        Err(e) => return (String::new(), Err(e)),
    };
    let mut analyzer = semantic::SemanticAnalyzer::new();
    if let Err(e) = analyzer.analyze_program(&program) {
        return (String::new(), Err(e));
    }
    let compiled = match vm::Compiler::compile_program(&program) {
        Ok(c) => c,
        Err(e) => return (String::new(), Err(e)),
    };
    let mut vm_inst = vm::Vm::new(compiled, std::path::PathBuf::new());
    match vm_inst.run() {
        Ok(()) => (vm_inst.output.clone(), Ok(())),
        Err(e) => (vm_inst.output.clone(), Err(e)),
    }
}

/// Diagnostic twin of `run_source`: the same source through
/// the tree-walking interpreter, for comparison and diagnosis
/// only — never a way to run programs (mirrors the CLI's
/// hidden `--مفسر`).
pub fn run_source_interpreter(source: &str) -> (String, Result<(), error::RioError>) {
    network::set_network_allowed(false);
    processes::set_process_policy(std::path::PathBuf::new(), false, std::path::PathBuf::new());
    let toks = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => return (String::new(), Err(e)),
    };
    let program = match parser::Parser::new(toks).parse_program() {
        Ok(p) => p,
        Err(e) => return (String::new(), Err(e)),
    };
    let mut analyzer = semantic::SemanticAnalyzer::new();
    if let Err(e) = analyzer.analyze_program(&program) {
        return (String::new(), Err(e));
    }
    let mut interp =
        interpreter::Interpreter::with_program_dir(std::path::PathBuf::new());
    match interp.run(&program) {
        Ok(()) => (interp.output.clone(), Ok(())),
        Err(e) => (interp.output.clone(), Err(e)),
    }
}
