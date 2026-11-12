//! مصنوعات AOT — مهمة 8 (ك-2.2): ترميز برنامج مترجم
//! (CompiledProgram) إلى بايتات محمولة وفكّها.
//!
//! ترميز يدوي ثنائي (little-endian)، بلا أي اعتماد
//! خارجي: لا serde ولا غيره — المحرك بلا اعتماديات.
//! الصيغة:
//!   [سحر «نيفا» (8 بايت UTF-8)] [إصدار u32 = 1]
//!   [كود: عدد التعليمات u64 + تعليمة...]
//!   [دوال: عدد u64 + (اسم، تعريف)... مرتبة بالاسم]
//!   [إغلاقات: عدد u64 + (معرّف u64، تعريف)... مرتبة بالمعرّف]
//! كل سلسلة = طول u32 + بايتات UTF-8؛ وكل قائمة =
//! عدد u64 + عناصر؛ وكل Option = وسم u8 (0/1) + قيمة.
//! ترتيب مدخلات القواميس مرتب بالمفتاح، فيكون المصنوع
//! حتميا: الترميز مرتين يعطي البايتات نفسها.
//!
//! أخطاء الفك كلها عربية: ملف ليس مصنوعا، أو إصدار
//! غير مدعوم، أو بايتات مقطوعة/زائدة، أو وسم مجهول.
//! main.rs يطبع رسالتها وحدها (بلا «السطر 0»).

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, TypeParam, UnOp};
use crate::error::{ErrorKind, RioError, RioResult};
use crate::value::Value;
use crate::vm::{BoolCtx, ClosureDef, CompiledProgram, FunctionDef, Instr, Op, VmClassDef};
use std::collections::HashMap;

/// سحر المصنوع: بايتات «نيفا» UTF-8.
pub const ARTIFACT_MAGIC: &[u8] = "نيفا".as_bytes();
/// إصدار صيغة المصنوع الحالي.
pub const ARTIFACT_VERSION: u32 = 3;
/// امتداد ملف المصنوع.
pub const ARTIFACT_EXT: &str = "نيفاب";
/// سحر ذيل الملف التنفيذي المستقل.
pub const STANDALONE_MAGIC: &[u8] = "نيفا-مستقل".as_bytes();

fn bad(message: String) -> RioError {
    RioError::new(ErrorKind::Io, message, 0, 0)
}

/// كاتب ثنائي صغير.
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }
    fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.u8(if v { 1 } else { 0 });
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    fn opt_str(&mut self, v: &Option<String>) {
        match v {
            None => self.u8(0),
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
        }
    }
    fn opt_usize(&mut self, v: &Option<usize>) {
        match v {
            None => self.u8(0),
            Some(n) => {
                self.u8(1);
                self.usize(*n);
            }
        }
    }
    fn vec_str(&mut self, v: &[String]) {
        self.u64(v.len() as u64);
        for s in v {
            self.str(s);
        }
    }
    fn opt_vec_str(&mut self, v: &Option<Vec<String>>) {
        match v {
            None => self.u8(0),
            Some(list) => {
                self.u8(1);
                self.vec_str(list);
            }
        }
    }
}

/// قارئ ثنائي صغير — كل قراءة تتحقق من الحدود.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn need(&self, n: usize) -> RioResult<()> {
        if self.pos + n > self.buf.len() {
            return Err(bad("مصنوع تالف: بايتات مقطوعة".to_string()));
        }
        Ok(())
    }
    fn u8(&mut self) -> RioResult<u8> {
        self.need(1)?;
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn u32(&mut self) -> RioResult<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
    fn u64(&mut self) -> RioResult<u64> {
        self.need(8)?;
        let v = u64::from_le_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }
    fn usize(&mut self) -> RioResult<usize> {
        Ok(self.u64()? as usize)
    }
    fn f64(&mut self) -> RioResult<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn bool(&mut self) -> RioResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(bad("مصنوع تالف: قيمة منطقية غير صالحة".to_string())),
        }
    }
    fn str(&mut self) -> RioResult<String> {
        let len = self.u32()? as usize;
        self.need(len)?;
        let s = std::str::from_utf8(&self.buf[self.pos..self.pos + len])
            .map_err(|_| bad("مصنوع تالف: نص ليس UTF-8 صالحا".to_string()))?;
        self.pos += len;
        Ok(s.to_string())
    }
    fn bytes(&mut self, n: usize) -> RioResult<&'a [u8]> {
        self.need(n)?;
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn opt_str(&mut self) -> RioResult<Option<String>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.str()?)),
            _ => Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
        }
    }
    fn opt_usize(&mut self) -> RioResult<Option<usize>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            _ => Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
        }
    }
    fn vec_str(&mut self) -> RioResult<Vec<String>> {
        let n = self.u64()? as usize;
        let mut v = Vec::with_capacity(n.min(1_000_000));
        for _ in 0..n {
            v.push(self.str()?);
        }
        Ok(v)
    }
    fn opt_vec_str(&mut self) -> RioResult<Option<Vec<String>>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.vec_str()?)),
            _ => Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
        }
    }
    fn expect_end(&self) -> RioResult<()> {
        if self.pos != self.buf.len() {
            return Err(bad("مصنوع تالف: بايتات زائدة في النهاية".to_string()));
        }
        Ok(())
    }
}

/// رمّز برنامجا مترجما إلى بايتات مصنوع.
pub fn encode_program(prog: &CompiledProgram) -> RioResult<Vec<u8>> {
    let mut w = Writer::new();
    w.bytes(ARTIFACT_MAGIC);
    w.u32(ARTIFACT_VERSION);
    enc_code(&mut w, &prog.code)?;
    // القواميس مرتبة بالمفتاح — مصنوع حتمي.
    let mut fns: Vec<(&String, &FunctionDef)> = prog.functions.iter().collect();
    fns.sort_by(|a, b| a.0.cmp(b.0));
    w.u64(fns.len() as u64);
    for (name, def) in fns {
        w.str(name);
        enc_function_def(&mut w, def)?;
    }
    let mut cls: Vec<(&u64, &FunctionDef)> = prog.closures.iter().collect();
    cls.sort_by(|a, b| a.0.cmp(b.0));
    w.u64(cls.len() as u64);
    for (id, def) in cls {
        w.u64(*id);
        enc_function_def(&mut w, def)?;
    }
    Ok(w.buf)
}

