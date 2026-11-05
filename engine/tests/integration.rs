//! Integration tests for the Niva Rust engine.
//! Run with: cargo test
//! Each example in examples/ is run with the real binary (installed as
//! `نيفا`; cargo builds it as `niva` — ASCII-only bin names) and its
//! stdout is compared exactly. Error tests run small programs from a
//! temporary directory and check the Arabic error, line and column.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_niva").to_string()
}

fn run_file(path: &str) -> (String, String, i32) {
    let out = Command::new(bin()).arg(path).output().expect("run نيفا");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn run_src(src: &str) -> (String, String, i32) {
    // Unique directory per call: cargo runs tests in parallel, so a shared
    // temp file would race and make tests read each other's programs.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "rio4140-engine-test-{}-{}",
        std::process::id(),
        n
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, src).unwrap();
    let result = Command::new(bin())
        .arg(&path)
        .current_dir(&dir)
        .output()
        .expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
        result.status.code().unwrap_or(-1),
    )
}

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

#[test]
fn examples_pass() {
    let cases: &[(&str, &str)] = &[
        (
            "01-hello.نيفا",
            "أهلا بك في نيفا\n٥\nالنتيجة: ٢٠\n",
        ),
        (
            "02-variables.نيفا",
            "٨\n١٥\nمحمود\nصحيح\nعدد\nنص\n٥٠\n١٢٣\n",
        ),
        (
            "03-conditions.نيفا",
            "ناجح\nفي المدى\nليس ٩٩\n",
        ),
        (
            "04-loops.نيفا",
            "٠\n١\n٢\nبعد الحلقة:\n٢\n٠\n١\n٢\nتكرار\nتكرار\n٣\n٤\n",
        ),
        (
            "05-functions.نيفا",
            "٧\nأهلا محمود\n١٠٠\n١٠٠\n",
        ),
        (
            "06-lists-dicts.نيفا",
            "١\n٣\n٦\n٣\n[١, ٩٩, ٣]\nمحمود\n٢٣\n٣\nاسم\nعمر\nمدينة\nصحيح\nصحيح\n",
        ),
        (
            "07-try-files.نيفا",
            "خطأ في القسمة\nانتهى\nمرحبا من محرك نيفا\n",
        ),
        (
            "08-full-program.نيفا",
            "محمود: ناجح\nسارة: ممتاز\nعلي: راسب\nالمتوسط:\n٧٢٫٣٣٣٣٣٣٣٣٣٣٣٣٣٣\nأعلى درجة:\n٩٢\nمجموع من الحلقة:\n٢١٧\n",
        ),
        (
            "09-modules.نيفا",
            "٧\n٤٢\n٣\n",
        ),
        (
            "10-types.نيفا",
            "٥\n٧\nمحمود\n",
        ),
        (
            "11-classes.نيفا",
            "٣\n٤\n٧\n١٣\n١٣\n",
        ),
        (
            "12-inheritance-interfaces.نيفا",
            "طالب: محمود\nيدرس\nمحمود\n",
        ),
        (
            "13-closures.نيفا",
            "١\n٢\n٣\n١\nأهلا محمود\n",
        ),
        (
            "14-lambdas.نيفا",
            "١٠\n١٢\n١٢\n١٠\n١١\n٧\n",
        ),
        (
            "15-methods.نيفا",
            "٥\nHELLO\nhello\nمحمود\nأهلا سارة\n[\"أ\", \"ب\", \"ج\"]\nصحيح\n٣\n[١, ٢, ٣, ٤]\n٤\n[١, ٢, ٣]\n",
        ),
        (
            "16-stdlib.نيفا",
            "٤\n١٠٢٤\n٣\n٤\n٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥\n٥\nصحيح\nصحيح\nصحيح\nصحيح\n",
        ),
        (
            "17-word-operators.نيفا",
            "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\nصحيح\nخطأ\nصحيح\nكبير\n",
        ),
    ];
    for (name, expected) in cases {
        let path = example_path(name);
        let (out, err, code) = run_file(path.to_str().unwrap());
        assert_eq!(code, 0, "{} failed: stderr={}", name, err);
        assert_eq!(out, *expected, "output mismatch for {}", name);
    }
    // The file example writes next to its program file (Meeting 4:
    // relative paths resolve against program_dir, not cwd); clean it up.
    let _ = fs::remove_file("niva-engine-test.txt");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = fs::remove_file(manifest.join("niva-engine-test.txt"));
    let _ = fs::remove_file(manifest.join("examples").join("niva-engine-test.txt"));
}

#[test]
fn function_scope_is_local() {
    let (_, err, code) = run_src(
        "دالة جمع(أ، ب):\n    نتيجة = أ + ب\n    ارجع نتيجة\nاطبع جمع(٢، ٣)\nاطبع نتيجة\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("متغير غير معرف"), "stderr: {}", err);
    assert!(err.contains("السطر"), "stderr: {}", err);
    assert!(err.contains("العمود"), "stderr: {}", err);
}

#[test]
fn loop_variable_visible_after_loop() {
    let (out, _, code) =
        run_src("لكل رقم في مدى(٣):\n    اطبع رقم\nاطبع رقم\n");
    assert_eq!(code, 0);
    assert_eq!(out, "٠\n١\n٢\n٢\n");
}

#[test]
fn type_error_has_kind_line_col() {
    let (_, err, code) = run_src("اطبع \"أ\" + ١\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع"), "stderr: {}", err);
    assert!(err.contains("السطر 1"), "stderr: {}", err);
}

