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
use niva_engine::lexer::{normalize_cli_word, tokenize};
use niva_engine::parser::Parser;
use niva_engine::{artifact, cgen, repl, vm};
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
     نيفا ابن برنامج.نيفا  بناء مصنوع بايت كود محمول (.نيفاب)\n\
     نيفا ابن --مستقل برنامج.نيفا   بناء ملف تنفيذي مستقل يعمل وحده\n\
     نيفا ابن --أصلي برنامج.نيفا    بناء ثنائي أصلي عبر مترجم C النظام\n\
     نيفا شغل مصنوع.نيفاب  تشغيل مصنوع بايت كود\n\
     --شجرة                عرض الشجرة النحوية ثم التشغيل\n\
     --اسمح-بالكل         السماح بملفات خارج مجلد البرنامج (للموثوقة فقط)\n\
     --اسمح-بالشبكة       السماح بالشبكة (شبكة.اجلب) — مغلقة افتراضيا\n\
     --اسمح-بالخادم       السماح بالخادم (شبكة.اخدم) — مغلق افتراضيا، وعلم الشبكة وحده لا يكفي\n\
     --مضيف-محلي مضيف       السماح بمضيف محلي صراحة (مثل 127.0.0.1) — وحده لا يفتح الشبكة\n\
     --مستقل               مع ابن: ملف تنفيذي مستقل بدلا من المصنوع\n\
     --أصلي                مع ابن: ثنائي أصلي (C) بدلا من المصنوع\n"
        .to_string()
}

