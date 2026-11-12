//! Lexer: Arabic source -> tokens, with INDENT/DEDENT for blocks.
//! Forgiving rules (keywords only): diacritics are skipped outside strings,
//! hamza variants are unified for keyword lookup, Arabic-Indic digits and
//! Arabic punctuation (، ؛) are accepted. Strings and comments are never
//! altered. Columns count characters, not bytes.

use crate::error::{RioError, RioResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Print,    // اطبع
    If,       // إذا
    Else,     // وإلا
    Elif,     // اواذا (elif بكلمة واحدة — الدفعة 2)
    While,    // بينما
    For,      // لكل
    In,       // في
    Func,     // دالة
    Lambda,   // دالة_مجهولة (Step 1, Meeting 13)
    Return,   // ارجع
    And,      // و
    Or,       // أو
    Not,      // ليس
    True,     // صحيح
    False,    // خطأ
    Nil,      // لاشيء
    Break,    // اكسر
    Continue, // تابع
    Pass,     // مرر
    Try,      // حاول
    Except,   // ماعدا
    Finally,  // أخيرا
    With,     // عند (البند 88 — مدير السياق)
    Assert,   // متوقع (البند 81 — الدفعة 2 من الـ579)
    Match,    // طابق (البند 99 — الدفعة 3 من الـ579)
    Case,     // حال (البند 100 — الدفعة 3 من الـ579)
    Class,    // فئة
    Create,   // أنشئ (إنشاء كائن من فئة)
    Inherit,  // ورث
    Interface, // واجهة
    Import,   // استورد
    Export,   // صدّر (Phase 1A Step 2: modules)
    As,       // باسم
    From,     // من
    Repeat,   // كرر
    Times,    // مرات
    AsLongAs, // طالما
    Greater,  // أكبر (كلمة مقارنة: أكبر من)
    Smaller,  // أصغر (كلمة مقارنة: أصغر من)
    Equals,   // يساوي (كلمة مقارنة: ==)
    No,       // لا (كلمة مقارنة: لا يساوي)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(f64),
    // ج6 (الدفعة الأخيرة — الأنواع والأعداد والدقة):
    // صيغ حرفية جديدة — تُفكَّك في المحلل إلى مدمجات
    // مسطحة (سابقة `مترابطة`/`تشكيلة` والنص التنفيذي)،
    // فلا عقدة AST جديدة ولا لمس لـcgen.rs:
    // `123x` كبير عشري نصي، و`1r2` كسر (بسط، مقام)،
    // و`0j1` و`4ت` مركب (حقيقي، تخيلي).
    BigInt(String),
    Rational(String, String),
    Complex(f64, f64),
    Str(String),
    /// نص تنفيذي `ف"..."` (البنود 163–165، الدفعة 2 من
    /// الـ579): المحتوى الخام بين علامتي الاقتباس، يفككه
    /// المحلل إلى أجزاء نصية وحقول `{تعبير}`.
    FStr(String),
    Keyword(Keyword),
    Plus,
    // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
    PlusEq,
// == مولّد-دفعة2-بدائل-الرموز: بداية ==
    SlashSlashEq,
    CaretCaretEq,
    ShlEq,
    ShrEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    SlashSlash,
    CaretCaret,
    Shl,
    Shr,
    AmpEq,
    PipeEq,
    CaretEq,
    Pipe,
    Amp,
    Caret,
    Tilde,
// == مولّد-دفعة2-بدائل-الرموز: نهاية ==
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    // البند 73 (`:=` — الاجتماع 83 §98): تعبير الإسناد.
    ColonEq,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Comma,
    Semicolon,
    Dot, // .  (Phase 1A Step 2: module.member; numbers consume '.' internally)
    // البند 8 — الزخارف (الاجتماع 72 §87): `@` رمز زخرفة
    // في بداية سطر قبل تعريف دالة أو فئة فقط؛ وفي أي
    // موضع آخر يرفضه المحلل النحوي (لا استيفاء خارج
    // كتلة `_س_`، ولا كتلة `_س_` أصلا في هذا البند).
    At,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

fn is_diacritic(c: char) -> bool {
    ('\u{064B}'..='\u{0652}').contains(&c) || c == '\u{0670}' || c == '\u{0640}'
}

fn normalize_keyword(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            'ؤ' => 'و',
            'ئ' => 'ي',
            'ة' => 'ه',
            'ى' => 'ي',
            other => other,
        })
        .collect()
}

