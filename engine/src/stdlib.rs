//! Step 2 (Meeting 16): the initial standard library.
//!
//! Native (Rust) modules, no external crates. A native module is a
//! Module value whose exports are NativeFunction values (or plain
//! constants). `load_module` consults `native_module` BEFORE
//! reading a `.نيفا` file, so `استورد رياضيات` works anywhere with
//! no file needed, and a same-named file in the program directory
//! never shadows a native module. The VM loads the same native
//! modules (task 4, ك-1.3), so stdlib works identically on both
//! engines.
//!
//! All angles are radians. The random module is a SplitMix64
//! generator seeded from system time + process id: fine for
//! games and simulations, NOT cryptographic — never present it
//! as secure randomness.

use crate::error::{RioError, RioResult};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A native stdlib function: engine context, args, source
/// line, source column.
///
/// --- سياق استدعاء الدوال الأصلية (خادم HTTPS، اجتماع 27) ---
///
/// الدوال الأصلية (NativeFunction) كانت تستدعى بلا سياق
/// محرك؛ فلا تستطيع دالة أصلية استدعاء دالة نيفا (معالج
/// الخادم مثلا). كل دالة أصلية تستلم الآن `&mut dyn
/// HandlerCaller`: واجهة المحرك المنفذ (المفسر أو المترجم)
/// الذي يوفر استدعاء المعالج + إذن الخادم + سياسة
/// ملفات الشهادة/المفتاح. القيم Rc لا تعبر الخيوط،
/// فالسياق لا يُنقَل إلى خيط اتصال أبدا — خيوط
/// الاتصالات ترسل الطلب إلى خيط المحرك عبر قناة،
/// والمحرك وحده يستدعي المعالج عبر هذا السياق.
pub trait HandlerCaller {
    /// استدعاء دالة نيفا (معالج) بقيمة واحدة، وإرجاع
    /// (النتيجة، المخرجات المطبوعة أثناء الاستدعاء).
    fn call_handler(
        &mut self,
        func: &Value,
        arg: Value,
        line: usize,
        col: usize,
    ) -> (RioResult<Value>, String);
    /// هل فُتح الخادم لهذا المحرك (علم --اسمح-بالخادم
    /// أو :اسمح-بالخادم)؟
    fn server_allowed(&self) -> bool;
    /// سياسة الملفات لتحميل الشهادة والمفتاح (مجلد
    /// البرنامج + موثوق) — تُقرأ من المحرك نفسه،
    /// لا من متغير عام.
    fn server_file_policy(&self) -> crate::permissions::FilePolicy;
    /// المجلد الحالي للجلسة (أساس المسارات النسبية).
    fn server_cwd(&self) -> std::path::PathBuf;
}

pub type NativeFn = fn(&mut dyn HandlerCaller, &[Value], usize, usize) -> RioResult<Value>;

/// Build the native module with this name, or None if it is not
/// a native module (then the caller falls back to a `.نيفا` file).
pub fn native_module(name: &str) -> Option<Value> {
    let entries: Vec<(&str, Value)> = match name {
        "رياضيات" => math_entries(),
        "عشوائي" => random_entries(),
        "وقت" => time_entries(),
        // Meeting 22 (DESIGN §16): the network module — one
        // function, اجلب(url), HTTP GET only, returns text.
        // Network is OFF by default (see network.rs); without
        // permission اجلب fails with a clear Arabic error.
        "شبكة" => network_entries(),
        // Catalog task 10 (عمليات, from kernel/): spawn, wait,
        // kill, current pid. Permission design first — see
        // CATALOG-01-OPERATIONS.md §1 (program-dir only by
        // default, --اسمح-بالكل lifts it; no shell ever).
        "عمليات" => processes_entries(),
        // Catalog task 11 (ذاكرة, from mm/): reserve a block,
        // free it, free memory now, page size. Permission
        // design first — see CATALOG-02-MEMORY.md §1 (handles
        // only, never a raw pointer; 64 MiB per block, 256 MiB
        // live in total; the two queries are Linux-native and
        // refuse honestly elsewhere).
        "ذاكرة" => memory_entries(),
        // Catalog task 12 (ملفات, from fs/): open a file by
        // handle, read it whole, write (replace) it whole.
        // Permission design first — see CATALOG-03-FILES.md §1
        // (the policy check happens once, at افتح, through the
        // session's own FilePolicy, so the outside-folder error
        // is identical to `اقرأ_ملف`'s; اقرأ/اكتب take a
        // numeric handle only; same 5 MiB read cap and 100 MiB
        // program-dir quota as the flat builtins; WASM refuses
        // with the flat builtins' own message).
        "ملفات" => files_entries(),
        _ => return None,
    };
    let mut exports: HashMap<String, Value> = HashMap::new();
    let mut scope: HashMap<String, Value> = HashMap::new();
    for (n, v) in entries {
        exports.insert(n.to_string(), v.clone());
        scope.insert(n.to_string(), v);
    }
    Some(Value::Module {
        name: name.to_string(),
        exports,
        scope,
    })
}

