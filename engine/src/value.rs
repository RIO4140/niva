//! Values and the type system.
//! Niva is dynamically typed: a variable may hold any type and may be
//! reassigned to another type, but every operation checks types at runtime
//! and fails with an Arabic type error instead of silently coercing.
//! Types: عدد (Number), نص (String), منطقي (Bool), قائمة (List),
//! قاموس (Dict), مترابطة (Tuple), تشكيلة (Set),
//! لاشيء (Nil), دالة (Function).

use crate::ast::{Stmt, TypeExpr};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Phase 1A Step 4: classes. Class and instance data live behind Rc
// so cloning an instance value shares its fields: a method that
// writes نفس.حقل = قيمة mutates the same fields the caller sees,
// without any write-back bookkeeping in the interpreter.
// ClassData methods map method name -> Function value.
#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: String,
    pub methods: HashMap<String, Value>,
    // Phase 1A Step 5: inheritance + interfaces.
    // base: parent class, if any (ورث). interfaces: names of
    // interfaces this class declares it implements (واجهة).
    // Method lookup walks base chain; interfaces are checked
    // at class definition time, not stored as methods.
    pub base: Option<Rc<ClassData>>,
    pub interfaces: Vec<String>,
    // البنود 145–147 (العموميات — الاجتماع 83 §98):
    // معاملات النوع المعلنة للفئة.
    pub type_params: Vec<crate::ast::TypeParam>,
}

#[derive(Debug, Clone)]
pub struct InterfaceData {
    pub name: String,
    // (method name, param count). v1 checks existence + arity only,
    // not param types or return types.
    pub methods: Vec<(String, usize)>,
}

impl ClassData {
    // Method lookup: own methods first, then base chain recursively.
    // Overriding works because own methods win.
    pub fn lookup_method(&self, name: &str) -> Option<Value> {
        if let Some(m) = self.methods.get(name) {
            return Some(m.clone());
        }
        if let Some(base) = &self.base {
            return base.lookup_method(name);
        }
        None
    }
}

#[derive(Debug)]
pub struct InstanceData {
    pub class: Rc<ClassData>,
    pub fields: RefCell<HashMap<String, Value>>,
    // البنود 145–147 (العموميات — الاجتماع 83 §98):
    // الربط المحلول وقت الإنشاء (اسم معامل النوع ← نوعه
    // الفعلي)؛ تستعمله طرائق الكائن لفحص معاملاتها
    // المعلنة بمعاملات النوع.
    pub type_binding: Vec<(String, String)>,
}
// Value::Instance holds Rc<InstanceData>, so cloning a Value clones
// the Rc pointer and shares fields; InstanceData itself is not Clone.

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    List(Vec<Value>),
    Dict(Vec<(String, Value)>),
    // البندان 138 و139 (الدفعة 3 من الـ579):
    // مترابطة: حاوية مرتبة غير قابلة للتعديل —
    // والإسناد إلى عنصرها خطأ (يرفضه المفسر والآلة
    // والأصلي برسالة «لا يمكن تعيين عنصر» نفسها).
    // تشكيلة: قيم فريدة بلا ترتيب — تُبنى بلا تكرار
    // (أول ظهور يفوز)، والمساواة بلا ترتيب.
    Tuple(Vec<Value>),
    Set(Vec<Value>),
    // ج6 (الدفعة الأخيرة — الأنواع والأعداد والدقة):
    // كبير: عدد صحيح ممتد بلا حد — سلسلة عشرية موحدة
    // (إشارة اختيارية ثم أرقام بلا أصفار بادئة، و"0" للصفر).
    // كسر: عدد نسبي دقيق — بسط ومقام موحدان (المقام موجب
    // والقاسم المشترك الأكبر 1، والصفر 0/1).
    // مركب: عدد مركب (حقيقي، تخيلي) بـf64.
    // بت: بتات 0/1 (الأهم أولا).
    // معلب: قيمة واحدة داخل علبة (عنصر واحد مهما كان نوعها).
    BigInt(String),
    Rational(String, String),
    Complex(f64, f64),
    Bits(Vec<bool>),
    Boxed(Box<Value>),
    Nil,
    // module: Some(module_name) for a function defined at the top
    // level of a module, so calls to it can push that module's scope.
    // None for functions defined in the main program (and, in Step 2,
    // for nested functions defined inside a module function, which
    // rely on the outer call's module scope already on the stack).
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        module: Option<String>,
        // Phase 1A Step 3: gradual types, None = untyped (أي).
        param_types: Vec<Option<String>>,
        return_type: Option<String>,
        // Step 1 (Meeting 10/11): lexical closures. The shared
        // scope chain at definition time, as Rc pointers, so a
        // returned function still sees (and mutates) its defining
        // environment. None for module functions (module: Some),
        // which keep the Step 2 module-scope push instead, and
        // for synthetic VM function carriers.
        captured: Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
        // Phase ك-1 (Meeting 24): VM closures. The interpreter
        // never sets this (None); the VM sets Some(id) when it
        // creates a closure value, where id keys the compiled
        // bytecode in the VM's closure registry. A plain id
        // (not the code itself) keeps value.rs free of VM types.
        vm_id: Option<u64>,
        // الدفعة الأخيرة DEF (البنود 77 و78 و109–112):
        // إعلانات النطاق في الجسم (تُحسَب عند التعريف
        // من الجسم كله، كـ collect_scope_decls)؛
        // والقيم الافتراضية المُقيَّمة وقت التعريف
        // (None = بلا افتراضية، فيُطلب المعامل)؛
        // وجامعا الزائد الموضعي والمفتاحي (None = بلا جامع).
        global_names: Vec<String>,
        nonlocal_names: Vec<String>,
        defaults: Vec<Option<Value>>,
        vararg: Option<String>,
        kwarg: Option<String>,
        // البنود 145–147 (العموميات — الاجتماع 83 §98):
        // معاملات النوع المعلنة للدالة.
        type_params: Vec<crate::ast::TypeParam>,
    },
    // Module: exports are the only names accessible from outside;
    // scope is the full top-level scope after module execution,
    // including private names, used for module function calls.
    Module {
        name: String,
        exports: HashMap<String, Value>,
        scope: HashMap<String, Value>,
    },
    // Class definition value: فئة اسم.
    Class(Rc<ClassData>),
    // Interface definition value: واجهة اسم.
    Interface(Rc<InterfaceData>),
    // Object instance: كائن من فئة. Rc-shared fields, see above.
    Instance(Rc<InstanceData>),
    // A method bound to an instance, produced by ك.طريقة evaluation.
    // Calling it binds نفس to the instance automatically.
    BoundMethod { func: Box<Value>, instance: Rc<InstanceData> },
    // A stdlib function implemented in Rust (Meeting 16). Its code
    // lives in the stdlib module; the value only carries the name
    // and the function pointer. type_name is "دالة", like a user
    // function, and the Call arm invokes the pointer directly.
    NativeFunction {
        name: String,
        func: crate::stdlib::NativeFn,
    },
    // ب6 — المولدات (الاجتماع 85 §100): كائن مولد كسول.
    // `id` مفتاح حالته في سجل المولدات (vm.rs) — إطار
    // موقوف (آلة بايت كود فرعية معلقة) وحالته؛ والقيمة
    // نفسها بيانات خالصة (Clone/Debug بلا مشاركة
    // Rc) فتنسخها الإسنادات كأي قيمة، وكل النسخ
    // تشير إلى المولد نفسه. `name` اسم دالته (أو
    // «تعبير مولد» للتعبير التوليدي) لرسائل الأخطاء.
    Generator { id: u64, name: String },
}

