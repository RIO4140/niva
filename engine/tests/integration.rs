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

/// تطبيع مسار مجلد مؤقت في المخرجات قبل المقارنة:
/// كل تشغيل اختبار يستخدم مجلدا مؤقتا فريدا (رقم مختلف)،
/// والأخطاء تسمّي الملف الآن (اجتماع 32 Q5) — فيختلف
/// المسار حرفيا بين تشغيلين مع أن الخطأ نفسه واحد.
/// يُستبدل مسار المجلد بـ `<مجلد>`، ويبقى اسم الملف
/// والسطر والعمود كما هما.
///
/// الصيغ الأربع (كما في parity_cases.rs — اجتماعا 35 و36):
/// المقنّنة (ماك ‎/tmp ← /private/tmp‎، ويندوز بادئة
/// `\\?\`) ثم الخام، بكل من صورتي الفاصل؛ ثم إصلاح
/// الفاصل الباقي بعد `<مجلد>` (`<مجلد>\` ← `<مجلد>/`).
///
/// والفاصل المختلط (الجولة 5 — عيب ويندوز في
/// operations_subprogram_both_modes): مسار العملية
/// الفرعية يأتي من `normalize()` في permissions.rs
/// بفاصل أول `/` وباقي `\` (`C:/Users\...`)، فلا تطابقه
/// أي صيغة موحدة — لا الخام ولا المائلة ولا المقنّنة.
/// لذلك تُطابَق نسخة موحّدة الفواصل (`\` ← `/`)، ويُستبدل
/// المدى نفسه في النص الأصل (الشرطتان بايت واحد،
/// فالإزاحات محفوظة)، فيبقى باقي النص كما هو.
fn norm_dir(s: &str, dir: &std::path::Path) -> String {
    let mut out = s.to_string();
    // 1) الصيغة المقنّنة أولا: فالخام لاحقة لها على
    // ويندوز، واستبداله أولا يفسدها ويترك `\\?\`.
    if let Ok(canon) = std::fs::canonicalize(dir) {
        let cn = canon.to_string_lossy().to_string();
        out = out
            .replace(&cn, "<مجلد>")
            .replace(&cn.replace('\\', "/"), "<مجلد>");
    }
    // 2) الصيغة الخام بصورتي الفاصل، ثم الفاصل الباقي.
    let native = dir.display().to_string();
    let slashed = native.replace('\\', "/");
    out = out
        .replace(&native, "<مجلد>")
        .replace(&slashed, "<مجلد>")
        .replace("<مجلد>\\", "<مجلد>/");
    // 3) الفاصل المختلط: طابِق على نسخة موحّدة الفواصل
    // واستبدل المدى نفسه في الأصل.
    loop {
        let unified = out.replace('\\', "/");
        let Some(pos) = unified.find(&slashed) else {
            break;
        };
        let end = pos + slashed.len();
        out.replace_range(pos..end, "<مجلد>");
        out = out.replace("<مجلد>\\", "<مجلد>/");
    }
    out
}

#[test]
fn norm_dir_covers_all_windows_forms() {
    // الجولة 5: رسالة الخطأ على ويندوز تحوي مسار المجلد
    // المؤقت بصيغ لا يطابقها dir.display() وحده — خام
    // (`\`)، ومائلة (`/`)، ومختلطة (`C:/Users\...` من
    // normalize() في permissions.rs). كلها يجب أن تصير
    // `<مجلد>/فاشل.نيفا` كما في توقع الاختبار.
    let dir = PathBuf::from(
        r"C:\Users\RUNNER~1\AppData\Local\Temp\rio4140-engine-module-test-8024-42",
    );
    let want =
        "خطأ وقت التشغيل: قسمة على صفر (الملف <مجلد>/فاشل.نيفا، السطر 2، العمود 8)";
    let forms = [
        r"C:\Users\RUNNER~1\AppData\Local\Temp\rio4140-engine-module-test-8024-42\فاشل.نيفا",
        r"C:/Users/RUNNER~1/AppData/Local/Temp/rio4140-engine-module-test-8024-42/فاشل.نيفا",
        r"C:/Users\RUNNER~1\AppData\Local\Temp\rio4140-engine-module-test-8024-42\فاشل.نيفا",
    ];
    for f in forms {
        let msg = format!(
            "خطأ وقت التشغيل: قسمة على صفر (الملف {}، السطر 2، العمود 8)",
            f
        );
        assert_eq!(norm_dir(&msg, &dir), want, "form: {}", f);
    }
    // الصيغة المقنّنة بمجلد حقيقي (ماك /tmp ← /private/tmp).
    let real = std::env::temp_dir().join("rio4140-engine-norm-test");
    fs::create_dir_all(&real).unwrap();
    let canon = std::fs::canonicalize(&real).unwrap();
    let msg = format!("خطأ (الملف {}/فاشل.نيفا)", canon.display());
    assert_eq!(norm_dir(&msg, &real), "خطأ (الملف <مجلد>/فاشل.نيفا)");
    let _ = fs::remove_dir_all(&real);
    // شرطة خلفية لا علاقة لها بالمسار تبقى كما هي.
    assert_eq!(
        norm_dir(r"نص بلا مسار C:\شيء آخر", &dir),
        r"نص بلا مسار C:\شيء آخر"
    );
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
    let out = norm_dir(
        &String::from_utf8_lossy(&result.stdout).to_string(),
        &dir,
    );
    let err = norm_dir(
        &String::from_utf8_lossy(&result.stderr).to_string(),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (out, err, result.status.code().unwrap_or(-1))
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
            "المساعد يعمل\n٠\nالفاشل يبدأ\n١\nصحيح\nمرفوض كما يجب\n",
        ),
        (
            "21-ذاكرة.نيفا",
            "صحيح\nغير معروفة كما يجب\n",
        ),
        (
            "22-ملفات.نيفا",
            "صحيح\nمرحبا من وحدة ملفات\nنص جديد\nغير معروف كما يجب\n",
        ),
        (
            "23-تشفير.نيفا",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n7a975f75dfe67b4da5d69269c602a827f2c1efeadee4bf1de4029b8b3c91accb\n",
        ),
        (
            "25-غير_متزامن.نيفا",
            "صحيح\nطلبت الكتابة ولم أنتظر\nكتابة\n٣٥\nطلبت القراءة ولم أنتظر\nمرحبا من غير_متزامن\nتم\n",
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
/// والخروج متطابقة حرفيا؛ وبعد البث الحي (اجتماع 32 Q8)
/// يتداخل مخرج المساعد مع مخرجات الأب زمنيا (انظر مثال 18).
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
        "المساعد يعمل\n٠\nالفاشل يبدأ\n١\nصحيح\nمرفوض كما يجب\n"
    );
    assert_eq!(
        err_v,
        "خطأ وقت التشغيل: قسمة على صفر (الملف <مجلد>/فاشل.نيفا، السطر 2، العمود 8)\n"
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

/// مهمة 14 (نظام): جدول النداءات في الوضعين على لينكس
/// (+Android) — العدد موجب، والاسم نص غير فارغ، والرقم
/// غير المعروف والسالب والنص والوسائط الزائدة أخطاء
/// عربية تُمسكها ماعدا، متطابقة في الوضعين.
/// (المطابقة الدقيقة مع جدول النواة في اختبارات الوحدة.)
#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn system_syscall_table_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد نظام\nاطبع نظام.عدد_النداءات() > ٠\nاطبع طول(نظام.اسم_النداء(٠)) > ٠\nحاول:\n    نظام.اسم_النداء(٩٩٩٩٩)\nماعدا:\n    اطبع \"غير معروف كما يجب\"\nحاول:\n    نظام.اسم_النداء(-١)\nماعدا:\n    اطبع \"غير معروف كما يجب\"\nحاول:\n    نظام.اسم_النداء(\"نص\")\nماعدا:\n    اطبع \"عدد كما يجب\"\nحاول:\n    نظام.عدد_النداءات(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
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
        "صحيح\nصحيح\nغير معروف كما يجب\nغير معروف كما يجب\nعدد كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 14 (نظام): استعلامات النواة والهوية على لينكس
/// (+Android) — قيم حقيقية من النظام، متطابقة في
/// الوضعين. (المطابقة الدقيقة مع /proc في اختبارات
/// الوحدة.)
#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn system_kernel_queries_linux_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد نظام\nاطبع طول(نظام.نسخة_النواة()) > ٠\nاطبع نظام.اسم_النظام()\nاطبع نظام.معرف_المستخدم() >= ٠\nاطبع نظام.معرف_المجموعة() >= ٠\n",
    )];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "صحيح\nLinux\nصحيح\nصحيح\n");
}

/// مهمة 14 (نظام): الدوال الست على غير لينكس
/// (macOS/Windows) — رفض صادق بخطأ عربي، متطابق في
/// الوضعين؛ لا قيمة مختلقة ولا انهيار.
#[test]
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn system_queries_refused_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد نظام\nحاول:\n    نظام.اسم_النداء(٠)\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    نظام.عدد_النداءات()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    نظام.نسخة_النواة()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    نظام.اسم_النظام()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    نظام.معرف_المستخدم()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    نظام.معرف_المجموعة()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\n",
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
        "غير مدعوم كما يجب\nغير مدعوم كما يجب\nغير مدعوم كما يجب\nغير مدعوم كما يجب\nغير مدعوم كما يجب\nغير مدعوم كما يجب\n"
    );
}

