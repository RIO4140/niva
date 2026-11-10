//! بدائيتا استيراد حزم «حزم» (التصميم CATALOG-20 §8):
//! الاستشارة الثانية والرابعة في ترتيب `استورد` —
//! `حزم/<اسم>/<اسم>.نيفا` داخل مجلد المشروع، ثم
//! المخزن العام `~/.نيفا/حزم/<اسم>-<إصدار>/<اسم>.نيفا`
//! (والنسخة: ما يثبّته قفل المشروع `قفل.حزمة` إن وُجد،
//! وإلا الأحدث في المخزن). تنفيذ واحد مشترك بين
//! المفسر والمترجم — فلا يختلف الوضعان أبدا.
//!
//! لا منطق حلّ هنا: القفل يُقرأ سطرا واحدا فقط
//! (الاسم ← الإصدار)، والحلّال نفسه كود نيفا في
//! مستودع `حزم` المستقل. والمقارنة العددية للإصدارات
//! مرآة `قارن_إصدارين` في الخطوة 2 من `حزم`
//! (الثلاثية أولا، ثم المستقر أكبر من التجريبي
//! بالثلاثية نفسها، ثم المعرفات واحدا واحدا).
//!
//! حدود صادقة (v1):
//! - الاستيراد المتداخل داخل ملف حزمة يتبع الترتيب
//!   نفسه من مجلد البرنامج — فلا مسار نسبي داخل
//!   مجلد الحزمة (سلوك `استورد` القائم لم يُمسّ).
//! - قفل لا يُقرأ (غائب أو غير نصي) يعني «بلا قفل»
//!   فيُختار الأحدث؛ أما سطر الحزمة نفسها بإصدار
//!   غير صالح فيُرفض بخطأ عربي — لا اختيار صامت
//!   لإصدار آخر حين يقيّد القفل.
//! - القفل المثبّت لإصدار غير مثبّت في المخزن لا
//!   يرجع إلى الأحدث أبدا — فالقفل مصدر الحقيقة.
//! - `~` هنا مجلد البرنامج نفسه (كما في `مسار_المخزن`
//!   في `حزم`) — فالمخزن تحت مجلد البرنامج، لا بيت
//!   نظام التشغيل.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// إصدار `رئيسي.فرعي.إصلاحي` مع لاحقة تجريبية اختيارية
/// (مطابقة التصميم §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PreId>,
}

/// معرف واحد من لاحقة تجريبية: رقمي أو حرفي.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreId {
    Num(u64),
    Str(String),
}

/// نتيجة بحث ملف وحدة: المسار الموجود (إن وُجد)
/// ومسارات المرشحين التي استُشيرت بالترتيب (لرسالة
/// الخطأ — فالمسارات كلها تحت مجلد البرنامج).
pub struct ModuleSearch {
    pub found: Option<PathBuf>,
    pub searched: Vec<PathBuf>,
}

/// مصدر مدمج لوحدات مدير الحزم (أمر المالك 2026-11-10
/// ~03:07: «حدث حزم» أمر مباشر في الموجه لأي مستخدم):
/// نسخ حرفية من `niva-packages/src/` موضوعة في
/// `src/حزم/` لتُبنى مع المحرك نفسه — فالعبارة تعمل
/// بلا تثبيت مسبق وبلا شبكة أولى. هذا ملاذ أخير فقط:
/// يُستشار بعد فشل بحث القرص كله (في المفسر
/// والمترجم معا)، فملف على القرص يسبقه دائما —
/// واختبارات المدير من ملفاته لا تتأثر أبدا. وعند أي
/// تغيير في المدير تُنسخ الملفات العشرة هنا يدويا
/// (لا مزامنة تلقائية — راجع سجل الدفعات).
pub fn مصدر_مدمج(وحدة: &str) -> Option<&'static str> {
    match وحدة {
        "أوامر" => Some(include_str!("حزم/أوامر.نيفا")),
        "إصدار" => Some(include_str!("حزم/إصدار.نيفا")),
        "بيان" => Some(include_str!("حزم/بيان.نيفا")),
        "حلال" => Some(include_str!("حزم/حلال.نيفا")),
        "سجل" => Some(include_str!("حزم/سجل.نيفا")),
        "قفل" => Some(include_str!("حزم/قفل.نيفا")),
        "مخزن" => Some(include_str!("حزم/مخزن.نيفا")),
        "مشروع" => Some(include_str!("حزم/مشروع.نيفا")),
        "نشر" => Some(include_str!("حزم/نشر.نيفا")),
        "نص" => Some(include_str!("حزم/نص.نيفا")),
        _ => None,
    }
}

