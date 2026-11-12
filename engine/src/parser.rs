//! Parser: tokens -> AST. Recursive descent with precedence:
//! or > and > comparison > additive > multiplicative > unary > primary.
//! Blocks are colon + indented lines (INDENT/DEDENT from the lexer),
//! or a single statement on the same line after the colon.

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, TypeExpr, UnOp};
use std::collections::HashMap;
use std::collections::HashSet;
use crate::error::{RioError, RioResult};
use crate::lexer::{tokenize, Keyword, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// الملف الذي يُحلَّل (لإسناد أخطاء النحو إليه) —
    /// اجتماع 32، السؤال الخامس. لا شيء للإدخال بلا ملف.
    file: Option<String>,
    /// البند 104 (`نوع` — الاجتماع 83 §98): سجل
    /// الأسماء المستعارة في هذا التحليل وحده (اسم ←
    /// هدفه القانوني خاما)؛ لا يعبر حدود التحليل.
    type_aliases: HashMap<String, String>,
    /// البنود 145–147 (العموميات — الاجتماع 83 §98):
    /// أسماء الفئات والواجهات المعرَّفة حتى الآن في هذا
    /// التحليل (لتمييز `اسم[نوع](...)` — تخصيص عام —
    /// عن فهرسة `ق[س](...)` عند الاستدعاء)؛ ومكدس
    /// معاملات النوع في نطاق التعريفات المحيطة (فاسم
    /// معامل النوع `ت` نوع صالح داخل تعريفه وحده).
    defined_types: HashSet<String>,
    type_param_stack: Vec<HashSet<String>>,
}

/// هل الرمز التالي قائمة فارغة `[]` على السطر نفسه
/// (قائمة حرفية كمعامل مجرد، لا فهرسة)؟ اجتماع 32،
/// السؤال الأول: القائمة الحرفية قائمة حرفية في كل موضع.
fn empty_list_follows(parser: &Parser, line: usize) -> bool {
    let tok = parser.peek();
    if tok.line != line || !matches!(tok.kind, TokenKind::LBracket) {
        return false;
    }
    let next = parser.peek_at(1);
    next.line == line && matches!(next.kind, TokenKind::RBracket)
}

/// Does the next token start a bare-call argument on the same
/// line as the dotted name (Meeting 27, §42.1)? True for value
/// starts (number, string, name, brackets, unary minus,
/// ليس / دالة_مجهولة / أنشئ / صحيح / خطأ / لاشيء) on that
/// line — false for newlines, separators, operators, and
/// keywords that never start a value.
fn bare_call_follows(tok: &Token, line: usize) -> bool {
    if tok.line != line {
        return false;
    }
    match &tok.kind {
        // ج6: الصيغ الحرفية الجديدة تبدأ قيمة كالعدد.
        TokenKind::Ident(_) | TokenKind::Number(_) | TokenKind::BigInt(_)
        | TokenKind::Rational(_, _) | TokenKind::Complex(_, _) | TokenKind::Str(_) => true,
        // NOTE: LParen and LBracket are deliberately NOT
        // here: `(` after a dotted name is the
        // parenthesized call (`شبكة.اجلب("...")`) and
        // `[` is the index operator (`س.قائمة[٠]`) —
        // both are handled by the loop arms above, and
        // the bare-call rule must never steal them.
        // (Found 2026-11-06: the first version listed
        // LParen here and broke every parenthesized
        // dotted call.)
        TokenKind::LBrace | TokenKind::Minus => true,
        TokenKind::Keyword(kw) => matches!(
            kw,
            Keyword::Lambda
                | Keyword::Not
                | Keyword::True
                | Keyword::False
                | Keyword::Nil
                | Keyword::Create
        ),
        _ => false,
    }
}

/// قائمة معاملات دالة (البنود 109–112 — الاجتماع
/// 83 §98): أسماء وأنواع وافتراضيات (تعبيرات
/// تُقيَّم وقت التعريف) وجامعا `*` و`**`.
struct ParamList {
    params: Vec<String>,
    param_types: Vec<Option<String>>,
    defaults: Vec<Option<Expr>>,
    vararg: Option<String>,
    kwarg: Option<String>,
}