#[test]
fn division_by_zero_error() {
    let (_, err, code) = run_src("اطبع ١٠ / ٠\n");
    assert_ne!(code, 0);
    assert!(err.contains("قسمة على صفر"), "stderr: {}", err);
}

#[test]
fn recursion_depth_limit_returns_arabic_error_not_crash() {
    // P0 (v1.2 final review): unbounded recursion must NOT overflow
    // the Rust stack. It must return a Niva error with kind, line, col.
    let (_, err, code) = run_src(
        "دالة عميقة(ن):\n    ارجع عميقة(ن + ١)\nاطبع عميقة(٠)\n",
    );
    assert_ne!(code, 0, "unbounded recursion must fail, not hang");
    assert!(
        err.contains("عمق الاستدعاء"),
        "expected Arabic depth error, got stderr: {}",
        err
    );
    assert!(err.contains("السطر"), "stderr: {}", err);
    assert!(
        !err.contains("stack overflow"),
        "must not crash the process, stderr: {}",
        err
    );
}

#[test]
fn shallow_recursion_still_works_and_depth_is_released() {
    // Depth 100 (well under the 500 limit) must work, and depth
    // must return to zero after a call, so a second call is not
    // falsely rejected.
    let (out, _, code) = run_src(
        "دالة عد(ن):\n    إذا ن == ٠:\n        ارجع ٠\n    وإلا:\n        ارجع عد(ن - ١) + ١\nاطبع عد(١٠٠)\nاطبع عد(١٠)\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "١٠٠\n١٠\n");
}

#[test]
fn recursion_depth_500_completes_extreme_shape() {
    // Meeting 15: the worst measured shape (10 locals + wide
    // expression) must complete at exactly the 500 cap, in the
    // debug binary (the worst profile). Max depth here is 500.
    let extreme = "دالة عد(ن):\n    أ1 = ن * 2\n    أ2 = أ1 + ن\n    أ3 = أ2 * 3\n    أ4 = أ3 + ن\n    أ5 = أ4 * 2\n    أ6 = أ5 + ن\n    أ7 = أ6 * 3\n    أ8 = أ7 + ن\n    أ9 = أ8 * 2\n    أ10 = أ9 + ن\n    إذا ن <= 0:\n        ارجع 0\n    ارجع عد(ن - 1) + أ1 + أ2 + أ3 + أ4 + أ5 + أ6 + أ7 + أ8 + أ9 + أ10\nاطبع عد(499)\n";
    let (out, err, code) = run_src(extreme);
    assert_eq!(code, 0, "depth 500 must complete, stderr: {}", err);
    assert_eq!(out, "٥٦٠١٢٧٥٠\n");
}

#[test]
fn recursion_beyond_500_is_clean_error() {
    // Meeting 15: depth 501 must be a clean Niva error naming
    // the 500 cap — never a stack overflow abort.
    let extreme = "دالة عد(ن):\n    أ1 = ن * 2\n    أ2 = أ1 + ن\n    أ3 = أ2 * 3\n    أ4 = أ3 + ن\n    أ5 = أ4 * 2\n    أ6 = أ5 + ن\n    أ7 = أ6 * 3\n    أ8 = أ7 + ن\n    أ9 = أ8 * 2\n    أ10 = أ9 + ن\n    إذا ن <= 0:\n        ارجع 0\n    ارجع عد(ن - 1) + أ1 + أ2 + أ3 + أ4 + أ5 + أ6 + أ7 + أ8 + أ9 + أ10\nاطبع عد(500)\n";
    let (_, err, code) = run_src(extreme);
    assert_ne!(code, 0, "depth 501 must fail");
    assert!(err.contains("تجاوز حد عمق الاستدعاء (500)"), "stderr: {}", err);
    assert!(!err.contains("stack overflow"), "must not crash, stderr: {}", err);
}

#[test]
fn vm_recursion_depth_500_matches_interpreter() {
    // Meeting 15: the cap is shared — the VM completes at 500
    // and errors cleanly at 501, like the interpreter.
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-depth-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    let prog = "دالة عد(ن):\n    إذا ن <= 0:\n        ارجع 0\n    ارجع عد(ن - 1) + 1\nاطبع عد(499)\n";
    std::fs::write(&path, prog).unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "٤٩٩\n");
    let prog2 = prog.replace("عد(499)", "عد(500)");
    std::fs::write(&path, prog2).unwrap();
    let out2 = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(out2.status.code().unwrap_or(0), 0);
    assert!(
        String::from_utf8_lossy(&out2.stderr).contains("تجاوز حد عمق الاستدعاء (500) في VM"),
        "stderr: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
}

#[test]
fn semantic_undefined_variable_caught_before_any_output() {
    // Phase 1A Step 1: the analyzer runs before execution, so a
    // program that prints first and then uses an undefined variable
    // must print NOTHING, not the first print.
    let (out, err, code) = run_src(
        "اطبع \"قبل\"\nاطبع غير_معرف\n",
    );
    assert_ne!(code, 0);
    assert_eq!(out, "", "analyzer must run before execution, out: {}", out);
    assert!(err.contains("متغير غير معرف"), "stderr: {}", err);
    assert!(err.contains("دلالي"), "stderr: {}", err);
}

#[test]
fn semantic_return_break_and_arity_errors() {
    let (_, err, code) = run_src("ارجع ١\n");
    assert_ne!(code, 0);
    assert!(err.contains("ارجع خارج دالة"), "stderr: {}", err);

    let (_, err, code) = run_src("اكسر\n");
    assert_ne!(code, 0);
    assert!(err.contains("اكسر خارج حلقة"), "stderr: {}", err);

    let (_, err, code) = run_src(
        "دالة جمع(أ، ب):\n    ارجع أ + ب\nاطبع جمع(١)\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("عدد المعاملات غير صحيح"), "stderr: {}", err);

    let (_, err, code) = run_src("اطبع طول(١، ٢)\n");
    assert_ne!(code, 0);
    assert!(err.contains("عدد المعاملات غير صحيح"), "stderr: {}", err);
}

#[test]
fn semantic_forward_call_fails_like_runtime() {
    // Calling a function before its definition fails at analysis,
    // matching runtime top-down definition order. Recursion passes.
    let (_, err, code) = run_src("اطبع جمع(١، ٢)\nدالة جمع(أ، ب):\n    ارجع أ + ب\n");
    assert_ne!(code, 0);
    assert!(err.contains("متغير غير معرف"), "stderr: {}", err);

    let (out, _, code) = run_src(
        "دالة عد(ن):\n    إذا ن == ٠:\n        ارجع ٠\n    وإلا:\n        ارجع عد(ن - ١)\nاطبع عد(٣)\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "٠\n");
}

fn run_with_modules(files: &[(&str, &str)], main: &str) -> (String, String, i32) {
    // Multi-file test: write module files + main file in one unique
    // temp dir, run the main file (modules resolve relative to it).
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "rio4140-engine-module-test-{}-{}",
        std::process::id(),
        n
    ));
    fs::create_dir_all(&dir).unwrap();
    for (name, src) in files {
        fs::write(dir.join(name), src).unwrap();
    }
    let result = Command::new(bin())
        .arg(dir.join(main))
        .current_dir(&dir)
        .output()
        .expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
        result.status.code().unwrap_or(-1),
    )
}