/// فكّ بايتات مصنوع إلى برنامج مترجم.
pub fn decode_program(bytes: &[u8]) -> RioResult<CompiledProgram> {
    let mut r = Reader::new(bytes);
    let magic = r.bytes(ARTIFACT_MAGIC.len())?;
    if magic != ARTIFACT_MAGIC {
        return Err(bad(
            "الملف ليس مصنوع بايت كود نيفا — ابنِه أولا بـ: نيفا ابنِ برنامج.نيفا".to_string(),
        ));
    }
    let version = r.u32()?;
    if version != ARTIFACT_VERSION {
        return Err(bad(format!(
            "إصدار مصنوع غير مدعوم: {} — أعد بناءه بنسخة أحدث من نيفا",
            version
        )));
    }
    let code = dec_code(&mut r)?;
    let n_fns = r.u64()? as usize;
    let mut functions = HashMap::new();
    for _ in 0..n_fns {
        let name = r.str()?;
        let def = dec_function_def(&mut r)?;
        functions.insert(name, def);
    }
    let n_cls = r.u64()? as usize;
    let mut closures = HashMap::new();
    for _ in 0..n_cls {
        let id = r.u64()?;
        let def = dec_function_def(&mut r)?;
        closures.insert(id, def);
    }
    r.expect_end()?;
    Ok(CompiledProgram { code, functions, closures })
}

/// أكبر معرّف إغلاق في برنامج مترجم — لرفع عدّاد
/// المعرّفات بعد تحميل مصنوع مبني في عملية أخرى
/// (انظر reserve_closure_ids_above في vm.rs).
pub fn max_closure_id(prog: &CompiledProgram) -> u64 {
    let mut m: u64 = 0;
    for id in prog.closures.keys() {
        m = m.max(*id);
    }
    let mut walk_code = |code: &[Instr]| {
        for instr in code {
            match &instr.op {
                Op::MakeClosure(def) | Op::DefineClosure(def) => {
                    m = m.max(def.id);
                }
                Op::DefineClass(class) => {
                    for id in class.own_method_ids.values() {
                        m = m.max(*id);
                    }
                }
                _ => {}
            }
        }
    };
    walk_code(&prog.code);
    for def in prog.functions.values() {
        walk_code(&def.code);
    }
    for def in prog.closures.values() {
        walk_code(&def.code);
    }
    m
}

/// غلّف بايتات تنفيذية بمصنوع في ذيلها — لصنع الملف
/// التنفيذي المستقل: [تنفيذية][مصنوع][طول u64][سحر].
pub fn wrap_standalone(exe_bytes: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(exe_bytes.len() + payload.len() + 8 + STANDALONE_MAGIC.len());
    out.extend_from_slice(exe_bytes);
    out.extend_from_slice(payload);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(STANDALONE_MAGIC);
    out
}

/// استخرج مصنوعا من ذيل ملف، إن وُجد (الملف التنفيذي
/// المستقل) — بلا قراءة الملف كله، بل قفز إلى الذيل.
pub fn extract_standalone_payload(exe_path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(exe_path).ok()?;
    let len = f.metadata().ok()?.len();
    let magic_len = STANDALONE_MAGIC.len() as u64;
    let tail_len = magic_len + 8;
    if len < tail_len {
        return None;
    }
    f.seek(SeekFrom::End(-(tail_len as i64))).ok()?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).ok()?;
    let (len_bytes, magic_bytes) = tail.split_at(8);
    if magic_bytes != STANDALONE_MAGIC {
        return None;
    }
    let payload_len = u64::from_le_bytes(len_bytes.try_into().ok()?);
    if payload_len > len - tail_len {
        return None;
    }
    f.seek(SeekFrom::End(-((tail_len + payload_len) as i64)))
        .ok()?;
    let mut payload = vec![0u8; payload_len as usize];
    f.read_exact(&mut payload).ok()?;
    Some(payload)
}

fn enc_code(w: &mut Writer, code: &[Instr]) -> RioResult<()> {
    w.u64(code.len() as u64);
    for instr in code {
        enc_op(w, &instr.op)?;
        w.u64(instr.line as u64);
        w.u64(instr.col as u64);
    }
    Ok(())
}

fn dec_code(r: &mut Reader) -> RioResult<Vec<Instr>> {
    let n = r.u64()? as usize;
    let mut code = Vec::with_capacity(n.min(1_000_000));
    for _ in 0..n {
        let op = dec_op(r)?;
        let line = r.u64()? as usize;
        let col = r.u64()? as usize;
        code.push(Instr { op, line, col });
    }
    Ok(code)
}

fn enc_function_def(w: &mut Writer, def: &FunctionDef) -> RioResult<()> {
    w.vec_str(&def.params);
    w.u64(def.param_types.len() as u64);
    for t in &def.param_types {
        w.opt_str(t);
    }
    w.opt_str(&def.return_type);
    // الدفعة الأخيرة DEF (البنود 109–112 و77 و78، الإصدار 2):
    // افتراضيات وجامعان وإعلانات نطاق.
    w.u64(def.defaults.len() as u64);
    for d in &def.defaults {
        match d {
            None => w.u8(0),
            Some(e) => {
                w.u8(1);
                enc_expr(w, e)?;
            }
        }
    }
    w.opt_str(&def.vararg);
    w.opt_str(&def.kwarg);
    w.vec_str(&def.global_names);
    w.vec_str(&def.nonlocal_names);
    enc_code(w, &def.code)?;
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    enc_type_params(w, &def.type_params);
    Ok(())
}

fn dec_function_def(r: &mut Reader) -> RioResult<FunctionDef> {
    let params = r.vec_str()?;
    let n_types = r.u64()? as usize;
    let mut param_types = Vec::with_capacity(n_types.min(1_000_000));
    for _ in 0..n_types {
        param_types.push(r.opt_str()?);
    }
    let return_type = r.opt_str()?;
    // الدفعة الأخيرة DEF (الإصدار 2).
    let n_defaults = r.u64()? as usize;
    let mut defaults = Vec::with_capacity(n_defaults.min(1_000_000));
    for _ in 0..n_defaults {
        match r.u8()? {
            0 => defaults.push(None),
            1 => defaults.push(Some(dec_expr(r)?)),
            _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
        }
    }
    let vararg = r.opt_str()?;
    let kwarg = r.opt_str()?;
    let global_names = r.vec_str()?;
    let nonlocal_names = r.vec_str()?;
    let code = dec_code(r)?;
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    let type_params = dec_type_params(r)?;
    // ب6: المصنوع القديم بلا حقل مولد — يُستنتج من الكود:
    // استدعاء `__ولد`/`__ولد_من` في تعليمات الدالة نفسها
    // (بلا نزول إلى أجسام إغلاقات متداخلة، كـ body_has_yield).
    let is_generator = code.iter().any(|i| {
        matches!(&i.op, Op::Call { name, .. } | Op::Builtin { name, .. }
            if name == crate::ast::YIELD_CALL || name == crate::ast::YIELD_FROM_CALL)
    });
    Ok(FunctionDef { params, param_types, return_type, code, is_generator, defaults, vararg, kwarg, global_names, nonlocal_names, type_params })
}

fn enc_closure_def(w: &mut Writer, def: &ClosureDef) -> RioResult<()> {
    w.u64(def.id);
    w.str(&def.name);
    w.vec_str(&def.params);
    w.u64(def.param_types.len() as u64);
    for t in &def.param_types {
        w.opt_str(t);
    }
    w.opt_str(&def.return_type);
    // الدفعة الأخيرة DEF (البنود 109–112، الإصدار 2):
    // افتراضيات وجامعان.
    w.u64(def.defaults.len() as u64);
    for d in &def.defaults {
        match d {
            None => w.u8(0),
            Some(e) => {
                w.u8(1);
                enc_expr(w, e)?;
            }
        }
    }
    w.opt_str(&def.vararg);
    w.opt_str(&def.kwarg);
    enc_stmts(w, &def.body)?;
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    enc_type_params(w, &def.type_params);
    Ok(())
}