/// نمط واحد داخل `حال` (البنود 99–100 و153–162،
/// الدفعة 3 من الـ579). الأنماط وحدها تُحلَّل هنا؛
/// والتفكيك إلى اختبار + ربط بعقد قائمة في
/// `desugar_match`/`pattern_test` — فلا عقد AST
/// جديدة، ويعمل المفسر والآلة والأصلي كما هي.
/// `|` بدائل تُوسَّع إلى أنماط عدة (جداء ديكارتي في
/// الأنماط المركبة)، فيصير كل بديل حالة في السلسلة،
/// ويبقى ترتيب الحالات كما كتبه المستخدم.
#[derive(Debug, Clone, PartialEq)]
enum Pat {
    /// حرفي: عدد أو نص أو صحيح أو خطا أو لاشيء
    /// (بما فيه العدد السالب).
    Lit(Expr),
    /// التقاط: اسم متغير جديد يُربط بالقيمة.
    Capture(String),
    /// شامل `_`: يطابق أي شيء بلا ربط.
    Wildcard,
    /// قيمة مؤهلة بنقطة: `وحدة.ثابت` أو `كائن.حقل`.
    Value(Expr),
    /// متسلسل: عناصر بلا نجمة بالترتيب، والنجمة
    /// اختيارية (عدد العناصر قبلها، واسم التقاط
    /// الباقي أو لا شيء لـ`*_`)، وموضعها في المصدر
    /// لاسم مؤقت فريد.
    Seq {
        elems: Vec<Pat>,
        star: Option<(usize, Option<String>, usize, usize)>,
    },
    /// فهرس: (مفتاح، نمط) + باقي `**اسم` اختياري.
    Map {
        entries: Vec<(Expr, Pat)>,
        rest: Option<String>,
    },
    /// صنف: اسم الصنف + (حقل، نمط) مفتاحية فقط —
    /// الموضعي مرفوض نحوا (لا ترتيب حقول معلن في
    /// فئات نيفا).
    Class {
        name: String,
        fields: Vec<(String, Pat)>,
    },
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, file: None, type_aliases: HashMap::new(), defined_types: HashSet::new(), type_param_stack: Vec::new() }
    }

    /// Parser مع اسم ملف يُسنَد إلى أخطاء النحو —
    /// السباكة داخل المحرك (اجتماع 32)؛ والتوصيل العلوي
    /// (main.rs) سطران بعد هبوط المهمة 29.
    pub fn named(tokens: Vec<Token>, file: &str) -> Self {
        Self { tokens, pos: 0, file: Some(file.to_string()), type_aliases: HashMap::new(), defined_types: HashSet::new(), type_param_stack: Vec::new() }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn is_separator(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Newline | TokenKind::Semicolon)
    }

    fn skip_separators(&mut self) {
        while self.is_separator() {
            self.advance();
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> RioResult<Token> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
            && self.peek_kind_eq(kind)
        {
            Ok(self.advance())
        } else {
            let t = self.peek().clone();
            Err(RioError::parse(format!("توقع {}، وجد غيره", what), t.line, t.col))
        }
    }

    fn peek_kind_eq(&self, kind: &TokenKind) -> bool {
        // Compare kinds ignoring payloads (Ident/Number/Str carry values).
        match (&self.peek().kind, kind) {
            (TokenKind::Ident(_), TokenKind::Ident(_)) => true,
            (TokenKind::Number(_), TokenKind::Number(_)) => true,
            (TokenKind::Str(_), TokenKind::Str(_)) => true,
            // ج6: الصيغ الحرفية — مقارنة بلا حمولة.
            (TokenKind::BigInt(_), TokenKind::BigInt(_)) => true,
            (TokenKind::Rational(_, _), TokenKind::Rational(_, _)) => true,
            (TokenKind::Complex(_, _), TokenKind::Complex(_, _)) => true,
            (a, b) => a == b,
        }
    }

    fn expect_keyword(&mut self, kw: Keyword, what: &str) -> RioResult<Token> {
        if matches!(self.peek().kind, TokenKind::Keyword(k) if k == kw) {
            Ok(self.advance())
        } else {
            let t = self.peek().clone();
            Err(RioError::parse(format!("توقع {}، وجد غيره", what), t.line, t.col))
        }
    }

    fn expect_ident(&mut self, what: &str) -> RioResult<(String, usize, usize)> {
        match &self.peek().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                let t = self.advance();
                Ok((name, t.line, t.col))
            }
            _ => {
                let t = self.peek().clone();
                Err(RioError::parse(format!("توقع {}، وجد غيره", what), t.line, t.col))
            }
        }
    }

    /// Member name after a dot: an identifier, or a keyword
    /// used as a name (its canonical normalized spelling —
    /// e.g. `عمليات.أنشئ` lexes أنشئ as Keyword::Create, and
    /// the member name is "انشي"). Keywords are names in this
    /// one position only; everywhere else expect_ident still
    /// rejects them, so no valid program changes meaning
    /// (this position was a parse error before).
    fn expect_member_name(&mut self) -> RioResult<(String, usize, usize)> {
        match &self.peek().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                let t = self.advance();
                Ok((name, t.line, t.col))
            }
            TokenKind::Keyword(kw) => {
                let name = crate::lexer::keyword_spelling(*kw).to_string();
                let t = self.advance();
                Ok((name, t.line, t.col))
            }
            _ => {
                let t = self.peek().clone();
                Err(RioError::parse(
                    "توقع اسم بعد النقطة، وجد غيره".to_string(),
                    t.line,
                    t.col,
                ))
            }
        }
    }

    fn expect_colon(&mut self) -> RioResult<()> {
        self.expect(&TokenKind::Colon, "نقطتين :")?;
        Ok(())
    }

    pub fn parse_program(&mut self) -> RioResult<Vec<Stmt>> {
        self.parse_program_inner()
            .map_err(|e| e.with_file_if_none(self.file.clone()))
    }

    fn parse_program_inner(&mut self) -> RioResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_separators();
            if self.is_eof() {
                break;
            }
            if matches!(self.peek().kind, TokenKind::Dedent) {
                // Stray dedent at top level: consume and continue.
                self.advance();
                continue;
            }
            stmts.extend(self.parse_statement()?);
        }
        Ok(stmts)
    }

    /// A block after ':'. Indented lines, or one statement on the same line.
    fn parse_block(&mut self) -> RioResult<Vec<Stmt>> {
        if matches!(self.peek().kind, TokenKind::Newline) {
            self.advance();
            if !matches!(self.peek().kind, TokenKind::Indent) {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "توقع كتلة بمسافة بادئة بعد النقطتين",
                    t.line,
                    t.col,
                ));
            }
            self.advance(); // Indent
            let mut stmts = Vec::new();
            loop {
                self.skip_separators();
                match self.peek().kind {
                    TokenKind::Dedent => {
                        self.advance();
                        break;
                    }
                    TokenKind::Eof => {
                        let t = self.peek().clone();
                        return Err(RioError::parse("نهاية غير متوقعة داخل كتلة", t.line, t.col));
                    }
                    _ => stmts.extend(self.parse_statement()?),
                }
            }
            Ok(stmts)
        } else {
            // Inline block: one statement on the same line
            // (or several, when one source line unpacks).
            self.parse_statement()
        }
    }

    /// جملة واحدة من المصدر قد تُفكَّك إلى عدة جمل
    /// (البند 69: التفكيك `س، ص = حاوية` عدة إسنادات)،
    /// فالنتيجة قائمة — وكل جملة عادية عنصر واحد.
    fn parse_statement(&mut self) -> RioResult<Vec<Stmt>> {
        // ب6 — المولدات: جملة تبدأ بـ`ولد` (وليس استدعاء
        // `ولد(...)` ولا إسنادا إليه ولا فهرسة) جملة
        // `ولد` — تُفكَّك إلى استدعاء داخلي (بلا عقدة
        // جديدة وبلا لمس cgen.rs).
        if self.peek_is_yield_start() {
            return self.parse_yield_stmt();
        }
        let tok = self.peek().clone();
        // البند 104 (`نوع` — الاجتماع 83 §98): بيان
        // اسم مستعار سياقي في بداية الجملة فقط —
        // `نوع اسم = نوع`؛ و`نوع(س)` يبقى استدعاء
        // المدمجة (يتبعه قوس لا معرّف).
        if let TokenKind::Ident(n) = &tok.kind {
            if crate::lexer::normalize_cli_word(n) == "نوع"
                && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
                && matches!(self.peek_at(2).kind, TokenKind::Eq)
            {
                return Ok(vec![self.parse_type_alias()?]);
            }
        }
        // البندان 77 و78 (`عام`/`نطاق` — الاجتماع 83 §98):
        // إعلان نطاق سياقي في بداية الجملة فقط —
        // `عام س، ص` أو `نطاق س، ص` (يتبعهما اسم معرّف)؛
        // وإلا فهما معرّفان عاديان — على نمط `نوع` (104)
        // و`ولد` (ب6)، فلا يُسرَق اسم متغير أو دالة في أي
        // برنامج صالح (مثال 05-functions: `عام = ١٠٠`).
        if let TokenKind::Ident(n) = &tok.kind {
            if (crate::lexer::normalize_cli_word(n) == "عام"
                || crate::lexer::normalize_cli_word(n) == "نطاق")
                && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
            {
                let global = crate::lexer::normalize_cli_word(n) == "عام";
                return Ok(vec![self.parse_scope_decl(global)?]);
            }
        }
        // البند 88 — `زامن عند` خارج النطاق تماما
        // (§103): رفض صريح بدل تحليل مشوَّش جملتين
        // (`زامن` متغير غير معرف ثم `عند` مستقلة).
        if let TokenKind::Ident(name) = &tok.kind {
            if crate::lexer::normalize_cli_word(name) == "زامن"
                && matches!(
                    self.peek_at(1).kind,
                    TokenKind::Keyword(Keyword::With)
                )
            {
                return Err(RioError::parse(
                    "زامن عند غير مدعومة بعد — الصيغة المتزامنة وحدها",
                    tok.line,
                    tok.col,
                ));
            }
        }
        match &tok.kind {
            // البند 8 — الزخارف: `@` في بداية جملة فقط.
            TokenKind::At => Ok(vec![self.parse_decorated()?]),
            TokenKind::Keyword(Keyword::Print) => Ok(vec![self.parse_print()?]),
            TokenKind::Keyword(Keyword::If) => Ok(vec![self.parse_if()?]),
            TokenKind::Keyword(Keyword::While) => Ok(vec![self.parse_while()?]),
            TokenKind::Keyword(Keyword::For) => Ok(vec![self.parse_for()?]),
            TokenKind::Keyword(Keyword::Repeat) => Ok(vec![self.parse_repeat()?]),
            TokenKind::Keyword(Keyword::Func) => Ok(vec![self.parse_funcdef()?]),
            TokenKind::Keyword(Keyword::Assert) => Ok(vec![self.parse_assert()?]),
            TokenKind::Keyword(Keyword::Match) => Ok(self.parse_match()?),
            TokenKind::Keyword(Keyword::Return) => {
                self.advance();
                let value = if self.is_separator()
                    || matches!(self.peek().kind, TokenKind::Dedent | TokenKind::Eof)
                {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                Ok(vec![Stmt::new(StmtKind::Return { value }, tok.line, tok.col)])
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.advance();
                Ok(vec![Stmt::new(StmtKind::Break, tok.line, tok.col)])
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.advance();
                Ok(vec![Stmt::new(StmtKind::Continue, tok.line, tok.col)])
            }
            TokenKind::Keyword(Keyword::Pass) => {
                self.advance();
                Ok(vec![Stmt::new(StmtKind::Pass, tok.line, tok.col)])
            }
            TokenKind::Keyword(Keyword::Try) => Ok(vec![self.parse_try()?]),
            TokenKind::Keyword(Keyword::With) => Ok(self.parse_with()?),
            TokenKind::Keyword(Keyword::Class) => Ok(vec![self.parse_classdef()?]),
            TokenKind::Keyword(Keyword::Interface) => Ok(vec![self.parse_interfacedef()?]),
            TokenKind::Keyword(Keyword::Import) => Ok(vec![self.parse_import()?]),
            TokenKind::Keyword(Keyword::From) => Ok(vec![self.parse_from_import()?]),
            TokenKind::Keyword(Keyword::Export) => Ok(vec![self.parse_export()?]),
            _ => {
                if self.looks_like_unpack() {
                    self.parse_unpack()
                } else {
                    Ok(vec![self.parse_assign_or_expr()?])
                }
            }
        }
    }

    /// البندان 77 و78 (`عام`/`نطاق` — الاجتماع 83 §98):
    /// إعلان نطاق — `عام س، ص` أو `نطاق س، ص`. الأسماء
    /// معرّفات عارية يفصل بينها فاصلة عربية أو لاتينية؛
    /// وبعدها نهاية السطر. بلا أسماء، أو باسم غير
    /// معرّف، أو بلا فاصلة بين اسمين — خطأ نحوي عربي.
    fn parse_scope_decl(&mut self, global: bool) -> RioResult<Stmt> {
        let tok = self.advance();
        let word = if global { "عام" } else { "نطاق" };
        let mut names = Vec::new();
        loop {
            let t = self.peek().clone();
            let name = match &t.kind {
                TokenKind::Ident(n) => n.clone(),
                _ => {
                    return Err(RioError::parse(
                        format!("إعلان النطاق يتطلب اسما، مثل {} س", word),
                        t.line,
                        t.col,
                    ));
                }
            };
            self.advance();
            names.push(name);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        // بعد الاسم الأخير نهاية سطر لا اسم آخر.
        let t = self.peek().clone();
        if matches!(t.kind, TokenKind::Ident(_)) {
            return Err(RioError::parse(
                "ضع فاصلة بين الأسماء في إعلان النطاق",
                t.line,
                t.col,
            ));
        }
        Ok(Stmt::new(
            if global {
                StmtKind::GlobalDecl { names }
            } else {
                StmtKind::NonlocalDecl { names }
            },
            tok.line,
            tok.col,
        ))
    }

    /// البند 8 — الزخارف (الاجتماع 72 §87): سطر أو أكثر
    /// من `@اسم` (كل زخرفة وحدها في سطرها)، يتبعها تعريف
    /// دالة أو فئة. أي موضع آخر لـ`@` مرفوض هنا: قبل غير
    /// تعريف، أو بلا تعريف بعده، أو بغير اسم بعد `@`.
    /// والزخرفة اسم أو سلسلة منقوطة (`وحدة.غلف`) فقط —
    /// لا استدعاء ولا تعبير، فتُستدعى وقت التعريف بلا
    /// معاملات إلا المعرف نفسه.
    fn parse_decorated(&mut self) -> RioResult<Stmt> {
        let tok = self.peek().clone(); // @
        let mut decorators: Vec<Expr> = Vec::new();
        loop {
            self.advance(); // @
            decorators.push(self.parse_decorator_expr()?);
            if matches!(self.peek().kind, TokenKind::Newline) {
                self.advance();
            } else if matches!(self.peek().kind, TokenKind::Eof) {
                // بلا سطر بعدها: يخطئ فحص التعريف أدناه.
            } else {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "الزخرفة يجب أن تكون وحدها في السطر",
                    t.line,
                    t.col,
                ));
            }
            if matches!(self.peek().kind, TokenKind::At) {
                continue;
            }
            break;
        }
        if self.is_eof() {
            return Err(RioError::parse(
                "الزخرفة يجب أن تسبق تعريف دالة أو فئة",
                tok.line,
                tok.col,
            ));
        }
        let defs = self.parse_statement()?;
        if defs.len() != 1 {
            return Err(RioError::parse(
                "الزخرفة يجب أن تسبق تعريف دالة أو فئة",
                tok.line,
                tok.col,
            ));
        }
        let def = defs.into_iter().next().unwrap();
        match &def.kind {
            StmtKind::FuncDef { .. } | StmtKind::ClassDef { .. } => Ok(Stmt::new(
                StmtKind::Decorated { decorators, def: Box::new(def) },
                tok.line,
                tok.col,
            )),
            _ => Err(RioError::parse(
                "الزخرفة يجب أن تسبق تعريف دالة أو فئة",
                def.line,
                def.col,
            )),
        }
    }

    /// اسم زخرفة: معرّف، أو سلسلة منقوطة من معرّفات.
    fn parse_decorator_expr(&mut self) -> RioResult<Expr> {
        let (name, line, col) = self.expect_ident("اسم الزخرفة بعد @")?;
        let mut expr = Expr::new(ExprKind::Ident(name), line, col);
        while matches!(self.peek().kind, TokenKind::Dot) {
            let tok = self.advance();
            let (mname, _, _) = self.expect_member_name()?;
            expr = Expr::new(
                ExprKind::Dot { target: Box::new(expr), name: mname },
                tok.line,
                tok.col,
            );
        }
        Ok(expr)
    }

    fn parse_import(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // استورد
        let (module, _, _) = self.expect_ident("اسم وحدة")?;
        let alias = if matches!(self.peek().kind, TokenKind::Keyword(Keyword::As)) {
            self.advance();
            let (a, _, _) = self.expect_ident("اسم مختصر بعد باسم")?;
            Some(a)
        } else {
            None
        };
        Ok(Stmt::new(
            StmtKind::Import { module, alias, names: None },
            tok.line,
            tok.col,
        ))
    }

    fn parse_from_import(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // من
        let (module, _, _) = self.expect_ident("اسم وحدة بعد من")?;
        self.expect_keyword(Keyword::Import, "استورد")?;
        let mut names = Vec::new();
        loop {
            let (n, _, _) = self.expect_ident("اسم مستورد")?;
            names.push(n);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        if names.is_empty() {
            return Err(RioError::parse(
                "من وحدة استورد تتطلب اسما واحدا على الأقل",
                tok.line,
                tok.col,
            ));
        }
        Ok(Stmt::new(
            StmtKind::Import { module, alias: None, names: Some(names) },
            tok.line,
            tok.col,
        ))
    }

    fn parse_export(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // صدّر
        let mut names = Vec::new();
        loop {
            let (n, _, _) = self.expect_ident("اسم مصدر")?;
            names.push(n);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        Ok(Stmt::new(StmtKind::Export { names }, tok.line, tok.col))
    }

    fn parse_print(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // اطبع
        let mut args = Vec::new();
        if matches!(self.peek().kind, TokenKind::LParen) {
            self.advance();
            if !matches!(self.peek().kind, TokenKind::RParen) {
                loop {
                    args.push(self.parse_expr()?);
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RParen) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
        } else if !(self.is_separator()
            || matches!(self.peek().kind, TokenKind::Dedent | TokenKind::Eof))
        {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        Ok(Stmt::new(StmtKind::Print { args }, tok.line, tok.col))
    }

    fn parse_if(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // إذا
        let cond = self.parse_expr()?;
        self.expect_colon()?;
        let then_body = self.parse_block()?;
        let mut elifs = Vec::new();
        let mut else_body = None;
        // `اواذا` (البند 90) و`والا اذا` شكلان لفرع واحد —
        // الحلقة تقبل كليهما بأي ترتيب حتى `والا` الأخيرة.
        loop {
            if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Elif)) {
                self.advance(); // اواذا
                let econd = self.parse_expr()?;
                self.expect_colon()?;
                let ebody = self.parse_block()?;
                elifs.push((econd, ebody));
            } else if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Else)) {
                self.advance(); // والا
                if matches!(self.peek().kind, TokenKind::Keyword(Keyword::If)) {
                    self.advance(); // اذا (بعد والا)
                    let econd = self.parse_expr()?;
                    self.expect_colon()?;
                    let ebody = self.parse_block()?;
                    elifs.push((econd, ebody));
                } else {
                    self.expect_colon()?;
                    else_body = Some(self.parse_block()?);
                    break;
                }
            } else {
                break;
            }
        }
        Ok(Stmt::new(
            StmtKind::If { cond, then_body, elifs, else_body },
            tok.line,
            tok.col,
        ))
    }

    fn parse_while(&mut self) -> RioResult<Stmt> {
        let tok = self.advance();
        let cond = self.parse_expr()?;
        self.expect_colon()?;
        let body = self.parse_block()?;
        Ok(Stmt::new(StmtKind::While { cond, body }, tok.line, tok.col))
    }

    fn parse_for(&mut self) -> RioResult<Stmt> {
        let tok = self.advance();
        let (var, _, _) = self.expect_ident("اسم متغير بعد لكل")?;
        self.expect_keyword(Keyword::In, "في")?;
        let iterable = self.parse_expr()?;
        self.expect_colon()?;
        let body = self.parse_block()?;
        Ok(Stmt::new(StmtKind::For { var, iterable, body }, tok.line, tok.col))
    }

    fn parse_repeat(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // كرر
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::AsLongAs)) {
            self.advance(); // طالما
            let cond = self.parse_expr()?;
            self.expect_colon()?;
            let body = self.parse_block()?;
            return Ok(Stmt::new(StmtKind::RepeatWhile { cond, body }, tok.line, tok.col));
        }
        let count = self.parse_expr()?;
        self.expect_keyword(Keyword::Times, "مرات")?;
        self.expect_colon()?;
        let body = self.parse_block()?;
        Ok(Stmt::new(StmtKind::RepeatCount { count, body }, tok.line, tok.col))
    }

    /// البنود 109–112 (الاجتماع 83 §98): `[** | *] اسم
    /// [: نوع] [= افتراضي]`؛ والافتراضيات متتالية
    /// من النهاية؛ و`*`/`**` في آخر القائمة؛ وتكرار
    /// الأسماء خطأ. `extended` كاذبة للامدا (بلا
    /// افتراضيات ولا جامعين — تُرفض تحليلا). النوع
    /// المعلن بعد `*اسم`/`**اسم` يُقبل نحويا ويُهمل
    /// دلاليا — §98 يسمّي الأعلام وحدها.
    fn parse_param_list(&mut self, extended: bool, what: &str) -> RioResult<ParamList> {
        self.expect(&TokenKind::LParen, "قوس فتح (")?;
        let mut params: Vec<String> = Vec::new();
        let mut param_types: Vec<Option<String>> = Vec::new();
        let mut defaults: Vec<Option<Expr>> = Vec::new();
        let mut vararg: Option<String> = None;
        let mut kwarg: Option<String> = None;
        let mut seen: Vec<String> = Vec::new();
        let mut seen_default = false;
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                if matches!(self.peek().kind, TokenKind::Star) {
                    if !extended {
                        let t = self.peek().clone();
                        return Err(RioError::parse(
                            "اللامدا لا تقبل معاملات جامعة (* أو **)",
                            t.line,
                            t.col,
                        ));
                    }
                    self.advance(); // *
                    if matches!(self.peek().kind, TokenKind::Star) {
                        self.advance(); // *
                        let (n, l, c) = self.expect_ident("اسم معامل بعد **")?;
                        if seen.contains(&n) {
                            return Err(RioError::parse(
                                format!("معامل مكرر في {}: {}", what, n),
                                l,
                                c,
                            ));
                        }
                        seen.push(n.clone());
                        if matches!(self.peek().kind, TokenKind::Colon) {
                            self.advance();
                            let _ = self.parse_type_name()?;
                        }
                        kwarg = Some(n);
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            let t = self.peek().clone();
                            return Err(RioError::parse(
                                "** يجب أن يكون آخر معامل",
                                t.line,
                                t.col,
                            ));
                        }
                        break;
                    }
                    let (n, l, c) = self.expect_ident("اسم معامل بعد *")?;
                    if seen.contains(&n) {
                        return Err(RioError::parse(
                            format!("معامل مكرر في {}: {}", what, n),
                            l,
                            c,
                        ));
                    }
                    seen.push(n.clone());
                    if matches!(self.peek().kind, TokenKind::Colon) {
                        self.advance();
                        let _ = self.parse_type_name()?;
                    }
                    vararg = Some(n);
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::Star) {
                            continue;
                        }
                        let t = self.peek().clone();
                        return Err(RioError::parse(
                            "بعد *اسم يأتي **اسم فقط أو قوس الإغلاق",
                            t.line,
                            t.col,
                        ));
                    }
                    break;
                }
                let (p, l, c) = self.expect_ident("اسم معامل")?;
                if seen.contains(&p) {
                    return Err(RioError::parse(
                        format!("معامل مكرر في {}: {}", what, p),
                        l,
                        c,
                    ));
                }
                seen.push(p.clone());
                params.push(p.clone());
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance();
                    param_types.push(Some(self.parse_type_name()?));
                } else {
                    param_types.push(None);
                }
                if matches!(self.peek().kind, TokenKind::Eq) {
                    if !extended {
                        let t = self.peek().clone();
                        return Err(RioError::parse(
                            "اللامدا لا تقبل معاملات افتراضية",
                            t.line,
                            t.col,
                        ));
                    }
                    self.advance(); // =
                    defaults.push(Some(self.parse_expr()?));
                    seen_default = true;
                } else {
                    if seen_default {
                        return Err(RioError::parse(
                            format!(
                                "المعامل {} بلا قيمة افتراضية بعد معامل له قيمة افتراضية",
                                p
                            ),
                            l,
                            c,
                        ));
                    }
                    defaults.push(None);
                }
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
        Ok(ParamList { params, param_types, defaults, vararg, kwarg })
    }

    /// البنود 109–112: هل المعامل التالي مفتاحي
    /// (`اسم = تعبير`)؟ الكشف `Ident` يتبعه `=` —
    /// و`:=` رمز آخر فلا التباس.
    fn kwarg_follows(&self) -> Option<String> {
        if let TokenKind::Ident(n) = &self.peek().kind {
            if matches!(self.peek_at(1).kind, TokenKind::Eq) {
                return Some(n.clone());
            }
        }
        None
    }

    /// قراءة معامل مفتاحي واحد (`اسم = تعبير`) بعد
    /// كشف `kwarg_follows` — مع رفض التكرار.
    fn parse_kwarg(&mut self, kwargs: &[(String, Expr)]) -> RioResult<(String, Expr)> {
        let (n, l, c) = self.expect_ident("اسم معامل مفتاحي")?;
        if kwargs.iter().any(|(k, _)| k == &n) {
            return Err(RioError::parse(
                format!("معامل مفتاحي مكرر: {}", n),
                l,
                c,
            ));
        }
        self.expect(&TokenKind::Eq, "علامة = بعد اسم المعامل المفتاحي")?;
        let v = self.parse_expr()?;
        Ok((n, v))
    }

    /// البنود 145–147 (العموميات — الاجتماع 83 §98):
    /// تحليل `[ت: حد = افتراضي، ...]` بعد اسم دالة أو
    /// فئة — والرمز الحالي `[`. الحد والافتراضي نوعان
    /// خامان (بلا توسعة أسماء مستعارة وبلا فحص معرفة
    /// هنا؛ والفحص الدلالي عند التعريف). التكرار ومع
    /// نوع معروف أو اسم مستعار خطأ نحوي.
    fn parse_type_params(&mut self) -> RioResult<Vec<crate::ast::TypeParam>> {
        self.expect(&TokenKind::LBracket, "قوس فتح [ لمعاملات النوع")?;
        let mut out: Vec<crate::ast::TypeParam> = Vec::new();
        loop {
            let (name, line, col) = self.expect_ident("اسم معامل نوع")?;
            if out.iter().any(|p| p.name == name) {
                return Err(RioError::parse(
                    format!("معامل نوع مكرر: {}", name),
                    line,
                    col,
                ));
            }
            if crate::ast::is_known_type(&name) {
                return Err(RioError::parse(
                    format!("الاسم {} نوع معروف، ولا يمكن تعريفه معامل نوع", name),
                    line,
                    col,
                ));
            }
            if self.type_aliases.contains_key(&name) {
                return Err(RioError::parse(
                    format!("الاسم {} اسم مستعار، ولا يمكن تعريفه معامل نوع", name),
                    line,
                    col,
                ));
            }
            let mut bound: Option<String> = None;
            let mut default: Option<String> = None;
            if matches!(self.peek().kind, TokenKind::Colon) {
                self.advance(); // :
                bound = Some(self.parse_type_expr_raw()?.display());
            }
            if matches!(self.peek().kind, TokenKind::Eq) {
                self.advance(); // =
                default = Some(self.parse_type_expr_raw()?.display());
            }
            out.push(crate::ast::TypeParam::new(name, bound, default));
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        self.expect(&TokenKind::RBracket, "قوس إغلاق ] بعد معاملات النوع")?;
        Ok(out)
    }

    /// هل أساس تعبير النوع اسم نوع معروف هنا (معروف
    /// أصلا، أو مستعار، أو فئة/واجهة معرَّفة أعلاه)؟
    fn is_type_arg_base(&self, name: &str) -> bool {
        crate::ast::is_known_type(name)
            || self.type_aliases.contains_key(name)
            || self.defined_types.contains(name)
            || self.type_param_stack.iter().any(|s| s.contains(name))
    }

    /// محاولة تخمينية: `[نوع، ...]` يتبعها `(` بعد
    /// معرّف — تخصيص عام `اسم[نوع](...)` لا فهرسة.
    /// عند أي فشل تُستعاد المواضع كلها (فلا أثر)،
    /// وتعود لا شيء فيسلك التحليل مسار الفهرسة.
    fn try_parse_type_apply(&mut self) -> Option<Vec<String>> {
        let saved = self.pos;
        let ok = (|| -> RioResult<Vec<String>> {
            self.advance(); // [
            let mut args: Vec<String> = Vec::new();
            loop {
                let t = self.parse_type_expr_raw()?;
                if !self.is_type_arg_base(t.base_name()) {
                    return Err(RioError::parse(
                        "ليس نوعا".to_string(),
                        0,
                        0,
                    ));
                }
                args.push(t.display());
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            self.expect(&TokenKind::RBracket, "قوس إغلاق ] بعد معاملات النوع")?;
            if !matches!(self.peek().kind, TokenKind::LParen) {
                return Err(RioError::parse("ليس استدعاء".to_string(), 0, 0));
            }
            Ok(args)
        })();
        match ok {
            Ok(args) => Some(args),
            Err(_) => {
                self.pos = saved;
                None
            }
        }
    }

    fn parse_funcdef(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // دالة
        // Constructor method name أنشئ is a keyword, not an Ident,
        // so accept it explicitly as a method name here.
        let (name, _, _) = if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Create)) {
            self.advance();
            ("أنشئ".to_string(), tok.line, tok.col)
        } else {
            self.expect_ident("اسم الدالة")?
        };
        let what = format!("الدالة {}", name);
        // البنود 145–147: معاملات النوع بعد الاسم مباشرة
        // (`دالة اسم[ت: حد = افتراضي](...)`)؛ وهي في
        // النطاق طوال المعاملات ونوع الإرجاع والجسم.
        let mut type_params: Vec<crate::ast::TypeParam> = Vec::new();
        if matches!(self.peek().kind, TokenKind::LBracket) {
            type_params = self.parse_type_params()?;
        }
        let tp_names: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        self.type_param_stack.push(tp_names);
        let pl = self.parse_param_list(true, &what)?;
        let ParamList { params, param_types, defaults, vararg, kwarg } = pl;
        // Optional return type: ): عدد:  — أو عام: ): قائمة[عدد]:
        // After ')', a Colon followed by a type name (with optional
        // [ ... ] type arguments) and another Colon is a return
        // type; a Colon followed by a block (Newline/Indent) is
        // just the block colon.
        // Meeting 26 (§41.3): لاشيء is a type name too, but it
        // lexes as Keyword::Nil — the lookahead must accept it.
        let mut return_type: Option<String> = None;
        if matches!(self.peek().kind, TokenKind::Colon) && self.return_type_follows() {
            self.advance(); // :
            return_type = Some(self.parse_type_name()?);
        }
        self.expect_colon()?;
        let body = self.parse_block()?;
        self.type_param_stack.pop();
        Ok(Stmt::new(
            StmtKind::FuncDef { name, params, param_types, return_type, defaults, vararg, kwarg, type_params, body },
            tok.line,
            tok.col,
        ))
    }

    fn parse_classdef(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // فئة
        let (name, _, _) = self.expect_ident("اسم الفئة")?;
        self.defined_types.insert(name.clone());
        // البنود 145–147: معاملات النوع بعد الاسم مباشرة
        // (`فئة اسم[ت: حد = افتراضي]:`)؛ وهي في النطاق
        // طوال الطرائق.
        let mut type_params: Vec<crate::ast::TypeParam> = Vec::new();
        if matches!(self.peek().kind, TokenKind::LBracket) {
            type_params = self.parse_type_params()?;
        }
        let tp_names: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        self.type_param_stack.push(tp_names);
        // Optional: ورث أب
        let mut base: Option<String> = None;
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Inherit)) {
            self.advance();
            let (b, _, _) = self.expect_ident("اسم فئة الأب بعد ورث")?;
            base = Some(b);
        }
        // Optional: واجهة و1، و2
        let mut interfaces: Vec<String> = Vec::new();
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Interface)) {
            self.advance();
            loop {
                let (iname, _, _) = self.expect_ident("اسم واجهة")?;
                interfaces.push(iname);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect_colon()?;
        let body = self.parse_block()?;
        self.type_param_stack.pop();
        let mut methods: Vec<Stmt> = Vec::new();
        for stmt in body {
            match &stmt.kind {
                StmtKind::FuncDef { name: mname, params, .. } => {
                    if params.iter().any(|p| p == "نفس") {
                        return Err(RioError::parse(
                            "نفس محجوز داخل طرق الفئة، ولا يكتب كمعامل",
                            stmt.line,
                            stmt.col,
                        ));
                    }
                    if methods.iter().any(|m| matches!(&m.kind, StmtKind::FuncDef { name: n, .. } if n == mname)) {
                        return Err(RioError::parse(
                            format!("طريقة مكررة في الفئة {}: {}", name, mname),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                    methods.push(stmt);
                }
                StmtKind::Pass => {}
                // البند 8: الزخرفة على طريقة فئة غير مدعومة —
                // الزخرفة قبل تعريف دالة أو فئة وحدهما.
                StmtKind::Decorated { .. } => {
                    return Err(RioError::parse(
                        "الزخرفة على طريقة فئة غير مدعومة — ضعها على الدالة أو الفئة نفسها",
                        stmt.line,
                        stmt.col,
                    ));
                }
                _ => {
                    return Err(RioError::parse(
                        "جسم الفئة يجب أن يحتوي على دوال فقط",
                        stmt.line,
                        stmt.col,
                    ));
                }
            }
        }
        Ok(Stmt::new(StmtKind::ClassDef { name, base, interfaces, type_params, methods }, tok.line, tok.col))
    }

    fn parse_interfacedef(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // واجهة
        let (name, _, _) = self.expect_ident("اسم الواجهة")?;
        self.defined_types.insert(name.clone());
        self.expect_colon()?;
        let body = self.parse_block()?;
        let mut methods: Vec<(String, usize)> = Vec::new();
        for stmt in body {
            match &stmt.kind {
                StmtKind::FuncDef { name: mname, params, body: mbody, .. } => {
                    // Interface methods must have no implementation:
                    // body must be exactly مرر.
                    let is_pass_only = mbody.len() == 1
                        && matches!(mbody[0].kind, StmtKind::Pass);
                    if !is_pass_only {
                        return Err(RioError::parse(
                            "طريقة الواجهة يجب أن تكون بلا تنفيذ (مرر)",
                            stmt.line,
                            stmt.col,
                        ));
                    }
                    if methods.iter().any(|(n, _)| n == mname) {
                        return Err(RioError::parse(
                            format!("طريقة مكررة في الواجهة {}: {}", name, mname),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                    methods.push((mname.clone(), params.len()));
                }
                StmtKind::Pass => {}
                _ => {
                    return Err(RioError::parse(
                        "جسم الواجهة يجب أن يحتوي على توقيعات دوال فقط",
                        stmt.line,
                        stmt.col,
                    ));
                }
            }
        }
        Ok(Stmt::new(StmtKind::InterfaceDef { name, methods }, tok.line, tok.col))
    }

    fn parse_try(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // حاول
        self.expect_colon()?;
        let try_body = self.parse_block()?;
        let mut except_body = None;
        let mut finally_body = None;
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Except)) {
            self.advance();
            self.expect_colon()?;
            except_body = Some(self.parse_block()?);
        }
        // البند 98 (`والا` بعد حاول — الاجتماع 83 §98):
        // الترتيب حاول، ماعدا؟، والا؟، أخيرا؟ — ووالا
        // وحدها لا تكفي (يبقى شرط «ماعدا أو أخيرا»).
        let mut else_body = None;
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Else)) {
            self.advance();
            self.expect_colon()?;
            else_body = Some(self.parse_block()?);
        }
        if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Finally)) {
            self.advance();
            self.expect_colon()?;
            finally_body = Some(self.parse_block()?);
        }
        if except_body.is_none() && finally_body.is_none() {
            return Err(RioError::parse(
                "حاول يجب أن يتبعه ماعدا أو أخيرا",
                tok.line,
                tok.col,
            ));
        }
        Ok(Stmt::new(
            StmtKind::Try { try_body, except_body, else_body, finally_body },
            tok.line,
            tok.col,
        ))
    }

    /// البند 88 — مدير السياق (الاجتماع 88 §103):
    /// الصياغة الأربع من §99، والمتزامنة منها
    /// (`زامن عند`) مرفوضة في `parse_statement`
    /// قبل الوصول هنا. الصيغتان المتزامنتان:
    /// `عند (عنصر، عنصر،): كتلة` (بفاصلة أخيرة
    /// اختيارية) و`عند عنصر، عنصر: كتلة`؛ والعنصر
    /// `تعبير [ك هدف]`، والهدف اسم أو فهرسة أو حقل.
    fn parse_with(&mut self) -> RioResult<Vec<Stmt>> {
        let tok = self.advance(); // عند
        let (line, col) = (tok.line, tok.col);
        let mut elements: Vec<(Expr, Option<AssignTarget>)> = Vec::new();
        if matches!(self.peek().kind, TokenKind::LParen) {
            self.advance(); // (
            loop {
                self.skip_separators();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    break;
                }
                elements.push(self.parse_with_element()?);
                self.skip_separators();
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    break;
                }
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "توقع ، أو ) داخل عند",
                    t.line,
                    t.col,
                ));
            }
        } else {
            loop {
                elements.push(self.parse_with_element()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    // فاصلة أخيرة مسموحة قبل النقطتين.
                    if matches!(self.peek().kind, TokenKind::Colon) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        if elements.is_empty() {
            return Err(RioError::parse(
                "عند يتطلب عنصرا واحدا على الأقل",
                line,
                col,
            ));
        }
        self.expect_colon()?;
        let body = self.parse_block()?;
        Ok(Self::desugar_with(elements, body, line, col))
    }

    /// عنصر `عند` واحد: تعبير، وربط اختياري بـ`ك`
    /// (معرّف عادي بعد التطبيع، كـ`ولد` و`من`).
    fn parse_with_element(&mut self) -> RioResult<(Expr, Option<AssignTarget>)> {
        let expr = self.parse_expr()?;
        let target = if self.peek_is_kaf() {
            self.advance(); // ك
            let texpr = self.parse_expr()?;
            Some(Self::expr_to_assign_target(texpr)?)
        } else {
            None
        };
        Ok((expr, target))
    }

    /// هل الرمز الحالي `ك` (ربط مدير السياق)؟
    fn peek_is_kaf(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(name) => crate::lexer::normalize_cli_word(name) == "ك",
            _ => false,
        }
    }

    /// تعبير إلى هدف إسناد (كـهدف التفكيك): اسم أو
    /// فهرسة أو حقل — لا تعبير آخر.
    fn expr_to_assign_target(expr: Expr) -> RioResult<AssignTarget> {
        match expr.kind {
            ExprKind::Ident(name) => Ok(AssignTarget::Var(name)),
            ExprKind::Index { target, index } => {
                Ok(AssignTarget::Index { target, index })
            }
            ExprKind::Dot { target, name } => {
                Ok(AssignTarget::Dot { target, name })
            }
            _ => Err(RioError::parse(
                "هدف ك يجب أن يكون اسما أو فهرسة أو حقلا، لا تعبيرا",
                expr.line,
                expr.col,
            )),
        }
    }

    /// البند 88 — تفكيك `عند` إلى عقد `حاول`/
    /// `ماعدا`/`أخيرا` القائمة — بلا عقدة جديدة وبلا
    /// لمس cgen.rs. لكل عنصر: مؤقت للمدير (تقييم مرة
    /// واحدة)، وفحص الدالتين قبل أي استدعاء، واستدعاء
    /// `دخول` مرة واحدة، وربط `ك` (أو المدير نفسه إن
    /// رجع لاشيء)، وراية خروج. والجسم داخل `حاول`؛
    /// و`ماعدا` تستدعي `خروج` بثلاثية الخطأ (نوع،
    /// رسالة، لاشيء — لا قيمة أثر في اللغة) وتقمع
    /// برجوع `صح` حرفيا وحده، وإلا تعيد رفع الخطأ
    /// نفسه من القاموس المحفوظ؛ و`أخيرا` تستدعي
    /// `خروج` بالثلاثية لاشيء عند الخروج الطبيعي (أو
    /// ارجع/اكسر/تابع) مع راية تمنع الازدواج. والراية
    /// تُضبط قبل الاستدعاء في المسارين، فإعادة تشغيل
    /// `أخيرا` في الثنائي الأصلي عند خطأ الخروج لا
    /// تعيد الاستدعاء؛ وفرع `وإلا` في `ماعدا` يعيد
    /// رفع خطأ `أخيرا` نفسه حين تشغّل `ماعدا` خطأ
    /// في المسار الطبيعي (سلوك الأصلي الموثق) بدلا
    /// من ابتلاعه. والعناصر متداخلة: دخول بالترتيب
    /// وخروج بالعكس، وخطأ دخول لاحق يخرج السابقين
    /// بالثلاثية نفسها.
    fn desugar_with(
        elements: Vec<(Expr, Option<AssignTarget>)>,
        body: Vec<Stmt>,
        line: usize,
        col: usize,
    ) -> Vec<Stmt> {
        let nil = || Expr::new(ExprKind::Nil, line, col);
        let false_e = || Expr::new(ExprKind::Bool(false), line, col);
        let true_e = || Expr::new(ExprKind::Bool(true), line, col);
        let mut current = body;
        for (i, (expr, target)) in elements.into_iter().enumerate().rev() {
            let mgr = format!("__عند_مدير_{}_{}_{}", line, col, i);
            let val = format!("__عند_قيمة_{}_{}_{}", line, col, i);
            let flag = format!("__عند_خرج_{}_{}_{}", line, col, i);
            let err = format!("__عند_خطأ_{}_{}_{}", line, col, i);
            let res = format!("__عند_نتيجة_{}_{}_{}", line, col, i);
            let mgr_e = Self::mk_ident(&mgr, line, col);
            let val_e = Self::mk_ident(&val, line, col);
            let flag_e = Self::mk_ident(&flag, line, col);
            let err_e = Self::mk_ident(&err, line, col);
            let res_e = Self::mk_ident(&res, line, col);
            let enter = Expr::new(
                ExprKind::Call {
                    callee: Box::new(Self::mk_dot(mgr_e.clone(), "دخول", line, col)),
                    args: vec![],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
                line,
                col,
            );
            let err_kind =
                Self::mk_index(err_e.clone(), Self::mk_str("نوع", line, col), line, col);
            let err_msg =
                Self::mk_index(err_e.clone(), Self::mk_str("رسالة", line, col), line, col);
            let exit_err = Expr::new(
                ExprKind::Call {
                    callee: Box::new(Self::mk_dot(mgr_e.clone(), "خروج", line, col)),
                    args: vec![err_kind, err_msg, nil()],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
                line,
                col,
            );
            let exit_nil = Expr::new(
                ExprKind::Call {
                    callee: Box::new(Self::mk_dot(mgr_e.clone(), "خروج", line, col)),
                    args: vec![nil(), nil(), nil()],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
                line,
                col,
            );
            let flag_is_false =
                Self::mk_bin(flag_e.clone(), BinOp::Eq, false_e(), line, col);
            let mut prologue = vec![
                Self::mk_assign_var(&mgr, expr, line, col),
                Stmt::new(
                    StmtKind::ExprStmt {
                        expr: Self::mk_global_call(
                            crate::ast::CTX_CHECK,
                            vec![mgr_e.clone()],
                            line,
                            col,
                        ),
                    },
                    line,
                    col,
                ),
                Self::mk_assign_var(&val, enter, line, col),
                Stmt::new(
                    StmtKind::If {
                        cond: Self::mk_bin(val_e.clone(), BinOp::Eq, nil(), line, col),
                        then_body: vec![Self::mk_assign_var(
                            &val,
                            mgr_e.clone(),
                            line,
                            col,
                        )],
                        elifs: vec![],
                        else_body: None,
                    },
                    line,
                    col,
                ),
                Self::mk_assign_var(&flag, false_e(), line, col),
            ];
            if let Some(t) = target {
                prologue.push(Stmt::new(
                    StmtKind::Assign {
                        target: t,
                        value: val_e.clone(),
                        annotation: None,
                        append: false,
                    },
                    line,
                    col,
                ));
            }
            let except_body = vec![Stmt::new(
                StmtKind::If {
                    cond: flag_is_false.clone(),
                    then_body: vec![
                        Self::mk_assign_var(
                            &err,
                            Self::mk_global_call(crate::ast::CTX_ERROR, vec![], line, col),
                            line,
                            col,
                        ),
                        Self::mk_assign_var(&flag, true_e(), line, col),
                        Self::mk_assign_var(&res, exit_err, line, col),
                        Stmt::new(
                            StmtKind::If {
                                cond: Self::mk_bin(
                                    res_e.clone(),
                                    BinOp::Eq,
                                    true_e(),
                                    line,
                                    col,
                                ),
                                then_body: vec![Stmt::new(StmtKind::Pass, line, col)],
                                elifs: vec![],
                                else_body: Some(vec![Stmt::new(
                                    StmtKind::ExprStmt {
                                        expr: Self::mk_global_call(
                                            crate::ast::CTX_RERAISE,
                                            vec![err_e.clone()],
                                            line,
                                            col,
                                        ),
                                    },
                                    line,
                                    col,
                                )]),
                            },
                            line,
                            col,
                        ),
                    ],
                    elifs: vec![],
                    else_body: Some(vec![Stmt::new(
                        StmtKind::ExprStmt {
                            expr: Self::mk_global_call(
                                crate::ast::CTX_RERAISE,
                                vec![Self::mk_global_call(
                                    crate::ast::CTX_ERROR,
                                    vec![],
                                    line,
                                    col,
                                )],
                                line,
                                col,
                            ),
                        },
                        line,
                        col,
                    )]),
                },
                line,
                col,
            )];
            let finally_body = vec![Stmt::new(
                StmtKind::If {
                    cond: flag_is_false,
                    then_body: vec![
                        Self::mk_assign_var(&flag, true_e(), line, col),
                        Self::mk_assign_var(&res, exit_nil, line, col),
                    ],
                    elifs: vec![],
                    else_body: None,
                },
                line,
                col,
            )];
            prologue.push(Stmt::new(
                StmtKind::Try {
                    try_body: current,
                    except_body: Some(except_body),
                    else_body: None,
                    finally_body: Some(finally_body),
                },
                line,
                col,
            ));
            current = prologue;
        }
        current
    }

    /// هل بعد `:` الحالية نوع إرجاع ثم `:` كتلة؟ يمسح
    /// اسم نوع (معرّف أو لاشيء)، ثم معاملات نوع اختيارية
    /// `[ ... ]` متوازنة، ثم `:` — فالنوع العام
    /// `قائمة[عدد]` نوع إرجاع أيضا لا كتلة.
    fn return_type_follows(&self) -> bool {
        let k = |j: usize| &self.peek_at(j).kind;
        let mut i = 1usize;
        // أسماء أنواع تُقرأ كلمات محجوزة لا معرّفات
        // (كـ لاشيء في الاجتماع 26): دالة وفئة وواجهة.
        if !(matches!(k(i), TokenKind::Ident(_))
            || matches!(
                k(i),
                TokenKind::Keyword(Keyword::Nil)
                    | TokenKind::Keyword(Keyword::Func)
                    | TokenKind::Keyword(Keyword::Class)
                    | TokenKind::Keyword(Keyword::Interface)
            ))
        {
            return false;
        }
        i += 1;
        if matches!(k(i), TokenKind::LBracket) {
            let mut depth = 0i32;
            loop {
                match k(i) {
                    TokenKind::LBracket => depth += 1,
                    TokenKind::RBracket => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    TokenKind::Eof | TokenKind::Newline | TokenKind::Dedent => {
                        return false;
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        matches!(k(i), TokenKind::Colon)
    }

    fn parse_type_name(&mut self) -> RioResult<String> {
        Ok(self.parse_type_expr()?.display())
    }

    /// البند 104 (`نوع` — الاجتماع 83 §98): بيان اسم
    /// مستعار — `نوع اسم = نوع`. الاسم المصادم لنوع
    /// معروف مرفوض؛ والهدف يُحلَّل خاما (بلا توسعة
    /// وبلا فحص معرفة — فيسمح بالإشارة الأمامية،
    /// وتُكشف الدورات والمجهول عند الاستعمال)؛
    /// وإعادة التعريف تستبدل.
    fn parse_type_alias(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // نوع
        let (name, _, _) = self.expect_ident("اسم النوع المستعار")?;
        if crate::ast::is_known_type(&name) {
            return Err(RioError::parse(
                format!(
                    "الاسم {} نوع معروف، ولا يمكن تعريفه اسما مستعارا",
                    name
                ),
                tok.line,
                tok.col,
            ));
        }
        self.expect(&TokenKind::Eq, "علامة = بعد اسم النوع المستعار")?;
        let target = self.parse_type_expr_raw()?;
        let target_str = target.display();
        self.type_aliases.insert(name.clone(), target_str.clone());
        Ok(Stmt::new(
            StmtKind::TypeAlias { name, target: target_str },
            tok.line,
            tok.col,
        ))
    }

    /// تحليل نوع خام لهدف الاسم المستعار: كـ
    /// parse_type_expr لكن بلا توسعة أسماء مستعارة
    /// وبلا فحص المعرفة — الأساس اسم فحسب، ومعاملاته
    /// تُحلَّل خاما بتكرار. عدد المعاملات يُفحص هنا
    /// فقط حين يكون الأساس نوعا عاما معروفا (قائمة/
    /// قاموس) أو نوعا مسمى لا يقبل معاملات؛ وإلا
    /// يُؤجَّل الفحص إلى الاستعمال.
    fn parse_type_expr_raw(&mut self) -> RioResult<TypeExpr> {
        let (name, line, col) = match &self.peek().kind {
            TokenKind::Keyword(kw) => {
                let spelled = match kw {
                    Keyword::Nil => Some("لاشيء"),
                    Keyword::Func => Some("دالة"),
                    Keyword::Class => Some("فئة"),
                    Keyword::Interface => Some("واجهة"),
                    _ => None,
                };
                match spelled {
                    Some(n) => {
                        let t = self.advance();
                        (n.to_string(), t.line, t.col)
                    }
                    None => self.expect_ident("اسم نوع")?,
                }
            }
            _ => self.expect_ident("اسم نوع")?,
        };
        if matches!(self.peek().kind, TokenKind::LBracket) {
            self.advance(); // [
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type_expr_raw()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            self.expect(&TokenKind::RBracket, "قوس إغلاق ] بعد معاملات النوع")?;
            match (name.as_str(), args.len()) {
                ("قائمة", 1) | ("قاموس", 2) => Ok(TypeExpr::Generic { name, args }),
                ("قائمة", n) => Err(RioError::parse(
                    format!("النوع قائمة يقبل معامل نوع واحدا، لكن أعطيت {}", n),
                    line,
                    col,
                )),
                ("قاموس", n) => Err(RioError::parse(
                    format!("النوع قاموس يقبل معاملي نوع، لكن أعطيت {}", n),
                    line,
                    col,
                )),
                (n, _) if crate::ast::is_known_type(n) => Err(RioError::parse(
                    format!("النوع {} لا يقبل معاملات نوع", n),
                    line,
                    col,
                )),
                _ => Ok(TypeExpr::Generic { name, args }),
            }
        } else {
            Ok(TypeExpr::Named(name))
        }
    }

    /// توسعة اسم (مستعار أو معروف) إلى نوعه النهائي —
    /// بتكرار مع كشف الدورات (مسار التوسعة وحده في
    /// `seen`، فيُدفع الاسم ويُسحب بعد توسعته — فلا
    /// تتداخل الفروع الشقيقة كـ `قاموس[أ، أ]`).
    fn expand_type_alias(
        &self,
        name: &str,
        seen: &mut Vec<String>,
        line: usize,
        col: usize,
    ) -> RioResult<TypeExpr> {
        if seen.iter().any(|n| n == name) {
            return Err(RioError::parse(
                format!("تعريف نوع دائري: {}", name),
                line,
                col,
            ));
        }
        // البنود 145–147: اسم معامل نوع في نطاق تعريف
        // محيط نوع صالح في ذاته — بلا توسعة وبلا فحص.
        if self.type_param_stack.iter().any(|s| s.contains(name)) {
            return Ok(TypeExpr::Named(name.to_string()));
        }
        match self.type_aliases.get(name) {
            Some(target) => {
                let parsed = crate::ast::TypeExpr::parse(target).ok_or_else(|| {
                    RioError::parse(format!("نوع غير معروف: {}", name), line, col)
                })?;
                seen.push(name.to_string());
                let r = self.expand_type_expr(&parsed, seen, line, col);
                seen.pop();
                r
            }
            None => {
                if crate::ast::is_known_type(name) {
                    Ok(TypeExpr::Named(name.to_string()))
                } else {
                    Err(RioError::parse(
                        format!("نوع غير معروف: {}", name),
                        line,
                        col,
                    ))
                }
            }
        }
    }

    /// توسعة تعبير نوع: أساسه المستعار يُستبدل (مع
    /// بقاء معاملات الاستعمال)، ومعاملاته تُوسَّع
    /// بتكرار؛ والمستعار إلى عام بمعاملات استعمال خطأ.
    fn expand_type_expr(
        &self,
        t: &TypeExpr,
        seen: &mut Vec<String>,
        line: usize,
        col: usize,
    ) -> RioResult<TypeExpr> {
        match t {
            TypeExpr::Named(n) => self.expand_type_alias(n, seen, line, col),
            TypeExpr::Generic { name, args } => {
                let base = self.expand_type_alias(name, seen, line, col)?;
                match base {
                    TypeExpr::Named(bn) => {
                        let mut out_args = Vec::new();
                        for a in args {
                            out_args.push(self.expand_type_expr(a, seen, line, col)?);
                        }
                        Ok(TypeExpr::Generic { name: bn, args: out_args })
                    }
                    TypeExpr::Generic { .. } => Err(RioError::parse(
                        format!("النوع {} لا يقبل معاملات نوع", name),
                        line,
                        col,
                    )),
                }
            }
        }
    }

    /// البند 8 — تمثيل نوعي حقيقي: اسم مسمى، أو نوع عام
    /// مطبَّق `اسم[نوع، نوع]` (تداخل مسموح). الأساس يجب
    /// أن يكون نوعا معروفا؛ والمعاملات: `قائمة` معامل
    /// واحد، و`قاموس` معاملان، وغيرهما لا يقبل معاملات
    /// نوع. العرض المخزّن قانوني (`قائمة[عدد]`) فيُحلَّل
    /// من جديد بنيويا عند كل فحص.
    fn parse_type_expr(&mut self) -> RioResult<TypeExpr> {
        // Meeting 26 (§41.3): لاشيء is a writable type name, but
        // it lexes as Keyword::Nil, not Ident — accept it here
        // only (params, return type, variable annotation all pass
        // through this one function). والبند 8: دالة وفئة
        // وواجهة كلمات محجوزة أيضا فتُقبل أسماء أنواع هنا
        // وحدها باللفظ نفسه.
        let (name, line, col) = match &self.peek().kind {
            TokenKind::Keyword(kw) => {
                let spelled = match kw {
                    Keyword::Nil => Some("لاشيء"),
                    Keyword::Func => Some("دالة"),
                    Keyword::Class => Some("فئة"),
                    Keyword::Interface => Some("واجهة"),
                    _ => None,
                };
                match spelled {
                    Some(n) => {
                        let t = self.advance();
                        (n.to_string(), t.line, t.col)
                    }
                    None => self.expect_ident("اسم نوع")?,
                }
            }
            _ => self.expect_ident("اسم نوع")?,
        };
        // البند 104 (`نوع`): الاسم المستعار يُوسَّع
        // هنا — قبل أي فحص معرفة — إلى نوعه النهائي
        // (سلاسل بتكرار، والدورات خطأ)؛ والمعروف
        // يبقى مسمى كما هو.
        let base = self.expand_type_alias(&name, &mut Vec::new(), line, col)?;
        if matches!(self.peek().kind, TokenKind::LBracket) {
            self.advance(); // [
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type_expr()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            self.expect(&TokenKind::RBracket, "قوس إغلاق ] بعد معاملات النوع")?;
            match (&base, args.len()) {
                (TypeExpr::Named(n), 1) if n == "قائمة" => {
                    Ok(TypeExpr::Generic { name: n.clone(), args })
                }
                (TypeExpr::Named(n), 2) if n == "قاموس" => {
                    Ok(TypeExpr::Generic { name: n.clone(), args })
                }
                (TypeExpr::Named(n), k) if n == "قائمة" => Err(RioError::parse(
                    format!("النوع قائمة يقبل معامل نوع واحدا، لكن أعطيت {}", k),
                    line,
                    col,
                )),
                (TypeExpr::Named(n), k) if n == "قاموس" => Err(RioError::parse(
                    format!("النوع قاموس يقبل معاملي نوع، لكن أعطيت {}", k),
                    line,
                    col,
                )),
                (TypeExpr::Named(n), _) => Err(RioError::parse(
                    format!("النوع {} لا يقبل معاملات نوع", n),
                    line,
                    col,
                )),
                (TypeExpr::Generic { .. }, _) => Err(RioError::parse(
                    format!("النوع {} لا يقبل معاملات نوع", name),
                    line,
                    col,
                )),
            }
        } else {
            Ok(base)
        }
    }

    /// Whether the reversed-assignment prototype applies at the
    /// current position: the left side is not a valid target,
    /// and a bare name followed by end-of-statement comes after
    /// `=`. Compiled out entirely unless the `prototype-rtl`
    /// feature is enabled.
    #[cfg(feature = "prototype-rtl")]
    fn reversed_assignment_applies(&self, expr_is_target: bool) -> bool {
        !expr_is_target
            && matches!(self.peek_at(1).kind, TokenKind::Ident(_))
            && matches!(
                self.peek_at(2).kind,
                TokenKind::Newline
                    | TokenKind::Semicolon
                    | TokenKind::Dedent
                    | TokenKind::Eof
            )
    }

    #[cfg(not(feature = "prototype-rtl"))]
    fn reversed_assignment_applies(&self, _expr_is_target: bool) -> bool {
        false
    }

// == مولّد-دفعة2-مساعد-الاستدعاء: بداية ==
    /// بناء استدعاء `رياضيات.اسم(يسار، يمين)` كعقدة AST —
    /// تفكيك العوامل الجديدة إلى استدعاءات مكتبية قائمة،
    /// بلا عقد جديدة، فيعمل المفسر والآلة والأصلي كما هي.
    /// يتطلب البرنامج `استورد رياضيات` كما يتطلبه
    /// استدعاء الدالة مباشرة.
    fn math_call(name: &str, left: Expr, right: Expr, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Call {
                callee: Box::new(Expr::new(
                    ExprKind::Dot {
                        target: Box::new(Expr::new(ExprKind::Ident("رياضيات".to_string()), line, col)),
                        name: name.to_string(),
                    },
                    line,
                    col,
                )),
                args: vec![left, right],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
            line,
            col,
        )
    }

    /// بناء استدعاء `رياضيات.اسم(معامل)` الأحادي.
    fn math_call1(name: &str, operand: Expr, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Call {
                callee: Box::new(Expr::new(
                    ExprKind::Dot {
                        target: Box::new(Expr::new(ExprKind::Ident("رياضيات".to_string()), line, col)),
                        name: name.to_string(),
                    },
                    line,
                    col,
                )),
                args: vec![operand],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
            line,
            col,
        )
    }
// == مولّد-دفعة2-مساعد-الاستدعاء: نهاية ==

    /// بناء استدعاء `تحقق.اسم(وسائط...)` كعقدة AST —
    /// كـmath_call لكن لوحدة تحقق (البندان 69 و81).
    /// يتطلب البرنامج `استورد تحقق` كما يتطلب
    /// استدعاء رياضيات استيرادها.
    fn validate_call(name: &str, args: Vec<Expr>, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Call {
                callee: Box::new(Expr::new(
                    ExprKind::Dot {
                        target: Box::new(Expr::new(ExprKind::Ident("تحقق".to_string()), line, col)),
                        name: name.to_string(),
                    },
                    line,
                    col,
                )),
                args,
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
            line,
            col,
        )
    }

    /// البند 81 — `متوقع`: `متوقع شرط` أو
    /// `متوقع شرط، "رسالة"` يُفكَّك إلى استدعاء
    /// `تحقق.متوقع(...)`؛ والرفض بخطأ عربي وقت
    /// التشغيل عند فشل الشرط (لا عقد جديدة).
    fn parse_assert(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // متوقع
        let cond = self.parse_expr()?;
        let mut args = vec![cond];
        if matches!(self.peek().kind, TokenKind::Comma) {
            self.advance(); // ،
            args.push(self.parse_expr()?);
            if matches!(self.peek().kind, TokenKind::Comma) {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "متوقع يقبل شرطا ورسالة اختيارية واحدة فقط",
                    t.line,
                    t.col,
                ));
            }
        }
        let (line, col) = (tok.line, tok.col);
        Ok(Stmt::new(
            StmtKind::ExprStmt { expr: Self::validate_call("متوقع", args, line, col) },
            line,
            col,
        ))
    }

    /// البندان 99 و100 — `طابق موضوع:` ثم حالات
    /// `حال نمط [اذا حراسة]:` بمسافة بادئة. أول حالة
    /// مطابقة تفوز؛ وبلا مطابقة لا ينفَّذ شيء (كما في
    /// ألف v5)؛ والموضوع يُقيَّم مرة واحدة في مؤقت.
    fn parse_match(&mut self) -> RioResult<Vec<Stmt>> {
        let tok = self.advance(); // طابق
        let (line, col) = (tok.line, tok.col);
        let subject = self.parse_expr()?;
        self.expect_colon()?;
        if !matches!(self.peek().kind, TokenKind::Newline) {
            let t = self.peek().clone();
            return Err(RioError::parse(
                "طابق يتطلب حالات بمسافة بادئة بعد النقطتين",
                t.line,
                t.col,
            ));
        }
        self.advance(); // Newline
        if !matches!(self.peek().kind, TokenKind::Indent) {
            let t = self.peek().clone();
            return Err(RioError::parse(
                "طابق يتطلب حالات بمسافة بادئة بعد النقطتين",
                t.line,
                t.col,
            ));
        }
        self.advance(); // Indent
        let mut cases: Vec<(Pat, Option<Expr>, Vec<Stmt>)> = Vec::new();
        loop {
            self.skip_separators();
            match self.peek().kind {
                TokenKind::Dedent => {
                    self.advance();
                    break;
                }
                TokenKind::Eof => {
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "نهاية غير متوقعة داخل طابق",
                        t.line,
                        t.col,
                    ));
                }
                TokenKind::Keyword(Keyword::Case) => {
                    self.advance(); // حال
                    let alts = self.parse_pattern()?;
                    let guard = if matches!(self.peek().kind, TokenKind::Keyword(Keyword::If)) {
                        self.advance(); // اذا (حراسة، البند 161)
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.expect_colon()?;
                    let body = self.parse_block()?;
                    for alt in alts {
                        cases.push((alt, guard.clone(), body.clone()));
                    }
                }
                _ => {
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "توقع حال داخل طابق",
                        t.line,
                        t.col,
                    ));
                }
            }
        }
        if cases.is_empty() {
            return Err(RioError::parse(
                "طابق يتطلب حالة واحدة على الأقل",
                line,
                col,
            ));
        }
        Ok(Self::desugar_match(subject, cases, line, col))
    }

    /// نمط (بدائل `|`): نمط مغلق واحد أو أكثر.
    fn parse_pattern(&mut self) -> RioResult<Vec<Pat>> {
        let mut alts = self.parse_closed_pattern()?;
        while matches!(self.peek().kind, TokenKind::Pipe) {
            self.advance(); // | (البند 162)
            alts.extend(self.parse_closed_pattern()?);
        }
        Ok(alts)
    }

    /// نمط مغلق واحد — وقد يتوسع إلى بدائل عدة
    /// (المجموعة شفافة؛ والمركب جداء ديكارتي).
    fn parse_closed_pattern(&mut self) -> RioResult<Vec<Pat>> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Number(n) => {
                let n = *n;
                self.advance();
                Ok(vec![Pat::Lit(Expr::new(
                    ExprKind::Number(n),
                    tok.line,
                    tok.col,
                ))])
            }
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                Ok(vec![Pat::Lit(Expr::new(
                    ExprKind::Str(s),
                    tok.line,
                    tok.col,
                ))])
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(vec![Pat::Lit(Expr::new(
                    ExprKind::Bool(true),
                    tok.line,
                    tok.col,
                ))])
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(vec![Pat::Lit(Expr::new(
                    ExprKind::Bool(false),
                    tok.line,
                    tok.col,
                ))])
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.advance();
                Ok(vec![Pat::Lit(Expr::new(
                    ExprKind::Nil,
                    tok.line,
                    tok.col,
                ))])
            }
            TokenKind::Minus => {
                self.advance(); // - (حرفي سالب، البند 153)
                let n = self.peek().clone();
                match &n.kind {
                    TokenKind::Number(v) => {
                        let v = *v;
                        self.advance();
                        Ok(vec![Pat::Lit(Expr::new(
                            ExprKind::Number(-v),
                            tok.line,
                            tok.col,
                        ))])
                    }
                    _ => Err(RioError::parse(
                        "النمط الحرفي السالب يتطلب عددا بعد -",
                        n.line,
                        n.col,
                    )),
                }
            }
            TokenKind::Ident(_) => {
                let (first, line, col) = self.expect_ident("اسم نمط")?;
                if first == "_" {
                    // البند 155: شامل.
                    return Ok(vec![Pat::Wildcard]);
                }
                let mut expr = Expr::new(ExprKind::Ident(first.clone()), line, col);
                let mut last = first;
                while matches!(self.peek().kind, TokenKind::Dot) {
                    self.advance(); // .
                    let (mname, _, _) = self.expect_member_name()?;
                    expr = Expr::new(
                        ExprKind::Dot {
                            target: Box::new(expr),
                            name: mname.clone(),
                        },
                        line,
                        col,
                    );
                    last = mname;
                }
                if matches!(self.peek().kind, TokenKind::LParen) {
                    // البند 160: نمط صنف.
                    return self.parse_class_pattern(last, line, col);
                }
                if matches!(expr.kind, ExprKind::Ident(_)) {
                    // البند 154: التقاط.
                    return Ok(vec![Pat::Capture(last)]);
                }
                // البند 156: قيمة مؤهلة بنقطة.
                Ok(vec![Pat::Value(expr)])
            }
            TokenKind::LBracket => {
                // البند 158: متسلسل [...] (نجمة اختيارية).
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBracket) {
                    self.advance();
                    return Ok(vec![Pat::Seq {
                        elems: Vec::new(),
                        star: None,
                    }]);
                }
                let (elem_alts, star) = self.parse_seq_first()?;
                self.parse_seq_rest(
                    elem_alts,
                    star,
                    &TokenKind::RBracket,
                    "قوس إغلاق ] في النمط المتسلسل",
                )
            }
            TokenKind::LParen => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "نمط فارغ () غير صالح",
                        t.line,
                        t.col,
                    ));
                }
                let (elem_alts, star) = self.parse_seq_first()?;
                if matches!(self.peek().kind, TokenKind::Comma) {
                    // البند 158: متسلسل مفتوح (نمط، نمط).
                    self.parse_seq_rest(
                        elem_alts,
                        star,
                        &TokenKind::RParen,
                        "قوس إغلاق ) في النمط المتسلسل",
                    )
                } else {
                    if star.is_some() {
                        let t = self.peek().clone();
                        return Err(RioError::parse(
                            "النجمة * وحدها ليست نمطا؛ ضعها في متسلسلة",
                            t.line,
                            t.col,
                        ));
                    }
                    // البند 157: مجموعة (نمط) — شفافة.
                    self.expect(
                        &TokenKind::RParen,
                        "قوس إغلاق ) في نمط المجموعة",
                    )?;
                    Ok(elem_alts.into_iter().next().unwrap())
                }
            }
            TokenKind::LBrace => self.parse_map_pattern(),
            _ => Err(RioError::parse(
                "نمط غير صالح بعد حال",
                tok.line,
                tok.col,
            )),
        }
    }

    /// أول عنصر متسلسلة: نجمة `*اسم`/`*_` أو نمط.
    /// يعيد (بدائل العنصر — فارغة للنجمة — والنجمة).
    fn parse_seq_first(
        &mut self,
    ) -> RioResult<(Vec<Vec<Pat>>, Option<(usize, Option<String>, usize, usize)>)> {
        if matches!(self.peek().kind, TokenKind::Star) {
            let (capture, nline, ncol) = self.parse_star_capture()?;
            return Ok((Vec::new(), Some((0, capture, nline, ncol))));
        }
        Ok((vec![self.parse_pattern()?], None))
    }

    /// `*اسم` أو `*_` بعد تقدّم `*`: يعيد (التقاط أو
    /// لا شيء، سطر، عمود).
    fn parse_star_capture(&mut self) -> RioResult<(Option<String>, usize, usize)> {
        self.advance(); // *
        let (name, nline, ncol) = match &self.peek().kind {
            TokenKind::Ident(n) => {
                let n = n.clone();
                let t = self.advance();
                (n, t.line, t.col)
            }
            _ => {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "بعد * في النمط المتسلسل يأتي اسم التقاط أو _",
                    t.line,
                    t.col,
                ));
            }
        };
        Ok((if name == "_" { None } else { Some(name) }, nline, ncol))
    }

    /// باقي عناصر متسلسلة بعد أول عنصر (محلل): فواصل
    /// وعناصر ونجمة واحدة اختيارية حتى رمز الإغلاق،
    /// ثم الجداء الديكارتي لبدائل العناصر.
    fn parse_seq_rest(
        &mut self,
        mut elem_alts: Vec<Vec<Pat>>,
        mut star: Option<(usize, Option<String>, usize, usize)>,
        close: &TokenKind,
        close_what: &str,
    ) -> RioResult<Vec<Pat>> {
        loop {
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance(); // ،
                if self.peek_kind_eq(close) {
                    self.advance();
                    break;
                }
            } else if self.peek_kind_eq(close) {
                self.advance();
                break;
            } else {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    format!("توقع ، أو {} في النمط المتسلسل", close_what),
                    t.line,
                    t.col,
                ));
            }
            if matches!(self.peek().kind, TokenKind::Star) {
                if star.is_some() {
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "نجمة واحدة فقط في النمط المتسلسل",
                        t.line,
                        t.col,
                    ));
                }
                let (capture, nline, ncol) = self.parse_star_capture()?;
                star = Some((elem_alts.len(), capture, nline, ncol));
                continue;
            }
            elem_alts.push(self.parse_pattern()?);
        }
        Ok(Self::product_seq(elem_alts, star))
    }

    /// البند 159: نمط فهرس `{مفتاح: نمط، ...}` مع
    /// باقي `**اسم` اختياري أخيرا.
    fn parse_map_pattern(&mut self) -> RioResult<Vec<Pat>> {
        self.advance(); // {
        if matches!(self.peek().kind, TokenKind::RBrace) {
            self.advance();
            return Ok(vec![Pat::Map {
                entries: Vec::new(),
                rest: None,
            }]);
        }
        let mut entry_alts: Vec<(Expr, Vec<Pat>)> = Vec::new();
        let mut rest: Option<String> = None;
        loop {
            if matches!(self.peek().kind, TokenKind::Star)
                && matches!(self.peek_at(1).kind, TokenKind::Star)
            {
                if rest.is_some() {
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "باقي ** واحد فقط في نمط الفهرس",
                        t.line,
                        t.col,
                    ));
                }
                self.advance(); // *
                self.advance(); // *
                let (name, _, _) = self.expect_ident("اسم التقاط بعد ** في نمط الفهرس")?;
                rest = Some(name);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RBrace) {
                        self.advance();
                        break;
                    }
                    let t = self.peek().clone();
                    return Err(RioError::parse(
                        "باقي ** يجب أن يكون آخر نمط الفهرس",
                        t.line,
                        t.col,
                    ));
                }
                self.expect(&TokenKind::RBrace, "قوس إغلاق } في نمط الفهرس")?;
                break;
            }
            let key = self.parse_map_key()?;
            self.expect(&TokenKind::Colon, "نقطتين : بعد مفتاح نمط الفهرس")?;
            let sub = self.parse_pattern()?;
            entry_alts.push((key, sub));
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    self.advance();
                    break;
                }
                continue;
            }
            self.expect(&TokenKind::RBrace, "قوس إغلاق } في نمط الفهرس")?;
            break;
        }
        Ok(Self::product_map(entry_alts, rest))
    }

    /// مفتاح نمط فهرس: نص حرفي، أو قيمة مسماة بنقطة
    /// (تُقيَّم وقت المطابقة ويجب أن تكون نصا — مفاتيح
    /// قواميس نيفا نصوص).
    fn parse_map_key(&mut self) -> RioResult<Expr> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::new(ExprKind::Str(s), tok.line, tok.col))
            }
            TokenKind::Ident(_) => {
                let (first, line, col) = self.expect_ident("مفتاح نمط الفهرس")?;
                if !matches!(self.peek().kind, TokenKind::Dot) {
                    return Err(RioError::parse(
                        "مفتاح الفهرس يجب أن يكون نصا أو قيمة مسماة بنقطة",
                        tok.line,
                        tok.col,
                    ));
                }
                let mut expr = Expr::new(ExprKind::Ident(first), line, col);
                while matches!(self.peek().kind, TokenKind::Dot) {
                    self.advance(); // .
                    let (mname, _, _) = self.expect_member_name()?;
                    expr = Expr::new(
                        ExprKind::Dot {
                            target: Box::new(expr),
                            name: mname,
                        },
                        line,
                        col,
                    );
                }
                Ok(expr)
            }
            _ => Err(RioError::parse(
                "مفتاح الفهرس يجب أن يكون نصا أو قيمة مسماة بنقطة",
                tok.line,
                tok.col,
            )),
        }
    }

    /// البند 160: نمط صنف `اسم(حقل=نمط، ...)` —
    /// المفتاحية فقط؛ والموضعي مرفوض نحوا.
    fn parse_class_pattern(
        &mut self,
        name: String,
        line: usize,
        col: usize,
    ) -> RioResult<Vec<Pat>> {
        self.advance(); // (
        if matches!(self.peek().kind, TokenKind::RParen) {
            self.advance();
            return Ok(vec![Pat::Class {
                name,
                fields: Vec::new(),
            }]);
        }
        let mut field_alts: Vec<(String, Vec<Pat>)> = Vec::new();
        loop {
            let t = self.peek().clone();
            let fname = match &t.kind {
                TokenKind::Ident(n) => n.clone(),
                _ => {
                    return Err(RioError::parse(
                        "توقع حقل=نمط داخل نمط الصنف",
                        t.line,
                        t.col,
                    ))
                }
            };
            if !matches!(self.peek_at(1).kind, TokenKind::Eq) {
                return Err(RioError::parse(
                    "نمط الصنف الموضعي غير مدعوم؛ استخدم حقل=نمط",
                    t.line,
                    t.col,
                ));
            }
            self.advance(); // اسم الحقل
            self.advance(); // =
            let sub = self.parse_pattern()?;
            field_alts.push((fname, sub));
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    break;
                }
                continue;
            }
            self.expect(&TokenKind::RParen, "قوس إغلاق ) في نمط الصنف")?;
            break;
        }
        Ok(Self::product_class(name, field_alts))
    }

    /// الجداء الديكارتي لبدائل عناصر متسلسلة.
    fn product_seq(
        elem_alts: Vec<Vec<Pat>>,
        star: Option<(usize, Option<String>, usize, usize)>,
    ) -> Vec<Pat> {
        let mut out = vec![Pat::Seq {
            elems: Vec::new(),
            star: star.clone(),
        }];
        for alts in &elem_alts {
            let mut next = Vec::new();
            for base in &out {
                for alt in alts {
                    if let Pat::Seq { elems, star: s } = base {
                        let mut elems = elems.clone();
                        elems.push(alt.clone());
                        next.push(Pat::Seq {
                            elems,
                            star: s.clone(),
                        });
                    }
                }
            }
            out = next;
        }
        out
    }

    /// الجداء الديكارتي لبدائل قيم نمط فهرس.
    fn product_map(entry_alts: Vec<(Expr, Vec<Pat>)>, rest: Option<String>) -> Vec<Pat> {
        let mut out = vec![Pat::Map {
            entries: Vec::new(),
            rest: rest.clone(),
        }];
        for (key, alts) in &entry_alts {
            let mut next = Vec::new();
            for base in &out {
                for alt in alts {
                    if let Pat::Map { entries, rest: r } = base {
                        let mut entries = entries.clone();
                        entries.push((key.clone(), alt.clone()));
                        next.push(Pat::Map {
                            entries,
                            rest: r.clone(),
                        });
                    }
                }
            }
            out = next;
        }
        out
    }

    /// الجداء الديكارتي لبدائل حقول نمط صنف.
    fn product_class(name: String, field_alts: Vec<(String, Vec<Pat>)>) -> Vec<Pat> {
        let mut out = vec![Pat::Class {
            name: name.clone(),
            fields: Vec::new(),
        }];
        for (field, alts) in &field_alts {
            let mut next = Vec::new();
            for base in &out {
                for alt in alts {
                    if let Pat::Class { fields, .. } = base {
                        let mut fields = fields.clone();
                        fields.push((field.clone(), alt.clone()));
                        next.push(Pat::Class {
                            name: name.clone(),
                            fields,
                        });
                    }
                }
            }
            out = next;
        }
        out
    }

    /// تفكيك `طابق` إلى جمل قائمة: مؤقت الموضوع مرة
    /// واحدة، وعلم `تم`، ثم لكل حالة `اذا (ليس تم) و
    /// اختبار:` فربط الالتقاطات، فـ`اذا حراسة:` فجسم
    /// الحالة فـ`تم = صحيح`. أول مطابقة تفوز لأن كل
    /// حالة لاحقة ترى العلم؛ ورفض الحراسة يترك العلم
    /// خطأ فتُجرَّب الحالة التالية. بلا أي عقد جديدة.
    fn desugar_match(
        subject: Expr,
        cases: Vec<(Pat, Option<Expr>, Vec<Stmt>)>,
        line: usize,
        col: usize,
    ) -> Vec<Stmt> {
        let subj_name = format!("__طابق_قيمة_{}_{}", line, col);
        let done_name = format!("__طابق_تم_{}_{}", line, col);
        let subj = Expr::new(ExprKind::Ident(subj_name.clone()), line, col);
        let mut out = vec![
            Stmt::new(
                StmtKind::Assign {
                    target: AssignTarget::Var(subj_name),
                    value: subject,
                    annotation: None,
                    append: false,
                },
                line,
                col,
            ),
            Stmt::new(
                StmtKind::Assign {
                    target: AssignTarget::Var(done_name.clone()),
                    value: Expr::new(ExprKind::Bool(false), line, col),
                    annotation: None,
                    append: false,
                },
                line,
                col,
            ),
        ];
        for (pat, guard, body) in cases {
            let (test, binds) = Self::pattern_test(&subj, &pat, line, col);
            let not_done = Expr::new(
                ExprKind::Unary {
                    op: UnOp::Not,
                    expr: Box::new(Expr::new(
                        ExprKind::Ident(done_name.clone()),
                        line,
                        col,
                    )),
                },
                line,
                col,
            );
            let cond = Expr::new(
                ExprKind::Binary {
                    left: Box::new(not_done),
                    op: BinOp::And,
                    right: Box::new(test),
                },
                line,
                col,
            );
            let mut then_body: Vec<Stmt> = binds;
            let inner_cond =
                guard.unwrap_or_else(|| Expr::new(ExprKind::Bool(true), line, col));
            let mut inner_body = body;
            inner_body.push(Stmt::new(
                StmtKind::Assign {
                    target: AssignTarget::Var(done_name.clone()),
                    value: Expr::new(ExprKind::Bool(true), line, col),
                    annotation: None,
                    append: false,
                },
                line,
                col,
            ));
            then_body.push(Stmt::new(
                StmtKind::If {
                    cond: inner_cond,
                    then_body: inner_body,
                    elifs: Vec::new(),
                    else_body: None,
                },
                line,
                col,
            ));
            out.push(Stmt::new(
                StmtKind::If {
                    cond,
                    then_body,
                    elifs: Vec::new(),
                    else_body: None,
                },
                line,
                col,
            ));
        }
        out
    }

    /// اختبار نمط على موضوع + جمل ربط التقاطاته:
    /// يعيد (تعبير منطقي، جمل إسناد). الاختبار كلي
    /// لا يخطئ أبدا على قيمة لا تطابق: كل فهرسة أو
    /// وصول لحقل يسبقه حارس نوع أو طول أو مفتاح أو
    /// صنف، و`و` دائرة قصر في المحركات الثلاثة،
    /// فيُحرس التقييم بالترتيب نفسه.
    fn pattern_test(subj: &Expr, pat: &Pat, line: usize, col: usize) -> (Expr, Vec<Stmt>) {
        match pat {
            Pat::Lit(lit) => (
                Self::mk_bin(subj.clone(), BinOp::Eq, lit.clone(), line, col),
                Vec::new(),
            ),
            Pat::Capture(name) => (
                Expr::new(ExprKind::Bool(true), line, col),
                vec![Self::mk_assign_var(name, subj.clone(), line, col)],
            ),
            Pat::Wildcard => (Expr::new(ExprKind::Bool(true), line, col), Vec::new()),
            Pat::Value(value) => (
                Self::mk_bin(subj.clone(), BinOp::Eq, value.clone(), line, col),
                Vec::new(),
            ),
            Pat::Seq { elems, star } => {
                let n = elems.len();
                let mut test = Self::mk_bin(
                    Self::mk_global_call("نوع", vec![subj.clone()], line, col),
                    BinOp::Eq,
                    Self::mk_str("قائمة", line, col),
                    line,
                    col,
                );
                let len_call = Self::mk_global_call("طول", vec![subj.clone()], line, col);
                let len_test = match star {
                    None => Self::mk_bin(
                        len_call,
                        BinOp::Eq,
                        Self::mk_num(n as f64, line, col),
                        line,
                        col,
                    ),
                    Some(_) => Self::mk_bin(
                        len_call,
                        BinOp::GtEq,
                        Self::mk_num(n as f64, line, col),
                        line,
                        col,
                    ),
                };
                test = Self::mk_and(test, len_test, line, col);
                let mut binds = Vec::new();
                let before = star.as_ref().map(|s| s.0).unwrap_or(n);
                for (i, elem) in elems.iter().enumerate() {
                    let index = if i < before {
                        Self::mk_num(i as f64, line, col)
                    } else {
                        // عنصر بعد النجمة: فهرسه من النهاية.
                        Self::mk_bin(
                            Self::mk_global_call("طول", vec![subj.clone()], line, col),
                            BinOp::Sub,
                            Self::mk_num((n - i) as f64, line, col),
                            line,
                            col,
                        )
                    };
                    let elem_subj = Self::mk_index(subj.clone(), index, line, col);
                    let (t, b) = Self::pattern_test(&elem_subj, elem, line, col);
                    test = Self::mk_and(test, t, line, col);
                    binds.extend(b);
                }
                if let Some((_, Some(capture), sline, scol)) = star {
                    // باقي النجمة يُبنى بحلقة `لكل` على
                    // `مدى` — بلا استيراد وبلا مساعد جديد:
                    // باقي = [] ثم أضف عناصر [قبل، طول-بعد).
                    let idx_name = format!("__طابق_فهرس_{}_{}", sline, scol);
                    let after = n - before;
                    binds.push(Self::mk_assign_var(
                        capture,
                        Expr::new(ExprKind::List(Vec::new()), line, col),
                        line,
                        col,
                    ));
                    let start = Self::mk_num(before as f64, line, col);
                    let end = Self::mk_bin(
                        Self::mk_global_call("طول", vec![subj.clone()], line, col),
                        BinOp::Sub,
                        Self::mk_num(after as f64, line, col),
                        line,
                        col,
                    );
                    let iterable =
                        Self::mk_global_call("مدى", vec![start, end], line, col);
                    let append = Stmt::new(
                        StmtKind::ExprStmt {
                            expr: Expr::new(
                                ExprKind::Call {
                                    callee: Box::new(Self::mk_dot(
                                        Expr::new(
                                            ExprKind::Ident(capture.clone()),
                                            line,
                                            col,
                                        ),
                                        "أضف",
                                        line,
                                        col,
                                    )),
                                    args: vec![Self::mk_index(
                                        subj.clone(),
                                        Self::mk_ident(&idx_name, line, col),
                                        line,
                                        col,
                                    )],
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
                                line,
                                col,
                            ),
                        },
                        line,
                        col,
                    );
                    binds.push(Stmt::new(
                        StmtKind::For {
                            var: idx_name,
                            iterable,
                            body: vec![append],
                        },
                        line,
                        col,
                    ));
                }
                (test, binds)
            }
            Pat::Map { entries, rest } => {
                let mut test = Self::mk_bin(
                    Self::mk_global_call("نوع", vec![subj.clone()], line, col),
                    BinOp::Eq,
                    Self::mk_str("قاموس", line, col),
                    line,
                    col,
                );
                let mut binds = Vec::new();
                let mut keys: Vec<Expr> = Vec::new();
                for (key, sub) in entries {
                    keys.push(key.clone());
                    // المفتاح المنقّط قد يقيَّم إلى غير نص —
                    // ومفاتيح القواميس نصوص — فيُحرس `في`
                    // بفحص نوعه أولا، وإلا رفع الاختبار
                    // خطأ بدل فشل المطابقة. الحرفي نص دائما.
                    if !matches!(key.kind, ExprKind::Str(_)) {
                        test = Self::mk_and(
                            test,
                            Self::mk_bin(
                                Self::mk_global_call("نوع", vec![key.clone()], line, col),
                                BinOp::Eq,
                                Self::mk_str("نص", line, col),
                                line,
                                col,
                            ),
                            line,
                            col,
                        );
                    }
                    test = Self::mk_and(
                        test,
                        Self::mk_bin(key.clone(), BinOp::In, subj.clone(), line, col),
                        line,
                        col,
                    );
                    let (t, b) = Self::pattern_test(
                        &Self::mk_index(subj.clone(), key.clone(), line, col),
                        sub,
                        line,
                        col,
                    );
                    test = Self::mk_and(test, t, line, col);
                    binds.extend(b);
                }
                if let Some(name) = rest {
                    if name != "_" {
                        binds.push(Self::mk_assign_var(
                            name,
                            Self::lib_call(
                                "مطابقة",
                                "بدون_مفاتيح",
                                vec![
                                    subj.clone(),
                                    Expr::new(ExprKind::List(keys), line, col),
                                ],
                                line,
                                col,
                            ),
                            line,
                            col,
                        ));
                    }
                }
                (test, binds)
            }
            Pat::Class { name, fields } => {
                let mut test = Self::mk_bin(
                    Self::lib_call(
                        "مطابقة",
                        "اسم_الصنف",
                        vec![subj.clone()],
                        line,
                        col,
                    ),
                    BinOp::Eq,
                    Self::mk_str(name, line, col),
                    line,
                    col,
                );
                let mut binds = Vec::new();
                for (field, sub) in fields {
                    let (t, b) = Self::pattern_test(
                        &Self::mk_dot(subj.clone(), field, line, col),
                        sub,
                        line,
                        col,
                    );
                    test = Self::mk_and(test, t, line, col);
                    binds.extend(b);
                }
                (test, binds)
            }
        }
    }

    /// بنّاؤو عقد التفكيك (سطر/عمود `طابق` كلها، كما
    /// في تفكيك الدفعة 2).
    fn mk_ident(name: &str, line: usize, col: usize) -> Expr {
        Expr::new(ExprKind::Ident(name.to_string()), line, col)
    }

    fn mk_str(s: &str, line: usize, col: usize) -> Expr {
        Expr::new(ExprKind::Str(s.to_string()), line, col)
    }

    fn mk_num(n: f64, line: usize, col: usize) -> Expr {
        Expr::new(ExprKind::Number(n), line, col)
    }

    fn mk_bin(left: Expr, op: BinOp, right: Expr, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
            line,
            col,
        )
    }

    fn mk_and(left: Expr, right: Expr, line: usize, col: usize) -> Expr {
        Self::mk_bin(left, BinOp::And, right, line, col)
    }

    fn mk_index(target: Expr, index: Expr, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Index {
                target: Box::new(target),
                index: Box::new(index),
            },
            line,
            col,
        )
    }

    fn mk_dot(target: Expr, name: &str, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Dot {
                target: Box::new(target),
                name: name.to_string(),
            },
            line,
            col,
        )
    }

    fn mk_global_call(name: &str, args: Vec<Expr>, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Call {
                callee: Box::new(Self::mk_ident(name, line, col)),
                args,
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
            line,
            col,
        )
    }

    /// `لاجل` بعد عنصر حاوية؟ الكلمة ليست محجوزة
    /// في المحلل اللغوي — معرّف عادٍ — فتُقارَن بعد
    /// التطبيع (تجريد التشكيل وتوحيد الهمزات)، كما
    /// تُحلّ مفاتيح المكتبة؛ ولا موضع صالح آخر يتبع
    /// فيه معرّفٌ عنصرا داخل `[...]` أو `{...}`،
    /// فلا غموض ولا كلمة مفتاحية جديدة.
    fn peek_is_lajl(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(name) => crate::lexer::normalize_cli_word(name) == "لاجل",
            _ => false,
        }
    }

    /// ب6: هل الرمز الحالي `من` (كلمة `من` المحجوزة،
    /// أو معرّف مطبَّع إليها) — لـ`ولد من مولد`؟
    fn peek_is_min(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Keyword(Keyword::From) => true,
            TokenKind::Ident(name) => crate::lexer::normalize_cli_word(name) == "من",
            _ => false,
        }
    }

    /// ب6: هل تبدأ الجملة الحالية بـ`ولد` كجملة مولد؟
    /// نعم حين يتبع `ولد`: `من`، أو فاصل/نهاية (ولد
    /// عار)، أو بداية قيمة في السطر نفسه. لا حين يتبعها
    /// `(` (استدعاء `ولد(...)` — كلمة المرور أو دالة
    /// مستخدمة)، أو `=`/`+=`/`:` (إسناد إلى متغير اسمه
    /// ولد)، أو `[`/`.`/`,` (فهرسة أو عضو أو عنصر).
    fn peek_is_yield_start(&self) -> bool {
        let is_walad = match &self.peek().kind {
            TokenKind::Ident(name) => crate::lexer::normalize_cli_word(name) == "ولد",
            _ => false,
        };
        if !is_walad {
            return false;
        }
        let tok = self.peek().clone();
        let next = self.peek_at(1).clone();
        if self.peek_is_min_at(1) {
            return true;
        }
        if matches!(next.kind, TokenKind::Newline | TokenKind::Semicolon | TokenKind::Dedent | TokenKind::Eof) {
            return true;
        }
        bare_call_follows(&next, tok.line)
    }

    /// ب6: `من` في الموضع `pos + offset` (للفحص قبل
    /// الاستهلاك في peek_is_yield_start).
    fn peek_is_min_at(&self, offset: usize) -> bool {
        match &self.peek_at(offset).kind {
            TokenKind::Keyword(Keyword::From) => true,
            TokenKind::Ident(name) => crate::lexer::normalize_cli_word(name) == "من",
            _ => false,
        }
    }

    /// ب6: جملة `ولد` — تفكيك إلى استدعاء داخلي:
    /// `ولد قيمة` → `__ولد(قيمة)`، و`ولد من مولد` →
    /// `__ولد_من(مولد)`، و`ولد` عارية → `__ولد(لاشيء)`.
    /// القيمة تعبير كامل (جشع كالباقي: `ولد ٥ + ١`
    /// يولّد ٦)؛ والمحلل الدلالي يرفضها خارج دالة.
    fn parse_yield_stmt(&mut self) -> RioResult<Vec<Stmt>> {
        let tok = self.advance(); // ولد
        let expr = if self.peek_is_min() {
            self.advance(); // من
            let value = self.parse_expr()?;
            Self::mk_global_call(crate::ast::YIELD_FROM_CALL, vec![value], tok.line, tok.col)
        } else {
            let value = if self.is_separator()
                || matches!(self.peek().kind, TokenKind::Dedent | TokenKind::Eof)
            {
                Expr::new(ExprKind::Nil, tok.line, tok.col)
            } else {
                self.parse_expr()?
            };
            Self::mk_global_call(crate::ast::YIELD_CALL, vec![value], tok.line, tok.col)
        };
        Ok(vec![Stmt::new(StmtKind::ExprStmt { expr }, tok.line, tok.col)])
    }

    /// ب6: التعبير التوليدي `(تعبير لاجل س في قابل اذا
    /// شرط)` → `__مولد_تعبير(دالة التعبير، دالة الشرط
    /// أو لاشيء، القابل)` — كسول: كل تقييم ينشئ مولدا
    /// جديدا؛ والتفكيك كالفهم الضمني (البنود 141–143)
    /// بلا عقدة جديدة.
    fn parse_genexpr(&mut self, elem: Expr, line: usize, col: usize) -> RioResult<Expr> {
        let (var, iterable, cond) = self.parse_comprehension_tail()?;
        self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
        Ok(Self::mk_global_call(
            crate::ast::GENEXPR_CALL,
            vec![
                Self::mk_lambda_one(&var, elem, line, col),
                Self::mk_cond_lambda(cond, &var, line, col),
                iterable,
            ],
            line,
            col,
        ))
    }

    /// ذيل الفهم بعد `لاجل`: متغير واحد، ثم `في`،
    /// ثم القابل، ثم شرط اختياري بعد `اذا` — يعيد
    /// (المتغير، القابل، الشرط أو لا شيء).
    fn parse_comprehension_tail(
        &mut self,
    ) -> RioResult<(String, Expr, Option<Expr>)> {
        self.advance(); // لاجل
        let (var, _, _) = self.expect_ident("اسم متغير بعد لاجل")?;
        self.expect_keyword(Keyword::In, "في")?;
        let iterable = self.parse_expr()?;
        let cond = if matches!(self.peek().kind, TokenKind::Keyword(Keyword::If)) {
            self.advance(); // اذا
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok((var, iterable, cond))
    }

    /// دالة مجهولة بمعامل واحد لجسم فهم.
    fn mk_lambda_one(var: &str, body: Expr, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Lambda {
                params: vec![var.to_string()],
                param_types: vec![None],
                body: Box::new(body),
            },
            line,
            col,
        )
    }

    /// شرط الفهم دالة مجهولة، أو `لاشيء` بلا شرط.
    fn mk_cond_lambda(cond: Option<Expr>, var: &str, line: usize, col: usize) -> Expr {
        match cond {
            Some(c) => Self::mk_lambda_one(var, c, line, col),
            None => Expr::new(ExprKind::Nil, line, col),
        }
    }

    /// البند 141: `[تعبير لاجل س في قابل اذا شرط]`
    /// → `افهم_قائمة(دالة_مجهولة(س): تعبير،
    /// دالة_مجهولة(س): شرط أو لاشيء، قابل)` — بلا
    /// عقد جديدة؛ والمدمجة تنفذ (حاوية فارغة +
    /// حلقة + إلحاق) في المحركات الثلاثة.
    fn parse_list_comprehension(
        &mut self,
        elem: Expr,
        line: usize,
        col: usize,
    ) -> RioResult<Expr> {
        let (var, iterable, cond) = self.parse_comprehension_tail()?;
        self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
        Ok(Self::mk_global_call(
            "افهم_قائمة",
            vec![
                Self::mk_lambda_one(&var, elem, line, col),
                Self::mk_cond_lambda(cond, &var, line, col),
                iterable,
            ],
            line,
            col,
        ))
    }

    /// البند 142: `{تعبير لاجل س في قابل اذا شرط}`
    /// → `افهم_تشكيلة(...)` بالشكل نفسه.
    fn parse_set_comprehension(
        &mut self,
        elem: Expr,
        line: usize,
        col: usize,
    ) -> RioResult<Expr> {
        let (var, iterable, cond) = self.parse_comprehension_tail()?;
        self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
        Ok(Self::mk_global_call(
            "افهم_تشكيلة",
            vec![
                Self::mk_lambda_one(&var, elem, line, col),
                Self::mk_cond_lambda(cond, &var, line, col),
                iterable,
            ],
            line,
            col,
        ))
    }

    /// البند 143: `{مفتاح: قيمة لاجل س في قابل اذا
    /// شرط}` → `افهم_فهرس(دالة_مجهولة(س): [مفتاح،
    /// قيمة]، دالة_مجهولة(س): شرط أو لاشيء، قابل)`.
    fn parse_dict_comprehension(
        &mut self,
        key: Expr,
        value: Expr,
        line: usize,
        col: usize,
    ) -> RioResult<Expr> {
        let (var, iterable, cond) = self.parse_comprehension_tail()?;
        self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
        let pair = Expr::new(ExprKind::List(vec![key, value]), line, col);
        Ok(Self::mk_global_call(
            "افهم_فهرس",
            vec![
                Self::mk_lambda_one(&var, pair, line, col),
                Self::mk_cond_lambda(cond, &var, line, col),
                iterable,
            ],
            line,
            col,
        ))
    }

    fn mk_assign_var(name: &str, value: Expr, line: usize, col: usize) -> Stmt {
        Stmt::new(
            StmtKind::Assign {
                target: AssignTarget::Var(name.to_string()),
                value,
                annotation: None,
                append: false,
            },
            line,
            col,
        )
    }

    /// هل السطر الحالي تفكيك `س، ص = حاوية`؟ مسح
    /// رموز حتى نهاية السطر: فاصلة بعمق صفر قبل أول
    /// `=` بعمق صفر تعني تفكيكا. الفواصل داخل
    /// الأقواس لا تُحسَب، و`=` داخل تعبير (مثل
    /// `س = دالة(١، ٢)`) لا تُسبَق بفاصلة — فلا
    /// برنامج صالح اليوم يُعاد توجيهه خطأ.
    fn looks_like_unpack(&self) -> bool {
        let mut depth = 0i32;
        let mut saw_comma = false;
        let mut i = 0usize;
        loop {
            match &self.peek_at(i).kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth -= 1,
                TokenKind::Eq if depth == 0 => return saw_comma,
                TokenKind::Comma if depth == 0 => saw_comma = true,
                TokenKind::Newline | TokenKind::Semicolon | TokenKind::Dedent | TokenKind::Eof => {
                    return false
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// البند 69 — إسناد متعدد/مفكك: `س، ص = حاوية`
    /// يُفكَّك إلى تحقق ثم إسنادات مفهرسة قائمة، بلا
    /// عقد جديدة. مسطح فقط: الأهداف أسماء أو فهرسة
    /// أو نقطة؛ واليمين يُقيَّم مرة واحدة في مؤقت
    /// `__تفكيك` (فلا تُستدعى دالة اليمين ثلاث مرات).
    /// كل الجمل المولّدة بسطر الجملة الأصلية وعمودها
    /// فيتطابق موضع الخطأ في المفسر والآلة والأصلي.
    fn parse_unpack(&mut self) -> RioResult<Vec<Stmt>> {
        let first = self.parse_expr()?;
        let mut targets = vec![first];
        while matches!(self.peek().kind, TokenKind::Comma) {
            self.advance(); // ،
            targets.push(self.parse_expr()?);
        }
        if !matches!(self.peek().kind, TokenKind::Eq) {
            let t = self.peek().clone();
            return Err(RioError::parse(
                "التفكيك يتطلب = وقيمة حاوية بعد الأهداف، مثل س، ص = قائمة",
                t.line,
                t.col,
            ));
        }
        self.advance(); // =
        let rhs = self.parse_expr()?;
        let mut assign_targets = Vec::new();
        for e in &targets {
            match &e.kind {
                ExprKind::Ident(name) => assign_targets.push(AssignTarget::Var(name.clone())),
                ExprKind::Index { target, index } => assign_targets.push(AssignTarget::Index {
                    target: target.clone(),
                    index: index.clone(),
                }),
                ExprKind::Dot { target, name } => assign_targets.push(AssignTarget::Dot {
                    target: target.clone(),
                    name: name.clone(),
                }),
                _ => {
                    return Err(RioError::parse(
                        "هدف التفكيك يجب أن يكون اسما أو فهرسة أو نقطة، لا تعبيرا",
                        e.line,
                        e.col,
                    ));
                }
            }
        }
        let (line, col) = (targets[0].line, targets[0].col);
        let n = assign_targets.len();
        let temp = "__تفكيك".to_string();
        let mut stmts = vec![Stmt::new(
            StmtKind::Assign {
                target: AssignTarget::Var(temp.clone()),
                value: rhs,
                annotation: None,
                append: false,
            },
            line,
            col,
        )];
        let check = Self::validate_call(
            "فكك",
            vec![
                Expr::new(ExprKind::Ident(temp.clone()), line, col),
                Expr::new(ExprKind::Number(n as f64), line, col),
            ],
            line,
            col,
        );
        stmts.push(Stmt::new(StmtKind::ExprStmt { expr: check }, line, col));
        for (i, t) in assign_targets.into_iter().enumerate() {
            let value = Expr::new(
                ExprKind::Index {
                    target: Box::new(Expr::new(ExprKind::Ident(temp.clone()), line, col)),
                    index: Box::new(Expr::new(ExprKind::Number(i as f64), line, col)),
                },
                line,
                col,
            );
            stmts.push(Stmt::new(
                StmtKind::Assign { target: t, value, annotation: None, append: false },
                line,
                col,
            ));
        }
        Ok(stmts)
    }

    fn parse_assign_or_expr(&mut self) -> RioResult<Stmt> {
        let expr = self.parse_expr()?;
        // Phase 1A Step 3: annotated assignment, Var only:
        // س: عدد = ٥
        if matches!(&expr.kind, ExprKind::Ident(_))
            && matches!(self.peek().kind, TokenKind::Colon)
        {
            self.advance(); // :
            let annotation = self.parse_type_name()?;
            if !matches!(self.peek().kind, TokenKind::Eq) {
                let t = self.peek().clone();
                return Err(RioError::parse(
                    "التعليق النوعي يتطلب تعيينا، مثل س: عدد = ٥",
                    t.line,
                    t.col,
                ));
            }
            self.advance(); // =
            let name = match &expr.kind {
                ExprKind::Ident(n) => n.clone(),
                _ => unreachable!(),
            };
            let value = self.parse_expr()?;
            return Ok(Stmt::new(
                StmtKind::Assign {
                    target: AssignTarget::Var(name),
                    value,
                    annotation: Some(annotation),
                    append: false,
                },
                expr.line,
                expr.col,
            ));
        }
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // `س += قيمة` — إلحاق لا استبدال، لهدف
        // متغير وحده (الفهرسة والنقطة مرفوضتان
        // تحليلا، بلا تقييم مزدوج للفهرس).
        if matches!(self.peek().kind, TokenKind::PlusEq) {
            self.advance(); // +=
            let name = match &expr.kind {
                ExprKind::Ident(n) => n.clone(),
                _ => {
                    return Err(RioError::parse(
                        "الإلحاق يدعم المتغير وحده، مثل س += قيمة",
                        expr.line,
                        expr.col,
                    ));
                }
            };
            let value = self.parse_expr()?;
            return Ok(Stmt::new(
                StmtKind::Assign {
                    target: AssignTarget::Var(name),
                    value,
                    annotation: None,
                    append: true,
                },
                expr.line,
                expr.col,
            ));
        }
// == مولّد-دفعة2-إسناد-مركب: بداية ==
        // الإسنادات المركبة (البنود 71–72): تفكيك إلى إسناد
        // عادي بقيمة مبنية من عقد قائمة أو استدعاء مكتبي،
        // بلا عقد AST جديدة — فيعمل المفسر والآلة والأصلي
        // كما هي بلا تغيير. الهدف متغير وحده (كـ+=).
        if let Some((bin_op, lib_fn)) = match &self.peek().kind {
            TokenKind::MinusEq => Some((Some(BinOp::Sub), None)),
            TokenKind::StarEq => Some((Some(BinOp::Mul), None)),
            TokenKind::SlashEq => Some((Some(BinOp::Div), None)),
            TokenKind::PercentEq => Some((Some(BinOp::Mod), None)),
            TokenKind::SlashSlashEq => Some((None, Some("قسمة_أرضية"))),
            TokenKind::AmpEq => Some((None, Some("بت_و"))),
            TokenKind::PipeEq => Some((None, Some("بت_او"))),
            TokenKind::CaretEq => Some((None, Some("قوة"))),
            TokenKind::CaretCaretEq => Some((None, Some("بت_عكس_حصري"))),
            TokenKind::ShlEq => Some((None, Some("بت_ازاحة_يسار"))),
            TokenKind::ShrEq => Some((None, Some("بت_ازاحة_يمين"))),
            _ => None,
        } {
            self.advance();
            let name = match &expr.kind {
                ExprKind::Ident(n) => n.clone(),
                _ => {
                    return Err(RioError::parse(
                        "الإسناد المركب يدعم المتغير وحده، مثل س -= قيمة",
                        expr.line,
                        expr.col,
                    ));
                }
            };
            let rhs = self.parse_expr()?;
            let (line, col) = (expr.line, expr.col);
            let value = match (bin_op, lib_fn) {
                (Some(op), _) => Expr::new(
                    ExprKind::Binary {
                        left: Box::new(Expr::new(ExprKind::Ident(name.clone()), line, col)),
                        op,
                        right: Box::new(rhs),
                    },
                    line,
                    col,
                ),
                (_, Some(f)) => Self::math_call(
                    f,
                    Expr::new(ExprKind::Ident(name.clone()), line, col),
                    rhs,
                    line,
                    col,
                ),
                _ => unreachable!(),
            };
            return Ok(Stmt::new(
                StmtKind::Assign { target: AssignTarget::Var(name), value, annotation: None, append: false },
                line,
                col,
            ));
        }
// == مولّد-دفعة2-إسناد-مركب: نهاية ==
        if matches!(self.peek().kind, TokenKind::Eq) {
            // The reversed-assignment prototype (syntax RTL)
            // is gated behind the `prototype-rtl` feature
            // (off by default): no build includes it unless
            // explicitly enabled, pending the owner's verdict.
            // The prototype applies only when the left side is
            // not a valid target (not a name, index, or dot),
            // and a bare name followed by end-of-statement comes
            // after `=` — so «س = ص» stays a normal assignment
            // (target س, value ص). It builds the same Assign
            // node, so interpreter, VM, and REPL need no change.
            let expr_is_target = matches!(
                &expr.kind,
                ExprKind::Ident(_) | ExprKind::Index { .. } | ExprKind::Dot { .. }
            );
            if self.reversed_assignment_applies(expr_is_target) {
                self.advance(); // =
                let name_tok = self.advance();
                let name = match &name_tok.kind {
                    TokenKind::Ident(n) => n.clone(),
                    _ => unreachable!(),
                };
                let (line, col) = (expr.line, expr.col);
                return Ok(Stmt::new(
                    StmtKind::Assign {
                        target: AssignTarget::Var(name),
                        value: expr,
                        annotation: None,
                        append: false,
                    },
                    line,
                    col,
                ));
            }
            self.advance();
            let target = match &expr.kind {
                ExprKind::Ident(name) => AssignTarget::Var(name.clone()),
                ExprKind::Index { target, index } => AssignTarget::Index {
                    target: target.clone(),
                    index: index.clone(),
                },
                ExprKind::Dot { target, name } => AssignTarget::Dot {
                    target: target.clone(),
                    name: name.clone(),
                },
                _ => {
                    return Err(RioError::parse(
                        "هدف التعيين غير صالح",
                        expr.line,
                        expr.col,
                    ));
                }
            };
            let value = self.parse_expr()?;
            return Ok(Stmt::new(
                StmtKind::Assign { target, value, annotation: None, append: false },
                expr.line,
                expr.col,
            ));
        }
        let (line, col) = (expr.line, expr.col);
        Ok(Stmt::new(StmtKind::ExprStmt { expr }, line, col))
    }

    // ---- expressions, precedence climbing ----
    fn parse_expr(&mut self) -> RioResult<Expr> {
        let expr = self.parse_or()?;
        // البند 73 (`:=` — الاجتماع 83 §98): إسناد
        // تعبيري — أضعف ربطا من `أو`، يميني التجميع.
        // الطرف الأيسر اسم وحده؛ القيمة تعبير كامل.
        if matches!(self.peek().kind, TokenKind::ColonEq) {
            let (line, col) = (expr.line, expr.col);
            let name = match &expr.kind {
                ExprKind::Ident(n) => n.clone(),
                _ => {
                    return Err(RioError::parse(
                        "الإسناد:= يدعم الاسم وحده، مثل س:= قيمة",
                        line,
                        col,
                    ));
                }
            };
            self.advance();
            let value = self.parse_expr()?;
            return Ok(Expr::new(
                ExprKind::AssignExpr { name, value: Box::new(value) },
                line,
                col,
            ));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_and()?;
        while matches!(self.peek().kind, TokenKind::Keyword(Keyword::Or)) {
            let tok = self.advance();
            let right = self.parse_and()?;
            left = Expr::new(
                ExprKind::Binary { left: Box::new(left), op: BinOp::Or, right: Box::new(right) },
                tok.line,
                tok.col,
            );
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_not()?;
        while matches!(self.peek().kind, TokenKind::Keyword(Keyword::And)) {
            let tok = self.advance();
            let right = self.parse_not()?;
            left = Expr::new(
                ExprKind::Binary { left: Box::new(left), op: BinOp::And, right: Box::new(right) },
                tok.line,
                tok.col,
            );
        }
        Ok(left)
    }

    /// `ليس` binds looser than comparison, like Python's `not`:
    /// `ليس س == ٩٩` means `ليس (س == ٩٩)`, not `(ليس س) == ٩٩`.
    fn parse_not(&mut self) -> RioResult<Expr> {
        let tok = self.peek().clone();
        if matches!(tok.kind, TokenKind::Keyword(Keyword::Not)) {
            self.advance();
            let operand = self.parse_not()?;
            return Ok(Expr::new(
                ExprKind::Unary { op: UnOp::Not, expr: Box::new(operand) },
                tok.line,
                tok.col,
            ));
        }
        self.parse_comparison()
    }

    /// بناء استدعاء `وحدة.اسم(معاملات...)` كعقدة AST —
    /// تعميم لمساعد الرياضيات المولّد، للبنود اليدوية في
    /// الدفعة 2 (117 و118 و133 و163–165): هوية وشريحة
    /// ونص تنفيذي. تفكيك إلى عقد قائمة واستدعاءات مكتبية
    /// فحسب، بلا عقد جديدة، فيعمل المفسر والآلة والأصلي
    /// كما هي. يتطلب البرنامج `استورد وحدة` كما يتطلبه
    /// استدعاء الدالة مباشرة.
    fn lib_call(module: &str, name: &str, args: Vec<Expr>, line: usize, col: usize) -> Expr {
        Expr::new(
            ExprKind::Call {
                callee: Box::new(Expr::new(
                    ExprKind::Dot {
                        target: Box::new(Expr::new(ExprKind::Ident(module.to_string()), line, col)),
                        name: name.to_string(),
                    },
                    line,
                    col,
                )),
                args,
                kwargs: Vec::new(),
            type_args: Vec::new(),
            },
            line,
            col,
        )
    }

    /// استدعاء `دوال.هوية(يسار، يمين)` — فحص الهوية
    /// (البندان 117 و118).
    fn identity_call(left: Expr, right: Expr, line: usize, col: usize) -> Expr {
        Self::lib_call("دوال", "هوية", vec![left, right], line, col)
    }

    fn parse_comparison(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_bitor()?;
        loop {
            // كلمات المقارنة (اجتماع 19): بأطول مطابقة أولا،
            // وتنتج نفس عقد AST الرموز.
            if let Some((op, count)) = self.word_operator() {
                let tok = self.peek().clone();
                for _ in 0..count {
                    self.advance();
                }
                let right = self.parse_bitor()?;
                left = Expr::new(
                    ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            // `هل` / `هل ليس` (البندان 117 و118، الدفعة 2 من
            // الـ579): عاملا مقارنة في مستوى المقارنة نفسه
            // كـ`في` و`ليس في`. `هل` معرّف لا كلمة محجوزة،
            // فيُقرأ عاملا هنا فقط (بعد طرف أيسر مكتمل)؛
            // و`هل` وحدها في موضع قيمة تبقى متغيرا عاديا.
            // `هل ليس` نفي للفحص: `ليس (هل)`.
            if matches!(&self.peek().kind, TokenKind::Ident(n) if n == "هل") {
                let tok = self.advance();
                let negated = matches!(self.peek().kind, TokenKind::Keyword(Keyword::Not));
                if negated {
                    self.advance(); // ليس
                }
                let right = self.parse_bitor()?;
                let check = Self::identity_call(left, right, tok.line, tok.col);
                left = if negated {
                    Expr::new(
                        ExprKind::Unary { op: UnOp::Not, expr: Box::new(check) },
                        tok.line,
                        tok.col,
                    )
                } else {
                    check
                };
                continue;
            }
            // ليس في  -> NotIn (two tokens)
            if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Not))
                && matches!(self.peek_at(1).kind, TokenKind::Keyword(Keyword::In))
            {
                let tok = self.advance();
                self.advance(); // في
                let right = self.parse_bitor()?;
                left = Expr::new(
                    ExprKind::Binary { left: Box::new(left), op: BinOp::NotIn, right: Box::new(right) },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            let op = match &self.peek().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::NotEq,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::GtEq => BinOp::GtEq,
                TokenKind::Keyword(Keyword::In) => BinOp::In,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_bitor()?;
            left = Expr::new(
                ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) },
                tok.line,
                tok.col,
            );
        }
        Ok(left)
    }

    /// كلمات المقارنة (اجتماع 19): تعيد العامل وعدد الرموز
    /// التي يستهلكها، أو لا شيء. بأطول مطابقة أولا، لأن
    /// «أكبر من أو يساوي» تحوي «أو» (كلمة محجوزة قائمة)
    /// فيجب أن تُقرأ عاملا واحدا لا «أكبر من» + «أو» +
    /// «يساوي». والرموز تبقى صالحة بجانب الكلمات.
    fn word_operator(&self) -> Option<(BinOp, usize)> {
        let k = |i: usize| &self.peek_at(i).kind;
        // 4 رموز: أكبر/أصغر من أو يساوي.
        if matches!(k(0), TokenKind::Keyword(Keyword::Greater))
            && matches!(k(1), TokenKind::Keyword(Keyword::From))
            && matches!(k(2), TokenKind::Keyword(Keyword::Or))
            && matches!(k(3), TokenKind::Keyword(Keyword::Equals))
        {
            return Some((BinOp::GtEq, 4));
        }
        if matches!(k(0), TokenKind::Keyword(Keyword::Smaller))
            && matches!(k(1), TokenKind::Keyword(Keyword::From))
            && matches!(k(2), TokenKind::Keyword(Keyword::Or))
            && matches!(k(3), TokenKind::Keyword(Keyword::Equals))
        {
            return Some((BinOp::LtEq, 4));
        }
        // رمزان: أكبر من، أصغر من، لا يساوي.
        if matches!(k(0), TokenKind::Keyword(Keyword::Greater))
            && matches!(k(1), TokenKind::Keyword(Keyword::From))
        {
            return Some((BinOp::Gt, 2));
        }
        if matches!(k(0), TokenKind::Keyword(Keyword::Smaller))
            && matches!(k(1), TokenKind::Keyword(Keyword::From))
        {
            return Some((BinOp::Lt, 2));
        }
        if matches!(k(0), TokenKind::Keyword(Keyword::No))
            && matches!(k(1), TokenKind::Keyword(Keyword::Equals))
        {
            return Some((BinOp::NotEq, 2));
        }
        // رمز واحد: يساوي.
        if matches!(k(0), TokenKind::Keyword(Keyword::Equals)) {
            return Some((BinOp::Eq, 1));
        }
        None
    }

// == مولّد-دفعة2-مستويات-البت: بداية ==
    fn parse_bitor(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_bitxor()?;
        loop {
            let f = match &self.peek().kind {
                TokenKind::Pipe => "بت_او",
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_bitxor()?;
            left = Self::math_call(f, left, right, tok.line, tok.col);
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_bitand()?;
        loop {
            let f = match &self.peek().kind {
                TokenKind::CaretCaret => "بت_عكس_حصري",
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_bitand()?;
            left = Self::math_call(f, left, right, tok.line, tok.col);
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            let f = match &self.peek().kind {
                TokenKind::Amp => "بت_و",
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_shift()?;
            left = Self::math_call(f, left, right, tok.line, tok.col);
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let f = match &self.peek().kind {
                TokenKind::Shl => "بت_ازاحة_يسار",
                TokenKind::Shr => "بت_ازاحة_يمين",
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_additive()?;
            left = Self::math_call(f, left, right, tok.line, tok.col);
        }
        Ok(left)
    }
// == مولّد-دفعة2-مستويات-البت: نهاية ==
    fn parse_additive(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match &self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::new(
                ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) },
                tok.line,
                tok.col,
            );
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
// == مولّد-دفعة2-قسمة-أرضية: بداية ==
            // القسمة الأرضية `//` (البند 123): في مستوى الضرب
            // (حد في ألف)، تُفكَّك إلى استدعاء مكتبي.
            if matches!(self.peek().kind, TokenKind::SlashSlash) {
                let tok = self.advance();
                let right = self.parse_unary()?;
                left = Self::math_call("قسمة_أرضية", left, right, tok.line, tok.col);
                continue;
            }
// == مولّد-دفعة2-قسمة-أرضية: نهاية ==
            let op = match &self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            let tok = self.advance();
            let right = self.parse_unary()?;
            left = Expr::new(
                ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) },
                tok.line,
                tok.col,
            );
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> RioResult<Expr> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::new(
                    ExprKind::Unary { op: UnOp::Neg, expr: Box::new(expr) },
                    tok.line,
                    tok.col,
                ))
            }
            TokenKind::Keyword(Keyword::Not) => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::new(
                    ExprKind::Unary { op: UnOp::Not, expr: Box::new(expr) },
                    tok.line,
                    tok.col,
                ))
            }
// == مولّد-دفعة2-عكس-بت: بداية ==
            TokenKind::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Self::math_call1("عكس_بت", expr, tok.line, tok.col))
            }
// == مولّد-دفعة2-عكس-بت: نهاية ==
// == مولّد-دفعة2-قوة: بداية ==
            _ => {
                // القوة `^` (أس في ألف: جذر "^" معامل) — يمينية
                // التجميع، وأعلى من الضرب؛ والطرف الأيمن معامل
                // كامل فيقبل إشارة أحادية وقوة أخرى.
                let base = self.parse_postfix()?;
                if matches!(self.peek().kind, TokenKind::Caret) {
                    let tok = self.advance();
                    let exp = self.parse_unary()?;
                    return Ok(Self::math_call("قوة", base, exp, tok.line, tok.col));
                }
                Ok(base)
            }
// == مولّد-دفعة2-قوة: نهاية ==
        }
    }

    /// بناء استدعاء `قوائم.شريحة(هدف، بداية، نهاية، خطوة)`
    /// — الحدود الغائبة `لاشيء` (البند 133، الدفعة 2 من
    /// الـ579).
    fn slice_call(
        target: Expr,
        start: Option<Expr>,
        end: Option<Expr>,
        step: Option<Expr>,
        line: usize,
        col: usize,
    ) -> Expr {
        let nil = Expr::new(ExprKind::Nil, line, col);
        let s = start.unwrap_or_else(|| nil.clone());
        let e = end.unwrap_or_else(|| nil.clone());
        let st = step.unwrap_or(nil);
        Self::lib_call("قوائم", "شريحة", vec![target, s, e, st], line, col)
    }

    /// ذيل الشريحة بعد أول نقطتين: `[نهاية]? [":" [خطوة]?]?`
    /// ثم `]` — يعيد (النهاية، الخطوة).
    fn parse_slice_tail(&mut self) -> RioResult<(Option<Expr>, Option<Expr>)> {
        let mut end = None;
        if !matches!(self.peek().kind, TokenKind::Colon | TokenKind::RBracket) {
            end = Some(self.parse_expr()?);
        }
        let mut step = None;
        if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            if !matches!(self.peek().kind, TokenKind::RBracket) {
                step = Some(self.parse_expr()?);
            }
        }
        self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
        Ok((end, step))
    }

    fn parse_postfix(&mut self) -> RioResult<Expr> {
        let mut expr = self.parse_primary()?;
        // البنود 145–147: تخصيص عام معلَّق (`اسم[نوع]`)
        // يستهلكه قوس الاستدعاء التالي وحده.
        let mut pending_type_args: Vec<String> = Vec::new();
        loop {
            if matches!(self.peek().kind, TokenKind::LParen) {
                let tok = self.advance();
                let mut args = Vec::new();
                let mut kwargs = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RParen) {
                    loop {
                        if self.kwarg_follows().is_some() {
                            let (k, v) = self.parse_kwarg(&kwargs)?;
                            kwargs.push((k, v));
                        } else {
                            if !kwargs.is_empty() {
                                let t = self.peek().clone();
                                return Err(RioError::parse(
                                    "معامل موضعي بعد معامل مفتاحي",
                                    t.line,
                                    t.col,
                                ));
                            }
                            args.push(self.parse_expr()?);
                        }
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                            if matches!(self.peek().kind, TokenKind::RParen) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
                let type_args = std::mem::take(&mut pending_type_args);
                expr = Expr::new(
                    ExprKind::Call { callee: Box::new(expr), args, kwargs, type_args },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            if matches!(self.peek().kind, TokenKind::LBracket) {
                // اجتماع 32، السؤال الأول: قوسان فارغان
                // `[]` بعد تعبير مكتمل ليسا فهرسة أبدا
                // (الفهرسة تحتاج فهرسا) — بل قائمة حرفية
                // تبدأ تعبيرا جديدا (معاملا مجردا تاليا
                // في استدعاء منقّط، أو عبارة تالية).
                // نتركهما دون استهلاك فيخرج التعبير كما هو.
                if matches!(self.peek_at(1).kind, TokenKind::RBracket) {
                    break;
                }
                // البنود 145–147: `معرّف[نوع](...)` تخصيص
                // عام لا فهرسة — والمحاولة تخمينية: عند
                // الفشل تُستعاد المواضع ويسلك مسار الفهرسة.
                if matches!(expr.kind, ExprKind::Ident(_)) {
                    if let Some(args) = self.try_parse_type_apply() {
                        pending_type_args = args;
                        continue;
                    }
                }
                let tok = self.advance();
                // القواطع (البند 133، الدفعة 2 من الـ579):
                // `[أ:ب]` و`[أ:ب:ج]` — البداية والنهاية والخطوة
                // كلها اختيارية. `س[أ]` وحده فهرسة كما كان.
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance(); // :
                    let (end, step) = self.parse_slice_tail()?;
                    expr = Self::slice_call(expr, None, end, step, tok.line, tok.col);
                    continue;
                }
                let index = self.parse_expr()?;
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance(); // :
                    let (end, step) = self.parse_slice_tail()?;
                    expr = Self::slice_call(expr, Some(index), end, step, tok.line, tok.col);
                    continue;
                }
                self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
                expr = Expr::new(
                    ExprKind::Index { target: Box::new(expr), index: Box::new(index) },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            if matches!(self.peek().kind, TokenKind::Dot) {
                let tok = self.advance();
                let (name, name_line, _) = self.expect_member_name()?;
                expr = Expr::new(
                    ExprKind::Dot { target: Box::new(expr), name },
                    tok.line,
                    tok.col,
                );
                // Meeting 27 (§42.1): bare call — `شبكة.اخدم
                // ٨٤٤٣ شهادة مفتاح معالج` (args separated by
                // spaces, no commas). After a dotted name, value
                // expressions on the SAME line are call
                // arguments: each arg is a full expression, and
                // the list ends at a newline, a separator, or a
                // token that cannot start a value. The
                // parenthesized form `شبكة.اخدم(٨٤٤٣، ...)`
                // keeps working as before. Operators still
                // continue the current expression (`س.طول + ١`
                // is an addition, not a call), and a dotted
                // name alone on its line stays a plain value.
                // اجتماع 32، السؤال الأول: قائمة فارغة `[]`
                // على السطر نفسه معامل مجرد أيضا — تبدأ
                // قائمة حرفية، لا فهرسة للمعامل السابق.
                if bare_call_follows(self.peek(), name_line)
                    || empty_list_follows(self, name_line)
                {
                    let mut args = Vec::new();
                    let mut kwargs = Vec::new();
                    while bare_call_follows(self.peek(), name_line)
                        || empty_list_follows(self, name_line)
                    {
                        if self.kwarg_follows().is_some() {
                            let (k, v) = self.parse_kwarg(&kwargs)?;
                            kwargs.push((k, v));
                        } else {
                            if !kwargs.is_empty() {
                                let t = self.peek().clone();
                                return Err(RioError::parse(
                                    "معامل موضعي بعد معامل مفتاحي",
                                    t.line,
                                    t.col,
                                ));
                            }
                            args.push(self.parse_expr()?);
                        }
                    }
                    expr = Expr::new(
                        ExprKind::Call { callee: Box::new(expr), args, kwargs, type_args: std::mem::take(&mut pending_type_args) },
                        tok.line,
                        tok.col,
                    );
                }
                continue;
            }
            break;
        }
        Ok(expr)
    }

    /// استدعاء `نص.نسق(قالب، [قيمة])` — تنسيق قيمة واحدة
    /// في النص التنفيذي (البنود 163–165).
    fn format_one(template: String, value: Expr, line: usize, col: usize) -> Expr {
        Self::lib_call(
            "نص",
            "نسق",
            vec![
                Expr::new(ExprKind::Str(template), line, col),
                Expr::new(ExprKind::List(vec![value]), line, col),
            ],
            line,
            col,
        )
    }

    /// تحليل تعبير حقل نص تنفيذي بإعادة تمثيل مصدره —
    /// المحلل نفسه، فيتحمل الحقل أي تعبير (بما فيه نص
    /// تنفيذي متداخل أو قاموس).
    fn parse_fstr_field(&self, src: &str, line: usize, col: usize) -> RioResult<Expr> {
        let toks = tokenize(src).map_err(|e| {
            RioError::parse(format!("تعبير غير صالح داخل النص التنفيذي: {}", e), line, col)
        })?;
        let mut sub = Parser::new(toks);
        let expr = sub.parse_expr().map_err(|e| {
            RioError::parse(format!("تعبير غير صالح داخل النص التنفيذي: {}", e), line, col)
        })?;
        if !matches!(sub.peek().kind, TokenKind::Newline | TokenKind::Eof) {
            return Err(RioError::parse(
                "تعبير غير صالح داخل النص التنفيذي: بقية غير متوقعة بعد التعبير",
                line,
                col,
            ));
        }
        Ok(expr)
    }

    /// تفكيك نص تنفيذي خام إلى سلسلة `+` (البنود 163–165):
    /// الأجزاء الحرفية نصوص، وكل حقل استدعاء `نص.نسق`
    /// (أو `نص.تمثيل` للتحويل)؛ والتصحيح `=` يسبق القيمة
    /// بلفظ التعبير. الصيغة تنسق القيمة العددية أولا، ثم
    /// التحويل يحول الناتج.
    fn parse_fstr(&mut self, raw: &str, line: usize, col: usize) -> RioResult<Expr> {
        let parts = split_fstr(raw, line, col)?;
        let mut expr: Option<Expr> = None;
        for part in parts {
            let piece = match part {
                FStrPart::Lit(s) => Expr::new(ExprKind::Str(s), line, col),
                FStrPart::Field { src, debug, conv, spec } => {
                    let value = self.parse_fstr_field(&src, line, col)?;
                    let mut v = value;
                    let has_spec = spec.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                    if has_spec {
                        let tpl = format!("{{0:{}}}", spec.unwrap());
                        v = Self::format_one(tpl, v, line, col);
                    }
                    match conv.as_deref() {
                        Some("نص") => {
                            v = Self::format_one("{0}".to_string(), v, line, col);
                        }
                        Some("تمثيل") => {
                            v = Self::lib_call("نص", "تمثيل", vec![v], line, col);
                        }
                        Some(_) => unreachable!(),
                        None => {
                            if !has_spec {
                                v = Self::format_one("{0}".to_string(), v, line, col);
                            }
                        }
                    }
                    if debug {
                        let label = Expr::new(ExprKind::Str(format!("{}=", src)), line, col);
                        Expr::new(
                            ExprKind::Binary { left: Box::new(label), op: BinOp::Add, right: Box::new(v) },
                            line,
                            col,
                        )
                    } else {
                        v
                    }
                }
            };
            expr = Some(match expr {
                None => piece,
                Some(acc) => Expr::new(
                    ExprKind::Binary { left: Box::new(acc), op: BinOp::Add, right: Box::new(piece) },
                    line,
                    col,
                ),
            });
        }
        Ok(expr.unwrap_or_else(|| Expr::new(ExprKind::Str(String::new()), line, col)))
    }

    fn parse_primary(&mut self) -> RioResult<Expr> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::new(ExprKind::Number(n), tok.line, tok.col))
            }
            // ج6: الصيغ الحرفية تُفكَّك إلى مدمجات مسطحة
            // (بلا عقدة AST جديدة وبلا لمس لـcgen.rs):
            // `123x` إلى عدد_كبير("123")، و`1r2` إلى
            // كسر("1"، "2")، و`0j1` و`4ت` إلى مركب(0، 1).
            TokenKind::BigInt(digits) => {
                let digits = digits.clone();
                self.advance();
                Ok(Self::mk_global_call(
                    "عدد_كبير",
                    vec![Expr::new(ExprKind::Str(digits), tok.line, tok.col)],
                    tok.line,
                    tok.col,
                ))
            }
            TokenKind::Rational(num, den) => {
                let (num, den) = (num.clone(), den.clone());
                self.advance();
                Ok(Self::mk_global_call(
                    // حرفي لا حرفي الوحدة: `كسر` مدمج مسطح
                    // ووحدة معا — فبعد `استورد كسر` يظلل
                    // الاسم الوحدةَ المدمجَ فينكسر الحرفي؛
                    // أما `كسر_حرفي` فلا وحدة بهذا الاسم أبدا.
                    "كسر_حرفي",
                    vec![
                        Expr::new(ExprKind::Str(num), tok.line, tok.col),
                        Expr::new(ExprKind::Str(den), tok.line, tok.col),
                    ],
                    tok.line,
                    tok.col,
                ))
            }
            TokenKind::Complex(re, im) => {
                let (re, im) = (*re, *im);
                self.advance();
                Ok(Self::mk_global_call(
                    // كالسابق: `مركب` وحدة أيضا بعد الاستيراد.
                    "مركب_حرفي",
                    vec![
                        Expr::new(ExprKind::Number(re), tok.line, tok.col),
                        Expr::new(ExprKind::Number(im), tok.line, tok.col),
                    ],
                    tok.line,
                    tok.col,
                ))
            }
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::new(ExprKind::Str(s), tok.line, tok.col))
            }
            TokenKind::FStr(raw) => {
                // النص التنفيذي (البنود 163–165): يفكك إلى
                // سلسلة `+` مع استدعاءات `نص.نسق` / `نص.تمثيل`.
                let raw = raw.clone();
                self.advance();
                self.parse_fstr(&raw, tok.line, tok.col)
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::new(ExprKind::Bool(true), tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::new(ExprKind::Bool(false), tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.advance();
                Ok(Expr::new(ExprKind::Nil, tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Lambda) => {
                // Meeting 13: دالة_مجهولة(س): س + 1 — single
                // expression, implicit return. The body is a full
                // expression, so it stops naturally at `,` `)` `]`
                // or newline; nesting works (currying).
                self.advance(); // دالة_مجهولة
                let pl = self.parse_param_list(false, "اللامدا")?;
                let ParamList { params, param_types, .. } = pl;
                self.expect_colon()?;
                let body = self.parse_expr()?;
                Ok(Expr::new(
                    ExprKind::Lambda { params, param_types, body: Box::new(body) },
                    tok.line,
                    tok.col,
                ))
            }
            TokenKind::Keyword(Keyword::Create) => {
                self.advance(); // أنشئ
                let (class, _, _) = self.expect_ident("اسم فئة بعد أنشئ")?;
                // البنود 145–147: تخصيص عام اختياري
                // (`أنشئ صندوق[عدد](...)`) قبل قوس المعاملات.
                let mut type_args: Vec<String> = Vec::new();
                if matches!(self.peek().kind, TokenKind::LBracket) {
                    self.advance(); // [
                    loop {
                        type_args.push(self.parse_type_expr_raw()?.display());
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                    self.expect(&TokenKind::RBracket, "قوس إغلاق ] بعد معاملات النوع")?;
                }
                let mut args = Vec::new();
                let mut kwargs = Vec::new();
                if matches!(self.peek().kind, TokenKind::LParen) {
                    self.advance();
                    if !matches!(self.peek().kind, TokenKind::RParen) {
                        loop {
                            if self.kwarg_follows().is_some() {
                                let (k, v) = self.parse_kwarg(&kwargs)?;
                                kwargs.push((k, v));
                            } else {
                                if !kwargs.is_empty() {
                                    let t = self.peek().clone();
                                    return Err(RioError::parse(
                                        "معامل موضعي بعد معامل مفتاحي",
                                        t.line,
                                        t.col,
                                    ));
                                }
                                args.push(self.parse_expr()?);
                            }
                            if matches!(self.peek().kind, TokenKind::Comma) {
                                self.advance();
                                if matches!(self.peek().kind, TokenKind::RParen) {
                                    break;
                                }
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
                }
                Ok(Expr::new(ExprKind::Create { class, args, kwargs, type_args }, tok.line, tok.col))
            }
            TokenKind::Ident(name) => {
                // ب6: `ولد` في موضع تعبير — `ولد من مولد`
                // تفويض، و`ولد قيمة` توليد (القيمة تعبير
                // كامل جشع)؛ أما `ولد(` فاستدعاء عادي،
                // و`ولد` وحدها (يتبعها فاصل أو `،` أو `)` أو
                // عامل) فالمعرّف نفسه — فلا يُسرَق اسم
                // متغير أو دالة في أي برنامج صالح.
                if crate::lexer::normalize_cli_word(name) == "ولد" {
                    // peek() ما زال على `ولد` نفسها هنا —
                    // فالتالية peek_at(1).
                    let next = self.peek_at(1).clone();
                    if !matches!(next.kind, TokenKind::LParen) {
                        if matches!(next.kind, TokenKind::Keyword(Keyword::From))
                            || matches!(&next.kind, TokenKind::Ident(n) if crate::lexer::normalize_cli_word(n) == "من")
                        {
                            self.advance(); // ولد
                            self.advance(); // من
                            let value = self.parse_expr()?;
                            return Ok(Self::mk_global_call(
                                crate::ast::YIELD_FROM_CALL,
                                vec![value],
                                tok.line,
                                tok.col,
                            ));
                        }
                        if bare_call_follows(&next, tok.line) {
                            self.advance(); // ولد
                            let value = self.parse_expr()?;
                            return Ok(Self::mk_global_call(
                                crate::ast::YIELD_CALL,
                                vec![value],
                                tok.line,
                                tok.col,
                            ));
                        }
                    }
                }
                let name = name.clone();
                self.advance();
                Ok(Expr::new(ExprKind::Ident(name), tok.line, tok.col))
            }
            TokenKind::LParen => {
                // البند 138 (الدفعة 3 من الـ579): مترابطة
                // `(أ، ب)` — تُفكَّك إلى استدعاء
                // `مترابطة(أ، ب)` بلا عقدة جديدة؛ و`()`
                // مترابطة فارغة، و`(س،)` مترابطة وحيدة،
                // و`(س)` تجميع كما كانت — والأنماط داخل
                // `طابق` لها محللها الخاص فلا تمسها.
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    self.advance();
                    return Ok(Self::mk_global_call(
                        "مترابطة",
                        Vec::new(),
                        tok.line,
                        tok.col,
                    ));
                }
                let first = self.parse_expr()?;
                // ب6: التعبير التوليدي `(تعبير لاجل س في
                // قابل اذا شرط)` — قبل فحص الفاصلة
                // (المترابطة)، فـ`لاجل` بعد العنصر تعبير
                // توليدي لا مترابطة.
                if self.peek_is_lajl() {
                    return self.parse_genexpr(first, tok.line, tok.col);
                }
                if matches!(self.peek().kind, TokenKind::Comma) {
                    let mut elems = vec![first];
                    loop {
                        self.advance(); // ،
                        if matches!(self.peek().kind, TokenKind::RParen) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                        if !matches!(self.peek().kind, TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
                    return Ok(Self::mk_global_call(
                        "مترابطة", elems, tok.line, tok.col,
                    ));
                }
                self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
                Ok(first)
            }
            TokenKind::LBracket => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBracket) {
                    let t = self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
                    return Ok(Expr::new(ExprKind::List(Vec::new()), tok.line, t.col));
                }
                let first = self.parse_expr()?;
                // البند 141: فهم المصفوفة `[تعبير لاجل
                // س في قابل اذا شرط]` — يُفكَّك في
                // parse_list_comprehension إلى استدعاء
                // `افهم_قائمة` بدالتين مجهولتين.
                if self.peek_is_lajl() {
                    return self.parse_list_comprehension(first, tok.line, tok.col);
                }
                let mut elems = vec![first];
                loop {
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RBracket) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                        continue;
                    }
                    break;
                }
                let t = self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
                Ok(Expr::new(ExprKind::List(elems), tok.line, t.col))
            }
            TokenKind::LBrace => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    let t = self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
                    return Ok(Expr::new(ExprKind::Dict(Vec::new()), tok.line, t.col));
                }
                // البندان 139 و142 و143: التشكيلة `{أ،
                // ب}` مقابل الفهرس `{م:ق}` — النقطتان
                // تميزان الفهرس؛ والفهم بـ`لاجل` بعد
                // العنصر (تشكيلة) أو بعد القيمة (فهرس).
                let first = self.parse_expr()?;
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance(); // :
                    let value = self.parse_expr()?;
                    if self.peek_is_lajl() {
                        return self.parse_dict_comprehension(
                            first, value, tok.line, tok.col,
                        );
                    }
                    // فهرس حرفي: المفتاح نص حرفي كما كان.
                    let key = match &first.kind {
                        ExprKind::Str(s) => s.clone(),
                        _ => {
                            let t = self.peek().clone();
                            return Err(RioError::parse(
                                "مفتاح القاموس يجب أن يكون نصا",
                                t.line,
                                t.col,
                            ));
                        }
                    };
                    let mut entries = vec![(key, value)];
                    loop {
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                            if matches!(self.peek().kind, TokenKind::RBrace) {
                                break;
                            }
                            let k = match &self.peek().kind {
                                TokenKind::Str(s) => {
                                    let s = s.clone();
                                    self.advance();
                                    s
                                }
                                _ => {
                                    let t = self.peek().clone();
                                    return Err(RioError::parse(
                                        "مفتاح القاموس يجب أن يكون نصا",
                                        t.line,
                                        t.col,
                                    ));
                                }
                            };
                            self.expect(&TokenKind::Colon, "نقطتين : بعد مفتاح القاموس")?;
                            let v = self.parse_expr()?;
                            entries.push((k, v));
                            continue;
                        }
                        break;
                    }
                    let t = self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
                    return Ok(Expr::new(ExprKind::Dict(entries), tok.line, t.col));
                }
                if self.peek_is_lajl() {
                    return self.parse_set_comprehension(first, tok.line, tok.col);
                }
                let mut elems = vec![first];
                loop {
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RBrace) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                        continue;
                    }
                    break;
                }
                let _t = self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
                Ok(Self::mk_global_call("تشكيلة", elems, tok.line, tok.col))
            }
            _ => Err(RioError::parse("تعبير غير متوقع", tok.line, tok.col)),
        }
    }
}

