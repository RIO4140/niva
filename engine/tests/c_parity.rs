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

/// حد السماحية المسمى: وحدتان عائمتان أخيرتان (2 ULP).
/// السبب (اجتماع 82 §97): تباين libm في الدوال
/// المتسامية عبر المنصات — `exp` غير ملزم التقريب
/// الصحيح في IEEE 754 — والصيغة مطابقة حرفيا في
/// الطرفين؛ فالفرق حتى 2 ULP في الخانة الأخيرة ليس
/// خطأ منطق في نيفا. تسري فقط على الحالات المعلَّمة
/// في `parity_cases.rs` (FLOAT_TOLERANT_CASES)، وفي
/// `c_parity` وحدها — لا على `parity_gate` ولا على
/// أي حالة غير معلَّمة.
const MAX_ULP_DIFF: u64 = 2;

/// وحدة عائمة أخيرة (ULP) عند مقدار `x` (الموجب).
/// للصفر ودون الطبيعي: أصغر موجب دون طبيعي.
fn ulp_of(x: f64) -> f64 {
    let ax = x.abs();
    if !ax.is_finite() || ax == 0.0 {
        return f64::MIN_POSITIVE;
    }
    let e = (ax.to_bits() >> 52) & 0x7ff;
    if e <= 52 {
        return f64::MIN_POSITIVE;
    }
    f64::from_bits((e - 52) << 52)
}

/// حوّل سطر مخرجات إلى عدد عائم إن كان السطر كله
/// عددا، وإلا لا شيء. المخرجات بأرقام عربية مشرقية
/// (٠-٩) والفاصل ٫ (U+066B)؛ وتُعاد إلى اللاتينية
/// قبل القراءة (والسالب ASCII `-` والأس `e` كما هما).
fn parse_output_number(line: &str) -> Option<f64> {
    let mut latin = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '٠'..='٩' => latin.push(char::from(b'0' + (c as u32 - '٠' as u32) as u8)),
            '٫' => latin.push('.'),
            other => latin.push(other),
        }
    }
    latin.parse::<f64>().ok()
}

/// قارن مخرجين سطرا بسطر بسماحية 2 ULP للأسطر
/// العددية وحدها. السطر المتساوي حرفيا يمر؛ والسطر
/// العددي في الطرفين يمر إن كان الفرق ≤ 2 ULP عند
/// مقدار الأكبر؛ والسطر العددي في طرف دون الآخر
/// فشل صريح؛ وعدد الأسطر المختلف فشل صريح.
fn outputs_close_ulp(a: &str, b: &str) -> bool {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    if la.len() != lb.len() {
        return false;
    }
    la.iter().zip(lb.iter()).all(|(x, y)| {
        if x == y {
            return true;
        }
        match (parse_output_number(x), parse_output_number(y)) {
            (Some(fx), Some(fy)) => {
                if !fx.is_finite() || !fy.is_finite() {
                    return false;
                }
                let diff = (fx - fy).abs();
                let ulp = ulp_of(fx.abs().max(fy.abs()));
                diff <= (MAX_ULP_DIFF as f64) * ulp
            }
            _ => false,
        }
    })
}

/// قارن المفسر بالثنائي الأصلي بسماحية 2 ULP في
/// المخرج فقط (للحالات المعلَّمة وحدها)؛ والخطأ
/// (stderr) ورمز الخروج يُقارنان حرفيا دائما بلا
/// أي سماحية.
fn assert_native_same_tol(name: &str, source: &str, files: &[(&str, &str)]) {
    let (out_i, err_i, code_i) = pc::run_one(source, files, true);
    let (out_c, err_c, code_c) = run_native(source, files);
    assert_eq!(
        (err_i.clone(), code_i),
        (err_c.clone(), code_c),
        "اختلاف تكافؤ أصلي (خطأ/خروج) في «{}»:\n--- المفسر (خروج {}):\nخطأ:\n{}\n--- الأصلي (خروج {}):\nخطأ:\n{}",
        name, code_i, err_i, code_c, err_c
    );
    assert!(
        outputs_close_ulp(&out_i, &out_c),
        "اختلاف تكافؤ أصلي (مخرج، فوق 2 ULP) في «{}»:\n--- المفسر:\n{}\n--- الأصلي:\n{}",
        name, out_i, out_c
    );
}

fn assert_native_src(name: &str, lines: &[&str]) {
    assert_native_same(name, &pc::src(lines), &[]);
}

