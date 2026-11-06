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
    ("صدر", "يصدّر اسما من الوحدة: صدر اسم"),
    ("باسم", "يستورد باسم آخر: استورد رياضيات باسم ر"),
    ("من", "يستورد أسماء محددة: من رياضيات استورد جذر"),
    ("كرر", "حلقة تكرار: كرر ٣ مرات:"),
    ("مرات", "مع كرر: كرر ٣ مرات:"),
    ("طالما", "حلقة تتكرر ما دام الشرط صحيحا (مرادف بينما)."),
    ("أكبر", "كلمة مقارنة: أكبر من (>)؛ أو أكبر من أو يساوي (>=)."),
    ("أصغر", "كلمة مقارنة: أصغر من (<)؛ أو أصغر من أو يساوي (<=)."),
    ("يساوي", "كلمة مقارنة: يساوي (==)."),
    ("لا", "كلمة مقارنة: لا يساوي (!=)."),
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
    ("اعرض_الملفات", "اعرض_الملفات(مسار) ← قائمة أسماء في المجلد"),
    ("المجلد_الحالي", "المجلد_الحالي() ← نص المسار الحالي بصيغة الترمنال (~)"),
    ("انتقل", "انتقل(مسار) ← يغيّر المجلد الحالي"),
    ("أنشئ_مجلد", "أنشئ_مجلد(مسار) ← ينشئ مجلدا"),
    ("احذف", "احذف(مسار) ← يحذف ملفا أو مجلدا فارغا"),
    ("احذف_مجلد", "احذف_مجلد(مسار) ← يحذف مجلدا فارغا"),
    ("احذف_ملف", "احذف_ملف(مسار) ← يحذف ملفا"),
    ("انسخ", "انسخ(مصدر، هدف) ← ينسخ ملفا"),
    ("انقل", "انقل(مصدر، هدف) ← ينقل ملفا أو مجلدا"),
    ("أنشئ_ملف", "أنشئ_ملف(مسار) ← ينشئ ملفا فارغا"),
    ("اعرض_البداية", "اعرض_البداية(مسار) أو (مسار، عدد) ← أول أسطر الملف"),
    ("اعرض_النهاية", "اعرض_النهاية(مسار) أو (مسار، عدد) ← آخر أسطر الملف"),
    ("عد_الأسطر", "عد_الأسطر(مسار) ← عدد أسطر الملف"),
    ("عد_الكلمات", "عد_الكلمات(مسار) ← عدد كلمات الملف"),
    ("ابحث_في_ملف", "ابحث_في_ملف(مسار، نص) ← أسطر تحوي النص"),
    ("ابحث_عن", "ابحث_عن(نص) أو (نص، مسار) ← ملفات تحوي النص"),
    ("اعرض_الشجرة", "اعرض_الشجرة(مسار) ← شجرة المجلدات والملفات"),
    ("حجم_المسار", "حجم_المسار(مسار) ← حجم الملف أو المجلد بالبايت"),
    ("أنشئ_رابط", "أنشئ_رابط(هدف، رابط) ← ينشئ رابطا رمزيا"),
    ("التاريخ_الآن", "التاريخ_الآن() ← نص التاريخ والوقت الحاليين"),
    ("النسخة", "النسخة() ← نص إصدار المحرك"),
    ("موجود", "موجود(مسار) ← منطقي: المسار موجود؟"),
    ("نوع_المسار", "نوع_المسار(مسار) ← نص: ملف أو مجلد أو غير موجود"),
    ("عد_الملفات", "عد_الملفات(مسار) ← عدد الملفات في المجلد"),
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
    ("فئة", "نوع: فئة."),
    ("واجهة", "نوع: واجهة."),
    ("كائن", "نوع: كائن من فئة."),
    ("أي", "نوع: أي قيمة، بلا فحص."),
];

