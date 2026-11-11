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
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    /// تنفيذ نص نيفا كبرنامج معزول (J3-69) — الافتراضي
    /// يرفض؛ ينفذه المفسر وآلة البايت كود فقط، لا الثنائي
    /// الأصلي (بلا مفسر مضمّن). تعيد (النتيجة، المخرجات
    /// المطبوعة أثناء التنفيذ) كاستدعاء المعالج؛ ونتيجة
    /// التنفيذ الناجح نص المخرجات الملتقَطة نفسها.
    fn eval_text(&mut self, _text: &str, line: usize, col: usize) -> (RioResult<Value>, String) {
        (
            Err(RioError::runtime(
                "تنفيذ النص غير مدعوم في هذا المحرك",
                line,
                col,
            )),
            String::new(),
        )
    }
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
        // Catalog task 14 (نظام, from arch/*/entry/syscalls):
        // the syscall-name table for this architecture plus
        // read-only identity queries (kernel version, OS name,
        // uid, gid). Permission design first — see
        // CATALOG-06-SYSTEM.md §1 (read-only only: no raw
        // syscall is ever executed — no `نادِ`; the table is
        // data, the queries read /proc on Linux; everywhere
        // else they refuse honestly).
        "نظام" => system_entries(),
        // Catalog task 16 (تواصل, from ipc/): pipes between
        // two processes — open a channel to a subprogram,
        // send a line, receive a line, close (wait for the
        // exit code) or stop (kill). Permission design first
        // — see CATALOG-08-IPC.md §1 (the spawn policy is
        // عمليات.أنشئ's own policy through the shared
        // builder; numeric ids only; at most 64 live
        // channels; one message is one line of at most 64
        // KiB; WASM refuses with the flat builtins' own
        // message; Android refuses spawning like عمليات).
        "تواصل" => ipc_entries(),
        // Catalog task 17 (تشفير, from crypto/): hashing
        // only in v1 — SHA-256 in pure Rust (no crate) plus
        // system randomness. Permission design first — see
        // CATALOG-09-CRYPTO.md §1 (no encryption, no
        // signing — task 18; no key or secret ever passes
        // through the module; جزّئ returns the hex
        // fingerprint; عشوائي_آمن returns bytes from the
        // OS — /dev/urandom on Unix, honest refusal on
        // Windows and in the browser).
        "تشفير" => crypto_entries(),
        // Catalog task 18 (أمن, from security/): read-only
        // Linux security queries — the current process's
        // capability sets, the active LSMs, and an
        // access(2)-style permission check for a file
        // inside the program folder. Permission design
        // first — see CATALOG-10-SECURITY.md §1
        // (read-only only: no capability is ever dropped
        // — أسقط_قدرة is deferred; Linux + Android only,
        // honest refusal elsewhere; in the browser
        // refused with the flat builtins' own message).
        "أمن" => security_entries(),
        // Catalog task 19 (غير_متزامن, from io_uring/):
        // non-blocking I/O primitives over files and
        // sockets — submit a read, write, send, or receive
        // and collect its result later, without blocking
        // the program or the REPL. Permission design
        // first — see CATALOG-11-ASYNC.md §1 (one worker
        // thread per operation; file paths resolve through
        // the session FilePolicy at submit time; socket
        // operations check the network gate and clone the
        // socket at submit time; I/O failures arrive as
        // result dicts, never as raised errors; no
        // io_uring, no tokio runtime, std only; in the
        // browser refused with the flat builtins' own
        // message).
        "غير_متزامن" => async_entries(),
        // Catalog task 20 (جهاز.حوار, Dialog): the first
        // device capability (Stage 4) — an interactive
        // dialog that asks the user and returns the value.
        // Permission design first — see
        // CATALOG-12-DEVICE-DIALOG.md §1 (the engine only
        // requests over the stderr control channel and waits
        // for a structured stdin line; the app displays and
        // answers; Android only, honest refusal elsewhere;
        // the dialog itself is the permission gate — every
        // dialog has an explicit cancel button).
        "جهاز" => device_entries(),
        // J14 الأساسي (الدفعة الأولى من برنامج الـ579،
        // الاجتماع 73 §88): البنود 220 و221 و222 و223
        // و224 و213 — وحدات أصلية نقية بلا اعتماديات.
        "نمط" => regex_entries(),
        // دفعة لينكس الأولى، الموجة الأولى §1 (البنود
        // 1–8 و18–20 و23 و27 و28): بحث بنمط ونص ثابت
        // مع خيارات المطابقة والإخراج.
        "بحث" => search_entries(),
        "جسون" => json_entries(),
        "اكسمل" => xml_entries(),
        "يامال" => yaml_entries(),
        "قوالب" => template_entries(),
        "تحقق" => validate_entries(),
        // باقي J14 (الدفعة الثالثة من برنامج الـ579):
        // البنود 205–211 و214–217 — وحدات أصلية نقية.
        "نص" => text_entries(),
        "أسس" => bases_entries(),
        "وحدات" => units_entries(),
        "أرشيف" => archive_entries(),
        "كلمة_مرور" => password_entries(),
        "وهمي" => fake_entries(),
        "تقويم" => calendar_entries(),
        "ألوان" => colors_entries(),
        "إيموجي" => emoji_entries(),
        // J10 الأساسي (الدفعة الرابعة من برنامج الـ579،
        // الاجتماع 73 §88): البنود 157 و159–165 — وحدات
        // أصلية نقية بلا اعتماديات. 161 ربط فقط (حوّل_وحدات
        // مبنية في وحدة وحدات) فلا إدخال لها هنا.
        "قوائم" => lists_entries(),
        "علمي" => science_entries(),
        "تمويل" => finance_entries(),
        "اكتواري" => actuarial_entries(),
        "إحصاء" => stats_entries(),
        "توزيعات" => distrib_entries(),
        // J3 (الدفعة الخامسة والأخيرة من الدفعة الأولى،
        // الاجتماع 73 §88): البنود 56–60 و61+65 و63–64
        // و73–75 و77–78 و80 في وحدة دوال جديدة، و69 في
        // وحدة تنفيذ جديدة؛ و55 و70 ربط فقط فلا إدخال
        // لهما هنا؛ و71 مؤجلة بقرار موثق (انظر آخر الملف).
        "دوال" => funcs_entries(),
        "تنفيذ" => exec_entries(),
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
        // J1 (الدفعة الأولى من برنامج الـ579، الاجتماع 73 §88):
        // الأفعال الحسابية والمنطقية — البنود 2–11 و14 و19 و20 و22.
        ("جذر_نوني", native("جذر_نوني", math_nth_root)),
        ("باقي", native("باقي", math_rem)),
        ("عاملي", native("عاملي", math_factorial)),
        ("توافيق", native("توافيق", math_choose)),
        // أصغر/أكبر كلمتا مقارنة محجوزتان: المحلل يحوّل
        // أصغر إلى Keyword::Smaller ويسمّي العضو بعد النقطة
        // بالتهجئة المطبّعة "اصغر" (keyword_spelling) —
        // فالمفتاح المخزن هو المطبّع، كما "انشي" في عمليات؛
        // ويصل إليه رياضيات.أصغر ورياضيات.اصغر معا.
        ("اصغر", native("اصغر", math_min2)),
        ("اكبر", native("اكبر", math_max2)),
        ("زد", native("زد", math_inc)),
        ("نقص", native("نقص", math_dec)),
        ("نصف", native("نصف", math_half)),
        ("ضعف", native("ضعف", math_double)),
        ("قاسم_مشترك_أكبر", native("قاسم_مشترك_أكبر", math_gcd)),
        ("مضاعف_مشترك_أصغر", native("مضاعف_مشترك_أصغر", math_lcm)),
        ("فرق_المجموعات", native("فرق_المجموعات", math_set_diff)),
        ("تطابق", native("تطابق", math_match)),
        ("جيب_عكسي", native("جيب_عكسي", math_asin)),
        ("جيب_تمام_عكسي", native("جيب_تمام_عكسي", math_acos)),
        ("ظل_عكسي", native("ظل_عكسي", math_atan)),
        ("قطبي", native("قطبي", math_polar)),
        ("ديكارتي", native("ديكارتي", math_cartesian)),
        ("أولي", native("أولي", math_is_prime)),
        ("حلل_إلى_عوامل", native("حلل_إلى_عوامل", math_factorize)),
        ("إشارة", native("إشارة", math_sign)),
        ("مقلوب", native("مقلوب", math_recip)),
        ("مربع", native("مربع", math_square)),
        // J10-157: الحساب النمطي — باقي وأس ومعكوس
        // وجمع وضرب وطرح بباقي قسمة محدد (موجب).
        ("باقي_نمطي", native("باقي_نمطي", math_mod_rem)),
        ("أس_نمطي", native("أس_نمطي", math_mod_pow)),
        ("معكوس_نمطي", native("معكوس_نمطي", math_mod_inv)),
        ("جمع_نمطي", native("جمع_نمطي", math_mod_add)),
        ("ضرب_نمطي", native("ضرب_نمطي", math_mod_mul)),
        ("طرح_نمطي", native("طرح_نمطي", math_mod_sub)),
// == مولّد-دفعة2-مداخل-رياضيات: بداية ==
        ("قسمة_أرضية", native("قسمة_أرضية", math_floor_div)),
        ("قسمة_ارضية", native("قسمة_ارضية", math_floor_div)),
        ("بت_او", native("بت_او", math_bit_or)),
        ("بت_و", native("بت_و", math_bit_and)),
        ("بت_عكس_حصري", native("بت_عكس_حصري", math_bit_xor)),
        ("بت_ازاحة_يسار", native("بت_ازاحة_يسار", math_bit_shl)),
        ("بت_ازاحة_يمين", native("بت_ازاحة_يمين", math_bit_shr)),
        ("عكس_بت", native("عكس_بت", math_bit_not)),
// == مولّد-دفعة2-مداخل-رياضيات: نهاية ==
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

// --- J1: الأفعال الحسابية والمنطقية (البنود 2–11 و14 و19 و20 و22) ---

fn math_nth_root(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جذر_نوني", args, 2, line, col)?;
    let x = need_number("رياضيات.جذر_نوني", &args[0], line, col)?;
    let n = need_integer("رياضيات.جذر_نوني", &args[1], line, col)?;
    if n == 0 {
        return Err(RioError::runtime("جذر من الدرجة صفر غير معرف", line, col));
    }
    if x < 0.0 && n % 2 == 0 {
        return Err(RioError::runtime("جذر من درجة زوجية لعدد سالب غير معرف", line, col));
    }
    let inv = 1.0 / n as f64;
    if x < 0.0 {
        Ok(Value::Number(-(-x).powf(inv)))
    } else {
        Ok(Value::Number(x.powf(inv)))
    }
}

fn math_rem(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.باقي", args, 2, line, col)?;
    let a = need_number("رياضيات.باقي", &args[0], line, col)?;
    let b = need_number("رياضيات.باقي", &args[1], line, col)?;
    if b == 0.0 {
        return Err(RioError::runtime("باقي القسمة على صفر غير معرف", line, col));
    }
    Ok(Value::Number(a % b))
}

// == مولّد-دفعة2-دوال-البت: بداية ==
/// اقتطاع عدد نيفا (f64) نحو الصفر إلى i64 لعمليات البت
/// (الدفعة 2): الكسر يُهمل؛ والقيم خارج المدى تتشبّع
/// (NaN إلى 0 واللانهاية إلى الحد) كما يفعل التحويل
/// `as` في Rust.
fn trunc_i64(x: f64) -> i64 {
    x as i64
}

/// القسمة الأرضية (الدفعة 2): floor(أ / ب) على f64؛ وب = 0 خطأ «قسمة على صفر» كـDiv.
fn math_floor_div(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.قسمة_أرضية", args, 2, line, col)?;
    let a = need_number("رياضيات.قسمة_أرضية", &args[0], line, col)?;
    let b = need_number("رياضيات.قسمة_أرضية", &args[1], line, col)?;
    if b == 0.0 {
        return Err(RioError::runtime("قسمة على صفر", line, col));
    }
    Ok(Value::Number((a / b).floor()))
}

/// عملية بت ثنائية (الدفعة 2): (أ as i64) | (ب as i64) بعد اقتطاع نحو الصفر.
fn math_bit_or(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.بت_او", args, 2, line, col)?;
    let a = trunc_i64(need_number("رياضيات.بت_او", &args[0], line, col)?);
    let b = trunc_i64(need_number("رياضيات.بت_او", &args[1], line, col)?);
    Ok(Value::Number((a | b) as f64))
}

/// عملية بت ثنائية (الدفعة 2): (أ as i64) & (ب as i64) بعد اقتطاع نحو الصفر.
fn math_bit_and(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.بت_و", args, 2, line, col)?;
    let a = trunc_i64(need_number("رياضيات.بت_و", &args[0], line, col)?);
    let b = trunc_i64(need_number("رياضيات.بت_و", &args[1], line, col)?);
    Ok(Value::Number((a & b) as f64))
}

/// عملية بت ثنائية (الدفعة 2): (أ as i64) ^ (ب as i64) بعد اقتطاع نحو الصفر.
fn math_bit_xor(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.بت_عكس_حصري", args, 2, line, col)?;
    let a = trunc_i64(need_number("رياضيات.بت_عكس_حصري", &args[0], line, col)?);
    let b = trunc_i64(need_number("رياضيات.بت_عكس_حصري", &args[1], line, col)?);
    Ok(Value::Number((a ^ b) as f64))
}

/// إزاحة البتات (الدفعة 2): (أ as i64) << (ب as i64)؛ مقدار الإزاحة 0..63 وإلا خطأ.
fn math_bit_shl(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.بت_ازاحة_يسار", args, 2, line, col)?;
    let a = trunc_i64(need_number("رياضيات.بت_ازاحة_يسار", &args[0], line, col)?);
    let n = trunc_i64(need_number("رياضيات.بت_ازاحة_يسار", &args[1], line, col)?);
    if !(0..64).contains(&n) {
        return Err(RioError::runtime("مقدار الإزاحة يجب أن يكون بين 0 و 63", line, col));
    }
    Ok(Value::Number((a << n) as f64))
}

/// إزاحة البتات (الدفعة 2): (أ as i64) >> (ب as i64) إزاحة حسابية؛ مقدار الإزاحة 0..63 وإلا خطأ.
fn math_bit_shr(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.بت_ازاحة_يمين", args, 2, line, col)?;
    let a = trunc_i64(need_number("رياضيات.بت_ازاحة_يمين", &args[0], line, col)?);
    let n = trunc_i64(need_number("رياضيات.بت_ازاحة_يمين", &args[1], line, col)?);
    if !(0..64).contains(&n) {
        return Err(RioError::runtime("مقدار الإزاحة يجب أن يكون بين 0 و 63", line, col));
    }
    Ok(Value::Number((a >> n) as f64))
}

/// عكس البت الأحادي `~` (الدفعة 2): ~(أ as i64) بعد اقتطاع نحو الصفر.
fn math_bit_not(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.عكس_بت", args, 1, line, col)?;
    let a = trunc_i64(need_number("رياضيات.عكس_بت", &args[0], line, col)?);
    Ok(Value::Number((!a) as f64))
}
// == مولّد-دفعة2-دوال-البت: نهاية ==
fn math_factorial(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.عاملي", args, 1, line, col)?;
    let n = need_integer("رياضيات.عاملي", &args[0], line, col)?;
    if n < 0 {
        return Err(RioError::runtime("عاملي عدد سالب غير معرف", line, col));
    }
    if n > 170 {
        return Err(RioError::runtime("عاملي عدد أكبر من 170 غير معرف", line, col));
    }
    let mut result = 1.0f64;
    for i in 2..=n {
        result *= i as f64;
    }
    Ok(Value::Number(result))
}

fn math_choose(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.توافيق", args, 2, line, col)?;
    let n = need_integer("رياضيات.توافيق", &args[0], line, col)?;
    let k = need_integer("رياضيات.توافيق", &args[1], line, col)?;
    if n < 0 || k < 0 || k > n {
        return Err(RioError::runtime("توافيق تتطلب عددين صحيحين حيث 0 ≤ ك ≤ ن", line, col));
    }
    let k = k.min(n - k);
    if k > 1000 {
        return Err(RioError::runtime("توافيق بعدد كبير غير مدعومة", line, col));
    }
    let mut result = 1.0f64;
    for i in 1..=k {
        result = result * (n - k + i) as f64 / i as f64;
    }
    Ok(Value::Number(result))
}

fn math_min2(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.اصغر", args, 2, line, col)?;
    let a = need_number("رياضيات.اصغر", &args[0], line, col)?;
    let b = need_number("رياضيات.اصغر", &args[1], line, col)?;
    Ok(Value::Number(a.min(b)))
}

fn math_max2(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.اكبر", args, 2, line, col)?;
    let a = need_number("رياضيات.اكبر", &args[0], line, col)?;
    let b = need_number("رياضيات.اكبر", &args[1], line, col)?;
    Ok(Value::Number(a.max(b)))
}

fn math_inc(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.زد", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.زد", &args[0], line, col)? + 1.0))
}

fn math_dec(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.نقص", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.نقص", &args[0], line, col)? - 1.0))
}

fn math_half(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.نصف", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.نصف", &args[0], line, col)? / 2.0))
}

fn math_double(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ضعف", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.ضعف", &args[0], line, col)? * 2.0))
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn math_gcd(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.قاسم_مشترك_أكبر", args, 2, line, col)?;
    let a = need_integer("رياضيات.قاسم_مشترك_أكبر", &args[0], line, col)?;
    let b = need_integer("رياضيات.قاسم_مشترك_أكبر", &args[1], line, col)?;
    let g = gcd_u64(a.unsigned_abs(), b.unsigned_abs());
    if g == 0 {
        return Err(RioError::runtime("القاسم المشترك الأكبر لصفر وصفر غير معرف", line, col));
    }
    Ok(Value::Number(g as f64))
}

fn math_lcm(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.مضاعف_مشترك_أصغر", args, 2, line, col)?;
    let a = need_integer("رياضيات.مضاعف_مشترك_أصغر", &args[0], line, col)?;
    let b = need_integer("رياضيات.مضاعف_مشترك_أصغر", &args[1], line, col)?;
    let (ua, ub) = (a.unsigned_abs(), b.unsigned_abs());
    if ua == 0 || ub == 0 {
        return Ok(Value::Number(0.0));
    }
    let g = gcd_u64(ua, ub);
    Ok(Value::Number(ua as f64 / g as f64 * ub as f64))
}

fn math_set_diff(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.فرق_المجموعات", args, 2, line, col)?;
    let (a, b) = match (&args[0], &args[1]) {
        (Value::List(a), Value::List(b)) => (a, b),
        _ => {
            return Err(RioError::typ(
                "الدالة رياضيات.فرق_المجموعات تتوقع قائمتين".to_string(),
                line,
                col,
            ))
        }
    };
    let out: Vec<Value> = a
        .iter()
        .filter(|item| !b.iter().any(|other| *item == other))
        .cloned()
        .collect();
    Ok(Value::List(out))
}

fn math_match(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.تطابق", args, 2, line, col)?;
    Ok(Value::Bool(args[0] == args[1]))
}

fn math_asin(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب_عكسي", args, 1, line, col)?;
    let x = need_number("رياضيات.جيب_عكسي", &args[0], line, col)?;
    if x < -1.0 || x > 1.0 {
        return Err(RioError::runtime("جيب عكسي لعدد خارج [-1، 1] غير معرف", line, col));
    }
    Ok(Value::Number(x.asin()))
}

fn math_acos(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب_تمام_عكسي", args, 1, line, col)?;
    let x = need_number("رياضيات.جيب_تمام_عكسي", &args[0], line, col)?;
    if x < -1.0 || x > 1.0 {
        return Err(RioError::runtime("جيب تمام عكسي لعدد خارج [-1، 1] غير معرف", line, col));
    }
    Ok(Value::Number(x.acos()))
}

fn math_atan(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ظل_عكسي", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.ظل_عكسي", &args[0], line, col)?.atan()))
}

fn math_polar(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.قطبي", args, 2, line, col)?;
    let x = need_number("رياضيات.قطبي", &args[0], line, col)?;
    let y = need_number("رياضيات.قطبي", &args[1], line, col)?;
    Ok(Value::List(vec![
        Value::Number(x.hypot(y)),
        Value::Number(y.atan2(x)),
    ]))
}

fn math_cartesian(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ديكارتي", args, 2, line, col)?;
    let r = need_number("رياضيات.ديكارتي", &args[0], line, col)?;
    let theta = need_number("رياضيات.ديكارتي", &args[1], line, col)?;
    Ok(Value::List(vec![
        Value::Number(r * theta.cos()),
        Value::Number(r * theta.sin()),
    ]))
}

fn is_prime_i64(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 {
        return true;
    }
    if n % 2 == 0 {
        return false;
    }
    let mut i = 3i64;
    while i <= n / i {
        if n % i == 0 {
            return false;
        }
        i += 2;
    }
    true
}

fn math_is_prime(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أولي", args, 1, line, col)?;
    let n = need_integer("رياضيات.أولي", &args[0], line, col)?;
    Ok(Value::Bool(is_prime_i64(n)))
}

fn math_factorize(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.حلل_إلى_عوامل", args, 1, line, col)?;
    let mut n = need_integer("رياضيات.حلل_إلى_عوامل", &args[0], line, col)?;
    if n < 2 {
        return Err(RioError::runtime("تحليل عدد أصغر من 2 غير معرف", line, col));
    }
    let mut out: Vec<Value> = Vec::new();
    let mut d = 2i64;
    while d <= n / d {
        while n % d == 0 {
            out.push(Value::Number(d as f64));
            n /= d;
        }
        d += if d == 2 { 1 } else { 2 };
    }
    if n > 1 {
        out.push(Value::Number(n as f64));
    }
    Ok(Value::List(out))
}

fn math_sign(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.إشارة", args, 1, line, col)?;
    let x = need_number("رياضيات.إشارة", &args[0], line, col)?;
    let s = if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    };
    Ok(Value::Number(s))
}

fn math_recip(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.مقلوب", args, 1, line, col)?;
    let x = need_number("رياضيات.مقلوب", &args[0], line, col)?;
    if x == 0.0 {
        return Err(RioError::runtime("مقلوب صفر غير معرف", line, col));
    }
    Ok(Value::Number(1.0 / x))
}

fn math_square(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.مربع", args, 1, line, col)?;
    let x = need_number("رياضيات.مربع", &args[0], line, col)?;
    Ok(Value::Number(x * x))
}

// ---------- 157: الحساب النمطي ----------
// كل المعاملات أعداد صحيحة، والمقسوم عليه عدد
// صحيح موجب. الضرب النمطي بطريقة المضاعفة الثنائية
// (جمع متكرر بباقي القسمة) فلا فيض أبدا — كل قيمة
// وسيطة دون ضعف المقسوم عليه (دون 9e15)؛ والمعكوس
// بأقليدس الموسع مع اختزال معاملَيْه بباقي القسمة
// كل خطوة فتبقى كل قيمة وسيطة دون المقسوم عليه.
// المرآة C في niva_std.c تحسب بالطريقة نفسها
// حرفيا ليتطابق الناتج بتّا بتّا.

/// ضرب نمطي آمن من الفيض: (a*b) % m بالمضاعفة.
fn mod_mul_u64(a: u64, b: u64, m: u64) -> u64 {
    let mut result = 0u64;
    let mut x = a % m;
    let mut y = b;
    while y > 0 {
        if y & 1 == 1 {
            result = (result + x) % m;
        }
        x = (x * 2) % m;
        y >>= 1;
    }
    result
}

/// أس نمطي: (base^exp) % m بالتربيع المتكرر.
fn mod_pow_u64(mut base: u64, mut exp: u64, m: u64) -> u64 {
    let mut result = 1u64 % m;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            result = mod_mul_u64(result, base, m);
        }
        base = mod_mul_u64(base, base, m);
        exp >>= 1;
    }
    result
}

/// معكوس نمطي: x حيث (a*x) % m == 1، أو لاشيء
/// إن لم يوجد (القاسم المشترك الأكبر ليس 1).
fn mod_inv_u64(a: u64, m: u64) -> Option<u64> {
    let mut old_r = a as i64;
    let mut r = m as i64;
    let mut old_s: i64 = 1;
    let mut s: i64 = 0;
    let mi = m as i64;
    while r != 0 {
        let q = old_r / r;
        let next_r = old_r - q * r;
        let qs = mod_mul_u64((q % mi) as u64, s as u64, m) as i64;
        let mut next_s = old_s - qs;
        if next_s < 0 {
            next_s += mi;
        }
        old_r = r;
        r = next_r;
        old_s = s;
        s = next_s;
    }
    if old_r != 1 {
        None
    } else {
        Some(old_s as u64)
    }
}

/// تطبيع عدد صحيح إلى [0، m): باقي قسمة غير سالب.
fn mod_norm(a: i64, m: i64) -> i64 {
    let r = a % m;
    if r < 0 {
        r + m
    } else {
        r
    }
}

fn math_mod_rem(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.باقي_نمطي", args, 2, line, col)?;
    let a = need_integer("رياضيات.باقي_نمطي", &args[0], line, col)?;
    let m = need_integer("رياضيات.باقي_نمطي", &args[1], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    Ok(Value::Number(mod_norm(a, m) as f64))
}

fn math_mod_pow(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أس_نمطي", args, 3, line, col)?;
    let base = need_integer("رياضيات.أس_نمطي", &args[0], line, col)?;
    let exp = need_integer("رياضيات.أس_نمطي", &args[1], line, col)?;
    let m = need_integer("رياضيات.أس_نمطي", &args[2], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    if exp < 0 {
        return Err(RioError::runtime("الأس النمطي يجب أن يكون عددا صحيحا غير سالب", line, col));
    }
    let b = mod_norm(base, m) as u64;
    Ok(Value::Number(mod_pow_u64(b, exp as u64, m as u64) as f64))
}

fn math_mod_inv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.معكوس_نمطي", args, 2, line, col)?;
    let a = need_integer("رياضيات.معكوس_نمطي", &args[0], line, col)?;
    let m = need_integer("رياضيات.معكوس_نمطي", &args[1], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    match mod_inv_u64(mod_norm(a, m) as u64, m as u64) {
        Some(x) => Ok(Value::Number(x as f64)),
        None => Err(RioError::runtime("لا معكوس نمطي لهذا العدد بهذا المقسوم عليه", line, col)),
    }
}

fn math_mod_add(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جمع_نمطي", args, 3, line, col)?;
    let a = need_integer("رياضيات.جمع_نمطي", &args[0], line, col)?;
    let b = need_integer("رياضيات.جمع_نمطي", &args[1], line, col)?;
    let m = need_integer("رياضيات.جمع_نمطي", &args[2], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    let r = (mod_norm(a, m) + mod_norm(b, m)) % m;
    Ok(Value::Number(r as f64))
}

fn math_mod_mul(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ضرب_نمطي", args, 3, line, col)?;
    let a = need_integer("رياضيات.ضرب_نمطي", &args[0], line, col)?;
    let b = need_integer("رياضيات.ضرب_نمطي", &args[1], line, col)?;
    let m = need_integer("رياضيات.ضرب_نمطي", &args[2], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    let r = mod_mul_u64(mod_norm(a, m) as u64, mod_norm(b, m) as u64, m as u64);
    Ok(Value::Number(r as f64))
}

fn math_mod_sub(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.طرح_نمطي", args, 3, line, col)?;
    let a = need_integer("رياضيات.طرح_نمطي", &args[0], line, col)?;
    let b = need_integer("رياضيات.طرح_نمطي", &args[1], line, col)?;
    let m = need_integer("رياضيات.طرح_نمطي", &args[2], line, col)?;
    if m <= 0 {
        return Err(RioError::runtime(
            "المقسوم عليه في الحساب النمطي يجب أن يكون عددا صحيحا موجبا",
            line,
            col,
        ));
    }
    let r = mod_norm(mod_norm(a, m) - mod_norm(b, m), m);
    Ok(Value::Number(r as f64))
}

// --- J14 الأساسي (الدفعة الأولى من برنامج الـ579، الاجتماع 73 §88):
// البنود 220 و221 و222 و223 و224 و213 — بلا اعتماديات جديدة، Rust نقي.
// رسائل الأخطاء ثابتة الصياغة عمدا (بلا تفاصيل موضعية) ليتطابق
// نصها حرفيا بين المفسر وآلة البايت كود والثنائي الأصلي.

// ---------- 220: التعابير النمطية ----------
// محرك Thompson NFA (محاكاة Pike) من الصفر — بلا تراجع، فلا
// تراجع كارثي أبدا. الحدود: النمط 1000 محرف، والنص مليون
// محرف، والبرنامج المترجم 10000 تعليمة، وخطوات المحاكاة
// مليون خطوة. الصياغة المدعومة: محارف عادية، ونقطة (أي
// محرف عدا سطر جديد)، وفئات [a-z] و[^...]، وهروب \d \D
// و\w \W و\s \S و\n و\r و\t و\b و\B، ومجموعات ملتقطة (...)
// وغير ملتقطة (?:...)، وتناوب |، وكمّيات * + ? {n} {n,}
// {n,m} مع نسخ كسولة ...؟، ومرساتا ^ و$ (بداية النص
// ونهايته فقط، بلا وضع متعدد الأسطر). الكمّية {n,m} على
// مجموعة ملتقطة تعيد استعمال خانات الالتقاط نفسها (فآخر
// تكرار هو الملتقَط)؛ و{ غير الصالحة نمط غير صالح؛
// و\0 مرفوض (نمط غير صالح) ليتطابق السلوك مع الخلفية C.

const RX_MAX_PATTERN: usize = 1000;
const RX_MAX_TEXT: usize = 1_000_000;
const RX_MAX_PROG: usize = 10_000;
const RX_MAX_STEPS: usize = 1_000_000;
const RX_MAX_GROUPS: usize = 32;
const RX_MAX_REPEAT: usize = 1000;

#[derive(Clone, Debug)]
enum RxInst {
    Chr(char),
    Any,
    Cls { ranges: Vec<(char, char)>, neg: bool },
    Digit(bool),
    Word(bool),
    Space(bool),
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Bol,
    Eol,
    Wb(bool),
    Match,
}

/// محرف كلمة: لاتيني رقمي أو _ أو عربي (النطاقات نفسها
/// في الخلفية C حرفيا).
fn rx_is_word(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || ('\u{0600}'..='\u{06FF}').contains(&c)
        || ('\u{0750}'..='\u{077F}').contains(&c)
        || ('\u{08A0}'..='\u{08FF}').contains(&c)
        || ('\u{FB50}'..='\u{FDFF}').contains(&c)
        || ('\u{FE70}'..='\u{FEFF}').contains(&c)
}

fn rx_is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}')
}

fn rx_class_matches(ranges: &[(char, char)], neg: bool, c: char) -> bool {
    let inside = ranges.iter().any(|(a, b)| *a <= c && c <= *b);
    inside != neg
}

/// هل تستهلك التعليمة محرفا `c` (أي تتقدم)؟
/// مع `fold`: طيّ الحالة (تجاهل الحالة) — يطابق
/// المحرف الحرفي والفئات بعد طيّ الطرفين إلى الصغير.
fn rx_fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn rx_consumes(inst: &RxInst, c: char, fold: bool) -> bool {
    match inst {
        RxInst::Chr(x) => {
            if fold {
                rx_fold(*x) == rx_fold(c)
            } else {
                *x == c
            }
        }
        RxInst::Any => c != '\n',
        RxInst::Cls { ranges, neg } => {
            if fold {
                let fc = rx_fold(c);
                let inside = ranges
                    .iter()
                    .any(|(a, b)| rx_fold(*a) <= fc && fc <= rx_fold(*b));
                inside != *neg
            } else {
                rx_class_matches(ranges, *neg, c)
            }
        }
        RxInst::Digit(neg) => c.is_ascii_digit() != *neg,
        RxInst::Word(neg) => rx_is_word(c) != *neg,
        RxInst::Space(neg) => rx_is_space(c) != *neg,
        _ => false,
    }
}

struct RxProg {
    prog: Vec<RxInst>,
    ngroups: usize,
    fold: bool,
}

#[derive(Clone)]
struct RxFrag {
    start: usize,
    // نهاية تعليمات القطعة (حصرية): النسخ في {n,m}
    // تنسخ [start, end) فقط — لا ما بُني بعدها.
    end: usize,
    outs: Vec<(usize, u8)>,
}

struct RxCompiler {
    chars: Vec<char>,
    pos: usize,
    prog: Vec<RxInst>,
    ngroups: usize,
}

impl RxCompiler {
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

    fn emit(&mut self, inst: RxInst) -> Result<usize, String> {
        if self.prog.len() >= RX_MAX_PROG {
            return Err("النمط كبير جدا".to_string());
        }
        self.prog.push(inst);
        Ok(self.prog.len() - 1)
    }

    fn patch(&mut self, outs: &[(usize, u8)], target: usize) {
        for (idx, slot) in outs {
            match &mut self.prog[*idx] {
                RxInst::Split(a, b) => {
                    if *slot == 0 {
                        *a = target;
                    } else {
                        *b = target;
                    }
                }
                RxInst::Jmp(t) => *t = target,
                _ => {}
            }
        }
    }

    /// قطعة من تعليمة واحدة تنتهي بقفزة معلّقة.
    fn frag_one(&mut self, inst: RxInst) -> Result<RxFrag, String> {
        let start = self.emit(inst)?;
        let jmp = self.emit(RxInst::Jmp(usize::MAX))?;
        Ok(RxFrag {
            start,
            end: self.prog.len(),
            outs: vec![(jmp, 0)],
        })
    }

    fn chain(&mut self, a: RxFrag, b: RxFrag) -> RxFrag {
        self.patch(&a.outs, b.start);
        RxFrag {
            start: a.start,
            end: b.end,
            outs: b.outs,
        }
    }

    fn seq(&mut self, frags: &[RxFrag]) -> Result<RxFrag, String> {
        if frags.is_empty() {
            let jmp = self.emit(RxInst::Jmp(usize::MAX))?;
            return Ok(RxFrag {
                start: jmp,
                end: self.prog.len(),
                outs: vec![(jmp, 0)],
            });
        }
        let mut cur = frags[0].clone();
        for f in &frags[1..] {
            cur = self.chain(cur, f.clone());
        }
        Ok(cur)
    }

    /// نجمة على قطعة (جشعة أو كسولة).
    fn star(&mut self, body: RxFrag, lazy: bool) -> Result<RxFrag, String> {
        let s = if lazy {
            self.emit(RxInst::Split(usize::MAX, body.start))?
        } else {
            self.emit(RxInst::Split(body.start, usize::MAX))?
        };
        self.patch(&body.outs, s);
        Ok(RxFrag {
            start: s,
            end: self.prog.len(),
            outs: vec![(s, if lazy { 0 } else { 1 })],
        })
    }

    /// علامة استفهام على قطعة (جشعة أو كسولة).
    fn opt(&mut self, body: RxFrag, lazy: bool) -> Result<RxFrag, String> {
        let s = if lazy {
            self.emit(RxInst::Split(usize::MAX, body.start))?
        } else {
            self.emit(RxInst::Split(body.start, usize::MAX))?
        };
        let mut outs = body.outs.clone();
        outs.push((s, if lazy { 0 } else { 1 }));
        Ok(RxFrag {
            start: s,
            end: self.prog.len(),
            outs,
        })
    }

    /// انسخ تعليمات القطعة [start, end) إلى نهاية
    /// البرنامج مع إزاحة الأهداف الداخلية؛ خانات
    /// الالتقاط نفسها تُعاد.
    /// (القفزة الختامية للنسخة تُرقَّع لاحقا عند ربطها،
    /// كما في الذرة الأصلية — فلا يضر أن هدفها المؤقت
    /// يشير إلى نسخة سابقة.)
    fn duplicate_frag(&mut self, frag: &RxFrag) -> Result<RxFrag, String> {
        let start = frag.start;
        let end = frag.end;
        let base = self.prog.len();
        for i in start..end {
            let inst = self.prog[i].clone();
            self.emit(inst)?;
        }
        for i in base..base + (end - start) {
            let inst = self.prog[i].clone();
            self.prog[i] = match inst {
                RxInst::Split(a, b) => RxInst::Split(
                    if (start..end).contains(&a) {
                        a - start + base
                    } else {
                        a
                    },
                    if (start..end).contains(&b) {
                        b - start + base
                    } else {
                        b
                    },
                ),
                RxInst::Jmp(t) => RxInst::Jmp(if (start..end).contains(&t) {
                    t - start + base
                } else {
                    t
                }),
                other => other,
            };
        }
        let outs: Vec<(usize, u8)> = frag
            .outs
            .iter()
            .map(|(idx, slot)| (idx - start + base, *slot))
            .collect();
        Ok(RxFrag {
            start: base,
            end: base + (end - start),
            outs,
        })
    }

    fn parse_alt(&mut self) -> Result<RxFrag, String> {
        let left = self.parse_concat()?;
        if self.peek() != Some('|') {
            return Ok(left);
        }
        self.pos += 1;
        let right = self.parse_alt()?;
        let s = self.emit(RxInst::Split(left.start, right.start))?;
        let mut outs = left.outs.clone();
        outs.extend(right.outs.iter().copied());
        Ok(RxFrag {
            start: s,
            end: self.prog.len(),
            outs,
        })
    }

    fn parse_concat(&mut self) -> Result<RxFrag, String> {
        let mut frags: Vec<RxFrag> = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            frags.push(self.parse_repeat()?);
        }
        self.seq(&frags)
    }

    fn parse_repeat(&mut self) -> Result<RxFrag, String> {
        let mut atom = self.parse_atom()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    let lazy = self.eat_lazy();
                    atom = self.star(atom, lazy)?;
                }
                Some('+') => {
                    self.pos += 1;
                    let lazy = self.eat_lazy();
                    // س+ = س ثم س*: ابدأ من الذرة نفسها.
                    let s = if lazy {
                        self.emit(RxInst::Split(usize::MAX, atom.start))?
                    } else {
                        self.emit(RxInst::Split(atom.start, usize::MAX))?
                    };
                    self.patch(&atom.outs, s);
                    atom = RxFrag {
                        start: atom.start,
                        end: self.prog.len(),
                        outs: vec![(s, if lazy { 0 } else { 1 })],
                    };
                }
                Some('?') => {
                    self.pos += 1;
                    let lazy = self.eat_lazy();
                    atom = self.opt(atom, lazy)?;
                }
                Some('{') => {
                    atom = self.parse_braces(atom)?;
                }
                _ => break,
            }
        }
        Ok(atom)
    }

    /// `?` بعد كمّية تعني النسخة الكسولة.
    fn eat_lazy(&mut self) -> bool {
        if self.peek() == Some('?') {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_braces(&mut self, atom: RxFrag) -> Result<RxFrag, String> {
        // الموضع عند '{'.
        self.pos += 1;
        let mut n_text = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                n_text.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if n_text.is_empty() {
            return Err("نمط غير صالح".to_string());
        }
        let n: usize = n_text.parse().map_err(|_| "نمط غير صالح".to_string())?;
        let mut m: Option<usize> = Some(n);
        if self.peek() == Some(',') {
            self.pos += 1;
            if self.peek() == Some('}') {
                m = None;
            } else {
                let mut m_text = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        m_text.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if m_text.is_empty() {
                    return Err("نمط غير صالح".to_string());
                }
                m = Some(m_text.parse().map_err(|_| "نمط غير صالح".to_string())?);
            }
        }
        if self.peek() != Some('}') {
            return Err("نمط غير صالح".to_string());
        }
        self.pos += 1;
        if n > RX_MAX_REPEAT || m.map(|x| x > RX_MAX_REPEAT).unwrap_or(false) {
            return Err("النمط كبير جدا".to_string());
        }
        if let Some(mv) = m {
            if mv < n {
                return Err("نمط غير صالح".to_string());
            }
        }
        let lazy = self.eat_lazy();
        if n == 0 && m == Some(0) {
            let jmp = self.emit(RxInst::Jmp(usize::MAX))?;
            return Ok(RxFrag {
                start: jmp,
                end: self.prog.len(),
                outs: vec![(jmp, 0)],
            });
        }
        // النسخة الأولى هي الذرة نفسها؛ والباقي نسخ منها.
        let mut result: Option<RxFrag> = None;
        for i in 0..n {
            let piece = if i == 0 {
                atom.clone()
            } else {
                self.duplicate_frag(&atom)?
            };
            result = Some(match result {
                None => piece,
                Some(prev) => self.chain(prev, piece),
            });
        }
        match m {
            None => {
                let extra = self.duplicate_frag(&atom)?;
                let starred = self.star(extra, lazy)?;
                result = Some(match result {
                    None => starred,
                    Some(prev) => self.chain(prev, starred),
                });
            }
            Some(mv) => {
                for _ in n..mv {
                    let extra = self.duplicate_frag(&atom)?;
                    let optional = self.opt(extra, lazy)?;
                    result = Some(match result {
                        None => optional,
                        Some(prev) => self.chain(prev, optional),
                    });
                }
            }
        }
        Ok(result.unwrap())
    }

    fn parse_atom(&mut self) -> Result<RxFrag, String> {
        match self.peek() {
            None => Err("نمط غير صالح".to_string()),
            Some('(') => {
                self.pos += 1;
                let capturing =
                    if self.peek() == Some('?') && self.chars.get(self.pos + 1) == Some(&':') {
                        self.pos += 2;
                        false
                    } else {
                        true
                    };
                let group = if capturing {
                    self.ngroups += 1;
                    if self.ngroups > RX_MAX_GROUPS {
                        return Err("النمط كبير جدا".to_string());
                    }
                    self.ngroups
                } else {
                    0
                };
                // حفظ البداية يُبث قبل المحتوى (لا بعده):
                // فبداية القطعة هي حفظ البداية نفسه.
                let save1 = if capturing {
                    Some(self.frag_one(RxInst::Save(2 * group))?)
                } else {
                    None
                };
                let inner = self.parse_alt()?;
                if self.next() != Some(')') {
                    return Err("نمط غير صالح".to_string());
                }
                if !capturing {
                    return Ok(inner);
                }
                let save1 = save1.unwrap();
                let save2 = self.frag_one(RxInst::Save(2 * group + 1))?;
                let chained = self.chain(save1, inner);
                Ok(self.chain(chained, save2))
            }
            Some('[') => {
                let inst = self.parse_class()?;
                self.frag_one(inst)
            }
            Some('.') => {
                self.pos += 1;
                self.frag_one(RxInst::Any)
            }
            Some('^') => {
                self.pos += 1;
                self.frag_one(RxInst::Bol)
            }
            Some('$') => {
                self.pos += 1;
                self.frag_one(RxInst::Eol)
            }
            Some('\\') => {
                self.pos += 1;
                let inst = self.parse_escape(false)?;
                self.frag_one(inst)
            }
            Some(')') | Some('|') => Err("نمط غير صالح".to_string()),
            Some(c) => {
                self.pos += 1;
                self.frag_one(RxInst::Chr(c))
            }
        }
    }

    /// هروب خارج فئة: يعيد التعليمة المقابلة.
    fn parse_escape(&mut self, in_class: bool) -> Result<RxInst, String> {
        let c = self.next().ok_or_else(|| "نمط غير صالح".to_string())?;
        match c {
            'd' => Ok(RxInst::Digit(false)),
            'D' => Ok(RxInst::Digit(true)),
            'w' => Ok(RxInst::Word(false)),
            'W' => Ok(RxInst::Word(true)),
            's' => Ok(RxInst::Space(false)),
            'S' => Ok(RxInst::Space(true)),
            'b' if !in_class => Ok(RxInst::Wb(true)),
            'B' if !in_class => Ok(RxInst::Wb(false)),
            'n' => Ok(RxInst::Chr('\n')),
            'r' => Ok(RxInst::Chr('\r')),
            't' => Ok(RxInst::Chr('\t')),
            'f' => Ok(RxInst::Chr('\u{000C}')),
            'v' => Ok(RxInst::Chr('\u{000B}')),
            _ => {
                if c.is_ascii_alphanumeric() {
                    Err("نمط غير صالح".to_string())
                } else {
                    Ok(RxInst::Chr(c))
                }
            }
        }
    }

    /// عنصر فئة واحد: محرف مفرد، أو فئة هروب (\d ونحوها)
    /// تُرجع نطاقاتها.
    fn class_item(&mut self) -> Result<ClassItem, String> {
        if self.peek() == Some('\\') {
            self.pos += 1;
            let c = self.next().ok_or_else(|| "نمط غير صالح".to_string())?;
            match c {
                'd' => Ok(ClassItem::Ranges(vec![('0', '9')])),
                'w' => Ok(ClassItem::Ranges(rx_word_ranges())),
                's' => Ok(ClassItem::Ranges(vec![
                    (' ', ' '),
                    ('\t', '\t'),
                    ('\n', '\n'),
                    ('\r', '\r'),
                    ('\u{000B}', '\u{000B}'),
                    ('\u{000C}', '\u{000C}'),
                ])),
                'D' | 'W' | 'S' => Err("نمط غير صالح".to_string()),
                'n' => Ok(ClassItem::Ch('\n')),
                'r' => Ok(ClassItem::Ch('\r')),
                't' => Ok(ClassItem::Ch('\t')),
                'f' => Ok(ClassItem::Ch('\u{000C}')),
                'v' => Ok(ClassItem::Ch('\u{000B}')),
                _ => {
                    if c.is_ascii_alphanumeric() {
                        Err("نمط غير صالح".to_string())
                    } else {
                        Ok(ClassItem::Ch(c))
                    }
                }
            }
        } else {
            let c = self.next().ok_or_else(|| "نمط غير صالح".to_string())?;
            Ok(ClassItem::Ch(c))
        }
    }

    fn parse_class(&mut self) -> Result<RxInst, String> {
        // الموضع عند '['.
        self.pos += 1;
        let neg = if self.peek() == Some('^') {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or_else(|| "نمط غير صالح".to_string())?;
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;
            let item = self.class_item()?;
            match item {
                ClassItem::Ranges(rs) => ranges.extend(rs),
                ClassItem::Ch(lo) => {
                    if self.peek() == Some('-')
                        && self.chars.get(self.pos + 1) != Some(&']')
                        && self.chars.get(self.pos + 1).is_some()
                    {
                        self.pos += 1;
                        let hi_item = self.class_item()?;
                        let hi = match hi_item {
                            ClassItem::Ch(h) => h,
                            ClassItem::Ranges(_) => {
                                return Err("نمط غير صالح".to_string())
                            }
                        };
                        if hi < lo {
                            return Err("نمط غير صالح".to_string());
                        }
                        ranges.push((lo, hi));
                    } else {
                        ranges.push((lo, lo));
                    }
                }
            }
        }
        Ok(RxInst::Cls { ranges, neg })
    }
}

enum ClassItem {
    Ch(char),
    Ranges(Vec<(char, char)>),
}

/// نطاقات محارف الكلمة (كما rx_is_word) للفئات.
fn rx_word_ranges() -> Vec<(char, char)> {
    vec![
        ('0', '9'),
        ('A', 'Z'),
        ('_', '_'),
        ('a', 'z'),
        ('\u{0600}', '\u{06FF}'),
        ('\u{0750}', '\u{077F}'),
        ('\u{08A0}', '\u{08FF}'),
        ('\u{FB50}', '\u{FDFF}'),
        ('\u{FE70}', '\u{FEFF}'),
    ]
}

fn rx_compile(pattern: &str, full: bool) -> Result<RxProg, String> {
    rx_compile_fold(pattern, full, false)
}

/// تجميع النمط مع طيّ الحالة اختيارا (وحدة بحث).
fn rx_compile_fold(pattern: &str, full: bool, fold: bool) -> Result<RxProg, String> {
    let chars: Vec<char> = pattern.chars().collect();
    if chars.len() > RX_MAX_PATTERN {
        return Err("النمط كبير جدا".to_string());
    }
    let mut c = RxCompiler {
        chars,
        pos: 0,
        prog: Vec::new(),
        ngroups: 0,
    };
    // نقطة الدخول يجب أن تكون بداية القطعة لا أول
    // تعليمة بُثت: فقطعة نجمة/اختيار/تناوب تبدأ
    // بانقسام يُبث بعد جسمها؛ فسلسلة الحفظ والمراسي
    // تُربط بالقطع لا بالسقوط الحر.
    let save0 = c.frag_one(RxInst::Save(0))?;
    let mut frag = save0;
    if full {
        let bol = c.frag_one(RxInst::Bol)?;
        frag = c.chain(frag, bol);
    }
    let body = c.parse_alt()?;
    if c.pos != c.chars.len() {
        return Err("نمط غير صالح".to_string());
    }
    frag = c.chain(frag, body);
    let save1 = c.frag_one(RxInst::Save(1))?;
    frag = c.chain(frag, save1);
    if full {
        let eol = c.frag_one(RxInst::Eol)?;
        frag = c.chain(frag, eol);
    }
    let match_idx = c.emit(RxInst::Match)?;
    c.patch(&frag.outs, match_idx);
    let _ = match_idx;
    Ok(RxProg {
        prog: c.prog,
        ngroups: c.ngroups,
        fold,
    })
}

#[derive(Clone)]
struct RxThread {
    pc: usize,
    saved: Vec<Option<usize>>,
}

/// أضف خيطا مع تتبع تعليمات إبسيلون (بأولوية الترتيب).
#[allow(clippy::too_many_arguments)]
fn rx_add_thread(
    prog: &[RxInst],
    list: &mut Vec<RxThread>,
    visited: &mut [bool],
    mut pc: usize,
    pos: usize,
    text: &[char],
    mut saved: Vec<Option<usize>>,
    steps: &mut usize,
) -> Result<(), String> {
    loop {
        *steps += 1;
        if *steps > RX_MAX_STEPS {
            return Err("تجاوز النمط حد الخطوات".to_string());
        }
        if pc >= prog.len() || visited[pc] {
            return Ok(());
        }
        match &prog[pc] {
            RxInst::Split(a, b) => {
                visited[pc] = true;
                let (a, b) = (*a, *b);
                rx_add_thread(prog, list, visited, a, pos, text, saved.clone(), steps)?;
                return rx_add_thread(prog, list, visited, b, pos, text, saved, steps);
            }
            RxInst::Jmp(t) => {
                visited[pc] = true;
                pc = *t;
            }
            RxInst::Save(s) => {
                visited[pc] = true;
                saved[*s] = Some(pos);
                pc += 1;
            }
            RxInst::Bol => {
                visited[pc] = true;
                if pos != 0 {
                    return Ok(());
                }
                pc += 1;
            }
            RxInst::Eol => {
                visited[pc] = true;
                if pos != text.len() {
                    return Ok(());
                }
                pc += 1;
            }
            RxInst::Wb(want) => {
                visited[pc] = true;
                let before = if pos == 0 {
                    false
                } else {
                    rx_is_word(text[pos - 1])
                };
                let after = if pos >= text.len() {
                    false
                } else {
                    rx_is_word(text[pos])
                };
                if (before != after) != *want {
                    return Ok(());
                }
                pc += 1;
            }
            _ => {
                visited[pc] = true;
                list.push(RxThread { pc, saved });
                return Ok(());
            }
        }
    }
}

/// مطابقة تبدأ عند `start` (أولوية اليسار أولا).
fn rx_match_at(
    prog: &RxProg,
    text: &[char],
    start: usize,
) -> Result<Option<Vec<Option<usize>>>, String> {
    let n = text.len();
    let nslots = 2 * (prog.ngroups + 1);
    let mut steps = 0usize;
    let mut clist: Vec<RxThread> = Vec::new();
    let mut visited = vec![false; prog.prog.len()];
    rx_add_thread(
        &prog.prog,
        &mut clist,
        &mut visited,
        0,
        start,
        text,
        vec![None; nslots],
        &mut steps,
    )?;
    let mut pos = start;
    loop {
        // أول مطابقة نهائية: كل خيط قبلها ميت هنا.
        let mut best: Option<Vec<Option<usize>>> = None;
        let mut alive = false;
        for t in &clist {
            match &prog.prog[t.pc] {
                RxInst::Match => {
                    best = Some(t.saved.clone());
                    break;
                }
                inst => {
                    if pos < n && rx_consumes(inst, text[pos], prog.fold) {
                        alive = true;
                        break;
                    }
                }
            }
        }
        if let Some(saved) = best {
            return Ok(Some(saved));
        }
        if !alive || pos >= n {
            return Ok(None);
        }
        let mut nlist: Vec<RxThread> = Vec::new();
        let mut nvisited = vec![false; prog.prog.len()];
        for t in &clist {
            steps += 1;
            if steps > RX_MAX_STEPS {
                return Err("تجاوز النمط حد الخطوات".to_string());
            }
            if rx_consumes(&prog.prog[t.pc], text[pos], prog.fold) {
                rx_add_thread(
                    &prog.prog,
                    &mut nlist,
                    &mut nvisited,
                    t.pc + 1,
                    pos + 1,
                    text,
                    t.saved.clone(),
                    &mut steps,
                )?;
            }
        }
        clist = nlist;
        pos += 1;
        if clist.is_empty() {
            return Ok(None);
        }
    }
}

/// أول مطابقة عند الموضع `from` أو بعده: (بداية، نهاية، خانات).
fn rx_find_from(
    prog: &RxProg,
    text: &[char],
    from: usize,
) -> Result<Option<(usize, usize, Vec<Option<usize>>)>, String> {
    for start in from..=text.len() {
        if let Some(saved) = rx_match_at(prog, text, start)? {
            let s = saved[0].unwrap_or(start);
            let e = saved[1].unwrap_or(start);
            return Ok(Some((s, e, saved)));
        }
    }
    Ok(None)
}

fn rx_check_text(text: &str) -> Result<Vec<char>, String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > RX_MAX_TEXT {
        return Err("النص كبير جدا".to_string());
    }
    Ok(chars)
}

/// مطابقة كاملة لنص بنمط (مع ^ و$ ضمنيين).
fn rx_full_match(pattern: &str, text: &str) -> Result<bool, String> {
    let chars = rx_check_text(text)?;
    let prog = rx_compile(pattern, true)?;
    Ok(rx_match_at(&prog, &chars, 0)?.is_some())
}

fn rx_find_first(
    pattern: &str,
    text: &str,
) -> Result<Option<(usize, usize, Vec<Option<usize>>, usize)>, String> {
    let chars = rx_check_text(text)?;
    let prog = rx_compile(pattern, false)?;
    match rx_find_from(&prog, &chars, 0)? {
        None => Ok(None),
        Some((s, e, saved)) => Ok(Some((s, e, saved, prog.ngroups))),
    }
}

fn rx_group_text(
    chars: &[char],
    saved: &[Option<usize>],
    group: usize,
) -> Option<String> {
    let s = saved.get(2 * group).copied().flatten()?;
    let e = saved.get(2 * group + 1).copied().flatten()?;
    Some(chars[s..e].iter().collect())
}

// --- وحدة نمط: دوال أصلية ---

fn regex_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("طابق", native("طابق", regex_match)),
        ("ابحث", native("ابحث", regex_search)),
        ("استخرج", native("استخرج", regex_extract)),
        ("استبدل", native("استبدل", regex_replace)),
        ("قسم", native("قسم", regex_split)),
        ("مجموعات", native("مجموعات", regex_groups)),
    ]
}

fn rx_err(e: String, line: usize, col: usize) -> RioError {
    RioError::runtime(e, line, col)
}

fn regex_match(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.طابق", args, 2, line, col)?;
    let pattern = need_string("نمط.طابق", &args[0], line, col)?;
    let text = need_string("نمط.طابق", &args[1], line, col)?;
    rx_full_match(&pattern, &text)
        .map(Value::Bool)
        .map_err(|e| rx_err(e, line, col))
}

fn regex_search(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.ابحث", args, 2, line, col)?;
    let pattern = need_string("نمط.ابحث", &args[0], line, col)?;
    let text = need_string("نمط.ابحث", &args[1], line, col)?;
    let chars = rx_check_text(&text).map_err(|e| rx_err(e, line, col))?;
    let prog = rx_compile(&pattern, false).map_err(|e| rx_err(e, line, col))?;
    match rx_find_from(&prog, &chars, 0).map_err(|e| rx_err(e, line, col))? {
        None => Ok(Value::Nil),
        Some((s, e, _)) => Ok(Value::Str(chars[s..e].iter().collect())),
    }
}

fn regex_extract(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.استخرج", args, 2, line, col)?;
    let pattern = need_string("نمط.استخرج", &args[0], line, col)?;
    let text = need_string("نمط.استخرج", &args[1], line, col)?;
    let chars = rx_check_text(&text).map_err(|e| rx_err(e, line, col))?;
    let prog = rx_compile(&pattern, false).map_err(|e| rx_err(e, line, col))?;
    let mut out: Vec<Value> = Vec::new();
    let mut pos = 0usize;
    while pos <= chars.len() {
        match rx_find_from(&prog, &chars, pos).map_err(|e| rx_err(e, line, col))? {
            None => break,
            Some((s, e, _)) => {
                out.push(Value::Str(chars[s..e].iter().collect()));
                if e == s {
                    if e >= chars.len() {
                        break;
                    }
                    pos = e + 1;
                } else {
                    pos = e;
                }
            }
        }
    }
    Ok(Value::List(out))
}

/// توسعة بديل الاستبدال: $0 للمطابقة كلها، و$1..$9
/// للمجموعات (غير الملتقَطة تصير فارغة)، و$$ للدولار.
fn rx_expand_replacement(
    repl: &str,
    chars: &[char],
    s: usize,
    e: usize,
    saved: &[Option<usize>],
    ngroups: usize,
) -> String {
    let mut out = String::new();
    let rchars: Vec<char> = repl.chars().collect();
    let mut i = 0usize;
    while i < rchars.len() {
        let c = rchars[i];
        if c == '$' && i + 1 < rchars.len() {
            let n = rchars[i + 1];
            if n == '$' {
                out.push('$');
                i += 2;
                continue;
            }
            if let Some(d) = n.to_digit(10) {
                let g = d as usize;
                if g == 0 {
                    out.extend(chars[s..e].iter().copied());
                } else if g <= ngroups {
                    if let Some(t) = rx_group_text(chars, saved, g) {
                        out.push_str(&t);
                    }
                }
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn regex_replace(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.استبدل", args, 3, line, col)?;
    let pattern = need_string("نمط.استبدل", &args[0], line, col)?;
    let text = need_string("نمط.استبدل", &args[1], line, col)?;
    let repl = need_string("نمط.استبدل", &args[2], line, col)?;
    regex_replace_fold(&pattern, &text, &repl, false, line, col)
}

/// محرك نمط.استبدل نفسه مع طيّ حالة اختياري — تعيد
/// بحث.استبدل_معاينة استعماله بلا محرك جديد.
fn regex_replace_fold(
    pattern: &str,
    text: &str,
    repl: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let chars = rx_check_text(text).map_err(|e| rx_err(e, line, col))?;
    let prog = rx_compile_fold(pattern, false, fold).map_err(|e| rx_err(e, line, col))?;
    let mut out = String::new();
    let mut pos = 0usize;
    while pos <= chars.len() {
        match rx_find_from(&prog, &chars, pos).map_err(|e| rx_err(e, line, col))? {
            None => break,
            Some((s, e, saved)) => {
                if out.len() > RX_MAX_TEXT || s - pos > RX_MAX_TEXT {
                    return Err(rx_err("النص كبير جدا".to_string(), line, col));
                }
                out.extend(chars[pos..s].iter().copied());
                out.push_str(&rx_expand_replacement(&repl, &chars, s, e, &saved, prog.ngroups));
                if out.chars().count() > RX_MAX_TEXT {
                    return Err(rx_err("النص كبير جدا".to_string(), line, col));
                }
                if e == s {
                    if e >= chars.len() {
                        pos = e + 1;
                        break;
                    }
                    out.push(chars[e]);
                    pos = e + 1;
                } else {
                    pos = e;
                }
            }
        }
    }
    if pos <= chars.len() {
        out.extend(chars[pos..].iter().copied());
    }
    Ok(Value::Str(out))
}

fn regex_split(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.قسم", args, 2, line, col)?;
    let pattern = need_string("نمط.قسم", &args[0], line, col)?;
    let text = need_string("نمط.قسم", &args[1], line, col)?;
    let chars = rx_check_text(&text).map_err(|e| rx_err(e, line, col))?;
    let prog = rx_compile(&pattern, false).map_err(|e| rx_err(e, line, col))?;
    let mut pieces: Vec<Value> = Vec::new();
    let mut last = 0usize;
    let mut pos = 0usize;
    while pos <= chars.len() {
        match rx_find_from(&prog, &chars, pos).map_err(|e| rx_err(e, line, col))? {
            None => break,
            Some((s, e, _)) => {
                if s == e {
                    // مطابقة فارغة: المحرف يبقى في القطعة الجارية.
                    if e >= chars.len() {
                        break;
                    }
                    pos = e + 1;
                } else {
                    pieces.push(Value::Str(chars[last..s].iter().collect()));
                    last = e;
                    pos = e;
                }
            }
        }
    }
    pieces.push(Value::Str(chars[last..].iter().collect()));
    Ok(Value::List(pieces))
}

fn regex_groups(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نمط.مجموعات", args, 2, line, col)?;
    let pattern = need_string("نمط.مجموعات", &args[0], line, col)?;
    let text = need_string("نمط.مجموعات", &args[1], line, col)?;
    let found = rx_find_first(&pattern, &text).map_err(|e| rx_err(e, line, col))?;
    match found {
        None => Ok(Value::List(vec![])),
        Some((_, _, saved, ngroups)) => {
            let chars = rx_check_text(&text).map_err(|e| rx_err(e, line, col))?;
            let mut out: Vec<Value> = Vec::new();
            for g in 1..=ngroups {
                match rx_group_text(&chars, &saved, g) {
                    Some(t) => out.push(Value::Str(t)),
                    None => out.push(Value::Nil),
                }
            }
            Ok(Value::List(out))
        }
    }
}

// ---------- وحدة بحث: دفعة لينكس الأولى، الموجة الأولى §1 ----------
// البنود 1 و2 و3 و4 و5 و6 و7 و8 و18 و19 و20 و23 و27 و28.
// تستعمل محرك التعابير في وحدة نمط (rx_compile_fold /
// rx_find_from) بلا محرك جديد؛ وتجاهل الحالة طيّ في
// RxProg (Rust وفي المرآة C معا).
// الشكل: ابحث_بنمط(نص، نمط، خيارات؟) وابحث_بنص_ثابت
// (نص، نص_ثابت، خيارات؟) — الافتراضي قائمة أسطر مطابقة
// نصية؛ ومع عمود_المطابقة: صحيح تصير قائمة سجلات
// (قواميس) بمفاتيح سطر ورقم_السطر وعمود ونص_مطابق —
// والعمود محرفي 1-based (لا بايتات)؛ وسجل واحد لكل
// سطر مطابق بالمطابقة الأولى المؤهلة فيه؛ ومع
// اعكس_المطابقة يحمل السجل عمود 1 ونص_مطابق فارغا
// (لا مطابقة فيه)؛ وقلم_النتائج يقلم السطر المعروض
// وحده (العمود على السطر الأصلي)؛ وصامت يرجع منطقيا
// (وُجد؟)؛ وأقصى_مطابقات يعد الأسطر المطابقة.
// تقسيم الأسطر على \n مع تجريد \r اللاحق (سلوك
// lines)؛ فالنص المنتهي بسطر جديد بلا سطر فارغ أخير.
// خيارات الحالة الثلاثة متعارضة مثنى مثنى (أي اثنين
// معا خطأ عربي)؛ والمفتاح المجهول ونوع الخيار الخاطئ
// خطأ عربي يسمي المفتاح.
// مفاتيح مخزنة بلا شدة ولا تطويل (قاعدة نبّه).
// ---------- وحدة بحث: دفعة لينكس الأولى، الموجة الأولى §2 ----------
// البنود 30 و36 و37 و38 و39 و40 و41 و42 و44 و45 و48
// و50 و51 و55 و57 و68 — امتداد على الشكل نفسه:
// - أقصى_عمق (عدد؛ 0 = المجلد الأعلى وحده) يسري في
//   البحث في الملفات فقط (ملفات_فيها وملفات_بلا_مطابقة).
// - عد_المطابقات يعد الأسطر المطابقة؛ وعد_كل_المطابقات
//   يعد كل المطابقات (لا الأسطر)؛ والمطابق_فقط يرجع
//   قائمة النصوص المطابقة وحدها؛ واستبدل_معاينة يرجع
//   النص بعد الاستبدال بلا تعديل أي ملف، بإعادة استعمال
//   محرك نمط.استبدل نفسه (rx_compile_fold و
//   rx_find_from وrx_expand_replacement) مع طيّ الحالة
//   من الخيارات — فلا محرك استبدال جديد.
// - سياق_قبل وسياق_بعد وسياق: مع سياق موجب تكتسب سجلات
//   النتائج حقلين قبل وبعد (كلاهما قائمة أسطر نصية)؛
//   وبدون سياق لا يظهر الحقلان؛ وسياق_قبل وسياق_بعد
//   المنفردان يغلبان سياقا حين يوجدان معه؛ والسياق
//   يجعل النتيجة سجلات ولو بلا عمود_المطابقة.
// - رقم_السطر (منطقي): صيغة سجلات بلا عمود بمفاتيح
//   سطر ورقم_السطر ونص_مطابق؛ وهو مفتاح خيار في بحث
//   فقط — ورقم_السطر في بيئة مفتاح آخر يرجع رقم السطر
//   الحالي في البرنامج، فنطاقان مختلفان بلا حجب.
// - أظهر_اسم_الملف (منطقي): في البحث في الملفات اسم
//   الملف هو النتيجة نفسها (قائمة مسارات)؛ ومقبول في
//   بحث النص بلا أثر. ولا يسمى اسم_الملف — فهي دالة
//   قائمة في وحدة نص.
// - جسون (منطقي): ترجع النتائج نص JSON عبر وحدة جسون
//   القائمة (json_write_value) — لا محلل JSON جديد.
// - لوّن (منطقي): يغلّف كل مطابق بلون واحد في الأسطر
//   المرجعة؛ والمحرك بلا دعم ANSI قائم (وحدة ألوان
//   تحويل قيم ألوان لا تلوين طرفية) فيُصدر رموز ANSI
//   القياسية: المطابق بالأحمر \x1b[31m ... \x1b[0m.
//   والتغليف على السطر المعروض (بعد القلم إن وُجد)؛
//   وفي المطابق_فقط يُغلَّف كل نص مطابق وحده.
// - رتب_النتائج(نتائج، اتجاه؟): ترتب سجلات النتائج
//   بالمفتاح (ملف ثم رقم_السطر)؛ والافتراضي تصاعدي؛
//   واتجاه نص «تصاعدي» أو «تنازلي»؛ وقوائم النصوص
//   تُرتب نصا؛ والفرز مستقر.
// - مساعدة(): نص مساعدة عربي قصير (الدوال + الخيارات).
//   ومساعدة هنا دالة في وحدة بحث — أما مساعدة كعلم
//   CLI (--مساعدة) وأمر REPL (:مساعدة) فنطاق مختلف
//   بلا حجب.
// - البحث في الملفات: اجتياز المجلد (يحترم أقصى_عمق)
//   وقراءة الملفات النصية؛ والملفات غير المقروءة أو
//   الثنائية (تحوي بايت صفر أو ليست UTF-8 صالحا)
//   تُتخطى من القائمتين معا؛ والروابط الرمزية تُتخطى؛
//   والنتائج مرتبة بالمسار؛ وأنماط أنماط_من_ملف صالحة
//   كأنماط للبحث.
// - أقصى_مطابقات يحد الأسطر المطابقة المعالجة في كل
//   الدوال (ففي العد يحد العد، وفي المطابق_فقط يحد
//   الأسطر التي تجمع منها المطابقات)؛ وصامت يرجع
//   منطقيا (هل وُجدت نتيجة؟) في دوال البحث والعد
//   والملفات؛ وفي استبدل_معاينة مقبول بلا أثر.

#[derive(Clone, Copy, Default)]
struct SearchOpts {
    ignore_case: bool,
    case_sensitive: bool,
    smart_case: bool,
    whole_word: bool,
    whole_line: bool,
    invert: bool,
    max_count: Option<i64>,
    column: bool,
    trim: bool,
    quiet: bool,
    before: Option<i64>,
    after: Option<i64>,
    context: Option<i64>,
    line_number: bool,
    show_filename: bool,
    json: bool,
    color: bool,
    max_depth: Option<i64>,
}

fn search_need_bool(
    display: &str,
    key: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قيمة منطقية للخيار {} لكن أعطيت {}",
                display,
                key,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn search_need_count(
    display: &str,
    key: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    let n = need_integer(display, v, line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            format!("الدالة {} الخيار {} لا يقبل عددا سالبا", display, key),
            line,
            col,
        ));
    }
    Ok(n)
}

fn search_parse_opts_dict(
    display: &str,
    pairs: &[(String, Value)],
    opts: &mut SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<()> {
    for (k, v) in pairs {
        match k.as_str() {
            "تجاهل_الحالة" => {
                opts.ignore_case = search_need_bool(display, k, v, line, col)?;
            }
            "حساسية_الحالة" => {
                opts.case_sensitive = search_need_bool(display, k, v, line, col)?;
            }
            "حالة_ذكية" => {
                opts.smart_case = search_need_bool(display, k, v, line, col)?;
            }
            "كلمة_كاملة" => {
                opts.whole_word = search_need_bool(display, k, v, line, col)?;
            }
            "سطر_كامل" => {
                opts.whole_line = search_need_bool(display, k, v, line, col)?;
            }
            "اعكس_المطابقة" => {
                opts.invert = search_need_bool(display, k, v, line, col)?;
            }
            "عمود_المطابقة" => {
                opts.column = search_need_bool(display, k, v, line, col)?;
            }
            "قلم_النتائج" => {
                opts.trim = search_need_bool(display, k, v, line, col)?;
            }
            "صامت" => {
                opts.quiet = search_need_bool(display, k, v, line, col)?;
            }
            "أقصى_مطابقات" => {
                opts.max_count = Some(search_need_count(display, k, v, line, col)?);
            }
            "سياق_قبل" => {
                opts.before = Some(search_need_count(display, k, v, line, col)?);
            }
            "سياق_بعد" => {
                opts.after = Some(search_need_count(display, k, v, line, col)?);
            }
            "سياق" => {
                opts.context = Some(search_need_count(display, k, v, line, col)?);
            }
            "رقم_السطر" => {
                opts.line_number = search_need_bool(display, k, v, line, col)?;
            }
            "أظهر_اسم_الملف" => {
                opts.show_filename = search_need_bool(display, k, v, line, col)?;
            }
            "جسون" => {
                opts.json = search_need_bool(display, k, v, line, col)?;
            }
            "لوّن" => {
                opts.color = search_need_bool(display, k, v, line, col)?;
            }
            "أقصى_عمق" => {
                opts.max_depth = Some(search_need_count(display, k, v, line, col)?);
            }
            other => {
                return Err(RioError::runtime(
                    format!("الدالة {} خيار غير معروف: {}", display, other),
                    line,
                    col,
                ));
            }
        }
    }
    let ncase = [opts.ignore_case, opts.case_sensitive, opts.smart_case]
        .iter()
        .filter(|b| **b)
        .count();
    if ncase > 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} خيارات حالة متضاربة: اختر واحدا فقط من تجاهل_الحالة وحساسية_الحالة وحالة_ذكية",
                display
            ),
            line,
            col,
        ));
    }
    Ok(())
}

fn search_parse_opts(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<SearchOpts> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                display,
                args.len()
            ),
            line,
            col,
        ));
    }
    let mut opts = SearchOpts::default();
    if args.len() == 3 {
        let pairs = need_dict(display, &args[2], line, col)?;
        search_parse_opts_dict(display, pairs, &mut opts, line, col)?;
    }
    Ok(opts)
}

fn search_parse_opts_4(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<SearchOpts> {
    if args.len() < 3 || args.len() > 4 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع ثلاثة معاملات أو أربعة لكن أعطيت {}",
                display,
                args.len()
            ),
            line,
            col,
        ));
    }
    let mut opts = SearchOpts::default();
    if args.len() == 4 {
        let pairs = need_dict(display, &args[3], line, col)?;
        search_parse_opts_dict(display, pairs, &mut opts, line, col)?;
    }
    Ok(opts)
}

/// السياق الفعلي (قبل، بعد): المنفرد يغلب المجمع.
fn search_eff_context(opts: &SearchOpts) -> (i64, i64) {
    let before = opts.before.or(opts.context).unwrap_or(0);
    let after = opts.after.or(opts.context).unwrap_or(0);
    (before, after)
}

fn search_has_context(opts: &SearchOpts) -> bool {
    let (b, a) = search_eff_context(opts);
    b > 0 || a > 0
}

/// هل النتيجة سجلات لا أسطر نصية؟
fn search_record_form(opts: &SearchOpts) -> bool {
    opts.column || opts.line_number || search_has_context(opts)
}

/// طيّ الحالة للنمط: تجاهل صريح، أو ذكي (يطوي إن كان
/// النمط صغيرا كله — بلا أي محرف كبير)، أو حساس.
fn search_fold(pattern: &str, opts: &SearchOpts) -> bool {
    if opts.ignore_case {
        return true;
    }
    if opts.smart_case {
        return !pattern.chars().any(|c| c.is_uppercase());
    }
    false
}

/// هل المدى [s, e) كلمة مستقلة (أو — للمطابقة الفارغة
/// — حد كلمة عند الموضع)؟
fn search_word_ok(chars: &[char], s: usize, e: usize) -> bool {
    if e > s {
        let before_ok = s == 0 || !rx_is_word(chars[s - 1]);
        let after_ok = e >= chars.len() || !rx_is_word(chars[e]);
        before_ok && after_ok
    } else {
        let before = s > 0 && rx_is_word(chars[s - 1]);
        let after = s < chars.len() && rx_is_word(chars[s]);
        before != after
    }
}

/// أول مطابقة تعبير مؤهلة في سطر (بداية، نهاية).
/// كل المطابقات المؤهلة في سطر (بداية، نهاية) — بلا
/// تداخل؛ والمطابقة الفارغة تتقدم محرفا واحدا؛ ومع
/// سطر_كامل مطابقة واحدة على الأكثر هي السطر كله.
fn search_line_all(
    prog: Option<&RxProg>,
    lit_chars: &[char],
    is_regex: bool,
    chars: &[char],
    fold: bool,
    whole_word: bool,
    whole_line: bool,
) -> Result<Vec<(usize, usize)>, String> {
    if whole_line {
        if is_regex {
            let p = prog.unwrap();
            match rx_find_from(p, chars, 0)? {
                None => return Ok(Vec::new()),
                Some((s, e, _)) => {
                    if s == 0 && e == chars.len() {
                        return Ok(vec![(s, e)]);
                    }
                    return Ok(Vec::new());
                }
            }
        }
        let eq = chars.len() == lit_chars.len()
            && chars.iter().zip(lit_chars.iter()).all(|(a, b)| {
                if fold {
                    rx_fold(*a) == rx_fold(*b)
                } else {
                    a == b
                }
            });
        if eq && (!whole_word || search_word_ok(chars, 0, chars.len())) {
            return Ok(vec![(0, chars.len())]);
        }
        return Ok(Vec::new());
    }
    if is_regex {
        let p = prog.unwrap();
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos <= chars.len() {
            match rx_find_from(p, chars, pos)? {
                None => break,
                Some((s, e, _)) => {
                    if !whole_word || search_word_ok(chars, s, e) {
                        out.push((s, e));
                    }
                    if e == s {
                        if e >= chars.len() {
                            break;
                        }
                        pos = e + 1;
                    } else {
                        pos = e;
                    }
                }
            }
        }
        Ok(out)
    } else {
        let mut out = Vec::new();
        if lit_chars.is_empty() {
            for p in 0..=chars.len() {
                if !whole_word || search_word_ok(chars, p, p) {
                    out.push((p, p));
                }
            }
            return Ok(out);
        }
        if lit_chars.len() > chars.len() {
            return Ok(out);
        }
        let eq = |a: char, b: char| {
            if fold {
                rx_fold(a) == rx_fold(b)
            } else {
                a == b
            }
        };
        let mut p = 0usize;
        while p + lit_chars.len() <= chars.len() {
            if lit_chars.iter().enumerate().all(|(i, b)| eq(chars[p + i], *b))
                && (!whole_word || search_word_ok(chars, p, p + lit_chars.len()))
            {
                out.push((p, p + lit_chars.len()));
                p += lit_chars.len();
            } else {
                p += 1;
            }
        }
        Ok(out)
    }
}

fn search_trimmed(raw: &str, opts: &SearchOpts) -> String {
    if opts.trim {
        raw.trim_matches(|c: char| c.is_ascii_whitespace())
            .to_string()
    } else {
        raw.to_string()
    }
}

struct SearchHit {
    idx: usize,
    shown: String,
    span: Option<(usize, usize)>,
    shown_span: Option<(usize, usize)>,
    matched: String,
    before: Vec<String>,
    after: Vec<String>,
}

/// فحص النص سطرا سطرا: الأسطر المؤهلة (مع العكس
/// والحد الأقصى) مع سياقها ومطابقتها الأولى.
fn search_scan(
    display: &str,
    text: &str,
    is_regex: bool,
    pattern: &str,
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Vec<SearchHit>> {
    let _ = rx_check_text(text).map_err(|e| rx_err(e, line, col))?;
    let fold = search_fold(pattern, opts);
    let prog = if is_regex {
        Some(
            rx_compile_fold(pattern, opts.whole_line, fold)
                .map_err(|e| rx_err(e, line, col))?,
        )
    } else {
        None
    };
    let lit_chars: Vec<char> = if is_regex {
        Vec::new()
    } else {
        pattern.chars().collect()
    };
    let lines: Vec<&str> = text.lines().collect();
    let (ctx_before, ctx_after) = search_eff_context(opts);
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut count: i64 = 0;
    for (idx, raw_line) in lines.iter().enumerate() {
        if let Some(max) = opts.max_count {
            if count >= max {
                break;
            }
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let all = search_line_all(
            prog.as_ref(),
            &lit_chars,
            is_regex,
            &chars,
            fold,
            opts.whole_word,
            opts.whole_line,
        )
        .map_err(|e| rx_err(e, line, col))?;
        let has = !all.is_empty();
        if has != opts.invert {
            count += 1;
            let shown = search_trimmed(raw_line, opts);
            let shown_span = if opts.invert {
                None
            } else {
                let schars: Vec<char> = shown.chars().collect();
                let sall = search_line_all(
                    prog.as_ref(),
                    &lit_chars,
                    is_regex,
                    &schars,
                    fold,
                    opts.whole_word,
                    opts.whole_line,
                )
                .map_err(|e| rx_err(e, line, col))?;
                sall.first().copied()
            };
            let (span, matched) = if opts.invert {
                (None, String::new())
            } else {
                let (s, e) = all[0];
                (Some((s, e)), chars[s..e].iter().collect())
            };
            let b = ctx_before as usize;
            let a = ctx_after as usize;
            let bfrom = idx.saturating_sub(b);
            let before: Vec<String> = lines[bfrom..idx]
                .iter()
                .map(|l| search_trimmed(l, opts))
                .collect();
            let ato = (idx + 1 + a).min(lines.len());
            let after: Vec<String> = lines[idx + 1..ato]
                .iter()
                .map(|l| search_trimmed(l, opts))
                .collect();
            hits.push(SearchHit {
                idx,
                shown,
                span,
                shown_span,
                matched,
                before,
                after,
            });
        }
    }
    Ok(hits)
}

/// تغليف المطابق في السطر المعروض بالأحمر
/// (ANSI القياسي)؛ وبلا مطابقة يبقى السطر كما هو.
fn search_color_shown(hit: &SearchHit, opts: &SearchOpts) -> String {
    if !opts.color {
        return hit.shown.clone();
    }
    match hit.shown_span {
        Some((s, e)) => {
            let chars: Vec<char> = hit.shown.chars().collect();
            let mut out = String::new();
            out.extend(chars[..s].iter());
            out.push_str("\u{1b}[31m");
            out.extend(chars[s..e].iter());
            out.push_str("\u{1b}[0m");
            out.extend(chars[e..].iter());
            out
        }
        None => hit.shown.clone(),
    }
}

fn search_maybe_json(
    value: Value,
    opts: &SearchOpts,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if !opts.json {
        return Ok(value);
    }
    let mut out = String::new();
    json_write_value(&value, &mut out, 0).map_err(|_| {
        RioError::runtime(
            format!("الدالة {} قيمة لا تقبل كتابة JSON", display),
            line,
            col,
        )
    })?;
    Ok(Value::Str(out))
}

fn search_render(
    display: &str,
    hits: &[SearchHit],
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if opts.quiet {
        return search_maybe_json(Value::Bool(!hits.is_empty()), opts, display, line, col);
    }
    let value = if search_record_form(opts) {
        Value::List(
            hits.iter()
                .map(|h| {
                    let mut pairs: Vec<(String, Value)> = vec![
                        ("سطر".to_string(), Value::Str(search_color_shown(h, opts))),
                        (
                            "رقم_السطر".to_string(),
                            Value::Number((h.idx + 1) as f64),
                        ),
                    ];
                    if opts.column {
                        pairs.push((
                            "عمود".to_string(),
                            Value::Number(h.span.map(|(s, _)| s + 1).unwrap_or(1) as f64),
                        ));
                    }
                    pairs.push(("نص_مطابق".to_string(), Value::Str(h.matched.clone())));
                    if search_has_context(opts) {
                        pairs.push((
                            "قبل".to_string(),
                            Value::List(
                                h.before.iter().map(|s| Value::Str(s.clone())).collect(),
                            ),
                        ));
                        pairs.push((
                            "بعد".to_string(),
                            Value::List(
                                h.after.iter().map(|s| Value::Str(s.clone())).collect(),
                            ),
                        ));
                    }
                    Value::Dict(pairs)
                })
                .collect(),
        )
    } else {
        Value::List(
            hits.iter()
                .map(|h| Value::Str(search_color_shown(h, opts)))
                .collect(),
        )
    };
    search_maybe_json(value, opts, display, line, col)
}

fn search_run(
    display: &str,
    text: &str,
    is_regex: bool,
    pattern: &str,
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let hits = search_scan(display, text, is_regex, pattern, opts, line, col)?;
    search_render(display, &hits, opts, line, col)
}

/// العد: كل الأسطر أو كل المطابقات — مع العكس
/// (السطر غير المطابق يُحسب مرة واحدة في عد الكل)
/// والحد الأقصى على الأسطر المؤهلة.
fn search_count_impl(
    display: &str,
    text: &str,
    is_regex: bool,
    pattern: &str,
    opts: &SearchOpts,
    all_matches: bool,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let _ = rx_check_text(text).map_err(|e| rx_err(e, line, col))?;
    let fold = search_fold(pattern, opts);
    let prog = if is_regex {
        Some(
            rx_compile_fold(pattern, opts.whole_line, fold)
                .map_err(|e| rx_err(e, line, col))?,
        )
    } else {
        None
    };
    let lit_chars: Vec<char> = if is_regex {
        Vec::new()
    } else {
        pattern.chars().collect()
    };
    let mut total: i64 = 0;
    let mut count: i64 = 0;
    for raw_line in text.lines() {
        if let Some(max) = opts.max_count {
            if count >= max {
                break;
            }
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let all = search_line_all(
            prog.as_ref(),
            &lit_chars,
            is_regex,
            &chars,
            fold,
            opts.whole_word,
            opts.whole_line,
        )
        .map_err(|e| rx_err(e, line, col))?;
        let has = !all.is_empty();
        if has != opts.invert {
            count += 1;
            if all_matches {
                total += if opts.invert { 1 } else { all.len() as i64 };
            } else {
                total += 1;
            }
        }
    }
    if opts.quiet {
        return search_maybe_json(Value::Bool(total > 0), opts, display, line, col);
    }
    search_maybe_json(Value::Number(total as f64), opts, display, line, col)
}

/// كل النصوص المطابقة وحدها — بترتيب الأسطر ثم
/// مواضعها؛ ومع العكس لا مطابقة فالقائمة فارغة.
fn search_only_impl(
    display: &str,
    text: &str,
    is_regex: bool,
    pattern: &str,
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let _ = rx_check_text(text).map_err(|e| rx_err(e, line, col))?;
    let fold = search_fold(pattern, opts);
    let prog = if is_regex {
        Some(
            rx_compile_fold(pattern, opts.whole_line, fold)
                .map_err(|e| rx_err(e, line, col))?,
        )
    } else {
        None
    };
    let lit_chars: Vec<char> = if is_regex {
        Vec::new()
    } else {
        pattern.chars().collect()
    };
    let mut out: Vec<Value> = Vec::new();
    let mut count: i64 = 0;
    for raw_line in text.lines() {
        if let Some(max) = opts.max_count {
            if count >= max {
                break;
            }
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let all = search_line_all(
            prog.as_ref(),
            &lit_chars,
            is_regex,
            &chars,
            fold,
            opts.whole_word,
            opts.whole_line,
        )
        .map_err(|e| rx_err(e, line, col))?;
        let has = !all.is_empty();
        if has != opts.invert {
            count += 1;
            if !opts.invert {
                for (s, e) in &all {
                    let m: String = chars[*s..*e].iter().collect();
                    if opts.color {
                        out.push(Value::Str(format!("\u{1b}[31m{}\u{1b}[0m", m)));
                    } else {
                        out.push(Value::Str(m));
                    }
                }
            }
        }
    }
    if opts.quiet {
        return search_maybe_json(Value::Bool(!out.is_empty()), opts, display, line, col);
    }
    search_maybe_json(Value::List(out), opts, display, line, col)
}

fn search_count_lines(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts("بحث.عد_المطابقات", args, line, col)?;
    let text = need_string("بحث.عد_المطابقات", &args[0], line, col)?;
    let pattern = need_string("بحث.عد_المطابقات", &args[1], line, col)?;
    search_count_impl(
        "بحث.عد_المطابقات",
        &text,
        true,
        &pattern,
        &opts,
        false,
        line,
        col,
    )
}

fn search_count_all(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts("بحث.عد_كل_المطابقات", args, line, col)?;
    let text = need_string("بحث.عد_كل_المطابقات", &args[0], line, col)?;
    let pattern = need_string("بحث.عد_كل_المطابقات", &args[1], line, col)?;
    search_count_impl(
        "بحث.عد_كل_المطابقات",
        &text,
        true,
        &pattern,
        &opts,
        true,
        line,
        col,
    )
}

fn search_only(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts("بحث.المطابق_فقط", args, line, col)?;
    let text = need_string("بحث.المطابق_فقط", &args[0], line, col)?;
    let pattern = need_string("بحث.المطابق_فقط", &args[1], line, col)?;
    search_only_impl("بحث.المطابق_فقط", &text, true, &pattern, &opts, line, col)
}

fn search_replace_preview(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts_4("بحث.استبدل_معاينة", args, line, col)?;
    let text = need_string("بحث.استبدل_معاينة", &args[0], line, col)?;
    let pattern = need_string("بحث.استبدل_معاينة", &args[1], line, col)?;
    let repl = need_string("بحث.استبدل_معاينة", &args[2], line, col)?;
    // إعادة استعمال محرك نمط.استبدل نفسه مع طيّ
    // الحالة من الخيارات؛ وباقي الخيارات (كلمة_كاملة
    // وسطر_كامل واعكس_المطابقة والسياق والعمود
    // وقلم_النتائج وصامت وجسون ولوّن وأقصى_عمق
    // وأظهر_اسم_الملف) مقبولة بلا أثر هنا.
    let fold = search_fold(&pattern, &opts);
    regex_replace_fold(&pattern, &text, &repl, fold, line, col)
}

fn search_regex(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts("بحث.ابحث_بنمط", args, line, col)?;
    let text = need_string("بحث.ابحث_بنمط", &args[0], line, col)?;
    let pattern = need_string("بحث.ابحث_بنمط", &args[1], line, col)?;
    search_run("بحث.ابحث_بنمط", &text, true, &pattern, &opts, line, col)
}

fn search_literal(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts("بحث.ابحث_بنص_ثابت", args, line, col)?;
    let text = need_string("بحث.ابحث_بنص_ثابت", &args[0], line, col)?;
    let literal = need_string("بحث.ابحث_بنص_ثابت", &args[1], line, col)?;
    search_run(
        "بحث.ابحث_بنص_ثابت",
        &text,
        false,
        &literal,
        &opts,
        line,
        col,
    )
}

fn search_patterns_from_file(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("بحث.أنماط_من_ملف", args, 1, line, col)?;
    let path = need_string("بحث.أنماط_من_ملف", &args[0], line, col)?;
    // المسار النسبي يُحل مقابل مجلد الجلسة (كما في
    // ملفات.افتح) لا مقابل مجلد العملية — فبعد هجرة
    // «نقل البيت» يصير مجلد الجلسة home/، وملف
    // أنشأه ملفات.اكتب لا يوجد إلا هناك.
    let full = {
        let p = std::path::Path::new(&path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            ctx.server_cwd().join(p)
        }
    };
    let content = std::fs::read_to_string(&full).map_err(|_| {
        RioError::runtime(format!("تعذر قراءة الملف {}", path), line, col)
    })?;
    let out: Vec<Value> = content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| Value::Str(l.to_string()))
        .collect();
    Ok(Value::List(out))
}

fn search_resolve_path(
    ctx: &mut dyn HandlerCaller,
    path: &str,
) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.server_cwd().join(p)
    }
}

/// اجتياز مجلد تكراريا: كل ملف عادي (مسار نظام +
/// مسار عرض)؛ والروابط الرمزية تُتخطى؛ والمجلد
/// الفرعي غير المقروء يُتخطى بصمت؛ والعمق 0 يعني
/// ملفات المجلد الأعلى وحدها.
fn search_walk_dir(
    dir_fs: &std::path::Path,
    dir_display: &str,
    depth: i64,
    max_depth: i64,
    out: &mut Vec<(std::path::PathBuf, String)>,
) {
    let entries = match std::fs::read_dir(dir_fs) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_display = if dir_display.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", dir_display, name)
        };
        if ft.is_dir() {
            if depth < max_depth {
                search_walk_dir(&entry.path(), &child_display, depth + 1, max_depth, out);
            }
        } else if ft.is_file() {
            out.push((entry.path(), child_display));
        }
    }
}

fn search_walk_files(
    root_display: &str,
    root_fs: &std::path::Path,
    max_depth: i64,
    line: usize,
    col: usize,
) -> RioResult<Vec<(std::path::PathBuf, String)>> {
    let meta = std::fs::metadata(root_fs).map_err(|_| {
        RioError::runtime(format!("المسار غير موجود: {}", root_display), line, col)
    })?;
    if meta.is_file() {
        return Ok(vec![(root_fs.to_path_buf(), root_display.to_string())]);
    }
    if !meta.is_dir() {
        return Err(RioError::runtime(
            format!("المسار غير موجود: {}", root_display),
            line,
            col,
        ));
    }
    let base = root_display.trim_end_matches('/');
    let base = if base == "." { "" } else { base };
    let mut out: Vec<(std::path::PathBuf, String)> = Vec::new();
    search_walk_dir(root_fs, base, 0, max_depth, &mut out);
    Ok(out)
}

/// هل في الملف مطابقة؟ ويرجع لاشيء للملف المتخطى
/// (غير مقروء أو ثنائي: بايت صفر أو ليس UTF-8
/// صالحا) — فلا يظهر في أي من القائمتين.
fn search_file_has_match(
    display: &str,
    fs_path: &std::path::Path,
    pattern: &str,
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Option<bool>> {
    let bytes = match std::fs::read(fs_path) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    if bytes.contains(&0) {
        return Ok(None);
    }
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    let hits = search_scan(display, &text, true, pattern, opts, line, col)?;
    Ok(Some(!hits.is_empty()))
}

fn search_files_impl(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    want_match: bool,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let opts = search_parse_opts(display, args, line, col)?;
    let path = need_string(display, &args[0], line, col)?;
    let pattern = need_string(display, &args[1], line, col)?;
    // التحقق من النمط أولا — فلو كانت كل الملفات
    // ثنائية أو غير مقروءة لما ظهر خطأ النمط أبدا.
    let _ = search_scan(display, "", true, &pattern, &opts, line, col)?;
    let max_depth = opts.max_depth.unwrap_or(i64::MAX);
    let root_fs = search_resolve_path(ctx, &path);
    let files = search_walk_files(&path, &root_fs, max_depth, line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for (fs_path, disp) in &files {
        let has = match search_file_has_match(display, fs_path, &pattern, &opts, line, col)? {
            Some(h) => h,
            None => continue,
        };
        if has == want_match {
            out.push(Value::Str(disp.clone()));
        }
    }
    out.sort_by(|a, b| match (a, b) {
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    if opts.quiet {
        return search_maybe_json(Value::Bool(!out.is_empty()), &opts, display, line, col);
    }
    search_maybe_json(Value::List(out), &opts, display, line, col)
}

fn search_files_with(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    search_files_impl(ctx, args, true, "بحث.ملفات_فيها", line, col)
}

fn search_files_without(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    search_files_impl(ctx, args, false, "بحث.ملفات_بلا_مطابقة", line, col)
}

/// مفتاح فرز سجل: (ملف، رقم_السطر)؛ والغائب
/// ملف فارغ ورقم صفر.
fn search_sort_key(
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<(String, f64)> {
    match v {
        Value::Dict(pairs) => {
            let file = match dict_get(pairs, "ملف") {
                None => String::new(),
                Some(Value::Str(s)) => s.clone(),
                Some(_) => {
                    return Err(RioError::runtime(
                        "الدالة بحث.رتب_النتائج تتوقع «ملف» نصا و«رقم_السطر» عددا في السجلات"
                            .to_string(),
                        line,
                        col,
                    ));
                }
            };
            let lineno = match dict_get(pairs, "رقم_السطر") {
                None => 0.0,
                Some(Value::Number(n)) => *n,
                Some(_) => {
                    return Err(RioError::runtime(
                        "الدالة بحث.رتب_النتائج تتوقع «ملف» نصا و«رقم_السطر» عددا في السجلات"
                            .to_string(),
                        line,
                        col,
                    ));
                }
            };
            Ok((file, lineno))
        }
        Value::Str(s) => Ok((s.clone(), 0.0)),
        _ => Err(RioError::runtime(
            "الدالة بحث.رتب_النتائج تتوقع قائمة سجلات أو قائمة نصوص".to_string(),
            line,
            col,
        )),
    }
}

fn search_sort_results(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if args.is_empty() || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة بحث.رتب_النتائج تتوقع معاملا أو معاملين لكن أعطيت {}",
                args.len()
            ),
            line,
            col,
        ));
    }
    let items = match &args[0] {
        Value::List(l) => l.clone(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة بحث.رتب_النتائج تتوقع قائمة لكن أعطيت {}",
                    other.type_name()
                ),
                line,
                col,
            ));
        }
    };
    let descending = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) if s == "تصاعدي" => false,
            Value::Str(s) if s == "تنازلي" => true,
            _ => {
                return Err(RioError::runtime(
                    "الدالة بحث.رتب_النتائج تتوقع اتجاها «تصاعدي» أو «تنازلي»".to_string(),
                    line,
                    col,
                ));
            }
        }
    } else {
        false
    };
    if !items.is_empty() {
        let first_is_dict = matches!(items[0], Value::Dict(_));
        let first_is_str = matches!(items[0], Value::Str(_));
        if !first_is_dict && !first_is_str {
            return Err(RioError::runtime(
                "الدالة بحث.رتب_النتائج تتوقع قائمة سجلات أو قائمة نصوص".to_string(),
                line,
                col,
            ));
        }
        for v in &items {
            if first_is_dict && !matches!(v, Value::Dict(_)) {
                return Err(RioError::runtime(
                    "الدالة بحث.رتب_النتائج تتوقع عناصر من نوع واحد".to_string(),
                    line,
                    col,
                ));
            }
            if first_is_str && !matches!(v, Value::Str(_)) {
                return Err(RioError::runtime(
                    "الدالة بحث.رتب_النتائج تتوقع عناصر من نوع واحد".to_string(),
                    line,
                    col,
                ));
            }
        }
    }
    let mut keyed: Vec<((String, f64), Value)> = Vec::new();
    for v in items {
        let key = search_sort_key(&v, line, col)?;
        keyed.push((key, v));
    }
    keyed.sort_by(|a, b| {
        let ord = a
            .0
            .0
            .cmp(&b.0.0)
            .then(a.0.1.partial_cmp(&b.0.1).unwrap_or(std::cmp::Ordering::Equal));
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });
    Ok(Value::List(keyed.into_iter().map(|(_, v)| v).collect()))
}

const SEARCH_HELP: &str = "بحث — البحث في النصوص والملفات.\nالدوال: ابحث_بنمط(نص، نمط، خيارات؟)؛ ابحث_بنص_ثابت(نص، نص_ثابت، خيارات؟)؛ عد_المطابقات(نص، نمط، خيارات؟)؛ عد_كل_المطابقات(نص، نمط، خيارات؟)؛ المطابق_فقط(نص، نمط، خيارات؟)؛ استبدل_معاينة(نص، نمط، بديل، خيارات؟)؛ ملفات_فيها(مسار، نمط، خيارات؟)؛ ملفات_بلا_مطابقة(مسار، نمط، خيارات؟)؛ رتب_النتائج(نتائج، اتجاه؟)؛ أنماط_من_ملف(مسار)؛ مساعدة().\nالخيارات: تجاهل_الحالة وحساسية_الحالة وحالة_ذكية وكلمة_كاملة وسطر_كامل واعكس_المطابقة وأقصى_مطابقات وعمود_المطابقة ورقم_السطر وسياق_قبل وسياق_بعد وسياق وقلم_النتائج وصامت وأظهر_اسم_الملف وجسون ولوّن وأقصى_عمق.\nاتجاه رتب_النتائج: «تصاعدي» أو «تنازلي» والافتراضي «تصاعدي».\nأقصى_عمق 0 يعني المجلد الأعلى وحده؛ ولوّن يغلّف المطابق بالأحمر؛ والملفات الثنائية أو غير المقروءة تُتخطى.";

fn search_help(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("بحث.مساعدة", args, 0, line, col)?;
    Ok(Value::Str(SEARCH_HELP.to_string()))
}

fn search_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("ابحث_بنمط", native("ابحث_بنمط", search_regex)),
        ("ابحث_بنص_ثابت", native("ابحث_بنص_ثابت", search_literal)),
        ("أنماط_من_ملف", native("أنماط_من_ملف", search_patterns_from_file)),
        ("عد_المطابقات", native("عد_المطابقات", search_count_lines)),
        ("عد_كل_المطابقات", native("عد_كل_المطابقات", search_count_all)),
        ("المطابق_فقط", native("المطابق_فقط", search_only)),
        ("استبدل_معاينة", native("استبدل_معاينة", search_replace_preview)),
        ("ملفات_فيها", native("ملفات_فيها", search_files_with)),
        ("ملفات_بلا_مطابقة", native("ملفات_بلا_مطابقة", search_files_without)),
        ("رتب_النتائج", native("رتب_النتائج", search_sort_results)),
        ("مساعدة", native("مساعدة", search_help)),
    ]
}

// ---------- 221: JSON ----------
// يعيد استعمال محلل device.rs (parse_json + Json +
// json_escape) — فلا محلل جديد؛ وتحويل null إلى لاشيء
// هنا لأن json_to_value في device.rs يرفض null بالتصميم
// (نتائج التحكم لا ترسله أبدا). الحدود: النص مليون
// محرف، وعمق الكتابة 100.

use crate::device::{json_escape, parse_json, Json};

fn json_to_niva(j: Json) -> Value {
    match j {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Bool(b),
        Json::Num(n) => Value::Number(n),
        Json::Str(s) => Value::Str(s),
        Json::Arr(items) => Value::List(items.into_iter().map(json_to_niva).collect()),
        Json::Obj(pairs) => {
            Value::Dict(pairs.into_iter().map(|(k, v)| (k, json_to_niva(v))).collect())
        }
    }
}

/// تنسيق عدد بصيغة JSON الغربية (بلا أرقام عربية).
fn json_fmt_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn json_write_value(v: &Value, out: &mut String, depth: usize) -> Result<(), String> {
    if depth > 100 {
        return Err("قيمة لا تقبل كتابة JSON".to_string());
    }
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if !n.is_finite() {
                return Err("قيمة لا تقبل كتابة JSON".to_string());
            }
            out.push_str(&json_fmt_number(*n));
        }
        Value::Str(s) => out.push_str(&json_escape(s)),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_write_value(item, out, depth + 1)?;
            }
            out.push(']');
        }
        Value::Dict(pairs) => {
            out.push('{');
            for (i, (k, val)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_escape(k));
                out.push(':');
                json_write_value(val, out, depth + 1)?;
            }
            out.push('}');
        }
        _ => return Err("قيمة لا تقبل كتابة JSON".to_string()),
    }
    Ok(())
}

fn json_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اقرأ", native("اقرأ", json_read)),
        ("اكتب", native("اكتب", json_write)),
        ("صالح", native("صالح", json_valid)),
    ]
}

fn json_read(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("جسون.اقرأ", args, 1, line, col)?;
    let text = need_string("جسون.اقرأ", &args[0], line, col)?;
    if text.chars().count() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    match parse_json(&text) {
        Ok(j) => Ok(json_to_niva(j)),
        Err(_) => Err(RioError::runtime("نص JSON غير صالح", line, col)),
    }
}

fn json_write(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("جسون.اكتب", args, 1, line, col)?;
    let mut out = String::new();
    json_write_value(&args[0], &mut out, 0)
        .map_err(|e| RioError::runtime(e, line, col))?;
    Ok(Value::Str(out))
}

fn json_valid(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("جسون.صالح", args, 1, line, col)?;
    let text = need_string("جسون.صالح", &args[0], line, col)?;
    if text.chars().count() > RX_MAX_TEXT {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(parse_json(&text).is_ok()))
}

// ---------- مساعدات مشتركة لوحدات J14 ----------

fn need_dict<'a>(
    display: &str,
    v: &'a Value,
    line: usize,
    col: usize,
) -> RioResult<&'a Vec<(String, Value)>> {
    match v {
        Value::Dict(pairs) => Ok(pairs),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قاموسا لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn dict_get<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// ---------- 222: XML ----------
// محلل XML نقي: عناصر وسمات ونصوص وتعليقات وتعليمات
// معالجة وCDATA وكيانات (&amp; &lt; &gt; &quot; &apos;
// و&#NN; و&#xHH;)؛ وإعلان DOCTYPE بلا قسم داخلي
// يُتخطى (مع قسم داخلي: يُتخطى بتتبع القوسين).
// القيمة: قاموس {وسم، سمات، أبناء، نص}؛ والنص تجميع
// نصوص الأبناء المباشرة كما هي (بلا تقليم). الحدود:
// مليون محرف، وعمق 100، و100 سمة لكل عنصر.

struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

struct XmlParser {
    chars: Vec<char>,
    pos: usize,
}

fn xml_err() -> String {
    "نص XML غير صالح".to_string()
}

impl XmlParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn starts_with(&self, s: &str) -> bool {
        let want: Vec<char> = s.chars().collect();
        if self.pos + want.len() > self.chars.len() {
            return false;
        }
        self.chars[self.pos..self.pos + want.len()] == want[..]
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.starts_with(s) {
            self.pos += s.chars().count();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.next() == Some(c) {
            Ok(())
        } else {
            Err(xml_err())
        }
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn parse_document(&mut self) -> Result<XmlNode, String> {
        loop {
            self.skip_ws();
            if self.eat("<!--") {
                self.skip_comment()?;
            } else if self.eat("<?") {
                self.skip_pi()?;
            } else if self.starts_with("<!DOCTYPE") {
                self.skip_doctype()?;
            } else {
                break;
            }
        }
        let node = self.parse_element(0)?;
        loop {
            self.skip_ws();
            if self.eat("<!--") {
                self.skip_comment()?;
            } else if self.eat("<?") {
                self.skip_pi()?;
            } else {
                break;
            }
        }
        if self.pos != self.chars.len() {
            return Err(xml_err());
        }
        Ok(node)
    }

    fn skip_comment(&mut self) -> Result<(), String> {
        loop {
            if self.starts_with("-->") {
                self.pos += 3;
                return Ok(());
            }
            if self.next().is_none() {
                return Err(xml_err());
            }
        }
    }

    fn skip_pi(&mut self) -> Result<(), String> {
        loop {
            match self.next() {
                None => return Err(xml_err()),
                Some('?') => {
                    if self.eat(">") {
                        return Ok(());
                    }
                }
                Some(_) => {}
            }
        }
    }

    fn skip_doctype(&mut self) -> Result<(), String> {
        // <!DOCTYPE ...> مع تتبع [ ] للقسم الداخلي.
        self.pos += "<!DOCTYPE".len();
        let mut depth = 0usize;
        while let Some(c) = self.next() {
            match c {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                '>' if depth == 0 => return Ok(()),
                _ => {}
            }
        }
        Err(xml_err())
    }

    fn parse_name(&mut self) -> Result<String, String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if c.is_alphabetic() || c == '_' || c == ':' => {
                self.pos += 1;
            }
            _ => return Err(xml_err()),
        }
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn decode_entity(&mut self) -> Result<String, String> {
        // الموضع عند '&' وقد استُهلك.
        let mut body = String::new();
        loop {
            match self.next() {
                None => return Err(xml_err()),
                Some(';') => break,
                Some(c) => {
                    body.push(c);
                    if body.len() > 12 {
                        return Err(xml_err());
                    }
                }
            }
        }
        match body.as_str() {
            "amp" => Ok("&".to_string()),
            "lt" => Ok("<".to_string()),
            "gt" => Ok(">".to_string()),
            "quot" => Ok("\"".to_string()),
            "apos" => Ok("'".to_string()),
            _ => {
                // الجسم بلا '&' (استُهلك قبل الاستدعاء).
                if let Some(num) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                    let code = u32::from_str_radix(num, 16).map_err(|_| xml_err())?;
                    match char::from_u32(code) {
                        Some(c) => Ok(c.to_string()),
                        None => Err(xml_err()),
                    }
                } else if let Some(num) = body.strip_prefix('#') {
                    let code: u32 = num.parse().map_err(|_| xml_err())?;
                    match char::from_u32(code) {
                        Some(c) => Ok(c.to_string()),
                        None => Err(xml_err()),
                    }
                } else {
                    Err(xml_err())
                }
            }
        }
    }

    /// نص حتى '<' مع فك الكيانات.
    fn read_text(&mut self) -> Result<String, String> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '<' {
                break;
            }
            self.pos += 1;
            if c == '&' {
                out.push_str(&self.decode_entity()?);
            } else {
                out.push(c);
            }
        }
        Ok(out)
    }

    fn parse_element(&mut self, depth: usize) -> Result<XmlNode, String> {
        if depth > 100 {
            return Err(xml_err());
        }
        if !self.eat("<") {
            return Err(xml_err());
        }
        let name = self.parse_name()?;
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some('/') => {
                    if self.eat("/>") {
                        return Ok(XmlNode {
                            name,
                            attrs,
                            children: vec![],
                            text: String::new(),
                        });
                    }
                    return Err(xml_err());
                }
                Some(_) => {
                    let aname = self.parse_name()?;
                    if attrs.iter().any(|(k, _)| *k == aname) {
                        return Err(xml_err());
                    }
                    self.skip_ws();
                    self.expect('=')?;
                    self.skip_ws();
                    let quote = match self.next() {
                        Some('"') => '"',
                        Some('\'') => '\'',
                        _ => return Err(xml_err()),
                    };
                    let mut value = String::new();
                    loop {
                        match self.next() {
                            None => return Err(xml_err()),
                            Some(c) if c == quote => break,
                            Some('&') => value.push_str(&self.decode_entity()?),
                            Some('<') => return Err(xml_err()),
                            Some(c) => value.push(c),
                        }
                    }
                    attrs.push((aname, value));
                    if attrs.len() > 100 {
                        return Err(xml_err());
                    }
                }
                None => return Err(xml_err()),
            }
        }
        let mut children: Vec<XmlNode> = Vec::new();
        let mut text = String::new();
        loop {
            if self.pos >= self.chars.len() {
                return Err(xml_err());
            }
            if self.starts_with("</") {
                self.pos += 2;
                let end = self.parse_name()?;
                if end != name {
                    return Err(xml_err());
                }
                self.skip_ws();
                self.expect('>')?;
                break;
            }
            if self.eat("<!--") {
                self.skip_comment()?;
                continue;
            }
            if self.starts_with("<![CDATA[") {
                self.pos += "<![CDATA[".len();
                loop {
                    if self.starts_with("]]>") {
                        self.pos += 3;
                        break;
                    }
                    match self.next() {
                        None => return Err(xml_err()),
                        Some(c) => text.push(c),
                    }
                }
                continue;
            }
            if self.eat("<?") {
                self.skip_pi()?;
                continue;
            }
            if self.peek() == Some('<') {
                children.push(self.parse_element(depth + 1)?);
                continue;
            }
            text.push_str(&self.read_text()?);
        }
        Ok(XmlNode {
            name,
            attrs,
            children,
            text,
        })
    }
}

fn xml_node_to_value(n: XmlNode) -> Value {
    Value::Dict(vec![
        ("وسم".to_string(), Value::Str(n.name)),
        (
            "سمات".to_string(),
            Value::Dict(
                n.attrs
                    .into_iter()
                    .map(|(k, v)| (k, Value::Str(v)))
                    .collect(),
            ),
        ),
        (
            "أبناء".to_string(),
            Value::List(n.children.into_iter().map(xml_node_to_value).collect()),
        ),
        ("نص".to_string(), Value::Str(n.text)),
    ])
}

fn xml_escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn xml_escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn xml_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn xml_write_value(v: &Value, out: &mut String, depth: usize) -> Result<(), String> {
    if depth > 100 {
        return Err("قيمة XML غير صالحة".to_string());
    }
    let pairs = match v {
        Value::Dict(pairs) => pairs,
        _ => return Err("قيمة XML غير صالحة".to_string()),
    };
    let name = match dict_get(pairs, "وسم") {
        Some(Value::Str(s)) if xml_valid_name(s) => s.clone(),
        _ => return Err("قيمة XML غير صالحة".to_string()),
    };
    let attrs: Vec<(String, String)> = match dict_get(pairs, "سمات") {
        None => vec![],
        Some(Value::Dict(apairs)) => {
            let mut list = Vec::new();
            for (k, val) in apairs {
                match val {
                    Value::Str(s) if xml_valid_name(k) => list.push((k.clone(), s.clone())),
                    _ => return Err("قيمة XML غير صالحة".to_string()),
                }
            }
            list
        }
        Some(_) => return Err("قيمة XML غير صالحة".to_string()),
    };
    let children: Vec<Value> = match dict_get(pairs, "أبناء") {
        None => vec![],
        Some(Value::List(items)) => items.clone(),
        Some(_) => return Err("قيمة XML غير صالحة".to_string()),
    };
    let text = match dict_get(pairs, "نص") {
        None => String::new(),
        Some(Value::Str(s)) => s.clone(),
        Some(_) => return Err("قيمة XML غير صالحة".to_string()),
    };
    out.push('<');
    out.push_str(&name);
    for (k, val) in &attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        xml_escape_attr(val, out);
        out.push('"');
    }
    if children.is_empty() && text.is_empty() {
        out.push_str("/>");
        return Ok(());
    }
    out.push('>');
    for child in &children {
        xml_write_value(child, out, depth + 1)?;
    }
    xml_escape_text(&text, out);
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
    Ok(())
}

fn xml_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اقرأ", native("اقرأ", xml_read)),
        ("اكتب", native("اكتب", xml_write)),
    ]
}

fn xml_read(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكسمل.اقرأ", args, 1, line, col)?;
    let text = need_string("اكسمل.اقرأ", &args[0], line, col)?;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    let mut p = XmlParser { chars, pos: 0 };
    match p.parse_document() {
        Ok(node) => Ok(xml_node_to_value(node)),
        Err(_) => Err(RioError::runtime("نص XML غير صالح", line, col)),
    }
}

fn xml_write(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكسمل.اكتب", args, 1, line, col)?;
    let mut out = String::new();
    xml_write_value(&args[0], &mut out, 0)
        .map_err(|e| RioError::runtime(e, line, col))?;
    Ok(Value::Str(out))
}

// ---------- 223: YAML ----------
// مجموعة فرعية موثقة: خرائط وقوائم بالمسافة البادئة
// (مسافات فقط)، وقيم نصية وعددية ومنطقية (صحيح/خطأ
// وtrue/false) وفارغة (لاشيء/null/~/فارغ)، وسلاسل
// مفردة ومزدوجة التنصيص، وتدفق [..] و{..} في سطر
// واحد، وكتل نصية | و> (مع - و+)، وتعليقات #،
// وعلامتا --- و... . خارج المجموعة: المراسي (& و*)،
// والوسوم (!)، والمفاتيح المعقدة (؟)، ومسافات
// الجدولة، ومتعدد الأسطر العادي بلا كتلة، والقيم
// المضمّنة بعد ---، والجذر بمحاذاة غير صفر،
// وهروب \u ببديلين في التنصيص المزدوج. والكتابة
// بمسافة بادئة مقدارها مسافتان؛ والمنطقي يُكتب
// صحيح/خطأ والفارغ لاشيء (وتُقرأ الصيغتان).

fn yaml_err() -> String {
    "نص YAML غير صالح".to_string()
}

#[derive(Clone)]
struct YamlLine {
    indent: usize,
    text: String,
    blank: bool,
}

/// قطع التعليق خارج التنصيص (# في البداية أو بعد مسافة).
fn yaml_strip_comment(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if in_single {
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
        } else if in_double {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
        } else if c == '\'' {
            in_single = true;
        } else if c == '"' {
            in_double = true;
        } else if c == '#' && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') {
            return chars[..i].iter().collect::<String>().trim_end().to_string();
        }
        i += 1;
    }
    s.trim_end().to_string()
}

fn yaml_split_lines(src: &str) -> Result<Vec<YamlLine>, String> {
    let mut lines = Vec::new();
    for raw in src.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let mut indent = 0usize;
        let mut has_tab = false;
        for c in raw.chars() {
            if c == ' ' {
                indent += 1;
            } else if c == '\t' {
                has_tab = true;
                break;
            } else {
                break;
            }
        }
        if has_tab {
            return Err(yaml_err());
        }
        let content = &raw[indent..];
        if content.trim().is_empty() {
            lines.push(YamlLine {
                indent: 0,
                text: String::new(),
                blank: true,
            });
            continue;
        }
        let text = yaml_strip_comment(content);
        if text.trim().is_empty() {
            lines.push(YamlLine {
                indent: 0,
                text: String::new(),
                blank: true,
            });
        } else {
            lines.push(YamlLine {
                indent,
                text,
                blank: false,
            });
        }
    }
    Ok(lines)
}

struct YamlParser {
    lines: Vec<YamlLine>,
    pos: usize,
}

/// أهو سطر عنصر قائمة ("- " أو "-")؟
fn yaml_is_dash(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn yaml_dash_rest(text: &str) -> &str {
    if text == "-" {
        ""
    } else {
        &text[2..]
    }
}

fn yaml_is_block_header(s: &str) -> bool {
    matches!(s, "|" | "|-" | "|+" | ">" | ">-" | ">+")
}

/// أول ':' فاصل (خارج التنصيص والتدفق، متبوعا بمسافة أو نهاية).
fn yaml_split_key(text: &str) -> Option<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if in_single {
            if c == '\'' {
                if chars.get(i + 1) == Some(&'\'') {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
        } else if in_double {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
        } else if c == '\'' {
            in_single = true;
        } else if c == '"' {
            in_double = true;
        } else if c == '[' || c == '{' {
            depth += 1;
        } else if c == ']' || c == '}' {
            depth = depth.saturating_sub(1);
        } else if c == ':' && depth == 0 && (i + 1 >= chars.len() || chars[i + 1] == ' ') {
            let key: String = chars[..i].iter().collect();
            let rest: String = chars[i + 1..].iter().collect();
            return Some((key.trim().to_string(), rest.trim().to_string()));
        }
        i += 1;
    }
    None
}

fn yaml_is_map_line(text: &str) -> bool {
    yaml_split_key(text).is_some()
}

/// فك تنصيص مفرد: '' ← '.
fn yaml_unquote_single(s: &str) -> Result<String, String> {
    if s.len() < 2 || !s.starts_with('\'') || !s.ends_with('\'') {
        return Err(yaml_err());
    }
    let inner = &s[1..s.len() - 1];
    Ok(inner.replace("''", "'"))
}

/// فك تنصيص مزدوج (الهروب الشائع؛ و\u بنقطة واحدة).
fn yaml_unquote_double(s: &str) -> Result<String, String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 2 || chars[0] != '"' || chars[chars.len() - 1] != '"' {
        return Err(yaml_err());
    }
    let mut out = String::new();
    let mut i = 1usize;
    while i + 1 < chars.len() + 1 && i < chars.len() - 1 + 1 {
        if i >= chars.len() - 1 {
            break;
        }
        let c = chars[i];
        if c == '\\' {
            i += 1;
            if i >= chars.len() - 1 {
                return Err(yaml_err());
            }
            let e = chars[i];
            match e {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000C}'),
                'u' => {
                    let mut code: u32 = 0;
                    for _ in 0..4 {
                        i += 1;
                        if i >= chars.len() - 1 {
                            return Err(yaml_err());
                        }
                        let d = chars[i].to_digit(16).ok_or_else(yaml_err)?;
                        code = code * 16 + d;
                    }
                    match char::from_u32(code) {
                        Some(c) if !(0xD800..0xE000).contains(&code) => out.push(c),
                        _ => return Err(yaml_err()),
                    }
                }
                _ => return Err(yaml_err()),
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    Ok(out)
}

fn yaml_parse_key(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err(yaml_err());
    }
    if s.starts_with('\'') {
        return yaml_unquote_single(s);
    }
    if s.starts_with('"') {
        return yaml_unquote_double(s);
    }
    Ok(s.to_string())
}

fn yaml_is_integer(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
}

fn yaml_is_float(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    if t.is_empty() {
        return false;
    }
    let (mant, exp) = match t.find(['e', 'E']) {
        Some(idx) => (&t[..idx], Some(&t[idx + 1..])),
        None => (t, None),
    };
    if let Some(e) = exp {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !e.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    let has_dot = mant.contains('.');
    let has_exp = exp.is_some();
    if !has_dot && !has_exp {
        return false;
    }
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if has_dot {
        let parts: Vec<&str> = mant.split('.').collect();
        if parts.len() != 2 {
            return false;
        }
    }
    true
}

fn yaml_parse_scalar(s: &str) -> Result<Value, String> {
    let s = s.trim();
    match s {
        "لاشيء" | "null" | "Null" | "NULL" | "~" => return Ok(Value::Nil),
        "صحيح" | "true" | "True" | "TRUE" => return Ok(Value::Bool(true)),
        "خطأ" | "false" | "False" | "FALSE" => return Ok(Value::Bool(false)),
        _ => {}
    }
    if s.starts_with('\'') {
        return Ok(Value::Str(yaml_unquote_single(s)?));
    }
    if s.starts_with('"') {
        return Ok(Value::Str(yaml_unquote_double(s)?));
    }
    if yaml_is_integer(s) {
        let n: f64 = s.parse().map_err(|_| yaml_err())?;
        if n.is_finite() {
            return Ok(Value::Number(n));
        }
        return Err(yaml_err());
    }
    if yaml_is_float(s) {
        let n: f64 = s.parse().map_err(|_| yaml_err())?;
        if n.is_finite() {
            return Ok(Value::Number(n));
        }
        return Err(yaml_err());
    }
    if s.is_empty()
        || s.starts_with(['!', '&', '*', '?', '|', '>', '%', '@', '`', '[', ']', '{', '}', ','])
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.chars().any(|c| (c as u32) < 0x20)
    {
        return Err(yaml_err());
    }
    Ok(Value::Str(s.to_string()))
}

/// محلل تدفق [..] و{..} على محارف.
struct YamlFlow {
    chars: Vec<char>,
    pos: usize,
}

impl YamlFlow {
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
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.next() == Some(c) {
            Ok(())
        } else {
            Err(yaml_err())
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some('[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                loop {
                    items.push(self.parse_value()?);
                    self.skip_ws();
                    match self.next() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err(yaml_err()),
                    }
                }
                Ok(Value::List(items))
            }
            Some('{') => {
                self.pos += 1;
                let mut pairs: Vec<(String, Value)> = Vec::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Value::Dict(pairs));
                }
                loop {
                    self.skip_ws();
                    let key = self.parse_flow_key()?;
                    self.skip_ws();
                    self.expect(':')?;
                    let val = self.parse_value()?;
                    if pairs.iter().any(|(k, _)| *k == key) {
                        return Err(yaml_err());
                    }
                    pairs.push((key, val));
                    self.skip_ws();
                    match self.next() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err(yaml_err()),
                    }
                }
                Ok(Value::Dict(pairs))
            }
            Some('"') | Some('\'') => {
                let text = self.parse_quoted()?;
                Ok(Value::Str(text))
            }
            Some(_) => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == ',' || c == ']' || c == '}' {
                        break;
                    }
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                let text = text.trim();
                if text.is_empty() {
                    return Err(yaml_err());
                }
                yaml_parse_scalar(text)
            }
            None => Err(yaml_err()),
        }
    }

    fn parse_flow_key(&mut self) -> Result<String, String> {
        self.skip_ws();
        match self.peek() {
            Some('"') | Some('\'') => self.parse_quoted(),
            _ => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == ':' || c == ',' || c == '}' {
                        break;
                    }
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                yaml_parse_key(text.trim())
            }
        }
    }

    fn parse_quoted(&mut self) -> Result<String, String> {
        let quote = self.next().ok_or_else(yaml_err)?;
        if quote == '\'' {
            let mut out = String::new();
            loop {
                match self.next() {
                    None => return Err(yaml_err()),
                    Some('\'') => {
                        if self.peek() == Some('\'') {
                            self.pos += 1;
                            out.push('\'');
                        } else {
                            break;
                        }
                    }
                    Some(c) => out.push(c),
                }
            }
            Ok(out)
        } else {
            let mut out = String::new();
            loop {
                match self.next() {
                    None => return Err(yaml_err()),
                    Some('"') => break,
                    Some('\\') => {
                        let e = self.next().ok_or_else(yaml_err)?;
                        match e {
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            '/' => out.push('/'),
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            'b' => out.push('\u{0008}'),
                            'f' => out.push('\u{000C}'),
                            'u' => {
                                let mut code: u32 = 0;
                                for _ in 0..4 {
                                    let d = self
                                        .next()
                                        .and_then(|c| c.to_digit(16))
                                        .ok_or_else(yaml_err)?;
                                    code = code * 16 + d;
                                }
                                match char::from_u32(code) {
                                    Some(c) if !(0xD800..0xE000).contains(&code) => {
                                        out.push(c)
                                    }
                                    _ => return Err(yaml_err()),
                                }
                            }
                            _ => return Err(yaml_err()),
                        }
                    }
                    Some(c) => out.push(c),
                }
            }
            Ok(out)
        }
    }
}

fn yaml_parse_inline(s: &str) -> Result<Value, String> {
    let s = s.trim();
    if s.starts_with('[') || s.starts_with('{') {
        let mut flow = YamlFlow {
            chars: s.chars().collect(),
            pos: 0,
        };
        let v = flow.parse_value()?;
        flow.skip_ws();
        if flow.pos != flow.chars.len() {
            return Err(yaml_err());
        }
        return Ok(v);
    }
    yaml_parse_scalar(s)
}

fn yaml_chomp(s: &str, chomp: char) -> String {
    match chomp {
        '-' => s.trim_end_matches('\n').to_string(),
        '+' => s.to_string(),
        _ => {
            let t = s.trim_end_matches('\n');
            if t.is_empty() {
                String::new()
            } else {
                format!("{}\n", t)
            }
        }
    }
}

impl YamlParser {
    /// أول سطر غير فارغ من الموضع الحالي.
    fn cur(&self) -> Option<&YamlLine> {
        let mut i = self.pos;
        while i < self.lines.len() {
            if !self.lines[i].blank {
                return Some(&self.lines[i]);
            }
            i += 1;
        }
        None
    }

    fn skip_blanks(&mut self) {
        while self.pos < self.lines.len() && self.lines[self.pos].blank {
            self.pos += 1;
        }
    }

    fn parse_document(&mut self) -> Result<Value, String> {
        self.skip_blanks();
        if self.pos >= self.lines.len() {
            return Ok(Value::Nil);
        }
        let first = self.lines[self.pos].text.clone();
        if first == "---" {
            self.pos += 1;
            self.skip_blanks();
            if self.pos >= self.lines.len() {
                return Ok(Value::Nil);
            }
        } else if first.starts_with("--- ") {
            return Err(yaml_err());
        }
        let indent = self.cur().map(|l| l.indent).unwrap_or(0);
        if indent != 0 {
            return Err(yaml_err());
        }
        let v = self.parse_block(0, 0)?;
        self.skip_blanks();
        if self.pos < self.lines.len() {
            if self.lines[self.pos].text == "..." {
                self.pos += 1;
                self.skip_blanks();
            }
            if self.pos < self.lines.len() {
                return Err(yaml_err());
            }
        }
        Ok(v)
    }

    fn parse_block(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        if depth > 50 {
            return Err(yaml_err());
        }
        let Some(line) = self.cur() else {
            return Ok(Value::Nil);
        };
        if line.indent < indent {
            return Ok(Value::Nil);
        }
        if line.indent > indent {
            return Err(yaml_err());
        }
        if yaml_is_dash(&line.text) {
            return self.parse_seq(indent, depth);
        }
        if yaml_is_map_line(&line.text) {
            return self.parse_map(indent, depth);
        }
        let text = line.text.clone();
        self.skip_blanks();
        self.pos += 1;
        yaml_parse_inline(&text)
    }

    fn parse_seq(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let mut items: Vec<Value> = Vec::new();
        loop {
            self.skip_blanks();
            let Some(line) = self.cur() else {
                break;
            };
            if line.indent != indent || !yaml_is_dash(&line.text) {
                break;
            }
            if line.text == "..." {
                break;
            }
            let rest = yaml_dash_rest(&line.text).trim().to_string();
            if rest.is_empty() {
                self.pos += 1;
                self.skip_blanks();
                let v = match self.cur() {
                    Some(l) if l.indent > indent => {
                        let ni = l.indent;
                        self.parse_block(ni, depth + 1)?
                    }
                    _ => Value::Nil,
                };
                items.push(v);
            } else if yaml_is_block_header(&rest) {
                self.pos += 1;
                items.push(self.parse_block_scalar(&rest, indent)?);
            } else {
                // سطر اصطناعي بمحاذاة indent+2 يحمل بقية السطر.
                self.lines[self.pos].indent = indent + 2;
                self.lines[self.pos].text = rest;
                let v = self.parse_block(indent + 2, depth + 1)?;
                items.push(v);
            }
        }
        Ok(Value::List(items))
    }

    fn parse_map(&mut self, indent: usize, depth: usize) -> Result<Value, String> {
        let mut pairs: Vec<(String, Value)> = Vec::new();
        loop {
            self.skip_blanks();
            let Some(line) = self.cur() else {
                break;
            };
            if line.indent != indent || yaml_is_dash(&line.text) {
                break;
            }
            if line.text == "..." {
                break;
            }
            let (key, rest) = match yaml_split_key(&line.text) {
                Some(kv) => kv,
                None => return Err(yaml_err()),
            };
            let key = yaml_parse_key(&key)?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(yaml_err());
            }
            if rest.is_empty() {
                self.pos += 1;
                self.skip_blanks();
                let v = match self.cur() {
                    Some(l) if l.indent > indent => {
                        let ni = l.indent;
                        self.parse_block(ni, depth + 1)?
                    }
                    _ => Value::Nil,
                };
                pairs.push((key, v));
            } else if yaml_is_block_header(&rest) {
                self.pos += 1;
                let v = self.parse_block_scalar(&rest, indent)?;
                pairs.push((key, v));
            } else {
                self.pos += 1;
                let v = yaml_parse_inline(&rest)?;
                pairs.push((key, v));
            }
        }
        Ok(Value::Dict(pairs))
    }

    fn parse_block_scalar(&mut self, header: &str, parent: usize) -> Result<Value, String> {
        let folded = header.starts_with('>');
        let chomp = if header.contains('-') {
            '-'
        } else if header.contains('+') {
            '+'
        } else {
            ' '
        };
        let mut raw: Vec<String> = Vec::new();
        let mut content_indent: Option<usize> = None;
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if line.blank {
                raw.push(String::new());
                self.pos += 1;
                continue;
            }
            if line.indent <= parent {
                break;
            }
            let ci = *content_indent.get_or_insert(line.indent);
            if line.indent < ci {
                break;
            }
            let prefix = " ".repeat(line.indent - ci);
            raw.push(format!("{}{}", prefix, line.text));
            self.pos += 1;
        }
        // الفراغات البادئة (أسطر فارغة قبل أول سطر محتوى)
        // محتوى في YAML فتُبقى كما هي.
        let body = if folded {
            let mut out = String::new();
            for (i, line) in raw.iter().enumerate() {
                if line.is_empty() {
                    out.push('\n');
                    continue;
                }
                out.push_str(line);
                if i + 1 >= raw.len() {
                    out.push('\n');
                } else if raw[i + 1].is_empty() {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            out
        } else {
            let mut out = String::new();
            for line in &raw {
                out.push_str(line);
                out.push('\n');
            }
            out
        };
        Ok(Value::Str(yaml_chomp(&body, chomp)))
    }
}

/// هل النص آمن كنص عادي في YAML (بلا تنصيص)؟
fn yaml_plain_safe(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s == "-" || s.starts_with("- ") {
        return false;
    }
    if s.starts_with(['!', '&', '*', '?', '|', '>', '%', '@', '`', '"', '\'', '#', '[', ']', '{', '}', ',']) {
        return false;
    }
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') {
        return false;
    }
    if s.starts_with(' ') || s.ends_with(' ') {
        return false;
    }
    if s.chars().any(|c| (c as u32) < 0x20) {
        return false;
    }
    if s == "لاشيء" || s == "null" || s == "Null" || s == "NULL" || s == "~" {
        return false;
    }
    if s == "صحيح" || s == "خطأ" || s == "true" || s == "True" || s == "TRUE" || s == "false" || s == "False" || s == "FALSE" {
        return false;
    }
    if yaml_is_integer(s) || yaml_is_float(s) {
        return false;
    }
    true
}

fn yaml_double_quote(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn yaml_quote_scalar(s: &str) -> String {
    if yaml_plain_safe(s) {
        s.to_string()
    } else {
        yaml_double_quote(s)
    }
}

fn yaml_inline(v: &Value) -> Result<String, String> {
    match v {
        Value::Nil => Ok("لاشيء".to_string()),
        Value::Bool(b) => Ok(if *b {
            "صحيح".to_string()
        } else {
            "خطأ".to_string()
        }),
        Value::Number(n) => {
            if !n.is_finite() {
                return Err("قيمة YAML غير صالحة".to_string());
            }
            Ok(json_fmt_number(*n))
        }
        Value::Str(s) => Ok(yaml_quote_scalar(s)),
        Value::List(items) if items.is_empty() => Ok("[]".to_string()),
        Value::Dict(pairs) if pairs.is_empty() => Ok("{}".to_string()),
        _ => Err("قيمة YAML غير صالحة".to_string()),
    }
}

fn yaml_is_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Nil | Value::Bool(_) | Value::Number(_) | Value::Str(_)
    )
}

fn yaml_block(v: &Value, indent: usize, depth: usize) -> Result<String, String> {
    if depth > 50 {
        return Err("قيمة YAML غير صالحة".to_string());
    }
    match v {
        Value::List(items) => {
            if items.is_empty() {
                return Ok("[]".to_string());
            }
            let mut lines: Vec<String> = Vec::new();
            let pad = " ".repeat(indent);
            for item in items {
                if yaml_is_scalar(item)
                    || matches!(item, Value::List(i) if i.is_empty())
                    || matches!(item, Value::Dict(p) if p.is_empty())
                {
                    lines.push(format!("{}- {}", pad, yaml_inline(item)?));
                } else {
                    let inner = yaml_block(item, indent + 2, depth + 1)?;
                    let mut parts = inner.splitn(2, '\n');
                    let first = parts.next().unwrap_or("");
                    let first = first
                        .strip_prefix(&" ".repeat(indent + 2))
                        .unwrap_or(first);
                    lines.push(format!("{}- {}", pad, first));
                    if let Some(rest) = parts.next() {
                        lines.push(rest.to_string());
                    }
                }
            }
            Ok(lines.join("\n"))
        }
        Value::Dict(pairs) => {
            if pairs.is_empty() {
                return Ok("{}".to_string());
            }
            let mut lines: Vec<String> = Vec::new();
            let pad = " ".repeat(indent);
            for (k, val) in pairs {
                let key = if yaml_plain_safe(k) && !k.contains(": ") && !k.ends_with(':') {
                    k.clone()
                } else {
                    yaml_double_quote(k)
                };
                if yaml_is_scalar(val)
                    || matches!(val, Value::List(i) if i.is_empty())
                    || matches!(val, Value::Dict(p) if p.is_empty())
                {
                    lines.push(format!("{}{}: {}", pad, key, yaml_inline(val)?));
                } else {
                    lines.push(format!("{}{}:", pad, key));
                    lines.push(yaml_block(val, indent + 2, depth + 1)?);
                }
            }
            Ok(lines.join("\n"))
        }
        _ => yaml_inline(v),
    }
}

fn yaml_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اقرأ", native("اقرأ", yaml_read)),
        ("اكتب", native("اكتب", yaml_write)),
    ]
}

fn yaml_read(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("يامال.اقرأ", args, 1, line, col)?;
    let text = need_string("يامال.اقرأ", &args[0], line, col)?;
    if text.chars().count() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    let lines = yaml_split_lines(&text).map_err(|e| RioError::runtime(e, line, col))?;
    let mut p = YamlParser { lines, pos: 0 };
    p.parse_document()
        .map_err(|e| RioError::runtime(e, line, col))
}

fn yaml_write(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("يامال.اكتب", args, 1, line, col)?;
    let mut out = yaml_block(&args[0], 0, 0).map_err(|e| RioError::runtime(e, line, col))?;
    out.push('\n');
    Ok(Value::Str(out))
}

// ---------- 224: القوالب ----------
// محرك قوالب نصية: {{اسم}} و{{مستخدم.اسم}}، وشرط
// {{إذا اسم}}...{{وإلا}}...{{/إذا}} (وليس للنفي)،
// وتكرار {{لكل عنصر في قائمة}}...{{/لكل}} مع {{مؤشر}}
// للفهرس، وهروب \{{ للقوسين حرفيا. القيم تُعرض كما
// يطبعها اطبع. حدود: القالب 100 ألف محرف، وتداخل
// 20، والمخرج مليون محرف. وسم متغير غير موجود خطأ
// عربي يسمّيه؛ أما في الشرط فالغائب قيمة خاطئة
// (فـ`ليس غائب` صحيحة)؛ والتكرار على غير قائمة خطأ عربي.

const TPL_MAX_CHARS: usize = 100_000;
const TPL_MAX_OUT: usize = 1_000_000;
const TPL_MAX_DEPTH: usize = 20;

#[derive(Clone, Debug)]
enum TplTok {
    Text(String),
    Tag(String),
}

fn tpl_tokenize(tpl: &str) -> Result<Vec<TplTok>, String> {
    let chars: Vec<char> = tpl.chars().collect();
    let mut toks = Vec::new();
    let mut text = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' && i + 2 < chars.len() && chars[i + 1] == '{' && chars[i + 2] == '{' {
            text.push_str("{{");
            i += 3;
            continue;
        }
        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
            let mut j = i + 2;
            let mut found = false;
            while j + 1 < chars.len() {
                if chars[j] == '}' && chars[j + 1] == '}' {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err("قالب غير صالح".to_string());
            }
            if !text.is_empty() {
                toks.push(TplTok::Text(std::mem::take(&mut text)));
            }
            let tag: String = chars[i + 2..j].iter().collect();
            let tag = tag.trim().to_string();
            if tag.is_empty() {
                return Err("قالب غير صالح".to_string());
            }
            toks.push(TplTok::Tag(tag));
            i = j + 2;
            continue;
        }
        text.push(chars[i]);
        i += 1;
    }
    if !text.is_empty() {
        toks.push(TplTok::Text(text));
    }
    Ok(toks)
}

fn tpl_push(out: &mut String, s: &str) -> Result<(), String> {
    if out.len() + s.len() > TPL_MAX_OUT {
        return Err("القالب كبير جدا".to_string());
    }
    out.push_str(s);
    Ok(())
}

fn tpl_lookup_path(path: &str, scopes: &[Vec<(String, Value)>]) -> Option<Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut cur: Option<Value> = None;
    for scope in scopes.iter().rev() {
        if let Some((_, v)) = scope.iter().find(|(k, _)| k == first) {
            cur = Some(v.clone());
            break;
        }
    }
    let mut cur = cur?;
    for part in parts {
        match cur {
            Value::Dict(ref pairs) => {
                cur = dict_get(pairs, part)?.clone();
            }
            _ => return None,
        }
    }
    Some(cur)
}

fn tpl_truthy(v: &Value) -> bool {
    match v {
        Value::Nil => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::List(items) => !items.is_empty(),
        Value::Dict(pairs) => !pairs.is_empty(),
        _ => true,
    }
}

/// شرط قالب: المتغير الغائب قيمة خاطئة (فـ`ليس غائب`
/// صحيحة) — أما وسم المتغير نفسه فالغائب فيه خطأ.
fn tpl_eval_cond(cond: &str, scopes: &[Vec<(String, Value)>]) -> Result<bool, String> {
    let cond = cond.trim();
    if let Some(rest) = cond.strip_prefix("ليس ") {
        let rest = rest.trim();
        let v = tpl_lookup_path(rest, scopes);
        return Ok(!v.map(|v| tpl_truthy(&v)).unwrap_or(false));
    }
    let v = tpl_lookup_path(cond, scopes);
    Ok(v.map(|v| tpl_truthy(&v)).unwrap_or(false))
}

/// اعثر على وسم الإغلاق المطابق من `i` (يعدّ الفتحات
/// المتداخلة من النوع نفسه)؛ يعيد فهرسه.
fn tpl_find_end(toks: &[TplTok], mut i: usize, end: usize, kind: &str) -> Result<usize, String> {
    let open_if = format!("{} ", "إذا");
    let open_each = format!("{} ", "لكل");
    let close = format!("/{}", kind);
    let mut depth = 0usize;
    while i < end {
        if let TplTok::Tag(tag) = &toks[i] {
            let tag = tag.trim();
            if kind == "إذا" && (tag == "إذا" || tag.starts_with(&open_if)) {
                depth += 1;
            } else if kind == "لكل" && (tag == "لكل" || tag.starts_with(&open_each)) {
                depth += 1;
            } else if tag == close {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
        }
        i += 1;
    }
    Err("قالب غير صالح".to_string())
}

/// اعرض الرموز [i, end)؛ وتوقف عند وسم إنهاء (وإلا
/// أو /إذا أو /لكل) فأعده مع فهرسه.
fn tpl_render_range(
    toks: &[TplTok],
    mut i: usize,
    end: usize,
    scopes: &mut Vec<Vec<(String, Value)>>,
    out: &mut String,
    depth: usize,
) -> Result<(usize, Option<String>), String> {
    if depth > TPL_MAX_DEPTH {
        return Err("القالب كبير جدا".to_string());
    }
    while i < end {
        match &toks[i] {
            TplTok::Text(s) => {
                let s = s.clone();
                tpl_push(out, &s)?;
                i += 1;
            }
            TplTok::Tag(raw) => {
                let tag = raw.trim().to_string();
                if tag == "وإلا" || tag == "/إذا" || tag == "/لكل" {
                    return Ok((i, Some(tag)));
                }
                if tag == "إذا" || tag.starts_with("إذا ") {
                    let cond = tag.strip_prefix("إذا").unwrap_or("").trim();
                    if cond.is_empty() {
                        return Err("قالب غير صالح".to_string());
                    }
                    let val = tpl_eval_cond(cond, scopes)?;
                    i += 1;
                    let (ni, term) = tpl_render_range(toks, i, end, scopes, out, depth + 1)?;
                    i = ni;
                    match term.as_deref() {
                        Some("وإلا") => {
                            if val {
                                i = tpl_find_end(toks, i + 1, end, "إذا")? + 1;
                            } else {
                                let (ni2, term2) =
                                    tpl_render_range(toks, i + 1, end, scopes, out, depth + 1)?;
                                if term2.as_deref() != Some("/إذا") {
                                    return Err("قالب غير صالح".to_string());
                                }
                                i = ni2 + 1;
                            }
                        }
                        Some("/إذا") => {
                            i += 1;
                        }
                        _ => return Err("قالب غير صالح".to_string()),
                    }
                    continue;
                }
                if tag == "لكل" || tag.starts_with("لكل ") {
                    let rest = tag.strip_prefix("لكل").unwrap_or("").trim();
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if parts.len() != 3 || parts[1] != "في" {
                        return Err("قالب غير صالح".to_string());
                    }
                    let var = parts[0].to_string();
                    let path = parts[2].to_string();
                    let list = tpl_lookup_path(&path, scopes)
                        .ok_or_else(|| format!("متغير قالب غير موجود: {}", path))?;
                    let items = match list {
                        Value::List(items) => items,
                        _ => return Err("قيمة قالب غير صالحة".to_string()),
                    };
                    let body_end = tpl_find_end(toks, i + 1, end, "لكل")?;
                    for (idx, item) in items.iter().enumerate() {
                        scopes.push(vec![
                            (var.clone(), item.clone()),
                            ("مؤشر".to_string(), Value::Number(idx as f64)),
                        ]);
                        let (_, term) =
                            tpl_render_range(toks, i + 1, body_end, scopes, out, depth + 1)?;
                        scopes.pop();
                        if term.is_some() {
                            return Err("قالب غير صالح".to_string());
                        }
                    }
                    i = body_end + 1;
                    continue;
                }
                if tag.starts_with('/') {
                    return Err("قالب غير صالح".to_string());
                }
                let v = tpl_lookup_path(&tag, scopes)
                    .ok_or_else(|| format!("متغير قالب غير موجود: {}", tag))?;
                let s = v.fmt_value();
                tpl_push(out, &s)?;
                i += 1;
            }
        }
    }
    Ok((i, None))
}

fn template_entries() -> Vec<(&'static str, Value)> {
    vec![("طبق", native("طبق", template_apply))]
}

fn template_apply(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوالب.طبق", args, 2, line, col)?;
    let tpl = need_string("قوالب.طبق", &args[0], line, col)?;
    let pairs = need_dict("قوالب.طبق", &args[1], line, col)?;
    if tpl.chars().count() > TPL_MAX_CHARS {
        return Err(RioError::runtime("القالب كبير جدا", line, col));
    }
    let toks = tpl_tokenize(&tpl).map_err(|e| RioError::runtime(e, line, col))?;
    let mut scopes: Vec<Vec<(String, Value)>> = vec![pairs.clone()];
    let mut out = String::new();
    let (ni, term) = tpl_render_range(&toks, 0, toks.len(), &mut scopes, &mut out, 0)
        .map_err(|e| RioError::runtime(e, line, col))?;
    if term.is_some() || ni != toks.len() {
        return Err(RioError::runtime("قالب غير صالح", line, col));
    }
    Ok(Value::Str(out))
}

// ---------- 213: التحقق ----------
// دوال تحقق مبنية على محرك التعابير النمطية نفسه
// (البند 220) — لا منطق مطابقة ثانٍ. الأنماط ثابتة:
// البريد والرابط والعدد تعابير نمطية خالصة؛ والهاتف
// يضيف عد أرقام (7 على الأقل)؛ والتاريخ يضيف صلاحية
// اليوم والشهر والسنة الكبيسة.

fn validate_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("بريد", native("بريد", validate_email)),
        ("رابط", native("رابط", validate_url)),
        ("هاتف", native("هاتف", validate_phone)),
        ("عدد", native("عدد", validate_number)),
        ("تاريخ", native("تاريخ", validate_date)),
        // البند 81 (الدفعة 2 من الـ579): متوقع — تأكيد
        // شرط يرفض بخطأ عربي عند فشله. بيته وحدة تحقق
        // لأنها وحدة التأكيدات والفحوصات القائمة (لا
        // وحدة جديدة لدالة واحدة)، ويفككه المحلل إلى
        // تحقق.متوقع(...) فيتطلب البرنامج استورد تحقق.
        ("متوقع", native("متوقع", validate_assert)),
        // البند 69 (الدفعة 2 من الـ579): فكك — تحقق
        // التفكيك قبل الفهرسة: قائمة بطول مطابق
        // تماما لعدد الأهداف، وإلا خطأ عربي صريح.
        ("فكك", native("فكك", validate_unpack)),
    ]
}

fn validate_email(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تحقق.بريد", args, 1, line, col)?;
    let s = need_string("تحقق.بريد", &args[0], line, col)?;
    rx_full_match(
        r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$",
        &s,
    )
    .map(Value::Bool)
    .map_err(|e| rx_err(e, line, col))
}

fn validate_url(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تحقق.رابط", args, 1, line, col)?;
    let s = need_string("تحقق.رابط", &args[0], line, col)?;
    rx_full_match(
        r"^https?://[A-Za-z0-9.-]+(:[0-9]+)?(/[^ ]*)?$",
        &s,
    )
    .map(Value::Bool)
    .map_err(|e| rx_err(e, line, col))
}

fn validate_phone(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تحقق.هاتف", args, 1, line, col)?;
    let s = need_string("تحقق.هاتف", &args[0], line, col)?;
    let shape = rx_full_match(r"^\+?[0-9][0-9 ()-]{6,19}$", &s).map_err(|e| rx_err(e, line, col))?;
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    Ok(Value::Bool(shape && digits >= 7))
}

fn validate_number(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تحقق.عدد", args, 1, line, col)?;
    let s = need_string("تحقق.عدد", &args[0], line, col)?;
    rx_full_match(r"^-?[0-9]+(\.[0-9]+)?$", &s)
        .map(Value::Bool)
        .map_err(|e| rx_err(e, line, col))
}

fn validate_date(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تحقق.تاريخ", args, 1, line, col)?;
    let s = need_string("تحقق.تاريخ", &args[0], line, col)?;
    let shape =
        rx_full_match(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$", &s).map_err(|e| rx_err(e, line, col))?;
    if !shape {
        return Ok(Value::Bool(false));
    }
    let chars: Vec<char> = s.chars().collect();
    let num = |a: usize, b: usize| -> u32 {
        chars[a..b].iter().collect::<String>().parse().unwrap_or(0)
    };
    let y = num(0, 4);
    let m = num(5, 7);
    let d = num(8, 10);
    if m < 1 || m > 12 || d < 1 {
        return Ok(Value::Bool(false));
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    Ok(Value::Bool(d <= dim))
}

/// البند 81 — `متوقع شرط` أو `متوقع شرط، "رسالة"`:
/// الشرط منطقي إجبارا؛ والنجاح يعيد لاشيء؛ والفشل
/// خطأ وقت تشغيل عربي — الرسالة المخصصة نصا كما
/// هي، وإلا «فشل التوقع: الشرط لم يتحقق».
fn validate_assert(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    if args.len() != 1 && args.len() != 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة تحقق.متوقع تتوقع معاملا واحدا أو معاملين لكن أعطيت {}",
                args.len()
            ),
            line,
            col,
        ));
    }
    let cond = match &args[0] {
        Value::Bool(b) => *b,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة تحقق.متوقع تتوقع قيمة منطقية لكن أعطيت {}",
                    other.type_name()
                ),
                line,
                col,
            ));
        }
    };
    if cond {
        return Ok(Value::Nil);
    }
    if args.len() == 2 {
        let msg = need_string("تحقق.متوقع", &args[1], line, col)?;
        return Err(RioError::runtime(msg, line, col));
    }
    Err(RioError::runtime(
        "فشل التوقع: الشرط لم يتحقق".to_string(),
        line,
        col,
    ))
}

/// البند 69 — تحقق التفكيك قبل الفهرسة: المصدر
/// قائمة، وطولها مطابق تماما لعدد الأهداف — نقص
/// أو زيادة أو مصدر غير قائمة خطأ عربي صريح يسمي
/// العددين. النجاح يعيد لاشيء.
fn validate_unpack(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    if args.len() != 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة تحقق.فكك تتوقع معاملين (حاوية وعدد) لكن أعطيت {}",
                args.len()
            ),
            line,
            col,
        ));
    }
    let list = need_list("تحقق.فكك", &args[0], line, col)?;
    let n = need_integer("تحقق.فكك", &args[1], line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            "الدالة تحقق.فكك تتوقع عدد عناصر غير سالب".to_string(),
            line,
            col,
        ));
    }
    let len = list.len() as i64;
    if len < n {
        return Err(RioError::runtime(
            format!(
                "التفكيك يتطلب {} عناصر لكن الحاوية تحوي {} فقط",
                n, len
            ),
            line,
            col,
        ));
    }
    if len > n {
        return Err(RioError::runtime(
            format!("التفكيك يتطلب {} عناصر لكن الحاوية تحوي {}", n, len),
            line,
            col,
        ));
    }
    Ok(Value::Nil)
}

// --- عشوائي ---

// SplitMix64 state, seeded once from system time + process id.
// A zero state means "not seeded yet" (the seed is forced nonzero).
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_u64() -> u64 {
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
        // المهمة 22 (نزّل): تنزيل ملف إلى القرص —
        // توأم اجلب على القرص (الجلب نفسه، والجسم
        // يُكتب ملفا بايتات خاما) + إلغاء بسطر stdin.
        // (المفتاح بلا شدة — كما «جزئ» و«حرر»: الشدة
        // تُحذف عند التحليل.)
        ("نزل", native("نزل", network_download)),
        ("اخدم", native("اخدم", network_serve)),
        // Catalog task 13 (مقابس, from net/): TCP/UDP sockets
        // under the same gate as اجلب. The stored keys are the
        // spellings the user writes: the lexer keeps an
        // identifier's spelling exactly (hamza preserved — only
        // diacritics are stripped), so أرسل is stored as
        // "أرسل", not normalized — the same rule as "أوقف" in
        // the عمليات module (task 10). Only أنشئ is different:
        // the lexer turns it into a keyword, so its key is
        // the normalized "انشي".
        ("اتصل", native("اتصل", net_connect)),
        ("مقبس", native("مقبس", net_socket)),
        ("استمع", native("استمع", net_listen)),
        ("أقبل", native("أقبل", net_accept)),
        ("اربط", native("اربط", net_bind)),
        ("أرسل", native("أرسل", net_send)),
        ("استقبل", native("استقبل", net_recv)),
        ("أرسل_إلى", native("أرسل_إلى", net_send_to)),
        ("استقبل_من", native("استقبل_من", net_recv_from)),
        ("أغلق", native("أغلق", net_close)),
        ("المنفذ", native("المنفذ", net_local_port)),
    ]
}

fn network_fetch(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.اجلب", args, 1, line, col)?;
    let url = need_string("شبكة.اجلب", &args[0], line, col)?;
    crate::network::fetch(&url, line, col)
}

// --- تنزيل ملف (المهمة 22، شبكة.نزّل) ---
//
// `شبكة.نزّل رابط مسار` — يرجع عدد البايتات
// المنزّلة. تحت بوابة الشبكة (كما اجلب) وFilePolicy
// (كما اكتب_ملف — فالمسار يُحلّ في network::download
// قبل أي شبكة). الاستدعاءات مفصولة بمسافات بلا
// فصلة، والصيغة المنقّطة هي الصحيحة (الاسم المستورد
// العاري لا يُستدعى بالمسافات — قيد محلل موثق في
// المهمة 20).
fn network_download(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.نزل", args, 2, line, col)?;
    let url = need_string("شبكة.نزل", &args[0], line, col)?;
    let path = need_string("شبكة.نزل", &args[1], line, col)?;
    if url.is_empty() {
        return Err(RioError::typ(
            "الدالة شبكة.نزل تتوقع رابطا غير فارغ",
            line,
            col,
        ));
    }
    if path.is_empty() {
        return Err(RioError::typ(
            "الدالة شبكة.نزل تتوقع مسارا غير فارغ",
            line,
            col,
        ));
    }
    // سياسة الجلسة ومجلدها من السياق نفسه (كما تحميل
    // الشهادة/المفتاح للخادم — فهما سياسة الجلسة
    // ومجلدها أنفسهما).
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::network::download(&url, &policy, &cwd, &path, line, col)
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

// --- مقابس (catalog task 13, from net/) ---
//
// Thin wrappers over crate::sockets: arity and types here,
// the gate and the work in the shared core. A socket handle
// is a non-negative integer; anything else is an Arabic
// type error naming the dotted function.

fn need_socket_id(display: &str, args: &[Value], at: usize, line: usize, col: usize) -> RioResult<u64> {
    let id = need_integer(display, &args[at], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع معرف مقبس غير سالب", display),
            line,
            col,
        ));
    }
    Ok(id as u64)
}

fn net_connect(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    if args.len() == 2 {
        let host = need_string("شبكة.اتصل", &args[0], line, col)?;
        let port = need_integer("شبكة.اتصل", &args[1], line, col)?;
        Ok(Value::Number(
            crate::sockets::connect_new(&host, port, line, col)? as f64,
        ))
    } else if args.len() == 3 {
        let id = need_socket_id("شبكة.اتصل", args, 0, line, col)?;
        let host = need_string("شبكة.اتصل", &args[1], line, col)?;
        let port = need_integer("شبكة.اتصل", &args[2], line, col)?;
        Ok(Value::Number(
            crate::sockets::connect_existing(id, &host, port, line, col)? as f64,
        ))
    } else {
        Err(RioError::typ(
            "الدالة شبكة.اتصل تتوقع معاملين (مضيف، منفذ) أو ثلاثة (معرف، مضيف، منفذ)",
            line,
            col,
        ))
    }
}

fn net_socket(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.مقبس", args, 1, line, col)?;
    let kind = need_string("شبكة.مقبس", &args[0], line, col)?;
    Ok(Value::Number(
        crate::sockets::create_socket(&kind, line, col)? as f64,
    ))
}

fn net_listen(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    if args.len() == 1 {
        let port = need_integer("شبكة.استمع", &args[0], line, col)?;
        Ok(Value::Number(
            crate::sockets::listen(port, None, line, col)? as f64,
        ))
    } else if args.len() == 2 {
        let port = need_integer("شبكة.استمع", &args[0], line, col)?;
        let host = need_string("شبكة.استمع", &args[1], line, col)?;
        Ok(Value::Number(
            crate::sockets::listen(port, Some(&host), line, col)? as f64,
        ))
    } else {
        Err(RioError::typ(
            "الدالة شبكة.استمع تتوقع معاملا واحدا (منفذ) أو معاملين (منفذ، مضيف)",
            line,
            col,
        ))
    }
}

fn net_accept(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.اقبل", args, 1, line, col)?;
    let id = need_socket_id("شبكة.اقبل", args, 0, line, col)?;
    Ok(Value::Number(
        crate::sockets::accept(id, line, col)? as f64,
    ))
}

fn net_bind(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.اربط", args, 2, line, col)?;
    let id = need_socket_id("شبكة.اربط", args, 0, line, col)?;
    let port = need_integer("شبكة.اربط", &args[1], line, col)?;
    crate::sockets::bind_udp(id, port, line, col)?;
    Ok(Value::Nil)
}

// --- إرسال HTTP (اجتماع 29، DESIGN §44؛ واجتماع 30، §45) ---
//
// `شبكة.أرسل(رابط، جسم[، نوع_المحتوى[، ترويسات]])`
// — HTTP POST، يرجع نص جسم الرد كما في `اجلب`.
// النوع الافتراضي `application/json`، والوسيط
// الثالث يبدّله (لتدفق الجهاز:
// `application/x-www-form-urlencoded`)، والوسيط
// الرابع الاختياري قاموس ترويسات المتصل {اسم:
// قيمة} نصا فقط (للمكالمات المصدّقة: Authorization
// وAccept). والمدارة (Host وContent-Length
// وContent-Type) تغلب على قيم المتصل؛ وCR/LF في
// اسم أو قيمة يُرفض بخطأ عربي قبل أي شبكة؛ وعند
// التحويل تُعاد الترويسات للمضيف المطابق فقط.
// تحت بوابة الشبكة نفسها بلا إذن جديد؛ وسقف جسم
// الطلب 5 ميغابايت؛ وغير-2xx خطأ عربي يسمّي الحالة.
// المفتاح المخزّن `أرسل` نفسه لمقابس المهمة 13 —
// فالدالة الواحدة توزّع على نوع المعامل الأول: عدد
// (معرف مقبس) يرسل على المقبس كما كان، ونص (رابط)
// يرسل POST. فلا مفتاح مكرر ولا كسر لواجهة المقابس.
fn network_post(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if args.len() < 2 || args.len() > 4 {
        return Err(RioError::typ(
            "الدالة شبكة.أرسل تتوقع معاملين (رابط جسم) أو 3 (رابط جسم نوع_المحتوى) أو 4 (رابط جسم نوع_المحتوى ترويسات)",
            line,
            col,
        ));
    }
    let url = need_string("شبكة.أرسل", &args[0], line, col)?;
    let body = need_string("شبكة.أرسل", &args[1], line, col)?;
    let content_type = if args.len() >= 3 {
        let ct = need_string("شبكة.أرسل", &args[2], line, col)?;
        if ct.is_empty() {
            return Err(RioError::typ(
                "الدالة شبكة.أرسل تتوقع نوع محتوى غير فارغ",
                line,
                col,
            ));
        }
        ct
    } else {
        "application/json".to_string()
    };
    // اجتماع 30: الوسيط الرابع قاموس ترويسات
    // نصا فقط — فغير القاموس أو قيمة غير نص
    // خطأ نوع عربي قبل أي شبكة.
    let headers: Vec<(String, String)> = if args.len() == 4 {
        match &args[3] {
            Value::Dict(entries) => {
                let mut out = Vec::with_capacity(entries.len());
                for (name, value) in entries {
                    match value {
                        Value::Str(text) => {
                            out.push((name.clone(), text.clone()));
                        }
                        _ => {
                            return Err(RioError::typ(
                                "الدالة شبكة.أرسل تتوقع ترويسات نصا فقط",
                                line,
                                col,
                            ));
                        }
                    }
                }
                out
            }
            _ => {
                return Err(RioError::typ(
                    "الدالة شبكة.أرسل تتوقع قاموس ترويسات في الوسيط الرابع",
                    line,
                    col,
                ));
            }
        }
    } else {
        Vec::new()
    };
    if url.is_empty() {
        return Err(RioError::typ(
            "الدالة شبكة.أرسل تتوقع رابطا غير فارغ",
            line,
            col,
        ));
    }
    crate::network::post(&url, &body, &content_type, &headers, line, col)
}

fn net_send(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    // اجتماع 29: المفتاح `أرسل` واحد لوجهتين — عدد
    // أولا يعني مقبسا (المهمة 13 كما كان)، ونص أولا
    // يعني رابطا فيُرسل POST. وغيرهما خطأ نوع صريح.
    if let Some(Value::Str(_)) = args.first() {
        return network_post(_ctx, args, line, col);
    }
    need_args("شبكة.أرسل", args, 2, line, col)?;
    let id = need_socket_id("شبكة.أرسل", args, 0, line, col)?;
    let text = need_string("شبكة.أرسل", &args[1], line, col)?;
    Ok(Value::Number(
        crate::sockets::send(id, &text, line, col)? as f64,
    ))
}

fn net_recv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.استقبل", args, 1, line, col)?;
    let id = need_socket_id("شبكة.استقبل", args, 0, line, col)?;
    Ok(Value::Str(crate::sockets::recv(id, line, col)?))
}

fn net_send_to(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.أرسل_إلى", args, 4, line, col)?;
    let id = need_socket_id("شبكة.أرسل_إلى", args, 0, line, col)?;
    let host = need_string("شبكة.أرسل_إلى", &args[1], line, col)?;
    let port = need_integer("شبكة.أرسل_إلى", &args[2], line, col)?;
    let text = need_string("شبكة.أرسل_إلى", &args[3], line, col)?;
    Ok(Value::Number(
        crate::sockets::send_to(id, &host, port, &text, line, col)? as f64,
    ))
}

fn net_recv_from(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.استقبل_من", args, 1, line, col)?;
    let id = need_socket_id("شبكة.استقبل_من", args, 0, line, col)?;
    let (text, host, port) = crate::sockets::recv_from(id, line, col)?;
    Ok(Value::Dict(vec![
        ("نص".to_string(), Value::Str(text)),
        ("مضيف".to_string(), Value::Str(host)),
        ("منفذ".to_string(), Value::Number(port as f64)),
    ]))
}

fn net_close(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.أغلق", args, 1, line, col)?;
    let id = need_socket_id("شبكة.أغلق", args, 0, line, col)?;
    crate::sockets::close_socket(id, line, col)?;
    Ok(Value::Nil)
}

fn net_local_port(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.المنفذ", args, 1, line, col)?;
    let id = need_socket_id("شبكة.المنفذ", args, 0, line, col)?;
    Ok(Value::Number(
        crate::sockets::local_port(id, line, col)? as f64,
    ))
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
    // اجتماع 32، السؤال السابع: الاسم المستعار
    // أنشئ_عملية (معرّف عادي بلا تصادم مع كلمة محجوزة)
    // يصدّر الدالة نفسها، فيعمل `من عمليات استورد
    // أنشئ_عملية` مجردا؛ ويبقى عمليات.أنشئ كما هو.
    vec![
        ("انشي", native("انشي", proc_spawn)),
        ("أنشئ_عملية", native("أنشئ_عملية", proc_spawn)),
        ("انتظر", native("انتظر", proc_wait)),
        ("أوقف", native("أوقف", proc_kill)),
        ("الحالية", native("الحالية", proc_current)),
        // المرحلة (هـ) — التحكم بالمهام (اجتماعا 64
        // و65 §79 و§80): أدوات الوظائف للسكربتات.
        // الأسماء بلا تشكيل ككل الأسماء.
        ("وظائف", native("وظائف", proc_jobs)),
        ("آخر_خلفية", native("آخر_خلفية", proc_last_background)),
        ("حية", native("حية", proc_is_alive)),
        ("انتظر_الكل", native("انتظر_الكل", proc_wait_all)),
        ("انتظر_أي", native("انتظر_أي", proc_wait_any)),
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

// المرحلة (هـ) — التحكم بالمهام (اجتماعا 64 و65
// §79 و§80): أدوات الوظائف للسكربتات.

/// `عمليات.وظائف()`: قائمة قواميس (رقم، أمر، حالة،
/// رمز أو لاشيء، معرف) — على نمط `تفاصيل_المسار`.
/// الحالة نص: تعمل، أو انتهت، أو أُوقفت؛ والرمز عدد
/// للمنتهية وحدها.
fn proc_jobs(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.وظائف", args, 0, line, col)?;
    let items: Vec<Value> = crate::processes::list_jobs()
        .iter()
        .map(|j| {
            let (status, code) = match j.status {
                crate::processes::JobStatus::Running => ("تعمل", Value::Nil),
                crate::processes::JobStatus::Done(c) => {
                    ("انتهت", Value::Number(c as f64))
                }
                crate::processes::JobStatus::Killed => ("أُوقفت", Value::Nil),
            };
            Value::Dict(vec![
                ("رقم".to_string(), Value::Number(j.number as f64)),
                ("أمر".to_string(), Value::Str(j.command.clone())),
                ("حالة".to_string(), Value::Str(status.to_string())),
                ("رمز".to_string(), code),
                ("معرف".to_string(), Value::Number(j.process_id as f64)),
            ])
        })
        .collect();
    Ok(Value::List(items))
}

/// `عمليات.آخر_خلفية()`: معرف آخر عملية خلفية
/// أُنشئت (عبر `&` أو عبر `عمليات.أنشئ` مباشرة)،
/// أو 0 حين لا خلفية بعد.
fn proc_last_background(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.آخر_خلفية", args, 0, line, col)?;
    Ok(Value::Number(
        crate::processes::last_background_id() as f64,
    ))
}

/// `عمليات.حية(معرف)`: صح/خطأ — هل العملية حية
/// (مقابل `kill -0`)؛ والغائبة خطأ لا صحيح.
fn proc_is_alive(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.حية", args, 1, line, col)?;
    let id = need_integer("عمليات.حية", &args[0], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            "الدالة عمليات.حية تتوقع معرف عملية غير سالب",
            line,
            col,
        ));
    }
    Ok(Value::Bool(crate::processes::is_alive(id as u64)))
}

/// `عمليات.انتظر_الكل()`: انتظار حاجب لكل وظيفة
/// جارية، ويرجع 0 دائما.
fn proc_wait_all(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.انتظر_الكل", args, 0, line, col)?;
    Ok(Value::Number(
        crate::processes::wait_all_jobs(line, col) as f64,
    ))
}

/// `عمليات.انتظر_أي()`: انتظار حاجب لأول وظيفة
/// جارية تنتهي — قاموس {وظيفة، رمز}.
fn proc_wait_any(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عمليات.انتظر_أي", args, 0, line, col)?;
    let (number, code) = crate::processes::wait_any_job(line, col)?;
    Ok(Value::Dict(vec![
        ("وظيفة".to_string(), Value::Number(number as f64)),
        ("رمز".to_string(), Value::Number(code as f64)),
    ]))
}

// --- تواصل (catalog task 16, from ipc/) ---

// Thin wrappers over crate::ipc: arity and types here,
// the gate and the work in the shared core. A channel id
// is a non-negative integer; anything else is an Arabic
// type error naming the dotted function. The stored keys
// are the spellings the user writes (hamza kept, like
// شبكة.أرسل): أنبوب، أرسل، استقبل، أغلق، أوقف.

fn ipc_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("أنبوب", native("أنبوب", ipc_open)),
        ("أرسل", native("أرسل", ipc_send)),
        ("استقبل", native("استقبل", ipc_recv)),
        ("أغلق", native("أغلق", ipc_close)),
        ("أوقف", native("أوقف", ipc_stop)),
    ]
}

fn need_channel_id(display: &str, args: &[Value], at: usize, line: usize, col: usize) -> RioResult<u64> {
    let id = need_integer(display, &args[at], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع معرف قناة غير سالب", display),
            line,
            col,
        ));
    }
    Ok(id as u64)
}

fn ipc_open(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تواصل.أنبوب", args, 1, line, col)?;
    let cmd = need_string("تواصل.أنبوب", &args[0], line, col)?;
    Ok(Value::Number(
        crate::ipc::open_pipe(&cmd, line, col)? as f64,
    ))
}

fn ipc_send(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تواصل.أرسل", args, 2, line, col)?;
    let id = need_channel_id("تواصل.أرسل", args, 0, line, col)?;
    let text = need_string("تواصل.أرسل", &args[1], line, col)?;
    Ok(Value::Number(
        crate::ipc::send(id, &text, line, col)? as f64,
    ))
}

fn ipc_recv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تواصل.استقبل", args, 1, line, col)?;
    let id = need_channel_id("تواصل.استقبل", args, 0, line, col)?;
    Ok(Value::Str(crate::ipc::recv(id, line, col)?))
}

fn ipc_close(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تواصل.أغلق", args, 1, line, col)?;
    let id = need_channel_id("تواصل.أغلق", args, 0, line, col)?;
    Ok(Value::Number(
        crate::ipc::close(id, line, col)? as f64,
    ))
}

fn ipc_stop(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تواصل.أوقف", args, 1, line, col)?;
    let id = need_channel_id("تواصل.أوقف", args, 0, line, col)?;
    crate::ipc::stop(id, line, col)?;
    Ok(Value::Nil)
}

// --- تشفير (catalog task 17, from crypto/) ---
//
// The stored keys are the spellings the user writes
// (hamza kept, like شبكة.أرسل): جزئ (the shadda is
// stripped at parse, like حرّر/حرر in ذاكرة — the user
// may write جزّئ or جزئ), عشوائي_آمن.

fn crypto_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("جزئ", native("جزئ", crypto_hash)),
        ("عشوائي_آمن", native("عشوائي_آمن", crypto_secure_random)),
        // باقي J14 — البند 209: ChaCha20 (RFC 8439) نقي.
        ("شفر", native("شفر", crypto_encrypt)),
        ("فك_التشفير", native("فك_التشفير", crypto_decrypt)),
    ]
}

fn crypto_hash(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تشفير.جزّئ", args, 1, line, col)?;
    let text = need_string("تشفير.جزّئ", &args[0], line, col)?;
    Ok(Value::Str(crate::crypto::sha256_hex(text.as_bytes())))
}

fn crypto_secure_random(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تشفير.عشوائي_آمن", args, 1, line, col)?;
    let n = need_integer("تشفير.عشوائي_آمن", &args[0], line, col)?;
    if n < 0 {
        return Err(RioError::typ(
            "الدالة تشفير.عشوائي_آمن تتوقع عددا غير سالب",
            line,
            col,
        ));
    }
    let bytes = crate::crypto::secure_random(n as usize, line, col)?;
    Ok(Value::List(
        bytes.into_iter().map(|b| Value::Number(b as f64)).collect(),
    ))
}

// --- أمن (catalog task 18, from security/) ---

fn security_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("قدرات_العملية", native("قدرات_العملية", sec_caps)),
        ("الوضع", native("الوضع", sec_mode)),
        ("تحقق", native("تحقق", sec_check)),
    ]
}

fn sec_caps(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("أمن.قدرات_العملية", args, 0, line, col)?;
    crate::security::process_capabilities(line, col)
}

fn sec_mode(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("أمن.الوضع", args, 0, line, col)?;
    crate::security::security_mode(line, col)
}

fn sec_check(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("أمن.تحقق", args, 1, line, col)?;
    let path = need_string("أمن.تحقق", &args[0], line, col)?;
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::security::check_access(&policy, &cwd, &path, line, col)
}

// --- غير_متزامن (catalog task 19, from io_uring/) ---
//
// The stored keys are the spellings the user writes
// (hamza kept, like شبكة.أرسل): اقرأ، أرسل، استقبل.

fn async_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("أنشئ_حلقة", native("أنشئ_حلقة", async_create_loop)),
        ("اقرأ", native("اقرأ", async_read)),
        ("اكتب", native("اكتب", async_write)),
        ("أرسل", native("أرسل", async_send)),
        ("استقبل", native("استقبل", async_recv)),
        ("انتظر_النتائج", native("انتظر_النتائج", async_wait)),
    ]
}

fn need_loop_id(display: &str, args: &[Value], at: usize, line: usize, col: usize) -> RioResult<u64> {
    let id = need_integer(display, &args[at], line, col)?;
    if id < 0 {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع معرف حلقة غير سالب", display),
            line,
            col,
        ));
    }
    Ok(id as u64)
}

fn async_create_loop(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.أنشئ_حلقة", args, 0, line, col)?;
    Ok(Value::Number(
        crate::async_io::create_loop(line, col)? as f64,
    ))
}

fn async_read(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.اقرأ", args, 3, line, col)?;
    let loop_id = need_loop_id("غير_متزامن.اقرأ", args, 0, line, col)?;
    let path = need_string("غير_متزامن.اقرأ", &args[1], line, col)?;
    let offset = need_integer("غير_متزامن.اقرأ", &args[2], line, col)?;
    if offset < 0 {
        return Err(RioError::typ(
            "الدالة غير_متزامن.اقرأ تتوقع موضعا غير سالب",
            line,
            col,
        ));
    }
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    Ok(Value::Number(
        crate::async_io::submit_read(&policy, &cwd, loop_id, &path, offset as u64, line, col)?
            as f64,
    ))
}

fn async_write(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.اكتب", args, 3, line, col)?;
    let loop_id = need_loop_id("غير_متزامن.اكتب", args, 0, line, col)?;
    let path = need_string("غير_متزامن.اكتب", &args[1], line, col)?;
    let text = need_string("غير_متزامن.اكتب", &args[2], line, col)?;
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    Ok(Value::Number(
        crate::async_io::submit_write(&policy, &cwd, loop_id, &path, &text, line, col)? as f64,
    ))
}

fn async_send(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.أرسل", args, 3, line, col)?;
    let loop_id = need_loop_id("غير_متزامن.أرسل", args, 0, line, col)?;
    let socket_id = need_socket_id("غير_متزامن.أرسل", args, 1, line, col)?;
    let text = need_string("غير_متزامن.أرسل", &args[2], line, col)?;
    Ok(Value::Number(
        crate::async_io::submit_send(loop_id, socket_id, &text, line, col)? as f64,
    ))
}

fn async_recv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.استقبل", args, 2, line, col)?;
    let loop_id = need_loop_id("غير_متزامن.استقبل", args, 0, line, col)?;
    let socket_id = need_socket_id("غير_متزامن.استقبل", args, 1, line, col)?;
    Ok(Value::Number(
        crate::async_io::submit_recv(loop_id, socket_id, line, col)? as f64,
    ))
}

fn async_wait(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("غير_متزامن.انتظر_النتائج", args, 1, line, col)?;
    let loop_id = need_loop_id("غير_متزامن.انتظر_النتائج", args, 0, line, col)?;
    crate::async_io::wait_results(loop_id, line, col)
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
        // عمليات SAF في شجرة ممنوحة (المهمة 28) —
        // مفاتيح مخزنة بلا شدة (قاعدة نبّه).
        (
            "امنح_مجلد",
            native("امنح_مجلد", file_saf_grant_tree),
        ),
        ("سرد_ممنوح", native("سرد_ممنوح", file_saf_list)),
        (
            "اقرأ_ممنوح",
            native("اقرأ_ممنوح", file_saf_read),
        ),
        (
            "اكتب_ممنوح",
            native("اكتب_ممنوح", file_saf_write),
        ),
        (
            "أنشئ_ملف_ممنوح",
            native("أنشئ_ملف_ممنوح", file_saf_create_file),
        ),
        (
            "أنشئ_مجلد_ممنوح",
            native("أنشئ_مجلد_ممنوح", file_saf_create_dir),
        ),
        (
            "تفاصيل_ممنوح",
            native("تفاصيل_ممنوح", file_saf_stat),
        ),
        (
            "احذف_ممنوح",
            native("احذف_ممنوح", file_saf_delete),
        ),
        ("الممنوح", native("الممنوح", file_saf_list_trees)),
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

// --- عمليات SAF في شجرة ممنوحة (المهمة 28) ---
// تغليف رقيق: الفحص كله في files.rs (عدد ونوع ثم
// منصة ثم قناة)، والمفاتيح المخزنة بلا شدة.

fn file_saf_grant_tree(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_grant_tree(args, line, col)
}

fn file_saf_list(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_list_granted(args, line, col)
}

fn file_saf_read(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_read_granted(args, line, col)
}

fn file_saf_write(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_write_granted(args, line, col)
}

fn file_saf_create_file(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_create_file_granted(args, line, col)
}

fn file_saf_create_dir(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_create_dir_granted(args, line, col)
}

fn file_saf_stat(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_stat_granted(args, line, col)
}

fn file_saf_delete(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_delete_granted(args, line, col)
}

fn file_saf_list_trees(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::files::saf_list_trees(args, line, col)
}

// --- نظام (catalog task 14, from arch/*/entry/syscalls) ---

fn system_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اسم_النداء", native("اسم_النداء", sys_name)),
        ("عدد_النداءات", native("عدد_النداءات", sys_count)),
        ("نسخة_النواة", native("نسخة_النواة", sys_kernel)),
        ("اسم_النظام", native("اسم_النظام", sys_os)),
        ("معرف_المستخدم", native("معرف_المستخدم", sys_uid)),
        ("معرف_المجموعة", native("معرف_المجموعة", sys_gid)),
    ]
}

fn sys_name(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.اسم_النداء", args, 1, line, col)?;
    let num = need_integer("نظام.اسم_النداء", &args[0], line, col)?;
    Ok(Value::Str(crate::system::syscall_name(num, line, col)?))
}

fn sys_count(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.عدد_النداءات", args, 0, line, col)?;
    Ok(Value::Number(
        crate::system::syscall_count(line, col)? as f64,
    ))
}

fn sys_kernel(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.نسخة_النواة", args, 0, line, col)?;
    Ok(Value::Str(crate::system::kernel_version(line, col)?))
}

fn sys_os(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.اسم_النظام", args, 0, line, col)?;
    Ok(Value::Str(crate::system::os_name(line, col)?))
}

fn sys_uid(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.معرف_المستخدم", args, 0, line, col)?;
    Ok(Value::Number(crate::system::user_id(line, col)? as f64))
}

fn sys_gid(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نظام.معرف_المجموعة", args, 0, line, col)?;
    Ok(Value::Number(crate::system::group_id(line, col)? as f64))
}

// --- وقت ---

fn time_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("الآن", native("الآن", time_now)),
        ("نوم", native("نوم", time_sleep)),
        ("التاريخ", native("التاريخ", time_date)),
        ("التاريخ_العالمي", native("التاريخ_العالمي", time_date_utc)),
        ("من_التاريخ", native("من_التاريخ", time_from_date)),
        (
            "من_التاريخ_العالمي",
            native("من_التاريخ_العالمي", time_from_date_utc),
        ),
        ("نص", native("نص", time_text)),
        ("نص_التاريخ", native("نص_التاريخ", time_text_date)),
        ("نص_الوقت", native("نص_الوقت", time_text_time)),
        ("اسم_اليوم", native("اسم_اليوم", time_weekday_name)),
        ("اسم_الشهر", native("اسم_الشهر", time_month_name)),
        ("فرق", native("فرق", time_diff)),
        ("أضف", native("أضف", time_add)),
        // باقي J14 — البند 205: تنسيق التاريخ/الوقت.
        ("نسق", native("نسق", time_format_custom)),
    ]
}

/// ثواني يونكس الآن كعدد صحيح (تُهمل الكسور).
/// `وقت.الآن` يعيد كسرا عشريا؛ والتقويم يعمل
/// بالثواني الصحيحة.
fn now_epoch() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0).floor() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// عدد ثواني يونكس من وسيط عددي (يقبل الكسر —
/// `وقت.الآن()` كسري — فيُهمل الجزء الكسري).
fn need_epoch(display: &str, v: &Value, line: usize, col: usize) -> RioResult<i64> {
    let x = need_number(display, v, line, col)?;
    if !x.is_finite() || x.abs() >= 9.0e15 {
        return Err(RioError::typ(
            format!("الدالة {display} تتوقع عددا صحيحا للثواني"),
            line,
            col,
        ));
    }
    Ok(x.floor() as i64)
}

fn time_date(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let epoch = match args.len() {
        0 => now_epoch(),
        1 => need_epoch("وقت.التاريخ", &args[0], line, col)?,
        _ => {
            return Err(RioError::runtime(
                "الدالة وقت.التاريخ تتوقع صفر معاملات أو معاملا واحدا (ثواني)",
                line,
                col,
            ))
        }
    };
    Ok(crate::time::to_dict(&crate::time::datetime_from_epoch(
        epoch,
        false,
        "وقت.التاريخ",
        line,
        col,
    )?))
}

fn time_date_utc(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let epoch = match args.len() {
        0 => now_epoch(),
        1 => need_epoch("وقت.التاريخ_العالمي", &args[0], line, col)?,
        _ => {
            return Err(RioError::runtime(
                "الدالة وقت.التاريخ_العالمي تتوقع صفر معاملات أو معاملا واحدا (ثواني)",
                line,
                col,
            ))
        }
    };
    Ok(crate::time::to_dict(&crate::time::datetime_from_epoch(
        epoch,
        true,
        "وقت.التاريخ_العالمي",
        line,
        col,
    )?))
}

fn time_from_date(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    time_from_date_impl(args, false, "وقت.من_التاريخ", line, col)
}

fn time_from_date_utc(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    time_from_date_impl(args, true, "وقت.من_التاريخ_العالمي", line, col)
}

fn time_from_date_impl(
    args: &[Value],
    utc: bool,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args(display, args, 6, line, col)?;
    let y = need_integer(display, &args[0], line, col)?;
    let mo = need_integer(display, &args[1], line, col)?;
    let d = need_integer(display, &args[2], line, col)?;
    let h = need_integer(display, &args[3], line, col)?;
    let mi = need_integer(display, &args[4], line, col)?;
    let s = need_integer(display, &args[5], line, col)?;
    Ok(crate::time::to_dict(
        &crate::time::datetime_from_components(y, mo, d, h, mi, s, utc, display, line, col)?,
    ))
}

fn time_text(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.نص", args, 1, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.نص", line, col)?;
    Ok(Value::Str(crate::time::format_datetime(&dt)))
}

fn time_text_date(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.نص_التاريخ", args, 1, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.نص_التاريخ", line, col)?;
    Ok(Value::Str(crate::time::format_date(&dt)))
}

fn time_text_time(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.نص_الوقت", args, 1, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.نص_الوقت", line, col)?;
    Ok(Value::Str(crate::time::format_time(&dt)))
}

fn time_weekday_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.اسم_اليوم", args, 1, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.اسم_اليوم", line, col)?;
    Ok(Value::Str(
        crate::time::weekday_name(dt.weekday).to_string(),
    ))
}

fn time_month_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.اسم_الشهر", args, 1, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.اسم_الشهر", line, col)?;
    Ok(Value::Str(
        crate::time::month_name(dt.month).to_string(),
    ))
}

fn time_diff(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.فرق", args, 2, line, col)?;
    let a = crate::time::from_dict(&args[0], "وقت.فرق", line, col)?;
    let b = crate::time::from_dict(&args[1], "وقت.فرق", line, col)?;
    Ok(Value::Number(
        crate::time::diff(&a, &b, "وقت.فرق", line, col)? as f64,
    ))
}

fn time_add(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.أضف", args, 2, line, col)?;
    let dt = crate::time::from_dict(&args[0], "وقت.أضف", line, col)?;
    let delta = need_integer("وقت.أضف", &args[1], line, col)?;
    Ok(crate::time::to_dict(&crate::time::shift(
        &dt, delta, "وقت.أضف", line, col,
    )?))
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

// --- جهاز (catalog task 20, Dialog — أول قدرات المرحلة 4؛
// task 21, Notify — نبّه + أزل_تنبيه) ---

fn device_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("حوار", native("حوار", device_dialog)),
        // المهمة 24 (إصلاح نبّه): المفتاح المخزن بلا
        // شدة — فالمحلل يحذف الشدة من المكتوب،
        // والمفتاح المشدد «نبّه» لم يُحلّ أبدا.
        ("نبه", native("نبه", device_notify)),
        ("أزل_تنبيه", native("أزل_تنبيه", device_remove_notify)),
        ("انطق", native("انطق", device_speak)),
        ("محركات_النطق", native("محركات_النطق", device_list_engines)),
        // المهمة 24 (شغّل_صوت + أوقف_صوت): المفتاح
        // المخزن «شغل_صوت» بلا شدة (المحلل يحذفها
        // من المكتوب — كما «جزئ» و«حرر» و«نزل»)؛
        // و«أوقف_صوت» بالهمزة حرفا (كما «أرسل»).
        ("شغل_صوت", native("شغل_صوت", device_play_audio)),
        ("أوقف_صوت", native("أوقف_صوت", device_stop_audio)),
        // المهمة 25 (حساس): المفتاح «حساس» بلا شدة
        // ولا همزة — فلا فخ تهجئة (درس المهمة 24).
        ("حساس", native("حساس", device_sensor)),
        // المهمة 26 (اسمع + بطارية): المفتاحان بلا
        // شدة ولا همزة — فلا فخ تهجئة (درس المهمة 24).
        ("اسمع", native("اسمع", device_listen)),
        ("بطارية", native("بطارية", device_battery)),
        // الدفعة 4أ من الكتالوج (البنود 22–34): المفاتيح
        // بلا شدة ولا همزة — فلا فخ تهجئة (درس المهمة 24).
        ("شارك_ملف", native("شارك_ملف", device_share_file)),
        ("شارك_نص", native("شارك_نص", device_share_text)),
        ("افتح_ملف", native("افتح_ملف", device_open_file)),
        ("افتح_رابط", native("افتح_رابط", device_open_url)),
        ("انسخ_للحافظة", native("انسخ_للحافظة", device_clipboard_set)),
        ("الحافظة", native("الحافظة", device_clipboard_get)),
        ("اختر_ملف", native("اختر_ملف", device_pick_file)),
        ("موقع", native("موقع", device_location_last)),
        ("موقع_حي", native("موقع_حي", device_location_once)),
        ("كاميرات", native("كاميرات", device_list_cameras)),
        ("التقط_صورة", native("التقط_صورة", device_capture_photo)),
        ("واي_فاي", native("واي_فاي", device_wifi_info)),
        ("مسح_واي_فاي", native("مسح_واي_فاي", device_wifi_scan)),
        // الدفعة 4ب من الكتالوج (البنود 61 و62 و66–72
        // و79 و105 و106 و119): المفاتيح بلا شدة ولا
        // همزة — فلا فخ تهجئة (درس المهمة 24)؛
        // و«سجل_صوت» بلا شدة (المكتوب «سجّل_صوت»
        // يُحلّ إليه بعد حذف الشدة).
        ("انسخ_آخر_خطأ", native("انسخ_آخر_خطأ", device_copy_last_error)),
        ("شارك_الجلسة", native("شارك_الجلسة", device_share_session)),
        ("أوقف_صوت_مؤقتا", native("أوقف_صوت_مؤقتا", device_pause_audio)),
        ("استأنف_صوت", native("استأنف_صوت", device_resume_audio)),
        ("معلومات_صوت", native("معلومات_صوت", device_audio_info)),
        ("حساسات", native("حساسات", device_list_sensors)),
        ("مستوى_الصوت", native("مستوى_الصوت", device_volume_level)),
        ("اضبط_مستوى_الصوت", native("اضبط_مستوى_الصوت", device_set_volume_level)),
        ("اهتز", native("اهتز", device_vibrate)),
        ("مصباح", native("مصباح", device_torch)),
        ("سجل_صوت", native("سجل_صوت", device_record_audio)),
        ("سطوع", native("سطوع", device_brightness)),
        ("توست", native("توست", device_toast)),
        ("معلومات_الصوت", native("معلومات_الصوت", device_audio_device_info)),
        // الدفعة 5 من الكتالوج (البنود 98 و99 خطوة
        // 1): المفاتيح بلا شدة ولا همزة — فلا فخ
        // تهجئة (درس المهمة 24)؛ و«نزل» بلا شدة
        // (المكتوب «نزّل» يُحلّ إليه بعد حذف الشدة).
        ("نزل", native("نزل", device_download)),
        ("اقرأ_مستند", native("اقرأ_مستند", device_read_document)),
        ("اعرض_مجلد", native("اعرض_مجلد", device_list_folder)),
    ]
}

fn device_dialog(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::show_dialog(args, line, col)
}

fn device_notify(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::show_notification(args, line, col)
}

fn device_remove_notify(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::remove_notification(args, line, col)
}

fn device_speak(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::speak_text(args, line, col)
}

fn device_list_engines(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::list_speech_engines(args, line, col)
}

fn device_play_audio(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::play_audio(args, &policy, &cwd, line, col)
}

fn device_stop_audio(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::stop_audio(args, line, col)
}

fn device_sensor(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::read_sensor(args, line, col)
}

fn device_listen(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::listen_speech(args, line, col)
}

fn device_battery(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::read_battery(args, line, col)
}

// الدفعة 4أ من الكتالوج (البنود 22–34).

fn device_share_file(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::share_file(args, &policy, &cwd, line, col)
}

fn device_share_text(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::share_text(args, line, col)
}

fn device_open_file(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::open_file(args, &policy, &cwd, line, col)
}

fn device_open_url(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::open_url(args, line, col)
}

fn device_clipboard_set(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::clipboard_set(args, line, col)
}

fn device_clipboard_get(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::clipboard_get(args, line, col)
}

fn device_pick_file(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::pick_file(args, line, col)
}

fn device_location_last(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::location_last(args, line, col)
}

fn device_location_once(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::location_once(args, line, col)
}

fn device_list_cameras(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::list_cameras(args, line, col)
}

fn device_capture_photo(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::capture_photo(args, &policy, &cwd, line, col)
}

fn device_wifi_info(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::wifi_info(args, line, col)
}

fn device_wifi_scan(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::wifi_scan(args, line, col)
}

// الدفعة 4ب من الكتالوج (البنود 61 و62 و66–72 و79 و105 و106 و119).

fn device_copy_last_error(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::copy_last_error(args, line, col)
}

fn device_share_session(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::share_session(args, line, col)
}

fn device_pause_audio(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::pause_audio(args, line, col)
}

fn device_resume_audio(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::resume_audio(args, line, col)
}

fn device_audio_info(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::audio_info(args, &policy, &cwd, line, col)
}

fn device_list_sensors(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::list_sensors(args, line, col)
}

fn device_volume_level(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::volume_level(args, line, col)
}

fn device_set_volume_level(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::set_volume_level(args, line, col)
}

fn device_vibrate(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::vibrate(args, line, col)
}

fn device_torch(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::torch(args, line, col)
}

fn device_record_audio(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let policy = ctx.server_file_policy();
    let cwd = ctx.server_cwd();
    crate::device::record_audio(args, &policy, &cwd, line, col)
}

fn device_brightness(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::brightness(args, line, col)
}

fn device_toast(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::toast(args, line, col)
}

fn device_audio_device_info(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::audio_device_info(args, line, col)
}

fn device_download(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::download(args, line, col)
}

fn device_read_document(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::read_document(args, line, col)
}

fn device_list_folder(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    crate::device::list_folder(args, line, col)
}


// ============================================================
// باقي J14 (الدفعة الثالثة من برنامج الـ579، الاجتماع 73 §88):
// البنود 205–211 و214–217.
// وحدات أصلية نقية بلا اعتماديات جديدة، وبلا مساس
// بـPURE_BUILTINS ولا semantic.rs ولا ast.rs.
// المفاتيح المخزنة بلا شدة دائما (المحلل يجرّد الشدة
// وقت التحليل): «نسق» من نسّق، و«شفر» من شفّر، و«ولد»
// من ولّد، و«حول_وحدات» من حوّل_وحدات — كما سابقة
// «انشي» في عمليات و«اصغر»/«اكبر» في رياضيات.
// ============================================================

fn need_list<'a>(
    display: &str,
    v: &'a Value,
    line: usize,
    col: usize,
) -> RioResult<&'a Vec<Value>> {
    match v {
        Value::List(items) => Ok(items),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// قائمة بايتات (أعداد صحيحة 0..255) أو نص (بايتات UTF-8).
fn need_bytes(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Vec<u8>> {
    match v {
        Value::Str(s) => Ok(s.as_bytes().to_vec()),
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Number(x) if x.fract() == 0.0 && *x >= 0.0 && *x <= 255.0 => {
                        out.push(*x as u8)
                    }
                    _ => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتوقع قائمة بايتات (أعداد 0..255)",
                                display
                            ),
                            line,
                            col,
                        ))
                    }
                }
            }
            Ok(out)
        }
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع نصا أو قائمة بايتات لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn need_key_bytes(
    display: &str,
    v: &Value,
    expected: usize,
    kind: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<u8>> {
    let bytes = need_bytes(display, v, line, col)?;
    if bytes.len() != expected {
        return Err(RioError::typ(
            format!(
                "الدالة {} تتوقع {} من {} بايتا لكن أعطيت {}",
                display,
                kind,
                expected,
                bytes.len()
            ),
            line,
            col,
        ));
    }
    Ok(bytes)
}

/// تحويل أرقام غربية في نص إلى أرقام عربية مشرقية،
/// والنقطة العشرية إلى ٫، وفاصل الآلاف إلى ٬ —
/// كما يطبع المحرك الأعداد (value.rs fmt_number).
fn arabicize_number(s: &str) -> String {
    const DIGITS: [char; 10] = ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];
    s.chars()
        .map(|c| match c {
            '0'..='9' => DIGITS[c as usize - '0' as usize],
            '.' => '٫',
            ',' => '٬',
            other => other,
        })
        .collect()
}

/// عدد بأرقام غربية قصيرة (كما Rust Display) قبل التعريب.
fn plain_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

// ---------- 205: تنسيق التاريخ/الوقت (إكمال وحدة وقت) ----------
// الصيغة رموز على نمط strftime: %Y سنة (4 أرقام)،
// %m شهر رقمان، %d يوم رقمان، %H ساعة رقمان،
// %M دقيقة رقمان، %S ثانية رقمان، %e يوم بلا صفر،
// %k ساعة بلا صفر، %A اسم اليوم العربي،
// %B اسم الشهر العربي، %% علامة %. أي رمز آخر خطأ.

fn time_format_custom(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وقت.نسق", args, 2, line, col)?;
    let dt = match &args[0] {
        Value::Number(x) => {
            let epoch = need_epoch("وقت.نسق", &args[0], line, col)?;
            let _ = x;
            crate::time::datetime_from_epoch(epoch, false, "وقت.نسق", line, col)?
        }
        Value::Dict(_) => crate::time::from_dict(&args[0], "وقت.نسق", line, col)?,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة وقت.نسق تتوقع قاموس تاريخ أو ثواني لكن أعطيت {}",
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let fmt = need_string("وقت.نسق", &args[1], line, col)?;
    if fmt.chars().count() > 1000 {
        return Err(RioError::runtime("صيغة التاريخ كبيرة جدا", line, col));
    }
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let tok = chars.next().ok_or_else(|| {
            RioError::runtime("صيغة تاريخ غير صالحة: % في النهاية", line, col)
        })?;
        match tok {
            'Y' => out.push_str(&format!("{:04}", dt.year)),
            'm' => out.push_str(&format!("{:02}", dt.month)),
            'd' => out.push_str(&format!("{:02}", dt.day)),
            'H' => out.push_str(&format!("{:02}", dt.hour)),
            'M' => out.push_str(&format!("{:02}", dt.minute)),
            'S' => out.push_str(&format!("{:02}", dt.second)),
            'e' => out.push_str(&format!("{}", dt.day)),
            'k' => out.push_str(&format!("{}", dt.hour)),
            'A' => out.push_str(crate::time::weekday_name(dt.weekday)),
            'B' => out.push_str(crate::time::month_name(dt.month)),
            '%' => out.push('%'),
            _ => {
                return Err(RioError::runtime(
                    format!("صيغة تاريخ غير صالحة: %{}", tok),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(Value::Str(out))
}

// ---------- 206: تنسيق النصوص (وحدة نص جديدة) ----------
// نسّق(قالب، قائمة): {0} و{1} موضعية.
// نسّق(قالب، قاموس): {اسم} مسماة.
// مواصفة بعد النقطتين للعدد فقط: [فاصل آلاف ,]
// [صفر وعرض 0W] [.دقة N] — مثال: {0:,.2} و{1:05}.
// {{ و}} هروب للقوسين. الأعداد تخرج بأرقام عربية
// كما يطبعها المحرك؛ والنصوص كما هي.
// + دوال وحدة نص الملفية (حزم/نص.نيفا) منفذة أصليا
// هنا بنفس الدلالة حرفيا — لأن الوحدة الأصلية تحجب
// الملف المدمج عند الاستيراد بالاسم، ومدير الحزم
// (أوامر.نيفا وغيرها) يستعمل هذه الدوال السبع:
// يبدأ_ب، ينتهي_ب، قسم_كلمات، عدد_مسافات_بادئة،
// من_الفهرس، قبل_الفهرس، اسم_الملف.

fn text_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("نسق", native("نسق", text_format)),
        ("يبدأ_ب", native("يبدأ_ب", text_starts_with)),
        ("ينتهي_ب", native("ينتهي_ب", text_ends_with)),
        ("قسم_كلمات", native("قسم_كلمات", text_split_words)),
        // J3-74: قسّم_إلى_كلمات — إكمال قسم_كلمات: أي
        // مسافة بيضاء ASCII فاصلة، لا المسافة الواحدة فقط.
        ("قسم_إلى_كلمات", native("قسم_إلى_كلمات", text_split_words_all)),
        (
            "عدد_مسافات_بادئة",
            native("عدد_مسافات_بادئة", text_leading_spaces),
        ),
        ("من_الفهرس", native("من_الفهرس", text_from_index)),
        ("قبل_الفهرس", native("قبل_الفهرس", text_before_index)),
        ("اسم_الملف", native("اسم_الملف", text_file_name)),
        // البند 164 (الدفعة 2 من الـ579): تحويل التمثيل.
        ("تمثيل", native("تمثيل", text_repr)),
    ]
}

/// يبدأ_ب(نص، بادئة) — صحيح إن بدأ النص بالبادئة،
/// وخطأ إن كانت البادئة أطول من النص أو لم تطابق.
fn text_starts_with(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.يبدأ_ب", args, 2, line, col)?;
    let text = need_string("نص.يبدأ_ب", &args[0], line, col)?;
    let prefix = need_string("نص.يبدأ_ب", &args[1], line, col)?;
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = prefix.chars().collect();
    if p.len() > t.len() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(t[..p.len()] == p[..]))
}

/// ينتهي_ب(نص، لاحقة) — صحيح إن انتهى النص باللاحقة،
/// وخطأ إن كانت اللاحقة أطول من النص أو لم تطابق.
fn text_ends_with(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.ينتهي_ب", args, 2, line, col)?;
    let text = need_string("نص.ينتهي_ب", &args[0], line, col)?;
    let suffix = need_string("نص.ينتهي_ب", &args[1], line, col)?;
    let t: Vec<char> = text.chars().collect();
    let s: Vec<char> = suffix.chars().collect();
    if s.len() > t.len() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(t[t.len() - s.len()..] == s[..]))
}

/// قسم_كلمات(نص) — قسم على المسافة الواحدة مع إسقاط
/// الأجزاء الفارغة، كما في نسخة الملف حرفيا.
fn text_split_words(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.قسم_كلمات", args, 1, line, col)?;
    let text = need_string("نص.قسم_كلمات", &args[0], line, col)?;
    let out: Vec<Value> = text
        .split(' ')
        .filter(|p| !p.is_empty())
        .map(|p| Value::Str(p.to_string()))
        .collect();
    Ok(Value::List(out))
}

/// عدد_مسافات_بادئة(سطر) — عدد المسافات في بداية السطر.
fn text_leading_spaces(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.عدد_مسافات_بادئة", args, 1, line, col)?;
    let text = need_string("نص.عدد_مسافات_بادئة", &args[0], line, col)?;
    let n = text.chars().take_while(|c| *c == ' ').count();
    Ok(Value::Number(n as f64))
}

/// من_الفهرس(نص، فهرس) — النص من الفهرس إلى النهاية
/// (بمحارف، لا بايتات). فهرس سالب أو أكبر من الطول
/// خطأ؛ فهرس يساوي الطول يعيد نصا فارغا.
fn text_from_index(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.من_الفهرس", args, 2, line, col)?;
    let text = need_string("نص.من_الفهرس", &args[0], line, col)?;
    let idx = need_integer("نص.من_الفهرس", &args[1], line, col)?;
    let chars: Vec<char> = text.chars().collect();
    if idx < 0 || idx as usize > chars.len() {
        return Err(RioError::runtime(
            "الفهرس خارج حدود النص في نص.من_الفهرس",
            line,
            col,
        ));
    }
    Ok(Value::Str(chars[idx as usize..].iter().collect()))
}

/// قبل_الفهرس(نص، فهرس) — النص من البداية إلى الفهرس
/// (بمحارف). فهرس سالب أو أكبر من الطول خطأ.
fn text_before_index(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.قبل_الفهرس", args, 2, line, col)?;
    let text = need_string("نص.قبل_الفهرس", &args[0], line, col)?;
    let idx = need_integer("نص.قبل_الفهرس", &args[1], line, col)?;
    let chars: Vec<char> = text.chars().collect();
    if idx < 0 || idx as usize > chars.len() {
        return Err(RioError::runtime(
            "الفهرس خارج حدود النص في نص.قبل_الفهرس",
            line,
            col,
        ));
    }
    Ok(Value::Str(chars[..idx as usize].iter().collect()))
}

/// اسم_الملف(مسار) — الجزء الأخير بعد آخر شرطة مائلة.
fn text_file_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.اسم_الملف", args, 1, line, col)?;
    let path = need_string("نص.اسم_الملف", &args[0], line, col)?;
    let last = path.split('/').next_back().unwrap_or("");
    Ok(Value::Str(last.to_string()))
}

/// نص.تمثيل(قيمة) — تمثيل القيمة للعرض داخل نص تنفيذي
/// (البند 164، الدفعة 2 من الـ579): النص بين علامتي
/// اقتباس مع هروب، وكل قيمة أخرى كما يطبعها المحرك
/// (والقائمة تقتبس نصوصها أصلا). يقابل `!تمثيل` في الحقل.
fn text_repr(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.تمثيل", args, 1, line, col)?;
    match &args[0] {
        Value::Str(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    other => out.push(other),
                }
            }
            out.push('"');
            Ok(Value::Str(out))
        }
        other => Ok(Value::Str(other.fmt_value())),
    }
}

fn text_format_number(n: f64, spec: &str) -> Result<String, String> {
    let mut rest = spec;
    let mut grouping = false;
    if let Some(r) = rest.strip_prefix(',') {
        grouping = true;
        rest = r;
    }
    let mut zero_width: Option<usize> = None;
    if let Some(r) = rest.strip_prefix('0') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err("مواصفة تنسيق غير صالحة".to_string());
        }
        zero_width = digits.parse::<usize>().ok();
        rest = &r[digits.len()..];
    }
    let mut precision: Option<usize> = None;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err("مواصفة تنسيق غير صالحة".to_string());
        }
        precision = digits.parse::<usize>().ok();
        rest = &r[digits.len()..];
    }
    if !rest.is_empty() {
        return Err("مواصفة تنسيق غير صالحة".to_string());
    }
    let mut s = match precision {
        Some(p) => format!("{:.*}", p, n),
        None => plain_number(n),
    };
    if grouping {
        let (sign, body) = if let Some(b) = s.strip_prefix('-') {
            ("-", b.to_string())
        } else {
            ("", s.clone())
        };
        let (int_part, frac_part) = match body.find('.') {
            Some(i) => (body[..i].to_string(), Some(body[i..].to_string())),
            None => (body, None),
        };
        let mut grouped = String::new();
        for (i, c) in int_part.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        let grouped: String = grouped.chars().rev().collect();
        s = format!("{}{}", sign, grouped);
        if let Some(f) = frac_part {
            s.push_str(&f);
        }
    } else if let Some(w) = zero_width {
        let (sign, body) = if let Some(b) = s.strip_prefix('-') {
            ("-", b.to_string())
        } else {
            ("", s.clone())
        };
        let (int_part, frac_part) = match body.find('.') {
            Some(i) => (body[..i].to_string(), Some(body[i..].to_string())),
            None => (body, None),
        };
        if int_part.len() < w {
            let pad = "0".repeat(w - int_part.len());
            s = format!("{}{}{}", sign, pad, int_part);
            if let Some(f) = frac_part {
                s.push_str(&f);
            }
        }
    }
    Ok(arabicize_number(&s))
}

fn text_format(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("نص.نسق", args, 2, line, col)?;
    let tpl = need_string("نص.نسق", &args[0], line, col)?;
    if tpl.chars().count() > 100000 {
        return Err(RioError::runtime("قالب التنسيق كبير جدا", line, col));
    }
    let positional: Option<&Vec<Value>> = match &args[1] {
        Value::List(items) => Some(items),
        _ => None,
    };
    let named: Option<&Vec<(String, Value)>> = match &args[1] {
        Value::Dict(pairs) => Some(pairs),
        _ => None,
    };
    if positional.is_none() && named.is_none() {
        return Err(RioError::typ(
            format!(
                "الدالة نص.نسق تتوقع قائمة أو قاموسا لكن أعطيت {}",
                args[1].type_name()
            ),
            line,
            col,
        ));
    }
    let mut out = String::new();
    let chars: Vec<char> = tpl.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                out.push('{');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            let mut inner = String::new();
            while j < chars.len() && chars[j] != '}' {
                inner.push(chars[j]);
                j += 1;
            }
            if j >= chars.len() {
                return Err(RioError::runtime("قالب تنسيق غير صالح: قوس غير مغلق", line, col));
            }
            if inner.is_empty() {
                return Err(RioError::runtime("قالب تنسيق غير صالح: موضع فارغ", line, col));
            }
            let (head, spec) = match inner.find(':') {
                Some(k) => (&inner[..k], Some(inner[k + 1..].to_string())),
                None => (inner.as_str(), None),
            };
            let value = if head.chars().all(|ch| ch.is_ascii_digit()) {
                let idx: usize = head.parse().map_err(|_| {
                    RioError::runtime("قالب تنسيق غير صالح: موضع غير رقمي", line, col)
                })?;
                match positional {
                    Some(items) => items.get(idx).ok_or_else(|| {
                        RioError::runtime(
                            format!("موضع تنسيق غير موجود: {}", idx),
                            line,
                            col,
                        )
                    })?,
                    None => {
                        return Err(RioError::runtime(
                            "قالب تنسيق غير صالح: موضع رقمي بلا قائمة",
                            line,
                            col,
                        ))
                    }
                }
            } else {
                match named {
                    Some(pairs) => dict_get(pairs, head).ok_or_else(|| {
                        RioError::runtime(
                            format!("اسم تنسيق غير موجود: {}", head),
                            line,
                            col,
                        )
                    })?,
                    None => {
                        return Err(RioError::runtime(
                            "قالب تنسيق غير صالح: اسم بلا قاموس",
                            line,
                            col,
                        ))
                    }
                }
            };
            match spec {
                None => out.push_str(&value.fmt_value()),
                Some(sp) => match value {
                    Value::Number(n) => {
                        let s = text_format_number(*n, &sp)
                            .map_err(|e| RioError::runtime(e, line, col))?;
                        out.push_str(&s);
                    }
                    _ => {
                        return Err(RioError::runtime(
                            "مواصفة تنسيق لغير عدد",
                            line,
                            col,
                        ))
                    }
                },
            }
            i = j + 1;
            continue;
        }
        if c == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                out.push('}');
                i += 2;
                continue;
            }
            return Err(RioError::runtime("قالب تنسيق غير صالح: قوس منفرد", line, col));
        }
        out.push(c);
        i += 1;
    }
    if out.chars().count() > 1000000 {
        return Err(RioError::runtime("ناتج التنسيق كبير جدا", line, col));
    }
    Ok(Value::Str(out))
}

// ---------- 207: تحويل القواعد العددية (وحدة أسس جديدة) ----------
// إلى_أساس(عدد، أساس 2..36) → نص بأرقام 0-9a-z
// (حروف لاتينية صغيرة — اصطلاح القواعد العددية).
// من_أساس(نص، أساس) → عدد؛ يقبل إشارة سالبة
// وأرقاما عربية مشرقية وحروفا كبيرة أو صغيرة.

fn bases_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("إلى_أساس", native("إلى_أساس", bases_to_base)),
        ("من_أساس", native("من_أساس", bases_from_base)),
    ]
}

const BASE_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn bases_to_base(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("أسس.إلى_أساس", args, 2, line, col)?;
    let n = need_integer("أسس.إلى_أساس", &args[0], line, col)?;
    let base = need_integer("أسس.إلى_أساس", &args[1], line, col)?;
    if !(2..=36).contains(&base) {
        return Err(RioError::typ(
            "الدالة أسس.إلى_أساس تتوقع أساسا بين 2 و 36",
            line,
            col,
        ));
    }
    if n == 0 {
        return Ok(Value::Str("0".to_string()));
    }
    let neg = n < 0;
    let mut v: u64 = n.unsigned_abs();
    let b = base as u64;
    let mut out: Vec<u8> = Vec::new();
    while v > 0 {
        out.push(BASE_DIGITS[(v % b) as usize]);
        v /= b;
    }
    if neg {
        out.push(b'-');
    }
    out.reverse();
    Ok(Value::Str(String::from_utf8(out).unwrap()))
}

fn bases_from_base(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("أسس.من_أساس", args, 2, line, col)?;
    let text = need_string("أسس.من_أساس", &args[0], line, col)?;
    let base = need_integer("أسس.من_أساس", &args[1], line, col)?;
    if !(2..=36).contains(&base) {
        return Err(RioError::typ(
            "الدالة أسس.من_أساس تتوقع أساسا بين 2 و 36",
            line,
            col,
        ));
    }
    let t = text.trim();
    if t.is_empty() {
        return Err(RioError::runtime("نص أساس فارغ", line, col));
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t),
    };
    if body.is_empty() {
        return Err(RioError::runtime("نص أساس غير صالح", line, col));
    }
    let mut value: i64 = 0;
    for c in body.chars() {
        let d: i64 = match c {
            '0'..='9' => c as i64 - '0' as i64,
            'a'..='z' => c as i64 - 'a' as i64 + 10,
            'A'..='Z' => c as i64 - 'A' as i64 + 10,
            '٠'..='٩' => c as i64 - '٠' as i64,
            _ => {
                return Err(RioError::runtime(
                    format!("محرف غير صالح في نص الأساس: {}", c),
                    line,
                    col,
                ))
            }
        };
        if d >= base {
            return Err(RioError::runtime(
                format!("محرف خارج الأساس {}: {}", base, c),
                line,
                col,
            ));
        }
        value = value
            .checked_mul(base)
            .and_then(|v| v.checked_add(d))
            .ok_or_else(|| RioError::runtime("عدد كبير جدا", line, col))?;
    }
    if neg {
        value = -value;
    }
    Ok(Value::Number(value as f64))
}

// ---------- 208: تحويل الوحدات (وحدة وحدات جديدة) ----------
// حوّل_وحدات(قيمة، من، إلى): طول ووزن وحجم ومساحة
// بعامل خطي عن وحدة الأساس، وحرارة بتحويل تآلفي
// عبر المئوية. وحدتان من فئتين مختلفتين أو وحدة
// مجهولة خطأ عربي. الفدان فدان مصري (4200.833 م²)،
// والغالون غالون أمريكي، والكوب كوب أمريكي.

fn units_entries() -> Vec<(&'static str, Value)> {
    vec![("حول_وحدات", native("حول_وحدات", units_convert))]
}

fn units_linear(name: &str) -> Option<(&'static str, f64)> {
    Some(match name {
        // طول — الأساس متر.
        "متر" => ("طول", 1.0),
        "كيلومتر" => ("طول", 1000.0),
        "سنتيمتر" => ("طول", 0.01),
        "مليمتر" => ("طول", 0.001),
        "ميل" => ("طول", 1609.344),
        "ياردة" => ("طول", 0.9144),
        "قدم" => ("طول", 0.3048),
        "بوصة" => ("طول", 0.0254),
        "ميل_بحري" => ("طول", 1852.0),
        // وزن — الأساس كيلوغرام.
        "كيلوغرام" => ("وزن", 1.0),
        "غرام" => ("وزن", 0.001),
        "طن" => ("وزن", 1000.0),
        "رطل" => ("وزن", 0.45359237),
        "أونصة" => ("وزن", 0.028349523125),
        // حجم — الأساس لتر.
        "لتر" => ("حجم", 1.0),
        "مليلتر" => ("حجم", 0.001),
        "متر_مكعب" => ("حجم", 1000.0),
        "غالون" => ("حجم", 3.785411784),
        "كوب" => ("حجم", 0.2365882365),
        // مساحة — الأساس متر مربع.
        "متر_مربع" => ("مساحة", 1.0),
        "كيلومتر_مربع" => ("مساحة", 1_000_000.0),
        "هكتار" => ("مساحة", 10000.0),
        "فدان" => ("مساحة", 4200.833),
        "قدم_مربع" => ("مساحة", 0.09290304),
        _ => return None,
    })
}

fn units_is_temp(name: &str) -> bool {
    matches!(name, "مئوية" | "فهرنهايت" | "كلفن")
}

fn units_to_celsius(v: f64, from: &str) -> f64 {
    match from {
        "مئوية" => v,
        "فهرنهايت" => (v - 32.0) * 5.0 / 9.0,
        _ => v - 273.15,
    }
}

fn units_from_celsius(c: f64, to: &str) -> f64 {
    match to {
        "مئوية" => c,
        "فهرنهايت" => c * 9.0 / 5.0 + 32.0,
        _ => c + 273.15,
    }
}

fn units_convert(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("وحدات.حول_وحدات", args, 3, line, col)?;
    let value = need_number("وحدات.حول_وحدات", &args[0], line, col)?;
    let from = need_string("وحدات.حول_وحدات", &args[1], line, col)?;
    let to = need_string("وحدات.حول_وحدات", &args[2], line, col)?;
    if units_is_temp(&from) || units_is_temp(&to) {
        if !units_is_temp(&from) || !units_is_temp(&to) {
            return Err(RioError::runtime(
                "وحدتا حرارة وغير حرارة لا تتحول إحداهما إلى الأخرى",
                line,
                col,
            ));
        }
        return Ok(Value::Number(units_from_celsius(
            units_to_celsius(value, &from),
            &to,
        )));
    }
    let (cat_from, f_from) = units_linear(&from).ok_or_else(|| {
        RioError::runtime(format!("وحدة غير معروفة: {}", from), line, col)
    })?;
    let (cat_to, f_to) = units_linear(&to).ok_or_else(|| {
        RioError::runtime(format!("وحدة غير معروفة: {}", to), line, col)
    })?;
    if cat_from != cat_to {
        return Err(RioError::runtime(
            format!("وحدتا {} و{} من فئتين مختلفتين", from, to),
            line,
            col,
        ));
    }
    Ok(Value::Number(value * f_from / f_to))
}

// ---------- 209: تشفير ChaCha20 (إكمال وحدة تشفير) ----------
// شفّر(بيانات، مفتاح 32 بايتا، نون 12 بايتا)
// → قائمة بايتات؛ وفك_التشفير(بايتات، مفتاح، نون)
// → نص. تشفير تيار ChaCha20 (RFC 8439) Rust نقي،
// العداد يبدأ من 1 كما في متجه الاختبار الرسمي.
// المفتاح والنون قوائم بايتات (0..255) — يولَّدان
// بعشوائي_آمن(32) وعشوائي_آمن(12). الشفرة نفسها
// متماثلة: فك التشفير هو التشفير نفسه على البايتات.
// بلا اعتماد جديد إطلاقا (ممنوع ring بلا اجتماع).

fn chacha_quarter(w: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    w[a] = w[a].wrapping_add(w[b]);
    w[d] ^= w[a];
    w[d] = w[d].rotate_left(16);
    w[c] = w[c].wrapping_add(w[d]);
    w[b] ^= w[c];
    w[b] = w[b].rotate_left(12);
    w[a] = w[a].wrapping_add(w[b]);
    w[d] ^= w[a];
    w[d] = w[d].rotate_left(8);
    w[c] = w[c].wrapping_add(w[d]);
    w[b] ^= w[c];
    w[b] = w[b].rotate_left(7);
}

fn chacha_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut s = [0u32; 16];
    s[0] = 0x61707865;
    s[1] = 0x3320646e;
    s[2] = 0x79622d32;
    s[3] = 0x6b206574;
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = u32::from_le_bytes(nonce[4 * i..4 * i + 4].try_into().unwrap());
    }
    let mut w = s;
    for _ in 0..10 {
        chacha_quarter(&mut w, 0, 4, 8, 12);
        chacha_quarter(&mut w, 1, 5, 9, 13);
        chacha_quarter(&mut w, 2, 6, 10, 14);
        chacha_quarter(&mut w, 3, 7, 11, 15);
        chacha_quarter(&mut w, 0, 5, 10, 15);
        chacha_quarter(&mut w, 1, 6, 11, 12);
        chacha_quarter(&mut w, 2, 7, 8, 13);
        chacha_quarter(&mut w, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4]
            .copy_from_slice(&w[i].wrapping_add(s[i]).to_le_bytes());
    }
    out
}

fn chacha_xor(data: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut counter: u32 = 1;
    for chunk in data.chunks(64) {
        let ks = chacha_block(key, counter, nonce);
        counter = counter.wrapping_add(1);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
    }
    out
}

fn crypto_encrypt(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تشفير.شفر", args, 3, line, col)?;
    let data = need_bytes("تشفير.شفر", &args[0], line, col)?;
    if data.len() > 10 * 1024 * 1024 {
        return Err(RioError::runtime("بيانات التشفير كبيرة جدا", line, col));
    }
    let key = need_key_bytes("تشفير.شفر", &args[1], 32, "مفتاح", line, col)?;
    let nonce = need_key_bytes("تشفير.شفر", &args[2], 12, "نون", line, col)?;
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key);
    let mut nonce_arr = [0u8; 12];
    nonce_arr.copy_from_slice(&nonce);
    let out = chacha_xor(&data, &key_arr, &nonce_arr);
    Ok(Value::List(
        out.into_iter().map(|b| Value::Number(b as f64)).collect(),
    ))
}

fn crypto_decrypt(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تشفير.فك_التشفير", args, 3, line, col)?;
    let data = need_bytes("تشفير.فك_التشفير", &args[0], line, col)?;
    if data.len() > 10 * 1024 * 1024 {
        return Err(RioError::runtime("بيانات التشفير كبيرة جدا", line, col));
    }
    let key = need_key_bytes("تشفير.فك_التشفير", &args[1], 32, "مفتاح", line, col)?;
    let nonce = need_key_bytes("تشفير.فك_التشفير", &args[2], 12, "نون", line, col)?;
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key);
    let mut nonce_arr = [0u8; 12];
    nonce_arr.copy_from_slice(&nonce);
    let out = chacha_xor(&data, &key_arr, &nonce_arr);
    String::from_utf8(out).map(Value::Str).map_err(|_| {
        RioError::runtime("نص غير صالح بعد فك التشفير", line, col)
    })
}

// ---------- 210: أرشفة ZIP (وحدة أرشيف جديدة) ----------
// اضغط(قائمة {اسم، محتوى}) → قائمة بايتات ZIP،
// وفك_ضغط(بايتات) → قائمة {اسم، محتوى}،
// وقائمة(بايتات) → قائمة أسماء.
// الحدود الموثقة: تخزين STORE فقط — بلا ضغط DEFLATE
// (لا ضاغط نقي عملي في النطاق)؛ وطريقة غير صفر
// تُرفض بخطأ عربي صريح؛ وتاريخ DOS ثابت (1980-01-01)
// فيجعل البايتات حتمية؛ وأسماء UTF-8؛ و1000 ملف
// كحد أقصى و10 ميبيبايت إجمالا؛ ويُتحقق من CRC32
// عند الفك فيُرفض الأرشيف التالف بخطأ عربي.

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB88320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        table[i as usize] = c;
    }
    table
}

fn crc32(data: &[u8]) -> u32 {
    let table = crc32_table();
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn zip_push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn zip_push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn zip_build(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, content) in files {
        let offset = out.len() as u32;
        let crc = crc32(content);
        let name_bytes = name.as_bytes();
        out.extend_from_slice(b"PK\x03\x04");
        zip_push_u16(&mut out, 20);
        zip_push_u16(&mut out, 0x0800);
        zip_push_u16(&mut out, 0);
        zip_push_u16(&mut out, 0);
        zip_push_u16(&mut out, 0x21);
        zip_push_u32(&mut out, crc);
        zip_push_u32(&mut out, content.len() as u32);
        zip_push_u32(&mut out, content.len() as u32);
        zip_push_u16(&mut out, name_bytes.len() as u16);
        zip_push_u16(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(content);
        central.extend_from_slice(b"PK\x01\x02");
        zip_push_u16(&mut central, 20);
        zip_push_u16(&mut central, 20);
        zip_push_u16(&mut central, 0x0800);
        zip_push_u16(&mut central, 0);
        zip_push_u16(&mut central, 0);
        zip_push_u16(&mut central, 0x21);
        zip_push_u32(&mut central, crc);
        zip_push_u32(&mut central, content.len() as u32);
        zip_push_u32(&mut central, content.len() as u32);
        zip_push_u16(&mut central, name_bytes.len() as u16);
        zip_push_u16(&mut central, 0);
        zip_push_u16(&mut central, 0);
        zip_push_u16(&mut central, 0);
        zip_push_u16(&mut central, 0);
        zip_push_u32(&mut central, 0);
        zip_push_u32(&mut central, offset);
        central.extend_from_slice(name_bytes);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    zip_push_u16(&mut out, 0);
    zip_push_u16(&mut out, 0);
    zip_push_u16(&mut out, files.len() as u16);
    zip_push_u16(&mut out, files.len() as u16);
    zip_push_u32(&mut out, cd_size);
    zip_push_u32(&mut out, cd_offset);
    zip_push_u16(&mut out, 0);
    out
}

fn zip_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn zip_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn zip_parse(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // أرشيف فارغ: سجل نهاية وحده (22 بايتا).
    if data.len() == 22 && data.starts_with(b"PK\x05\x06") {
        return Ok(Vec::new());
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        if data.len() - pos < 4 || &data[pos..pos + 4] != b"PK\x03\x04" {
            break;
        }
        if data.len() - pos < 30 {
            return Err("أرشيف تالف: ترويسة ناقصة".to_string());
        }
        let method = zip_u16(data, pos + 8).unwrap();
        if method != 0 {
            return Err("طريقة ضغط غير مدعومة — التخزين فقط".to_string());
        }
        let crc = zip_u32(data, pos + 14).unwrap();
        let size = zip_u32(data, pos + 22).unwrap() as usize;
        let name_len = zip_u16(data, pos + 26).unwrap() as usize;
        let extra_len = zip_u16(data, pos + 28).unwrap() as usize;
        let name_at = pos + 30;
        let content_at = name_at
            .checked_add(name_len)
            .and_then(|v| v.checked_add(extra_len))
            .ok_or_else(|| "أرشيف تالف".to_string())?;
        if content_at.checked_add(size).map_or(true, |e| e > data.len()) {
            return Err("أرشيف تالف: محتوى ناقص".to_string());
        }
        let name = String::from_utf8(data[name_at..name_at + name_len].to_vec())
            .map_err(|_| "أرشيف تالف: اسم غير صالح".to_string())?;
        let content = data[content_at..content_at + size].to_vec();
        if crc32(&content) != crc {
            return Err("أرشيف تالف: بصمة CRC غير مطابقة".to_string());
        }
        files.push((name, content));
        pos = content_at + size;
        if files.len() > 1000 {
            return Err("أرشيف كبير جدا".to_string());
        }
    }
    if files.is_empty() {
        return Err("أرشيف تالف: لا ملفات".to_string());
    }
    Ok(files)
}

fn zip_need_files(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<(String, Vec<u8>)>> {
    let items = need_list(display, v, line, col)?;
    if items.len() > 1000 {
        return Err(RioError::runtime("قائمة ملفات كبيرة جدا", line, col));
    }
    let mut total = 0usize;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for item in items {
        let pairs = need_dict(display, item, line, col)?;
        let name = match dict_get(pairs, "اسم") {
            Some(Value::Str(s)) => s.clone(),
            _ => {
                return Err(RioError::typ(
                    format!("الدالة {} تتوقع ملفا فيه اسم نصا", display),
                    line,
                    col,
                ))
            }
        };
        let content = match dict_get(pairs, "محتوى") {
            Some(Value::Str(s)) => s.as_bytes().to_vec(),
            Some(Value::List(_)) => need_bytes(display, &dict_get(pairs, "محتوى").unwrap().clone(), line, col)?,
            _ => {
                return Err(RioError::typ(
                    format!("الدالة {} تتوقع ملفا فيه محتوى نصا أو بايتات", display),
                    line,
                    col,
                ))
            }
        };
        if name.as_bytes().len() > 1000 || name.is_empty() {
            return Err(RioError::runtime("اسم ملف غير صالح في الأرشيف", line, col));
        }
        total += content.len();
        if total > 10 * 1024 * 1024 {
            return Err(RioError::runtime("أرشيف كبير جدا", line, col));
        }
        files.push((name, content));
    }
    Ok(files)
}

fn zip_file_dict(name: &str, content: &[u8]) -> Value {
    Value::Dict(vec![
        ("اسم".to_string(), Value::Str(name.to_string())),
        (
            "محتوى".to_string(),
            Value::Str(String::from_utf8_lossy(content).into_owned()),
        ),
    ])
}

fn archive_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اضغط", native("اضغط", archive_compress)),
        ("فك_ضغط", native("فك_ضغط", archive_decompress)),
        ("قائمة", native("قائمة", archive_list)),
    ]
}

fn archive_compress(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("أرشيف.اضغط", args, 1, line, col)?;
    let files = zip_need_files("أرشيف.اضغط", &args[0], line, col)?;
    let bytes = zip_build(&files);
    Ok(Value::List(
        bytes.into_iter().map(|b| Value::Number(b as f64)).collect(),
    ))
}

fn archive_decompress(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("أرشيف.فك_ضغط", args, 1, line, col)?;
    let data = need_bytes("أرشيف.فك_ضغط", &args[0], line, col)?;
    if data.len() > 10 * 1024 * 1024 {
        return Err(RioError::runtime("أرشيف كبير جدا", line, col));
    }
    let files = zip_parse(&data).map_err(|e| RioError::runtime(e, line, col))?;
    Ok(Value::List(
        files
            .iter()
            .map(|(n, c)| zip_file_dict(n, c))
            .collect(),
    ))
}

fn archive_list(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("أرشيف.قائمة", args, 1, line, col)?;
    let data = need_bytes("أرشيف.قائمة", &args[0], line, col)?;
    if data.len() > 10 * 1024 * 1024 {
        return Err(RioError::runtime("أرشيف كبير جدا", line, col));
    }
    let files = zip_parse(&data).map_err(|e| RioError::runtime(e, line, col))?;
    Ok(Value::List(
        files
            .into_iter()
            .map(|(n, _)| Value::Str(n))
            .collect(),
    ))
}

// ---------- 211: كلمات المرور (وحدة كلمة_مرور جديدة) ----------
// ولّد(طول، [مجموعة]): كلمة مرور من عشوائية آمنة
// (تشفير.عشوائي_آمن) بأخذ مرفوض للتحيز — فكل محرف
// محتمل بالتساوي. الطول 1..128. المجموعة الافتراضية
// حروف وأرقام ورموز ASCII. على WASM وويندوز يرفض
// بصدق كما يرفض عشوائي_آمن نفسه (بلا عشوائية آمنة
// في std هناك).
// قوة(كلمة) → عدد 0..100 حتمي: نقطتان لكل محرف
// حتى 20 محرفا (40) + 15 لكل فئة حاضرة (صغيرة،
// كبيرة، رقم، رمز) (60). مثال: «abc» = 21،
// و«Abc123!@#» = 78.

fn password_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("ولد", native("ولد", password_generate)),
        ("قوة", native("قوة", password_strength)),
    ]
}

const PASSWORD_DEFAULT_CHARSET: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*";

fn password_generate(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    if args.len() != 1 && args.len() != 2 {
        return Err(RioError::runtime(
            "الدالة كلمة_مرور.ولّد تتوقع معاملا واحدا أو معاملين (طول ومجموعة)",
            line,
            col,
        ));
    }
    let len = need_integer("كلمة_مرور.ولّد", &args[0], line, col)?;
    if !(1..=128).contains(&len) {
        return Err(RioError::typ(
            "الدالة كلمة_مرور.ولّد تتوقع طولا بين 1 و 128",
            line,
            col,
        ));
    }
    let charset: Vec<char> = match args.get(1) {
        Some(v) => {
            let s = need_string("كلمة_مرور.ولّد", v, line, col)?;
            let chars: Vec<char> = s.chars().collect();
            if chars.is_empty() || chars.len() > 256 {
                return Err(RioError::typ(
                    "الدالة كلمة_مرور.ولّد تتوقع مجموعة غير فارغة",
                    line,
                    col,
                ));
            }
            chars
        }
        None => PASSWORD_DEFAULT_CHARSET.chars().collect(),
    };
    let n = charset.len();
    let limit = 256usize - (256 % n);
    let mut out: Vec<char> = Vec::with_capacity(len as usize);
    let mut fetched = 0usize;
    while out.len() < len as usize {
        if fetched >= 4096 {
            return Err(RioError::runtime("تعذّر توليد كلمة مرور", line, col));
        }
        let bytes = crate::crypto::secure_random(64, line, col)?;
        fetched += 64;
        for b in bytes {
            if out.len() >= len as usize {
                break;
            }
            let v = b as usize;
            if v < limit {
                out.push(charset[v % n]);
            }
        }
    }
    Ok(Value::Str(out.into_iter().collect()))
}

fn password_strength(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("كلمة_مرور.قوة", args, 1, line, col)?;
    let s = need_string("كلمة_مرور.قوة", &args[0], line, col)?;
    let count = s.chars().count();
    let mut lower = false;
    let mut upper = false;
    let mut digit = false;
    let mut symbol = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() || (c.is_alphabetic() && c.is_lowercase()) {
            lower = true;
        } else if c.is_ascii_uppercase() || (c.is_alphabetic() && c.is_uppercase()) {
            upper = true;
        } else if c.is_ascii_digit() {
            digit = true;
        } else {
            symbol = true;
        }
    }
    let mut score = (count.min(20) * 2) as i64;
    if lower {
        score += 15;
    }
    if upper {
        score += 15;
    }
    if digit {
        score += 15;
    }
    if symbol {
        score += 15;
    }
    Ok(Value::Number(score.min(100) as f64))
}

// ---------- 214: بيانات وهمية (وحدة وهمي جديدة) ----------
// حتمية بالبذرة: البذرة نفسها تعطي البيانات نفسها
// في كل محرك — فتصلح للاختبار والعروض. مولد
// SplitMix64 (بلا حالة مشتركة وبلا اعتماد).
// اسم(بذرة) و بريد(بذرة) و هاتف(بذرة)
// و عنوان(بذرة) و مدينة(بذرة).

fn splitmix_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fake_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اسم", native("اسم", fake_name)),
        ("بريد", native("بريد", fake_email)),
        ("هاتف", native("هاتف", fake_phone)),
        ("عنوان", native("عنوان", fake_address)),
        ("مدينة", native("مدينة", fake_city)),
    ]
}

const FAKE_FIRST: [&str; 20] = [
    "أحمد", "محمد", "سارة", "منى", "علي", "فاطمة", "حسن", "نور", "كريم", "ليلى", "عمر",
    "هدى", "يوسف", "مريم", "خالد", "آية", "إبراهيم", "سميرة", "طارق", "رشا",
];
const FAKE_LAST: [&str; 20] = [
    "علي", "حسن", "محمد", "إبراهيم", "أحمد", "سليمان", "عبد الله", "مصطفى", "حسين",
    "عبد الرحمن", "السيد", "فرج", "نصر", "عبد العزيز", "الشريف", "حمدي", "رمضان",
    "جابر", "عبد النبي", "توفيق",
];
const FAKE_FIRST_LATIN: [&str; 20] = [
    "ahmed", "mohamed", "sara", "mona", "ali", "fatima", "hassan", "nour", "karim",
    "layla", "omar", "huda", "youssef", "mariam", "khaled", "aya", "ibrahim", "samira",
    "tarek", "rasha",
];
const FAKE_LAST_LATIN: [&str; 20] = [
    "ali", "hassan", "mohamed", "ibrahim", "ahmed", "soliman", "abdullah", "mostafa",
    "hussein", "abdelrahman", "elsayed", "farag", "nasr", "abdelaziz", "elsherif",
    "hamdi", "ramadan", "gaber", "abdelnabi", "tawfik",
];
const FAKE_CITIES: [&str; 20] = [
    "القاهرة", "الجيزة", "الإسكندرية", "أسيوط", "المنصورة", "طنطا", "الزقازيق",
    "المنيا", "سوهاج", "قنا", "الأقصر", "أسوان", "بورسعيد", "السويس", "الإسماعيلية",
    "بني سويف", "الفيوم", "دمياط", "كفر الشيخ", "البحيرة",
];
const FAKE_STREETS: [&str; 20] = [
    "شارع التحرير", "شارع الجمهورية", "شارع النيل", "شارع الجامعة", "شارع بورسعيد",
    "شارع السودان", "شارع فيصل", "شارع الهرم", "شارع رمسيس", "شارع القصر العيني",
    "شارع الأزهر", "شارع محمد علي", "شارع طلعت حرب", "شارع قصر النيل", "شارع المنيل",
    "شارع عباس العقاد", "شارع مكرم عبيد", "شارع جسر السويس", "شارع أحمد عرابي",
    "شارع البحر الأعظم",
];

fn fake_seed(display: &str, args: &[Value], line: usize, col: usize) -> RioResult<u64> {
    need_args(display, args, 1, line, col)?;
    let seed = need_integer(display, &args[0], line, col)?;
    Ok(seed as u64)
}

fn fake_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut st = fake_seed("وهمي.اسم", args, line, col)?;
    let first = FAKE_FIRST[(splitmix_next(&mut st) % 20) as usize];
    let last = FAKE_LAST[(splitmix_next(&mut st) % 20) as usize];
    Ok(Value::Str(format!("{} {}", first, last)))
}

fn fake_email(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut st = fake_seed("وهمي.بريد", args, line, col)? ^ 0xA076_1D64_78BD_642F;
    let first = FAKE_FIRST_LATIN[(splitmix_next(&mut st) % 20) as usize];
    let last = FAKE_LAST_LATIN[(splitmix_next(&mut st) % 20) as usize];
    let num = splitmix_next(&mut st) % 90 + 10;
    Ok(Value::Str(format!(
        "{}.{}{}@example.com",
        first, last, num
    )))
}

fn fake_phone(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut st = fake_seed("وهمي.هاتف", args, line, col)? ^ 0xE703_7ED1_A0B4_28DB;
    let prefixes = ["010", "011", "012", "015"];
    let prefix = prefixes[(splitmix_next(&mut st) % 4) as usize];
    let mut digits = String::new();
    for _ in 0..8 {
        digits.push(char::from_digit((splitmix_next(&mut st) % 10) as u32, 10).unwrap());
    }
    Ok(Value::Str(format!("+20 {}{}", prefix, digits)))
}

fn fake_address(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut st = fake_seed("وهمي.عنوان", args, line, col)? ^ 0x8EBC_6AF0_9C88_C6E3;
    let street = FAKE_STREETS[(splitmix_next(&mut st) % 20) as usize];
    let num = splitmix_next(&mut st) % 200 + 1;
    let city = FAKE_CITIES[(splitmix_next(&mut st) % 20) as usize];
    Ok(Value::Str(format!("{} {}، {}", street, num, city)))
}

fn fake_city(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut st = fake_seed("وهمي.مدينة", args, line, col)? ^ 0x5899_65CC_7537_4CC3;
    Ok(Value::Str(
        FAKE_CITIES[(splitmix_next(&mut st) % 20) as usize].to_string(),
    ))
}

// ---------- 215: التقويم الهجري/الميلادي (وحدة تقويم جديدة) ----------
// خوارزمية الكويت (تقويم أم القرى الحسابي):
// هجري_إلى_ميلادي وميلادي_إلى_هجري يعيدان قائمة
// [سنة، شهر، يوم]. التقويم حسابي — قد يختلف يوما
// واحدا عن الرؤية الفعلية للهلال. سنة هجرية كبيسة
// إذا كان (11*سنة+14) % 30 < 11 (30 سنة دورة).

fn hijri_is_leap(y: i64) -> bool {
    (11 * y + 14) % 30 < 11
}

fn hijri_month_days(y: i64, m: i64) -> i64 {
    if m == 12 && hijri_is_leap(y) {
        30
    } else if m % 2 == 1 {
        30
    } else {
        29
    }
}

fn hijri_to_jd(y: i64, m: i64, d: i64) -> i64 {
    (d - 1) + (m - 1) * 29 + (m - 1) / 2 + (y - 1) * 354 + (3 + 11 * y) / 30 + 1948440
}

fn jd_to_hijri(jd: i64) -> (i64, i64, i64) {
    let days = jd - 1948440;
    let year = (30 * days + 10646) / 10631;
    let start = (year - 1) * 354 + (3 + 11 * year) / 30;
    let mut day = days - start + 1;
    let mut month = 1i64;
    loop {
        let dim = hijri_month_days(year, month);
        if day <= dim {
            break;
        }
        day -= dim;
        month += 1;
    }
    (year, month, day)
}

fn greg_to_jd(y: i64, m: u32, d: u32) -> i64 {
    crate::time::days_from_civil(y, m, d) + 2440588
}

fn jd_to_greg(jd: i64) -> (i64, u32, u32) {
    crate::time::civil_from_days(jd - 2440588)
}

fn calendar_entries() -> Vec<(&'static str, Value)> {
    vec![
        (
            "هجري_إلى_ميلادي",
            native("هجري_إلى_ميلادي", calendar_hijri_to_greg),
        ),
        (
            "ميلادي_إلى_هجري",
            native("ميلادي_إلى_هجري", calendar_greg_to_hijri),
        ),
        (
            "اسم_شهر_هجري",
            native("اسم_شهر_هجري", calendar_hijri_month_name),
        ),
        (
            "كبيسة_هجرية",
            native("كبيسة_هجرية", calendar_hijri_leap),
        ),
    ]
}

const HIJRI_MONTHS: [&str; 12] = [
    "محرم", "صفر", "ربيع الأول", "ربيع الآخر", "جمادى الأولى", "جمادى الآخرة", "رجب",
    "شعبان", "رمضان", "شوال", "ذو القعدة", "ذو الحجة",
];

fn calendar_hijri_to_greg(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تقويم.هجري_إلى_ميلادي", args, 3, line, col)?;
    let y = need_integer("تقويم.هجري_إلى_ميلادي", &args[0], line, col)?;
    let m = need_integer("تقويم.هجري_إلى_ميلادي", &args[1], line, col)?;
    let d = need_integer("تقويم.هجري_إلى_ميلادي", &args[2], line, col)?;
    if !(1..=3000).contains(&y) || !(1..=12).contains(&m) {
        return Err(RioError::typ(
            "الدالة تقويم.هجري_إلى_ميلادي تتوقع سنة هجرية بين 1 و 3000 وشهرا بين 1 و 12",
            line,
            col,
        ));
    }
    if d < 1 || d > hijri_month_days(y, m) {
        return Err(RioError::typ(
            "الدالة تقويم.هجري_إلى_ميلادي تتوقع يوما صالحا في هذا الشهر الهجري",
            line,
            col,
        ));
    }
    let (gy, gm, gd) = jd_to_greg(hijri_to_jd(y, m, d));
    Ok(Value::List(vec![
        Value::Number(gy as f64),
        Value::Number(gm as f64),
        Value::Number(gd as f64),
    ]))
}

fn calendar_greg_to_hijri(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تقويم.ميلادي_إلى_هجري", args, 3, line, col)?;
    let y = need_integer("تقويم.ميلادي_إلى_هجري", &args[0], line, col)?;
    let m = need_integer("تقويم.ميلادي_إلى_هجري", &args[1], line, col)?;
    let d = need_integer("تقويم.ميلادي_إلى_هجري", &args[2], line, col)?;
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > 31 {
        return Err(RioError::typ(
            "الدالة تقويم.ميلادي_إلى_هجري تتوقع تاريخا ميلاديا صالحا",
            line,
            col,
        ));
    }
    let (ry, rm, rd) = crate::time::civil_from_days(crate::time::days_from_civil(
        y,
        m as u32,
        d as u32,
    ));
    if ry != y || rm != m as u32 || rd != d as u32 {
        return Err(RioError::typ(
            "الدالة تقويم.ميلادي_إلى_هجري تتوقع يوما صالحا في هذا الشهر الميلادي",
            line,
            col,
        ));
    }
    let (hy, hm, hd) = jd_to_hijri(greg_to_jd(y, m as u32, d as u32));
    Ok(Value::List(vec![
        Value::Number(hy as f64),
        Value::Number(hm as f64),
        Value::Number(hd as f64),
    ]))
}

fn calendar_hijri_month_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تقويم.اسم_شهر_هجري", args, 1, line, col)?;
    let m = need_integer("تقويم.اسم_شهر_هجري", &args[0], line, col)?;
    if !(1..=12).contains(&m) {
        return Err(RioError::typ(
            "الدالة تقويم.اسم_شهر_هجري تتوقع شهرا بين 1 و 12",
            line,
            col,
        ));
    }
    Ok(Value::Str(HIJRI_MONTHS[(m - 1) as usize].to_string()))
}

fn calendar_hijri_leap(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("تقويم.كبيسة_هجرية", args, 1, line, col)?;
    let y = need_integer("تقويم.كبيسة_هجرية", &args[0], line, col)?;
    if y < 1 {
        return Err(RioError::typ(
            "الدالة تقويم.كبيسة_هجرية تتوقع سنة هجرية موجبة",
            line,
            col,
        ));
    }
    Ok(Value::Bool(hijri_is_leap(y)))
}

// ---------- 216: الألوان (وحدة ألوان جديدة) ----------
// إلى_صبغة(ر،غ،ب) → [صبغة 0..360، تشبع 0..1، قيمة 0..1]
// ومن_صبغة(صبغة، تشبع، قيمة) → [ر،غ،ب] أعداد صحيحة.
// وإلى_سداسي(ر،غ،ب) → "#rrggbb"؛ ومن_سداسي(نص)
// → [ر،غ،ب]. ومزج(لون1، لون2، نسبة 0..1) → [ر،غ،ب]
// باستيفاء خطي وتقريب نصف بعيد عن الصفر.

fn colors_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("إلى_صبغة", native("إلى_صبغة", colors_to_hsv)),
        ("من_صبغة", native("من_صبغة", colors_from_hsv)),
        ("إلى_سداسي", native("إلى_سداسي", colors_to_hex)),
        ("من_سداسي", native("من_سداسي", colors_from_hex)),
        ("مزج", native("مزج", colors_mix)),
    ]
}

fn colors_need_rgb(
    display: &str,
    args: &[Value],
    at: usize,
    line: usize,
    col: usize,
) -> RioResult<(f64, f64, f64)> {
    let items = need_list(display, &args[at], line, col)?;
    if items.len() != 3 {
        return Err(RioError::typ(
            format!("الدالة {} تتوقع لونا [ر، غ، ب]", display),
            line,
            col,
        ));
    }
    let mut rgb = [0f64; 3];
    for (i, v) in items.iter().enumerate() {
        let n = need_number(display, v, line, col)?;
        if !(0.0..=255.0).contains(&n) {
            return Err(RioError::typ(
                format!("الدالة {} تتوقع كل قناة لون بين 0 و 255", display),
                line,
                col,
            ));
        }
        rgb[i] = n;
    }
    Ok((rgb[0], rgb[1], rgb[2]))
}

fn colors_need_rgb_ints(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<(i64, i64, i64)> {
    let mut out = [0i64; 3];
    for i in 0..3 {
        let n = need_integer(display, &args[i], line, col)?;
        if !(0..=255).contains(&n) {
            return Err(RioError::typ(
                format!("الدالة {} تتوقع كل قناة لون بين 0 و 255", display),
                line,
                col,
            ));
        }
        out[i] = n;
    }
    Ok((out[0], out[1], out[2]))
}

fn colors_to_hsv(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("ألوان.إلى_صبغة", args, 3, line, col)?;
    let (r, g, b) = colors_need_rgb_ints("ألوان.إلى_صبغة", args, line, col)?;
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    let sat = if max == 0.0 { 0.0 } else { delta / max };
    Ok(Value::List(vec![
        Value::Number(hue),
        Value::Number(sat),
        Value::Number(max),
    ]))
}

fn colors_from_hsv(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("ألوان.من_صبغة", args, 3, line, col)?;
    let h = need_number("ألوان.من_صبغة", &args[0], line, col)?;
    let s = need_number("ألوان.من_صبغة", &args[1], line, col)?;
    let v = need_number("ألوان.من_صبغة", &args[2], line, col)?;
    if !(0.0..360.0).contains(&h) && h != 360.0 {
        return Err(RioError::typ(
            "الدالة ألوان.من_صبغة تتوقع صبغة بين 0 و 360",
            line,
            col,
        ));
    }
    if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&v) {
        return Err(RioError::typ(
            "الدالة ألوان.من_صبغة تتوقع تشبعا وقيمة بين 0 و 1",
            line,
            col,
        ));
    }
    let h = if h == 360.0 { 0.0 } else { h };
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Ok(Value::List(vec![
        Value::Number(((r + m) * 255.0).round()),
        Value::Number(((g + m) * 255.0).round()),
        Value::Number(((b + m) * 255.0).round()),
    ]))
}

fn colors_to_hex(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("ألوان.إلى_سداسي", args, 3, line, col)?;
    let (r, g, b) = colors_need_rgb_ints("ألوان.إلى_سداسي", args, line, col)?;
    Ok(Value::Str(format!("#{:02x}{:02x}{:02x}", r, g, b)))
}

fn colors_from_hex(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("ألوان.من_سداسي", args, 1, line, col)?;
    let text = need_string("ألوان.من_سداسي", &args[0], line, col)?;
    let hex = text.strip_prefix('#').unwrap_or(&text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(RioError::runtime("لون سداسي غير صالح: توقع #rrggbb", line, col));
    }
    let parse = |s: &str| i64::from_str_radix(s, 16).unwrap();
    Ok(Value::List(vec![
        Value::Number(parse(&hex[0..2]) as f64),
        Value::Number(parse(&hex[2..4]) as f64),
        Value::Number(parse(&hex[4..6]) as f64),
    ]))
}

fn colors_mix(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("ألوان.مزج", args, 3, line, col)?;
    let (r1, g1, b1) = colors_need_rgb("ألوان.مزج", args, 0, line, col)?;
    let (r2, g2, b2) = colors_need_rgb("ألوان.مزج", args, 1, line, col)?;
    let t = need_number("ألوان.مزج", &args[2], line, col)?;
    if !(0.0..=1.0).contains(&t) {
        return Err(RioError::typ(
            "الدالة ألوان.مزج تتوقع نسبة بين 0 و 1",
            line,
            col,
        ));
    }
    Ok(Value::List(vec![
        Value::Number((r1 * (1.0 - t) + r2 * t).round()),
        Value::Number((g1 * (1.0 - t) + g2 * t).round()),
        Value::Number((b1 * (1.0 - t) + b2 * t).round()),
    ]))
}

// ---------- 217: جدول الإيموجي (وحدة إيموجي جديدة) ----------
// رمز(اسم) → إيموجي؛ واسم(رمز) → اسم عربي أو لاشيء؛
// وقائمة() → كل الأسماء بالترتيب؛ وابحث(نص)
// → الأسماء الحاوية للنص. 40 رمزا بأسمائها العربية.

fn emoji_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("رمز", native("رمز", emoji_symbol)),
        ("اسم", native("اسم", emoji_name)),
        ("قائمة", native("قائمة", emoji_list)),
        ("ابحث", native("ابحث", emoji_search)),
    ]
}

const EMOJI_TABLE: [(&str, &str); 40] = [
    ("وجه مبتسم", "😊"),
    ("وجه ضاحك", "😂"),
    ("وجه حزين", "😢"),
    ("وجه غاضب", "😠"),
    ("وجه نائم", "😴"),
    ("وجه مريض", "🤒"),
    ("وجه عبقري", "🤓"),
    ("قلب", "❤"),
    ("نار", "🔥"),
    ("شمس", "☀"),
    ("قمر", "🌙"),
    ("نجمة", "⭐"),
    ("وردة", "🌹"),
    ("زهرة", "🌸"),
    ("شجرة", "🌳"),
    ("قطة", "🐱"),
    ("كلب", "🐶"),
    ("طائر", "🐦"),
    ("سمكة", "🐟"),
    ("تفاحة", "🍎"),
    ("قهوة", "☕"),
    ("كتاب", "📖"),
    ("سيارة", "🚗"),
    ("بيت", "🏠"),
    ("موسيقى", "🎵"),
    ("كرة", "⚽"),
    ("هدية", "🎁"),
    ("تاج", "👑"),
    ("كاميرا", "📷"),
    ("هاتف", "📱"),
    ("حاسوب", "💻"),
    ("مصباح", "💡"),
    ("مفتاح", "🔑"),
    ("ساعة", "⏰"),
    ("تقويم", "📅"),
    ("علم", "🚩"),
    ("طائرة", "✈"),
    ("سفينة", "🚢"),
    ("جبل", "⛰"),
    ("بحر", "🌊"),
];

fn emoji_symbol(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("إيموجي.رمز", args, 1, line, col)?;
    let name = need_string("إيموجي.رمز", &args[0], line, col)?;
    for (n, e) in EMOJI_TABLE {
        if n == name {
            return Ok(Value::Str(e.to_string()));
        }
    }
    Err(RioError::runtime(
        format!("اسم إيموجي غير موجود: {}", name),
        line,
        col,
    ))
}

fn emoji_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("إيموجي.اسم", args, 1, line, col)?;
    let sym = need_string("إيموجي.اسم", &args[0], line, col)?;
    for (n, e) in EMOJI_TABLE {
        if e == sym {
            return Ok(Value::Str(n.to_string()));
        }
    }
    Ok(Value::Nil)
}

fn emoji_list(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("إيموجي.قائمة", args, 0, line, col)?;
    Ok(Value::List(
        EMOJI_TABLE
            .iter()
            .map(|(n, _)| Value::Str(n.to_string()))
            .collect(),
    ))
}

fn emoji_search(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("إيموجي.ابحث", args, 1, line, col)?;
    let q = need_string("إيموجي.ابحث", &args[0], line, col)?;
    Ok(Value::List(
        EMOJI_TABLE
            .iter()
            .filter(|(n, _)| n.contains(&q))
            .map(|(n, _)| Value::Str(n.to_string()))
            .collect(),
    ))
}

// ---------- J10 الأساسي (الدفعة الرابعة من برنامج الـ579، الاجتماع 73 §88):
// البنود 157 (في رياضيات أعلاه) و159–165 — وحدات أصلية نقية، Rust نقي.
// رسائل الأخطاء ثابتة الصياغة عمدا (بلا تفاصيل موضعية) ليتطابق
// نصها حرفيا بين المفسر وآلة البايت كود والثنائي الأصلي.

// ---------- 159: الطي (وحدة قوائم جديدة) ----------
// اطوِ_بـ(قائمة، ابتدائي، دالة/عملية): تطبق دالة التجميع
// على المتراكم وكل عنصر بالترتيب، وتبدأ من الابتدائي؛
// والقائمة الفارغة تعيد الابتدائي كما هو.
// دالة التجميع دالة نيفا بمعامل واحد: قائمة [المتراكم، العنصر]
// (سياق الاستدعاء الأصلي يمرر معاملا واحدا — سابقة شبكة.اخدم،
// فتُمرر الثنائية كقائمة)؛ أو نص عملية مدمجة: "جمع" أو "ضرب"
// أو "أكبر" أو "أصغر" (أعداد فقط).
// المفتاح المخزن «اطو_ب»: المحلل يجرّد الكسرة والتطويل
// وقت التحليل، فيُكتب الاسم في المصدر بأي التهجئتين
// (اطوِ_بـ أو اطو_ب) ويُحل إلى المفتاح نفسه.
// هذا هو اطوِ_بـ نفسه من البند J3-55 — يُبنى هنا مرة
// واحدة، والبنّاء هـ يربطه فقط ولا يعيد بناءه.
// لا تصادم مع ملف مدمج: لا ملف قوائم.نيفا في engine/src/حزم/
// (الملفات المدمجة عشرة فقط: أوامر، إصدار، بيان، حلال، سجل،
// قفل، مخزن، مشروع، نشر، نص) — فالوحدة الأصلية آمنة.

fn lists_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اطو_ب", native("اطو_ب", lists_fold)),
        // J3 — إكمال وحدة قوائم: 56 و57 و58 و59 و60 و73.
        // (55 ربط فقط: اطوِ_بـ مبنية هنا أصلا، انظر آخر الملف.)
        ("جدول_ب", native("جدول_ب", lists_table)),
        ("جمع_حسب", native("جمع_حسب", lists_group_by)),
        ("بادئات", native("بادئات", lists_prefixes)),
        ("لاحقات", native("لاحقات", lists_suffixes)),
        ("مجاميع_تراكمية", native("مجاميع_تراكمية", lists_cumulative)),
        ("نوافذ", native("نوافذ", lists_windows)),
        ("خارج_النوافذ", native("خارج_النوافذ", lists_windows_outside)),
        ("رتب", native("رتب", lists_sort)),
        ("رتب_تنازليا", native("رتب_تنازليا", lists_sort_desc)),
        ("قطع", native("قطع", lists_chunk)),
        ("قطع_بفاصل", native("قطع_بفاصل", lists_split_sep)),
        // البند 133 (الدفعة 2 من الـ579): القواطع.
        ("شريحة", native("شريحة", lists_slice)),
    ]
}

fn lists_fold(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.اطو_ب", args, 3, line, col)?;
    let items = need_list("قوائم.اطو_ب", &args[0], line, col)?.clone();
    let mut acc = args[1].clone();
    match &args[2] {
        Value::Str(op) => {
            for item in &items {
                let x = need_number("قوائم.اطو_ب", &acc, line, col)?;
                let y = need_number("قوائم.اطو_ب", item, line, col)?;
                acc = Value::Number(match op.as_str() {
                    "جمع" => x + y,
                    "ضرب" => x * y,
                    "أكبر" => x.max(y),
                    "أصغر" => x.min(y),
                    _ => {
                        return Err(RioError::runtime(
                            format!("عملية طي غير معروفة: {} — المتاح: جمع، ضرب، أكبر، أصغر", op),
                            line,
                            col,
                        ))
                    }
                });
            }
            Ok(acc)
        }
        func => {
            for item in items {
                let pair = Value::List(vec![acc.clone(), item]);
                let (res, produced) = ctx.call_handler(func, pair, line, col);
                if !produced.is_empty() {
                    print!("{}", produced);
                }
                acc = res?;
            }
            Ok(acc)
        }
    }
}

/// حد شريحة: لاشيء غائب، وإلا عدد صحيح (البند 133).
fn slice_bound(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Option<i64>> {
    match v {
        Value::Nil => Ok(None),
        _ => Ok(Some(need_integer(display, v, line, col)?)),
    }
}

/// فهارس الشريحة بمنطق بايثون (PySlice_AdjustIndices):
/// خطوة موجبة: البداية ٠ والنهاية الطول افتراضا، والقص
/// إلى [٠، الطول]؛ وخطوة سالبة: البداية الطول-١ والنهاية
/// «قبل الصفر» افتراضا، والقص إلى [-١، الطول-١]؛ والسالب
/// الصريح يزاح بالطول قبل القص (فالنهاية -١ الصريحة تعني
/// آخر عنصر، لا «قبل الصفر»).
fn slice_indices(len: i64, start: Option<i64>, end: Option<i64>, step: i64) -> Vec<usize> {
    let mut out = Vec::new();
    if step > 0 {
        let mut s = match start {
            Some(x) => if x < 0 { x + len } else { x },
            None => 0,
        };
        let mut e = match end {
            Some(x) => if x < 0 { x + len } else { x },
            None => len,
        };
        s = s.clamp(0, len);
        e = e.clamp(0, len);
        let mut i = s;
        while i < e {
            out.push(i as usize);
            i += step;
        }
    } else {
        let mut s = match start {
            Some(x) => if x < 0 { x + len } else { x },
            None => len - 1,
        };
        let mut e = match end {
            Some(x) => if x < 0 { x + len } else { x },
            None => -1,
        };
        s = s.clamp(-1, len - 1);
        e = e.clamp(-1, len - 1);
        let mut i = s;
        while i > e {
            out.push(i as usize);
            i += step;
        }
    }
    out
}

/// قوائم.شريحة(قيمة، بداية، نهاية، خطوة) — أخذ جزء من
/// قائمة أو نص (البند 133، الدفعة 2 من الـ579): الحدود
/// لاشيء للغائب، والخطوة لاشيء للواحد؛ وخطوة صفر خطأ؛
/// والنص يُقطع بالمحارف لا بالبايتات؛ وغير القائمة
/// والنص خطأ نوع.
fn lists_slice(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "قوائم.شريحة";
    need_args(D, args, 4, line, col)?;
    let start = slice_bound(D, &args[1], line, col)?;
    let end = slice_bound(D, &args[2], line, col)?;
    let step = match &args[3] {
        Value::Nil => 1,
        v => need_integer(D, v, line, col)?,
    };
    if step == 0 {
        return Err(RioError::runtime("خطوة الشريحة صفر في قوائم.شريحة", line, col));
    }
    match &args[0] {
        Value::List(items) => {
            let idx = slice_indices(items.len() as i64, start, end, step);
            Ok(Value::List(idx.iter().map(|&i| items[i].clone()).collect()))
        }
        Value::Str(text) => {
            let chars: Vec<char> = text.chars().collect();
            let idx = slice_indices(chars.len() as i64, start, end, step);
            Ok(Value::Str(idx.iter().map(|&i| chars[i]).collect()))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة قوائم.شريحة تتوقع قائمة أو نصا لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

// ---------- 160: الآلة الحاسبة العلمية (وحدة علمي جديدة) ----------
// النطاق الموثق: 11 ثابتا فيزيائيا بقيم SI (CODATA 2018
// حيث تنطبق؛ وسرعة الضوء قيمة معرّفة بالضبط، وتسارع
// الجاذبية القيمة القياسية المتفق عليها) + خطاف الوحدات
// وحدة_الثابت(اسم) الذي يعيد وحدة كل ثابت في SI.
// خارج النطاق: أي ثابت آخر (كيميائي أو فلكي أو غيره)،
// وأي محرك وحدات — التحويل بين الوحدات في وحدة وحدات.

fn science_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("سرعة_الضوء", Value::Number(299792458.0)),
        ("ثابت_الجاذبية", Value::Number(6.67430e-11)),
        ("ثابت_بلانك", Value::Number(6.62607015e-34)),
        ("شحنة_العنصر", Value::Number(1.602176634e-19)),
        ("ثابت_أفوجادرو", Value::Number(6.02214076e23)),
        ("ثابت_بولتزمان", Value::Number(1.380649e-23)),
        ("كتلة_الإلكترون", Value::Number(9.1093837015e-31)),
        ("كتلة_البروتون", Value::Number(1.67262192369e-27)),
        ("ثابت_الغاز", Value::Number(8.314462618)),
        ("تسارع_الجاذبية", Value::Number(9.80665)),
        ("ثابت_ستيفان_بولتزمان", Value::Number(5.670374419e-8)),
        ("وحدة_الثابت", native("وحدة_الثابت", science_unit)),
    ]
}

fn science_unit(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("علمي.وحدة_الثابت", args, 1, line, col)?;
    let name = need_string("علمي.وحدة_الثابت", &args[0], line, col)?;
    let unit = match name.as_str() {
        "سرعة_الضوء" => "متر/ثانية",
        "ثابت_الجاذبية" => "متر مكعب/كيلوغرام/ثانية مربعة",
        "ثابت_بلانك" => "جول ثانية",
        "شحنة_العنصر" => "كولوم",
        "ثابت_أفوجادرو" => "1/مول",
        "ثابت_بولتزمان" => "جول/كلفن",
        "كتلة_الإلكترون" => "كيلوغرام",
        "كتلة_البروتون" => "كيلوغرام",
        "ثابت_الغاز" => "جول/مول/كلفن",
        "تسارع_الجاذبية" => "متر/ثانية مربعة",
        "ثابت_ستيفان_بولتزمان" => "واط/متر مربع/كلفن أس 4",
        _ => {
            return Err(RioError::runtime(
                format!("ثابت علمي غير موجود: {}", name),
                line,
                col,
            ))
        }
    };
    Ok(Value::Str(unit.to_string()))
}

// ---------- 162: الفائدة المركبة (وحدة تمويل جديدة) ----------
// فائدة_مركبة(أصل، معدل_سنوي، سنوات، مرات_في_السنة):
// أصل × (1 + معدل/مرات)^(مرات×سنوات)؛ والنسخة المستمرة
// أصل × ه^(معدل×سنوات)؛ وقيمة_حالية عكسها؛ ومعدل_فعلي_سنوي
// (1 + معدل/مرات)^مرات - 1؛ وقسط_شهري قسط القرض المتساوي.
// الشروط: سنوات ≥ 0، ومرات عدد صحيح ≥ 1، وشهور عدد صحيح
// ≥ 1، و(1 + معدل/مرات) > 0؛ والمعدل صفر يعيد الأصل
// نفسه في القسط (قسط = أصل/شهور).

fn finance_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("فائدة_مركبة", native("فائدة_مركبة", finance_compound)),
        ("فائدة_مركبة_مستمرة", native("فائدة_مركبة_مستمرة", finance_continuous)),
        ("قيمة_حالية", native("قيمة_حالية", finance_present)),
        ("معدل_فعلي_سنوي", native("معدل_فعلي_سنوي", finance_effective)),
        ("قسط_شهري", native("قسط_شهري", finance_payment)),
    ]
}

fn finance_check_rate(rate: f64, times: i64, line: usize, col: usize) -> RioResult<()> {
    if times < 1 {
        return Err(RioError::runtime("مرات التركيب في السنة يجب أن تكون عددا صحيحا ≥ 1", line, col));
    }
    if 1.0 + rate / times as f64 <= 0.0 {
        return Err(RioError::runtime("المعدل يجعل أساس الفائدة غير موجب", line, col));
    }
    Ok(())
}

fn finance_compound(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تمويل.فائدة_مركبة", args, 4, line, col)?;
    let principal = need_number("تمويل.فائدة_مركبة", &args[0], line, col)?;
    let rate = need_number("تمويل.فائدة_مركبة", &args[1], line, col)?;
    let years = need_number("تمويل.فائدة_مركبة", &args[2], line, col)?;
    let times = need_integer("تمويل.فائدة_مركبة", &args[3], line, col)?;
    if years < 0.0 {
        return Err(RioError::runtime("السنوات يجب أن تكون ≥ 0", line, col));
    }
    finance_check_rate(rate, times, line, col)?;
    let n = times as f64;
    Ok(Value::Number(principal * (1.0 + rate / n).powf(n * years)))
}

fn finance_continuous(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تمويل.فائدة_مركبة_مستمرة", args, 3, line, col)?;
    let principal = need_number("تمويل.فائدة_مركبة_مستمرة", &args[0], line, col)?;
    let rate = need_number("تمويل.فائدة_مركبة_مستمرة", &args[1], line, col)?;
    let years = need_number("تمويل.فائدة_مركبة_مستمرة", &args[2], line, col)?;
    if years < 0.0 {
        return Err(RioError::runtime("السنوات يجب أن تكون ≥ 0", line, col));
    }
    Ok(Value::Number(principal * (rate * years).exp()))
}

fn finance_present(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تمويل.قيمة_حالية", args, 4, line, col)?;
    let future = need_number("تمويل.قيمة_حالية", &args[0], line, col)?;
    let rate = need_number("تمويل.قيمة_حالية", &args[1], line, col)?;
    let years = need_number("تمويل.قيمة_حالية", &args[2], line, col)?;
    let times = need_integer("تمويل.قيمة_حالية", &args[3], line, col)?;
    if years < 0.0 {
        return Err(RioError::runtime("السنوات يجب أن تكون ≥ 0", line, col));
    }
    finance_check_rate(rate, times, line, col)?;
    let n = times as f64;
    Ok(Value::Number(future / (1.0 + rate / n).powf(n * years)))
}

fn finance_effective(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تمويل.معدل_فعلي_سنوي", args, 2, line, col)?;
    let rate = need_number("تمويل.معدل_فعلي_سنوي", &args[0], line, col)?;
    let times = need_integer("تمويل.معدل_فعلي_سنوي", &args[1], line, col)?;
    finance_check_rate(rate, times, line, col)?;
    let n = times as f64;
    Ok(Value::Number((1.0 + rate / n).powf(n) - 1.0))
}

fn finance_payment(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تمويل.قسط_شهري", args, 3, line, col)?;
    let principal = need_number("تمويل.قسط_شهري", &args[0], line, col)?;
    let rate = need_number("تمويل.قسط_شهري", &args[1], line, col)?;
    let months = need_integer("تمويل.قسط_شهري", &args[2], line, col)?;
    if months < 1 {
        return Err(RioError::runtime("الشهور يجب أن تكون عددا صحيحا ≥ 1", line, col));
    }
    let i = rate / 12.0;
    if i == 0.0 {
        return Ok(Value::Number(principal / months as f64));
    }
    if 1.0 + i <= 0.0 {
        return Err(RioError::runtime("المعدل يجعل أساس القسط غير موجب", line, col));
    }
    Ok(Value::Number(principal * i / (1.0 - (1.0 + i).powf(-(months as f64)))))
}

// ---------- 163: أساسيات اكتوارية (وحدة اكتواري جديدة) ----------
// النطاق الموثق: القيمة الزمنية للنقود للدفعات الدورية
// المتساوية فقط (عدة المعاشات والتأمين): قيمة حالية
// ومستقبلية للدفعات، وقسط من قيمة حالية أو مستقبلية.
// خارج النطاق: جداول الحياة والوفيات (تحتاج جداول
// خارجية لم تُبنَ بعد)، ونماذج المخاطر، والاحتياطيات.
// الشروط: فترات عدد صحيح ≥ 1، والمعدل > -1؛ والمعدل
// صفر يعيد الحساب الخطي (قسط × فترات).

fn actuarial_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("قيمة_حالية_للدفعات", native("قيمة_حالية_للدفعات", actuarial_pv)),
        ("قيمة_مستقبلية_للدفعات", native("قيمة_مستقبلية_للدفعات", actuarial_fv)),
        ("قسط_من_قيمة_حالية", native("قسط_من_قيمة_حالية", actuarial_pmt_pv)),
        ("قسط_من_قيمة_مستقبلية", native("قسط_من_قيمة_مستقبلية", actuarial_pmt_fv)),
    ]
}

fn actuarial_check(rate: f64, periods: i64, line: usize, col: usize) -> RioResult<()> {
    if periods < 1 {
        return Err(RioError::runtime("الفترات يجب أن تكون عددا صحيحا ≥ 1", line, col));
    }
    if 1.0 + rate <= 0.0 {
        return Err(RioError::runtime("المعدل يجب أن يكون > -1", line, col));
    }
    Ok(())
}

fn actuarial_pv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكتواري.قيمة_حالية_للدفعات", args, 3, line, col)?;
    let pmt = need_number("اكتواري.قيمة_حالية_للدفعات", &args[0], line, col)?;
    let rate = need_number("اكتواري.قيمة_حالية_للدفعات", &args[1], line, col)?;
    let periods = need_integer("اكتواري.قيمة_حالية_للدفعات", &args[2], line, col)?;
    actuarial_check(rate, periods, line, col)?;
    let n = periods as f64;
    if rate == 0.0 {
        return Ok(Value::Number(pmt * n));
    }
    Ok(Value::Number(pmt * (1.0 - (1.0 + rate).powf(-n)) / rate))
}

fn actuarial_fv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكتواري.قيمة_مستقبلية_للدفعات", args, 3, line, col)?;
    let pmt = need_number("اكتواري.قيمة_مستقبلية_للدفعات", &args[0], line, col)?;
    let rate = need_number("اكتواري.قيمة_مستقبلية_للدفعات", &args[1], line, col)?;
    let periods = need_integer("اكتواري.قيمة_مستقبلية_للدفعات", &args[2], line, col)?;
    actuarial_check(rate, periods, line, col)?;
    let n = periods as f64;
    if rate == 0.0 {
        return Ok(Value::Number(pmt * n));
    }
    Ok(Value::Number(pmt * ((1.0 + rate).powf(n) - 1.0) / rate))
}

fn actuarial_pmt_pv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكتواري.قسط_من_قيمة_حالية", args, 3, line, col)?;
    let pv = need_number("اكتواري.قسط_من_قيمة_حالية", &args[0], line, col)?;
    let rate = need_number("اكتواري.قسط_من_قيمة_حالية", &args[1], line, col)?;
    let periods = need_integer("اكتواري.قسط_من_قيمة_حالية", &args[2], line, col)?;
    actuarial_check(rate, periods, line, col)?;
    let n = periods as f64;
    if rate == 0.0 {
        return Ok(Value::Number(pv / n));
    }
    Ok(Value::Number(pv * rate / (1.0 - (1.0 + rate).powf(-n))))
}

fn actuarial_pmt_fv(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("اكتواري.قسط_من_قيمة_مستقبلية", args, 3, line, col)?;
    let fv = need_number("اكتواري.قسط_من_قيمة_مستقبلية", &args[0], line, col)?;
    let rate = need_number("اكتواري.قسط_من_قيمة_مستقبلية", &args[1], line, col)?;
    let periods = need_integer("اكتواري.قسط_من_قيمة_مستقبلية", &args[2], line, col)?;
    actuarial_check(rate, periods, line, col)?;
    let n = periods as f64;
    if rate == 0.0 {
        return Ok(Value::Number(fv / n));
    }
    Ok(Value::Number(fv * rate / ((1.0 + rate).powf(n) - 1.0)))
}

// ---------- 164: الإحصاء الأساسي (وحدة إحصاء جديدة) ----------
// متوسط، وسيط، منوال، تباين وانحراف معياري (للمجتمع
// وللعينة)، مجموع، مدى. الوسيط بترتيب نسخة؛ والمنوال
// القيمة الأكثر تكرارا، وعند التساوي يعيد أصغرها؛
// وانحراف العينة يقسم على (ن-1) فيرفض عينة من عنصر
// واحد؛ والقائمة الفارغة أو غير العددية أو غير المنتهية
// (لانهاية أو لاعدد) خطأ عربي.

fn stats_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("متوسط", native("متوسط", stats_mean)),
        ("وسيط", native("وسيط", stats_median)),
        ("منوال", native("منوال", stats_mode)),
        ("انحراف_معياري", native("انحراف_معياري", stats_stddev)),
        ("انحراف_معياري_للعينة", native("انحراف_معياري_للعينة", stats_sample_stddev)),
        ("تباين", native("تباين", stats_variance)),
        ("تباين_العينة", native("تباين_العينة", stats_sample_variance)),
        ("مجموع", native("مجموع", stats_sum)),
        ("مدى", native("مدى", stats_range)),
    ]
}

/// قائمة أعداد غير فارغة ومنتهية، بالترتيب نفسه.
fn stats_numbers(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Vec<f64>> {
    let items = need_list(display, v, line, col)?;
    if items.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع قائمة غير فارغة", display),
            line,
            col,
        ));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let x = need_number(display, item, line, col)?;
        if !x.is_finite() {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع أعدادا منتهية", display),
                line,
                col,
            ));
        }
        out.push(x);
    }
    Ok(out)
}

fn stats_mean_of(xs: &[f64]) -> f64 {
    let mut sum = 0.0;
    for x in xs {
        sum += *x;
    }
    sum / xs.len() as f64
}

fn stats_var_of(xs: &[f64], mean: f64, sample: bool) -> f64 {
    let mut acc = 0.0;
    for x in xs {
        let d = *x - mean;
        acc += d * d;
    }
    let n = xs.len() as f64;
    if sample {
        acc / (n - 1.0)
    } else {
        acc / n
    }
}

fn stats_mean(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.متوسط", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.متوسط", &args[0], line, col)?;
    Ok(Value::Number(stats_mean_of(&xs)))
}

fn stats_median(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.وسيط", args, 1, line, col)?;
    let mut xs = stats_numbers("إحصاء.وسيط", &args[0], line, col)?;
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        Ok(Value::Number(xs[n / 2]))
    } else {
        Ok(Value::Number((xs[n / 2 - 1] + xs[n / 2]) / 2.0))
    }
}

fn stats_mode(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.منوال", args, 1, line, col)?;
    let mut xs = stats_numbers("إحصاء.منوال", &args[0], line, col)?;
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut best = xs[0];
    let mut best_count = 1usize;
    let mut cur = xs[0];
    let mut cur_count = 1usize;
    for x in xs.iter().skip(1) {
        if *x == cur {
            cur_count += 1;
        } else {
            cur = *x;
            cur_count = 1;
        }
        if cur_count > best_count {
            best = cur;
            best_count = cur_count;
        }
    }
    Ok(Value::Number(best))
}

fn stats_stddev(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.انحراف_معياري", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.انحراف_معياري", &args[0], line, col)?;
    let mean = stats_mean_of(&xs);
    Ok(Value::Number(stats_var_of(&xs, mean, false).sqrt()))
}

fn stats_sample_stddev(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.انحراف_معياري_للعينة", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.انحراف_معياري_للعينة", &args[0], line, col)?;
    if xs.len() < 2 {
        return Err(RioError::runtime("انحراف العينة المعياري يحتاج عنصرين على الأقل", line, col));
    }
    let mean = stats_mean_of(&xs);
    Ok(Value::Number(stats_var_of(&xs, mean, true).sqrt()))
}

fn stats_variance(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.تباين", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.تباين", &args[0], line, col)?;
    let mean = stats_mean_of(&xs);
    Ok(Value::Number(stats_var_of(&xs, mean, false)))
}

fn stats_sample_variance(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.تباين_العينة", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.تباين_العينة", &args[0], line, col)?;
    if xs.len() < 2 {
        return Err(RioError::runtime("تباين العينة يحتاج عنصرين على الأقل", line, col));
    }
    let mean = stats_mean_of(&xs);
    Ok(Value::Number(stats_var_of(&xs, mean, true)))
}

fn stats_sum(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.مجموع", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.مجموع", &args[0], line, col)?;
    let mut sum = 0.0;
    for x in xs {
        sum += x;
    }
    Ok(Value::Number(sum))
}

fn stats_range(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("إحصاء.مدى", args, 1, line, col)?;
    let xs = stats_numbers("إحصاء.مدى", &args[0], line, col)?;
    let mut lo = xs[0];
    let mut hi = xs[0];
    for x in xs.iter().skip(1) {
        if *x < lo {
            lo = *x;
        }
        if *x > hi {
            hi = *x;
        }
    }
    Ok(Value::Number(hi - lo))
}

// ---------- 165: التوزيعات الإحصائية (وحدة توزيعات جديدة) ----------
// طبيعي (كثافة وتراكمي وكمّي) ومنتظم (كثافة وتراكمي)
// وثنائي وبواسون (احتمال كتلة). دالة الخطأ بتقريب
// أبراموفيتز-ستيغن 7.1.26 (خطأ < 1.5e-7) — بالصيغة
// نفسها حرفيا في المرآة C ليتطابق الناتج بتّا بتّا؛
// والكمّي بتنصيف 100 خطوة على التراكمي نفسه. بلا
// أخذ عينات عشوائية — التوزيعات دوال حتمية فقط
// (والعشوائية في وحدة عشوائي) فيدخل التكافؤ.
// المفتاح المخزن لطبيعي_كمّي «طبيعي_كمي» (تجريد الشدة).
// الشروط: انحراف > 0، وأدنى < أقصى، و0 < احتمال < 1،
// و0 ≤ احتمال_ثنائي ≤ 1، و0 ≤ نجاحات ≤ محاولات، ومتوسط
// بواسون ≥ 0 وأحداث عدد صحيح ≥ 0.

fn distrib_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("طبيعي_كثافة", native("طبيعي_كثافة", distrib_norm_pdf)),
        ("طبيعي_تراكمي", native("طبيعي_تراكمي", distrib_norm_cdf)),
        ("طبيعي_كمي", native("طبيعي_كمي", distrib_norm_quantile)),
        ("منتظم_كثافة", native("منتظم_كثافة", distrib_unif_pdf)),
        ("منتظم_تراكمي", native("منتظم_تراكمي", distrib_unif_cdf)),
        ("ثنائي_احتمال", native("ثنائي_احتمال", distrib_binom)),
        ("بواسون_احتمال", native("بواسون_احتمال", distrib_poisson)),
    ]
}

/// دالة الخطأ — أبراموفيتز-ستيغن 7.1.26؛ وصفر عند صفر بالضبط.
fn distrib_erf(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t;
    let y = 1.0 - poly * (-ax * ax).exp();
    sign * y
}

fn distrib_norm_cdf_value(x: f64, mu: f64, sigma: f64) -> f64 {
    0.5 * (1.0 + distrib_erf((x - mu) / (sigma * std::f64::consts::SQRT_2)))
}

fn distrib_norm_pdf(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.طبيعي_كثافة", args, 3, line, col)?;
    let x = need_number("توزيعات.طبيعي_كثافة", &args[0], line, col)?;
    let mu = need_number("توزيعات.طبيعي_كثافة", &args[1], line, col)?;
    let sigma = need_number("توزيعات.طبيعي_كثافة", &args[2], line, col)?;
    if sigma <= 0.0 {
        return Err(RioError::runtime("الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col));
    }
    let z = (x - mu) / sigma;
    let pdf = (-0.5 * z * z).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt());
    Ok(Value::Number(pdf))
}

fn distrib_norm_cdf(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.طبيعي_تراكمي", args, 3, line, col)?;
    let x = need_number("توزيعات.طبيعي_تراكمي", &args[0], line, col)?;
    let mu = need_number("توزيعات.طبيعي_تراكمي", &args[1], line, col)?;
    let sigma = need_number("توزيعات.طبيعي_تراكمي", &args[2], line, col)?;
    if sigma <= 0.0 {
        return Err(RioError::runtime("الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col));
    }
    Ok(Value::Number(distrib_norm_cdf_value(x, mu, sigma)))
}

fn distrib_norm_quantile(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.طبيعي_كمي", args, 3, line, col)?;
    let p = need_number("توزيعات.طبيعي_كمي", &args[0], line, col)?;
    let mu = need_number("توزيعات.طبيعي_كمي", &args[1], line, col)?;
    let sigma = need_number("توزيعات.طبيعي_كمي", &args[2], line, col)?;
    if sigma <= 0.0 {
        return Err(RioError::runtime("الانحراف المعياري للتوزيع الطبيعي يجب أن يكون > 0", line, col));
    }
    if p <= 0.0 || p >= 1.0 {
        return Err(RioError::runtime("الاحتمال للكمّي الطبيعي يجب أن يكون بين 0 و1 حصرا", line, col));
    }
    let mut lo = mu - 10.0 * sigma;
    let mut hi = mu + 10.0 * sigma;
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if distrib_norm_cdf_value(mid, mu, sigma) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(Value::Number((lo + hi) / 2.0))
}

fn distrib_unif_pdf(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.منتظم_كثافة", args, 3, line, col)?;
    let x = need_number("توزيعات.منتظم_كثافة", &args[0], line, col)?;
    let lo = need_number("توزيعات.منتظم_كثافة", &args[1], line, col)?;
    let hi = need_number("توزيعات.منتظم_كثافة", &args[2], line, col)?;
    if hi <= lo {
        return Err(RioError::runtime("الحد الأقصى للتوزيع المنتظم يجب أن يكون أكبر من الأدنى", line, col));
    }
    if x < lo || x > hi {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(1.0 / (hi - lo)))
}

fn distrib_unif_cdf(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.منتظم_تراكمي", args, 3, line, col)?;
    let x = need_number("توزيعات.منتظم_تراكمي", &args[0], line, col)?;
    let lo = need_number("توزيعات.منتظم_تراكمي", &args[1], line, col)?;
    let hi = need_number("توزيعات.منتظم_تراكمي", &args[2], line, col)?;
    if hi <= lo {
        return Err(RioError::runtime("الحد الأقصى للتوزيع المنتظم يجب أن يكون أكبر من الأدنى", line, col));
    }
    if x <= lo {
        return Ok(Value::Number(0.0));
    }
    if x >= hi {
        return Ok(Value::Number(1.0));
    }
    Ok(Value::Number((x - lo) / (hi - lo)))
}

fn distrib_binom(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.ثنائي_احتمال", args, 3, line, col)?;
    let k = need_integer("توزيعات.ثنائي_احتمال", &args[0], line, col)?;
    let n = need_integer("توزيعات.ثنائي_احتمال", &args[1], line, col)?;
    let p = need_number("توزيعات.ثنائي_احتمال", &args[2], line, col)?;
    if n < 0 || k < 0 || k > n {
        return Err(RioError::runtime("النجاحات والمحاولات للتوزيع الثنائي يجب أن تحقق 0 ≤ نجاحات ≤ محاولات", line, col));
    }
    if p < 0.0 || p > 1.0 {
        return Err(RioError::runtime("الاحتمال للتوزيع الثنائي يجب أن يكون بين 0 و1", line, col));
    }
    let kk = k.min(n - k);
    let mut choose = 1.0;
    for i in 1..=kk {
        choose = choose * (n - kk + i) as f64 / i as f64;
    }
    Ok(Value::Number(choose * p.powf(k as f64) * (1.0 - p).powf((n - k) as f64)))
}

fn distrib_poisson(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("توزيعات.بواسون_احتمال", args, 2, line, col)?;
    let k = need_integer("توزيعات.بواسون_احتمال", &args[0], line, col)?;
    let lambda = need_number("توزيعات.بواسون_احتمال", &args[1], line, col)?;
    if k < 0 {
        return Err(RioError::runtime("الأحداث لتوزيع بواسون يجب أن تكون عددا صحيحا ≥ 0", line, col));
    }
    if lambda < 0.0 {
        return Err(RioError::runtime("المتوسط لتوزيع بواسون يجب أن يكون ≥ 0", line, col));
    }
    let mut fact = 1.0;
    for i in 2..=k {
        fact *= i as f64;
    }
    Ok(Value::Number((-lambda).exp() * lambda.powf(k as f64) / fact))
}

// ---------- J3 — الظروف والروابط (الدفعة الخامسة والأخيرة من
// الدفعة الأولى من برنامج الـ579، الاجتماع 73 §88):
// البنود 55–81 — إكمال وحدة قوائم + وحدة دوال جديدة +
// إكمال وحدة نص + وحدة تنفيذ جديدة (69) + ربط 55 و70.
// رسائل الأخطاء ثابتة الصياغة عمدا (بلا تفاصيل موضعية)
// ليتطابق نصها حرفيا بين المفسر وآلة البايت كود والثنائي الأصلي.
// المفاتيح المخزنة هي الصيغ بعد تجريد التشكيل والتطويل
// فقط (سابقة «انشي» و«اطو_ب»): المحلل يجرّد التشكيل
// والتطويل من المعرفات وقت التحليل، لكنه لا يوحّد
// الهمزات (أ/إ/آ/ؤ/ئ/ة/ى تبقى كما كُتبت) — فالمفتاح
// يحمل الهمزة كما في الاسم، والتهجئة بالشدة أو بدونها
// تُحل إلى المفتاح نفسه (رتّب/رتب، ركّب/ركب).
// جدول_ب، جمع_حسب، بادئات، مجاميع_تراكمية، رتب، قطع،
// ركب، اختر_دالة، ثبت، بدل_المعاملين، احفظ_النتائج،
// كرر_حتى، كرر_ن_مرة، اعكس_الدالة، آلة_حالات، قسم_إلى_كلمات، نفذ_نصا.
// لا تصادم مع ملف مدمج: لا ملف دوال.نيفا ولا تنفيذ.نيفا
// في engine/src/حزم/ (الملفات المدمجة عشرة فقط: أوامر،
// إصدار، بيان، حلال، سجل، قفل، مخزن، مشروع، نشر، نص).
//
// صيغة التطبيق الفوري (سابقة اطوِ_بـ من البنّاء د): دوال
// الدرجة العليا هنا تستلم دالة نيفا بمعامل واحد — زوج
// [أ، ب] للثنائية — وتطبقها فورا؛ فهي لا تُرجع دالة
// جديدة. والسبب موثق: قيمة الدالة الأصلية
// (NativeFunction) مؤشر دالة بلا حالة ملتقَطة، ولا
// تمثيل جديد لقيمة دالة يُخترع صامتا. فالتركيب
// (ركّب) والتطبيق الجزئي (ثبّت) وتبديل المعاملين
// والتخزين المؤقت (احفظ_النتائج) والتكرار كلها صيغ
// تطبيق فوري: تستلم الدالة ومعاملاتها وتعيد النتيجة.
// وخارج النطاق: 62 و66 و67 و76 و72 و79 و81 (لا تناسب
// أو موجودة)؛ و71 مؤجلة بقرار موثق في آخر هذا القسم.

// ---------- مساعدات J3 ----------

/// استدعاء دالة نيفا بمعامل واحد مع تمرير مخرجاتها
/// المطبوعة إلى المخرجات القياسية (سابقة اطوِ_بـ).
fn j3_call_single(
    ctx: &mut dyn HandlerCaller,
    func: &Value,
    arg: Value,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let (res, produced) = ctx.call_handler(func, arg, line, col);
    if !produced.is_empty() {
        print!("{}", produced);
    }
    res
}

/// استدعاء دالة ثنائية بزوج [أ، ب] كمعامل واحد.
fn j3_call_pair(
    ctx: &mut dyn HandlerCaller,
    func: &Value,
    a: Value,
    b: Value,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    j3_call_single(ctx, func, Value::List(vec![a, b]), line, col)
}

/// استدعاء شرط (دالة تعيد منطقيا) بمعامل واحد.
fn j3_call_bool(
    ctx: &mut dyn HandlerCaller,
    func: &Value,
    arg: Value,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    let v = j3_call_single(ctx, func, arg, line, col)?;
    match v {
        Value::Bool(b) => Ok(b),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع أن يعيد الشرط منطقيا لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// عملية ثنائية مدمجة على عددين: جمع، ضرب، أكبر، أصغر.
/// `نوع` كلمة العملية في رسالة الخطأ (طي، جدول).
fn j3_op(op: &str, x: f64, y: f64, kind: &str, line: usize, col: usize) -> RioResult<f64> {
    match op {
        "جمع" => Ok(x + y),
        "ضرب" => Ok(x * y),
        "أكبر" => Ok(x.max(y)),
        "أصغر" => Ok(x.min(y)),
        _ => Err(RioError::runtime(
            format!(
                "عملية {} غير معروفة: {} — المتاح: جمع، ضرب، أكبر، أصغر",
                kind, op
            ),
            line,
            col,
        )),
    }
}

/// قائمة أعداد من قيمة (خطأ عربي إن لم تكن كذلك).
fn j3_numbers(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Vec<f64>> {
    let items = need_list(display, v, line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        out.push(need_number(display, it, line, col)?);
    }
    Ok(out)
}

/// هل القيمة دالة (نيفا أو أصلية أو طريقة مربوطة)؟
fn j3_is_func(v: &Value) -> bool {
    matches!(
        v,
        Value::Function { .. } | Value::NativeFunction { .. } | Value::BoundMethod { .. }
    )
}

fn j3_need_func(display: &str, v: &Value, line: usize, col: usize) -> RioResult<()> {
    if j3_is_func(v) {
        Ok(())
    } else {
        Err(RioError::typ(
            format!("الدالة {} تتوقع دالة لكن أعطيت {}", display, v.type_name()),
            line,
            col,
        ))
    }
}

// ---------- 55: الطي — ربط فقط، بلا بناء ----------
// اطوِ_بـ مبنية في وحدة قوائم أعلاه (البنّاء د، J10-159)
// وهي اطوِ_بـ نفسها من البند J3-55 — فلا دالة تُعاد هنا.
// حالتها في التكافؤ: قوائم-ج3-55-ربط (تشير إليها).
//
// ---------- 56: الجدول ----------
// جدول_بـ(أ، ب، دالة/عملية): تطبيق فعل بين كل عنصر من
// أ وكل عنصر من ب — مصفوفة صفوفها عناصر أ وأعمدتها
// عناصر ب. نص عملية مدمجة (جمع/ضرب/أكبر/أصغر، أعداد
// فقط) أو دالة ثنائية بزوج [أ، ب]. قائمة فارغة في أي
// طرف تعيد قائمة فارغة.
fn lists_table(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.جدول_ب", args, 3, line, col)?;
    let a = need_list("قوائم.جدول_ب", &args[0], line, col)?.clone();
    let b = need_list("قوائم.جدول_ب", &args[1], line, col)?.clone();
    let mut rows: Vec<Value> = Vec::new();
    match &args[2] {
        Value::Str(op) => {
            for x in &a {
                let mut row: Vec<Value> = Vec::new();
                for y in &b {
                    let nx = need_number("قوائم.جدول_ب", x, line, col)?;
                    let ny = need_number("قوائم.جدول_ب", y, line, col)?;
                    row.push(Value::Number(j3_op(op, nx, ny, "جدول", line, col)?));
                }
                rows.push(Value::List(row));
            }
        }
        func => {
            for x in &a {
                let mut row: Vec<Value> = Vec::new();
                for y in &b {
                    row.push(j3_call_pair(ctx, func, x.clone(), y.clone(), line, col)?);
                }
                rows.push(Value::List(row));
            }
        }
    }
    Ok(Value::List(rows))
}

// ---------- 57: التجميع حسب ----------
// جمّع_حسب(قائمة، دالة_مفتاح): تجميع عناصر القائمة حسب
// مفتاح تعيده الدالة لكل عنصر؛ والنتيجة قائمة أزواج
// [مفتاح، مجموعة] بترتيب أول ظهور للمفتاح. المفتاح هو
// الصيغة المطبوعة للقيمة المعادة (نص كما هو، وعدد
// بأرقامه العربية). الجزء المائل من البند (تطبيق فعل
// على مجموعات مائلة) نادر — يُتخطَّى عمدا ويُوثَّق هنا:
// بُني التجميع حسب المفتاح وحده.
// القائمة الفارغة تعيد قائمة فارغة.
fn lists_group_by(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.جمع_حسب", args, 2, line, col)?;
    let items = need_list("قوائم.جمع_حسب", &args[0], line, col)?.clone();
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for item in items {
        let key_v = j3_call_single(ctx, &args[1], item.clone(), line, col)?;
        let key = key_v.fmt_value();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(item),
            None => groups.push((key, vec![item])),
        }
    }
    Ok(Value::List(
        groups
            .into_iter()
            .map(|(k, g)| Value::List(vec![Value::Str(k), Value::List(g)]))
            .collect(),
    ))
}

// ---------- 58: البادئات واللاحقات والتراكمي ----------
// بادئات(قائمة): كل بادئة من القائمة ([أ]، [أ، ب]، ...).
// لاحقات(قائمة): كل لاحقة ([أ، ب، ج]، [ب، ج]، [ج]).
// مجاميع_تراكمية(قائمة أعداد): المجاميع الجزئية
// ([١، ٣، ٦، ١٠] من [١، ٢، ٣، ٤]). القائمة الفارغة
// تعيد قائمة فارغة في الثلاث.
fn lists_prefixes(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.بادئات", args, 1, line, col)?;
    let items = need_list("قوائم.بادئات", &args[0], line, col)?.clone();
    let mut out: Vec<Value> = Vec::new();
    for i in 1..=items.len() {
        out.push(Value::List(items[..i].to_vec()));
    }
    Ok(Value::List(out))
}

fn lists_suffixes(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.لاحقات", args, 1, line, col)?;
    let items = need_list("قوائم.لاحقات", &args[0], line, col)?.clone();
    let mut out: Vec<Value> = Vec::new();
    for i in 0..items.len() {
        out.push(Value::List(items[i..].to_vec()));
    }
    Ok(Value::List(out))
}

fn lists_cumulative(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.مجاميع_تراكمية", args, 1, line, col)?;
    let xs = j3_numbers("قوائم.مجاميع_تراكمية", &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    let mut acc = 0.0;
    for x in xs {
        acc += x;
        out.push(Value::Number(acc));
    }
    Ok(Value::List(out))
}

// ---------- 59: النوافذ ----------
// نوافذ(قائمة، حجم): كل نافذة متتالية من الحجم المعطى
// (منزلقة بموضع واحد). خارج_النوافذ(قائمة، حجم): لكل
// نافذة، قائمة ما عداها من العناصر (المكمل). الحجم عدد
// صحيح ≥ 1 ولا يتجاوز طول القائمة؛ وإلا خطأ عربي.
fn lists_windows_check(display: &str, args: &[Value], line: usize, col: usize) -> RioResult<(Vec<Value>, i64)> {
    need_args(display, args, 2, line, col)?;
    let items = need_list(display, &args[0], line, col)?.clone();
    let size = need_integer(display, &args[1], line, col)?;
    if size < 1 {
        return Err(RioError::runtime(
            format!("حجم النافذة في {} يجب أن يكون عددا صحيحا ≥ 1", display),
            line,
            col,
        ));
    }
    if size as usize > items.len() {
        return Err(RioError::runtime(
            format!("حجم النافذة أكبر من القائمة في {}", display),
            line,
            col,
        ));
    }
    Ok((items, size))
}

fn lists_windows(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let (items, size) = lists_windows_check("قوائم.نوافذ", args, line, col)?;
    let size = size as usize;
    let mut out: Vec<Value> = Vec::new();
    for start in 0..=(items.len() - size) {
        out.push(Value::List(items[start..start + size].to_vec()));
    }
    Ok(Value::List(out))
}

fn lists_windows_outside(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let (items, size) = lists_windows_check("قوائم.خارج_النوافذ", args, line, col)?;
    let size = size as usize;
    let mut out: Vec<Value> = Vec::new();
    for start in 0..=(items.len() - size) {
        let mut comp: Vec<Value> = Vec::new();
        comp.extend_from_slice(&items[..start]);
        comp.extend_from_slice(&items[start + size..]);
        out.push(Value::List(comp));
    }
    Ok(Value::List(out))
}

// ---------- 60: الترتيب تصاعديا وتنازليا ----------
// رتّب(قائمة): تصاعديا؛ رتّب_تنازليا(قائمة): تنازليا.
// قائمة أعداد (تُرفض غير المنتهية ليتطابق المحركان) أو
// قائمة نصوص (ترتيب معجمي ببايتات UTF-8)؛ والقائمة
// المختلطة أو غيرهما خطأ؛ والفارغة تعيد فارغة.
fn lists_sort_impl(items: Vec<Value>, desc: bool, display: &str, line: usize, col: usize) -> RioResult<Value> {
    if items.is_empty() {
        return Ok(Value::List(vec![]));
    }
    match &items[0] {
        Value::Number(_) => {
            let mut xs: Vec<f64> = Vec::with_capacity(items.len());
            for it in &items {
                let n = need_number(display, it, line, col)?;
                if !n.is_finite() {
                    return Err(RioError::runtime(
                        format!("الترتيب في {} يرفض الأعداد غير المنتهية", display),
                        line,
                        col,
                    ));
                }
                xs.push(n);
            }
            if desc {
                xs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            } else {
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            }
            Ok(Value::List(xs.into_iter().map(Value::Number).collect()))
        }
        Value::Str(_) => {
            let mut xs: Vec<String> = Vec::with_capacity(items.len());
            for it in &items {
                xs.push(need_string(display, it, line, col)?);
            }
            if desc {
                xs.sort_by(|a, b| b.cmp(a));
            } else {
                xs.sort();
            }
            Ok(Value::List(xs.into_iter().map(Value::Str).collect()))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أعداد أو قائمة نصوص لكن أعطيت قائمة تبدأ بـ {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn lists_sort(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.رتب", args, 1, line, col)?;
    let items = need_list("قوائم.رتب", &args[0], line, col)?.clone();
    lists_sort_impl(items, false, "قوائم.رتب", line, col)
}

fn lists_sort_desc(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.رتب_تنازليا", args, 1, line, col)?;
    let items = need_list("قوائم.رتب_تنازليا", &args[0], line, col)?.clone();
    lists_sort_impl(items, true, "قوائم.رتب_تنازليا", line, col)
}

// ---------- 73: القطع ----------
// قطّع(قائمة/نص، حجم): تقسيم إلى أجزاء متساوية الحجم
// (الأخير قد يكون أصغر)؛ والنص يُقطَّع بمحارفه.
// قطّع_بفاصل(قائمة/نص، فاصل): تقسيم عند كل ظهور للفاصل
// (يُسقَط الفاصل نفسه، والأجزاء الفارغة تُحفَظ)؛ والنص
// يُقسَّم بنص فاصل غير فارغ. الحجم عدد صحيح ≥ 1.
fn lists_chunk(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.قطع", args, 2, line, col)?;
    let size = need_integer("قوائم.قطع", &args[1], line, col)?;
    if size < 1 {
        return Err(RioError::runtime(
            "حجم الجزء في قوائم.قطع يجب أن يكون عددا صحيحا ≥ 1",
            line,
            col,
        ));
    }
    let size = size as usize;
    match &args[0] {
        Value::List(items) => {
            let mut out: Vec<Value> = Vec::new();
            for chunk in items.chunks(size) {
                out.push(Value::List(chunk.to_vec()));
            }
            Ok(Value::List(out))
        }
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let mut out: Vec<Value> = Vec::new();
            for chunk in chars.chunks(size) {
                out.push(Value::Str(chunk.iter().collect()));
            }
            Ok(Value::List(out))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة قوائم.قطع تتوقع قائمة أو نصا لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

fn lists_split_sep(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("قوائم.قطع_بفاصل", args, 2, line, col)?;
    match (&args[0], &args[1]) {
        (Value::List(items), sep) => {
            let mut out: Vec<Value> = Vec::new();
            let mut cur: Vec<Value> = Vec::new();
            for it in items {
                if it == sep {
                    out.push(Value::List(cur));
                    cur = Vec::new();
                } else {
                    cur.push(it.clone());
                }
            }
            out.push(Value::List(cur));
            Ok(Value::List(out))
        }
        (Value::Str(s), Value::Str(sep)) => {
            if sep.is_empty() {
                return Err(RioError::runtime(
                    "الفاصل الفارغ مرفوض في قوائم.قطع_بفاصل",
                    line,
                    col,
                ));
            }
            Ok(Value::List(
                s.split(sep.as_str())
                    .map(|p| Value::Str(p.to_string()))
                    .collect(),
            ))
        }
        (Value::Str(_), other) => Err(RioError::typ(
            format!(
                "الدالة قوائم.قطع_بفاصل تتوقع فاصلا نصيا للنص لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
        (other, _) => Err(RioError::typ(
            format!(
                "الدالة قوائم.قطع_بفاصل تتوقع قائمة أو نصا لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

// ---------- وحدة دوال جديدة (J3: 61+65 و63 و64 و75 و77 و78 و80 و74-آلة) ----------
// دالة تركيب واحدة للبندين 61 و65 معا (لا دالتين):
// ركّب(خارجية، داخلية، قيمة) = خارجية(داخلية(قيمة)).
// وكل دوال هذه الوحدة بصيغة التطبيق الفوري (انظر رأس القسم).

fn funcs_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("ركب", native("ركب", funcs_compose)),
        ("اختر_دالة", native("اختر_دالة", funcs_choose)),
        ("اختر_دالة_بشرط", native("اختر_دالة_بشرط", funcs_choose_if)),
        ("ثبت", native("ثبت", funcs_partial)),
        ("بدل_المعاملين", native("بدل_المعاملين", funcs_swap)),
        ("احفظ_النتائج", native("احفظ_النتائج", funcs_memoize)),
        ("كرر_حتى", native("كرر_حتى", funcs_repeat_until)),
        ("كرر_ن_مرة", native("كرر_ن_مرة", funcs_repeat_n)),
        ("اعكس_الدالة", native("اعكس_الدالة", funcs_inverse)),
        ("آلة_حالات", native("آلة_حالات", funcs_state_machine)),
        // البندان 117 و118 (الدفعة 2 من الـ579): فحص الهوية.
        ("هوية", native("هوية", funcs_identity)),
    ]
}

/// فحص الهوية (البندان 117 و118، الدفعة 2 من الـ579):
/// `دوال.هوية(أ، ب)` — هل القيمتان نفس الكائن؟
///
/// الدلالة (موثقة هنا وفي المرآة C):
/// - أنواع القيم (عدد، نص، منطقي، لاشيء): الهوية =
///   المساواة — فلا «كائن» مميزا لها.
/// - القوائم والقواميس: الهوية خطأ دائما — فهي أنواع
///   قيمية في نيفا: الإسناد والقراءة ينسخانها عميقا في
///   المحركات الثلاثة (المفسر ينسخ في `lookup`، والأصلي
///   في `nv_clone_value`)، فلا يوجد «نفس الكائن» قابل
///   للملاحظة لها أصلا؛ وقائمتان بنفس المحتوى ليستا
///   متطابقتين أبدا، و`ب = أ` نسخة لا نفس القائمة.
/// - الكائنات والفئات والواجهات: نفس المؤشر (Rc) — نفس
///   الكائن حرفيا؛ وكائنان بحقول متساوية ليسا متطابقين.
/// - الدوال: المساواة البنيوية مع هوية النطاقات الملتقَطة
///   (كما في `PartialEq`) — دالتان من نفس المصنع في نفس
///   الاستدعاء تشتركان نطاقهما فمتطابقتان، ومن استدعاءين
///   مختلفين فلا. والوحدات والدوال الأصلية بالمساواة نفسها.
/// - نوعان مختلفان: خطأ دائما.
fn values_identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Nil, Value::Nil) => true,
        // قوائم وقواميس: بلا هوية ملاحَظة (انظر التعليق أعلاه).
        (Value::List(_), _) | (_, Value::List(_)) => false,
        (Value::Dict(_), _) | (_, Value::Dict(_)) => false,
        (Value::Instance(x), Value::Instance(y)) => std::rc::Rc::ptr_eq(x, y),
        (Value::Class(x), Value::Class(y)) => std::rc::Rc::ptr_eq(x, y),
        (Value::Interface(x), Value::Interface(y)) => std::rc::Rc::ptr_eq(x, y),
        (
            Value::BoundMethod { func: f1, instance: i1 },
            Value::BoundMethod { func: f2, instance: i2 },
        ) => std::rc::Rc::ptr_eq(i1, i2) && values_identical(f1, f2),
        (Value::Function { .. }, Value::Function { .. }) => a == b,
        (Value::Module { .. }, Value::Module { .. }) => a == b,
        (Value::NativeFunction { .. }, Value::NativeFunction { .. }) => a == b,
        _ => false,
    }
}

/// دوال.هوية(أ، ب) — فحص الهوية (البندان 117 و118).
fn funcs_identity(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("دوال.هوية", args, 2, line, col)?;
    Ok(Value::Bool(values_identical(&args[0], &args[1])))
}

// ---------- 61 + 65: دالة تركيب واحدة ----------
// ركّب(خارجية، داخلية، قيمة): تطبق الداخلية على القيمة
// ثم الخارجية على النتيجة. (61 تركيب فوق و65 تركيب
// مع ثنائية — يكفي تركيب واحد واضح بقرار §88.)
fn funcs_compose(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.ركب", args, 3, line, col)?;
    let mid = j3_call_single(ctx, &args[1], args[2].clone(), line, col)?;
    j3_call_single(ctx, &args[0], mid, line, col)
}

// ---------- 63: اختيار الدالة ----------
// اختر_دالة(قائمة دوال، فهرس): تعيد الدالة عند الفهرس
// (صفر-الأساس) ليناديها المتصل. فهرس خارج الحدود خطأ.
// اختر_دالة_بشرط(دوال، شروط، قيمة): تعيد أول دالة
// يتحقق شرطها (دالة الشرط تستلم القيمة وتعيد منطقيا)
// على القيمة؛ وإن لم يتحقق شرط فخطأ.
fn funcs_choose(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.اختر_دالة", args, 2, line, col)?;
    let funcs = need_list("دوال.اختر_دالة", &args[0], line, col)?;
    let idx = need_integer("دوال.اختر_دالة", &args[1], line, col)?;
    if idx < 0 || idx as usize >= funcs.len() {
        return Err(RioError::runtime(
            "فهرس الدالة خارج حدود القائمة في دوال.اختر_دالة",
            line,
            col,
        ));
    }
    let f = &funcs[idx as usize];
    j3_need_func("دوال.اختر_دالة", f, line, col)?;
    Ok(f.clone())
}

fn funcs_choose_if(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.اختر_دالة_بشرط", args, 3, line, col)?;
    let funcs = need_list("دوال.اختر_دالة_بشرط", &args[0], line, col)?.clone();
    let conds = need_list("دوال.اختر_دالة_بشرط", &args[1], line, col)?.clone();
    if funcs.len() != conds.len() {
        return Err(RioError::runtime(
            "قائمتا الدوال والشروط في دوال.اختر_دالة_بشرط يجب أن تتساويا طولا",
            line,
            col,
        ));
    }
    for (f, c) in funcs.iter().zip(conds.iter()) {
        j3_need_func("دوال.اختر_دالة_بشرط", f, line, col)?;
        if j3_call_bool(ctx, c, args[2].clone(), "دوال.اختر_دالة_بشرط", line, col)? {
            return Ok(f.clone());
        }
    }
    Err(RioError::runtime(
        "لا دالة يتحقق شرطها في دوال.اختر_دالة_بشرط",
        line,
        col,
    ))
}

// ---------- 64: التطبيق الجزئي ----------
// ثبّت(دالة ثنائية، معامل مثبت، معامل حر): تطبق الدالة
// بزوج [المثبت، الحر] — أي تثبيت المعامل الأول ثم
// التطبيق فورا (صيغة التطبيق الفوري، انظر رأس القسم).
fn funcs_partial(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.ثبت", args, 3, line, col)?;
    j3_call_pair(ctx, &args[0], args[1].clone(), args[2].clone(), line, col)
}

// ---------- 75: تبديل المعاملات ----------
// بدّل_المعاملين(دالة ثنائية، أ، ب): تطبق الدالة بزوج
// [ب، أ] — أي بترتيب معكوس.
fn funcs_swap(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.بدل_المعاملين", args, 3, line, col)?;
    j3_call_pair(ctx, &args[0], args[2].clone(), args[1].clone(), line, col)
}

// ---------- 77: التخزين المؤقت ----------
// احفظ_النتائج(دالة مسماة، قيمة): تعيد نتيجة الدالة على
// القيمة، محفوظة بعد أول حساب؛ فتكرار النداء بالقيمة
// نفسها لا يعيد تنفيذ الدالة (وآثارها الجانبية، كالطباعة،
// لا تتكرر). المفتاح اسم الدالة + الصيغة المطبوعة
// للمعامل، والذاكرة محلية للخيط الجاري (بلا مشاركة
// بين الخيوط). قيدان موثقان: (1) الدالة المجهولة بلا
// اسم تُرفَض (كل المجهولات بالاسم نفسه فتتصادم)؛
// (2) دالتان مسماتان مختلفتان بالاسم نفسه تتشاركان
// الذاكرة — فهذه للدوال النقية المسماة الثابتة، لا
// لإعادة تعريف دالة بعد حفظ نتائجها.
thread_local! {
    static MEMO_CACHE: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
}

fn func_memo_name(v: &Value) -> Option<String> {
    match v {
        Value::Function { name, .. } => Some(name.clone()),
        Value::NativeFunction { name, .. } => Some(name.clone()),
        Value::BoundMethod { func, .. } => func_memo_name(func),
        _ => None,
    }
}

fn funcs_memoize(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.احفظ_النتائج", args, 2, line, col)?;
    let name = match func_memo_name(&args[0]) {
        Some(n) if n != "مجهولة" => n,
        _ => {
            return Err(RioError::runtime(
                "احفظ_النتائج يستقبل دالة مسماة فقط — الدالة المجهولة بلا اسم لا تُحفظ نتائجها",
                line,
                col,
            ))
        }
    };
    let key = format!("{}|{}", name, args[1].fmt_value());
    let hit = MEMO_CACHE.with(|c| c.borrow().get(&key).cloned());
    if let Some(v) = hit {
        return Ok(v);
    }
    let v = j3_call_single(ctx, &args[0], args[1].clone(), line, col)?;
    MEMO_CACHE.with(|c| {
        c.borrow_mut().insert(key, v.clone());
    });
    Ok(v)
}

// ---------- 78: التكرار حتى / التكرار ن مرات ----------
// كرر_حتى(دالة، ابتدائي، شرط): تطبق الدالة على القيمة
// حتى يتحقق الشرط (يُفحَص قبل كل تطبيق، فالقيمة
// الابتدائية المحققة للشرط تعود كما هي)؛ والحد
// 10000 تكرار — بعده خطأ عربي لا حلقة أبدية.
// كرر_ن_مرة(دالة، ابتدائي، عدد): تطبق الدالة ن مرات؛
// وعدد 0 يعيد الابتدائي كما هو؛ والسالب خطأ.
const REPEAT_UNTIL_MAX: usize = 10000;

fn funcs_repeat_until(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.كرر_حتى", args, 3, line, col)?;
    let mut cur = args[1].clone();
    for _ in 0..REPEAT_UNTIL_MAX {
        if j3_call_bool(ctx, &args[2], cur.clone(), "دوال.كرر_حتى", line, col)? {
            return Ok(cur);
        }
        cur = j3_call_single(ctx, &args[0], cur, line, col)?;
    }
    Err(RioError::runtime(
        "تجاوز كرر_حتى حد التكرار (10000) — الشرط لم يتحقق",
        line,
        col,
    ))
}

fn funcs_repeat_n(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.كرر_ن_مرة", args, 3, line, col)?;
    let n = need_integer("دوال.كرر_ن_مرة", &args[2], line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            "عدد التكرار في دوال.كرر_ن_مرة يجب أن يكون ≥ 0",
            line,
            col,
        ));
    }
    let mut cur = args[1].clone();
    for _ in 0..n {
        cur = j3_call_single(ctx, &args[0], cur, line, col)?;
    }
    Ok(cur)
}

// ---------- 80: عكس الدالة — مجموعة محدودة ----------
// اعكس_الدالة(عملية، معامل، قيمة): تطبق عكس العملية
// المسماة على القيمة. المجموعة المحدودة الموثقة:
// أضف (عكسها طرح المعامل)، اطرح (عكسها جمع المعامل)،
// اضرب (عكسها قسمة على المعامل — والمعامل صفر خطأ)،
// اقسم (عكسها ضرب في المعامل — والمعامل صفر خطأ).
// عكس دالة نيفا عامة مستحيل بلا تحليل رمزي لجسمها —
// فلا يُبنى هنا؛ والبند مبني على هذه المجموعة وحدها.
fn funcs_inverse(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.اعكس_الدالة", args, 3, line, col)?;
    let op = need_string("دوال.اعكس_الدالة", &args[0], line, col)?;
    let k = need_number("دوال.اعكس_الدالة", &args[1], line, col)?;
    let y = need_number("دوال.اعكس_الدالة", &args[2], line, col)?;
    let r = match op.as_str() {
        "أضف" => y - k,
        "اطرح" => y + k,
        "اضرب" => {
            if k == 0.0 {
                return Err(RioError::runtime(
                    "لا عكس للضرب في صفر في دوال.اعكس_الدالة",
                    line,
                    col,
                ));
            }
            y / k
        }
        "اقسم" => {
            if k == 0.0 {
                return Err(RioError::runtime(
                    "لا عكس للقسمة على صفر في دوال.اعكس_الدالة",
                    line,
                    col,
                ));
            }
            y * k
        }
        _ => {
            return Err(RioError::runtime(
                format!(
                    "عملية بلا عكس معروف: {} — المتاح: أضف، اطرح، اضرب، اقسم",
                    op
                ),
                line,
                col,
            ))
        }
    };
    Ok(Value::Number(r))
}

// ---------- 74: آلة حالات ----------
// آلة_حالات(انتقالات، بداية، مدخلات): آلة حالات بسيطة
// موثقة — انتقالات قاموس {حالة: {مدخل: حالة تالية}}،
// وبداية نص، ومدخلات قائمة نصوص؛ وتعيد قائمة الحالات
// المزارة بدءا بالبداية. انتقال غائب أو صف ليس قاموسا
// أو حالة تالية ليست نصا: خطأ عربي. وتقسيم الكلمات في
// وحدة نص (قسّم_إلى_كلمات) — انظر أدناه.
fn funcs_state_machine(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("دوال.آلة_حالات", args, 3, line, col)?;
    let trans = need_dict("دوال.آلة_حالات", &args[0], line, col)?;
    let start = need_string("دوال.آلة_حالات", &args[1], line, col)?;
    let inputs = need_list("دوال.آلة_حالات", &args[2], line, col)?.clone();
    let mut state = start;
    let mut visited: Vec<Value> = vec![Value::Str(state.clone())];
    for inp_v in &inputs {
        let inp = need_string("دوال.آلة_حالات", inp_v, line, col)?;
        let row = match dict_get(trans, &state) {
            Some(v) => v,
            None => {
                return Err(RioError::runtime(
                    format!("لا انتقالات من الحالة {} في دوال.آلة_حالات", state),
                    line,
                    col,
                ))
            }
        };
        let row_pairs = match row {
            Value::Dict(p) => p,
            _ => {
                return Err(RioError::runtime(
                    format!("صف الانتقالات للحالة {} ليس قاموسا في دوال.آلة_حالات", state),
                    line,
                    col,
                ))
            }
        };
        let next = match dict_get(row_pairs, &inp) {
            Some(v) => v,
            None => {
                return Err(RioError::runtime(
                    format!(
                        "لا انتقال من الحالة {} بالمدخل {} في دوال.آلة_حالات",
                        state, inp
                    ),
                    line,
                    col,
                ))
            }
        };
        match next {
            Value::Str(s) => {
                state = s.clone();
            }
            _ => {
                return Err(RioError::runtime(
                    format!("الحالة التالية ليست نصا في دوال.آلة_حالات"),
                    line,
                    col,
                ))
            }
        }
        visited.push(Value::Str(state.clone()));
    }
    Ok(Value::List(visited))
}

// ---------- 74: تقسيم الكلمات (إكمال وحدة نص) ----------
// قسّم_إلى_كلمات(نص): قسم على أي مسافة بيضاء ASCII
// (مسافة، جدول، سطر جديد، إرجاع) مع إسقاط الأجزاء
// الفارغة — إكمال قسم_كلمات التي تقسم على المسافة
// الواحدة فقط. (المسافات البيضاء ASCII وحدها عمدا
// ليتطابق التقسيم حرفيا مع المرآة C.)
fn text_split_words_all(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("نص.قسم_إلى_كلمات", args, 1, line, col)?;
    let text = need_string("نص.قسم_إلى_كلمات", &args[0], line, col)?;
    let out: Vec<Value> = text
        .split(|c: char| c.is_ascii_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| Value::Str(p.to_string()))
        .collect();
    Ok(Value::List(out))
}

// ---------- 70: التنسيق — ربط فقط، بلا بناء ----------
// نسّق مبنية في وحدة نص أعلاه (البنّاء ج، J14-220)
// وهي نسّق نفسها من البند J3-70 — فلا دالة تُعاد هنا.
// حالتها في التكافؤ: نص-ج3-70-ربط (تشير إليها).
//
// ---------- 69: تنفيذ نص — وحدة تنفيذ جديدة ----------
// نفّذ_نصا(نص، موافقة): تنفذ النص كبرنامج نيفا معزول
// وتعيد نص مخرجاته الملتقَطة. الشروط الأربعة الملزمة:
// (1) إذن صريح: المعامل الثاني منطقي صحيح — وخطأ
// أو غائب يرفض التنفيذ؛ (2) عزل أمني: نطاق جديد فارغ
// بلا متغيرات المتصل، ومجلد برنامج فارغ (فالملفات
// مرفوضة)، والشبكة والعمليات والخادم معطلة أثناء
// التنفيذ الداخلي ثم تُستعاد أذونات المتصل؛ (3) حد
// الحجم: 10000 محرف؛ (4) حد الزمن: 5 ثوان تُفحَص
// عند كل جملة (مفسر) وكل 4096 تعليمة (آلة) — انظر
// eval_deadline في interpreter.rs وvm.rs. والتداخل:
// 3 مستويات حدا أقصى. والتكافؤ الأصلي (C) مستحيل:
// الثنائي الأصلي بلا مفسر مضمّن — فحالة التكافؤ في
// بوابة المفسر/الآلة فقط وتُتخطَّى صراحة في c_parity
// (C_SKIP، كسابقة تثبيت-محلي)، والمرآة C رفض صريح
// بخطأ عربي لا وحدة غائبة. وأخطاء النص الداخلي
// (تحليل أو دلالة أو تشغيل) تنتقل إلى المتصل كما هي
// ليمسكها بـ«حاول»؛ والمخرجات الجزئية عند خطأ تُهمَل.
static EVAL_DEPTH: AtomicUsize = AtomicUsize::new(0);
const EVAL_MAX_TEXT: usize = 10000;
const EVAL_MAX_DEPTH: usize = 3;

fn exec_entries() -> Vec<(&'static str, Value)> {
    vec![("نفذ_نصا", native("نفذ_نصا", exec_text))]
}

fn exec_text(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("تنفيذ.نفذ_نصا", args, 2, line, col)?;
    let text = need_string("تنفيذ.نفذ_نصا", &args[0], line, col)?;
    let consent = match &args[1] {
        Value::Bool(b) => *b,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة تنفيذ.نفذ_نصا تتوقع موافقة منطقية لكن أعطيت {}",
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    if !consent {
        return Err(RioError::runtime(
            "تنفيذ النص يحتاج إذنا صريحا — مرر صحيح كمعامل ثانٍ",
            line,
            col,
        ));
    }
    if text.chars().count() > EVAL_MAX_TEXT {
        return Err(RioError::runtime(
            "نص التنفيذ أكبر من الحد المسموح (10000 محرف)",
            line,
            col,
        ));
    }
    let depth = EVAL_DEPTH.fetch_add(1, Ordering::SeqCst);
    if depth >= EVAL_MAX_DEPTH {
        EVAL_DEPTH.fetch_sub(1, Ordering::SeqCst);
        return Err(RioError::runtime(
            "تجاوز تنفيذ النص حد التداخل المسموح (3)",
            line,
            col,
        ));
    }
    let (res, produced) = ctx.eval_text(&text, line, col);
    EVAL_DEPTH.fetch_sub(1, Ordering::SeqCst);
    if !produced.is_empty() {
        print!("{}", produced);
    }
    res
}

// ---------- 71: الملاءمة — مؤجلة بقرار موثق ----------
// البند غامض بلا سلوك محدد يُبنى أو يُختبر: «تخصيص سلوك
// فعل بمعاملات (مثال: جمع بدقة أعلى)» — لا يسمي دالة
// ولا معاملات ولا نتيجة. وأي دالة «خصّص» عامة بلا
// معنى محدد ستكون واجهة مخترعة لا تكافؤ لها. فالبند
// مؤجل حتى يُحدَّد سلوك واحد قابل للبناء والاختبار —
// لا بناء هنا، ويسجَّل التأجيل في سطر البند في الكتالوج.
