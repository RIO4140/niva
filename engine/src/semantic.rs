//! Semantic analyzer — Phase 1A Steps 1-3.
//!
//! Runs AFTER parsing and BEFORE execution. It catches, with an Arabic
//! error carrying line + column, the mistakes that used to surface only
//! at runtime, after part of the program had already run:
//! - reading a variable that was never defined in any visible scope
//! - `ارجع` outside a function
//! - `اكسر` / `تابع` outside a loop (a loop inside a function does
//!   NOT count for a break inside a nested function, and vice versa)
//! - calling a known user function or builtin with the wrong number
//!   of arguments
//! - (Step 3, gradual types) assigning a known-typed value to a
//!   variable, parameter, or return declared with a different type.
//!   Unknown (untyped / أي) never errors statically; it is checked
//!   at runtime by the interpreter, so gradual code is not falsely
//!   rejected.
//!
//! What it does NOT do yet (later Phase 1A steps, not this one):
//! - classes, interfaces (Phase 1A spec in v1.3, not v1)
//!
//! Phase 1A Step 2 adds modules to this analyzer for the MAIN
//! program only: an Import defines its bound names, an Export is a
//! no-op, and a Dot access analyzes its target but does NOT check
//! the member name (member existence and export status are checked
//! at runtime, when the module file is actually loaded).
//! Module FILES themselves are not semantically analyzed in Step 2:
//! they are parsed and executed by the interpreter's loader, with
//! the module name added to any runtime error. A two-pass module
//! analysis (so a module function may reference a module global
//! defined later in the file) is a later step, not this one.
//!
//! Scope rules mirror the interpreter exactly, so the analyzer never
//! rejects a program the interpreter would accept, and never accepts
//! one the interpreter would reject for scope reasons:
//! - a function pushes a NEW scope; parameters live in it
//! - `إذا` / `بينما` / `لكل` / `كرر` / `حاول` do NOT push a scope:
//!   variables defined inside them stay visible after them, and a
//!   loop variable stays visible after the loop
//! - a function name is added to its enclosing scope BEFORE its body
//!   is analyzed, so recursion passes, but a call to a function
//!   defined LATER in the same scope fails, exactly as it would at
//!   runtime (the interpreter defines functions top-down)
//! - assignment defines a variable in the innermost scope, never in
//!   an outer scope, matching `define_local` in the interpreter

use crate::ast::{AssignTarget, Expr, ExprKind, Stmt, StmtKind};
use crate::error::{RioError, RioResult};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct VarInfo {
    // Some(n) if a known user function with n params, None otherwise.
    arity: Option<usize>,
    // Declared type for a variable (annotation), None = untyped.
    // For a module binding, Some("وحدة").
    declared_type: Option<String>,
    // For a known user function only.
    param_types: Vec<Option<String>>,
    return_type: Option<String>,
    // البنود 109–112: أسماء المعاملات وعدد
    // الافتراضيات والجامعان — للفحص الستاتيكي
    // للمفتاحيات حين يكون المستدعى معروفا.
    params: Vec<String>,
    n_defaults: usize,
    has_vararg: bool,
    has_kwarg: bool,
    // البنود 145–147: معاملات النوع المعلنة للدالة
    // (فارغة لغير العامة).
    type_params: Vec<crate::ast::TypeParam>,
}

impl VarInfo {
    fn var() -> Self {
        Self { arity: None, declared_type: None, param_types: vec![], return_type: None, params: vec![], n_defaults: 0, has_vararg: false, has_kwarg: false, type_params: vec![] }
    }
    fn var_typed(t: &str) -> Self {
        Self { arity: None, declared_type: Some(t.to_string()), param_types: vec![], return_type: None, params: vec![], n_defaults: 0, has_vararg: false, has_kwarg: false, type_params: vec![] }
    }
    fn func(arity: usize, param_types: Vec<Option<String>>, return_type: Option<String>, params: Vec<String>, n_defaults: usize, has_vararg: bool, has_kwarg: bool, type_params: Vec<crate::ast::TypeParam>) -> Self {
        Self { arity: Some(arity), declared_type: None, param_types, return_type, params, n_defaults, has_vararg, has_kwarg, type_params }
    }
}

/// One scope: name -> info about that name in this scope.
type Scope = HashMap<String, VarInfo>;

// Audit م-12: Clone so the REPL can trial-analyze a line on
// a copy (code-first check) without touching the real state.
#[derive(Debug, Clone)]
pub struct SemanticAnalyzer {
    scopes: Vec<Scope>,
    function_depth: usize,
    loop_depth: usize,
    // Stack of declared return types for enclosing functions.
    return_type_stack: Vec<Option<String>>,
    // الدفعة الأخيرة DEF (البنود 77 و78): إعلانات
    // النطاق لكل دالة محيطة — (عام، نطاق)؛ موازٍ
    // لدخول الدوال (فارغ في المستوى الأعلى وداخل لامدا).
    decl_stack: Vec<(HashSet<String>, HashSet<String>)>,
    // البنود 145–147 (العموميات — الاجتماع 83 §98):
    // معاملات النوع في نطاق التعريفات المحيطة —
    // (الاسم، حده أو لا شيء) لكل مستوى؛ والاستبدال
    // يأخذ أقرب مستوى أولا (الظل الداخلي يغلب).
    type_param_stack: Vec<Vec<(String, Option<String>)>>,
}

