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
        (
            "18-عمليات.نيفا",
            "المساعد يعمل\nالفاشل يبدأ\n٠\n١\nصحيح\nمرفوض كما يجب\n",
        ),
        (
            "21-ذاكرة.نيفا",
            "صحيح\nغير معروفة كما يجب\n",
        ),
        (
            "22-ملفات.نيفا",
            "صحيح\nمرحبا من وحدة ملفات\nنص جديد\nغير معروف كما يجب\n",
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

/// مهمة 10 (عمليات): برنامجا نيفا فرعيان (ناجح وفاشل)
/// يُشغَّلان ويُنتظَران في الوضعين — المخرج والخطأ
/// والخروج متطابقة حرفيا (مخرجات الطفلين حية أولا، ثم
/// مخرجات الأب المجمّعة — انظر مثال 18).
#[test]
fn operations_subprogram_both_modes() {
    let files: &[(&str, &str)] = &[
        ("مساعد.نيفا", "اطبع \"المساعد يعمل\"\n"),
        ("فاشل.نيفا", "اطبع \"الفاشل يبدأ\"\nاطبع ١ / ٠\n"),
        (
            "برنامج.نيفا",
            "استورد عمليات\nمعرف_مساعد = عمليات.أنشئ(\"مساعد.نيفا\")\nاطبع عمليات.انتظر(معرف_مساعد)\nمعرف_فاشل = عمليات.انشي(\"فاشل.نيفا\")\nاطبع عمليات.انتظر(معرف_فاشل)\nاطبع عمليات.الحالية() > ٠\nحاول:\n    عمليات.أنشئ(\"/bin/true\")\nماعدا:\n    اطبع \"مرفوض كما يجب\"\n",
        ),
    ];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "المساعد يعمل\nالفاشل يبدأ\n٠\n١\nصحيح\nمرفوض كما يجب\n"
    );
    assert_eq!(
        err_v,
        "خطأ وقت التشغيل: قسمة على صفر (السطر 2، العمود 8)\n"
    );
}

/// تجهيزة Unix لبرامج النظام في اختبارات التكامل: أول موجود
/// من /bin/<الاسم> ثم /usr/bin/<الاسم> — نمط true_program
/// في processes.rs. الغرض هنا تشغيل برامج حقيقية في الوضع
/// الموثوق، لا تخطيط نظام بعينه: /bin/true غائب عن عدّاء
/// macOS 26 (التشغيل 42221442116 — التكامل 78/79 بينما lib
/// خضراء لأن تجهيزتها تنتقل إلى /usr/bin/true)، فالكتابة
/// الثابتة لـ/bin/true تفشل هناك بينما /usr/bin/true موجود.
/// إن غاب الاثنان يفشل الاختبار بصوت عال بدلا من التخطي.
#[cfg(unix)]
fn os_program(name: &str) -> String {
    for dir in ["/bin", "/usr/bin"] {
        let p = format!("{}/{}", dir, name);
        if std::path::Path::new(&p).exists() {
            if dir == "/usr/bin" {
                eprintln!("ملاحظة اختبار: {} غائب عن /bin — يُستخدم {}", name, p);
            }
            return p;
        }
    }
    panic!(
        "لا يوجد برنامج {} على هذا النظام — لا /bin/{} ولا /usr/bin/{}",
        name, name, name
    );
}

/// مهمة 10 (عمليات): برامج نظام حقيقية في الوضع الموثوق
/// (--اسمح-بالكل) — Unix فقط (لا true/false/sleep في Windows).
#[test]
#[cfg(unix)]
fn operations_os_program_trusted_both_modes() {
    let prog_true = os_program("true");
    let prog_false = os_program("false");
    let prog_sleep = os_program("sleep");
    let src = format!(
        "استورد عمليات\nمن عمليات استورد انتظر\nأ = عمليات.أنشئ(\"{}\")\nاطبع انتظر(أ)\nب = عمليات.أنشئ(\"{}\")\nاطبع انتظر(ب)\nج = عمليات.أنشئ(\"{} 30\")\nعمليات.أوقف(ج)\nاطبع \"تم الإيقاف\"\nاطبع عمليات.الحالية() > ٠\n",
        prog_true, prog_false, prog_sleep
    );
    let files: &[(&str, &str)] = &[("برنامج.نيفا", src.as_str())];
    let (out_i, err_i, code_i) = run_with_modules_flags(
        files,
        "برنامج.نيفا",
        &["--اسمح-بالكل", "--مفسر"],
    );
    let (out_v, err_v, code_v) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--اسمح-بالكل"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "٠\n١\nتم الإيقاف\nصحيح\n");
}

/// مهمة 10 (عمليات): الرفض في الوضع المقيد (الافتراضي)
/// — خارج المجلد، واسم غير موجود، ومعرف غير معروف، وأمر
/// فارغ — كلها أخطاء عربية تُمسكها ماعدا، متطابقة في الوضعين.
#[test]
fn operations_permission_refused_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد عمليات\nحاول:\n    عمليات.أنشئ(\"/bin/true\")\nماعدا:\n    اطبع \"مرفوض كما يجب\"\nحاول:\n    عمليات.أنشئ(\"لاوجود\")\nماعدا:\n    اطبع \"غير موجود كما يجب\"\nحاول:\n    عمليات.انتظر(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"غير معروف كما يجب\"\nحاول:\n    عمليات.أنشئ(\"\")\nماعدا:\n    اطبع \"فارغ كما يجب\"\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "مرفوض كما يجب\nغير موجود كما يجب\nغير معروف كما يجب\nفارغ كما يجب\n"
    );
}

/// مهمة 11 (ذاكرة): حجز وتحرير في الوضعين — المخرج
/// والخطأ والخروج متطابقة حرفيا؛ والمعرف المزدوج
/// والحجم غير الصالح وتجاوز الحد أخطاء عربية تُمسكها
/// ماعدا، متطابقة في الوضعين.
#[test]
fn memory_alloc_free_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد ذاكرة\nكتلة = ذاكرة.احجز(١٠٢٤)\nاطبع كتلة > ٠\nذاكرة.حرّر(كتلة)\nحاول:\n    ذاكرة.حرّر(كتلة)\nماعدا:\n    اطبع \"غير معروفة كما يجب\"\nحاول:\n    ذاكرة.حرّر(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"غير معروفة كما يجب\"\nحاول:\n    ذاكرة.احجز(٠)\nماعدا:\n    اطبع \"حجم غير صالح كما يجب\"\nحاول:\n    ذاكرة.احجز(٦٤ * ١٠٢٤ * ١٠٢٤ + ١)\nماعدا:\n    اطبع \"تجاوز الحد كما يجب\"\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "صحيح\nغير معروفة كما يجب\nغير معروفة كما يجب\nحجم غير صالح كما يجب\nتجاوز الحد كما يجب\n"
    );
}

