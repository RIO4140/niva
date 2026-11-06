//! الصيغ العادية بلا تشكيل — المهمة 9 (قاعدة بلا تشكيل):
//! «ابن» و«شغل» و«صدر» و«ه» تعمل في CLI والمفسر وVM
//! والمصنوعات؛ والصيغ المشكولة «ابنِ»/«شغّل»/«صدّر»/«هـ»
//! ما زالت مقبولة للتوافق، لا تُطلَب أبدا.
//!
//! كل اختبار يشغل الثنائي الحقيقي (CARGO_BIN_EXE_niva)
//! في مجلد مؤقت فريد، ويقارن المخرج ورمز الخروج حرفيا.
//! (أمر المالك 2026-11-06 ~05:08: لا تشكيل ولا تطويل
//! في أي اسم أمر/كلمة مفتاحية/علم/دالة مدمجة/ثابت.)

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn bin() -> String {
    env!("CARGO_BIN_EXE_niva").to_string()
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("niva-plain-{}-{}", std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(dir: &PathBuf, args: &[&str]) -> (String, String, bool) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.success(),
    )
}

/// «ابن» العادية تبني مصنوعا، و«شغل» العادية تشغّله،
/// والمخرج مطابق للتشغيل المباشر.
#[test]
fn plain_ابن_builds_and_شغل_runs() {
    let dir = fresh_dir();
    fs::write(
        dir.join("برنامج.نيفا"),
        "دالة مضاعف(س):\n    ارجع س * ٢\nاطبع مضاعف(٢١)\n",
    )
    .unwrap();
    let (direct_out, _, direct_ok) = run(&dir, &["برنامج.نيفا"]);
    assert!(direct_ok, "التشغيل المباشر فشل");
    assert_eq!(direct_out.trim(), "٤٢");

    let (_, _, build_ok) = run(&dir, &["ابن", "برنامج.نيفا"]);
    assert!(build_ok, "ابن العادية فشلت");
    assert!(dir.join("برنامج.نيفاب").exists(), "المصنوع لم يُبنَ");

    let (art_out, _, art_ok) = run(&dir, &["شغل", "برنامج.نيفاب"]);
    assert!(art_ok, "شغل العادية فشلت");
    assert_eq!(art_out.trim(), direct_out.trim());
}

/// الصيغ المشكولة «ابنِ»/«شغّل» ما زالت مقبولة توافقا.
#[test]
fn diacritic_ابن_شغل_still_accepted() {
    let dir = fresh_dir();
    fs::write(dir.join("برنامج.نيفا"), "اطبع ٧\n").unwrap();
    let (_, _, build_ok) = run(&dir, &["ابنِ", "برنامج.نيفا"]);
    assert!(build_ok, "ابنِ المشكولة رُفضت");
    assert!(dir.join("برنامج.نيفاب").exists(), "المصنوع لم يُبنَ");
    let (out, _, ok) = run(&dir, &["شغّل", "برنامج.نيفاب"]);
    assert!(ok, "شغّل المشكولة رُفضت");
    assert_eq!(out.trim(), "٧");
}

/// «صدر» العادية في وحدة: المترجم (الافتراضي) والمفسر
/// (--مفسر) يعطيان المخرج نفسه.
#[test]
fn plain_صدر_in_module_both_modes() {
    let dir = fresh_dir();
    fs::write(
        dir.join("وحدة.نيفا"),
        "صدر اجمع\nدالة اجمع(أ، ب):\n    ارجع أ + ب\n",
    )
    .unwrap();
    fs::write(dir.join("برنامج.نيفا"), "استورد وحدة\nاطبع وحدة.اجمع(٣، ٤)\n").unwrap();
    let (vm_out, _, vm_ok) = run(&dir, &["برنامج.نيفا"]);
    let (int_out, _, int_ok) = run(&dir, &["--مفسر", "برنامج.نيفا"]);
    assert!(vm_ok && int_ok, "تشغيل الوحدة فشل");
    assert_eq!(vm_out.trim(), "٧");
    assert_eq!(int_out.trim(), vm_out.trim());
}

/// «ه» العادية و«هـ» المطوّلة: المخرج نفسه في
/// المترجم والمفسر — ٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥.
#[test]
fn plain_ه_and_هـ_both_modes() {
    let dir = fresh_dir();
    fs::write(dir.join("أ.نيفا"), "استورد رياضيات\nاطبع رياضيات.ه\n").unwrap();
    fs::write(dir.join("ب.نيفا"), "استورد رياضيات\nاطبع رياضيات.هـ\n").unwrap();
    let (a_vm, _, a_vm_ok) = run(&dir, &["أ.نيفا"]);
    let (a_int, _, a_int_ok) = run(&dir, &["--مفسر", "أ.نيفا"]);
    let (b_vm, _, b_vm_ok) = run(&dir, &["ب.نيفا"]);
    let (b_int, _, b_int_ok) = run(&dir, &["--مفسر", "ب.نيفا"]);
    assert!(a_vm_ok && a_int_ok && b_vm_ok && b_int_ok, "تشغيل ه فشل");
    assert_eq!(a_vm.trim(), "٢٫٧١٨٢٨١٨٢٨٤٥٩٠٤٥");
    assert_eq!(a_int.trim(), a_vm.trim());
    assert_eq!(b_vm.trim(), a_vm.trim());
    assert_eq!(b_int.trim(), a_vm.trim());
}

/// --مساعدة تعرض الصيغ القانونية العادية، ولا تذكر
/// الصيغ المشكولة أبدا.
#[test]
fn help_uses_plain_forms() {
    let dir = fresh_dir();
    let (out, _, ok) = run(&dir, &["--مساعدة"]);
    assert!(ok, "--مساعدة فشلت");
    assert!(out.contains("ابن"), "المساعدة بلا ابن");
    assert!(out.contains("شغل"), "المساعدة بلا شغل");
    assert!(!out.contains("ابنِ"), "المساعدة تذكر ابنِ");
    assert!(!out.contains("شغّل"), "المساعدة تذكر شغّل");
}

/// الأعلام ذات التاء المربوطة تعمل بعد التطبيع
/// (ة إلى ه): --مساعدة و--نسخة و--شجرة و--اسمح-بالشبكة.
/// (كشف انحدار: التطبيع كان يكسرها — المقارنة كانت
/// بالصيغة الخام، فرفضها الخادم «خيار غير معروف».)
#[test]
fn flags_with_ة_work_after_normalization() {
    let dir = fresh_dir();
    fs::write(dir.join("برنامج.نيفا"), "اطبع ٣\n").unwrap();
    let (help_out, _, help_ok) = run(&dir, &["--مساعدة"]);
    assert!(help_ok && help_out.contains("استخدام"), "--مساعدة فشلت");
    let (ver_out, _, ver_ok) = run(&dir, &["--نسخة"]);
    assert!(ver_ok && ver_out.contains("محرك نيفا"), "--نسخة فشلت");
    let (_, _, tree_ok) = run(&dir, &["--شجرة", "برنامج.نيفا"]);
    assert!(tree_ok, "--شجرة فشلت");
    let (net_out, _, net_ok) = run(&dir, &["--اسمح-بالشبكة", "برنامج.نيفا"]);
    assert!(net_ok && net_out.trim() == "٣", "--اسمح-بالشبكة فشلت");
}
