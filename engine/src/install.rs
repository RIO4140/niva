//! تثبيت أدوات نيفا — أمر `تثبيت` بالاسم الكامل (بلا كود
//! حرفين): ينزّل أداة/وحدة `.نيفا` من رابط أو مسار محلي
//! ويضعها في مسار الاستيراد (مجلد البرنامج)، فيقدر
//! `استورد` يستوردها بعدها. تنفيذ واحد مشترك للمحركين
//! (المفسر + المترجم) — لا يُنفَّذ مرتين أبدا.
//!
//! الحدود (أوامر المالك): تيرمكس إلهام للميزات فقط —
//! نيفا لا تثبّت شيئا في تيرمكس أبدا: لا pkg ولا apt ولا
//! تشغيل أي مدير حزم، ولا أدوات لينكس الخارجية ولا
//! ثنائيات أجنبية ولا chmod ولا إدارة PATH. تثبيت
//! لأدوات نيفا فقط (ملفات `.نيفا` مكتوبة بنيفا).
//! والتثبيت بالاسم (سجل حزم) خطوة لاحقة — ليست هنا.
//!
//! القواعد المطبقة هنا:
//! - المصدر رابط http/https: يُنزَّل عبر `شبكة.اجلب`
//!   نفسها — فتسري بوابة الشبكة (بلا سماح يرفض
//!   بخطأ عربي صادق ولا تُتجاوز البوابة أبدا) وقواعد
//!   `اجلب` نفسها (لا عناوين خاصة، وسقف 5 ميغابايت).
//! - المصدر مسار محلي: يُقرأ عبر سياسة الملفات نفسها
//!   (`resolve_in` بمجلد الجلسة — كـ `اقرأ_ملف`)،
//!   وبسقف القراءة نفسه (5 ميغابايت) ونص UTF-8 فقط.
//! - الاسم من آخر مقطع في الرابط أو من اسم المسار
//!   المحلي؛ ويجب أن ينتهي بـ `.نيفا`، وبلا اجتياز
//!   مسار (`..` و`/` و`\` مرفوضة) — والملف المثبّت
//!   يذهب إلى مجلد البرنامج فقط.
//! - بلا كتابة صامتة فوق موجود: وحدة بالاسم نفسه
//!   مثبّتة أصلا تُرفض بخطأ عربي صادق (التحديث مهمة
//!   لاحقة). واسم وحدة أصلية (رياضيات، شبكة، ...) لا
//!   يُثبَّت أبدا — فـ `استورد` يقدّم الأصلية على
//!   الملف، والملف المثبّت بهذا الاسم لا يُستورد أبدا.
//! - الكتابة تحت حصة التخزين نفسها (100 ميغابايت)
//!   كـ `اكتب_ملف`، وبلا نص خطأ نظام تشغيل إنجليزي
//!   إطلاقا في أي رسالة من هذه الوحدة.

use crate::error::{ErrorKind, RioError, RioResult};
use crate::permissions::FilePolicy;
use crate::value::Value;
use std::path::Path;

/// حصة التخزين لمجلد البرنامج (100 ميغابايت) — نفس
/// الثابت ونفس المشي كفحص المحركين، فلا تنحرف الرسالة
/// عن `اكتب_ملف` أبدا (نمط `files.rs`).
fn check_disk_quota(
    base: &Path,
    extra_bytes: u64,
    line: usize,
    col: usize,
) -> RioResult<()> {
    let mut budget = 10000usize;
    let used = crate::interpreter::Interpreter::dir_size(base, &mut budget);
    if used.saturating_add(extra_bytes) > crate::interpreter::Interpreter::DISK_QUOTA_BYTES
    {
        return Err(RioError::new(
            ErrorKind::Io,
            "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة",
            line,
            col,
        ));
    }
    Ok(())
}

/// اسم ملف وحدة صالح للتثبيت؟ يعيد الجذع (اسم الوحدة
/// كما يستورده `استورد` — بلا `.نيفا`) أو خطأ عربيا.
fn validate_module_filename(name: &str, line: usize, col: usize) -> RioResult<String> {
    let stem = match name.strip_suffix(".نيفا") {
        Some(s) => s,
        None => {
            return Err(RioError::new(
                ErrorKind::Io,
                "تثبيت يقبل ملفات .نيفا فقط",
                line,
                col,
            ))
        }
    };
    if stem.is_empty()
        || stem == "."
        || stem == ".."
        || stem.contains('/')
        || stem.contains('\\')
    {
        return Err(RioError::new(
            ErrorKind::Io,
            "تثبيت يقبل ملفات .نيفا فقط",
            line,
            col,
        ));
    }
    Ok(stem.to_string())
}