/// مهمة 11 (ذاكرة): الاستعلامان على لينكس (+Android)
/// — قيم موجبة حقيقية من النظام، متطابقة في الوضعين.
/// (المطابقة الدقيقة مع /proc في اختبارات الوحدة.)
#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn memory_queries_linux_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد ذاكرة\nاطبع ذاكرة.المتاح() > ٠\nاطبع ذاكرة.حجم_الصفحة() > ٠\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "صحيح\nصحيح\n");
}

/// مهمة 11 (ذاكرة): الاستعلامان على غير لينكس
/// (macOS/Windows) — رفض صادق بخطأ عربي، متطابق في
/// الوضعين؛ لا قيمة مختلقة ولا انهيار.
#[test]
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn memory_queries_refused_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد ذاكرة\nحاول:\n    ذاكرة.المتاح()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    ذاكرة.حجم_الصفحة()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "غير مدعوم كما يجب\nغير مدعوم كما يجب\n");
}

/// مهمة 12 (ملفات): فتح وقراءة وكتابة في الوضعين — المخرج
/// والخطأ والخروج متطابقة حرفيا؛ والمعرف غير المعروف
/// والخروج عن المجلد والمجلد والمجلد الأب الغائب والوسائط
/// الخاطئة أخطاء عربية تُمسكها ماعدا، متطابقة في الوضعين.
#[test]
fn files_open_read_write_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد ملفات\nم = ملفات.افتح(\"أ.txt\")\nاطبع م > ٠\nملفات.اكتب(م، \"مرحبا\")\nاطبع ملفات.اقرأ(م)\nملفات.اكتب(م، \"جديد\")\nاطبع ملفات.اقرأ(م)\nحاول:\n    ملفات.اقرأ(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"غير معروف كما يجب\"\nحاول:\n    ملفات.افتح(\"../خارج.txt\")\nماعدا:\n    اطبع \"خارج كما يجب\"\nحاول:\n    ملفات.افتح(\".\")\nماعدا:\n    اطبع \"ليس ملفا كما يجب\"\nحاول:\n    ملفات.افتح(\"فرعي/غائب.txt\")\nماعدا:\n    اطبع \"تعذر كما يجب\"\nحاول:\n    ملفات.افتح(٥)\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    ملفات.اقرأ(م، \"زيادة\")\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "صحيح\nمرحبا\nجديد\nغير معروف كما يجب\nخارج كما يجب\nليس ملفا كما يجب\nتعذر كما يجب\nنص كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 12 (ملفات): خطأ الخروج عن المجلد هو الخطأ نفسه
/// حرفيا كـ `اقرأ_ملف` — لا نصان مختلفان لنفس الرفض.
/// (الرسالة نفسها حرفيا؛ والعمود وحده يختلف بصيغة
/// الاستدعاء — الاستدعاء المنقّط يبلّغ عن موضع العضو،
/// والمدمج المسطح عن موضع القوس — فيُقارن النص بلا
/// لاحقة الموضع.)
#[test]
fn files_policy_error_matches_flat_builtin() {
    fn strip_pos(s: &str) -> String {
        match s.rfind(" (السطر ") {
            Some(i) => s[..i].to_string(),
            None => s.to_string(),
        }
    }
    let prog_a: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد ملفات\nملفات.افتح(\"../خارج.txt\")\n",
    )];
    // Program B has no import line: pad it so the failing
    // call sits on line 2, exactly like program A.
    let prog_b: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "# سطر\nاقرأ_ملف(\"../خارج.txt\")\n",
    )];
    let (out_ai, err_ai, code_ai) =
        run_with_modules_flags(prog_a, "برنامج.نيفا", &["--مفسر"]);
    let (out_av, err_av, code_av) = run_with_modules(prog_a, "برنامج.نيفا");
    let (out_bi, err_bi, code_bi) =
        run_with_modules_flags(prog_b, "برنامج.نيفا", &["--مفسر"]);
    let (out_bv, err_bv, code_bv) = run_with_modules(prog_b, "برنامج.نيفا");
    for (label, code) in [
        ("A-interp", code_ai),
        ("A-vm", code_av),
        ("B-interp", code_bi),
        ("B-vm", code_bv),
    ] {
        assert_ne!(code, 0, "{} must fail", label);
    }
    assert_eq!(err_av, err_ai, "A differs across engines");
    assert_eq!(err_bv, err_bi, "B differs across engines");
    assert_eq!(
        strip_pos(&err_av),
        strip_pos(&err_bv),
        "policy error differs between ملفات.افتح and اقرأ_ملف:\n--- A: {}\n--- B: {}",
        err_av, err_bv
    );
    assert!(err_av.contains("خارج مجلد البرنامج"), "stderr: {}", err_av);
    let _ = (out_ai, out_av, out_bi, out_bv);
}

/// تثبيت أدوات نيفا: البرنامج ينشئ مصدره بنفسه (مجلد
/// فرعي + ملف وحدة)، فيثبّته بالمسار المحلي، فيستورده،
/// والثانية مرفوضة — المخرج والخطأ والخروج متطابقة
/// حرفيا في الوضعين.
#[test]
fn install_local_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "أنشئ_مجلد(\"مصادر\")\nاكتب_ملف(\"مصادر/أداة.نيفا\", \"صدر قيمة\\nقيمة = ٤٢\\n\")\nاطبع تثبيت(\"مصادر/أداة.نيفا\")\nاستورد أداة\nاطبع أداة.قيمة\nحاول:\n    تثبيت(\"مصادر/أداة.نيفا\")\nماعدا:\n    اطبع \"مثبّتة كما يجب\"\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "ثُبّتت الوحدة أداة\n٤٢\nمثبّتة كما يجب\n"
    );
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
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "٤٩٩\n");
    let prog2 = prog.replace("عد(499)", "عد(500)");
    std::fs::write(&path, prog2).unwrap();
    let out2 = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = std::fs::remove_dir_all(&dir);
    assert_ne!(out2.status.code().unwrap_or(0), 0);
    // مهمة 6 (بوابة التكافؤ ك-1.5): رسالة حد العمق
    // في VM هي رسالة المفسر نفسها حرفيا — اللاحقة
    // «في VM» القديمة أزيلت؛ التكافؤ الكامل في
    // tests/parity_gate.rs (parity_depth_limit).
    assert!(
        String::from_utf8_lossy(&out2.stderr)
            .contains("تجاوز حد عمق الاستدعاء (500). أوقف الاستدعاء الذاتي أو قلل عمقه"),
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
    run_with_modules_flags(files, main, &[])
}

