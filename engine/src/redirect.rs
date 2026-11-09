//! محلل سطر التوجيه والأنابيب — المرحلة (ج)، اجتماعا 62 و63 (§77 و§78).
//!
//! خالص بلا إدخال/إخراج: يقسّم سطر REPL إلى مقاطع (`&&` / `||`) ثم
//! أنابيب (`|` / `|&`) ثم مراحل بتوجيهاتها. التنفيذ في `repl.rs`
//! (التحقق المسبق، وتشغيل المراحل، والكتابة اللاحقة).
//!
//! القواعد المثبتة بالاجتماعين:
//! - `>` و`<` المنفردان توجيه فقط حين يكون الهدف نصا مقتبسا أو
//!   مسارا يحوي `/` أو `.` أو يبدأ بـ`~` أو اسما خاصا؛ والعدد أو
//!   الاسم العاري بلا نقطة/شرطة هدف مقارنة لا توجيه (فـ`س = ٥ > ٣`
//!   مقارنة تبقى كما هي، ومن أراد ملفا بهذا الاسم اقتبسه).
//! - `>>` و`<<` و`<<<` و`<>` و`>|` و`>&` و`|&` بلا غموض أصلا
//!   (لا معاملات كهذه في اللغة).
//! - الواصفات 0 و1 و2 فقط؛ و`3>` وما فوق خطأ «واصف غير معروف».
//! - التوجيهات تُطبَّق بالتسلسل كما في باش.
//! - `&` وحده خلفية — مؤجلة لاجتماع المهام، فهو خطأ صيغة هنا.
//! - `{متغير}>ملف` موقوف — خطأ صيغة.
//! - مستند هنا واحد في السطر.
//! - `#` تعليق حتى نهاية السطر (كـالمعجم)، والسلاسل بعلامتي
//!   `"` و`'` مع هروب `\` (كـالمعجم) — فلا تقسيم داخلها.
//!
//! لا بايثون ولا اعتماديات جديدة: فحص نصي خالص على الأحرف.

/// مصدر الإدخال (الواصف 0) لمرحلة واحدة.
#[derive(Debug, Clone, PartialEq)]
pub enum StdinSrc {
    /// الطرفية كما هي (بلا توجيه).
    Terminal,
    /// نص جاهز (سلسلة هنا، أو محتوى ملف قُرئ في التحقق المسبق).
    Text(String),
    /// ملف يُقرأ في التحقق المسبق قبل أي تنفيذ.
    File(String),
    /// مغلق (`<&-`): `ادخل` يرجع "" فورا.
    Closed,
}

/// وجهة مخرج (الواصف 1 أو 2) لمرحلة واحدة.
#[derive(Debug, Clone, PartialEq)]
pub enum OutDst {
    /// مخرج السطر نفسه (الشاشة).
    Terminal,
    /// ملف: `append` للإلحاق، و`read_write` لـ`<>` (كتابة فوق
    /// أول الملف مع بقاء باقيه)، و`force` لـ`>|` (تجاوز منع المسح).
    File {
        path: String,
        append: bool,
        read_write: bool,
        force: bool,
    },
    /// أنبوب (مخرج هذه المرحلة مدخل التالية).
    Pipe,
    /// تجاهل (`>&-` — كـ`/dev/null`).
    Discard,
    /// واصف السطر نفسه (`Fd(1)` مخرجه، و`Fd(2)` خطؤه) — ناتج
    /// الازدواج حين تكون الوجهة الحالية طرفية.
    Fd(u8),
}

/// مواصفات مستند هنا (`<<` أو `<<-`).
#[derive(Debug, Clone, PartialEq)]
pub struct HeredocSpec {
    pub delimiter: String,
    pub strip_tabs: bool,
}

/// مرحلة واحدة في أنبوب: نصها (بلا توجيهاتها) ووجهاتها.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub text: String,
    pub stdin: StdinSrc,
    pub stdout: OutDst,
    pub stderr: OutDst,
    /// `|&`: خطأ هذه المرحلة ينضم لأنبوبها.
    pub pipe_err: bool,
    pub heredoc: Option<HeredocSpec>,
    pub here_string: Option<String>,
}

impl Stage {
    fn plain(text: String) -> Self {
        Stage {
            text,
            stdin: StdinSrc::Terminal,
            stdout: OutDst::Terminal,
            stderr: OutDst::Terminal,
            pipe_err: false,
            heredoc: None,
            here_string: None,
        }
    }

    /// هل تحمل هذه المرحلة أي توجيه (فالسطر يستحق الاعتراض)؟
    pub fn has_redirect(&self) -> bool {
        self.stdin != StdinSrc::Terminal
            || self.stdout != OutDst::Terminal
            || self.stderr != OutDst::Terminal
            || self.pipe_err
            || self.heredoc.is_some()
            || self.here_string.is_some()
    }
}

/// رابط بين مقطعين: الأول بلا رابط، ثم `&&` أو `||`
/// أو `&` (الخلفية: المقطع قبلها يجري في الخلفية
/// — اجتماع 64 §79).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegOp {
    First,
    And,
    Or,
    /// `&`: المقطع السابق خلفية؛ والمقطع التالي
    /// ينفَّذ فورا حسب رابطه (أو لا شيء بعده).
    Background,
}

/// مقطع: أنبوب واحد (مرحلة أو أكثر) مسبوق برابطه.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub op: SegOp,
    pub stages: Vec<Stage>,
}

/// سطر محلل كاملا.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    pub segments: Vec<Segment>,
}