const CALC_MODULE: &str = "صدّر اجمع، الثابت، احصل\nالثابت = ١٠\nدالة مساعد(س):\n    ارجع س * ٢\nدالة اجمع(أ، ب):\n    ارجع مساعد(أ) + ب + الثابت\nدالة احصل():\n    ارجع الثابت\nسر = ٩٩٩\n";

#[test]
fn module_import_dot_from_and_alias() {
    // Exported function uses a private helper (مساعد) and a module
    // global (الثابت); both must be visible inside the module call.
    let (out, err, code) = run_with_modules(
        &[
            ("حساب.نيفا", CALC_MODULE),
            (
                "برنامج.نيفا",
                "استورد حساب\nاطبع حساب.اجمع(٣، ٤)\nاطبع حساب.احصل()\nمن حساب استورد اجمع\nاطبع اجمع(١، ٢)\nاستورد حساب باسم ح\nاطبع ح.احصل()\nاطبع حساب\n",
            ),
        ],
        "برنامج.نيفا",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "٢٠\n١٠\n١٤\n١٠\n<وحدة حساب>\n");
}

#[test]
fn module_private_name_rejected() {
    let (_, err, code) = run_with_modules(
        &[
            ("حساب.نيفا", CALC_MODULE),
            ("برنامج.نيفا", "استورد حساب\nاطبع حساب.سر\n"),
        ],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert!(err.contains("غير مصدر"), "stderr: {}", err);
    assert!(err.contains("حساب"), "stderr: {}", err);

    let (_, err, code) = run_with_modules(
        &[
            ("حساب.نيفا", CALC_MODULE),
            ("برنامج.نيفا", "من حساب استورد سر\n"),
        ],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert!(err.contains("غير مصدر"), "stderr: {}", err);
}

#[test]
fn module_missing_and_cycle_errors() {
    let (_, err, code) = run_with_modules(
        &[("برنامج.نيفا", "استورد لاوجود\n")],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert!(err.contains("وحدة غير موجودة"), "stderr: {}", err);
    assert!(err.contains("لاوجود"), "stderr: {}", err);

    let (_, err, code) = run_with_modules(
        &[
            ("أ.نيفا", "صدّر من_أ\nاستورد ب\nدالة من_أ():\n    ارجع ١\n"),
            ("ب.نيفا", "صدّر من_ب\nاستورد أ\nدالة من_ب():\n    ارجع ٢\n"),
            ("برنامج.نيفا", "استورد أ\n"),
        ],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert!(err.contains("استيراد دائري"), "stderr: {}", err);
}

#[test]
fn module_load_once() {
    let (out, err, code) = run_with_modules(
        &[
            ("مرة.نيفا", "صدّر قيمة\nاطبع \"تحميل مرة\"\nقيمة = ٧\n"),
            (
                "برنامج.نيفا",
                "استورد مرة\nاستورد مرة\nمن مرة استورد قيمة\nاطبع قيمة\n",
            ),
        ],
        "برنامج.نيفا",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "تحميل مرة\n٧\n");
}

#[test]
fn module_semantic_analysis_still_applies_to_main() {
    // Main program: import defines حساب, but غيرمعرف is still a
    // semantic error before any output.
    let (out, err, code) = run_with_modules(
        &[
            ("حساب.نيفا", CALC_MODULE),
            (
                "برنامج.نيفا",
                "استورد حساب\nاطبع حساب.اجمع(١، ٢)\nاطبع غيرمعرف\n",
            ),
        ],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert_eq!(out, "");
    assert!(err.contains("متغير غير معرف"), "stderr: {}", err);
    assert!(err.contains("دلالي"), "stderr: {}", err);
}

#[test]
fn dot_on_non_module_is_type_error() {
    let (_, err, code) = run_src("س = ٥\nاطبع س.شيء\n");
    assert_ne!(code, 0);
    assert!(err.contains("الوصول بالنقطة"), "stderr: {}", err);
}

#[test]
fn not_binds_looser_than_comparison() {
    // ليس س == ٩٩  means  ليس (س == ٩٩), not (ليس س) == ٩٩.
    let (out, _, code) = run_src("س = ١٠\nإذا ليس س == ٩٩:\n    اطبع \"صح\"\n");
    assert_eq!(code, 0);
    assert_eq!(out, "صح\n");
}

#[test]
fn type_annotated_assignment_and_function_ok() {
    let (out, _, code) = run_src(
        "س: عدد = ٥\nاطبع س\nدالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\nاطبع جمع(٣، ٤)\nص: نص = \"مرحبا\"\nاطبع ص\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "٥\n٧\nمرحبا\n");
}

#[test]
fn type_static_mismatch_is_semantic_no_output() {
    // Literal mismatch: analyzer catches it before any output.
    let (out, err, code) = run_src("اطبع \"قبل\"\nس: عدد = \"نص\"\n");
    assert_ne!(code, 0);
    assert_eq!(out, "");
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
    assert!(err.contains("دلالي"), "stderr: {}", err);

    let (_, err, code) = run_src(
        "دالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\nاطبع جمع(١، \"نص\")\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
    assert!(err.contains("دلالي"), "stderr: {}", err);

    let (_, err, code) = run_src(
        "دالة احصل(): عدد:\n    ارجع \"نص\"\nاطبع احصل()\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);

    let (_, err, code) = run_src("س: عدد = ٥\nس = \"نص\"\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
}

#[test]
fn type_gradual_runtime_mismatch() {
    // Value from an untyped variable: static type unknown, so the
    // analyzer passes, but the runtime annotated-assignment check
    // catches it, after earlier output (runtime, not semantic).
    let (out, err, code) = run_src("اطبع \"قبل\"\nس = \"نص\"\nص: عدد = س\n");
    assert_ne!(code, 0);
    assert_eq!(out, "قبل\n");
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
    assert!(!err.contains("دلالي"), "must be runtime, not semantic, stderr: {}", err);
}

#[test]
fn type_any_passes() {
    let (out, _, code) = run_src(
        "س: أي = ٥\nس = \"نص\"\nاطبع س\nدالة نقل(ق: أي): أي:\n    ارجع ق\nاطبع نقل(٧)\nاطبع نقل(\"أ\")\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "نص\n٧\nأ\n");
}

#[test]
fn type_unknown_name_is_parse_error() {
    let (_, err, code) = run_src("س: غيرمعروف = ٥\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير معروف"), "stderr: {}", err);
}

#[test]
fn type_module_function_runtime_check() {
    // Module member calls are not statically type-checked (member
    // type unknown to the main analyzer), but the runtime param
    // check in call_user catches a wrong argument type.
    let (_, err, code) = run_with_modules(
        &[
            (
                "حساب.نيفا",
                "صدّر جمع\nدالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\n",
            ),
            (
                "برنامج.نيفا",
                "استورد حساب\nاطبع حساب.جمع(١، \"نص\")\n",
            ),
        ],
        "برنامج.نيفا",
    );
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
}

#[test]
fn class_simple_constructor_methods_and_field_persistence() {
    let (out, err, code) = run_src(
        "فئة نقطة:\n    دالة أنشئ(س، ص):\n        نفس.س = س\n        نفس.ص = ص\n    دالة مجموع():\n        ارجع نفس.س + نفس.ص\n    دالة زد(مقدار):\n        نفس.س = نفس.س + مقدار\n        ارجع نفس.س\nك = أنشئ نقطة(٣، ٤)\nاطبع ك.س\nاطبع ك.مجموع()\nاطبع ك.زد(١٠)\nاطبع ك.س\n",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "٣\n٧\n١٣\n١٣\n");
}

#[test]
fn class_errors_are_arabic() {
    let (_, err, code) = run_src("ك = أنشئ غيرموجود()\n");
    assert_ne!(code, 0);
    assert!(err.contains("فئة غير معرفة"), "stderr: {}", err);
    let (_, err, code) = run_src("فئة س:\n    مرر\nك = س(١)\n");
    assert_ne!(code, 0);
    assert!(err.contains("استخدم أنشئ"), "stderr: {}", err);
    let (_, err, code) = run_src(
        "فئة س:\n    مرر\nك = أنشئ س()\nاطبع ك.غيرموجود\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("ليس له حقل أو طريقة"), "stderr: {}", err);
}

#[test]
fn class_inheritance_override_and_constructor_inheritance() {
    let (out, err, code) = run_src(
        "فئة شخص:\n    دالة أنشئ(اسم):\n        نفس.اسم = اسم\n    دالة عرّف():\n        ارجع نفس.اسم\nفئة طالب ورث شخص:\n    دالة عرّف():\n        ارجع \"طالب: \" + نفس.اسم\nط = أنشئ طالب(\"محمود\")\nاطبع ط.عرّف()\nاطبع ط.اسم\n",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "طالب: محمود\nمحمود\n");
}

#[test]
fn class_interface_compliance_errors() {
    let (_, err, code) = run_src(
        "واجهة قابل:\n    دالة م():\n        مرر\nفئة س واجهة قابل:\n    مرر\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("لا تنفذ طريقة م"), "stderr: {}", err);
    let (_, err, code) = run_src(
        "واجهة قابل:\n    دالة م(أ، ب):\n        مرر\nفئة س واجهة قابل:\n    دالة م(أ):\n        مرر\n",
    );
    assert_ne!(code, 0);
    assert!(err.contains("عدد معاملات غير صحيح"), "stderr: {}", err);
}

#[test]
fn vm_slice1_matches_interpreter_for_core_program() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "س = ٠\nبينما س < ٥:\n    س = س + ١\nاطبع س\nإذا س == ٥ و س > ٢:\n    اطبع \"نعم\"\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0);
    assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), "٥\nنعم\n");
}

#[test]
fn vm_rejects_lambda_with_clear_arabic_error() {
    // Meeting 13: lambdas are interpreter-only until step 4,
    // like returned closures — a clean error, no crash.
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-lambda-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "مضاعف = دالة_مجهولة(س): س * 2\nاطبع مضاعف(5)\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(out.status.code().unwrap_or(0), 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("الدالة المجهولة"));
}

#[test]
fn methods_vm_error_is_clear() {
    // Meeting 14: methods are interpreter-only; the VM's existing
    // dot-access error already names the problem.
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-methods-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "اطبع \"مرحبا\".طول()\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(out.status.code().unwrap_or(0), 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("الوصول بالنقطة"));
}

#[test]
fn methods_unknown_name_is_semantic_error() {
    // Meeting 14: an unknown method on a known-type target is
    // caught by the semantic analyzer, before any execution.
    let (_, err, code) = run_src("اطبع \"نص\".غير_موجود()\n");
    assert_ne!(code, 0);
    assert!(err.contains("طريقة غير معروفة لنوع نص: غير_موجود"));
}

#[test]
fn stdlib_unknown_module_is_io_error() {
    // Meeting 16: a name that is neither native nor a file is
    // the same "module not found" error as a missing file module.
    let (_, err, code) = run_src("استورد وحدة_وهمية\n");
    assert_ne!(code, 0);
    assert!(err.contains("وحدة غير موجودة: وحدة_وهمية"));
}

#[test]
fn stdlib_native_module_not_shadowed_by_file() {
    // Meeting 16: a same-named .نيفا file in the program
    // directory must never shadow a native module.
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-stdlib-shadow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("رياضيات.نيفا"), "صدّر س\nس = 999\n").unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "استورد رياضيات\nاطبع رياضيات.جذر(16)\n").unwrap();
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "٤\n");
}

#[test]
fn stdlib_vm_error_is_clear() {
    // Meeting 16: stdlib is interpreter-only in v1; the VM's
    // existing Import error already says so clearly.
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-stdlib-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "استورد رياضيات\nاطبع رياضيات.جذر(16)\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(out.status.code().unwrap_or(0), 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("غير مدعومة"));
}

#[test]
fn file_policy_restricted_blocks_escape_and_allow_all_permits_it() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-perm-int-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "اكتب_ملف(\"../niva-perm-escape.txt\"، \"x\")\n").unwrap();
    // Default (restricted): must fail, and must NOT create the file outside.
    // current_dir = program dir, so the relative ../ path means
    // exactly dir.parent()/niva-perm-escape.txt in both modes.
    let out = Command::new(bin()).arg(&path).current_dir(&dir).output().expect("run نيفا");
    assert_ne!(out.status.code().unwrap_or(0), 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("خارج مجلد البرنامج"));
    assert!(!dir.parent().unwrap().join("niva-perm-escape.txt").exists());
    // Trusted (--اسمح-بالكل): must succeed and create the file outside.
    let out2 = Command::new(bin()).arg(&path).arg("--اسمح-بالكل").current_dir(&dir).output().expect("run نيفا --اسمح-بالكل");
    assert_eq!(out2.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out2.stderr));
    let escaped = dir.parent().unwrap().join("niva-perm-escape.txt");
    assert!(escaped.exists());
    let _ = fs::remove_file(&escaped);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn file_policy_relative_inside_program_dir_works() {
    let (out, _, code) = run_src("اكتب_ملف(\"out.txt\"، \"أهلا\")\nاطبع اقرأ_ملف(\"out.txt\")\n");
    assert_eq!(code, 0);
    assert_eq!(out, "أهلا\n");
}

#[test]
fn vm_slice2_functions_match_interpreter() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-fn-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "دالة جمع(أ، ب):\n    ارجع أ + ب\nدالة عاملي(ن):\n    إذا ن <= ١:\n        ارجع ١\n    ارجع ن * عاملي(ن - ١)\nاطبع جمع(٢، ٣)\nاطبع عاملي(٥)\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let out2 = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), "٥\n١٢٠\n");
    assert_eq!(String::from_utf8_lossy(&out2.stdout).to_string(), "٥\n١٢٠\n");
}

#[test]
fn vm_slice3_lists_match_interpreter() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-list-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "قائمة = [١، ٢، ٣]\nقائمة[١] = ٩٩\nاطبع قائمة\nقاموس = {\"اسم\": \"محمود\"}\nقاموس[\"مدينة\"] = \"القاهرة\"\nاطبع قاموس[\"مدينة\"]\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let out2 = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), "[١, ٩٩, ٣]\nالقاهرة\n");
    assert_eq!(String::from_utf8_lossy(&out2.stdout).to_string(), "[١, ٩٩, ٣]\nالقاهرة\n");
}

#[test]
fn repl_session_via_stdin() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn niva repl");
    let input = "س = ٥\nس * ٢\nدالة جمع(أ، ب):\n    ارجع أ + ب\nجمع(٢، ٣)\n:خروج\n";
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait repl");
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(stdout.contains("محرك نيفا ١٫٠٫٠"), "stdout: {}", stdout);
    assert!(stdout.contains("١٠\n"), "stdout: {}", stdout);
    // Step 1: the call completes the pending function block, so
    // its result follows the continuation prompt (>), not a
    // fresh ~ $ prompt — the block only ends on a line at indent 0.
    assert!(stdout.contains("٥\n"), "stdout: {}", stdout);
    // The prompts are the terminal ones (owner's orders
    // 2026-11-05 ~22:58 and ~23:03): `~ $ ` at home, `> `
    // while a block is open — never `نيفا: ` / `أكمل: `.
    assert!(stdout.contains("~ $ "), "stdout: {}", stdout);
    assert!(stdout.contains("> "), "stdout: {}", stdout);
}

