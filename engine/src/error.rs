//! Error handling: every Niva error carries a kind, an Arabic message,
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
    /// الملف الذي وقع فيه الخطأ (الملف الرئيسي أو ملف
    /// الوحدة المستوردة) — اجتماع 32، السؤال الخامس.
    /// لا شيء حين لا ملف (إدخال REPL، أو نص بلا ملف)،
    /// فيبقى العرض القديم بلا اسم ملف حرفيا.
    pub file: Option<String>,
}

impl RioError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, line: usize, col: usize) -> Self {
        Self { kind, message: message.into(), line, col, file: None }
    }
    /// سمِّ الملف صراحة (يغلب أي اسم سابق).
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
    /// سمِّ الملف إن لم يكن مسمى بعد — إسناد الوحدة
    /// لا يغلب إسناد وحدة أعمق سبقته (استيراد متداخل).
    pub fn with_file_if_none(mut self, file: Option<String>) -> Self {
        if self.file.is_none() {
            self.file = file;
        }
        self
    }
    /// أضف بادئة إلى الرسالة مع بقاء الملف كما هو —
    /// «في وحدة X: » عند عبور حدود وحدة.
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.message = format!("{}{}", prefix, self.message);
        self
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
        // اجتماع 32: الصيغة المتسقة — الملف أولا حين
        // يكون معلوما، ثم السطر والعمود كما كانا.
        // بلا ملف يبقى العرض القديم حرفيا.
        match &self.file {
            Some(file) => write!(
                f,
                "خطأ {}: {} (الملف {}، السطر {}، العمود {})",
                self.kind.arabic(),
                self.message,
                file,
                self.line,
                self.col
            ),
            None => write!(
                f,
                "خطأ {}: {} (السطر {}، العمود {})",
                self.kind.arabic(),
                self.message,
                self.line,
                self.col
            ),
        }
    }
}

impl std::error::Error for RioError {}

pub type RioResult<T> = Result<T, RioError>;