/// استخدام البرنامج المستقل (ملف تنفيذي بُني بـ ابن --مستقل).
fn standalone_usage() -> String {
    "استخدام: <البرنامج> [--اسمح-بالكل] [--اسمح-بالشبكة] [--مضيف-محلي مضيف]\n\
     برنامج نيفا مستقل — يعمل وحده بلا تثبيت نيفا.\n\
     --مساعدة         عرض هذه المساعدة\n\
     --نسخة           عرض إصدار المحرك\n\
     --اسمح-بالكل   السماح بملفات خارج مجلد البرنامج (للموثوقة فقط)\n\
     --اسمح-بالشبكة السماح بالشبكة — مغلقة افتراضيا\n\
     --اسمح-بالخادم السماح بالخادم — مغلق افتراضيا، وعلم الشبكة وحده لا يكفي\n\
     --مضيف-محلي مضيف السماح بمضيف محلي صراحة (مثل 127.0.0.1) — وحده لا يفتح الشبكة\n"
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
    // مهمة 8 (ك-2.2): الملف التنفيذي المستقل يكتشف نفسه
    // أولا — إن كان الملف التنفيذي الحالي يحمل مصنوعا
    // في ذيله، فهو برنامج مستقل، لا محرك نيفا. الفحص
    // رخيص: قراءة ذيل الملف فقط، لا الملف كله.
    if let Some(payload) = current_exe_standalone_payload() {
        return run_standalone(payload);
    }
    let args: Vec<String> = env::args().collect();
    // أمر المالك 2026-11-06 ~05:08: لا تشكيل ولا تطويل في أي
    // أمر/علم — المطابقة على الصيغة المطبّعة (نفس قواعد
    // المحلل)؛ والمسارات تبقى خاما كما كتبها المستخدم.
    // تنبيه: nargs مطبّعة — التطبيع يحوّل ة إلى ه،
    // فكل مقارنة أدناه تُكتب بالصيغة المطبّعة
    // (--مساعده، --نسخه، --شجره، --اسمح-بالشبكه)؛
    // أما نصوص العرض (usage والرسائل) فتبقى بالصيغة
    // القانونية (مساعدة، نسخة، شجرة، شبكة).
    let nargs: Vec<String> = args.iter().map(|a| normalize_cli_word(a)).collect();
    if nargs.iter().any(|a| a == "--مساعده") {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if nargs.iter().any(|a| a == "--نسخه") {
        println!("محرك نيفا ١٫٠٫٠");
        return ExitCode::SUCCESS;
    }
    // خيار غير معروف: خطأ عربي واضح بدلا من تجاهله بصمت.
    // Arabic flags only — a Latin flag is rejected here.
    // --مفسر is a HIDDEN diagnostic flag (DESIGN §39.3):
    // known to the parser, but never shown in --مساعدة.
    const KNOWN_FLAGS: [&str; 11] = [
        "--مساعده",
        "--نسخه",
        "--تفاعلي",
        "--مفسر",
        "--شجره",
        "--اسمح-بالكل",
        "--اسمح-بالشبكه",
        "--اسمح-بالخادم",
        "--مضيف-محلي",
        "--مستقل",
        "--اصلي",
    ];
    // ك-2 (مهمة 7): --افتراضي أُزيل — الآلة الافتراضية هي
    // المحرك الافتراضي الآن، فلا معنى للعلم. خطأ عربي واضح
    // يقول ذلك، بدلا من «خيار غير معروف» المضللة.
    if nargs.iter().any(|a| a == "--افتراضي") {
        eprintln!("الآلة الافتراضية هي المحرك الافتراضي الآن — لا حاجة إلى --افتراضي");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .zip(nargs.iter().skip(1))
        .find(|(_, n)| n.starts_with('-') && !KNOWN_FLAGS.contains(&n.as_str()))
        .map(|(raw, _)| raw)
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
    let allow_network = nargs.iter().any(|a| a == "--اسمح-بالشبكه");
    // Meeting 27: the server is a separate permission — the
    // network flag alone is NOT enough to serve.
    let allow_server = nargs.iter().any(|a| a == "--اسمح-بالخادم");
    let mut local_hosts: Vec<String> = Vec::new();
    let mut arg_iter = args.iter().skip(1).peekable();
    let mut file_args: Vec<String> = Vec::new();
    while let Some(a) = arg_iter.next() {
        if normalize_cli_word(a) == "--مضيف-محلي" {
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
    let allow_all = nargs.iter().any(|a| a == "--اسمح-بالكل");
    // ك-2 (مهمة 7): --مفسر علم تشخيص خفي — مفسر الشجرة
    // للمقارنة والتشخيص فقط، ولا يظهر في --مساعدة.
    let use_interpreter = nargs.iter().any(|a| a == "--مفسر");
    // مهمة 8 (ك-2.2): أوامر البناء والتشغيل — أول موضعي
    // هو الأمر نفسه (ابن/شغل)، والثاني مساره.
    // الصيغ القانونية بلا تشكيل؛ والمشكولة (ابنِ، شغّل)
    // مقبولة توافقا عبر التطبيع.
    let first_cmd = file_args.first().map(|s| normalize_cli_word(s));
    if first_cmd.as_deref() == Some("ابن") {
        return do_build(&args, &file_args);
    }
    if first_cmd.as_deref() == Some("شغل") {
        return do_run_artifact(&args, &file_args, allow_all, allow_network, allow_server, &local_hosts);
    }
    // --مستقل بلا ابن لا معنى له.
    if nargs.iter().any(|a| a == "--مستقل") {
        eprintln!("خطأ: العلم --مستقل يناسب أمر ابن فقط — مثال: نيفا ابن --مستقل برنامج.نيفا");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    // --أصلي بلا ابن لا معنى له.
    if nargs.iter().any(|a| a == "--اصلي") {
        eprintln!("خطأ: العلم --أصلي يناسب أمر ابن فقط — مثال: نيفا ابن --أصلي برنامج.نيفا");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    if file_arg.is_none() {
        // Same 256 MB stack thread as file mode, so the depth-500
        // recursion limit is a Niva error, not a Rust stack overflow.
        let handle = std::thread::Builder::new()
            .stack_size(STACK_SIZE_BYTES)
            .spawn(move || repl::run_repl(allow_all, allow_network, allow_server, &local_hosts, use_interpreter));
        return match handle {
            Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
            Err(_) => ExitCode::FAILURE,
        };
    }
    let path = file_arg.unwrap();
    let show_ast = nargs.iter().any(|a| a == "--شجره");
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
    let program_dir = program_dir_of(&path);
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
                // Meeting 27: the server flag is separate —
                // the network flag alone does not open it.
                interp.set_server_allowed(allow_server);
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
            let compiled = match vm::Compiler::compile_program(&program) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    return ExitCode::FAILURE;
                }
            };
            return run_vm(compiled, program_dir, allow_all, allow_network, allow_server, &local_hosts);
        });
    match handle {
        Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => ExitCode::FAILURE,
    }
}

/// مجلد البرنامج من مسار ملفه: مجلد الأب مطلقا، أو
/// مجلد التشغيل إن كان الملف بلا مجلد.
fn program_dir_of(path: &str) -> std::path::PathBuf {
    std::path::Path::new(path)
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
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// شغّل برنامجا مترجما على VM — مسار التشغيل العادي
/// ومسار المصنوع ومسار المستقل واحد، فلا يختلفا أبدا:
/// بوابة الشبكة كما في مسار المفسر تماما (مهمة 4)،
/// و--اسمح-بالكل كما يُمرَّر إلى المفسر (مهمة 5).
fn run_vm(
    compiled: vm::CompiledProgram,
    program_dir: std::path::PathBuf,
    allow_all: bool,
    allow_network: bool,
    allow_server: bool,
    local_hosts: &[String],
) -> ExitCode {
    niva_engine::network::set_network_allowed(allow_network);
    for host in local_hosts {
        niva_engine::network::allow_local_host(host);
    }
    // مهمة 10: سياسة العمليات للمسار المباشر (المترجم
    // الافتراضي) — مجلد البرنامج + موثوق، وcwd يبدأ منه.
    niva_engine::processes::set_process_policy(
        program_dir.clone(),
        allow_all,
        program_dir.clone(),
    );
    let mut vm_inst = vm::Vm::new(compiled, program_dir);
    vm_inst.set_trusted(allow_all);
    vm_inst.set_server_allowed(allow_server);
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
}

/// مصنوع الملف التنفيذي الحالي، إن كان مستقلا — بلا
/// قراءة الملف كله: قفز إلى الذيل فقط. لا شيء إن لم
/// يكن الملف التنفيذي مستقلا (الحالة العادية).
fn current_exe_standalone_payload() -> Option<Vec<u8>> {
    let exe = std::env::current_exe().ok()?;
    artifact::extract_standalone_payload(&exe)
}

/// شغّل برنامجا مستقلا: فكّ مصنوع الذيل وشغّله على
/// VM، ومجلد البرنامج هو مجلد الملف التنفيذي نفسه
/// (قياسا على مجلد ملف .نيفا في الوضع العادي).
/// أعلام الصلاحيات وحدها مسموحة؛ وأي موضعي آخر خطأ.
fn run_standalone(payload: Vec<u8>) -> ExitCode {
    let args: Vec<String> = env::args().collect();
    // المطابقة على الصيغة المطبّعة (أمر المالك 2026-11-06
    // ~05:08)؛ والرسائل تعرض ما كتبه المستخدم خاما.
    let nargs: Vec<String> = args.iter().map(|a| normalize_cli_word(a)).collect();
    if nargs.iter().any(|a| a == "--مساعده") {
        print!("{}", standalone_usage());
        return ExitCode::SUCCESS;
    }
    if nargs.iter().any(|a| a == "--نسخه") {
        println!("محرك نيفا ١٫٠٫٠");
        return ExitCode::SUCCESS;
    }
    const ALLOWED: [&str; 6] = [
        "--مساعده",
        "--نسخه",
        "--اسمح-بالكل",
        "--اسمح-بالشبكه",
        "--اسمح-بالخادم",
        "--مضيف-محلي",
    ];
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .zip(nargs.iter().skip(1))
        .find(|(_, n)| n.starts_with('-') && !ALLOWED.contains(&n.as_str()))
        .map(|(raw, _)| raw)
    {
        eprintln!("خيار غير معروف: {}", unknown);
        eprint!("{}", standalone_usage());
        return ExitCode::FAILURE;
    }
    let allow_network = nargs.iter().any(|a| a == "--اسمح-بالشبكه");
    let allow_all = nargs.iter().any(|a| a == "--اسمح-بالكل");
    let allow_server = nargs.iter().any(|a| a == "--اسمح-بالخادم");
    let mut local_hosts: Vec<String> = Vec::new();
    let mut arg_iter = args.iter().skip(1).peekable();
    while let Some(a) = arg_iter.next() {
        if normalize_cli_word(a) == "--مضيف-محلي" {
            match arg_iter.next() {
                Some(host) => local_hosts.push(host.clone()),
                None => {
                    eprintln!("خيار --مضيف-محلي يحتاج مضيفا بعده");
                    eprint!("{}", standalone_usage());
                    return ExitCode::FAILURE;
                }
            }
        } else if !a.starts_with('-') {
            eprintln!("خطأ: البرنامج المستقل لا يأخذ ملف مصدر — شغّله مباشرة");
            eprint!("{}", standalone_usage());
            return ExitCode::FAILURE;
        }
    }
    let program_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    // الفك والتشغيل داخل الخيط: CompiledProgram يحمل
    // Rc فلا يعبر خيطا — كالترجمة في المسار العادي.
    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE_BYTES)
        .spawn(move || {
            let prog = match artifact::decode_program(&payload) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("خطأ: {}", e.message);
                    return ExitCode::FAILURE;
                }
            };
            // مهمة 8: ارفع عدّاد الإغلاقات فوق معرّفات
            // المصنوع، فلا يصطدم بها إغلاق وحدة تُترجَم
            // وقت التشغيل.
            vm::reserve_closure_ids_above(artifact::max_closure_id(&prog));
            run_vm(prog, program_dir, allow_all, allow_network, allow_server, &local_hosts)
        });
    match handle {
        Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => ExitCode::FAILURE,
    }
}

