//! بوابة تسرب المفسر — اجتماع 32، السؤال الحادي عشر.
//!
//! الأنماط الأربعة المثبتة لعزل إطارات استدعاء
//! المفسر، مفسرا مقابل الآلة الافتراضية (المترجم):
//! كل حالة كانت تطبع في المفسر غير ما يطبعه
//! المترجم قبل العزل. الحالات نفسها في
//! `parity_cases::LEAK_CASES` — مصدر واحد للحقيقة.
//!
//! التشغيل: cargo test --test leak_parity
//! (متسلسلا: cargo test -- --test-threads=1)

#[path = "parity_cases.rs"]
mod parity_cases;

fn assert_same(name: &str, source: &str, files: &[(&str, &str)]) {
    let (out_i, err_i, code_i) = parity_cases::run_one(source, files, true);
    let (out_v, err_v, code_v) = parity_cases::run_one(source, files, false);
    assert_eq!(
        (out_i.clone(), err_i.clone(), code_i),
        (out_v.clone(), err_v.clone(), code_v),
        "اختلاف تكافؤ تسرب في «{}»:\n--- المفسر (خروج {}):\nمخرج:\n{}\nخطأ:\n{}\n--- الافتراضي (خروج {}):\nمخرج:\n{}\nخطأ:\n{}",
        name, code_i, out_i, err_i, code_v, out_v, err_v
    );
}

fn run_case(name: &str) {
    let (n, lines, files) = parity_cases::LEAK_CASES
        .iter()
        .find(|(n, _, _)| *n == name)
        .unwrap_or_else(|| panic!("حالة تسرب غير موجودة: {}", name));
    assert_same(n, &parity_cases::src(lines), files);
}

#[test]
fn leak_module_local_1() {
    run_case("تسرب-وحدة-1");
}

#[test]
fn leak_caller_chain_poisoning_2() {
    run_case("تسميم-متصل-2");
}

#[test]
fn leak_helper_overwrites_caller_3() {
    run_case("تسرب-مساعد-3");
}

#[test]
fn leak_inside_one_module_4() {
    run_case("تسرب-داخل-وحدة-4");
}