// Captured environments compare by scope identity (same Rc
// pointers in the same order), not by deep value comparison:
// two closures from the same factory call share their scopes,
// two from different calls do not.
fn captured_eq(
    a: &Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
    b: &Option<Vec<Rc<RefCell<HashMap<String, Value>>>>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.len() == y.len()
                && x.iter().zip(y.iter()).all(|(p, q)| Rc::ptr_eq(p, q))
        }
        _ => false,
    }
}

// Manual PartialEq: Rc values compare by identity, not by deep
// field comparison, so == on classes/instances means same object.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Dict(a), Value::Dict(b)) => a == b,
            // المترابطة مرتبة كالقائمة؛ والتشكيلة بلا
            // ترتيب: الطول نفسه وكل عنصر في إحداهما
            // له مساوٍ في الأخرى.
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => {
                a.len() == b.len()
                    && a.iter().all(|x| b.iter().any(|y| x == y))
            }
            (Value::Nil, Value::Nil) => true,
            (
                Value::Function { name: n1, params: p1, body: b1, module: m1, param_types: pt1, return_type: r1, captured: c1, vm_id: v1, global_names: g1, nonlocal_names: n1_, defaults: d1, vararg: va1, kwarg: kw1, type_params: tp1 },
                Value::Function { name: n2, params: p2, body: b2, module: m2, param_types: pt2, return_type: r2, captured: c2, vm_id: v2, global_names: g2, nonlocal_names: n2_, defaults: d2, vararg: va2, kwarg: kw2, type_params: tp2 },
            ) => n1 == n2 && p1 == p2 && b1 == b2 && m1 == m2 && pt1 == pt2 && r1 == r2
                && captured_eq(c1, c2) && v1 == v2 && g1 == g2 && n1_ == n2_ && d1 == d2 && va1 == va2 && kw1 == kw2 && tp1 == tp2,
            (
                Value::Module { name: n1, exports: e1, scope: s1 },
                Value::Module { name: n2, exports: e2, scope: s2 },
            ) => n1 == n2 && e1 == e2 && s1 == s2,
            (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
            (Value::Interface(a), Value::Interface(b)) => Rc::ptr_eq(a, b),
            (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
            (
                Value::BoundMethod { func: f1, instance: i1 },
                Value::BoundMethod { func: f2, instance: i2 },
            ) => f1 == f2 && Rc::ptr_eq(i1, i2),
            (
                Value::NativeFunction { name: n1, func: f1 },
                Value::NativeFunction { name: n2, func: f2 },
            ) => n1 == n2 && f1 == f2,
            // ب6: مولدان متساويان إن كانا المولد نفسه (المعرف).
            (Value::Generator { id: a, .. }, Value::Generator { id: b, .. }) => a == b,
            // ج6: الأنواع الجديدة — والمساواة العددية
            // عبر الأنواع (كبير/كسر/عدد/مركب بصفر تخيلي)
            // دقيقة لا عائمة، في الذراع الحارس أدناه.
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::Rational(a1, a2), Value::Rational(b1, b2)) => a1 == b1 && a2 == b2,
            (Value::Complex(a1, a2), Value::Complex(b1, b2)) => a1 == b1 && a2 == b2,
            (Value::Bits(a), Value::Bits(b)) => a == b,
            (Value::Boxed(a), Value::Boxed(b)) => a == b,
            (Value::Boxed(a), b) => &**a == b,
            (a, Value::Boxed(b)) => a == &**b,
            (a, b) if numeric_eq(a, b) => true,
            _ => false,
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "عدد",
            Value::Str(_) => "نص",
            Value::Bool(_) => "منطقي",
            Value::List(_) => "قائمة",
            Value::Dict(_) => "قاموس",
            Value::Tuple(_) => "مترابطة",
            Value::Set(_) => "تشكيلة",
            // ج6: أسماء الأنواع الجديدة.
            Value::BigInt(_) => "كبير",
            Value::Rational(_, _) => "كسر",
            Value::Complex(_, _) => "مركب",
            Value::Bits(_) => "بت",
            Value::Boxed(_) => "معلب",
            Value::Nil => "لاشيء",
            Value::Function { .. } => "دالة",
            Value::Module { .. } => "وحدة",
            // ب6: نوع المولد «مولد».
            Value::Generator { .. } => "مولد",
            Value::Class(_) => "فئة",
            Value::Interface(_) => "واجهة",
            Value::Instance(_) => "كائن",
            Value::BoundMethod { .. } => "دالة",
            Value::NativeFunction { .. } => "دالة",
        }
    }

    /// البند 88 — مدير السياق (الاجتماع 88 §103):
    /// عضو بالاسم كما يحلّه الوصول بالنقطة تماما —
    /// حقل أولا ثم طريقة (مربوطة) في الكائن، وتصدير
    /// في الوحدة؛ ولا شيء لغيرهما أو للغائب. يستعملها
    /// `__عند_افحص` في المفسر والآلة معا.
    pub fn عند_عضو(&self, name: &str) -> Option<Value> {
        match self {
            Value::Instance(data) => {
                if let Some(v) = data.fields.borrow().get(name) {
                    return Some(v.clone());
                }
                data.class.lookup_method(name).map(|m| Value::BoundMethod {
                    func: Box::new(m),
                    instance: Rc::clone(data),
                })
            }
            Value::Module { exports, .. } => exports.get(name).cloned(),
            _ => None,
        }
    }

    /// البند 88: هل هذه القيمة قابلة للاستدعاء
    /// كدالة مدير سياق — دالة أو طريقة مربوطة أو
    /// مدمجة؛ والفئة ليست قابلة (استدعاؤها يحتاج
    /// `أنشئ`)، والحقل غير الدالة ليس قابلا.
    pub fn قابل_للاستدعاء(&self) -> bool {
        matches!(
            self,
            Value::Function { .. } | Value::BoundMethod { .. } | Value::NativeFunction { .. }
        )
    }

    /// البند 88: فحص مدير السياق — الدالتان معا قبل
    /// أي استدعاء؛ والرسالتان حرفيا من §103.
    pub fn عند_افحص(v: &Value, line: usize, col: usize) -> crate::error::RioResult<()> {
        match v.عند_عضو("دخول") {
            Some(m) if m.قابل_للاستدعاء() => {}
            _ => {
                return Err(crate::error::RioError::runtime(
                    "القيمة ليست مدير سياق — لا تحمل دالة دخول",
                    line,
                    col,
                ))
            }
        }
        match v.عند_عضو("خروج") {
            Some(m) if m.قابل_للاستدعاء() => {}
            _ => {
                return Err(crate::error::RioError::runtime(
                    "القيمة ليست مدير سياق — لا تحمل دالة خروج",
                    line,
                    col,
                ))
            }
        }
        Ok(())
    }

    /// البند 88: قاموس الخطأ الممسوك لـ`__عند_خطأ()` —
    /// نوع (نص عربي كما في رسالة الخطأ) ورسالة وسطر
    /// وعمود وملف (أو لاشيء) وعلم الوحدة الغائبة.
    pub fn عند_قاموس_خطأ(e: &crate::error::RioError) -> Value {
        Value::Dict(vec![
            ("نوع".to_string(), Value::Str(e.kind.arabic().to_string())),
            ("رسالة".to_string(), Value::Str(e.message.clone())),
            ("سطر".to_string(), Value::Number(e.line as f64)),
            ("عمود".to_string(), Value::Number(e.col as f64)),
            (
                "ملف".to_string(),
                match &e.file {
                    Some(f) => Value::Str(f.clone()),
                    None => Value::Nil,
                },
            ),
            (
                "وحدة_غائبة".to_string(),
                Value::Bool(e.is_module_not_found()),
            ),
        ])
    }

    /// البند 88: عكس القاموس — إعادة بناء الخطأ نفسه
    /// (النوع والرسالة والسطر والعمود والملف وعلم
    /// الوحدة الغائبة) لـ`__عند_أعد_رفع(خطأ)`؛ وقيم
    /// ناقصة أو بأنواع خاطئة تعود إلى الافتراضي
    /// دفاعيا (داخلي فقط — القاموس يبنيه المحرك).
    pub fn عند_خطأ_من_قاموس(
        pairs: &[(String, Value)],
        line: usize,
        col: usize,
    ) -> crate::error::RioError {
        let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        let kind = match get("نوع") {
            Some(Value::Str(s)) => crate::error::ErrorKind::من_العربية(s),
            _ => crate::error::ErrorKind::Runtime,
        };
        let msg = match get("رسالة") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let ln = match get("سطر") {
            Some(Value::Number(n)) => *n as usize,
            _ => line,
        };
        let cl = match get("عمود") {
            Some(Value::Number(n)) => *n as usize,
            _ => col,
        };
        let mut e = crate::error::RioError::new(kind, msg, ln, cl);
        if let Some(Value::Str(f)) = get("ملف") {
            e = e.with_file(f.clone());
        }
        if matches!(get("وحدة_غائبة"), Some(Value::Bool(true))) {
            e = e.with_module_not_found();
        }
        e
    }

    /// هل تطابق هذه القيمة النوع المعلن بنيويا (البند 8)؟
    /// الاسم المسمى يطابقه اسم النوع؛ والعام يطابقه أساسه
    /// زائد فحص العناصر: `قائمة[عدد]` تتطلب كل عنصر عددا،
    /// و`قاموس[نص، عدد]` تتطلب كل مفتاح نصا وكل قيمة عددا.
    /// `أي` بلا فحص؛ ونص غير قابل للتحليل يُقارَن حرفيا
    /// كما كان (توافق دفاعي).
    pub fn matches_declared_type(&self, declared: &str) -> bool {
        if declared == "أي" {
            return true;
        }
        let Some(t) = TypeExpr::parse(declared) else {
            return self.type_name() == declared;
        };
        self.matches_type_expr(&t)
    }

    fn matches_type_expr(&self, t: &TypeExpr) -> bool {
        match t {
            TypeExpr::Named(n) => n == "أي" || self.type_name() == n,
            TypeExpr::Generic { name, args } => {
                if self.type_name() != name {
                    return false;
                }
                match (name.as_str(), args.as_slice()) {
                    ("قائمة", [elem]) => match self {
                        Value::List(items) => {
                            items.iter().all(|v| v.matches_type_expr(elem))
                        }
                        _ => false,
                    },
                    ("مترابطة", [elem]) => match self {
                        Value::Tuple(items) => {
                            items.iter().all(|v| v.matches_type_expr(elem))
                        }
                        _ => false,
                    },
                    ("تشكيلة", [elem]) => match self {
                        Value::Set(items) => {
                            items.iter().all(|v| v.matches_type_expr(elem))
                        }
                        _ => false,
                    },
                    ("قاموس", [key, val]) => match self {
                        Value::Dict(entries) => {
                            let key_ok = matches!(key, TypeExpr::Named(k) if k == "أي" || k == "نص");
                            key_ok
                                && entries
                                    .iter()
                                    .all(|(_, v)| v.matches_type_expr(val))
                        }
                        _ => false,
                    },
                    // عام على غير حاوية (لا يبنيه المحلل):
                    // الأساس وحده يكفي.
                    _ => true,
                }
            }
        }
    }

    /// البندان 138 و139 (الدفعة 3 من الـ579): بناء
    /// تشكيلة من قيم — بلا تكرار (أول ظهور يفوز،
    /// والترتيب الداخلي ترتيب أول ظهور، لتخرج
    /// المخرجات حتمية في المحركات الثلاثة).
    pub fn make_set(items: Vec<Value>) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for v in items {
            if !out.iter().any(|x| x == &v) {
                out.push(v);
            }
        }
        Value::Set(out)
    }

    /// عناصر أي قابل للتكرار في `لكل` وفي الفهم
    /// الضمني (البنود 141–143): قائمة أو نص (محارف)
    /// أو مترابطة أو تشكيلة (بترتيب أول ظهور)؛ وغيرها
    /// لا شيء — فيرفض المتصل برسالة `لكل` نفسها.
    pub fn iterable_items(&self) -> Option<Vec<Value>> {
        match self {
            Value::List(items) => Some(items.clone()),
            Value::Tuple(items) => Some(items.clone()),
            Value::Set(items) => Some(items.clone()),
            Value::Str(s) => Some(s.chars().map(|c| Value::Str(c.to_string())).collect()),
            // ج6: التكرار على بت يعطي أعداد 0/1 (الأهم أولا).
            Value::Bits(bits) => Some(
                bits.iter()
                    .map(|b| Value::Number(if *b { 1.0 } else { 0.0 }))
                    .collect(),
            ),
            _ => None,
        }
    }

    fn fmt_number(n: f64) -> String {
        let s = if n.fract() == 0.0 && n.abs() < 9.0e15 {
            format!("{}", n as i64)
        } else {
            format!("{}", n)
        };
        // أرقام المخرجات عربية (اجتماع 21): الأرقام الغربية
        // إلى المشرقية، والنقطة العشرية إلى ٫ (U+066B).
        // والنصوص لا تمس؛ والتدوين العلمي يبقي e اللاتينية.
        arabic_digits(&s)
    }

    /// Display for `اطبع`: strings print raw, everything else prints
    /// in a Python-like form (lists/dicts with quoted string elements).
    pub fn fmt_value(&self) -> String {
        match self {
            Value::Number(n) => Self::fmt_number(*n),
            // ج6: عرض الأنواع الجديدة — بالأرقام العربية
            // نفسها (الكبير والكسر سلسلتان عشريتان، والمركب
            // بصيغة `أ+ب ت`، والبت `بت(1010)`، والمعلب
            // `معلب(داخله)`).
            Value::BigInt(s) => arabic_digits(s),
            // ج6: الكسر بمقام 1 يطبع عددا صحيحا (كـ1 في
            // بايثون: Fraction(1, 1) تطبع 1)؛ وغيره بسط/مقام.
            Value::Rational(num, den) => {
                if den == "1" {
                    arabic_digits(num)
                } else {
                    format!("{}/{}", arabic_digits(num), arabic_digits(den))
                }
            }
            Value::Complex(re, im) => {
                if *im == 0.0 {
                    Self::fmt_number(*re)
                } else if *re == 0.0 {
                    format!("{}ت", Self::fmt_number(*im))
                } else if *im > 0.0 {
                    format!("{}+{}ت", Self::fmt_number(*re), Self::fmt_number(*im))
                } else {
                    format!("{}{}ت", Self::fmt_number(*re), Self::fmt_number(*im))
                }
            }
            Value::Bits(bits) => {
                let inner: String = bits
                    .iter()
                    .map(|b| if *b { '١' } else { '٠' })
                    .collect();
                format!("بت({})", inner)
            }
            Value::Boxed(inner) => format!("معلب({})", inner.fmt_value()),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => {
                if *b {
                    "صحيح".to_string()
                } else {
                    "خطأ".to_string()
                }
            }
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.fmt_in_collection()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Dict(pairs) => {
                let inner: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("\"{}\": {}", escape_str(k), v.fmt_in_collection()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.fmt_in_collection()).collect();
                if items.len() == 1 {
                    format!("({},)", inner.join(", "))
                } else {
                    format!("({})", inner.join(", "))
                }
            }
            Value::Set(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.fmt_in_collection()).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Nil => "لاشيء".to_string(),
            Value::Function { name, .. } => format!("<دالة {}>", name),
            Value::Module { name, .. } => format!("<وحدة {}>", name),
            Value::Class(data) => format!("<فئة {}>", data.name),
            Value::Interface(data) => format!("<واجهة {}>", data.name),
            Value::Instance(data) => format!("<كائن {}>", data.class.name),
            Value::BoundMethod { func, .. } => match &**func {
                Value::Function { name, .. } => format!("<دالة {}>", name),
                _ => "<دالة>".to_string(),
            },
            Value::NativeFunction { name, .. } => format!("<دالة {}>", name),
            // ب6: عرض المولد باسمه.
            Value::Generator { name, .. } => format!("<مولد {}>", name),
        }
    }

    fn fmt_in_collection(&self) -> String {
        match self {
            Value::Str(s) => format!("\"{}\"", escape_str(s)),
            other => other.fmt_value(),
        }
    }
}

