//! بوابة مطابقة الخلفية الأصلية (C) — مهمة 29.
//!
//! تشغّل الجداول نفسها حرفيا التي تشغّلها بوابة
//! المطابقة الأصلية (parity_gate.rs: المفسر مقابل
//! الآلة الافتراضية) — لكن الطرف الثاني هنا هو
//! الثنائي الأصلي المبني بـ `نيفا ابن --أصلي`.
//! المقارنة حرفية (المخرج، الخطأ، رمز الخروج) بعد
//! تطبيع مسار المجلد المؤقت إلى `<مجلد>`.
//!
//! خارج النطاق عمدا: اختبارا خادم TLS
//! (parity_server وparity_server_h2) — الخلفية
//! الأصلية HTTP عادي فقط (بلا TLS)؛ ويحل محلهما
//! هنا اختبار خادم HTTP عادي (`c_parity_server_http`).
//!
//! حد AOT موثق (تخطّيه C_SKIP): حالة «تثبيت-محلي»
//! تنشئ وحدة في زمن التشغيل ثم تستوردها — والثنائي
//! الأصلي يترجم الوحدات وقت البناء، فلا يستطيع
//! استيراد مصدر نيفا وُلد بعد البناء (مثل أي مترجم
//! مسبق: لا ‏include‏ لملف يتولد أثناء التشغيل).
//! التخطي صريح ومطبوع، لا صامت.
//!
//! التشغيل: cargo test --test c_parity
//! (متسلسلا: cargo test -- --test-threads=1)

#[path = "parity_cases.rs"]
mod parity_cases;

use parity_cases as pc;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// ابنِ مصدرا إلى ثنائي أصلي داخل `dir` ثم شغّله
/// بلا أعلام؛ يعيد (المخرج، الخطأ، رمز الخروج) بعد
/// تطبيع مسار المجلد المؤقت نفسه في الخطأ.
/// فشل البناء نفسه خطأ اختبار (لا مقارنة صامتة).
fn run_native(source: &str, files: &[(&str, &str)]) -> (String, String, i32) {
    let dir = pc::fresh_dir();
    fs::write(dir.join("prog.نيفا"), source).unwrap();
    for (name, content) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
    // البناء أولا (مخرجه لا يدخل المقارنة).
    let build = Command::new(pc::bin())
        .args(["ابن", "--أصلي", "prog.نيفا"])
        .current_dir(&dir)
        .output()
        .expect("run نيفا ابن --أصلي");
    if !build.status.success() {
        // خطأ مصدر (لغوي/نحوي/دلالي): يظهر عند البناء
        // في الخلفية الأصلية وعند التشغيل في المفسر —
        // قارنه هو نفسه، ولا تعتبره فشل بناء. أما رسالة
        // «فشل بناء الثنائي الأصلي» فهي عطل خلفية
        // حقيقي (خطأ cc أو مولد) فيفشل الاختبار.
        let build_err =
            String::from_utf8_lossy(&build.stderr).to_string();
        assert!(
            !build_err.contains("فشل بناء الثنائي الأصلي"),
            "فشل بناء الثنائي الأصلي لمصدر:\n{}\nخطأ البناء:\n{}",
            source, build_err
        );
        let err = pc::normalize_temp_dir(build_err, &dir);
        let _ = fs::remove_dir_all(&dir);
        return (String::new(), err, build.status.code().unwrap_or(-1));
    }
    let bin_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let out = Command::new(dir.join(bin_name))
        .current_dir(&dir)
        .output()
        .expect("run الثنائي الأصلي");
    let err = pc::normalize_temp_dir(
        String::from_utf8_lossy(&out.stderr).to_string(),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        err,
        out.status.code().unwrap_or(-1),
    )
}