/// مهمة 4 (ك-1.3): مثل run_with_modules لكن بأعلام
/// إضافية (مثل --مفسر)، لمطابقة الوحدات في VM.
fn run_with_modules_flags(
    files: &[(&str, &str)],
    main: &str,
    flags: &[&str],
) -> (String, String, i32) {
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
    let mut cmd = Command::new(bin());
    cmd.arg(dir.join(main));
    for flag in flags {
        cmd.arg(flag);
    }
    let result = cmd.current_dir(&dir).output().expect("run نيفا");
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

// مهمة 4 (ك-1.3): الوحدات في VM — نفس سيناريوهات
// CALC_MODULE في الوضعين: نفس المخرجات أو نفس الخطأ.

#[test]
fn module_import_dot_from_and_alias_vm_parity() {
    let files: &[(&str, &str)] = &[
        ("حساب.نيفا", CALC_MODULE),
        (
            "برنامج.نيفا",
            "استورد حساب\nاطبع حساب.اجمع(٣، ٤)\nاطبع حساب.احصل()\nمن حساب استورد اجمع\nاطبع اجمع(١، ٢)\nاستورد حساب باسم ح\nاطبع ح.احصل()\nاطبع حساب\n",
        ),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٢٠\n١٠\n١٤\n١٠\n<وحدة حساب>\n");
}

#[test]
fn module_private_and_missing_and_cycle_vm_parity() {
    // Private name: the same «غير مصدر» error in both modes.
    let files: &[(&str, &str)] = &[
        ("حساب.نيفا", CALC_MODULE),
        ("برنامج.نيفا", "استورد حساب\nاطبع حساب.سر\n"),
    ];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("غير مصدر"), "stderr: {}", err_i);
    assert_eq!(err_v, err_i, "VM error differs from interpreter");
    // Missing module. (Full stderr equality is impossible here:
    // each run uses its own temp dir, and the error names the
    // searched path — so compare the message content, not the
    // path.)
    let files: &[(&str, &str)] = &[("برنامج.نيفا", "استورد لاوجود\n")];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("وحدة غير موجودة"), "stderr: {}", err_i);
    assert!(err_i.contains("لاوجود"), "stderr: {}", err_i);
    assert!(err_v.contains("وحدة غير موجودة"), "stderr: {}", err_v);
    assert!(err_v.contains("لاوجود"), "stderr: {}", err_v);
    // Cycle.
    let files: &[(&str, &str)] = &[
        ("أ.نيفا", "صدّر من_أ\nاستورد ب\nدالة من_أ():\n    ارجع ١\n"),
        ("ب.نيفا", "صدّر من_ب\nاستورد أ\nدالة من_ب():\n    ارجع ٢\n"),
        ("برنامج.نيفا", "استورد أ\n"),
    ];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("استيراد دائري"), "stderr: {}", err_i);
    assert_eq!(err_v, err_i, "VM error differs from interpreter");
}

#[test]
fn module_load_once_and_mutation_vm_parity() {
    // Load-once: the module top level runs exactly once.
    let files: &[(&str, &str)] = &[
        ("مرة.نيفا", "صدّر قيمة\nاطبع \"تحميل مرة\"\nقيمة = ٧\n"),
        (
            "برنامج.نيفا",
            "استورد مرة\nاستورد مرة\nمن مرة استورد قيمة\nاطبع قيمة\n",
        ),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "تحميل مرة\n٧\n");
    // Global mutation does not persist across module calls
    // (fresh scope clone per call, like the interpreter).
    let files: &[(&str, &str)] = &[
        (
            "عداد.نيفا",
            "صدّر زد، اقرأ\nعداد = ٠\nدالة زد():\n    عداد = عداد + ١\n    ارجع عداد\nدالة اقرأ():\n    ارجع عداد\n",
        ),
        (
            "برنامج.نيفا",
            "استورد عداد\nاطبع عداد.زد()\nاطبع عداد.زد()\nاطبع عداد.اقرأ()\n",
        ),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "١\n١\n٠\n");
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
fn type_nil_writable_accept_and_reject() {
    // Meeting 26 (§41.3): لاشيء is a writable type name.
    // Accept: a لاشيء variable holds only لاشيء; a function
    // declared `: لاشيء` returns لاشيء or nothing (bare
    // return, or falling off the end).
    let (out, _, code) = run_src(
        "س: لاشيء = لاشيء\nاطبع س\nدالة بلا_قيمة(): لاشيء:\n    ارجع\nاطبع بلا_قيمة()\nدالة ساقطة(): لاشيء:\n    مرر\nاطبع ساقطة()\nدالة خذ(ق: لاشيء): لاشيء:\n    ارجع ق\nاطبع خذ(لاشيء)\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "لاشيء\nلاشيء\nلاشيء\nلاشيء\n");

    // Reject: a لاشيء variable cannot hold a number.
    let (_, err, code) = run_src("س: لاشيء = ٥\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);

    // Reject: a function declared `: لاشيء` cannot return a number.
    let (_, err, code) = run_src("دالة احصل(): لاشيء:\n    ارجع ٥\nاطبع احصل()\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);

    // Reject: a لاشيء parameter cannot take a number.
    let (_, err, code) = run_src("دالة خذ(ق: لاشيء):\n    ارجع ق\nاطبع خذ(٥)\n");
    assert_ne!(code, 0);
    assert!(err.contains("نوع غير متطابق"), "stderr: {}", err);
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
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code().unwrap_or(-1), 0);
    assert_eq!(String::from_utf8_lossy(&out.stdout).to_string(), "٥\nنعم\n");
}

#[test]
fn vm_lambda_parity_with_interpreter() {
    // Phase ك-1 (Meeting 24): lambdas now WORK in the VM —
    // the same program gives the same output on the
    // interpreter (--مفسر) and on the VM (default).
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("niva-vm-lambda-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    std::fs::write(&path, "مضاعف = دالة_مجهولة(س): س * 2\nاطبع مضاعف(5)\n").unwrap();
    let out_vm = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let out_interp = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out_vm.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out_vm.stderr));
    assert_eq!(out_interp.status.code().unwrap_or(-1), 0);
    assert_eq!(String::from_utf8_lossy(&out_vm.stdout), "١٠\n");
    assert_eq!(String::from_utf8_lossy(&out_vm.stdout), String::from_utf8_lossy(&out_interp.stdout));
}

#[test]
fn vm_control_flow_examples_parity() {
    // ك-1.2: examples 04, 06, 08 give identical outputs on
    // the interpreter (--مفسر) and on the VM (for, repeat, while,
    // break, continue, in — the whole control-flow surface).
    use std::process::Command;
    for name in ["04-loops.نيفا", "06-lists-dicts.نيفا", "08-full-program.نيفا"] {
        let path = example_path(name);
        let out_vm = Command::new(bin()).arg(&path).output().expect("run نيفا");
        let out_interp = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
        assert_eq!(out_vm.status.code().unwrap_or(-1), 0, "{} VM stderr: {}", name, String::from_utf8_lossy(&out_vm.stderr));
        assert_eq!(out_interp.status.code().unwrap_or(-1), 0, "{}", name);
        assert_eq!(
            String::from_utf8_lossy(&out_vm.stdout),
            String::from_utf8_lossy(&out_interp.stdout),
            "{} differs",
            name
        );
    }
}

#[test]
fn vm_files_example07_full_parity() {
    // مهمة 5 (ك-1.4): example 07 كاملا متطابق في الوضعين —
    // كان اختبار تكافؤ جزئي في ك-1.2 لأن ذيله يستخدم
    // اكتب_ملف/اقرأ_ملف؛ الفجوة أُغلقت هنا.
    use std::process::Command;
    let path = example_path("07-try-files.نيفا");
    let out_interp = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
    let out_vm = Command::new(bin()).arg(&path).output().expect("run نيفا");
    assert_eq!(out_interp.status.code().unwrap_or(-1), 0);
    assert_eq!(out_vm.status.code().unwrap_or(-1), 0);
    assert_eq!(
        String::from_utf8_lossy(&out_interp.stdout),
        "خطأ في القسمة\nانتهى\nمرحبا من محرك نيفا\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&out_vm.stdout),
        String::from_utf8_lossy(&out_interp.stdout)
    );
    assert_eq!(out_vm.stderr, out_interp.stderr);
}

/// مهمة 3: شغّل برنامجا صغيرا في الوضعين وأعد
/// ((مخرجات المفسر، خطؤه، خروجه)، (مخرجات VM، خطؤها، خروجها)).
fn run_both_src(src: &str) -> ((String, String, i32), (String, String, i32)) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("niva-vm-task3-{}-{}", std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, src).unwrap();
    let interp = Command::new(bin())
        .arg(&path)
        .arg("--مفسر")
        .current_dir(&dir)
        .output()
        .expect("run نيفا");
    let vm = Command::new(bin())
        .arg(&path)
        
        .current_dir(&dir)
        .output()
        .expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    (
        (
            String::from_utf8_lossy(&interp.stdout).to_string(),
            String::from_utf8_lossy(&interp.stderr).to_string(),
            interp.status.code().unwrap_or(-1),
        ),
        (
            String::from_utf8_lossy(&vm.stdout).to_string(),
            String::from_utf8_lossy(&vm.stderr).to_string(),
            vm.status.code().unwrap_or(-1),
        ),
    )
}

/// مهمة 5: مثل run_both_src لكن بمجلدين متطابقين
/// (ملفات بذر واحدة) — البرنامج قد يكتب/يحذف، فتشغيل
/// الوضعين في المجلد نفسه يجعل الثاني يرى آثار الأول.
fn run_both_src_two_dirs(
    src: &str,
    files: &[(&str, &str)],
) -> ((String, String, i32), (String, String, i32)) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("niva-vm-task5-{}-{}", std::process::id(), n));
    let dir_i = base.join("مفسر");
    let dir_v = base.join("vm");
    for dir in [&dir_i, &dir_v] {
        fs::create_dir_all(dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        fs::write(dir.join("prog.نيفا"), src).unwrap();
    }
    let run = |dir: &std::path::Path, vm: bool| -> (String, String, i32) {
        let mut cmd = Command::new(bin());
        if !vm {
            cmd.arg("--مفسر");
        }
        let out = cmd
            .arg(dir.join("prog.نيفا"))
            .current_dir(dir)
            .output()
            .expect("run نيفا");
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.code().unwrap_or(-1),
        )
    };
    let i = run(&dir_i, false);
    let v = run(&dir_v, true);
    let _ = fs::remove_dir_all(&base);
    (i, v)
}

