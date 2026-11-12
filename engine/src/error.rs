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

    /// البند 88 — مدير السياق: عكس `arabic` — من
    /// الاسم العربي كما يظهر في رسالة الخطأ إلى النوع؛
    /// وغير المعروف يعود وقت تشغيل (دفاعي، داخلي فقط).
    pub fn من_العربية(name: &str) -> Self {
        match name {
            "لغوي" => ErrorKind::Lex,
            "نحوي" => ErrorKind::Parse,
            "نوع" => ErrorKind::Type,
            "إدخال/إخراج" => ErrorKind::Io,
            "دلالي" => ErrorKind::Semantic,
            _ => ErrorKind::Runtime,
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
    /// الدفعة 1 (§69 ممتد): وحدة مستوردة غائبة — ملف
    /// مطلوب غير موجود، فيرجع المتصل 127 لا 1 ولا 2.
    /// يُضبط فقط عبر المنشئ `module_not_found` أدناه،
    /// فلا يتأثر ببادئة «في وحدة X: » اللاحقة.
    pub module_not_found: bool,
}

impl RioError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, line: usize, col: usize) -> Self {
        Self { kind, message: message.into(), line, col, file: None, module_not_found: false }
    }
    /// وحدة مستوردة غائبة: خطأ إدخال/إخراج بالرسالة
    /// نفسها حرفيا («وحدة غير موجودة: ...») مع علامة
    /// تعيين الرمز 127 — الدفعة 1 (§69 ممتد).
    pub fn module_not_found(
        module: &str,
        searched: &str,
        line: usize,
        col: usize,
    ) -> Self {
        Self::new(
            ErrorKind::Io,
            format!("وحدة غير موجودة: {} (بحثت في {})", module, searched),
            line,
            col,
        )
        .with_module_not_found()
    }
    /// اضبط علامة الوحدة الغائبة (سلسلة بناء).
    pub fn with_module_not_found(mut self) -> Self {
        self.module_not_found = true;
        self
    }
    /// هل هذا الخطأ وحدة مستوردة غائبة؟ (127 لا 1 ولا 2).
    pub fn is_module_not_found(&self) -> bool {
        self.module_not_found
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