/// يفك ترميز النسبة المئوية في مقطع واحد من الرابط
/// (`%D8%A3` إلى الحرف العربي) — فالروابط العربية
/// تصل مقاطعها مرمّزة عادة، والاسم الصحيح هو المفكوك.
/// التسلسلات التالفة تبقى كما هي حرفيا، فيرفضها
/// التحقق بعدها إن لزم (فلا اسم مختلق أبدا).
fn percent_decode(segment: &str) -> String {
    fn hex_digit(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|d| d as u8)
    }
    let bytes = segment.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = if bytes[i] == b'%' && i + 2 < bytes.len() {
            match (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                (Some(hi), Some(lo)) => Some((hi << 4) | lo),
                _ => None,
            }
        } else {
            None
        };
        if let Some(byte) = pair {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// اسم الملف من رابط: آخر مقطع بعد `/`، بلا استعلام
/// (`?...`) وبلا جزء (`#...`)، مع فك ترميز النسبة
/// المئوية — فالاسم من رابط عربي مرمّز هو الاسم العربي
/// نفسه، لا السلسلة المرمّزة.
fn module_name_from_url(url: &str, line: usize, col: usize) -> RioResult<String> {
    let no_fragment = url.split('#').next().unwrap_or(url);
    let no_query = no_fragment.split('?').next().unwrap_or(no_fragment);
    let last = no_query.rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        return Err(RioError::new(
            ErrorKind::Io,
            "الرابط لا يشير إلى ملف .نيفا",
            line,
            col,
        ));
    }
    let decoded = percent_decode(last);
    match validate_module_filename(&decoded, line, col) {
        Ok(stem) => Ok(stem),
        Err(_) => Err(RioError::new(
            ErrorKind::Io,
            "الرابط لا يشير إلى ملف .نيفا",
            line,
            col,
        )),
    }
}

