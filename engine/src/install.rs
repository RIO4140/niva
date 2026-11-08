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
//! المصادر المقبولة:
//! - رابط http/https مباشر لملف `.نيفا` (يُنزَّل عبر
//!   `شبكة.اجلب` نفسها — فتسري بوابة الشبكة وقواعد
//!   `اجلب` نفسها: لا عناوين خاصة، وسقف 5 ميغابايت).
//! - مسار محلي (يُقرأ عبر سياسة الملفات نفسها).
//! - رابط مستودع جيت هاب `github.com/<مالك>/<مستودع>`
//!   (ومسار فرعي `/tree/<فرع>/<مسار>` إن ورد): يُجلب
//!   سرد المحتويات من واجهة جيت هاب عبر `اجلب` نفسها؛
//!   فإن وُجد ملف `.نيفا` واحد ثُبّت وحده، وملفان
//!   فأكثر ثُبّتت كلها مرة واحدة (تثبيت-الكل: المستوى
//!   الأعلى فقط، سقف 20 ملفا و5 ميغابايت إجمالا،
//!   والمتعارض مع مثبَّت بنفس الاسم يُتخطَّى مع ملاحظة)،
//!   ولا ملف يُرفض صراحة. ومسار يشير إلى ملف `.نيفا`
//!   مفرد يُثبَّت مباشرة.
//! - رابط إصدار جيت هاب `github.com/<مالك>/<مستودع>
//!   /releases/tag/<وسم>`: تُجلب أصول الإصدار من الواجهة؛
//!   فإن وُجد أصل `.نيفا` واحد نُزّل من رابطه المباشر
//!   وثُبّت، وأكثر من واحد ثُبّتت كلها مرة واحدة
//!   (تثبيت-الكل كما في المستودع)، ولا أصل يُرفض صراحة.
//! - روابط github.com بصيغ أخرى (blob/pull/issues) مرفوضة
//!   صراحة — فلا يُنزَّل HTML صفحة كملف `.نيفا` أبدا.
//!
//! قاعدة واجهة جيت هاب: `https://api.github.com` افتراضيا؛
//! ومتغير البيئة `NIVA_GITHUB_API_BASE` يوجّهها إلى قاعدة
//! أخرى (للاختبار المحلي بخادم محاكٍ). وقيمة بلا مخطط
//! http/https مرفوضة صراحة.
//!
//! تنزيل أصل إصدار يمر عبر إعادة توجيه من github.com
//! إلى مضيف التنزيل (release-assets)؛ وسياسة الشبكة
//! تسمح بهذا التحويل وحده بقائمة سماح اجتماع 28
//! (§43): المخطط https والمضيف مطابق حرفيا والمنفذ
//! 443 — وما عداه من تحويل إلى مضيف مختلف مرفوض كما
//! كان. والمساران البديلان يعملان أيضا: رابط المستودع
//! والرابط المباشر للملف.
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
//! - بعد نجاح تثبيت برابط (مباشر أو مستودع أو إصدار)
//!   يُطبع سطر ثانٍ «بصمة الملف: <بصمة SHA-256>» —
//!   بصمة المحتوى المكتوب نفسه (فكرة الثقة 7)؛
//!   والتثبيت من مسار محلي بلا بصمة (الملف عند
//!   المستخدم أصلا).
//! - الكتابة تحت حصة التخزين نفسها (100 ميغابايت)
//!   كـ `اكتب_ملف`، وبلا نص خطأ نظام تشغيل إنجليزي
//!   إطلاقا في أي رسالة من هذه الوحدة.
//! - الأداة المثبّتة برنامج نيفا عادي في مسار الاستيراد
//!   فقط — لا مساس بالمكتبة الأصلية ولا بالمحرك، ولا
//!   أمر تشغيل جديد في الكود: يشغّلها المستخدم بأوامرها
//!   هي (`نيفا <اسم>.نيفا` أو `استورد` أو أمر التفاعلي
//!   `شغّل <اسم>.نيفا`).

use crate::device::{parse_json, Json};
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

/// قاعدة واجهة جيت هاب الافتراضية.
const GITHUB_API_BASE: &str = "https://api.github.com";

/// قاعدة واجهة جيت هاب: متغير البيئة `NIVA_GITHUB_API_BASE`
/// يوجّهها إلى قاعدة أخرى (للاختبار المحلي بخادم محاكٍ)؛
/// وإلا الافتراضية. وقيمة بلا مخطط http/https مرفوضة صراحة.
fn github_api_base(line: usize, col: usize) -> RioResult<String> {
    match std::env::var("NIVA_GITHUB_API_BASE") {
        Ok(base) if !base.trim().is_empty() => {
            let base = base.trim().trim_end_matches('/').to_string();
            let lower = base.to_ascii_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") {
                Ok(base)
            } else {
                Err(RioError::new(
                    ErrorKind::Io,
                    "قاعدة واجهة جيت هاب غير صالحة",
                    line,
                    col,
                ))
            }
        }
        _ => Ok(GITHUB_API_BASE.to_string()),
    }
}

/// يرمّز مقطعا واحدا من مسار أو استعلام بالنسب المئوية
/// (كل بايت غير آمن يصير `%XX` بأحرف كبيرة) — فالمقاطع
/// العربية في روابط الواجهة تصل سليمة، والشرطات المائلة
/// في قيمة `ref` تُرمَّز فلا تنكسر بنية المسار.
fn percent_encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for &b in segment.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// اسم مالك أو مستودع صالح في جيت هاب (بعد فك الترميز):
/// أحرف وأرقام و`-` و`_` و`.` فقط، وأوله حرف أو رقم،
/// وبلا `..` — فلا يخرج الاسم من مسار الواجهة أبدا.
fn validate_github_name(name: &str, line: usize, col: usize) -> RioResult<String> {
    let decoded = percent_decode(name);
    let invalid = decoded.is_empty()
        || decoded == "."
        || decoded == ".."
        || decoded.contains("..")
        || !decoded
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
        || !decoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if invalid {
        return Err(RioError::new(
            ErrorKind::Io,
            "رابط جيت هاب غير صالح",
            line,
            col,
        ));
    }
    Ok(decoded)
}