fn dec_closure_def(r: &mut Reader) -> RioResult<ClosureDef> {
    let id = r.u64()?;
    let name = r.str()?;
    let params = r.vec_str()?;
    let n_types = r.u64()? as usize;
    let mut param_types = Vec::with_capacity(n_types.min(1_000_000));
    for _ in 0..n_types {
        param_types.push(r.opt_str()?);
    }
    let return_type = r.opt_str()?;
    // الدفعة الأخيرة DEF (الإصدار 2).
    let n_defaults = r.u64()? as usize;
    let mut defaults = Vec::with_capacity(n_defaults.min(1_000_000));
    for _ in 0..n_defaults {
        match r.u8()? {
            0 => defaults.push(None),
            1 => defaults.push(Some(dec_expr(r)?)),
            _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
        }
    }
    let vararg = r.opt_str()?;
    let kwarg = r.opt_str()?;
    let body = dec_stmts(r)?;
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    let type_params = dec_type_params(r)?;
    // ب6: كـ FunctionDef — من الجسم نفسه، لا من التخزين
    // (المصنوع القديم بلا حقل مولد).
    let is_generator = crate::ast::body_has_yield(&body);
    Ok(ClosureDef { id, name, params, param_types, return_type, body, is_generator, defaults, vararg, kwarg, type_params })
}

fn enc_class_def(w: &mut Writer, class: &VmClassDef) -> RioResult<()> {
    w.str(&class.name);
    w.opt_str(&class.base);
    w.vec_str(&class.interfaces);
    let mut methods: Vec<(&String, &FunctionDef)> = class.methods.iter().collect();
    methods.sort_by(|a, b| a.0.cmp(b.0));
    w.u64(methods.len() as u64);
    for (name, def) in methods {
        w.str(name);
        enc_function_def(w, def)?;
    }
    let mut ids: Vec<(&String, &u64)> = class.own_method_ids.iter().collect();
    ids.sort_by(|a, b| a.0.cmp(b.0));
    w.u64(ids.len() as u64);
    for (name, id) in ids {
        w.str(name);
        w.u64(*id);
    }
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    enc_type_params(w, &class.type_params);
    Ok(())
}

fn dec_class_def(r: &mut Reader) -> RioResult<VmClassDef> {
    let name = r.str()?;
    let base = r.opt_str()?;
    let interfaces = r.vec_str()?;
    let n_methods = r.u64()? as usize;
    let mut methods = HashMap::new();
    for _ in 0..n_methods {
        let mname = r.str()?;
        let def = dec_function_def(r)?;
        methods.insert(mname, def);
    }
    let n_ids = r.u64()? as usize;
    let mut own_method_ids = HashMap::new();
    for _ in 0..n_ids {
        let mname = r.str()?;
        let id = r.u64()?;
        own_method_ids.insert(mname, id);
    }
    // البنود 145–147 (الإصدار 3): معاملات النوع.
    let type_params = dec_type_params(r)?;
    Ok(VmClassDef { name, base, interfaces, methods, own_method_ids, type_params })
}

/// البنود 145–147 (الإصدار 3): ترميز معاملات النوع —
/// لكل معامل: الاسم، ثم الحد (أو لا شيء)، ثم الافتراضي (أو لا شيء).
fn enc_type_params(w: &mut Writer, params: &[TypeParam]) {
    w.u64(params.len() as u64);
    for tp in params {
        w.str(&tp.name);
        w.opt_str(&tp.bound);
        w.opt_str(&tp.default);
    }
}

fn dec_type_params(r: &mut Reader) -> RioResult<Vec<TypeParam>> {
    let n = r.u64()? as usize;
    let mut out = Vec::with_capacity(n.min(1_000_000));
    for _ in 0..n {
        out.push(TypeParam { name: r.str()?, bound: r.opt_str()?, default: r.opt_str()? });
    }
    Ok(out)
}

fn enc_bool_ctx(w: &mut Writer, ctx: &BoolCtx) {
    let tag = match ctx {
        BoolCtx::If => 0,
        BoolCtx::Elif => 1,
        BoolCtx::And => 2,
        BoolCtx::Or => 3,
    };
    w.u8(tag);
}

fn dec_bool_ctx(r: &mut Reader) -> RioResult<BoolCtx> {
    match r.u8()? {
        0 => Ok(BoolCtx::If),
        1 => Ok(BoolCtx::Elif),
        2 => Ok(BoolCtx::And),
        3 => Ok(BoolCtx::Or),
        t => Err(bad(format!("مصنوع تالف: سياق منطقي غير معروف {}", t))),
    }
}

/// قيم الثوابت فقط (Number/Str/Bool/List/Dict/Nil) —
/// هذا كل ما يصدره المترجم في Op::Const؛ وأي قيمة
/// أخرى (دالة، وحدة، فئة، كائن) خطأ ترميز عربي،
/// لا فساد صامت.
fn enc_value(w: &mut Writer, v: &Value) -> RioResult<()> {
    match v {
        Value::Number(n) => {
            w.u8(0);
            w.f64(*n);
        }
        Value::Str(s) => {
            w.u8(1);
            w.str(s);
        }
        Value::Bool(b) => {
            w.u8(2);
            w.bool(*b);
        }
        Value::List(items) => {
            w.u8(3);
            w.u64(items.len() as u64);
            for item in items {
                enc_value(w, item)?;
            }
        }
        Value::Dict(entries) => {
            w.u8(4);
            w.u64(entries.len() as u64);
            for (k, val) in entries {
                w.str(k);
                enc_value(w, val)?;
            }
        }
        Value::Nil => {
            w.u8(5);
        }
        other => {
            return Err(bad(format!(
                "قيمة لا يمكن تخزينها في مصنوع بايت كود: {}",
                other.type_name()
            )));
        }
    }
    Ok(())
}

fn dec_value(r: &mut Reader) -> RioResult<Value> {
    match r.u8()? {
        0 => Ok(Value::Number(r.f64()?)),
        1 => Ok(Value::Str(r.str()?)),
        2 => Ok(Value::Bool(r.bool()?)),
        3 => {
            let n = r.u64()? as usize;
            let mut items = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                items.push(dec_value(r)?);
            }
            Ok(Value::List(items))
        }
        4 => {
            let n = r.u64()? as usize;
            let mut entries = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                let k = r.str()?;
                let v = dec_value(r)?;
                entries.push((k, v));
            }
            Ok(Value::Dict(entries))
        }
        5 => Ok(Value::Nil),
        t => Err(bad(format!("مصنوع تالف: وسم قيمة غير معروف {}", t))),
    }
}

