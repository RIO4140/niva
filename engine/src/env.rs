//! متغيرات بيئة نيفا (اجتماع 45 §59، البنود 39–41).
//!
//! ثمانية متغيرات فقط في v1، بأسماء عربية، للقراءة فقط،
//! عبر الدالة المدمجة `بيئة(اسم)` — بلا تمرير لبيئة
//! النظام في v1: القيم كلها من المحرك نفسه، فلا يرى
//! البرنامج أسرار الجهاز ولا متغيراته.
//!
//! القيم (اجتماع 45):
//! - البيت = `~`
//! - مجلد_البرنامج = `$نيفا`
//! - المسار = `$نيفا` (مجلد البرنامج وحده في v1)
//! - إصدار_نيفا = إصدار المحرك (مثال «١٫٠٫٠»)
//! - الطرفية = `نيفا` (هوية صادقة، لا ادعاء طرفية أخرى)
//! - اللغة = `ar_EG.UTF-8`
//! - المجلد_المؤقت = `$نيفا/tmp`
//! - الألوان = `truecolor`

/// أسماء المتغيرات الثمانية في v1، بالترتيب الذي
/// يظهر في التوثيق.
pub(crate) const NAMES: [&str; 8] = [
    "البيت",
    "مجلد_البرنامج",
    "المسار",
    "إصدار_نيفا",
    "الطرفية",
    "اللغة",
    "المجلد_المؤقت",
    "الألوان",
];

/// قيمة متغير معروف، أو `None` إن لم يكن من الثمانية.
/// `version` هو إصدار المحرك (قيمة `إصدار_نيفا`).
pub(crate) fn lookup(name: &str, version: &str) -> Option<String> {
    if !NAMES.contains(&name) {
        return None;
    }
    let value: &str = match name {
        "البيت" => "~",
        "مجلد_البرنامج" => "$نيفا",
        "المسار" => "$نيفا",
        "إصدار_نيفا" => return Some(version.to_string()),
        "الطرفية" => "نيفا",
        "اللغة" => "ar_EG.UTF-8",
        "المجلد_المؤقت" => "$نيفا/tmp",
        "الألوان" => "truecolor",
        // NAMES يضمن الوصول هنا للأسماء الثمانية فقط.
        _ => return None,
    };
    Some(value.to_string())
}

/// قراءة مع التحقق (البند 41): الاسم غير الصالح أو
/// المجهول خطأ وقت تشغيل يسمّي الاسم، بلا إيقاف
/// للجلسة — يطبع الخطأ ويكمل المستخدم.
pub(crate) fn lookup_checked(name: &str, version: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err(format!("اسم متغير البيئة غير صالح: {}", name));
    }
    lookup(name, version).ok_or_else(|| format!("متغير بيئة غير معروف: {}", name))
}

/// صلاحية اسم متغير (البند 41): حروف عربية وأرقام
/// وشرطة سفلية، ولا يبدأ برقم، ولا يكون فارغا.
pub(crate) fn valid_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    // الأول ليس رقما — أي رقم (غربي أو عربي مشرقي).
    if chars.next().map(|c| c.is_numeric()).unwrap_or(false) {
        return false;
    }
    name.chars().all(|c| {
        c == '_'
            || c.is_ascii_digit()
            || ('\u{0600}'..='\u{06FF}').contains(&c)
            || ('\u{0750}'..='\u{077F}').contains(&c)
            || ('\u{08A0}'..='\u{08FF}').contains(&c)
            || ('\u{FB50}'..='\u{FDFF}').contains(&c)
            || ('\u{FE70}'..='\u{FEFF}').contains(&c)
    })
}

/// صلاحية قيمة (البند 41): بلا بايت فارغ.
pub(crate) fn valid_value(value: &str) -> bool {
    !value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eight_resolve() {
        let v = "١٫٠٫٠";
        assert_eq!(lookup("البيت", v).as_deref(), Some("~"));
        assert_eq!(lookup("مجلد_البرنامج", v).as_deref(), Some("$نيفا"));
        assert_eq!(lookup("المسار", v).as_deref(), Some("$نيفا"));
        assert_eq!(lookup("إصدار_نيفا", v).as_deref(), Some("١٫٠٫٠"));
        assert_eq!(lookup("الطرفية", v).as_deref(), Some("نيفا"));
        assert_eq!(lookup("اللغة", v).as_deref(), Some("ar_EG.UTF-8"));
        assert_eq!(lookup("المجلد_المؤقت", v).as_deref(), Some("$نيفا/tmp"));
        assert_eq!(lookup("الألوان", v).as_deref(), Some("truecolor"));
        // الثمانية فقط: لا تمرير لبيئة النظام.
        assert_eq!(lookup("PATH", v), None);
        assert_eq!(lookup("HOME", v), None);
        assert_eq!(lookup("بيت", v), None);
        assert_eq!(NAMES.len(), 8);
    }

    #[test]
    fn name_validation() {
        assert!(valid_name("البيت"));
        assert!(valid_name("مجلد_البرنامج"));
        assert!(valid_name("اسم١٢٣"));
        assert!(!valid_name(""));
        assert!(!valid_name("١بيت"));
        assert!(!valid_name("بيت بيت"));
        assert!(!valid_name("بيت-بيت"));
        assert!(!valid_name("بيت/بيت"));
        assert!(!valid_name("HOME"));
        assert!(!valid_name("بيت!"));
    }

    #[test]
    fn value_validation() {
        assert!(valid_value("قيمة عادية"));
        assert!(valid_value(""));
        assert!(!valid_value("قبل\0بعد"));
    }
}