/// مصدر جيت هاب بعد تصنيف الرابط.
#[derive(Debug)]
enum GithubSource {
    /// مستودع: مالك، مستودع، فرع (بلا فرع = الافتراضي)،
    /// مسار (بلا مسار = الجذر). وفرع فيه شرطة مائلة لا
    /// يُميَّز عن المسار في الرابط — فيُقرأ أول مقطع بعد
    /// `tree` فرعا والباقي مسارا (حد موثق).
    Repo {
        owner: String,
        repo: String,
        branch: Option<String>,
        path: String,
    },
    /// إصدار: مالك، مستودع، وسم (وشرطات الوسم المائلة
    /// تُرمَّز في مسار الواجهة فتصل سليمة).
    Release {
        owner: String,
        repo: String,
        tag: String,
    },
}

/// صنّف رابطا: جيت هاب أم لا؟ يعيد None لغير جيت هاب
/// (فيسلك مسار الرابط المباشر أو المحلي القديم).
/// وروابط github.com بصيغ غير معروفة (blob/pull/issues
/// وغيرها) مرفوضة صراحة — فلا يُنزَّل HTML صفحة كملف
/// `.نيفا` أبدا.
fn classify_github_url(
    source: &str,
    line: usize,
    col: usize,
) -> RioResult<Option<GithubSource>> {
    let lower = source.to_ascii_lowercase();
    let after_scheme = if lower.starts_with("https://") {
        &source["https://".len()..]
    } else if lower.starts_with("http://") {
        &source["http://".len()..]
    } else {
        return Ok(None);
    };
    // افصل المضيف عن المسار (بلا استعلام وبلا جزء)،
    // واحفظ حالة الأحرف في المقاطع من الأصل.
    let no_fragment = after_scheme.split('#').next().unwrap_or(after_scheme);
    let no_query = no_fragment.split('?').next().unwrap_or(no_fragment);
    let (host, path) = match no_query.find('/') {
        Some(i) => (&no_query[..i], &no_query[i..]),
        None => (no_query, ""),
    };
    let host = host
        .strip_prefix("www.")
        .unwrap_or(host)
        .to_ascii_lowercase();
    if host != "github.com" {
        return Ok(None);
    }
    let bad = || {
        RioError::new(ErrorKind::Io, "رابط جيت هاب غير صالح", line, col)
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(bad());
    }
    let owner = validate_github_name(segments[0], line, col)?;
    let repo = validate_github_name(segments[1], line, col)?;
    match segments.get(2) {
        None => Ok(Some(GithubSource::Repo {
            owner,
            repo,
            branch: None,
            path: String::new(),
        })),
        Some(&"tree") => {
            let branch = match segments.get(3) {
                Some(b) => percent_decode(b),
                None => return Err(bad()),
            };
            if branch.is_empty() {
                return Err(bad());
            }
            let path = segments[4..]
                .iter()
                .map(|s| percent_decode(s))
                .collect::<Vec<_>>()
                .join("/");
            Ok(Some(GithubSource::Repo {
                owner,
                repo,
                branch: Some(branch),
                path,
            }))
        }
        Some(&"releases") => {
            if segments.get(3) != Some(&"tag") || segments.len() < 5 {
                return Err(RioError::new(
                    ErrorKind::Io,
                    "رابط جيت هاب غير مدعوم — استخدم رابط مستودع أو إصدار أو رابطا مباشرا لملف .نيفا",
                    line,
                    col,
                ));
            }
            let tag = segments[4..]
                .iter()
                .map(|s| percent_decode(s))
                .collect::<Vec<_>>()
                .join("/");
            if tag.is_empty() {
                return Err(bad());
            }
            Ok(Some(GithubSource::Release { owner, repo, tag }))
        }
        Some(_) => Err(RioError::new(
            ErrorKind::Io,
            "رابط جيت هاب غير مدعوم — استخدم رابط مستودع أو إصدار أو رابطا مباشرا لملف .نيفا",
            line,
            col,
        )),
    }
}

/// حقل من كائن JSON أو بلا شيء.
fn json_field<'a>(json: &'a Json, key: &str) -> Option<&'a Json> {
    match json {
        Json::Obj(pairs) => pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v),
        _ => None,
    }
}

/// نص قيمة JSON أو بلا شيء.
fn json_str(json: &Json) -> Option<&str> {
    match json {
        Json::Str(s) => Some(s),
        _ => None,
    }
}

/// اجلب رد واجهة جيت هاب وحلّله JSON — عبر `اجلب` نفسها
/// فتسري بوابة الشبكة وقواعدها (ورمز حالة غير 2xx يرفضه
/// `اجلب` برسالة عربية قبل أن يصل الرد هنا).
fn fetch_github_json(
    api_base: &str,
    api_path: &str,
    line: usize,
    col: usize,
) -> RioResult<Json> {
    let url = format!("{}{}", api_base, api_path);
    let body = match crate::network::fetch(&url, line, col)? {
        Value::Str(text) => text,
        _ => {
            return Err(RioError::new(
                ErrorKind::Io,
                "رد جيت هاب غير مفهوم",
                line,
                col,
            ))
        }
    };
    parse_json(&body).map_err(|_| {
        RioError::new(ErrorKind::Io, "رد جيت هاب غير مفهوم", line, col)
    })
}

