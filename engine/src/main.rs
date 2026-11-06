//! Niva engine CLI: `نيفا برنامج.نيفا`
//! Compiler (bytecode VM) by default since phase ك-2
//! (Meeting 25, task 7): the source compiles once to bytecode
//! and runs on the VM; the tree-walking interpreter is only a
//! hidden diagnostic mode (`--مفسر`), never documented as a
//! way to run programs. The produced binary runs alone:
//! it needs no Python installation and no runtime other than itself.
//!
//! Usage (all Arabic, no exceptions — the installed command is نيفا;
//! cargo still builds the binary as `niva` internally, because cargo
//! bin names are ASCII-only; install.sh copies it to نيفا):
//!   نيفا                      enter the interactive REPL (on the VM)
//!   نيفا --تفاعلي            enter the interactive REPL
//!   نيفا برنامج.نيفا          run a .نيفا program (on the VM)
//!   نيفا برنامج.نيفا --شجرة   print the AST, then run
//!   نيفا --مساعدة             usage
//!   نيفا --نسخة               version
//!
//! The engine itself lives in lib.rs; this file is only the CLI
//! (flags, file reading, the 256 MB stack thread, exit codes).

use niva_engine::interpreter::Interpreter;
use niva_engine::semantic::SemanticAnalyzer;
use niva_engine::lexer::tokenize;
use niva_engine::parser::Parser;
use niva_engine::{repl, vm};
use std::env;
use std::fs;
use std::process::ExitCode;

// Meeting 15: the interpreter/VM thread stack. Measured ceilings
// on a 64 MB stack (cap lifted): debug-extreme ~176, release
// ~1000+. At 256 MB the ceilings scale ~4x, so the 500 depth
// cap below holds a ~30% margin on the worst measured shape.
const STACK_SIZE_BYTES: usize = 256 * 1024 * 1024;

fn usage() -> String {
    "استخدام: نيفا [برنامج.نيفا] [--شجرة] [--اسمح-بالكل] [--اسمح-بالشبكة] [--مضيف-محلي مضيف]\n\
     نيفا                  دخول الوضع التفاعلي\n\
     نيفا --تفاعلي        دخول الوضع التفاعلي\n\
     نيفا --مساعدة         عرض هذه المساعدة\n\
     نيفا --نسخة           عرض الإصدار\n\
     --شجرة                عرض الشجرة النحوية ثم التشغيل\n\
     --اسمح-بالكل         السماح بملفات خارج مجلد البرنامج (للموثوقة فقط)\n\
     --اسمح-بالشبكة       السماح بالشبكة (شبكة.اجلب) — مغلقة افتراضيا\n\
     --مضيف-محلي مضيف       السماح بمضيف محلي صراحة (مثل 127.0.0.1) — وحده لا يفتح الشبكة\n"
        .to_string()
}