/// قارن المفسر بالثنائي الأصلي حرفيا.
fn assert_native_same(name: &str, source: &str, files: &[(&str, &str)]) {
    let (out_i, err_i, code_i) = pc::run_one(source, files, true);
    let (out_c, err_c, code_c) = run_native(source, files);
    assert_eq!(
        (out_i.clone(), err_i.clone(), code_i),
        (out_c.clone(), err_c.clone(), code_c),
        "اختلاف تكافؤ أصلي في «{}»:\n--- المفسر (خروج {}):\nمخرج:\n{}\nخطأ:\n{}\n--- الأصلي (خروج {}):\nمخرج:\n{}\nخطأ:\n{}",
        name, code_i, out_i, err_i, code_c, out_c, err_c
    );
}

fn assert_native_src(name: &str, lines: &[&str]) {
    assert_native_same(name, &pc::src(lines), &[]);
}

/// شغّل ملف مثال بالثنائي الأصلي مقابل المفسر.
fn assert_example_native(name: &str) {
    let exdir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    // المفسر أولا (من نسخة مؤقتة من مجلد الأمثلة).
    let dir_i = pc::fresh_dir();
    for entry in fs::read_dir(&exdir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            fs::copy(&path, dir_i.join(path.file_name().unwrap())).unwrap();
        }
    }
    let out_i = Command::new(pc::bin())
        .arg("--مفسر")
        .arg(dir_i.join(name))
        .current_dir(&dir_i)
        .output()
        .expect("run نيفا --مفسر");
    let err_i = pc::normalize_temp_dir(
        String::from_utf8_lossy(&out_i.stderr).to_string(),
        &dir_i,
    );
    let res_i = (
        String::from_utf8_lossy(&out_i.stdout).to_string(),
        err_i,
        out_i.status.code().unwrap_or(-1),
    );
    let _ = fs::remove_dir_all(&dir_i);
    // الأصلي: بناء ثم تشغيل من نسخة مؤقتة ثانية.
    let dir_c = pc::fresh_dir();
    for entry in fs::read_dir(&exdir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            fs::copy(&path, dir_c.join(path.file_name().unwrap())).unwrap();
        }
    }
    let build = Command::new(pc::bin())
        .args(["ابن", "--أصلي", name])
        .current_dir(&dir_c)
        .output()
        .expect("run نيفا ابن --أصلي");
    assert!(
        build.status.success(),
        "فشل بناء المثال الأصلي «{}»:\n{}",
        name,
        String::from_utf8_lossy(&build.stderr)
    );
    let stem = name.strip_suffix(".نيفا").unwrap();
    let bin_name = if cfg!(windows) {
        format!("{}.exe", stem)
    } else {
        stem.to_string()
    };
    let out_c = Command::new(dir_c.join(&bin_name))
        .current_dir(&dir_c)
        .output()
        .expect("run الثنائي الأصلي");
    let err_c = pc::normalize_temp_dir(
        String::from_utf8_lossy(&out_c.stderr).to_string(),
        &dir_c,
    );
    let res_c = (
        String::from_utf8_lossy(&out_c.stdout).to_string(),
        err_c,
        out_c.status.code().unwrap_or(-1),
    );
    let _ = fs::remove_dir_all(&dir_c);
    assert_eq!(
        res_i, res_c,
        "اختلاف تكافؤ أصلي في المثال «{}»:\n--- المفسر: {:?}\n--- الأصلي: {:?}",
        name, res_i, res_c
    );
}

#[test]
fn c_parity_examples() {
    for name in pc::EXAMPLES {
        assert_example_native(name);
    }
}

#[test]
fn c_parity_hesab_module() {
    let hesab = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("حساب.نيفا"),
    )
    .unwrap();
    assert_native_same(
        "حساب-وحدة",
        &pc::src(pc::HESAB_MAIN),
        &[("حساب.نيفا", &hesab)],
    );
}

