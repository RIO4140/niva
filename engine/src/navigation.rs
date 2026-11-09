//! مساعدات التنقل المشتركة (اجتماع 56 §71 — الدفعة 2 من
//! برنامج المميزات الـ216: الميزات 4 و7–13 و16 و17 و19–22
//! و26 و179 و23 جزئيا).
//!
//! دوال خالصة بلا حالة جلسة، يستخدمها المفسر والآلة
//! الافتراضية معا (والخلفية C مرآتها في `niva_rt.c`)،
//! فلا يتباعد سلوك المحركين: تحليل العلم النصي، وتحليل
//! فهرس المكدس، وتوسعة `~+` و`~+N`، ومسافة التصحيح
//! (خطأ داميراو واحد)، وتدوير المكدس.

use crate::value::Value;

/// وضع حل المسار: منطقي (معجمي، الافتراضي) أو فيزيائي
/// (حل الروابط أولا). الصيغتان «المتحقق» منهما تطابقان
/// نظيرتيهما: التحقق `-e` في باش بلا أثر إضافي هنا، لأن
/// المجلد بعد الانتقال الفيزيائي محدد دائما
/// بـ`canonicalize` (اجتماع 56 §71، السؤال الخامس).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavMode {
    Logical,
    Physical,
}

impl NavMode {
    pub(crate) fn is_physical(self) -> bool {
        self == NavMode::Physical
    }
}

/// تحليل العلم النصي الثاني لـ`انتقل` (أو الوحيد
/// لـ`المجلد_الحالي` حين يكون فيزيائيا/منطقيا فقط):
/// «منطقي» و«فيزيائي» و«منطقي_متحقق» و«فيزيائي_متحقق».
/// `None` = علم غير معروف.
pub(crate) fn parse_mode_flag(s: &str) -> Option<NavMode> {
    match s {
        "منطقي" | "منطقي_متحقق" => Some(NavMode::Logical),
        "فيزيائي" | "فيزيائي_متحقق" => Some(NavMode::Physical),
        _ => None,
    }
}

/// تحليل العلم النصي لـ`المجلد_الحالي`: «منطقي» أو
/// «فيزيائي» فقط (بلا صيغ «متحقق» هنا — لا تحقق في
/// القراءة). `None` = علم غير معروف.
pub(crate) fn parse_pwd_flag(s: &str) -> Option<NavMode> {
    match s {
        "منطقي" => Some(NavMode::Logical),
        "فيزيائي" => Some(NavMode::Physical),
        _ => None,
    }
}

/// عدد صحيح من نص بأرقام غربية أو عربية مشرقية، مع
/// إشارة سالبة اختيارية في أوله. `None` = ليس عددا.
pub(crate) fn parse_arabic_int(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    if digits.is_empty() {
        return None;
    }
    let mut n: i64 = 0;
    for c in digits.chars() {
        let d = match c {
            '0'..='9' => c as i64 - '0' as i64,
            '٠'..='٩' => c as i64 - '٠' as i64,
            '۰'..='۹' => c as i64 - '۰' as i64,
            _ => return None,
        };
        n = n.checked_mul(10)?.checked_add(d)?;
    }
    Some(if neg { -n } else { n })
}

/// فهرس مكدس من معامل دالة: عدد صحيح، أو نص رقمي
/// (لعبارة REPL التي تمرر نصوصا). `None` = ليس فهرسا.
pub(crate) fn parse_stack_index(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) if n.fract() == 0.0 && *n >= -9007199254740992.0 && *n <= 9007199254740992.0 => {
            Some(*n as i64)
        }
        Value::Str(s) => parse_arabic_int(s),
        _ => None,
    }
}

/// حل فهرس مكدس (سالب من النهاية) إلى موضع في
/// `Vec` طوله `len`. `None` = خارج المكدس.
pub(crate) fn resolve_stack_index(idx: i64, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let len = len as i64;
    let pos = if idx >= 0 { idx } else { len + idx };
    if pos < 0 || pos >= len {
        None
    } else {
        Some(pos as usize)
    }
}