fn escape_str(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// الدفعة 1 — رموز الخروج (الميزتان 72 و76، اجتماع 54
/// §69): رمز خروج صالح هو عدد صحيح بين ٠ و٢٥٥. أي قيمة
/// أخرى (كسر، أو سالب، أو فوق ٢٥٥، أو غير عدد) ترفض.
/// مساعد واحد للمحركين الراستيين، فلا يفترقان في الحد.
pub fn exit_code_number(v: &Value) -> Option<u8> {
    match v {
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= 255.0 => {
            Some(*n as u8)
        }
        _ => None,
    }
}

/// ج6 (الدفعة الأخيرة — الأنواع والأعداد والدقة):
/// تحويل أرقام غربية ونقطة عشرية إلى العربية المشرقية
/// (والنقطة إلى ٫)؛ والإشارة والتدوين العلمي كما هما.
pub(crate) fn arabic_digits(s: &str) -> String {
    const ARABIC_DIGITS: [char; 10] = ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];
    s.chars()
        .map(|c| match c {
            '0'..='9' => ARABIC_DIGITS[c as usize - '0' as usize],
            '.' => '٫',
            other => other,
        })
        .collect()
}

// ---------- الأعداد الصحيحة الممتدة (بنية عشرية نصية) ----------
// التمثيل الموحد: إشارة اختيارية `-` ثم أرقام بلا أصفار
// بادئة، و"0" للصفر بلا إشارة. كل العمليات هنا خالصة
// وحتمية، وتشاركها الدوال المدمجة والعمليات الثنائية.