fn enc_op(w: &mut Writer, op: &Op) -> RioResult<()> {
    match op {
        Op::Const(v) => {
            w.u8(0);
            enc_value(w, v)?;
        }
        Op::Load(s) => {
            w.u8(1);
            w.str(s);
        }
        Op::Store(s) => {
            w.u8(2);
            w.str(s);
        }
        // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
        Op::StoreAppend(s) => {
            w.u8(57);
            w.str(s);
        }
        Op::Pop => w.u8(3),
        Op::Add => w.u8(4),
        Op::Sub => w.u8(5),
        Op::Mul => w.u8(6),
        Op::Div => w.u8(7),
        Op::Mod => w.u8(8),
        Op::Eq => w.u8(9),
        Op::NotEq => w.u8(10),
        Op::Lt => w.u8(11),
        Op::Gt => w.u8(12),
        Op::LtEq => w.u8(13),
        Op::GtEq => w.u8(14),
        Op::And => w.u8(15),
        Op::Or => w.u8(16),
        Op::Not => w.u8(17),
        Op::Neg => w.u8(18),
        Op::Jump(t) => {
            w.u8(19);
            w.usize(*t);
        }
        Op::JumpIfFalse(t) => {
            w.u8(20);
            w.usize(*t);
        }
        Op::JumpIfTrue(t) => {
            w.u8(21);
            w.usize(*t);
        }
        Op::Print(n) => {
            w.u8(22);
            w.usize(*n);
        }
        Op::Call { name, nargs, kwargs, callee_line, callee_col, type_args } => {
            w.u8(23);
            w.str(name);
            w.usize(*nargs);
            w.vec_str(kwargs);
            w.usize(*callee_line);
            w.usize(*callee_col);
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            w.vec_str(type_args);
        }
        Op::Return => w.u8(24),
        Op::BuildList(n) => {
            w.u8(25);
            w.usize(*n);
        }
        Op::BuildDict(keys) => {
            w.u8(26);
            w.vec_str(keys);
        }
        Op::Index { idx_line, idx_col } => {
            w.u8(27);
            w.usize(*idx_line);
            w.usize(*idx_col);
        }
        Op::StoreIndex { name, idx_line, idx_col, tgt_line, tgt_col } => {
            w.u8(28);
            w.str(name);
            w.usize(*idx_line);
            w.usize(*idx_col);
            w.usize(*tgt_line);
            w.usize(*tgt_col);
        }
        Op::Builtin { name, nargs, kwargs, callee_line, callee_col } => {
            w.u8(29);
            w.str(name);
            w.usize(*nargs);
            w.vec_str(kwargs);
            w.usize(*callee_line);
            w.usize(*callee_col);
        }
        Op::DefineClass(class) => {
            w.u8(30);
            enc_class_def(w, class)?;
        }
        Op::DefineInterface(name, methods) => {
            w.u8(31);
            w.str(name);
            w.u64(methods.len() as u64);
            for (mname, arity) in methods {
                w.str(mname);
                w.usize(*arity);
            }
        }
        Op::Dot(name) => {
            w.u8(32);
            w.str(name);
        }
        Op::StoreDot(name) => {
            w.u8(33);
            w.str(name);
        }
        Op::Create { class, nargs, kwargs, type_args } => {
            w.u8(34);
            w.str(class);
            w.usize(*nargs);
            w.vec_str(kwargs);
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            w.vec_str(type_args);
        }
        Op::CallDotMethod { name, var, nargs, kwargs } => {
            w.u8(35);
            w.str(name);
            w.opt_str(var);
            w.usize(*nargs);
            w.vec_str(kwargs);
        }
        Op::MakeClosure(def) => {
            w.u8(36);
            enc_closure_def(w, def)?;
        }
        Op::DefineClosure(def) => {
            w.u8(37);
            enc_closure_def(w, def)?;
        }
        Op::CallValue { nargs, kwargs, callee_line, callee_col, type_args } => {
            w.u8(38);
            w.usize(*nargs);
            w.vec_str(kwargs);
            w.usize(*callee_line);
            w.usize(*callee_col);
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            w.vec_str(type_args);
        }
        Op::CheckBool(ctx) => {
            w.u8(39);
            enc_bool_ctx(w, ctx);
        }
        Op::CheckAssignType { name, expected } => {
            w.u8(40);
            w.str(name);
            w.str(expected);
        }
        Op::Import { module, alias, names } => {
            w.u8(41);
            w.str(module);
            w.opt_str(alias);
            w.opt_vec_str(names);
        }
        Op::JumpIfFalseLoop(t) => {
            w.u8(42);
            w.usize(*t);
        }
        Op::ForSetup { var, end_pc } => {
            w.u8(43);
            w.str(var);
            w.usize(*end_pc);
        }
        Op::ForNext(t) => {
            w.u8(44);
            w.usize(*t);
        }
        Op::RepeatCountSetup { end_pc } => {
            w.u8(45);
            w.usize(*end_pc);
        }
        Op::RepeatCountNext(t) => {
            w.u8(46);
            w.usize(*t);
        }
        Op::WhileBegin { end_pc } => {
            w.u8(47);
            w.usize(*end_pc);
        }
        Op::WhileTick => w.u8(48),
        Op::LoopEnd => w.u8(49),
        Op::Break => w.u8(50),
        Op::Continue => w.u8(51),
        Op::TryBegin { except_pc, finally_pc, after_pc } => {
            w.u8(52);
            w.opt_usize(except_pc);
            w.opt_usize(finally_pc);
            w.usize(*after_pc);
        }
        Op::EndTryBlock { finally_pc, after_pc } => {
            w.u8(53);
            w.opt_usize(finally_pc);
            w.usize(*after_pc);
        }
        Op::EndFinally => w.u8(54),
        Op::In => w.u8(55),
        Op::NotIn => w.u8(56),
    }
    Ok(())
}