/// تطبيع كلمة أمر/علم في CLI بنفس قواعد المحلل تماما:
/// تجريد التشكيل والتطويل (is_diacritic) ثم توحيد الهمزات
/// والتاء المربوطة والألف المقصورة (normalize_keyword).
/// الصيغ القانونية بلا تشكيل أبدا (ابن، شغل)؛ والصيغ
/// المشكولة (ابنِ، شغّل) مقبولة توافقا فقط، لا تُطلَب.
/// (أمر المالك 2026-11-06 ~05:08: لا تشكيل ولا تطويل
/// في أي اسم أمر/كلمة مفتاحية/علم/دالة مدمجة/ثابت —
/// حروف عادية فقط، دائما.)
pub fn normalize_cli_word(word: &str) -> String {
    let stripped: String = word.chars().filter(|c| !is_diacritic(*c)).collect();
    normalize_keyword(&stripped)
}

/// هل هذه الكلمة (بعد التطبيع) كلمة محجوزة في
/// اللغة؟ الدفعة 3 (الميزة 44): طبقة «كلمة محجوزة»
/// في وصف الأمر — والمقارنة بعد التطبيع كما يميز
/// المحلل الكلمة المحجوزة نفسها.
pub(crate) fn is_keyword_word(word: &str) -> bool {
    keyword_for(&normalize_cli_word(word)).is_some()
}

fn keyword_for(normalized: &str) -> Option<Keyword> {
    Some(match normalized {
        "اطبع" => Keyword::Print,
        "اذا" => Keyword::If,
        "والا" => Keyword::Else,
        "اواذا" => Keyword::Elif,
        "بينما" => Keyword::While,
        "لكل" => Keyword::For,
        "في" => Keyword::In,
        "داله" => Keyword::Func,
        // دالة_مجهولة normalizes to داله_مجهوله (ة -> ه).
        "داله_مجهوله" => Keyword::Lambda,
        "ارجع" => Keyword::Return,
        "و" => Keyword::And,
        "او" => Keyword::Or,
        "ليس" => Keyword::Not,
        "صحيح" => Keyword::True,
        "خطا" => Keyword::False,
        "لاشيء" => Keyword::Nil,
        "اكسر" => Keyword::Break,
        "تابع" => Keyword::Continue,
        "مرر" => Keyword::Pass,
        "حاول" => Keyword::Try,
        "ماعدا" => Keyword::Except,
        "اخيرا" => Keyword::Finally,
        // البندان 77 و78 (الاجتماع 83 §98): `عام` و
        // `نطاق` ليستا هنا عمدا — كلمتان سياقيتان
        // (إعلان نطاق في بداية الجملة فقط حين يتبعهما
        // اسم معرّف؛ وإلا معرّفان عاديان) على نمط `نوع`
        // و`ولد` — فلا تُسرَقا اسم متغير (مثال 05-functions:
        // `عام = ١٠٠`؛ والادعاء القديم «بلا تصادم» كان
        // خاطئا وكشفه السيريال الكامل).
        // البند 88 — مدير السياق (الاجتماع 88 §103):
        // `عند` كلمة محجوزة؛ و`زامن عند` خارج النطاق
        // تماما فلا كلمة `زامن` هنا.
        "عند" => Keyword::With,
        "متوقع" => Keyword::Assert,
        // البندان 99 و100 (الدفعة 3 من الـ579): طابق/حال.
        // لا تصادم: `نمط.طابق` دالة مكتبية يبقى اسم
        // عضوها "طابق" عبر keyword_spelling بعد النقطة،
        // و`حال` لم تُستعمل معرّفا عاريا في أي موضع.
        "طابق" => Keyword::Match,
        "حال" => Keyword::Case,
        // فئة normalizes to فيه (ئ -> ي, ة -> ه); the فئه key
        // never matches normalized input, but is kept harmless.
        "فئه" => Keyword::Class,
        "فيه" => Keyword::Class,
        "انشي" => Keyword::Create,
        "ورث" => Keyword::Inherit,
        "واجهه" => Keyword::Interface,
        "استورد" => Keyword::Import,
        "صدر" => Keyword::Export,
        "باسم" => Keyword::As,
        "من" => Keyword::From,
        "كرر" => Keyword::Repeat,
        "مرات" => Keyword::Times,
        "طالما" => Keyword::AsLongAs,
        // كلمات المقارنة (اجتماع 19): تطبيع أ -> ا.
        "اكبر" => Keyword::Greater,
        "اصغر" => Keyword::Smaller,
        "يساوي" => Keyword::Equals,
        "لا" => Keyword::No,
        _ => return None,
    })
}