impl ParsedLine {
    /// هل يستحق السطر اعتراض التوجيه/الأنابيب أصلا؟
    /// (بلا مقاطع متعددة ولا أنبوب ولا توجيه يمر كما هو.)
    pub fn has_work(&self) -> bool {
        if self.segments.len() > 1 {
            return true;
        }
        self.segments
            .iter()
            .any(|s| s.stages.len() > 1 || s.stages.iter().any(|st| st.has_redirect()))
    }

    /// أول مستند هنا في السطر (مقطع، مرحلة، مواصفاته).
    pub fn heredoc(&self) -> Option<(usize, usize, &HeredocSpec)> {
        for (si, seg) in self.segments.iter().enumerate() {
            for (ti, stage) in seg.stages.iter().enumerate() {
                if let Some(spec) = &stage.heredoc {
                    return Some((si, ti, spec));
                }
            }
        }
        None
    }

    /// عدد المستندات هنا في السطر كله.
    pub fn heredoc_count(&self) -> usize {
        self.segments
            .iter()
            .flat_map(|s| s.stages.iter())
            .filter(|st| st.heredoc.is_some())
            .count()
    }
}

/// خطأ تحليل سطر: رسالة عربية ورمز (2 سوء استخدام للصيغة،
/// و1 فشل توجيه — كـباش: خطأ التوجيه 1 وخطأ الصيغة 2).
#[derive(Debug, Clone, PartialEq)]
pub struct RedirectError {
    pub message: String,
    pub code: u8,
}

fn err2(message: &str) -> RedirectError {
    RedirectError {
        message: message.to_string(),
        code: 2,
    }
}

fn err1(message: String) -> RedirectError {
    RedirectError {
        message,
        code: 1,
    }
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || ('٠'..='٩').contains(&c)
}

fn digit_value(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        c.to_digit(10)
    } else if ('٠'..='٩').contains(&c) {
        Some(c as u32 - '٠' as u32)
    } else {
        None
    }
}

/// هل الهدف المقتبس أو العاري مسار (فيُقرأ توجيها لا مقارنة)؟
/// مسار: يحوي `/` أو `.` أو يبدأ بـ`~`.
fn is_path_like(s: &str) -> bool {
    s.contains('/') || s.contains('.') || s.starts_with('~')
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// ألغِ هروب المعجم في نص مقتبس (`\n` و`\t` و`\r` و`\\`
/// و`\"` و`\'`، وغيرها يبقى بشرطته كما في المعجم).
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// ماسح على أحرف السطر مع مواضعها البايتية، يحفظ حالة السلسلة
/// والعمق والأقواس — فالتقسيم لا يرى ما داخل سلسلة أو قوسين.
struct Scanner {
    chars: Vec<(usize, char)>,
}

impl Scanner {
    fn new(line: &str) -> Self {
        Scanner {
            chars: line.char_indices().collect(),
        }
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).map(|(_, c)| *c)
    }

    fn text(&self, from: usize, to: usize) -> String {
        self.chars[from..to].iter().map(|(_, c)| *c).collect()
    }
}