/// تحليل بادئة `~+` في مسار: `~+` وحدها أو `~+/...`
/// (الحالي)، و`~+N` أو `~+N/...` (عنصر المكدس N:
/// 0 = الحالي، وN ≥ 1 من الأحدث). ترجع `(الفهرس،
/// الباقي بعد البادئة)` — الباقي يبدأ بـ`/` أو فارغ.
/// `None` = ليست صيغة `~+`. `~-` ليست هنا أبدا:
/// محذوفة بأمر المالك فتبقى اسما حرفيا.
pub(crate) fn parse_tilde_plus(path: &str) -> Option<(i64, &str)> {
    let rest = path.strip_prefix("~+")?;
    if rest.is_empty() || rest.starts_with('/') {
        return Some((0, rest));
    }
    // أرقام (غربية أو مشرقية) ثم نهاية أو `/`.
    let mut end = 0usize;
    for (i, c) in rest.char_indices() {
        if c.is_ascii_digit() || ('٠'..='٩').contains(&c) || ('۰'..='۹').contains(&c) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let num = parse_arabic_int(&rest[..end])?;
    let tail = &rest[end..];
    if tail.is_empty() || tail.starts_with('/') {
        Some((num, tail))
    } else {
        None
    }
}

/// هل يفصل بين الاسمين خطأ داميراو واحد بالضبط
/// (إدراج حرف أو حذفه أو استبداله أو تبديل حرفين
/// متجاورين)؟ مسافة 0 = متطابقان (لا خطأ). مقارنة
/// على نقاط الشفرة (chars) لا البايتات، فالحرف
/// العربي الواحد خطوة واحدة.
pub(crate) fn edit_distance_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a == b {
        return false;
    }
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > 1 {
        return false;
    }
    if n == m {
        // استبدال واحد، أو تبديل متجاورين واحد.
        let mut diffs: Vec<usize> = Vec::new();
        for i in 0..n {
            if a[i] != b[i] {
                diffs.push(i);
            }
        }
        if diffs.len() == 1 {
            return true;
        }
        if diffs.len() == 2 {
            let (i, j) = (diffs[0], diffs[1]);
            return j == i + 1 && a[i] == b[j] && a[j] == b[i];
        }
        return false;
    }
    // إدراج/حذف واحد: الأقصر بادئة للأطول مع تخط واحد.
    let (short, long) = if n < m { (&a, &b) } else { (&b, &a) };
    let mut i = 0usize;
    let mut j = 0usize;
    let mut skipped = false;
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
        } else if !skipped {
            skipped = true;
            j += 1;
        } else {
            return false;
        }
    }
    true
}

/// المسار الفيزيائي لمجلد: `canonicalize` (حل كل الروابط
/// الرمزية و`..` على الحقيقي) إن كان مجلدا موجودا،
/// وإلا `None` (فيسقط المتصل إلى المنطقي).
pub(crate) fn canonical_dir(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let canon = std::fs::canonicalize(path).ok()?;
    if canon.is_dir() {
        Some(canon)
    } else {
        None
    }
}

/// هل تُستشار `CDPATH` لهذا المعامل؟ POSIX (مثبت
/// بفحوص باش 5.2 الحية 2026-11-09): لا تُستشار حين
/// يكون المكون الأول `.` أو `..` — أي المعامل `.`
/// أو `..` أو يبدأ بـ`./` أو `../`. المكون اللاحق
/// لا يمنعها (`هدف/..` تُستشار فيها `CDPATH`).
pub(crate) fn cdpath_applies(raw: &str) -> bool {
    !(raw == "." || raw == ".." || raw.starts_with("./") || raw.starts_with("../"))
}

/// نتيجة محاولة انتقال واحدة — نجاح بمسار، أو غياب،
/// أو موجود ليس مجلدا، أو خطأ سياسة (خروج من النطاق)
/// يُحفَظ ليُعرض. مشتركة بين المفسر والآلة الافتراضية.
pub(crate) enum TryDir {
    Ok(std::path::PathBuf),
    NotFound,
    NotDir,
    Policy(crate::error::RioError),
}