/// طبّع الأرقام المشرقية (٠-٩) إلى لاتينية، والفاصلة
/// العشرية العربية (٫) إلى نقطة — كما في `طبّع_أرقام`
/// في `إصدار.نيفا` حرفيا. الأرقام اللاتينية وكل ما
/// عداها يبقى كما هو.
fn normalize_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '٠'..='٩' => ((c as u32 - '٠' as u32) as u8 + b'0') as char,
            '٫' => '.',
            other => other,
        })
        .collect()
}

/// أحرف ممنوعة في معرفات اللاحقة التجريبية — كما في
/// `صالح_لاحقة` في `إصدار.نيفا` حرفيا (والشرطة `-`
/// مسموحة داخل المعرف؛ والنقطة فاصل لا حرف).
const FORBIDDEN_SUFFIX_CHARS: &[char] = &[
    ' ', ':', '/', '\\', '_', '#', '@', '،', '؟', '!', 'ـ', 'ً', 'ٌ', 'ٍ', 'َ', 'ُ', 'ِ', 'ّ', 'ْ',
    '(', ')', '[', ']', '{', '}', '|', '*', '?', '<', '>', '"', '\'', '`', '~', '$', '%', '^',
    '&', '+', '=', ';', ',',
];

/// تحقق من لاحقة تجريبية خام (بعد `-` الأول): كل
/// معرف مفصول بنقطة غير فارغ وفيه حرف غير `-`،
/// وبلا حرف ممنوع. مرآة `صالح_لاحقة`.
fn valid_suffix(pre: &str) -> bool {
    if pre.is_empty() {
        return false;
    }
    for id in pre.split('.') {
        if id.is_empty() {
            return false;
        }
        if !id.chars().any(|c| c != '-') {
            return false;
        }
        if id.chars().any(|c| FORBIDDEN_SUFFIX_CHARS.contains(&c)) {
            return false;
        }
    }
    true
}

/// حلّل نص إصدار `X.Y.Z` مع لاحقة تجريبية اختيارية
/// بعد `-` (معرفات مفصولة بنقاط — مرآة `صالح_لاحقة`:
/// كل معرف غير فارغ وفيه حرف غير `-` وبلا حرف
/// ممنوع). الأرقام المشرقية تُطبَّع. أي صيغة أخرى
/// تعيد `None`.
pub fn parse_version(text: &str) -> Option<Version> {
    let text = normalize_digits(text.trim());
    if text.is_empty() {
        return None;
    }
    let (core, pre) = match text.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (text.as_str(), None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        nums[i] = p.parse::<u64>().ok()?;
    }
    let mut pre_ids = Vec::new();
    if let Some(pre) = pre {
        if !valid_suffix(pre) {
            return None;
        }
        for id in pre.split('.') {
            if id.bytes().all(|b| b.is_ascii_digit()) {
                match id.parse::<u64>() {
                    Ok(n) => pre_ids.push(PreId::Num(n)),
                    Err(_) => pre_ids.push(PreId::Str(id.to_string())),
                }
            } else {
                pre_ids.push(PreId::Str(id.to_string()));
            }
        }
    }
    Some(Version {
        major: nums[0],
        minor: nums[1],
        patch: nums[2],
        pre: pre_ids,
    })
}