/// مهمة 5: مثل run_both_src_two_dirs لكن مع stdin
/// موصول (لاختبار ادخل) — نفس الإدخال للوضعين.
fn run_both_src_two_dirs_stdin(
    src: &str,
    files: &[(&str, &str)],
    stdin: &str,
) -> ((String, String, i32), (String, String, i32)) {
    use std::io::Write as _;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("niva-vm-task5-stdin-{}-{}", std::process::id(), n));
    let dir_i = base.join("مفسر");
    let dir_v = base.join("vm");
    for dir in [&dir_i, &dir_v] {
        fs::create_dir_all(dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        fs::write(dir.join("prog.نيفا"), src).unwrap();
    }
    let run = |dir: &std::path::Path, vm: bool| -> (String, String, i32) {
        let mut cmd = Command::new(bin());
        if !vm {
            cmd.arg("--مفسر");
        }
        let mut child = cmd
            .arg(dir.join("prog.نيفا"))
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn نيفا");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("run نيفا");
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.code().unwrap_or(-1),
        )
    };
    let i = run(&dir_i, false);
    let v = run(&dir_v, true);
    let _ = fs::remove_dir_all(&base);
    (i, v)
}

#[test]
fn vm_files_calc_module_and_input_parity() {
    // مهمة 5: حساب (وحدة) + ادخل — نفس المخرجات في
    // الوضعين مع الإدخال نفسه من stdin.
    let calc = "صدّر اجمع، ضعف\nدالة اجمع(أ، ب):\n    ارجع أ + ب\nدالة ضعف(س):\n    ارجع س * ٢\n";
    let src = "استورد حساب\nاطبع حساب.اجمع(٣، ٤)\nاطبع حساب.ضعف(٢١)\nاسم = ادخل(\"ما اسمك؟ \")\nاطبع اسم\nاطبع حساب.اجمع(١٠، ٥)\n";
    let ((out_i, _, code_i), (out_v, err_v, code_v)) =
        run_both_src_two_dirs_stdin(src, &[("حساب.نيفا", calc)], "محمود\n");
    assert_eq!(code_i, 0);
    assert_eq!(code_v, 0, "vm stderr: {}", err_v);
    assert_eq!(out_i, "ما اسمك؟ ٧\n٤٢\nمحمود\n١٥\n");
    assert_eq!(out_v, out_i);
}

