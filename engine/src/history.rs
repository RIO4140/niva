//! محفوظات الجلسة التفاعلية — اجتماع 60 §75 (الدفعة 6: الميزات 149–158).
//!
//! المحفوظات تفاعلية بطبيعتها: بنية `History` يملكها `Repl`
//! وحده، بلا دالة مدمجة في المفسر/الآلة الافتراضية وبلا
//! مرآة في الخلفية C — فالسكربتات بلا محفوظات في باش أيضا.
//! كل سطر مكتوب يمر بنقطة تسجيل واحدة (`record`) تطبق
//! قواعد الحفظ (155) وأنماط التجاهل (156) والحدين (149)،
//! ويسجل السطر المكتوب وحده: المقتطف المولد من اختصار
//! وسطور ملفات البدء واستدعاء الخطاف لا تسجل أبدا
//! (تُعلق عبر `suspend_recording` ثم تُستأنف، أو تُقتطع
//! بعد `eval` كما يفعل الخطاف اليوم).
//!
//! ملف المحفوظات (`~/محفوظات` افتراضيا): سطر نصي لكل
//! مدخل، يسبقه سطر `#<ثواني الحقبة>` بطابعه الزمني
//! (صيغة ملف باش نفسها؛ والسطر `#` غير الرقمي خالصا
//! مدخل عادي لا طابعا). الإلحاق افتراضي (عكس باش —
//! جلسات نيفا المتعددة تتعاون على الملف نفسه).

use std::path::{Path, PathBuf};

/// الحد الافتراضي في الذاكرة وفي الملف: 500 (افتراضي باش).
pub const DEFAULT_MAX_MEMORY: usize = 500;
/// الحد الافتراضي في الملف: 500 (افتراضي باش).
pub const DEFAULT_MAX_FILE: usize = 500;
/// اسم ملف المحفوظات الافتراضي في البيت.
pub const DEFAULT_FILE_NAME: &str = "محفوظات";

/// مفاتيح الإعدادات في `niva.properties` (اجتماع 60).
pub const KEY_FILE: &str = "ملف_المحفوظات";
/// مفتاح حد الذاكرة في الإعدادات.
pub const KEY_MAX_MEMORY: &str = "حد_المحفوظات";
/// مفتاح حد الملف في الإعدادات.
pub const KEY_MAX_FILE: &str = "حد_ملف_المحفوظات";
/// مفتاح قواعد الحفظ في الإعدادات.
pub const KEY_CONTROL: &str = "قواعد_المحفوظات";
/// مفتاح أنماط التجاهل في الإعدادات.
pub const KEY_IGNORE: &str = "تجاهل_المحفوظات";
/// مفتاح صيغة الطابع الزمني في الإعدادات.
pub const KEY_TIME_FORMAT: &str = "صيغة_وقت_المحفوظات";

/// مدخل واحد: النص كما كُتب، وثواني الحقبة حين كُتب
/// (0 حين لا طابع — كسطر كُتب يدويا في الملف).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// النص كما كُتب.
    pub text: String,
    /// ثواني الحقبة (0 = بلا طابع).
    pub time: u64,
}

/// ثواني الحقبة الآن (ساعة النظام؛ و0 حين تعذر قراءتها).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// حوّل رقما مكتوبا بأرقام عربية أو لاتينية إلى عدد صحيح
/// (بإشارة سالبة اختيارية) — للأرقام في `محفوظات` و`حرر`
/// والتوسيع. بلا أرقام عشرية وبلا فراغات داخلية.
pub fn parse_history_number(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || ('٠'..='٩').contains(&c)) {
        return None;
    }
    let latin: String = digits
        .chars()
        .map(|c| {
            if ('٠'..='٩').contains(&c) {
                char::from_digit(c as u32 - '٠' as u32, 10).unwrap()
            } else {
                c
            }
        })
        .collect();
    let n: i64 = latin.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// مطابقة نمط تجاهل على سطر كامل: `*` أي run من الأحرف
/// و`?` حرف واحد — خالصة بلا اعتماد، على الأحرف لا البايتات.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// إعدادات المحفوظات (الاجتماع 60 §75، السؤال الأول
/// والسابع والثامن): ملف وحدان وقواعد وأنماط وصيغة وقت.
#[derive(Clone, Debug)]
pub struct HistorySettings {
    /// ملف المحفوظات (None = بلا ملف).
    pub file: Option<PathBuf>,
    /// حد الذاكرة (0 = لا يُحفظ شيء).
    pub max_memory: usize,
    /// حد الملف (0 = الملف فارغ دائما).
    pub max_file: usize,
    /// تجاهل المكرر (يطابق السابق مباشرة).
    pub ignore_dups: bool,
    /// تجاهل البادئ بفراغ.
    pub ignore_space: bool,
    /// مسح المكرر (يحذف كل المطابقات القديمة قبل الحفظ).
    pub erase_dups: bool,
    /// أنماط تجاهل (`*` و`?`، تطابق السطر كاملا).
    pub ignore_patterns: Vec<String>,
    /// صيغة عرض الطابع (None = بلا طوابع في العرض).
    pub time_format: Option<String>,
}

impl HistorySettings {
    /// الافتراضيات: `~/محفوظات`، و500/500.
    pub fn defaults(program_dir: &Path) -> Self {
        HistorySettings {
            file: Some(program_dir.join("home").join(DEFAULT_FILE_NAME)),
            max_memory: DEFAULT_MAX_MEMORY,
            max_file: DEFAULT_MAX_FILE,
            ignore_dups: false,
            ignore_space: false,
            erase_dups: false,
            ignore_patterns: Vec::new(),
            time_format: None,
        }
    }

