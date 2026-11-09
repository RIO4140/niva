//! الموجهات والخطاف والمهلة — اجتماع 61 §76 (الدفعة 7: الميزات 174–176).
//!
//! مقابل `PS1` و`PS2` و`PS4` و`PROMPT_COMMAND` و`TMOUT` في باش 5.2:
//! موجه أول وموجه استمرار وموجه تتبّع (يُخزَّن وحده — فلا وضع
//! تتبّع في نيفا أصلا) وأمر ينفذ قبل كل موجه أول ومهلة خمول
//! تسجّل الخروج تلقائيا. كلها إعدادات جلسة تفاعلية في
//! `niva.properties` + عبارات REPL قارئة/ضابطة بالأسماء
//! نفسها — بلا دالة مدمجة في المفسر/الآلة الافتراضية وبلا
//! مرآة في الخلفية C (تفاعلية بطبيعتها، كما في §75).
//!
//! التوسيع عند كل طباعة: خمسة رموز عربية بين قوسين
//! معقوفين — `{المجلد}` و`{المستخدم}` و`{المضيف}`
//! و`{الوقت}` و`{علامة}` — والرمز المجهول يبقى حرفيا بلا
//! خطأ؛ ولا تسلسلات باش اللاتينية (`\s \v \$` ونحوها).

/// مفتاح الموجه الأول في `niva.properties` (مقابل `PS1`).
pub const KEY_PRIMARY: &str = "موجه_أول";
/// مفتاح موجه الاستمرار في `niva.properties` (مقابل `PS2`).
pub const KEY_CONTINUATION: &str = "موجه_استمرار";
/// مفتاح موجه التتبع في `niva.properties` (مقابل `PS4` — يُخزَّن وحده).
pub const KEY_TRACE: &str = "موجه_تتبع";
/// مفتاح أمر ما قبل الموجه في `niva.properties` (مقابل `PROMPT_COMMAND`).
pub const KEY_HOOK: &str = "أمر_قبل_الموجه";
/// مفتاح مهلة الخمول في `niva.properties` (مقابل `TMOUT`).
pub const KEY_TIMEOUT: &str = "مهلة_الخمول";

/// الموجه الأول الافتراضي — نفس موجه اليوم حرفيا.
pub const DEFAULT_PRIMARY: &str = "{المجلد} $ ";
/// موجه الاستمرار الافتراضي.
pub const DEFAULT_CONTINUATION: &str = "> ";
/// موجه التتبع الافتراضي (يُخزَّن وحده — بلا عرض تتبّع).
pub const DEFAULT_TRACE: &str = "+ ";

/// رسالة انتهاء المهلة — تُطبع قبل الخروج التلقائي.
pub const TIMEOUT_MESSAGE: &str = "انتهت المهلة في انتظار الإدخال: تسجيل خروج تلقائي";

/// إعدادات الموجهات والخطاف والمهلة لجلسة تفاعلية واحدة.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptSettings {
    /// قالب الموجه الأول.
    pub primary: String,
    /// قالب موجه الاستمرار.
    pub continuation: String,
    /// قالب موجه التتبع (يُخزَّن وحده).
    pub trace: String,
    /// أمر ما قبل الموجه (مقتطف نيفا واحد؛ والفارغ = بلا خطاف).
    pub hook: String,
    /// مهلة الخمول بالثواني (0 = بلا مهلة).
    pub timeout_secs: u64,
}

impl Default for PromptSettings {
    fn default() -> Self {
        Self {
            primary: DEFAULT_PRIMARY.to_string(),
            continuation: DEFAULT_CONTINUATION.to_string(),
            trace: DEFAULT_TRACE.to_string(),
            hook: String::new(),
            timeout_secs: 0,
        }
    }
}

impl PromptSettings {
    /// الافتراضيات.
    pub fn defaults() -> Self {
        Self::default()
    }

