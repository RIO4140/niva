//! جهاز — حوار تفاعلي (المهمة 20 من خطة البناء،
//! أول قدرات واجهات الجهاز — المرحلة 4).
//!
//! المبدأ: المحرك عملية فرعية في التطبيق، فيطلب
//! الحوار عبر قناة التحكم المنظمة على stderr (نمط
//! م-6: `\u{0001}نيفا-تحكم:…` حين يكون
//! `NIVA_CONTROL_CHANNEL=stderr` مضبوطا) وينتظر
//! النتيجة سطرا منظما على stdin. لا واجهة في
//! المحرك أبدا — المحرك يطلب فقط، والتطبيق يعرض
//! ويعيد. وخارج أندرويد لا حوار إطلاقا (خطأ عربي
//! واضح)، فلا سطر تحكم يُرسَل ولا انتظار يُدخَل.
//!
//! التصميم الكامل — بما فيه بروتوكول الطلب/النتيجة
//! وأنواع الحوار التسعة في v1 وتأجيل «صوتي» وبوابة
//! الإذن وترتيب الفحوص — في CATALOG-12-DEVICE-DIALOG.md.

use crate::error::{RioError, RioResult};
use crate::value::Value;

#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// بادئة سطر طلب الحوار على stderr (نمط م-6) —
/// أندرويد فقط (لا مرسِل لها في غيره).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:حوار:";
/// بادئة سطر نتيجة الحوار على stdin — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const RESULT_PREFIX: &str = "\u{0001}نيفا-نتيجة:";

/// طلب حوار متحقق منه (خالص — يعمل على كل المنصات).
#[derive(Debug, Clone, PartialEq)]
pub struct DialogRequest {
    /// نوع الحوار: تأكيد، نص، رقم، اختيار واحد،
    /// اختيارات، قائمة، عداد، تاريخ، وقت.
    pub kind: String,
    /// سؤال الحوار (غير فارغ أبدا).
    pub title: String,
    /// خيارات أنواع الاختيار الثلاثة فقط.
    pub options: Option<Vec<String>>,
    /// حدّا العداد فقط.
    pub min: Option<i64>,
    /// حدّا العداد فقط.
    pub max: Option<i64>,
    /// افتراضي العداد فقط (إن أُعطي).
    pub default: Option<i64>,
}

fn typ_error(display: &str, what: &str, line: usize, col: usize) -> RioError {
    RioError::typ(format!("الدالة {} {}", display, what), line, col)
}