/// مهمة 15 (وقت): تقويم UTC حتمي في الوضعين —
/// تحويل ثواني ثابتة، وتنسيق، وأسماء، وفرق، وإضافة،
/// وأخطاء تحقق عربية — متطابقة حرفيا في الوضعين
/// على كل المنصات.
#[test]
fn time_calendar_utc_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد وقت\nت = وقت.التاريخ_العالمي(٠)\nاطبع ت[\"سنة\"]\nاطبع ت[\"شهر\"]\nاطبع ت[\"يوم\"]\nاطبع ت[\"ساعة\"]\nاطبع ت[\"دقيقة\"]\nاطبع ت[\"ثانية\"]\nاطبع ت[\"يوم_الأسبوع\"]\nاطبع ت[\"ثواني\"]\nاطبع ت[\"عالمي\"]\nاطبع وقت.نص(ت)\nاطبع وقت.نص_التاريخ(ت)\nاطبع وقت.نص_الوقت(ت)\nاطبع وقت.اسم_اليوم(ت)\nاطبع وقت.اسم_الشهر(وقت.التاريخ_العالمي(١٧٩٣٩٦٦٤٠٠))\nاطبع وقت.اسم_اليوم(وقت.التاريخ_العالمي(١٧٩٣٩٦٦٤٠٠))\nاطبع وقت.من_التاريخ_العالمي(٢٠٢٦، ١١، ٦، ١٢، ٠، ٠)[\"ثواني\"]\nاطبع وقت.فرق(وقت.التاريخ_العالمي(٨٦٤٠٠)، وقت.التاريخ_العالمي(٠))\nاطبع وقت.أضف(وقت.التاريخ_العالمي(٠)، ٣٦٠٠)[\"ساعة\"]\nاطبع وقت.أضف(وقت.التاريخ_العالمي(٠)، -٨٦٤٠٠)[\"يوم\"]\nحاول:\n    وقت.من_التاريخ_العالمي(٢٠٢٦، ١٣، ١، ٠، ٠، ٠)\nماعدا:\n    اطبع \"شهر كما يجب\"\nحاول:\n    وقت.من_التاريخ_العالمي(٢٠٢٣، ٢، ٢٩، ٠، ٠، ٠)\nماعدا:\n    اطبع \"يوم كما يجب\"\nحاول:\n    وقت.نص(٥)\nماعدا:\n    اطبع \"قاموس كما يجب\"\nحاول:\n    وقت.فرق(وقت.التاريخ_العالمي(٠))\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
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
        "١٩٧٠\n١\n١\n٠\n٠\n٠\n٤\n٠\nصحيح\n١٩٧٠-٠١-٠١ ٠٠:٠٠:٠٠\n١٩٧٠-٠١-٠١\n٠٠:٠٠:٠٠\nالخميس\nنوفمبر\nالجمعة\n١٧٩٣٩٦٦٤٠٠\n٨٦٤٠٠\n١\n٣١\nشهر كما يجب\nيوم كما يجب\nقاموس كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 15 (وقت): التقويم المحلي على يونكس —
/// الذهاب والإياب عبر المنطقة المحلية يحفظان
/// الثواني، و«الآن» يوافق التاريخ الحالي، وشكل
/// النص 19 حرفا — متطابق في الوضعين. (القيم
/// المطلقة للمنطقة لا تُثبَّت هنا لأنها تختلف
/// بين الآلات؛ والتحقق الدقيق ضد `date` في
/// الإثبات الحي.)
#[test]
#[cfg(unix)]
fn time_calendar_local_unix_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد وقت\nث = وقت.التاريخ(١٧٩٣٩٦٦٤٠٠)\nاطبع ث[\"ثواني\"]\nث٢ = وقت.من_التاريخ(ث[\"سنة\"]، ث[\"شهر\"]، ث[\"يوم\"]، ث[\"ساعة\"]، ث[\"دقيقة\"]، ث[\"ثانية\"])\nاطبع ث٢[\"ثواني\"] == ث[\"ثواني\"]\nاطبع طول(وقت.نص(وقت.التاريخ())) == ١٩\nاطبع مطلق(وقت.الآن() - وقت.التاريخ()[\"ثواني\"]) < ٥\nاطبع وقت.أضف(ث، ٣٦٠٠)[\"ثواني\"] == ث[\"ثواني\"] + ٣٦٠٠\n",
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
        "١٧٩٣٩٦٦٤٠٠\nصحيح\nصحيح\nصحيح\nصحيح\n"
    );
}

/// مهمة 15 (وقت): التقويم المحلي على Windows —
/// رفض صادق بخطأ عربي (لا ملف TZif هناك)،
/// بينما UTC يعمل — متطابق في الوضعين.
#[test]
#[cfg(windows)]
fn time_local_refused_windows_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد وقت\nحاول:\n    وقت.التاريخ()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    وقت.من_التاريخ(٢٠٢٦، ١١، ٦، ١٢، ٠، ٠)\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nاطبع وقت.التاريخ_العالمي(٠)[\"سنة\"]\nاطبع وقت.أضف(وقت.التاريخ_العالمي(٠)، ٣٦٠٠)[\"ساعة\"]\n",
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
        "غير مدعوم كما يجب\nغير مدعوم كما يجب\n١٩٧٠\n١\n"
    );
}

/// مهمة 16 (تواصل): أنبوب بين عمليتي نيفا حقيقيتين
/// في الوضعين — الأب يفتح قناة إلى برنامج نيفا فرعي
/// (مساعد.نيفا)، يرسل سطرا، يستقبل الرد، يغلق
/// (فيرجع خروج ٠) — المخرج والخطأ والخروج متطابقة
/// حرفيا في الوضعين. (الأرسل يرجع ١١ بايتا: «محمود»
/// خمسة أحرف × بايتان + سطر جديد.)
#[test]
fn ipc_pipe_echo_both_modes() {
    let files: &[(&str, &str)] = &[
        (
            "برنامج.نيفا",
            "استورد تواصل\nق = تواصل.أنبوب(\"مساعد.نيفا\")\nاطبع ق > ٠\nاطبع تواصل.أرسل(ق، \"محمود\")\nاطبع تواصل.استقبل(ق)\nاطبع تواصل.أغلق(ق)\nاطبع \"تم\"\n",
        ),
        (
            "مساعد.نيفا",
            "س = ادخل()\nاطبع \"أهلا \" + س\n",
        ),
    ];
    let (out_i, err_i, code_i) =
        run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "صحيح\n١١\nأهلا محمود\n٠\nتم\n");
}

/// مهمة 16 (تواصل): المعرف غير المعروف والأمر الفارغ
/// في الوضعين — أخطاء عربية تُمسكها ماعدا، متطابقة
/// في الوضعين، بلا نص خطأ نظام تشغيل إنجليزي.
#[test]
fn ipc_unknown_id_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد تواصل\nحاول:\n    تواصل.أرسل(٩٩٩٩٩٩، \"س\")\nماعدا:\n    اطبع \"قناة كما يجب\"\nحاول:\n    تواصل.استقبل(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"قناة كما يجب\"\nحاول:\n    تواصل.أغلق(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"قناة كما يجب\"\nحاول:\n    تواصل.أوقف(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"قناة كما يجب\"\nحاول:\n    تواصل.أنبوب(\"\")\nماعدا:\n    اطبع \"فارغ كما يجب\"\n",
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
        "قناة كما يجب\nقناة كما يجب\nقناة كما يجب\nقناة كما يجب\nفارغ كما يجب\n"
    );
}

/// مهمة 17 (تشفير): التجزئة في الوضعين — بصمتا
/// «abc» والفارغ تطابقان متجهات NIST حرفيا، وطول
/// بصمة نص عربي ٦٤ حرفا؛ والنوع والوسائط الخاطئة
/// أخطاء عربية تُمسكها ماعدا، متطابقة في الوضعين.
#[test]
fn crypto_hash_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد تشفير\nاطبع تشفير.جزّئ(\"abc\")\nاطبع تشفير.جزّئ(\"\")\nاطبع طول(تشفير.جزّئ(\"محمود\"))\nحاول:\n    تشفير.جزّئ(٥)\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    تشفير.جزّئ()\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    تشفير.جزّئ(\"أ\", \"ب\")\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
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
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n٦٤\nنص كما يجب\nوسائط كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 17 (تشفير): العشوائية الآمنة على يونكس
/// (لينكس/macOS/أندرويد) في الوضعين — الطول
/// والمدى صحيحان، والصفر قائمة فارغة، والسالب
/// وتجاوز الحد أخطاء عربية تُمسكها ماعدا.
/// (القيم نفسها عشوائية — لا تُقارن أبدا.)
#[test]
#[cfg(unix)]
fn crypto_random_unix_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد تشفير\nب = تشفير.عشوائي_آمن(١٦)\nاطبع طول(ب) == ١٦\nاطبع ب[٠] >= ٠\nاطبع ب[٠] <= ٢٥٥\nاطبع طول(تشفير.عشوائي_آمن(٠)) == ٠\nحاول:\n    تشفير.عشوائي_آمن(٠ - ١)\nماعدا:\n    اطبع \"سالب كما يجب\"\nحاول:\n    تشفير.عشوائي_آمن(١٠٤٨٥٧٧)\nماعدا:\n    اطبع \"تجاوز كما يجب\"\n",
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
        "صحيح\nصحيح\nصحيح\nصحيح\nسالب كما يجب\nتجاوز كما يجب\n"
    );
}

/// مهمة 17 (تشفير): العشوائية الآمنة على غير يونكس
/// (Windows) — رفض صادق بخطأ عربي، متطابق في
/// الوضعين؛ والسالب وتجاوز الحد يُرفضان بالتحقق
/// قبل فحص المنصة، فهما كما يجب على كل المنصات.
#[test]
#[cfg(not(unix))]
fn crypto_random_refused_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد تشفير\nحاول:\n    تشفير.عشوائي_آمن(١٦)\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    تشفير.عشوائي_آمن(٠ - ١)\nماعدا:\n    اطبع \"سالب كما يجب\"\nحاول:\n    تشفير.عشوائي_آمن(١٠٤٨٥٧٧)\nماعدا:\n    اطبع \"تجاوز كما يجب\"\n",
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
        "غير مدعوم كما يجب\nسالب كما يجب\nتجاوز كما يجب\n"
    );
}

/// مهمة 18 (أمن): قدرات العملية ووضع LSM على لينكس
/// (+Android) في الوضعين — بنيويا فقط (القيم تختلف
/// بين آلة وأخرى): القاموس له المفاتيح الأربعة، وكل
/// مجموعة قائمة، وقائمة الوضع قائمة؛ والوسائط الخاطئة
/// أخطاء عربية تُمسكها ماعدا.
#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn security_capabilities_linux_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد أمن\nق = أمن.قدرات_العملية()\nاطبع طول(ق) == ٤\nاطبع طول(ق[\"فعالة\"]) >= ٠\nاطبع طول(ق[\"مسموحة\"]) >= ٠\nاطبع طول(ق[\"مقيدة\"]) >= ٠\nاطبع طول(ق[\"موروثة\"]) >= ٠\nاطبع طول(أمن.الوضع()) >= ٠\nحاول:\n    أمن.قدرات_العملية(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    أمن.الوضع(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
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
        "صحيح\nصحيح\nصحيح\nصحيح\nصحيح\nصحيح\nوسائط كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 18 (أمن): التحقق من صلاحيات ملف على لينكس