/// أمر ابن: `نيفا ابن برنامج.نيفا` يبني مصنوع بايت
/// كود محمول (.نيفاب) بجانب المصدر؛ و`نيفا ابن
/// --مستقل برنامج.نيفا` يبني ملفا تنفيذيا مستقلا
/// (نسخة من نيفا نفسها + المصنوع في ذيلها).
/// الأخطاء هي أخطاء التشغيل العادي نفسها (لغوي، نحوي،
/// دلالي، ترجمة) — فالنموذج المعكوس مرفوض هنا كما هناك،
/// لأنه يُرفَض في التحليل أصلا.
fn do_build(args: &[String], file_args: &[String]) -> ExitCode {
    const FORBIDDEN: [&str; 7] = [
        "--مفسر",
        "--تفاعلي",
        "--شجره",
        "--اسمح-بالكل",
        "--اسمح-بالشبكه",
        "--اسمح-بالخادم",
        "--مضيف-محلي",
    ];
    // المطابقة على الصيغة المطبّعة؛ والرسالة تعرض العلم
    // كما كتبه المستخدم خاما.
    if let Some(flag) = args
        .iter()
        .zip(args.iter().map(|a| normalize_cli_word(a)))
        .find(|(_, n)| FORBIDDEN.contains(&n.as_str()))
        .map(|(raw, _)| raw)
    {
        eprintln!("خطأ: العلم {} لا يناسب أمر ابن", flag);
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    let src_path = match file_args.get(1) {
        Some(p) => p.clone(),
        None => {
            eprintln!("خطأ: ابن يحتاج ملف مصدر — مثال: نيفا ابن برنامج.نيفا");
            eprint!("{}", usage());
            return ExitCode::FAILURE;
        }
    };
    if file_args.len() > 2 {
        eprintln!("خطأ: ابن يبني ملفا واحدا فقط");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    let standalone = args
        .iter()
        .any(|a| normalize_cli_word(a) == "--مستقل");
    let native = args
        .iter()
        .any(|a| normalize_cli_word(a) == "--اصلي");
    if standalone && native {
        eprintln!("خطأ: لا تجمع --مستقل و--أصلي — اختر أحدهما");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    if native {
        // الثنائي الأصلي: المولد يحلل المصدر بنفسه.
        let out_path = {
            let p = std::path::Path::new(&src_path);
            let mut o = p.with_extension("");
            if cfg!(windows) {
                o.set_extension("exe");
            }
            o
        };
        return match cgen::build_native(std::path::Path::new(&src_path), &out_path) {
            Ok(()) => {
                println!("بُني الثنائي الأصلي: {}", out_path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}", e);
                ExitCode::FAILURE
            }
        };
    }
    let source = match fs::read_to_string(&src_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "خطأ إدخال/إخراج: تعذر قراءة الملف {}: {}",
                src_path,
                io_error_arabic(&e)
            );
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
    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE_BYTES)
        .spawn(move || {
            let mut analyzer = SemanticAnalyzer::new();
            if let Err(e) = analyzer.analyze_program(&program) {
                eprintln!("{}", e);
                return ExitCode::FAILURE;
            }
            let compiled = match vm::Compiler::compile_program(&program) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    return ExitCode::FAILURE;
                }
            };
            let payload = match artifact::encode_program(&compiled) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("خطأ: {}", e.message);
                    return ExitCode::FAILURE;
                }
            };
            if standalone {
                build_standalone(&src_path, &payload)
            } else {
                let out_path =
                    std::path::Path::new(&src_path).with_extension(artifact::ARTIFACT_EXT);
                match fs::write(&out_path, &payload) {
                    Ok(()) => {
                        println!("بُني المصنوع: {}", out_path.display());
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!(
                            "خطأ إدخال/إخراج: تعذر كتابة الملف {}: {}",
                            out_path.display(),
                            io_error_arabic(&e)
                        );
                        ExitCode::FAILURE
                    }
                }
            }
        });
    match handle {
        Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => ExitCode::FAILURE,
    }
}