fn native(name: &str, func: NativeFn) -> Value {
    Value::NativeFunction {
        name: name.to_string(),
        func,
    }
}

// Error helpers: same Arabic wording style as the builtins
// (arity first, then types), with the full dotted name so the
// user knows exactly which function failed.
fn need_args(display: &str, args: &[Value], expected: usize, line: usize, col: usize) -> RioResult<()> {
    if args.len() != expected {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع {} معاملات لكن أعطيت {}",
                display,
                expected,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

fn need_number(display: &str, v: &Value, line: usize, col: usize) -> RioResult<f64> {
    match v {
        Value::Number(x) => Ok(*x),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع عددا لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn need_integer(display: &str, v: &Value, line: usize, col: usize) -> RioResult<i64> {
    let x = need_number(display, v, line, col)?;
    if x.fract() != 0.0 || x.abs() >= 9.0e15 {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع عددا صحيحا", display),
            line,
            col,
        ));
    }
    Ok(x as i64)
}

fn need_string(display: &str, v: &Value, line: usize, col: usize) -> RioResult<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع نصا لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

// --- رياضيات ---

fn math_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("جذر", native("جذر", math_sqrt)),
        ("قوة", native("قوة", math_pow)),
        ("جيب", native("جيب", math_sin)),
        ("جيب_تمام", native("جيب_تمام", math_cos)),
        ("ظل", native("ظل", math_tan)),
        ("لوغاريتم", native("لوغاريتم", math_ln)),
        ("أس_طبيعي", native("أس_طبيعي", math_exp)),
        ("أرضية", native("أرضية", math_floor)),
        ("سقف", native("سقف", math_ceil)),
        ("باي", Value::Number(std::f64::consts::PI)),
        // Written هـ in source: the tatweel is a diacritic, so the
        // lexer stores the identifier as ه — register that form.
        ("ه", Value::Number(std::f64::consts::E)),
    ]
}

fn math_sqrt(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جذر", args, 1, line, col)?;
    let x = need_number("رياضيات.جذر", &args[0], line, col)?;
    if x < 0.0 {
        return Err(RioError::runtime("جذر عدد سالب غير معرف", line, col));
    }
    Ok(Value::Number(x.sqrt()))
}

fn math_pow(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.قوة", args, 2, line, col)?;
    let base = need_number("رياضيات.قوة", &args[0], line, col)?;
    let exp = need_number("رياضيات.قوة", &args[1], line, col)?;
    Ok(Value::Number(base.powf(exp)))
}

fn math_sin(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.جيب", &args[0], line, col)?.sin()))
}

fn math_cos(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب_تمام", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.جيب_تمام", &args[0], line, col)?.cos()))
}

fn math_tan(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ظل", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.ظل", &args[0], line, col)?.tan()))
}

fn math_ln(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.لوغاريتم", args, 1, line, col)?;
    let x = need_number("رياضيات.لوغاريتم", &args[0], line, col)?;
    if x <= 0.0 {
        return Err(RioError::runtime("لوغاريتم عدد غير موجب غير معرف", line, col));
    }
    Ok(Value::Number(x.ln()))
}