#[test]
fn repl_eof_exits_cleanly() {
    use std::process::{Command, Stdio};
    let mut child = Command::new(bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn niva repl");
    // Close stdin immediately (EOF with no input).
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("wait repl");
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn vm_slice4_builtins_match_interpreter() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("rio-vm-builtin-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "قائمة = [٣، ١، ٢]\nاطبع طول(قائمة)\nاطبع مجموع(قائمة)\nاطبع أقصى(قائمة)\nاطبع أدنى(قائمة)\nاطبع مطلق(-٥)\nاطبع عدد(\"٤٢\")\nاطبع نص(٥)\nاطبع نوع(قائمة)\nاطبع مدى(٣)\n").unwrap();
    let out = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let out2 = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), "٣\n٦\n٣\n١\n٥\n٤٢\n٥\nقائمة\n[٠, ١, ٢]\n");
    assert_eq!(String::from_utf8_lossy(&out2.stdout).to_string(), "٣\n٦\n٣\n١\n٥\n٤٢\n٥\nقائمة\n[٠, ١, ٢]\n");
}

#[test]
fn vm_slice5_classes_match_interpreter() {
    use std::process::Command;
    let cases = [
        ("11-classes.نيفا", "٣\n٤\n٧\n١٣\n١٣\n"),
        ("12-inheritance-interfaces.نيفا", "طالب: محمود\nيدرس\nمحمود\n"),
    ];
    for (name, expected) in cases {
        let path = example_path(name);
        let out = Command::new(bin())
            .arg(&path)
            .arg("--افتراضي")
            .output()
            .expect("run نيفا --افتراضي");
        let out2 = Command::new(bin()).arg(&path).output().expect("run نيفا");
        assert_eq!(out.status.code().unwrap_or(-1), 0, "{} vm stderr: {}", name, String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), expected, "vm {}", name);
        assert_eq!(String::from_utf8_lossy(&out2.stdout).to_string(), expected, "interp {}", name);
    }
}

