//! مصنوعات AOT — مهمة 8 (ك-2.2): `نيفا ابن` و`نيفا شغل`
//! والملف التنفيذي المستقل.
//!
//! كل اختبار يشغل الثنائي الحقيقي (CARGO_BIN_EXE_niva)
//! في مجلدات مؤقتة فريدة، ويقارن المخرج والخطأ ورمز
//! الخروج حرفيا بين التشغيل العادي والمصنوع — كما تفعل
//! بوابة التكافؤ. لا تطبيع إلا لمسار المجلد المؤقت
//! نفسه في رسائل الخطأ.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn bin() -> String {
    env!("CARGO_BIN_EXE_niva").to_string()
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "niva-artifact-{}-{}",
        std::process::id(),
        n
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn src(lines: &[&str]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// نسخ مجلد الأمثلة كاملا (ملفات فقط) إلى مجلد جديد.
fn copy_examples_to(dir: &PathBuf) {
    let exdir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    for entry in fs::read_dir(&exdir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
        }
    }
}

/// PATH منزوع منه نيفا المثبتة — لإثبات أن المستقل
/// يعمل وحده. على Unix مجلدا النظام فقط؛ وعلى Windows
/// مجلدا النظام من SystemRoot (مسارات Unix غير موجودة
/// هناك، ووضعها كان يكسر تحميل مكتبات النظام).
fn scrubbed_path() -> String {
    #[cfg(unix)]
    {
        "/usr/bin:/bin".to_string()
    }
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        format!("{}\\System32;{}", root, root)
    }
}

/// اسم الملف التنفيذي المستقل — بلا لاحقة على Unix،
/// و.exe على Windows (كما يبنيه build_standalone).
fn standalone_name() -> &'static str {
    if cfg!(windows) {
        "fib.exe"
    } else {
        "fib"
    }
}

/// شغل أمر نيفا بمعاملاته في مجلد، وأعد
/// (المخرج، الخطأ بعد تطبيع المجلد، رمز الخروج).
fn run_cmd(args: &[&str], dir: &PathBuf, scrub_path: bool) -> (String, String, i32) {
    let mut cmd = Command::new(bin());
    cmd.args(args).current_dir(dir);
    if scrub_path {
        // بلا ~/.local/bin في PATH — إثبات أن المستقل
        // يعمل وحده، لا بنيفا المثبتة.
        cmd.env("PATH", scrubbed_path());
    }
    let out = cmd.output().expect("run نيفا");
    let dir_str = dir.to_string_lossy().to_string();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr)
            .to_string()
            .replace(&dir_str, "<مجلد>"),
        out.status.code().unwrap_or(-1),
    )
}

/// كل مثال يعمل ثلاث مرات بمخرج متطابق: تشغيل عادي،
/// ثم بناء مصنوع، ثم تشغيل المصنوع.
#[test]
fn artifact_roundtrip_all_examples() {
    for name in [
        "01-hello.نيفا",
        "02-variables.نيفا",
        "03-conditions.نيفا",
        "04-loops.نيفا",
        "05-functions.نيفا",
        "06-lists-dicts.نيفا",
        "07-try-files.نيفا",
        "08-full-program.نيفا",
        "09-modules.نيفا",
        "10-types.نيفا",
        "11-classes.نيفا",
        "12-inheritance-interfaces.نيفا",
        "13-closures.نيفا",
        "14-lambdas.نيفا",
        "15-methods.نيفا",
        "16-stdlib.نيفا",
        "17-word-operators.نيفا",
    ] {
        // تشغيل عادي في مجلد خاص.
        let dir_a = fresh_dir();
        copy_examples_to(&dir_a);
        let direct = run_cmd(&[name], &dir_a, false);
        // بناء المصنوع وتشغيله في مجلد خاص آخر.
        let dir_b = fresh_dir();
        copy_examples_to(&dir_b);
        let (build_out, build_err, build_code) = run_cmd(&["ابن", name], &dir_b, false);
        assert_eq!(
            build_code, 0,
            "ابن فشل في المثال «{}»: مخرج:\n{}\nخطأ:\n{}",
            name, build_out, build_err
        );
        let artifact_name = name.replace(".نيفا", ".نيفاب");
        assert!(
            dir_b.join(&artifact_name).exists(),
            "المصنوع {} غير موجود بعد ابن في المثال «{}»",
            artifact_name,
            name
        );
        let via_artifact = run_cmd(&["شغل", &artifact_name], &dir_b, false);
        assert_eq!(
            direct, via_artifact,
            "اختلاف مصنوع في المثال «{}»:\n--- عادي: {:?}\n--- مصنوع: {:?}",
            name, direct, via_artifact
        );
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }
}