fn dec_op(r: &mut Reader) -> RioResult<Op> {
    let tag = r.u8()?;
    let op = match tag {
        0 => Op::Const(dec_value(r)?),
        1 => Op::Load(r.str()?),
        2 => Op::Store(r.str()?),
        // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
        57 => Op::StoreAppend(r.str()?),
        3 => Op::Pop,
        4 => Op::Add,
        5 => Op::Sub,
        6 => Op::Mul,
        7 => Op::Div,
        8 => Op::Mod,
        9 => Op::Eq,
        10 => Op::NotEq,
        11 => Op::Lt,
        12 => Op::Gt,
        13 => Op::LtEq,
        14 => Op::GtEq,
        15 => Op::And,
        16 => Op::Or,
        17 => Op::Not,
        18 => Op::Neg,
        19 => Op::Jump(r.usize()?),
        20 => Op::JumpIfFalse(r.usize()?),
        21 => Op::JumpIfTrue(r.usize()?),
        22 => Op::Print(r.usize()?),
        23 => Op::Call {
            name: r.str()?,
            nargs: r.usize()?,
            kwargs: r.vec_str()?,
            callee_line: r.usize()?,
            callee_col: r.usize()?,
            type_args: r.vec_str()?,
        },
        24 => Op::Return,
        25 => Op::BuildList(r.usize()?),
        26 => Op::BuildDict(r.vec_str()?),
        27 => Op::Index { idx_line: r.usize()?, idx_col: r.usize()? },
        28 => Op::StoreIndex {
            name: r.str()?,
            idx_line: r.usize()?,
            idx_col: r.usize()?,
            tgt_line: r.usize()?,
            tgt_col: r.usize()?,
        },
        29 => Op::Builtin {
            name: r.str()?,
            nargs: r.usize()?,
            kwargs: r.vec_str()?,
            callee_line: r.usize()?,
            callee_col: r.usize()?,
        },
        30 => Op::DefineClass(dec_class_def(r)?),
        31 => {
            let name = r.str()?;
            let n = r.u64()? as usize;
            let mut methods = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                methods.push((r.str()?, r.usize()?));
            }
            Op::DefineInterface(name, methods)
        }
        32 => Op::Dot(r.str()?),
        33 => Op::StoreDot(r.str()?),
        34 => Op::Create { class: r.str()?, nargs: r.usize()?, kwargs: r.vec_str()?, type_args: r.vec_str()? },
        35 => Op::CallDotMethod { name: r.str()?, var: r.opt_str()?, nargs: r.usize()?, kwargs: r.vec_str()? },
        36 => Op::MakeClosure(dec_closure_def(r)?),
        37 => Op::DefineClosure(dec_closure_def(r)?),
        38 => Op::CallValue {
            nargs: r.usize()?,
            kwargs: r.vec_str()?,
            callee_line: r.usize()?,
            callee_col: r.usize()?,
            type_args: r.vec_str()?,
        },
        39 => Op::CheckBool(dec_bool_ctx(r)?),
        40 => Op::CheckAssignType { name: r.str()?, expected: r.str()? },
        41 => Op::Import { module: r.str()?, alias: r.opt_str()?, names: r.opt_vec_str()? },
        42 => Op::JumpIfFalseLoop(r.usize()?),
        43 => Op::ForSetup { var: r.str()?, end_pc: r.usize()? },
        44 => Op::ForNext(r.usize()?),
        45 => Op::RepeatCountSetup { end_pc: r.usize()? },
        46 => Op::RepeatCountNext(r.usize()?),
        47 => Op::WhileBegin { end_pc: r.usize()? },
        48 => Op::WhileTick,
        49 => Op::LoopEnd,
        50 => Op::Break,
        51 => Op::Continue,
        52 => Op::TryBegin {
            except_pc: r.opt_usize()?,
            finally_pc: r.opt_usize()?,
            after_pc: r.usize()?,
        },
        53 => Op::EndTryBlock { finally_pc: r.opt_usize()?, after_pc: r.usize()? },
        54 => Op::EndFinally,
        55 => Op::In,
        56 => Op::NotIn,
        t => return Err(bad(format!("مصنوع تالف: وسم تعليمة غير معروف {}", t))),
    };
    Ok(op)
}

fn enc_binop(w: &mut Writer, op: &BinOp) {
    let tag = match op {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Mod => 4,
        BinOp::Eq => 5,
        BinOp::NotEq => 6,
        BinOp::Lt => 7,
        BinOp::Gt => 8,
        BinOp::LtEq => 9,
        BinOp::GtEq => 10,
        BinOp::And => 11,
        BinOp::Or => 12,
        BinOp::In => 13,
        BinOp::NotIn => 14,
    };
    w.u8(tag);
}

fn dec_binop(r: &mut Reader) -> RioResult<BinOp> {
    match r.u8()? {
        0 => Ok(BinOp::Add),
        1 => Ok(BinOp::Sub),
        2 => Ok(BinOp::Mul),
        3 => Ok(BinOp::Div),
        4 => Ok(BinOp::Mod),
        5 => Ok(BinOp::Eq),
        6 => Ok(BinOp::NotEq),
        7 => Ok(BinOp::Lt),
        8 => Ok(BinOp::Gt),
        9 => Ok(BinOp::LtEq),
        10 => Ok(BinOp::GtEq),
        11 => Ok(BinOp::And),
        12 => Ok(BinOp::Or),
        13 => Ok(BinOp::In),
        14 => Ok(BinOp::NotIn),
        t => Err(bad(format!("مصنوع تالف: وسم عامل ثنائي غير معروف {}", t))),
    }
}

fn enc_unop(w: &mut Writer, op: &UnOp) {
    w.u8(match op {
        UnOp::Neg => 0,
        UnOp::Not => 1,
    });
}

fn dec_unop(r: &mut Reader) -> RioResult<UnOp> {
    match r.u8()? {
        0 => Ok(UnOp::Neg),
        1 => Ok(UnOp::Not),
        t => Err(bad(format!("مصنوع تالف: وسم عامل أحادي غير معروف {}", t))),
    }
}

fn enc_assign_target(w: &mut Writer, t: &AssignTarget) -> RioResult<()> {
    match t {
        AssignTarget::Var(name) => {
            w.u8(0);
            w.str(name);
        }
        AssignTarget::Index { target, index } => {
            w.u8(1);
            enc_expr(w, target)?;
            enc_expr(w, index)?;
        }
        AssignTarget::Dot { target, name } => {
            w.u8(2);
            enc_expr(w, target)?;
            w.str(name);
        }
    }
    Ok(())
}

fn dec_assign_target(r: &mut Reader) -> RioResult<AssignTarget> {
    match r.u8()? {
        0 => Ok(AssignTarget::Var(r.str()?)),
        1 => Ok(AssignTarget::Index { target: Box::new(dec_expr(r)?), index: Box::new(dec_expr(r)?) }),
        2 => Ok(AssignTarget::Dot { target: Box::new(dec_expr(r)?), name: r.str()? }),
        t => Err(bad(format!("مصنوع تالف: وسم هدف تعيين غير معروف {}", t))),
    }
}

fn enc_expr(w: &mut Writer, e: &Expr) -> RioResult<()> {
    enc_expr_kind(w, &e.kind)?;
    w.u64(e.line as u64);
    w.u64(e.col as u64);
    Ok(())
}

fn dec_expr(r: &mut Reader) -> RioResult<Expr> {
    let kind = dec_expr_kind(r)?;
    let line = r.u64()? as usize;
    let col = r.u64()? as usize;
    Ok(Expr { kind, line, col })
}

