//! Parser: tokens -> AST. Recursive descent with precedence:
//! or > and > comparison > additive > multiplicative > unary > primary.
//! Blocks are colon + indented lines (INDENT/DEDENT from the lexer),
//! or a single statement on the same line after the colon.

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{RioError, RioResult};
use crate::lexer::{Keyword, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
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

    fn expect_colon(&mut self) -> RioResult<()> {
        self.expect(&TokenKind::Colon, "نقطتين :")?;
        Ok(())
    }

    pub fn parse_program(&mut self) -> RioResult<Vec<Stmt>> {
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
            stmts.push(self.parse_statement()?);
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
                    _ => stmts.push(self.parse_statement()?),
                }
            }
            Ok(stmts)
        } else {
            // Inline block: one statement on the same line.
            Ok(vec![self.parse_statement()?])
        }
    }

    fn parse_statement(&mut self) -> RioResult<Stmt> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Keyword(Keyword::Print) => self.parse_print(),
            TokenKind::Keyword(Keyword::If) => self.parse_if(),
            TokenKind::Keyword(Keyword::While) => self.parse_while(),
            TokenKind::Keyword(Keyword::For) => self.parse_for(),
            TokenKind::Keyword(Keyword::Repeat) => self.parse_repeat(),
            TokenKind::Keyword(Keyword::Func) => self.parse_funcdef(),
            TokenKind::Keyword(Keyword::Return) => {
                self.advance();
                let value = if self.is_separator()
                    || matches!(self.peek().kind, TokenKind::Dedent | TokenKind::Eof)
                {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                Ok(Stmt::new(StmtKind::Return { value }, tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.advance();
                Ok(Stmt::new(StmtKind::Break, tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.advance();
                Ok(Stmt::new(StmtKind::Continue, tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Pass) => {
                self.advance();
                Ok(Stmt::new(StmtKind::Pass, tok.line, tok.col))
            }
            TokenKind::Keyword(Keyword::Try) => self.parse_try(),
            TokenKind::Keyword(Keyword::Class) => self.parse_classdef(),
            TokenKind::Keyword(Keyword::Interface) => self.parse_interfacedef(),
            TokenKind::Keyword(Keyword::Import) => self.parse_import(),
            TokenKind::Keyword(Keyword::From) => self.parse_from_import(),
            TokenKind::Keyword(Keyword::Export) => self.parse_export(),
            _ => self.parse_assign_or_expr(),
        }
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
        while matches!(self.peek().kind, TokenKind::Keyword(Keyword::Else)) {
            self.advance(); // وإلا
            if matches!(self.peek().kind, TokenKind::Keyword(Keyword::If)) {
                self.advance(); // إذا (بعد وإلا)
                let econd = self.parse_expr()?;
                self.expect_colon()?;
                let ebody = self.parse_block()?;
                elifs.push((econd, ebody));
            } else {
                self.expect_colon()?;
                else_body = Some(self.parse_block()?);
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
        // Optional return type: ): عدد:
        // After ')', a Colon followed by a type name and another
        // Colon is a return type; a Colon followed by a block
        // (Newline/Indent) is just the block colon.
        let mut return_type: Option<String> = None;
        if matches!(self.peek().kind, TokenKind::Colon) {
            if matches!(self.peek_at(1).kind, TokenKind::Ident(_))
                && matches!(self.peek_at(2).kind, TokenKind::Colon)
            {
                self.advance(); // :
                return_type = Some(self.parse_type_name()?);
            }
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

    fn parse_type_name(&mut self) -> RioResult<String> {
        let (name, line, col) = self.expect_ident("اسم نوع")?;
        if crate::ast::is_known_type(&name) {
            Ok(name)
        } else {
            Err(RioError::parse(format!("نوع غير معروف: {}", name), line, col))
        }
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
                },
                expr.line,
                expr.col,
            ));
        }
        if matches!(self.peek().kind, TokenKind::Eq) {
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
                StmtKind::Assign { target, value, annotation: None },
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

    fn parse_comparison(&mut self) -> RioResult<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            // كلمات المقارنة (اجتماع 19): بأطول مطابقة أولا،
            // وتنتج نفس عقد AST الرموز.
            if let Some((op, count)) = self.word_operator() {
                let tok = self.peek().clone();
                for _ in 0..count {
                    self.advance();
                }
                let right = self.parse_additive()?;
                left = Expr::new(
                    ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            // ليس في  -> NotIn (two tokens)
            if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Not))
                && matches!(self.peek_at(1).kind, TokenKind::Keyword(Keyword::In))
            {
                let tok = self.advance();
                self.advance(); // في
                let right = self.parse_additive()?;
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
            let right = self.parse_additive()?;
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
            _ => self.parse_postfix(),
        }
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
                let tok = self.advance();
                let index = self.parse_expr()?;
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
                let (name, _, _) = self.expect_ident("اسم بعد النقطة")?;
                expr = Expr::new(
                    ExprKind::Dot { target: Box::new(expr), name },
                    tok.line,
                    tok.col,
                );
                continue;
            }
            break;
        }
        Ok(expr)
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
    fn repeat_count() {
        let prog = parse("كرر ٣ مرات:\n    اطبع ١\n");
        assert!(matches!(prog[0].kind, StmtKind::RepeatCount { .. }));
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