fn builtin_arity(name: &str) -> Option<(usize, usize)> {
    // (min, max) arguments, inclusive. None = not a builtin.
    match name {
        "طول" | "مجموع" | "أدنى" | "أقصى" | "مطلق" | "عدد" | "نص" | "نوع"
        | "اقرأ_ملف" | "احذف_مجلد" | "احذف_ملف"
        | "أنشئ_ملف" | "عد_الأسطر" | "عد_الكلمات" | "ابحث_عن" | "حجم_المسار"
        | "موجود" | "نوع_المسار" | "تفاصيل_المسار" | "تثبيت" | "بيئة" => Some((1, 1)),
        // المرحلة (د) — الإكمال (اجتماع 66 §81): بديل
        // البيئة والبديل المشروط وإزالة البادئة/اللاحقة
        // معاملان، والفهارس والمصدر وحذف الدالة معامل واحد.
        "بيئة_أو" | "بيئة_إن_وجد" | "أزل_بادئة" | "أزل_لاحقة" => Some((2, 2)),
        "فهارس" | "مصدر" | "احذف_دالة" => Some((1, 1)),
        // المرحلة (د) — الإكمال (اجتماع 67 §82): عرض
        // المتغيرات والدوال بلا معاملات.
        "اعرض_المتغيرات" | "اعرض_الدوال" => Some((0, 0)),
        // المرحلة (د) — الإكمال (اجتماع 69 §84): احذف
        // بعلم ثان اختياري «تجاهل_الغائب».
        "احذف" => Some((1, 2)),
        // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
        // `أنشئ_مجلد` بمعامل وضع اختياري.
        "أنشئ_مجلد" => Some((1, 2)),
        "مدى" => Some((1, 3)),
        // ب6 — المولدات (الاجتماع 85 §100): التقدم
        // والإغلاق معامل واحد؛ والاستدعاءات الداخلية
        // المفككة في المحلل: `ولد` و`ولد من` معامل
        // واحد، والتعبير التوليدي ثلاثة (دالتا التعبير
        // والشرط، والقابل).
        "التالي" | "أغلق_المولد" => Some((1, 1)),
        "__ولد" | "__ولد_من" => Some((1, 1)),
        "__مولد_تعبير" => Some((3, 3)),
        // البند 88 — مدير السياق (الاجتماع 88 §103):
        // الاستدعاءات الداخلية المفككة في المحلل من
        // `عند`: الفحص معامل واحد، وقراءة الخطأ الممسوك
        // بلا معاملات، وإعادة الرفع معامل واحد (قاموس
        // الخطأ المحفوظ).
        "__عند_افحص" => Some((1, 1)),
        "__عند_خطأ" => Some((0, 0)),
        "__عند_أعد_رفع" => Some((1, 1)),
        "اكتب_ملف" | "انقل" | "أنشئ_رابط" | "ابحث_في_ملف" => Some((2, 2)),
        // المرحلة (د) — الإكمال (اجتماع 69 §84): انسخ
        // بعلم ثالث اختياري «لا_تكتب».
        "انسخ" => Some((2, 3)),
        // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
        // النسخ التكراري والمحدَّث والأرشيفي والخفيف
        // والحذف التكراري والرابطان الصلب والنسبي.
        "انسخ_تكراري" | "انسخ_محدث" | "انسخ_أرشيف" | "انسخ_خفيف" | "أنشئ_رابط_صلب"
        | "أنشئ_رابط_نسبي" => Some((2, 2)),
        "احذف_تكراري" => Some((1, 1)),
        "ادخل" => Some((0, 1)),
        "اعرض_الشجرة" => Some((0, 1)),
        // المرحلة (د) — الإكمال (اجتماع 69 §84): عرض
        // الملفات بمسار اختياري وأعلام نصية اختيارية.
        "اعرض_الملفات" => Some((0, usize::MAX)),
        "التاريخ_الآن" | "النسخة" | "إصدار_المحرك"
        // الدفعة 1 — رموز الخروج (اجتماع 54 §69): قراءة
        // الرمز واسم البرنامج بلا معاملات، والخروج بـ0 أو
        // 1 معاملات (بلا معامل = رمز آخر أمر).
        | "رمز_الخروج" | "اسم_البرنامج"
        // المرحلة (ج) — التوجيه والأنابيب (اجتماعا 62
        // و63 §77 و§78): قراءة الإدخال الموجَّه كاملا،
        // وأمرا النجاح والفشل الفارغان، وقائمة رموز الأنبوب.
        | "اقرأ_الإدخال" | "نجاح" | "فشل" | "رموز_الأنبوب" => {
            Some((0, 0))
        }
        // المرحلة (ج): `وزع` مسار واحد على الأقل،
        // ومعامل نصي أخير اختياري "ألحق".
        "وزع" => Some((1, usize::MAX)),
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // التجميد والحذف والفحص والمرجع والعرض
        // والمفاتيح — أسماء الدوال الجديدة.
        "جمد" | "احذف_متغير" | "افحص_متغير" | "مفاتيح" => Some((1, 1)),
        "مرجع" => Some((2, 2)),
        "اعرض_البيئة" => Some((0, 0)),
        "خروج" => Some((0, 1)),
        "عد_الملفات" | "اعرض_الشجرة" => Some((0, 1)),
        // الدفعة 3 — التشغيل (اجتماع 57 §72): وصف
        // الأمر ومكان الأداة بمعامل نصي واحد،
        // والاستبدال بمعامل نصي واحد.
        "نوع_الامر" | "مكان_الاداة" | "استبدل_الجلسة" => Some((1, 1)),
        // اجتماع 56 §71 — الدفعة 2 (التنقل): `انتقل`
        // بلا معاملات → الأب (`..`)، ومعامل واحد
        // (مسار)، ومعامل ثان اختياري (علم نصي)؛
        // و`المجلد_الحالي` بمعامل علم اختياري؛ والمكدس
        // و`CDPATH` والوضع الفيزيائي كما في §71.
        "انتقل" => Some((0, 2)),
        "المجلد_الحالي" | "ادفع_مجلد" | "أخرج_مجلد" | "مسار_بحث_المجلدات"
        | "الوضع_الفيزيائي" => Some((0, 1)),
        "اعرض_المكدس" | "امسح_المكدس" => Some((0, 0)),
        // الدفعة 3 من الـ579 — الحاويات والفهم (البنود
        // 138 و139 و141–143): `مترابطة` و`تشكيلة`
        // متغيرتا المعاملات (صفر فأكثر)، والفهم الثلاثة
        // بثلاثة معاملات (دالة التعبير، دالة الشرط أو
        // لاشيء، القابل) — وبلا هذين السطرين يرفض
        // المحلل الدلالي الاستدعاءات المفككة في
        // المحركات الثلاثة كلها («متغير غير معرف»).
        "مترابطة" | "تشكيلة" => Some((0, usize::MAX)),
        "افهم_قائمة" | "افهم_تشكيلة" | "افهم_فهرس" => Some((3, 3)),
        // ج6 (الدفعة الأخيرة): مدمجات الصيغ الحرفية
        // المفككة في المحلل — وبلا هذه الأسطر يرفض
        // المحلل الدلالي `123x` و`1r2` و`0j1` و`4ت`
        // («متغير غير معرف») في المحركات الثلاثة.
        "عدد_كبير" => Some((1, 1)),
        "كسر" | "مركب" | "كسر_حرفي" | "مركب_حرفي" => Some((1, 2)),
        "اعرض_البداية" | "اعرض_النهاية" => Some((1, 2)),
        _ => None,
    }
}