fn math_exp(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أس_طبيعي", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.أس_طبيعي", &args[0], line, col)?.exp()))
}

fn math_floor(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أرضية", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.أرضية", &args[0], line, col)?.floor()))
}

fn math_ceil(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.سقف", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.سقف", &args[0], line, col)?.ceil()))
}

// --- عشوائي ---

// SplitMix64 state, seeded once from system time + process id.
// A zero state means "not seeded yet" (the seed is forced nonzero).
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

fn next_u64() -> u64 {
    let mut state = RNG_STATE.load(Ordering::Relaxed);
    if state == 0 {
        // SystemTime traps on wasm32-unknown-unknown (no clock
        // in std there), so the browser clock comes from js-sys.
        #[cfg(target_arch = "wasm32")]
        let nanos = (js_sys::Date::now() * 1_000_000.0) as u64;
        #[cfg(not(target_arch = "wasm32"))]
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        // No processes on wasm32 — the time nanos alone seed it.
        #[cfg(target_arch = "wasm32")]
        let pid_part = 0u64;
        #[cfg(not(target_arch = "wasm32"))]
        let pid_part = (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15);
        state = nanos ^ pid_part ^ 0x2545F4914F6CDD1D;
        if state == 0 {
            state = 0x2545F4914F6CDD1D;
        }
        RNG_STATE.store(state, Ordering::Relaxed);
    }
    let z = state.wrapping_add(0x9E3779B97F4A7C15);
    RNG_STATE.store(z, Ordering::Relaxed);
    let mut x = z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

fn random_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("عدد_عشوائي", native("عدد_عشوائي", random_float)),
        ("صحيح_عشوائي", native("صحيح_عشوائي", random_int)),
        ("اختيار", native("اختيار", random_choice)),
    ]
}

fn random_float(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عشوائي.عدد_عشوائي", args, 0, line, col)?;
    Ok(Value::Number((next_u64() >> 11) as f64 / (1u64 << 53) as f64))
}

fn random_int(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عشوائي.صحيح_عشوائي", args, 2, line, col)?;
    let min = need_integer("عشوائي.صحيح_عشوائي", &args[0], line, col)?;
    let max = need_integer("عشوائي.صحيح_عشوائي", &args[1], line, col)?;
    if min > max {
        return Err(RioError::runtime(
            "صحيح_عشوائي: الحد الأدنى أكبر من الحد الأقصى",
            line,
            col,
        ));
    }
    let range = (max as u128 - min as u128 + 1) as u64;
    Ok(Value::Number((min as i128 + (next_u64() % range) as i128) as f64))
}