/// جزء من نص تنفيذي بعد تقسيمه: نص حرفي، أو حقل
/// `{تعبير}` مع التصحيح والتحويل والتنسيق (البنود
/// 163–165، الدفعة 2 من الـ579).
enum FStrPart {
    Lit(String),
    Field {
        /// مصدر التعبير كما كُتب (بلا التصحيح والتحويل والتنسيق).
        src: String,
        /// `=` للتصحيح: يطبع `التعبير=` قبل القيمة.
        debug: bool,
        /// التحويل بعد `!`: `نص` أو `تمثيل` أو لا شيء.
        conv: Option<String>,
        /// الصيغة بعد `:` أو لا شيء.
        spec: Option<String>,
    },
}

/// تقسيم محتوى نص تنفيذي خام إلى أجزاء حرفية وحقول.
/// `{{` و`}}` هروب للقوسين في النص الحرفي؛ والشرطة
/// المائلة هروب نص عادي كما في المحلل اللغوي؛ وداخل
/// الحقل تُتجاهل الأقواس داخل النصوص ويُحسب التداخل،
/// فيتحمل الحقل قاموسا ونصا تنفيذيا متداخلا.
fn split_fstr(raw: &str, line: usize, col: usize) -> RioResult<Vec<FStrPart>> {
    let chars: Vec<char> = raw.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                lit.push('{');
                i += 2;
                continue;
            }
            let mut field = String::new();
            let mut depth = 1usize;
            let mut j = i + 1;
            let mut in_str: Option<char> = None;
            while j < chars.len() {
                let ch = chars[j];
                if let Some(q) = in_str {
                    field.push(ch);
                    if ch == '\\' && j + 1 < chars.len() {
                        field.push(chars[j + 1]);
                        j += 2;
                        continue;
                    }
                    if ch == q {
                        in_str = None;
                    }
                    j += 1;
                    continue;
                }
                if ch == '"' || ch == '\'' {
                    in_str = Some(ch);
                    field.push(ch);
                    j += 1;
                    continue;
                }
                if ch == '{' {
                    depth += 1;
                    field.push(ch);
                    j += 1;
                    continue;
                }
                if ch == '}' {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                    field.push(ch);
                    j += 1;
                    continue;
                }
                field.push(ch);
                j += 1;
            }
            if depth != 0 {
                return Err(RioError::parse("قوس { غير مغلق في النص التنفيذي", line, col));
            }
            if in_str.is_some() {
                return Err(RioError::parse("نص غير مغلق داخل النص التنفيذي", line, col));
            }
            if !lit.is_empty() {
                parts.push(FStrPart::Lit(std::mem::take(&mut lit)));
            }
            parts.push(parse_fstr_field_parts(&field, line, col)?);
            i = j;
            continue;
        }
        if c == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                lit.push('}');
                i += 2;
                continue;
            }
            return Err(RioError::parse("قوس } منفرد في النص التنفيذي", line, col));
        }
        if c == '\\' && i + 1 < chars.len() {
            let esc = chars[i + 1];
            match esc {
                'n' => lit.push('\n'),
                't' => lit.push('\t'),
                'r' => lit.push('\r'),
                '\\' => lit.push('\\'),
                '"' => lit.push('"'),
                '\'' => lit.push('\''),
                other => {
                    lit.push('\\');
                    lit.push(other);
                }
            }
            i += 2;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    if !lit.is_empty() {
        parts.push(FStrPart::Lit(lit));
    }
    Ok(parts)
}