    /// من إدخالات `niva.properties` — `set_if_not`: المفتاح
    /// الغائب يأخذ الافتراضي، والحاضر يُحفظ كما هو (حتى
    /// الفارغ للخطاف والمهلة)؛ والقيمة السيئة للمهلة
    /// تُسقط إلى الافتراضي بصمت (والضبط الحي من العبارة
    /// يرفضها بخطأ صريح بدلا من ذلك — انظر `Repl`).
    pub fn from_entries(entries: &[(String, String)]) -> Self {
        let mut s = Self::defaults();
        for (key, value) in entries {
            match key.as_str() {
                KEY_PRIMARY => s.primary = value.clone(),
                KEY_CONTINUATION => s.continuation = value.clone(),
                KEY_TRACE => s.trace = value.clone(),
                KEY_HOOK => s.hook = value.clone(),
                KEY_TIMEOUT => {
                    if let Some(n) = parse_timeout(value) {
                        s.timeout_secs = n;
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// هل للجلسة خطاف قبل الموجه؟
    pub fn has_hook(&self) -> bool {
        !self.hook.trim().is_empty()
    }

    /// هل للجلسة مهلة خمول؟
    pub fn has_timeout(&self) -> bool {
        self.timeout_secs > 0
    }
}

/// حلّل قيمة مهلة: عدد ثوان صحيح موجب (أرقام عربية
/// ولاتينية)؛ والفارغ أو `بلا` = 0 (بلا مهلة)؛ وغير
/// العدد أو السالب = `None` (قيمة سيئة).
pub fn parse_timeout(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() || t == "بلا" {
        return Some(0);
    }
    match crate::history::parse_history_number(t) {
        Some(n) if n >= 0 => Some(n as u64),
        _ => None,
    }
}

/// سياق توسيع قالب موجه — القيم الحية عند الطباعة.
#[derive(Clone, Debug)]
pub struct PromptContext {
    /// عرض المجلد الحالي (`~ $ ` ونحوه).
    pub current_dir: String,
    /// اسم المستخدم (None حين تعذّر — يبقى الرمز حرفيا).
    pub username: Option<String>,
    /// اسم المضيف (None حين تعذّر — يبقى الرمز حرفيا).
    pub hostname: Option<String>,
    /// هل المستخدم جذر (فالعلامة `#` لا `$`).
    pub is_root: bool,
    /// الساعة والدقيقة المحليتان («١٩:٤٥» — None حين تعذّرتا).
    pub hour_min: Option<String>,
}

impl PromptContext {
    /// السياق الحي لمجلد حالي معطى: المستخدم والمضيف
    /// من `env.rs` القائم، والوقت من ساعة النظام.
    pub fn current(current_dir: &str) -> Self {
        Self {
            current_dir: current_dir.to_string(),
            username: crate::env::username().ok(),
            hostname: crate::env::hostname().ok(),
            is_root: is_root_user(),
            hour_min: current_hour_min(),
        }
    }
}

/// هل المستخدم الحالي جذر (رقم المستخدم 0)؟ — من
/// `/proc/self/status` على لينكس/أندرويد؛ وخارجهما لا.
fn is_root_user() -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find(|l| l.starts_with("Uid:")).and_then(|l| {
                    l.split_whitespace().nth(1).map(|uid| uid == "0")
                })
            })
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        false
    }
}

/// الساعة والدقيقة المحليتان بأرقام مشرقية («١٩:٤٥»)؛
/// و`None` حين تعذّرت قراءة المنطقة (فيبقى الرمز حرفيا).
fn current_hour_min() -> Option<String> {
    let secs = crate::history::now_secs();
    if secs == 0 {
        return None;
    }
    let dt = crate::time::datetime_from_epoch(secs as i64, false, "موجه", 0, 0).ok()?;
    // «سس:دد» — أول خمسة أحرف من «سس:دد:ثث» (تقطيع
    // على الأحرف لا على البايتات — الأرقام مشرقية).
    Some(crate::time::format_time(&dt).chars().take(5).collect())
}

/// وسّع قالب موجه: الرموز الخمسة المعروفة تُستبدل
/// بقيم السياق الحية، والمجهول بين القوسين يبقى حرفيا
/// بلا خطأ؛ و`$` الحرفي في القالب يبقى `$` (لا توسع له).
pub fn expand_prompt(template: &str, ctx: &PromptContext) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                let replacement: Option<String> = match name {
                    "المجلد" => Some(ctx.current_dir.clone()),
                    "المستخدم" => ctx.username.clone(),
                    "المضيف" => ctx.hostname.clone(),
                    "الوقت" => ctx.hour_min.clone(),
                    "علامة" => Some(if ctx.is_root { "#".to_string() } else { "$".to_string() }),
                    _ => None,
                };
                match replacement {
                    Some(value) => {
                        out.push_str(&rest[..open]);
                        out.push_str(&value);
                    }
                    None => {
                        // رمز مجهول — يبقى حرفيا كما كُتب.
                        out.push_str(&rest[..open + 1 + close + 1]);
                    }
                }
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PromptContext {
        PromptContext {
            current_dir: "~".to_string(),
            username: Some("محمود".to_string()),
            hostname: Some("بيت".to_string()),
            is_root: false,
            hour_min: Some("١٩:٤٥".to_string()),
        }
    }