/// التحقق الخالص من وسائط `جهاز.حوار` — يعمل على كل
/// المنصات (نمط `تشفير`: أخطاء الوسائط قبل فحص المنصة).
pub fn validate_dialog_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<DialogRequest> {
    const DISPLAY: &str = "جهاز.حوار";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 أو 3 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let kind = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لنوع الحوار لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    let title = match &args[1] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لعنوان الحوار لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if title.is_empty() {
        return Err(RioError::runtime("عنوان الحوار فارغ", line, col));
    }
    match kind.as_str() {
        // الأنواع ذات المعاملين فقط.
        "تأكيد" | "نص" | "رقم" | "تاريخ" | "وقت" => {
            if args.len() != 2 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 2 معاملات لنوع {} لكن أعطيت {}",
                        DISPLAY,
                        kind,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            Ok(DialogRequest {
                kind,
                title,
                options: None,
                min: None,
                max: None,
                default: None,
            })
        }
        // «صوتي» نوع معروف مؤجل (لا مجهول): يحتاج إذن
        // ميكروفون وقت التشغيل + تعرفا صوتيا.
        "صوتي" => Err(RioError::runtime(
            "الحوار الصوتي غير متاح بعد — يحتاج إذن ميكروفون وتعرفا صوتيا",
            line,
            col,
        )),
        // أنواع الاختيار الثلاثة: قائمة نصوص ثالثة.
        "اختيار واحد" | "اختيارات" | "قائمة" => {
            if args.len() != 3 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 3 معاملات لنوع {} لكن أعطيت {}",
                        DISPLAY,
                        kind,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            let options = match &args[2] {
                Value::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            Value::Str(s) => out.push(s.clone()),
                            other => {
                                return Err(typ_error(
                                    DISPLAY,
                                    &format!(
                                        "تتوقع نصوصا في قائمة الخيارات لكن أعطيت {}",
                                        other.type_name()
                                    ),
                                    line,
                                    col,
                                ))
                            }
                        }
                    }
                    out
                }
                other => {
                    return Err(typ_error(
                        DISPLAY,
                        &format!(
                            "تتوقع قائمة خيارات لكن أعطيت {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    ))
                }
            };
            if options.is_empty() {
                return Err(RioError::runtime("قائمة الخيارات فارغة", line, col));
            }
            Ok(DialogRequest {
                kind,
                title,
                options: Some(options),
                min: None,
                max: None,
                default: None,
            })
        }
        // العداد: قاموس حدود ثالث.
        "عداد" => {
            if args.len() != 3 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 3 معاملات لنوع عداد لكن أعطيت {}",
                        DISPLAY,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            let dict = match &args[2] {
                Value::Dict(pairs) => pairs,
                other => {
                    return Err(typ_error(
                        DISPLAY,
                        &format!(
                            "تتوقع قاموس حدود للعداد لكن أعطيت {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    ))
                }
            };
            let mut min: Option<i64> = None;
            let mut max: Option<i64> = None;
            let mut default: Option<i64> = None;
            for (key, value) in dict {
                let num = match value {
                    Value::Number(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => *x as i64,
                    _ => {
                        return Err(typ_error(
                            DISPLAY,
                            &format!("تتوقع عددا صحيحا لقيمة «{}» في قاموس العداد", key),
                            line,
                            col,
                        ))
                    }
                };
                match key.as_str() {
                    "أدنى" => min = Some(num),
                    "أقصى" => max = Some(num),
                    "افتراضي" => default = Some(num),
                    _ => {
                        return Err(RioError::runtime(
                            format!("مفتاح غير معروف في قاموس العداد: {}", key),
                            line,
                            col,
                        ))
                    }
                }
            }
            let min = match min {
                Some(v) => v,
                None => {
                    return Err(RioError::runtime(
                        "قاموس العداد يفتقد «أدنى»",
                        line,
                        col,
                    ))
                }
            };
            let max = match max {
                Some(v) => v,
                None => {
                    return Err(RioError::runtime(
                        "قاموس العداد يفتقد «أقصى»",
                        line,
                        col,
                    ))
                }
            };
            if min > max {
                return Err(RioError::runtime(
                    "الحد الأدنى أكبر من الحد الأقصى",
                    line,
                    col,
                ));
            }
            if let Some(d) = default {
                if d < min || d > max {
                    return Err(RioError::runtime(
                        "القيمة الافتراضية خارج النطاق",
                        line,
                        col,
                    ));
                }
            }
            Ok(DialogRequest {
                kind,
                title,
                options: None,
                min: Some(min),
                max: Some(max),
                default,
            })
        }
        _ => Err(RioError::runtime(
            format!("نوع حوار غير معروف: {}", kind),
            line,
            col,
        )),
    }
}

/// نقطة الدخول: `جهاز.حوار(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم الطلب والانتظار.
pub fn show_dialog(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let req = validate_dialog_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        // التحقق جرى أعلاه (أخطاء الوسائط تعمل في
        // المتصفح)؛ والرفض هنا لا يحتاج الطلب نفسه.
        let _ = &req;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        show_dialog_native(&req, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn show_dialog_native(req: &DialogRequest, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = req;
        Err(RioError::runtime(
            "جهاز.حوار غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            request_and_wait(req, line, col)
        } else {
            Err(RioError::runtime("الحوار متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// أندرويد فقط: إرسال الطلب على stderr ثم انتظار سطر
/// النتيجة على stdin (آلية `ادخل` نفسها).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
fn request_and_wait(req: &DialogRequest, line: usize, col: usize) -> RioResult<Value> {
    eprintln!("{}{}", CONTROL_PREFIX, serialize_request(req));
    let stdin = std::io::stdin();
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = stdin
            .read_line(&mut buf)
            .map_err(|_| RioError::runtime("تعذر قراءة نتيجة الحوار", line, col))?;
        if n == 0 {
            return Err(RioError::runtime(
                "انتهى الإدخال أثناء انتظار نتيجة الحوار",
                line,
                col,
            ));
        }
        let text = buf.trim_end_matches(['\n', '\r']);
        if let Some(payload) = text.strip_prefix(RESULT_PREFIX) {
            return parse_dialog_result(payload, line, col);
        }
        // سطر آخر أثناء الحوار — يُتجاهل (لا يصل عمليا:
        // الحوار نمطي في التطبيق فلا كتابة في الترمنال
        // أثناءه — والتجاهل دفاع فقط).
    }
}

/// تسلسل طلب الحوار إلى JSON (تهريب يدوي — المحرك
/// بلا serde ولا تبعية جديدة).
pub fn serialize_request(req: &DialogRequest) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نوع\":");
    out.push_str(&json_escape(&req.kind));
    out.push_str(",\"عنوان\":");
    out.push_str(&json_escape(&req.title));
    if let Some(options) = &req.options {
        out.push_str(",\"خيارات\":[");
        for (i, opt) in options.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_escape(opt));
        }
        out.push(']');
    }
    if let Some(min) = req.min {
        out.push_str(&format!(",\"أدنى\":{}", min));
    }
    if let Some(max) = req.max {
        out.push_str(&format!(",\"أقصى\":{}", max));
    }
    if let Some(default) = req.default {
        out.push_str(&format!(",\"افتراضي\":{}", default));
    }
    out.push('}');
    out
}

/// تهريب نص إلى سلسلة JSON (مع علامتي التنصيص).
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// قيمة JSON (محلّل صغير مكتوب يدويا — std فقط).
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn new(text: &str) -> Self {
        JsonParser {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, what: char) -> Result<(), String> {
        match self.next() {
            Some(c) if c == what => Ok(()),
            _ => Err(format!("توقع «{}»", what)),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), String> {
        for expected in word.chars() {
            match self.next() {
                Some(c) if c == expected => {}
                _ => return Err(format!("توقع «{}»", word)),
            }
        }
        Ok(())
    }

    fn parse_value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(Json::Str(self.parse_string()?)),
            Some('t') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some('f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some('n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(format!("حرف غير متوقع «{}»", c)),
            None => Err("انتهى النص فجأة".to_string()),
        }
    }

    fn parse_object(&mut self) -> Result<Json, String> {
        self.expect('{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err("توقع مفتاح نصا".to_string());
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some('}') => break,
                _ => return Err("توقع «،» أو «}»".to_string()),
            }
        }
        Ok(Json::Obj(pairs))
    }

    fn parse_array(&mut self) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some(']') => break,
                _ => return Err("توقع «،» أو «]»".to_string()),
            }
        }
        Ok(Json::Arr(items))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err("سلسلة غير مغلقة".to_string()),
                Some('"') => break,
                Some('\\') => match self.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{0008}'),
                    Some('f') => out.push('\u{000C}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let first = self.parse_hex4()?;
                        if (0xD800..0xDC00).contains(&first) {
                            // زوج بديل: يليه \uXXXX للبديل المنخفض.
                            if self.next() != Some('\\') || self.next() != Some('u') {
                                return Err("بديل عالٍ بلا بديل منخفض".to_string());
                            }
                            let second = self.parse_hex4()?;
                            if !(0xDC00..0xE000).contains(&second) {
                                return Err("بديل منخفض غير صالح".to_string());
                            }
                            let code =
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                            match char::from_u32(code) {
                                Some(c) => out.push(c),
                                None => return Err("نقطة شفرة غير صالحة".to_string()),
                            }
                        } else if (0xDC00..0xE000).contains(&first) {
                            return Err("بديل منخفض بلا بديل عالٍ".to_string());
                        } else {
                            match char::from_u32(first) {
                                Some(c) => out.push(c),
                                None => return Err("نقطة شفرة غير صالحة".to_string()),
                            }
                        }
                    }
                    _ => return Err("تهريب غير صالح".to_string()),
                },
                Some(c) if (c as u32) < 0x20 => {
                    return Err("حرف تحكم عارٍ في سلسلة".to_string())
                }
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = match self.next() {
                Some(c) => c.to_digit(16),
                None => None,
            };
            match digit {
                Some(d) => value = value * 16 + d,
                None => return Err("رقم سداسي عشري غير صالح".to_string()),
            }
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err("عدد غير صالح".to_string()),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {}
                _ => return Err("عدد غير صالح".to_string()),
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {}
                _ => return Err("عدد غير صالح".to_string()),
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        match text.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Json::Num(n)),
            _ => Err("عدد غير صالح".to_string()),
        }
    }
}