fn enc_expr_kind(w: &mut Writer, k: &ExprKind) -> RioResult<()> {
    match k {
        ExprKind::Number(n) => {
            w.u8(0);
            w.f64(*n);
        }
        ExprKind::Str(s) => {
            w.u8(1);
            w.str(s);
        }
        ExprKind::Bool(b) => {
            w.u8(2);
            w.bool(*b);
        }
        ExprKind::Nil => w.u8(3),
        ExprKind::Ident(s) => {
            w.u8(4);
            w.str(s);
        }
        ExprKind::List(items) => {
            w.u8(5);
            w.u64(items.len() as u64);
            for item in items {
                enc_expr(w, item)?;
            }
        }
        ExprKind::Dict(entries) => {
            w.u8(6);
            w.u64(entries.len() as u64);
            for (key, val) in entries {
                w.str(key);
                enc_expr(w, val)?;
            }
        }
        ExprKind::Binary { left, op, right } => {
            w.u8(7);
            enc_expr(w, left)?;
            enc_binop(w, op);
            enc_expr(w, right)?;
        }
        ExprKind::Unary { op, expr } => {
            w.u8(8);
            enc_unop(w, op);
            enc_expr(w, expr)?;
        }
        ExprKind::Call { callee, args, kwargs, type_args } => {
            w.u8(9);
            enc_expr(w, callee)?;
            w.u64(args.len() as u64);
            for arg in args {
                enc_expr(w, arg)?;
            }
            // الدفعة الأخيرة DEF (البنود 109–112): المعاملات
            // المفتاحية (الإصدار 2).
            w.u64(kwargs.len() as u64);
            for (kname, kval) in kwargs {
                w.str(kname);
                enc_expr(w, kval)?;
            }
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            w.vec_str(type_args);
        }
        ExprKind::Index { target, index } => {
            w.u8(10);
            enc_expr(w, target)?;
            enc_expr(w, index)?;
        }
        ExprKind::Dot { target, name } => {
            w.u8(11);
            enc_expr(w, target)?;
            w.str(name);
        }
        ExprKind::Create { class, args, kwargs, type_args } => {
            w.u8(12);
            w.str(class);
            w.u64(args.len() as u64);
            for arg in args {
                enc_expr(w, arg)?;
            }
            // الدفعة الأخيرة DEF (البنود 109–112): معاملات
            // المنشئ المفتاحية (الإصدار 2).
            w.u64(kwargs.len() as u64);
            for (kname, kval) in kwargs {
                w.str(kname);
                enc_expr(w, kval)?;
            }
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            w.vec_str(type_args);
        }
        ExprKind::Lambda { params, param_types, body } => {
            w.u8(13);
            w.vec_str(params);
            w.u64(param_types.len() as u64);
            for t in param_types {
                w.opt_str(t);
            }
            enc_expr(w, body)?;
        }
        // البند 73 (`:=` — الاجتماع 83 §98): وسم جديد 14.
        ExprKind::AssignExpr { name, value } => {
            w.u8(14);
            w.str(name);
            enc_expr(w, value)?;
        }
    }
    Ok(())
}

fn dec_expr_kind(r: &mut Reader) -> RioResult<ExprKind> {
    let tag = r.u8()?;
    let kind = match tag {
        0 => ExprKind::Number(r.f64()?),
        1 => ExprKind::Str(r.str()?),
        2 => ExprKind::Bool(r.bool()?),
        3 => ExprKind::Nil,
        4 => ExprKind::Ident(r.str()?),
        5 => {
            let n = r.u64()? as usize;
            let mut items = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                items.push(dec_expr(r)?);
            }
            ExprKind::List(items)
        }
        6 => {
            let n = r.u64()? as usize;
            let mut entries = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                entries.push((r.str()?, dec_expr(r)?));
            }
            ExprKind::Dict(entries)
        }
        7 => ExprKind::Binary {
            left: Box::new(dec_expr(r)?),
            op: dec_binop(r)?,
            right: Box::new(dec_expr(r)?),
        },
        8 => ExprKind::Unary { op: dec_unop(r)?, expr: Box::new(dec_expr(r)?) },
        9 => {
            let callee = Box::new(dec_expr(r)?);
            let n = r.u64()? as usize;
            let mut args = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                args.push(dec_expr(r)?);
            }
            // الدفعة الأخيرة DEF (الإصدار 2): معاملات مفتاحية.
            let nk = r.u64()? as usize;
            let mut kwargs = Vec::with_capacity(nk.min(1_000_000));
            for _ in 0..nk {
                kwargs.push((r.str()?, dec_expr(r)?));
            }
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            let type_args = r.vec_str()?;
            ExprKind::Call { callee, args, kwargs, type_args }
        }
        10 => ExprKind::Index { target: Box::new(dec_expr(r)?), index: Box::new(dec_expr(r)?) },
        11 => ExprKind::Dot { target: Box::new(dec_expr(r)?), name: r.str()? },
        12 => {
            let class = r.str()?;
            let n = r.u64()? as usize;
            let mut args = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                args.push(dec_expr(r)?);
            }
            // الدفعة الأخيرة DEF (الإصدار 2): معاملات مفتاحية.
            let nk = r.u64()? as usize;
            let mut kwargs = Vec::with_capacity(nk.min(1_000_000));
            for _ in 0..nk {
                kwargs.push((r.str()?, dec_expr(r)?));
            }
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            let type_args = r.vec_str()?;
            ExprKind::Create { class, args, kwargs, type_args }
        }
        13 => {
            let params = r.vec_str()?;
            let n_types = r.u64()? as usize;
            let mut param_types = Vec::with_capacity(n_types.min(1_000_000));
            for _ in 0..n_types {
                param_types.push(r.opt_str()?);
            }
            let body = Box::new(dec_expr(r)?);
            ExprKind::Lambda { params, param_types, body }
        }
        // البند 73 (`:=` — الاجتماع 83 §98): وسم جديد 14.
        14 => ExprKind::AssignExpr { name: r.str()?, value: Box::new(dec_expr(r)?) },
        t => return Err(bad(format!("مصنوع تالف: وسم تعبير غير معروف {}", t))),
    };
    Ok(kind)
}

fn enc_stmts(w: &mut Writer, stmts: &[Stmt]) -> RioResult<()> {
    w.u64(stmts.len() as u64);
    for stmt in stmts {
        enc_stmt_kind(w, &stmt.kind)?;
        w.u64(stmt.line as u64);
        w.u64(stmt.col as u64);
    }
    Ok(())
}

fn dec_stmts(r: &mut Reader) -> RioResult<Vec<Stmt>> {
    let n = r.u64()? as usize;
    let mut stmts = Vec::with_capacity(n.min(1_000_000));
    for _ in 0..n {
        let kind = dec_stmt_kind(r)?;
        let line = r.u64()? as usize;
        let col = r.u64()? as usize;
        stmts.push(Stmt { kind, line, col });
    }
    Ok(stmts)
}