/// توحيد نص إلى كبير صالح، أو None إن لم يكن `[+-]?أرقام`.
pub(crate) fn bigint_norm(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let stripped = digits.trim_start_matches('0');
    if stripped.is_empty() {
        return Some("0".to_string());
    }
    Some(if neg { format!("-{}", stripped) } else { stripped.to_string() })
}

pub(crate) fn bigint_is_zero(s: &str) -> bool {
    s == "0"
}

pub(crate) fn bigint_neg(s: &str) -> String {
    if s == "0" {
        return "0".to_string();
    }
    if let Some(rest) = s.strip_prefix('-') {
        rest.to_string()
    } else {
        format!("-{}", s)
    }
}

fn bigint_split(s: &str) -> (bool, Vec<u8>) {
    // (سالب؟، أرقام من الأقل أهمية إلى الأعلى).
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let mut v: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    v.reverse();
    (neg, v)
}

fn bigint_from_lsf(neg: bool, mut v: Vec<u8>) -> String {
    while v.len() > 1 && v[v.len() - 1] == 0 {
        v.pop();
    }
    if v.len() == 1 && v[0] == 0 {
        return "0".to_string();
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    for d in v.iter().rev() {
        out.push((b'0' + *d) as char);
    }
    out
}

fn bigint_cmp_mag(a: &str, b: &str) -> std::cmp::Ordering {
    let (_, av) = bigint_split(a);
    let (_, bv) = bigint_split(b);
    let (mut al, mut bl) = (av.len(), bv.len());
    while al > 1 && av[al - 1] == 0 {
        al -= 1;
    }
    while bl > 1 && bv[bl - 1] == 0 {
        bl -= 1;
    }
    if al != bl {
        return al.cmp(&bl);
    }
    for i in (0..al).rev() {
        if av[i] != bv[i] {
            return av[i].cmp(&bv[i]);
        }
    }
    std::cmp::Ordering::Equal
}

pub(crate) fn bigint_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let (an, _) = bigint_split(a);
    let (bn, _) = bigint_split(b);
    match (an, bn) {
        (true, false) => return Less,
        (false, true) => return Greater,
        _ => {}
    }
    let m = bigint_cmp_mag(a, b);
    if an { m.reverse() } else { m }
}