/// ثبّت أداة نيفا من رابط أو مسار محلي إلى مجلد البرنامج.
/// يعيد نص تأكيد («ثُبّتت الوحدة {الاسم}») يطبعه الطرفي.
/// `policy` و`cwd` هما سياسة الجلسة ومجلدها (للمصدر
/// المحلي)؛ و`program_dir` هو مسار الاستيراد (للهدف).
pub(crate) fn install_module(
    policy: &FilePolicy,
    cwd: &Path,
    program_dir: &Path,
    source: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    // رابط أم مسار محلي؟ مخطط غير http/https يرفض
    // برسالة واضحة بدلا من أن يُقرأ كمسار غائب.
    let is_url = {
        let lower = source.to_ascii_lowercase();
        lower.starts_with("http://") || lower.starts_with("https://")
    };
    if source.contains("://") && !is_url {
        return Err(RioError::new(
            ErrorKind::Io,
            "تثبيت يقبل روابط http وhttps فقط",
            line,
            col,
        ));
    }
    let (stem, content): (String, String) = if is_url {
        let stem = module_name_from_url(source, line, col)?;
        let body = crate::network::fetch(source, line, col)?;
        match body {
            Value::Str(text) => (stem, text),
            _ => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    "الرابط لا يشير إلى ملف .نيفا",
                    line,
                    col,
                ))
            }
        }
    } else {
        let resolved = policy.resolve_in(cwd, source, line, col)?;
        let display = policy.display_str(source);
        match std::fs::metadata(&resolved) {
            Ok(m) if m.is_dir() => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!("ليس ملفا: {}", display),
                    line,
                    col,
                ))
            }
            Ok(_) => {}
            Err(_) => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!("الملف غير موجود: {}", display),
                    line,
                    col,
                ))
            }
        }
        let name = match resolved.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    "تثبيت يقبل ملفات .نيفا فقط",
                    line,
                    col,
                ))
            }
        };
        let stem = validate_module_filename(&name, line, col)?;
        let text = crate::interpreter::Interpreter::read_file_capped(
            &resolved, &display, line, col,
        )?;
        (stem, text)
    };
    // اسم وحدة أصلية لا يُثبَّت: `استورد` يقدّم الأصلية
    // على الملف دائما، فالملف المثبّت بهذا الاسم لا
    // يُستورد أبدا — رفض صريح خير من تثبيت بلا أثر.
    if crate::stdlib::native_module(&stem).is_some() {
        return Err(RioError::new(
            ErrorKind::Io,
            format!("{} وحدة أصلية — لا يمكن تثبيت وحدة بهذا الاسم", stem),
            line,
            col,
        ));
    }
    // بلا كتابة صامتة فوق موجود — التحديث مهمة لاحقة.
    let target = program_dir.join(format!("{}.نيفا", stem));
    if target.exists() {
        return Err(RioError::new(
            ErrorKind::Io,
            format!(
                "الوحدة {} مثبّتة أصلا — احذف الملف أولا لإعادة تثبيتها",
                stem
            ),
            line,
            col,
        ));
    }
    check_disk_quota(program_dir, content.len() as u64, line, col)?;
    std::fs::write(&target, &content).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر تثبيت الوحدة {}", stem),
            line,
            col,
        )
    })?;
    Ok(Value::Str(format!("ثُبّتت الوحدة {}", stem)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// قفل تسلسل اختبارات التثبيت: الشبكة علم عام
    /// (والاختبارات المتوازية تتشاركه)، والكتابة في
    /// مجلدات مؤقتة فريدة لكل اختبار — نمط NET_TEST_LOCK.
    static INSTALL_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn install_test_lock() -> std::sync::MutexGuard<'static, ()> {
        INSTALL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn test_policy(dir: &Path) -> FilePolicy {
        FilePolicy::restricted(dir.to_path_buf())
    }

    fn test_dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir()
            .join(format!("niva-install-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("مصادر")).unwrap();
        (base.clone(), base.join("مصادر"))
    }

    fn cleanup(base: &Path) {
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn install_name_from_url_ok() {
        assert_eq!(
            module_name_from_url("http://مثال.مصر/أدوات/أداة.نيفا", 1, 1).unwrap(),
            "أداة"
        );
        assert_eq!(
            module_name_from_url("https://مثال.مصر/أداة.نيفا?إصدار=٢#جزء", 1, 1).unwrap(),
            "أداة"
        );
        assert_eq!(
            module_name_from_url("https://مثال.مصر/tool.نيفا", 1, 1).unwrap(),
            "tool"
        );
    }

    #[test]
    fn install_name_from_url_rejected() {
        assert!(module_name_from_url("https://مثال.مصر/", 1, 1).is_err());
        assert!(module_name_from_url("https://مثال.مصر/أداة.txt", 1, 1).is_err());
        assert!(module_name_from_url("https://مثال.مصر/.نيفا", 1, 1).is_err());
        assert!(module_name_from_url("https://مثال.مصر/..نيفا", 1, 1).is_err());
        // مرمّز بلا لاحقة .نيفا بعد الفك — مرفوض.
        assert!(module_name_from_url(
            "https://مثال.مصر/%D8%A3%D8%AF%D8%A7%D8%A9.txt",
            1,
            1
        )
        .is_err());
    }

    #[test]
    fn install_name_from_url_percent_decoded() {
        // الرابط العربي يصل مقطعه مرمّزا عادة — الاسم
        // بعد الفك هو «أداة»، لا السلسلة المرمّزة.
        assert_eq!(
            module_name_from_url(
                "http://127.0.0.1:8765/%D8%A3%D8%AF%D8%A7%D8%A9.%D9%86%D9%8A%D9%81%D8%A7",
                1,
                1
            )
            .unwrap(),
            "أداة"
        );
        // الترميز المختلط (لاتيني + عربي مرمّز) يفك أيضا.
        assert_eq!(
            percent_decode("tool-%D8%A3.نيفا"),
            "tool-أ.نيفا"
        );
        // تسلسل تالف يبقى حرفيا — فلا اسم مختلق.
        assert_eq!(percent_decode("%ZZ%2.نيفا"), "%ZZ%2.نيفا");
        // زوج سداسي صحيح لكنه بايت UTF-8 ناقص وحده —
        // يفك إلى بايت تالف فيستبدل به (خسارة آمنة).
        assert_eq!(percent_decode("%D8.نيفا"), "�.نيفا");
    }

    #[test]
    fn install_validate_filename() {
        assert_eq!(validate_module_filename("أداة.نيفا", 1, 1).unwrap(), "أداة");
        assert!(validate_module_filename("أداة.txt", 1, 1).is_err());
        assert!(validate_module_filename(".نيفا", 1, 1).is_err());
        assert!(validate_module_filename("أ/ب.نيفا", 1, 1).is_err());
        assert!(validate_module_filename("أ\\ب.نيفا", 1, 1).is_err());
    }

    #[test]
    fn install_local_path_then_already_installed() {
        let _guard = install_test_lock();
        let (base, src_dir) = test_dirs("local");
        let policy = test_policy(&base);
        std::fs::write(
            src_dir.join("أداة.نيفا"),
            "صدر قيمة\nقيمة = ٤٢\n",
        )
        .unwrap();
        let got = install_module(
            &policy,
            &base,
            &base,
            "مصادر/أداة.نيفا",
            1,
            1,
        )
        .unwrap();
        assert_eq!(got, Value::Str("ثُبّتت الوحدة أداة".to_string()));
        assert_eq!(
            std::fs::read_to_string(base.join("أداة.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        // الثانية مرفوضة: بلا كتابة صامتة فوق موجود.
        let err = install_module(&policy, &base, &base, "مصادر/أداة.نيفا", 1, 1)
            .unwrap_err();
        assert!(
            err.to_string().contains("مثبّتة أصلا"),
            "got: {}",
            err
        );
        // والمحتوى الأول بقي كما هو — بلا استبدال.
        assert_eq!(
            std::fs::read_to_string(base.join("أداة.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        cleanup(&base);
    }

    #[test]
    fn install_local_rejections() {
        let _guard = install_test_lock();
        let (base, src_dir) = test_dirs("reject");
        let policy = test_policy(&base);
        std::fs::write(src_dir.join("نص.txt"), "نص").unwrap();
        std::fs::write(src_dir.join("أداة.نيفا"), "صدر قيمة\nقيمة = ١\n").unwrap();
        // ليس .نيفا.
        let err = install_module(&policy, &base, &base, "مصادر/نص.txt", 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains(".نيفا"), "got: {}", err);
        // خارج مجلد البرنامج.
        let err = install_module(&policy, &base, &base, "../خارج.نيفا", 1, 1)
            .unwrap_err();
        assert!(
            err.to_string().contains("خارج مجلد البرنامج"),
            "got: {}",
            err
        );
        // مجلد لا ملف.
        let err = install_module(&policy, &base, &base, "مصادر", 1, 1).unwrap_err();
        assert!(err.to_string().contains("ليس ملفا"), "got: {}", err);
        // ملف غائب.
        let err =
            install_module(&policy, &base, &base, "مصادر/غائب.نيفا", 1, 1)
                .unwrap_err();
        assert!(
            err.to_string().contains("غير موجود"),
            "got: {}",
            err
        );
        // اسم وحدة أصلية لا يُثبَّت.
        let err = install_module(&policy, &base, &base, "مصادر/أداة.نيفا", 1, 1)
            .unwrap();
        assert_eq!(err, Value::Str("ثُبّتت الوحدة أداة".to_string()));
        // (رياضيات أصلية — المصدر باسمها يُرفض.)
        std::fs::write(src_dir.join("رياضيات.نيفا"), "صدر س\nس = ١\n").unwrap();
        let err =
            install_module(&policy, &base, &base, "مصادر/رياضيات.نيفا", 1, 1)
                .unwrap_err();
        assert!(
            err.to_string().contains("وحدة أصلية"),
            "got: {}",
            err
        );
        cleanup(&base);
    }

    #[test]
    fn install_non_http_scheme_rejected() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("scheme");
        let policy = test_policy(&base);
        let err = install_module(
            &policy,
            &base,
            &base,
            "ftp://مثال.مصر/أداة.نيفا",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("http وhttps"),
            "got: {}",
            err
        );
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_url_without_network_refused() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("nogate");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(false);
        let err = install_module(
            &policy,
            &base,
            &base,
            "http://127.0.0.1:9/أداة.نيفا",
            1,
            1,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains(crate::network::PERMISSION_ERROR_MARKER),
            "got: {}",
            err
        );
        assert!(!base.join("أداة.نيفا").exists());
        cleanup(&base);
    }
}