    #[test]
    fn defaults_are_today_prompt() {
        let s = PromptSettings::defaults();
        assert_eq!(s.primary, "{المجلد} $ ");
        assert_eq!(s.continuation, "> ");
        assert_eq!(s.trace, "+ ");
        assert_eq!(s.hook, "");
        assert_eq!(s.timeout_secs, 0);
        assert!(!s.has_hook());
        assert!(!s.has_timeout());
    }

    #[test]
    fn from_entries_set_if_not() {
        // الغائب يأخذ الافتراضي، والحاضر يفوز عليه.
        let entries = vec![("موجه_أول".to_string(), "جاهز> ".to_string())];
        let s = PromptSettings::from_entries(&entries);
        assert_eq!(s.primary, "جاهز> ");
        assert_eq!(s.continuation, "> ");
        assert_eq!(s.trace, "+ ");
        // الحاضر الفارغ يُحفظ فارغا — لا يطغى عليه افتراضي.
        let entries = vec![
            ("أمر_قبل_الموجه".to_string(), "".to_string()),
            ("مهلة_الخمول".to_string(), "".to_string()),
        ];
        let s = PromptSettings::from_entries(&entries);
        assert_eq!(s.hook, "");
        assert_eq!(s.timeout_secs, 0);
        // القيمة السيئة للمهلة تُسقط إلى الافتراضي بصمت.
        let entries = vec![("مهلة_الخمول".to_string(), "غريب".to_string())];
        assert_eq!(PromptSettings::from_entries(&entries).timeout_secs, 0);
        let entries = vec![("مهلة_الخمول".to_string(), "-٥".to_string())];
        assert_eq!(PromptSettings::from_entries(&entries).timeout_secs, 0);
        let entries = vec![("مهلة_الخمول".to_string(), "٦٠".to_string())];
        assert_eq!(PromptSettings::from_entries(&entries).timeout_secs, 60);
    }

    #[test]
    fn parse_timeout_forms() {
        assert_eq!(parse_timeout(""), Some(0));
        assert_eq!(parse_timeout("بلا"), Some(0));
        assert_eq!(parse_timeout("0"), Some(0));
        assert_eq!(parse_timeout("٦٠"), Some(60));
        assert_eq!(parse_timeout("120"), Some(120));
        assert_eq!(parse_timeout(" ٣٠ "), Some(30));
        assert_eq!(parse_timeout("غريب"), None);
        assert_eq!(parse_timeout("-٥"), None);
        assert_eq!(parse_timeout("١٫٥"), None);
    }

    #[test]
    fn expand_all_five_sequences() {
        let c = ctx();
        assert_eq!(expand_prompt("{المجلد} $ ", &c), "~ $ ");
        assert_eq!(
            expand_prompt("{المستخدم}@{المضيف} {المجلد} {علامة} ", &c),
            "محمود@بيت ~ $ "
        );
        assert_eq!(expand_prompt("الآن {الوقت}", &c), "الآن ١٩:٤٥");
        // الجذر علامته `#`.
        let mut root = ctx();
        root.is_root = true;
        assert_eq!(expand_prompt("{علامة} ", &root), "# ");
    }

    #[test]
    fn expand_unknown_stays_literal() {
        let c = ctx();
        assert_eq!(expand_prompt("{غريب} $ ", &c), "{غريب} $ ");
        assert_eq!(expand_prompt("بلا أقواس $ ", &c), "بلا أقواس $ ");
        // قوس مفتوح بلا إغلاق يبقى كما هو.
        assert_eq!(expand_prompt("قبل {المجلد", &c), "قبل {المجلد");
        // القيم الغائبة تبقي رمزها حرفيا.
        let mut no_user = ctx();
        no_user.username = None;
        no_user.hour_min = None;
        assert_eq!(
            expand_prompt("{المستخدم} {الوقت} ", &no_user),
            "{المستخدم} {الوقت} "
        );
    }

    #[test]
    fn expand_default_template() {
        // القالب الافتراضي نفسه يتوسع إلى موجه اليوم.
        assert_eq!(expand_prompt(DEFAULT_PRIMARY, &ctx()), "~ $ ");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn live_context_reads_user_host_time() {
        let c = PromptContext::current("~");
        assert_eq!(c.current_dir, "~");
        assert!(c.username.is_some());
        assert!(c.hostname.is_some());
        // ساعة حية بصيغة «سس:دد» بأرقام مشرقية.
        let hm = c.hour_min.expect("hour_min");
        assert_eq!(hm.chars().count(), 5);
        assert_eq!(hm.chars().nth(2), Some(':'));
    }
}