fn random_choice(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عشوائي.اختيار", args, 1, line, col)?;
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                return Err(RioError::runtime(
                    "لا يمكن الاختيار من قائمة فارغة",
                    line,
                    col,
                ));
            }
            Ok(items[(next_u64() % items.len() as u64) as usize].clone())
        }
        other => Err(RioError::typ(
            format!(
                "الدالة عشوائي.اختيار تتوقع قائمة لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

// --- شبكة (Meeting 22, DESIGN §16) ---

fn network_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اجلب", native("اجلب", network_fetch)),
        ("اخدم", native("اخدم", network_serve)),
    ]
}

fn network_fetch(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.اجلب", args, 1, line, col)?;
    let url = need_string("شبكة.اجلب", &args[0], line, col)?;
    crate::network::fetch(&url, line, col)
}

// --- خادم HTTPS (اجتماع 27، س1 ب) ---
//
// `شبكة.اخدم منفذ شهادة مفتاح معالج [مضيف]` — خادم HTTPS
// جاهز بمعالج واحد: المعالج دالة نيفا تأخذ قاموس طلب
// (طريقة، مسار، استعلام، ترويسات، جسم) وتعيد نصا أو
// قاموس استجابة (حالة، نص، ترويسات). الاستدعاء يحجب
// (يخدم حتى المقاطعة). المضيف الخامس اختياري: الافتراضي
// 127.0.0.1 (محلي فقط)، و0.0.0.0 للكشف فقط بمضيف صريح.
// المعاملات مفصولة بمسافات، بلا فواصل — ككل استدعاءات
// نيفا. الخادم مغلق افتراضيا: يحتاج --اسمح-بالخادم
// (علم الشبكة وحده لا يكفي) أو :اسمح-بالخادم في الكونسول.
fn network_serve(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    if args.len() != 4 && args.len() != 5 {
        return Err(RioError::typ(
            "الدالة شبكة.اخدم تتوقع 4 معاملات (منفذ شهادة مفتاح معالج) أو 5 (مع مضيف)",
            line,
            col,
        ));
    }
    let port = need_integer("شبكة.اخدم", &args[0], line, col)?;
    if !(1..=65535).contains(&port) {
        return Err(RioError::typ(
            "الدالة شبكة.اخدم تتوقع منفذا بين 1 و 65535",
            line,
            col,
        ));
    }
    let cert = need_string("شبكة.اخدم", &args[1], line, col)?;
    let key = need_string("شبكة.اخدم", &args[2], line, col)?;
    let handler = args[3].clone();
    if !matches!(handler, Value::Function { .. }) {
        return Err(RioError::typ(
            "الدالة شبكة.اخدم تتوقع معالجا دالة نيفا",
            line,
            col,
        ));
    }
    let host = if args.len() == 5 {
        need_string("شبكة.اخدم", &args[4], line, col)?
    } else {
        "127.0.0.1".to_string()
    };
    crate::network::serve(
        ctx,
        port as u16,
        &cert,
        &key,
        handler,
        &host,
        line,
        col,
    )
}

// --- عمليات (catalog task 10, from kernel/) ---

fn processes_entries() -> Vec<(&'static str, Value)> {
    // The stored key for أنشئ is its normalized spelling
    // "انشي": the lexer turns أنشئ into Keyword::Create, and
    // the parser names the member by keyword_spelling — so
    // both `عمليات.أنشئ` and `عمليات.انشي` reach this entry.
    // The other three are plain identifiers: the lexer keeps
    // their spelling exactly (hamza/ة preserved — only
    // diacritics are stripped), so the keys are the spellings
    // the user writes: أوقف، الحالية.
    vec![
        ("انشي", native("انشي", proc_spawn)),
        ("انتظر", native("انتظر", proc_wait)),
        ("أوقف", native("أوقف", proc_kill)),
        ("الحالية", native("الحالية", proc_current)),
    ]
}

fn proc_spawn(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.أنشئ", args, 1, line, col)?;
    let cmd = need_string("عمليات.أنشئ", &args[0], line, col)?;
    Ok(Value::Number(
        crate::processes::spawn_process(&cmd, line, col)? as f64,
    ))
}

fn proc_wait(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.انتظر", args, 1, line, col)?;
    let id = need_integer("عمليات.انتظر", &args[0], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            "الدالة عمليات.انتظر تتوقع معرف عملية غير سالب",
            line,
            col,
        ));
    }
    Ok(Value::Number(
        crate::processes::wait_process(id as u64, line, col)? as f64,
    ))
}

fn proc_kill(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.أوقف", args, 1, line, col)?;
    let id = need_integer("عمليات.أوقف", &args[0], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            "الدالة عمليات.أوقف تتوقع معرف عملية غير سالب",
            line,
            col,
        ));
    }
    crate::processes::kill_process(id as u64, line, col)?;
    Ok(Value::Nil)
}

fn proc_current(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.الحالية", args, 0, line, col)?;
    Ok(Value::Number(
        crate::processes::current_process_id(line, col)? as f64,
    ))
}

// --- ذاكرة (catalog task 11, from mm/) ---

fn memory_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("احجز", native("احجز", mem_alloc)),
        // المفتاح المخزَّن لـ حرّر هو صيغته المطبّعة "حرر":
        // الشدة تشكيل فيُجرَّد عند التحليل، فيصل الاسم بلا
        // شدة — كما في "انشي" للمهمة 10. ويعمل بذلك
        // `ذاكرة.حرّر` و`ذاكرة.حرر` معا.
        ("حرر", native("حرر", mem_free)),
        ("المتاح", native("المتاح", mem_available)),
        ("حجم_الصفحة", native("حجم_الصفحة", mem_page_size)),
    ]
}