fn builtin_return_type(name: &str) -> Option<&'static str> {
    match name {
        "طول" | "مجموع" | "مطلق" | "عدد" | "عد_الأسطر" | "عد_الكلمات" | "حجم_المسار"
        | "عد_الملفات" => Some("عدد"),
        "نص" | "نوع" | "اقرأ_ملف" | "ادخل" | "اقرأ_الإدخال" | "المجلد_الحالي" | "اعرض_البداية"
        | "اعرض_النهاية" | "التاريخ_الآن" | "النسخة" | "إصدار_المحرك" | "نوع_المسار"
        // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
        // النسخ الخفيف يرجع نص الطريقة («مستنسخ» أو
        // «متناثر» أو «عادي»)، والمحدَّث يرجع منطقيا
        // (نُسخ؟)، و`أنشئ_مجلد` ترجع قائمة المنشأ.
        | "انسخ_خفيف" => Some("نص"),
        "انسخ_محدث" => Some("منطقي"),
        "أنشئ_مجلد" => Some("قائمة"),
        // الدفعة 1 — رموز الخروج: الرمز عدد، واسم
        // البرنامج نص، والخروج يرجع لاشيء (ولا يرجع أصلا
        // — ينهي الجلسة أو البرنامج).
        | "تثبيت" | "بيئة" | "اسم_البرنامج" => Some("نص"),
        // المرحلة (د) — الإكمال (اجتماع 66 §81): بديلا
        // البيئة وإزالتا البادئة/اللاحقة نص، والفهارس
        // وعرض المتغيرات والدوال قوائم، والمصدر وحذف
        // الدالة لاشيء.
        "بيئة_أو" | "بيئة_إن_وجد" | "أزل_بادئة" | "أزل_لاحقة" => Some("نص"),
        "فهارس" | "اعرض_المتغيرات" | "اعرض_الدوال" => Some("قائمة"),
        "مصدر" | "احذف_دالة" => Some("لاشيء"),
        "رمز_الخروج" => Some("عدد"),
        // المرحلة (ج): `نجاح` منطقي، و`فشل` لاشيء،
        // و`وزع` نص فارغ (صامت كتعبير عار — فتمريرة
        // المدخل وحدها تظهر، بلا «لاشيء» بعدها)،
        // و`رموز_الأنبوب` قائمة أعداد.
        "نجاح" => Some("منطقي"),
        "فشل" => Some("لاشيء"),
        "وزع" => Some("نص"),
        "رموز_الأنبوب" => Some("قائمة"),
        "خروج" => Some("لاشيء"),
        // الدفعة 4 — المتغيرات (اجتماع 58 §73):
        // الفحص نص، والعرض والمفاتيح قائمتان،
        // والتجميد والحذف والمرجع لاشيء.
        "افحص_متغير" => Some("نص"),
        "اعرض_البيئة" | "مفاتيح" => Some("قائمة"),
        "جمد" | "احذف_متغير" | "مرجع" => Some("لاشيء"),
        // الدفعة 3 — التشغيل (اجتماع 57 §72): وصف
        // الأمر نص دائما؛ والاستبدال لا يرجع أصلا
        // (ينهي الجلسة أو البرنامج) كـ`خروج`.
        // `مكان_الاداة` بلا نوع ثابت هنا (نص أو
        // لاشيء) فيبقى مجهولا — كـ`أدنى`/`أقصى`.
        "نوع_الامر" => Some("نص"),
        "استبدل_الجلسة" => Some("لاشيء"),
        "مدى" | "اعرض_الملفات" | "ابحث_في_ملف" | "ابحث_عن" | "اعرض_الشجرة"
        // اجتماع 56 §71 — الدفعة 2 (التنقل): عرض
        // المكدس وقائمة `CDPATH` قائمتان.
        | "اعرض_المكدس" | "مسار_بحث_المجلدات" => Some("قائمة"),
        "اكتب_ملف" | "انتقل" | "احذف" | "احذف_مجلد" | "احذف_ملف"
        | "انسخ" | "انقل" | "أنشئ_ملف" | "أنشئ_رابط"
        // الدفعة 5 — أدوات الملفات (اجتماع 59 §74):
        // النسخ التكراري والأرشيفي والحذف التكراري
        // والرابطان يرجعون لاشيء؛ و`أنشئ_مجلد` ترجع
        // قائمة (فوق في ذراعها الخاص).
        | "انسخ_تكراري" | "انسخ_أرشيف" | "احذف_تكراري" | "أنشئ_رابط_صلب"
        | "أنشئ_رابط_نسبي"
        // اجتماع 56 §71 — الدفعة 2 (التنقل): الدفع
        // والإخراج والمسح ترجع لاشيء.
        | "ادفع_مجلد" | "أخرج_مجلد" | "امسح_المكدس" => Some("لاشيء"),
        "موجود" => Some("منطقي"),
        // اجتماع 56 §71 — الدفعة 2 (التنقل): الوضع
        // الفيزيائي يرجع قيمة منطقية.
        "الوضع_الفيزيائي" => Some("منطقي"),
        "تفاصيل_المسار" => Some("قاموس"),
        // الدفعة 3 من الـ579 — الحاويات والفهم:
        // المترابطة مترابطة، والتشكيلة وفهمها تشكيلة،
        // وفهم القائمة قائمة، وفهم الفهرس قاموس.
        "مترابطة" => Some("مترابطة"),
        "تشكيلة" | "افهم_تشكيلة" => Some("تشكيلة"),
        "افهم_قائمة" => Some("قائمة"),
        "افهم_فهرس" => Some("قاموس"),
        // ج6: أنواع المدمجات المفككة.
        "عدد_كبير" => Some("كبير"),
        "كسر" | "كسر_حرفي" => Some("كسر"),
        "مركب" | "مركب_حرفي" => Some("مركب"),
        // أدنى / أقصى depend on input type: unknown in Step 3.
        _ => None,
    }
}

pub(crate) fn is_builtin(name: &str) -> bool {
    builtin_arity(name).is_some()
}