fn assert_native_src_tol(name: &str, lines: &[&str]) {
    assert_native_same_tol(name, &pc::src(lines), &[]);
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
/// «تنفيذ-ج3-69» و«تنفيذ-ج3-69-أخطاء»: تنفيذ نص بلا
/// مفسر مضمّن في الثنائي الأصلي (J3-69) — فحالتا
/// التكافؤ في بوابة المفسر/الآلة فقط.
/// «شبكة9-...» (التسع): وحدة الشبكة مرفوضة أصليا على
/// ويندوز (حد AOT موثق — الاجتماع 96 §111)، على غرار
/// حالات شبكة8؛ والمفسر/الآلة يغطيانها في parity_gate.
const C_SKIP: &[&str] = &["تثبيت-محلي", "تنفيذ-ج3-69", "تنفيذ-ج3-69-أخطاء", "مولدات-أساسية", "مولدات-كسل", "مولدات-حالة", "مولدات-لانهائي", "مولدات-استنفاد", "مولدات-ولد-من", "مولدات-اغلاق", "مولدات-استئناف-متداخل", "مولدات-تعبير", "مولدات-تعبير-كسل", "مولدات-خطأ-داخلي", "مولدات-متداخل-دالتان", "مولدات-نوع-وطباعة", "مولدات-ولد-من-خطأ", "مولدات-اغلاق-جديد", "مولدات-ولد-بلا-قيمة", "مولدات-ارجاع-مولد", "مولدات-مهمل-بلا-اغلاق", "مولدات-تعليق-داخل-اذا", "شبكة8-طلب-جديد", "شبكة8-ضوابط-سلسلة", "شبكة8-مصادقة-سلسلة", "شبكة8-توقيع-شهادة", "شبكة8-بلا-رأس-مكرر", "شبكة8-ضابط-مهلة-صفر", "شبكة8-ضابط-محاولات-كثيرة", "شبكة8-ضابط-ترميز-خطأ", "شبكة8-ضابط-تحقق-شهادة", "شبكة8-نفذ-بلا-رابط", "شبكة8-نفذ-طريقة-فارغة", "شبكة8-نفذ-خيار-مجهول", "شبكة8-نفذ-رأس-سيئ", "شبكة8-نفذ-جسم-سيئ", "شبكة8-نفذ-مدى-مقلوب", "شبكة8-طلب-رابط-فارغ", "شبكة8-نفذ-شبكة-مغلقة", "شبكة8-مع-كوكيز", "شبكة8-أرسل-json-بلا-رابط", "شبكة8-أرسل-نموذج-حقول-سيئة", "شبكة8-أرسل-نموذج-زوج-سيئ", "شبكة8-أرسل-خام-جسم-سيئ", "شبكة8-اطلب-مخصص-طريقة-فارغة", "شبكة8-ارفع-ملف-غائب", "شبكة8-نزل-الكل-روابط-سيئة", "شبكة8-نزل-من-قائمة-غائبة", "شبكة8-بحفظ-في", "شبكة8-اجلب-ftp-رابط-سيئ", "شبكة8-نزل-ftp-مسار-فارغ", "شبكة8-ارفع-ftp-ملف-غائب", "شبكة8-اشترك-mqtt-وسيط-سيئ", "شبكة8-اشترك-mqtt-مهلة-سيئة", "شبكة8-انشر-mqtt-موضوع-فارغ", "شبكة8-اجلب-sftp-مرفوض", "شبكة8-نزل-sftp-مرفوض", "شبكة8-اطلب-http3-مرفوض", "شبكة8-نزل-خلفية-مرفوض", "شبكة8-اجلب-ftps-رابط-سيئ", "شبكة8-جلسة-جديدة", "شبكة8-جلسة-مسماة", "شبكة8-جلسة-حفظ-قراءة", "شبكة8-اقرأ-جلسة-غائبة-قراءة", "شبكة8-اقرأ-جلسة-غائبة", "شبكة8-اقرأ-كوكيز-غائب", "شبكة8-اقرأ-وسم-غائب", "شبكة8-رمز-عنوان", "شبكة8-أضف-معامل", "شبكة8-رد-عرض", "شبكة8-نسق-json", "شبكة8-نسق-json-سيئ", "شبكة8-وقع-طلب-خوارزمية", "شبكة8-احفظ-وسم-بلا-وسم", "شبكة8-عناويني-قائمة", "شبكة8-عنوان-واجهة-lo", "شبكة8-عنوان-واجهة-غائبة", "شبكة8-عناويني-مختصرة", "شبكة8-مساراتي-عدد", "شبكة8-مسار-لمضيف", "شبكة8-حالة-الشبكة-عدد", "شبكة8-جيراني-نوع", "شبكة8-جار-غائب", "شبكة8-إحصاءات-بروتوكول", "شبكة8-قواعد-توجيه", "شبكة8-معلومات-جهاز", "شبكة8-افحص-منفذ-خطأ", "شبكة8-افحص-نطاق-كبير", "شبكة8-تتبع-أقصى-سيئ", "شبكة8-اتصل-مشفر-مغلق", "شبكة8-اتصل-بمهلة-ثوان-سيئة", "شبكة8-انبض-مستمر-عدد-سيئ", "شبكة8-حل-اسم-فارغ", "شبكة8-افحص-udp-منفذ-سيئ", "شبكة8-قواعد-جدار-نوع", "شبكة8-اتصالاتي-نوع", "شبكة9-انبض-udp", "شبكة9-اكشف-الخدمة-مغلق", "شبكة9-اكتب-الفحص-xml", "شبكة9-اكتب-الفحص-بحث", "شبكة9-اكتب-الفحص-كل-الصيغ", "شبكة9-أكمل-الفحص", "شبكة9-اتصل-udp-نوع", "شبكة9-استمع-udp-نوع", "شبكة9-اتصل-عبر-وكيل-سيئ"];

#[test]
fn c_parity_integration_scenarios() {
    let mut skipped = 0;
    for (name, lines) in pc::INTEGRATION_CASES {
        if C_SKIP.contains(&name) {
            skipped += 1;
            println!("تُخطى (حد AOT موثق): {}", name);
            continue;
        }
        if pc::FLOAT_TOLERANT_CASES.contains(&name) {
            assert_native_src_tol(name, lines);
        } else {
            assert_native_src(name, lines);
        }
    }
    println!("حالات متكاملة: {} مرت، {} تُخطى (حد AOT)", pc::INTEGRATION_CASES.len() - skipped, skipped);
}

/// الدفعة الأخيرة DEF — البنود 73/77/78/98/104/109–112:
/// الحالات نفسها بالبادئة «دفعة-أخيرة-DEF-» مقابل
/// الثنائي الأصلي.
#[test]
fn c_parity_def_final_batch() {
    let mut n = 0;
    for (name, lines) in pc::INTEGRATION_CASES {
        if name.starts_with("دفعة-أخيرة-DEF-") {
            assert_native_src(name, lines);
            n += 1;
        }
    }
    assert!(n > 0, "لا حالات دفعة-أخيرة-DEF في الجدول");
    println!("حالات الدفعة الأخيرة DEF (أصلي): {}", n);
}

#[test]
fn c_parity_extra_probes() {
    for (name, lines) in pc::EXTRA_CASES {
        assert_native_src(name, lines);
    }
}

// الدفعة 4 — المتغيرات (اجتماع 58 §73): تكافؤ أصلي
// للإلحاق والتجميد والترابطية والفحص والمرجع —
// بلا قيم ديناميكية عشوائية (عشوائي/ثواني) لأن
// المقارنة حرفية.
#[test]
fn c_parity_batch4_variables() {
    assert_native_src(
        "دفعة4-إلحاق",
        &[
            "ن = \"أهلا\"",
            "ن += \" وسهلا\"",
            "اطبع ن",
            "ع = ٥",
            "ع += ٣",
            "اطبع ع",
            "ق = [١، ٢]",
            "ق += [٣]",
            "اطبع ق",
            "خ = {\"أ\": ١}",
            "خ += {\"ب\": ٢}",
            "اطبع خ",
            "غ += ٧",
            "اطبع غ",
        ],
    );
    assert_native_src(
        "دفعة4-ترابطية-فحص-مرجع",
        &[
            "خ = {}",
            "خ[\"أ\"] = ١",
            "خ[\"ب\"] = ٢",
            "اطبع مفاتيح(خ)",
            "خ += {\"ج\": ٣، \"أ\": ٩}",
            "اطبع خ",
            "اطبع مفاتيح(خ)",
            "س = ٥",
            "اطبع افحص_متغير(\"س\")",
            "ص = ١٠",
            "مرجع(\"ر\", \"ص\")",
            "ر = ٢٠",
            "اطبع ص",
            "اطبع افحص_متغير(\"ر\")",
            "احذف_متغير(\"ر\")",
            "اطبع ص",
            "ق = اعرض_البيئة()",
            "اطبع طول(ق)",
            "اطبع ق[٠]",
            "اطبع ق[٤]",
            "اطبع بيئة(\"رقم_السطر\")",
        ],
    );
    assert_native_src(
        "دفعة4-مجمد-خطأ",
        &["س = ٥", "جمد(\"س\")", "س = ٦"],
    );
    assert_native_src(
        "دفعة4-محذوف-خطأ",
        &["ص = ١", "احذف_متغير(\"ص\")", "اطبع ص"],
    );
}

#[test]
fn c_parity_batch5_files() {
    // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
    // المفسر مقابل الثنائي الأصلي حرفيا.
    assert_native_same(
        "دفعة5-نسخ-تكراري-مجلد-لمس-حذف",
        &pc::src(&[
            "أنشئ_مجلد(\"مجلد/فرعي\")",
            "اكتب_ملف(\"مجلد/أ.txt\"، \"أ\")",
            "اكتب_ملف(\"مجلد/فرعي/ب.txt\"، \"ب\")",
            "انسخ_تكراري(\"مجلد\"، \"نسخة\")",
            "اطبع اقرأ_ملف(\"نسخة/أ.txt\")",
            "اطبع اقرأ_ملف(\"نسخة/فرعي/ب.txt\")",
            "اطبع أنشئ_مجلد(\"س/ص/ع\"، \"750\")",
            "أنشئ_ملف(\"مجلد/أ.txt\")",
            "اطبع اقرأ_ملف(\"مجلد/أ.txt\")",
            "احذف_تكراري(\"نسخة\")",
            "اطبع موجود(\"نسخة\")",
        ]),
        &[],
    );
    assert_native_same(
        "دفعة5-محدث-خفيف",
        &pc::src(&[
            "اكتب_ملف(\"أ.txt\"، \"قديم\")",
            "اطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")",
            "اطبع انسخ_محدث(\"أ.txt\"، \"ب.txt\")",
            "اطبع اقرأ_ملف(\"ب.txt\")",
            "اطبع انسخ_خفيف(\"أ.txt\"، \"ج.txt\")",
            "اطبع اقرأ_ملف(\"ج.txt\")",
        ]),
        &[],
    );
    assert_native_same(
        "دفعة5-رفض-نسخ-مجلد",
        &pc::src(&[
            "أنشئ_مجلد(\"مجلد\")",
            "اكتب_ملف(\"مجلد/أ.txt\"، \"أ\")",
            "انسخ(\"مجلد\"، \"نسخة\")",
        ]),
        &[],
    );
    assert_native_same(
        "دفعة5-حارس-الجذر",
        &pc::src(&["احذف(\"/\")"]),
        &[],
    );
    assert_native_same(
        "دفعة5-حارس-النقطة",
        &pc::src(&["احذف(\".\")"]),
        &[],
    );
    assert_native_same(
        "دفعة5-حارس-الجذر-تكراري",
        &pc::src(&["احذف_تكراري(\"/\")"]),
        &[],
    );
    assert_native_same(
        "دفعة5-وضع-غير-ثماني",
        &pc::src(&["أنشئ_مجلد(\"أ\"، \"888\")"]),
        &[],
    );
    assert_native_same(
        "دفعة5-صلب-غائب",
        &pc::src(&["أنشئ_رابط_صلب(\"غائب.txt\"، \"ب.txt\")"]),
        &[],
    );
    assert_native_same(
        "دفعة5-حذف-غير-فارغ",
        &pc::src(&[
            "أنشئ_مجلد(\"مجلد\")",
            "اكتب_ملف(\"مجلد/أ.txt\"، \"أ\")",
            "احذف(\"مجلد\")",
        ]),
        &[],
    );
    assert_native_same(
        "دفعة5-أرشيف",
        &pc::src(&[
            "أنشئ_مجلد(\"مجلد/فرعي\")",
            "اكتب_ملف(\"مجلد/أ.txt\"، \"أ\")",
            "اكتب_ملف(\"مجلد/فرعي/ب.txt\"، \"ب\")",
            "انسخ_أرشيف(\"مجلد\"، \"نسخة\")",
            "اطبع اقرأ_ملف(\"نسخة/أ.txt\")",
            "اطبع اقرأ_ملف(\"نسخة/فرعي/ب.txt\")",
        ]),
        &[],
    );
}

#[test]
#[cfg(unix)]
fn c_parity_batch5_links() {
    // الدفعة 5 — الروابط (يونكس فقط: إنشاء الروابط
    // الرمزية على ويندوز يحتاج امتيازا).
    assert_native_same(
        "دفعة5-رابط-نسبي-يبقى-بعد-النقل",
        &pc::src(&[
            "أنشئ_مجلد(\"شجرة/أ/ب\")",
            "اكتب_ملف(\"شجرة/أ/ب/ملف.txt\"، \"داخل\")",
            "أنشئ_رابط_نسبي(\"شجرة/أ/ب/ملف.txt\"، \"شجرة/أ/رابط.txt\")",
            "اطبع اقرأ_ملف(\"شجرة/أ/رابط.txt\")",
            "انقل(\"شجرة\"، \"شجرة2\")",
            "اطبع اقرأ_ملف(\"شجرة2/أ/رابط.txt\")",
            "أنشئ_رابط_صلب(\"شجرة2/أ/ب/ملف.txt\"، \"صلب.txt\")",
            "اطبع اقرأ_ملف(\"صلب.txt\")",
        ]),
        &[],
    );
}

