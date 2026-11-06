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

fn net_send(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
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
        ("نبّه", native("نبّه", device_notify)),
        ("أزل_تنبيه", native("أزل_تنبيه", device_remove_notify)),
        ("انطق", native("انطق", device_speak)),
        ("محركات_النطق", native("محركات_النطق", device_list_engines)),
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
