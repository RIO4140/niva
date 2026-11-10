//! مولد كود C من AST — خلفية C الأصلية (مهمة 29، خطة §29،
//! DESIGN §39.5 ك-3). يعيد استخدام المحلل الدلالي نفسه
//! (لا محلل جديد): المدخل برنامج محلل، والمخرج مصدر C
//! كامل يترجمه cc النظام إلى ثنائي أصلي.
//!
//! الدلالة: سلسلة نطاقات ديناميكية مثل الآلة الافتراضية
//! (أقرب نطاق يربط الاسم، وإلا الأعمق)؛ والقوائم والقواميس
//! قيم تُنسخ عند التعيين؛ ودوال الوحدات تستنسخ نطاق
//! وحدتها جديدا كل استدعاء. كل الأخطاء عربية برسائل
//! المحرك نفسها (line/col من AST).

use crate::ast::{AssignTarget, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
use crate::error::{ErrorKind, RioError, RioResult};
use crate::lexer::tokenize;
use crate::packages;
use crate::parser::Parser;
use crate::semantic::SemanticAnalyzer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------- أدوات نصية ----------

/// حرف C: نص عربي إلى حرفي C (ASCII فقط — كل بايت
/// غير ASCII يخرج \xNN، فالمصدر المولد آمن على أي cc).
/// هروب بايتات سلسلة نصية إلى حرفي C.
/// البايتات غير ASCII تُكتب هروبا ثمانيا بثلاثة أرقام
/// (`\NNN`) لا ست عشريا: هروب `\xNN` في C جشع —
/// إن تبعه رقم ست عشري ASCII (كـ«1» في «مثال18»)
/// ابتلعه وصار بايتا خاطئا (أثبتّه: `\xd9\x8418`
/// تعطي 0x18 لا 0x84 ثم «18»). الثماني بثلاثة
/// أرقام لا يتجاوزها أبدا، فالحرف التالي سليم دائما.
pub fn c_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out
}

fn c_str(s: &str) -> String {
    format!("\"{}\"", c_escape(s))
}

/// عدد f64 إلى حرفي C يعيد التحويل نفسه.
fn c_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 9.0e15 {
        format!("{}.0", n as i64)
    } else {
        format!("{:.17e}", n)
    }
}

// ---------- معلومات وحدة مولدة ----------

struct ModuleInfo {
    init_fn: String,
    loaded_flag: String,
    val_var: String,
    // فهرس ملف الوحدة في جدول nv_source_files
    // (اجتماع 33: أخطاؤها تسمّي ملفها كما في المفسر).
    file_idx: usize,
}

// ---------- سياق توليد ----------

#[derive(Clone)]
struct LoopCtx {
    head: String,
    end: String,
}

#[derive(Clone)]
struct TryCtx {
    handler: String,
    finally_body: Option<Vec<Stmt>>,
}

struct Ctx {
    scope_var: String,
    in_function: bool,
    func_end: String,
    loop_stack: Vec<LoopCtx>,
    try_stack: Vec<TryCtx>,
    // الوحدة الجاري توليدها (لتعليم دوال مستواها الأعلى).
    module_name: Option<String>,
    module_scope_var: Option<String>,
    // عمق تداخل الدوال (0 = المستوى الأعلى للوحدة/البرنامج).
    func_depth: usize,
    // فهرس ملف المصدر الجاري توليده في جدول
    // nv_source_files ([0] الملف الرئيسي) — يضبطه
    // الكود المولّد في nv_cur_file_idx عند كل جملة.
    file_idx: usize,
}

impl Ctx {
    fn top(scope_var: &str, func_end: &str) -> Self {
        Ctx {
            scope_var: scope_var.to_string(),
            in_function: false,
            func_end: func_end.to_string(),
            loop_stack: Vec::new(),
            try_stack: Vec::new(),
            module_name: None,
            module_scope_var: None,
            func_depth: 0,
            file_idx: 0,
        }
    }
}

// ---------- المولد ----------

pub struct Gen {
    funcs: String,
    next_func: usize,
    next_mod: usize,
    tmp: usize,
    live: Vec<String>,
    modules: HashMap<String, ModuleInfo>,
    import_stack: Vec<String>,
    program_dir: PathBuf,
    // ملفات المصدر بترتيب جدول nv_source_files الذي
    // يولَّد في الكود ([0] الملف الرئيسي ثم كل وحدة
    // مستوردة) — العرض بشرطة مائلة كما في المفسر.
    files: Vec<String>,
}

impl Gen {
    pub fn new(program_dir: PathBuf, main_file: String) -> Self {
        Gen {
            funcs: String::new(),
            next_func: 0,
            next_mod: 0,
            tmp: 0,
            live: Vec::new(),
            modules: HashMap::new(),
            import_stack: Vec::new(),
            program_dir,
            files: vec![main_file],
        }
    }

    fn fresh_func(&mut self) -> String {
        let id = self.next_func;
        self.next_func += 1;
        format!("nv_f{}", id)
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let id = self.next_func;
        self.next_func += 1;
        format!("_{}{}", prefix, id)
    }

    // --- المتغيرات المؤقتة والجذور ---
    // كل نتيجة تعبير تعيش في متغير C محلي؛ والقيم الحية
    // تُجذَّر حول كل استدعاء قد يخصص (فيجمع القمامة).

    fn new_temp(&mut self) -> String {
        let id = self.tmp;
        self.tmp += 1;
        format!("_t{}", id)
    }

    /// جذّر كل القيم الحية الآن؛ يعيد عدد المدفوعات.
    fn root_live(&mut self, out: &mut String) -> usize {
        let n = self.live.len();
        for (i, t) in self.live.iter().enumerate() {
            out.push_str(&format!(
                "size_t _ri{}_{} = nv_root_push_idx(); *nv_root_slot(_ri{}_{}) = {};\n",
                self.tmp, i, self.tmp, i, t
            ));
        }
        self.tmp += 1;
        n
    }

    fn unroot(&mut self, out: &mut String, n: usize) {
        if n > 0 {
            out.push_str(&format!("nv_root_pop_n({});\n", n));
        }
    }

    /// قيّم تعبيرا إلى متغير مؤقت حي؛ يعيد اسمه.
    fn eval_into_temp(&mut self, e: &Expr, ctx: &mut Ctx, out: &mut String) -> String {
        let t = self.new_temp();
        let val = self.emit_expr(e, ctx, out);
        out.push_str(&format!("NvValue {} = {};\n", t, val));
        self.live.push(t.clone());
        t
    }

    /// حرر متغيرا مؤقتا (أزله من الحية).
    fn release(&mut self, t: &str) {
        if let Some(pos) = self.live.iter().position(|x| x == t) {
            self.live.remove(pos);
        }
    }

    /// حرر كل المؤقتات بعد نقطة (نهاية جملة).
    fn checkpoint_live(&self) -> usize {
        self.live.len()
    }

    fn restore_live(&mut self, n: usize) {
        self.live.truncate(n);
    }

    /// ادخل دالة C متداخلة: المؤقتات الحية في الخارج لا
    /// تنتمي إلى الدالة الجديدة (أسماؤها غير معلنة فيها).
    fn enter_nested_fn(&mut self) -> Vec<String> {
        std::mem::take(&mut self.live)
    }

    fn exit_nested_fn(&mut self, saved: Vec<String>) {
        self.live = saved;
    }

    // ---------- التعابير ----------
    // تعيد تعبير C (غالبا اسم متغير مؤقت حي).