/// (+Android) في الوضعين — ملف البرنامج نفسه (0644،
/// بملكية العملية): قراءة وكتابة صحيح، تنفيذ خطأ؛
/// والمفقود والخارج عن المجلد والوسائط الخاطئة
/// أخطاء عربية تُمسكها ماعدا.
#[test]
#[cfg(any(target_os = "linux", target_os = "android"))]
fn security_check_linux_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد أمن\nح = أمن.تحقق(\"برنامج.نيفا\")\nاطبع ح[\"قراءة\"]\nاطبع ح[\"كتابة\"]\nاطبع ح[\"تنفيذ\"]\nحاول:\n    أمن.تحقق(\"مفقود.نيفا\")\nماعدا:\n    اطبع \"غير موجود كما يجب\"\nحاول:\n    أمن.تحقق(\"../خارج.نيفا\")\nماعدا:\n    اطبع \"سياسة كما يجب\"\nحاول:\n    أمن.تحقق(٥)\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    أمن.تحقق()\nماعدا:\n    اطبع \"وسائط كما يجب\"\n",
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
        "صحيح\nصحيح\nخطأ\nغير موجود كما يجب\nسياسة كما يجب\nنص كما يجب\nوسائط كما يجب\n"
    );
}

/// مهمة 18 (أمن): الدوال الثلاث على غير لينكس
/// (macOS/Windows) — رفض صادق بخطأ عربي، متطابق في
/// الوضعين؛ والوسائط الخاطئة تُرفض بالتحقق قبل فحص
/// المنصة، فهي كما يجب على كل المنصات.
#[test]
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn security_refused_both_modes() {
    let files: &[(&str, &str)] = &[(
        "برنامج.نيفا",
        "استورد أمن\nحاول:\n    أمن.قدرات_العملية()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    أمن.الوضع()\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    أمن.تحقق(\"برنامج.نيفا\")\nماعدا:\n    اطبع \"غير مدعوم كما يجب\"\nحاول:\n    أمن.قدرات_العملية(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    أمن.تحقق(٥)\nماعدا:\n    اطبع \"نص كما يجب\"\n",
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
        "غير مدعوم كما يجب\nغير مدعوم كما يجب\nغير مدعوم كما يجب\nوسائط كما يجب\nنص كما يجب\n"
    );
}

/// مهمة 19 (غير_متزامن): قراءة وكتابة ملف غير
/// متزامنتين في الوضعين — الطلب يرجع فورا (معرف
/// العملية)، والعمل الوسطي يجري قبل الجمع، والنتائج
/// قواميس {المعرف، النوع، المحتوى}؛ والمحتوى المكتوب
/// يُقرأ متزامنا بعدها؛ والمخرج متطابق حرفيا.
#[test]
fn async_files_both_modes() {
    let src = "استورد غير_متزامن\nحلقة = غير_متزامن.أنشئ_حلقة()\nعملية = غير_متزامن.اقرأ(حلقة، \"رسالة.txt\"، ٠)\nاطبع عملية > ٠\nاطبع \"طلبت القراءة ولم أنتظر\"\nنتائج = غير_متزامن.انتظر_النتائج(حلقة)\nاطبع طول(نتائج)\nاطبع نتائج[٠][\"النوع\"]\nاطبع نتائج[٠][\"المحتوى\"]\nعملية = غير_متزامن.اكتب(حلقة، \"جديد.txt\"، \"نص جديد\")\nنتائج = غير_متزامن.انتظر_النتائج(حلقة)\nاطبع نتائج[٠][\"النوع\"]\nاطبع نتائج[٠][\"المحتوى\"]\nاطبع اقرأ_ملف(\"جديد.txt\")\nاطبع \"تم\"\n";
    let files: &[(&str, &str)] = &[("رسالة.txt", "مرحبا من غير_متزامن")];
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) =
        run_both_src_two_dirs(src, files);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "صحيح\nطلبت القراءة ولم أنتظر\n١\nقراءة\nمرحبا من غير_متزامن\nكتابة\n١٣\nنص جديد\nتم\n"
    );
}

/// مهمة 19 (غير_متزامن): إرسال واستقبال غير متزامنين
/// على مقبس TCP في الوضعين — خادم صدى جديد لكل وضع؛
/// والمقبس غير المعروف يرفضه الطلب فورا (البوابة
/// مفتوحة بالأعلام)؛ والبوابة المغلقة ترفض الطلب
/// برسالة البوابة نفسها في الوضعين.
#[test]
fn async_socket_echo_both_modes() {
    for interpreter in [true, false] {
        let (port, server) = tcp_echo_server_once();
        let prog = format!(
            "استورد شبكة\nاستورد غير_متزامن\nم = شبكة.اتصل(\"127.0.0.1\"، {})\nحلقة = غير_متزامن.أنشئ_حلقة()\nعملية = غير_متزامن.أرسل(حلقة، م، \"مرحبا\")\nاطبع عملية > ٠\nنتائج = غير_متزامن.انتظر_النتائج(حلقة)\nاطبع نتائج[٠][\"النوع\"]\nاطبع نتائج[٠][\"المحتوى\"]\nحاول:\n    غير_متزامن.استقبل(حلقة، ٩٩٩٩٩٩)\nماعدا:\n    اطبع \"مقبس كما يجب\"\nعملية = غير_متزامن.استقبل(حلقة، م)\nنتائج = غير_متزامن.انتظر_النتائج(حلقة)\nاطبع نتائج[٠][\"النوع\"]\nاطبع نتائج[٠][\"المحتوى\"]\nشبكة.أغلق(م)\nاطبع \"تم\"\n",
            port
        );
        let flags: &[&str] = if interpreter {
            &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) = run_src_flags(&prog, flags);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(out, "صحيح\nإرسال\n١٠\nمقبس كما يجب\nاستقبال\nمرحبا\nتم\n");
        server.join().unwrap();
    }
    // The closed gate refuses the async send at submit
    // with the same Arabic gate error in both modes.
    let prog = "استورد غير_متزامن\nحلقة = غير_متزامن.أنشئ_حلقة()\nغير_متزامن.أرسل(حلقة، ١، \"س\")\n";
    let (_, err_i, code_i) = run_src_flags(prog, &["--مفسر"]);
    let (_, err_v, code_v) = run_src_flags(prog, &[]);
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("الشبكة مغلقة افتراضيا"), "stderr: {}", err_i);
    assert_eq!(err_v, err_i, "VM gate error differs from interpreter");
}

/// مهمة 19 (غير_متزامن): الحلقة غير المعروفة والخروج
/// عن المجلد أخطاء عربية يرفعها الطلب فورا وتُمسكها
/// ماعدا؛ والملف المفقود ليس رفعا بل نتيجة تحمل
/// «خطأ» — كلها متطابقة في الوضعين.
#[test]
fn async_unknown_and_policy_both_modes() {
    let src = "استورد غير_متزامن\nحاول:\n    غير_متزامن.اقرأ(٩٩٩٩٩٩، \"أ.txt\"، ٠)\nماعدا:\n    اطبع \"حلقة كما يجب\"\nحاول:\n    غير_متزامن.انتظر_النتائج(٩٩٩٩٩٩)\nماعدا:\n    اطبع \"حلقة كما يجب\"\nحلقة = غير_متزامن.أنشئ_حلقة()\nحاول:\n    غير_متزامن.اقرأ(حلقة، \"../خارج.txt\"، ٠)\nماعدا:\n    اطبع \"سياسة كما يجب\"\nعملية = غير_متزامن.اقرأ(حلقة، \"مفقود.txt\"، ٠)\nنتائج = غير_متزامن.انتظر_النتائج(حلقة)\nاطبع نتائج[٠][\"النوع\"]\nاطبع نتائج[٠][\"خطأ\"]\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "حلقة كما يجب\nحلقة كما يجب\nسياسة كما يجب\nقراءة\nتعذر قراءة الملف مفقود.txt\nتم\n"
    );
}

/// مهمة 20 (جهاز.حوار): خارج أندرويد الرفض العربي
/// الواضح، متطابقا في الوضعين؛ وأخطاء الوسائط
/// والأنواع (قبل فحص المنصة) تُمسكها ماعدا.
/// (على أندرويد يعرض الحوار فعلا — فهذا الاختبار
/// مقيد بغير أندرويد، حيث لا انتظار بلا مجيب.)
#[test]
#[cfg(not(target_os = "android"))]
fn device_dialog_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.حوار \"تأكيد\" \"هل تحفظ؟\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.حوار \"غريب\" \"س؟\"\nماعدا:\n    اطبع \"نوع كما يجب\"\nحاول:\n    جهاز.حوار \"صوتي\" \"تكلم\"\nماعدا:\n    اطبع \"صوتي كما يجب\"\nحاول:\n    جهاز.حوار \"تأكيد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.حوار \"تأكيد\" \"\"\nماعدا:\n    اطبع \"عنوان كما يجب\"\nحاول:\n    جهاز.حوار \"اختيار واحد\" \"اختر\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nخ = []\nحاول:\n    جهاز.حوار \"اختيار واحد\" \"اختر\" خ\nماعدا:\n    اطبع \"خيارات كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nنوع كما يجب\nصوتي كما يجب\nوسائط كما يجب\nعنوان كما يجب\nوسائط كما يجب\nخيارات كما يجب\nتم\n"
    );
}

/// مهمة 20 (جهاز.حوار): تحقق قاموس العداد خالص
/// (قبل المنصة) — الحدود الصحيحة تصل إلى الرفض
/// العربي خارج أندرويد؛ والمفقود والمعكوس والافتراضي
/// خارج النطاق والمفتاح الغريب أخطاء عربية تُمسكها
/// ماعدا، متطابقة في الوضعين.
#[test]
#[cfg(not(target_os = "android"))]
fn device_counter_args_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.حوار \"عداد\" \"كم؟\" {\"أدنى\": ٠، \"أقصى\": ١٠}\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.حوار \"عداد\" \"كم؟\" {\"أقصى\": ١٠}\nماعدا:\n    اطبع \"أدنى كما يجب\"\nحاول:\n    جهاز.حوار \"عداد\" \"كم؟\" {\"أدنى\": ٩، \"أقصى\": ٢}\nماعدا:\n    اطبع \"حدود كما يجب\"\nحاول:\n    جهاز.حوار \"عداد\" \"كم؟\" {\"أدنى\": ٠، \"أقصى\": ٥، \"افتراضي\": ٩}\nماعدا:\n    اطبع \"افتراضي كما يجب\"\nحاول:\n    جهاز.حوار \"عداد\" \"كم؟\" {\"أدنى\": ٠، \"أقصى\": ٥، \"خطوة\": ١}\nماعدا:\n    اطبع \"مفتاح كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nأدنى كما يجب\nحدود كما يجب\nافتراضي كما يجب\nمفتاح كما يجب\nتم\n"
    );
}