#[test]
#[cfg(unix)]
fn c_parity_batch5_xargs_missing_program() {
    // الدفعة 5 — مرر_أسماء.نفذ مع برنامج غائب (يونكس
    // فقط: إنشاء العمليات على ويندوز مرفوض بقرار
    // اجتماع 35 §50، فالثنائي الأصلي هناك يرفض
    // الإنشاء بينما المفسر يبلغ غياب البرنامج —
    // والحالة نفسها منقولة حرفيا من EXTRA_CASES
    // بقرار اجتماع 89 §104).
    assert_native_src(
        "مرر-لينكس5-خطأ-برنامج",
        &[
            "استورد مرر_أسماء",
            r#"اطبع مرر_أسماء.نفذ("برنامج_غائب_تماماً {}", ["أ"])"#,
        ],
    );
}

#[test]
fn c_parity_nav56_stack() {
    // اجتماع 56 §71 — الدفعة 2 (التنقل): المكدس
    // (دفع/تبديل/إخراج/عرض) — المفسر مقابل الثنائي
    // الأصلي حرفيا.
    assert_native_src(
        "nav56-مكدس",
        &[
            "أنشئ_مجلد(\"أ\")",
            "أنشئ_مجلد(\"ب\")",
            "ادفع_مجلد(\"أ\")",
            "ادفع_مجلد(\"~/ب\")",
            "س = اعرض_المكدس()",
            "اطبع طول(س)",
            "اطبع س[٠]",
            "اطبع س[١]",
            "اطبع س[٢]",
            "ادفع_مجلد()",
            "اطبع المجلد_الحالي()",
            "أخرج_مجلد()",
            "اطبع المجلد_الحالي()",
        ],
    );
}

#[test]
fn c_parity_nav56_cdpath_spell_var_tilde() {
    // اجتماع 56 §71: CDPATH وتصحيح الإملاء واسم
    // المتغير كمجلد و`~+N` — المفسر مقابل الثنائي
    // الأصلي حرفيا.
    assert_native_src(
        "nav56-بحث-وتصحيح-ومتغير",
        &[
            "أنشئ_مجلد(\"مشاريع/هدف\")",
            "مسار_بحث_المجلدات([\"مشاريع\"])",
            "انتقل(\"هدف\")",
            "اطبع المجلد_الحالي()",
            "انتقل(\"~\")",
            "انتقل(\"مشاري\")",
            "اطبع المجلد_الحالي()",
            "انتقل(\"~\")",
            "أنشئ_مجلد(\"أ\")",
            "وجهتي = \"أ\"",
            "انتقل(\"وجهتي\")",
            "اطبع المجلد_الحالي()",
            "ادفع_مجلد(\"~/مشاريع\")",
            "انتقل(\"~+1\")",
            "اطبع المجلد_الحالي()",
        ],
    );
}

#[test]
fn c_parity_nav56_cdpath_dotdot() {
    // انحدار (إصلاح انهيار `..` المعجمي): مع CDPATH
    // مضبوطا، `..` تذهب إلى الأب — وPOSIX: `../..`
    // لا تُستشار فيها `CDPATH` — المفسر مقابل
    // الثنائي الأصلي حرفيا.
    assert_native_src(
        "nav56-مسار-بحث-نقطتان",
        &[
            "أنشئ_مجلد(\"مشاريع/هدف\")",
            "أنشئ_مجلد(\"أ\")",
            "مسار_بحث_المجلدات([\"مشاريع\"])",
            "انتقل(\"هدف\")",
            "انتقل(\"..\")",
            "اطبع المجلد_الحالي()",
            "مسار_بحث_المجلدات([\"~/أ\"])",
            "انتقل(\"~/مشاريع/هدف\")",
            "انتقل(\"../..\")",
            "اطبع المجلد_الحالي()",
        ],
    );
}

#[test]
fn c_parity_nav56_flag_error() {
    // اجتماع 56 §71: العلم الغريب — رسالة الخطأ
    // ورمز الخروج حرفيا بين المفسر والأصلي.
    assert_native_src(
        "nav56-علم-غريب",
        &["انتقل(\"أ\"، \"غريب\")"],
    );
}

#[test]
fn c_parity_nav56_physical() {
    // اجتماع 56 §71: المنطقي مقابل الفيزيائي —
    // المفسر مقابل الثنائي الأصلي حرفيا (مسار المجلد
    // المؤقت يُطبَّع إلى `<مجلد>` في الطرفين).
    assert_native_src(
        "nav56-فيزيائي",
        &[
            "أنشئ_مجلد(\"حقيقي/فرعي\")",
            "أنشئ_رابط(\"حقيقي\"، \"رابط\")",
            "انتقل(\"رابط/فرعي\")",
            "اطبع المجلد_الحالي()",
            "اطبع المجلد_الحالي(\"فيزيائي\")",
            "انتقل(\"..\"، \"فيزيائي\")",
            "اطبع المجلد_الحالي()",
        ],
    );
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


// ----- الدفعة 8 — الطبقة 1 (اجتماع 90 §105) -----

#[test]
#[cfg(unix)]
fn c_parity_batch8_pure() {
    // الحالات النقية أصليا (المفسر مقابل الثنائي
    // الأصلي)؛ وهي في INTEGRATION_CASES وفي C_SKIP
    // (سابقة المولدات).
    for (name, lines) in pc::INTEGRATION_CASES {
        if name.starts_with("شبكة8-") {
            assert_native_src(name, lines);
        }
    }
}

#[cfg(unix)]
fn b8_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// gzip بكتلة مخزنة (بلا ضغط فعلي) — حتمي.
#[cfg(unix)]
fn b8_gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0x03];
    out.push(0x01);
    let len = data.len() as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&b8_crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