/// ملفات `.نيفا` في رد قائمة محتويات (مصفوفة): الاسم
/// ورابط التنزيل لكل ملف (المجلدات بلا رابط تُتخطى).
fn niva_files_in_listing(json: &Json, line: usize, col: usize) -> RioResult<Vec<(String, String)>> {
    let items = match json {
        Json::Arr(items) => items,
        _ => {
            return Err(RioError::new(
                ErrorKind::Io,
                "رد جيت هاب غير مفهوم",
                line,
                col,
            ))
        }
    };
    let mut out = Vec::new();
    for item in items {
        let name = json_field(item, "name").and_then(json_str);
        let kind = json_field(item, "type").and_then(json_str);
        let url = json_field(item, "download_url").and_then(json_str);
        if let (Some(name), Some("file"), Some(url)) = (name, kind, url) {
            if name.ends_with(".نيفا") {
                out.push((name.to_string(), url.to_string()));
            }
        }
    }
    Ok(out)
}

/// عدد بأرقام مشرقية لرسائل تثبيت-الكل — نفس تنسيق
/// مخرجات المحرك (أرقام `اطبع` المشرقية).
fn eastern_number(n: usize) -> String {
    Value::Number(n as f64).fmt_value()
}

/// سقف ملفات تثبيت-الكل: يُفحص من عدد السرد قبل أي
/// تنزيل — فلا يبدأ تثبيت ضخم أبدا.
const INSTALL_ALL_MAX_FILES: usize = 20;
/// سقف بايتات تثبيت-الكل إجمالا (5 ميغابايت) على مجموع
/// البايتات المنزَّلة — بلا احتساب المتخطَّى (لا يُنزَّل).
const INSTALL_ALL_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// خطأ توقف في تثبيت-الكل: يسمّي الملف وعدد ما ثُبّت
/// قبله — فإعادة التشغيل تعرف أين توقفت وتُكمل الناقص.
fn install_all_stop(
    stem: &str,
    installed: usize,
    reason: &str,
    line: usize,
    col: usize,
) -> RioError {
    RioError::new(
        ErrorKind::Io,
        format!(
            "تعذّر تثبيت {} (ثُبّت {} قبله): {}",
            stem,
            eastern_number(installed),
            reason
        ),
        line,
        col,
    )
}

/// ثبّت ملفات من سرد (مستودع أو إصدار): صفر يُرفض
/// صراحة، وواحد يسلك مسار الملف المفرد حرفيا،
/// واثنان فأكثر يسلكان مسار تثبيت-الكل.
fn install_listed_files(
    files: Vec<(String, String)>,
    where_ar: &str,
    program_dir: &Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    match files.len() {
        0 => Err(RioError::new(
            ErrorKind::Io,
            format!("لا يوجد ملف .نيفا {}", where_ar),
            line,
            col,
        )),
        1 => {
            let (name, url) = files.into_iter().next().unwrap();
            install_github_file(&name, &url, program_dir, line, col)
        }
        _ => install_all_files(files, program_dir, line, col),
    }
}

/// ثبّت كل ملفات `.نيفا` في سرد (مستودع أو إصدار):
/// المستوى الأعلى فقط، بترتيب السرد. المتعارض مع
/// مثبَّت بنفس الاسم يُتخطَّى مع ملاحظة ويُثبَّت
/// الباقي؛ وأول فشل تنزيل/كتابة أو تجاوز للسقف
/// الإجمالي يوقف التثبيت بخطأ يسمّي الملف وعدد ما
/// ثُبّت قبله، وما ثُبّت يبقى مثبَّتا. والملخص سطر
/// واحد بالأرقام المشرقية؛ وإن كان المثبَّت صفرا
/// (الكل متخطَّى) فخطأ صريح بدل نجاح صامت.
fn install_all_files(
    files: Vec<(String, String)>,
    program_dir: &Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if files.len() > INSTALL_ALL_MAX_FILES {
        return Err(RioError::new(
            ErrorKind::Io,
            format!(
                "تثبيت-الكل يقبل {} ملفا على الأكثر (وجد {})",
                eastern_number(INSTALL_ALL_MAX_FILES),
                eastern_number(files.len())
            ),
            line,
            col,
        ));
    }
    let total = files.len();
    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut total_bytes: u64 = 0;
    let mut lines: Vec<String> = Vec::new();
    for (name, url) in &files {
        let stem = match validate_module_filename(name, line, col) {
            Ok(stem) => stem,
            Err(e) => {
                return Err(install_all_stop(name, installed, &e.message, line, col))
            }
        };
        // متعارض مع مثبَّت بنفس الاسم: يُتخطَّى بلا
        // تنزيل وبلا كتابة — الملف المثبَّت لا يُمس.
        if program_dir.join(format!("{}.نيفا", stem)).exists() {
            skipped += 1;
            lines.push(format!("تُخطَّى {}: مثبّتة أصلا", stem));
            continue;
        }
        let body = match crate::network::fetch(url, line, col) {
            Ok(Value::Str(text)) => text,
            Ok(_) => {
                return Err(install_all_stop(
                    &stem,
                    installed,
                    "الرابط لا يشير إلى ملف .نيفا",
                    line,
                    col,
                ))
            }
            Err(e) => {
                return Err(install_all_stop(&stem, installed, &e.message, line, col))
            }
        };
        let body_len = body.len() as u64;
        if total_bytes + body_len > INSTALL_ALL_MAX_BYTES {
            return Err(RioError::new(
                ErrorKind::Io,
                format!(
                    "تجاوز سقف {} ميغابايت عند {} (ثُبّت {} قبله)",
                    eastern_number(5),
                    stem,
                    eastern_number(installed)
                ),
                line,
                col,
            ));
        }
        match write_installed_module(program_dir, stem.clone(), body, true, line, col) {
            Ok(Value::Str(msg)) => {
                lines.push(msg);
                installed += 1;
                total_bytes += body_len;
            }
            Ok(_) => {
                return Err(install_all_stop(
                    &stem,
                    installed,
                    "رد غير مفهوم",
                    line,
                    col,
                ))
            }
            Err(e) => {
                return Err(install_all_stop(&stem, installed, &e.message, line, col))
            }
        }
    }
    if installed == 0 {
        return Err(RioError::new(
            ErrorKind::Io,
            "كل الملفات مثبّتة أصلا",
            line,
            col,
        ));
    }
    lines.push(format!(
        "ثُبّت {} من {} (تُخطَّى {})",
        eastern_number(installed),
        eastern_number(total),
        eastern_number(skipped)
    ));
    Ok(Value::Str(lines.join("\n")))
}

