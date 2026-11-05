//! Niva language server (LSP over stdio).
//!
//! v1: diagnostics, hover, document symbols, completion.
//!
//! Meeting 12 (Q2 condition): diagnostics reuse the semantic
//! analyzer's EXACT code path — the same tokenize, parse, analyze
//! sequence as the CLI and `run_source` — no duplicate logic.
//! A diagnostic is the analyzer's own Arabic error (kind +
//! message), with the range taken from its line/column.
//!
//! Run: `niva-lsp` reads LSP (JSON-RPC over stdio) on stdin and
//! writes on stdout. Any editor with an LSP client can use it;
//! the VS Code client wiring is a separate step.

use lsp_server::{Connection, ErrorCode, Message, Notification, RequestId, Response};
use lsp_types::*;
use niva_engine::error::RioError;
use niva_engine::lexer::{tokenize, Keyword, TokenKind};
use niva_engine::parser::Parser;
use niva_engine::semantic::SemanticAnalyzer;
use std::collections::HashMap;
use std::error::Error;

// ---------------------------------------------------------------------
// Static knowledge: keywords, builtins, types, stdlib, methods.
// One-line Arabic docs, taken from the engine's own behaviour.
// ---------------------------------------------------------------------

const KEYWORDS: &[(&str, &str)] = &[
    ("اطبع", "يطبع قيمة أو أكثر، مفصولة بمسافة، ثم سطر جديد."),
    ("إذا", "شرط: إذا الشرط: ... وإلا إذا ... وإلا ..."),
    ("وإلا إذا", "شرط ثان بعد إذا فشل شرطها."),
    ("وإلا", "الفرع الأخير عندما تفشل كل الشروط."),
    ("بينما", "حلقة تتكرر ما دام الشرط صحيحا."),
    ("لكل", "حلقة على عناصر قائمة أو مدى: لكل عنصر في قائمة:"),
    ("في", "يختبر العضوية: عنصر في قائمة؛ أو يحدد متغير الحلقة في لكل."),
    ("دالة", "تعريف دالة: دالة اسم(معاملات):"),
    ("دالة_مجهولة", "دالة بلا اسم: دالة_مجهولة(س): س + 1"),
    ("ارجع", "يعيد قيمة من الدالة وينهيها."),
    ("و", "عامل منطقي: و (AND)."),
    ("أو", "عامل منطقي: أو (OR)."),
    ("ليس", "عامل منطقي: ليس (NOT)."),
    ("صحيح", "القيمة المنطقية: صحيح."),
    ("خطأ", "القيمة المنطقية: خطأ."),
    ("لاشيء", "قيمة فارغة: لاشيء."),
    ("اكسر", "يخرج من الحلقة فورا."),
    ("تابع", "يتخطى باقي جسم الحلقة إلى التكرار التالي."),
    ("مرر", "جملة فارغة، تفعل لا شيء."),
    ("حاول", "يبدأ كتلة محاولة: حاول: ... ماعدا: ... أخيرا:"),
    ("ماعدا", "يمسك الخطأ بعد حاول."),
    ("أخيرا", "كتلة تنفذ دائما بعد حاول، خطأ كان أو لا."),
    ("فئة", "تعريف فئة: فئة اسم:"),
    ("أنشئ", "ينشئ كائنا من فئة: أنشئ نقطة(٣، ٤)"),
    ("ورث", "وراثة: فئة ابن ورث أب:"),
    ("واجهة", "تعريف واجهة: واجهة اسم:"),
    ("استورد", "يستورد وحدة: استورد رياضيات"),
    ("صدّر", "يصدّر اسما من الوحدة: صدّر اسم"),
    ("باسم", "يستورد باسم آخر: استورد رياضيات باسم ر"),
    ("من", "يستورد أسماء محددة: من رياضيات استورد جذر"),
    ("كرر", "حلقة تكرار: كرر ٣ مرات:"),
    ("مرات", "مع كرر: كرر ٣ مرات:"),
    ("طالما", "حلقة تتكرر ما دام الشرط صحيحا (مرادف بينما)."),
    ("نفس", "يشير إلى الكائن الحالي داخل طرق الفئة."),
];