fn bigint_add_mag(a: &str, b: &str) -> String {
    let (_, av) = bigint_split(a);
    let (_, bv) = bigint_split(b);
    let mut out = Vec::new();
    let mut carry = 0u8;
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        let s = x + y + carry;
        out.push(s % 10);
        carry = s / 10;
    }
    if carry > 0 {
        out.push(carry);
    }
    bigint_from_lsf(false, out)
}

fn bigint_sub_mag(a: &str, b: &str) -> String {
    // يتطلب |أ| >= |ب|.
    let (_, av) = bigint_split(a);
    let (_, bv) = bigint_split(b);
    let mut out = Vec::new();
    let mut borrow = 0i8;
    for i in 0..av.len() {
        let x = av[i] as i8 - borrow;
        let y = bv.get(i).copied().unwrap_or(0) as i8;
        if x < y {
            out.push((x + 10 - y) as u8);
            borrow = 1;
        } else {
            out.push((x - y) as u8);
            borrow = 0;
        }
    }
    bigint_from_lsf(false, out)
}

pub(crate) fn bigint_add(a: &str, b: &str) -> String {
    let (an, _) = bigint_split(a);
    let (bn, _) = bigint_split(b);
    match (an, bn) {
        (false, false) => bigint_add_mag(a, b),
        (true, true) => bigint_neg(&bigint_add_mag(a, b)),
        (false, true) => match bigint_cmp_mag(a, b) {
            std::cmp::Ordering::Equal => "0".to_string(),
            std::cmp::Ordering::Greater => bigint_sub_mag(a, b),
            std::cmp::Ordering::Less => bigint_neg(&bigint_sub_mag(b, a)),
        },
        (true, false) => match bigint_cmp_mag(a, b) {
            std::cmp::Ordering::Equal => "0".to_string(),
            std::cmp::Ordering::Greater => bigint_neg(&bigint_sub_mag(a, b)),
            std::cmp::Ordering::Less => bigint_sub_mag(b, a),
        },
    }
}

