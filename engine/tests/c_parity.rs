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
const C_SKIP: &[&str] = &["تثبيت-محلي", "تنفيذ-ج3-69", "تنفيذ-ج3-69-أخطاء", "مولدات-أساسية", "مولدات-كسل", "مولدات-حالة", "مولدات-لانهائي", "مولدات-استنفاد", "مولدات-ولد-من", "مولدات-اغلاق", "مولدات-استئناف-متداخل", "مولدات-تعبير", "مولدات-تعبير-كسل", "مولدات-خطأ-داخلي", "مولدات-متداخل-دالتان", "مولدات-نوع-وطباعة", "مولدات-ولد-من-خطأ", "مولدات-اغلاق-جديد", "مولدات-ولد-بلا-قيمة", "مولدات-ارجاع-مولد", "مولدات-مهمل-بلا-اغلاق", "مولدات-تعليق-داخل-اذا"];

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
