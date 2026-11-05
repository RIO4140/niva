//! Rio4140 engine CLI: `rio program.rio`
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
mod value;

use interpreter::Interpreter;
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
        println!("Rio4140 engine 1.0.0 (Rust, Tree-Walking Interpreter, no Python)");
        return ExitCode::SUCCESS;
    }
    if args.len() < 2 {
        eprint!("{}", usage());
        return ExitCode::from(2);
    }
    let path = &args[1];
    let show_ast = args.iter().any(|a| a == "--ast");

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

    let mut interp = Interpreter::new();
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
}