/// مهمة 21 (جهاز.نبّه + أزل_تنبيه): خارج أندرويد الرفض
/// العربي الواضح، متطابقا في الوضعين؛ والإزالة ترفض
/// بالمنصة قبل الحياة (فمعرف غير حيّ على لينكس يرفض
/// بالمنصة لا بـ«غير معروف»)؛ وأخطاء الوسائط قبل
/// فحص المنصة تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_notify_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.نبّه \"انتهى\" \"البناء جاهز\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.أزل_تنبيه ١\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.نبّه \"عنوان\" \"\"\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.نبّه \"\" \"نص\"\nماعدا:\n    اطبع \"عنوان كما يجب\"\nحاول:\n    جهاز.نبّه \"عنوان\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.أزل_تنبيه \"١\"\nماعدا:\n    اطبع \"عدد كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nرفض كما يجب\nنص كما يجب\nعنوان كما يجب\nوسائط كما يجب\nعدد كما يجب\nتم\n"
    );
}

/// مهمة 21 (جهاز.أزل_تنبيه): أخطاء وسائط المعرف —
/// عدد وكسري وصفر وسالب وبلا معامل — أخطاء عربية
/// تُمسكها ماعدا، متطابقة في الوضعين، على كل المنصات
/// (التحقق قبل فحص المنصة).
#[test]
#[cfg(not(target_os = "android"))]
fn device_remove_args_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.أزل_تنبيه ١.٥\nماعدا:\n    اطبع \"صحيح كما يجب\"\nحاول:\n    جهاز.أزل_تنبيه ٠\nماعدا:\n    اطبع \"موجب كما يجب\"\nحاول:\n    جهاز.أزل_تنبيه -٢\nماعدا:\n    اطبع \"موجب كما يجب\"\nحاول:\n    جهاز.أزل_تنبيه()\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.نبّه ٥ \"نص\"\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.نبّه \"عنوان\" ٥\nماعدا:\n    اطبع \"نص كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "صحيح كما يجب\nموجب كما يجب\nموجب كما يجب\nوسائط كما يجب\nنص كما يجب\nنص كما يجب\nتم\n"
    );
}

/// مهمة 23 (جهاز.انطق + محركات_النطق): خارج
/// أندرويد الرفض العربي الواضح، متطابقا في
/// الوضعين؛ وأخطاء الوسائط قبل فحص المنصة
/// تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_speak_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.انطق \"مرحبا بالعالم\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.انطق \"\"\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.انطق ٥\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.انطق()\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.محركات_النطق()\nماعدا:\n    اطبع \"رفض كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nنص كما يجب\nنص كما يجب\nوسائط كما يجب\nرفض كما يجب\nتم\n"
    );
}