#[test]
fn word_operators_work_in_interpreter_and_vm() {
    // اجتماع 19: كلمات المقارنة تنتج نفس عقد AST الرموز،
    // فيعمل المفسر وVM بلا تغيير فيهما.
    let src = "اطبع ٥ يساوي ٥\nاطبع ٥ لا يساوي ٦\nاطبع ٧ أكبر من ٥\nاطبع ٣ أصغر من ٥\nاطبع ٥ أكبر من أو يساوي ٥\nاطبع ٥ أصغر من أو يساوي ٦\nاطبع ٥ أكبر من أو يساوي ٦\nاطبع ٥ أكبر من ٧ أو ٥ يساوي ٥\n";
    let expected = "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\nصحيح\nخطأ\nصحيح\n";
    let (out, _, code) = run_src(src);
    assert_eq!(code, 0);
    assert_eq!(out, expected);
    // --افتراضي: نفس المخرجات (نفس عقد AST).
    let dir = std::env::temp_dir().join(format!("niva-vm-words-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, src).unwrap();
    let vm = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(vm.status.code().unwrap_or(-1), 0, "vm stderr={}", String::from_utf8_lossy(&vm.stderr));
    assert_eq!(String::from_utf8_lossy(&vm.stdout), expected);
}

#[test]
fn word_operators_with_not_and_or() {
    // ليس تربط أضعف من المقارنة حتى مع الكلمات، و«أو» بعد
    // «أكبر من» تبقى أو منطقية لا جزءا من العامل.
    let (out, _, code) = run_src(
        "س = ١٠\nإذا ليس س يساوي ٩٩:\n    اطبع \"ليس ٩٩\"\nاطبع ١ أكبر من ٢ أو ٣ أصغر من ٤\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "ليس ٩٩\nصحيح\n");
}

#[test]
#[cfg(feature = "prototype-rtl")]
fn reversed_assignment_prototype() {
    // نموذج صيغة RTL تجريبي: التعيين المعكوس «قيمة = اسم»
    // — يعمل فقط عند تفعيل الميزة، والصيغة العادية تبقى.
    let (out, _, code) = run_src(
        "٥ = س\n٣ = ص\nاطبع س + ص\nإذا س أكبر من ص:\n    اطبع \"س أكبر\"\n٧ = ع\nاطبع ع * ٢\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "٨\nس أكبر\n١٤\n");
    // والصيغة العادية لا تتأثر: «س = ص» تعيين عادي
    // (الهدف س، والقيمة ص) لا معكوس.
    let (out2, _, code2) = run_src("س = ٥\nص = س\nاطبع ص\n");
    assert_eq!(code2, 0);
    assert_eq!(out2, "٥\n");
    // والقيمة المعقدة تعمل: نتيجة دالة = اسم.
    let (out3, _, code3) = run_src(
        "دالة جمع(أ، ب):\n    ارجع أ + ب\nجمع(٢، ٣) = ناتج\nاطبع ناتج\n",
    );
    assert_eq!(code3, 0);
    assert_eq!(out3, "٥\n");
    // والحد: الطرف الأيمن ليس اسما مفردا فليس معكوسا.
    let (_, err, code4) = run_src("٥ = قائمة[٠]\n");
    assert_ne!(code4, 0);
    assert!(err.contains("هدف التعيين غير صالح"), "stderr: {}", err);
    // --افتراضي: نفس العقدة، نفس المخرجات.
    let dir = std::env::temp_dir().join(format!("niva-vm-revassign-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "٥ = س\n٣ = ص\nاطبع س + ص\n").unwrap();
    let vm = Command::new(bin()).arg(&path).arg("--افتراضي").output().expect("run نيفا --افتراضي");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(vm.status.code().unwrap_or(-1), 0, "vm stderr={}", String::from_utf8_lossy(&vm.stderr));
    assert_eq!(String::from_utf8_lossy(&vm.stdout), "٨\n");
}

// Public build (no prototype feature): the reversed form is
// rejected with the normal error at the binary level — this is
// the language the console and the public repo ship.
#[test]
#[cfg(not(feature = "prototype-rtl"))]
fn reversed_assignment_rejected_without_feature() {
    let (_, err, code) = run_src("٥ = س\n");
    assert_ne!(code, 0);
    assert!(err.contains("هدف التعيين غير صالح"), "stderr: {}", err);
    // The normal form still works.
    let (out, _, code2) = run_src("س = ٥\nص = ٣\nاطبع س + ص\n");
    assert_eq!(code2, 0);
    assert_eq!(out, "٨\n");
}

// --- Meeting 22: شبكة (network) at the CLI level ---
// Each test gets its own local HTTP server (127.0.0.1 on an
// ephemeral port). The binary is a separate process per run,
// so the process-wide network policy never leaks between tests.

fn run_src_flags(src: &str, flags: &[&str]) -> (String, String, i32) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "rio4140-engine-net-{}-{}",
        std::process::id(),
        n
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, src).unwrap();
    let mut cmd = Command::new(bin());
    cmd.arg(&path);
    for f in flags {
        cmd.arg(f);
    }
    let result = cmd.current_dir(&dir).output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
        result.status.code().unwrap_or(-1),
    )
}

fn net_test_server() -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut req: Vec<u8> = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if req.len() >= 4 && &req[req.len() - 4..] == b"\r\n\r\n" {
                            break;
                        }
                        if req.len() > 8192 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let req_text = String::from_utf8_lossy(&req).into_owned();
            let path = req_text
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let body = "مرحبا من الخادم".as_bytes().to_vec();
            let (status, extra): (&str, String) = match path.as_str() {
                "/hello" => (
                    "200 OK",
                    format!("Content-Length: {}\r\n", body.len()),
                ),
                _ => ("404 Not Found", "Content-Length: 0\r\n".to_string()),
            };
            let mut resp = format!("HTTP/1.1 {}\r\n{}Connection: close\r\n\r\n", status, extra)
                .into_bytes();
            if path == "/hello" {
                resp.extend_from_slice(&body);
            }
            let _ = stream.write_all(&resp);
        }
    });
    port
}