#[cfg(unix)]
struct B8Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[cfg(unix)]
fn b8_read_req(stream: &mut std::net::TcpStream) -> Option<B8Req> {
    use std::io::Read;
    let mut buf: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => buf.push(byte[0]),
            Err(_) => return None,
        }
        if buf.len() > 65536 {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    for ln in lines {
        if let Some((n, v)) = ln.split_once(':') {
            headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
    };
    let mut body = Vec::new();
    if let Some(te) = get("transfer-encoding") {
        if te.to_ascii_lowercase().contains("chunked") {
            loop {
                let mut line: Vec<u8> = Vec::new();
                let mut b = [0u8; 1];
                while !line.ends_with(b"\r\n") {
                    match stream.read(&mut b) {
                        Ok(0) => break,
                        Ok(_) => line.push(b[0]),
                        Err(_) => break,
                    }
                }
                let size_str = String::from_utf8_lossy(&line).trim().to_string();
                let size = usize::from_str_radix(size_str.trim(), 16).unwrap_or(0);
                if size == 0 {
                    let mut tail = [0u8; 2];
                    let _ = stream.read_exact(&mut tail);
                    break;
                }
                let mut chunk = vec![0u8; size];
                stream.read_exact(&mut chunk).ok()?;
                body.extend_from_slice(&chunk);
                let mut crlf = [0u8; 2];
                stream.read_exact(&mut crlf).ok()?;
            }
        }
    } else if let Some(cl) = get("content-length") {
        let n: usize = cl.parse().unwrap_or(0);
        let mut b = vec![0u8; n];
        stream.read_exact(&mut b).ok()?;
        body = b;
    }
    Some(B8Req {
        method,
        target,
        headers,
        body,
    })
}

#[cfg(unix)]
fn b8_respond(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) {
    use std::io::Write;
    let mut out = format!("HTTP/1.1 {} {}\r\n", status, reason);
    for (n, v) in headers {
        out.push_str(&format!("{}: {}\r\n", n, v));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    let _ = stream.write_all(&bytes);
    let _ = stream.flush();
}

#[cfg(unix)]
fn b8_route(req: &B8Req) -> (u16, &'static str, Vec<(String, String)>, Vec<u8>) {
    let get = |n: &str| {
        req.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    };
    let ct = ("Content-Type".to_string(), "text/plain; charset=utf-8".to_string());
    let path = req.target.split('?').next().unwrap_or("");
    match (req.method.as_str(), path) {
        ("GET", "/hello") => (200, "OK", vec![ct], b"hello world".to_vec()),
        ("GET", "/redir") | ("POST", "/redir-post") => (
            302,
            "Found",
            vec![ct, ("Location".to_string(), "/hello".to_string())],
            Vec::new(),
        ),
        ("GET", "/gzip") => (
            200,
            "OK",
            vec![
                ct,
                ("Content-Encoding".to_string(), "gzip".to_string()),
            ],
            b8_gzip_stored(b"compressed text here"),
        ),
        ("GET", "/notfound") => (404, "Not Found", vec![ct], b"missing".to_vec()),
        ("GET", "/range") => {
            let full = b"0123456789ABCDEFGHIJ";
            if let Some(r) = get("range") {
                let spec = r.strip_prefix("bytes=").unwrap_or("");
                let mut it = spec.split('-');
                let a: usize = it.next().unwrap_or("").parse().unwrap_or(0);
                let b: usize = it.next().unwrap_or("").parse().unwrap_or(full.len() - 1);
                let b = b.min(full.len() - 1);
                let slice = &full[a..=b];
                (
                    206,
                    "Partial Content",
                    vec![
                        ct,
                        (
                            "Content-Range".to_string(),
                            format!("bytes {}-{}/{}", a, b, full.len()),
                        ),
                    ],
                    slice.to_vec(),
                )
            } else {
                (200, "OK", vec![ct], full.to_vec())
            }
        }
        ("GET", "/cookie") => (
            200,
            "OK",
            vec![ct],
            get("cookie").unwrap_or("بلا").as_bytes().to_vec(),
        ),
        ("GET", "/basic") => {
            if get("authorization")
                .map(|v| v.starts_with("Basic "))
                .unwrap_or(false)
            {
                (200, "OK", vec![ct], b"basic ok".to_vec())
            } else {
                (
                    401,
                    "Unauthorized",
                    vec![
                        ct,
                        (
                            "WWW-Authenticate".to_string(),
                            "Basic realm=\"test\"".to_string(),
                        ),
                    ],
                    b"need basic".to_vec(),
                )
            }
        }
        ("GET", "/bearer") => {
            if get("authorization")
                .map(|v| v == "Bearer رمز-اختبار")
                .unwrap_or(false)
            {
                (200, "OK", vec![ct], b"bearer ok".to_vec())
            } else {
                (401, "Unauthorized", vec![ct], b"need bearer".to_vec())
            }
        }
        ("GET", "/digest") => {
            if get("authorization")
                .map(|v| v.starts_with("Digest "))
                .unwrap_or(false)
            {
                (200, "OK", vec![ct], b"secret data".to_vec())
            } else {
                (
                    401,
                    "Unauthorized",
                    vec![
                        ct,
                        (
                            "WWW-Authenticate".to_string(),
                            "Digest realm=\"test\", nonce=\"abc123\", qop=\"auth\", algorithm=MD5"
                                .to_string(),
                        ),
                    ],
                    b"need digest".to_vec(),
                )
            }
        }
        ("GET", "/sigv4") => {
            let ok = get("authorization")
                .map(|v| v.starts_with("AWS4-HMAC-SHA256 Credential=مفتاح-اختبار/"))
                .unwrap_or(false)
                && get("x-amz-date").is_some()
                && get("x-amz-content-sha256").is_some();
            if ok {
                (200, "OK", vec![ct], b"signed ok".to_vec())
            } else {
                (401, "Unauthorized", vec![ct], b"need sigv4".to_vec())
            }
        }
        ("GET", "/httpsig") => {
            let ok = get("authorization")
                .map(|v| v.starts_with("Signature keyId=\"معرف-اختبار\""))
                .unwrap_or(false)
                && get("date").is_some();
            if ok {
                (200, "OK", vec![ct], b"signed ok".to_vec())
            } else {
                (401, "Unauthorized", vec![ct], b"need httpsig".to_vec())
            }
        }
        ("GET", "/headers") => {
            let mut hs: Vec<(String, String)> = req.headers.clone();
            hs.sort_by(|a, b| a.0.cmp(&b.0));
            let body = hs
                .iter()
                .map(|(n, v)| format!("{}: {}", n, v))
                .collect::<Vec<_>>()
                .join("\n");
            (200, "OK", vec![ct], body.into_bytes())
        }
        ("POST", "/echo-chunked") => (200, "OK", vec![ct], req.body.clone()),
        ("HEAD", "/echo") => (200, "OK", vec![ct], Vec::new()),
        (_, "/echo") => {
            let mut b = format!("{} {}", req.method, req.target).into_bytes();
            if !req.body.is_empty() {
                b.push(b':');
                b.extend_from_slice(&req.body);
            }
            (200, "OK", vec![ct], b)
        }
        ("GET", "/site") => (
            200,
            "OK",
            vec![ct],
            b"<html><body><a href=\"/site/a\">\xd8\xa3</a><a href=\"b\">\xd8\xa8</a></body></html>".to_vec(),
        ),
        ("GET", "/site/a") => (200, "OK", vec![ct], "صفحة أ".as_bytes().to_vec()),
        ("GET", "/b") => (200, "OK", vec![ct], "صفحة ب".as_bytes().to_vec()),
        ("GET", "/dl-name") => (
            200,
            "OK",
            vec![
                ct,
                (
                    "Content-Disposition".to_string(),
                    "attachment; filename=\"اسم-ملف.txt\"".to_string(),
                ),
            ],
            "محتوى الملف".as_bytes().to_vec(),
        ),
        _ => (404, "Not Found", vec![ct], b"missing".to_vec()),
    }
}

#[cfg(unix)]
fn b8_serve_forever(listener: std::net::TcpListener) {
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            if let Some(req) = b8_read_req(&mut s) {
                let (st, reason, hdrs, body) = b8_route(&req);
                let hrefs: Vec<(&str, &str)> = hdrs
                    .iter()
                    .map(|(n, v)| (n.as_str(), v.as_str()))
                    .collect();
                b8_respond(&mut s, st, reason, &hrefs, &body);
            }
        }
    }
}

#[cfg(unix)]
fn b8_serve_proxy_forever(listener: std::net::TcpListener) {
    use std::io::{Read, Write};
    for stream in listener.incoming() {
        if let Ok(mut client) = stream {
            let req = match b8_read_req(&mut client) {
                Some(r) => r,
                None => continue,
            };
            let rest = req.target.strip_prefix("http://").unwrap_or(&req.target);
            let (hostport, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, "/"),
            };
            let (host, port) = match hostport.rfind(':') {
                Some(i) => (&hostport[..i], hostport[i + 1..].parse().unwrap_or(80)),
                None => (hostport, 80),
            };
            if let Ok(mut upstream) = std::net::TcpStream::connect((host, port)) {
                let mut out = format!("{} {} HTTP/1.1\r\n", req.method, path);
                for (n, v) in &req.headers {
                    if n == "proxy-authorization" {
                        continue;
                    }
                    out.push_str(&format!("{}: {}\r\n", n, v));
                }
                out.push_str("\r\n");
                let mut bytes = out.into_bytes();
                bytes.extend_from_slice(&req.body);
                if upstream.write_all(&bytes).is_ok() {
                    let mut resp = Vec::new();
                    let _ = upstream.read_to_end(&mut resp);
                    let _ = client.write_all(&resp);
                }
            }
            let _ = client.flush();
        }
    }
}

#[cfg(unix)]
fn b8_serve_socks5_forever(listener: std::net::TcpListener) {
    use std::io::{Read, Write};
    for stream in listener.incoming() {
        if let Ok(mut client) = stream {
            std::thread::spawn(move || {
                let _ = client.set_read_timeout(Some(std::time::Duration::from_secs(10)));
                let mut hdr = [0u8; 2];
                if client.read_exact(&mut hdr).is_err() {
                    return;
                }
                let n = hdr[1] as usize;
                let mut methods = vec![0u8; n];
                if client.read_exact(&mut methods).is_err() {
                    return;
                }
                if client.write_all(&[5, 0]).is_err() {
                    return;
                }
                let mut req4 = [0u8; 4];
                if client.read_exact(&mut req4).is_err() {
                    return;
                }
                if req4[0] != 5 || req4[1] != 1 {
                    return;
                }
                let (host, port) = match req4[3] {
                    1 => {
                        let mut ip = [0u8; 4];
                        if client.read_exact(&mut ip).is_err() {
                            return;
                        }
                        let mut pb = [0u8; 2];
                        if client.read_exact(&mut pb).is_err() {
                            return;
                        }
                        (
                            format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]),
                            u16::from_be_bytes(pb),
                        )
                    }
                    3 => {
                        let mut l = [0u8; 1];
                        if client.read_exact(&mut l).is_err() {
                            return;
                        }
                        let mut name = vec![0u8; l[0] as usize];
                        if client.read_exact(&mut name).is_err() {
                            return;
                        }
                        let mut pb = [0u8; 2];
                        if client.read_exact(&mut pb).is_err() {
                            return;
                        }
                        (
                            String::from_utf8_lossy(&name).into_owned(),
                            u16::from_be_bytes(pb),
                        )
                    }
                    _ => return,
                };
                match std::net::TcpStream::connect((host.as_str(), port)) {
                    Ok(mut upstream) => {
                        if client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).is_err() {
                            return;
                        }
                        let mut up2 = match upstream.try_clone() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let mut c2 = match client.try_clone() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let t = std::thread::spawn(move || {
                            let mut buf = [0u8; 8192];
                            loop {
                                match up2.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if c2.write_all(&buf[..n]).is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                            let _ = c2.shutdown(std::net::Shutdown::Write);
                        });
                        let mut buf = [0u8; 8192];
                        loop {
                            match client.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    if upstream.write_all(&buf[..n]).is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        let _ = upstream.shutdown(std::net::Shutdown::Write);
                        let _ = t.join();
                    }
                    Err(_) => {
                        let _ = client.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]);
                    }
                }
            });
        }
    }
}