/// Canonical (normalized) spelling of a keyword, for use as a
/// member name after a dot: `عمليات.أنشئ` lexes أنشئ as
/// Keyword::Create (the lexeme is discarded), so the parser
/// needs the spelling back to name the member. Every spelling
/// round-trips: keyword_for(spelling) == the keyword
/// (unit-tested below). These are the same strings keyword_for
/// matches on — plain letters only, no tashkeel, no tatweel.
pub(crate) fn keyword_spelling(kw: Keyword) -> &'static str {
    match kw {
        Keyword::Print => "اطبع",
        Keyword::If => "اذا",
        Keyword::Else => "والا",
        Keyword::Elif => "اواذا",
        Keyword::While => "بينما",
        Keyword::For => "لكل",
        Keyword::In => "في",
        Keyword::Func => "داله",
        Keyword::Lambda => "داله_مجهوله",
        Keyword::Return => "ارجع",
        Keyword::And => "و",
        Keyword::Or => "او",
        Keyword::Not => "ليس",
        Keyword::True => "صحيح",
        Keyword::False => "خطا",
        Keyword::Nil => "لاشيء",
        Keyword::Break => "اكسر",
        Keyword::Continue => "تابع",
        Keyword::Pass => "مرر",
        Keyword::Try => "حاول",
        Keyword::Except => "ماعدا",
        Keyword::Finally => "اخيرا",
        Keyword::With => "عند",
        Keyword::Assert => "متوقع",
        Keyword::Match => "طابق",
        Keyword::Case => "حال",
        Keyword::Class => "فيه",
        Keyword::Create => "انشي",
        Keyword::Inherit => "ورث",
        Keyword::Interface => "واجهه",
        Keyword::Import => "استورد",
        Keyword::Export => "صدر",
        Keyword::As => "باسم",
        Keyword::From => "من",
        Keyword::Repeat => "كرر",
        Keyword::Times => "مرات",
        Keyword::AsLongAs => "طالما",
        Keyword::Greater => "اكبر",
        Keyword::Smaller => "اصغر",
        Keyword::Equals => "يساوي",
        Keyword::No => "لا",
    }
}

fn arabic_digit_value(c: char) -> Option<u32> {
    match c {
        '٠'..='٩' => Some(c as u32 - '٠' as u32),
        _ => None,
    }
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || arabic_digit_value(c).is_some()
}