fn net_program(port: u16) -> String {
    format!(
        "استورد شبكة\nاطبع شبكة.اجلب(\"http://127.0.0.1:{}/hello\")\n",
        port
    )
}

#[test]
fn network_fetch_with_flag() {
    let port = net_test_server();
    let (out, err, code) = run_src_flags(
        &net_program(port),
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "مرحبا من الخادم\n", "stderr: {}", err);
}

#[test]
fn network_permission_refused_without_flag() {
    let port = net_test_server();
    let (_out, err, code) = run_src_flags(&net_program(port), &[]);
    assert_ne!(code, 0);
    assert!(
        err.contains("الشبكة مغلقة افتراضيا"),
        "stderr: {}",
        err
    );
}

#[test]
fn network_https_refused_with_flag() {
    let (_out, err, code) = run_src_flags(
        "استورد شبكة\nاطبع شبكة.اجلب(\"https://example.com/\")\n",
        &["--اسمح-بالشبكة"],
    );
    assert_ne!(code, 0);
    assert!(err.contains("HTTPS غير مدعوم"), "stderr: {}", err);
}

#[test]
fn network_local_host_refused_without_allow_local_host() {
    let port = net_test_server();
    let (_out, err, code) = run_src_flags(&net_program(port), &["--اسمح-بالشبكة"]);
    assert_ne!(code, 0);
    assert!(err.contains("مرفوض"), "stderr: {}", err);
    assert!(err.contains("حلقي"), "stderr: {}", err);
}