/// شغّل جالبا بالمفسر وبالثنائي الأصلي (بأعلام
/// الشبكة) وقارنهما حرفيا — في مجلدين منفصلين
/// (فالحفظ يكتب ملفات).
#[cfg(unix)]
fn b8_fetch_pair(name: &str, src: &str) {
    let dir_i = pc::fresh_dir();
    let dir_c = pc::fresh_dir();
    fs::write(dir_i.join("prog.نيفا"), src).unwrap();
    fs::write(dir_c.join("prog.نيفا"), src).unwrap();
    let out_i = Command::new(pc::bin())
        .args(["--مفسر", "--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"])
        .arg(dir_i.join("prog.نيفا"))
        .current_dir(&dir_i)
        .output()
        .expect("run جالب مفسر");
    let build = Command::new(pc::bin())
        .args(["ابن", "--أصلي", "prog.نيفا"])
        .current_dir(&dir_c)
        .output()
        .expect("run نيفا ابن --أصلي");
    assert!(
        build.status.success(),
        "فشل بناء جالب أصلي {}: {}",
        name,
        String::from_utf8_lossy(&build.stderr)
    );
    let out_c = Command::new(dir_c.join("prog"))
        .args(["--اسمح-بالشبكة", "--مضيف-محلي", "127.0.0.1"])
        .current_dir(&dir_c)
        .output()
        .expect("run جالب أصلي");
    let oi = String::from_utf8_lossy(&out_i.stdout).to_string();
    let ei = pc::normalize_temp_dir(String::from_utf8_lossy(&out_i.stderr).to_string(), &dir_i);
    let oc = String::from_utf8_lossy(&out_c.stdout).to_string();
    let ec = pc::normalize_temp_dir(String::from_utf8_lossy(&out_c.stderr).to_string(), &dir_c);
    assert_eq!(
        (
            oi.clone(),
            ei.clone(),
            out_i.status.code().unwrap_or(-1)
        ),
        (
            oc.clone(),
            ec.clone(),
            out_c.status.code().unwrap_or(-1)
        ),
        "اختلاف تكافؤ أصلي في «{}»:\n--- المفسر:\nمخرج:\n{}\nخطأ:\n{}\n--- الأصلي:\nمخرج:\n{}\nخطأ:\n{}",
        name, oi, ei, oc, ec
    );
    let _ = fs::remove_dir_all(&dir_i);
    let _ = fs::remove_dir_all(&dir_c);
}

#[test]
#[cfg(unix)]
fn c_parity_batch8_http() {
    // الدفعة 8 — الطبقة 1 (اجتماع 90 §105): جلب
    // حي من خادم محلي (المفسر مقابل الأصلي) — على
    // نمط c_parity_server_http.
    for port in ["18109", "18110", "18111"] {
        let _ = Command::new("fuser")
            .args(["-k", &format!("{}/tcp", port)])
            .output();
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let http = std::net::TcpListener::bind("127.0.0.1:18109").expect("bind http");
    let proxy = std::net::TcpListener::bind("127.0.0.1:18110").expect("bind proxy");
    let socks = std::net::TcpListener::bind("127.0.0.1:18111").expect("bind socks5");
    std::thread::spawn(move || b8_serve_forever(http));
    std::thread::spawn(move || b8_serve_proxy_forever(proxy));
    std::thread::spawn(move || b8_serve_socks5_forever(socks));
    std::thread::sleep(std::time::Duration::from_millis(200));

    b8_fetch_pair(
        "شبكة8-حي-أساسي",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/hello")"#,
            "رد = شبكة.نفذ(طلب)",
            "اطبع رد",
            r#"طلب2 = شبكة.طلب_جديد("http://127.0.0.1:18109/redir")"#,
            "رد2 = شبكة.نفذ(طلب2)",
            r#"اطبع رد2["رمز"]"#,
            r#"اطبع رد2["جسم"]"#,
            r#"طلب3 = شبكة.طلب_جديد("http://127.0.0.1:18109/gzip")"#,
            "رد3 = شبكة.نفذ(طلب3)",
            r#"اطبع رد3["جسم"]"#,
            r#"طلب4 = شبكة.طلب_جديد("http://127.0.0.1:18109/notfound")"#,
            "رد4 = شبكة.نفذ(طلب4)",
            r#"اطبع رد4["رمز"]"#,
            r#"اطبع رد4["جسم"]"#,
            r#"اطبع رد4["بايتات"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-طرق",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo"، "طريقة": "PUT"، "جسم": "جسم-وضع"، "نوع_المحتوى": "text/plain"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo"، "طريقة": "DELETE"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo"، "طريقة": "تتبع"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo"، "طريقة": "POST"، "جسم": "مرحبا"، "نوع_المحتوى": "text/plain"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo"، "طريقة": "HEAD"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
            r#"اطبع رد["بايتات"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-متقطع-مدى-كوكيز-ترويسات",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = {"رابط": "http://127.0.0.1:18109/echo-chunked"، "طريقة": "POST"، "جسم": "جسم-متقطع-طويل-للفحص"، "نوع_المحتوى": "text/plain"}"#,
            "طلب = شبكة.بإرسال_متقطع(طلب، صحيح)",
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/range")"#,
            "طلب = شبكة.بمدى(طلب، ٥، ٩)",
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/cookie")"#,
            r#"طلب = شبكة.مع_كوكيز(طلب، "جلسة"، "قيمة-١")"#,
            r#"طلب = شبكة.مع_كوكيز(طلب، "لغة"، "عربي")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/headers")"#,
            r#"طلب = شبكة.باسم_متصفح(طلب، "وكيل-اختبار")"#,
            r#"طلب = شبكة.بإحالة(طلب، "http://مرجع.example/")"#,
            r#"طلب = شبكة.باللغة(طلب، "ar")"#,
            r#"طلب = شبكة.مع_رأس(طلب، "X-مخصص"، "نعم")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-مصادقة",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/basic")"#,
            r#"طلب = شبكة.باسم_وكلمة_سر(طلب، "مستخدم"، "سر")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/bearer")"#,
            r#"طلب = شبكة.برمز(طلب، "رمز-اختبار")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/digest")"#,
            r#"طلب = شبكة.بمصادقة_هضمية(طلب، "مستخدم"، "سر")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-توقيع",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/sigv4")"#,
            r#"طلب = شبكة.بتوقيع_أمازون(طلب، "مفتاح-اختبار"، "سر-اختبار")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/httpsig")"#,
            r#"طلب = شبكة.بتوقيع_طلب(طلب، "معرف-اختبار"، "مفتاح-اختبار")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-حفظ-تتبع",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = {"رابط": "http://127.0.0.1:18109/hello"، "حفظ_في": "محفوظ.txt"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
            r#"اطبع رد["بايتات"]"#,
            r#"اطبع اقرأ_ملف("محفوظ.txt")"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/hello")"#,
            r#"طلب = شبكة.بتتبع(طلب، "تتبع.txt")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع اقرأ_ملف("تتبع.txt")"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-تحقق-حالة",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/notfound")"#,
            "طلب = شبكة.بتحقق_الحالة(طلب، صحيح)",
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-وكيل",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/hello")"#,
            r#"طلب = شبكة.عبر_وكيل(طلب، "127.0.0.1:18110")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/hello")"#,
            r#"طلب = شبكة.عبر_وكيل_socks5(طلب، "127.0.0.1:18111")"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/hello")"#,
            r#"طلب = شبكة.عبر_وكيل(طلب، "127.0.0.1:18110")"#,
            r#"طلب = شبكة.بلا_وكيل_ل(طلب، ["127.0.0.1"])"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-توجيه",
        &pc::src(&[
            "استورد شبكة",
            r#"طلب = شبكة.طلب_جديد("http://127.0.0.1:18109/redir")"#,
            "طلب = شبكة.باتباع_التوجيه(طلب، خطأ)",
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
            r#"اطبع رد["بايتات"]"#,
            r#"طلب = {"رابط": "http://127.0.0.1:18109/redir-post"، "طريقة": "POST"، "جسم": "بيانات"، "نوع_المحتوى": "text/plain"}"#,
            "رد = شبكة.نفذ(طلب)",
            r#"اطبع رد["رمز"]"#,
            r#"اطبع رد["جسم"]"#,
        ]),
    );
}