/// ابنِ الملف التنفيذي المستقل: نسخة من نيفا نفسها
/// (الملف التنفيذي الحالي) + المصنوع في ذيلها.
fn build_standalone(src_path: &str, payload: &[u8]) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => {
            eprintln!("خطأ: تعذر تحديد الملف التنفيذي الحالي لبناء المستقل");
            return ExitCode::FAILURE;
        }
    };
    let exe_bytes = match fs::read(&exe) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "خطأ إدخال/إخراج: تعذر قراءة الملف {}: {}",
                exe.display(),
                io_error_arabic(&e)
            );
            return ExitCode::FAILURE;
        }
    };
    let mut out_path = std::path::PathBuf::from(src_path);
    out_path.set_extension("");
    if cfg!(windows) {
        out_path.set_extension("exe");
    }
    // حارس: لا تكتب فوق نيفا نفسها أبدا.
    let same = std::fs::canonicalize(&out_path)
        .ok()
        .zip(std::fs::canonicalize(&exe).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if same {
        eprintln!("خطأ: اسم المخرج يطابق الملف التنفيذي الحالي — غيّر اسم المصدر");
        return ExitCode::FAILURE;
    }
    let wrapped = artifact::wrap_standalone(&exe_bytes, payload);
    if let Err(e) = fs::write(&out_path, &wrapped) {
        eprintln!(
            "خطأ إدخال/إخراج: تعذر كتابة الملف {}: {}",
            out_path.display(),
            io_error_arabic(&e)
        );
        return ExitCode::FAILURE;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(0o755));
    }
    println!("بُني الملف التنفيذي المستقل: {}", out_path.display());
    ExitCode::SUCCESS
}

