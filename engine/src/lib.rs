//! Niva engine as a library.
//!
//! The binary (main.rs) is a thin CLI over this: it reads a file
//! and prints the result. The WASM playground calls `run_source`
//! directly with source text, no filesystem involved.
//!
//! `run_source` follows the CLI path exactly: tokenize, parse,
//! semantic analysis BEFORE any execution, then run the
//! tree-walking interpreter. It returns the printed output plus
//! the result; partial output is returned even on error, like
//! the CLI (which prints partial output, then the Arabic error).

pub mod ast;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod network;
pub mod parser;
pub mod permissions;
pub mod repl;
pub mod semantic;
pub mod stdlib;
pub mod value;
pub mod vm;

/// Run Niva source text with the interpreter (never the VM).
/// The program directory is empty: in the browser there is no
/// filesystem, so file modules resolve to nothing and the file
/// builtins are gated off on wasm32 (see `call_builtin`).
/// Network is OFF for a one-shot run: there is no flag here,
/// so the gate is reset explicitly — a previous console
/// session's permission never leaks into a later run_source
/// call in the same (WASM) instance.
pub fn run_source(source: &str) -> (String, Result<(), error::RioError>) {
    network::set_network_allowed(false);
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