/// مهمة 23 (جهاز.محركات_النطق): أخطاء وسائط
/// السرد — معامل زائد ومعاملان — وأخطاء عدد
/// معاملات انطق (معاملان)، أخطاء عربية تُمسكها
/// ماعدا، متطابقة في الوضعين، على كل المنصات
/// (التحقق قبل فحص المنصة).
#[test]
#[cfg(not(target_os = "android"))]
fn device_engines_args_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.محركات_النطق \"زائد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.محركات_النطق \"أ\" \"ب\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.انطق \"أ\" \"ب\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "وسائط كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// مهمة 24 (جهاز.شغّل_صوت + أوقف_صوت): خارج أندرويد
/// الرفض العربي الواضح، متطابقا في الوضعين؛ وأخطاء
/// الوسائط قبل فحص المنصة تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_audio_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.شغّل_صوت \"موسيقى.mp3\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.أوقف_صوت()\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.شغّل_صوت \"\"\nماعدا:\n    اطبع \"مسار كما يجب\"\nحاول:\n    جهاز.شغّل_صوت ٥\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.شغّل_صوت()\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.أوقف_صوت \"زائد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nرفض كما يجب\nمسار كما يجب\nنص كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// المهمة 24 — انحدار اسم `جهاز.نبّه` (إصلاح
/// المفتاح المشدد): الاستدعاء بلا `حاول/ماعدا` في
/// الوضعين يفشل حتما خارج أندرويد، لكن بخطأ
/// المنصة («غير مدعوم على هذه المنصة») لا بخطأ
/// الاسم («غير مصدر»). هذا الاختبار غير فراغي
/// عمدا — فنسخة `حاول/ماعدا` تطبع النتيجة نفسها
/// عند أي خطأ، فمرّت فراغيا على الخلل منذ
/// المهمة 21.
#[test]
#[cfg(not(target_os = "android"))]
fn device_notify_name_resolves_both_modes() {
    let src = "استورد جهاز\nجهاز.نبّه \"انتهى\" \"البناء جاهز\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_ne!(code_i, 0, "interpreter unexpectedly succeeded");
    assert_ne!(code_v, 0, "VM unexpectedly succeeded");
    assert!(err_i.contains("جهاز.نبه غير مدعوم على هذه المنصة"), "stderr: {}", err_i);
    assert!(err_v.contains("جهاز.نبه غير مدعوم على هذه المنصة"), "stderr: {}", err_v);
    assert!(!err_i.contains("غير مصدر"), "name did not resolve: {}", err_i);
    assert!(!err_v.contains("غير مصدر"), "name did not resolve: {}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
}

/// مهمة 25 (جهاز.حساس): خارج أندرويد الرفض العربي
/// الواضح، متطابقا في الوضعين؛ واسم غير معروف
/// ووسائط خاطئة قبل فحص المنصة تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_sensor_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.حساس \"تسارع\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.حساس \"ضوء\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.حساس \"\"\nماعدا:\n    اطبع \"اسم كما يجب\"\nحاول:\n    جهاز.حساس \"نبض\"\nماعدا:\n    اطبع \"غير معروف كما يجب\"\nحاول:\n    جهاز.حساس ٥\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    جهاز.حساس()\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.حساس \"تسارع\" \"زائد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nرفض كما يجب\nاسم كما يجب\nغير معروف كما يجب\nنص كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// المهمة 25 — انحدار اسم `جهاز.حساس` (درس المهمة
/// 24): الاستدعاء بلا `حاول/ماعدا` في الوضعين يفشل
/// حتما خارج أندرويد، لكن بخطأ المنصة («غير مدعوم
/// على هذه المنصة») لا بخطأ الاسم («غير مصدر»).
/// هذا الاختبار غير فراغي عمدا — فنسخة
/// `حاول/ماعدا` تطبع النتيجة نفسها عند أي خطأ.
#[test]
#[cfg(not(target_os = "android"))]
fn device_sensor_name_resolves_both_modes() {
    let src = "استورد جهاز\nجهاز.حساس \"تسارع\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_ne!(code_i, 0, "interpreter unexpectedly succeeded");
    assert_ne!(code_v, 0, "VM unexpectedly succeeded");
    assert!(err_i.contains("جهاز.حساس غير مدعوم على هذه المنصة"), "stderr: {}", err_i);
    assert!(err_v.contains("جهاز.حساس غير مدعوم على هذه المنصة"), "stderr: {}", err_v);
    assert!(!err_i.contains("غير مصدر"), "name did not resolve: {}", err_i);
    assert!(!err_v.contains("غير مصدر"), "name did not resolve: {}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
}

/// المهمة 26 (جهاز.اسمع): خارج أندرويد الرفض
/// العربي الواضح، متطابقا في الوضعين؛ وأخطاء
/// الوسائط قبل فحص المنصة تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_listen_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.اسمع()\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.اسمع \"زائد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.اسمع(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// المهمة 26 — انحدار اسم `جهاز.اسمع` (درس المهمة
/// 24): الاستدعاء بلا `حاول/ماعدا` في الوضعين يفشل
/// حتما خارج أندرويد، لكن بخطأ المنصة («غير مدعوم
/// على هذه المنصة») لا بخطأ الاسم («غير مصدر»).
/// هذا الاختبار غير فراغي عمدا — فنسخة
/// `حاول/ماعدا` تطبع النتيجة نفسها عند أي خطأ.
#[test]
#[cfg(not(target_os = "android"))]
fn device_listen_name_resolves_both_modes() {
    let src = "استورد جهاز\nجهاز.اسمع()\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_ne!(code_i, 0, "interpreter unexpectedly succeeded");
    assert_ne!(code_v, 0, "VM unexpectedly succeeded");
    assert!(err_i.contains("جهاز.اسمع غير مدعوم على هذه المنصة"), "stderr: {}", err_i);
    assert!(err_v.contains("جهاز.اسمع غير مدعوم على هذه المنصة"), "stderr: {}", err_v);
    assert!(!err_i.contains("غير مصدر"), "name did not resolve: {}", err_i);
    assert!(!err_v.contains("غير مصدر"), "name did not resolve: {}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
}

/// المهمة 26 (جهاز.بطارية): خارج أندرويد الرفض
/// العربي الواضح، متطابقا في الوضعين؛ وأخطاء
/// الوسائط قبل فحص المنصة تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn device_battery_refused_both_modes() {
    let src = "استورد جهاز\nحاول:\n    جهاز.بطارية()\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    جهاز.بطارية \"زائد\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    جهاز.بطارية(١)\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// المهمة 26 — انحدار اسم `جهاز.بطارية` (درس المهمة
/// 24): الاستدعاء بلا `حاول/ماعدا` في الوضعين يفشل
/// حتما خارج أندرويد، لكن بخطأ المنصة («غير مدعوم
/// على هذه المنصة») لا بخطأ الاسم («غير مصدر»).
/// هذا الاختبار غير فراغي عمدا — فنسخة
/// `حاول/ماعدا` تطبع النتيجة نفسها عند أي خطأ.
#[test]
#[cfg(not(target_os = "android"))]
fn device_battery_name_resolves_both_modes() {
    let src = "استورد جهاز\nجهاز.بطارية()\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_ne!(code_i, 0, "interpreter unexpectedly succeeded");
    assert_ne!(code_v, 0, "VM unexpectedly succeeded");
    assert!(err_i.contains("جهاز.بطارية غير مدعوم على هذه المنصة"), "stderr: {}", err_i);
    assert!(err_v.contains("جهاز.بطارية غير مدعوم على هذه المنصة"), "stderr: {}", err_v);
    assert!(!err_i.contains("غير مصدر"), "name did not resolve: {}", err_i);
    assert!(!err_v.contains("غير مصدر"), "name did not resolve: {}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
}

/// المهمة 28 (ملفات SAF): خارج أندرويد الرفض العربي
/// الواضح باسم الدالة، متطابقا في الوضعين؛ وأخطاء
/// الوسائط (عدد ومعرف ومسار ونص) قبل فحص المنصة
/// تُمسكها ماعدا.
#[test]
#[cfg(not(target_os = "android"))]
fn files_saf_refused_both_modes() {
    let src = "استورد ملفات\nحاول:\n    ملفات.امنح_مجلد()\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.الممنوح()\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.سرد_ممنوح ١ \"\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.اقرأ_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.اكتب_ممنوح ١ \"أ\" \"نص\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.أنشئ_ملف_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.أنشئ_مجلد_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.تفاصيل_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.احذف_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"رفض كما يجب\"\nحاول:\n    ملفات.سرد_ممنوح ٠ \"\"\nماعدا:\n    اطبع \"معرف كما يجب\"\nحاول:\n    ملفات.سرد_ممنوح ١.٥ \"\"\nماعدا:\n    اطبع \"معرف كما يجب\"\nحاول:\n    ملفات.سرد_ممنوح \"١\" \"\"\nماعدا:\n    اطبع \"معرف كما يجب\"\nحاول:\n    ملفات.سرد_ممنوح ١ ٢\nماعدا:\n    اطبع \"مسار كما يجب\"\nحاول:\n    ملفات.اكتب_ممنوح ١ \"أ\" ٣\nماعدا:\n    اطبع \"نص كما يجب\"\nحاول:\n    ملفات.اكتب_ممنوح ١ \"أ\"\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    ملفات.امنح_مجلد ١\nماعدا:\n    اطبع \"وسائط كما يجب\"\nحاول:\n    ملفات.الممنوح ١\nماعدا:\n    اطبع \"وسائط كما يجب\"\nاطبع \"تم\"\n";
    let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "رفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nرفض كما يجب\nمعرف كما يجب\nمعرف كما يجب\nمعرف كما يجب\nمسار كما يجب\nنص كما يجب\nوسائط كما يجب\nوسائط كما يجب\nوسائط كما يجب\nتم\n"
    );
}

/// المهمة 28 — انحدار أسماء SAF التسعة (درس المهمة
/// 24): الاستدعاء بلا `حاول/ماعدا` في الوضعين يفشل
/// حتما خارج أندرويد، لكن بخطأ المنصة («غير مدعوم
/// على هذه المنصة») لا بخطأ الاسم («غير مصدر»).
/// هذا الاختبار غير فراغي عمدا — فنسخة
/// `حاول/ماعدا` تطبع النتيجة نفسها عند أي خطأ.
#[test]
#[cfg(not(target_os = "android"))]
fn files_saf_name_resolves_both_modes() {
    let cases: [(&str, &str); 9] = [
        ("امنح_مجلد", "استورد ملفات\nملفات.امنح_مجلد()\n"),
        ("سرد_ممنوح", "استورد ملفات\nملفات.سرد_ممنوح ١ \"\"\n"),
        ("اقرأ_ممنوح", "استورد ملفات\nملفات.اقرأ_ممنوح ١ \"أ\"\n"),
        (
            "اكتب_ممنوح",
            "استورد ملفات\nملفات.اكتب_ممنوح ١ \"أ\" \"نص\"\n",
        ),
        (
            "أنشئ_ملف_ممنوح",
            "استورد ملفات\nملفات.أنشئ_ملف_ممنوح ١ \"أ\"\n",
        ),
        (
            "أنشئ_مجلد_ممنوح",
            "استورد ملفات\nملفات.أنشئ_مجلد_ممنوح ١ \"أ\"\n",
        ),
        (
            "تفاصيل_ممنوح",
            "استورد ملفات\nملفات.تفاصيل_ممنوح ١ \"أ\"\n",
        ),
        (
            "احذف_ممنوح",
            "استورد ملفات\nملفات.احذف_ممنوح ١ \"أ\"\n",
        ),
        ("الممنوح", "استورد ملفات\nملفات.الممنوح()\n"),
    ];
    for (name, src) in cases {
        let ((out_i, err_i, code_i), (out_v, err_v, code_v)) = run_both_src(src);
        assert_ne!(code_i, 0, "{}: interpreter unexpectedly succeeded", name);
        assert_ne!(code_v, 0, "{}: VM unexpectedly succeeded", name);
        assert!(
            err_i.contains(&format!("ملفات.{} غير مدعوم على هذه المنصة", name)),
            "{}: stderr: {}",
            name,
            err_i
        );
        assert!(
            err_v.contains(&format!("ملفات.{} غير مدعوم على هذه المنصة", name)),
            "{}: stderr: {}",
            name,
            err_v
        );
        assert!(!err_i.contains("غير مصدر"), "{}: name did not resolve: {}", name, err_i);
        assert!(!err_v.contains("غير مصدر"), "{}: name did not resolve: {}", name, err_v);
        assert_eq!(out_v, out_i);
        assert_eq!(err_v, err_i);
    }
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
    // بعد Q5 (اجتماع 32) الخطأ يسمّي الملف قبل السطر
    // والعمود: «(الملف …، السطر س، العمود ع)» — فيُجرَّد
    // من «(الملف» لا من «(السطر»؛ ويبقى القديم احتياطا
    // لصيغة بلا ملف.
    fn strip_pos(s: &str) -> String {
        match s.rfind(" (الملف ") {
            Some(i) => s[..i].to_string(),
            None => match s.rfind(" (السطر ") {
                Some(j) => s[..j].to_string(),
                None => s.to_string(),
            },
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

/// خادم جيت هاب محاكى لاختبارات التكامل: يربط 127.0.0.1
/// على منفذ مؤقت ويرد بـ JSON canned حسب المسار —
/// نمط net_test_server القائم. الثنائي يوجَّه إليه
/// بمتغير البيئة NIVA_GITHUB_API_BASE.
fn github_mock_server() -> u16 {
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
            let target = req_text
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let path = target
                .split('?')
                .next()
                .unwrap_or("/")
                .to_string();
            let base = format!("http://127.0.0.1:{}", port);
            let file_body = "صدر قيمة\nقيمة = ٤٢\n";
            let (status, body): (&str, String) = match path.as_str() {
                // قائمة الجذر: ملف .نيفا واحد + نص + مجلد.
                "/repos/owner/repo/contents" => (
                    "200 OK",
                    format!(
                        "[{{\"name\":\"tool.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/tool.نيفا\"}},{{\"name\":\"note.txt\",\"type\":\"file\",\"download_url\":\"{}/files/note.txt\"}},{{\"name\":\"sub\",\"type\":\"dir\",\"download_url\":null}}]",
                        base, base
                    ),
                ),
                // مسار فيه ملفان — يُثبَّتان معا (تثبيت-الكل).
                // الأحجام من السرد نفسه (32 بايتا لكل
                // منهما — حجم محتوى المحاكي حرفيا).
                "/repos/owner/repo/contents/sub" => (
                    "200 OK",
                    format!(
                        "[{{\"name\":\"a.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/a.نيفا\",\"size\":32}},{{\"name\":\"b.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/b.نيفا\",\"size\":32}}]",
                        base, base
                    ),
                ),
                // إصدار بأصل .نيفا واحد (رابطه على الخادم نفسه).
                "/repos/owner/repo/releases/tags/v1" => (
                    "200 OK",
                    format!(
                        "{{\"assets\":[{{\"name\":\"tool.نيفا\",\"browser_download_url\":\"{}/files/tool.نيفا\"}}]}}",
                        base
                    ),
                ),
                _ if path.starts_with("/files/") => {
                    ("200 OK", file_body.to_string())
                }
                _ => (
                    "404 Not Found",
                    "{\"message\":\"Not Found\"}".to_string(),
                ),
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                )
                .as_bytes(),
            );
        }
    });
    port
}

/// تثبيت من رابط مستودع جيت هاب في الوضعين: الخادم
/// المحاكي يرد بملف `.نيفا` واحد، فيُثبَّت ويُستورد
/// وتُطبع قيمته — المخرج والخروج متطابقان حرفيا.
#[test]
fn install_github_repo_both_modes() {
    let port = github_mock_server();
    let api_base = format!("http://127.0.0.1:{}", port);
    let prog = "اطبع تثبيت(\"https://github.com/owner/repo\")\nاستورد tool\nاطبع tool.قيمة\n";
    for interpreter in [true, false] {
        let flags: &[&str] = if interpreter {
            &[
                "--مفسر",
                "--اسمح-بالشبكة",
                "--مضيف-محلي",
                "127.0.0.1",
            ]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) =
            run_src_flags_env(prog, flags, &[("NIVA_GITHUB_API_BASE", &api_base)]);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(out, "جارٍ قراءة قائمة الملفات...\nثُبّتت الوحدة tool\nبصمة الملف: 4cee6f791f5495b679f4a5317bd3afccab74d827761164d6b4a4e4f76770d249\n٤٢\n");
    }
}

/// تثبيت-الكل من رابط مستودع فيه ملفان في الوضعين:
/// الملفان يُثبَّتان معا ثم يُستورد أحدهما وتُطبع
/// قيمته — اجتماع 43 §58 استبدل الرفض القديم لأكثر
/// من ملف بهذا المسار، فهذا الاختبار يثبت السلوك
/// الجديد لا القديم. والسطور تُبعث لحظة إنتاجها
/// (قراءة، خطة بالحجم، جارٍ ونجاح لكل ملف، ملخص)
/// وبلا سطر بصمة؛ والقيمة سلسلة فارغة.
#[test]
fn install_github_repo_multiple_install_all_both_modes() {
    let port = github_mock_server();
    let api_base = format!("http://127.0.0.1:{}", port);
    let prog = "تثبيت(\"https://github.com/owner/repo/tree/main/sub\")\nاستورد a\nاطبع a.قيمة\n";
    for interpreter in [true, false] {
        let flags: &[&str] = if interpreter {
            &[
                "--مفسر",
                "--اسمح-بالشبكة",
                "--مضيف-محلي",
                "127.0.0.1",
            ]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) =
            run_src_flags_env(prog, flags, &[("NIVA_GITHUB_API_BASE", &api_base)]);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(
            out,
            "جارٍ قراءة قائمة الملفات...\nسيُثبَّت ٢ ملفات (الحجم الإجمالي ٦٤ بايت)\nجارٍ تنزيل a (٣٢ بايت)... (١ من ٢)\nثُبّتت الوحدة a\nجارٍ تنزيل b (٣٢ بايت)... (٢ من ٢)\nثُبّتت الوحدة b\nثُبّت ٢ من ٢ (تُخطَّى ٠)\n٤٢\n"
        );
    }
}

/// ترتيب سطور تثبيت-الكل في وضع البرنامج (المخزَّن):
/// `اطبع` قبل التثبيت وبعده — سطور التقدم تتداخل
/// معها بالترتيب نفسه في الوضعين، بلا تقديم ولا تأخير.
#[test]
fn install_all_order_with_prints_both_modes() {
    let port = github_mock_server();
    let api_base = format!("http://127.0.0.1:{}", port);
    let prog = "اطبع \"قبل\"\nتثبيت(\"https://github.com/owner/repo/tree/main/sub\")\nاطبع \"بعد\"\n";
    for interpreter in [true, false] {
        let flags: &[&str] = if interpreter {
            &[
                "--مفسر",
                "--اسمح-بالشبكة",
                "--مضيف-محلي",
                "127.0.0.1",
            ]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) =
            run_src_flags_env(prog, flags, &[("NIVA_GITHUB_API_BASE", &api_base)]);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(
            out,
            "قبل\nجارٍ قراءة قائمة الملفات...\nسيُثبَّت ٢ ملفات (الحجم الإجمالي ٦٤ بايت)\nجارٍ تنزيل a (٣٢ بايت)... (١ من ٢)\nثُبّتت الوحدة a\nجارٍ تنزيل b (٣٢ بايت)... (٢ من ٢)\nثُبّتت الوحدة b\nثُبّت ٢ من ٢ (تُخطَّى ٠)\nبعد\n"
        );
    }
}

/// تثبيت من رابط إصدار جيت هاب في الوضعين: أصل `.نيفا`
/// واحد رابطه على الخادم نفسه، فيُثبَّت ويُستورد.
#[test]
fn install_github_release_both_modes() {
    let port = github_mock_server();
    let api_base = format!("http://127.0.0.1:{}", port);
    let prog = "اطبع تثبيت(\"https://github.com/owner/repo/releases/tag/v1\")\nاستورد tool\nاطبع tool.قيمة\n";
    for interpreter in [true, false] {
        let flags: &[&str] = if interpreter {
            &[
                "--مفسر",
                "--اسمح-بالشبكة",
                "--مضيف-محلي",
                "127.0.0.1",
            ]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) =
            run_src_flags_env(prog, flags, &[("NIVA_GITHUB_API_BASE", &api_base)]);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(out, "جارٍ قراءة قائمة الملفات...\nثُبّتت الوحدة tool\nبصمة الملف: 4cee6f791f5495b679f4a5317bd3afccab74d827761164d6b4a4e4f76770d249\n٤٢\n");
    }
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
        // ملفات متداخلة (مثل `حزم/أداة/أداة.نيفا`) تحتاج
        // مجلداتها الأب أولا.
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, src).unwrap();
    }
    let mut cmd = Command::new(bin());
    cmd.arg(dir.join(main));
    for flag in flags {
        cmd.arg(flag);
    }
    let result = cmd.current_dir(&dir).output().expect("run نيفا");
    let out = norm_dir(
        &String::from_utf8_lossy(&result.stdout).to_string(),
        &dir,
    );
    let err = norm_dir(
        &String::from_utf8_lossy(&result.stderr).to_string(),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (out, err, result.status.code().unwrap_or(-1))
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
    assert!(err_i.contains("أ ← ب ← أ"), "stderr: {}", err_i);
    assert!(err_v.contains("استيراد دائري"), "stderr: {}", err_v);
    assert!(err_v.contains("أ ← ب ← أ"), "stderr: {}", err_v);
    // (المساواة الكاملة مستحيلة هنا أيضا: كل تشغيل
    // بمجلد مؤقت مختلف، ورسالة الدورة تسمي الملف —
    // فالمقارنة على المحتوى كما في حالة «غير موجودة».)
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

// بدائيتا استيراد «حزم» (التصميم §8): `حزم/<اسم>/`
// ثم مجلد البرنامج، ثم المخزن العام (قفل ثم أحدث).
// كل اختبار نجاح يستخدم الاسم المستورد مباشرة بلا
// `حاول` — فهو إثبات أن الاسم يُحلّ (لا اختبار فراغي).

const PKG_MAIN: &str = "استورد أداة\nاطبع أداة.قيمة\nاطبع أداة.اجمع(١، ٢)\n";

fn pkg_module_value(value: &str) -> String {
    format!("صدّر قيمة، اجمع\nقيمة = {}\nدالة اجمع(أ، ب):\n    ارجع أ + ب\n", value)
}

#[test]
fn package_import_from_project_hazm_dir() {
    let mod_42 = pkg_module_value("٤٢");
    let files: &[(&str, &str)] = &[
        ("حزم/أداة/أداة.نيفا", &mod_42),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٤٢\n٣\n");
}

#[test]
fn package_import_project_dir_beats_program_dir() {
    let mod_42 = pkg_module_value("٤٢");
    let mod_99 = pkg_module_value("٩٩");
    let files: &[(&str, &str)] = &[
        ("حزم/أداة/أداة.نيفا", &mod_42),
        ("أداة.نيفا", &mod_99),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٤٢\n٣\n");
}

#[test]
fn package_import_program_dir_beats_store() {
    let mod_99 = pkg_module_value("٩٩");
    let mod_77 = pkg_module_value("٧٧");
    let files: &[(&str, &str)] = &[
        ("أداة.نيفا", &mod_99),
        (".نيفا/حزم/أداة-9.0.0/أداة.نيفا", &mod_77),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٩٩\n٣\n");
}

#[test]
fn package_import_store_newest_without_lock() {
    let mod_1 = pkg_module_value("١");
    let mod_2 = pkg_module_value("٢");
    let files: &[(&str, &str)] = &[
        (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", &mod_1),
        (".نيفا/حزم/أداة-2.0.0/أداة.نيفا", &mod_2),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٢\n٣\n");
}

#[test]
fn package_import_store_lock_pin() {
    let mod_1 = pkg_module_value("١");
    let mod_2 = pkg_module_value("٢");
    let files: &[(&str, &str)] = &[
        (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", &mod_1),
        (".نيفا/حزم/أداة-2.0.0/أداة.نيفا", &mod_2),
        (
            "قفل.حزمة",
            "# قفل حزم — يولَّد آليا؛ لا تحرره بيدك\nأداة 1.0.0 github.com/مالك/أداة 9f2c\n",
        ),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "١\n٣\n");
}

#[test]
fn package_import_store_lock_pin_missing_fails() {
    // قفل يثبّت إصدارا غير مثبّت — لا رجوع صامت إلى الأحدث.
    let mod_1 = pkg_module_value("١");
    let mod_2 = pkg_module_value("٢");
    let files: &[(&str, &str)] = &[
        (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", &mod_1),
        (".نيفا/حزم/أداة-2.0.0/أداة.نيفا", &mod_2),
        (
            "قفل.حزمة",
            "# قفل حزم\nأداة 3.0.0 github.com/مالك/أداة 9f2c\n",
        ),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_ne!(code_i, 0);
    assert!(err_i.contains("وحدة غير موجودة"), "stderr: {}", err_i);
    assert!(err_i.contains("أداة-3.0.0"), "stderr: {}", err_i);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_v, 0);
    assert!(err_v.contains("وحدة غير موجودة"), "stderr: {}", err_v);
    assert!(err_v.contains("أداة-3.0.0"), "stderr: {}", err_v);
}

#[test]
fn package_import_native_not_shadowed_by_package() {
    let files: &[(&str, &str)] = &[
        ("حزم/رياضيات/رياضيات.نيفا", "صدّر س\nس = 999\n"),
        (
            "برنامج.نيفا",
            "استورد رياضيات\nاطبع رياضيات.جذر(16)\n",
        ),
    ];
    let (out_i, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    let (out_v, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "٤\n");
}

#[test]
fn package_import_missing_lists_searched_paths() {
    let files: &[(&str, &str)] = &[("برنامج.نيفا", "استورد لاوجود\n")];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_ne!(code_i, 0);
    assert!(err_i.contains("وحدة غير موجودة"), "stderr: {}", err_i);
    assert!(err_i.contains("لاوجود"), "stderr: {}", err_i);
    assert!(
        err_i.contains("حزم/لاوجود/لاوجود.نيفا"),
        "stderr: {}",
        err_i
    );
    assert!(err_i.contains(".نيفا/حزم"), "stderr: {}", err_i);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_v, 0);
    assert!(err_v.contains("وحدة غير موجودة"), "stderr: {}", err_v);
    assert!(err_v.contains("حزم/لاوجود/لاوجود.نيفا"), "stderr: {}", err_v);
}

#[test]
fn package_import_lock_malformed_version() {
    let mod_1 = pkg_module_value("١");
    let files: &[(&str, &str)] = &[
        (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", &mod_1),
        (
            "قفل.حزمة",
            "# قفل حزم\nأداة 1.2 github.com/مالك/أداة 9f2c\n",
        ),
        ("برنامج.نيفا", PKG_MAIN),
    ];
    let (_, err_i, code_i) = run_with_modules_flags(files, "برنامج.نيفا", &["--مفسر"]);
    assert_ne!(code_i, 0);
    assert!(
        err_i.contains("إصدار غير صالح في القفل"),
        "stderr: {}",
        err_i
    );
    assert!(err_i.contains("أداة"), "stderr: {}", err_i);
    let (_, err_v, code_v) = run_with_modules(files, "برنامج.نيفا");
    assert_ne!(code_v, 0);
    assert!(
        err_v.contains("إصدار غير صالح في القفل"),
        "stderr: {}",
        err_v
    );
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
            norm_dir(&String::from_utf8_lossy(&out.stdout).to_string(), dir),
            norm_dir(&String::from_utf8_lossy(&out.stderr).to_string(), dir),
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
            norm_dir(&String::from_utf8_lossy(&out.stdout).to_string(), dir),
            norm_dir(&String::from_utf8_lossy(&out.stderr).to_string(), dir),
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
    // بعد البث الحي (اجتماع 32 Q8) المخرج بترتيب البرنامج
    // في الوضعين: ٧ و٤٢ قبل سؤال ادخل، لا السؤال أولا.
    assert_eq!(out_i, "٧\n٤٢\nما اسمك؟ محمود\n١٥\n");
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
            norm_dir(&String::from_utf8_lossy(&out.stdout).to_string(), dir),
            norm_dir(&String::from_utf8_lossy(&out.stderr).to_string(), dir),
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
    run_src_flags_env(src, flags, &[])
}

/// مثل run_src_flags لكن بمتغيرات بيئة إضافية للعملية
/// الفرعية (لاختبارات تثبيت جيت هاب: توجيه واجهة جيت
/// هاب إلى الخادم المحاكي عبر NIVA_GITHUB_API_BASE).
fn run_src_flags_env(
    src: &str,
    flags: &[&str],
    envs: &[(&str, &str)],
) -> (String, String, i32) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "rio4140-engine-net-env-{}-{}",
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
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let result = cmd.current_dir(&dir).output().expect("run نيفا");
    let out = norm_dir(
        &String::from_utf8_lossy(&result.stdout).to_string(),
        &dir,
    );
    let err = norm_dir(
        &String::from_utf8_lossy(&result.stderr).to_string(),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (out, err, result.status.code().unwrap_or(-1))
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
            // المهمة 22: /drip يتدفق ببطء (بايت كل
            // 300ms) لاختبار إلغاء جلب طويل.
            if path == "/drip" {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
                );
                for i in 0..10u8 {
                    let _ = stream.write_all(&[b'a' + i]);
                    let _ = stream.flush();
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                continue;
            }
            let (status, body): (&str, Vec<u8>) = match path.as_str() {
                "/hello" => ("200 OK", "مرحبا من الخادم".as_bytes().to_vec()),
                // إصلاح ترميز المسار في اجلب: الخادم يرى
                // المسار مرمّزا — فالرابط العربي الخام يصل
                // هنا مرمّزا، وبلا الترميز كان يسقط في 404.
                "/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7" => {
                    ("200 OK", "أداة من الخادم".as_bytes().to_vec())
                }
                _ => ("404 Not Found", Vec::new()),
            };
            let extra = format!("Content-Length: {}\r\n", body.len());
            let mut resp = format!("HTTP/1.1 {}\r\n{}Connection: close\r\n\r\n", status, extra)
                .into_bytes();
            resp.extend_from_slice(&body);
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

// إصلاح ترميز المسار في اجلب: برنامج نيفا يجلب رابطا
// عربيا خاما في الوضعين — والخادم يرى المسار مرمّزا.
#[test]
fn network_fetch_arabic_path_both_modes() {
    let port = net_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.اجلب(\"http://127.0.0.1:{}/أداة.نيفا\")\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(out_v, "أداة من الخادم\n", "stderr={}", err_v);
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

// المهمة 22: شبكة.نزّل في الوضعين — ينزل ملف
// /hello إلى مجلد البرنامج، ويطبع عدد بايتاته
// ثم يقرأه بنفسه (فالمجلد المؤقت يُحذف بعد
// التشغيل، فيتحقق البرنامج من الملف لا الاختبار).
#[test]
fn network_download_both_modes() {
    let port = net_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.نزّل(\"http://127.0.0.1:{}/hello\", \"نزل.بن\")\nاطبع اقرأ_ملف(\"نزل.بن\")\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    // «مرحبا من الخادم» = 28 بايت UTF-8، فيطبع ٢٨.
    assert_eq!(out_v, "٢٨\nمرحبا من الخادم\n", "stdout={}", out_v);
}

#[test]
fn network_download_refused_without_flag() {
    let port = net_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.نزّل(\"http://127.0.0.1:{}/hello\", \"نزل.بن\")\n",
        port
    );
    let (_out, err, code) = run_src_flags(&src, &[]);
    assert_ne!(code, 0);
    assert!(err.contains("الشبكة مغلقة افتراضيا"), "stderr: {}", err);
}

// المهمة 22: وسائط نزّل الخاطئة في الوضعين —
// عدد معاملات صفر (بصيغة الأقواس)، ونوع خاطئ،
// ورابط فارغ، ومسار فارغ.
#[test]
fn network_download_args_both_modes() {
    let flags_i = ["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let flags_v = ["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let cases = [
        ("استورد شبكة\nاطبع شبكة.نزّل()\n", "معاملات"),
        (
            "استورد شبكة\nاطبع شبكة.نزّل ١ \"ملف.بن\"\n",
            "نص",
        ),
        (
            "استورد شبكة\nاطبع شبكة.نزّل \"\" \"ملف.بن\"\n",
            "رابطا غير فارغ",
        ),
        (
            "استورد شبكة\nاطبع شبكة.نزّل \"http://127.0.0.1:9/x\" \"\"\n",
            "مسارا غير فارغ",
        ),
    ];
    for (src, needle) in cases {
        let (_o, err_i, c_i) = run_src_flags(src, &flags_i);
        let (_o2, err_v, c_v) = run_src_flags(src, &flags_v);
        assert_ne!(c_i, 0, "interpreter accepted: {}", src);
        assert_ne!(c_v, 0, "VM accepted: {}", src);
        assert!(err_i.contains(needle), "interpreter stderr: {}", err_i);
        assert!(err_v.contains(needle), "VM stderr: {}", err_v);
    }
}

// المهمة 22: خطأ HTTP لا يترك ملفا جزئيا —
// البرنامج نفسه يتحقق: قراءة الملف الغائب
// خطأ، فيطبع «لا ملف».
#[test]
fn network_download_http_error_no_partial_both_modes() {
    let port = net_test_server();
    let src = format!(
        "استورد شبكة\nحاول:\n    شبكة.نزّل(\"http://127.0.0.1:{}/missing\", \"فشل.بن\")\n    اطبع \"نجح؟\"\nماعدا:\n    اطبع \"فشل كما يجب\"\nحاول:\n    اطبع اقرأ_ملف(\"فشل.بن\")\nماعدا:\n    اطبع \"لا ملف\"\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "فشل كما يجب\nلا ملف\n", "stdout={}", out_v);
}

// اجتماع 29: خادم POST محاكٍ لاختبارات التكامل —
// يقرأ الترويسات ثم الجسم (بطول المحتوى) ويردّ
// الطريقة ونوع المحتوى والجسم كما وصلته.
fn net_post_test_server() -> u16 {
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
            let head_end = loop {
                match stream.read(&mut buf) {
                    Ok(0) => break None,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if let Some(p) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(p);
                        }
                        if req.len() > 16384 {
                            break None;
                        }
                    }
                    Err(_) => break None,
                }
            };
            let Some(head_end) = head_end else {
                continue;
            };
            let head = String::from_utf8_lossy(&req[..head_end]).into_owned();
            let mut body: Vec<u8> = req[head_end + 4..].to_vec();
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or("");
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("/").to_string();
            let mut content_length = 0usize;
            let mut content_type = String::new();
            // اجتماع 30: التقاط ترويسات المتصل
            // لفحصها — فيردّها /echo-headers كما وصلت.
            let mut authorization = String::new();
            let mut x_test = String::new();
            for l in lines {
                if let Some((name, value)) = l.split_once(':') {
                    match name.trim().to_ascii_lowercase().as_str() {
                        "content-length" => {
                            content_length = value.trim().parse().unwrap_or(0)
                        }
                        "content-type" => content_type = value.trim().to_string(),
                        "authorization" => {
                            authorization = value.trim().to_string()
                        }
                        "x-test" => x_test = value.trim().to_string(),
                        _ => {}
                    }
                }
            }
            while body.len() < content_length {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => body.extend_from_slice(&buf[..n]),
                    Err(_) => break,
                }
            }
            body.truncate(content_length);
            let body_text = String::from_utf8_lossy(&body).into_owned();
            let (status, resp_body): (&str, Vec<u8>) =
                match (method.as_str(), path.as_str()) {
                    ("POST", "/echo") => (
                        "200 OK",
                        format!("طريقة:POST نوع:{} جسم:{}", content_type, body_text)
                            .into_bytes(),
                    ),
                    // اجتماع 30: يردّ ترويسات المتصل
                    // كما وصلت (تفويض + مخصص + نوع).
                    ("POST", "/echo-headers") => (
                        "200 OK",
                        format!(
                            "طريقة:POST تفويض:{} مخصص:{} نوع:{}",
                            authorization, x_test, content_type
                        )
                        .into_bytes(),
                    ),
                    _ => ("404 Not Found", Vec::new()),
                };
            let extra = format!("Content-Length: {}\r\n", resp_body.len());
            let mut resp =
                format!("HTTP/1.1 {}\r\n{}Connection: close\r\n\r\n", status, extra)
                    .into_bytes();
            resp.extend_from_slice(&resp_body);
            let _ = stream.write_all(&resp);
        }
    });
    port
}

// اجتماع 29: شبكة.أرسل من برنامج نيفا في الوضعين —
// النوع الافتراضي application/json والجسم يصل كما هو.
#[test]
fn network_post_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/echo\", \"بيانات\")\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v, "طريقة:POST نوع:application/json جسم:بيانات\n",
        "stdout={}", out_v
    );
}

// اجتماع 29: شبكة.أرسل بنوع محتوى مبدَّل (تدفق الجهاز:
// application/x-www-form-urlencoded) في الوضعين.
#[test]
fn network_post_custom_content_type_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/echo\", \"code=abc&grant_type=device_code\", \"application/x-www-form-urlencoded\")\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "طريقة:POST نوع:application/x-www-form-urlencoded جسم:code=abc&grant_type=device_code\n",
        "stdout={}", out_v
    );
}

// اجتماع 29: POST إلى مسار غائب خطأ يسمّي 404 في الوضعين.
#[test]
fn network_post_non_2xx_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/missing\", \"بيانات\")\n",
        port
    );
    let flags_i = ["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let flags_v = ["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let (_o, err_i, c_i) = run_src_flags(&src, &flags_i);
    let (_o2, err_v, c_v) = run_src_flags(&src, &flags_v);
    assert_ne!(c_i, 0, "interpreter accepted a 404 POST");
    assert_ne!(c_v, 0, "VM accepted a 404 POST");
    assert!(err_i.contains("404"), "interpreter stderr: {}", err_i);
    assert!(err_v.contains("404"), "VM stderr: {}", err_v);
}

// اجتماع 29: POST بلا بوابة شبكة مرفوض في الوضعين.
#[test]
fn network_post_gate_refused_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/echo\", \"بيانات\")\n",
        port
    );
    let (_o, err_i, c_i) = run_src_flags(&src, &["--مفسر"]);
    let (_o2, err_v, c_v) = run_src_flags(&src, &[]);
    assert_ne!(c_i, 0, "interpreter allowed POST without the flag");
    assert_ne!(c_v, 0, "VM allowed POST without the flag");
    assert!(
        err_i.contains("الشبكة مغلقة افتراضيا"),
        "interpreter stderr: {}",
        err_i
    );
    assert!(
        err_v.contains("الشبكة مغلقة افتراضيا"),
        "VM stderr: {}",
        err_v
    );
}

// اجتماع 30: شبكة.أرسل بالوسيط الرابع (قاموس
// ترويسات) — التفويض والمخصص يصلان في الوضعين.
#[test]
fn network_post_headers_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nترويسات = {{\"Authorization\": \"Bearer رمز-اختبار\"، \"X-Test\": \"قيمة\"}}\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/echo-headers\", \"بيانات\", \"application/json\", ترويسات)\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_flags(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    let (out_v, err_v, code_v) = run_src_flags(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(err_v, err_i);
    assert_eq!(
        out_v,
        "طريقة:POST تفويض:Bearer رمز-اختبار مخصص:قيمة نوع:application/json\n",
        "stdout={}",
        out_v
    );
}

// اجتماع 30: CR/LF في قيمة ترويسة يُرفض بخطأ
// عربي قبل أي شبكة في الوضعين.
#[test]
fn network_post_headers_crlf_refused_both_modes() {
    let port = net_post_test_server();
    let src = format!(
        "استورد شبكة\nترويسات = {{\"X-Bad\": \"حقن\\r\\nترويسة\"}}\nاطبع شبكة.أرسل(\"http://127.0.0.1:{}/echo\", \"بيانات\", \"application/json\", ترويسات)\n",
        port
    );
    let flags_i = ["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let flags_v = ["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"];
    let (_o, err_i, c_i) = run_src_flags(&src, &flags_i);
    let (_o2, err_v, c_v) = run_src_flags(&src, &flags_v);
    assert_ne!(c_i, 0, "interpreter accepted a CRLF header");
    assert_ne!(c_v, 0, "VM accepted a CRLF header");
    assert!(err_i.contains("ترويسة"), "interpreter stderr: {}", err_i);
    assert!(err_v.contains("ترويسة"), "VM stderr: {}", err_v);
}

// المهمة 22: إلغاء جلب طويل من stdin — يكتب
// الاختبار سطر الإلغاء في stdin الطفل بعد 800ms
// (والجلب المتدفق يستغرق ~3s)، فيمسك البرنامج
// الخطأ بـحاول/ماعدا ويطبع «أُلغي» ويخرج 0.
fn run_src_stdin(src: &str, flags: &[&str], stdin_line: &str, delay_ms: u64) -> (String, String, i32) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "rio4140-engine-cancel-{}-{}",
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
    let mut child = cmd
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run نيفا");
    let mut stdin = child.stdin.take().unwrap();
    let line = stdin_line.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        use std::io::Write;
        let _ = stdin.write_all(line.as_bytes());
        let _ = stdin.flush();
    });
    let result = child.wait_with_output().expect("wait نيفا");
    let _ = fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
        result.status.code().unwrap_or(-1),
    )
}

#[test]
fn network_fetch_cancelled_from_stdin_both_modes() {
    let port = net_test_server();
    let src = format!(
        "استورد شبكة\nحاول:\n    اطبع شبكة.اجلب(\"http://127.0.0.1:{}/drip\")\nماعدا:\n    اطبع \"أُلغي\"\n",
        port
    );
    let (out_i, err_i, code_i) = run_src_stdin(
        &src,
        &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
        "\u{0001}نيفا-إلغاء\n",
        800,
    );
    let (out_v, err_v, code_v) = run_src_stdin(
        &src,
        &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"],
        "\u{0001}نيفا-إلغاء\n",
        800,
    );
    assert_eq!(code_i, 0, "interpreter failed: stderr={}", err_i);
    assert_eq!(code_v, 0, "VM failed: stderr={}", err_v);
    assert_eq!(out_v, out_i);
    assert_eq!(out_v, "أُلغي\n", "stdout={}", out_v);
}

#[test]
fn network_fetch_parity_in_vm() {    // Meeting 25 task 4 (ك-1.3): شبكة.اجلب يعمل الآن في VM
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

// --- مقابس (catalog task 13, from net/) ---

/// A one-shot TCP echo server: accepts one client, echoes one
/// message back, exits. Returns the port it bound.
fn tcp_echo_server_once() -> (u16, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf) {
                if n > 0 {
                    let _ = stream.write_all(&buf[..n]);
                }
            }
        }
    });
    (port, handle)
}