/// قسّم السطر على `&&` و`||` (خارج السلاسل والأقواس والتعليق).
/// يعيد المقاطع مع روابطها.
fn split_segments(line: &str) -> Result<Vec<(SegOp, String)>, RedirectError> {
    let sc = Scanner::new(line);
    let mut segments: Vec<(SegOp, String)> = Vec::new();
    let mut start = 0usize;
    let mut op = SegOp::First;
    let mut i = 0usize;
    let mut in_string: Option<char> = None;
    let mut depth: i32 = 0;
    while i < sc.len() {
        let c = sc.at(i).unwrap();
        if let Some(q) = in_string {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => in_string = Some(c),
            '#' if depth == 0 => break,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            '&' if depth == 0 => {
                if i > 0 && matches!(sc.at(i - 1), Some('|') | Some('>') | Some('<')) {
                    // `|&` (خطأ الأنبوب) أو `>&` / `<&`
                    // (ازدواج): `&` هنا جزء من العامل
                    // لا فاصل خلفية.
                    i += 1;
                    continue;
                }
                if sc.at(i + 1) == Some('&') {
                    segments.push((op, sc.text(start, i)));
                    op = SegOp::And;
                    i += 2;
                    start = i;
                    continue;
                }
                if sc.at(i + 1) == Some('>') {
                    // `&>` / `&>>`: عامل توجيه لا فاصل
                    // خلفية — يبقى للمرحلة.
                    i += 1;
                    continue;
                }
                // المرحلة (هـ) — `&` منفردة: فاصل خلفية
                // (اجتماع 64 §79): تخلّف المقطع قبلها
                // مباشرة، والتالي ينفَّذ فورا حسب رابطه.
                segments.push((op, sc.text(start, i)));
                op = SegOp::Background;
                i += 1;
                start = i;
                continue;
            }
            '|' if depth == 0 => {
                if i > 0 && sc.at(i - 1) == Some('>') {
                    // `>|` (القوة): هذه الأنبوبة جزء من
                    // عامل التوجيه لا فاصل مراحل — فـ`اطبع
                    // "أ" >| خ.txt` مرحلة واحدة لا مرحلتين.
                    i += 1;
                    continue;
                }
                if sc.at(i + 1) == Some('|') {
                    segments.push((op, sc.text(start, i)));
                    op = SegOp::Or;
                    i += 2;
                    start = i;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    segments.push((op, sc.text(start, sc.len())));
    Ok(segments)
}

/// قسّم مقطعا على `|` و`|&` (خارج السلاسل والأقواس والتعليق).
/// يعيد نصوص المراحل مع علم `|&` لكل مرحلة (خطؤها ينضم لأنبوبها).
fn split_pipeline(segment: &str) -> Result<Vec<(String, bool)>, RedirectError> {
    let sc = Scanner::new(segment);
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_string: Option<char> = None;
    let mut depth: i32 = 0;
    while i < sc.len() {
        let c = sc.at(i).unwrap();
        if let Some(q) = in_string {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => in_string = Some(c),
            '#' if depth == 0 => break,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            '|' if depth == 0 => {
                if i > 0 && sc.at(i - 1) == Some('>') {
                    // `>|` (القوة): هذه الأنبوبة جزء من
                    // عامل التوجيه لا فاصل مراحل — فـ`اطبع
                    // "أ" >| خ.txt` مرحلة واحدة لا مرحلتين.
                    i += 1;
                    continue;
                }
                if sc.at(i + 1) == Some('|') {
                    // `||` هنا مستحيل (قسّمته المقاطع) — لكن
                    // إن وُجد داخل قوسين فهو نص مرحلة لا فاصل.
                    i += 1;
                    continue;
                }
                let pipe_err = sc.at(i + 1) == Some('&');
                parts.push((sc.text(start, i), pipe_err));
                i += if pipe_err { 2 } else { 1 };
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push((sc.text(start, sc.len()), false));
    for (text, _) in &parts {
        if text.trim().is_empty() {
            return Err(err2("أنبوب بمرحلة فارغة"));
        }
    }
    Ok(parts)
}

/// معامل توجيه واحد كما يُقرأ من السطر.
enum Op {
    /// `>` أو `2>` أو `>|`: كتابة (مسح).
    Write {
        fd: u8,
        path: String,
        force: bool,
    },
    /// `>>`: إلحاق.
    Append { fd: u8, path: String },
    /// `<`: قراءة.
    Read { path: String },
    /// `<>`: قراءة وكتابة معا بلا مسح.
    ReadWrite { path: String },
    /// `>&` / `<&`: ازدواج أو إغلاق أو نقل.
    Dup {
        /// الواصف المُعاد توجيهه (الهدف).
        dst: u8,
        /// الواصف المصدر (1 أو 2 للكتابة، و0 للقراءة)، أو
        /// `None` للإغلاق (`-`).
        src: Option<u8>,
        /// `true` حين انتهى المعامل بـ`-` (نقل: انسخ ثم أغلق الأصل).
        mv: bool,
        /// `true` للقراءة (`<&`)، و`false` للكتابة (`>&`).
        input: bool,
    },
    /// `&>` / `&>>`: الخرج والخطأ معا إلى ملف واحد.
    Both { path: String, append: bool },
    /// `<<< "نص"`: سلسلة هنا.
    HereString { text: String },
    /// `<< محدد` / `<<- محدد`: مستند هنا.
    Heredoc { delimiter: String, strip_tabs: bool },
}

/// اقرأ معاملا (نصا مقتبسا أو كلمة عارية) بعد موضع ما.
/// يعيد (النص، مقتبس؟، موضع ما بعد المعامل).
fn read_operand(
    sc: &Scanner,
    mut i: usize,
) -> Result<(String, bool, usize), RedirectError> {
    while i < sc.len() && matches!(sc.at(i), Some(' ') | Some('\t')) {
        i += 1;
    }
    if i >= sc.len() {
        return Err(err2("توجيه بلا هدف"));
    }
    if matches!(sc.at(i), Some('"') | Some('\'')) {
        let quote = sc.at(i).unwrap();
        let mut j = i + 1;
        let mut raw = String::new();
        let mut closed = false;
        while j < sc.len() {
            let c = sc.at(j).unwrap();
            if c == '\\' && j + 1 < sc.len() {
                raw.push(c);
                raw.push(sc.at(j + 1).unwrap());
                j += 2;
                continue;
            }
            if c == quote {
                closed = true;
                j += 1;
                break;
            }
            raw.push(c);
            j += 1;
        }
        if !closed {
            return Err(err2("نص غير مغلق"));
        }
        return Ok((unescape(&raw), true, j));
    }
    let start = i;
    while i < sc.len() {
        let c = sc.at(i).unwrap();
        if c == ' '
            || c == '\t'
            || c == '>'
            || c == '<'
            || c == '|'
            || c == '&'
            || c == ';'
            || c == '"'
            || c == '\''
            || c == '#'
        {
            break;
        }
        i += 1;
    }
    if i == start {
        return Err(err2("توجيه بلا هدف"));
    }
    Ok((sc.text(start, i), false, i))
}

/// أرقام الواصف الملتصقة مباشرة قبل موضع ما (لاتينية وعربية).
/// يعيد (القيمة، موضع أول رقم) إن وُجدت وكانت بادئة صالحة
/// (بداية السطر أو فراغ أو `|` أو `;` أو `&` قبلها — فـ`س٢`
/// اسم لا واصف).
fn fd_prefix(sc: &Scanner, op_at: usize) -> Option<(u8, usize)> {
    let mut j = op_at;
    while j > 0 && sc.at(j - 1).map(is_digit).unwrap_or(false) {
        j -= 1;
    }
    if j == op_at {
        return None;
    }
    let ok_before = j == 0
        || matches!(sc.at(j - 1), Some(' ') | Some('\t') | Some('|') | Some(';') | Some('&'));
    if !ok_before {
        return None;
    }
    let mut value: u32 = 0;
    for k in j..op_at {
        value = value * 10 + digit_value(sc.at(k).unwrap()).unwrap_or(0);
        if value > 255 {
            return Some((255, j));
        }
    }
    Some((value as u8, j))
}

/// طبّق الأسماء الخاصة على معامل كتابة: يعيد وجهة جاهزة أو
/// `None` حين يبقى مسارا عاديا.
fn special_write(fd: u8, path: &str, stage: &Stage) -> Result<Option<OutDst>, RedirectError> {
    match path {
        "/dev/null" => Ok(Some(OutDst::Discard)),
        "/dev/stdout" => Ok(Some(if fd == 1 {
            OutDst::Terminal
        } else {
            stage.stdout.clone()
        })),
        "/dev/stderr" => Ok(Some(if fd == 2 {
            OutDst::Terminal
        } else {
            OutDst::Fd(2)
        })),
        "/dev/stdin" => Err(err1("لا يمكن الكتابة إلى /dev/stdin".to_string())),
        p if p.starts_with("/dev/tcp/") || p.starts_with("/dev/udp/") => Err(err1(
            "التوجيه إلى الشبكة موقوف — استخدم اجلب".to_string(),
        )),
        p if p.starts_with("/dev/fd/") => {
            Err(err1("الواصف /dev/fd غير مدعوم".to_string()))
        }
        _ => Ok(None),
    }
}

/// حلّل مرحلة واحدة (نصها بلا `|`): استخرج توجيهاتها اللاحقة
/// وطبّقها بالتسلسل على وجهاتها.
fn parse_stage(text: &str) -> Result<Stage, RedirectError> {
    let sc = Scanner::new(text);
    let mut stage = Stage::plain(String::new());
    let mut stmt_end: Option<usize> = None;
    let mut i = 0usize;
    let mut in_string: Option<char> = None;
    let mut depth: i32 = 0;
    while i < sc.len() {
        let c = sc.at(i).unwrap();
        if let Some(q) = in_string {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => in_string = Some(c),
            '#' if depth == 0 => break,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            '&' if depth == 0 => {
                // `&>` / `&>>` (الخرج والخطأ معا)؛ وأي `&`
                // منفردة أخرى قسّمها `split_segments` فاصلا
                // للخلفية قبل الوصول هنا — فوصولها هنا
                // يعني أنها جزء من مرحلة لا فاصل.
                if sc.at(i + 1) == Some('>') {
                    let append = sc.at(i + 2) == Some('>');
                    let after = i + if append { 3 } else { 2 };
                    let (path, _, next) = read_operand(&sc, after)?;
                    if stmt_end.is_none() {
                        stmt_end = Some(i);
                    }
                    apply_op(&mut stage, Op::Both { path, append })?;
                    i = next;
                    continue;
                }
                return Err(err2("الخلفية (&) فاصل بين المقاطع لا جزء من مرحلة"));
            }
            '>' | '<' if depth == 0 => {
                // `{متغير}>ملف` موقوف (§77 السؤال العاشر) —
                // يُفحص قبل قرار المقارنة (فهدفه قد يكون
                // اسما عاريا بلا نقطة فيُظن مقارنة).
                check_var_fd(&sc, i)?;
                let (op, next) = parse_redirect_op(&sc, i, c)?;
                match op {
                    // `>` أو `<` المنفرد بهدف غير مسار: مقارنة
                    // لا توجيه — يتوقف المسح هنا ويبقى السطر كودا.
                    None => break,
                    Some(op) => {
                        if stmt_end.is_none() {
                            stmt_end = Some(op_stmt_end(&sc, i));
                        }
                        apply_op(&mut stage, op)?;
                        i = next;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    let end = stmt_end.unwrap_or(sc.len());
    stage.text = sc.text(0, end).trim_end().to_string();
    Ok(stage)
}

/// موضع نهاية العبارة قبل معامل عند `i`: بداية أرقام الواصف
/// الملتصقة إن وُجدت، وإلا موضع المعامل نفسه.
fn op_stmt_end(sc: &Scanner, i: usize) -> usize {
    match fd_prefix(sc, i) {
        Some((_, start)) => start,
        None => i,
    }
}

/// ارفض صيغة `{متغير}>ملف` قبل معامل عند `i`.
fn check_var_fd(sc: &Scanner, i: usize) -> Result<(), RedirectError> {
    let before = sc.text(0, i);
    let trimmed = before.trim_end();
    if !trimmed.ends_with('}') {
        return Ok(());
    }
    if let Some(open) = trimmed.rfind('{') {
        let inner = &trimmed[open + 1..trimmed.len() - 1];
        if is_identifier(inner) {
            return Err(err2("متغير الواصف موقوف — {متغير}>ملف غير مدعوم"));
        }
    }
    Ok(())
}

/// حلّل معامل توجيه عند موضع `i` (حرفه `>` أو `<`).
/// يعيد (`None` حين كان `>`/`<` المنفرد مقارنة لا توجيها،
/// وموضع ما بعد المعامل).
fn parse_redirect_op(
    sc: &Scanner,
    i: usize,
    c: char,
) -> Result<(Option<Op>, usize), RedirectError> {
    let next = sc.at(i + 1);
    let next2 = sc.at(i + 2);
    if c == '>' {
        if next == Some('>') {
            let (fd, _) = fd_prefix(sc, i).unwrap_or((1, i));
            let (path, _, after) = read_operand(sc, i + 2)?;
            return Ok((Some(Op::Append { fd, path }), after));
        }
        if next == Some('|') {
            let (fd, _) = fd_prefix(sc, i).unwrap_or((1, i));
            let (path, _, after) = read_operand(sc, i + 2)?;
            return Ok((Some(Op::Write { fd, path, force: true }), after));
        }
        if next == Some('&') {
            return parse_dup_op(sc, i, false);
        }
        // `>` المنفرد: واصفه 1 أو 2 صراحة، وإلا 1.
        let (fd, _) = fd_prefix(sc, i).unwrap_or((1, i));
        let (path, quoted, after) = read_operand(sc, i + 1)?;
        if !quoted && !is_path_like(&path) {
            return Ok((None, i));
        }
        return Ok((Some(Op::Write { fd, path, force: false }), after));
    }
    // `<`: المستند هنا والسلسلة هنا والقراءة-الكتابة
    // للمدخل (الواصف 0) وحده — فأي رقم غير 0 مرفوض.
    if let Some((fd, _)) = fd_prefix(sc, i) {
        if next != Some('&') && fd != 0 {
            return Err(err2(&format!("واصف غير معروف: {}", fd)));
        }
    }
    if next == Some('<') {
        if next2 == Some('-') {
            let (delim, _, after) = read_operand(sc, i + 3)?;
            return Ok((
                Some(Op::Heredoc {
                    delimiter: delim,
                    strip_tabs: true,
                }),
                after,
            ));
        }
        if next2 == Some('<') {
            let (text, _, after) = read_operand(sc, i + 3)?;
            return Ok((Some(Op::HereString { text }), after));
        }
        let (delim, _, after) = read_operand(sc, i + 2)?;
        return Ok((
            Some(Op::Heredoc {
                delimiter: delim,
                strip_tabs: false,
            }),
            after,
        ));
    }
    if next == Some('>') {
        let (path, _, after) = read_operand(sc, i + 2)?;
        return Ok((Some(Op::ReadWrite { path }), after));
    }
    if next == Some('&') {
        return parse_dup_op(sc, i, true);
    }
    let (path, quoted, after) = read_operand(sc, i + 1)?;
    if !quoted && !is_path_like(&path) {
        return Ok((None, i));
    }
    Ok((Some(Op::Read { path }), after))
}

/// حلّل `>&` أو `<&` عند موضع `i` (معامله `-` أو رقم أو كلمة).
fn parse_dup_op(
    sc: &Scanner,
    i: usize,
    input: bool,
) -> Result<(Option<Op>, usize), RedirectError> {
    let (dst, _) = if input {
        fd_prefix(sc, i).unwrap_or((0, i))
    } else {
        fd_prefix(sc, i).unwrap_or((1, i))
    };
    let mut j = i + 2;
    while j < sc.len() && matches!(sc.at(j), Some(' ') | Some('\t')) {
        j += 1;
    }
    if j >= sc.len() {
        return Err(err2("توجيه بلا هدف"));
    }
    if sc.at(j) == Some('-') {
        return Ok((
            Some(Op::Dup {
                dst,
                src: None,
                mv: false,
                input,
            }),
            j + 1,
        ));
    }
    // رقم (مع `-` لاحقة اختيارية للنقل).
    if sc.at(j).map(is_digit).unwrap_or(false) {
        let start = j;
        while j < sc.len() && sc.at(j).map(is_digit).unwrap_or(false) {
            j += 1;
        }
        let mut value: u32 = 0;
        for k in start..j {
            value = value * 10 + digit_value(sc.at(k).unwrap()).unwrap_or(0);
        }
        let mv = sc.at(j) == Some('-');
        if mv {
            j += 1;
        }
        return Ok((
            Some(Op::Dup {
                dst,
                src: Some(value as u8),
                mv,
                input,
            }),
            j,
        ));
    }
    // كلمة: مع الواصف 1 للكتابة تعني `&>`؛ وغيرها غامض.
    let (word, _, after) = read_operand(sc, j)?;
    if !input && dst == 1 {
        return Ok((Some(Op::Both { path: word, append: false }), after));
    }
    Err(err2("توجيه غامض"))
}

/// طبّق معامل واحد على وجهات المرحلة (بالتسلسل كما في باش).
fn apply_op(stage: &mut Stage, op: Op) -> Result<(), RedirectError> {
    match op {
        Op::Write { fd, path, force } => {
            if fd != 1 && fd != 2 {
                return Err(err2(&format!("واصف غير معروف: {}", fd)));
            }
            let file = OutDst::File {
                path: path.clone(),
                append: false,
                read_write: false,
                force,
            };
            let dst = match special_write(fd, &path, stage)? {
                Some(d) => d,
                None => file,
            };
            if fd == 1 {
                stage.stdout = dst;
            } else {
                stage.stderr = dst;
            }
        }
        Op::Append { fd, path } => {
            if fd != 1 && fd != 2 {
                return Err(err2(&format!("واصف غير معروف: {}", fd)));
            }
            let file = OutDst::File {
                path: path.clone(),
                append: true,
                read_write: false,
                force: false,
            };
            let dst = match special_write(fd, &path, stage)? {
                Some(d) => d,
                None => file,
            };
            if fd == 1 {
                stage.stdout = dst;
            } else {
                stage.stderr = dst;
            }
        }
        Op::Read { path } => {
            stage.stdin = match path.as_str() {
                "/dev/null" => StdinSrc::Text(String::new()),
                "/dev/stdin" => StdinSrc::Terminal,
                "/dev/stdout" | "/dev/stderr" => {
                    return Err(err1(format!("لا يمكن القراءة من {}", path)));
                }
                p if p.starts_with("/dev/tcp/")
                    || p.starts_with("/dev/udp/")
                    || p.starts_with("/dev/fd/") =>
                {
                    return Err(err1(
                        "التوجيه إلى الشبكة موقوف — استخدم اجلب".to_string(),
                    ));
                }
                _ => StdinSrc::File(path),
            };
            stage.heredoc = None;
            stage.here_string = None;
        }
        Op::ReadWrite { path } => {
            if path == "/dev/null" {
                stage.stdin = StdinSrc::Text(String::new());
                stage.stdout = OutDst::Discard;
            } else if path == "/dev/stdin" {
                return Err(err1(
                    "لا يمكن القراءة والكتابة في /dev/stdin".to_string(),
                ));
            } else if path.starts_with("/dev/tcp/")
                || path.starts_with("/dev/udp/")
                || path.starts_with("/dev/fd/")
            {
                return Err(err1(
                    "التوجيه إلى الشبكة موقوف — استخدم اجلب".to_string(),
                ));
            } else {
                stage.stdin = StdinSrc::File(path.clone());
                stage.stdout = OutDst::File {
                    path,
                    append: false,
                    read_write: true,
                    force: false,
                };
            }
            stage.heredoc = None;
            stage.here_string = None;
        }
        Op::Dup {
            dst,
            src,
            mv,
            input,
        } => {
            if input {
                if dst != 0 {
                    return Err(err2(&format!("واصف غير معروف: {}", dst)));
                }
                match src {
                    None => stage.stdin = StdinSrc::Closed,
                    Some(0) => {
                        if mv {
                            stage.stdin = StdinSrc::Closed;
                        }
                    }
                    Some(other) => {
                        return Err(err2(&format!("واصف غير معروف: {}", other)));
                    }
                }
            } else {
                if dst != 1 && dst != 2 {
                    return Err(err2(&format!("واصف غير معروف: {}", dst)));
                }
                match src {
                    None => {
                        if dst == 1 {
                            stage.stdout = OutDst::Discard;
                        } else {
                            stage.stderr = OutDst::Discard;
                        }
                    }
                    Some(s) => {
                        if s != 1 && s != 2 {
                            return Err(err2(&format!("واصف غير معروف: {}", s)));
                        }
                        let target = if s == 1 {
                            match &stage.stdout {
                                OutDst::Terminal => OutDst::Fd(1),
                                other => other.clone(),
                            }
                        } else {
                            match &stage.stderr {
                                OutDst::Terminal => OutDst::Fd(2),
                                other => other.clone(),
                            }
                        };
                        if dst == 1 {
                            stage.stdout = target;
                        } else {
                            stage.stderr = target;
                        }
                        if mv {
                            if s == 1 {
                                stage.stdout = OutDst::Discard;
                            } else {
                                stage.stderr = OutDst::Discard;
                            }
                        }
                    }
                }
            }
        }
        Op::Both { path, append } => {
            let file = OutDst::File {
                path: path.clone(),
                append,
                read_write: false,
                force: false,
            };
            let dst = match special_write(1, &path, stage)? {
                Some(OutDst::Terminal) => OutDst::Terminal,
                Some(OutDst::Fd(2)) => OutDst::Fd(2),
                Some(other) => other,
                None => file,
            };
            // `/dev/stdout` مع `&>` تعني الطرفية للاثنين.
            stage.stdout = dst.clone();
            stage.stderr = dst;
        }
        Op::HereString { text } => {
            stage.here_string = Some(text);
            stage.heredoc = None;
            stage.stdin = StdinSrc::Terminal;
        }
        Op::Heredoc {
            delimiter,
            strip_tabs,
        } => {
            if stage.heredoc.is_some() {
                return Err(err2("مستند هنا واحد في السطر"));
            }
            stage.heredoc = Some(HeredocSpec {
                delimiter,
                strip_tabs,
            });
            stage.here_string = None;
            stage.stdin = StdinSrc::Terminal;
        }
    }
    Ok(())
}

/// حلّل سطرا كاملا: مقاطع ثم أنابيب ثم توجيهات كل مرحلة.
pub fn parse_line(line: &str) -> Result<ParsedLine, RedirectError> {
    let mut segments = Vec::new();
    let raw = split_segments(line)?;
    let n_raw = raw.len();
    for (idx, (op, seg_text)) in raw.into_iter().enumerate() {
        // `&` أخيرة: المقطع الفارغ بعدها علامة خلفية
        // للمقطع قبلها لا مرحلة فارغة (فـ`شغل x &`
        // مقطع خلفي واحد وعلامة — لا خطأ).
        if op == SegOp::Background && seg_text.trim().is_empty() && idx + 1 == n_raw {
            segments.push(Segment {
                op,
                stages: Vec::new(),
            });
            continue;
        }
        let mut stages = Vec::new();
        for (text, pipe_err) in split_pipeline(&seg_text)? {
            let mut stage = parse_stage(&text)?;
            stage.pipe_err = pipe_err;
            stages.push(stage);
        }
        // ضبط الأنبوب لغير الأخيرة بعد اكتمال القائمة
        // (عدد المراحل معروف الآن).
        let n = stages.len();
        for (idx, stage) in stages.iter_mut().enumerate() {
            if idx + 1 < n && stage.stdout == OutDst::Terminal {
                stage.stdout = OutDst::Pipe;
            }
        }
        // `|&`: الخطأ ينسخ وجهة الخرج الحالية (بعد تثبيت
        // الأنبوب) — فيتجاوز أي توجيه خطأ سابق كما في
        // باش (الازدواج يُلحق آخر قائمة التوجيهات).
        for stage in stages.iter_mut() {
            if stage.pipe_err {
                stage.stderr = stage.stdout.clone();
            }
        }
        segments.push(Segment { op, stages });
    }
    let parsed = ParsedLine { segments };
    if parsed.heredoc_count() > 1 {
        return Err(err2("مستند هنا واحد في السطر"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_of(line: &str) -> Stage {
        let parsed = parse_line(line).expect(line);
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].stages.len(), 1);
        parsed
            .segments
            .into_iter()
            .next()
            .unwrap()
            .stages
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn write_redirect() {
        let s = stage_of("اطبع \"أ\" > خ.txt");
        assert_eq!(s.text, "اطبع \"أ\"");
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
        assert_eq!(s.stderr, OutDst::Terminal);
    }

    #[test]
    fn append_and_force() {
        // `>|` مرحلة واحدة (لا يُقسَّم كأنبوب).
        let s = stage_of("اطبع \"أ\" >| خ.txt");
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: true,
            }
        );
        let s = stage_of("اطبع \"أ\" >> خ.txt");
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: true,
                read_write: false,
                force: false,
            }
        );
        let s = stage_of("اطبع \"أ\" >| خ.txt");
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: true,
            }
        );
    }

    #[test]
    fn read_and_read_write() {
        let s = stage_of("ادخل < خ.txt");
        assert_eq!(s.stdin, StdinSrc::File("خ.txt".to_string()));
        let s = stage_of("ادخل <> خ.txt");
        assert_eq!(s.stdin, StdinSrc::File("خ.txt".to_string()));
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: true,
                force: false,
            }
        );
    }

    #[test]
    fn numeric_fd_and_dup() {
        let s = stage_of("شغل غائب 2> خطأ.txt");
        assert_eq!(
            s.stderr,
            OutDst::File {
                path: "خطأ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
        let s = stage_of("اطبع \"أ\" > خ.txt 2>&1");
        assert_eq!(
            s.stderr,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
        // الترتيب حاسم: `2>&1 >ملف` يبقي الخطأ على الطرفية.
        let s = stage_of("اطبع \"أ\" 2>&1 > خ.txt");
        assert_eq!(s.stderr, OutDst::Fd(1));
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
    }

    #[test]
    fn both_and_pipe_err() {
        let s = stage_of("اطبع \"أ\" &> خ.txt");
        assert_eq!(
            s.stdout,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
        assert_eq!(s.stdout, s.stderr);
        let parsed = parse_line("اطبع \"أ\" |& عد_الكلمات").unwrap();
        assert_eq!(parsed.segments[0].stages.len(), 2);
        assert!(parsed.segments[0].stages[0].pipe_err);
        assert_eq!(parsed.segments[0].stages[0].stdout, OutDst::Pipe);
        // `|&` ينسخ وجهة الخرج (الأنبوب) إلى الخطأ —
        // فينسخ توجيه خطأ سابق لو وجد.
        assert_eq!(parsed.segments[0].stages[0].stderr, OutDst::Pipe);
        let parsed = parse_line("فشل 2> خ.txt |& عد_الكلمات").unwrap();
        assert_eq!(parsed.segments[0].stages[0].stderr, OutDst::Pipe);
    }

    #[test]
    fn close_and_move() {
        let s = stage_of("اطبع \"أ\" >&-");
        assert_eq!(s.stdout, OutDst::Discard);
        let s = stage_of("اطبع \"أ\" 2>&-");
        assert_eq!(s.stderr, OutDst::Discard);
        let s = stage_of("ادخل <&-");
        assert_eq!(s.stdin, StdinSrc::Closed);
        let s = stage_of("اطبع \"أ\" > خ.txt 2>&1-");
        assert_eq!(s.stdout, OutDst::Discard);
        assert_eq!(
            s.stderr,
            OutDst::File {
                path: "خ.txt".to_string(),
                append: false,
                read_write: false,
                force: false,
            }
        );
    }

    #[test]
    fn input_fd_rules() {
        // `<>` و`<<` و`<<<` و`<` المنفرد للواصف 0
        // وحده؛ فأي رقم غير 0 «واصف غير معروف» (رمز 2).
        for line in ["ادخل 2< خ.txt", "ادخل 2<< نهاية", "ادخل 2<<< \"نص\"", "ادخل 2<> خ.txt"] {
            let err = parse_line(line).unwrap_err();
            assert_eq!(err.code, 2);
            assert!(err.message.contains("واصف غير معروف"));
        }
        // والواصف 0 صراحة مقبول في القراءة.
        let s = stage_of("ادخل 0< خ.txt");
        assert_eq!(s.stdin, StdinSrc::File("خ.txt".to_string()));
    }

    #[test]
    fn heredoc_and_here_string() {
        let s = stage_of("ادخل << نهاية");
        assert_eq!(
            s.heredoc,
            Some(HeredocSpec {
                delimiter: "نهاية".to_string(),
                strip_tabs: false,
            })
        );
        let s = stage_of("ادخل <<- نهاية");
        assert_eq!(
            s.heredoc,
            Some(HeredocSpec {
                delimiter: "نهاية".to_string(),
                strip_tabs: true,
            })
        );
        let s = stage_of("ادخل <<< \"نص هنا\"");
        assert_eq!(s.here_string, Some("نص هنا".to_string()));
    }

    #[test]
    fn comparison_is_not_redirect() {
        // مقارنة تبقى كودا: بلا عمل توجيه فلا اعتراض.
        for line in ["س = ٥ > ٣", "٥ > ٣", "اطبع \"أ\" > خ", "س < ص"] {
            let parsed = parse_line(line).unwrap();
            assert!(!parsed.has_work(), "{}", line);
        }
        // والاسم المقتبس توجيه.
        let parsed = parse_line("اطبع \"أ\" > \"خ\"").unwrap();
        assert!(parsed.has_work());
    }

    #[test]
    fn pipe_and_connectors() {
        let parsed = parse_line("اطبع \"أ ب\" | عد_الكلمات").unwrap();
        assert_eq!(parsed.segments[0].stages.len(), 2);
        assert_eq!(parsed.segments[0].stages[0].stdout, OutDst::Pipe);
        let parsed = parse_line("شغل غائب || اطبع \"بديل\"").unwrap();
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[1].op, SegOp::Or);
        let parsed = parse_line("نجاح && اطبع \"تم\"").unwrap();
        assert_eq!(parsed.segments[1].op, SegOp::And);
    }

    #[test]
    fn special_names() {
        let s = stage_of("اطبع \"أ\" > /dev/null");
        assert_eq!(s.stdout, OutDst::Discard);
        let s = stage_of("ادخل < /dev/null");
        assert_eq!(s.stdin, StdinSrc::Text(String::new()));
        let e = parse_line("اطبع \"أ\" > /dev/tcp/مثال/80").unwrap_err();
        assert_eq!(e.code, 1);
    }

    #[test]
    fn errors() {
        // واصف حر مرفوض.
        assert_eq!(parse_line("اطبع \"أ\" 3> خ.txt").unwrap_err().code, 2);
        // متغير الواصف موقوف.
        assert_eq!(
            parse_line("اطبع \"أ\" {و}>ملف").unwrap_err().code,
            2
        );
        // المرحلة (هـ): `&` أخيرة تُحلَّل لا خطأ —
        // الخطأ يأتي عند التنفيذ إن لم تكن خلفية
        // صالحة (عمليات نظام أو `شغل`).
        let parsed = parse_line("اطبع \"أ\" &").unwrap();
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[1].op, SegOp::Background);
        assert!(parsed.segments[1].stages.is_empty());
        // `&` وحيدة بلا مقطع قبلها: مرحلة فارغة.
        assert_eq!(parse_line("&").unwrap_err().code, 2);
        // مرحلة فارغة في الأنبوب.
        assert_eq!(parse_line("اطبع \"أ\" |").unwrap_err().code, 2);
        // مستندان في سطر واحد.
        assert_eq!(
            parse_line("ادخل << أ | ادخل << ب").unwrap_err().code,
            2
        );
        // توجيه بلا هدف.
        assert_eq!(parse_line("اطبع \"أ\" >").unwrap_err().code, 2);
        // `>` داخل سلسلة لا يقسّم.
        let s = stage_of("اطبع \"أ > ب\"");
        assert_eq!(s.text, "اطبع \"أ > ب\"");
        assert!(!parse_line("اطبع \"أ > ب\"").unwrap().has_work());
    }

    // المرحلة (هـ) — `&` فاصل خلفية (اجتماع 64 §79).
    #[test]
    fn background_separator() {
        // وسطا: المقطع قبلها خلفية، والتالي أول.
        let parsed = parse_line("شغل أ.نيفا & اطبع ١").unwrap();
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[0].op, SegOp::First);
        assert_eq!(parsed.segments[0].stages.len(), 1);
        assert_eq!(parsed.segments[0].stages[0].text, "شغل أ.نيفا");
        assert_eq!(parsed.segments[1].op, SegOp::Background);
        assert_eq!(parsed.segments[1].stages[0].text.trim(), "اطبع ١");
        // آخرا: علامة فارغة بعدها.
        let parsed = parse_line("شغل أ.نيفا &").unwrap();
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[1].op, SegOp::Background);
        assert!(parsed.segments[1].stages.is_empty());
        assert!(parsed.has_work());
        // `&&` تبقى شرطا لا خلفية.
        let parsed = parse_line("شغل أ.نيفا && اطبع ١").unwrap();
        assert_eq!(parsed.segments[1].op, SegOp::And);
        // `&>` تبقى توجيها لا فاصلا.
        let s = stage_of("اطبع \"أ\" &> خ.txt");
        assert_eq!(s.text, "اطبع \"أ\"");
        assert!(matches!(s.stdout, OutDst::File { .. }));
        assert_eq!(s.stderr, s.stdout);
        // `&` داخل سلسلة لا تقسّم.
        let parsed = parse_line("اطبع \"أ & ب\"").unwrap();
        assert_eq!(parsed.segments.len(), 1);
        // `& &` (مقطع فارغ وسطا): خطأ صيغة.
        assert_eq!(parse_line("شغل أ.نيفا & & اطبع ١").unwrap_err().code, 2);
    }

    #[test]
    fn empty_statement_redirect_only() {
        // `> ملف` وحده: عبارة فارغة وتوجيه — ينشئ/يمسح الملف.
        let s = stage_of("> خ.txt");
        assert_eq!(s.text, "");
        assert!(matches!(s.stdout, OutDst::File { .. }));
    }
}