/// نزّل ملف `.نيفا` من رابطه وثبّته في مسار الاستيراد
/// (الاسم من القائمة — لا من الرابط — فلا يختلفا).
fn install_github_file(
    name: &str,
    url: &str,
    program_dir: &Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let stem = validate_module_filename(name, line, col)?;
    let body = match crate::network::fetch(url, line, col)? {
        Value::Str(text) => text,
        _ => {
            return Err(RioError::new(
                ErrorKind::Io,
                "الرابط لا يشير إلى ملف .نيفا",
                line,
                col,
            ))
        }
    };
    // رابط جيت هاب — فتُطبع البصمة مع رسالة النجاح.
    write_installed_module(program_dir, stem, body, true, line, col)
}

/// ثبّت من مصدر جيت هاب (مستودع أو إصدار) بعد تصنيفه.
fn install_from_github(
    api_base: &str,
    program_dir: &Path,
    gh: GithubSource,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    match gh {
        GithubSource::Repo {
            owner,
            repo,
            branch,
            path,
        } => {
            let mut api_path = format!(
                "/repos/{}/{}/contents",
                percent_encode_segment(&owner),
                percent_encode_segment(&repo)
            );
            if !path.is_empty() {
                let encoded: Vec<String> =
                    path.split('/').map(percent_encode_segment).collect();
                api_path.push('/');
                api_path.push_str(&encoded.join("/"));
            }
            if let Some(branch) = &branch {
                api_path.push_str("?ref=");
                api_path.push_str(&percent_encode_segment(branch));
            }
            let json = fetch_github_json(api_base, &api_path, line, col)?;
            let where_ar = if path.is_empty() {
                "في هذا المستودع"
            } else {
                "في هذا المسار من المستودع"
            };
            // قائمة مجلد أم ملف مفرد؟ المسار إلى ملف يعيد
            // كائنا واحدا — فيُثبَّت مباشرة إن كان `.نيفا`.
            let files = match &json {
                Json::Arr(_) => niva_files_in_listing(&json, line, col)?,
                Json::Obj(_) => {
                    let name = json_field(&json, "name").and_then(json_str);
                    let kind = json_field(&json, "type").and_then(json_str);
                    let url = json_field(&json, "download_url").and_then(json_str);
                    match (name, kind, url) {
                        (Some(name), Some("file"), Some(url))
                            if name.ends_with(".نيفا") =>
                        {
                            vec![(name.to_string(), url.to_string())]
                        }
                        _ => {
                            return Err(RioError::new(
                                ErrorKind::Io,
                                "المسار ليس مجلدا ولا ملف .نيفا",
                                line,
                                col,
                            ))
                        }
                    }
                }
                _ => {
                    return Err(RioError::new(
                        ErrorKind::Io,
                        "رد جيت هاب غير مفهوم",
                        line,
                        col,
                    ))
                }
            };
            install_listed_files(files, where_ar, program_dir, line, col)
        }
        GithubSource::Release { owner, repo, tag } => {
            let api_path = format!(
                "/repos/{}/{}/releases/tags/{}",
                percent_encode_segment(&owner),
                percent_encode_segment(&repo),
                percent_encode_segment(&tag)
            );
            let json = fetch_github_json(api_base, &api_path, line, col)?;
            let assets = match json_field(&json, "assets") {
                Some(Json::Arr(items)) => items,
                _ => {
                    return Err(RioError::new(
                        ErrorKind::Io,
                        "رد جيت هاب غير مفهوم",
                        line,
                        col,
                    ))
                }
            };
            let mut files = Vec::new();
            for item in assets {
                let name = json_field(item, "name").and_then(json_str);
                let url = json_field(item, "browser_download_url").and_then(json_str);
                if let (Some(name), Some(url)) = (name, url) {
                    if name.ends_with(".نيفا") {
                        files.push((name.to_string(), url.to_string()));
                    }
                }
            }
            install_listed_files(files, "في هذا الإصدار", program_dir, line, col)
        }
    }
}

/// تحقق الاسم الأصلي + بلا كتابة فوق موجود + الحصة +
/// الكتابة — مشترك بين المسارات كلها (مباشر/محلي/
/// جيت هاب) فلا ينحرف سلوك عن سلوك. ومع `with_fingerprint`
/// (الرابط وحده) يعود نص التأكيد بسطر ثانٍ يحمل بصمة
/// SHA-256 للمحتوى المكتوب نفسه.
fn write_installed_module(
    program_dir: &Path,
    stem: String,
    content: String,
    with_fingerprint: bool,
    line: usize,
    col: usize,
) -> RioResult<Value> {
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
    // بصمة الملف (فكرة الثقة 7): للرابط وحده — بصمة
    // SHA-256 للمحتوى المكتوب نفسه (وهي بايتات الملف
    // حرفيا، فالكتابة أعلاه كتبت هذا المحتوى بعينه —
    // فلا قراءة ثانية ولا بصمة تحقق سابقة هنا).
    // والمسار المحلي بلا بصمة: الملف عند المستخدم أصلا.
    if with_fingerprint {
        let fingerprint = crate::crypto::sha256_hex(content.as_bytes());
        return Ok(Value::Str(format!(
            "ثُبّتت الوحدة {}\nبصمة الملف: {}",
            stem, fingerprint
        )));
    }
    Ok(Value::Str(format!("ثُبّتت الوحدة {}", stem)))
}

