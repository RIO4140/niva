//! Step 2 (Meeting 16): the initial standard library.
//!
//! Native (Rust) modules, no external crates. A native module is a
//! Module value whose exports are NativeFunction values (or plain
//! constants). `load_module` consults `native_module` BEFORE
//! reading a `.نيفا` file, so `استورد رياضيات` works anywhere with
//! no file needed, and a same-named file in the program directory
//! never shadows a native module. Interpreter-only in v1: the VM
//! has no modules at all yet, so stdlib under `--افتراضي` fails with
//! the VM's existing module error.
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

/// A native stdlib function: args, source line, source column.
pub type NativeFn = fn(&[Value], usize, usize) -> RioResult<Value>;

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

fn math_sqrt(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جذر", args, 1, line, col)?;
    let x = need_number("رياضيات.جذر", &args[0], line, col)?;
    if x < 0.0 {
        return Err(RioError::runtime("جذر عدد سالب غير معرف", line, col));
    }
    Ok(Value::Number(x.sqrt()))
}

fn math_pow(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.قوة", args, 2, line, col)?;
    let base = need_number("رياضيات.قوة", &args[0], line, col)?;
    let exp = need_number("رياضيات.قوة", &args[1], line, col)?;
    Ok(Value::Number(base.powf(exp)))
}

fn math_sin(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.جيب", &args[0], line, col)?.sin()))
}

fn math_cos(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.جيب_تمام", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.جيب_تمام", &args[0], line, col)?.cos()))
}

fn math_tan(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.ظل", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.ظل", &args[0], line, col)?.tan()))
}

fn math_ln(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.لوغاريتم", args, 1, line, col)?;
    let x = need_number("رياضيات.لوغاريتم", &args[0], line, col)?;
    if x <= 0.0 {
        return Err(RioError::runtime("لوغاريتم عدد غير موجب غير معرف", line, col));
    }
    Ok(Value::Number(x.ln()))
}

fn math_exp(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أس_طبيعي", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.أس_طبيعي", &args[0], line, col)?.exp()))
}

fn math_floor(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("رياضيات.أرضية", args, 1, line, col)?;
    Ok(Value::Number(need_number("رياضيات.أرضية", &args[0], line, col)?.floor()))
}

fn math_ceil(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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

fn random_float(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("عشوائي.عدد_عشوائي", args, 0, line, col)?;
    Ok(Value::Number((next_u64() >> 11) as f64 / (1u64 << 53) as f64))
}

fn random_int(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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

fn random_choice(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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
    vec![("اجلب", native("اجلب", network_fetch))]
}

fn network_fetch(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    need_args("شبكة.اجلب", args, 1, line, col)?;
    let url = need_string("شبكة.اجلب", &args[0], line, col)?;
    crate::network::fetch(&url, line, col)
}

// --- وقت ---

fn time_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("الآن", native("الآن", time_now)),
        ("نوم", native("نوم", time_sleep)),
    ]
}

fn time_now(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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

fn time_sleep(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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