fn mem_alloc(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ذاكرة.احجز", args, 1, line, col)?;
    let size = need_integer("ذاكرة.احجز", &args[0], line, col)?;
    Ok(Value::Number(
        crate::memory::allocate_block(size, line, col)? as f64,
    ))
}

fn mem_free(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ذاكرة.حرّر", args, 1, line, col)?;
    let id = need_integer("ذاكرة.حرّر", &args[0], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            "الدالة ذاكرة.حرّر تتوقع معرف كتلة غير سالب",
            line,
            col,
        ));
    }
    crate::memory::free_block(id as u64, line, col)?;
    Ok(Value::Nil)
}

fn mem_available(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ذاكرة.المتاح", args, 0, line, col)?;
    Ok(Value::Number(
        crate::memory::available_memory(line, col)? as f64,
    ))
}

fn mem_page_size(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ذاكرة.حجم_الصفحة", args, 0, line, col)?;
    Ok(Value::Number(
        crate::memory::page_size(line, col)? as f64,
    ))
}

// --- ملفات (catalog task 12, from fs/) ---

fn files_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("افتح", native("افتح", file_open)),
        ("اقرأ", native("اقرأ", file_read)),
        ("اكتب", native("اكتب", file_write)),
    ]
}

fn file_open(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ملفات.افتح", args, 1, line, col)?;
    let path = need_string("ملفات.افتح", &args[0], line, col)?;
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    Ok(Value::Number(
        crate::files::open_file(&policy, &cwd, &path, line, col)? as f64,
    ))
}

fn file_read(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ملفات.اقرأ", args, 1, line, col)?;
    let id = need_integer("ملفات.اقرأ", &args[0], line, col)?;
    Ok(Value::Str(crate::files::read_file(id, line, col)?))
}

fn file_write(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("ملفات.اكتب", args, 2, line, col)?;
    let id = need_integer("ملفات.اكتب", &args[0], line, col)?;
    let content = need_string("ملفات.اكتب", &args[1], line, col)?;
    let policy = ctx.server_file_policy();
    crate::files::write_file(&policy, id, &content, line, col)?;
    Ok(Value::Nil)
}

// --- وقت ---

fn time_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("الآن", native("الآن", time_now)),
        ("نوم", native("نوم", time_sleep)),
    ]
}

fn time_now(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("وقت.الآن", args, 0, line, col)?;
    // SystemTime traps on wasm32-unknown-unknown; the browser
    // clock comes from js-sys there.
    #[cfg(target_arch = "wasm32")]
    let secs = js_sys::Date::now() / 1000.0;
    #[cfg(not(target_arch = "wasm32"))]
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Ok(Value::Number(secs))
}

fn time_sleep(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("وقت.نوم", args, 1, line, col)?;
    let secs = need_number("وقت.نوم", &args[0], line, col)?;
    if secs < 0.0 {
        return Err(RioError::runtime("نوم مدة سالبة غير مسموح", line, col));
    }
    // Audit م-16: cap the sleep at 60 seconds, and refuse
    // non-finite values (NaN/inf would panic
    // Duration::from_secs_f64) — a huge sleep must never hang
    // a session with no way back.
    if !secs.is_finite() {
        return Err(RioError::runtime("نوم مدة غير صالحة", line, col));
    }
    if secs > 60.0 {
        return Err(RioError::runtime("نوم أكثر من 60 ثانية غير مسموح", line, col));
    }
    // Meeting 17 (WASM): thread::sleep blocks (and panics) on
    // single-threaded WASM — a clean error instead.
    #[cfg(target_arch = "wasm32")]
    {
        return Err(RioError::runtime("نوم غير مدعوم في متصفح الويب", line, col));
    }
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(Duration::from_secs_f64(secs));
    Ok(Value::Nil)
}