/// ثبّت أداة نيفا من رابط أو مسار محلي إلى مجلد البرنامج.
/// يعيد نص تأكيد («ثُبّتت الوحدة {الاسم}») يطبعه الطرفي —
/// ومع الرابط سطر ثانٍ «بصمة الملف: <بصمة SHA-256>»
/// (فكرة الثقة 7)؛ والمسار المحلي بلا سطر ثانٍ.
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
    let api_base = github_api_base(line, col)?;
    install_module_with_api(policy, cwd, program_dir, source, &api_base, line, col)
}

/// التنفيذ الداخلي بقاعدة واجهة صريحة (الاختبارات تمرّر
/// خادمها المحاكي؛ والعام يمرّر القاعدة الحقيقية).
fn install_module_with_api(
    policy: &FilePolicy,
    cwd: &Path,
    program_dir: &Path,
    source: &str,
    api_base: &str,
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
    // رابط جيت هاب (مستودع أو إصدار) — قبل المباشر.
    if is_url {
        if let Some(gh) = classify_github_url(source, line, col)? {
            return install_from_github(api_base, program_dir, gh, line, col);
        }
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
    // رابط مباشر يطبع البصمة؛ والمسار المحلي لا.
    write_installed_module(program_dir, stem, content, is_url, line, col)
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

    #[test]
    fn github_url_classify() {
        // جذر مستودع.
        match classify_github_url("https://github.com/owner/repo", 1, 1)
            .unwrap()
            .unwrap()
        {
            GithubSource::Repo {
                owner,
                repo,
                branch,
                path,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert!(branch.is_none());
                assert!(path.is_empty());
            }
            _ => panic!("expected repo"),
        }
        // شجرة بفرع ومسار + شرطة أخيرة.
        match classify_github_url(
            "https://github.com/owner/repo/tree/main/sub/dir/",
            1,
            1,
        )
        .unwrap()
        .unwrap()
        {
            GithubSource::Repo { branch, path, .. } => {
                assert_eq!(branch.as_deref(), Some("main"));
                assert_eq!(path, "sub/dir");
            }
            _ => panic!("expected repo tree"),
        }
        // إصدار + www + http.
        match classify_github_url(
            "http://www.github.com/owner/repo/releases/tag/v1.2.3",
            1,
            1,
        )
        .unwrap()
        .unwrap()
        {
            GithubSource::Release { owner, repo, tag } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert_eq!(tag, "v1.2.3");
            }
            _ => panic!("expected release"),
        }
        // ليس جيت هاب → None (يسلك مسار الرابط المباشر).
        assert!(classify_github_url("https://example.com/owner/repo", 1, 1)
            .unwrap()
            .is_none());
        assert!(classify_github_url(
            "https://raw.githubusercontent.com/owner/repo/main/tool.نيفا",
            1,
            1
        )
        .unwrap()
        .is_none());
        // blob مرفوض صراحة — لا HTML صفحة كملف.
        let err = classify_github_url(
            "https://github.com/owner/repo/blob/main/tool.نيفا",
            1,
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("غير مدعوم"), "got: {}", err);
        // releases بلا tag مرفوضة.
        let err =
            classify_github_url("https://github.com/owner/repo/releases", 1, 1)
                .unwrap_err();
        assert!(err.to_string().contains("غير مدعوم"), "got: {}", err);
        // بلا مستودع.
        assert!(classify_github_url("https://github.com/owner", 1, 1).is_err());
        // اسم فيه `..` مرفوض.
        assert!(classify_github_url("https://github.com/ow..ner/repo", 1, 1).is_err());
    }

    #[test]
    fn github_percent_encode_segment() {
        assert_eq!(percent_encode_segment("main"), "main");
        assert_eq!(percent_encode_segment("v1.2.3"), "v1.2.3");
        assert_eq!(percent_encode_segment("a/b"), "a%2Fb");
        assert_eq!(percent_encode_segment("a b"), "a%20b");
        assert_eq!(
            percent_encode_segment("أداة"),
            "%D8%A3%D8%AF%D8%A7%D8%A9"
        );
    }

    #[test]
    fn github_api_base_env() {
        let _guard = install_test_lock();
        std::env::remove_var("NIVA_GITHUB_API_BASE");
        assert_eq!(
            github_api_base(1, 1).unwrap(),
            "https://api.github.com"
        );
        std::env::set_var("NIVA_GITHUB_API_BASE", "http://127.0.0.1:9/");
        assert_eq!(github_api_base(1, 1).unwrap(), "http://127.0.0.1:9");
        std::env::set_var("NIVA_GITHUB_API_BASE", "ftp://x");
        assert!(github_api_base(1, 1).is_err());
        std::env::remove_var("NIVA_GITHUB_API_BASE");
    }

    /// خادم جيت هاب محاكى (اختبارات فقط): يربط 127.0.0.1
    /// على منفذ مؤقت ويرد بـ JSON canned حسب المسار —
    /// نمط خادم اختبارات الشبكة القائم. كل اختبار يأخذ
    /// خادمه (منفذه) فلا تتشارك الاختبارات حالة.
    #[cfg(not(target_arch = "wasm32"))]
    fn github_mock_server() -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut req: Vec<u8> = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.len() >= 4 && &req[req.len() - 4..] == b"\r\n\r\n" {
                                break;
                            }
                            if req.len() > 8192 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let req_text = String::from_utf8_lossy(&req).into_owned();
                let target = req_text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                // المسار بلا استعلام (فـ `?ref=` لا يكسر
                // المطابقة)، ويفك ترميزه (فـ `اجلب`
                // يرمّز المقاطع العربية على السلك).
                let path = percent_decode(
                    target.split('?').next().unwrap_or("/"),
                );
                let base = format!("http://127.0.0.1:{}", port);
                let file_body = "صدر قيمة\nقيمة = ٤٢\n";
                // إعادة توجيه إلى مضيف مختلف — يحاكي رابط
                // إصدار جيت هاب الحقيقي (release-assets).
                if path == "/redirect-cross" {
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 302 Found\r\nLocation: http://localhost:{}/files/tool.نيفا\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            port
                        )
                        .as_bytes(),
                    );
                    continue;
                }
                // إعادة توجيه إلى المضيف نفسه — كرابط أصل
                // إصدار يعيد توجيها داخل شبكة التوصيل نفسها.
                if path == "/redirect-same" {
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 302 Found\r\nLocation: {}/files/tool.نيفا\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            base
                        )
                        .as_bytes(),
                    );
                    continue;
                }
                let (status, body): (&str, String) = match path.as_str() {
                    // قائمة الجذر: ملف .نيفا واحد + نص + مجلد.
                    "/repos/owner/repo/contents" => (
                        "200 OK",
                        format!(
                            "[{{\"name\":\"tool.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/tool.نيفا\"}},{{\"name\":\"note.txt\",\"type\":\"file\",\"download_url\":\"{}/files/note.txt\"}},{{\"name\":\"sub\",\"type\":\"dir\",\"download_url\":null}}]",
                            base, base
                        ),
                    ),
                    // مسار فيه ملفان — تثبيت-الكل يثبّتهما معا.
                    "/repos/owner/repo/contents/sub" => (
                        "200 OK",
                        format!(
                            "[{{\"name\":\"a.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/a.نيفا\"}},{{\"name\":\"b.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/b.نيفا\"}}]",
                            base, base
                        ),
                    ),
                    // مسار فيه 21 ملفا — فوق سقف تثبيت-الكل
                    // (20) فيُرفض قبل أي تنزيل.
                    "/repos/owner/repo/contents/many" => {
                        let items: Vec<String> = (0..21)
                            .map(|i| {
                                format!(
                                    "{{\"name\":\"f{:02}.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/f{:02}.نيفا\"}}",
                                    i, base, i
                                )
                            })
                            .collect();
                        ("200 OK", format!("[{}]", items.join(",")))
                    }
                    // مسار فيه ملفان كبيران (3 ميغابايت لكل
                    // منهما) — مجموعهما فوق سقف 5 ميغابايت
                    // فيتوقف الثاني بعد تثبيت الأول.
                    "/repos/owner/repo/contents/big" => (
                        "200 OK",
                        format!(
                            "[{{\"name\":\"big1.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/big1.نيفا\"}},{{\"name\":\"big2.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/big2.نيفا\"}}]",
                            base, base
                        ),
                    ),
                    // مسار بلا ملف .نيفا.
                    "/repos/owner/repo/contents/empty" => (
                        "200 OK",
                        format!(
                            "[{{\"name\":\"note.txt\",\"type\":\"file\",\"download_url\":\"{}/files/note.txt\"}}]",
                            base
                        ),
                    ),
                    // مسار ملف مفرد .نيفا — كائن لا مصفوفة.
                    "/repos/owner/repo/contents/tool.نيفا" => (
                        "200 OK",
                        format!(
                            "{{\"name\":\"tool.نيفا\",\"type\":\"file\",\"download_url\":\"{}/files/tool.نيفا\"}}",
                            base
                        ),
                    ),
                    // مسار ملف مفرد ليس .نيفا.
                    "/repos/owner/repo/contents/note.txt" => (
                        "200 OK",
                        format!(
                            "{{\"name\":\"note.txt\",\"type\":\"file\",\"download_url\":\"{}/files/note.txt\"}}",
                            base
                        ),
                    ),
                    // إصدار بأصل .نيفا واحد (رابطه على الخادم
                    // نفسه — فيُحمَّل فعلا ويثبَّت).
                    "/repos/owner/repo/releases/tags/v1" => (
                        "200 OK",
                        format!(
                            "{{\"assets\":[{{\"name\":\"tool.نيفا\",\"browser_download_url\":\"{}/files/tool.نيفا\"}}]}}",
                            base
                        ),
                    ),
                    // إصدار بأصلين.
                    "/repos/owner/repo/releases/tags/v2" => (
                        "200 OK",
                        format!(
                            "{{\"assets\":[{{\"name\":\"a.نيفا\",\"browser_download_url\":\"{}/files/a.نيفا\"}},{{\"name\":\"b.نيفا\",\"browser_download_url\":\"{}/files/b.نيفا\"}}]}}",
                            base, base
                        ),
                    ),
                    // إصدار بلا أصل .نيفا.
                    "/repos/owner/repo/releases/tags/v3" => (
                        "200 OK",
                        "{\"assets\":[{\"name\":\"tool.zip\",\"browser_download_url\":\"http://127.0.0.1:1/files/tool.zip\"}]}"
                            .to_string(),
                    ),
                    // إصدار أصله يعيد توجيها إلى مضيف مختلف.
                    "/repos/owner/repo/releases/tags/v4" => (
                        "200 OK",
                        format!(
                            "{{\"assets\":[{{\"name\":\"tool.نيفا\",\"browser_download_url\":\"{}/redirect-cross\"}}]}}",
                            base
                        ),
                    ),
                    // إصدار أصله يعيد توجيها إلى المضيف نفسه
                    // — فيُتَّبَع ويثبَّت (اجتماع 28: التحويل
                    // إلى المضيف نفسه مسموح كما كان، وإلى
                    // مضيف التنزيل المسموح مسموح بشروطه).
                    "/repos/owner/repo/releases/tags/v5" => (
                        "200 OK",
                        format!(
                            "{{\"assets\":[{{\"name\":\"tool.نيفا\",\"browser_download_url\":\"{}/redirect-same\"}}]}}",
                            base
                        ),
                    ),
                    // ملفان كبيران لتثبيت-الكل (3 ميغابايت
                    // لكل منهما) — قبل مسار /files/ العام.
                    "/files/big1.نيفا" => ("200 OK", "x".repeat(3 * 1024 * 1024)),
                    "/files/big2.نيفا" => ("200 OK", "x".repeat(3 * 1024 * 1024)),
                    _ if path.starts_with("/files/") => {
                        ("200 OK", file_body.to_string())
                    }
                    _ => (
                        "404 Not Found",
                        "{\"message\":\"Not Found\"}".to_string(),
                    ),
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    )
                    .as_bytes(),
                );
            }
        });
        port
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn github_install(
        base: &Path,
        policy: &FilePolicy,
        api_base: &str,
        source: &str,
    ) -> Result<Value, RioError> {
        install_module_with_api(policy, base, base, source, api_base, 1, 1)
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_repo_single_file() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-repo");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        assert_eq!(
            std::fs::read_to_string(base.join("tool.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        // الثانية مرفوضة: بلا كتابة صامتة فوق موجود.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo",
        )
        .unwrap_err();
        assert!(err.to_string().contains("مثبّتة أصلا"), "got: {}", err);
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_repo_tree_branch_and_file() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-tree");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        // مسار ملف مفرد (مع فرع واستعلام ref).
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/tool.نيفا",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_repo_refusals() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-refuse");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        // ملفان — تثبيت-الكل يثبّتهما معا: مغطى
        // باختبار install_github_repo_install_all
        // المستقل أدناه، لا هنا.
        // لا ملف .نيفا.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/empty",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("لا يوجد ملف .نيفا"),
            "got: {}",
            err
        );
        // مسار ملف ليس .نيفا.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/note.txt",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("ليس مجلدا ولا ملف .نيفا"),
            "got: {}",
            err
        );
        // مستودع غائب — 404 من الواجهة برسالة عربية.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/ghost",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("رمز الحالة 404"),
            "got: {}",
            err
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_single_asset() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-rel1");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v1",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        assert_eq!(
            std::fs::read_to_string(base.join("tool.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_refusals() {
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-relx");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        // أصلان — تثبيت-الكل يثبّتهما معا: مغطى
        // باختبار install_github_release_install_all
        // المستقل أدناه، لا هنا.
        // بلا أصل .نيفا.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v3",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("لا يوجد ملف .نيفا"),
            "got: {}",
            err
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_cross_host_redirect_refused() {
        // حد موثق: رابط أصل الإصدار يعيد توجيها إلى مضيف
        // مختلف (localhost هنا — وقائمة سماح اجتماع 28
        // تغطي مضيف التنزيل وحده) فيرفضه `اجلب` بسياسة
        // إعادة التوجيه — وهذا الاختبار يثبت أن الرفض
        // هو السياسة نفسها لا عيب في مسار الإصدار.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-rel4");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v4",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("مضيف مختلف"),
            "got: {}",
            err
        );
        assert!(!base.join("tool.نيفا").exists());
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_redirect_same_host_followed() {
        // اجتماع 28: أصل إصدار يعيد توجيها إلى المضيف
        // نفسه يُتَّبَع ويثبَّت — فمسار الإصدار يكمل عبر
        // التحويل، كما يكمل عبر التحويل إلى المضيف
        // المسموح في الشبكة الحقيقية.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-rel5");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v5",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        assert_eq!(
            std::fs::read_to_string(base.join("tool.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_repo_install_all() {
        // اجتماع 43 §58: مجلد فيه ملفان — تثبيت-الكل
        // يثبّتهما معا: سطر لكل ملف + بصمته + سطر الملخص.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-all-repo");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/sub",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة a\nبصمة الملف: {}\nثُبّتت الوحدة b\nبصمة الملف: {}\nثُبّت ٢ من ٢ (تُخطَّى ٠)",
                MOCK_FILE_FINGERPRINT, MOCK_FILE_FINGERPRINT
            ))
        );
        assert_eq!(
            std::fs::read_to_string(base.join("a.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        assert_eq!(
            std::fs::read_to_string(base.join("b.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_install_all() {
        // اجتماع 43 §58: إصدار بأصلين — تثبيت-الكل
        // يثبّتهما معا بالصيغة نفسها.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-all-rel");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v2",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة a\nبصمة الملف: {}\nثُبّتت الوحدة b\nبصمة الملف: {}\nثُبّت ٢ من ٢ (تُخطَّى ٠)",
                MOCK_FILE_FINGERPRINT, MOCK_FILE_FINGERPRINT
            ))
        );
        assert!(base.join("a.نيفا").exists());
        assert!(base.join("b.نيفا").exists());
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_all_skip_and_resume() {
        // اجتماع 43 §58: المتعارض يُتخطَّى مع ملاحظة
        // ويُثبَّت الباقي؛ والملف المثبَّت لا يُمس؛
        // وإعادة التشغيل حين يكون الكل مثبَّتا خطأ
        // صريح بدل نجاح صامت.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-all-skip");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        std::fs::write(base.join("a.نيفا"), "قديم\n").unwrap();
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/sub",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "تُخطَّى a: مثبّتة أصلا\nثُبّتت الوحدة b\nبصمة الملف: {}\nثُبّت ١ من ٢ (تُخطَّى ١)",
                MOCK_FILE_FINGERPRINT
            ))
        );
        // المتخطَّى بقي كما هو — بلا استبدال.
        assert_eq!(
            std::fs::read_to_string(base.join("a.نيفا")).unwrap(),
            "قديم\n"
        );
        assert!(base.join("b.نيفا").exists());
        // الكل مثبَّت الآن — إعادة التشغيل خطأ صريح.
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/sub",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("كل الملفات مثبّتة أصلا"),
            "got: {}",
            err
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_all_count_cap() {
        // اجتماع 43 §58: سقف 20 ملفا يُفحص قبل أي
        // تنزيل — 21 ملفا تُرفض بلا كتابة.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-all-cap-n");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/many",
        )
        .unwrap_err();
        assert!(err.to_string().contains("٢٠"), "got: {}", err);
        assert!(err.to_string().contains("٢١"), "got: {}", err);
        assert!(!base.join("f00.نيفا").exists());
        assert!(!base.join("f20.نيفا").exists());
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_all_size_cap_and_resume() {
        // اجتماع 43 §58: سقف 5 ميغابايت إجمالا — ملفان
        // بثلاثة ميغابايت لكل منهما: الأول يُثبَّت،
        // والثاني يوقف بخطأ يسمّيه وعدد ما ثُبّت قبله؛
        // وإعادة التشغيل تُكمل الناقص (الأول يُتخطَّى
        // والثاني يُثبَّت).
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-all-cap-b");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let err = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/big",
        )
        .unwrap_err();
        assert!(err.to_string().contains("٥ ميغابايت"), "got: {}", err);
        assert!(err.to_string().contains("big2"), "got: {}", err);
        assert!(err.to_string().contains("١"), "got: {}", err);
        assert_eq!(
            std::fs::metadata(base.join("big1.نيفا")).unwrap().len(),
            3 * 1024 * 1024
        );
        assert!(!base.join("big2.نيفا").exists());
        // إعادة التشغيل تكمل الناقص.
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/tree/main/big",
        )
        .unwrap();
        let text = match got {
            Value::Str(s) => s,
            _ => panic!("expected text value"),
        };
        assert!(
            text.contains("تُخطَّى big1: مثبّتة أصلا"),
            "got: {}",
            text
        );
        assert!(
            text.contains("ثُبّت ١ من ٢ (تُخطَّى ١)"),
            "got: {}",
            text
        );
        assert_eq!(
            std::fs::metadata(base.join("big2.نيفا")).unwrap().len(),
            3 * 1024 * 1024
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    /// بصمة الملف المتوقعة لمحتوى الخادم المحاكي
    /// «صدر قيمة\nقيمة = ٤٢\n» — محسوبة بأداة النظام
    /// (sha256sum) لا بالمحرك نفسه، فلا يختبر الكود
    /// نفسه بنفسه.
    const MOCK_FILE_FINGERPRINT: &str =
        "4cee6f791f5495b679f4a5317bd3afccab74d827761164d6b4a4e4f76770d249";

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_direct_url_prints_fingerprint() {
        // الرابط المباشر: رسالة النجاح بسطرها الأول
        // كما هي، ثم سطر البصمة — والبصمة هي SHA-256
        // للمحتوى المثبّت نفسه.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("url-fp");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = install_module_with_api(
            &policy,
            &base,
            &base,
            &format!("http://127.0.0.1:{}/files/tool.نيفا", port),
            &api_base,
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        assert_eq!(
            std::fs::read_to_string(base.join("tool.نيفا")).unwrap(),
            "صدر قيمة\nقيمة = ٤٢\n"
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_repo_prints_fingerprint() {
        // رابط المستودع: البصمة مع النجاح أيضا.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-repo-fp");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn install_github_release_prints_fingerprint() {
        // رابط الإصدار: البصمة مع النجاح أيضا.
        let _guard = install_test_lock();
        let (base, _src_dir) = test_dirs("gh-rel-fp");
        let policy = test_policy(&base);
        crate::network::set_network_allowed(true);
        crate::network::allow_local_host("127.0.0.1");
        let port = github_mock_server();
        let api_base = format!("http://127.0.0.1:{}", port);
        let got = github_install(
            &base,
            &policy,
            &api_base,
            "https://github.com/owner/repo/releases/tag/v1",
        )
        .unwrap();
        assert_eq!(
            got,
            Value::Str(format!(
                "ثُبّتت الوحدة tool\nبصمة الملف: {}",
                MOCK_FILE_FINGERPRINT
            ))
        );
        crate::network::set_network_allowed(false);
        cleanup(&base);
    }

    #[test]
    fn install_local_no_fingerprint() {
        // المسار المحلي: رسالة النجاح وحدها حرفيا —
        // بلا سطر بصمة إطلاقا.
        let _guard = install_test_lock();
        let (base, src_dir) = test_dirs("local-fp");
        let policy = test_policy(&base);
        std::fs::write(src_dir.join("أداة.نيفا"), "صدر قيمة\nقيمة = ٤٢\n")
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
        match &got {
            Value::Str(s) => assert!(!s.contains("بصمة"), "got: {}", s),
            _ => panic!("expected text, got: {:?}", got),
        }
        cleanup(&base);
    }
}