pub(crate) fn bigint_sub(a: &str, b: &str) -> String {
    bigint_add(a, &bigint_neg(b))
}

pub(crate) fn bigint_mul(a: &str, b: &str) -> String {
    let (an, _) = bigint_split(a);
    let (bn, _) = bigint_split(b);
    if bigint_is_zero(a) || bigint_is_zero(b) {
        return "0".to_string();
    }
    let (_, av) = bigint_split(a);
    let (_, bv) = bigint_split(b);
    let mut out = vec![0u16; av.len() + bv.len()];
    for i in 0..av.len() {
        let mut carry = 0u16;
        for j in 0..bv.len() {
            let s = out[i + j] + (av[i] as u16) * (bv[j] as u16) + carry;
            out[i + j] = s % 10;
            carry = s / 10;
        }
        let mut k = i + bv.len();
        while carry > 0 {
            let s = out[k] + carry;
            out[k] = s % 10;
            carry = s / 10;
            k += 1;
        }
    }
    let v: Vec<u8> = out.into_iter().map(|x| x as u8).collect();
    let neg = an != bn;
    bigint_from_lsf(neg, v)
}

fn bigint_mul_small(a: &str, m: u8) -> String {
    if m == 0 || bigint_is_zero(a) {
        return "0".to_string();
    }
    let (_, av) = bigint_split(a);
    let mut out = Vec::new();
    let mut carry = 0u16;
    for d in av {
        let s = (d as u16) * (m as u16) + carry;
        out.push((s % 10) as u8);
        carry = s / 10;
    }
    while carry > 0 {
        out.push((carry % 10) as u8);
        carry /= 10;
    }
    bigint_from_lsf(false, out)
}