const BUILTINS: &[(&str, &str)] = &[
    ("طول", "طول(قائمة أو نص) ← عدد"),
    ("مدى", "مدى(نهاية) أو مدى(بداية، نهاية) ← قائمة أعداد"),
    ("مجموع", "مجموع(قائمة أعداد) ← عدد"),
    ("أدنى", "أدنى(قائمة) ← أصغر عنصر"),
    ("أقصى", "أقصى(قائمة) ← أكبر عنصر"),
    ("مطلق", "مطلق(عدد) ← القيمة المطلقة"),
    ("عدد", "عدد(قيمة) ← يحول نصا أو منطقيا إلى عدد"),
    ("نص", "نص(قيمة) ← يحول أي قيمة إلى نص"),
    ("نوع", "نوع(قيمة) ← نص باسم نوع القيمة"),
    ("اقرأ_ملف", "اقرأ_ملف(مسار) ← نص الملف (مقيد بمجلد البرنامج)"),
    ("اكتب_ملف", "اكتب_ملف(مسار، نص) ← لاشيء"),
    ("ادخل", "ادخل() أو ادخل(سؤال) ← نص من المستخدم"),
];

const TYPES: &[(&str, &str)] = &[
    ("عدد", "نوع: عدد (صحيح أو عشري)."),
    ("نص", "نوع: نص."),
    ("منطقي", "نوع: صحيح أو خطأ."),
    ("قائمة", "نوع: قائمة عناصر."),
    ("قاموس", "نوع: قاموس مفاتيح وقيم."),
    ("لاشيء", "نوع: القيمة الفارغة."),
    ("دالة", "نوع: دالة."),
    ("وحدة", "نوع: وحدة مستوردة."),
    ("كائن", "نوع: كائن من فئة."),
    ("أي", "نوع: أي قيمة، بلا فحص."),
];

const STDLIB: &[(&str, &str)] = &[
    ("رياضيات", "وحدة: جذر، قوة، جيب، جيب_تمام، ظل، لوغاريتم، أس_طبيعي، أرضية، سقف، باي، هـ"),
    ("عشوائي", "وحدة: عدد_عشوائي، صحيح_عشوائي، اختيار (للألعاب والمحاكاة فقط، ليست تشفيرا)"),
    ("وقت", "وحدة: الآن (ثواني Unix)، نوم (ثواني)"),
];

const STR_METHODS: &[(&str, &str)] = &[
    ("كبير", "طريقة نص: كبير() ← النص بأحرف كبيرة"),
    ("صغير", "طريقة نص: صغير() ← النص بأحرف صغيرة"),
    ("قص", "طريقة نص: قص() ← النص بلا فراغات الأطراف"),
    ("استبدل", "طريقة نص: استبدل(قديم، جديد) ← نص مستبدل"),
    ("قسم", "طريقة نص: قسم(فاصل) ← قائمة نصوص"),
    ("يحتوي", "طريقة نص: يحتوي(نص) ← منطقي"),
    ("طول", "طريقة نص: طول() ← عدد الأحرف"),
];

const LIST_METHODS: &[(&str, &str)] = &[
    ("أضف", "طريقة قائمة: أضف(عنصر) ← تضيف في النهاية"),
    ("أزل", "طريقة قائمة: أزل() أو أزل(فهرس) ← العنصر المزال"),
    ("يحتوي", "طريقة قائمة: يحتوي(عنصر) ← منطقي"),
    ("طول", "طريقة قائمة: طول() ← عدد العناصر"),
];

// ---------------------------------------------------------------------
// Diagnostics: the analyzer's exact code path (Meeting 12).
// ---------------------------------------------------------------------