const STDLIB: &[(&str, &str)] = &[
    ("رياضيات", "وحدة: جذر، قوة، جيب، جيب_تمام، ظل، لوغاريتم، أس_طبيعي، أرضية، سقف، باي، هـ"),
    ("عشوائي", "وحدة: عدد_عشوائي، صحيح_عشوائي، اختيار (للألعاب والمحاكاة فقط، ليست تشفيرا)"),
    ("وقت", "وحدة: الآن (ثواني Unix)، نوم (ثواني)، والتقويم: التاريخ (ثواني) ← قاموس سنة/شهر/يوم/ساعة/دقيقة/ثانية/يوم_الأسبوع/ثواني/عالمي، التاريخ_العالمي (ثواني) ← القاموس نفسه بتوقيت UTC، من_التاريخ (سنة، شهر، يوم، ساعة، دقيقة، ثانية) ← قاموس، من_التاريخ_العالمي (...) ← قاموس UTC، نص (قاموس) ← «٢٠٢٦-١١-٠٦ ١٩:٤٥:٣٠»، نص_التاريخ/نص_الوقت، اسم_اليوم/اسم_الشهر، فرق (قاموس، قاموس) ← ثواني، أضف (قاموس، ثواني) ← قاموس — المحلي من /etc/localtime ومرفوض صادقا على Windows وفي المتصفح"),
    ("شبكة", "وحدة: اجلب (رابط) ← نص صفحة — مغلقة افتراضيا، HTTP وHTTPS معا في النسخة الأصلية؛ اخدم (منفذ، شهادة، مفتاح، معالج) ← خادم HTTPS — مغلق افتراضيا بعلم --اسمح-بالخادم؛ والمقابس: اتصل (مضيف، منفذ) ← معرف مقبس TCP، مقبس (\"tcp\"/\"udp\") ← معرف، استمع (منفذ) ← معرف مستمع، أقبل (معرف) ← معرف متصل، اربط (معرف، منفذ)، أرسل/استقبل (معرف)، أرسل_إلى/استقبل_من (UDP)، أغلق (معرف)، المنفذ (معرف) — تحت بوابة الشبكة نفسها"),
    ("عمليات", "وحدة: أنشئ (أمر) ← معرف عملية، انتظر (معرف) ← رمز الخروج، أوقف (معرف)، الحالية () ← معرف العملية — مقيدة بمجلد البرنامج افتراضيا، بلا صدفة أبدا"),
    ("ذاكرة", "وحدة: احجز (حجم) ← معرف كتلة، حرّر (معرف)، المتاح () ← بايتات حرة، حجم_الصفحة () ← بايتات — لا مؤشر خام أبدا؛ والاستعلامان أصليان على لينكس ومرفوضان صادقا على غيره"),
    ("ملفات", "وحدة: افتح (مسار) ← معرف ملف، اقرأ (معرف) ← نص المحتوى كاملا، اكتب (معرف، نص) — الفحص عند الفتح بسياسة الجلسة نفسها؛ والقراءة بسقف 5 ميغابايت والكتابة بحصة 100 ميغابايت؛ وفي المتصفح مرفوضة"),
    ("نظام", "وحدة: اسم_النداء (رقم) ← اسم النداء في جدول هذه المعمارية، عدد_النداءات () ← عدد النداءات (386 على x86-64، 324 على aarch64)، نسخة_النواة () ← إصدار النواة، اسم_النظام () ← اسم النظام، معرف_المستخدم () / معرف_المجموعة () — قراءة فقط، بلا نداء خام أبدا؛ والدوال الست أصلية على لينكس ومرفوضة صادقا على غيره؛ وفي المتصفح مرفوضة"),
    ("تواصل", "وحدة: أنبوب (أمر) ← معرف قناة إلى برنامج فرعي بأنبوبي إدخال وإخراج، أرسل (معرف، نص) ← البايتات المكتوبة (سطر واحد بحد 64 كيبيبايت)، استقبل (معرف) ← سطر واحد بلا السطر الجديد (مهلة 30 ثانية)، أغلق (معرف) ← رمز الخروج، أوقف (معرف) — سياسة الإنشاء هي سياسة عمليات.أنشئ نفسها؛ وأقصى 64 قناة حية؛ وفي المتصفح مرفوضة، والإنشاء مرفوض في التطبيق"),
    ("تشفير", "وحدة: جزّئ (نص) ← بصمة SHA-256 (64 حرفا سداسية عشرية صغيرة)، عشوائي_آمن (عدد) ← قائمة أعداد صحيحة (0..255) بايتات عشوائية آمنة من النظام (بحد أقصى 1048576 بايتا) — التجزئة تعمل في كل مكان بما فيه المتصفح، والعشوائية من /dev/urandom على يونكس ومرفوضة صادقا على Windows وفي المتصفح"),
    ("أمن", "وحدة: قدرات_العملية () ← قاموس المجموعات الأربع للقدرات (فعالة، مسموحة، مقيدة، موروثة) — كل منها قائمة أسماء، الوضع () ← قائمة أسماء وحدات الأمن المفعّلة، تحقق (مسار) ← قاموس {قراءة، كتابة، تنفيذ} لملف داخل مجلد البرنامج — قراءة فقط، بلا إسقاط قدرة في v1؛ والدوال الثلاث أصلية على لينكس ومرفوضة صادقا على غيره؛ وفي المتصفح مرفوضة"),
    ("غير_متزامن", "وحدة: أنشئ_حلقة () ← معرف الحلقة، اقرأ (حلقة، مسار، موضع) ← معرف العملية (قراءة ملف من إزاحة بلا انتظار)، اكتب (حلقة، مسار، نص) ← معرف العملية (كتابة ملف كاملة بلا انتظار)، أرسل (حلقة، معرف_مقبس، نص) ← معرف العملية، استقبل (حلقة، معرف_مقبس) ← معرف العملية، انتظر_النتائج (حلقة) ← قائمة قواميس {المعرف، النوع، المحتوى} للنجاح و{المعرف، النوع، خطأ} للفشل — الطلب يرجع فورا بلا حجب؛ والمسارات عبر سياسة الملفات نفسها والمقابس عبر بوابة الشبكة نفسها؛ وتعمل على كل المنصات الأصلية، وفي المتصفح مرفوضة"),
    ("جهاز", "وحدة: حوار (نوع، عنوان[, خيارات|حدود]) ← قيمة الإجابة — حوار تفاعلي: تأكيد ← منطقي، نص ← نص، رقم ← عدد، اختيار واحد ← نص الخيار، اختيارات ← قائمة نصوص، قائمة ← نص الخيار الملموس، عداد ← عدد صحيح (قاموس {\"أدنى\"، \"أقصى\"})، تاريخ ← قاموس {سنة، شهر، يوم}، وقت ← قاموس {ساعة، دقيقة} — يعمل على أندرويد داخل التطبيق فقط؛ وخارج أندرويد مرفوض بخطأ عربي واضح؛ والإلغاء يرفع «ألغى المستخدم الحوار»؛ و«صوتي» مؤجل في v1؛ ونبّه (عنوان، نص) ← معرف التنبيه (عدد صحيح رتيب من 1، سقف 64 حيّا)، أزل_تنبيه (معرف) ← لاشيء — إشعار أندرويد؛ والقناة تُنشأ تلقائيا من التطبيق؛ وإذن الإشعارات (أندرويد 13+) يُطلب عند أول نبّه بلا إذن، والرفض يرفع خطأ عربيا؛ والمنشور يعيش بعد البرنامج حتى يُمسح أو يُزال"),
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
        source: Some("نيفا".to_string()),
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

/// ترميز URI للنشر: العملاء الصحيحون يرسلون URI
/// مرمّزا (VS Code يرمّز العربية)، لكن عميلا يرسل
/// المسار خاما (file:///tmp/test.نيفا) كان يُسقط
/// الخادم كاملا — تحليل Uri يرفض غير ASCII، والخطأ
/// كان يصعد بـ `?` حتى main فيخرج الخادم. نرمّز كل
/// بايت غير ASCII والفراغ قبل التحليل، فتصل التشخيصات
/// دائما؛ وإن بقي التحليل مستحيلا، نتخطى هذا الإشعار
/// فقط ولا نُسقط الخادم أبدا.
fn encode_uri(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b >= 0x80 || b == b' ' {
            out.push_str(&format!("%{:02X}", b));
        } else {
            out.push(b as char);
        }
    }
    out
}