    /// من إدخالات `niva.properties`: مفاتيح الاجتماع 60
    /// الستة؛ والقيمة السيئة تُسقط إلى الافتراضي بصمت
    /// (والضبط الحي من العبارة يرفضها بخطأ صريح بدلا
    /// من ذلك — انظر `Repl`).
    pub fn from_entries(program_dir: &Path, entries: &[(String, String)]) -> Self {
        let mut s = Self::defaults(program_dir);
        for (key, value) in entries {
            match key.as_str() {
                KEY_FILE => {
                    if value.trim().is_empty() {
                        s.file = None;
                    } else {
                        s.file = Some(resolve_history_file(program_dir, value));
                    }
                }
                KEY_MAX_MEMORY => {
                    if let Some(n) = parse_history_number(value) {
                        if n >= 0 {
                            s.max_memory = n as usize;
                        }
                    }
                }
                KEY_MAX_FILE => {
                    if let Some(n) = parse_history_number(value) {
                        if n >= 0 {
                            s.max_file = n as usize;
                        }
                    }
                }
                KEY_CONTROL => {
                    if let Ok((d, sp, e)) = parse_control(value) {
                        s.ignore_dups = d;
                        s.ignore_space = sp;
                        s.erase_dups = e;
                    }
                }
                KEY_IGNORE => {
                    s.ignore_patterns = parse_patterns(value);
                }
                KEY_TIME_FORMAT => {
                    if value.trim().is_empty() {
                        s.time_format = None;
                    } else {
                        s.time_format = Some(value.clone());
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// القواعد الحالية نصا واحدا مفصولا بمسافات
    /// (للعرض والتخزين): «تجاهل_المكرر تجاهل_المسافات»...
    /// و«بلا قواعد» حين لا قواعد — النص وحده لا يُخزَّن.
    pub fn control_string(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ignore_dups && self.ignore_space {
            parts.push("تجاهل_الاثنين");
        } else {
            if self.ignore_dups {
                parts.push("تجاهل_المكرر");
            }
            if self.ignore_space {
                parts.push("تجاهل_المسافات");
            }
        }
        if self.erase_dups {
            parts.push("مسح_المكرر");
        }
        parts.join(" ")
    }

    /// الأنماط الحالية نصا واحدا مفصولا بمسافات.
    pub fn ignore_string(&self) -> String {
        self.ignore_patterns.join(" ")
    }
}

/// حل مسار ملف المحفوظات من قيمة الإعدادات: `~`
/// و`~/...` إلى البيت، والنسبي إلى البيت، والمطلق كما
/// هو (والفحص ضد الخروج من مجلد البرنامج في `Repl`
/// عند الضبط الحي — أما القراءة من الملف فتثق بقيمته
/// لأن الملف يحرره المستخدم نفسه في بيته).
pub fn resolve_history_file(program_dir: &Path, value: &str) -> PathBuf {
    let v = value.trim();
    let home = program_dir.join("home");
    if v == "~" {
        return home.join(DEFAULT_FILE_NAME);
    }
    if let Some(rest) = v.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = PathBuf::from(v);
    if p.is_absolute() {
        p
    } else {
        home.join(p)
    }
}

/// عرض مسار الملف بصيغة `~` حين يكون تحت البيت.
pub fn display_history_file(program_dir: &Path, path: &Path) -> String {
    let home = program_dir.join("home");
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// حلل قيمة قواعد الحفظ: `تجاهل_المكرر` و`تجاهل_المسافات`
/// و`مسح_المكرر` و`تجاهل_الاثنين` (الأخير = الأولان معا)،
/// مفصولة بمسافات. القيمة المجهولة خطأ يسميها.
pub fn parse_control(value: &str) -> Result<(bool, bool, bool), String> {
    let mut dups = false;
    let mut space = false;
    let mut erase = false;
    for w in value.split_whitespace() {
        match w {
            "تجاهل_المكرر" => dups = true,
            "تجاهل_المسافات" => space = true,
            "مسح_المكرر" => erase = true,
            "تجاهل_الاثنين" => {
                dups = true;
                space = true;
            }
            "بلا" => {}
            other => return Err(format!("قاعدة محفوظات غير معروفة: {}", other)),
        }
    }
    Ok((dups, space, erase))
}

/// حلل قيمة أنماط التجاهل: كلمات مفصولة بمسافات،
/// و`بلا` وحدها تعني بلا أنماط.
pub fn parse_patterns(value: &str) -> Vec<String> {
    let words: Vec<&str> = value.split_whitespace().collect();
    if words.len() == 1 && words[0] == "بلا" {
        return Vec::new();
    }
    words.iter().map(|w| w.to_string()).collect()
}

/// تحقق من صيغة وقت: الرموز المسموحة `%Y` و`%m` و`%d`
/// و`%H` و`%M` و`%S` و`%F` و`%T` و`%%` فقط؛ وأي `%`
/// أخرى خطأ يسميها.
pub fn check_time_format(format: &str) -> Result<(), String> {
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some(x) if "YmdHMSFT%".contains(x) => {}
                Some(x) => return Err(format!("رمز وقت غير معروف: %{}", x)),
                None => return Err("صيغة الوقت تنتهي بـ% بلا رمز".to_string()),
            }
        }
    }
    Ok(())
}

/// حوّل أرقاما لاتينية في نص إلى أرقام مشرقية للعرض.
fn eastern_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('٠' as u32 + (c as u32 - '0' as u32)).unwrap(),
            other => other,
        })
        .collect()
}

/// نسّق ثواني الحقبة حسب الصيغة (الرموز الثمانية في
/// الاجتماع 60): التاريخ والوقت المحليان، بأرقام مشرقية.
/// بلا صيغة أو بلا طابع (0) يعيد None.
pub fn format_time(format: &str, epoch: u64) -> Option<String> {
    if epoch == 0 {
        return None;
    }
    let dt = crate::time::datetime_from_epoch(epoch as i64, false, "محفوظات", 1, 1)
        .or_else(|_| crate::time::datetime_from_epoch(epoch as i64, true, "محفوظات", 1, 1))
        .ok()?;
    let num = |n: u32, w: usize| eastern_digits(&format!("{:0width$}", n, width = w));
    let date = format!("{}-{}-{}", num(dt.year as u32, 4), num(dt.month, 2), num(dt.day, 2));
    let time = format!("{}:{}:{}", num(dt.hour, 2), num(dt.minute, 2), num(dt.second, 2));
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('F') => out.push_str(&date),
            Some('T') => out.push_str(&time),
            Some('Y') => out.push_str(&num(dt.year as u32, 4)),
            Some('m') => out.push_str(&num(dt.month, 2)),
            Some('d') => out.push_str(&num(dt.day, 2)),
            Some('H') => out.push_str(&num(dt.hour, 2)),
            Some('M') => out.push_str(&num(dt.minute, 2)),
            Some('S') => out.push_str(&num(dt.second, 2)),
            Some('%') => out.push('%'),
            _ => {}
        }
    }
    Some(out)
}

/// محفوظات جلسة واحدة: المدخلات (نص + طابع) وإعداداتها
/// وحالتا التعاون مع الملف (كم مدخل محفوظ، وكم سطرا
/// من الملف استُهلك) ومؤشر التنقل.
pub struct History {
    entries: Vec<HistoryEntry>,
    settings: HistorySettings,
    /// عدد المدخلات الأولى المحفوظة في الملف فعلا
    /// (ما بعدها جديد منذ آخر حفظ/تحميل).
    saved_len: usize,
    /// سطور الملف المستهلكة (لـ`اقرأ_جديد`: ما بعدها جديد).
    lines_read: usize,
    /// مؤشر التنقل بالأسهم (None = لا تنقل جار).
    nav: Option<usize>,
    /// تعليق التسجيل (سطور البدء والمقتطفات والخطاف):
    /// `record` لا تفعل شيئا ما دام العداد فوق الصفر.
    suspended: u32,
}

impl History {
    /// افتراضيات على مجلد برنامج معطى.
    pub fn new(program_dir: &Path) -> Self {
        Self::from_settings(program_dir, &[])
    }

