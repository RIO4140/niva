//! Error handling: every Rio error carries a kind, an Arabic message,
//! and a line + column (character columns, not bytes, so Arabic text counts
//! as the user sees it). The CLI prints exactly this format to stderr.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Lex,
    Parse,
    Type,
    Runtime,
    Io,
    // Phase 1A Step 1: errors found by the semantic analyzer BEFORE
    // execution (undefined variables, return/break outside their
    // context, wrong call arity). These are not runtime errors,
    // because the program never runs when they occur.
    Semantic,
}

impl ErrorKind {
    pub fn arabic(&self) -> &'static str {
        match self {
            ErrorKind::Lex => "لغوي",
            ErrorKind::Parse => "نحوي",
            ErrorKind::Type => "نوع",
            ErrorKind::Runtime => "وقت التشغيل",
            ErrorKind::Io => "إدخال/إخراج",
            ErrorKind::Semantic => "دلالي",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RioError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl RioError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, line: usize, col: usize) -> Self {
        Self { kind, message: message.into(), line, col }
    }
    pub fn lex(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(ErrorKind::Lex, message, line, col)
    }
    pub fn parse(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(ErrorKind::Parse, message, line, col)
    }
    pub fn typ(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(ErrorKind::Type, message, line, col)
    }
    pub fn runtime(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(ErrorKind::Runtime, message, line, col)
    }
    pub fn semantic(message: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(ErrorKind::Semantic, message, line, col)
    }
}

impl fmt::Display for RioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "خطأ {}: {} (السطر {}، العمود {})",
            self.kind.arabic(),
            self.message,
            self.line,
            self.col
        )
    }
}

impl std::error::Error for RioError {}

pub type RioResult<T> = Result<T, RioError>;