/// تحليل نص حقل واحد `{قيمة_موضحة "="؟ [تحويل] [تنسيق]}`
/// إلى أجزائه: الصيغة أول `:` في المستوى صفر (خارج
/// النصوص والأقواس)؛ والتحويل أول `!` في المستوى صفر
/// غير متبوع بـ`=`؛ والتصحيح `=` أخيرة مفردة. تعبير
/// فيه `:` (مثل دالة مجهولة) يوضع بين قوسين داخل الحقل.
fn parse_fstr_field_parts(field: &str, line: usize, col: usize) -> RioResult<FStrPart> {
    let field = field.trim();
    if field.is_empty() {
        return Err(RioError::parse("حقل فارغ {} في النص التنفيذي", line, col));
    }
    let (head, spec) = split_fstr_colon(field);
    let (expr, conv) = split_fstr_bang(head);
    let (expr, debug) = split_fstr_debug(expr);
    if expr.trim().is_empty() {
        return Err(RioError::parse("حقل بلا تعبير في النص التنفيذي", line, col));
    }
    if let Some(c) = &conv {
        if c != "نص" && c != "تمثيل" {
            return Err(RioError::parse(
                format!(
                    "تحويل غير معروف في النص التنفيذي: !{} — المتاح: !نص، !تمثيل",
                    c
                ),
                line,
                col,
            ));
        }
    }
    Ok(FStrPart::Field {
        src: expr.trim().to_string(),
        debug,
        conv,
        spec: spec.map(|s| {
            let t = s.trim();
            if t.is_empty() {
                String::new()
            } else {
                t.to_string()
            }
        }),
    })
}