fn enc_stmt_kind(w: &mut Writer, k: &StmtKind) -> RioResult<()> {
    match k {
        StmtKind::Assign { target, value, annotation, append } => {
            w.u8(0);
            enc_assign_target(w, target)?;
            enc_expr(w, value)?;
            w.opt_str(annotation);
            // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
            w.u8(if *append { 1 } else { 0 });
        }
        StmtKind::Print { args } => {
            w.u8(1);
            w.u64(args.len() as u64);
            for arg in args {
                enc_expr(w, arg)?;
            }
        }
        StmtKind::If { cond, then_body, elifs, else_body } => {
            w.u8(2);
            enc_expr(w, cond)?;
            enc_stmts(w, then_body)?;
            w.u64(elifs.len() as u64);
            for (econd, ebody) in elifs {
                enc_expr(w, econd)?;
                enc_stmts(w, ebody)?;
            }
            match else_body {
                None => w.u8(0),
                Some(body) => {
                    w.u8(1);
                    enc_stmts(w, body)?;
                }
            }
        }
        StmtKind::While { cond, body } => {
            w.u8(3);
            enc_expr(w, cond)?;
            enc_stmts(w, body)?;
        }
        StmtKind::For { var, iterable, body } => {
            w.u8(4);
            w.str(var);
            enc_expr(w, iterable)?;
            enc_stmts(w, body)?;
        }
        StmtKind::RepeatCount { count, body } => {
            w.u8(5);
            enc_expr(w, count)?;
            enc_stmts(w, body)?;
        }
        StmtKind::RepeatWhile { cond, body } => {
            w.u8(6);
            enc_expr(w, cond)?;
            enc_stmts(w, body)?;
        }
        StmtKind::FuncDef { name, params, param_types, return_type, defaults, vararg, kwarg, body, type_params } => {
            w.u8(7);
            w.str(name);
            w.vec_str(params);
            w.u64(param_types.len() as u64);
            for t in param_types {
                w.opt_str(t);
            }
            w.opt_str(return_type);
            // الدفعة الأخيرة DEF (البنود 109–112، الإصدار 2):
            // قيم افتراضية (تعبير أو لا شيء لكل معامل)، ثم
            // اسما الجامعين (أو لا شيء).
            w.u64(defaults.len() as u64);
            for d in defaults {
                match d {
                    None => w.u8(0),
                    Some(e) => {
                        w.u8(1);
                        enc_expr(w, e)?;
                    }
                }
            }
            w.opt_str(vararg);
            w.opt_str(kwarg);
            enc_stmts(w, body)?;
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            enc_type_params(w, type_params);
        }
        StmtKind::Return { value } => {
            w.u8(8);
            match value {
                None => w.u8(0),
                Some(e) => {
                    w.u8(1);
                    enc_expr(w, e)?;
                }
            }
        }
        StmtKind::Break => w.u8(9),
        StmtKind::Continue => w.u8(10),
        StmtKind::Pass => w.u8(11),
        StmtKind::Try { try_body, except_body, else_body, finally_body } => {
            w.u8(12);
            enc_stmts(w, try_body)?;
            match except_body {
                None => w.u8(0),
                Some(body) => {
                    w.u8(1);
                    enc_stmts(w, body)?;
                }
            }
            // البند 98 (`والا` بعد حاول — الإصدار 2).
            match else_body {
                None => w.u8(0),
                Some(body) => {
                    w.u8(1);
                    enc_stmts(w, body)?;
                }
            }
            match finally_body {
                None => w.u8(0),
                Some(body) => {
                    w.u8(1);
                    enc_stmts(w, body)?;
                }
            }
        }
        StmtKind::ExprStmt { expr } => {
            w.u8(13);
            enc_expr(w, expr)?;
        }
        StmtKind::Import { module, alias, names } => {
            w.u8(14);
            w.str(module);
            w.opt_str(alias);
            w.opt_vec_str(names);
        }
        StmtKind::Export { names } => {
            w.u8(15);
            w.vec_str(names);
        }
        StmtKind::ClassDef { name, base, interfaces, methods, type_params } => {
            w.u8(16);
            w.str(name);
            w.opt_str(base);
            w.vec_str(interfaces);
            enc_stmts(w, methods)?;
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            enc_type_params(w, type_params);
        }
        StmtKind::InterfaceDef { name, methods } => {
            w.u8(17);
            w.str(name);
            w.u64(methods.len() as u64);
            for (mname, arity) in methods {
                w.str(mname);
                w.usize(*arity);
            }
        }
        // البند 8 — الزخارف: الوسم 18 (جديد؛ فلا تصادم
        // مع مصنوعات قديمة تحمل وسوما 0..17).
        StmtKind::Decorated { decorators, def } => {
            w.u8(18);
            w.u64(decorators.len() as u64);
            for d in decorators {
                enc_expr(w, d)?;
            }
            enc_stmts(w, std::slice::from_ref(def))?;
        }
        // الدفعة الأخيرة DEF (البنود 77 و78 و104، الإصدار 2):
        // أوسام جديدة 19–21 — فلا تصادم مع مصنوع قديم.
        StmtKind::GlobalDecl { names } => {
            w.u8(19);
            w.vec_str(names);
        }
        StmtKind::NonlocalDecl { names } => {
            w.u8(20);
            w.vec_str(names);
        }
        StmtKind::TypeAlias { name, target } => {
            w.u8(21);
            w.str(name);
            w.str(target);
        }
    }
    Ok(())
}