#[cfg(unix)]
fn b8_ftp_read_line(stream: &mut std::net::TcpStream) -> Option<String> {
    use std::io::Read;
    let mut out: Vec<u8> = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match stream.read(&mut b) {
            Ok(0) => return None,
            Ok(_) => {
                if b[0] == b'\n' {
                    break;
                }
                out.push(b[0]);
            }
            Err(_) => return None,
        }
    }
    if out.last() == Some(&b'\r') {
        out.pop();
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(unix)]
fn b8_serve_ftp_forever(listener: std::net::TcpListener) {
    use std::io::Write;
    let files: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, bool)>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::from([(
        "/hello.txt".to_string(),
        ("محتوى ftp".as_bytes().to_vec(), false),
    )])));
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let files = files.clone();
            std::thread::spawn(move || {
                let _ = s.write_all("220 بسيط جاهز\r\n".as_bytes());
                let mut data_listener: Option<std::net::TcpListener> = None;
                loop {
                    let line = match b8_ftp_read_line(&mut s) {
                        Some(l) => l,
                        None => break,
                    };
                    let mut parts = line.splitn(2, ' ');
                    let cmd = parts.next().unwrap_or("").to_uppercase();
                    let arg = parts.next().unwrap_or("").to_string();
                    match cmd.as_str() {
                        "USER" | "PASS" => {
                            let _ = s.write_all("230 دخول مقبول\r\n".as_bytes());
                        }
                        "TYPE" => {
                            let _ = s.write_all("200 نوع ثنائي\r\n".as_bytes());
                        }
                        "PASV" => match std::net::TcpListener::bind("127.0.0.1:0") {
                            Ok(l) => {
                                let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
                                data_listener = Some(l);
                                let _ = s.write_all(
                                    format!(
                                        "227 دخول سلبي (127,0,0,1,{},{})\r\n",
                                        port / 256,
                                        port % 256
                                    )
                                    .as_bytes(),
                                );
                            }
                            Err(_) => {
                                let _ = s.write_all("425 لا منفذ\r\n".as_bytes());
                            }
                        },
                        "RETR" => {
                            let entry = files.lock().unwrap().get(&arg).cloned();
                            match entry {
                                Some((data, once)) => {
                                    if once {
                                        files.lock().unwrap().remove(&arg);
                                    }
                                    let _ = s.write_all("150 فتح\r\n".as_bytes());
                                    if let Some(l) = data_listener.take() {
                                        if let Ok((mut d, _)) = l.accept() {
                                            let _ = d.write_all(&data);
                                        }
                                    }
                                    let _ = s.write_all("226 اكتمل\r\n".as_bytes());
                                }
                                None => {
                                    let _ = s.write_all("550 غير موجود\r\n".as_bytes());
                                }
                            }
                        }
                        "STOR" => {
                            let _ = s.write_all("150 فتح\r\n".as_bytes());
                            let mut data: Vec<u8> = Vec::new();
                            if let Some(l) = data_listener.take() {
                                if let Ok((mut d, _)) = l.accept() {
                                    use std::io::Read;
                                    let _ = d.read_to_end(&mut data);
                                }
                            }
                            files.lock().unwrap().insert(arg, (data, true));
                            let _ = s.write_all("226 اكتمل\r\n".as_bytes());
                        }
                        "QUIT" => {
                            let _ = s.write_all("221 وداعا\r\n".as_bytes());
                            break;
                        }
                        _ => {
                            let _ = s.write_all("500 غير معروف\r\n".as_bytes());
                        }
                    }
                }
            });
        }
    }
}

#[cfg(unix)]
fn b8_mqtt_read_varint(stream: &mut std::net::TcpStream) -> Option<usize> {
    use std::io::Read;
    let mut len: usize = 0;
    let mut mult: usize = 1;
    loop {
        let mut b = [0u8; 1];
        if stream.read_exact(&mut b).is_err() {
            return None;
        }
        len += ((b[0] & 0x7F) as usize) * mult;
        if b[0] & 0x80 == 0 {
            break;
        }
        mult *= 128;
    }
    Some(len)
}

#[cfg(unix)]
fn b8_mqtt_read_packet(stream: &mut std::net::TcpStream) -> Option<(u8, Vec<u8>)> {
    use std::io::Read;
    let mut h = [0u8; 1];
    if stream.read_exact(&mut h).is_err() {
        return None;
    }
    let len = b8_mqtt_read_varint(stream)?;
    let mut body = vec![0u8; len];
    if stream.read_exact(&mut body).is_err() {
        return None;
    }
    Some((h[0], body))
}

#[cfg(unix)]
fn b8_mqtt_write_packet(stream: &mut std::net::TcpStream, cmd: u8, body: &[u8]) {
    use std::io::Write;
    let mut out: Vec<u8> = vec![cmd];
    let mut n = body.len();
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    let _ = stream.write_all(&out);
}

#[cfg(unix)]
fn b8_serve_mqtt_forever(listener: std::net::TcpListener) {
    let store: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let store = store.clone();
            std::thread::spawn(move || {
                let Some(_) = b8_mqtt_read_packet(&mut s) else {
                    return;
                };
                b8_mqtt_write_packet(&mut s, 0x20, &[0x00, 0x00]);
                loop {
                    let (cmd, body) = match b8_mqtt_read_packet(&mut s) {
                        Some(p) => p,
                        None => return,
                    };
                    match cmd {
                        0x82 => {
                            if body.len() >= 4 {
                                let tlen =
                                    ((body[2] as usize) << 8) | body[3] as usize;
                                if body.len() >= 4 + tlen {
                                    let topic = String::from_utf8_lossy(
                                        &body[4..4 + tlen],
                                    )
                                    .into_owned();
                                    b8_mqtt_write_packet(
                                        &mut s,
                                        0x90,
                                        &[body[0], body[1], 0x00],
                                    );
                                    let mut held = store.lock().unwrap();
                                    let mut i = 0;
                                    while i < held.len() {
                                        if held[i].0 == topic {
                                            let (_, payload) = held.remove(i);
                                            let mut pb: Vec<u8> = Vec::new();
                                            pb.push(((topic.len() >> 8) & 0xFF) as u8);
                                            pb.push((topic.len() & 0xFF) as u8);
                                            pb.extend_from_slice(topic.as_bytes());
                                            pb.extend_from_slice(&payload);
                                            b8_mqtt_write_packet(&mut s, 0x30, &pb);
                                        } else {
                                            i += 1;
                                        }
                                    }
                                }
                            }
                        }
                        0x30 => {
                            if body.len() >= 2 {
                                let tlen =
                                    ((body[0] as usize) << 8) | body[1] as usize;
                                if body.len() >= 2 + tlen {
                                    let topic = String::from_utf8_lossy(
                                        &body[2..2 + tlen],
                                    )
                                    .into_owned();
                                    let payload = body[2 + tlen..].to_vec();
                                    store.lock().unwrap().push((topic, payload));
                                }
                            }
                        }
                        0xC0 => {
                            b8_mqtt_write_packet(&mut s, 0xD0, &[]);
                        }
                        0xE0 => return,
                        _ => {}
                    }
                }
            });
        }
    }
}


