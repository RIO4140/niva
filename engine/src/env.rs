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

/// كل الأسماء التي تقرأها `بيئة()`، بالترتيب الثابت
/// الذي تعرضه `اعرض_البيئة()` (اجتماع 58 §73):
/// الثمانية الثابتة، ثم `المجلد_الحالي` الديناميكي
/// (اجتماع 56 §71)، ثم `المستخدم` و`اسم_المضيف`
/// (الميزتان 100 و177)، ثم الديناميكية الثلاث
/// (الميزة 101).
pub(crate) const ALL_NAMES: [&str; 14] = [
    "البيت",
    "مجلد_البرنامج",
    "المسار",
    "إصدار_نيفا",
    "الطرفية",
    "اللغة",
    "المجلد_المؤقت",
    "الألوان",
    "المجلد_الحالي",
    "المستخدم",
    "اسم_المضيف",
    "عشوائي",
    "ثواني",
    "رقم_السطر",
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

// ---------- الدفعة 4 — المتغيرات (اجتماع 58 §73) ----------

/// عدد عشوائي صحيح 0–32767، جديد عند كل قراءة
/// (مقابل `$RANDOM` — الحد نفسه في باش). يعيد
/// استعمال مولد `عشوائي` نفسه، فلا مولد ثان.
pub(crate) fn random_0_32767() -> u32 {
    (crate::stdlib::next_u64() % 32768) as u32
}

/// اسم المضيف (الميزة 177، مقابل `HOSTNAME`):
/// من `/proc/sys/kernel/hostname` على لينكس/
/// أندرويد — سابقة `نظام.اسم_النظام` نفسها —
/// ورفض صريح خارجهما، بلا قراءة بيئة موروثة أبدا.
pub(crate) fn hostname() -> Result<String, String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let raw = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map_err(|_| "تعذر قراءة اسم المضيف".to_string())?;
        let name = raw.trim().to_string();
        if name.is_empty() {
            return Err("اسم المضيف فارغ".to_string());
        }
        Ok(name)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err("بيئة(\"اسم_المضيف\") غير مدعومة على هذه المنصة".to_string())
    }
}

/// اسم المستخدم (الميزة 100، مقابل `USER`): رقم
/// المستخدم من `/proc/self/status` ثم اسمه من
/// `/etc/passwd` — سابقة `نظام.معرف_المستخدم`
/// نفسها — ورفض صريح خارجهما، بلا قراءة بيئة
/// موروثة أبدا.
pub(crate) fn username() -> Result<String, String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let status = std::fs::read_to_string("/proc/self/status")
            .map_err(|_| "تعذر قراءة معرف المستخدم".to_string())?;
        let uid = status
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .ok_or_else(|| "تعذر قراءة معرف المستخدم".to_string())?;
        let passwd = std::fs::read_to_string("/etc/passwd")
            .map_err(|_| "تعذر قراءة اسم المستخدم".to_string())?;
        for line in passwd.lines() {
            let mut parts = line.split(':');
            if let (Some(name), Some(_), Some(id)) =
                (parts.next(), parts.next(), parts.next())
            {
                if id == uid && !name.is_empty() {
                    return Ok(name.to_string());
                }
            }
        }
        Err("اسم المستخدم غير موجود".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err("بيئة(\"المستخدم\") غير مدعومة على هذه المنصة".to_string())
    }
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

    // الدفعة 4 — المتغيرات (اجتماع 58 §73).
    #[test]
    fn all_fourteen_names() {
        assert_eq!(ALL_NAMES.len(), 14);
        assert_eq!(NAMES.len(), 8);
        for name in NAMES {
            assert!(ALL_NAMES.contains(&name));
        }
        for name in ["المجلد_الحالي", "المستخدم", "اسم_المضيف", "عشوائي", "ثواني", "رقم_السطر"] {
            assert!(ALL_NAMES.contains(&name));
        }
    }

    #[test]
    fn random_in_bash_range() {
        for _ in 0..20 {
            assert!(random_0_32767() <= 32767);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn hostname_and_username_read() {
        assert!(!hostname().unwrap().is_empty());
        assert!(!username().unwrap().is_empty());
    }
}