/// A one-shot UDP echo server: echoes one datagram back.
fn udp_echo_server_once() -> (u16, std::thread::JoinHandle<()>) {
    use std::net::UdpSocket;
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = sock.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        if let Ok((n, src)) = sock.recv_from(&mut buf) {
            let _ = sock.send_to(&buf[..n], src);
        }
    });
    (port, handle)
}

/// Does this machine allow a UDP loopback send? Same probe as
/// the sockets unit tests: this VM's sandbox blocks UDP sendto
/// with EPERM, GitHub CI runners allow it.
fn udp_loopback_works() -> bool {
    use std::net::UdpSocket;
    let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    let Ok(port) = sock.local_addr().map(|a| a.port()) else {
        return false;
    };
    sock.send_to(b"p", ("127.0.0.1", port)).is_ok()
}

/// Format a port the way `اطبع` prints a number: Arabic-Indic
/// digits.
fn arabic_digits(n: u16) -> String {
    const DIGITS: [&str; 10] = ["٠", "١", "٢", "٣", "٤", "٥", "٦", "٧", "٨", "٩"];
    n.to_string()
        .chars()
        .map(|c| DIGITS[c as usize - '0' as usize])
        .collect()
}

#[test]
fn sockets_tcp_echo_both_modes() {
    // Client: connect, send, receive, close — identical
    // output in both modes, fresh echo server per mode.
    for interpreter in [true, false] {
        let (port, server) = tcp_echo_server_once();
        let prog = format!(
            "استورد شبكة\nم = شبكة.اتصل(\"127.0.0.1\"، {})\nاطبع شبكة.أرسل(م، \"مرحبا\")\nاطبع شبكة.استقبل(م)\nشبكة.أغلق(م)\nاطبع \"تم\"\n",
            port
        );
        let flags: &[&str] = if interpreter {
            &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) = run_src_flags(&prog, flags);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(out, "١٠\nمرحبا\nتم\n");
        server.join().unwrap();
    }
    // The closed gate refuses with the same Arabic error in
    // both modes (the program needs no server for this).
    let prog = "استورد شبكة\nم = شبكة.اتصل(\"127.0.0.1\"، 9)\n";
    let (_, err_i, code_i) = run_src_flags(prog, &["--مفسر"]);
    let (_, err_v, code_v) = run_src_flags(prog, &[]);
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("الشبكة مغلقة افتراضيا"), "stderr: {}", err_i);
    assert_eq!(err_v, err_i, "VM gate error differs from interpreter");
    // A private address is refused the same way in both
    // modes, even with the network flag on.
    let prog = "استورد شبكة\nم = شبكة.اتصل(\"10.1.2.3\"، 80)\n";
    let (_, err_i, code_i) = run_src_flags(prog, &["--مفسر", "--اسمح-بالشبكة"]);
    let (_, err_v, code_v) = run_src_flags(prog, &["--اسمح-بالشبكة"]);
    assert_ne!(code_i, 0);
    assert_ne!(code_v, 0);
    assert!(err_i.contains("مرفوض"), "stderr: {}", err_i);
    assert!(err_i.contains("خاص"), "stderr: {}", err_i);
    assert_eq!(err_v, err_i, "VM private-address error differs");
}