/// تحليل نص JSON كاملا (بلا بقايا بعد القيمة).
pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut parser = JsonParser::new(text);
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err("بقايا بعد قيمة JSON".to_string());
    }
    Ok(value)
}

/// تحويل قيمة JSON إلى قيمة نيفا (null مرفوض: التطبيق
/// لا يرسله أبدا).
fn json_to_value(json: Json) -> Result<Value, String> {
    match json {
        Json::Null => Err("قيمة null غير متوقعة".to_string()),
        Json::Bool(b) => Ok(Value::Bool(b)),
        Json::Num(n) => Ok(Value::Number(n)),
        Json::Str(s) => Ok(Value::Str(s)),
        Json::Arr(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(item)?);
            }
            Ok(Value::List(out))
        }
        Json::Obj(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                out.push((key, json_to_value(value)?));
            }
            Ok(Value::Dict(out))
        }
    }
}

/// تحليل سطر نتيجة الحوار (الحمولة بعد البادئة) إلى
/// قيمة نيفا أو رفع خطأ عربي (إلغاء أو خطأ تطبيق أو
/// نتيجة مشوهة).
pub fn parse_dialog_result(payload: &str, line: usize, col: usize) -> RioResult<Value> {
    let invalid = || RioError::runtime("نتيجة حوار غير صالحة", line, col);
    let json = parse_json(payload).map_err(|_| invalid())?;
    let pairs = match json {
        Json::Obj(pairs) => pairs,
        _ => return Err(invalid()),
    };
    for (key, value) in &pairs {
        match key.as_str() {
            "قيمة" => return json_to_value(value.clone()).map_err(|_| invalid()),
            "ألغي" => {
                if *value == Json::Bool(true) {
                    return Err(RioError::runtime("ألغى المستخدم الحوار", line, col));
                }
                return Err(invalid());
            }
            "خطأ" => match value {
                Json::Str(text) => {
                    return Err(RioError::runtime(text.clone(), line, col));
                }
                _ => return Err(invalid()),
            },
            _ => return Err(invalid()),
        }
    }
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_val(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    fn num_val(n: f64) -> Value {
        Value::Number(n)
    }

    fn list_val(items: Vec<Value>) -> Value {
        Value::List(items)
    }

    fn dict_val(pairs: Vec<(&str, Value)>) -> Value {
        Value::Dict(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    fn req(kind: &str, title: &str) -> DialogRequest {
        DialogRequest {
            kind: kind.to_string(),
            title: title.to_string(),
            options: None,
            min: None,
            max: None,
            default: None,
        }
    }

    #[test]
    fn validate_two_arg_kinds_ok() {
        for kind in ["تأكيد", "نص", "رقم", "تاريخ", "وقت"] {
            let got = validate_dialog_args(&[str_val(kind), str_val("س؟")], 1, 1).unwrap();
            assert_eq!(got, req(kind, "س؟"));
        }
    }

    #[test]
    fn validate_wrong_arity() {
        let err = validate_dialog_args(&[str_val("تأكيد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 أو 3 معاملات"), "got: {}", err);
        let err = validate_dialog_args(
            &[str_val("تأكيد"), str_val("س؟"), str_val("زائد"), str_val("رابع")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("2 أو 3 معاملات"), "got: {}", err);
    }

    #[test]
    fn validate_non_string_args() {
        let err = validate_dialog_args(&[num_val(5.0), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لنوع الحوار"), "got: {}", err);
        let err = validate_dialog_args(&[str_val("نص"), Value::Bool(true)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لعنوان الحوار"), "got: {}", err);
    }

    #[test]
    fn validate_empty_title() {
        let err = validate_dialog_args(&[str_val("نص"), str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عنوان الحوار فارغ"), "got: {}", err);
    }

    #[test]
    fn validate_unknown_and_voice() {
        let err = validate_dialog_args(&[str_val("غريب"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نوع حوار غير معروف: غريب"), "got: {}", err);
        let err = validate_dialog_args(&[str_val("صوتي"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("الحوار الصوتي غير متاح بعد"),
            "got: {}",
            err
        );
    }

    #[test]
    fn validate_choice_options() {
        let ok = validate_dialog_args(
            &[
                str_val("اختيار واحد"),
                str_val("اختر"),
                list_val(vec![str_val("أ"), str_val("ب")]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(ok.options, Some(vec!["أ".to_string(), "ب".to_string()]));
        // بلا معامل ثالث.
        let err = validate_dialog_args(&[str_val("اختيار واحد"), str_val("اختر")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("3 معاملات"), "got: {}", err);
        // قائمة فارغة.
        let err = validate_dialog_args(
            &[str_val("قائمة"), str_val("اختر"), list_val(vec![])],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("قائمة الخيارات فارغة"), "got: {}", err);
        // عنصر غير نص.
        let err = validate_dialog_args(
            &[
                str_val("اختيارات"),
                str_val("اختر"),
                list_val(vec![str_val("أ"), num_val(1.0)]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("نصوصا في قائمة الخيارات"), "got: {}", err);
        // معامل ثالث زائد لنوع بمعاملين.
        let err = validate_dialog_args(
            &[str_val("نص"), str_val("س؟"), str_val("زائد")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("2 معاملات لنوع نص"), "got: {}", err);
    }

    #[test]
    fn validate_counter() {
        let ok = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(10.0)),
                    ("افتراضي", num_val(3.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!((ok.min, ok.max, ok.default), (Some(0), Some(10), Some(3)));
        // بلا افتراضي.
        let ok = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(1.0)), ("أقصى", num_val(5.0))]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(ok.default, None);
        // يفتقد أدنى.
        let err = validate_dialog_args(
            &[str_val("عداد"), str_val("كم؟"), dict_val(vec![("أقصى", num_val(5.0))])],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("يفتقد «أدنى»"), "got: {}", err);
        // مفتاح غير معروف.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(5.0)),
                    ("خطوة", num_val(1.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("مفتاح غير معروف في قاموس العداد: خطوة"), "got: {}", err);
        // أدنى أكبر من أقصى.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(9.0)), ("أقصى", num_val(2.0))]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("الحد الأدنى أكبر من الحد الأقصى"), "got: {}", err);
        // افتراضي خارج النطاق.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(5.0)),
                    ("افتراضي", num_val(9.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("القيمة الافتراضية خارج النطاق"), "got: {}", err);
        // حد كسري.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(0.5)), ("أقصى", num_val(5.0))]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
    }

    #[test]
    fn json_escape_round_trip() {
        let original = "قال \"مرحبا\"\nسطر جديد \\ شرطة";
        let escaped = json_escape(original);
        let parsed = parse_json(&escaped).unwrap();
        assert_eq!(parsed, Json::Str(original.to_string()));
        // عربي خالص يبقى حرفيا (بلا \u).
        assert_eq!(json_escape("محمود"), "\"محمود\"");
    }

    #[test]
    fn json_parse_values() {
        assert_eq!(parse_json("true").unwrap(), Json::Bool(true));
        assert_eq!(parse_json("false").unwrap(), Json::Bool(false));
        assert_eq!(parse_json("null").unwrap(), Json::Null);
        assert_eq!(parse_json("42").unwrap(), Json::Num(42.0));
        assert_eq!(parse_json("-3.5e2").unwrap(), Json::Num(-350.0));
        assert_eq!(
            parse_json("[\"أ\", ١]").unwrap_err(),
            "حرف غير متوقع «١»".to_string()
        );
        assert_eq!(
            parse_json("[\"أ\", 1]").unwrap(),
            Json::Arr(vec![Json::Str("أ".to_string()), Json::Num(1.0)])
        );
        assert_eq!(
            parse_json("{\"سنة\": 2026, \"شهر\": 11}").unwrap(),
            Json::Obj(vec![
                ("سنة".to_string(), Json::Num(2026.0)),
                ("شهر".to_string(), Json::Num(11.0)),
            ])
        );
        // تهريب \u + زوج بديل (وجه مبتسم).
        assert_eq!(
            parse_json("\"\\u0645\\u062d\\u0645\\u0648\\u062f\"").unwrap(),
            Json::Str("محمود".to_string())
        );
        assert_eq!(
            parse_json("\"\\uD83D\\uDE00\"").unwrap(),
            Json::Str("😀".to_string())
        );
        // مرفوضات.
        assert!(parse_json("{\"أ\": 1,}").is_err());
        assert!(parse_json("[1, 2").is_err());
        assert!(parse_json("\"غير مغلقة").is_err());
        assert!(parse_json("01").is_err());
        assert!(parse_json("true false").is_err());
        assert!(parse_json("nul").is_err());
    }

    #[test]
    fn serialize_request_round_trip() {
        let mut r = req("اختيار واحد", "اختر لونا");
        r.options = Some(vec!["أحمر".to_string(), "أزرق \"غامق\"".to_string()]);
        let text = serialize_request(&r);
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![
                ("نوع".to_string(), Json::Str("اختيار واحد".to_string())),
                ("عنوان".to_string(), Json::Str("اختر لونا".to_string())),
                (
                    "خيارات".to_string(),
                    Json::Arr(vec![
                        Json::Str("أحمر".to_string()),
                        Json::Str("أزرق \"غامق\"".to_string()),
                    ])
                ),
            ])
        );
        let mut c = req("عداد", "كم؟");
        c.min = Some(-5);
        c.max = Some(10);
        c.default = Some(3);
        let text = serialize_request(&c);
        assert!(text.contains("\"أدنى\":-5"), "got: {}", text);
        assert!(text.contains("\"أقصى\":10"), "got: {}", text);
        assert!(text.contains("\"افتراضي\":3"), "got: {}", text);
    }

    #[test]
    fn parse_result_value_kinds() {
        // منطقي.
        assert_eq!(
            parse_dialog_result("{\"قيمة\": true}", 1, 1).unwrap(),
            Value::Bool(true)
        );
        // عدد.
        assert_eq!(
            parse_dialog_result("{\"قيمة\": 42}", 1, 1).unwrap(),
            Value::Number(42.0)
        );
        // نص عربي.
        assert_eq!(
            parse_dialog_result("{\"قيمة\": \"محمود\"}", 1, 1).unwrap(),
            Value::Str("محمود".to_string())
        );
        // قائمة نصوص.
        assert_eq!(
            parse_dialog_result("{\"قيمة\": [\"أ\", \"ب\"]}", 1, 1).unwrap(),
            Value::List(vec![Value::Str("أ".to_string()), Value::Str("ب".to_string())])
        );
        // قاموس تاريخ.
        assert_eq!(
            parse_dialog_result("{\"قيمة\": {\"سنة\": 2026, \"شهر\": 11, \"يوم\": 6}}", 1, 1)
                .unwrap(),
            Value::Dict(vec![
                ("سنة".to_string(), Value::Number(2026.0)),
                ("شهر".to_string(), Value::Number(11.0)),
                ("يوم".to_string(), Value::Number(6.0)),
            ])
        );
    }

    #[test]
    fn parse_result_cancel_error_invalid() {
        let err = parse_dialog_result("{\"ألغي\": true}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ألغى المستخدم الحوار"), "got: {}", err);
        let err = parse_dialog_result("{\"خطأ\": \"أدخل رقما\"}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("أدخل رقما"), "got: {}", err);
        // مشوه.
        let err = parse_dialog_result("ليست json", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة حوار غير صالحة"), "got: {}", err);
        // مفتاح غير معروف.
        let err = parse_dialog_result("{\"غريب\": 1}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة حوار غير صالحة"), "got: {}", err);
        // قيمة null مرفوضة.
        let err = parse_dialog_result("{\"قيمة\": null}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة حوار غير صالحة"), "got: {}", err);
    }

    #[test]
    fn show_dialog_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = show_dialog(&[str_val("تأكيد"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("غير مدعوم على هذه المنصة"), "got: {}", err);
        // والوسائط الخاطئة ترفض قبل المنصة (نمط تشفير).
        let err = show_dialog(&[str_val("غريب"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نوع حوار غير معروف"), "got: {}", err);
    }
}
