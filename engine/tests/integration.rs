//! Integration tests for the Rio Rust engine.
//! Run with: cargo test
//! Each example in examples/ is run with the real `rio` binary and its
//! stdout is compared exactly. Error tests run small programs from a
//! temporary directory and check the Arabic error, line and column.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> String {
    env!("CARGO_BIN_EXE_rio").to_string()
}

fn run_file(path: &str) -> (String, String, i32) {
    let out = Command::new(bin()).arg(path).output().expect("run rio");
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
    let path = dir.join("prog.rio");
    fs::write(&path, src).unwrap();
    let result = Command::new(bin())
        .arg(&path)
        .current_dir(&dir)
        .output()
        .expect("run rio");
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
            "01-hello.rio",
            "أهلا بك في Rio\n5\nالنتيجة: 20\n",
        ),
        (
            "02-variables.rio",
            "8\n15\nمحمود\nصحيح\nعدد\nنص\n50\n123\n",
        ),
        (
            "03-conditions.rio",
            "ناجح\nفي المدى\nليس ٩٩\n",
        ),
        (
            "04-loops.rio",
            "0\n1\n2\nبعد الحلقة:\n2\n0\n1\n2\nتكرار\nتكرار\n3\n4\n",
        ),
        (
            "05-functions.rio",
            "7\nأهلا محمود\n100\n100\n",
        ),
        (
            "06-lists-dicts.rio",
            "1\n3\n6\n3\n[1, 99, 3]\nمحمود\n23\n3\nاسم\nعمر\nمدينة\nصحيح\nصحيح\n",
        ),
        (
            "07-try-files.rio",
            "خطأ في القسمة\nانتهى\nمرحبا من محرك Rio\n",
        ),
        (
            "08-full-program.rio",
            "محمود: ناجح\nسارة: ممتاز\nعلي: راسب\nالمتوسط:\n72.33333333333333\nأعلى درجة:\n92\nمجموع من الحلقة:\n217\n",
        ),
        (
            "09-modules.rio",
            "7\n42\n3\n",
        ),
        (
            "10-types.rio",
            "5\n7\nمحمود\n",
        ),
        (
            "11-classes.rio",
            "3\n4\n7\n13\n13\n",
        ),
        (
            "12-inheritance-interfaces.rio",
            "طالب: محمود\nيدرس\nمحمود\n",
        ),
    ];
    for (name, expected) in cases {
        let path = example_path(name);
        let (out, err, code) = run_file(path.to_str().unwrap());
        assert_eq!(code, 0, "{} failed: stderr={}", name, err);
        assert_eq!(out, *expected, "output mismatch for {}", name);
    }
    // The file example writes into the current directory; clean it up.
    let _ = fs::remove_file("rio-engine-test.txt");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = fs::remove_file(manifest.join("rio-engine-test.txt"));
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
    assert_eq!(out, "0\n1\n2\n2\n");
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
    // the Rust stack. It must return a Rio error with kind, line, col.
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
    // Depth 100 (well under the 200 limit) must work, and depth
    // must return to zero after a call, so a second call is not
    // falsely rejected.
    let (out, _, code) = run_src(
        "دالة عد(ن):\n    إذا ن == ٠:\n        ارجع ٠\n    وإلا:\n        ارجع عد(ن - ١) + ١\nاطبع عد(١٠٠)\nاطبع عد(١٠)\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "100\n10\n");
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
    assert_eq!(out, "0\n");
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
        .expect("run rio");
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
            ("حساب.rio", CALC_MODULE),
            (
                "برنامج.rio",
                "استورد حساب\nاطبع حساب.اجمع(٣، ٤)\nاطبع حساب.احصل()\nمن حساب استورد اجمع\nاطبع اجمع(١، ٢)\nاستورد حساب باسم ح\nاطبع ح.احصل()\nاطبع حساب\n",
            ),
        ],
        "برنامج.rio",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "20\n10\n14\n10\n<وحدة حساب>\n");
}

#[test]
fn module_private_name_rejected() {
    let (_, err, code) = run_with_modules(
        &[
            ("حساب.rio", CALC_MODULE),
            ("برنامج.rio", "استورد حساب\nاطبع حساب.سر\n"),
        ],
        "برنامج.rio",
    );
    assert_ne!(code, 0);
    assert!(err.contains("غير مصدر"), "stderr: {}", err);
    assert!(err.contains("حساب"), "stderr: {}", err);

    let (_, err, code) = run_with_modules(
        &[
            ("حساب.rio", CALC_MODULE),
            ("برنامج.rio", "من حساب استورد سر\n"),
        ],
        "برنامج.rio",
    );
    assert_ne!(code, 0);
    assert!(err.contains("غير مصدر"), "stderr: {}", err);
}

#[test]
fn module_missing_and_cycle_errors() {
    let (_, err, code) = run_with_modules(
        &[("برنامج.rio", "استورد لاوجود\n")],
        "برنامج.rio",
    );
    assert_ne!(code, 0);
    assert!(err.contains("وحدة غير موجودة"), "stderr: {}", err);
    assert!(err.contains("لاوجود"), "stderr: {}", err);

    let (_, err, code) = run_with_modules(
        &[
            ("أ.rio", "صدّر من_أ\nاستورد ب\nدالة من_أ():\n    ارجع ١\n"),
            ("ب.rio", "صدّر من_ب\nاستورد أ\nدالة من_ب():\n    ارجع ٢\n"),
            ("برنامج.rio", "استورد أ\n"),
        ],
        "برنامج.rio",
    );
    assert_ne!(code, 0);
    assert!(err.contains("استيراد دائري"), "stderr: {}", err);
}

#[test]
fn module_load_once() {
    let (out, err, code) = run_with_modules(
        &[
            ("مرة.rio", "صدّر قيمة\nاطبع \"تحميل مرة\"\nقيمة = ٧\n"),
            (
                "برنامج.rio",
                "استورد مرة\nاستورد مرة\nمن مرة استورد قيمة\nاطبع قيمة\n",
            ),
        ],
        "برنامج.rio",
    );
    assert_eq!(code, 0, "stderr: {}", err);
    assert_eq!(out, "تحميل مرة\n7\n");
}

#[test]
fn module_semantic_analysis_still_applies_to_main() {
    // Main program: import defines حساب, but غيرمعرف is still a
    // semantic error before any output.
    let (out, err, code) = run_with_modules(
        &[
            ("حساب.rio", CALC_MODULE),
            (
                "برنامج.rio",
                "استورد حساب\nاطبع حساب.اجمع(١، ٢)\nاطبع غيرمعرف\n",
            ),
        ],
        "برنامج.rio",
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
    assert_eq!(out, "5\n7\nمرحبا\n");
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
    assert_eq!(out, "نص\n7\nأ\n");
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
                "حساب.rio",
                "صدّر جمع\nدالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\n",
            ),
            (
                "برنامج.rio",
                "استورد حساب\nاطبع حساب.جمع(١، \"نص\")\n",
            ),
        ],
        "برنامج.rio",
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
    assert_eq!(out, "3\n7\n13\n13\n");
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