#[test]
#[cfg(unix)]
fn c_parity_batch8_live3() {
    // الدفعة 8 — الطبقة 3 (اجتماع 90 §105): FTP/MQTT حي
    // من خادمين محليين (المفسر مقابل الأصلي).
    let _ = Command::new("fuser")
        .args(["-k", "18113/tcp"])
        .output();
    let _ = Command::new("fuser")
        .args(["-k", "18114/tcp"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let ftp = std::net::TcpListener::bind("127.0.0.1:18113").expect("bind live3 ftp");
    let mqtt =
        std::net::TcpListener::bind("127.0.0.1:18114").expect("bind live3 mqtt");
    std::thread::spawn(move || b8_serve_ftp_forever(ftp));
    std::thread::spawn(move || b8_serve_mqtt_forever(mqtt));
    std::thread::sleep(std::time::Duration::from_millis(200));

    b8_fetch_pair(
        "شبكة8-حي-ftp",
        &pc::src(&[
            "استورد شبكة",
            r#"نص = شبكة.اجلب_ftp("ftp://127.0.0.1:18113/hello.txt")"#,
            "اطبع نص",
            r#"عدد = شبكة.نزل_ftp("ftp://127.0.0.1:18113/hello.txt"، "نزل.txt")"#,
            "اطبع عدد",
            r#"اطبع اقرأ_ملف("نزل.txt")"#,
            r#"اكتب_ملف("رفع.txt"، "محتوى الرفع")"#,
            r#"عدد = شبكة.ارفع_ftp("ftp://127.0.0.1:18113/رفع-ftp.txt"، "رفع.txt")"#,
            "اطبع عدد",
            r#"نص = شبكة.اجلب_ftp("ftp://127.0.0.1:18113/رفع-ftp.txt")"#,
            "اطبع نص",
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-mqtt",
        &pc::src(&[
            "استورد شبكة",
            r#"نتيجة = شبكة.انشر_mqtt("mqtt://127.0.0.1:18114"، "اختبار/موضوع"، "رسالة اختبار")"#,
            "اطبع نتيجة",
            r#"رسائل = شبكة.اشترك_mqtt("mqtt://127.0.0.1:18114"، "اختبار/موضوع"، ٣)"#,
            "اطبع رسائل",
            r#"رسائل = شبكة.اشترك_mqtt("mqtt://127.0.0.1:18114"، "موضوع/آخر"، ٢)"#,
            "اطبع رسائل",
        ]),
    );
}


#[test]
#[cfg(unix)]
fn c_parity_batch8_live5() {
    // الدفعة 8 — الطبقة 5ب (اجتماع 90 §105): فحص واتصال
    // حي من خادم HTTP محلي (المفسر مقابل الأصلي).
    let _ = Command::new("fuser")
        .args(["-k", "18115/tcp"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let http = std::net::TcpListener::bind("127.0.0.1:18115").expect("bind live5 http");
    std::thread::spawn(move || b8_serve_forever(http));
    b8_fetch_pair(
        "شبكة8-حي-نبض",
        r#"استورد شبكة
اطبع شبكة.انبض("127.0.0.1"، ١٨١١٥)
اطبع شبكة.انبض("127.0.0.1"، ١)
م = شبكة.انبض_مستمر("127.0.0.1"، ٢، ٠)
اطبع م
اطبع شبكة.زمن_الذهاب_والعودة("127.0.0.1"، ١٨١١٥) > ٠
اطبع شبكة.زمن_الذهاب_والعودة("127.0.0.1"، ١)
اطبع طول(شبكة.حل_الاسم("localhost"))"#,
    );
    b8_fetch_pair(
        "شبكة8-حي-فحص",
        r#"استورد شبكة
اطبع شبكة.افحص_المنفذ("127.0.0.1"، ١٨١١٥)
اطبع شبكة.افحص_المنفذ("127.0.0.1"، ١)
ف = شبكة.افحص_المنافذ("127.0.0.1"، [١٨١١٥، ١])
اطبع ف["18115"]
اطبع ف["1"]
اطبع شبكة.افحص_نطاق("127.0.0.1"، ١٨١١٥، ١٨١١٥)
اطبع طول(شبكة.افحص_سريع("127.0.0.1"))
اطبع شبكة.افحص_udp("127.0.0.1"، ١٨١١٥)
اطبع شبكة.انبض_icmp("127.0.0.1")"#,
    );
    b8_fetch_pair(
        "شبكة8-حي-تتبع",
        r#"استورد شبكة
ق = شبكة.تتبع_المسار("127.0.0.1"، ٢)
اطبع طول(ق)
أول = ق[٠]
اطبع أول["قفزة"]
اطبع أول["عنوان"]"#,
    );
    b8_fetch_pair(
        "شبكة8-حي-اتصال",
        r#"استورد شبكة
معرف = شبكة.اتصل_بمهلة("127.0.0.1"، ١٨١١٥، ٥)
اطبع معرف > ٠
شبكة.أرسل(معرف، "GET /hello HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
اطبع شبكة.استقبل(معرف)
شبكة.أغلق(معرف)"#,
    );
    b8_fetch_pair(
        "شبكة8-حي-معلومات",
        r#"استورد شبكة
اطبع طول(شبكة.عناويني())
ع = شبكة.عناويني()
أول = ع[٠]
اطبع أول["عنوان"]
ج = شبكة.معلومات_الجهاز("lo")
اطبع ج["وحدة"]
اطبع طول(شبكة.إحصاءات_الشبكة())
إ = شبكة.إحصاءات_الشبكة()
أول2 = إ[٠]
اطبع أول2["اسم"]"#,
    );
}

#[test]
#[cfg(unix)]
fn c_parity_batch8_live4() {
    // الدفعة 8 — الطبقة 4 (اجتماع 90 §105): جلسات +
    // نقية + توقيع — حي بلا خادم (المفسر مقابل الأصلي).
    // قيم التوقيع المعتمدة على الوقت لا تُطبع (التنفيذان
    // يفصل بينهما بناء)؛ الثابت منها فقط (هاش الجسم +
    // مفاتيح القاموس) + التحقق العميق في verify_sign4.py.
    b8_fetch_pair(
        "شبكة8-حي-جلسات",
        &pc::src(&[
            "استورد شبكة",
            r#"اكتب_ملف("كوكيز.txt"، "example.com\tTRUE\t/\tFALSE\t1893456000\tجلسة\t١٢٣\n")"#,
            r#"كوكيز = شبكة.اقرأ_كوكيز("كوكيز.txt")"#,
            r#"اطبع كوكيز["جلسة"]"#,
            r#"اكتب_ملف("دخول.txt"، "example.com مستخدم كلمة-سر\n")"#,
            r#"دخول = شبكة.اقرأ_بيانات_الدخول("دخول.txt")"#,
            r#"حساب = دخول["example.com"]"#,
            r#"اطبع حساب["مستخدم"]"#,
            r#"اطبع حساب["كلمة_سر"]"#,
            r#"اكتب_ملف("خيارات.txt"، "مهلة=٥\nمحاولات=٣\nصامت=صحيح\n")"#,
            r#"خيارات = شبكة.اقرأ_إعدادات_التنزيل("خيارات.txt")"#,
            r#"اطبع خيارات["مهلة"]"#,
            r#"اطبع خيارات["صامت"]"#,
            r#"شبكة.اكتب_إعدادات_التنزيل({"مهلة": ١٠، "صامت": خطأ}، "خيارات2.txt")"#,
            r#"اطبع اقرأ_ملف("خيارات2.txt")"#,
            r#"رد = {"رمز": ٢٠٠، "جسم": "نص"، "رؤوس": {"etag": "\"أ-١\""}}"#,
            r#"وسم = شبكة.احفظ_الوسم(رد، "وسم.txt")"#,
            "اطبع وسم",
            r#"اطبع شبكة.اقرأ_الوسم("وسم.txt")"#,
            r#"عدد = شبكة.احفظ_الرؤوس(رد، "رؤوس.txt")"#,
            "اطبع عدد",
            r#"اطبع اقرأ_ملف("رؤوس.txt")"#,
            r#"شبكة.اكتب_الفحص("الفحص ناجح"، "فحص.txt")"#,
            r#"اطبع اقرأ_ملف("فحص.txt")"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-نقية",
        &pc::src(&[
            "استورد شبكة",
            "استورد جسون",
            r#"اطبع شبكة.رمز_العنوان("أهلا/عالم؟")"#,
            r#"اطبع شبكة.أضف_معامل("https://x/y#جزء"، "بحث"، "أهلا")"#,
            r#"رد = {"رمز": ٤٠٤، "جسم": جسون.اكتب({"خطأ": "غير موجود"، "عدد": [١، ٢]})، "رؤوس": {"نوع": "تطبيق/json"}}"#,
            "اطبع شبكة.نسق_json(رد)",
            "اطبع شبكة.اعرض_ملونا(رد)",
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-توقيع",
        &pc::src(&[
            "استورد شبكة",
            "استورد جسون",
            r#"طلب = {"رابط": "http://127.0.0.1:18109/hello"، "طريقة": "POST"، "جسم": "جسم الاختبار"}"#,
            r#"ترويسات = شبكة.وقع_أمازون(طلب، "مفتاح-اختبار"، "سر-اختبار")"#,
            r#"اطبع ترويسات["x-amz-content-sha256"]"#,
            "اطبع جسون.مفاتيح(ترويسات)",
            r#"شبكة.وقع_الطلب(طلب، "معرف-اختبار"، "مفتاح-اختبار")"#,
            r#"اطبع "وقع الطلب كما يجب""#,
        ]),
    );
}

#[test]
#[cfg(unix)]
fn c_parity_batch8_live2() {
    // الدفعة 8 — الطبقة 2 (اجتماع 90 §105): أغلفة
    // فورية وتنزيل — حي من خادم محلي (المفسر مقابل
    // الأصلي)، على نمط c_parity_batch8_http.
    let _ = Command::new("fuser")
        .args(["-k", "18112/tcp"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let http = std::net::TcpListener::bind("127.0.0.1:18112").expect("bind live2");
    std::thread::spawn(move || b8_serve_forever(http));
    std::thread::sleep(std::time::Duration::from_millis(200));

    b8_fetch_pair(
        "شبكة8-حي-إرسال",
        &pc::src(&[
            "استورد شبكة",
            r#"رد = شبكة.أرسل_json("http://127.0.0.1:18112/echo"، {"اسم": "اختبار"، "عدد": ٣})"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.أرسل_نموذج("http://127.0.0.1:18112/echo"، {"اسم": "اختبار"، "عدد": ٣})"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.أرسل_خام("http://127.0.0.1:18112/echo"، "خام")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.أرسل_ثنائي("http://127.0.0.1:18112/echo"، [٧٢، ١٠٥])"#,
            r#"اطبع رد["جسم"]"#,
            r#"اكتب_ملف("ملف.txt"، "محتوى الملف")"#,
            r#"رد = شبكة.أرسل_من_ملف("http://127.0.0.1:18112/echo"، "ملف.txt")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.أرسل_من_المدخل("http://127.0.0.1:18112/echo")"#,
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-طرق2",
        &pc::src(&[
            "استورد شبكة",
            r#"رد = شبكة.استبدل("http://127.0.0.1:18112/echo"، "جديد")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.عدل("http://127.0.0.1:18112/echo"، "تعديل")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.احذف_مورد("http://127.0.0.1:18112/echo")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.افحص_رؤوس("http://127.0.0.1:18112/echo")"#,
            "اطبع رد",
            r#"رد = شبكة.اطلب_خيارات("http://127.0.0.1:18112/echo")"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.اطلب_مخصص("http://127.0.0.1:18112/echo"، "تتبع"، "جسم")"#,
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-رفع",
        &pc::src(&[
            "استورد شبكة",
            r#"اكتب_ملف("رفع.txt"، "بيانات الرفع")"#,
            r#"رد = شبكة.ارفع_ملف("http://127.0.0.1:18112/echo"، "رفع.txt"، {"وصف": "اختبار"})"#,
            r#"اطبع رد["جسم"]"#,
            r#"رد = شبكة.ارفع_جسم_ملف("http://127.0.0.1:18112/echo"، "رفع.txt")"#,
            r#"اطبع رد["جسم"]"#,
        ]),
    );
    b8_fetch_pair(
        "شبكة8-حي-تنزيل2",
        &pc::src(&[
            "استورد شبكة",
            r#"رد = شبكة.نزل_باسم_الخادم("http://127.0.0.1:18112/dl-name")"#,
            r#"اطبع اقرأ_ملف("اسم-ملف.txt")"#,
            r#"رد = شبكة.نزل_الكل(["http://127.0.0.1:18112/hello"، "http://127.0.0.1:18112/notfound"])"#,
            "اطبع رد",
            r#"اطبع اقرأ_ملف("hello")"#,
            r#"اطبع اقرأ_ملف("notfound")"#,
            r#"اكتب_ملف("جزئي.txt"، "01234")"#,
            r#"رد = شبكة.أكمل_التنزيل("http://127.0.0.1:18112/range"، "جزئي.txt")"#,
            r#"اطبع اقرأ_ملف("جزئي.txt")"#,
            r#"اكتب_ملف("قائمة.txt"، "http://127.0.0.1:18112/hello\nhttp://127.0.0.1:18112/gzip\n")"#,
            r#"رد = شبكة.نزل_من_قائمة("قائمة.txt")"#,
            "اطبع رد",
            r#"اطبع اقرأ_ملف("gzip")"#,
            r#"رد = شبكة.نزل_الموقع("http://127.0.0.1:18112/site")"#,
            "اطبع رد",
            r#"اطبع اقرأ_ملف("site")"#,
            r#"اطبع اقرأ_ملف("a")"#,
            r#"اطبع اقرأ_ملف("b")"#,
        ]),
    );
}

#[cfg(unix)]
fn b9_serve_banner_forever(listener: std::net::TcpListener) {
    use std::io::Write;
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let _ = s.write_all(b"SSH-2.0-OpenSSH_9.0\r\n");
            let _ = s.flush();
        }
    }
}

#[cfg(unix)]
fn b9_serve_connect_forever(listener: std::net::TcpListener) {
    use std::io::{Read, Write};
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
            let _ = s.flush();
        }
    }
}

#[test]
#[cfg(unix)]
fn c_parity_batch9_live() {
    // الدفعة 9 — شبكة 9: ربط UDP + بانر SSH +
    // وكيل CONNECT — حي من خوادم محلية (المفسر
    // مقابل الأصلي)، على نمط c_parity_batch8_live2.
    // ملاحظة بيئة: رمل هذا الجهاز يحظر إرسال UDP
    // (sendto → EPERM) فزوج UDP بلا إرسال — ربط
    // ومنفذ واتصال فقط.
    for port in ["18120", "18121", "18122"] {
        let _ = Command::new("fuser")
            .args(["-k", &format!("{}/tcp", port)])
            .output();
    }
    let _ = Command::new("fuser")
        .args(["-k", "18120/udp"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let banner = std::net::TcpListener::bind("127.0.0.1:18121").expect("bind banner9");
    let proxy = std::net::TcpListener::bind("127.0.0.1:18122").expect("bind proxy9");
    std::thread::spawn(move || b9_serve_banner_forever(banner));
    std::thread::spawn(move || b9_serve_connect_forever(proxy));
    std::thread::sleep(std::time::Duration::from_millis(200));

    b8_fetch_pair(
        "شبكة9-حي-udp",
        &pc::src(&[
            "استورد شبكة",
            r#"مستمع = شبكة.استمع_udp(١٨١٢٠)"#,
            r#"اطبع شبكة.المنفذ(مستمع)"#,
            r#"متصل = شبكة.اتصل_udp("127.0.0.1"، ١٨١٢١)"#,
            "اطبع متصل >= ٠",
        ]),
    );
    b8_fetch_pair(
        "شبكة9-حي-بانر",
        &pc::src(&[
            "استورد شبكة",
            r#"خدمة = شبكة.اكشف_الخدمة("127.0.0.1"، ١٨١٢١)"#,
            "اطبع خدمة",
        ]),
    );
    b8_fetch_pair(
        "شبكة9-حي-وكيل",
        &pc::src(&[
            "استورد شبكة",
            r#"مقبس = شبكة.اتصل_عبر_وكيل("127.0.0.1"، ١٨١٢٣، "127.0.0.1:18122")"#,
            "اطبع مقبس > ٠",
        ]),
    );
}

// الدفعة 1 — رموز الخروج (اجتماع 54 §69): تكافؤ
// المفسر والخلفية الأصلية C في رموز الخروج.
#[test]
fn c_parity_exit_code_chosen() {
    assert_native_same(
        "خروج برمز مختار",
        "اطبع \"قبل\"\nخروج(٧)\nاطبع \"بعد\"\n",
        &[],
    );
}

#[test]
fn c_parity_exit_code_after_caught_error() {
    assert_native_same(
        "رمز الخروج بعد خطأ ممسوك",
        "حاول:\n    س = غير_معرف\nماعدا:\n    اطبع رمز_الخروج()\n",
        &[],
    );
}

#[test]
fn c_parity_exit_skips_except_runs_finally() {
    assert_native_same(
        "خروج يتجاوز ماعدا ويشغل أخيرا",
        "حاول:\n    خروج(٥)\nماعدا:\n    اطبع \"ماعدا\"\nأخيرا:\n    اطبع \"أخيرا\"\nاطبع \"بعد\"\n",
        &[],
    );
}

#[test]
fn c_parity_program_name_nonempty() {
    // اسم_البرنامج() غير فارغ في C (لا مقارنة حرفية
    // مع المفسر لأن argv[0] يختلف بطبيعته).
    let (out, err, code) = run_native("اطبع اسم_البرنامج()\n", &[]);
    assert_eq!(code, 0, "stderr={}", err);
    assert!(!out.trim().is_empty(), "empty program name");
}

#[test]
fn c_parity_missing_module_is_127() {
    // الدفعة 1 (§69 ممتد): وحدة غائبة → 127 في المفسر
    // وفي البناء الأصلي (يحلها المولد وقت البناء).
    assert_native_same("وحدة مستوردة غائبة", "استورد لاوجود\n", &[]);
}

// الدفعة 3 — التشغيل (اجتماع 57 §72): وصف الأمر
// ومكان الأداة والاستبدال في الخلفية الأصلية.
#[test]
fn c_parity_type_command_layers() {
    assert_native_same(
        "وصف الأمر بطبقاته",
        "دالة عرفتها():\n    ارجع ١\nاطبع نوع_الامر(\"شغل\")\nاطبع نوع_الامر(\"ات\")\nاطبع نوع_الامر(\"اذا\")\nاطبع نوع_الامر(\"عرفتها\")\nاطبع نوع_الامر(\"انتقل\")\nاطبع نوع_الامر(\"ادفع مجلد\")\nاطبع نوع_الامر(\"أداة\")\nاطبع نوع_الامر(\"غائب\")\n",
        &[("أداة.نيفا", "اطبع \"أداة\"\n")],
    );
}

#[test]
fn c_parity_tool_location() {
    // المسار المطلق يختلف بين المجلدين المؤقتين،
    // فقارن بمقارنات منطقية لا بحرفية المسار.
    let (out, err, code) = run_native(
        "اطبع مكان_الاداة(\"أداة\")\nاطبع مكان_الاداة(\"غائب\")\n",
        &[("أداة.نيفا", "اطبع \"أداة\"\n")],
    );
    assert_eq!(code, 0, "stderr={}", err);
    assert!(out.contains("أداة.نيفا"), "got: {}", out);
    assert!(out.contains("لاشيء"), "got: {}", out);
    // الاسم المساري مرفوض كـ`شغل`.
    let (_, err, code) = run_native("اطبع مكان_الاداة(\"أ/ب\")\n", &[]);
    assert_eq!(code, 1, "stderr={}", err);
    assert!(err.contains("بلا مسار"), "got: {}", err);
}

#[test]
fn c_parity_exec_unsupported() {
    // لا مفسر داخل الثنائي الأصلي: خطأ صريح ورمز 1.
    let (_, err, code) = run_native(
        "استبدل_الجلسة(\"أداة\")\n",
        &[("أداة.نيفا", "اطبع \"أداة\"\n")],
    );
    assert_eq!(code, 1, "stderr={}", err);
    assert!(
        err.contains("غير مدعوم في البرنامج الأصلي"),
        "got: {}",
        err
    );
    // الاسم المساري يُرفض قبل رسالة عدم الدعم.
    let (_, err, code) = run_native("استبدل_الجلسة(\"أ/ب\")\n", &[]);
    assert_eq!(code, 1, "stderr={}", err);
    assert!(err.contains("بلا مسار"), "got: {}", err);
}

#[test]
fn c_parity_gen_rejected() {
    // ب6 — المولدات غير مدعومة في الثنائي الأصلي:
    // استدعاء دالة مولد يصل إلى `__ولد` المفكك فيرفض
    // بالرسالة القانونية حرفيا، خروج 1 (نمط J3-69).
    let (_, err, code) = run_native(
        "دالة عدّاد():\n    ولد ١\n    ولد ٢\nم = عدّاد()\nاطبع التالي(م)\n",
        &[],
    );
    assert_eq!(code, 1, "stderr={}", err);
    assert!(
        err.contains("المولدات غير مدعومة في الثنائي الأصلي — شغّل في المفسر أو آلة البايت كود"),
        "got: {}",
        err
    );
    // التعبير التوليدي يُرفض بالرسالة نفسها.
    let (_, err, code) = run_native(
        "مربعات = (س * س لاجل س في [١، ٢، ٣])\nاطبع التالي(مربعات)\n",
        &[],
    );
    assert_eq!(code, 1, "stderr={}", err);
    assert!(
        err.contains("المولدات غير مدعومة في الثنائي الأصلي"),
        "got: {}",
        err
    );
}