fn diagnostic(e: &RioError, source: &str) -> Diagnostic {
    let line = e.line.saturating_sub(1) as u32;
    let col = e.col.saturating_sub(1) as u32;
    let line_len = source
        .lines()
        .nth(line as usize)
        .map(|l| l.chars().count())
        .unwrap_or(0) as u32;
    let start_char = col.min(line_len);
    let end_char = if line_len > start_char {
        start_char + 1
    } else {
        start_char
    };
    Diagnostic {
        range: Range {
            start: Position {
                line,
                character: start_char,
            },
            end: Position {
                line,
                character: end_char,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("niva".to_string()),
        message: format!("خطأ {}: {}", e.kind.arabic(), e.message),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let toks = match tokenize(source) {
        Ok(t) => t,
        Err(e) => return vec![diagnostic(&e, source)],
    };
    let program = match Parser::new(toks).parse_program() {
        Ok(p) => p,
        Err(e) => return vec![diagnostic(&e, source)],
    };
    let mut analyzer = SemanticAnalyzer::new();
    match analyzer.analyze_program(&program) {
        Ok(()) => Vec::new(),
        Err(e) => vec![diagnostic(&e, source)],
    }
}

// ---------------------------------------------------------------------
// Hover: the word under the cursor, from the static tables.
// LSP positions are UTF-16 units; Arabic (BMP) is one unit per
// character, so character counts match here.
// ---------------------------------------------------------------------

fn word_at(text: &str, line: u32, character: u32) -> Option<(String, Range)> {
    let line_text = text.lines().nth(line as usize)?;
    let chars: Vec<char> = line_text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut pos = (character as usize).min(chars.len());
    if pos >= chars.len() || !is_word(chars[pos]) {
        if pos == 0 {
            return None;
        }
        pos -= 1;
        if !is_word(chars[pos]) {
            return None;
        }
    }
    let mut start = pos;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = pos;
    while end + 1 < chars.len() && is_word(chars[end + 1]) {
        end += 1;
    }
    let word: String = chars[start..=end].iter().collect();
    Some((
        word,
        Range {
            start: Position {
                line,
                character: start as u32,
            },
            end: Position {
                line,
                character: (end + 1) as u32,
            },
        },
    ))
}

fn hover_for(word: &str) -> Option<String> {
    for (name, doc) in KEYWORDS.iter().chain(BUILTINS).chain(TYPES).chain(STDLIB) {
        if *name == word {
            return Some(doc.to_string());
        }
    }
    for (name, doc) in STR_METHODS.iter().chain(LIST_METHODS) {
        if *name == word {
            return Some(doc.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------
// Document symbols: `دالة NAME` / `فئة NAME` from the lexer tokens
// (exact name positions, no AST spans needed).
// ---------------------------------------------------------------------

fn symbols_for(source: &str) -> Vec<DocumentSymbol> {
    let toks = match tokenize(source) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < toks.len() {
        let (kind, name) = match (&toks[i].kind, &toks[i + 1].kind) {
            (TokenKind::Keyword(Keyword::Func), TokenKind::Ident(n)) => {
                (SymbolKind::FUNCTION, n.clone())
            }
            (TokenKind::Keyword(Keyword::Class), TokenKind::Ident(n)) => {
                (SymbolKind::CLASS, n.clone())
            }
            _ => {
                i += 1;
                continue;
            }
        };
        let def_line = (toks[i].line - 1) as u32;
        let name_line = (toks[i + 1].line - 1) as u32;
        let name_len = name.chars().count() as u32;
        let line_len = source
            .lines()
            .nth(def_line as usize)
            .map(|l| l.chars().count())
            .unwrap_or(0) as u32;
        out.push(DocumentSymbol {
            name: name.clone(),
            detail: None,
            kind,
            tags: None,
            deprecated: None,
            range: Range {
                start: Position {
                    line: def_line,
                    character: (toks[i].col - 1) as u32,
                },
                end: Position {
                    line: def_line,
                    character: line_len,
                },
            },
            selection_range: Range {
                start: Position {
                    line: name_line,
                    character: (toks[i + 1].col - 1) as u32,
                },
                end: Position {
                    line: name_line,
                    character: (toks[i + 1].col - 1) as u32 + name_len,
                },
            },
            children: None,
        });
        i += 2;
    }
    out
}

// ---------------------------------------------------------------------
// Completion: static lists. After a dot, methods + stdlib members.
// ---------------------------------------------------------------------

fn completion_items(dot_context: bool) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    if dot_context {
        for (name, doc) in STR_METHODS.iter().chain(LIST_METHODS) {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(doc.to_string()),
                ..Default::default()
            });
        }
        for (name, doc) in STDLIB {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(doc.to_string()),
                ..Default::default()
            });
        }
        return items;
    }
    for (name, doc) in KEYWORDS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(doc.to_string()),
            ..Default::default()
        });
    }
    for (name, doc) in BUILTINS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(doc.to_string()),
            ..Default::default()
        });
    }
    for (name, doc) in TYPES {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(doc.to_string()),
            ..Default::default()
        });
    }
    for (name, doc) in STDLIB {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(doc.to_string()),
            ..Default::default()
        });
    }
    items
}