/// قسّم على أول `:` في المستوى صفر (خارج النصوص
/// والأقواس الثلاثة) — يعيد (قبلها، بعدها أو لا شيء).
fn split_fstr_colon(field: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let mut it = field.char_indices().peekable();
    while let Some((idx, ch)) = it.next() {
        if let Some(q) = in_str {
            if ch == '\\' {
                it.next();
                continue;
            }
            if ch == q {
                in_str = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => in_str = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return (&field[..idx], Some(&field[idx + 1..])),
            _ => {}
        }
    }
    (field, None)
}

/// قسّم على أول `!` في المستوى صفر غير متبوع بـ`=`
/// (`!=` مقارنة لا تحويل) — يعيد (قبلها، الاسم بعدها
/// أو لا شيء).
fn split_fstr_bang(head: &str) -> (&str, Option<String>) {
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let mut it = head.char_indices().peekable();
    while let Some((idx, ch)) = it.next() {
        if let Some(q) = in_str {
            if ch == '\\' {
                it.next();
                continue;
            }
            if ch == q {
                in_str = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => in_str = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '!' if depth == 0 => {
                if head[idx + 1..].starts_with('=') {
                    continue;
                }
                let conv = head[idx + 1..].trim().to_string();
                return (&head[..idx], Some(conv));
            }
            _ => {}
        }
    }
    (head, None)
}

/// التصحيح: `=` أخيرة مفردة في آخر التعبير (بعد قص
/// المسافات الأخيرة)؛ و`==` و`>=` و`<=` و`!=` ليست
/// تصحيحا. يعيد (التعبير بلا `=`، هل هو تصحيح).
fn split_fstr_debug(expr: &str) -> (&str, bool) {
    let t = expr.trim_end();
    if !t.ends_with('=') {
        return (expr, false);
    }
    let before = &t[..t.len() - 1];
    if before.ends_with(['=', '!', '<', '>', ':']) {
        return (expr, false);
    }
    (before, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse(src: &str) -> Vec<Stmt> {
        let toks = tokenize(src).unwrap();
        Parser::new(toks).parse_program().unwrap()
    }

    #[test]
    fn if_elif_else() {
        let prog = parse("إذا س > ٥:\n    اطبع س\nوإلا إذا س > ١:\n    اطبع ١\nوإلا:\n    اطبع ٠\n");
        assert_eq!(prog.len(), 1);
        assert!(matches!(prog[0].kind, StmtKind::If { .. }));
    }

    #[test]
    fn empty_list_as_bare_call_arg() {
        // اجتماع 32، السؤال الأول: [] معامل مجرد لا فهرسة.
        let prog = parse("جهاز.حوار \"اختيار واحد\" \"اختر\" []\n");
        assert_eq!(prog.len(), 1);
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "حوار"));
                    assert_eq!(args.len(), 3);
                    assert!(matches!(&args[2].kind, ExprKind::List(e) if e.is_empty()));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
        // القائمة الفارغة معاملا أول أيضا.
        let prog = parse("جهاز.حوار [] \"ب\"\n");
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[0].kind, ExprKind::List(e) if e.is_empty()));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
    }

    #[test]
    fn index_after_dotted_name_still_indexes() {
        // الفهرسة بعد اسم منقّط تبقى فهرسة: س.قائمة[٠]
        // ليست استدعاء، و[٠] داخل معامل مجرد فهرسة أيضا.
        let prog = parse("س = س.قائمة[٠]\n");
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => {
                assert!(matches!(&value.kind, ExprKind::Index { .. }));
            }
            other => panic!("expected Assign, got {:?}", other),
        }
        let prog = parse("شبكة.اجلب قائمة[٠]\n");
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert_eq!(args.len(), 1);
                    assert!(matches!(&args[0].kind, ExprKind::Index { .. }));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
        // الصيغة المقوسة بقائمة فارغة كانت تعمل وتبقى.
        let prog = parse("اجلب(س، [])\n");
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[1].kind, ExprKind::List(e) if e.is_empty()));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
    }

    #[test]
    fn named_parser_tags_parse_errors_with_file() {
        let toks = tokenize("إذا س > ٥:\n").unwrap();
        let e = Parser::named(toks, "برنامج.نيفا")
            .parse_program()
            .unwrap_err();
        assert_eq!(e.file.as_deref(), Some("برنامج.نيفا"));
        assert!(e.to_string().contains("الملف برنامج.نيفا"));
    }

    #[test]
    fn repeat_count() {
        let prog = parse("كرر ٣ مرات:\n    اطبع ١\n");
        assert!(matches!(prog[0].kind, StmtKind::RepeatCount { .. }));
    }

    #[test]
    fn unpack_two_targets() {
        // البند 69 (الدفعة 2 من الـ579): س، ص = [١، ٢]
        // أربع جمل: مؤقت، ثم تحقق فكك، ثم إسنادان مفهرسان.
        let prog = parse("س، ص = [١، ٢]\n");
        assert_eq!(prog.len(), 4);
        match &prog[0].kind {
            StmtKind::Assign { target, value, .. } => {
                assert!(matches!(target, AssignTarget::Var(n) if n == "__تفكيك"));
                assert!(matches!(&value.kind, ExprKind::List(e) if e.len() == 2));
            }
            other => panic!("expected temp Assign, got {:?}", other),
        }
        match &prog[1].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "فكك"));
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[1].kind, ExprKind::Number(n) if (*n - 2.0).abs() < 1e-9));
                }
                other => panic!("expected فكك Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
        for (i, want) in ["س", "ص"].iter().enumerate() {
            match &prog[2 + i].kind {
                StmtKind::Assign { target, value, .. } => {
                    assert!(matches!(target, AssignTarget::Var(n) if n == want));
                    match &value.kind {
                        ExprKind::Index { target: t, index } => {
                            assert!(matches!(&t.kind, ExprKind::Ident(n) if n == "__تفكيك"));
                            assert!(matches!(&index.kind, ExprKind::Number(n) if (*n - i as f64).abs() < 1e-9));
                        }
                        other => panic!("expected Index, got {:?}", other),
                    }
                }
                other => panic!("expected Assign, got {:?}", other),
            }
        }
    }

    #[test]
    fn unpack_dot_target() {
        // هدف منقّط مسموح: س.حقل، ص = [١، ٢].
        let prog = parse("س.حقل، ص = [١، ٢]\n");
        assert_eq!(prog.len(), 4);
        assert!(matches!(&prog[2].kind, StmtKind::Assign { target: AssignTarget::Dot { name, .. }, .. } if name == "حقل"));
    }

    #[test]
    fn unpack_bad_target_rejected() {
        // هدف تعبير (س + ١) مرفوض بخطأ نحوي عربي.
        let toks = tokenize("س + ١، ص = [١، ٢]\n").unwrap();
        let e = Parser::new(toks).parse_program().unwrap_err();
        assert!(e.to_string().contains("هدف التفكيك"));
    }

    #[test]
    fn assert_desugars_to_validate_call() {
        // البند 81 (الدفعة 2 من الـ579): متوقع شرط
        // استدعاء تحقق.متوقع(شرط)؛ ومع رسالة معاملان.
        let prog = parse("متوقع س == ١\n");
        assert_eq!(prog.len(), 1);
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "متوقع"));
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected متوقع Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
        let prog = parse("متوقع س == ١، \"رسالة\"\n");
        match &prog[0].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[1].kind, ExprKind::Str(s) if s == "رسالة"));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
    }

    #[test]
    fn match_desugars_to_if_chain() {
        // البنود 99–100 و153–155 (الدفعة 3 من الـ579):
        // طابق يُفكَّك إلى إسناد الموضوع مرة واحدة
        // وعلم `تم` وسلسلة `اذا` — بلا عقد جديدة.
        let prog = parse(
            "طابق س:\n    حال ١:\n        اطبع \"واحد\"\n    حال ن:\n        اطبع ن\n    حال _:\n        اطبع \"أي شيء\"\n",
        );
        assert_eq!(prog.len(), 5);
        match &prog[0].kind {
            StmtKind::Assign { target, .. } => match target {
                AssignTarget::Var(name) => {
                    assert_eq!(name, "__طابق_قيمة_1_1");
                }
                _ => panic!("expected Var target"),
            },
            other => panic!("expected Assign, got {:?}", other),
        }
        // أول حالة: شرطها `و` بين نفي العلم والمساواة.
        match &prog[2].kind {
            StmtKind::If { cond, .. } => match &cond.kind {
                ExprKind::Binary { op, .. } => assert_eq!(*op, BinOp::And),
                other => panic!("expected And cond, got {:?}", other),
            },
            other => panic!("expected If, got {:?}", other),
        }
        // حالة الالتقاط تربط المتغير قبل جسمها.
        match &prog[3].kind {
            StmtKind::If { then_body, .. } => match &then_body[0].kind {
                StmtKind::Assign { target, .. } => match target {
                    AssignTarget::Var(name) => assert_eq!(name, "ن"),
                    _ => panic!("expected Var target"),
                },
                other => panic!("expected capture Assign, got {:?}", other),
            },
            other => panic!("expected If, got {:?}", other),
        }
        // حالة الشامل بلا ربط: جسمها الخارجي `اذا`
        // الحراسة وحدها.
        match &prog[4].kind {
            StmtKind::If { then_body, .. } => {
                assert_eq!(then_body.len(), 1);
                assert!(matches!(then_body[0].kind, StmtKind::If { .. }));
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    #[test]
    fn match_star_and_class_desugar() {
        // البنود 158 (نجمة) و160 (صنف): النجمة حلقة
        // `لكل` على `مدى`، والصنف استدعاء
        // `مطابقة.اسم_الصنف`.
        let prog = parse(
            "طابق ق:\n    حال [أ، *باقي]:\n        اطبع باقي\n    حال نقطة(س=س):\n        اطبع س\n",
        );
        assert_eq!(prog.len(), 4);
        match &prog[2].kind {
            StmtKind::If { then_body, .. } => {
                assert!(then_body
                    .iter()
                    .any(|s| matches!(&s.kind, StmtKind::For { .. })));
                assert!(then_body.iter().any(|s| matches!(
                    &s.kind,
                    StmtKind::Assign { target, .. } if matches!(target, AssignTarget::Var(n) if n == "باقي")
                )));
            }
            other => panic!("expected If, got {:?}", other),
        }
        match &prog[3].kind {
            StmtKind::If { cond, .. } => {
                // اختبار الصنف `و` بين مساواة اسم الصنف
                // واختبار الحقل — فيكفي أن يحمل الشرط
                // استدعاء `مطابقة.اسم_الصنف`.
                assert!(format!("{:?}", cond).contains("اسم_الصنف"));
                assert!(format!("{:?}", cond).contains("مطابقة"));
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    #[test]
    fn match_errors() {
        // طابق بلا حالات، ونمط صنف موضعي، وحال بلا
        // طابق: كلها أخطاء نحو عربية.
        let toks = tokenize("طابق س:\n").unwrap();
        assert!(Parser::new(toks).parse_program().is_err());
        let toks = tokenize("طابق ن:\n    حال نقطة(س):\n        اطبع ١\n").unwrap();
        let e = Parser::new(toks).parse_program().unwrap_err();
        assert!(e.to_string().contains("موضعي"));
        let toks = tokenize("حال ١:\n    اطبع ١\n").unwrap();
        assert!(Parser::new(toks).parse_program().is_err());
    }

    #[test]
    fn identity_operator_desugars() {
        // البندان 117 و118 (الدفعة 2 من الـ579): `هل`
        // استدعاء دوال.هوية، و`هل ليس` نفيه؛ و`هل` وحدها
        // في موضع قيمة متغير عاديا.
        let prog = parse("اطبع س هل ص\n");
        assert_eq!(prog.len(), 1);
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { target, name }
                        if name == "هوية"
                        && matches!(&target.kind, ExprKind::Ident(n) if n == "دوال")));
                    assert_eq!(args.len(), 2);
                }
                other => panic!("expected هوية Call, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع س هل ليس ص\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Unary { op: UnOp::Not, expr } => {
                    assert!(matches!(&expr.kind, ExprKind::Call { kwargs: _, .. }));
                }
                other => panic!("expected Not(هوية), got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع هل\n");
        match &prog[0].kind {
            StmtKind::Print { args } => {
                assert!(matches!(&args[0].kind, ExprKind::Ident(n) if n == "هل"));
            }
            other => panic!("expected Print, got {:?}", other),
        }
    }

    #[test]
    fn slice_desugars() {
        // البند 133 (الدفعة 2 من الـ579): `ق[1:3]` استدعاء
        // قوائم.شريحة بأربعة معاملات؛ و`ق[1]` فهرسة كما كان.
        let prog = parse("س = ق[1:3]\n");
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => match &value.kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "شريحة"));
                    assert_eq!(args.len(), 4);
                    assert!(matches!(&args[0].kind, ExprKind::Ident(n) if n == "ق"));
                    assert!(matches!(&args[1].kind, ExprKind::Number(n) if (*n - 1.0).abs() < 1e-9));
                    assert!(matches!(&args[2].kind, ExprKind::Number(n) if (*n - 3.0).abs() < 1e-9));
                    assert!(matches!(&args[3].kind, ExprKind::Nil));
                }
                other => panic!("expected شريحة Call, got {:?}", other),
            },
            other => panic!("expected Assign, got {:?}", other),
        }
        let prog = parse("س = ق[:]\n");
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => match &value.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert!(matches!(&args[1].kind, ExprKind::Nil));
                    assert!(matches!(&args[2].kind, ExprKind::Nil));
                    assert!(matches!(&args[3].kind, ExprKind::Nil));
                }
                other => panic!("expected شريحة Call, got {:?}", other),
            },
            other => panic!("expected Assign, got {:?}", other),
        }
        let prog = parse("س = ق[::-1]\n");
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => match &value.kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert!(matches!(&args[3].kind, ExprKind::Unary { op: UnOp::Neg, .. }));
                }
                other => panic!("expected شريحة Call, got {:?}", other),
            },
            other => panic!("expected Assign, got {:?}", other),
        }
        let prog = parse("س = ق[1]\n");
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => {
                assert!(matches!(&value.kind, ExprKind::Index { .. }));
            }
            other => panic!("expected Assign, got {:?}", other),
        }
    }

    #[test]
    fn fstring_desugars() {
        // البنود 163–165 (الدفعة 2 من الـ579): `ف"س={س}"`
        // سلسلة `+`؛ والحقل استدعاء نص.نسق؛ والتحويل
        // والتنسيق والتصحيح.
        let prog = parse("اطبع ف\"س={س}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Binary { left, op: BinOp::Add, right } => {
                    assert!(matches!(&left.kind, ExprKind::Str(s) if s == "س="));
                    assert!(matches!(&right.kind, ExprKind::Call { callee, kwargs: _, .. }
                        if matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "نسق")));
                }
                other => panic!("expected Add chain, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع ف\"{س!تمثيل}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Call { callee, kwargs: _,
                ..
            } => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "تمثيل"));
                }
                other => panic!("expected تمثيل Call, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع ف\"{س:.2}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Call { args, kwargs: _,
                ..
            } => {
                    assert!(matches!(&args[0].kind, ExprKind::Str(s) if s == "{0:.2}"));
                }
                other => panic!("expected نسق Call, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        // التصحيح `=` يسبق القيمة بلفظ التعبير.
        let prog = parse("اطبع ف\"{س=}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Binary { left, op: BinOp::Add, .. } => {
                    assert!(matches!(&left.kind, ExprKind::Str(s) if s == "س="));
                }
                other => panic!("expected debug Add, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        // أخطاء: تحويل مجهول وحقل فارغ وقوس منفرد.
        assert!(Parser::new(tokenize("اطبع ف\"{س!غريب}\"\n").unwrap())
            .parse_program()
            .is_err());
        assert!(Parser::new(tokenize("اطبع ف\"{}\"\n").unwrap())
            .parse_program()
            .is_err());
        assert!(Parser::new(tokenize("اطبع ف\"نص}\"\n").unwrap())
            .parse_program()
            .is_err());
    }

    #[test]
    fn lambda_expression() {
        let prog = parse("مضاعف = دالة_مجهولة(س): س * 2\n");
        assert_eq!(prog.len(), 1);
        match &prog[0].kind {
            StmtKind::Assign { value, .. } => match &value.kind {
                ExprKind::Lambda { params, param_types, body } => {
                    assert_eq!(params, &["س".to_string()]);
                    assert_eq!(param_types, &[None]);
                    assert!(matches!(body.kind, ExprKind::Binary { .. }));
                }
                other => panic!("expected Lambda, got {:?}", other),
            },
            other => panic!("expected Assign, got {:?}", other),
        }
    }

    #[test]
    fn with_desugar_shape() {
        // البند 88 (الاجتماع 88 §103): `عند م ك س:`
        // تُفكَّك إلى 7 جمل — مؤقت المدير، وفحص
        // الدالتين، واستدعاء دخول، وربط احتياطي،
        // وراية خروج، وربط ك، ثم حاول بماعدا وأخيرا.
        let prog = parse("عند م ك س:\n    اطبع س\n");
        assert_eq!(prog.len(), 7);
        assert!(matches!(&prog[0].kind, StmtKind::Assign { target: AssignTarget::Var(n), .. } if n == "__عند_مدير_1_1_0"));
        match &prog[1].kind {
            StmtKind::ExprStmt { expr } => match &expr.kind {
                ExprKind::Call { callee, args,
                kwargs: _,
             ..} => {
                    assert!(matches!(&callee.kind, ExprKind::Ident(n) if n == "__عند_افحص"));
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected افحص Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
        assert!(matches!(&prog[5].kind, StmtKind::Assign { target: AssignTarget::Var(n), .. } if n == "س"));
        match &prog[6].kind {
            StmtKind::Try { try_body, except_body, finally_body, .. } => {
                assert_eq!(try_body.len(), 1);
                assert!(except_body.is_some());
                assert!(finally_body.is_some());
            }
            other => panic!("expected Try, got {:?}", other),
        }
        // الصيغة المقوسة بفاصلة أخيرة وعنصران:
        // الطبقة الخارجية 6 جمل (بلا ربط ك)، وداخل
        // حاولها طبقة ثانية 6 جمل (دخول بالترتيب
        // وخروج بالعكس).
        let prog2 = parse("عند (م، ن،):\n    اطبع ١\n");
        assert_eq!(prog2.len(), 6);
        match &prog2[5].kind {
            StmtKind::Try { try_body, .. } => assert_eq!(try_body.len(), 6),
            other => panic!("expected Try, got {:?}", other),
        }
        // `زامن عند` مرفوضة تحليلا (خارج النطاق).
        assert!(Parser::new(tokenize("زامن عند م:\n    اطبع ١\n").unwrap())
            .parse_program()
            .is_err());
        // `عند` بلا عناصر مرفوضة.
        assert!(Parser::new(tokenize("عند ():\n    اطبع ١\n").unwrap())
            .parse_program()
            .is_err());
    }
}