#[test]
fn sockets_tcp_listen_accept_both_modes() {
    // Server: the Niva program itself listens, accepts, and
    // answers; a Rust client connects and checks the reply.
    // The child runs in the background while the client
    // connects — run_src_flags cannot do that (it waits).
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    for interpreter in [true, false] {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "rio4140-engine-sockets-{}-{}",
            std::process::id(),
            n
        ));
        fs::create_dir_all(&dir).unwrap();
        let port = free_port();
        let src = format!(
            "استورد شبكة\nم = شبكة.استمع({})\nاطبع شبكة.المنفذ(م)\nع = شبكة.أقبل(م)\nاطبع شبكة.استقبل(ع)\nشبكة.أرسل(ع، \"جواب\")\nشبكة.أغلق(ع)\nشبكة.أغلق(م)\nاطبع \"تم\"\n",
            port
        );
        fs::write(dir.join("برنامج.نيفا"), &src).unwrap();
        let mut cmd = Command::new(bin());
        cmd.arg(dir.join("برنامج.نيفا"));
        if interpreter {
            cmd.arg("--مفسر");
        }
        cmd.args(["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]);
        let mut child = cmd
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run نيفا");
        // Poll until the server listens (up to 5 seconds).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(_) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        panic!("Niva server never listened on {}", port);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all("سؤال".as_bytes()).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 64];
        while got.len() < "جواب".len() {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        assert_eq!(String::from_utf8(got).unwrap(), "جواب");
        let out = child.wait_with_output().expect("wait نيفا");
        assert_eq!(
            out.status.code(),
            Some(0),
            "server failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).to_string(),
            format!("{}\nسؤال\nتم\n", arabic_digits(port))
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn sockets_udp_echo_both_modes() {
    if !udp_loopback_works() {
        eprintln!("تخطَّى: UDP loopback محظور في هذا الصندوق (EPERM) — يعمل في CI");
        return;
    }
    for interpreter in [true, false] {
        let (port, server) = udp_echo_server_once();
        let prog = format!(
            "استورد شبكة\nم = شبكة.مقبس(\"udp\")\nاطبع شبكة.أرسل_إلى(م، \"127.0.0.1\"، {}، \"حزمة\")\nر = شبكة.استقبل_من(م)\nاطبع ر[\"نص\"]\nاطبع ر[\"مضيف\"]\nاطبع ر[\"منفذ\"]\nشبكة.أغلق(م)\nاطبع \"تم\"\n",
            port
        );
        let flags: &[&str] = if interpreter {
            &["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        } else {
            &["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"]
        };
        let (out, err, code) = run_src_flags(&prog, flags);
        assert_eq!(code, 0, "failed: stderr={}", err);
        assert_eq!(
            out,
            format!("٨\nحزمة\n127.0.0.1\n{}\nتم\n", arabic_digits(port))
        );
        server.join().unwrap();
    }
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
