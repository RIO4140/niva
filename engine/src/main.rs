//! Rio engine CLI: `rio program.rio`
//! Tree-walking interpreter written in Rust. No Python, no transpilation,
//! no machine-code compiler in v1. The produced binary runs alone:
//! it needs no Python installation and no runtime other than itself.
//!
//! Usage:
//!   rio program.rio          run a .rio program
//!   rio program.rio --ast    print the AST, then run
//!   rio --help               usage
//!   rio --version            version

mod ast;
mod error;
mod interpreter;
mod lexer;
mod parser;
mod permissions;
mod semantic;
mod value;
mod vm;

use interpreter::Interpreter;
use semantic::SemanticAnalyzer;
use lexer::tokenize;
use parser::Parser;
use std::env;
use std::fs;
use std::process::ExitCode;

fn usage() -> String {
    "استخدام: rio program.rio [--ast]\n\
     rio --help     عرض هذه المساعدة\n\
     rio --version  عرض الإصدار\n"
        .to_string()
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Rio engine 1.0.0 (Rust, Tree-Walking Interpreter, no Python)");
        return ExitCode::SUCCESS;
    }
    if args.len() < 2 {
        eprint!("{}", usage());
        return ExitCode::from(2);
    }
    let path = &args[1];
    let show_ast = args.iter().any(|a| a == "--ast");
    // Phase 1B slice 1: experimental VM, --vm flag only.
    // Default remains the tree-walking interpreter.
    let use_vm = args.iter().any(|a| a == "--vm");
    // Meeting 4: trusted file mode, explicit flag only.
    // Default is restricted to the program directory.
    let allow_all = args.iter().any(|a| a == "--allow-all");

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("خطأ إدخال/إخراج: تعذر قراءة الملف {}: {}", path, e);
            return ExitCode::FAILURE;
        }
    };

    let tokens = match tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
    };
    let program = match Parser::new(tokens).parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
    };

    if show_ast {
        println!("{:#?}", program);
    }

    // Phase 1A Step 5 fix: run the interpreter in a thread with a
    // large stack (64 MB), so Rio's own call-depth limit (200) is
    // enforced by a Rio error, not by a Rust stack overflow, in
    // BOTH debug and release builds. Debug frames grew with the
    // class/interface AST variants, and depth-100 recursion that
    // previously passed in debug began overflowing the default
    // 8 MB main stack. This restores the original guarantee.
    let program_dir = std::path::Path::new(path)
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            } else if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(p)
            }
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            // Phase 1A Step 1: semantic analysis BEFORE execution. If it
            // fails, the program never runs, so no side effect (print, file
            // write) happens before the error.
            let mut analyzer = SemanticAnalyzer::new();
            if let Err(e) = analyzer.analyze_program(&program) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
            // Phase 1B slice 1: experimental VM path, --vm only.
            if use_vm {
                let code = match vm::Compiler::compile_program(&program) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("{}", e);
                        return ExitCode::FAILURE;
                    }
                };
                let mut vm_inst = vm::Vm::new(code);
                return match vm_inst.run() {
                    Ok(()) => {
                        print!("{}", vm_inst.output);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        print!("{}", vm_inst.output);
                        eprintln!("{}", e);
                        ExitCode::FAILURE
                    }
                };
            }
            // Phase 1A Step 2: modules are resolved relative to the main
            // program's directory, not the current working directory.
            let mut interp = Interpreter::with_program_dir(program_dir);
            // Meeting 4: trusted mode is opt-in only.
            interp.set_trusted(allow_all);
            match interp.run(&program) {
                Ok(()) => {
                    print!("{}", interp.output);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    // Partial output first, then the Arabic error with line/col.
                    print!("{}", interp.output);
                    eprintln!("{}", e);
                    ExitCode::FAILURE
                }
            }
        });
    match handle {
        Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => ExitCode::FAILURE,
    }
}