#[test]
fn vm_files_outside_dir_refusal_cli_parity() {
    // مهمة 5: رفض خارج المجلد عبر سطر الأوامر — نفس
    // الخطأ العربي ونفس الخروج 1 في الوضعين؛ ومع
    // --اسمح-بالكل تنجح القراءة في الوضعين.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!(
        "niva-vm-task5-refuse-{}-{}",
        std::process::id(),
        n
    ));
    let dir_i = base.join("مفسر");
    let dir_v = base.join("vm");
    for dir in [&dir_i, &dir_v] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("prog.نيفا"), "اطبع اقرأ_ملف(\"../سر.txt\")\n").unwrap();
    }
    fs::write(base.join("سر.txt"), "سرّي\n").unwrap();
    let run = |dir: &std::path::Path, vm: bool, trusted: bool| -> (String, String, i32) {
        let mut cmd = Command::new(bin());
        if !vm {
            cmd.arg("--مفسر");
        }
        if trusted {
            cmd.arg("--اسمح-بالكل");
        }
        let out = cmd
            .arg(dir.join("prog.نيفا"))
            .current_dir(dir)
            .output()
            .expect("run نيفا");
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.code().unwrap_or(-1),
        )
    };
    let (out_i, err_i, code_i) = run(&dir_i, false, false);
    let (out_v, err_v, code_v) = run(&dir_v, true, false);
    assert_eq!(code_i, 1);
    assert_eq!(code_v, 1);
    assert_eq!(out_i, out_v);
    assert_eq!(err_i, err_v);
    assert!(err_i.contains("خارج مجلد البرنامج"), "stderr: {}", err_i);
    let (out_i, _, code_i) = run(&dir_i, false, true);
    let (out_v, _, code_v) = run(&dir_v, true, true);
    assert_eq!(code_i, 0);
    assert_eq!(code_v, 0);
    assert_eq!(out_i, "سرّي\n\n");
    assert_eq!(out_v, out_i);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn vm_files_broad_parity() {
    // مهمة 5: برنامج واحد يمرّ على معظم دوال الملفات —
    // كتابة وقراءة ومجلدات وبحث وشجرة ونسخ ونقل وحذف.
    let src = "اكتب_ملف(\"نص.txt\"، \"ألف\\nباء\\nجيم\\n\")\nاطبع اعرض_البداية(\"نص.txt\"، ٢)\nاطبع عد_الأسطر(\"نص.txt\")\nاطبع عد_الكلمات(\"نص.txt\")\nاطبع ابحث_في_ملف(\"باء\"، \"نص.txt\")\nأنشئ_مجلد(\"فرعي\")\nانتقل(\"فرعي\")\nاكتب_ملف(\"داخل.txt\"، \"داخل\")\nاطبع المجلد_الحالي()\nانتقل(\"..\")\nاطبع اعرض_الشجرة()\nاطبع ابحث_عن(\"نص\")\nاطبع حجم_المسار(\"نص.txt\")\nانسخ(\"نص.txt\"، \"نسخة.txt\")\nانقل(\"نسخة.txt\"، \"منقول.txt\")\nاطبع اقرأ_ملف(\"منقول.txt\")\nاحذف_ملف(\"منقول.txt\")\nاطبع موجود(\"منقول.txt\")\nاطبع النسخة()\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src_two_dirs(src, &[]);
    assert_eq!(code_i, 0, "interpreter stderr: {}", err_i);
    assert_eq!(code_v, 0, "vm stderr: {}", err_v);
    assert_eq!(out_i, out_v);
    assert_eq!(
        out_i,
        "ألف\nباء\n٣\n٣\n[\"باء\"]\n~/فرعي\n[\"prog.نيفا\", \"فرعي/\", \"  داخل.txt\", \"نص.txt\"]\n[\"نص.txt\"]\n٢١\nألف\nباء\nجيم\n\nخطأ\nمحرك نيفا ١٫٠٫٠\n"
    );
}

#[test]
fn methods_vm_parity_with_interpreter() {
    // مهمة 3: طرق النصوص تعمل في VM كما في المفسر —
    // كان هذا اختبار رفض (طريقة غير مدعومة)، صار
    // اختبار تكافؤ بعد إصلاح VM.
    let ((out_i, _, code_i), (out_v, err_v, code_v)) = run_both_src("اطبع \"مرحبا\".طول()\n");
    assert_eq!(code_i, 0);
    assert_eq!(code_v, 0, "vm stderr: {}", err_v);
    assert_eq!(out_i, "٥\n");
    assert_eq!(out_v, out_i);
}

#[test]
fn vm_methods_example15_matches_interpreter() {
    // مهمة 3: المثال 15 يعمل في الوضعين بمخرجات متطابقة.
    let path = example_path("15-methods.نيفا");
    let expected = "٥\nHELLO\nhello\nمحمود\nأهلا سارة\n[\"أ\", \"ب\", \"ج\"]\nصحيح\n٣\n[١, ٢, ٣, ٤]\n٤\n[١, ٢, ٣]\n";
    let out = Command::new(bin())
        .arg(&path)
        .output()
        .expect("run نيفا");
    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "vm stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn vm_method_scope_parity_with_interpreter() {
    // مهمة 3: الطرق تلتقط نطاق تعريفها في الوضعين.
    let cases: &[(&str, &str)] = &[
        (
            "س = ٥\nفئة عداد:\n    دالة اقرأ():\n        ارجع س\nع = أنشئ عداد()\nاطبع ع.اقرأ()\n",
            "٥\n",
        ),
        (
            "دالة اصنع():\n    س = ٧\n    فئة ص:\n        دالة اقرأ():\n            ارجع س\n    ارجع أنشئ ص()\nع = اصنع()\nاطبع ع.اقرأ()\n",
            "٧\n",
        ),
        (
            "دالة اصنع():\n    س = ٠\n    فئة عداد:\n        دالة زد():\n            س = س + ١\n            ارجع س\n    ارجع أنشئ عداد()\nع = اصنع()\nاطبع ع.زد()\nاطبع ع.زد()\n",
            "١\n٢\n",
        ),
        (
            "دالة اصنع_أساس():\n    س = ١١\n    فئة أساس:\n        دالة اقرأ(): عدد:\n            ارجع س\n    ارجع أساس\nأساس = اصنع_أساس()\nفئة فرعي ورث أساس:\n    دالة زد():\n        ارجع ١\nع = أنشئ فرعي()\nاطبع ع.اقرأ()\nاطبع ع.زد()\n",
            "١١\n١\n",
        ),
    ];
    for (src, expected) in cases {
        let ((out_i, _, code_i), (out_v, err_v, code_v)) = run_both_src(src);
        assert_eq!(code_i, 0, "interp failed: {}", src);
        assert_eq!(code_v, 0, "vm failed: {} stderr: {}", src, err_v);
        assert_eq!(out_i, *expected, "interp output: {}", src);
        assert_eq!(out_v, *expected, "vm output: {}", src);
    }
}