    fn emit_expr(&mut self, e: &Expr, ctx: &mut Ctx, out: &mut String) -> String {
        match &e.kind {
            ExprKind::Number(n) => {
                let t = self.new_temp();
                out.push_str(&format!("NvValue {} = nv_num({});\n", t, c_num(*n)));
                self.live.push(t.clone());
                t
            }
            ExprKind::Str(s) => {
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!(
                    "NvValue {} = nv_str_cstr({});\n",
                    t,
                    c_str(s)
                ));
                self.unroot(out, n);
                self.live.push(t.clone());
                t
            }
            ExprKind::Bool(b) => {
                let t = self.new_temp();
                out.push_str(&format!(
                    "NvValue {} = nv_bool({});\n",
                    t,
                    if *b { 1 } else { 0 }
                ));
                self.live.push(t.clone());
                t
            }
            ExprKind::Nil => {
                let t = self.new_temp();
                out.push_str(&format!("NvValue {} = nv_nil();\n", t));
                self.live.push(t.clone());
                t
            }
            ExprKind::Ident(name) => {
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!(
                    "NvValue {} = nv_lookup_or_error({}, {}, {}, {});\n",
                    t,
                    ctx.scope_var,
                    c_str(name),
                    e.line,
                    e.col
                ));
                self.unroot(out, n);
                // قراءة متغير تنسخ القوائم/القواميس (دلالة المحرك).
                // وt نفسها (نسخة جديدة في كومة) يجب أن تكون
                // حية أثناء النسخة الثانية، وإلا كنستها
                // الجامعة أثناء تخصيص القائمة الجديدة.
                let t2 = self.new_temp();
                self.live.push(t.clone());
                let n2 = self.root_live(out);
                out.push_str(&format!("NvValue {} = nv_clone_value({});\n", t2, t));
                self.unroot(out, n2);
                self.release(&t);
                self.live.push(t2.clone());
                // متغير غير معرف: القراءة المباشرة لاشيء هنا؛
                // والتحقق الحقيقي في موضع الاستخدام (المحلل
                // الدلالي يرفض غير المعرف ستاتيكيا أصلا، فهذا
                // المسار دفاعي فقط).
                t2
            }
            ExprKind::List(elems) => {
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!("NvValue {} = nv_list_new();\n", t));
                self.unroot(out, n);
                self.live.push(t.clone());
                for el in elems {
                    let et = self.eval_into_temp(el, ctx, out);
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "nv_list_push_raw({}.u.list, {});\n",
                        t, et
                    ));
                    self.unroot(out, n);
                    self.release(&et);
                }
                t
            }
            ExprKind::Dict(entries) => {
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!("NvValue {} = nv_dict_new();\n", t));
                self.unroot(out, n);
                self.live.push(t.clone());
                for (k, v) in entries {
                    let vt = self.eval_into_temp(v, ctx, out);
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "nv_dict_set_raw({}.u.dict, {}, {});\n",
                        t,
                        c_str(k),
                        vt
                    ));
                    self.unroot(out, n);
                    self.release(&vt);
                }
                t
            }
            ExprKind::Binary { left, op, right } => self.emit_binary(left, op, right, e, ctx, out),
            ExprKind::Unary { op, expr: inner } => {
                let it = self.eval_into_temp(inner, ctx, out);
                let t = self.new_temp();
                match op {
                    UnOp::Neg => {
                        out.push_str(&format!(
                            "NvValue {} = nv_neg({}, {}, {});\n",
                            t, it, e.line, e.col
                        ));
                    }
                    UnOp::Not => {
                        out.push_str(&format!(
                            "NvValue {} = nv_not({}, {}, {});\n",
                            t, it, e.line, e.col
                        ));
                    }
                }
                self.release(&it);
                self.live.push(t.clone());
                t
            }
            ExprKind::Call { callee, args } => self.emit_call(callee, args, e, ctx, out),
            ExprKind::Index { target, index } => {
                let tt = self.eval_into_temp(target, ctx, out);
                let it = self.eval_into_temp(index, ctx, out);
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!(
                    "NvValue {} = nv_index({}, {}, {}, {}, {}, {});\n",
                    t, tt, it, e.line, e.col, index.line, index.col
                ));
                self.unroot(out, n);
                self.release(&tt);
                self.release(&it);
                self.live.push(t.clone());
                t
            }
            ExprKind::Dot { target, name } => {
                let tt = self.eval_into_temp(target, ctx, out);
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!(
                    "NvValue {} = nv_dot({}, {}, {}, {});\n",
                    t,
                    tt,
                    c_str(name),
                    e.line,
                    e.col
                ));
                self.unroot(out, n);
                self.release(&tt);
                self.live.push(t.clone());
                t
            }
            ExprKind::Create { class, args } => {
                let mut arg_tmps = Vec::new();
                for a in args {
                    arg_tmps.push(self.eval_into_temp(a, ctx, out));
                }
                let t = self.new_temp();
                let n = self.root_live(out);
                if arg_tmps.is_empty() {
                    out.push_str(&format!(
                        "NvValue {} = nv_create_by_name({}, {}, NULL, 0, {}, {});\n",
                        t,
                        ctx.scope_var,
                        c_str(class),
                        e.line,
                        e.col
                    ));
                } else {
                    out.push_str(&format!(
                        "NvValue _args{}[] = {{{}}};\n",
                        self.tmp,
                        arg_tmps.join(", ")
                    ));
                    self.tmp += 1;
                    out.push_str(&format!(
                        "NvValue {} = nv_create_by_name({}, {}, _args{}, {}, {}, {});\n",
                        t,
                        ctx.scope_var,
                        c_str(class),
                        self.tmp - 1,
                        arg_tmps.len(),
                        e.line,
                        e.col
                    ));
                }
                self.unroot(out, n);
                for a in arg_tmps {
                    self.release(&a);
                }
                self.live.push(t.clone());
                t
            }
            ExprKind::Lambda { params, param_types, body } => {
                self.emit_lambda(params, param_types, body, e, ctx, out)
            }
        }
    }

    fn emit_binary(
        &mut self,
        left: &Expr,
        op: &BinOp,
        right: &Expr,
        e: &Expr,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> String {
        // و/أو بدائرة قصر (رسائل المحرك الخاصة).
        if *op == BinOp::And || *op == BinOp::Or {
            let lt = self.eval_into_temp(left, ctx, out);
            let t = self.new_temp();
            let is_and = *op == BinOp::And;
            let word = if is_and { "و" } else { "أو" };
            // القيمة الافتراضية: و -> خطأ، أو -> صحيح.
            out.push_str(&format!(
                "NvValue {} = nv_bool({});\n",
                t,
                if is_and { 0 } else { 1 }
            ));
            self.live.push(t.clone());
            let cond = if is_and {
                format!("nv_check_bool_op({}, {}, {}, {})", lt, c_str(word), left.line, left.col)
            } else {
                format!("!nv_check_bool_op({}, {}, {}, {})", lt, c_str(word), left.line, left.col)
            };
            out.push_str(&format!("if ({}) {{\n", cond));
            let mark = self.checkpoint_live();
            let rt = self.eval_into_temp(right, ctx, out);
            out.push_str(&format!(
                "{} = nv_bool(nv_check_bool_op({}, {}, {}, {}));\n",
                t, rt, c_str(word), right.line, right.col
            ));
            self.restore_live(mark);
            out.push_str("}\n");
            self.release(&lt);
            return t;
        }
        let lt = self.eval_into_temp(left, ctx, out);
        let rt = self.eval_into_temp(right, ctx, out);
        let t = self.new_temp();
        let call = match op {
            BinOp::Add => format!("nv_add({}, {}, {}, {})", lt, rt, e.line, e.col),
            BinOp::Sub => format!("nv_sub({}, {}, {}, {})", lt, rt, e.line, e.col),
            BinOp::Mul => format!("nv_mul({}, {}, {}, {})", lt, rt, e.line, e.col),
            BinOp::Div => format!("nv_div({}, {}, {}, {})", lt, rt, e.line, e.col),
            BinOp::Mod => format!("nv_mod({}, {}, {}, {})", lt, rt, e.line, e.col),
            BinOp::Eq => format!("nv_bool(nv_eq({}, {}))", lt, rt),
            BinOp::NotEq => format!("nv_bool(!nv_eq({}, {}))", lt, rt),
            BinOp::Lt => format!("nv_bool(nv_lt({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::Gt => format!("nv_bool(nv_gt({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::LtEq => format!("nv_bool(nv_le({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::GtEq => format!("nv_bool(nv_ge({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::In => format!("nv_bool(nv_is_in({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::NotIn => format!("nv_bool(!nv_is_in({}, {}, {}, {}))", lt, rt, e.line, e.col),
            BinOp::And | BinOp::Or => unreachable!(),
        };
        let needs_root = !matches!(op, BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::In | BinOp::NotIn);
        if needs_root {
            let n = self.root_live(out);
            out.push_str(&format!("NvValue {} = {};\n", t, call));
            self.unroot(out, n);
        } else {
            out.push_str(&format!("NvValue {} = {};\n", t, call));
        }
        self.release(&lt);
        self.release(&rt);
        self.live.push(t.clone());
        t
    }

    fn emit_lambda(
        &mut self,
        params: &[String],
        param_types: &[Option<String>],
        body: &Expr,
        e: &Expr,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> String {
        let fname = self.fresh_func();
        // جسم الدالة المجهولة: ارجع التعبير وحده.
        let mut fbody = String::new();
        fbody.push_str(&format!(
            "static NvValue {}(NvScope *frame, NvValue *args, int nargs, int line, int col) {{\n",
            fname
        ));
        fbody.push_str("(void)args; (void)nargs; (void)line; (void)col;\n");
        fbody.push_str("NvValue _ret = nv_nil();\n");
        fbody.push_str(&format!("nv_cur_file_idx = {};\n", ctx.file_idx));
        let mut fctx = Ctx::top("frame", "_end");
        fctx.in_function = true;
        fctx.module_name = ctx.module_name.clone();
        fctx.module_scope_var = ctx.module_scope_var.clone();
        fctx.func_depth = ctx.func_depth + 1;
        fctx.file_idx = ctx.file_idx;
        let saved_live = self.enter_nested_fn();
        let vt = self.eval_into_temp(body, &mut fctx, &mut fbody);
        fbody.push_str(&format!("_ret = {};\n", vt));
        self.exit_nested_fn(saved_live);
        fbody.push_str("nv_flow = NV_FLOW_RETURN;\n");
        fbody.push_str("_end:;\nreturn _ret;\n}\n");
        self.funcs.push_str(&fbody);
        // قيمة الدالة (تلتقط النطاق الحالي).
        let t = self.new_temp();
        let n = self.root_live(out);
        let (pnames, ptypes) = self.param_tables(params, param_types);
        let (mod_arg, mod_scope) = self.func_module_args(ctx);
        out.push_str(&format!(
            "NvValue {} = nv_func_new({}, {}, {}, {}, {}, {}, {}, {}, {});\n",
            t,
            c_str("مجهولة"),
            fname,
            ctx.scope_var,
            mod_arg,
            mod_scope,
            pnames,
            ptypes,
            "NULL",
            params.len()
        ));
        self.unroot(out, n);
        self.live.push(t.clone());
        let _ = e;
        t
    }

    /// جداول أسماء/أنواع المعاملات كحرفيات C (أو NULL).
    fn param_tables(&mut self, params: &[String], param_types: &[Option<String>]) -> (String, String) {
        if params.is_empty() {
            return ("NULL".to_string(), "NULL".to_string());
        }
        let names: Vec<String> = params.iter().map(|p| c_str(p)).collect();
        let types: Vec<String> = param_types
            .iter()
            .map(|t| match t {
                Some(s) => c_str(s),
                None => "NULL".to_string(),
            })
            .collect();
        let n_id = self.tmp;
        self.tmp += 1;
        let names_lit = format!(
            "(char *[]){{{}}}",
            names.join(", ")
        );
        let types_lit = format!(
            "(char *[]){{{}}}",
            types.join(", ")
        );
        let _ = n_id;
        (names_lit, types_lit)
    }

    /// وسائط الوحدة لدالة تُعرَّف الآن: (اسم الوحدة أو NULL،
    /// نطاق الوحدة أو NULL). تُعلَّم دوال المستوى الأعلى
    /// للوحدة فقط (سلسلة ملتقَطة طولها 1).
    fn func_module_args(&mut self, ctx: &Ctx) -> (String, String) {
        if ctx.func_depth == 0 {
            if let (Some(m), Some(s)) = (&ctx.module_name, &ctx.module_scope_var) {
                return (c_str(m), s.clone());
            }
        }
        ("NULL".to_string(), "NULL".to_string())
    }

    fn emit_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        e: &Expr,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> String {
        // استدعاء باسم معرّف: النطاق أولا ثم المدمجة.
        if let ExprKind::Ident(name) = &callee.kind {
            let mut arg_tmps = Vec::new();
            for a in args {
                arg_tmps.push(self.eval_into_temp(a, ctx, out));
            }
            let t = self.new_temp();
            let n = self.root_live(out);
            if arg_tmps.is_empty() {
                out.push_str(&format!(
                    "NvValue {} = nv_call_ident_at({}, {}, NULL, 0, {}, {}, {}, {});\n",
                    t,
                    ctx.scope_var,
                    c_str(name),
                    e.line,
                    e.col,
                    callee.line,
                    callee.col
                ));
            } else {
                let arr = format!("_args{}", self.tmp);
                self.tmp += 1;
                out.push_str(&format!(
                    "NvValue {}[] = {{{}}};\n",
                    arr,
                    arg_tmps.join(", ")
                ));
                out.push_str(&format!(
                    "NvValue {} = nv_call_ident_at({}, {}, {}, {}, {}, {}, {}, {});\n",
                    t,
                    ctx.scope_var,
                    c_str(name),
                    arr,
                    arg_tmps.len(),
                    e.line,
                    e.col,
                    callee.line,
                    callee.col
                ));
            }
            self.unroot(out, n);
            for a in arg_tmps {
                self.release(&a);
            }
            self.live.push(t.clone());
            return t;
        }
        // استدعاء بنقطة على نص/قائمة: طريقة مدمجة.
        if let ExprKind::Dot { target, name } = &callee.kind {
            let is_plain_var = matches!(target.kind, ExprKind::Ident(_));
            let target_name = match &target.kind {
                ExprKind::Ident(nm) => Some(nm.clone()),
                _ => None,
            };
            let is_mutating = name == "أضف" || name == "أزل";
            // هدف غير متغير عادٍ مع طريقة معدِّلة: الفحص
            // ستاتيكي (العدد معروف وقت البناء؛ والعـدد أولا
            // ثم فحص الهدف — كما في المفسر).
            if is_mutating && !is_plain_var {
                if name == "أضف" && args.len() != 1 {
                    out.push_str(&format!(
                        "nv_error_fmt(NV_ERR_RUNTIME, {}, {}, \"الطريقة أضف تتوقع 1 معاملات لكن أعطيت %d\", {});\n",
                        e.line, e.col, args.len()
                    ));
                } else if name == "أزل" && args.len() > 1 {
                    out.push_str(&format!(
                        "nv_error_fmt(NV_ERR_RUNTIME, {}, {}, \"الطريقة أزل تتوقع 0 أو 1 معاملات لكن أعطيت %d\", {});\n",
                        e.line, e.col, args.len()
                    ));
                } else {
                    out.push_str(&format!(
                        "nv_error(NV_ERR_RUNTIME, \"طريقة معدلة للقائمة (أضف/أزل) تحتاج متغيرا عاديا، لا نتيجة تعبير\", {}, {});\n",
                        e.line, e.col
                    ));
                }
                let t = self.new_temp();
                out.push_str(&format!("NvValue {} = nv_nil();\n", t));
                self.live.push(t.clone());
                return t;
            }
            let tt = self.eval_into_temp(target, ctx, out);
            let mut arg_tmps = Vec::new();
            for a in args {
                arg_tmps.push(self.eval_into_temp(a, ctx, out));
            }
            let t = self.new_temp();
            let n = self.root_live(out);
            if arg_tmps.is_empty() {
                out.push_str(&format!(
                    "NvValue {} = nv_call_dot_at({}, {}, NULL, 0, {}, {}, {}, {});\n",
                    t, tt, c_str(name), e.line, e.col, callee.line, callee.col
                ));
            } else {
                let arr = format!("_args{}", self.tmp);
                self.tmp += 1;
                out.push_str(&format!(
                    "NvValue {}[] = {{{}}};\n",
                    arr,
                    arg_tmps.join(", ")
                ));
                out.push_str(&format!(
                    "NvValue {} = nv_call_dot_at({}, {}, {}, {}, {}, {}, {}, {});\n",
                    t,
                    tt,
                    c_str(name),
                    arr,
                    arg_tmps.len(),
                    e.line,
                    e.col,
                    callee.line,
                    callee.col
                ));
            }
            self.unroot(out, n);
            for a in arg_tmps {
                self.release(&a);
            }
            // الكتابة العائدة: طريقة معدِّلة على متغير عادٍ
            // تكتب القائمة (المعدَّلة في مكانها) إلى المتغير.
            if is_mutating {
                if let Some(vn) = target_name {
                    let n2 = self.root_live(out);
                    out.push_str(&format!(
                        "nv_scope_assign({}, {}, {}, {}, {});\n",
                        ctx.scope_var,
                        c_str(&vn),
                        tt,
                        e.line,
                        e.col
                    ));
                    self.unroot(out, n2);
                }
            }
            self.release(&tt);
            self.live.push(t.clone());
            return t;
        }
        // استدعاء عام: قيّم المستدعى ثم استدعه.
        let ct = self.eval_into_temp(callee, ctx, out);
        let mut arg_tmps = Vec::new();
        for a in args {
            arg_tmps.push(self.eval_into_temp(a, ctx, out));
        }
        let t = self.new_temp();
        let n = self.root_live(out);
        if arg_tmps.is_empty() {
            out.push_str(&format!(
                "NvValue {} = nv_call_at({}, NULL, 0, {}, {}, {}, {});\n",
                t, ct, e.line, e.col, callee.line, callee.col
            ));
        } else {
            let arr = format!("_args{}", self.tmp);
            self.tmp += 1;
            out.push_str(&format!(
                "NvValue {}[] = {{{}}};\n",
                arr,
                arg_tmps.join(", ")
            ));
            out.push_str(&format!(
                "NvValue {} = nv_call_at({}, {}, {}, {}, {}, {}, {});\n",
                t,
                ct,
                arr,
                arg_tmps.len(),
                e.line,
                e.col,
                callee.line,
                callee.col
            ));
        }
        self.unroot(out, n);
        for a in arg_tmps {
            self.release(&a);
        }
        self.release(&ct);
        self.live.push(t.clone());
        t
    }
}

impl Gen {
    // ---------- الجمل ----------

    fn emit_stmts(&mut self, stmts: &[Stmt], ctx: &mut Ctx, out: &mut String) -> RioResult<()> {
        for s in stmts {
            self.emit_stmt(s, ctx, out)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, s: &Stmt, ctx: &mut Ctx, out: &mut String) -> RioResult<()> {
        let mark = self.checkpoint_live();
        // اجتماع 33: كل جملة تضبط ملف التنفيذ الجاري
        // قبل تنفيذها، فيسمّى الملف الصحيح عند أي خطأ
        // يقع فيها أو في دالة تستدعيها — حتى بعد التقاط
        // استثناء (الاستعادة تفوتها القفزة).
        out.push_str(&format!("nv_cur_file_idx = {};\n", ctx.file_idx));
        match &s.kind {
            StmtKind::ExprStmt { expr } => {
                let _ = self.eval_into_temp(expr, ctx, out);
            }
            StmtKind::Assign {
                target,
                value,
                annotation,
                append,
            } => {
                self.emit_assign(target, value, annotation, *append, s, ctx, out);
            }
            StmtKind::Print { args } => {
                let mut arg_tmps = Vec::new();
                for a in args {
                    arg_tmps.push(self.eval_into_temp(a, ctx, out));
                }
                let n = self.root_live(out);
                if arg_tmps.is_empty() {
                    out.push_str("nv_print_args(NULL, 0);\n");
                } else {
                    let arr = format!("_args{}", self.tmp);
                    self.tmp += 1;
                    out.push_str(&format!(
                        "NvValue {}[] = {{{}}};\n",
                        arr,
                        arg_tmps.join(", ")
                    ));
                    out.push_str(&format!(
                        "nv_print_args({}, {});\n",
                        arr,
                        arg_tmps.len()
                    ));
                }
                self.unroot(out, n);
                for a in arg_tmps {
                    self.release(&a);
                }
            }
            StmtKind::Return { value } => {
                if let Some(v) = value {
                    let vt = self.eval_into_temp(v, ctx, out);
                    self.emit_exit(ctx, out, "NV_FLOW_RETURN", Some(&vt))?;
                } else {
                    self.emit_exit(ctx, out, "NV_FLOW_RETURN", None)?;
                }
            }
            StmtKind::Break => {
                self.emit_exit(ctx, out, "NV_FLOW_BREAK", None)?;
            }
            StmtKind::Continue => {
                self.emit_exit(ctx, out, "NV_FLOW_CONTINUE", None)?;
            }
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                // سلسلة متداخلة: كل شرط يُقيَّم داخل كتلة else
                // الخاصة به، فلا يقع تقييم بين } وelse أبدا.
                let ct = self.eval_into_temp(cond, ctx, out);
                out.push_str(&format!(
                    "if (nv_check_bool({}, \"شرط إذا\", {}, {})) {{\n",
                    ct, cond.line, cond.col
                ));
                self.release(&ct);
                self.emit_stmts(then_body, ctx, out)?;
                out.push_str("} else {\n");
                for (ec, eb) in elifs {
                    let et = self.eval_into_temp(ec, ctx, out);
                    out.push_str(&format!(
                        "if (nv_check_bool({}, \"شرط وإلا إذا\", {}, {})) {{\n",
                        et, ec.line, ec.col
                    ));
                    self.release(&et);
                    self.emit_stmts(eb, ctx, out)?;
                    out.push_str("} else {\n");
                }
                if let Some(eb) = else_body {
                    self.emit_stmts(eb, ctx, out)?;
                }
                for _ in elifs {
                    out.push_str("}\n");
                }
                out.push_str("}\n");
            }
            StmtKind::While { cond, body } => {
                self.emit_while(cond, body, s, ctx, out)?;
            }
            StmtKind::For {
                var,
                iterable,
                body,
            } => {
                self.emit_for(var, iterable, body, s, ctx, out)?;
            }
            StmtKind::RepeatCount { count, body } => {
                self.emit_repeat(count, body, s, ctx, out)?;
            }
            StmtKind::RepeatWhile { cond, body } => {
                self.emit_while(cond, body, s, ctx, out)?;
            }
            StmtKind::Pass => {}
            StmtKind::FuncDef {
                name,
                params,
                param_types,
                return_type,
                body,
            } => {
                let fname = self.fresh_func();
                let mut fbody = String::new();
                fbody.push_str(&format!(
                    "static NvValue {}(NvScope *frame, NvValue *args, int nargs, int line, int col) {{\n",
                    fname
                ));
                fbody.push_str("(void)args; (void)nargs; (void)line; (void)col;\n");
                fbody.push_str("NvValue _ret = nv_nil();\n");
        fbody.push_str(&format!("nv_cur_file_idx = {};\n", ctx.file_idx));
                let mut fctx = Ctx::top("frame", "_end");
                fctx.in_function = true;
                fctx.module_name = ctx.module_name.clone();
                fctx.module_scope_var = ctx.module_scope_var.clone();
                fctx.func_depth = ctx.func_depth + 1;
        fctx.file_idx = ctx.file_idx;
                let saved_live = self.enter_nested_fn();
                self.emit_stmts(body, &mut fctx, &mut fbody)?;
                self.exit_nested_fn(saved_live);
                fbody.push_str("_end:;\nreturn _ret;\n}\n");
                self.funcs.push_str(&fbody);
                let t = self.new_temp();
                let n = self.root_live(out);
                let (pnames, ptypes) = self.param_tables(params, param_types);
                let (mod_arg, mod_scope) = self.func_module_args(ctx);
                out.push_str(&format!(
                    "NvValue {} = nv_func_new({}, {}, {}, {}, {}, {}, {}, {}, {});\n",
                    t,
                    c_str(name),
                    fname,
                    ctx.scope_var,
                    mod_arg,
                    mod_scope,
                    pnames,
                    ptypes,
                    return_type.as_deref().map(c_str).unwrap_or_else(|| "NULL".to_string()),
                    params.len()
                ));
                self.unroot(out, n);
                let n2 = self.root_live(out);
                // الدفعة 4 — المتغيرات (اجتماع 58 §73):
                // إعادة تعريف اسم مجمد مرفوضة.
                out.push_str(&format!(
                    "nv_check_not_frozen({}, {}, {});\n",
                    c_str(name),
                    s.line,
                    s.col
                ));
                out.push_str(&format!(
                    "nv_scope_define({}, {}, {});\n",
                    ctx.scope_var,
                    c_str(name),
                    t
                ));
                self.unroot(out, n2);
                self.release(&t);
            }
            StmtKind::ClassDef {
                name,
                base,
                interfaces,
                methods,
            } => {
                self.emit_class(name, base, interfaces, methods, s, ctx, out)?;
            }
            StmtKind::InterfaceDef { name, methods } => {
                let t = self.new_temp();
                let n = self.root_live(out);
                out.push_str(&format!(
                    "NvValue {} = nv_iface_new({});\n",
                    t,
                    c_str(name)
                ));
                self.unroot(out, n);
                self.live.push(t.clone());
                for (mname, marity) in methods {
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "nv_iface_add_method({}, {}, {});\n",
                        t,
                        c_str(mname),
                        marity
                    ));
                    self.unroot(out, n);
                }
                let n = self.root_live(out);
                out.push_str(&format!(
                    "nv_scope_define({}, {}, {});\n",
                    ctx.scope_var,
                    c_str(name),
                    t
                ));
                self.unroot(out, n);
                self.release(&t);
            }
            StmtKind::Import {
                module,
                alias,
                names,
            } => {
                self.emit_import(module, alias, names, s, ctx, out)?;
            }
            StmtKind::Export { .. } => {
                // لا أثر وقت التشغيل (الصادرات جُمعت مسبقا من
                // المستوى الأعلى — كما في المفسر).
            }
            StmtKind::Try {
                try_body,
                except_body,
                finally_body,
            } => {
                self.emit_try(try_body, except_body, finally_body, s, ctx, out)?;
            }
        }
        self.restore_live(mark);
        Ok(())
    }

    // ---------- الخروج (ارجع/اكسر/تابع): أخيرا ثم القفز ----------
    // يفرغ المعالجات والأخيرا من الأعمق للأقرب، ثم يضبط
    // التدفق ويقفز إلى تسمية نهاية الدالة/الحلقة.
    fn emit_exit(
        &mut self,
        ctx: &mut Ctx,
        out: &mut String,
        flow: &str,
        ret_temp: Option<&str>,
    ) -> RioResult<()> {
        // احفظ قيمة الإرجاع (إن وجدت) في _ret أولا.
        if let Some(vt) = ret_temp {
            out.push_str(&format!("_ret = {};\n", vt));
        }
        // فرّغ المعالجات: لكل محاولة (من الأعمق) شغّل
        // أخيرا ثم أزل معالجها (الخروج العادي لا longjmp،
        // فالإزالة يدوية هنا).
        let tries: Vec<TryCtx> = ctx.try_stack.clone();
        for tr in tries.iter().rev() {
            if let Some(fin) = &tr.finally_body {
                let fmark = self.checkpoint_live();
                let mut fctx = Ctx {
                    scope_var: ctx.scope_var.clone(),
                    in_function: ctx.in_function,
                    func_end: ctx.func_end.clone(),
                    loop_stack: ctx.loop_stack.clone(),
                    try_stack: Vec::new(),
                    module_name: ctx.module_name.clone(),
                    module_scope_var: ctx.module_scope_var.clone(),
                    func_depth: ctx.func_depth,
                    file_idx: ctx.file_idx,
                };
                self.emit_stmts(fin, &mut fctx, out)?;
                self.restore_live(fmark);
            }
            out.push_str(&format!("nv_handlers = {}.prev;\n", tr.handler));
        }
        out.push_str(&format!("nv_flow = {};\n", flow));
        // قفز إلى نهاية الدالة (ارجع) أو نهاية/رأس الحلقة.
        match flow {
            "NV_FLOW_RETURN" => {
                out.push_str(&format!("goto {};\n", ctx.func_end));
            }
            "NV_FLOW_BREAK" => {
                let l = ctx.loop_stack.last().unwrap();
                out.push_str(&format!("goto {};\n", l.end));
            }
            "NV_FLOW_CONTINUE" => {
                let l = ctx.loop_stack.last().unwrap();
                out.push_str(&format!("goto {};\n", l.head));
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    // ---------- الحلقات ----------

    fn emit_while(&mut self, cond: &Expr, body: &[Stmt], s: &Stmt, ctx: &mut Ctx, out: &mut String) -> RioResult<()> {
        let head = self.fresh_label("lh");
        let end = self.fresh_label("le");
        out.push_str(&format!("long _iter{} = 0;\n", self.tmp));
        self.tmp += 1;
        let iter_var = format!("_iter{}", self.tmp - 1);
        ctx.loop_stack.push(LoopCtx { head: head.clone(), end: end.clone() });
        out.push_str(&format!("{}:;\n", head));
        // الشرط (في كل دورة).
        let ct = self.eval_into_temp(cond, ctx, out);
        out.push_str(&format!(
            "if (!nv_check_bool({}, \"شرط الحلقة\", {}, {})) goto {};\n",
            ct, cond.line, cond.col, end
        ));
        self.release(&ct);
        out.push_str(&format!(
            "if (++{} > 1000000) nv_error(NV_ERR_RUNTIME, \"تم تجاوز حد التكرار، هل الحلقة لا تنتهي؟\", {}, {});\n",
            iter_var, cond.line, cond.col
        ));
        self.emit_stmts(body, ctx, out)?;
        // تدفق عادٍ: ارجع إلى الرأس.
        out.push_str(&format!("goto {};\n", head));
        out.push_str(&format!("{}:;\n", end));
        ctx.loop_stack.pop();
        let _ = s;
        Ok(())
    }

    fn emit_for(&mut self, var: &str, iter: &Expr, body: &[Stmt], s: &Stmt, ctx: &mut Ctx, out: &mut String) -> RioResult<()> {
        let head = self.fresh_label("lh");
        let end = self.fresh_label("le");
        // لقطة القائمة/النص (المحرك ينسخ العناصر أولا).
        let it = self.eval_into_temp(iter, ctx, out);
        let snap = self.new_temp();
        let n = self.root_live(out);
        out.push_str(&format!(
            "NvValue {} = nv_for_snapshot({}, {}, {});\n",
            snap, it, s.line, s.col
        ));
        self.unroot(out, n);
        self.release(&it);
        self.live.push(snap.clone());
        let idx = format!("_fi{}", self.tmp);
        self.tmp += 1;
        out.push_str(&format!("size_t {} = 0;\n", idx));
        ctx.loop_stack.push(LoopCtx { head: head.clone(), end: end.clone() });
        out.push_str(&format!("{}:;\n", head));
        out.push_str(&format!(
            "if ({} >= {}.u.list->len) goto {};\n",
            idx, snap, end
        ));
        // اربط متغير الحلقة.
        let vt = self.new_temp();
        out.push_str(&format!(
            "NvValue {} = {}.u.list->items[{}];\n",
            vt, snap, idx
        ));
        self.live.push(vt.clone());
        let n = self.root_live(out);
        out.push_str(&format!(
            "nv_scope_define({}, {}, {});\n",
            ctx.scope_var,
            c_str(var),
            vt
        ));
        self.unroot(out, n);
        self.release(&vt);
        self.emit_stmts(body, ctx, out)?;
        out.push_str(&format!("{}++;\n", idx));
        out.push_str(&format!("goto {};\n", head));
        out.push_str(&format!("{}:;\n", end));
        ctx.loop_stack.pop();
        self.release(&snap);
        let _ = s;
        Ok(())
    }

    fn emit_repeat(&mut self, count: &Expr, body: &[Stmt], s: &Stmt, ctx: &mut Ctx, out: &mut String) -> RioResult<()> {
        let head = self.fresh_label("lh");
        let end = self.fresh_label("le");
        let ct = self.eval_into_temp(count, ctx, out);
        let n = self.root_live(out);
        out.push_str(&format!(
            "long _rep{} = nv_repeat_count({}, {}, {});\n",
            self.tmp, ct, s.line, s.col
        ));
        self.tmp += 1;
        self.unroot(out, n);
        self.release(&ct);
        let rep_var = format!("_rep{}", self.tmp - 1);
        let idx = format!("_ri{}", self.tmp);
        self.tmp += 1;
        out.push_str(&format!("long {} = 0;\n", idx));
        ctx.loop_stack.push(LoopCtx { head: head.clone(), end: end.clone() });
        out.push_str(&format!("{}:;\n", head));
        out.push_str(&format!("if ({} >= {}) goto {};\n", idx, rep_var, end));
        self.emit_stmts(body, ctx, out)?;
        out.push_str(&format!("{}++;\n", idx));
        out.push_str(&format!("goto {};\n", head));
        out.push_str(&format!("{}:;\n", end));
        ctx.loop_stack.pop();
        let _ = s;
        Ok(())
    }

    // ---------- التعيين ----------

    fn emit_assign(
        &mut self,
        target: &AssignTarget,
        value: &Expr,
        annotation: &Option<String>,
        append: bool,
        s: &Stmt,
        ctx: &mut Ctx,
        out: &mut String,
    ) {
        let vt = self.eval_into_temp(value, ctx, out);
        // فحص النوع المعلن قبل التعيين (رسالة المحرك).
        if let Some(ann) = annotation {
            let var_name = match target {
                AssignTarget::Var(n) => n.clone(),
                _ => String::new(),
            };
            out.push_str(&format!(
                "nv_check_assign_type({}, {}, {}, {}, {});\n",
                vt,
                c_str(ann),
                c_str(&var_name),
                s.line,
                s.col
            ));
        }
        match target {
            AssignTarget::Var(name) => {
                let n = self.root_live(out);
                if append {
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): الإلحاق — القديم من النطاق
                    // أو الهوية من نوع الجديد، ثم `+`.
                    let cell = format!("_cell{}", self.tmp);
                    self.tmp += 1;
                    let old = format!("_old{}", self.tmp);
                    self.tmp += 1;
                    let joined = format!("_new{}", self.tmp);
                    self.tmp += 1;
                    out.push_str(&format!(
                        "NvCell *{} = nv_scope_lookup_cell({}, {});\n",
                        cell,
                        ctx.scope_var,
                        c_str(name)
                    ));
                    out.push_str(&format!("NvValue {};\n", old));
                    out.push_str(&format!("if ({} == NULL) {{\n", cell));
                    out.push_str(&format!(
                        "if ({}.tag == NV_NUM) {} = nv_num(0);\n",
                        vt, old
                    ));
                    out.push_str(&format!(
                        "else if ({}.tag == NV_STR) {} = nv_str_cstr(\"\");\n",
                        vt, old
                    ));
                    out.push_str(&format!(
                        "else if ({}.tag == NV_LIST) {} = nv_list_new();\n",
                        vt, old
                    ));
                    out.push_str(&format!(
                        "else if ({}.tag == NV_DICT) {} = nv_dict_new();\n",
                        vt, old
                    ));
                    out.push_str(&format!(
                        "else nv_error_fmt(NV_ERR_TYPE, {}, {}, \"لا يمكن الإلحاق بقيمة %s\", nv_type_name({}));\n",
                        s.line, s.col, vt
                    ));
                    out.push_str(&format!(
                        "}} else {{ {} = {}->val; }}\n",
                        old, cell
                    ));
                    out.push_str(&format!(
                        "NvValue {} = nv_add({}, {}, {}, {});\n",
                        joined, old, vt, s.line, s.col
                    ));
                    // جذّر الناتج قبل التعيين (قد يخصص
                    // التعيين فيجمع — ككل مؤقت حي).
                    self.live.push(joined.clone());
                    let n2 = self.root_live(out);
                    out.push_str(&format!(
                        "nv_scope_assign({}, {}, {}, {}, {});\n",
                        ctx.scope_var,
                        c_str(name),
                        joined,
                        s.line,
                        s.col
                    ));
                    self.unroot(out, n2);
                    self.release(&joined);
                } else {
                    out.push_str(&format!(
                        "nv_scope_assign({}, {}, {}, {}, {});\n",
                        ctx.scope_var,
                        c_str(name),
                        vt,
                        s.line,
                        s.col
                    ));
                }
                self.unroot(out, n);
                self.release(&vt);
            }
            AssignTarget::Index { target: tgt, index } => {
                // الهدف المعقد مرفوض في المحرك v1 (رسالة
                // المفسر)؛ والقيمة قُيّمت قبل هذا الرفض.
                if let ExprKind::Ident(vn) = &tgt.kind {
                    let it = self.eval_into_temp(index, ctx, out);
                    let n = self.root_live(out);
                    // الدفعة 4 — المتغيرات (اجتماع 58
                    // §73): الفهرسة في مجمد مرفوضة.
                    out.push_str(&format!(
                        "nv_check_not_frozen({}, {}, {});\n",
                        c_str(vn),
                        s.line,
                        s.col
                    ));
                    out.push_str(&format!(
                        "nv_index_assign(nv_scope_slot_ptr({}, {}, {}, {}), {}, {}, {}, {}, {}, {});\n",
                        ctx.scope_var, c_str(vn), tgt.line, tgt.col,
                        it, vt, index.line, index.col, tgt.line, tgt.col
                    ));
                    self.unroot(out, n);
                    self.release(&it);
                    self.release(&vt);
                } else {
                    out.push_str(&format!(
                        "nv_error(NV_ERR_PARSE, \"تعيين الفهرسة المعقدة غير مدعوم في المحرك v1\", {}, {});\n",
                        s.line, s.col
                    ));
                }
            }
            AssignTarget::Dot { target: tgt, name } => {
                let tt = self.eval_into_temp(tgt, ctx, out);
                let n = self.root_live(out);
                out.push_str(&format!(
                    "nv_dot_assign({}, {}, {}, {}, {});\n",
                    tt,
                    c_str(name),
                    vt,
                    s.line,
                    s.col
                ));
                self.unroot(out, n);
                self.release(&tt);
                self.release(&vt);
            }
        }
    }

    // ---------- الفئات ----------

    fn emit_class(
        &mut self,
        name: &str,
        base: &Option<String>,
        interfaces: &[String],
        methods: &[Stmt],
        s: &Stmt,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> RioResult<()> {
        // ابدأ الفئة (تسوية القاعدة والواجهات وقت التشغيل).
        let t = self.new_temp();
        let n = self.root_live(out);
        if let Some(b) = base {
            out.push_str(&format!(
                "NvValue {} = nv_class_begin_named({}, {}, {}, {}, {});\n",
                t,
                ctx.scope_var,
                c_str(name),
                c_str(b),
                s.line,
                s.col
            ));
        } else {
            out.push_str(&format!("NvValue {} = nv_class_new({}, NULL);\n", t, c_str(name)));
        }
        self.unroot(out, n);
        self.live.push(t.clone());
        // عرّف الفئة أولا (لتستدعي نفسها في طرائقها).
        let n = self.root_live(out);
        out.push_str(&format!(
            "nv_scope_define({}, {}, {});\n",
            ctx.scope_var,
            c_str(name),
            t
        ));
        self.unroot(out, n);
        // الطرائق (كل طريقة جملة FuncDef — استخرجها).
        for mstmt in methods {
            let m = match &mstmt.kind {
                StmtKind::FuncDef {
                    name,
                    params,
                    param_types,
                    return_type,
                    body,
                } => (name, params, param_types, return_type, body),
                _ => continue,
            };
            let (mname, mparams, mparam_types, mreturn_type, mbody) = m;
            let fname = self.fresh_func();
            let mut fbody = String::new();
            fbody.push_str(&format!(
                "static NvValue {}(NvScope *frame, NvValue *args, int nargs, int line, int col) {{\n",
                fname
            ));
            fbody.push_str("(void)args; (void)nargs; (void)line; (void)col;\n");
            fbody.push_str("NvValue _ret = nv_nil();\n");
        fbody.push_str(&format!("nv_cur_file_idx = {};\n", ctx.file_idx));
            let mut fctx = Ctx::top("frame", "_end");
            fctx.in_function = true;
            fctx.module_name = ctx.module_name.clone();
            fctx.module_scope_var = ctx.module_scope_var.clone();
            fctx.func_depth = ctx.func_depth + 1;
        fctx.file_idx = ctx.file_idx;
            let saved_live = self.enter_nested_fn();
            self.emit_stmts(mbody, &mut fctx, &mut fbody)?;
            self.exit_nested_fn(saved_live);
            fbody.push_str("_end:;\nreturn _ret;\n}\n");
            self.funcs.push_str(&fbody);
            let mt = self.new_temp();
            let n = self.root_live(out);
            let (pnames, ptypes) = self.param_tables(mparams, mparam_types);
            out.push_str(&format!(
                "NvValue {} = nv_func_new({}, {}, {}, NULL, NULL, {}, {}, {}, {});\n",
                mt,
                c_str(mname),
                fname,
                ctx.scope_var,
                pnames,
                ptypes,
                mreturn_type.as_deref().map(c_str).unwrap_or_else(|| "NULL".to_string()),
                mparams.len()
            ));
            self.unroot(out, n);
            let n = self.root_live(out);
            out.push_str(&format!(
                "nv_class_add_method({}, {}, {});\n",
                t,
                c_str(mname),
                mt
            ));
            self.unroot(out, n);
            self.release(&mt);
        }
        // الواجهات (الفحص بعد إضافة الطرائق كلها — يتحقق
        // أن الفئة تنفذ طرق الواجهة بعدد معاملاتها).
        for iface in interfaces {
            let n = self.root_live(out);
            out.push_str(&format!(
                "nv_class_add_interface({}, {}, nv_lookup_or_nil({}, {}), {}, {});\n",
                t,
                c_str(iface),
                ctx.scope_var,
                c_str(iface),
                s.line,
                s.col
            ));
            self.unroot(out, n);
        }
        self.release(&t);
        Ok(())
    }

    // ---------- حاول/ماعدا/أخيرا ----------

    fn emit_try(
        &mut self,
        body: &[Stmt],
        except: &Option<Vec<Stmt>>,
        finally: &Option<Vec<Stmt>>,
        s: &Stmt,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> RioResult<()> {
        // كل محاولة تفعل setjmp — حتى أخيرا وحدها — لأن
        // nv_error يقفز إلى المعالج الأعمق، وفرع الخطأ
        // هو الذي يشغّل أخيرا ثم يعيد الرفع.
        let hid = self.fresh_label("h");
        let cpid = self.fresh_label("cp");
        out.push_str(&format!("NvHandler {};\n", hid));
        out.push_str(&format!("NvCheckpoint {} = nv_checkpoint();\n", cpid));
        out.push_str(&format!("{}.prev = nv_handlers;\n", hid));
        out.push_str(&format!("{}.has_except = {};\n", hid, if except.is_some() { 1 } else { 0 }));
        out.push_str(&format!("nv_handlers = &{};\n", hid));
        ctx.try_stack.push(TryCtx {
            handler: hid.clone(),
            finally_body: finally.clone(),
        });
        let has_except = except.is_some();
        out.push_str(&format!("if (setjmp({}.jb) == 0) {{\n", hid));
        self.emit_stmts(body, ctx, out)?;
        out.push_str("} else {\n");
        // مسار الخطأ: استعد النقاط. (nv_error أزال المعالج
        // نفسه قبل القفز، فـ nv_handlers تحته الآن.)
        out.push_str(&format!("nv_restore_checkpoint({});\n", cpid));
        // الدفعة 1: الخروج لا تمسكه ماعدا — أخيرا
        // وحدها ثم إعادة رفع الطلب إلى المعالج الأعمق.
        out.push_str("if (nv_exit_requested) {\n");
        if let Some(fin) = finally {
            let fmark = self.checkpoint_live();
            let mut fctx = Ctx {
                scope_var: ctx.scope_var.clone(),
                in_function: ctx.in_function,
                func_end: ctx.func_end.clone(),
                loop_stack: ctx.loop_stack.clone(),
                try_stack: Vec::new(),
                module_name: ctx.module_name.clone(),
                module_scope_var: ctx.module_scope_var.clone(),
                func_depth: ctx.func_depth,
                file_idx: ctx.file_idx,
            };
            self.emit_stmts(fin, &mut fctx, out)?;
            self.restore_live(fmark);
        }
        out.push_str("nv_reraise_current();\n");
        out.push_str("}\n");
        if has_except {
            // معالج فرعي يحمي ماعدا: إن أخطأت ماعدا شغّل
            // أخيرا ثم أعد الرفع إلى ما تحت هذه المحاولة.
            let sub = self.fresh_label("hs");
            let subcp = self.fresh_label("cps");
            out.push_str(&format!("NvHandler {};\n", sub));
            out.push_str(&format!("NvCheckpoint {} = nv_checkpoint();\n", subcp));
            out.push_str(&format!("{}.prev = nv_handlers;\n", sub));
            out.push_str(&format!("{}.has_except = 0;\n", sub));
            out.push_str(&format!("nv_handlers = &{};\n", sub));
            out.push_str(&format!("if (setjmp({}.jb) == 0) {{\n", sub));
            let ebody = except.as_ref().unwrap();
            // الدفعة 1: الخطأ الممسوك رمز خروج 1 —
            // يُضبط قبل جسم ماعدا، فتقرأه
            // `رمز_الخروج()` داخله.
            out.push_str("nv_last_exit_code = 1;\n");
            self.emit_stmts(ebody, ctx, out)?;
            out.push_str("} else {\n");
            out.push_str(&format!("nv_restore_checkpoint({});\n", subcp));
            if let Some(fin) = finally {
                let fmark = self.checkpoint_live();
                let mut fctx = Ctx {
                    scope_var: ctx.scope_var.clone(),
                    in_function: ctx.in_function,
                    func_end: ctx.func_end.clone(),
                    loop_stack: ctx.loop_stack.clone(),
                    try_stack: Vec::new(),
                    module_name: ctx.module_name.clone(),
                    module_scope_var: ctx.module_scope_var.clone(),
                    func_depth: ctx.func_depth,
                    file_idx: ctx.file_idx,
                };
                self.emit_stmts(fin, &mut fctx, out)?;
                self.restore_live(fmark);
            }
            // (المعالج الفرعي أزيله nv_error أصلا؛ والرفع
            // يقفز إلى ما تحت المحاولة الحالية.)
            out.push_str("nv_reraise_current();\n");
            out.push_str("}\n");
            out.push_str(&format!("nv_handlers = {}.prev;\n", sub));
        } else {
            // بلا ماعدا: أخيرا ثم أعد الرفع.
            if let Some(fin) = finally {
                let fmark = self.checkpoint_live();
                let mut fctx = Ctx {
                    scope_var: ctx.scope_var.clone(),
                    in_function: ctx.in_function,
                    func_end: ctx.func_end.clone(),
                    loop_stack: ctx.loop_stack.clone(),
                    try_stack: Vec::new(),
                    module_name: ctx.module_name.clone(),
                    module_scope_var: ctx.module_scope_var.clone(),
                    func_depth: ctx.func_depth,
                    file_idx: ctx.file_idx,
                };
                self.emit_stmts(fin, &mut fctx, out)?;
                self.restore_live(fmark);
            }
            out.push_str("nv_reraise_current();\n");
        }
        out.push_str("}\n");
        // المسار العادي (أو بعد ماعدا ناجحة): أخيرا ثم أزل المعالج.
        if let Some(fin) = finally {
            let fmark = self.checkpoint_live();
            let mut fctx = Ctx {
                scope_var: ctx.scope_var.clone(),
                in_function: ctx.in_function,
                func_end: ctx.func_end.clone(),
                loop_stack: ctx.loop_stack.clone(),
                try_stack: Vec::new(),
                module_name: ctx.module_name.clone(),
                module_scope_var: ctx.module_scope_var.clone(),
                func_depth: ctx.func_depth,
                file_idx: ctx.file_idx,
            };
            self.emit_stmts(fin, &mut fctx, out)?;
            self.restore_live(fmark);
        }
        out.push_str(&format!("nv_handlers = {}.prev;\n", hid));
        ctx.try_stack.pop();
        let _ = s;
        Ok(())
    }
}

impl Gen {
    // ---------- الاستيراد ----------

    fn emit_import(
        &mut self,
        module: &str,
        alias: &Option<String>,
        names: &Option<Vec<String>>,
        s: &Stmt,
        ctx: &mut Ctx,
        out: &mut String,
    ) -> RioResult<()> {
        // الأصلية أولا (لا يظللها ملف أبدا — كما في المفسر).
        if crate::stdlib::native_module(module).is_some() {
            let mt = self.new_temp();
            let n = self.root_live(out);
            out.push_str(&format!(
                "NvValue {} = *nv_native_module({});\n",
                mt,
                c_str(module)
            ));
            self.unroot(out, n);
            self.live.push(mt.clone());
            match names {
                Some(ns) => {
                    for nm in ns {
                        let et = self.new_temp();
                        let n = self.root_live(out);
                        out.push_str(&format!(
                            "NvValue {} = nv_module_export({}, {}, {}, {});\n",
                            et,
                            mt,
                            c_str(nm),
                            s.line,
                            s.col
                        ));
                        self.unroot(out, n);
                        let n = self.root_live(out);
                        out.push_str(&format!(
                            "nv_scope_define({}, {}, {});\n",
                            ctx.scope_var,
                            c_str(nm),
                            et
                        ));
                        self.unroot(out, n);
                        self.release(&et);
                    }
                }
                None => {
                    let bind = alias.clone().unwrap_or_else(|| module.to_string());
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "nv_scope_define({}, {}, {});\n",
                        ctx.scope_var,
                        c_str(&bind),
                        mt
                    ));
                    self.unroot(out, n);
                }
            }
            self.release(&mt);
            return Ok(());
        }
        // وحدة ملف: حلّ المسار وقت البناء (الترتيب نفسه).
        let info = self.ensure_module(module, s.line, s.col, ctx.file_idx)?;
        // غلاف معالج حول تهيئة الوحدة: خطأ مستواها الأعلى
        // يُسبق بـ «في وحدة X: » ثم يُعاد رفعه، كما في
        // المفسر حرفيا (الاستيراد المتداخل يضاعف البادئة
        // كما هناك). النمط نفسه في emit_try.
        let mh = self.fresh_label("mh");
        let prefix = format!("في وحدة {}: ", module);
        out.push_str(&format!(
            "{{ NvHandler {}; {}.prev = nv_handlers; {}.has_except = 0; nv_handlers = &{}; if (setjmp({}.jb) == 0) {{ ",
            mh, mh, mh, mh, mh
        ));
        let bind = alias.clone().unwrap_or_else(|| module.to_string());
        out.push_str(&format!(
            "if (!{}) {{ NvValue _mv = {}(); {} = _mv; {} = 1; }} ",
            info.loaded_flag, info.init_fn, info.val_var, info.loaded_flag
        ));
        out.push_str(&format!(
            "}} else {{ nv_prefix_error({}); nv_reraise_current(); }} nv_handlers = {}.prev; }}\n",
            c_str(&prefix),
            mh
        ));
        match names {
            Some(ns) => {
                for nm in ns {
                    let et = self.new_temp();
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "NvValue {} = nv_module_export({}, {}, {}, {});\n",
                        et,
                        info.val_var,
                        c_str(nm),
                        s.line,
                        s.col
                    ));
                    self.unroot(out, n);
                    let n = self.root_live(out);
                    out.push_str(&format!(
                        "nv_scope_define({}, {}, {});\n",
                        ctx.scope_var,
                        c_str(nm),
                        et
                    ));
                    self.unroot(out, n);
                    self.release(&et);
                }
            }
            None => {
                let n = self.root_live(out);
                out.push_str(&format!(
                    "nv_scope_define({}, {}, {});\n",
                    ctx.scope_var,
                    c_str(&bind),
                    info.val_var
                ));
                self.unroot(out, n);
            }
        }
        Ok(())
    }

    /// اضمن توليد دالة تهيئة وحدة ملف؛ يعيد معلوماتها.
    /// cur_file_idx: ملف الجملة المستورِدة (تُسنَد إليه
    /// أخطاء «غير موجودة» والدائرية والقراءة كما في المفسر).
    fn ensure_module(
        &mut self,
        module: &str,
        line: usize,
        col: usize,
        cur_file_idx: usize,
    ) -> RioResult<ModuleInfo> {
        if let Some(info) = self.modules.get(module) {
            return Ok(ModuleInfo {
                init_fn: info.init_fn.clone(),
                loaded_flag: info.loaded_flag.clone(),
                val_var: info.val_var.clone(),
                file_idx: info.file_idx,
            });
        }
        if self.import_stack.contains(&module.to_string()) {
            let mut cycle = self.import_stack.clone();
            cycle.push(module.to_string());
            return Err(RioError::runtime(
                format!("استيراد دائري: {}", cycle.join(" ← ")),
                line,
                col,
            )
            .with_file_if_none(Some(self.files[cur_file_idx].clone())));
        }
        let found = packages::find_module_file(&self.program_dir, module).map_err(|msg| {
            RioError::new(ErrorKind::Io, msg, line, col)
                .with_file_if_none(Some(self.files[cur_file_idx].clone()))
        })?;
        let path = match found.found {
            Some(p) => p,
            None => {
                let searched = found
                    .searched
                    .iter()
                    .map(|p| crate::permissions::slash_display(p))
                    .collect::<Vec<_>>()
                    .join("؛ ");
                return Err(RioError::module_not_found(module, &searched, line, col)
                    .with_file_if_none(Some(self.files[cur_file_idx].clone())));
            }
        };
        // عرض مسار الوحدة بشرطة مائلة كما في المفسر،
        // وسجّله في جدول ملفات المصدر (أو أعد فهرسه).
        let display = crate::permissions::slash_display(&path);
        let file_idx = match self.files.iter().position(|f| f == &display) {
            Some(i) => i,
            None => {
                self.files.push(display.clone());
                self.files.len() - 1
            }
        };
        let source = std::fs::read_to_string(&path).map_err(|e| {
            RioError::new(
                ErrorKind::Io,
                format!("تعذر قراءة الوحدة {}: {}", module, e),
                line,
                col,
            )
            .with_file_if_none(Some(self.files[cur_file_idx].clone()))
        })?;
        let tokens = tokenize(&source).map_err(|e| {
            RioError::new(
                e.kind,
                format!("في وحدة {}: {}", module, e.message),
                e.line,
                e.col,
            )
            .with_file(display.clone())
        })?;
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().map_err(|e| {
            RioError::new(
                e.kind,
                format!("في وحدة {}: {}", module, e.message),
                e.line,
                e.col,
            )
            .with_file(display.clone())
        })?;
        // الصادرات: من جمل صدّر في المستوى الأعلى فقط.
        let mut exports: Vec<String> = Vec::new();
        for stmt in &program {
            if let StmtKind::Export { names } = &stmt.kind {
                for n in names {
                    if !exports.contains(n) {
                        exports.push(n.clone());
                    }
                }
            }
        }
        let id = self.next_mod;
        self.next_mod += 1;
        let info = ModuleInfo {
            init_fn: format!("nv_mod{}", id),
            loaded_flag: format!("nv_mod_loaded_{}", id),
            val_var: format!("nv_mod_val_{}", id),
            file_idx,
        };
        self.modules.insert(
            module.to_string(),
            ModuleInfo {
                init_fn: info.init_fn.clone(),
                loaded_flag: info.loaded_flag.clone(),
                val_var: info.val_var.clone(),
                file_idx,
            },
        );
        self.import_stack.push(module.to_string());
        let mut body = String::new();
        body.push_str(&format!(
            "static NvValue {}(void) {{\n",
            info.init_fn
        ));
        // ضبط الملف عند دخول تهيئة الوحدة: أخطاء
        // مستواها الأعلى تسمّي ملفها كما في المفسر.
        body.push_str(&format!("nv_cur_file_idx = {};\n", file_idx));
        body.push_str(&format!(
            "nv_add_global_root_ptr(&{});\n",
            info.val_var
        ));
        body.push_str("NvScope *_scope = nv_scope_new(NULL);\n");
        body.push_str("size_t _ri = nv_root_push_idx();\n");
        body.push_str(&format!(
            "NvValue _mod = nv_module_new({}, _scope);\n",
            c_str(module)
        ));
        body.push_str("*nv_root_slot(_ri) = _mod;\n");
        let mut mctx = Ctx::top("_scope", "_mend");
        mctx.module_name = Some(module.to_string());
        mctx.module_scope_var = Some("_scope".to_string());
        mctx.func_depth = 0;
        mctx.file_idx = file_idx;
        let saved_live = self.enter_nested_fn();
        self.emit_stmts(&program, &mut mctx, &mut body)
            .map_err(|e| e.with_file_if_none(Some(display.clone())))?;
        // ابنِ قاموس الصادرات (الأسماء الموجودة فقط) — ما
        // زال داخل جسم الوحدة، فالحية فارغة إلا ext.
        let ext = self.new_temp();
        let n = self.root_live(&mut body);
        body.push_str(&format!("NvValue {} = nv_dict_new();\n", ext));
        self.unroot(&mut body, n);
        self.live.push(ext.clone());
        body.push_str("NvCell *_ec = NULL;\n");
        for ename in &exports {
            let n = self.root_live(&mut body);
            body.push_str(&format!(
                "_ec = nv_scope_lookup_cell(_scope, {});\n",
                c_str(ename)
            ));
            body.push_str(&format!(
                "if (_ec) nv_dict_set_raw({}.u.dict, {}, _ec->val);\n",
                ext,
                c_str(ename)
            ));
            self.unroot(&mut body, n);
        }
        body.push_str(&format!("_mod.u.module->exports = {}.u.dict;\n", ext));
        self.release(&ext);
        body.push_str("nv_root_pop();\n");
        body.push_str("return _mod;\n");
        body.push_str("}\n");
        self.exit_nested_fn(saved_live);
        self.funcs.push_str(&body);
        self.import_stack.pop();
        Ok(info)
    }
}

// ---------- السائق الأعلى ----------

/// ولّد مصدر C كاملا من برنامج نيفا (محلل دلاليا).
/// main_file: عرض مسار الملف الرئيسي بشرطة مائلة كما
/// في المفسر — تُسنَد إليه أخطاء الملف الرئيسي، ويصير
/// [0] في جدول ملفات المصدر المولّد.
pub fn generate_native_c(source: &str, program_dir: &Path, main_file: &str) -> RioResult<String> {
    let main_file = main_file.to_string();
    let tokens =
        tokenize(source).map_err(|e| e.with_file_if_none(Some(main_file.clone())))?;
    let mut parser = Parser::new(tokens);
    let program = parser
        .parse_program()
        .map_err(|e| e.with_file_if_none(Some(main_file.clone())))?;
    let mut analyzer = SemanticAnalyzer::new();
    analyzer
        .analyze_program(&program)
        .map_err(|e| e.with_file_if_none(Some(main_file.clone())))?;
    let mut gen = Gen::new(program_dir.to_path_buf(), main_file);
    let mut body = String::new();
    body.push_str("static void nv_prog_body(NvScope *top) {\n");
    body.push_str("nv_cur_file_idx = 0;\n");
    let mut ctx = Ctx::top("top", "_pend");
    let mark = gen.checkpoint_live();
    gen.emit_stmts(&program, &mut ctx, &mut body)
        .map_err(|e| e.with_file_if_none(Some(gen.files[0].clone())))?;
    gen.restore_live(mark);
    body.push_str("_pend:;\n}\n");
    // إعلانات أمامية (الدوال المولدة تُستدعى قبل تعريفها
    // أحيانا — كدالة تستدعي دالة لاحقة في البرنامج).
    let mut out = String::new();
    out.push_str("#include \"niva_rt.h\"\n");
    out.push_str("#include <stdio.h>\n");
    // جدول ملفات المصدر (اجتماع 33: كل خطأ وقت تشغيل
    // يسمّي ملفه كما في المفسر) — بلا static، يربطه
    // وقت التشغيل عبر extern؛ و[0] الملف الرئيسي.
    out.push_str("const char *nv_source_files[] = {");
    for (i, f) in gen.files.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&c_str(f));
    }
    out.push_str("};\n");
    out.push_str(&format!(
        "const int nv_n_source_files = {};\n",
        gen.files.len()
    ));
    for i in 0..gen.next_func {
        out.push_str(&format!(
            "static NvValue nv_f{}(NvScope *frame, NvValue *args, int nargs, int line, int col);\n",
            i
        ));
    }
    for i in 0..gen.next_mod {
        out.push_str(&format!("static NvValue nv_mod{}(void);\n", i));
        out.push_str(&format!("static int nv_mod_loaded_{} = 0;\n", i));
        out.push_str(&format!("static NvValue nv_mod_val_{};\n", i));
    }
    out.push_str(&gen.funcs);
    out.push_str(&body);
    out.push_str("int main(int argc, char **argv) {\n");
    out.push_str("    return nv_main(argc, argv, nv_prog_body);\n");
    out.push_str("}\n");
    Ok(out)
}

// ---------- بناء الثنائي عبر cc ----------

/// ترجم سطر خطأ cc واحد إلى عربية (الأنماط المعروفة)؛
/// وما لا يُعرف يُسقط (يُحفظ السجل كاملا في ملف).
fn translate_cc_line(line: &str, out: &mut Vec<String>) {
    let l = line.trim();
    if l.is_empty() {
        return;
    }
    // undefined reference to `X'
    if let Some(pos) = l.find("undefined reference to") {
        let rest = &l[pos + "undefined reference to".len()..];
        let sym = rest.trim().trim_matches(|c| c == '`' || c == '\'' || c == '"');
        out.push(format!("مرجع غير معرف: {}", sym));
        return;
    }
    if l.contains("No such file or directory") {
        out.push("ملف أو مجلد غير موجود".to_string());
        return;
    }
    if l.contains("Permission denied") {
        out.push("رفض الإذن".to_string());
        return;
    }
    if l.contains("No space left on device") {
        out.push("لا مساحة متبقية على الجهاز".to_string());
        return;
    }
    if l.contains("cannot find -l") {
        out.push("مكتبة ربط مفقودة".to_string());
        return;
    }
    // سطر مجهول: لا يُعرض إنجليزيا أبدا.
}

/// تعليمات تثبيت cc حسب المنصة (عربية كاملة).
fn cc_install_hint() -> &'static str {
    if cfg!(target_os = "android") {
        "ثبّت المترجم بالأمر: pkg install clang"
    } else if cfg!(target_os = "windows") {
        "ثبّت المترجم MinGW ثم أضفه إلى PATH"
    } else if cfg!(target_os = "macos") {
        "ثبّت أدوات المطور بالأمر: xcode-select --install"
    } else {
        "ثبّت المترجم بالأمر: apt install gcc"
    }
}

/// بيان ويندوز يجعل صفحة أكواد العملية UTF-8
/// (activeCodePage) — تكافؤ C على ويندوز، التشغيل
/// 42706100129: واجهات الملفات الضيقة في وقت
/// التشغيل C تحوّل المسارات عبر صفحة ANSI (1252
/// افتراضيا) فتضيع أسماء الملفات والأدوات العربية
/// (مكان_الاداة يرجع لاشيء ونسخ/لمس الملفات
/// العربية تفشل)؛ وبهذا البيان تصبح UTF-8 فيطابق
/// الأصلي المفسر. يُضمَّن في كل ثنائي أصلي يُبنى
/// على ويندوز.
#[cfg(windows)]
const NATIVE_UTF8_MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n  <assemblyIdentity version=\"1.0.0.0\" name=\"niva\" type=\"win32\"/>\n  <application>\n    <windowsSettings>\n      <activeCodePage xmlns=\"http://schemas.microsoft.com/SMI/2019/WindowsSettings\">UTF-8</activeCodePage>\n    </windowsSettings>\n  </application>\n</assembly>\n";

/// اكتب البيان وملف الموارد وضمّنهما بـwindres إلى
/// ملف موارد (‎.res) جاهز للربط — ويندوز فقط.
/// windres يأتي مع MinGW نفسه (مع cc)؛ وغيابه
/// خطأ عربي صريح لا بناء صامت مكسور (فالثنائي
/// بلا البيان يفشل في المسارات العربية).
#[cfg(windows)]
fn embed_utf8_manifest(
    tmp: &std::path::Path,
) -> RioResult<std::path::PathBuf> {
    let manifest = tmp.join("niva.manifest");
    std::fs::write(&manifest, NATIVE_UTF8_MANIFEST).map_err(|e| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر كتابة niva.manifest: {}", e),
            0,
            0,
        )
    })?;
    let rc = tmp.join("niva.rc");
    std::fs::write(&rc, "1 24 \"niva.manifest\"\n").map_err(|e| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر كتابة niva.rc: {}", e),
            0,
            0,
        )
    })?;
    let res = tmp.join("niva.res");
    let wr = std::process::Command::new("windres")
        .arg("--input-format=rc")
        .arg("-i")
        .arg(&rc)
        .arg("--output-format=coff")
        .arg("-o")
        .arg(&res)
        .current_dir(tmp)
        .output();
    match wr {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(RioError::new(
            ErrorKind::Io,
            "windres غير موجود — لا يمكن تضمين بيان UTF-8 في الثنائي الأصلي. ثبّت MinGW كاملا (يأتي مع cc).".to_string(),
            0,
            0,
        )),
        Err(e) => Err(RioError::new(
            ErrorKind::Io,
            format!("تعذر تشغيل windres: {}", e),
            0,
            0,
        )),
        Ok(o) if !o.status.success() => Err(RioError::new(
            ErrorKind::Io,
            format!(
                "فشل windres في تضمين بيان UTF-8: {}",
                String::from_utf8_lossy(&o.stderr)
            ),
            0,
            0,
        )),
        Ok(_) => Ok(res),
    }
}

/// ابنِ ثنائيا أصليا من ملف مصدر نيفا إلى مسار الخرج.
/// يكتب C المولد + ملفات وقت التشغيل إلى مجلد مؤقت،
/// ثم يشغّل cc النظام. كل الرسائل عربية.
pub fn build_native(source_path: &Path, out_path: &Path) -> RioResult<()> {
    let source = std::fs::read_to_string(source_path).map_err(|e| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر قراءة الملف {}: {}", source_path.display(), e),
            0,
            0,
        )
    })?;
    // مسار مطلق لمجلد البرنامج: رسائل البحث عن الوحدات
    // تعرض مسارات مطلقة كما في المفسر (لا نسبية).
    let abs_source = std::fs::canonicalize(source_path)
        .unwrap_or_else(|_| source_path.to_path_buf());
    let program_dir = abs_source
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let main_display = crate::permissions::slash_display(&abs_source);
    let c_source = generate_native_c(&source, &program_dir, &main_display)?;
    // مجلد مؤقت للبناء.
    let tmp = std::env::temp_dir().join(format!("niva-build-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| {
        RioError::new(ErrorKind::Io, format!("تعذر إنشاء مجلد البناء: {}", e), 0, 0)
    })?;
    let write = |name: &str, content: &str| -> RioResult<std::path::PathBuf> {
        let p = tmp.join(name);
        std::fs::write(&p, content).map_err(|e| {
            RioError::new(ErrorKind::Io, format!("تعذر كتابة {}: {}", name, e), 0, 0)
        })?;
        Ok(p)
    };
    let prog_c = write("برنامج.c", &c_source)?;
    let rt_h = write("niva_rt.h", include_str!("crt/niva_rt.h"))?;
    let rt_c = write("niva_rt.c", include_str!("crt/niva_rt.c"))?;
    let std_c = write("niva_std.c", include_str!("crt/niva_std.c"))?;
    let net_c = write("niva_net.c", include_str!("crt/niva_net.c"))?;
    let _ = rt_h;
    // تحقق من وجود cc أولا.
    let cc_probe = std::process::Command::new("cc")
        .arg("--version")
        .output();
    match cc_probe {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RioError::new(
                ErrorKind::Io,
                format!(
                    "المترجم cc غير موجود — لا يمكن بناء ثنائي أصلي. {}",
                    cc_install_hint()
                ),
                0,
                0,
            ));
        }
        Err(e) => {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("تعذر تشغيل المترجم cc: {}", e),
                0,
                0,
            ));
        }
        Ok(_) => {}
    }
    // ويندوز فقط: بيان UTF-8 في الثنائي (انظر
    // NATIVE_UTF8_MANIFEST) — بلا بيان تضيع
    // المسارات العربية في الثنائي الأصلي.
    #[cfg(windows)]
    let res_arg = Some(embed_utf8_manifest(&tmp)?);
    #[cfg(not(windows))]
    let res_arg: Option<std::path::PathBuf> = None;
    let out_arg = out_path.to_string_lossy().to_string();
    let mut cc_cmd = std::process::Command::new("cc");
    cc_cmd
        // -O0 (المرحلة 2 — إرجاع مؤقت): انهيار ويندوز
        // الأصلي في سيناريو «نظام-جدول» على -O2 (تشغيل CI
        // رقم 42750995204، اختبار c_parity_integration_scenarios)؛
        // والعودة إلى -O2 معلقة على التشخيص على فرع منفصل
        // بلا مساس بـmain.
        .arg("-O0")
        // gnu11 لا c11 الصارم (اجتماع 35 §50): يفتح
        // إعلانات memmem وstrcasestr على ماك (clang
        // يخفيها تحت __STRICT_ANSI__ مع c11)؛ ولينكس
        // أخضر كما كان (_GNU_SOURCE معرّف أصلا).
        .arg("-std=gnu11")
        .arg("-o")
        .arg(&out_arg)
        .arg(&prog_c)
        .arg(&rt_c)
        .arg(&std_c)
        .arg(&net_c);
    if let Some(res) = &res_arg {
        cc_cmd.arg(res);
    }
    let output = cc_cmd
        .arg("-lm")
        .arg("-lpthread")
        .output()
        .map_err(|e| {
            RioError::new(ErrorKind::Io, format!("تعذر تشغيل المترجم cc: {}", e), 0, 0)
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let mut translated: Vec<String> = Vec::new();
        for line in stderr.lines() {
            translate_cc_line(line, &mut translated);
        }
        // احفظ السجل الخام للتشخيص (لا يُعرض).
        let log_path = tmp.join("cc-سجل.txt");
        let _ = std::fs::write(&log_path, &stderr);
        let mut msg = String::from("فشل بناء الثنائي الأصلي");
        if !translated.is_empty() {
            msg.push_str(": ");
            msg.push_str(&translated.join("؛ "));
        }
        // ضمّن أول سطور السجل الخام في الرسالة نفسها:
        // عدّاء CI بلا cc عامل يفشل بسطر «ملف أو مجلد
        // غير موجود» بلا أي سبب ظاهر، ومسار ملف مؤقت
        // يختفي مع المجلد — فالسبب يجب أن يظهر في
        // سجلات CI نفسها. الترجمة العربية أولا، والسجل
        // الخام (إنجليزي المترجم) تشخيص فقط بعده.
        let log_lines: Vec<&str> = stderr.lines().collect();
        if !log_lines.is_empty() {
            msg.push_str("\nسجل المترجم:");
            let shown = log_lines.len().min(30);
            for l in &log_lines[..shown] {
                msg.push('\n');
                msg.push_str(l);
            }
            if log_lines.len() > shown {
                msg.push_str(&format!(
                    "\n… ({} سطرا أخرى في السجل)",
                    log_lines.len() - shown
                ));
            }
        }
        msg.push_str(&format!(
            "\n(سجل البناء الكامل في {})",
            log_path.display()
        ));
        return Err(RioError::new(ErrorKind::Runtime, msg, 0, 0));
    }
    // تنظيف المؤقت (أبقِ سجل cc عند الفشل فقط).
    let _ = std::fs::remove_dir_all(&tmp);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out_path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::NATIVE_UTF8_MANIFEST;

    #[test]
    fn native_utf8_manifest_declares_utf8_code_page() {
        assert!(
            NATIVE_UTF8_MANIFEST.contains("activeCodePage"),
            "got: {}",
            NATIVE_UTF8_MANIFEST
        );
        assert!(
            NATIVE_UTF8_MANIFEST.contains(">UTF-8<"),
            "got: {}",
            NATIVE_UTF8_MANIFEST
        );
        assert!(
            NATIVE_UTF8_MANIFEST.contains("manifestVersion=\"1.0\""),
            "got: {}",
            NATIVE_UTF8_MANIFEST
        );
    }
}