#[test]
fn c_parity_package_import() {
    let main = pc::src(pc::PACKAGE_MAIN);
    assert_native_same(
        "حزمة-حزم-مجلد",
        &main,
        &[("حزم/أداة/أداة.نيفا", pc::PKG_42)],
    );
    assert_native_same(
        "حزمة-مخزن-أحدث",
        &main,
        &[
            (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", pc::PKG_1),
            (".نيفا/حزم/أداة-2.0.0/أداة.نيفا", pc::PKG_2),
        ],
    );
    assert_native_same(
        "حزمة-مخزن-قفل",
        &main,
        &[
            (".نيفا/حزم/أداة-1.0.0/أداة.نيفا", pc::PKG_1),
            (".نيفا/حزم/أداة-2.0.0/أداة.نيفا", pc::PKG_2),
            ("قفل.حزمة", pc::LOCK_1),
        ],
    );
    assert_native_same(
        "حزمة-غائبة",
        &pc::src(&["استورد لاوجود"]),
        &[],
    );
}

/// حالات خارج قدرة الترجمة المسبقة عمدا (تُتخطى
/// صراحة مع طباعة السبب، ولا تُحسب خضراء صامتة).
/// «تثبيت-محلي»: تنشئ وحدة أثناء التشغيل ثم تستوردها.
const C_SKIP: &[&str] = &["تثبيت-محلي"];

#[test]
fn c_parity_integration_scenarios() {
    let mut skipped = 0;
    for (name, lines) in pc::INTEGRATION_CASES {
        if C_SKIP.contains(&name) {
            skipped += 1;
            println!("تُخطى (حد AOT موثق): {}", name);
            continue;
        }
        assert_native_src(name, lines);
    }
    println!("حالات متكاملة: {} مرت، {} تُخطى (حد AOT)", pc::INTEGRATION_CASES.len() - skipped, skipped);
}

#[test]
fn c_parity_extra_probes() {
    for (name, lines) in pc::EXTRA_CASES {
        assert_native_src(name, lines);
    }
}

#[test]
fn c_parity_depth_limit() {
    assert_native_src("depth-500-completes", pc::DEPTH_OK);
    assert_native_src("depth-501-errors", pc::DEPTH_ERR);
}

#[test]
fn c_parity_million_iterations() {
    let source = pc::src(pc::MILLION_LINES);
    let t_i = Instant::now();
    let (out_i, err_i, code_i) = pc::run_one(&source, &[], true);
    let d_i = t_i.elapsed();
    let t_c = Instant::now();
    let (out_c, err_c, code_c) = run_native(&source, &[]);
    let d_c = t_c.elapsed();
    eprintln!(
        "مليون تكرار: المفسر {:?} — الأصلي {:?}",
        d_i, d_c
    );
    assert_eq!(code_i, 0, "المفسر لم يكمل المليون: {}", err_i);
    assert_eq!(
        (out_i, err_i, code_i),
        (out_c, err_c, code_c),
        "اختلاف تكافؤ أصلي في المليون تكرار"
    );
}

#[test]
fn c_parity_fib30() {
    let source = pc::src(pc::FIB30_LINES);
    let t_i = Instant::now();
    let (out_i, err_i, code_i) = pc::run_one(&source, &[], true);
    let d_i = t_i.elapsed();
    let t_c = Instant::now();
    let (out_c, err_c, code_c) = run_native(&source, &[]);
    let d_c = t_c.elapsed();
    eprintln!("ليف(٣٠): المفسر {:?} — الأصلي {:?}", d_i, d_c);
    assert_eq!(code_i, 0, "المفسر لم يكمل ليف(٣٠): {}", err_i);
    assert_eq!(out_i.trim(), "٨٣٢٠٤٠", "المفسر: ليف(٣٠) ≠ 832040: {}", out_i);
    assert_eq!(
        (out_i, err_i, code_i),
        (out_c, err_c, code_c),
        "اختلاف تكافؤ أصلي في ليف(٣٠)"
    );
}

/// خادم HTTP عادي (بلا TLS) — بديل اختباري خادم
/// TLS في البوابة الأصلية: الخلفية الأصلية HTTP
/// عادي فقط. خادم Rust TLS فقط دائما (الشهادة
/// الفارغة خطأ «تعذر قراءة ملف الشهادة»)، فلا
/// اتجاه «خادم مفسر» هنا أبدا: الاتجاهان على
/// الخادم الأصلي نفسه — جالب مفسر ثم جالب أصلي.
/// المقارنة: سطر البدء حرفيا + الجسم المجلوب من
/// كل مسار حرفيا.
#[test]
fn c_parity_server_http() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let server_src = pc::src(&[
        "استورد شبكة",
        "دالة عالج(طلب):",
        "    إذا طلب[\"مسار\"] == \"/text\":",
        "        ارجع \"وصل نص\"",
        "    وإلا إذا طلب[\"مسار\"] == \"/num\":",
        "        ارجع \"٤٢\"",
        "    وإلا:",
        "        ارجع \"وصل آخر\"",
        "شبكة.اخدم ١٨١٠٧ \"\" \"\" عالج",
    ]);
    let fetch_src = pc::src(&[
        "استورد شبكة",
        "اطبع شبكة.اجلب(\"http://127.0.0.1:18107/text\")",
        "اطبع شبكة.اجلب(\"http://127.0.0.1:18107/num\")",
        "اطبع شبكة.اجلب(\"http://127.0.0.1:18107/other\")",
    ]);
    // اقتل مستمعا قديما على المنفذ أولا (تشغيل سابق
    // فاشل قد يترك خادما يحجز المنفذ).
    let _ = Command::new("fuser").args(["-k", "18107/tcp"]).output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let dir = pc::fresh_dir();
    fs::write(dir.join("prog.نيفا"), &server_src).unwrap();
    let build = Command::new(pc::bin())
        .args(["ابن", "--أصلي", "prog.نيفا"])
        .current_dir(&dir)
        .output()
        .expect("run نيفا ابن --أصلي");
    assert!(
        build.status.success(),
        "فشل بناء خادم أصلي: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let bin_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let srv = Command::new(dir.join(bin_name))
        .args(["--اسمح-بالخادم"])
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn خادم أصلي");
    // حارس يقتل الخادم عند أي خروج (حتى الفشل) —
    // فلا يبقى مستمع يحجز المنفذ للتشغيل التالي.
    struct ServerGuard(Option<std::process::Child>);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            if let Some(mut c) = self.0.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
    let mut guard = ServerGuard(Some(srv));
    let srv = guard.0.as_mut().unwrap();
    let mut reader = BufReader::new(srv.stdout.take().unwrap());
    let mut start_line = String::new();
    reader.read_line(&mut start_line).unwrap();
    assert_eq!(
        start_line.trim(),
        "الخادم يعمل على http://127.0.0.1:18107",
        "سطر بدء الخادم الأصلي"
    );
    // 1) جالب مفسر من الخادم الأصلي.
    fs::write(dir.join("جالب.نيفا"), &fetch_src).unwrap();
    let out_i = Command::new(pc::bin())
        .args(["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"])
        .arg("جالب.نيفا")
        .current_dir(&dir)
        .output()
        .expect("run جالب مفسر");
    assert_eq!(
        String::from_utf8_lossy(&out_i.stdout).to_string(),
        "وصل نص\n٤٢\nوصل آخر\n",
        "جلب المفسر من الخادم الأصلي: {}",
        String::from_utf8_lossy(&out_i.stderr)
    );
    // 2) جالب أصلي من الخادم الأصلي نفسه.
    let build2 = Command::new(pc::bin())
        .args(["ابن", "--أصلي", "جالب.نيفا"])
        .current_dir(&dir)
        .output()
        .expect("run نيفا ابن --أصلي");
    assert!(
        build2.status.success(),
        "فشل بناء جالب أصلي: {}",
        String::from_utf8_lossy(&build2.stderr)
    );
    let bin2 = if cfg!(windows) { "جالب.exe" } else { "جالب" };
    let out_c = Command::new(dir.join(bin2))
        .args(["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"])
        .current_dir(&dir)
        .output()
        .expect("run جالب أصلي");
    assert_eq!(
        String::from_utf8_lossy(&out_c.stdout).to_string(),
        "وصل نص\n٤٢\nوصل آخر\n",
        "جلب الأصلي من الخادم الأصلي: {}",
        String::from_utf8_lossy(&out_c.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}