/// Static type check helper: if both types are known and differ,
/// and neither is `أي`, return a semantic type-mismatch error.
/// Unknown on either side passes (gradual typing).
/// البند 8: المقارنة بنيوية — العام يطابقه أساسه
/// (`قائمة` تطابق `قائمة[عدد]` ستاتيكيا؛ وفحص العناصر
/// وقت التشغيل وحده).
fn check_type_match(
    expected: &str,
    actual: Option<String>,
    what: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    if expected == "أي" {
        return Ok(());
    }
    if let Some(a) = actual {
        if a == "أي" {
            return Ok(());
        }
        let ok = match crate::ast::TypeExpr::parse(expected) {
            Some(t) => t.matches_declared(&a),
            None => a == expected,
        };
        if !ok {
            return Err(RioError::semantic(
                format!("نوع غير متطابق: {} معلن {}، لكن القيمة {}", what, expected, a),
                line,
                col,
            ));
        }
    }
    Ok(())
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            function_depth: 0,
            loop_depth: 0,
            return_type_stack: vec![],
            decl_stack: Vec::new(),
            type_param_stack: Vec::new(),
        }
    }

    /// البنود 145–147: استبدال أسماء معاملات النوع
    /// في نوع معلن بحدودها (ستاتيكيا: `ت: عدد` تعني
    /// أن قيم `ت` أعداد)؛ وبلا حد تصير `أي`؛ وبلا
    /// معاملات نوع في النطاق يبقى النوع كما هو.
    fn subst_bound_type(&self, ty: &str) -> String {
        if self.type_param_stack.is_empty() {
            return ty.to_string();
        }
        let Some(t) = crate::ast::TypeExpr::parse(ty) else {
            return ty.to_string();
        };
        self.subst_bound_expr(&t).display()
    }

    fn subst_bound_expr(&self, t: &crate::ast::TypeExpr) -> crate::ast::TypeExpr {
        match t {
            crate::ast::TypeExpr::Named(n) => {
                for level in self.type_param_stack.iter().rev() {
                    if let Some((_, bound)) = level.iter().find(|(k, _)| k == n) {
                        return match bound {
                            Some(b) => crate::ast::TypeExpr::parse(b)
                                .unwrap_or_else(|| crate::ast::TypeExpr::Named(b.clone())),
                            None => crate::ast::TypeExpr::Named("أي".to_string()),
                        };
                    }
                }
                crate::ast::TypeExpr::Named(n.clone())
            }
            crate::ast::TypeExpr::Generic { name, args } => crate::ast::TypeExpr::Generic {
                name: name.clone(),
                args: args.iter().map(|a| self.subst_bound_expr(a)).collect(),
            },
        }
    }

    pub fn analyze_program(&mut self, program: &[Stmt]) -> RioResult<()> {
        for stmt in program {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn define(&mut self, name: &str, info: VarInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), info);
        }
    }

    fn lookup(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    pub(crate) fn is_defined(&self, name: &str) -> bool {
        self.lookup(name).is_some() || is_builtin(name)
    }

    /// الدفعة الأخيرة DEF (البنود 77 و78): هل الاسم
    /// معلن عاما / محيطا في الدالة الجارية؟
    fn is_global(&self, name: &str) -> bool {
        self.decl_stack
            .last()
            .map(|(g, _)| g.contains(name))
            .unwrap_or(false)
    }

    fn is_nonlocal(&self, name: &str) -> bool {
        self.decl_stack
            .last()
            .map(|(_, n)| n.contains(name))
            .unwrap_or(false)
    }

    /// ربط محيط (لا المحلي ولا العام) — للفحص النوعي
    /// عند إسناد باسم معلن محيطا.
    fn lookup_nonlocal(&self, name: &str) -> Option<VarInfo> {
        let n = self.scopes.len();
        for scope in self.scopes.iter().take(n.saturating_sub(1)).skip(1).rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    /// تحقق من إعلانات دالة ثم ادفعها: لا اسم في
    /// المجموعتين معا، ولا معامل معلن عاما/محيطا.
    /// `what` لرسالة الخطأ (الدالة X / الطريقة X).
    fn push_decls(
        &mut self,
        body: &[Stmt],
        params: &[String],
        what: &str,
        line: usize,
        col: usize,
    ) -> RioResult<()> {
        let (gv, nv) = crate::ast::collect_scope_decls(body);
        let g: HashSet<String> = gv.into_iter().collect();
        let n: HashSet<String> = nv.into_iter().collect();
        for name in g.intersection(&n) {
            return Err(RioError::semantic(
                format!("المتغير {} معلن عاما ومحيطا معا", name),
                line,
                col,
            ));
        }
        for p in params {
            if g.contains(p) {
                return Err(RioError::semantic(
                    format!("المعامل {} لا يمكن إعلانه عاما في {}", p, what),
                    line,
                    col,
                ));
            }
            if n.contains(p) {
                return Err(RioError::semantic(
                    format!("المعامل {} لا يمكن إعلانه محيطا في {}", p, what),
                    line,
                    col,
                ));
            }
        }
        self.decl_stack.push((g, n));
        Ok(())
    }

    /// Static type of an expression, if known. None = unknown (أي).
    /// Only declared types, literals, and known function/builtin
    /// returns are known; everything else (index, dot, binary with
    /// unknown operands, untyped variables) is unknown and passes.
    fn infer_type(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Number(_) => Some("عدد".to_string()),
            ExprKind::Str(_) => Some("نص".to_string()),
            ExprKind::Bool(_) => Some("منطقي".to_string()),
            ExprKind::Nil => Some("لاشيء".to_string()),
            ExprKind::List(_) => Some("قائمة".to_string()),
            ExprKind::Dict(_) => Some("قاموس".to_string()),
            ExprKind::Ident(name) => {
                if let Some(info) = self.lookup(name) {
                    if let Some(t) = info.declared_type {
                        return Some(t);
                    }
                    if info.arity.is_some() {
                        return Some("دالة".to_string());
                    }
                    return None;
                }
                // Builtin name used as a value (not a call): a function.
                if is_builtin(name) {
                    return Some("دالة".to_string());
                }
                None
            }
            ExprKind::Call { callee, .. } => {
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(info) = self.lookup(name) {
                        if let Some(rt) = info.return_type {
                            return Some(rt);
                        }
                        if info.arity.is_some() {
                            return None;
                        }
                    }
                    if let Some(rt) = builtin_return_type(name) {
                        return Some(rt.to_string());
                    }
                }
                None
            }
            ExprKind::Binary { left, op, right } => {
                use crate::ast::BinOp;
                match op {
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
                    | BinOp::And | BinOp::Or | BinOp::In | BinOp::NotIn => {
                        Some("منطقي".to_string())
                    }
                    BinOp::Add => {
                        let lt = self.infer_type(left)?;
                        let rt = self.infer_type(right)?;
                        if lt == rt && (lt == "عدد" || lt == "نص" || lt == "قائمة") {
                            Some(lt)
                        } else {
                            None
                        }
                    }
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        let lt = self.infer_type(left)?;
                        let rt = self.infer_type(right)?;
                        if lt == "عدد" && rt == "عدد" {
                            Some("عدد".to_string())
                        } else {
                            None
                        }
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                use crate::ast::UnOp;
                match op {
                    UnOp::Neg => {
                        if self.infer_type(inner)? == "عدد" {
                            Some("عدد".to_string())
                        } else {
                            None
                        }
                    }
                    UnOp::Not => {
                        if self.infer_type(inner)? == "منطقي" {
                            Some("منطقي".to_string())
                        } else {
                            None
                        }
                    }
                }
            }
            ExprKind::Index { .. } | ExprKind::Dot { .. } => None,
            ExprKind::Create { .. } => Some("كائن".to_string()),
            ExprKind::Lambda { .. } => Some("دالة".to_string()),
            // البند 73 (`:=`): نوع التعبير نوع قيمته.
            ExprKind::AssignExpr { value, .. } => self.infer_type(value),
        }
    }

    fn analyze_block(&mut self, body: &[Stmt]) -> RioResult<()> {
        for stmt in body {
            self.analyze_stmt(stmt)?;
        }
        Ok(())
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> RioResult<()> {
        match &stmt.kind {
            // الدفعة الأخيرة DEF (البنود 77 و78 و104):
            // `عام` بلا أثر في المستوى الأعلى، وداخل
            // الدالة جُمعت إعلاناتها عند دخولها؛
            // و`نطاق` خارج دالة خطأ دلالي؛ وبيان
            // الاسم المستعار بلا أثر (المرحلة هـ).
            StmtKind::GlobalDecl { .. } => Ok(()),
            StmtKind::NonlocalDecl { .. } => {
                if self.function_depth == 0 {
                    return Err(RioError::semantic(
                        "نطاق خارج دالة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(())
            }
            StmtKind::TypeAlias { .. } => Ok(()),
            StmtKind::Assign { target, value, annotation, append } => {
                // Value first, matching runtime evaluation order.
                self.analyze_expr(value)?;
                let value_type = self.infer_type(value);
                // الدفعة 4 — المتغيرات (اجتماع 58 §73):
                // الإلحاق (`append`) يفحص القيمة اليمنى
                // نفسها: نوعها يجب أن يطابق المعلن (أو
                // يعرّف المتغير الغائب بنوعها — الهوية
                // من نوع اليمنى وقت التشغيل)، فلا فحص
                // إضافي هنا غير فحص `=` القائم.
                let _ = append;
                match target {
                    AssignTarget::Var(name) => {
                        // الدفعة الأخيرة DEF (البنود 77 و78):
                        // توجيه الإسناد حسب إعلان النطاق —
                        // عام إلى scopes[0] (نطاق البرنامج)،
                        // ونطاق إلى أقرب ربط محيط (بلا تعريف
                        // محلي — غيابه خطأ زمن-تشغيل)، وإلا
                        // أقرب ربط كالسابق (Step 1، الاجتماع 11):
                        // إسناد يستهدف أقرب ربط موجود، لا ظلا
                        // محليا جديدا؛ والغائب وحده يُعرَّف في
                        // النطاق الجاري. فحص النوع المعلن يستعمل
                        // نوع الربط المستهدَف.
                        let is_g = self.is_global(name);
                        let is_n = self.is_nonlocal(name);
                        let existing_decl = if is_g {
                            self.scopes[0]
                                .get(name)
                                .and_then(|i| i.declared_type.clone())
                        } else if is_n {
                            self.lookup_nonlocal(name)
                                .and_then(|i| i.declared_type)
                        } else {
                            self.lookup(name)
                                .and_then(|info| info.declared_type.clone())
                        };
                        if let Some(decl) = &existing_decl {
                            let decl = self.subst_bound_type(decl);
                            let value_type = value_type.clone().map(|t| self.subst_bound_type(&t));
                            check_type_match(
                                &decl,
                                value_type,
                                &format!("المتغير {}", name),
                                stmt.line,
                                stmt.col,
                            )?;
                        }
                        if let Some(ann) = annotation {
                            let ann = self.subst_bound_type(ann);
                            let value_type = value_type.map(|t| self.subst_bound_type(&t));
                            check_type_match(
                                &ann,
                                value_type,
                                &format!("المتغير {}", name),
                                stmt.line,
                                stmt.col,
                            )?;
                            // An annotation on an existing binding
                            // must agree with its declared type.
                            if let Some(decl) = &existing_decl {
                                if decl.as_str() != ann.as_str() {
                                    return Err(RioError::semantic(
                                        format!(
                                            "المتغير {} معلن {} في نطاق خارجي، ولا يمكن إعادة إعلانه {}",
                                            name, decl, ann
                                        ),
                                        stmt.line,
                                        stmt.col,
                                    ));
                                }
                            }
                            // No existing binding: the annotation
                            // declares a new binding in the target scope.
                            if is_g {
                                if !self.scopes[0].contains_key(name) {
                                    self.scopes[0]
                                        .insert(name.clone(), VarInfo::var_typed(&ann));
                                }
                            } else if !is_n && self.lookup(name).is_none() {
                                self.define(name, VarInfo::var_typed(&ann));
                            }
                        } else if is_g {
                            if !self.scopes[0].contains_key(name) {
                                self.scopes[0].insert(name.clone(), VarInfo::var());
                            }
                        } else if !is_n && self.lookup(name).is_none() {
                            self.define(name, VarInfo::var());
                        }
                    }
                    AssignTarget::Dot { target: t, name: _ } => {
                        // Field assignment: target must be analyzable
                        // (a variable, نفس inside a method, or a dot
                        // chain). Field existence/type is runtime.
                        self.analyze_expr(t)?;
                    }
                    AssignTarget::Index { target: t, index: i } => {
                        // Narrowed v1.3 Target: only Ident[Expr] is
                        // valid here; anything else is a semantic error
                        // (the interpreter also rejects it at runtime).
                        match &t.kind {
                            ExprKind::Ident(name) => {
                                if !self.is_defined(name) {
                                    return Err(RioError::semantic(
                                        format!("متغير غير معرف: {}", name),
                                        t.line,
                                        t.col,
                                    ));
                                }
                                self.analyze_expr(i)?;
                            }
                            _ => {
                                return Err(RioError::semantic(
                                    "هدف التعيين غير صالح",
                                    stmt.line,
                                    stmt.col,
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }
            StmtKind::Print { args } => {
                for a in args {
                    self.analyze_expr(a)?;
                }
                Ok(())
            }
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                self.analyze_expr(cond)?;
                self.analyze_block(then_body)?;
                for (c, b) in elifs {
                    self.analyze_expr(c)?;
                    self.analyze_block(b)?;
                }
                if let Some(b) = else_body {
                    self.analyze_block(b)?;
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                self.analyze_expr(cond)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::For {
                var,
                iterable,
                body,
            } => {
                self.analyze_expr(iterable)?;
                // Loop variable stays in the CURRENT scope, not a new
                // one, and stays visible after the loop.
                self.define(var, VarInfo::var());
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::RepeatCount { count, body } => {
                self.analyze_expr(count)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::RepeatWhile { cond, body } => {
                self.analyze_expr(cond)?;
                self.loop_depth += 1;
                let r = self.analyze_block(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::FuncDef { name, params, param_types, return_type, defaults, vararg, kwarg, type_params, body } => {
                // البنود 145–147: معاملات النوع في النطاق
                // طوال الجسم (لأنواع المعاملات والإرجاع
                // والتعليقات داخله).
                let tp_level: Vec<(String, Option<String>)> = type_params
                    .iter()
                    .map(|tp| (tp.name.clone(), tp.bound.clone()))
                    .collect();
                self.type_param_stack.push(tp_level);
                // Duplicate parameters are a definition error.
                let mut seen = HashSet::new();
                for p in params {
                    if !seen.insert(p) {
                        return Err(RioError::semantic(
                            format!("معامل مكرر في الدالة {}: {}", name, p),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                // Define the function BEFORE its body, so recursion
                // passes, and so later calls in the enclosing scope
                // pass. Calls before this definition fail, as at runtime.
                let n_defaults = defaults.iter().filter(|d| d.is_some()).count();
                self.define(
                    name,
                    VarInfo::func(params.len(), param_types.clone(), return_type.clone(), params.clone(), n_defaults, vararg.is_some(), kwarg.is_some(), type_params.clone()),
                );
                // البنود 109–112: الافتراضيات تُحلَّل في
                // نطاق التعريف (لا نطاق الجسم).
                for d in defaults.iter().flatten() {
                    self.analyze_expr(d)?;
                }
                self.push_decls(body, params, &format!("الدالة {}", name), stmt.line, stmt.col)?;
                self.scopes.push(HashMap::new());
                for (p, pt) in params.iter().zip(param_types.iter()) {
                    match pt {
                        Some(t) => self.define(p, VarInfo::var_typed(t)),
                        None => self.define(p, VarInfo::var()),
                    }
                }
                // الجامعان اسمان في نطاق الجسم: قائمة وقاموس.
                if let Some(vn) = vararg {
                    self.define(vn, VarInfo::var_typed("قائمة"));
                }
                if let Some(kn) = kwarg {
                    self.define(kn, VarInfo::var_typed("قاموس"));
                }
                self.function_depth += 1;
                self.return_type_stack.push(return_type.clone());
                let saved_loop = self.loop_depth;
                self.loop_depth = 0;
                let r = self.analyze_block(body);
                self.loop_depth = saved_loop;
                self.return_type_stack.pop();
                self.function_depth -= 1;
                self.scopes.pop();
                self.decl_stack.pop();
                self.type_param_stack.pop();
                r
            }
            StmtKind::Return { value } => {
                if self.function_depth == 0 {
                    return Err(RioError::semantic(
                        "ارجع خارج دالة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                if let Some(v) = value {
                    self.analyze_expr(v)?;
                    if let Some(Some(rt)) = self.return_type_stack.last() {
                        let rt = self.subst_bound_type(rt);
                        let vt = self.infer_type(v).map(|t| self.subst_bound_type(&t));
                        check_type_match(
                            &rt,
                            vt,
                            &format!("قيمة ارجع"),
                            stmt.line,
                            stmt.col,
                        )?;
                    }
                } else {
                    // Return without a value: type لاشيء.
                    if let Some(Some(rt)) = self.return_type_stack.last() {
                        let rt = self.subst_bound_type(rt);
                        check_type_match(
                            &rt,
                            Some("لاشيء".to_string()),
                            &format!("قيمة ارجع"),
                            stmt.line,
                            stmt.col,
                        )?;
                    }
                }
                Ok(())
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    return Err(RioError::semantic(
                        "اكسر خارج حلقة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(())
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    return Err(RioError::semantic(
                        "تابع خارج حلقة",
                        stmt.line,
                        stmt.col,
                    ));
                }
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::Try {
                try_body,
                except_body,
                else_body,
                finally_body,
            } => {
                self.analyze_block(try_body)?;
                // البند 98 (`والا` بعد حاول): يُحلَّل جسمها
                // كجسم حاول (بعد نجاحه العادي).
                if let Some(b) = else_body {
                    self.analyze_block(b)?;
                }
                if let Some(b) = except_body {
                    self.analyze_block(b)?;
                }
                if let Some(b) = finally_body {
                    self.analyze_block(b)?;
                }
                Ok(())
            }
            StmtKind::ExprStmt { expr } => self.analyze_expr(expr),
            // البند 8 — الزخارف: الزخرفة يجب أن تكون معرفة
            // قبلها (من أعلى إلى أسفل، كالدوال)؛ ثم يُحلَّل
            // التعريف نفسه؛ وبعدها يُنسى توقيع المعرف —
            // فالزخرفة تستبدله بنتيجتها، ونوعها وقيمتها
            // مجهولان ستاتيكيا، والفحص الدقيق وقت التشغيل.
            StmtKind::Decorated { decorators, def } => {
                for d in decorators {
                    self.analyze_expr(d)?;
                }
                self.analyze_stmt(def)?;
                let name = match &def.kind {
                    StmtKind::FuncDef { name, .. } => Some(name.clone()),
                    StmtKind::ClassDef { name, .. } => Some(name.clone()),
                    _ => None,
                };
                if let Some(n) = name {
                    self.define(&n, VarInfo::var());
                }
                Ok(())
            }
            StmtKind::ClassDef { name, base, interfaces, type_params, methods } => {
                // البنود 145–147: معاملات نوع الفئة في
                // النطاق طوال طرائقها.
                let class_tp: Vec<(String, Option<String>)> = type_params
                    .iter()
                    .map(|tp| (tp.name.clone(), tp.bound.clone()))
                    .collect();
                self.type_param_stack.push(class_tp);
                // Base and interfaces must already be defined
                // (top-down, like functions). Compliance itself
                // is checked at runtime, where method maps exist.
                if let Some(b) = base {
                    if !self.is_defined(b) {
                        return Err(RioError::semantic(
                            format!("فئة أب غير معرفة: {}", b),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                for iname in interfaces {
                    if !self.is_defined(iname) {
                        return Err(RioError::semantic(
                            format!("واجهة غير معرفة: {}", iname),
                            stmt.line,
                            stmt.col,
                        ));
                    }
                }
                // Class name is defined BEFORE its methods, so a
                // method body may create instances of its own class.
                // Its static type is فئة.
                self.define(name, VarInfo::var_typed("فئة"));
                for m in methods {
                    if let StmtKind::FuncDef { name: mn, params, param_types, return_type, defaults, vararg, kwarg, type_params: m_type_params, body } = &m.kind {
                        // البنود 145–147: معاملات نوع الطريقة
                        // نفسها فوق معاملات نوع الفئة.
                        let m_tp: Vec<(String, Option<String>)> = m_type_params
                            .iter()
                            .map(|tp| (tp.name.clone(), tp.bound.clone()))
                            .collect();
                        self.type_param_stack.push(m_tp);
                        let mut seen = HashSet::new();
                        for p in params {
                            if !seen.insert(p) {
                                return Err(RioError::semantic(
                                    format!("معامل مكرر في الطريقة {}: {}", mn, p),
                                    m.line,
                                    m.col,
                                ));
                            }
                        }
                        // البنود 109–112: الافتراضيات تُحلَّل في
                        // نطاق التعريف (نطاق الفئة هنا).
                        for d in defaults.iter().flatten() {
                            self.analyze_expr(d)?;
                        }
                        // Method scope: params + نفس (auto-bound to
                        // the instance, type كائن). نفس is NOT a param.
                        self.scopes.push(HashMap::new());
                        for (p, pt) in params.iter().zip(param_types.iter()) {
                            match pt {
                                Some(t) => self.define(p, VarInfo::var_typed(t)),
                                None => self.define(p, VarInfo::var()),
                            }
                        }
                        if let Some(vn) = vararg {
                            self.define(vn, VarInfo::var_typed("قائمة"));
                        }
                        if let Some(kn) = kwarg {
                            self.define(kn, VarInfo::var_typed("قاموس"));
                        }
                        self.define("نفس", VarInfo::var_typed("كائن"));
                        self.push_decls(body, params, &format!("الطريقة {}", mn), m.line, m.col)?;
                        self.function_depth += 1;
                        self.return_type_stack.push(return_type.clone());
                        let saved_loop = self.loop_depth;
                        self.loop_depth = 0;
                        let r = self.analyze_block(body);
                        self.loop_depth = saved_loop;
                        self.return_type_stack.pop();
                        self.function_depth -= 1;
                        self.scopes.pop();
                        self.decl_stack.pop();
                        self.type_param_stack.pop();
                        r?;
                    }
                }
                self.type_param_stack.pop();
                Ok(())
            }
            StmtKind::InterfaceDef { name, .. } => {
                // Interface name is defined, type واجهة.
                // Its methods have no body to analyze (مرر only).
                self.define(name, VarInfo::var_typed("واجهة"));
                Ok(())
            }
            StmtKind::Import { module, alias, names } => {
                match names {
                    Some(names) => {
                        // من وحدة استورد اسم1، اسم2 : each name is
                        // defined, with unknown arity and type (it may
                        // be a value, not a function; runtime checks).
                        for n in names {
                            self.define(n, VarInfo::var());
                        }
                    }
                    None => {
                        // استورد وحدة [باسم مختصر] : bind module value,
                        // whose type is وحدة.
                        let bind = alias.clone().unwrap_or_else(|| module.clone());
                        self.define(&bind, VarInfo::var_typed("وحدة"));
                    }
                }
                Ok(())
            }
            StmtKind::Export { .. } => Ok(()),
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) -> RioResult<()> {
        match &expr.kind {
            ExprKind::Number(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Nil => Ok(()),
            ExprKind::Ident(name) => {
                if self.is_defined(name) {
                    Ok(())
                } else {
                    Err(RioError::semantic(
                        format!("متغير غير معرف: {}", name),
                        expr.line,
                        expr.col,
                    ))
                }
            }
            ExprKind::List(items) => {
                for it in items {
                    self.analyze_expr(it)?;
                }
                Ok(())
            }
            ExprKind::Dict(pairs) => {
                for (_, v) in pairs {
                    self.analyze_expr(v)?;
                }
                Ok(())
            }
            // البند 73 (`:=`): تحليل القيمة، ثم تعريف
            // الاسم بنوعها المستنتج — كإسناد `=` بلا
            // تعليق: عام إلى scopes[0]، ونطاق بلا
            // تعريف محلي، وإلا في النطاق الجاري إن
            // كان الاسم غائبا في كل النطاقات.
            ExprKind::AssignExpr { name, value } => {
                self.analyze_expr(value)?;
                let t = self.infer_type(value);
                if self.is_global(name) {
                    if !self.scopes[0].contains_key(name) {
                        match t {
                            Some(t) => {
                                self.scopes[0]
                                    .insert(name.clone(), VarInfo::var_typed(&t));
                            }
                            None => {
                                self.scopes[0].insert(name.clone(), VarInfo::var());
                            }
                        }
                    }
                } else if !self.is_nonlocal(name) && self.lookup(name).is_none() {
                    match t {
                        Some(t) => self.define(name, VarInfo::var_typed(&t)),
                        None => self.define(name, VarInfo::var()),
                    }
                }
                Ok(())
            }
            ExprKind::Binary { left, op, right } => {
                // `و` / `أو` short-circuit at runtime, but both sides
                // are still in scope, so both are analyzed.
                let _ = op;
                self.analyze_expr(left)?;
                self.analyze_expr(right)?;
                Ok(())
            }
            ExprKind::Unary { op: _, expr: e } => self.analyze_expr(e),
            ExprKind::Call { callee, args, kwargs, .. } => {
                // ب6: `ولد`/`ولد من` خارج أي دالة خطأ
                // دلالي — المولد دالة تحوي ولدا، فلا
                // توليد في المستوى الأعلى. (لامدا تحوي
                // ولدا مولد هي الأخرى: جسمها المخزَّن
                // `ارجع` يحوي الاستدعاء، فيكشفه فحص
                // الجسم عند استدعائها.)
                if let ExprKind::Ident(name) = &callee.kind {
                    if (name == crate::ast::YIELD_CALL || name == crate::ast::YIELD_FROM_CALL)
                        && self.function_depth == 0
                    {
                        return Err(RioError::semantic(
                            "ولد خارج دالة — التوليد يكون داخل دالة فقط",
                            expr.line,
                            expr.col,
                        ));
                    }
                }
                // Static matching for direct calls to known
                // functions (البنود 109–112): same order and
                // messages as the runtime resolver.
                let mut known_sig: Option<(Vec<String>, Vec<Option<String>>, bool)> = None;
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(info) = self.lookup(name) {
                        if let Some(n) = info.arity {
                            let plain = info.n_defaults == 0
                                && !info.has_vararg
                                && !info.has_kwarg
                                && kwargs.is_empty();
                            if plain {
                                if args.len() != n {
                                    return Err(RioError::semantic(
                                        format!(
                                            "عدد المعاملات غير صحيح: الدالة {} تتوقع {} لكن أعطيت {}",
                                            name,
                                            n,
                                            args.len()
                                        ),
                                        expr.line,
                                        expr.col,
                                    ));
                                }
                            } else {
                                // (1) فيض موضعي بلا جامع.
                                if !info.has_vararg && args.len() > n {
                                    return Err(RioError::semantic(
                                        format!(
                                            "عدد المعاملات غير صحيح: الدالة {} تتوقع {} لكن أعطيت {}",
                                            name,
                                            n,
                                            args.len() + kwargs.len()
                                        ),
                                        expr.line,
                                        expr.col,
                                    ));
                                }
                                // (3) ازدواج، ثم (4) مجهول —
                                // لكل مفتاحي بالترتيب.
                                for (k, _) in kwargs {
                                    match info.params.iter().position(|p| p == k) {
                                        Some(i) => {
                                            if i < args.len() {
                                                return Err(RioError::semantic(
                                                    format!("المعامل {} أعطي موضعيا ومفتاحيا", k),
                                                    expr.line,
                                                    expr.col,
                                                ));
                                            }
                                        }
                                        None => {
                                            if !info.has_kwarg {
                                                return Err(RioError::semantic(
                                                    format!("معامل مفتاحي غير معروف: {}", k),
                                                    expr.line,
                                                    expr.col,
                                                ));
                                            }
                                        }
                                    }
                                }
                                // (5) مطلوب مفقود — الافتراضيات
                                // متتالية من النهاية.
                                for (i, p) in info.params.iter().enumerate() {
                                    let has_default = i >= n - info.n_defaults;
                                    let filled_kw =
                                        kwargs.iter().any(|(k, _)| k == p);
                                    if i >= args.len() && !filled_kw && !has_default {
                                        return Err(RioError::semantic(
                                            format!("المعامل {} مطلوب في الدالة {}", p, name),
                                            expr.line,
                                            expr.col,
                                        ));
                                    }
                                }
                            }
                            // البنود 145–147: الدالة العامة يُفحص
                            // نوع معاملاتها وقت التشغيل بالربط
                            // المحلول (رسائله الدقيقة) — فالفحص
                            // الستاتيكي هنا يتخطاها كلها، وإلا
                            // رفض النوع المعلن `ت` حرفيا كل قيمة.
                            known_sig = Some((info.params.clone(), info.param_types.clone(), !info.type_params.is_empty()));
                        }
                    } else if let Some((min, max)) = builtin_arity(name) {
                        if !kwargs.is_empty() {
                            return Err(RioError::semantic(
                                format!("معامل مفتاحي غير معروف: {}", kwargs[0].0),
                                expr.line,
                                expr.col,
                            ));
                        }
                        if args.len() < min || args.len() > max {
                            return Err(RioError::semantic(
                                format!(
                                    "عدد المعاملات غير صحيح: الدالة {} تتوقع {} إلى {} لكن أعطيت {}",
                                    name, min, max, args.len()
                                ),
                                expr.line,
                                expr.col,
                            ));
                        }
                    } else if !self.is_defined(name) {
                        return Err(RioError::semantic(
                            format!("متغير غير معرف: {}", name),
                            callee.line,
                            callee.col,
                        ));
                    }
                    // If the callee is a variable holding a function
                    // (alias), arity is unknown at analysis time and
                    // is checked at runtime, not here.
                } else {
                    self.analyze_expr(callee)?;
                }
                for a in args {
                    self.analyze_expr(a)?;
                }
                for (_, v) in kwargs {
                    self.analyze_expr(v)?;
                }
                // Phase 1A Step 3: param type check for known
                // functions, after args are analyzed. Unknown arg
                // types pass (gradual); runtime checks them.
                // المفتاحيات تُفحص بنوع معاملها المسمى.
                if let Some((params, pts, is_generic)) = known_sig {
                    if !is_generic {
                    if let ExprKind::Ident(name) = &callee.kind {
                        for (arg, pt) in args.iter().zip(pts.iter()) {
                            if let Some(pt) = pt {
                                let at = self.infer_type(arg);
                                check_type_match(
                                    pt,
                                    at,
                                    &format!("معامل في الدالة {}", name),
                                    arg.line,
                                    arg.col,
                                )?;
                            }
                        }
                        for (k, v) in kwargs {
                            if let Some(i) = params.iter().position(|p| p == k) {
                                if let Some(Some(pt)) = pts.get(i) {
                                    let at = self.infer_type(v);
                                    check_type_match(
                                        pt,
                                        at,
                                        &format!("معامل في الدالة {}", name),
                                        v.line,
                                        v.col,
                                    )?;
                                }
                            }
                        }
                    }
                    }
                }
                Ok(())
            }
            ExprKind::Index { target, index } => {
                self.analyze_expr(target)?;
                self.analyze_expr(index)?;
                Ok(())
            }
            ExprKind::Create { class, args, kwargs, .. } => {
                // Class must be defined (top-down, like functions).
                // Its type is checked at runtime, so a variable
                // holding a class passes here (gradual).
                if !self.is_defined(class) {
                    return Err(RioError::semantic(
                        format!("فئة غير معرفة: {}", class),
                        expr.line,
                        expr.col,
                    ));
                }
                for a in args {
                    self.analyze_expr(a)?;
                }
                for (_, v) in kwargs {
                    self.analyze_expr(v)?;
                }
                Ok(())
            }
            ExprKind::Dot { target, name } => {
                // Member existence / export status are runtime
                // checks (module file is loaded at runtime).
                self.analyze_expr(target)?;
                // Meeting 14: static check for string/list methods
                // when the target type is known (gradual: unknown
                // types pass, and are checked at runtime instead).
                if let Some(t) = self.infer_type(target) {
                    if t == "نص" && !crate::ast::STR_METHODS.contains(&name.as_str()) {
                        return Err(RioError::semantic(
                            format!("طريقة غير معروفة لنوع نص: {}", name),
                            expr.line,
                            expr.col,
                        ));
                    }
                    if t == "قائمة" && !crate::ast::LIST_METHODS.contains(&name.as_str()) {
                        return Err(RioError::semantic(
                            format!("طريقة غير معروفة لنوع قائمة: {}", name),
                            expr.line,
                            expr.col,
                        ));
                    }
                }
                Ok(())
            }
            ExprKind::Lambda { params, param_types, body } => {
                // Meeting 13: like a FuncDef body, but no name
                // and no declared return type. Duplicate params
                // are a definition error; the body sees the
                // enclosing scopes (lexical capture).
                let mut seen = HashSet::new();
                for p in params {
                    if !seen.insert(p) {
                        return Err(RioError::semantic(
                            format!("معامل مكرر في الدالة المجهولة: {}", p),
                            expr.line,
                            expr.col,
                        ));
                    }
                }
                self.scopes.push(HashMap::new());
                for (p, pt) in params.iter().zip(param_types.iter()) {
                    match pt {
                        Some(t) => self.define(p, VarInfo::var_typed(t)),
                        None => self.define(p, VarInfo::var()),
                    }
                }
                // الدفعة الأخيرة DEF: لامدا بلا إعلانات —
                // فإسناد `:=` فيها إسناد عادي.
                self.decl_stack.push((HashSet::new(), HashSet::new()));
                self.function_depth += 1;
                self.return_type_stack.push(None);
                let saved_loop = self.loop_depth;
                self.loop_depth = 0;
                let r = self.analyze_expr(body);
                self.loop_depth = saved_loop;
                self.return_type_stack.pop();
                self.function_depth -= 1;
                self.scopes.pop();
                self.decl_stack.pop();
                r
            }
        }
    }
}