/// قسمة صحيحة مقطوعة (باقي بإشارة المقسوم) — None عند
/// القسمة على صفر. تعيد (خارج، باقي) موحدين.
pub(crate) fn bigint_divmod(a: &str, b: &str) -> Option<(String, String)> {
    if bigint_is_zero(b) {
        return None;
    }
    let (an, _) = bigint_split(a);
    let (bn, _) = bigint_split(b);
    let amag = if an { &a[1..] } else { a };
    let bmag = if bn { &b[1..] } else { b };
    if bigint_cmp_mag(amag, bmag) == std::cmp::Ordering::Less {
        return Some(("0".to_string(), a.to_string()));
    }
    // قسمة مطولة عشرية: باقٍ يضرب في 10 ويضاف رقم.
    let digits: Vec<u8> = amag.bytes().map(|c| c - b'0').collect();
    let mut rem = "0".to_string();
    let mut quot = String::new();
    for d in digits {
        rem = bigint_add_mag(&bigint_mul_small(&rem, 10), &d.to_string());
        let mut q = 9u8;
        while q > 0 && bigint_cmp_mag(&bigint_mul_small(bmag, q), &rem) == std::cmp::Ordering::Greater {
            q -= 1;
        }
        quot.push((b'0' + q) as char);
        if q > 0 {
            rem = bigint_sub_mag(&rem, &bigint_mul_small(bmag, q));
        }
    }
    let qnorm = bigint_norm(&quot).unwrap_or_else(|| "0".to_string());
    let q = if an != bn { bigint_neg(&qnorm) } else { qnorm };
    let r = if an { bigint_neg(&rem) } else { rem };
    Some((q, r))
}

pub(crate) fn bigint_gcd(a: &str, b: &str) -> String {
    let mut x = if a.starts_with('-') { a[1..].to_string() } else { a.to_string() };
    let mut y = if b.starts_with('-') { b[1..].to_string() } else { b.to_string() };
    while !bigint_is_zero(&y) {
        let (_, r) = bigint_divmod(&x, &y).unwrap_or_else(|| ("0".to_string(), "0".to_string()));
        x = y;
        y = r;
    }
    x
}

pub(crate) fn bigint_pow10(n: usize) -> String {
    let mut s = String::from("1");
    s.push_str(&"0".repeat(n));
    s
}

pub(crate) fn bigint_to_f64(s: &str) -> f64 {
    let (neg, v) = bigint_split(s);
    let mut out = 0.0f64;
    for d in v.iter().rev() {
        out = out * 10.0 + (*d as f64);
    }
    if neg { -out } else { out }
}

/// عدد f64 صحيح منتهٍ إلى كبير، أو None (كسر/غير منتهٍ).
pub(crate) fn bigint_from_f64(n: f64) -> Option<String> {
    if !n.is_finite() || n.fract() != 0.0 {
        return None;
    }
    bigint_norm(&format!("{:.0}", n))
}

// ---------- الكسور النسبية الدقيقة ----------

/// توحيد بسط/مقام إلى كسر موحد، أو None (مقام صفر أو نص سيئ).
pub(crate) fn rat_norm(num: &str, den: &str) -> Option<(String, String)> {
    let n = bigint_norm(num)?;
    let d = bigint_norm(den)?;
    if bigint_is_zero(&d) {
        return None;
    }
    let (n, d) = if d.starts_with('-') {
        (bigint_neg(&n), bigint_neg(&d))
    } else {
        (n, d)
    };
    if bigint_is_zero(&n) {
        return Some(("0".to_string(), "1".to_string()));
    }
    let g = bigint_gcd(&n, &d);
    let (q1, _) = bigint_divmod(&n, &g)?;
    let (q2, _) = bigint_divmod(&d, &g)?;
    Some((q1, q2))
}

pub(crate) fn rat_neg(r: &(String, String)) -> (String, String) {
    (bigint_neg(&r.0), r.1.clone())
}

pub(crate) fn rat_add(a: &(String, String), b: &(String, String)) -> (String, String) {
    let n = bigint_add(
        &bigint_mul(&a.0, &b.1),
        &bigint_mul(&b.0, &a.1),
    );
    let d = bigint_mul(&a.1, &b.1);
    rat_norm(&n, &d).unwrap_or_else(|| ("0".to_string(), "1".to_string()))
}

pub(crate) fn rat_sub(a: &(String, String), b: &(String, String)) -> (String, String) {
    rat_add(a, &rat_neg(b))
}

pub(crate) fn rat_mul(a: &(String, String), b: &(String, String)) -> (String, String) {
    let n = bigint_mul(&a.0, &b.0);
    let d = bigint_mul(&a.1, &b.1);
    rat_norm(&n, &d).unwrap_or_else(|| ("0".to_string(), "1".to_string()))
}

pub(crate) fn rat_div(a: &(String, String), b: &(String, String)) -> Option<(String, String)> {
    if bigint_is_zero(&b.0) {
        return None;
    }
    let n = bigint_mul(&a.0, &b.1);
    let d = bigint_mul(&a.1, &b.0);
    rat_norm(&n, &d)
}