pub fn tokenize(source: &str) -> RioResult<Vec<Token>> {
    // اجتماع 32، السؤال الخامس: BOM ‏(U+FEFF)‏ في أول
    // الملف يُتخطَّى بصمت — إزالته هنا قبل تقسيم الأسطر
    // فيبقى السطر 1 والعمود 1 للحرف الحقيقي الأول.
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let mut tokens: Vec<Token> = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let chars: Vec<char> = raw_line.chars().collect();
        // Leading whitespace: space = 1, tab = 8.
        let mut leading = 0usize;
        let mut indent = 0usize;
        while leading < chars.len() && (chars[leading] == ' ' || chars[leading] == '\t') {
            if chars[leading] == '\t' {
                indent += 8;
            } else {
                indent += 1;
            }
            leading += 1;
        }
        let rest: Vec<char> = chars[leading..].to_vec();
        // Blank or comment-only lines do not affect indentation.
        let trimmed: String = rest.iter().collect::<String>().trim().to_string();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Indentation change.
        let top = *indent_stack.last().unwrap();
        if indent > top {
            indent_stack.push(indent);
            tokens.push(Token { kind: TokenKind::Indent, line: line_no, col: 1 });
        } else {
            while indent < *indent_stack.last().unwrap() {
                indent_stack.pop();
                tokens.push(Token { kind: TokenKind::Dedent, line: line_no, col: 1 });
            }
            if indent != *indent_stack.last().unwrap() {
                return Err(RioError::lex(
                    "خطأ في المسافة البادئة: مستوى غير متوقع",
                    line_no,
                    1,
                ));
            }
        }
        // Tokenize the rest of the line.
        let mut i = 0usize;
        while i < rest.len() {
            let c = rest[i];
            let col = leading + i + 1;
            if c == ' ' || c == '\t' {
                i += 1;
                continue;
            }
            if c == '#' {
                break;
            }
            if c == '"' || c == '\'' {
                let quote = c;
                let start_col = col;
                let mut value = String::new();
                i += 1;
                let mut closed = false;
                while i < rest.len() {
                    let ch = rest[i];
                    if ch == '\\' && i + 1 < rest.len() {
                        let esc = rest[i + 1];
                        match esc {
                            'n' => value.push('\n'),
                            't' => value.push('\t'),
                            'r' => value.push('\r'),
                            '\\' => value.push('\\'),
                            '"' => value.push('"'),
                            '\'' => value.push('\''),
                            other => {
                                value.push('\\');
                                value.push(other);
                            }
                        }
                        i += 2;
                        continue;
                    }
                    if ch == quote {
                        closed = true;
                        i += 1;
                        break;
                    }
                    value.push(ch);
                    i += 1;
                }
                if !closed {
                    return Err(RioError::lex("نص غير مغلق", line_no, start_col));
                }
                tokens.push(Token { kind: TokenKind::Str(value), line: line_no, col: start_col });
                continue;
            }
            if is_digit(c) {
                let start_col = col;
                let mut num = String::new();
                while i < rest.len() && is_digit(rest[i]) {
                    if let Some(d) = arabic_digit_value(rest[i]) {
                        num.push(char::from_digit(d, 10).unwrap());
                    } else {
                        num.push(rest[i]);
                    }
                    i += 1;
                }
                // ج6: الجزء الصحيح وحده (للواحق)، وعلم الكسر.
                let int_part = num.clone();
                let mut has_frac = false;
                // اجتماع 32، السؤال الثالث: الفاصلة
                // العشرية العربية ٫ (U+066B) داخل عدد
                // فاصلة عشرية كالنقطة تماما — ١٫٥ عدد
                // واحد، والعرض يستخدم ٫ أصلا فتتسق
                // الكتابة مع العرض. خارج عدد تبقى حرفا
                // غير معروف كما كانت.
                if i + 1 < rest.len() && (rest[i] == '.' || rest[i] == '٫') && is_digit(rest[i + 1]) {
                    has_frac = true;
                    num.push('.');
                    i += 1;
                    while i < rest.len() && is_digit(rest[i]) {
                        if let Some(d) = arabic_digit_value(rest[i]) {
                            num.push(char::from_digit(d, 10).unwrap());
                        } else {
                            num.push(rest[i]);
                        }
                        i += 1;
                    }
                }
                let value: f64 = num.parse().map_err(|_| {
                    RioError::lex(format!("رقم غير صالح: {}", num), line_no, start_col)
                })?;
                // ج6: لواحق الصيغ الحرفية — ملاصقة بلا فراغ:
                // `x` بعد صحيح بلا كسر = كبير (`123x`)؛
                // و`r` + أرقام = كسر (`1r2`)؛ و`j` + أرقام
                // = مركب (`0j1`)؛ و`ت` = تخيلي خالص (`4ت`).
                // الكسر بعد فاصلة عشرية مرفوض (صياغة الكسر
                // صحيحة فقط) فيقع في خطأ المحلل لا هنا.
                if !has_frac {
                    if i < rest.len() && rest[i] == 'x' {
                        let digits = int_part.clone();
                        i += 1;
                        tokens.push(Token {
                            kind: TokenKind::BigInt(digits),
                            line: line_no,
                            col: start_col,
                        });
                        continue;
                    }
                    if i + 1 < rest.len() && rest[i] == 'r' && is_digit(rest[i + 1]) {
                        i += 1;
                        let mut den = String::new();
                        while i < rest.len() && is_digit(rest[i]) {
                            if let Some(d) = arabic_digit_value(rest[i]) {
                                den.push(char::from_digit(d, 10).unwrap());
                            } else {
                                den.push(rest[i]);
                            }
                            i += 1;
                        }
                        tokens.push(Token {
                            kind: TokenKind::Rational(int_part.clone(), den),
                            line: line_no,
                            col: start_col,
                        });
                        continue;
                    }
                    if i + 1 < rest.len() && rest[i] == 'j' && is_digit(rest[i + 1]) {
                        i += 1;
                        let mut im = String::new();
                        while i < rest.len() && is_digit(rest[i]) {
                            if let Some(d) = arabic_digit_value(rest[i]) {
                                im.push(char::from_digit(d, 10).unwrap());
                            } else {
                                im.push(rest[i]);
                            }
                            i += 1;
                        }
                        let re: f64 = int_part.parse().unwrap_or(0.0);
                        let imv: f64 = im.parse().unwrap_or(0.0);
                        tokens.push(Token {
                            kind: TokenKind::Complex(re, imv),
                            line: line_no,
                            col: start_col,
                        });
                        continue;
                    }
                    if i < rest.len() && rest[i] == 'ت' {
                        i += 1;
                        let imv: f64 = int_part.parse().unwrap_or(0.0);
                        tokens.push(Token {
                            kind: TokenKind::Complex(0.0, imv),
                            line: line_no,
                            col: start_col,
                        });
                        continue;
                    }
                }
                tokens.push(Token { kind: TokenKind::Number(value), line: line_no, col: start_col });
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let start_col = col;
                let mut word = String::new();
                while i < rest.len() {
                    let ch = rest[i];
                    if is_diacritic(ch) {
                        i += 1;
                        continue;
                    }
                    if ch.is_alphanumeric() || ch == '_' {
                        word.push(ch);
                        i += 1;
                    } else {
                        break;
                    }
                }
                let norm = normalize_keyword(&word);
                // النص التنفيذي (البنود 163–165، الدفعة 2 من
                // الـ579): بادئة `ف` قبل علامة الاقتباس مباشرة
                // (`ف"..."` أو `ف'...'`) — بلا مسافة بينهما.
                // المحتوى خام (بما فيه الشرطات المائلة) ليفككه
                // المحلل إلى أجزاء وحقول؛ والنص سطر واحد ككل
                // نص في نيفا. `ف` وحدها بلا اقتباس بعدها تبقى
                // معرّفا عاديا، وكلمة أطول تبدأ بف (فهم) لا تمس.
                if word == "ف" && i < rest.len() && (rest[i] == '"' || rest[i] == '\'') {
                    let quote = rest[i];
                    i += 1;
                    let mut raw = String::new();
                    let mut closed = false;
                    while i < rest.len() {
                        let ch = rest[i];
                        if ch == '\\' && i + 1 < rest.len() {
                            raw.push(ch);
                            raw.push(rest[i + 1]);
                            i += 2;
                            continue;
                        }
                        if ch == quote {
                            closed = true;
                            i += 1;
                            break;
                        }
                        raw.push(ch);
                        i += 1;
                    }
                    if !closed {
                        return Err(RioError::lex("نص تنفيذي غير مغلق", line_no, start_col));
                    }
                    tokens.push(Token { kind: TokenKind::FStr(raw), line: line_no, col: start_col });
                    continue;
                }
                if let Some(kw) = keyword_for(&norm) {
                    tokens.push(Token { kind: TokenKind::Keyword(kw), line: line_no, col: start_col });
                } else {
                    tokens.push(Token { kind: TokenKind::Ident(word), line: line_no, col: start_col });
                }
                continue;
            }
// == مولّد-دفعة2-رموز-ثلاثية: بداية ==
            // رموز ثلاثية (الأطول أولا): `//=` و`^^=` و`<<=` و`>>=`.
            let three = if i + 2 < rest.len() { Some((c, rest[i + 1], rest[i + 2])) } else { None };
            if let Some((kind, advance)) = match three {
                Some(('/', '/', '=')) => Some((TokenKind::SlashSlashEq, 3)),
                Some(('^', '^', '=')) => Some((TokenKind::CaretCaretEq, 3)),
                Some(('<', '<', '=')) => Some((TokenKind::ShlEq, 3)),
                Some(('>', '>', '=')) => Some((TokenKind::ShrEq, 3)),
                _ => None,
            } {
                tokens.push(Token { kind, line: line_no, col });
                i += advance;
                continue;
            }
// == مولّد-دفعة2-رموز-ثلاثية: نهاية ==
            // Operators and punctuation.
            let two = if i + 1 < rest.len() { Some((c, rest[i + 1])) } else { None };
            let (kind, advance): (TokenKind, usize) = match two {
                Some(('=', '=')) => (TokenKind::EqEq, 2),
                Some(('!', '=')) => (TokenKind::NotEq, 2),
                Some(('<', '=')) => (TokenKind::LtEq, 2),
                Some(('>', '=')) => (TokenKind::GtEq, 2),
                // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
                Some(('+', '=')) => (TokenKind::PlusEq, 2),
// == مولّد-دفعة2-رموز-ثنائية: بداية ==
                Some(('-', '=')) => (TokenKind::MinusEq, 2),
                Some(('*', '=')) => (TokenKind::StarEq, 2),
                Some(('/', '=')) => (TokenKind::SlashEq, 2),
                Some(('%', '=')) => (TokenKind::PercentEq, 2),
                Some(('/', '/')) => (TokenKind::SlashSlash, 2),
                Some(('^', '^')) => (TokenKind::CaretCaret, 2),
                Some(('<', '<')) => (TokenKind::Shl, 2),
                Some(('>', '>')) => (TokenKind::Shr, 2),
                Some(('&', '=')) => (TokenKind::AmpEq, 2),
                Some(('|', '=')) => (TokenKind::PipeEq, 2),
                Some(('^', '=')) => (TokenKind::CaretEq, 2),
                // البند 73 (`:=` — الاجتماع 83 §98).
                Some((':', '=')) => (TokenKind::ColonEq, 2),
// == مولّد-دفعة2-رموز-ثنائية: نهاية ==
                _ => match c {
                    '+' => (TokenKind::Plus, 1),
                    '-' => (TokenKind::Minus, 1),
                    '*' => (TokenKind::Star, 1),
                    '/' => (TokenKind::Slash, 1),
                    '%' => (TokenKind::Percent, 1),
// == مولّد-دفعة2-رموز-أحادية: بداية ==
                    '|' => (TokenKind::Pipe, 1),
                    '&' => (TokenKind::Amp, 1),
                    '^' => (TokenKind::Caret, 1),
                    '~' => (TokenKind::Tilde, 1),
// == مولّد-دفعة2-رموز-أحادية: نهاية ==
                    '=' => (TokenKind::Eq, 1),
                    '<' => (TokenKind::Lt, 1),
                    '>' => (TokenKind::Gt, 1),
                    '(' => (TokenKind::LParen, 1),
                    ')' => (TokenKind::RParen, 1),
                    '[' => (TokenKind::LBracket, 1),
                    ']' => (TokenKind::RBracket, 1),
                    '{' => (TokenKind::LBrace, 1),
                    '}' => (TokenKind::RBrace, 1),
                    ':' => (TokenKind::Colon, 1),
                    '.' => (TokenKind::Dot, 1),
                    '@' => (TokenKind::At, 1),
                    ',' | '،' => (TokenKind::Comma, 1),
                    ';' | '؛' => (TokenKind::Semicolon, 1),
                    '!' => {
                        return Err(RioError::lex(
                            "علامة ! وحدها غير صالحة، استخدم !=",
                            line_no,
                            col,
                        ));
                    }
                    _ => {
                        return Err(RioError::lex(
                            format!("حرف غير معروف: {}", c),
                            line_no,
                            col,
                        ));
                    }
                },
            };
            tokens.push(Token { kind, line: line_no, col });
            i += advance;
        }
        tokens.push(Token { kind: TokenKind::Newline, line: line_no, col: leading + rest.len() + 1 });
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            line: source.lines().count() + 1,
            col: 1,
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        line: source.lines().count() + 1,
        col: 1,
    });
    Ok(tokens)
}