/// برنامج سائق يستورد وحدة حساب — ذهابا وإيابا عبر
/// المصنوع، بما فيه الإغلاقات داخل الوحدة المستوردة
/// (اختبار اصطدام معرّفات الإغلاقات عند تحميل وحدة
/// وقت التشغيل فوق مصنوع).
#[test]
fn artifact_roundtrip_hesab_module() {
    let hesab = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("حساب.نيفا"),
    )
    .unwrap();
    let driver = src(&[
        "استورد حساب",
        "اطبع حساب.اجمع(١٠، ٢٠)",
        "اطبع حساب.ضعف(٢١)",
        "من حساب استورد اجمع",
        "اطبع اجمع(حساب.ضعف(٣)، ٤)",
        "دالة اصنع():",
        "    س = ٠",
        "    دالة زد():",
        "        س = س + ١",
        "        ارجع س",
        "    ارجع زد",
        "ع = اصنع()",
        "اطبع ع()",
        "اطبع ع()",
        "اطبع حساب.ضعف(ع())",
    ]);
    for (label, mode) in [("عادي", false), ("مصنوع", true)] {
        let dir = fresh_dir();
        fs::write(dir.join("prog.نيفا"), &driver).unwrap();
        fs::write(dir.join("حساب.نيفا"), &hesab).unwrap();
        let got = if mode {
            let (_, build_err, build_code) = run_cmd(&["ابن", "prog.نيفا"], &dir, false);
            assert_eq!(build_code, 0, "ابن فشل: {}", build_err);
            run_cmd(&["شغل", "prog.نيفاب"], &dir, false)
        } else {
            run_cmd(&["prog.نيفا"], &dir, false)
        };
        assert_eq!(
            got,
            (
                "٣٠\n٤٢\n١٠\n١\n٢\n٦\n".to_string(),
                String::new(),
                0
            ),
            "اختلاف في وضع «{}»: {:?}",
            label,
            got
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

/// ليف(25) = 75025 عبر المصنوع — قيمة فيبوناتشي
/// غير بديهية، لا نسخة مصغرة من ليف(30) (قياس
/// ليف(30) الكامل يدوي في التقرير، لا في الاختبار،
/// لئلا يبطئ CI عشرات الثواني).
#[test]
fn artifact_fib_value() {
    let dir = fresh_dir();
    let fib = src(&[
        "دالة ليف(ن):",
        "    إذا ن < ٢:",
        "        ارجع ن",
        "    وإلا:",
        "        ارجع ليف(ن - ١) + ليف(ن - ٢)",
        "اطبع ليف(٢٥)",
    ]);
    fs::write(dir.join("fib.نيفا"), &fib).unwrap();
    let (_, build_err, build_code) = run_cmd(&["ابن", "fib.نيفا"], &dir, false);
    assert_eq!(build_code, 0, "ابن فشل: {}", build_err);
    let (out, err, code) = run_cmd(&["شغل", "fib.نيفاب"], &dir, false);
    assert_eq!((out.trim(), err.as_str(), code), ("٧٥٠٢٥", "", 0));
    let _ = fs::remove_dir_all(&dir);
}

/// الملف التنفيذي المستقل يعمل وحده: PATH بلا
/// ~/.local/bin، ومجلد عمل نظيف، واستدعاء بمسار
/// مطلق من مجلد آخر — ولا أثر لنيفا المثبتة.
#[test]
fn artifact_standalone_runs_alone() {
    let dir = fresh_dir();
    let fib = src(&[
        "دالة ليف(ن):",
        "    إذا ن < ٢:",
        "        ارجع ن",
        "    وإلا:",
        "        ارجع ليف(ن - ١) + ليف(ن - ٢)",
        "اطبع ليف(٢٥)",
    ]);
    fs::write(dir.join("fib.نيفا"), &fib).unwrap();
    let (_, build_err, build_code) =
        run_cmd(&["ابن", "--مستقل", "fib.نيفا"], &dir, false);
    assert_eq!(build_code, 0, "ابن --مستقل فشل: {}", build_err);
    let standalone = dir.join(standalone_name());
    assert!(standalone.exists(), "الملف المستقل غير موجود");
    // بتات التنفيذ Unix فقط — على Windows لا بتات
    // تنفيذية، والوجود باسم .exe هو الإثبات.
    #[cfg(unix)]
    {
        let mode = fs::metadata(&standalone).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "الملف المستقل ليس تنفيذيا");
    }
    // تشغيل بمسار نسبي وPATH منزوع منه نيفا — الملف
    // نفسه هو البرنامج، لا نيفا المثبتة.
    // على Unix يلزم ./ لأن مجلد العمل ليس في PATH؛
    // وعلى Windows .\fib.exe مسار نسبي صريح يحله
    // النظام ضد مجلد العمل مباشرة.
    #[cfg(unix)]
    let rel = "./fib";
    #[cfg(windows)]
    let rel = ".\\fib.exe";
    let mut cmd = Command::new(rel);
    cmd.current_dir(&dir).env("PATH", scrubbed_path());
    let res = cmd.output().expect("run standalone");
    assert_eq!(res.status.code(), Some(0));
    assert_eq!(
        (
            String::from_utf8_lossy(&res.stdout).trim().to_string(),
            String::from_utf8_lossy(&res.stderr).to_string(),
            res.status.code().unwrap_or(-1)
        ),
        ("٧٥٠٢٥".to_string(), String::new(), 0)
    );
    // تشغيل بمسار مطلق من مجلد آخر.
    let other = fresh_dir();
    let mut cmd = Command::new(&standalone);
    cmd.current_dir(&other).env("PATH", scrubbed_path());
    let res = cmd.output().expect("run standalone");
    assert_eq!(res.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&res.stdout).trim(),
        "٧٥٠٢٥",
        "المستقل بمسار مطلق من مجلد آخر: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&other);
}

/// مدخلات خاطئة: مصدر بدل مصنوع، ومصنوع مقطوع،
/// وملف غائب، ونموذج معكوس، وأمر بلا ملف.
#[test]
fn artifact_rejects_bad_inputs() {
    let dir = fresh_dir();
    fs::write(dir.join("prog.نيفا"), "اطبع ٥\n").unwrap();
    // شغل على ملف مصدر.
    let (_, err, code) = run_cmd(&["شغل", "prog.نيفا"], &dir, false);
    assert_eq!(code, 1, "شغل قبل مصدرا");
    assert!(err.contains("ليس مصنوع"), "رسالة شغل على مصدر: {}", err);
    // مصنوع مقطوع.
    let (_, _, build_code) = run_cmd(&["ابن", "prog.نيفا"], &dir, false);
    assert_eq!(build_code, 0);
    let mut bytes = fs::read(dir.join("prog.نيفاب")).unwrap();
    bytes.truncate(bytes.len() / 2);
    fs::write(dir.join("مقطوع.نيفاب"), &bytes).unwrap();
    let (_, err, code) = run_cmd(&["شغل", "مقطوع.نيفاب"], &dir, false);
    assert_eq!(code, 1, "شغل قبل مصنوعا مقطوعا");
    assert!(err.contains("تالف"), "رسالة مصنوع مقطوع: {}", err);
    // ملف غائب.
    let (_, err, code) = run_cmd(&["شغل", "غائب.نيفاب"], &dir, false);
    assert_eq!(code, 1, "شغل قبل ملفا غائبا");
    assert!(!err.is_empty(), "رسالة ملف غائب فارغة");
    // النموذج المعكوس مرفوض في البناء كما في التشغيل.
    fs::write(dir.join("معكوس.نيفا"), "٥ = س\n").unwrap();
    let (_, err, code) = run_cmd(&["ابن", "معكوس.نيفا"], &dir, false);
    assert_eq!(code, 1, "ابن قبل النموذج المعكوس");
    assert!(
        err.contains("هدف التعيين غير صالح"),
        "رسالة النموذج المعكوس: {}",
        err
    );
    // ابن بلا ملف، وشغل بلا ملف.
    let (_, _, code) = run_cmd(&["ابن"], &dir, false);
    assert_eq!(code, 1, "ابن بلا ملف نجح");
    let (_, _, code) = run_cmd(&["شغل"], &dir, false);
    assert_eq!(code, 1, "شغل بلا ملف نجح");
    let _ = fs::remove_dir_all(&dir);
}