/// قارن إصدارين: الثلاثية عدديا أولا، ثم المستقر أكبر
/// من أي تجريبي بالثلاثية نفسها، ثم المعرفات واحدا
/// واحدا (رقميان عدديا، والرقمي أصغر من الحرفي،
/// وحرفيان معجميا)، ثم القائمة الأقصر أصغر.
pub fn compare_versions(a: &Version, b: &Version) -> Ordering {
    (a.major, a.minor, a.patch)
        .cmp(&(b.major, b.minor, b.patch))
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (x, y) in a.pre.iter().zip(b.pre.iter()) {
                    let ord = match (x, y) {
                        (PreId::Num(m), PreId::Num(n)) => m.cmp(n),
                        (PreId::Num(_), PreId::Str(_)) => Ordering::Less,
                        (PreId::Str(_), PreId::Num(_)) => Ordering::Greater,
                        (PreId::Str(m), PreId::Str(n)) => m.cmp(n),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                a.pre.len().cmp(&b.pre.len())
            }
        })
}

/// اقرأ نص القفل (`قفل.حزمة` في مجلد البرنامج) إن
/// وُجد وكان نصا صالحا بسقف 5 ميغابايت (سقف قراءة
/// الوحدات نفسه). الغائب أو غير النصي أو الأكبر
/// يعني «بلا قفل» — لا خطأ هنا.
fn read_lock_text(program_dir: &Path) -> Option<String> {
    let path = program_dir.join("قفل.حزمة");
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > 5 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    String::from_utf8(bytes).ok()
}

/// الإصدار المثبّت لحزمة في قفل المشروع، إن وُجد
/// سطرها. سطر تعليق (`#...`) يُتخطى — ومنه سطور
/// `# تجاوز:` (فالسطر الرئيسي يحمل الإصدار المحلول
/// أصلا). أول سطر مطابق يفوز. إصدار غير صالح في
/// سطر الحزمة خطأ عربي (لا اختيار صامت لغيره).
pub fn locked_version(program_dir: &Path, module: &str) -> Result<Option<String>, String> {
    let Some(text) = read_lock_text(program_dir) else {
        return Ok(None);
    };
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        if name != module {
            continue;
        }
        let Some(version) = fields.next() else {
            continue;
        };
        if parse_version(version).is_none() {
            return Err(format!(
                "إصدار غير صالح في القفل للحزمة {}: {}",
                module, version
            ));
        }
        return Ok(Some(version.to_string()));
    }
    Ok(None)
}

/// مجلد المخزن العام تحت مجلد البرنامج.
fn store_dir(program_dir: &Path) -> PathBuf {
    program_dir.join(".نيفا").join("حزم")
}

/// مخازن البحث (اجتماع 47 §61): مخزن مجلد البرنامج
/// أولا؛ وإن كان مجلد البرنامج `home` نفسه فمخزن
/// مجلد الأب (الترمنال) ثانيا — فبرنامج سائب في
/// البيت يرى ما ثبّته `حزم` من التفاعلي.
fn store_dirs(program_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![store_dir(program_dir)];
    if program_dir
        .file_name()
        .map(|n| n == "home")
        .unwrap_or(false)
    {
        if let Some(parent) = program_dir.parent() {
            let up = store_dir(parent);
            if up != dirs[0] {
                dirs.push(up);
            }
        }
    }
    dirs
}