    /// من إدخالات `niva.properties`.
    pub fn from_settings(program_dir: &Path, entries: &[(String, String)]) -> Self {
        History {
            entries: Vec::new(),
            settings: HistorySettings::from_entries(program_dir, entries),
            saved_len: 0,
            lines_read: 0,
            nav: None,
            suspended: 0,
        }
    }

    /// الإعدادات الحالية.
    pub fn settings(&self) -> &HistorySettings {
        &self.settings
    }

    /// عدد المدخلات.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// فارغة؟
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// المدخلات كلها (نص + طابع).
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// النصوص وحدها (للعرض والاختبار).
    pub fn texts(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.text.clone()).collect()
    }

    /// نص آخر مدخل.
    pub fn last_text(&self) -> Option<&str> {
        self.entries.last().map(|e| e.text.as_str())
    }

    /// علّق التسجيل (للمدخلات الداخلية لا المكتوبة).
    pub fn suspend_recording(&mut self) {
        self.suspended = self.suspended.saturating_add(1);
    }

    /// استأنف التسجيل بعد تعليق.
    pub fn resume_recording(&mut self) {
        self.suspended = self.suspended.saturating_sub(1);
    }

    /// نقطة التسجيل الوحيدة: طبّق قواعد الحفظ وأنماط
    /// التجاهل والحد، وسجّل بطابع الآن. السطر الفارغ
    /// (بعد تقليم الطرفين) لا يُسجَّل أبدا.
    pub fn record(&mut self, line: &str) {
        if self.suspended > 0 {
            return;
        }
        if line.trim().is_empty() {
            return;
        }
        self.nav = None;
        if self.settings.ignore_space && (line.starts_with(' ') || line.starts_with('\t')) {
            return;
        }
        if self
            .settings
            .ignore_patterns
            .iter()
            .any(|p| glob_match(p, line))
        {
            return;
        }
        if self.settings.ignore_dups
            && self.entries.last().map(|e| e.text.as_str()) == Some(line)
        {
            return;
        }
        if self.settings.erase_dups {
            let before = self.entries.len();
            self.entries.retain(|e| e.text != line);
            let removed = before - self.entries.len();
            self.saved_len = self.saved_len.saturating_sub(removed);
        }
        self.entries.push(HistoryEntry {
            text: line.to_string(),
            time: now_secs(),
        });
        self.enforce_max_memory();
    }

    /// اقتطع الأقدم إلى حد الذاكرة (يُستدعى بعد كل دفع).
    fn enforce_max_memory(&mut self) {
        let max = self.settings.max_memory;
        if self.entries.len() > max {
            let drop = self.entries.len() - max;
            self.entries.drain(..drop);
            self.saved_len = self.saved_len.saturating_sub(drop);
        }
    }

    /// امسح الذاكرة وحدهما (لا الملف): `امسح` و`:مسح`
    /// و`:امسح-المحفوظات`. و`lines_read` يبقى — فلا
    /// يعيد `اقرأ_جديد` ما مُسح.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.saved_len = 0;
        self.nav = None;
    }

    /// اقتطع إلى طول معطى (إلغاء دفع داخلي: الخطاف).
    pub fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
        self.saved_len = self.saved_len.min(len);
        if let Some(n) = self.nav {
            if n >= len {
                self.nav = None;
            }
        }
    }

    /// استبدل نص آخر مدخل (سطر `عكس` يُسجَّل كاملا
    /// ببادئاته) مع بقاء طابعه.
    pub fn replace_last_text(&mut self, text: &str) {
        if let Some(last) = self.entries.last_mut() {
            last.text = text.to_string();
        }
    }

    /// احذف مدخلا واحدا (بالفهرس الداخلي 0-based).
    /// تعيد false حين خرج الفهرس.
    pub fn delete(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        if index < self.saved_len {
            self.saved_len -= 1;
        }
        self.nav = None;
        true
    }

    /// احذف مدى شاملا (بالفهرس الداخلي 0-based).
    pub fn delete_range(&mut self, first: usize, last: usize) {
        if first > last || first >= self.entries.len() {
            return;
        }
        let last = last.min(self.entries.len() - 1);
        let count = last - first + 1;
        self.entries.drain(first..=last);
        if first < self.saved_len {
            let saved_in_range = (self.saved_len - first).min(count);
            self.saved_len -= saved_in_range;
        }
        self.nav = None;
    }

    /// حل رقم محفوظات (1 = الأقدم؛ والسالب من النهاية،
    /// -1 = الأخير) إلى فهرس داخلي.
    pub fn resolve(&self, n: i64) -> Option<usize> {
        resolve_in(&self.entries, n)
    }

    /// توسيع التاريخ (151): سطر يبدأ بـ`!`. تعيد السطر
    /// الموسع كاملا، أو رسالة الخطأ العربي.
    pub fn expand(&self, line: &str) -> Result<String, String> {
        self.expand_in(&self.entries, line)
    }

    /// توسيع التاريخ مستثنيا آخر مدخل — لـ`محفوظات اطبع`
    /// (فسطر الأمر نفسه سُجَّل قبل التنفيذ، والتوسيع
    /// في باش يحدث وقت القراءة قبل التسجيل).
    pub fn expand_excluding_last(&self, line: &str) -> Result<String, String> {
        let end = self.entries.len().saturating_sub(1);
        self.expand_in(&self.entries[..end], line)
    }

    /// منطق التوسيع الخالص على شريحة مدخلات.
    fn expand_in(&self, entries: &[HistoryEntry], line: &str) -> Result<String, String> {
        let rest = &line[1..];
        if rest.is_empty() {
            return Err("خطأ: توسيع التاريخ يتوقع حدثا بعد !".to_string());
        }
        // `!!` + باق اختياري.
        if let Some(tail) = rest.strip_prefix('!') {
            let last = entries.last().ok_or_else(|| {
                "خطأ: لا يوجد أمر سابق للتوسيع".to_string()
            })?;
            return Ok(join_expanded(&last.text, tail));
        }
        // `!-عدد` + باق اختياري.
        if let Some(neg) = rest.strip_prefix('-') {
            let (num, tail) = split_number(neg);
            let n = num.ok_or_else(|| {
                "خطأ: توسيع التاريخ يتوقع عددا بعد !-".to_string()
            })?;
            let idx = resolve_in(entries, -n).ok_or_else(|| {
                "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات".to_string()
            })?;
            return Ok(join_expanded(&entries[idx].text, tail));
        }
        // `!عدد` + باق اختياري.
        if rest
            .chars()
            .next()
            .map(|c| c.is_ascii_digit() || ('٠'..='٩').contains(&c))
            .unwrap_or(false)
        {
            let (num, tail) = split_number(rest);
            let n = num.unwrap_or(0);
            let idx = resolve_in(entries, n).ok_or_else(|| {
                "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات".to_string()
            })?;
            return Ok(join_expanded(&entries[idx].text, tail));
        }
        // `!?نص؟` (الإغلاق `؟` أو `?` أو بلا إغلاق).
        if let Some(after) = rest.strip_prefix('?') {
            let (needle, tail) = split_search_needle(after);
            if needle.is_empty() {
                return Err("خطأ: البحث في المحفوظات يتوقع نصا بعد !؟".to_string());
            }
            let found = entries
                .iter()
                .rev()
                .find(|e| e.text.contains(needle))
                .ok_or_else(|| format!("خطأ: لا يوجد أمر يحتوي «{}» في المحفوظات", needle))?;
            return Ok(join_expanded(&found.text, tail));
        }
        // `!نص` + باق اختياري: آخر أمر يبدأ بالنص.
        let (needle, tail) = split_word(rest);
        let found = entries
            .iter()
            .rev()
            .find(|e| e.text.starts_with(needle))
            .ok_or_else(|| format!("خطأ: لا يوجد أمر يبدأ بـ«{}» في المحفوظات", needle))?;
        Ok(join_expanded(&found.text, tail))
    }

    /// الاستبدال السريع (152): سطر `^قديم^جديد^` (والأخيرة
    /// اختيارية) على آخر أمر — أول ظهور وحده.
    pub fn quick_subst_line(&self, line: &str) -> Result<String, String> {
        let body = &line[1..];
        let mut parts = body.splitn(3, '^');
        let old = parts.next().unwrap_or("");
        let new = match parts.next() {
            Some(n) => n,
            None => {
                return Err("خطأ: الاستبدال السريع صيغته ^قديم^جديد^".to_string());
            }
        };
        if old.is_empty() {
            return Err("خطأ: الاستبدال السريع يتوقع نصا قديما غير فارغ".to_string());
        }
        let last = self
            .entries
            .last()
            .ok_or_else(|| "خطأ: لا يوجد أمر سابق للاستبدال".to_string())?;
        match last.text.find(old) {
            Some(pos) => {
                let mut out = last.text.clone();
                out.replace_range(pos..pos + old.len(), new);
                Ok(out)
            }
            None => Err(format!("خطأ: «{}» غير موجود في آخر أمر", old)),
        }
    }

    /// تحرير أمر سابق وإعادة تنفيذه (154 — `fc -s`):
    /// استبدل أول ظهور في مدخل معطى (بالفهرس الداخلي).
    pub fn edit_entry(&self, index: usize, old: &str, new: &str) -> Result<String, String> {
        if old.is_empty() {
            return Err("خطأ: التحرير يتوقع نصا قديما غير فارغ".to_string());
        }
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| "خطأ: لا يوجد أمر بهذا الرقم في المحفوظات".to_string())?;
        match entry.text.find(old) {
            Some(pos) => {
                let mut out = entry.text.clone();
                out.replace_range(pos..pos + old.len(), new);
                Ok(out)
            }
            None => Err(format!("خطأ: «{}» غير موجود في هذا الأمر", old)),
        }
    }

    /// البحث العكسي (157): أحدث مدخل يحتوي النص، مع
    /// رقمه (1-based من الأقدم).
    pub fn search_reverse(&self, query: &str) -> Option<(usize, &HistoryEntry)> {
        self.entries
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.text.contains(query))
            .map(|(i, e)| (i + 1, e))
    }

    /// البحث العكسي مستثنيا آخر مدخل — لـ`محفوظات ابحث`
    /// (فسطر الأمر نفسه سُجَّل قبل التنفيذ ويحوي نص
    /// البحث حتما، فيجد نفسه بلا فائدة).
    pub fn search_reverse_excluding_last(
        &self,
        query: &str,
    ) -> Option<(usize, &HistoryEntry)> {
        let end = self.entries.len().saturating_sub(1);
        self.entries[..end]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.text.contains(query))
            .map(|(i, e)| (i + 1, e))
    }

    /// البحث العكسي الأقدم التالي (تكرار `Ctrl+R`):
    /// أحدث مطابق برقم أقل من `before` (1-based).
    pub fn search_reverse_older(
        &self,
        query: &str,
        before: usize,
    ) -> Option<(usize, &HistoryEntry)> {
        if before == 0 {
            return None;
        }
        self.entries
            .iter()
            .enumerate()
            .take(before - 1)
            .rev()
            .find(|(_, e)| e.text.contains(query))
            .map(|(i, e)| (i + 1, e))
    }

    /// السابق (السهم لأعلى / `ق`): من الأحدث إلى الأقدم.
    pub fn history_prev(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.nav {
            None => self.entries.len() - 1,
            Some(0) => return Some(self.entries[0].text.clone()),
            Some(i) => i - 1,
        };
        self.nav = Some(next);
        Some(self.entries[next].text.clone())
    }

    /// التالي (السهم لأسفل / `ت`): إلى الأحدث؛ وبعد
    /// الأحدث `None` فيستعيد المضيف مسودته.
    pub fn history_next(&mut self) -> Option<String> {
        let cur = self.nav?;
        if cur + 1 < self.entries.len() {
            self.nav = Some(cur + 1);
            Some(self.entries[cur + 1].text.clone())
        } else {
            self.nav = None;
            None
        }
    }

    /// مسار الملف الحالي (None = بلا ملف).
    pub fn file_path(&self) -> Option<&Path> {
        self.settings.file.as_deref()
    }

    /// عيّن ملف المحفوظات (None = بلا ملف). تغيير الملف
    /// يجعل كل المدخلات غير محفوظة فيه، ويصفّر عدّاد
    /// السطور المقروءة (فـ`اقرأ_جديد` بعده تقرأ الملف
    /// الجديد من أوله — والصحيح بعده `اقرأ`).
    pub fn set_file(&mut self, file: Option<PathBuf>) {
        self.settings.file = file;
        self.saved_len = 0;
        self.lines_read = 0;
    }

    /// عيّن حد الذاكرة (واقتطع الأقدم فورا عند التصغير).
    pub fn set_max_memory(&mut self, max: usize) {
        self.settings.max_memory = max;
        self.enforce_max_memory();
    }

    /// عيّن حد الملف (واقتطع الملف فورا عند التصغير
    /// إن وجد — بلا ملف أو بلا قراءة يبقى الحد للكتابة
    /// التالية). تعيد رسالة خطأ عربي عند تعذر الكتابة.
    pub fn set_max_file(&mut self, max: usize) -> Result<(), String> {
        self.settings.max_file = max;
        let Some(path) = self.settings.file.clone() else {
            return Ok(());
        };
        if !path.is_file() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|_| "تعذّر قراءة ملف المحفوظات".to_string())?;
        let (entries, _) = parse_file_content(&content);
        let keep: Vec<&HistoryEntry> = entries.iter().rev().take(max).collect();
        let mut kept: Vec<HistoryEntry> = keep.into_iter().rev().cloned().collect();
        if max == 0 {
            kept.clear();
        }
        let text = format_file_content(&kept);
        write_history_file(&path, &text)?;
        Ok(())
    }

    /// عيّن قواعد الحفظ.
    pub fn set_control(&mut self, dups: bool, space: bool, erase: bool) {
        self.settings.ignore_dups = dups;
        self.settings.ignore_space = space;
        self.settings.erase_dups = erase;
    }

    /// عيّن أنماط التجاهل.
    pub fn set_patterns(&mut self, patterns: Vec<String>) {
        self.settings.ignore_patterns = patterns;
    }

    /// عيّن صيغة الطابع (None = بلا طوابع في العرض).
    pub fn set_time_format(&mut self, format: Option<String>) {
        self.settings.time_format = format;
    }

    /// طابع مدخل معروضا حسب الصيغة (None = بلا صيغة
    /// أو بلا طابع).
    pub fn display_time(&self, entry: &HistoryEntry) -> Option<String> {
        self.settings
            .time_format
            .as_deref()
            .and_then(|f| format_time(f, entry.time))
    }

    /// اقرأ الملف كاملا إلى الذاكرة (-r): تستبدل
    /// الذاكرة بالملف، وكل المدخلات تصير محفوظة.
    /// الملف الغائب = ذاكرة فارغة بلا خطأ.
    pub fn load_file(&mut self) -> Result<usize, String> {
        let Some(path) = self.settings.file.clone() else {
            return Err("لا يوجد ملف محفوظات — عيّن ملف_المحفوظات أولا".to_string());
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err("تعذّر قراءة ملف المحفوظات".to_string()),
        };
        let (mut entries, lines) = parse_file_content(&content);
        if entries.len() > self.settings.max_file {
            let drop = entries.len() - self.settings.max_file;
            entries.drain(..drop);
        }
        self.lines_read = lines;
        self.entries = entries;
        self.saved_len = self.entries.len();
        self.nav = None;
        Ok(self.entries.len())
    }

    /// اقرأ الأسطر الجديدة فقط (-n): ما بعد السطور
    /// المستهلكة، تُلحق بالذاكرة بلا قواعد حفظ
    /// (فُحصت حين حُفظت)، وتصير محفوظة فورا.
    pub fn load_new_file(&mut self) -> Result<usize, String> {
        let Some(path) = self.settings.file.clone() else {
            return Err("لا يوجد ملف محفوظات — عيّن ملف_المحفوظات أولا".to_string());
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err("تعذّر قراءة ملف المحفوظات".to_string()),
        };
        let total_lines = content.lines().count();
        let new_text: String = content
            .lines()
            .skip(self.lines_read)
            .collect::<Vec<&str>>()
            .join("\n");
        let (mut entries, _) = parse_file_content(&new_text);
        let added = entries.len();
        self.entries.append(&mut entries);
        self.enforce_max_memory();
        self.saved_len = self.entries.len();
        self.lines_read = total_lines;
        self.nav = None;
        Ok(added)
    }

    /// ألحق الجديد فقط بالملف (-a وعند الخروج): مدخلات
    /// ما بعد `saved_len`؛ وإن تجاوز الملف حدّه أُعيدت
    /// كتابته بآخر `max_file` مدخلا. تعيد عدد ما أُلحق.
    pub fn append_file(&mut self) -> Result<usize, String> {
        let Some(path) = self.settings.file.clone() else {
            return Err("لا يوجد ملف محفوظات — عيّن ملف_المحفوظات أولا".to_string());
        };
        if self.saved_len >= self.entries.len() {
            return Ok(0);
        }
        if self.settings.max_file == 0 {
            // الحد صفر: الملف فارغ دائما، والمدخلات
            // تصير محفوظة (بكتابة فراغ).
            write_history_file(&path, "")?;
            self.saved_len = self.entries.len();
            return Ok(0);
        }
        let new_entries: Vec<HistoryEntry> = self.entries[self.saved_len..].to_vec();
        let appended = new_entries.len();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| "تعذّر إنشاء مجلد ملف المحفوظات".to_string())?;
        }
        let mut existing = String::new();
        if path.is_file() {
            existing = std::fs::read_to_string(&path)
                .map_err(|_| "تعذّر قراءة ملف المحفوظات".to_string())?;
        }
        let (mut all, _) = parse_file_content(&existing);
        all.extend(new_entries);
        if all.len() > self.settings.max_file {
            let drop = all.len() - self.settings.max_file;
            all.drain(..drop);
        }
        let text = format_file_content(&all);
        write_history_file(&path, &text)?;
        self.saved_len = self.entries.len();
        Ok(appended)
    }

    /// ألحق الجديد عند الخروج (:خروج وEOF و`استبدل`):
    /// كـ`append_file` لكنه لا ينشئ مجلدات أبدا — فالدخول
    /// التفاعلي أنشأها (`ensure_first_run`)، وغيابها
    /// يعني جلسة بلا مجلدات (اختبار أو WASM) فيُتخطى
    /// الحفظ بصمت بلا تلويث لشجرة المصدر.
    pub fn append_file_on_exit(&mut self) -> Result<usize, String> {
        let Some(path) = self.settings.file.clone() else {
            return Err("لا يوجد ملف محفوظات — عيّن ملف_المحفوظات أولا".to_string());
        };
        if self.saved_len >= self.entries.len() {
            return Ok(0);
        }
        match path.parent() {
            Some(parent) if parent.is_dir() => {}
            _ => return Ok(0),
        }
        self.append_file()
    }

    /// اكتب كل القائمة فوق الملف (-w): آخر `max_file`
    /// مدخلا من الذاكرة.
    pub fn save_file(&mut self) -> Result<(), String> {
        let Some(path) = self.settings.file.clone() else {
            return Err("لا يوجد ملف محفوظات — عيّن ملف_المحفوظات أولا".to_string());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| "تعذّر إنشاء مجلد ملف المحفوظات".to_string())?;
        }
        let keep: Vec<HistoryEntry> = if self.entries.len() > self.settings.max_file {
            self.entries[self.entries.len() - self.settings.max_file..].to_vec()
        } else {
            self.entries.clone()
        };
        let text = format_file_content(&keep);
        write_history_file(&path, &text)?;
        self.saved_len = self.entries.len();
        Ok(())
    }
}