// ---------------------------------------------------------------------
// Server plumbing.
// ---------------------------------------------------------------------

fn publish(connection: &Connection, uri: &str, text: &str) -> Result<(), Box<dyn Error>> {
    let params = PublishDiagnosticsParams {
        uri: uri.parse::<Uri>()?,
        diagnostics: diagnostics_for(text),
        version: None,
    };
    connection.sender.send(Message::Notification(Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        serde_json::to_value(params)?,
    )))?;
    Ok(())
}

fn respond_ok(
    connection: &Connection,
    id: RequestId,
    result: serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    connection
        .sender
        .send(Message::Response(Response::new_ok(id, result)))?;
    Ok(())
}

fn main_loop(connection: Connection) -> Result<(), Box<dyn Error>> {
    let mut docs: HashMap<String, String> = HashMap::new();
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                let result: Result<serde_json::Value, Box<dyn Error>> = (|| match req
                    .method
                    .as_str()
                {
                    "textDocument/hover" => {
                        let p: HoverParams = serde_json::from_value(req.params.clone())?;
                        let pos = p.text_document_position_params.position;
                        let uri = p
                            .text_document_position_params
                            .text_document
                            .uri
                            .as_str()
                            .to_string();
                        let hover = docs.get(&uri).and_then(|text| {
                            word_at(text, pos.line, pos.character).and_then(|(word, range)| {
                                hover_for(&word).map(|doc| Hover {
                                    contents: HoverContents::Markup(MarkupContent {
                                        kind: MarkupKind::Markdown,
                                        value: doc,
                                    }),
                                    range: Some(range),
                                })
                            })
                        });
                        Ok(serde_json::to_value(hover)?)
                    }
                    "textDocument/documentSymbol" => {
                        let p: DocumentSymbolParams =
                            serde_json::from_value(req.params.clone())?;
                        let uri = p.text_document.uri.as_str().to_string();
                        let symbols = docs
                            .get(&uri)
                            .map(|text| symbols_for(text))
                            .unwrap_or_default();
                        Ok(serde_json::to_value(symbols)?)
                    }
                    "textDocument/completion" => {
                        let p: CompletionParams = serde_json::from_value(req.params.clone())?;
                        let dot = p
                            .context
                            .as_ref()
                            .and_then(|c| c.trigger_character.as_deref())
                            == Some(".");
                        let items = completion_items(dot);
                        Ok(serde_json::to_value(CompletionResponse::Array(items))?)
                    }
                    _ => Err(format!("method not supported: {}", req.method).into()),
                })();
                match result {
                    Ok(value) => respond_ok(&connection, req.id, value)?,
                    Err(e) => {
                        let is_unsupported = e.to_string().starts_with("method not supported");
                        let code = if is_unsupported {
                            ErrorCode::MethodNotFound
                        } else {
                            ErrorCode::InternalError
                        };
                        connection.sender.send(Message::Response(Response::new_err(
                            req.id,
                            code as i32,
                            e.to_string(),
                        )))?;
                    }
                }
            }
            Message::Response(_) => {}
            Message::Notification(not) => match not.method.as_str() {
                "textDocument/didOpen" => {
                    let p: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
                    let uri = p.text_document.uri.as_str().to_string();
                    let text = p.text_document.text.clone();
                    docs.insert(uri.clone(), text.clone());
                    publish(&connection, &uri, &text)?;
                }
                "textDocument/didChange" => {
                    let p: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
                    let uri = p.text_document.uri.as_str().to_string();
                    if let Some(change) = p.content_changes.into_iter().last() {
                        let text = change.text.clone();
                        docs.insert(uri.clone(), text.clone());
                        publish(&connection, &uri, &text)?;
                    }
                }
                "textDocument/didClose" => {
                    let p: DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
                    let uri = p.text_document.uri.as_str().to_string();
                    docs.remove(&uri);
                    publish(&connection, &uri, "")?;
                }
                "exit" => break,
                _ => {}
            },
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let (connection, io_threads) = Connection::stdio();
    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    })?;
    let _init_params = connection.initialize(server_capabilities)?;
    main_loop(connection)?;
    io_threads.join()?;
    Ok(())
}
