//! Parser: tokens -> AST. Recursive descent with precedence:
//! or > and > comparison > additive > multiplicative > unary > primary.
//! Blocks are colon + indented lines (INDENT/DEDENT from the lexer),
//! or a single statement on the same line after the colon.

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, TypeExpr, UnOp};
use crate::error::{RioError, RioResult};
use crate::lexer::{tokenize, Keyword, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// الملف الذي يُحلَّل (لإسناد أخطاء النحو إليه) —
    /// اجتماع 32، السؤال الخامس. لا شيء للإدخال بلا ملف.
    file: Option<String>,
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
        TokenKind::Ident(_) | TokenKind::Number(_) | TokenKind::Str(_) => true,
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

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, file: None }
    }

    /// Parser مع اسم ملف يُسنَد إلى أخطاء النحو —
    /// السباكة داخل المحرك (اجتماع 32)؛ والتوصيل العلوي
    /// (main.rs) سطران بعد هبوط المهمة 29.
    pub fn named(tokens: Vec<Token>, file: &str) -> Self {
        Self { tokens, pos: 0, file: Some(file.to_string()) }
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
        let tok = self.peek().clone();
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

    fn parse_param_list(&mut self) -> RioResult<(Vec<String>, Vec<Option<String>>)> {
        self.expect(&TokenKind::LParen, "قوس فتح (")?;
        let mut params = Vec::new();
        let mut param_types: Vec<Option<String>> = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let (p, _, _) = self.expect_ident("اسم معامل")?;
                params.push(p);
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance();
                    param_types.push(Some(self.parse_type_name()?));
                } else {
                    param_types.push(None);
                }
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
        Ok((params, param_types))
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
        let (params, param_types) = self.parse_param_list()?;
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
        Ok(Stmt::new(
            StmtKind::FuncDef { name, params, param_types, return_type, body },
            tok.line,
            tok.col,
        ))
    }

    fn parse_classdef(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // فئة
        let (name, _, _) = self.expect_ident("اسم الفئة")?;
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
        Ok(Stmt::new(StmtKind::ClassDef { name, base, interfaces, methods }, tok.line, tok.col))
    }

    fn parse_interfacedef(&mut self) -> RioResult<Stmt> {
        let tok = self.advance(); // واجهة
        let (name, _, _) = self.expect_ident("اسم الواجهة")?;
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
            StmtKind::Try { try_body, except_body, finally_body },
            tok.line,
            tok.col,
        ))
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
        if !crate::ast::is_known_type(&name) {
            return Err(RioError::parse(format!("نوع غير معروف: {}", name), line, col));
        }
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
                _ => Err(RioError::parse(
                    format!("النوع {} لا يقبل معاملات نوع", name),
                    line,
                    col,
                )),
            }
        } else {
            Ok(TypeExpr::Named(name))
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
        self.parse_or()
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
        loop {
            if matches!(self.peek().kind, TokenKind::LParen) {
                let tok = self.advance();
                let mut args = Vec::new();
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
                expr = Expr::new(
                    ExprKind::Call { callee: Box::new(expr), args },
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
                    while bare_call_follows(self.peek(), name_line)
                        || empty_list_follows(self, name_line)
                    {
                        args.push(self.parse_expr()?);
                    }
                    expr = Expr::new(
                        ExprKind::Call { callee: Box::new(expr), args },
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
                let (params, param_types) = self.parse_param_list()?;
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
                }
                Ok(Expr::new(ExprKind::Create { class, args }, tok.line, tok.col))
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::new(ExprKind::Ident(name), tok.line, tok.col))
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "قوس إغلاق )")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                            if matches!(self.peek().kind, TokenKind::RBracket) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
                let t = self.expect(&TokenKind::RBracket, "قوس إغلاق ]")?;
                Ok(Expr::new(ExprKind::List(elems), tok.line, t.col))
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RBrace) {
                    loop {
                        let key = match &self.peek().kind {
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
                        let value = self.parse_expr()?;
                        entries.push((key, value));
                        if matches!(self.peek().kind, TokenKind::Comma) {
                            self.advance();
                            if matches!(self.peek().kind, TokenKind::RBrace) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
                let t = self.expect(&TokenKind::RBrace, "قوس إغلاق }")?;
                Ok(Expr::new(ExprKind::Dict(entries), tok.line, t.col))
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
                ExprKind::Call { callee, args } => {
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
                ExprKind::Call { args, .. } => {
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
                ExprKind::Call { args, .. } => {
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
                ExprKind::Call { args, .. } => {
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
                ExprKind::Call { callee, args } => {
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
                ExprKind::Call { callee, args } => {
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
                ExprKind::Call { args, .. } => {
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[1].kind, ExprKind::Str(s) if s == "رسالة"));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            other => panic!("expected ExprStmt, got {:?}", other),
        }
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
                ExprKind::Call { callee, args } => {
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
                    assert!(matches!(&expr.kind, ExprKind::Call { .. }));
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
                ExprKind::Call { callee, args } => {
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
                ExprKind::Call { args, .. } => {
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
                ExprKind::Call { args, .. } => {
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
                    assert!(matches!(&right.kind, ExprKind::Call { callee, .. }
                        if matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "نسق")));
                }
                other => panic!("expected Add chain, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع ف\"{س!تمثيل}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Call { callee, .. } => {
                    assert!(matches!(&callee.kind, ExprKind::Dot { name, .. } if name == "تمثيل"));
                }
                other => panic!("expected تمثيل Call, got {:?}", other),
            },
            other => panic!("expected Print, got {:?}", other),
        }
        let prog = parse("اطبع ف\"{س:.2}\"\n");
        match &prog[0].kind {
            StmtKind::Print { args } => match &args[0].kind {
                ExprKind::Call { args, .. } => {
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
}