/// حل رقم محفوظات على شريحة مدخلات (1 = الأقدم؛
/// والسالب من النهاية، -1 = الأخير).
fn resolve_in(entries: &[HistoryEntry], n: i64) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    if n > 0 {
        let i = (n - 1) as usize;
        if i < entries.len() {
            Some(i)
        } else {
            None
        }
    } else if n < 0 {
        let from_end = (-n) as usize;
        if from_end <= entries.len() {
            Some(entries.len() - from_end)
        } else {
            None
        }
    } else {
        None
    }
}

/// ألحق باقي السطر بعد المحدد بمسافة (باش يلحقه).
fn join_expanded(base: &str, tail: &str) -> String {
    let tail = tail.trim_start();
    if tail.is_empty() {
        base.to_string()
    } else {
        format!("{} {}", base, tail)
    }
}

/// اقسم بداية نص إلى عدد (عربي/لاتيني) وباقيه.
fn split_number(s: &str) -> (Option<i64>, &str) {
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || ('٠'..='٩').contains(&c) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return (None, s);
    }
    (parse_history_number(&s[..end]), &s[end..])
}

/// اقسم بداية نص إلى كلمة أولى (حتى أول فراغ) وباقيه.
fn split_word(s: &str) -> (&str, &str) {
    match s.find(|c: char| c.is_whitespace()) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

/// اقسم نص بحث `!?`: حتى إغلاق `؟` أو `?`، أو حتى النهاية.
fn split_search_needle(s: &str) -> (&str, &str) {
    match s.find(['؟', '?']) {
        Some(i) => {
            let ch_len = s[i..].chars().next().unwrap().len_utf8();
            (&s[..i], &s[i + ch_len..])
        }
        None => (s, ""),
    }
}

/// حلل محتوى ملف محفوظات إلى مدخلات وعدد سطوره:
/// سطر `#` + أرقام خالصا طابع للمدخل التالي، وما عداه
/// مدخل بطابع معلق (0 حين لا طابع).
/// سطر طابع زمني في ملف المحفوظات: `#` متبوعة
/// بأرقام لاتينية خالصة (والسطر `#` غير الرقمي
/// خالصا مدخل عادي لا طابع).
fn is_history_timestamp(line: &str) -> bool {
    line.len() > 1
        && line.starts_with('#')
        && line[1..].chars().all(|c| c.is_ascii_digit())
}

pub fn parse_file_content(content: &str) -> (Vec<HistoryEntry>, usize) {
    // الملف المنسّق (من `format_file_content`) يكتب
    // `#<حقبة>` قبل كل مدخل، فيكون المدخل متعدد
    // الأسطر (كتلة) سطورا تابعة للسطر الأول حتى
    // الطابع التالي — أما الملف العادي بلا أي طابع
    // (سطور مكتوبة يدويا) فيبقى كل سطر مدخلا وحده.
    let structured = content.lines().any(is_history_timestamp);
    let mut entries = Vec::new();
    let mut pending_time: u64 = 0;
    let mut fresh = false; // شوهد `#<حقبة>` ولم يستهلك بعد
    let mut lines = 0;
    for line in content.lines() {
        lines += 1;
        if is_history_timestamp(line) {
            pending_time = line[1..].parse().unwrap_or(0);
            fresh = true;
            continue;
        }
        if !structured || fresh || entries.is_empty() {
            entries.push(HistoryEntry {
                text: line.to_string(),
                time: pending_time,
            });
            fresh = false;
        } else {
            // سطر تابع لمدخل متعدد الأسطر (كتلة).
            let last = entries.last_mut().unwrap();
            last.text.push('\n');
            last.text.push_str(line);
        }
        pending_time = 0;
    }
    (entries, lines)
}

/// نسّق مدخلات لكتابتها في الملف: `#<طابع>` ثم النص
/// لكل مدخل (والطابع 0 يُكتب `#0` صراحة فيبقى الملف
/// قابلا للقراءة المتماثلة).
pub fn format_file_content(entries: &[HistoryEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        out.push('#');
        out.push_str(&e.time.to_string());
        out.push('\n');
        out.push_str(&e.text);
        out.push('\n');
    }
    out
}

/// اكتب ملف المحفوظات ذريا (عبر `install::write_atomic`
/// حيث أمكن، وإلا كتابة مباشرة).
fn write_history_file(path: &Path, text: &str) -> Result<(), String> {
    crate::install::write_atomic(path, text.as_bytes())
        .map_err(|_| "تعذّر كتابة ملف المحفوظات".to_string())
}

/// محرر سطر خالص (158 — جانب المحرك): نص + مؤشر أحرف،
/// بلا قراءة stdin وبلا طباعة. للمضيفين ذوي الإدخال
/// الخام (التطبيق)؛ وحلقة `run_repl` المطبوخة لا
/// تستخدمه. المؤشر على الأحرف لا البايتات (فالعربية
/// متعددة البايتات).
#[derive(Clone, Debug, Default)]
pub struct LineEditor {
    /// النص كأحرف.
    text: Vec<char>,
    /// المؤشر: عدد الأحرف قبله (0 = البداية).
    cursor: usize,
    /// آخر ما حُذف (للاستعادة).
    killed: Vec<char>,
}

impl LineEditor {
    /// محرر فارغ.
    pub fn new() -> Self {
        LineEditor::default()
    }

    /// محرر على نص، والمؤشر في النهاية.
    pub fn from_text(s: &str) -> Self {
        let text: Vec<char> = s.chars().collect();
        let cursor = text.len();
        LineEditor {
            text,
            cursor,
            killed: Vec::new(),
        }
    }

    /// النص الحالي.
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// موضع المؤشر (أحرف من البداية).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// أدخل نصا عند المؤشر.
    pub fn insert(&mut self, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        for (i, c) in chars.into_iter().enumerate() {
            self.text.insert(self.cursor + i, c);
        }
        self.cursor += s.chars().count();
    }

    /// احذف الحرف قبل المؤشر (رجوع).
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let c = self.text.remove(self.cursor);
            self.killed = vec![c];
        }
    }

    /// احذف الحرف عند المؤشر.
    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            let c = self.text.remove(self.cursor);
            self.killed = vec![c];
        }
    }

    /// المؤشر إلى البداية.
    pub fn move_start(&mut self) {
        self.cursor = 0;
    }

    /// المؤشر إلى النهاية.
    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// حرف واحد لليسار (للخلف).
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// حرف واحد لليمين (للأمام).
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor += 1;
        }
    }

    /// إلى بداية الكلمة السابقة (تخطي الفراغات ثم الكلمة).
    pub fn move_word_left(&mut self) {
        while self.cursor > 0 && self.text[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
        }
        while self.cursor > 0 && is_word_char(self.text[self.cursor - 1]) {
            self.cursor -= 1;
        }
    }

    /// إلى بداية الكلمة التالية (تخطي الكلمة ثم الفراغات).
    pub fn move_word_right(&mut self) {
        while self.cursor < self.text.len() && is_word_char(self.text[self.cursor]) {
            self.cursor += 1;
        }
        while self.cursor < self.text.len() && self.text[self.cursor].is_whitespace() {
            self.cursor += 1;
        }
    }

    /// احذف الكلمة قبل المؤشر (مع فراغاتها البادئة)
    /// واحفظها للاستعادة.
    pub fn delete_word(&mut self) {
        let end = self.cursor;
        let mut start = end;
        while start > 0 && self.text[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && is_word_char(self.text[start - 1]) {
            start -= 1;
        }
        if start < end {
            self.killed = self.text[start..end].to_vec();
            self.text.drain(start..end);
            self.cursor = start;
        }
    }

    /// احذف من المؤشر إلى النهاية واحفظه للاستعادة.
    pub fn delete_to_end(&mut self) {
        if self.cursor < self.text.len() {
            self.killed = self.text[self.cursor..].to_vec();
            self.text.truncate(self.cursor);
        }
    }

    /// احذف من البداية إلى المؤشر واحفظه للاستعادة.
    pub fn delete_to_start(&mut self) {
        if self.cursor > 0 {
            self.killed = self.text[..self.cursor].to_vec();
            self.text.drain(..self.cursor);
            self.cursor = 0;
        }
    }

    /// بدّل آخر حرفين قبل المؤشر.
    pub fn swap_last_two(&mut self) {
        if self.cursor >= 2 {
            self.text.swap(self.cursor - 2, self.cursor - 1);
        }
    }

    /// أعد إدخال آخر ما حُذف عند المؤشر.
    pub fn yank(&mut self) {
        let killed = self.killed.clone();
        for (i, c) in killed.into_iter().enumerate() {
            self.text.insert(self.cursor + i, c);
        }
        self.cursor += self.killed.len();
    }
}