/// أحدث إصدار مثبّت لحزمة في المخزن العام، من بين
/// المجلدات التي تحوي ملف الدخول `<اسم>.نيفا` فعلا.
/// يُرجَع الإصدار كما كُتب في اسم المجلد.
/// (اجتماع 47 §61: يُفحص مخزن الأب أيضا حين يكون
/// مجلد البرنامج `home`، والأحدث بين المخزنين يفوز.)
pub fn newest_store_version(program_dir: &Path, module: &str) -> Option<String> {
    let prefix = format!("{}-", module);
    let entry_file = format!("{}.نيفا", module);
    let mut best: Option<(Version, String)> = None;
    for dir in store_dirs(program_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let Some(version) = parse_version(rest) else {
                continue;
            };
            if !entry.path().join(&entry_file).exists() {
                continue;
            }
            let replace = match &best {
                None => true,
                Some((current, _)) => compare_versions(&version, current) == Ordering::Greater,
            };
            if replace {
                best = Some((version, rest.to_string()));
            }
        }
    }
    best.map(|(_, rest)| rest)
}

/// ابحث عن ملف وحدة بالاسم في ترتيب §8 (بعد الأصلية
/// التي يفحصها المتصل قبل النداء): `حزم/<اسم>/`، ثم
/// مجلد البرنامج، ثم المخزن العام (قفل ثم أحدث).
/// الاسم الفارغ أو الحاوي فاصلا مساريا لا يُبحث
/// (فيُرجَع بلا نتيجة — فالاسم من المحلل معرف
/// وحده أصلا).
pub fn find_module_file(program_dir: &Path, module: &str) -> Result<ModuleSearch, String> {
    let mut searched: Vec<PathBuf> = Vec::new();
    let plain = program_dir.join(format!("{}.نيفا", module));
    if module.is_empty()
        || module.contains('/')
        || module.contains('\\')
        || module == "."
        || module == ".."
    {
        searched.push(plain);
        return Ok(ModuleSearch {
            found: None,
            searched,
        });
    }
    // 2 — `حزم/<اسم>/<اسم>.نيفا` داخل مجلد المشروع.
    let project = program_dir
        .join("حزم")
        .join(module)
        .join(format!("{}.نيفا", module));
    searched.push(project.clone());
    if project.exists() {
        return Ok(ModuleSearch {
            found: Some(project),
            searched,
        });
    }
    // 3 — مجلد البرنامج (سلوك اليوم كما هو).
    searched.push(plain.clone());
    if plain.exists() {
        return Ok(ModuleSearch {
            found: Some(plain),
            searched,
        });
    }
    // 4 — المخزن العام: قفل ثم أحدث.
    // (اجتماع 47 §61: مخزن الأب أيضا حين يكون مجلد
    // البرنامج `home` — والترتيب مخزن البيت أولا.)
    match locked_version(program_dir, module)? {
        Some(pinned) => {
            for dir in store_dirs(program_dir) {
                let candidate = dir
                    .join(format!("{}-{}", module, pinned))
                    .join(format!("{}.نيفا", module));
                searched.push(candidate.clone());
                if candidate.exists() {
                    return Ok(ModuleSearch {
                        found: Some(candidate),
                        searched,
                    });
                }
            }
            // القفل يقيّد — لا رجوع إلى الأحدث أبدا.
            Ok(ModuleSearch {
                found: None,
                searched,
            })
        }
        None => match newest_store_version(program_dir, module) {
            Some(newest) => {
                for dir in store_dirs(program_dir) {
                    let candidate = dir
                        .join(format!("{}-{}", module, newest))
                        .join(format!("{}.نيفا", module));
                    searched.push(candidate.clone());
                    if candidate.exists() {
                        return Ok(ModuleSearch {
                            found: Some(candidate),
                            searched,
                        });
                    }
                }
                Ok(ModuleSearch {
                    found: None,
                    searched,
                })
            }
            None => {
                for dir in store_dirs(program_dir) {
                    searched.push(dir);
                }
                Ok(ModuleSearch {
                    found: None,
                    searched,
                })
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("niva-packages-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }

    fn version(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            pre: vec![],
        }
    }

    #[test]
    fn parse_version_plain_and_arabic_digits() {
        assert_eq!(parse_version("1.2.3"), Some(version(1, 2, 3)));
        assert_eq!(parse_version("١٫٢٫٣"), Some(version(1, 2, 3)));
        assert_eq!(parse_version("1.10.0"), Some(version(1, 10, 0)));
        assert_eq!(parse_version("  2.0.0  "), Some(version(2, 0, 0)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1..3"), None);
        assert_eq!(parse_version("-1.2.3"), None);
    }

    #[test]
    fn parse_version_prerelease() {
        let v = parse_version("1.0.0-ألفا.٢").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.pre, vec![PreId::Str("ألفا".to_string()), PreId::Num(2)]);
        // الشرطة مسموحة داخل المعرف (كـ`صالح_لاحقة`)،
        // والمعرف المؤلف من شرطات وحده مرفوض.
        assert!(parse_version("1.0.0-ألفا-ب").is_some());
        assert_eq!(parse_version("1.0.0--"), None);
        assert_eq!(parse_version("1.0.0-"), None);
        assert_eq!(parse_version("1.0.0-ألفا..٢"), None);
        assert_eq!(parse_version("1.0.0-ألفا_ب"), None);
        assert_eq!(parse_version("1.0.0+بناء"), None);
    }

    #[test]
    fn compare_versions_triplet_then_prerelease() {
        assert_eq!(
            compare_versions(&version(1, 10, 0), &version(1, 2, 0)),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions(&version(1, 2, 3), &version(1, 2, 3)),
            Ordering::Equal
        );
        // المستقر أكبر من التجريبي بالثلاثية نفسها.
        let stable = version(1, 0, 0);
        let mut pre = version(1, 0, 0);
        pre.pre = vec![PreId::Str("ألفا".to_string())];
        assert_eq!(compare_versions(&stable, &pre), Ordering::Greater);
        assert_eq!(compare_versions(&pre, &stable), Ordering::Less);
        // الثلاثية أولا: تجريبي بثلاثية أعلى يفوز على مستقر أدنى.
        let mut pre2 = version(2, 0, 0);
        pre2.pre = vec![PreId::Str("ألفا".to_string())];
        assert_eq!(compare_versions(&pre2, &stable), Ordering::Greater);
        // رقمي أصغر من حرفي؛ ورقميان عدديا.
        let mut a = version(1, 0, 0);
        a.pre = vec![PreId::Num(2)];
        let mut b = version(1, 0, 0);
        b.pre = vec![PreId::Str("ألفا".to_string())];
        assert_eq!(compare_versions(&a, &b), Ordering::Less);
        let mut c = version(1, 0, 0);
        c.pre = vec![PreId::Num(10)];
        assert_eq!(compare_versions(&a, &c), Ordering::Less);
        // القائمة الأقصر أصغر عند تساوي البادئة.
        let mut d = version(1, 0, 0);
        d.pre = vec![PreId::Str("ألفا".to_string())];
        let mut e = version(1, 0, 0);
        e.pre = vec![
            PreId::Str("ألفا".to_string()),
            PreId::Num(1),
        ];
        assert_eq!(compare_versions(&d, &e), Ordering::Less);
    }

    #[test]
    fn locked_version_reads_pin_and_skips_comments() {
        let dir = temp_dir("lock");
        write(
            &dir,
            "قفل.حزمة",
            "# قفل حزم — يولَّد آليا؛ لا تحرره بيدك\nأداة-أ 1.2.3 github.com/مالك/أداة-أ 9f2c\n# تجاوز: أداة-ب 2.0.0\nأداة-ب 2.0.1 github.com/مالك/أداة-ب 41ab\n",
        );
        assert_eq!(
            locked_version(&dir, "أداة-أ").unwrap(),
            Some("1.2.3".to_string())
        );
        // سطر التجاوز تعليق — والسطر الرئيسي يحمل المحلول.
        assert_eq!(
            locked_version(&dir, "أداة-ب").unwrap(),
            Some("2.0.1".to_string())
        );
        assert_eq!(locked_version(&dir, "غائبة").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locked_version_missing_or_unreadable_is_no_pin() {
        let dir = temp_dir("lock-missing");
        assert_eq!(locked_version(&dir, "أداة").unwrap(), None);
        write(&dir, "قفل.حزمة", "نص ليس قفلا\nبلا إصدار هنا\n");
        assert_eq!(locked_version(&dir, "أداة").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locked_version_malformed_pin_is_error() {
        let dir = temp_dir("lock-bad");
        write(
            &dir,
            "قفل.حزمة",
            "أداة 1.2 github.com/مالك/أداة 9f2c\n",
        );
        let err = locked_version(&dir, "أداة").unwrap_err();
        assert!(err.contains("إصدار غير صالح في القفل"), "err: {}", err);
        assert!(err.contains("أداة"), "err: {}", err);
        assert!(err.contains("1.2"), "err: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newest_store_version_picks_max_with_entry_file() {
        let dir = temp_dir("store");
        write(&dir, ".نيفا/حزم/أداة-1.0.0/أداة.نيفا", "صدّر ق\nق = ١\n");
        write(&dir, ".نيفا/حزم/أداة-2.0.0/أداة.نيفا", "صدّر ق\nق = ٢\n");
        // تجريبي بثلاثية أعلى يفوز على مستقر أدنى.
        write(
            &dir,
            ".نيفا/حزم/أداة-3.0.0-ألفا/أداة.نيفا",
            "صدّر ق\nق = ٣\n",
        );
        // مجلد بلا ملف دخول لا يُحتسب ولو كان الأحدث.
        write(
            &dir,
            ".نيفا/حزم/أداة-9.9.9/مصدر/رئيسي.نيفا",
            "اطبع ٩\n",
        );
        // اسم آخر لا يتداخل.
        write(&dir, ".نيفا/حزم/أداة-أخرى-5.0.0/أداة-أخرى.نيفا", "صدّر ق\nق = ٥\n");
        assert_eq!(
            newest_store_version(&dir, "أداة"),
            Some("3.0.0-ألفا".to_string())
        );
        assert_eq!(
            newest_store_version(&dir, "أداة-أخرى"),
            Some("5.0.0".to_string())
        );
        assert_eq!(newest_store_version(&dir, "غائبة"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newest_store_version_stable_beats_prerelease_same_triplet() {
        let dir = temp_dir("store-pre");
        write(
            &dir,
            ".نيفا/حزم/أداة-1.0.0-ألفا/أداة.نيفا",
            "صدّر ق\nق = ١\n",
        );
        write(&dir, ".نيفا/حزم/أداة-1.0.0/أداة.نيفا", "صدّر ق\nق = ٢\n");
        assert_eq!(
            newest_store_version(&dir, "أداة"),
            Some("1.0.0".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_order_project_then_program_then_store() {
        let dir = temp_dir("order");
        write(&dir, "حزم/أداة/أداة.نيفا", "صدّر ق\nق = ١\n");
        write(&dir, "أداة.نيفا", "صدّر ق\nق = ٢\n");
        write(&dir, ".نيفا/حزم/أداة-9.0.0/أداة.نيفا", "صدّر ق\nق = ٣\n");
        // حزم/ أولا.
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join("حزم/أداة/أداة.نيفا"))
        );
        // بلا حزم/ — مجلد البرنامج قبل المخزن.
        std::fs::remove_dir_all(dir.join("حزم")).unwrap();
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(found.found, Some(dir.join("أداة.نيفا")));
        // بلا مجلد برنامج — المخزن (الأحدث).
        std::fs::remove_file(dir.join("أداة.نيفا")).unwrap();
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join(".نيفا/حزم/أداة-9.0.0/أداة.نيفا"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_lock_pin_beats_newest_and_missing_pin_fails() {
        let dir = temp_dir("pin");
        write(&dir, ".نيفا/حزم/أداة-1.0.0/أداة.نيفا", "صدّر ق\nق = ١\n");
        write(&dir, ".نيفا/حزم/أداة-2.0.0/أداة.نيفا", "صدّر ق\nق = ٢\n");
        // بلا قفل — الأحدث.
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join(".نيفا/حزم/أداة-2.0.0/أداة.نيفا"))
        );
        // قفل يثبّت 1.0.0 — يفوز على الأحدث.
        write(
            &dir,
            "قفل.حزمة",
            "# قفل حزم\nأداة 1.0.0 github.com/مالك/أداة 9f2c\n",
        );
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join(".نيفا/حزم/أداة-1.0.0/أداة.نيفا"))
        );
        // قفل يثبّت إصدارا غير مثبّت — لا رجوع إلى الأحدث.
        write(
            &dir,
            "قفل.حزمة",
            "# قفل حزم\nأداة 3.0.0 github.com/مالك/أداة 9f2c\n",
        );
        let found = find_module_file(&dir, "أداة").unwrap();
        assert_eq!(found.found, None);
        assert!(
            found
                .searched
                .iter()
                .any(|p| p.ends_with("أداة-3.0.0/أداة.نيفا")),
            "searched: {:?}",
            found.searched
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_home_sees_parent_store() {
        // اجتماع 47 §61: برنامج في `home/` يرى مخزن الأب.
        let dir = temp_dir("home-store");
        write(&dir, ".نيفا/حزم/أداة-1.1.0/أداة.نيفا", "صدّر ق\nق = ٤٣\n");
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let home = dir.join("home");
        // بلا قفل — الأحدث من مخزن الأب.
        let found = find_module_file(&home, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join(".نيفا/حزم/أداة-1.1.0/أداة.نيفا"))
        );
        // مخزن البيت نفسه أولا حين يوجد فيه الأحدث.
        write(
            &dir,
            "home/.نيفا/حزم/أداة-1.2.0/أداة.نيفا",
            "صدّر ق\nق = ٤٤\n",
        );
        let found = find_module_file(&home, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join("home/.نيفا/حزم/أداة-1.2.0/أداة.نيفا"))
        );
        // المقيَّد بالقفل يجده من مخزن الأب، ويرفض الأحدث.
        write(
            &dir,
            "home/قفل.حزمة",
            "# قفل حزم\nأداة 1.1.0 github.com/مالك/أداة 9f2c\n",
        );
        let found = find_module_file(&home, "أداة").unwrap();
        assert_eq!(
            found.found,
            Some(dir.join(".نيفا/حزم/أداة-1.1.0/أداة.نيفا"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_non_home_does_not_see_parent_store() {
        // اجتماع 47 §61: الرجوع لـ`home` وحده — مجلد
        // آخر لا يرى مخزن أبيه.
        let dir = temp_dir("no-home-store");
        write(&dir, ".نيفا/حزم/أداة-1.1.0/أداة.نيفا", "صدّر ق\nق = ٤٣\n");
        std::fs::create_dir_all(dir.join("فرعي")).unwrap();
        let found = find_module_file(&dir.join("فرعي"), "أداة").unwrap();
        assert_eq!(found.found, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_missing_lists_all_searched() {
        let dir = temp_dir("missing");
        let found = find_module_file(&dir, "لاوجود").unwrap();
        assert_eq!(found.found, None);
        assert_eq!(found.searched.len(), 3);
        assert!(found.searched[0].ends_with("حزم/لاوجود/لاوجود.نيفا"));
        assert!(found.searched[1].ends_with("لاوجود.نيفا"));
        assert!(found.searched[2].ends_with(".نيفا/حزم"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_module_file_rejects_path_separators() {
        let dir = temp_dir("seps");
        write(&dir, "أداة.نيفا", "صدّر ق\nق = ١\n");
        let found = find_module_file(&dir, "أداة/شر").unwrap();
        assert_eq!(found.found, None);
        let found = find_module_file(&dir, "..").unwrap();
        assert_eq!(found.found, None);
        let found = find_module_file(&dir, "").unwrap();
        assert_eq!(found.found, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