/// باقي الكسر المقطوع: أ - قطع(أ/ب)*ب (بإشارة أ).
pub(crate) fn rat_rem(a: &(String, String), b: &(String, String)) -> Option<(String, String)> {
    let q = rat_div_to_bigint(a, b)?;
    let prod = rat_mul(b, &rat_from_bigint(&q));
    Some(rat_sub(a, &prod))
}

fn rat_div_to_bigint(a: &(String, String), b: &(String, String)) -> Option<String> {
    if bigint_is_zero(&b.0) {
        return None;
    }
    let n = bigint_mul(&a.0, &b.1);
    let d = bigint_mul(&a.1, &b.0);
    let (q, _) = bigint_divmod(&n, &d)?;
    Some(q)
}

fn rat_from_bigint(n: &str) -> (String, String) {
    (n.to_string(), "1".to_string())
}

pub(crate) fn rat_cmp(a: &(String, String), b: &(String, String)) -> std::cmp::Ordering {
    let l = bigint_mul(&a.0, &b.1);
    let r = bigint_mul(&b.0, &a.1);
    bigint_cmp(&l, &r)
}

pub(crate) fn rat_to_f64(r: &(String, String)) -> f64 {
    bigint_to_f64(&r.0) / bigint_to_f64(&r.1)
}

/// تحويل نص عشري `[-]أرقام[.أرقام]` إلى كسر دقيق.
pub(crate) fn rat_from_decimal(s: &str) -> Option<(String, String)> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    if body.matches('.').count() > 1 {
        return None;
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let scale = bigint_pow10(frac_part.len());
    let digits = format!("{}{}", int_part, frac_part);
    let mut num = bigint_norm(&digits).unwrap_or_else(|| "0".to_string());
    if neg {
        num = bigint_neg(&num);
    }
    rat_norm(&num, &scale)
}

/// عدد f64 منتهٍ إلى كسره العشري الأقصر دقيقا.
pub(crate) fn rat_from_f64(n: f64) -> Option<(String, String)> {
    if !n.is_finite() {
        return None;
    }
    rat_from_decimal(&format!("{}", n))
}

// ---------- تحويلات القيم العددية ----------

/// قيمة عددية (عدد/كبير/كسر) إلى كسرها الدقيق، أو None.
pub(crate) fn value_to_rational(v: &Value) -> Option<(String, String)> {
    match v {
        Value::Number(n) => rat_from_f64(*n),
        Value::BigInt(s) => Some((s.clone(), "1".to_string())),
        Value::Rational(n, d) => Some((n.clone(), d.clone())),
        _ => None,
    }
}

/// قيمة عددية إلى زوج مركب (حقيقي، تخيلي)، أو None.
pub(crate) fn value_to_complex(v: &Value) -> Option<(f64, f64)> {
    match v {
        Value::Number(n) => Some((*n, 0.0)),
        Value::BigInt(s) => Some((bigint_to_f64(s), 0.0)),
        Value::Rational(n, d) => Some((rat_to_f64(&(n.clone(), d.clone())), 0.0)),
        Value::Complex(re, im) => Some((*re, *im)),
        _ => None,
    }
}

/// هل القيمة عددية (عدد/كبير/كسر/مركب)؟
pub(crate) fn value_is_numeric(v: &Value) -> bool {
    matches!(
        v,
        Value::Number(_) | Value::BigInt(_) | Value::Rational(_, _) | Value::Complex(_, _)
    )
}

/// مساواة عددية دقيقة عبر الأنواع: الكبير والكسر والعدد
/// (بتمثيله العشري) والمركب بصفر تخيلي. غير العددية false.
pub(crate) fn numeric_eq(a: &Value, b: &Value) -> bool {
    if !value_is_numeric(a) || !value_is_numeric(b) {
        return false;
    }
    let ac = value_to_complex(a).unwrap_or((f64::NAN, f64::NAN));
    let bc = value_to_complex(b).unwrap_or((f64::NAN, f64::NAN));
    let a_complex = matches!(a, Value::Complex(_, _)) && ac.1 != 0.0;
    let b_complex = matches!(b, Value::Complex(_, _)) && bc.1 != 0.0;
    if a_complex || b_complex {
        return ac.0 == bc.0 && ac.1 == bc.1;
    }
    match (value_to_rational(a), value_to_rational(b)) {
        (Some(ra), Some(rb)) => rat_cmp(&ra, &rb) == std::cmp::Ordering::Equal,
        _ => {
            // غير منتهٍ (ما لا نهاية/NaN): مقارنة f64 كما كانت.
            let (x, y) = match (a, b) {
                (Value::Number(x), Value::Number(y)) => (*x, *y),
                _ => return false,
            };
            x == y
        }
    }
}

/// ترتيب عددي دقيق عبر (عدد/كبير/كسر)؛ والمركب بلا ترتيب.
pub(crate) fn numeric_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if !value_is_numeric(a) || !value_is_numeric(b) {
        return None;
    }
    if matches!(a, Value::Complex(_, _)) || matches!(b, Value::Complex(_, _)) {
        return None;
    }
    match (value_to_rational(a), value_to_rational(b)) {
        (Some(ra), Some(rb)) => Some(rat_cmp(&ra, &rb)),
        _ => match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
            _ => None,
        },
    }
}