/// حرف كلمة: حرف أو رقم أو `_` (عربي ولاتيني معا).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with(lines: &[&str]) -> History {
        let dir = std::env::temp_dir().join("niva-history-test");
        let mut h = History::new(&dir);
        h.set_file(None);
        for l in lines {
            h.record(l);
        }
        h
    }

    #[test]
    fn glob_match_basics() {
        assert!(glob_match("*", "أي شيء"));
        assert!(glob_match("ات", "ات"));
        assert!(!glob_match("ات", "اتصال"));
        assert!(glob_match("ات*", "اتصال"));
        assert!(glob_match("*ملف", "اعرض ملف"));
        assert!(glob_match("ا?رض", "اعرض"));
        assert!(!glob_match("ا?رض", "اعرض ملف"));
        assert!(glob_match("م*ف", "محفوظات ملف"));
        assert!(!glob_match("م*ل", "محفوظات ملف"));
        assert!(!glob_match("س*", "ص = ٥"));
    }

    #[test]
    fn parse_history_number_arabic_and_latin() {
        assert_eq!(parse_history_number("٣"), Some(3));
        assert_eq!(parse_history_number("-٢"), Some(-2));
        assert_eq!(parse_history_number("12"), Some(12));
        assert_eq!(parse_history_number(""), None);
        assert_eq!(parse_history_number("٣٫٥"), None);
        assert_eq!(parse_history_number("أ"), None);
    }

    #[test]
    fn record_applies_control_rules() {
        let dir = std::env::temp_dir().join("niva-history-test");
        let mut h = History::new(&dir);
        h.set_file(None);
        h.set_control(true, true, false);
        h.record("س = ٥");
        h.record("س = ٥");
        assert_eq!(h.len(), 1);
        h.record(" ص = ٧");
        assert_eq!(h.len(), 1);
        h.record("ص = ٧");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn record_erase_dups_removes_old_matches() {
        let mut h = history_with(&["أ", "ب", "أ"]);
        h.set_control(false, false, true);
        h.record("أ");
        assert_eq!(h.texts(), vec!["ب".to_string(), "أ".to_string()]);
    }

    #[test]
    fn record_ignores_patterns() {
        let mut h = history_with(&[]);
        h.set_patterns(vec!["ات".to_string(), "محفوظات*".to_string()]);
        h.record("ات");
        h.record("محفوظات ابحث س");
        h.record("س = ٥");
        assert_eq!(h.texts(), vec!["س = ٥".to_string()]);
    }

    #[test]
    fn record_enforces_max_memory() {
        let mut h = history_with(&[]);
        h.set_max_memory(3);
        for l in ["١", "٢", "٣", "٤"] {
            h.record(l);
        }
        assert_eq!(h.texts(), vec!["٢".to_string(), "٣".to_string(), "٤".to_string()]);
    }

    #[test]
    fn expand_forms() {
        let h = history_with(&["س = ٥", "اطبع س", "ص = ٧"]);
        assert_eq!(h.expand("!!").unwrap(), "ص = ٧");
        assert_eq!(h.expand("!١").unwrap(), "س = ٥");
        assert_eq!(h.expand("!2").unwrap(), "اطبع س");
        assert_eq!(h.expand("!-١").unwrap(), "ص = ٧");
        assert_eq!(h.expand("!-2").unwrap(), "اطبع س");
        assert_eq!(h.expand("!اطبع").unwrap(), "اطبع س");
        assert_eq!(h.expand("!?٧؟").unwrap(), "ص = ٧");
        assert_eq!(h.expand("!?س").unwrap(), "اطبع س");
        assert_eq!(h.expand("!! + ١").unwrap(), "ص = ٧ + ١");
        assert!(h.expand("!٩").is_err());
        assert!(h.expand("!غائب").is_err());
        assert!(h.expand("!?غائب؟").is_err());
    }

    #[test]
    fn expand_empty_history_errors() {
        let h = history_with(&[]);
        assert!(h.expand("!!").is_err());
        assert!(h.quick_subst_line("^أ^ب^").is_err());
    }

    #[test]
    fn quick_subst_first_occurrence_only() {
        let h = history_with(&["اطبع س + س"]);
        assert_eq!(h.quick_subst_line("^س^ص^").unwrap(), "اطبع ص + س");
        assert_eq!(h.quick_subst_line("^س^ص").unwrap(), "اطبع ص + س");
        assert!(h.quick_subst_line("^غائب^ص^").is_err());
        assert!(h.quick_subst_line("^^ص^").is_err());
        assert!(h.quick_subst_line("^س").is_err());
    }

    #[test]
    fn resolve_positive_and_negative() {
        let h = history_with(&["أ", "ب", "ج"]);
        assert_eq!(h.resolve(1), Some(0));
        assert_eq!(h.resolve(3), Some(2));
        assert_eq!(h.resolve(4), None);
        assert_eq!(h.resolve(-1), Some(2));
        assert_eq!(h.resolve(-3), Some(0));
        assert_eq!(h.resolve(-4), None);
        assert_eq!(h.resolve(0), None);
    }

    #[test]
    fn delete_single_and_range() {
        let mut h = history_with(&["أ", "ب", "ج", "د"]);
        assert!(h.delete(1));
        assert_eq!(h.texts(), vec!["أ".to_string(), "ج".to_string(), "د".to_string()]);
        assert!(!h.delete(9));
        h.delete_range(0, 1);
        assert_eq!(h.texts(), vec!["د".to_string()]);
    }

    #[test]
    fn search_reverse_latest_and_older() {
        let h = history_with(&["اطبع أ", "س = ٥", "اطبع ب"]);
        assert_eq!(h.search_reverse("اطبع").unwrap().0, 3);
        assert_eq!(h.search_reverse_older("اطبع", 3).unwrap().0, 1);
        assert!(h.search_reverse("غائب").is_none());
        assert_eq!(h.search_reverse_excluding_last("اطبع").unwrap().0, 1);
        assert!(h
            .search_reverse_excluding_last("المحفوظات")
            .is_none());
        assert!(h.search_reverse_older("اطبع", 1).is_none());
    }

    #[test]
    fn nav_prev_next_and_reset() {
        let mut h = history_with(&["أ", "ب", "ج"]);
        assert_eq!(h.history_prev().as_deref(), Some("ج"));
        assert_eq!(h.history_prev().as_deref(), Some("ب"));
        assert_eq!(h.history_prev().as_deref(), Some("أ"));
        assert_eq!(h.history_prev().as_deref(), Some("أ"));
        assert_eq!(h.history_next().as_deref(), Some("ب"));
        assert_eq!(h.history_next().as_deref(), Some("ج"));
        assert_eq!(h.history_next(), None);
        h.record("د");
        assert_eq!(h.history_prev().as_deref(), Some("د"));
    }

    #[test]
    fn file_roundtrip_with_timestamps() {
        let dir = std::env::temp_dir()
            .join(format!("niva-history-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = History::new(&dir);
        h.record("س = ٥");
        h.record("اطبع س");
        assert_eq!(h.append_file().unwrap(), 2);
        let path = dir.join("home").join("محفوظات");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("#"), "got: {}", content);
        let mut h2 = History::new(&dir);
        assert_eq!(h2.load_file().unwrap(), 2);
        assert_eq!(h2.texts(), vec!["س = ٥".to_string(), "اطبع س".to_string()]);
        // الإلحاق الثاني بلا جديد يكتب صفرا، والجديد يلحق وحده.
        assert_eq!(h2.append_file().unwrap(), 0);
        h2.record("ص = ٧");
        assert_eq!(h2.append_file().unwrap(), 1);
        let mut h3 = History::new(&dir);
        assert_eq!(h3.load_file().unwrap(), 3);
        // `اقرأ_جديد` بعد تحميل كامل لا يجلب شيئا.
        assert_eq!(h3.load_new_file().unwrap(), 0);
        // كتابة خارجية ثم `اقرأ_جديد` تجلب الجديد وحده.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("#0\nخارجي = ١\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(h3.load_new_file().unwrap(), 1);
        assert_eq!(h3.last_text(), Some("خارجي = ١"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_roundtrip_keeps_multiline_entry() {
        // الكتلة مدخل واحد في الملف وبعد التحميل:
        // سطورها التابعة لا تنقسم مدخلات.
        let dir = std::env::temp_dir()
            .join(format!("niva-history-multi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = History::new(&dir);
        h.set_max_file(10).unwrap();
        let block = "إذا صحيح:\n    اطبع ١\nس = ٥";
        h.record(block);
        h.record("س");
        h.append_file().unwrap();
        let mut h2 = History::new(&dir);
        h2.load_file().unwrap();
        assert_eq!(h2.texts(), vec![block.to_string(), "س".to_string()]);
        assert_eq!(h2.entries()[0].time, h.entries()[0].time);
        let _ = std::fs::remove_dir_all(&dir);
        // وملف عادي بلا أي طابع: كل سطر مدخل وحده.
        let (plain, lines) = parse_file_content("أ\nب\nج\n");
        assert_eq!(lines, 3);
        assert_eq!(plain.len(), 3);
        assert_eq!(plain[1].text, "ب");
        // وسطر `#` غير الرقمي مدخل عادي في الملف العادي.
        let (plain, _) = parse_file_content("#تعليق\nأ\n");
        assert_eq!(plain.len(), 2);
        assert_eq!(plain[0].text, "#تعليق");
    }

    #[test]
    fn file_max_truncates_oldest_on_append() {
        let dir = std::env::temp_dir()
            .join(format!("niva-history-max-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = History::new(&dir);
        h.set_max_file(2).unwrap();
        for l in ["١", "٢", "٣"] {
            h.record(l);
        }
        h.append_file().unwrap();
        let mut h2 = History::new(&dir);
        h2.set_max_file(2).unwrap();
        h2.load_file().unwrap();
        assert_eq!(h2.texts(), vec!["٢".to_string(), "٣".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_control_and_patterns() {
        assert_eq!(parse_control("تجاهل_المكرر تجاهل_المسافات").unwrap(), (true, true, false));
        assert_eq!(parse_control("تجاهل_الاثنين مسح_المكرر").unwrap(), (true, true, true));
        assert_eq!(parse_control("").unwrap(), (false, false, false));
        assert!(parse_control("غريب").is_err());
        assert_eq!(parse_patterns("ات محفوظات*"), vec!["ات".to_string(), "محفوظات*".to_string()]);
        assert!(parse_patterns("بلا").is_empty());
    }

    #[test]
    fn time_format_tokens() {
        assert!(check_time_format("%F %T").is_ok());
        assert!(check_time_format("%Y-%m-%d").is_ok());
        assert!(check_time_format("سنة %Y").is_ok());
        assert!(check_time_format("%Q").is_err());
        assert!(check_time_format("%").is_err());
        let out = format_time("%F %T", 1794063600).unwrap();
        assert!(out.contains("-"), "got: {}", out);
        assert!(out.contains(":"), "got: {}", out);
        assert!(format_time("%F", 0).is_none());
    }

    #[test]
    fn line_editor_arabic_chars() {
        let mut e = LineEditor::from_text("اطبع س");
        assert_eq!(e.cursor(), 6);
        e.move_word_left();
        assert_eq!(e.cursor(), 5);
        e.insert("ص + ");
        assert_eq!(e.text(), "اطبع ص + س");
        e.move_start();
        e.delete_word();
        assert_eq!(e.text(), "اطبع ص + س");
        e.move_end();
        e.backspace();
        assert_eq!(e.text(), "اطبع ص + ");
        e.yank();
        assert_eq!(e.text(), "اطبع ص + س");
        e.move_start();
        e.delete_to_end();
        assert_eq!(e.text(), "");
        e.yank();
        assert_eq!(e.text(), "اطبع ص + س");
    }

    #[test]
    fn line_editor_swap_and_word_delete() {
        let mut e = LineEditor::from_text("سص");
        e.swap_last_two();
        assert_eq!(e.text(), "صس");
        e.move_start();
        e.move_word_right();
        assert_eq!(e.cursor(), 2);
        e.move_start();
        e.delete_word();
        assert_eq!(e.text(), "صس");
        let mut e2 = LineEditor::from_text("س = ٥ + ص");
        e2.delete_to_start();
        assert_eq!(e2.text(), "");
        assert_eq!(e2.cursor(), 0);
        e2.yank();
        assert_eq!(e2.text(), "س = ٥ + ص");
    }
}