#[test]
fn network_flag_does_not_change_file_permissions() {
    // --اسمح-بالشبكة opens the network only: reading a file
    // outside the program directory is still refused without
    // --اسمح-بالكل (the two flags are independent, Meeting 22).
    let (_out, err, code) = run_src_flags(
        "اطبع اقرأ_ملف(\"/etc/passwd\")\n",
        &["--اسمح-بالشبكة"],
    );
    assert_ne!(code, 0);
    assert!(err.contains("خارج مجلد البرنامج"), "stderr: {}", err);
}

#[test]
fn network_unknown_flag_is_arabic_error() {
    let (_out, err, code) = run_src_flags("اطبع ١\n", &["--اسمح-بالشبكةx"]);
    assert_ne!(code, 0);
    assert!(err.contains("خيار غير معروف"), "stderr: {}", err);
}

#[test]
fn network_vm_has_no_modules() {
    // The VM has no modules at all in v1 (stdlib is
    // interpreter-only), so استورد شبكة under --افتراضي fails with
    // the VM's existing module error — اجلب is not silently
    // available there. This test pins that behavior.
    let port = net_test_server();
    let (_out, err, code) = run_src_flags(
        &net_program(port),
        &["--افتراضي", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_ne!(code, 0);
    assert!(
        err.contains("وحدة") || err.contains("غير مدعوم") || err.contains("الوحدات"),
        "stderr: {}",
        err
    );
}

// CLI Arabization (owner's strict rule, 2026-11-05): every flag,
// the version and the help text are Arabic; Latin flags are
// rejected with an Arabic error. The binary is still built as
// `niva` by cargo (ASCII-only bin names) — the installed command
// is نيفا (install.sh copies it).
fn run_args(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(bin()).args(args).output().expect("run نيفا");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn cli_version_is_arabic() {
    let (out, _err, code) = run_args(&["--نسخة"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "محرك نيفا ١٫٠٫٠");
}

#[test]
fn cli_help_is_arabic() {
    let (out, _err, code) = run_args(&["--مساعدة"]);
    assert_eq!(code, 0);
    assert!(out.contains("استخدام: نيفا"), "got: {}", out);
    for flag in [
        "--مساعدة",
        "--نسخة",
        "--تفاعلي",
        "--شجرة",
        "--افتراضي",
        "--اسمح-بالكل",
        "--اسمح-بالشبكة",
        "--مضيف-محلي",
    ] {
        assert!(out.contains(flag), "help missing {}: {}", flag, out);
    }
    // No Latin flag survives in the help text.
    for latin in [
        "--help",
        "--version",
        "--repl",
        "--vm",
        "--ast",
        "--allow-all",
        "--allow-network",
        "--allow-local-host",
    ] {
        assert!(!out.contains(latin), "help still names {}: {}", latin, out);
    }
}

#[test]
fn cli_latin_flags_are_rejected_in_arabic() {
    for latin in [
        "--help", "--version", "--repl", "--vm", "--ast", "--allow-all", "--allow-network",
        "-h",
    ] {
        let (_out, err, code) = run_args(&[latin]);
        assert_ne!(code, 0, "{} was accepted", latin);
        assert!(err.contains("خيار غير معروف"), "{}: {}", latin, err);
    }
}