/// تدوير المكدس إلى العنصر `pos`: يصير العنصر هو
/// المجلد الحالي، ويدخل المجلد القديم مكانه في
/// المكدس (دوران القائمة الدائرية [الحالي، عناصر
/// المكدس]). ترجع المجلد الجديد الحالي.
pub(crate) fn rotate_stack(
    cwd: &mut std::path::PathBuf,
    stack: &mut Vec<std::path::PathBuf>,
    pos: usize,
) -> std::path::PathBuf {
    let target = stack.remove(pos);
    let old = std::mem::replace(cwd, target);
    stack.insert(pos, old);
    cwd.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_flags() {
        assert_eq!(parse_mode_flag("منطقي"), Some(NavMode::Logical));
        assert_eq!(parse_mode_flag("فيزيائي"), Some(NavMode::Physical));
        assert_eq!(parse_mode_flag("منطقي_متحقق"), Some(NavMode::Logical));
        assert_eq!(parse_mode_flag("فيزيائي_متحقق"), Some(NavMode::Physical));
        assert_eq!(parse_mode_flag("-P"), None);
        assert_eq!(parse_mode_flag(""), None);
        assert_eq!(parse_pwd_flag("فيزيائي_متحقق"), None);
    }

    #[test]
    fn cdpath_applies_dot_components() {
        // POSIX (فحوص باش 5.2 الحية): المكون الأول
        // `.` أو `..` يلغي `CDPATH`؛ والمكون اللاحق
        // لا يلغيها.
        assert!(!cdpath_applies("."));
        assert!(!cdpath_applies(".."));
        assert!(!cdpath_applies("./أ"));
        assert!(!cdpath_applies("../أ"));
        assert!(!cdpath_applies("../.."));
        assert!(!cdpath_applies("./.."));
        assert!(cdpath_applies("هدف"));
        assert!(cdpath_applies("هدف/.."));
        assert!(cdpath_applies("هدف/../أ"));
        assert!(cdpath_applies(""));
    }

    #[test]
    fn arabic_ints() {
        assert_eq!(parse_arabic_int("٣"), Some(3));
        assert_eq!(parse_arabic_int("-٢"), Some(-2));
        assert_eq!(parse_arabic_int("12"), Some(12));
        assert_eq!(parse_arabic_int("١٢a"), None);
        assert_eq!(parse_arabic_int(""), None);
        assert_eq!(parse_arabic_int("-"), None);
    }

    #[test]
    fn tilde_plus_forms() {
        assert_eq!(parse_tilde_plus("~+"), Some((0, "")));
        assert_eq!(parse_tilde_plus("~+/أ"), Some((0, "/أ")));
        assert_eq!(parse_tilde_plus("~+1"), Some((1, "")));
        assert_eq!(parse_tilde_plus("~+٢/ب"), Some((2, "/ب")));
        assert_eq!(parse_tilde_plus("~+x"), None);
        assert_eq!(parse_tilde_plus("~-"), None);
        assert_eq!(parse_tilde_plus("~"), None);
        assert_eq!(parse_tilde_plus("~/أ"), None);
    }

    #[test]
    fn stack_index_resolution() {
        assert_eq!(resolve_stack_index(0, 3), Some(0));
        assert_eq!(resolve_stack_index(-1, 3), Some(2));
        assert_eq!(resolve_stack_index(3, 3), None);
        assert_eq!(resolve_stack_index(0, 0), None);
    }

    #[test]
    fn single_edit_spelling() {
        // استبدال واحد.
        assert!(edit_distance_one("مشاريع", "مشاريع".replace('ي', "ا").as_str()));
        assert!(edit_distance_one("مشاريع", "مشارع"));
        // حذف واحد وإدراج واحد.
        assert!(edit_distance_one("مشاريع", "مشاري"));
        assert!(edit_distance_one("مشاري", "مشاريع"));
        // تبديل متجاورين واحد.
        assert!(edit_distance_one("مشاريع", "مشايع"));
        // متطابقان = ليس خطأ.
        assert!(!edit_distance_one("مشاريع", "مشاريع"));
        // خطآن.
        assert!(!edit_distance_one("مشاريع", "مشار"));
        assert!(!edit_distance_one("مشاريع", "كتب"));
        // فرق طول أكبر من واحد.
        assert!(!edit_distance_one("أ", "أبجد"));
    }

    #[test]
    fn rotation_moves_old_into_place() {
        use std::path::PathBuf;
        let mut cwd = PathBuf::from("/بيت");
        let mut stack = vec![
            PathBuf::from("/أ"),
            PathBuf::from("/ب"),
            PathBuf::from("/ج"),
        ];
        let now = rotate_stack(&mut cwd, &mut stack, 1);
        assert_eq!(now, PathBuf::from("/ب"));
        assert_eq!(cwd, PathBuf::from("/ب"));
        assert_eq!(
            stack,
            vec![
                PathBuf::from("/أ"),
                PathBuf::from("/بيت"),
                PathBuf::from("/ج"),
            ]
        );
    }
}
