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
    Str(String),
    Keyword(Keyword),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
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

fn keyword_for(normalized: &str) -> Option<Keyword> {
    Some(match normalized {
        "اطبع" => Keyword::Print,
        "اذا" => Keyword::If,
        "والا" => Keyword::Else,
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
                if i + 1 < rest.len() && rest[i] == '.' && is_digit(rest[i + 1]) {
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
                if let Some(kw) = keyword_for(&norm) {
                    tokens.push(Token { kind: TokenKind::Keyword(kw), line: line_no, col: start_col });
                } else {
                    tokens.push(Token { kind: TokenKind::Ident(word), line: line_no, col: start_col });
                }
                continue;
            }
            // Operators and punctuation.
            let two = if i + 1 < rest.len() { Some((c, rest[i + 1])) } else { None };
            let (kind, advance): (TokenKind, usize) = match two {
                Some(('=', '=')) => (TokenKind::EqEq, 2),
                Some(('!', '=')) => (TokenKind::NotEq, 2),
                Some(('<', '=')) => (TokenKind::LtEq, 2),
                Some(('>', '=')) => (TokenKind::GtEq, 2),
                _ => match c {
                    '+' => (TokenKind::Plus, 1),
                    '-' => (TokenKind::Minus, 1),
                    '*' => (TokenKind::Star, 1),
                    '/' => (TokenKind::Slash, 1),
                    '%' => (TokenKind::Percent, 1),
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
    fn keyword_spelling_round_trips() {
        // مهمة 10: كل كلمة مفتاحية لها صيغة قانونية واحدة
        // تعيدها keyword_for إلى نفس الكلمة — فاسم العضو
        // بعد النقطة (مثل انشي من أنشئ) ثابت دائما.
        let all = [
            Keyword::Print,
            Keyword::If,
            Keyword::Else,
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
        assert_eq!(all.len(), 36);
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