/// أمر شغل: `نيفا شغل مصنوع.نيفاب` يفكّ المصنوع
/// ويشغّله على VM — المخرج ورمز الخروج كما في التشغيل
/// العادي حرفيا. مجلد البرنامج هو مجلد المصنوع،
/// فملفات الوحدات المستوردة (.نيفا) تسافر بجانبه
/// وتُترجَم عند التشغيل، كما في الوضع العادي.
/// أعلام الصلاحيات وحدها مسموحة.
fn do_run_artifact(
    args: &[String],
    file_args: &[String],
    allow_all: bool,
    allow_network: bool,
    allow_server: bool,
    local_hosts: &[String],
) -> ExitCode {
    const FORBIDDEN: [&str; 4] = ["--مفسر", "--تفاعلي", "--شجره", "--مستقل"];
    if let Some(flag) = args
        .iter()
        .zip(args.iter().map(|a| normalize_cli_word(a)))
        .find(|(_, n)| FORBIDDEN.contains(&n.as_str()))
        .map(|(raw, _)| raw)
    {
        eprintln!("خطأ: العلم {} لا يناسب أمر شغل", flag);
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    let art_path = match file_args.get(1) {
        Some(p) => p.clone(),
        None => {
            eprintln!("خطأ: شغل يحتاج مصنوعا — مثال: نيفا شغل مصنوع.نيفاب");
            eprint!("{}", usage());
            return ExitCode::FAILURE;
        }
    };
    if file_args.len() > 2 {
        eprintln!("خطأ: شغل يشغّل مصنوعا واحدا فقط");
        eprint!("{}", usage());
        return ExitCode::FAILURE;
    }
    let bytes = match fs::read(&art_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "خطأ إدخال/إخراج: تعذر قراءة الملف {}: {}",
                art_path,
                io_error_arabic(&e)
            );
            return ExitCode::FAILURE;
        }
    };
    let program_dir = program_dir_of(&art_path);
    // نسخة مملوكة للخيط (الخيط المولود يشترط 'static).
    let local_hosts = local_hosts.to_vec();
    // الفك والتشغيل داخل الخيط: CompiledProgram يحمل
    // Rc فلا يعبر خيطا — كالترجمة في المسار العادي.
    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE_BYTES)
        .spawn(move || {
            let prog = match artifact::decode_program(&bytes) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("خطأ: {}", e.message);
                    return ExitCode::FAILURE;
                }
            };
            // مهمة 8: ارفع عدّاد الإغلاقات فوق معرّفات
            // المصنوع، فلا يصطدم بها إغلاق وحدة تُترجَم
            // وقت التشغيل.
            vm::reserve_closure_ids_above(artifact::max_closure_id(&prog));
            run_vm(prog, program_dir, allow_all, allow_network, allow_server, &local_hosts)
        });
    match handle {
        Ok(h) => h.join().unwrap_or(ExitCode::FAILURE),
        Err(_) => ExitCode::FAILURE,
    }
}