#[test]
fn vm_return_type_parity_with_interpreter() {
    // مهمة 3: فحص نوع الإرجاع — الرسائل نفسها في الوضعين.
    let cases: &[&str] = &[
        "دالة احصل(): عدد:\n    س = \"نص\"\n    ارجع س\nاطبع احصل()\n",
        "فئة ص:\n    دالة قيمة(): عدد:\n        س = \"نص\"\n        ارجع س\nع = أنشئ ص()\nاطبع ع.قيمة()\n",
        "دالة احصل(): عدد:\n    مرر\nاطبع احصل()\n",
        "فئة ص:\n    دالة قيمة(): عدد:\n        مرر\nع = أنشئ ص()\nاطبع ع.قيمة()\n",
        "فئة ص:\n    دالة خذ(س: عدد):\n        ارجع س\nع = أنشئ ص()\nق = \"نص\"\nاطبع ع.خذ(ق)\n",
    ];
    for src in cases {
        let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
        assert_ne!(code_i, 0, "interp should fail: {}", src);
        assert_eq!(code_v, code_i, "vm exit differs: {}", src);
        assert_eq!(out_v, out_i, "vm stdout differs: {}", src);
        assert_eq!(
            err_v, err_i,
            "vm stderr differs: {}\ninterp: {}\nvm: {}",
            src, err_i, err_v
        );
    }
    // والصحيح يمر في الوضعين.
    let ((out_i, _, code_i), (out_v, _, code_v)) = run_both_src(
        "دالة نقل(ق: أي): أي:\n    ارجع ق\nاطبع نقل(٧)\nاطبع نقل(\"أ\")\nدالة جمع(أ: عدد، ب: عدد): عدد:\n    ارجع أ + ب\nاطبع جمع(٣، ٤)\n",
    );
    assert_eq!(code_i, 0);
    assert_eq!(code_v, 0);
    assert_eq!(out_i, "٧\nأ\n٧\n");
    assert_eq!(out_v, out_i);
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
fn stdlib_vm_parity_with_interpreter() {
    // Meeting 25 task 4 (ك-1.3): الوحدات والمكتبة القياسية
    // تعمل الآن في VM — المثالان 09 (وحدات ملفية) و16
    // (مكتبة أصلية) يعطيان المخرجات نفسها في الوضعين.
    for (name, expected) in [
        ("09-modules.نيفا", "٧\n٤٢\n٣\n"),
        (
            "16-stdlib.نيفا",
            "٤\n١٠٢٤\n٣\n٤\n٣٫١٤١٥٩٢٦٥٣٥٨٩٧٩٣\n٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥\n٥\nصحيح\nصحيح\nصحيح\nصحيح\n",
        ),
    ] {
        let path = example_path(name);
        // الافتراضي (بلا علم) هو VM الآن؛ و--مفسر هو المفسر.
        let (out_v, err_v, code_v) = run_file(path.to_str().unwrap());
        assert_eq!(code_v, 0, "{} VM failed: stderr={}", name, err_v);
        assert_eq!(out_v, expected, "VM output mismatch for {}", name);
        let out_i = Command::new(bin())
            .arg(&path)
            .arg("--مفسر")
            .output()
            .expect("run نيفا");
        assert_eq!(
            out_i.status.code().unwrap_or(-1),
            0,
            "{} interpreter failed: stderr={}",
            name,
            String::from_utf8_lossy(&out_i.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out_i.stdout),
            expected,
            "interpreter output mismatch for {}",
            name
        );
    }
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
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let out2 = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
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
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let out2 = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
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
fn repl_clear_history_keeps_variables() {
    // Task 2 (Meeting 25 plan): the engine half of
    // :امسح-المحفوظات — the :اعرض lines disappear, the
    // variables stay. Driven through the real binary via
    // stdin, like repl_session_via_stdin.
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn niva repl");
    let input = "س = ٥\nاطبع س\n:اعرض\n:امسح-المحفوظات\n:اعرض\nاطبع س\n:خروج\n";
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait repl");
    assert_eq!(out.status.code().unwrap_or(-1), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    // The first :اعرض shows the session lines.
    assert!(stdout.contains("س = ٥"), "stdout: {}", stdout);
    // The command confirms in Arabic.
    assert!(stdout.contains("مسحت محفوظات الجلسة."), "stdout: {}", stdout);
    // After the confirmation, the old lines are gone: the
    // second :اعرض prints only its header, and no `س = ٥`
    // appears anywhere past the confirmation.
    let after = stdout
        .split_once("مسحت محفوظات الجلسة.")
        .expect("confirmation line present")
        .1;
    assert!(!after.contains("س = ٥"), "stdout after clear: {}", after);
    assert!(after.contains("ما كتبته في هذه الجلسة:"), "stdout after clear: {}", after);
    // The variable survived the clear: اطبع س still prints ٥.
    assert!(after.contains("٥\n"), "stdout after clear: {}", after);
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
    let out = Command::new(bin()).arg(&path).output().expect("run نيفا");
    let out2 = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
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
            .output()
            .expect("run نيفا");
        let out2 = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
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
    // --مفسر: نفس المخرجات (نفس عقد AST).
    let dir = std::env::temp_dir().join(format!("niva-vm-words-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, src).unwrap();
    let interp = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(interp.status.code().unwrap_or(-1), 0, "interp stderr={}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(String::from_utf8_lossy(&interp.stdout), expected);
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
    // --مفسر: نفس العقدة، نفس المخرجات.
    let dir = std::env::temp_dir().join(format!("niva-vm-revassign-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prog.نيفا");
    fs::write(&path, "٥ = س\n٣ = ص\nاطبع س + ص\n").unwrap();
    let interp = Command::new(bin()).arg(&path).arg("--مفسر").output().expect("run نيفا");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(interp.status.code().unwrap_or(-1), 0, "interp stderr={}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(String::from_utf8_lossy(&interp.stdout), "٨\n");
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

/// A TLS-only test server (self-signed fixture cert) for the
/// binary-level tests: the binary cannot inject a test root,
/// so a self-signed certificate must be rejected with the
/// Arabic certificate error in both modes (Meeting 26).
#[cfg(feature = "https")]
fn net_tls_test_server() -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let cert_der = include_bytes!("fixtures/tls-selfsigned-cert.der").to_vec();
    let key_der = include_bytes!("fixtures/tls-selfsigned-key.der").to_vec();
    let cert = rustls::pki_types::CertificateDer::from(cert_der);
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(key_der),
    );
    let config = std::sync::Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("test TLS server cert"),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let conn = match rustls::ServerConnection::new(std::sync::Arc::clone(&config)) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let mut tls = rustls::StreamOwned::new(conn, stream);
            // Read the request, then answer (the client never
            // gets here: the handshake fails on the certificate
            // first — but a tolerant client would see /hello).
            let mut req: Vec<u8> = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match tls.read(&mut buf) {
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
            let body = "مرحبا من الخادم".as_bytes().to_vec();
            let mut resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            resp.extend_from_slice(&body);
            let _ = tls.write_all(&resp);
        }
    });
    port
}

#[test]
fn network_fetch_with_flag() {
    let port = net_test_server();
    let (out, err, code) = run_src_flags(
        &net_program(port),
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
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

// Meeting 26: without the `https` feature, HTTPS is refused
// before any traffic. With the feature, this same fetch
// attempts a real TLS connection (see the parity test below).
#[test]
#[cfg(not(feature = "https"))]
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
fn network_fetch_parity_in_vm() {
    // Meeting 25 task 4 (ك-1.3): شبكة.اجلب يعمل الآن في VM
    // عبر جلب المحرك نفسه وبوابته العملية-الواسعة —
    // الجسم نفسه، وخطأ البوابة المغلقة نفسه، وخطأ HTTPS
    // نفسه، في الوضعين.
    let port = net_test_server();
    let prog = net_program(port);
    // مسموح: الجسم نفسه في الوضعين.
    let (out_i, err_i, code_i) = run_src_flags(
        &prog,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &prog,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "مرحبا من الخادم\n");
    // البوابة مغلقة: خطأ المفسر نفسه في الوضعين.
    let (_, err_i2, code_i2) = run_src_flags(&prog, &["--مفسر"]);
    let (_, err_v2, code_v2) = run_src_flags(&prog, &[]);
    assert_ne!(code_i2, 0);
    assert_ne!(code_v2, 0);
    assert!(err_i2.contains("الشبكة مغلقة افتراضيا"), "stderr: {}", err_i2);
    assert_eq!(err_v2, err_i2, "VM gate error differs from interpreter");
    // HTTPS: الخطأ العربي نفسه في الوضعين. مع الميزة،
    // العميل يتحدث TLS إلى خادم موقَّع ذاتيا فتُرفض الشهادة؛
    // وبدونها، HTTPS مرفوض قبل أي حركة.
    #[cfg(feature = "https")]
    let https_port = net_tls_test_server();
    #[cfg(not(feature = "https"))]
    let https_port = port;
    let https_prog = format!(
        "استورد شبكة\nاطبع شبكة.اجلب(\"https://127.0.0.1:{}/hello\")\n",
        https_port
    );
    let (_, err_i3, code_i3) = run_src_flags(
        &https_prog,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (_, err_v3, code_v3) = run_src_flags(
        &https_prog,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_ne!(code_i3, 0);
    assert_ne!(code_v3, 0);
    #[cfg(feature = "https")]
    assert!(
        err_i3.contains("شهادة الخادم غير موثوقة"),
        "stderr: {}",
        err_i3
    );
    #[cfg(not(feature = "https"))]
    assert!(err_i3.contains("HTTPS غير مدعوم"), "stderr: {}", err_i3);
    assert_eq!(err_v3, err_i3, "VM HTTPS error differs from interpreter");
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
        "--اسمح-بالكل",
        "--اسمح-بالشبكة",
        "--مضيف-محلي",
    ] {
        assert!(out.contains(flag), "help missing {}: {}", flag, out);
    }
    // ك-2 (مهمة 7): --افتراضي أُزيل، و--مفسر علم تشخيص خفي
    // لا يظهر في المساعدة أبدا.
    assert!(!out.contains("--افتراضي"), "help still names --افتراضي: {}", out);
    assert!(!out.contains("--مفسر"), "help leaks hidden --مفسر: {}", out);
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

// ك-2 (مهمة 7): --افتراضي مرفوض بخطأ عربي يقول إن VM هي
// الافتراضي الآن؛ و--مفسر يشغّل المفسر (ملف وREPL).
#[test]
fn cli_default_flag_removed_and_interpreter_flag_runs() {
    let (_out, err, code) = run_args(&["--افتراضي"]);
    assert_ne!(code, 0, "--افتراضي was accepted");
    assert!(
        err.contains("الآلة الافتراضية هي المحرك الافتراضي الآن"),
        "stderr: {}",
        err
    );
    // --مفسر يشغّل الملف على المفسر — المخرج نفسه.
    let (out, _, code) = run_src_flags("اطبع ١ + ٢\n", &["--مفسر"]);
    assert_eq!(code, 0);
    assert_eq!(out, "٣\n");
    // --مفسر مع --مساعدة ما زال يعرض المساعدة.
    let (out, _, code) = run_args(&["--مفسر", "--مساعدة"]);
    assert_eq!(code, 0);
    assert!(out.contains("استخدام: نيفا"), "got: {}", out);
}

#[test]
fn repl_interpreter_flag_session_via_stdin() {
    // --مفسر بلا ملف يدخل REPL على المفسر (تشخيص فقط):
    // المتغيرات تبقى، والتعبير يُطبع، والموجّه نفسه.
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(bin())
        .arg("--مفسر")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn niva repl --مفسر");
    let input = "س = ٥\nس * ٢\n:خروج\n";
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
    assert!(stdout.contains("~ $ "), "stdout: {}", stdout);
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

// --- خادم HTTPS (اجتماع 27) ---
//
// برنامج الخادم يحجب إلى الأبد، فهذه الاختبارات تشغّل
// ثنائي CLI (مخرج موصول بأنبوب)، وتنتظر سطر البدء،
// وتجلب بعميل rustls يثق بشهادة اختبار CA، ثم تقتل
// العملية. البرنامج يعمل بعلم --اسمح-بالخادم وحده
// (بلا --اسمح-بالشبكة): علم الخادم وحده هو المطلوب.

fn server_program(port: u16) -> String {
    format!(
        "استورد شبكة\nدالة عالج(طلب):\n    إذا طلب[\"مسار\"] == \"/dict\":\n        ارجع {{\"حالة\": ٢٠١، \"نص\": \"تم\"، \"ترويسات\": {{\"X-Custom\": \"نعم\"}}}}\n    إذا طلب[\"مسار\"] == \"/err\":\n        ارجع ١ / ٠\n    ارجع \"مسار:\" + طلب[\"مسار\"]\nشبكة.اخدم {} \"server-cert.pem\" \"server-key.pem\" عالج\n",
        port
    )
}

fn server_program_selfsigned(port: u16) -> String {
    format!(
        "استورد شبكة\nدالة عالج(طلب):\n    ارجع \"ذاتي\"\nشبكة.اخدم {} \"server-selfsigned-cert.pem\" \"server-selfsigned-key.pem\" عالج\n",
        port
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn server_pem(name: &str) -> &'static str {
    match name {
        "server-cert.pem" => include_str!("fixtures/server-cert.pem"),
        "server-key.pem" => include_str!("fixtures/server-key.pem"),
        "server-selfsigned-cert.pem" => {
            include_str!("fixtures/server-selfsigned-cert.pem")
        }
        "server-selfsigned-key.pem" => {
            include_str!("fixtures/server-selfsigned-key.pem")
        }
        _ => panic!("unknown server fixture: {}", name),
    }
}

/// شغّل برنامج خادم: يكتب البرنامج + الشهادات في مجلد
/// مؤقت، ويشغّل الثنائي بمخرج موصول، ويعيد (العملية،
/// قناة سطور المخرج، المجلد). القارئ في خيط منفصل
/// لأن المخرج أنبوب والعملية تحجب.
fn spawn_server(
    prog: &str,
    flags: &[&str],
) -> (
    std::process::Child,
    std::sync::mpsc::Receiver<String>,
    PathBuf,
) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let dir = {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "rio4140-engine-server-{}-{}",
            std::process::id(),
            n
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    };
    fs::write(dir.join("prog.نيفا"), prog).unwrap();
    for f in [
        "server-cert.pem",
        "server-key.pem",
        "server-selfsigned-cert.pem",
        "server-selfsigned-key.pem",
    ] {
        fs::write(dir.join(f), server_pem(f)).unwrap();
    }
    let mut cmd = Command::new(bin());
    for f in flags {
        cmd.arg(f);
    }
    let mut child = cmd
        .arg(dir.join("prog.نيفا"))
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn نيفا server");
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok() {
            if line.is_empty() {
                break;
            }
            let _ = tx.send(std::mem::take(&mut line));
            line = String::new();
        }
    });
    (child, rx, dir)
}

/// اجمع سطور المخرج حتى سطر البدء (أو انتهاء المهلة)، ثم
/// استنزف 500ms إضافية (سطر الإعلان — مثل تحذير الموقعة
/// ذاتيا — يأتي بعد سطر البدء مباشرة)؛ يعيد كل السطور.
fn wait_startup(rx: &std::sync::mpsc::Receiver<String>, secs: u64) -> Vec<String> {
    let mut lines = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut started = false;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let wait = if started {
            std::time::Duration::from_millis(500).min(left)
        } else {
            left
        };
        match rx.recv_timeout(wait) {
            Ok(line) => {
                if line.starts_with("الخادم يعمل على") {
                    started = true;
                }
                lines.push(line);
                if started {
                    // استنزاف إضافي واحد فقط: السطر التالي
                    // (الإعلان) أو انتهاء 500ms.
                    match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                        Ok(extra) => lines.push(extra),
                        Err(_) => {}
                    }
                    break;
                }
            }
            Err(_) => break,
        }
    }
    lines
}

fn kill_server(mut child: std::process::Child, dir: &PathBuf) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(dir);
}

/// جلب HTTPS بعميل يثق بشهادة اختبار CA فقط؛ يعيد
/// (رمز الحالة، الجسم).
fn https_fetch(port: u16, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let ca_der = include_bytes!("fixtures/tls-ca-cert.der").to_vec();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(ca_der))
        .unwrap();
    let config = std::sync::Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
    let conn = rustls::ClientConnection::new(config, name).unwrap();
    let mut tls = rustls::StreamOwned::new(conn, stream);
    tls.write_all(
        format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            path
        )
        .as_bytes(),
    )
    .unwrap();
    let mut raw = Vec::new();
    tls.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = match text.split_once("\r\n\r\n") {
        Some((h, b)) => (h, b.to_string()),
        None => (text.as_str(), String::new()),
    };
    let status: u16 = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, body)
}

#[test]
#[cfg(feature = "https")]
fn server_serves_in_both_modes() {
    // المعالج دالة نيفا حقيقية، والبرنامج نفسه في
    // الوضعين، وعلم الخادم وحده (بلا علم الشبكة).
    let port = free_port();
    let prog = server_program(port);
    let mut all = Vec::new();
    for interpreter in [false, true] {
        let flags: &[&str] = if interpreter {
            &["--مفسر", "--اسمح-بالخادم"]
        } else {
            &["--اسمح-بالخادم"]
        };
        let (child, rx, dir) = spawn_server(&prog, flags);
        let startup = wait_startup(&rx, 10);
        assert!(
            startup.iter().any(|l| l.starts_with("الخادم يعمل على")),
            "no startup line, interpreter={}: {:?}",
            interpreter,
            startup
        );
        assert!(
            !startup.iter().any(|l| l.contains("موقعة ذاتيا")),
            "a CA-signed server must not announce self-signed: {:?}",
            startup
        );
        let hello = https_fetch(port, "/hello");
        let dict = https_fetch(port, "/dict");
        let err = https_fetch(port, "/err");
        assert_eq!(hello, (200, "مسار:/hello".to_string()));
        assert_eq!(dict, (201, "تم".to_string()));
        assert_eq!(err, (500, "خطأ في معالج الطلب".to_string()));
        assert!(
            !err.1.contains("قسمة"),
            "the real handler error leaked: {:?}",
            err
        );
        all.push((startup, hello, dict, err));
        kill_server(child, &dir);
    }
    assert_eq!(all[0], all[1], "VM and interpreter server outputs differ");
}

#[test]
fn server_permission_refused_without_flag() {
    let port = free_port();
    let prog = server_program(port);
    for interpreter in [false, true] {
        let flags: &[&str] = if interpreter { &["--مفسر"] } else { &[] };
        let (_out, err, code) = run_src_flags(&prog, flags);
        assert_ne!(code, 0, "interpreter={}", interpreter);
        assert!(
            err.contains("الخادم مغلق افتراضيا"),
            "interpreter={} stderr: {}",
            interpreter,
            err
        );
    }
}

#[test]
fn server_network_flag_alone_is_not_enough() {
    let port = free_port();
    let prog = server_program(port);
    let (_out, err, code) = run_src_flags(&prog, &["--اسمح-بالشبكة"]);
    assert_ne!(code, 0);
    assert!(
        err.contains("الخادم مغلق افتراضيا"),
        "stderr: {}",
        err
    );
}

#[test]
#[cfg(feature = "https")]
fn server_self_signed_announcement_and_client_rejection() {
    let port = free_port();
    let prog = server_program_selfsigned(port);
    let (child, rx, dir) = spawn_server(&prog, &["--اسمح-بالخادم"]);
    let startup = wait_startup(&rx, 10);
    assert!(
        startup.iter().any(|l| l.contains("الشهادة موقعة ذاتيا — للتطوير فقط")),
        "no self-signed announcement: {:?}",
        startup
    );
    // عميل يثق بـ CA الاختبار فقط: يرفض الموقعة ذاتيا.
    let refused = {
        use std::io::Write;
        let ca_der = include_bytes!("fixtures/tls-ca-cert.der").to_vec();
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(ca_der))
            .unwrap();
        let config = std::sync::Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string()).unwrap();
        let conn = rustls::ClientConnection::new(config, name).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, stream);
        tls.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .is_err()
    };
    assert!(refused, "the test-CA client should reject the self-signed server");
    kill_server(child, &dir);
}