/// رسالة خطأ إدخال/إخراج بالعربي، بدلا من رسالة نظام التشغيل الإنجليزية.
fn io_error_arabic(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => "الملف أو المجلد غير موجود".to_string(),
        PermissionDenied => "تم رفض الإذن".to_string(),
        AlreadyExists => "الملف موجود بالفعل".to_string(),
        InvalidInput => "إدخال غير صالح".to_string(),
        // Any other OS error: Arabic only — the raw OS message is
        // English, so it is never shown to the user.
        _ => "خطأ نظام غير معروف".to_string(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--مساعدة") {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--نسخة") {
        println!("محرك نيفا ١٫٠٫٠");
        return ExitCode::SUCCESS;
    }
    // خيار غير معروف: خطأ عربي واضح بدلا من تجاهله بصمت.
    // Arabic flags only — a Latin flag is rejected here.
    // --مفسر is a HIDDEN diagnostic flag (DESIGN §39.3):
    // known to the parser, but never shown in --مساعدة.
    const KNOWN_FLAGS: [&str; 8] = [
        "--مساعدة",
        "--نسخة",
        "--تفاعلي",
        "--مفسر",
        "--شجرة",
        "--اسمح-بالكل",
        "--اسمح-بالشبكة",
        "--مضيف-محلي",
    ];
    // ك-2 (مهمة 7): --افتراضي أُزيل — الآلة الافتراضية هي
    // المحرك الافتراضي الآن، فلا معنى للعلم. خطأ عربي واضح
    // يقول ذلك، بدلا من «خيار غير معروف» المضللة.
    if args.iter().any(|a| a == "--افتراضي") {
        eprintln!("الآلة الافتراضية هي المحرك الافتراضي الآن — لا حاجة إلى --افتراضي");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .find(|a| a.starts_with('-') && !KNOWN_FLAGS.contains(&a.as_str()))
    {
        eprintln!("خيار غير معروف: {}", unknown);
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    // Meeting 22: network is OFF by default. --اسمح-بالشبكة
    // opens it; --مضيف-محلي HOST (repeatable) explicitly
    // authorizes a local host (the only way a loopback/private
    // address can be fetched — see network.rs). The local-host
    // flag alone does NOT open the network.
    let allow_network = args.iter().any(|a| a == "--اسمح-بالشبكة");
    let mut local_hosts: Vec<String> = Vec::new();
    let mut arg_iter = args.iter().skip(1).peekable();
    let mut file_args: Vec<String> = Vec::new();
    while let Some(a) = arg_iter.next() {
        if a == "--مضيف-محلي" {
            match arg_iter.next() {
                Some(host) => local_hosts.push(host.clone()),
                None => {
                    eprintln!("خيار --مضيف-محلي يحتاج مضيفا بعده");
                    eprint!("{}", usage());
                    return ExitCode::FAILURE;
                }
            }
        } else if !a.starts_with('-') {
            file_args.push(a.clone());
        }
    }
    let file_arg = file_args.first().cloned();
    // No file path given (no args, or only flags) -> interactive REPL.
    // --تفاعلي is explicit; the REPL runs on the compiler (VM) by
    // default, or on the interpreter with --مفسر (diagnostic only).
    let allow_all = args.iter().any(|a| a == "--اسمح-بالكل");
    // ك-2 (مهمة 7): --مفسر علم تشخيص خفي — مفسر الشجرة
    // للمقارنة والتشخيص فقط، ولا يظهر في --مساعدة.
    let use_interpreter = args.iter().any(|a| a == "--مفسر");
    if file_arg.is_none() {
        // Same 256 MB stack thread as file mode, so the depth-500
        // recursion limit is a Niva error, not a Rust stack overflow.
        let handle = std::thread::Builder::new()
            .stack_size(STACK_SIZE_BYTES)
            .spawn(move || repl::run_repl(allow_all, allow_network, &local_hosts, use_interpreter));
        return match handle {
            Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
            Err(_) => ExitCode::FAILURE,
        };
    }
    let path = file_arg.unwrap();
    let show_ast = args.iter().any(|a| a == "--شجرة");
    // Meeting 4: trusted file mode, explicit flag only.
    // Default is restricted to the program directory.
    // (allow_all was already read above, before the REPL branch.)

    let source = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("خطأ إدخال/إخراج: تعذر قراءة الملف {}: {}", path, io_error_arabic(&e));
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
    // large stack (256 MB, Meeting 15), so Niva's own call-depth
    // limit (500) is enforced by a Niva error, not by a Rust stack
    // overflow, in BOTH debug and release builds. Debug frames grew
    // with the class/interface AST variants, and depth-100 recursion
    // that previously passed in debug began overflowing the default
    // 8 MB main stack. This restores the original guarantee.
    let program_dir = std::path::Path::new(&path)
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
        .stack_size(STACK_SIZE_BYTES)
        .spawn(move || {
            // Phase 1A Step 1: semantic analysis BEFORE execution. If it
            // fails, the program never runs, so no side effect (print, file
            // write) happens before the error.
            let mut analyzer = SemanticAnalyzer::new();
            if let Err(e) = analyzer.analyze_program(&program) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
            // ك-2 (مهمة 7): --مفسر فقط يشغّل مفسر الشجرة
            // (تشخيص ومقارنة)؛ الافتراضي هو المترجم (VM).
            if use_interpreter {
                // Phase 1A Step 2: modules are resolved relative to the main
                // program's directory, not the current working directory.
                let mut interp = Interpreter::with_program_dir(program_dir);
                // Meeting 4: trusted mode is opt-in only.
                interp.set_trusted(allow_all);
                // Meeting 22: network is opt-in only, independent
                // of file trust (--اسمح-بالكل does not open it).
                interp.set_network_allowed(allow_network);
                for host in &local_hosts {
                    interp.allow_local_network_host(host);
                }
                return match interp.run(&program) {
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
                };
            }
            // Default engine: the compiler (VM). The source
            // compiles once to bytecode, then runs.
            // مهمة 4 (ك-1.3): بوابة الشبكة عملية-واسعة؛
            // مسار VM يضبطها كما يضبطها مسار المفسر
            // تماما، فيتصرف شبكة.اجلب في الوضعين
            // بالطريقة نفسها (مغلقة افتراضيا، وتُفتح
            // بـ --اسمح-بالشبكة، مع --مضيف-محلي).
            niva_engine::network::set_network_allowed(allow_network);
            for host in &local_hosts {
                niva_engine::network::allow_local_host(host);
            }
            let compiled = match vm::Compiler::compile_program(&program) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    return ExitCode::FAILURE;
                }
            };
            let mut vm_inst = vm::Vm::new(compiled, program_dir.clone());
            // مهمة 5 (ك-1.4): تمرير --اسمح-بالكل إلى VM
            // كما يُمرَّر إلى المفسر، فتنطبق سياسة
            // الملفات نفسها (مقيّدة افتراضيا بمجلد البرنامج).
            vm_inst.set_trusted(allow_all);
            match vm_inst.run() {
                Ok(()) => {
                    print!("{}", vm_inst.output);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    print!("{}", vm_inst.output);
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