fn publish(connection: &Connection, uri: &str, text: &str) -> Result<(), Box<dyn Error>> {
    let parsed: Uri = match encode_uri(uri).parse() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("تخطي نشر التشخيصات: URI غير صالح {}: {}", uri, e);
            return Ok(());
        }
    };
    let params = PublishDiagnosticsParams {
        uri: parsed,
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

/// استخراج URI نصيا خاما من معاملات JSON — بلا تحليل
/// Uri الصارم (يرفض غير ASCII: عميل يرسل مسارا خاما
/// مثل file:///tmp/test.نيفا كان يُسقط الخادم كاملا
/// عند فكّ المعاملات). الترميز يتم عند النشر فقط
/// (encode_uri). غياب المفتاح يعيد None.
fn raw_uri(params: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let mut v = params;
    for k in keys {
        v = v.get(*k)?;
    }
    v.as_str().map(|s| s.to_string())
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
                        // HoverParams مسطّح (serde flatten):
                        // textDocument و position في أعلى المعاملات.
                        let uri = raw_uri(&req.params, &["textDocument", "uri"])
                            .ok_or("hover بلا URI")?;
                        let pos: Position =
                            serde_json::from_value(req.params["position"].clone())?;
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
                        let uri = raw_uri(&req.params, &["textDocument", "uri"])
                            .ok_or("رموز بلا URI")?;
                        let symbols = docs
                            .get(&uri)
                            .map(|text| symbols_for(text))
                            .unwrap_or_default();
                        Ok(serde_json::to_value(symbols)?)
                    }
                    "textDocument/completion" => {
                        let dot = req
                            .params
                            .get("context")
                            .and_then(|c| c.get("triggerCharacter"))
                            .and_then(|t| t.as_str())
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
                // رسالة واحدة سيئة لا تُسقط الخادم أبدا:
                // تجاهلها وسجّلها، وتابع الرسالة التالية.
                "textDocument/didOpen" => {
                    let uri = match raw_uri(&not.params, &["textDocument", "uri"]) {
                        Some(u) => u,
                        None => {
                            eprintln!("تجاهل didOpen بلا URI");
                            continue;
                        }
                    };
                    let text = match not.params["textDocument"]["text"].as_str() {
                        Some(t) => t.to_string(),
                        None => {
                            eprintln!("تجاهل didOpen بلا نص");
                            continue;
                        }
                    };
                    docs.insert(uri.clone(), text.clone());
                    if let Err(e) = publish(&connection, &uri, &text) {
                        eprintln!("نشر التشخيصات فشل: {}", e);
                    }
                }
                "textDocument/didChange" => {
                    let uri = match raw_uri(&not.params, &["textDocument", "uri"]) {
                        Some(u) => u,
                        None => {
                            eprintln!("تجاهل didChange بلا URI");
                            continue;
                        }
                    };
                    if let Some(text) = not.params["contentChanges"]
                        .as_array()
                        .and_then(|a| a.last())
                        .and_then(|c| c.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        let text = text.to_string();
                        docs.insert(uri.clone(), text.clone());
                        if let Err(e) = publish(&connection, &uri, &text) {
                            eprintln!("نشر التشخيصات فشل: {}", e);
                        }
                    }
                }
                "textDocument/didClose" => {
                    let uri = match raw_uri(&not.params, &["textDocument", "uri"]) {
                        Some(u) => u,
                        None => {
                            eprintln!("تجاهل didClose بلا URI");
                            continue;
                        }
                    };
                    docs.remove(&uri);
                    if let Err(e) = publish(&connection, &uri, "") {
                        eprintln!("نشر التشخيصات فشل: {}", e);
                    }
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