fn dec_stmt_kind(r: &mut Reader) -> RioResult<StmtKind> {
    let tag = r.u8()?;
    let kind = match tag {
        0 => StmtKind::Assign {
            target: dec_assign_target(r)?,
            value: dec_expr(r)?,
            annotation: r.opt_str()?,
            // الدفعة 4 — المتغيرات (اجتماع 58 §73): `+=`.
            append: r.u8()? != 0,
        },
        1 => {
            let n = r.u64()? as usize;
            let mut args = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                args.push(dec_expr(r)?);
            }
            StmtKind::Print { args }
        }
        2 => {
            let cond = dec_expr(r)?;
            let then_body = dec_stmts(r)?;
            let n_elifs = r.u64()? as usize;
            let mut elifs = Vec::with_capacity(n_elifs.min(1_000_000));
            for _ in 0..n_elifs {
                elifs.push((dec_expr(r)?, dec_stmts(r)?));
            }
            let else_body = match r.u8()? {
                0 => None,
                1 => Some(dec_stmts(r)?),
                _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
            };
            StmtKind::If { cond, then_body, elifs, else_body }
        }
        3 => StmtKind::While { cond: dec_expr(r)?, body: dec_stmts(r)? },
        4 => StmtKind::For {
            var: r.str()?,
            iterable: dec_expr(r)?,
            body: dec_stmts(r)?,
        },
        5 => StmtKind::RepeatCount { count: dec_expr(r)?, body: dec_stmts(r)? },
        6 => StmtKind::RepeatWhile { cond: dec_expr(r)?, body: dec_stmts(r)? },
        7 => {
            let name = r.str()?;
            let params = r.vec_str()?;
            let n_types = r.u64()? as usize;
            let mut param_types = Vec::with_capacity(n_types.min(1_000_000));
            for _ in 0..n_types {
                param_types.push(r.opt_str()?);
            }
            let return_type = r.opt_str()?;
            // الدفعة الأخيرة DEF (البنود 109–112، الإصدار 2).
            let n_defaults = r.u64()? as usize;
            let mut defaults = Vec::with_capacity(n_defaults.min(1_000_000));
            for _ in 0..n_defaults {
                match r.u8()? {
                    0 => defaults.push(None),
                    1 => defaults.push(Some(dec_expr(r)?)),
                    _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
                }
            }
            let vararg = r.opt_str()?;
            let kwarg = r.opt_str()?;
            let body = dec_stmts(r)?;
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            let type_params = dec_type_params(r)?;
            StmtKind::FuncDef { name, params, param_types, return_type, defaults, vararg, kwarg, body, type_params }
        }
        8 => {
            let value = match r.u8()? {
                0 => None,
                1 => Some(dec_expr(r)?),
                _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
            };
            StmtKind::Return { value }
        }
        9 => StmtKind::Break,
        10 => StmtKind::Continue,
        11 => StmtKind::Pass,
        12 => {
            let try_body = dec_stmts(r)?;
            let except_body = match r.u8()? {
                0 => None,
                1 => Some(dec_stmts(r)?),
                _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
            };
            // البند 98 (`والا` بعد حاول — الإصدار 2).
            let else_body = match r.u8()? {
                0 => None,
                1 => Some(dec_stmts(r)?),
                _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
            };
            let finally_body = match r.u8()? {
                0 => None,
                1 => Some(dec_stmts(r)?),
                _ => return Err(bad("مصنوع تالف: وسم خيار غير صالح".to_string())),
            };
            StmtKind::Try { try_body, except_body, else_body, finally_body }
        }
        13 => StmtKind::ExprStmt { expr: dec_expr(r)? },
        14 => StmtKind::Import { module: r.str()?, alias: r.opt_str()?, names: r.opt_vec_str()? },
        15 => StmtKind::Export { names: r.vec_str()? },
        16 => {
            let name = r.str()?;
            let base = r.opt_str()?;
            let interfaces = r.vec_str()?;
            let methods = dec_stmts(r)?;
            // البنود 145–147 (الإصدار 3): معاملات النوع.
            let type_params = dec_type_params(r)?;
            StmtKind::ClassDef { name, base, interfaces, methods, type_params }
        },
        17 => {
            let name = r.str()?;
            let n = r.u64()? as usize;
            let mut methods = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                methods.push((r.str()?, r.usize()?));
            }
            StmtKind::InterfaceDef { name, methods }
        }
        18 => {
            let n = r.u64()? as usize;
            let mut decorators = Vec::with_capacity(n.min(1_000_000));
            for _ in 0..n {
                decorators.push(dec_expr(r)?);
            }
            let mut defs = dec_stmts(r)?;
            if defs.len() != 1 {
                return Err(bad(
                    "مصنوع تالف: زخرفة بلا تعريف واحد".to_string(),
                ));
            }
            let def = defs.pop().unwrap();
            StmtKind::Decorated { decorators, def: Box::new(def) }
        }
        // الدفعة الأخيرة DEF (البنود 77 و78 و104، الإصدار 2).
        19 => StmtKind::GlobalDecl { names: r.vec_str()? },
        20 => StmtKind::NonlocalDecl { names: r.vec_str()? },
        21 => StmtKind::TypeAlias { name: r.str()?, target: r.str()? },
        t => return Err(bad(format!("مصنوع تالف: وسم جملة غير معروف {}", t))),
    };
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;

    fn compile_src(source: &str) -> CompiledProgram {
        let tokens = tokenize(source).unwrap();
        let program = Parser::new(tokens).parse_program().unwrap();
        crate::vm::Compiler::compile_program(&program).unwrap()
    }

    #[test]
    fn artifact_roundtrip_kitchen_sink() {
        // كل شيء: إغلاقات ولامدا وفئات ووراثة وواجهات
        // وحاول ومكتبة ووحدات ومحاولة/ماعدا/أخيرا.
        let source = r#"
استورد رياضيات
فئة نقطة:
    دالة أنشئ(س، ص):
        نفس.س = س
        نفس.ص = ص
    دالة مجموع():
        ارجع نفس.س + نفس.ص
واجهة قابل:
    دالة م():
        مرر
دالة اصنع_عداد():
    ع = ٠
    دالة زد():
        ع = ع + ١
        ارجع ع
    ارجع زد
دالة غلف_اختبار(د):
    ارجع د
@غلف_اختبار
دالة مختارة():
    ارجع ٩
مضاعف = دالة_مجهولة(س): س * ٢
حاول:
    س = ١ / ٠
ماعدا:
    س = ٧
أخيرا:
    س = س + ١
اطبع س
"#;
        let prog = compile_src(source);
        let bytes = encode_program(&prog).unwrap();
        let bytes2 = encode_program(&prog).unwrap();
        assert_eq!(bytes, bytes2, "الترميز غير حتمي");
        let back = decode_program(&bytes).unwrap();
        let bytes3 = encode_program(&back).unwrap();
        assert_eq!(bytes, bytes3, "فك ثم ترميز لا يعيد البايتات نفسها");
        assert_eq!(back.code.len(), prog.code.len());
        assert_eq!(back.functions.len(), prog.functions.len());
        assert_eq!(back.closures.len(), prog.closures.len());
        assert_eq!(max_closure_id(&back), max_closure_id(&prog));
    }

    #[test]
    fn artifact_rejects_corrupt() {
        let prog = compile_src("اطبع ٥\n");
        let bytes = encode_program(&prog).unwrap();
        // سحر خاطئ.
        let mut bad_magic = bytes.clone();
        bad_magic[0] = 0;
        assert!(decode_program(&bad_magic).is_err());
        // مقطوع.
        assert!(decode_program(&bytes[..bytes.len() / 2]).is_err());
        // بايتات زائدة.
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_program(&extra).is_err());
        // إصدار غير مدعوم.
        let mut bad_version = bytes.clone();
        bad_version[8] = 99;
        assert!(decode_program(&bad_version).is_err());
        // ملف مصدر عادي ليس مصنوعا.
        assert!(decode_program("اطبع ٥\n".as_bytes()).is_err());
        // فارغ.
        assert!(decode_program(&[]).is_err());
    }

    #[test]
    fn artifact_standalone_wrap_roundtrip() {
        let prog = compile_src("اطبع ٥\n");
        let payload = encode_program(&prog).unwrap();
        let exe = "تنفيذية-وهمية".as_bytes();
        let wrapped = wrap_standalone(exe, &payload);
        // الذيل: الحمولة ثم الطول ثم السحر.
        assert!(wrapped.ends_with(STANDALONE_MAGIC));
        let tmp = std::env::temp_dir().join("niva-standalone-wrap-test.bin");
        std::fs::write(&tmp, &wrapped).unwrap();
        let got = extract_standalone_payload(&tmp).unwrap();
        assert_eq!(got, payload);
        let _ = std::fs::remove_file(&tmp);
        // ملف بلا ذيل مستقل يعطي لا شيء.
        let plain = std::env::temp_dir().join("niva-standalone-plain-test.bin");
        std::fs::write(&plain, exe).unwrap();
        assert!(extract_standalone_payload(&plain).is_none());
        let _ = std::fs::remove_file(&plain);
    }
}