/// tokenize مع اسم ملف يُسنَد إلى أي خطأ لغوي —
/// السباكة داخل المحرك (اجتماع 32)؛ والتوصيل العلوي
/// (main.rs) سطران بعد هبوط المهمة 29.
pub fn tokenize_named(source: &str, file: &str) -> RioResult<Vec<Token>> {
    tokenize(source).map_err(|e| e.with_file(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_and_digits() {
        let toks = tokenize("اطبع ١٢٣\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Print)));
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 123.0).abs() < 1e-9));
    }

    #[test]
    fn assert_keyword() {
        // البند 81 (الدفعة 2 من الـ579): متوقع كلمة
        // محجوزة، لا معرّفا.
        let toks = tokenize("متوقع س == ١\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Assert)));
    }

    #[test]
    fn match_case_keywords() {
        // البندان 99 و100 (الدفعة 3 من الـ579): طابق
        // وحال كلمتان محجوزتان، لا معرّفان؛ والتشكيل
        // يُجرَّد عنهما كما عن سائر الكلمات.
        let toks = tokenize("طابق س:\n    حال ١:\n        اطبع ١\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Match)));
        // [طابق، س، :، سطر، مسافة، حال، ...].
        assert!(matches!(toks[5].kind, TokenKind::Keyword(Keyword::Case)));
        let toks = tokenize("طابِق س\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Match)));
        // `نمط.طابق` بعد النقطة يبقى اسما: الكلمة
        // تُقرأ كلمةً والمحلل يسميها عبر التهجئة.
        let toks = tokenize("اطبع نمط.طابق(\"a\", \"a\")\n").unwrap();
        // [اطبع، نمط، .، طابق، ...].
        assert!(matches!(toks[3].kind, TokenKind::Keyword(Keyword::Match)));
    }

    #[test]
    fn string_is_not_translated() {
        let toks = tokenize("اطبع \"إذا ١٢٣\"\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Str(ref s) if s == "إذا ١٢٣"));
    }

    #[test]
    fn diacritic_keyword() {
        let toks = tokenize("اِطبع \"مرحبا\"\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Print)));
    }

    #[test]
    fn arabic_decimal_separator() {
        // اجتماع 32، السؤال الثالث: ٫ فاصلة عشرية.
        let toks = tokenize("اطبع ١٫٥\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 1.5).abs() < 1e-9));
        let toks = tokenize("اطبع 1٫5\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 1.5).abs() < 1e-9));
        let toks = tokenize("اطبع ١٫٥ + ٢٫٢٥\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 1.5).abs() < 1e-9));
        assert!(matches!(toks[3].kind, TokenKind::Number(n) if (n - 2.25).abs() < 1e-9));
        // النقطة ASCII ما زالت تعمل كما كانت.
        let toks = tokenize("اطبع ١.٥\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 1.5).abs() < 1e-9));
    }

    #[test]
    fn arabic_separator_alone_is_not_a_number() {
        // ٫ بلا أرقام حولها ليست عددا: في أول السطر
        // حرف غير معروف؛ وبعد عدد بلا كسر بعدها كذلك.
        assert!(tokenize("اطبع ٫٥\n").is_err());
        assert!(tokenize("اطبع ١٫\n").is_err());
    }

    #[test]
    fn bom_is_skipped_silently() {
        // اجتماع 32، السؤال الخامس: BOM في أول الملف
        // يُتخطَّى بصمت، والسطر 1 والعمود 1 للحرف الأول.
        let toks = tokenize("\u{FEFF}اطبع ١٢٣\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword(Keyword::Print)));
        assert_eq!((toks[0].line, toks[0].col), (1, 1));
        assert!(matches!(toks[1].kind, TokenKind::Number(n) if (n - 123.0).abs() < 1e-9));
        // BOM وسط الملف ليس في أوله: يبقى حرفا غير معروف.
        assert!(tokenize("اطبع ١\n\u{FEFF}اطبع ٢\n").is_err());
    }

    #[test]
    fn named_tokenize_tags_lex_errors_with_file() {
        let e = tokenize_named("اطبع \"نص\n", "برنامج.نيفا").unwrap_err();
        assert_eq!(e.file.as_deref(), Some("برنامج.نيفا"));
        assert!(e.to_string().contains("الملف برنامج.نيفا"));
    }

    #[test]
    fn fstring_prefix() {
        // البنود 163–165 (الدفعة 2 من الـ579): `ف"..."` رمز
        // نص تنفيذي بمحتواه الخام؛ و`ف` وحدها معرّف.
        let toks = tokenize("اطبع ف\"العدد {س}\"\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::FStr(ref s) if s == "العدد {س}"));
        let toks = tokenize("اطبع ف'نص {س}'\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::FStr(ref s) if s == "نص {س}"));
        let toks = tokenize("ف = ٥\n").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Ident(ref s) if s == "ف"));
        let toks = tokenize("اطبع فهم\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Ident(ref s) if s == "فهم"));
        // `ف` ثم نص بمسافة بينهما: معرّف ونص منفصلان.
        let toks = tokenize("اطبع ف \"نص\"\n").unwrap();
        assert!(matches!(toks[1].kind, TokenKind::Ident(ref s) if s == "ف"));
        assert!(matches!(toks[2].kind, TokenKind::Str(ref s) if s == "نص"));
        // غير مغلق خطأ لغوي.
        assert!(tokenize("اطبع ف\"نص\n").is_err());
    }

    #[test]
    fn keyword_spelling_round_trips() {
        // مهمة 10: كل كلمة مفتاحية لها صيغة قانونية واحدة
        // تعيدها keyword_for إلى نفس الكلمة — فاسم العضو
        // بعد النقطة (مثل انشي من أنشئ) ثابت دائما.
        let all = [
            Keyword::Print,
            Keyword::If,
            Keyword::Else,
            Keyword::Elif,
            Keyword::While,
            Keyword::For,
            Keyword::In,
            Keyword::Func,
            Keyword::Lambda,
            Keyword::Return,
            Keyword::And,
            Keyword::Or,
            Keyword::Not,
            Keyword::True,
            Keyword::False,
            Keyword::Nil,
            Keyword::Break,
            Keyword::Continue,
            Keyword::Pass,
            Keyword::Try,
            Keyword::Except,
            Keyword::Finally,
            Keyword::With,
            Keyword::Assert,
            Keyword::Match,
            Keyword::Case,
            Keyword::Class,
            Keyword::Create,
            Keyword::Inherit,
            Keyword::Interface,
            Keyword::Import,
            Keyword::Export,
            Keyword::As,
            Keyword::From,
            Keyword::Repeat,
            Keyword::Times,
            Keyword::AsLongAs,
            Keyword::Greater,
            Keyword::Smaller,
            Keyword::Equals,
            Keyword::No,
        ];
        assert_eq!(all.len(), 41);
        for kw in all {
            let spelling = keyword_spelling(kw);
            assert_eq!(keyword_for(spelling), Some(kw), "spelling: {}", spelling);
            // بلا تشكيل ولا تطويل أبدا (قاعدة بلا تشكيل).
            assert!(
                !spelling.chars().any(is_diacritic),
                "tashkeel in spelling: {}",
                spelling
            );
            assert!(!spelling.contains('ـ'), "tatweel in spelling: {}", spelling);
        }
    }
}
