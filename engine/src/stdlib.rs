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
    /// ب6 — المولدات: ألحق نصا بمخرجات المتصل (طباعة
    /// جسم مولد أثناء تقدمه تصل إلى مخرجات المتصل
    /// بالترتيب). الافتراضي صامت (لمنفذ اختبار StubCaller
    /// وغيره ممن بلا مخرجات).
    fn append_output(&mut self, _text: &str) {}
    /// ب6: ثبّت رمز خروج طلبه مولد (`خروج()` داخل
    /// جسمه) في المتصل. الافتراضي صامت.
    fn set_exit_code(&mut self, _code: u8) {}
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
/// ---------- وحدة فرق (دفعة لينكس 3، موجة النص) ----------
///
/// فرق نصوص ومجلدات بخوارزمية LCS (أطول تتابع مشترك)
/// بجدول برمجة ديناميكية؛ والحد: حاصل ضرب عدد
/// الأسطر ≤ 4 ملايين (وإلا خطأ عربي). وسطر النهاية
/// لا يفرّق (lines)؛ والأسطر الفارغة المتجاهلة لا
/// تظهر في المخرجات؛ ومفتاح المقارنة يطوي الحالة
/// والفراغات حسب الخيارات. وسجلات الفرق موحدة
/// الشكل {نوع، قديم، جديد، رقم_قديم، رقم_جديد}
/// (والغائب لاشيء)؛ والمغير اقتران حذف وإضافة
/// متجاورين. والصيغة الموحدة GNU (-u) بسياق 3.

#[derive(Clone, Copy, Default)]
struct DiffOpts {
    unified: bool,
    ignore_case: bool,
    ignore_space: bool,
    ignore_all_space: bool,
    ignore_blank: bool,
    brief: bool,
}

fn diff_parse_opts(
    display: &str,
    pairs: &[(String, Value)],
    allow_format: bool,
    allow_brief: bool,
    allow_exclude: bool,
    line: usize,
    col: usize,
) -> RioResult<(DiffOpts, Option<String>)> {
    let mut opts = DiffOpts::default();
    let mut exclude: Option<String> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "صيغة" => {
                if !allow_format {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                let s = need_string(display, v, line, col)?;
                if s != "موحد" {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} الصيغة {} غير مدعومة (المدعوم: موحد)",
                            display, s
                        ),
                        line,
                        col,
                    ));
                }
                opts.unified = true;
            }
            "مختصر" => {
                if !allow_brief {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                opts.brief = search_need_bool(display, k, v, line, col)?;
            }
            "تجاهل_الحالة" => {
                opts.ignore_case = search_need_bool(display, k, v, line, col)?;
            }
            "تجاهل_الفراغات" => {
                opts.ignore_space = search_need_bool(display, k, v, line, col)?;
            }
            "تجاهل_كل_الفراغات" => {
                opts.ignore_all_space = search_need_bool(display, k, v, line, col)?;
            }
            "تجاهل_الأسطر_الفارغة" => {
                opts.ignore_blank = search_need_bool(display, k, v, line, col)?;
            }
            "استثن_ملفات" => {
                if !allow_exclude {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                exclude = Some(need_string(display, v, line, col)?);
            }
            other => {
                return Err(RioError::runtime(
                    format!("الدالة {} لا تقبل الخيار {}", display, other),
                    line,
                    col,
                ));
            }
        }
    }
    if opts.brief && opts.unified {
        return Err(RioError::runtime(
            format!("الدالة {} لا تجمع مختصر مع صيغة", display),
            line,
            col,
        ));
    }
    Ok((opts, exclude))
}

/// مفتاح مقارنة سطر حسب خيارات التجاهل.
fn diff_key(line: &str, opts: &DiffOpts) -> String {
    let mut s = if opts.ignore_case {
        line.to_lowercase()
    } else {
        line.to_string()
    };
    if opts.ignore_all_space {
        s = s.chars().filter(|c| *c != ' ' && *c != '\t').collect();
    } else if opts.ignore_space {
        s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    s
}

/// تسلسل مقارن: (رقم السطر الأصلي 0-based، السطر،
/// المفتاح) — مع حذف الفارغة عند التجاهل.
fn diff_seq(text: &str, opts: &DiffOpts) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if opts.ignore_blank && l.trim().is_empty() {
            continue;
        }
        let key = diff_key(l, opts);
        out.push((i, l.to_string(), key));
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum DiffOp {
    Equal(usize, usize),
    Del(usize),
    Ins(usize),
}

/// عمليات LCS بين مفتاحي التسلسلين.
fn diff_compute(
    display: &str,
    a: &[(usize, String, String)],
    b: &[(usize, String, String)],
    line: usize,
    col: usize,
) -> RioResult<Vec<DiffOp>> {
    let n = a.len();
    let m = b.len();
    if n.checked_mul(m).unwrap_or(usize::MAX) > 4_000_000 {
        return Err(RioError::runtime(
            format!("الدالة {} النصان كبيران جدا للفرق", display),
            line,
            col,
        ));
    }
    let w = m + 1;
    let mut dp = vec![0u32; (n + 1) * w];
    for i in 1..=n {
        for j in 1..=m {
            dp[i * w + j] = if a[i - 1].2 == b[j - 1].2 {
                dp[(i - 1) * w + j - 1] + 1
            } else {
                dp[(i - 1) * w + j].max(dp[i * w + j - 1])
            };
        }
    }
    let mut ops: Vec<DiffOp> = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1].2 == b[j - 1].2 {
            ops.push(DiffOp::Equal(i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i * w + j - 1] >= dp[(i - 1) * w + j]) {
            ops.push(DiffOp::Ins(j - 1));
            j -= 1;
        } else {
            ops.push(DiffOp::Del(i - 1));
            i -= 1;
        }
    }
    ops.reverse();
    Ok(ops)
}

/// سجل فرق واحد موحد الشكل.
fn diff_record(
    kind: &str,
    old: Option<&(usize, String, String)>,
    new: Option<&(usize, String, String)>,
) -> Value {
    Value::Dict(vec![
        ("نوع".to_string(), Value::Str(kind.to_string())),
        (
            "قديم".to_string(),
            old.map(|e| Value::Str(e.1.clone()))
                .unwrap_or(Value::Nil),
        ),
        (
            "جديد".to_string(),
            new.map(|e| Value::Str(e.1.clone()))
                .unwrap_or(Value::Nil),
        ),
        (
            "رقم_قديم".to_string(),
            old.map(|e| Value::Number((e.0 + 1) as f64))
                .unwrap_or(Value::Nil),
        ),
        (
            "رقم_جديد".to_string(),
            new.map(|e| Value::Number((e.0 + 1) as f64))
                .unwrap_or(Value::Nil),
        ),
    ])
}

/// عمليات ← سجلات، مع اقتران الحذف والإضافة
/// المتجاورين في مغير.
fn diff_to_records(
    a: &[(usize, String, String)],
    b: &[(usize, String, String)],
    ops: &[DiffOp],
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while i < ops.len() {
        match ops[i] {
            DiffOp::Equal(_, _) => {
                i += 1;
            }
            _ => {
                let mut dels: Vec<usize> = Vec::new();
                let mut inss: Vec<usize> = Vec::new();
                while i < ops.len() {
                    match ops[i] {
                        DiffOp::Del(x) => dels.push(x),
                        DiffOp::Ins(y) => inss.push(y),
                        DiffOp::Equal(_, _) => break,
                    }
                    i += 1;
                }
                let paired = dels.len().min(inss.len());
                for k in 0..paired {
                    out.push(diff_record("مغير", Some(&a[dels[k]]), Some(&b[inss[k]])));
                }
                for k in paired..dels.len() {
                    out.push(diff_record("محذوف", Some(&a[dels[k]]), None));
                }
                for k in paired..inss.len() {
                    out.push(diff_record("مضاف", None, Some(&b[inss[k]])));
                }
            }
        }
    }
    out
}

/// رأس مقطع موحد: -بداية[,عدد] +بداية[,عدد].
fn diff_hunk_header(a_pos: usize, a_count: usize, b_pos: usize, b_count: usize) -> String {
    let a = if a_count == 1 {
        format!("-{}", a_pos + 1)
    } else if a_count == 0 {
        format!("-{}", a_pos)
    } else {
        format!("-{},{}", a_pos + 1, a_count)
    };
    let b = if b_count == 1 {
        format!("+{}", b_pos + 1)
    } else if b_count == 0 {
        format!("+{}", b_pos)
    } else {
        format!("+{},{}", b_pos + 1, b_count)
    };
    format!("@@ {} {} @@", a, b)
}

/// عمليات ← نص موحد (سياق 3، وبلا تغيير ← فارغ).
fn diff_to_unified(
    a: &[(usize, String, String)],
    b: &[(usize, String, String)],
    ops: &[DiffOp],
) -> String {
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, o)| !matches!(o, DiffOp::Equal(_, _)))
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return String::new();
    }
    let mut out = String::from("--- قديم\n+++ جديد\n");
    let mut hunk_start = changed[0].saturating_sub(3);
    let mut hunk_end = changed[0];
    let mut flush = |out: &mut String, ops: &[DiffOp], from: usize, to: usize| {
        let mut a_pos = 0usize;
        let mut b_pos = 0usize;
        for o in &ops[..from] {
            match o {
                DiffOp::Equal(_, _) => {
                    a_pos += 1;
                    b_pos += 1;
                }
                DiffOp::Del(_) => a_pos += 1,
                DiffOp::Ins(_) => b_pos += 1,
            }
        }
        let (mut a_count, mut b_count) = (0usize, 0usize);
        for o in &ops[from..=to] {
            match o {
                DiffOp::Equal(_, _) => {
                    a_count += 1;
                    b_count += 1;
                }
                DiffOp::Del(_) => a_count += 1,
                DiffOp::Ins(_) => b_count += 1,
            }
        }
        out.push_str(&diff_hunk_header(a_pos, a_count, b_pos, b_count));
        out.push('\n');
        for o in &ops[from..=to] {
            match o {
                DiffOp::Equal(x, _) => {
                    out.push(' ');
                    out.push_str(&a[*x].1);
                    out.push('\n');
                }
                DiffOp::Del(x) => {
                    out.push('-');
                    out.push_str(&a[*x].1);
                    out.push('\n');
                }
                DiffOp::Ins(y) => {
                    out.push('+');
                    out.push_str(&b[*y].1);
                    out.push('\n');
                }
            }
        }
    };
    for k in 1..changed.len() {
        let c = changed[k];
        if c - hunk_end > 6 {
            let to = (hunk_end + 3).min(ops.len() - 1);
            flush(&mut out, ops, hunk_start, to);
            hunk_start = c.saturating_sub(3);
        }
        hunk_end = c;
    }
    let to = (hunk_end + 3).min(ops.len() - 1);
    flush(&mut out, ops, hunk_start, to);
    out
}

/// فرق.فرق(نص1، نص2، خيارات؟) — البنود 186 و187
/// و190 و191 و192 و193 و196: سجلات افتراضيا، أو
/// نص موحد مع صيغة موحد، أو منطقي مع مختصر.
fn diff_diff(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "فرق.فرق";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let a_text = need_string(DISPLAY, &args[0], line, col)?;
    let b_text = need_string(DISPLAY, &args[1], line, col)?;
    let mut opts = DiffOpts::default();
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        let (o, _) = diff_parse_opts(DISPLAY, pairs, true, true, false, line, col)?;
        opts = o;
    }
    let a = diff_seq(&a_text, &opts);
    let b = diff_seq(&b_text, &opts);
    let ops = diff_compute(DISPLAY, &a, &b, line, col)?;
    if opts.brief {
        let different = ops.iter().any(|o| !matches!(o, DiffOp::Equal(_, _)));
        return Ok(Value::Bool(different));
    }
    if opts.unified {
        return Ok(Value::Str(diff_to_unified(&a, &b, &ops)));
    }
    Ok(Value::List(diff_to_records(&a, &b, &ops)))
}

/// فرق.متطابقان(نص1، نص2، خيارات؟) — البند 202:
/// مقارنة صامتة بخيارات التجاهل الأربعة فقط.
fn diff_same(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "فرق.متطابقان";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let a_text = need_string(DISPLAY, &args[0], line, col)?;
    let b_text = need_string(DISPLAY, &args[1], line, col)?;
    let mut opts = DiffOpts::default();
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        let (o, _) = diff_parse_opts(DISPLAY, pairs, false, false, false, line, col)?;
        opts = o;
    }
    let a = diff_seq(&a_text, &opts);
    let b = diff_seq(&b_text, &opts);
    let ops = diff_compute(DISPLAY, &a, &b, line, col)?;
    Ok(Value::Bool(
        ops.iter().all(|o| matches!(o, DiffOp::Equal(_, _))),
    ))
}

/// فرق.فرق_مجلدين(مجلد1، مجلد2، خيارات؟) — البندان
/// 197 و198: قارن مجلدين تكراريا؛ والسجل {ملف،
/// حالة، فرق}؛ والحالات: متطابق ومختلف وموجود_في_
/// الأول_فقط وموجود_في_الثاني_فقط؛ والثنائي يُقارن
/// بايتات (وفرقه فارغ)؛ واستثن_ملفات كروي على
/// المسار النسبي أو الاسم العاري.
fn diff_dirs(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "فرق.فرق_مجلدين";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let dir1 = need_string(DISPLAY, &args[0], line, col)?;
    let dir2 = need_string(DISPLAY, &args[1], line, col)?;
    let mut opts = DiffOpts::default();
    let mut exclude: Option<String> = None;
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        let (o, e) = diff_parse_opts(DISPLAY, pairs, false, false, true, line, col)?;
        opts = o;
        exclude = e;
    }
    if let Some(pat) = &exclude {
        search_glob_match(pat, "", 0, 0)?;
    }
    let mut wopts = SearchOpts::default();
    wopts.include_hidden = true;
    wopts.recursive = true;
    let mut collect = |dir: &str| -> RioResult<Vec<(String, std::path::PathBuf)>> {
        let root_fs = search_resolve_path(ctx, dir);
        let meta = std::fs::metadata(&root_fs).map_err(|_| {
            RioError::runtime(format!("المسار غير موجود: {}", dir), line, col)
        })?;
        if !meta.is_dir() {
            return Err(RioError::runtime(
                format!("المسار ليس مجلدا: {}", dir),
                line,
                col,
            ));
        }
        let files = search_walk_files(dir, &root_fs, &wopts, line, col)?;
        let base = dir.trim_end_matches('/');
        let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
        for (fs, disp) in files {
            let rel = if base.is_empty() {
                disp.clone()
            } else if let Some(r) = disp.strip_prefix(&format!("{}/", base)) {
                r.to_string()
            } else {
                disp.clone()
            };
            out.push((rel, fs));
        }
        Ok(out)
    };
    let list1 = collect(&dir1)?;
    let list2 = collect(&dir2)?;
    let map1: std::collections::HashMap<&str, &std::path::PathBuf> =
        list1.iter().map(|(r, p)| (r.as_str(), p)).collect();
    let map2: std::collections::HashMap<&str, &std::path::PathBuf> =
        list2.iter().map(|(r, p)| (r.as_str(), p)).collect();
    let mut rels: Vec<&str> = map1.keys().chain(map2.keys()).copied().collect();
    rels.sort();
    rels.dedup();
    let mut out: Vec<Value> = Vec::new();
    for rel in rels {
        if let Some(pat) = &exclude {
            let bare = rel.rsplit('/').next().unwrap_or(rel);
            if search_glob_match(pat, rel, 0, 0).unwrap_or(false)
                || search_glob_match(pat, bare, 0, 0).unwrap_or(false)
            {
                continue;
            }
        }
        let (state, recs): (&str, Vec<Value>) = match (map1.get(rel), map2.get(rel)) {
            (Some(_), None) => ("موجود_في_الأول_فقط", vec![]),
            (None, Some(_)) => ("موجود_في_الثاني_فقط", vec![]),
            (Some(p1), Some(p2)) => {
                let b1 = std::fs::read(p1).unwrap_or_default();
                let b2 = std::fs::read(p2).unwrap_or_default();
                if b1 == b2 {
                    ("متطابق", vec![])
                } else {
                    let t1 = String::from_utf8(b1);
                    let t2 = String::from_utf8(b2);
                    match (t1, t2) {
                        (Ok(s1), Ok(s2)) => {
                            let a = diff_seq(&s1, &opts);
                            let b = diff_seq(&s2, &opts);
                            match diff_compute(DISPLAY, &a, &b, line, col) {
                                Ok(ops) => {
                                    let rs = diff_to_records(&a, &b, &ops);
                                    if rs.is_empty() {
                                        ("متطابق", vec![])
                                    } else {
                                        ("مختلف", rs)
                                    }
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        _ => ("مختلف", vec![]),
                    }
                }
            }
            (None, None) => continue,
        };
        out.push(Value::Dict(vec![
            ("ملف".to_string(), Value::Str(rel.to_string())),
            ("حالة".to_string(), Value::Str(state.to_string())),
            ("فرق".to_string(), Value::List(recs)),
        ]));
    }
    Ok(Value::List(out))
}

fn diff_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("فرق", native("فرق", diff_diff)),
        ("فرق_مجلدين", native("فرق_مجلدين", diff_dirs)),
        ("متطابقان", native("متطابقان", diff_same)),
    ]
}

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
        // دفعة لينكس 2، البنّاء اليدوي H2 (البنود
        // 129–133 و145 و146): آلية عنوان تدفق
        // + عدل_في_مكانه — وحدة جديدة بهذه الدالة أولا.
        "تدفق" => flow_entries(),
        // لينكس3 موجة النص: وحدة فرق (§7).
        "فرق" => diff_entries(),
        "جسون" => json_entries(),
        "اكسمل" => xml_entries(),
        "يامال" => yaml_entries(),
        "قوالب" => template_entries(),
        "تحقق" => validate_entries(),
        // الدفعة 3 من الـ579 (البنود 99–100 و153–162):
        // مطابقة الأنماط — مساعدا التفكيك اللذان لا
        // يبنيهما إلا التفكيك نفسه: اسم صنف كائن،
        // وقاموس بلا مفاتيح لباقي `**` في نمط الفهرس.
        // باقي الأنماط بمدمجات قائمة (نوع وطول ومدى
        // وأضف) فلا تحتاج مساعدا.
        "مطابقة" => match_entries(),
        // J11 البيانات والجداول (الدفعة الأخيرة من الـ579):
        // البنود 168 و170 و171 و173 و174 — وحدة جداول جديدة؛
        // 172 مؤجلة بقرار (تحتاج اعتمادا أصليا جديدا).
        "جداول" => tables_entries(),
// BEGIN J12-GEN rust-modules

        // J12 الرسوميات والوسائط (الدفعة الأخيرة من الـ579):
        // مخططات SVG، وصور BMP/PNG/GIF، ووسائط WAV، ومحاكاة حياة —
        // مخرجات ملفات فقط، بلا نافذة تفاعلية أبدا (§88).
        "مخططات" => charts_entries(),
        "صور" => images_entries(),
        "وسائط" => media_entries(),
        "محاكاة" => sim_entries(),
        // END J12-GEN rust-modules
        // BEGIN J13-GEN rust-modules

        // J13 التوسعات الشبكية (الدفعة الأخيرة من الـ579):
        // صفحات HTML، وبريد SMTP/POP3، ومحادثة، وFTP، وNTP —
        // طبقة بروتوكول نقية حتمية فوق المقابس القائمة (§88).
        "صفحات" => pages_entries(),
        "بريد" => mail_entries(),
        "محادثة" => chat_entries(),
        "نقل_ملفات" => ftp_entries(),
        "وقت_شبكة" => ntp_entries(),
        // END J13-GEN rust-modules
// BEGIN J15-GEN rust-modules

        // J15 أنماط الاستخدام (الدفعة الأخيرة من الـ579):
        // وحدة لغات جديدة للغات المصغرة (240).
        "لغات" => langs_entries(),
        // END J15-GEN rust-modules

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
// BEGIN J6-GEN rust-modules
        // ج6 (الدفعة الأخيرة — الأنواع والأعداد والدقة):
        // البنود 99 و101 و102 و103 و105 و106.
        "دقة" => precision_entries(),
        "كسر" => rational_entries(),
        "مركب" => complex_entries(),
        "بت" => bits_entries(),
        "معلب" => boxed_entries(),
// END J6-GEN rust-modules
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

#[derive(Clone, Default)]
struct SearchOpts {
    include_hidden: bool,
    respect_ignore: bool,
    recursive: bool,
    file_pattern: Option<String>,
    exclude_files: Option<String>,
    exclude_dirs: Option<String>,
    file_type: Option<String>,
    not_file_type: Option<String>,
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
            "أدرج_المخفية" => {
                opts.include_hidden = search_need_bool(display, k, v, line, col)?;
            }
            "احترم_التجاهل" => {
                opts.respect_ignore = search_need_bool(display, k, v, line, col)?;
            }
            "تكراري" => {
                opts.recursive = search_need_bool(display, k, v, line, col)?;
            }
            "نمط_الملفات" => {
                opts.file_pattern = Some(search_need_text(display, k, v, line, col)?);
            }
            "استثن_ملفات" => {
                opts.exclude_files = Some(search_need_text(display, k, v, line, col)?);
            }
            "استثن_مجلدات" => {
                opts.exclude_dirs = Some(search_need_text(display, k, v, line, col)?);
            }
            "نوع_الملف" => {
                opts.file_type = Some(search_need_text(display, k, v, line, col)?);
            }
            "ليس_نوع_الملف" => {
                opts.not_file_type = Some(search_need_text(display, k, v, line, col)?);
            }
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
    if opts.file_type.is_some() && opts.not_file_type.is_some() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} خيارات متضاربة: اختر واحدا فقط من نوع_الملف وليس_نوع_الملف",
                display
            ),
            line,
            col,
        ));
    }
    if !opts.recursive && opts.max_depth.is_some() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} خياران متضاربان: تكراري كاذب مع أقصى_عمق صريح",
                display
            ),
            line,
            col,
        ));
    }
    // التحقق المبكر من الكروي ونوع الملف — فخطأ
    // الخيار يظهر ولو كان المجلد فارغا.
    for pat in [&opts.file_pattern, &opts.exclude_files, &opts.exclude_dirs]
        .into_iter()
        .flatten()
    {
        search_glob_match(pat, "", 0, 0)?;
    }
    for t in [&opts.file_type, &opts.not_file_type].into_iter().flatten() {
        search_file_type_exts(t, 0, 0)?;
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
    opts.recursive = true;
    opts.respect_ignore = true;
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
    opts.recursive = true;
    opts.respect_ignore = true;
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
/// دفعة لينكس 2، موجة النص: خيارات المشي —
/// أدرج_المخفية (69) واحترم_التجاهل (70)
/// ونمط_الملفات (77) ونوع_الملف (80)
/// وليس_نوع_الملف (81) وتكراري (97)
/// واستثن_ملفات (101) واستثن_مجلدات (102).
/// والمخفية اسم يبدأ بنقطة (ملف أو مجلد)؛ وأنماط
/// التجاهل موروثة من الجذر إلى كل مجلد فرعي
/// (بلا `!` وبلا `/` وبلا مجلدات أب — قيد H1)؛
/// والكروي يطابق مسار العرض النسبي أو الاسم
/// العاري؛ ونوع الملف يقارن الامتداد بعد خفض
/// الحالة. والكرويات وأنواع الملفات متحقق منها
/// عند تحليل الخيارات، فلا خطأ هنا.
fn search_walk_dir(
    dir_fs: &std::path::Path,
    dir_display: &str,
    depth: i64,
    max_depth: i64,
    opts: &SearchOpts,
    ignore: &mut Vec<String>,
    out: &mut Vec<(std::path::PathBuf, String)>,
) {
    let entries = match std::fs::read_dir(dir_fs) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());
    // أنماط تجاهل هذا المجلد تنضم إلى الموروثة
    // لشجرته، وتُسحب عند الخروج منه.
    let base_len = ignore.len();
    if opts.respect_ignore {
        ignore.extend(search_ignore_patterns(dir_fs));
    }
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
        // المخفية (69): اسم يبدأ بنقطة — ملفا كان أو مجلدا.
        if !opts.include_hidden && name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            // استثن_مجلدات (102): كروي على الاسم العاري مع تقليم النزول.
            if let Some(pat) = &opts.exclude_dirs {
                if search_glob_match(pat, &name, 0, 0).unwrap_or(false) {
                    continue;
                }
            }
            // احترم_التجاهل (70): مجلد متجاهل لا يُنزَل إليه.
            if opts.respect_ignore
                && (search_ignore_match(ignore, &child_display)
                    || search_ignore_match(ignore, &name))
            {
                continue;
            }
            if depth < max_depth {
                search_walk_dir(
                    &entry.path(),
                    &child_display,
                    depth + 1,
                    max_depth,
                    opts,
                    ignore,
                    out,
                );
            }
        } else if ft.is_file() {
            // احترم_التجاهل (70).
            if opts.respect_ignore
                && (search_ignore_match(ignore, &child_display)
                    || search_ignore_match(ignore, &name))
            {
                continue;
            }
            // نمط_الملفات (77): مسار العرض أو الاسم العاري.
            if let Some(pat) = &opts.file_pattern {
                if !search_glob_match(pat, &child_display, 0, 0).unwrap_or(false)
                    && !search_glob_match(pat, &name, 0, 0).unwrap_or(false)
                {
                    continue;
                }
            }
            // نوع_الملف (80) وليس_نوع_الملف (81):
            // مقارنة الامتداد بعد خفض الحالة.
            if opts.file_type.is_some() || opts.not_file_type.is_some() {
                let ext = std::path::Path::new(&name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase());
                if let Some(t) = &opts.file_type {
                    let exts = search_file_type_exts(t, 0, 0).unwrap_or(&[]);
                    if !ext.as_deref().is_some_and(|e| exts.contains(&e)) {
                        continue;
                    }
                }
                if let Some(t) = &opts.not_file_type {
                    let exts = search_file_type_exts(t, 0, 0).unwrap_or(&[]);
                    if ext.as_deref().is_some_and(|e| exts.contains(&e)) {
                        continue;
                    }
                }
            }
            // استثن_ملفات (101): كروي واحد على العرض أو العاري.
            if let Some(pat) = &opts.exclude_files {
                if search_glob_match(pat, &child_display, 0, 0).unwrap_or(false)
                    || search_glob_match(pat, &name, 0, 0).unwrap_or(false)
                {
                    continue;
                }
            }
            out.push((entry.path(), child_display));
        }
    }
    ignore.truncate(base_len);
}

fn search_walk_files(
    root_display: &str,
    root_fs: &std::path::Path,
    opts: &SearchOpts,
    line: usize,
    col: usize,
) -> RioResult<Vec<(std::path::PathBuf, String)>> {
    let meta = std::fs::metadata(root_fs).map_err(|_| {
        RioError::runtime(format!("المسار غير موجود: {}", root_display), line, col)
    })?;
    if meta.is_file() {
        // الجذر ملف مسمى صراحة: يُرجع كما هو بلا
        // ترشيح بخيارات المشي.
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
    let mut ignore: Vec<String> = Vec::new();
    search_walk_dir(
        root_fs,
        base,
        0,
        search_eff_max_depth(opts),
        opts,
        &mut ignore,
        &mut out,
    );
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
    let root_fs = search_resolve_path(ctx, &path);
    let files = search_walk_files(&path, &root_fs, &opts, line, col)?;
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

const SEARCH_HELP: &str = "بحث — البحث في النصوص والملفات.\nالدوال: ابحث_بنمط(نص، نمط، خيارات؟)؛ ابحث_بنص_ثابت(نص، نص_ثابت، خيارات؟)؛ عد_المطابقات(نص، نمط، خيارات؟)؛ عد_كل_المطابقات(نص، نمط، خيارات؟)؛ المطابق_فقط(نص، نمط، خيارات؟)؛ استبدل_معاينة(نص، نمط، بديل، خيارات؟)؛ ملفات_فيها(مسار، نمط، خيارات؟)؛ ملفات_بلا_مطابقة(مسار، نمط، خيارات؟)؛ رتب_النتائج(نتائج، اتجاه؟)؛ أنماط_من_ملف(مسار)؛ نمط_ملفات(نمط، اسم)؛ اقرأ_المدخل()؛ ابحث_في_مجلدات(قائمة_مسارات، نمط، خيارات؟)؛ ابحث_في_المدخل(نمط، خيارات؟)؛ ابحث_بأنماط(نص، قائمة_أنماط، خيارات؟)؛ مساعدة().\nالخيارات: تجاهل_الحالة وحساسية_الحالة وحالة_ذكية وكلمة_كاملة وسطر_كامل واعكس_المطابقة وأقصى_مطابقات وعمود_المطابقة ورقم_السطر وسياق_قبل وسياق_بعد وسياق وقلم_النتائج وصامت وأظهر_اسم_الملف وجسون ولوّن وأقصى_عمق وأدرج_المخفية واحترم_التجاهل ونمط_الملفات ونوع_الملف وليس_نوع_الملف وتكراري واستثن_ملفات واستثن_مجلدات.\nاتجاه رتب_النتائج: «تصاعدي» أو «تنازلي» والافتراضي «تصاعدي».\nأقصى_عمق 0 يعني المجلد الأعلى وحده؛ ولوّن يغلّف المطابق بالأحمر؛ والملفات الثنائية أو غير المقروءة تُتخطى.";

fn search_help(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("بحث.مساعدة", args, 0, line, col)?;
    Ok(Value::Str(SEARCH_HELP.to_string()))
}

/// خيار نصي في قاموس خيارات بحث: نص لا منطقي.
fn search_need_text(
    display: &str,
    key: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع نصا للخيار {} لكن أعطيت {}",
                display,
                key,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// العمق الفعلي للمشي: تكراري كاذب ≡ أقصى_عمق 0
/// (البند 97)؛ والتعارض مع أقصى_عمق صريح مرفوض
/// عند تحليل الخيارات.
fn search_eff_max_depth(opts: &SearchOpts) -> i64 {
    if !opts.recursive {
        0
    } else {
        opts.max_depth.unwrap_or(i64::MAX)
    }
}

/// اقرأ كل المدخل القياسي نصا حتى EOF عبر مضخة
/// stdin (كما `ادخل`)؛ والفارغ ← نص فارغ؛ والحد
/// مليون محرف.
fn search_read_stdin_text(line: usize, col: usize) -> RioResult<String> {
    let mut out = String::new();
    let mut nchars = 0usize;
    loop {
        match crate::stdin_pump::stdin_line() {
            Some(l) => {
                nchars += l.chars().count();
                if nchars > RX_MAX_TEXT {
                    return Err(RioError::runtime("النص كبير جدا", line, col));
                }
                out.push_str(&l);
            }
            None => break,
        }
    }
    Ok(out)
}

/// بحث.ابحث_في_مجلدات(قائمة_مسارات، نمط، خيارات؟)
/// — البند 85: ابحث في نصوص ملفات جذور عدة وارجع
/// سجلات {ملف، سطر، رقم_السطر} (+ عمود ونص_مطابق
/// وقبل وبعد حسب الخيارات)، مرتبة بـ(ملف، رقم_السطر)؛
/// وصامت يرجع منطقيا، وجسون يرجع نص JSON. والملفات
/// الثنائية أو غير المقروءة تُتخطى كملفات_فيها؛
/// وخيارات المشي كلها تسري على كل جذر.
fn search_in_dirs(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "بحث.ابحث_في_مجلدات";
    let opts = search_parse_opts(DISPLAY, args, line, col)?;
    let roots = match &args[0] {
        Value::List(l) => l.clone(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قائمة مسارات لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let pattern = need_string(DISPLAY, &args[1], line, col)?;
    // التحقق من النمط أولا — فلو كانت كل الملفات
    // ثنائية أو غير مقروءة لما ظهر خطأ النمط أبدا.
    let _ = search_scan(DISPLAY, "", true, &pattern, &opts, line, col)?;
    let mut files: Vec<(std::path::PathBuf, String)> = Vec::new();
    for root in &roots {
        let path = need_string(DISPLAY, root, line, col)?;
        let root_fs = search_resolve_path(ctx, &path);
        let mut found = search_walk_files(&path, &root_fs, &opts, line, col)?;
        files.append(&mut found);
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    let mut records: Vec<Value> = Vec::new();
    let mut remaining = opts.max_count;
    for (fs_path, disp) in &files {
        let bytes = match std::fs::read(fs_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut scan_opts = opts.clone();
        scan_opts.max_count = remaining;
        let hits = search_scan(DISPLAY, &text, true, &pattern, &scan_opts, line, col)?;
        if let Some(left) = remaining {
            remaining = Some(left - hits.len() as i64);
        }
        for h in &hits {
            let mut pairs: Vec<(String, Value)> = vec![
                ("ملف".to_string(), Value::Str(disp.clone())),
                ("سطر".to_string(), Value::Str(search_color_shown(h, &opts))),
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
            if search_has_context(&opts) {
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
            records.push(Value::Dict(pairs));
        }
        if let Some(left) = remaining {
            if left <= 0 {
                break;
            }
        }
    }
    if opts.quiet {
        return search_maybe_json(
            Value::Bool(!records.is_empty()),
            &opts,
            DISPLAY,
            line,
            col,
        );
    }
    search_maybe_json(Value::List(records), &opts, DISPLAY, line, col)
}

/// بحث.ابحث_في_المدخل(نمط، خيارات؟) — البند 86:
/// ابحث_بنمط على نص المدخل القياسي كله. وانحراف
/// UTF-8 الموثق من H1 قائم: المضخة تقص البايت غير
/// الصالح، والمرآة C ترفضه بخطأ عربي صريح.
fn search_in_stdin(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "بحث.ابحث_في_المدخل";
    if args.len() < 1 || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملا أو معاملين لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let mut opts = SearchOpts::default();
    opts.recursive = true;
    opts.respect_ignore = true;
    if args.len() == 2 {
        let pairs = need_dict(DISPLAY, &args[1], line, col)?;
        search_parse_opts_dict(DISPLAY, pairs, &mut opts, line, col)?;
    }
    let pattern = need_string(DISPLAY, &args[0], line, col)?;
    let text = search_read_stdin_text(line, col)?;
    search_run(DISPLAY, &text, true, &pattern, &opts, line, col)
}

/// بحث.ابحث_بأنماط(نص، قائمة_أنماط، خيارات؟) —
/// البند 114: اتحاد الأسطر المطابقة لأي نمط، بلا
/// تكرار، مرتبة برقم السطر؛ والسطر الواحد يُحسب
/// مرة واحدة ولو طابق أنماطا عدة (ويأخذ مطابقة
/// أول نمط طابقه). وأقصى_مطابقات حد إجمالي على
/// الأسطر المرجعة.
fn search_in_patterns(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "بحث.ابحث_بأنماط";
    let opts = search_parse_opts(DISPLAY, args, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let patterns = match &args[1] {
        Value::List(l) => l.clone(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قائمة أنماط لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let mut by_idx: std::collections::BTreeMap<usize, SearchHit> =
        std::collections::BTreeMap::new();
    for p in &patterns {
        let pat = need_string(DISPLAY, p, line, col)?;
        let hits = search_scan(DISPLAY, &text, true, &pat, &opts, line, col)?;
        for h in hits {
            by_idx.entry(h.idx).or_insert(h);
        }
    }
    let mut hits: Vec<SearchHit> = by_idx.into_values().collect();
    if let Some(max) = opts.max_count {
        hits.truncate(max as usize);
    }
    search_render(DISPLAY, &hits, &opts, line, col)
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
        ("نمط_ملفات", native("نمط_ملفات", search_glob_match_fn)),
        ("اقرأ_المدخل", native("اقرأ_المدخل", search_read_stdin_fn)),
        ("ابحث_في_مجلدات", native("ابحث_في_مجلدات", search_in_dirs)),
        ("ابحث_في_المدخل", native("ابحث_في_المدخل", search_in_stdin)),
        ("ابحث_بأنماط", native("ابحث_بأنماط", search_in_patterns)),
        ("مساعدة", native("مساعدة", search_help)),
    ]
}

// ---------- وحدة بحث: دفعة لينكس 2، مساعدات يدوية (H1) ----------
// البنود 69 و70 و77 و80 و81 و85 و86 و89 و90 من §3.
// محركات صغيرة جديدة بلا مولّد:
// (1) مترجم كروي (glob ← نمط): يترجم النمط الكروي
// إلى نص نمط (regex) ثم يجمّعه بمحرك نمط القائم
// (rx_compile عبر rx_full_match) فتكون دلالة
// المطابقة دلالة المحرك نفسه لا محركا ثانيا.
// الأشكال: `**` أي عمق بما فيه `/`؛ و`*` داخل
// مقطع واحد بلا `/`؛ و`?` محرف واحد بلا `/`؛
// و`[...]` فئة محارف كفئات نمط (النفي بـ`!` أو
// `^` في أولها، والمدى `أ-ي`، وأول `]` حرفي)؛
// و`{a,b}` بدائل مفصولة بفاصلة بلا تداخل؛ و`\`
// يهرب المحرف التالي حرفيا. و`/` قبل `**` أخيرة
// (`a/**`) تطابق `a` نفسه وكل ما تحته؛ و`**/`
// تطابق صفر مجلدات أو أكثر (`**/*.txt` يطابق
// `أ.txt` في الجذر أيضا). والنمط الكروي غير
// الصالح (فئة بلا إغلاق، وقوس بدائل بلا إغلاق أو
// متداخل، وهروب أخير، ومدى معكوس، ونمط أطول من
// حد محرك النمط) ← خطأ عربي «نمط ملفات غير
// صالح». والنمط الفارغ صالح ويطابق الاسم الفارغ
// وحده. والفاصلة داخل فئة في بدائل تُحترم (لا
// تقسم البديل).
// (2) قارئ تجاهل: يقرأ ملفي `.ignore` و`.gitignore`
// في مجلد (الجذر ومجلداته الفرعية أثناء البحث؛
// ولا مجلدات الأب فوق الجذر — ذلك البند 73
// الأصفر لاحقا) ويعيد أنماطهما سطورا (فارغ
// وتعليق `#` يُتخطى، ومسافة أخيرة تُقلم)؛
// والملف الغائب أو غير المقروء أو غير UTF-8
// يُتجاهل بصمت. والمطابقة كروية بسيطة بالاسم
// النسبي (بشرطات مائلة) عبر المترجم الكروي؛
// ونمط تالف في الملف يُتخطى بصمت. والنفي `!`
// والمجلدات `/` هما البند 88 الأصفر — لا يُبنيان
// هنا، فلا يفسرا (يعاملان كمحارف عادية).
// (3) قارئ stdin: يقرأ كل المدخل القياسي نصا
// عبر مضخة stdin (stdin_line — كما `ادخل`، فلا
// قراءة مباشرة تنازع المضخة) حتى EOF؛ والمدخل
// الفارغ ← نص فارغ؛ والحد مليون محرف كحد نص
// محرك النمط. وحد: أسطر المضخة نصوص UTF-8
// بالبناء، فبايت غير صالح يوقف المضخة عندها
// (يقص الإدخال) ولا يصل هنا كخطأ — أما المرآة
// C فتقرأ بايتات وتفحص UTF-8 فترفض غير الصالح
// بخطأ عربي صريح (انحراف موثق، ولا يظهر في
// التكافؤ لأن بوابته تغذي stdin فارغا).
// (4) جدول أنواع الملفات: اسم ← امتدادات،
// بالجدول نفسه حرفيا في المرآة C؛ ونوع مجهول
// ← خطأ عربي يسرد الأنواع المعروفة.

fn search_glob_err() -> String {
    "نمط ملفات غير صالح".to_string()
}

/// أهرب محرفا حرفيا خارج فئة (للهروب `\` في
/// الكروي وللمحارف الخاصة في النمط).
fn search_glob_escape_literal(out: &mut String, c: char) {
    match c {
        '.' | '+' | '(' | ')' | '|' | '^' | '$' | '*' | '?' | '[' | ']' | '{' | '}' | '\\' => {
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
    }
}

/// محرف واحد داخل فئة (مع هروب محتمل): يعيد
/// (المحرف، الموضع التالي، مهروب؟).
fn search_glob_class_char(
    chars: &[char],
    i: usize,
) -> Result<(char, usize, bool), String> {
    if i >= chars.len() {
        return Err(search_glob_err());
    }
    if chars[i] == '\\' {
        if i + 1 >= chars.len() {
            return Err(search_glob_err());
        }
        Ok((chars[i + 1], i + 2, true))
    } else {
        Ok((chars[i], i + 1, false))
    }
}

/// أصدر محرفا داخل فئة نمط: المهروب الأبجدي
/// حرفي بلا شرطة (فـ`\d` في الكروي `d` حرفية
/// لا فئة أرقام)؛ و`]` و`^` حرفيان بشرطة.
fn search_glob_class_emit(out: &mut String, c: char, escaped: bool) {
    if escaped {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    } else {
        match c {
            ']' | '^' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
}

/// فئة `[...]` بدءا من `[`: يعيد الموضع بعد
/// `]` المغلقة؛ وبلا إغلاق أو مدى معكوس ← خطأ.
fn search_glob_class(chars: &[char], start: usize, out: &mut String) -> Result<usize, String> {
    let mut i = start + 1;
    out.push('[');
    if i < chars.len() && (chars[i] == '!' || chars[i] == '^') {
        out.push('^');
        i += 1;
    }
    let mut first = true;
    loop {
        if i >= chars.len() {
            return Err(search_glob_err());
        }
        let c = chars[i];
        if c == ']' && !first {
            out.push(']');
            return Ok(i + 1);
        }
        first = false;
        let (lo, ni, lo_esc) = search_glob_class_char(chars, i)?;
        // مدى `س-ص` (لا `-` أخيرة ولا `-` قبل `]`).
        if chars.get(ni) == Some(&'-')
            && chars.get(ni + 1).is_some()
            && chars.get(ni + 1) != Some(&']')
        {
            let (hi, hi_next, hi_esc) = search_glob_class_char(chars, ni + 1)?;
            if hi < lo {
                return Err(search_glob_err());
            }
            search_glob_class_emit(out, lo, lo_esc);
            out.push('-');
            search_glob_class_emit(out, hi, hi_esc);
            i = hi_next;
        } else {
            search_glob_class_emit(out, lo, lo_esc);
            i = ni;
        }
    }
}

/// بدائل `{a,b}` بدءا من `{`: بلا تداخل؛ وبلا
/// إغلاق ← خطأ؛ والفاصلة داخل فئة لا تقسم.
fn search_glob_braces(chars: &[char], start: usize, out: &mut String) -> Result<usize, String> {
    let mut j = start + 1;
    loop {
        if j >= chars.len() {
            return Err(search_glob_err());
        }
        match chars[j] {
            '{' => return Err(search_glob_err()),
            '}' => break,
            _ => j += 1,
        }
    }
    let inner = &chars[start + 1..j];
    // قسّم على الفاصلة خارج الفئات وحدها.
    let mut alts: Vec<&[char]> = Vec::new();
    let mut astart = 0usize;
    let mut k = 0usize;
    let mut in_class = false;
    let mut class_first = false;
    while k < inner.len() {
        let c = inner[k];
        if in_class {
            if c == '\\' {
                k += 2;
                continue;
            }
            if (c == '!' || c == '^') && class_first {
                k += 1;
                continue;
            }
            if c == ']' && !class_first {
                in_class = false;
            }
            class_first = false;
            k += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                class_first = true;
                k += 1;
            }
            ',' => {
                alts.push(&inner[astart..k]);
                astart = k + 1;
                k += 1;
            }
            _ => k += 1,
        }
    }
    alts.push(&inner[astart..]);
    out.push_str("(?:");
    let mut first_alt = true;
    for alt in alts {
        if !first_alt {
            out.push('|');
        }
        first_alt = false;
        let t = search_glob_translate(alt, true)?;
        out.push_str(&t);
    }
    out.push(')');
    Ok(j + 1)
}

/// ترجمة مقطع كروي إلى نص نمط؛ و`in_brace`
/// صحيحا داخل بديل (فـ`{` ثانية خطأ).
fn search_glob_translate(chars: &[char], in_brace: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if chars.get(i + 1) == Some(&'*') {
                    if chars.get(i + 2) == Some(&'/') {
                        out.push_str("(.*/)?");
                        i += 3;
                    } else {
                        out.push_str(".*");
                        i += 2;
                    }
                } else {
                    out.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            '[' => {
                i = search_glob_class(chars, i, &mut out)?;
            }
            '{' => {
                if in_brace {
                    return Err(search_glob_err());
                }
                i = search_glob_braces(chars, i, &mut out)?;
            }
            '}' => {
                out.push_str("\\}");
                i += 1;
            }
            '\\' => {
                if i + 1 >= chars.len() {
                    return Err(search_glob_err());
                }
                search_glob_escape_literal(&mut out, chars[i + 1]);
                i += 2;
            }
            '/' => {
                if chars.get(i + 1) == Some(&'*')
                    && chars.get(i + 2) == Some(&'*')
                    && i + 3 == chars.len()
                {
                    out.push_str("(/.*)?");
                    i += 3;
                } else {
                    out.push('/');
                    i += 1;
                }
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' => {
                out.push('\\');
                out.push(chars[i]);
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// ترجم نمطا كرويا كاملا إلى نص نمط.
fn search_glob_to_regex(glob: &str) -> Result<String, String> {
    let chars: Vec<char> = glob.chars().collect();
    if chars.len() > RX_MAX_PATTERN {
        return Err(search_glob_err());
    }
    let out = search_glob_translate(&chars, false)?;
    if out.chars().count() > RX_MAX_PATTERN {
        return Err(search_glob_err());
    }
    Ok(out)
}

/// هل يطابق الاسم النمط الكروي؟ (مطابقة كاملة
/// بمحرك النمط نفسه.)
fn search_glob_match(glob: &str, name: &str, line: usize, col: usize) -> RioResult<bool> {
    let regex = search_glob_to_regex(glob).map_err(|e| RioError::runtime(e, line, col))?;
    rx_full_match(&regex, name).map_err(|e| rx_err(e, line, col))
}

fn search_glob_match_fn(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("بحث.نمط_ملفات", args, 2, line, col)?;
    let glob = need_string("بحث.نمط_ملفات", &args[0], line, col)?;
    let name = need_string("بحث.نمط_ملفات", &args[1], line, col)?;
    Ok(Value::Bool(search_glob_match(&glob, &name, line, col)?))
}

/// أنماط التجاهل في مجلد: سطور `.ignore` ثم
/// `.gitignore` (فارغ وتعليق `#` يُتخطى، ومسافة
/// أخيرة تُقلم)؛ والغائب أو غير المقروء أو غير
/// UTF-8 يُتجاهل بصمت.
// يسمح بالرمز الميت: يستدعيها مولّد الدفعة (G) في
// البحث في المجلدات — لا دالة مستخدم لها بعد.
#[allow(dead_code)]
fn search_ignore_patterns(dir_fs: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in [".ignore", ".gitignore"] {
        let text = match std::fs::read_to_string(dir_fs.join(name)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for raw in text.lines() {
            let pat = raw.trim_end_matches(|c: char| c.is_ascii_whitespace());
            if pat.is_empty() || pat.starts_with('#') {
                continue;
            }
            out.push(pat.to_string());
        }
    }
    out
}

/// هل يطابق الاسم النسبي (بشرطات مائلة) أي نمط
/// تجاهل؟ والنمط التالف في الملف يُتخطى بصمت.
#[allow(dead_code)]
fn search_ignore_match(patterns: &[String], rel: &str) -> bool {
    for pat in patterns {
        if let Ok(regex) = search_glob_to_regex(pat) {
            if let Ok(true) = rx_full_match(&regex, rel) {
                return true;
            }
        }
    }
    false
}

/// اقرأ كل المدخل القياسي نصا حتى EOF عبر مضخة
/// stdin (كما `ادخل`)؛ والفارغ ← نص فارغ؛ والحد
/// مليون محرف.
fn search_read_stdin_fn(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("بحث.اقرأ_المدخل", args, 0, line, col)?;
    Ok(Value::Str(search_read_stdin_text(line, col)?))
}

/// جدول أنواع الملفات (الاسم ← الامتدادات) —
/// بالجدول نفسه حرفيا في المرآة C.
#[allow(dead_code)]
const SEARCH_FILE_TYPES: &[(&str, &[&str])] = &[
    ("نيفا", &["niva"]),
    ("نص", &["txt", "md", "text", "log"]),
    ("سي", &["c", "h"]),
    ("بايثون", &["py"]),
    ("جافا", &["java"]),
    ("جافاسكربت", &["js"]),
    ("رست", &["rs"]),
];

const SEARCH_FILE_TYPES_KNOWN: &str = "نيفا، نص، سي، بايثون، جافا، جافاسكربت، رست";

/// امتدادات نوع ملف بالاسم؛ والمجهول ← خطأ
/// عربي يسرد الأنواع المعروفة.
#[allow(dead_code)]
fn search_file_type_exts(name: &str, line: usize, col: usize) -> RioResult<&'static [&'static str]> {
    for (t, exts) in SEARCH_FILE_TYPES {
        if *t == name {
            return Ok(exts);
        }
    }
    Err(RioError::runtime(
        format!(
            "نوع ملف غير معروف: {} (الأنواع المعروفة: {})",
            name, SEARCH_FILE_TYPES_KNOWN
        ),
        line,
        col,
    ))
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

/// تلوين نص JSON بلا إعادة كتابة: المفاتيح سماوية
/// (36) والنصوص خضراء (32) والأعداد صفراء (33)
/// وصحيح/خطأ أرجوانية (35) وnull حمراء (31)؛ وكل
/// رمز يغلَّف بإعادة ضبط؛ والمسافات تنسخ خاما.
fn b3_json_colorize(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_key = j < chars.len() && chars[j] == ':';
            let code = if is_key { 36 } else { 32 };
            out.push_str(&format!("\x1b[{}m{}\x1b[0m", code, tok));
            continue;
        }
        if c == '-' || c.is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric()
                    || chars[i] == '-'
                    || chars[i] == '+'
                    || chars[i] == '.')
            {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            out.push_str(&format!("\x1b[33m{}\x1b[0m", tok));
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            let code = if tok == "null" { 31 } else { 35 };
            out.push_str(&format!("\x1b[{}m{}\x1b[0m", code, tok));
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// جسون.لون(نص_جيسون) — البند 213 (يدوي: نموذج
/// جديد — ملوّن JSON): يتحقق من صحة JSON أولا ثم
/// يلوّنه بألوان ANSI الثابتة أعلاه.
fn json_color(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.لون";
    need_args(DISPLAY, args, 1, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    if text.chars().count() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    if parse_json(&text).is_err() {
        return Err(RioError::runtime("نص JSON غير صالح", line, col));
    }
    Ok(Value::Str(b3_json_colorize(&text)))
}

/// قسّم نصا إلى قيم JSON عليا متتالية (للابتلاع):
/// مسح بعمق الأقواس مع وعي النصوص والهروب؛ والقيم
/// القياسية تنتهي عند مسافة بيضاء؛ والباقي غير
/// المكتمل يُترك قطعة ليرفضه المحلل بخطأ عربي.
fn b3_split_top_json(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        if chars[i] == '{' || chars[i] == '[' {
            let mut depth = 0i64;
            while i < chars.len() {
                let c = chars[i];
                if c == '"' {
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\\' {
                            i += 2;
                            continue;
                        }
                        if chars[i] == '"' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                if c == '{' || c == '[' {
                    depth += 1;
                } else if c == '}' || c == ']' {
                    depth -= 1;
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i += 1;
            }
        } else if chars[i] == '"' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else {
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
        }
        out.push(chars[start..i].iter().collect());
    }
    out
}

/// جسون.ابتلع(نص) — البند 205 (يدوي: نموذج جديد
/// — مقسّم قيم عليا): اجمع كل قيم JSON في النص في
/// مصفوفة واحدة؛ والفارغ ← قائمة فارغة؛ والتالف ←
/// خطأ عربي.
fn json_slurp(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.ابتلع";
    need_args(DISPLAY, args, 1, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    if text.chars().count() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    let mut out: Vec<Value> = Vec::new();
    for piece in b3_split_top_json(&text) {
        match parse_json(&piece) {
            Ok(j) => out.push(json_to_niva(j)),
            Err(_) => {
                return Err(RioError::runtime("نص JSON غير صالح", line, col));
            }
        }
    }
    Ok(Value::List(out))
}

/// جسون.مرشح_من_ملف(قيمة، مسار) — البند 220
/// (يدوي: نموذج جديد — مرشح ملفي): اقرأ أسماء
/// حقول من ملف (سطرا بسطر، مع تقليم وتخطي
/// الفارغ) وارجع قاموسا بها من القيمة (والغائب
/// لاشيء)؛ والقيمة غير القاموس والملف الغائب أو
/// غير النصي خطأ عربي.
fn json_filter_from_file(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.مرشح_من_ملف";
    need_args(DISPLAY, args, 2, line, col)?;
    let pairs = match &args[0] {
        Value::Dict(p) => p.clone(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قاموسا لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let path = need_string(DISPLAY, &args[1], line, col)?;
    let full = search_resolve_path(ctx, &path);
    let content = match std::fs::read_to_string(&full) {
        Ok(t) => t,
        Err(_) => {
            return Err(RioError::runtime(
                format!("الدالة {} تعذر قراءة الملف: {}", DISPLAY, path),
                line,
                col,
            ))
        }
    };
    if content.chars().count() > RX_MAX_TEXT {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    let mut out: Vec<(String, Value)> = Vec::new();
    for raw in content.lines() {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let v = pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Nil);
        out.push((name.to_string(), v));
    }
    Ok(Value::Dict(out))
}

// BEGIN لينكس3-نص دوال جسون Rust
/// جسون.من_أسطر(قيمة) — البند 204.
fn json_from_lines(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.من_أسطر";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    let text = need_string(DISPLAY, v, line, col)?;
    Ok(Value::List(
        flow_lines(&text)
            .into_iter()
            .map(|s| Value::Str(s.to_string()))
            .collect(),
    ))
}

/// جسون.رتب_المفاتيح(قيمة) — البند 212.
fn json_sort_keys(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.رتب_المفاتيح";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    Ok(b3_json_sorted(v))
}

/// جسون.حقل_اختياري — البند 232.
fn json_opt_field(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.حقل_اختياري";
    need_args(DISPLAY, args, 2, line, col)?;
    let name = need_string(DISPLAY, &args[1], line, col)?;
    match &args[0] {
        Value::Dict(pairs) => Ok(dict_get(pairs, &name).cloned().unwrap_or(Value::Nil)),
        _ => Ok(Value::Nil),
    }
}

/// جسون.عناصر(قيمة) — البند 233.
fn json_elements(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.عناصر";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    match v {
        Value::List(items) => Ok(Value::List(items.clone())),
        Value::Dict(pairs) => Ok(Value::List(
            pairs.iter().map(|(_, val)| val.clone()).collect(),
        )),
        Value::Nil => Ok(Value::List(vec![])),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أو قاموسا أو لاشيء لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.عنصر_برقم — البند 234.
fn json_nth(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.عنصر_برقم";
    need_args(DISPLAY, args, 2, line, col)?;
    let items = need_list(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let len = items.len() as i64;
    let idx = if n < 0 { len + n } else { n };
    if idx < 0 || idx >= len {
        return Err(RioError::runtime(
            format!(
                "الدالة {} فهرس خارج النطاق: {} والقائمة طولها {}",
                DISPLAY, n, len
            ),
            line,
            col,
        ));
    }
    Ok(items[idx as usize].clone())
}

/// جسون.انتق_حقولا — البند 240.
fn json_pick_fields(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.انتق_حقولا";
    need_args(DISPLAY, args, 2, line, col)?;
    let pairs = need_dict(DISPLAY, &args[0], line, col)?.clone();
    let names = need_list(DISPLAY, &args[1], line, col)?.clone();
    let mut out: Vec<(String, Value)> = Vec::new();
    for n in &names {
        let name = need_string(DISPLAY, n, line, col)?;
        let v = dict_get(&pairs, &name).cloned().unwrap_or(Value::Nil);
        out.push((name, v));
    }
    Ok(Value::Dict(out))
}

/// جسون.كل_ما_تحتها(قيمة) — البند 242.
fn json_recurse(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.كل_ما_تحتها";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    let mut out: Vec<Value> = Vec::new();
    b3_json_recurse(v, &mut out);
    Ok(Value::List(out))
}

/// جسون.اطرح — البند 245.
fn json_sub(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.اطرح";
    need_args(DISPLAY, args, 2, line, col)?;
    match (&args[0], &args[1]) {
        (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a - b)),
        (Value::List(a), Value::List(b)) => Ok(Value::List(
            a.iter()
                .filter(|x| !b.contains(x))
                .cloned()
                .collect(),
        )),
        (a, b) => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع عددين أو قائمتين لكن أعطيت {} و{}",
                DISPLAY,
                a.type_name(),
                b.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.كرر_نص — البند 246.
fn json_repeat_text(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.كرر_نص";
    need_args(DISPLAY, args, 2, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            format!("الدالة {} عدد التكرار {} غير صالح (الصالح 0 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    if text.chars().count() * n as usize > 1_000_000 {
        return Err(RioError::runtime("النص كبير جدا", line, col));
    }
    Ok(Value::Str(text.repeat(n as usize)))
}

/// جسون.أو_بديل — البند 249.
fn json_alt(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.أو_بديل";
    need_args(DISPLAY, args, 2, line, col)?;
    match &args[0] {
        Value::Nil | Value::Bool(false) => Ok(args[1].clone()),
        _ => Ok(args[0].clone()),
    }
}

/// جسون.مفاتيح(قيمة) — البند 269.
fn json_keys(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.مفاتيح";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    match v {
        Value::Dict(pairs) => {
            let mut keys: Vec<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
            keys.sort();
            Ok(Value::List(keys.into_iter().map(Value::Str).collect()))
        }
        Value::List(items) => Ok(Value::List(
            (0..items.len()).map(|i| Value::Number(i as f64)).collect(),
        )),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قاموسا أو قائمة لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.اجمع(قيمة) — البند 281.
fn json_add(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.اجمع";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    let items = need_list(DISPLAY, v, line, col)?;
    if items.is_empty() {
        return Ok(Value::Nil);
    }
    match &items[0] {
        Value::Number(_) => {
            let mut sum = 0.0;
            for it in items {
                sum += need_number(DISPLAY, it, line, col)?;
            }
            Ok(Value::Number(sum))
        }
        Value::Str(_) => {
            let mut out = String::new();
            for it in items {
                out.push_str(&need_string(DISPLAY, it, line, col)?);
            }
            Ok(Value::Str(out))
        }
        Value::List(_) => {
            let mut out: Vec<Value> = Vec::new();
            for it in items {
                out.extend(need_list(DISPLAY, it, line, col)?.iter().cloned());
            }
            Ok(Value::List(out))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أعداد أو نصوص أو قوائم لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.أي_عنصر — أي_عنصر(قائمة، دالة_شرط) — البند 282: هل أي عنصر يحقق شرطا (والفارغة خطأ).
fn json_any(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.أي_عنصر";
    need_args(DISPLAY, args, 2, line, col)?;
    let items = need_list(DISPLAY, &args[0], line, col)?.clone();
    j3_need_func(DISPLAY, &args[1], line, col)?;
    for it in items {
        if j3_call_bool(ctx, &args[1], it, DISPLAY, line, col)? {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

/// جسون.كل_العناصر — كل_العناصر(قائمة، دالة_شرط) — البند 283: هل كل العناصر تحقق شرطا (والفارغة صحيح).
fn json_all(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.كل_العناصر";
    need_args(DISPLAY, args, 2, line, col)?;
    let items = need_list(DISPLAY, &args[0], line, col)?.clone();
    j3_need_func(DISPLAY, &args[1], line, col)?;
    for it in items {
        if !j3_call_bool(ctx, &args[1], it, DISPLAY, line, col)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

/// جسون.إلى_نص(قيمة) — البند 288.
fn json_to_text(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.إلى_نص";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    Ok(Value::Str(
        b3_json_to_text(v).map_err(|e| RioError::runtime(e, line, col))?,
    ))
}

/// جسون.مواضع — البند 297.
fn json_indices(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.مواضع";
    need_args(DISPLAY, args, 2, line, col)?;
    match (&args[0], &args[1]) {
        (Value::List(items), needle) => {
            let mut out: Vec<Value> = Vec::new();
            for (i, it) in items.iter().enumerate() {
                if it == needle {
                    out.push(Value::Number(i as f64));
                }
            }
            Ok(Value::List(out))
        }
        (Value::Str(text), Value::Str(needle)) => {
            if needle.is_empty() {
                return Err(RioError::runtime(
                    format!("الدالة {} النص المطلوب فارغ", DISPLAY),
                    line,
                    col,
                ));
            }
            let h: Vec<char> = text.chars().collect();
            let n: Vec<char> = needle.chars().collect();
            let mut out: Vec<Value> = Vec::new();
            let mut i = 0usize;
            while i + n.len() <= h.len() {
                if h[i..i + n.len()] == n[..] {
                    out.push(Value::Number(i as f64));
                    i += n.len();
                } else {
                    i += 1;
                }
            }
            Ok(Value::List(out))
        }
        (a, _) => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}",
                DISPLAY,
                a.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.فهرس_أول — البند 298.
fn json_index_first(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.فهرس_أول";
    need_args(DISPLAY, args, 2, line, col)?;
    let needle = &args[1];
    match &args[0] {
        Value::List(items) => {
            let pos = items.iter().position(|it| it == needle);
            Ok(pos.map(|p| Value::Number(p as f64)).unwrap_or(Value::Nil))
        }
        Value::Str(text) => {
            let sub = match needle {
                Value::Str(s) => s,
                other => {
                    return Err(RioError::typ(
                        format!(
                            "الدالة {} تتوقع نصا مطلوبا في نص لكن أعطيت {}",
                            DISPLAY,
                            other.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            };
            if sub.is_empty() {
                return Err(RioError::runtime(
                    format!("الدالة {} النص المطلوب فارغ", DISPLAY),
                    line,
                    col,
                ));
            }
            let h: Vec<char> = text.chars().collect();
            let n: Vec<char> = sub.chars().collect();
            let mut pos: Option<usize> = None;
            let mut i = 0usize;
            while i + n.len() <= h.len() {
                if h[i..i + n.len()] == n[..] {
                    pos = Some(i);
                    break;
                }
                i += 1;
            }
            pos;
            Ok(pos.map(|p| Value::Number(p as f64)).unwrap_or(Value::Nil))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.فهرس_آخر — البند 298.
fn json_index_last(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.فهرس_آخر";
    need_args(DISPLAY, args, 2, line, col)?;
    let needle = &args[1];
    match &args[0] {
        Value::List(items) => {
            let pos = items.iter().rposition(|it| it == needle);
            Ok(pos.map(|p| Value::Number(p as f64)).unwrap_or(Value::Nil))
        }
        Value::Str(text) => {
            let sub = match needle {
                Value::Str(s) => s,
                other => {
                    return Err(RioError::typ(
                        format!(
                            "الدالة {} تتوقع نصا مطلوبا في نص لكن أعطيت {}",
                            DISPLAY,
                            other.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            };
            if sub.is_empty() {
                return Err(RioError::runtime(
                    format!("الدالة {} النص المطلوب فارغ", DISPLAY),
                    line,
                    col,
                ));
            }
            let h: Vec<char> = text.chars().collect();
            let n: Vec<char> = sub.chars().collect();
            let mut pos: Option<usize> = None;
            let mut i = 0usize;
            while i + n.len() <= h.len() {
                if h[i..i + n.len()] == n[..] {
                    pos = Some(i);
                }
                i += 1;
            }
            pos;
            Ok(pos.map(|p| Value::Number(p as f64)).unwrap_or(Value::Nil))
        }
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.إلى_إدخالات(قيمة) — البند 304.
fn json_to_entries(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.إلى_إدخالات";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    match v {
        Value::Dict(pairs) => Ok(Value::List(
            pairs
                .iter()
                .map(|(k, val)| {
                    Value::Dict(vec![
                        ("مفتاح".to_string(), Value::Str(k.clone())),
                        ("قيمة".to_string(), val.clone()),
                    ])
                })
                .collect(),
        )),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قاموسا لكن أعطيت {}",
                DISPLAY,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// جسون.من_إدخالات(قيمة) — البند 305.
fn json_from_entries(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.من_إدخالات";
    need_args(DISPLAY, args, 1, line, col)?;
    let v = &args[0];
    let items = need_list(DISPLAY, v, line, col)?;
    let mut out: Vec<(String, Value)> = Vec::new();
    for it in items {
        let pairs = need_dict(DISPLAY, it, line, col)?;
        let key = match dict_get(pairs, "مفتاح") {
            Some(Value::Str(s)) => s.clone(),
            Some(other) => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع مفتاحا نصيا لكن أعطيت {}",
                        DISPLAY,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!("الدالة {} إدخال بلا مفتاح", DISPLAY),
                    line,
                    col,
                ))
            }
        };
        let val = dict_get(pairs, "قيمة").cloned().unwrap_or(Value::Nil);
        if let Some(slot) = out.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = val;
        } else {
            out.push((key, val));
        }
    }
    Ok(Value::Dict(out))
}

/// جسون.جمع_حسب — جمع_حسب(قائمة، دالة_مفتاح) — البند 307: اجمع عناصر متساوية المفتاح في مجموعات (بترتيب أول ظهور).
fn json_group_by(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "جسون.جمع_حسب";
    need_args(DISPLAY, args, 2, line, col)?;
    let items = need_list(DISPLAY, &args[0], line, col)?.clone();
    j3_need_func(DISPLAY, &args[1], line, col)?;
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
            .map(|(_, g)| Value::List(g))
            .collect(),
    ))
}
// END لينكس3-نص دوال جسون Rust
fn json_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اقرأ", native("اقرأ", json_read)),
        ("اكتب", native("اكتب", json_write)),
        ("صالح", native("صالح", json_valid)),// BEGIN لينكس3-نص مداخل جسون
        ("من_أسطر", native("من_أسطر", json_from_lines)),
        ("ابتلع", native("ابتلع", json_slurp)),
        ("رتب_المفاتيح", native("رتب_المفاتيح", json_sort_keys)),
        ("لون", native("لون", json_color)),
        ("مرشح_من_ملف", native("مرشح_من_ملف", json_filter_from_file)),
        ("حقل_اختياري", native("حقل_اختياري", json_opt_field)),
        ("عناصر", native("عناصر", json_elements)),
        ("عنصر_برقم", native("عنصر_برقم", json_nth)),
        ("انتق_حقولا", native("انتق_حقولا", json_pick_fields)),
        ("كل_ما_تحتها", native("كل_ما_تحتها", json_recurse)),
        ("اطرح", native("اطرح", json_sub)),
        ("كرر_نص", native("كرر_نص", json_repeat_text)),
        ("أو_بديل", native("أو_بديل", json_alt)),
        ("مفاتيح", native("مفاتيح", json_keys)),
        ("اجمع", native("اجمع", json_add)),
        ("أي_عنصر", native("أي_عنصر", json_any)),
        ("كل_العناصر", native("كل_العناصر", json_all)),
        ("إلى_نص", native("إلى_نص", json_to_text)),
        ("مواضع", native("مواضع", json_indices)),
        ("فهرس_أول", native("فهرس_أول", json_index_first)),
        ("فهرس_آخر", native("فهرس_آخر", json_index_last)),
        ("إلى_إدخالات", native("إلى_إدخالات", json_to_entries)),
        ("من_إدخالات", native("من_إدخالات", json_from_entries)),
        ("جمع_حسب", native("جمع_حسب", json_group_by)),
// END لينكس3-نص مداخل جسون

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

/// وحدة مطابقة الأنماط (الدفعة 3 من الـ579، البنود
/// 99–100 و153–162): مساعدا تفكيك `طابق` فحسب.
fn match_entries() -> Vec<(&'static str, Value)> {
    vec![
        // البند 160: اسم صنف كائن، أو لاشيء لغير
        // الكائن — فاختبار `اسم_الصنف(ق) == "نقطة"`
        // كلي لا يخطئ على غير الكائن.
        ("اسم_الصنف", native("اسم_الصنف", match_class_name)),
        // البند 159: قاموس جديد بلا المفاتيح المذكورة
        // — باقي `**` في نمط الفهرس.
        (
            "بدون_مفاتيح",
            native("بدون_مفاتيح", match_dict_without),
        ),
    ]
}

fn match_class_name(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("مطابقة.اسم_الصنف", args, 1, line, col)?;
    match &args[0] {
        Value::Instance(data) => Ok(Value::Str(data.class.name.clone())),
        _ => Ok(Value::Nil),
    }
}

fn match_dict_without(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args("مطابقة.بدون_مفاتيح", args, 2, line, col)?;
    let dict = need_dict("مطابقة.بدون_مفاتيح", &args[0], line, col)?;
    let keys = need_list("مطابقة.بدون_مفاتيح", &args[1], line, col)?;
    let mut skip: Vec<&str> = Vec::new();
    for k in keys {
        match k {
            Value::Str(s) => skip.push(s),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة مطابقة.بدون_مفاتيح تتوقع قائمة مفاتيح نصية لكن أعطيت {}",
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(Value::Dict(
        dict.iter()
            .filter(|(k, _)| !skip.contains(&k.as_str()))
            .cloned()
            .collect(),
    ))
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

/// تنسيق عدد صحيح بمواصفة printf (عرض وأعلام):
/// الإشارة ثم الأصفار ثم الأرقام، أو المسافات حسب العلم.
fn b3_printf_int(digits: &str, neg: bool, plus: bool, space: bool, zero: bool, minus: bool, width: usize) -> String {
    let sign = if neg {
        "-"
    } else if plus {
        "+"
    } else if space {
        " "
    } else {
        ""
    };
    let mut body = digits.to_string();
    if zero && !minus && width > sign.len() + body.len() {
        let zeros = width - sign.len() - body.len();
        body = "0".repeat(zeros) + &body;
    }
    let mut out = format!("{}{}", sign, body);
    if width > out.chars().count() {
        let pad = width - out.chars().count();
        if minus {
            out.push_str(&" ".repeat(pad));
        } else {
            out = " ".repeat(pad) + &out;
        }
    }
    out
}

/// نص.نسق_بصيغة(صيغة، قائمة_قيم) — البندان 174
/// و173 (يدوي: نموذج جديد — printf؛ والطباعة
/// المنسقة 173 هي اطبع(نسق_بصيغة(...))): %d و%i
/// و%s و%c و%f و%x و%X و%o و%%، مع الأعلام (- و+
/// ومسافة و0 و#) والعرض والدقة. والأرقام غربية
/// (نص printf)؛ و%s للأعداد بصيغة JSON المختصرة؛
/// و%f بدقة افتراضية 6؛ و%x و%X و%o لأعداد صحيحة
/// غير سالبة؛ و%e و%g غير مدعومتين (خطأ عربي)؛
/// والمعاملات الناقصة أو الزائدة خطأ عربي.
fn text_printf_format(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "نص.نسق_بصيغة";
    need_args(DISPLAY, args, 2, line, col)?;
    let fmt = need_string(DISPLAY, &args[0], line, col)?;
    if fmt.chars().count() > 100000 {
        return Err(RioError::runtime("صيغة التنسيق كبيرة جدا", line, col));
    }
    let vals = need_list(DISPLAY, &args[1], line, col)?;
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let mut used = 0usize;
    let mut need_arg = |line: usize, col: usize| -> RioResult<&Value> {
        if used >= vals.len() {
            return Err(RioError::runtime(
                format!("الدالة {} مواصفة التنسيق تتطلب معاملا آخر", DISPLAY),
                line,
                col,
            ));
        }
        let v = &vals[used];
        used += 1;
        Ok(v)
    };
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        let mut minus = false;
        let mut plus = false;
        let mut space = false;
        let mut zero = false;
        let mut hash = false;
        loop {
            match chars.peek() {
                Some('-') => {
                    minus = true;
                    chars.next();
                }
                Some('+') => {
                    plus = true;
                    chars.next();
                }
                Some(' ') => {
                    space = true;
                    chars.next();
                }
                Some('0') => {
                    zero = true;
                    chars.next();
                }
                Some('#') => {
                    hash = true;
                    chars.next();
                }
                _ => break,
            }
        }
        let mut width = 0usize;
        while let Some(d) = chars.peek().filter(|c| c.is_ascii_digit()) {
            width = width * 10 + (*d as usize - '0' as usize);
            chars.next();
        }
        let mut prec: Option<usize> = None;
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut p = 0usize;
            let mut any = false;
            while let Some(d) = chars.peek().filter(|c| c.is_ascii_digit()) {
                p = p * 10 + (*d as usize - '0' as usize);
                any = true;
                chars.next();
            }
            if !any {
                return Err(RioError::runtime(
                    format!("الدالة {} دقة بلا أرقام في مواصفة التنسيق", DISPLAY),
                    line,
                    col,
                ));
            }
            prec = Some(p);
        }
        let conv = match chars.next() {
            Some(c) => c,
            None => {
                return Err(RioError::runtime(
                    format!("الدالة {} مواصفة تنسيق ناقصة في نهاية الصيغة", DISPLAY),
                    line,
                    col,
                ))
            }
        };
        match conv {
            'd' | 'i' => {
                let v = need_arg(line, col)?;
                let n = need_number(DISPLAY, v, line, col)?;
                if !n.is_finite() || n < i64::MIN as f64 || n > i64::MAX as f64 {
                    return Err(RioError::runtime(
                        format!("الدالة {} العدد {} لا يصلح لمواصفة %d", DISPLAY, json_fmt_number(n)),
                        line,
                        col,
                    ));
                }
                let iv = n.trunc() as i64;
                out.push_str(&b3_printf_int(
                    &iv.unsigned_abs().to_string(),
                    iv < 0,
                    plus,
                    space,
                    zero,
                    minus,
                    width,
                ));
            }
            'f' => {
                let v = need_arg(line, col)?;
                let n = need_number(DISPLAY, v, line, col)?;
                if !n.is_finite() {
                    return Err(RioError::runtime(
                        format!("الدالة {} العدد غير المنتهي لا يصلح لمواصفة %f", DISPLAY),
                        line,
                        col,
                    ));
                }
                let p = prec.unwrap_or(6);
                let mut body = format!("{:.*}", p, n);
                let neg = body.starts_with('-');
                if neg {
                    body = body[1..].to_string();
                }
                out.push_str(&b3_printf_int(&body, neg, plus, space, zero, minus, width));
            }
            'x' | 'X' | 'o' => {
                let v = need_arg(line, col)?;
                let n = need_number(DISPLAY, v, line, col)?;
                if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u64::MAX as f64 {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} العدد {} لا يصلح لمواصفة %{}",
                            DISPLAY,
                            json_fmt_number(n),
                            conv
                        ),
                        line,
                        col,
                    ));
                }
                let u = n as u64;
                let mut digits = match conv {
                    'x' => format!("{:x}", u),
                    'X' => format!("{:X}", u),
                    _ => format!("{:o}", u),
                };
                if hash {
                    let pre = match conv {
                        'x' => "0x",
                        'X' => "0X",
                        _ => "0",
                    };
                    if !(conv == 'o' && digits.starts_with('0')) {
                        digits = format!("{}{}", pre, digits);
                    }
                }
                let mut s = digits;
                if width > s.chars().count() {
                    let pad = width - s.chars().count();
                    if minus {
                        s.push_str(&" ".repeat(pad));
                    } else if zero {
                        s = "0".repeat(pad) + &s;
                    } else {
                        s = " ".repeat(pad) + &s;
                    }
                }
                out.push_str(&s);
            }
            's' => {
                let v = need_arg(line, col)?;
                let mut s = match v {
                    Value::Str(t) => t.clone(),
                    Value::Number(n) => json_fmt_number(*n),
                    Value::Bool(b) => {
                        if *b {
                            "صحيح".to_string()
                        } else {
                            "خطأ".to_string()
                        }
                    }
                    Value::Nil => "لاشيء".to_string(),
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتوقع نصا أو عددا أو منطقيا لمواصفة %s لكن أعطيت {}",
                                DISPLAY,
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                };
                if let Some(p) = prec {
                    if s.chars().count() > p {
                        s = s.chars().take(p).collect();
                    }
                }
                if width > s.chars().count() {
                    let pad = width - s.chars().count();
                    if minus {
                        s.push_str(&" ".repeat(pad));
                    } else if zero {
                        s = "0".repeat(pad) + &s;
                    } else {
                        s = " ".repeat(pad) + &s;
                    }
                }
                out.push_str(&s);
            }
            'c' => {
                let v = need_arg(line, col)?;
                let ch = match v {
                    Value::Number(n) => {
                        let code = *n as i64;
                        if *n < 0.0 || *n > 1114111.0 || n.fract() != 0.0 {
                            return Err(RioError::runtime(
                                format!("الدالة {} العدد {} لا يصلح محرفا لمواصفة %c", DISPLAY, json_fmt_number(*n)),
                                line,
                                col,
                            ));
                        }
                        char::from_u32(code as u32).ok_or_else(|| {
                            RioError::runtime(
                                format!("الدالة {} العدد {} لا يصلح محرفا لمواصفة %c", DISPLAY, code),
                                line,
                                col,
                            )
                        })?
                    }
                    Value::Str(t) => {
                        let mut it = t.chars();
                        match (it.next(), it.next()) {
                            (Some(c), None) => c,
                            _ => {
                                return Err(RioError::runtime(
                                    format!("الدالة {} مواصفة %c تتطلب محرفا واحدا", DISPLAY),
                                    line,
                                    col,
                                ))
                            }
                        }
                    }
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتوقع عددا أو محرفا لمواصفة %c لكن أعطيت {}",
                                DISPLAY,
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                };
                let mut s = ch.to_string();
                if width > 1 {
                    let pad = width - 1;
                    if minus {
                        s.push_str(&" ".repeat(pad));
                    } else {
                        s = " ".repeat(pad) + &s;
                    }
                }
                out.push_str(&s);
            }
            other => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} مواصفة التنسيق %{} غير مدعومة (المدعوم: %d و%s و%c و%f و%x و%X و%o و%%)",
                        DISPLAY, other
                    ),
                    line,
                    col,
                ));
            }
        }
    }
    if used < vals.len() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} معاملات زائدة في نسق_بصيغة (استعملت {} من {})",
                DISPLAY,
                used,
                vals.len()
            ),
            line,
            col,
        ));
    }
    Ok(Value::Str(out))
}

// BEGIN لينكس3-نص دوال نص Rust
/// نص.قلم(نص) — البند 180: أزل المسافات من
/// طرفي نص (تقليم يونيكود).
fn text_trim(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "نص.قلم";
    need_args(DISPLAY, args, 1, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    Ok(Value::Str(text.trim().to_string()))
}
// END لينكس3-نص دوال نص Rust
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
// BEGIN J2-GEN entries-text
// J2 (الدفعة الأخيرة من الـ579): البندان 50 و51.
        ("رمز_يونيكود", native("رمز_يونيكود", text_unicode)),
        ("أبجدية", native("أبجدية", text_alphabet)),
// END J2-GEN entries-text
// BEGIN J15-GEN rust-entries-text

// J15 (230): معالجة نصوص بالجمل.
        ("قسم_جمل", native("قسم_جمل", text_split_sentences)),
        ("عدد_الجمل", native("عدد_الجمل", text_sentence_count)),
        ("جملة", native("جملة", text_sentence_at)),
        ("طبق_على_جمل", native("طبق_على_جمل", text_apply_sentences)),
        ("رشح_جمل", native("رشح_جمل", text_filter_sentences)),
        ("أطول_جملة", native("أطول_جملة", text_longest_sentence)),
        ("أقصر_جملة", native("أقصر_جملة", text_shortest_sentence)),// BEGIN لينكس3-نص مداخل نص
        ("نسق_بصيغة", native("نسق_بصيغة", text_printf_format)),
        ("قلم", native("قلم", text_trim)),
// END لينكس3-نص مداخل نص

// END J15-GEN rust-entries-text

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

// BEGIN لينكس3-نص دوال قوائم Rust
/// قوائم.رتب_حسب(قائمة، دالة_مفتاح) — البند 277:
/// رتب مصفوفة حسب قيمة مفتاح لكل عنصر (بترتيب JSON
/// الكلي، وفرز مستقر).
fn lists_sort_by(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "قوائم.رتب_حسب";
    need_args(DISPLAY, args, 2, line, col)?;
    let items = need_list(DISPLAY, &args[0], line, col)?.clone();
    j3_need_func(DISPLAY, &args[1], line, col)?;
    let mut keyed: Vec<(Value, usize, Value)> = Vec::new();
    for (i, it) in items.into_iter().enumerate() {
        let k = j3_call_single(ctx, &args[1], it.clone(), line, col)?;
        keyed.push((k, i, it));
    }
    keyed.sort_by(|a, b| b3_json_cmp(&a.0, &b.0).then(a.1.cmp(&b.1)));
    Ok(Value::List(
        keyed.into_iter().map(|(_, _, it)| it).collect(),
    ))
}
// END لينكس3-نص دوال قوائم Rust
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
// BEGIN J2-GEN entries-lists
// J2 (الدفعة الأخيرة من الـ579): البنود 25 و26 و28 و29 و32 و33 و35 و37 و38 و39 و40 و41 و42 و45 و46 و52.
        ("شكل", native("شكل", lists_shape)),
        ("أعد_تشكيل", native("أعد_تشكيل", lists_reshape)),
        ("سطح", native("سطح", lists_flatten)),
        ("خيط", native("خيط", lists_stitch)),
        ("صفح", native("صفح", lists_stack)),
        ("فكك_مستوى", native("فكك_مستوى", lists_unpack_one)),
        ("اربط", native("اربط", lists_concat2)),
        ("آخر", native("آخر", lists_last)),
        ("قص_الأخير", native("قص_الأخير", lists_drop_last)),
        ("عدل_عند", native("عدل_عند", lists_update_at)),
        ("كرر_كل", native("كرر_كل", lists_replicate)),
        ("فهرس_أول", native("فهرس_أول", lists_index_first)),
        ("فهرس_آخر", native("فهرس_آخر", lists_index_last)),
        ("فهرس_فترات", native("فهرس_فترات", lists_interval_index)),
        ("فهرس_تبديل", native("فهرس_تبديل", lists_perm_rank)),
        ("طبق_تبديل", native("طبق_تبديل", lists_apply_perm)),
        ("اعكس", native("اعكس", lists_reverse)),
        ("دور", native("دور", lists_rotate)),
        ("بدل_المحاور", native("بدل_المحاور", lists_transpose)),
        ("علب", native("علب", lists_box)),
        ("افتح", native("افتح", lists_unbox)),
        ("فريد", native("فريد", lists_unique)),
        ("قناع_فريد", native("قناع_فريد", lists_unique_mask)),
        ("قارن_الكل_بالكل", native("قارن_الكل_بالكل", lists_self_compare)),
        ("فهرس_مسار", native("فهرس_مسار", lists_path_index)),
// END J2-GEN entries-lists
// BEGIN J5-GEN entries-lists
// J5 (الدفعة الأخيرة من الـ579): البنود 93 و95 و96.
        ("طبق_على_كل_عنصر", native("طبق_على_كل_عنصر", lists_apply_each)),
        ("طبق_على_الصفوف", native("طبق_على_الصفوف", lists_apply_rows)),
        ("بث", native("بث", lists_broadcast)),
        ("بث_بمالئ", native("بث_بمالئ", lists_broadcast_fill)),
// END J5-GEN entries-lists
// BEGIN J15-GEN rust-entries-lists

// J15 (225): استعلام على القوائم.
        ("رشح_بقيمة", native("رشح_بقيمة", lists_filter_value)),
        ("رشح_بدالة", native("رشح_بدالة", lists_filter_fn)),// BEGIN لينكس3-نص مداخل قوائم
        ("رتب_حسب", native("رتب_حسب", lists_sort_by)),
// END لينكس3-نص مداخل قوائم

// END J15-GEN rust-entries-lists

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

// BEGIN J2-GEN fns-lists
// ---------- J2 — المصفوفات (الدفعة الأخيرة من الـ579، الخطوة 4 §88) ----------

/// فهرس سالب يُزاح بالطول، والخارج عن النطاق خطأ عربي (J2).
fn j2_resolve_index(display: &str, len: i64, idx: i64, line: usize, col: usize) -> RioResult<usize> {
    let r = if idx < 0 { idx + len } else { idx };
    if r < 0 || r >= len {
        return Err(RioError::runtime(
            format!(
                "الفهرس {} خارج النطاق في {} (الطول {})",
                idx, display, len
            ),
            line,
            col,
        ));
    }
    Ok(r as usize)
}

/// صف مصفوفة: قائمة، وإلا خطأ نوع عربي (J2).
fn j2_need_row<'a>(
    display: &str,
    v: &'a Value,
    line: usize,
    col: usize,
) -> RioResult<&'a Vec<Value>> {
    match v {
        Value::List(items) => Ok(items),
        other => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع صفا (قائمة) لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// تحقق أن قائمة أعداد تبديل لأعداد ٠ إلى ن-١ بلا تكرار (J2).
fn j2_need_perm(display: &str, p: &[Value], line: usize, col: usize) -> RioResult<Vec<usize>> {
    let n = p.len();
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for v in p {
        let x = need_integer(display, v, line, col)?;
        if x < 0 || x as usize >= n || seen[x as usize] {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتطلب تبديلا لأعداد ٠ إلى {} بلا تكرار",
                    display,
                    n.saturating_sub(1)
                ),
                line,
                col,
            ));
        }
        seen[x as usize] = true;
        out.push(x as usize);
    }
    Ok(out)
}

/// قوائم.شكل(قيمة) — أبعاد مصفوفة (البند 25): قائمة مسطحة
/// [ن]؛ وقائمة صفوف [صفوف، أعمدة]؛ ومتداخلة أعمق تتتبع
/// الصف الأول؛ والصفوف غير المتساوية خطأ عربي؛ والنص
/// [عدد محارفه]؛ وغيرهما خطأ نوع.
fn lists_shape(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.شكل";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::List(items) => {
            let mut dims: Vec<Value> = vec![Value::Number(items.len() as f64)];
            let mut cur: &[Value] = items;
            while let Some(Value::List(first)) = cur.first() {
                let w = first.len();
                for row in cur.iter() {
                    match row {
                        Value::List(r) if r.len() == w => {}
                        _ => {
                            return Err(RioError::runtime(
                                "صفوف مصفوفة غير متساوية في قوائم.شكل".to_string(),
                                line,
                                col,
                            ))
                        }
                    }
                }
                dims.push(Value::Number(w as f64));
                cur = first;
            }
            Ok(Value::List(dims))
        }
        Value::Str(s) => Ok(Value::List(vec![Value::Number(s.chars().count() as f64)])),
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// قوائم.أعد_تشكيل(قيمة، أبعاد) — إعادة تشكيل قائمة مسطحة
/// (أو محارف نص) بأبعاد جديدة (البند 25): حاصل ضرب
/// الأبعاد يجب أن يساوي عدد العناصر تماما؛ والأبعاد
/// السالبة خطأ؛ والأبعاد الفارغة تعيد العنصر وحده.
fn lists_reshape(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.أعد_تشكيل";
    need_args(D, args, 2, line, col)?;
    let dims_v = need_list(D, &args[1], line, col)?;
    let mut dims: Vec<i64> = Vec::new();
    for d in dims_v {
        let n = need_integer(D, d, line, col)?;
        if n < 0 {
            return Err(RioError::runtime(
                "أبعاد قوائم.أعد_تشكيل لا تكون سالبة".to_string(),
                line,
                col,
            ));
        }
        dims.push(n);
    }
    let mut total: i64 = 1;
    for d in &dims {
        total = total.checked_mul(*d).ok_or_else(|| {
            RioError::runtime(
                "أبعاد كبيرة جدا في قوائم.أعد_تشكيل".to_string(),
                line,
                col,
            )
        })?;
    }
    let flat: Vec<Value> = match &args[0] {
        Value::List(items) => items.clone(),
        Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
        other => {
            return Err(RioError::typ(
                format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
                line,
                col,
            ))
        }
    };
    if total != flat.len() as i64 {
        return Err(RioError::runtime(
            format!(
                "إعادة التشكيل في قوائم.أعد_تشكيل تتطلب {} عناصر لكن أعطيت {}",
                total,
                flat.len()
            ),
            line,
            col,
        ));
    }
    fn build(dims: &[i64], flat: &[Value], pos: &mut usize) -> Value {
        if dims.is_empty() {
            let v = flat[*pos].clone();
            *pos += 1;
            return v;
        }
        let mut out = Vec::new();
        for _ in 0..dims[0] {
            out.push(build(&dims[1..], flat, pos));
        }
        Value::List(out)
    }
    let mut pos = 0usize;
    Ok(build(&dims, &flat, &mut pos))
}


/// قوائم.سطّح(قائمة) — تسطيح تكراري كامل (البند 26): كل
/// قائمة متداخلة بأي عمق تنحل إلى عناصرها؛ وغير القوائم
/// يبقى كما هو؛ والقائمة الفارغة تعيد فارغة.
fn lists_flatten(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.سطح";
    need_args(D, args, 1, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    fn flat(items: &[Value], out: &mut Vec<Value>) {
        for it in items {
            match it {
                Value::List(inner) => flat(inner, out),
                other => out.push(other.clone()),
            }
        }
    }
    let mut out = Vec::new();
    flat(&items, &mut out);
    Ok(Value::List(out))
}


/// قوائم.خيّط(مصفوفة، مصفوفة) — دمج جنبا إلى جنب (البند 28):
/// المصفوفتان قوائم صفوف بعدد الصفوف نفسه؛ وكل صف في
/// النتيجة صفاه ملتصقان؛ وصف ليس قائمة أو عدد صفوف
/// مختلف خطأ عربي.
fn lists_stitch(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.خيط";
    need_args(D, args, 2, line, col)?;
    let a = need_list(D, &args[0], line, col)?.clone();
    let b = need_list(D, &args[1], line, col)?.clone();
    if a.len() != b.len() {
        return Err(RioError::runtime(
            format!(
                "الخياطة في قوائم.خيط تتطلب صفوفا متساوية العدد ({} مقابل {})",
                a.len(),
                b.len()
            ),
            line,
            col,
        ));
    }
    let mut out = Vec::new();
    for (ra, rb) in a.iter().zip(b.iter()) {
        let la = j2_need_row(D, ra, line, col)?;
        let lb = j2_need_row(D, rb, line, col)?;
        let mut row = la.clone();
        row.extend(lb.iter().cloned());
        out.push(Value::List(row));
    }
    Ok(Value::List(out))
}


/// قوائم.صفّح(مصفوفة، مصفوفة) — طبقة فوق طبقة (البند 28):
/// صفوف الثانية بعد صفوف الأولى؛ وصف ليس قائمة خطأ عربي.
fn lists_stack(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.صفح";
    need_args(D, args, 2, line, col)?;
    let a = need_list(D, &args[0], line, col)?.clone();
    let b = need_list(D, &args[1], line, col)?.clone();
    let mut out = Vec::new();
    for r in a.iter().chain(b.iter()) {
        let row = j2_need_row(D, r, line, col)?;
        out.push(Value::List(row.clone()));
    }
    Ok(Value::List(out))
}


/// قوائم.فكك_مستوى(قائمة) — تفكيك مستوى واحد (البند 29):
/// عناصر القائمة التي هي قوائم تنحل في مكانها، وغيرها
/// يبقى كما هو؛ ويختلف عن سطّح بأنه لا يتكرر للداخل.
fn lists_unpack_one(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فكك_مستوى";
    need_args(D, args, 1, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut out = Vec::new();
    for it in items {
        match it {
            Value::List(inner) => out.extend(inner.iter().cloned()),
            other => out.push(other),
        }
    }
    Ok(Value::List(out))
}


/// قوائم.اربط(قائمة، قائمة) — ربط قائمتين في واحدة (البند 29):
/// العناصر كلها بالترتيب؛ ولا تغيّر في المعاملين.
fn lists_concat2(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.اربط";
    need_args(D, args, 2, line, col)?;
    let a = need_list(D, &args[0], line, col)?;
    let b = need_list(D, &args[1], line, col)?;
    let mut out = a.clone();
    out.extend(b.iter().cloned());
    Ok(Value::List(out))
}


/// قوائم.آخر(قيمة) — آخر عنصر (البند 32): قائمة أو نص؛
/// والفارغ خطأ عربي.
fn lists_last(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.آخر";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::List(items) => items.last().cloned().ok_or_else(|| {
            RioError::runtime("قائمة فارغة في قوائم.آخر".to_string(), line, col)
        }),
        Value::Str(s) => s
            .chars()
            .last()
            .map(|c| Value::Str(c.to_string()))
            .ok_or_else(|| RioError::runtime("نص فارغ في قوائم.آخر".to_string(), line, col)),
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// قوائم.قصّ_الأخير(قيمة) — القيمة بلا آخر عنصر (البند 32):
/// قائمة أو نص؛ والفارغ يعيد فارغا.
fn lists_drop_last(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.قص_الأخير";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                Ok(Value::List(vec![]))
            } else {
                Ok(Value::List(items[..items.len() - 1].to_vec()))
            }
        }
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            if chars.is_empty() {
                Ok(Value::Str(String::new()))
            } else {
                Ok(Value::Str(chars[..chars.len() - 1].iter().collect()))
            }
        }
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// قوائم.عدّل_عند(قائمة، فهارس، قيم) — استبدال عناصر في
/// مواضع معطاة بقيم جديدة (البند 33): الفهارس والقيم
/// قائمتان بالعدد نفسه؛ والفهرس السالب يُزاح من النهاية؛
/// والخارج عن النطاق أو العدد المختلف خطأ عربي؛ ولا
/// تغيّر في القائمة الأصلية.
fn lists_update_at(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.عدل_عند";
    need_args(D, args, 3, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let idxs = need_list(D, &args[1], line, col)?.clone();
    let vals = need_list(D, &args[2], line, col)?.clone();
    if idxs.len() != vals.len() {
        return Err(RioError::runtime(
            format!(
                "قوائم.عدل_عند تتطلب فهارس وقيما بنفس العدد ({} مقابل {})",
                idxs.len(),
                vals.len()
            ),
            line,
            col,
        ));
    }
    let mut out = items;
    for (iv, v) in idxs.iter().zip(vals.iter()) {
        let i = need_integer(D, iv, line, col)?;
        let pos = j2_resolve_index(D, out.len() as i64, i, line, col)?;
        out[pos] = v.clone();
    }
    Ok(Value::List(out))
}


/// قوائم.كرّر_كل(قائمة، أعداد) — تكرار كل عنصر بعدد مرات
/// معطى (البند 35): الأعداد قائمة بالطول نفسه أو عدد واحد
/// لكل العناصر؛ والسالب خطأ؛ والمجموع فوق مليون عنصر خطأ.
fn lists_replicate(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.كرر_كل";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let counts: Vec<i64> = match &args[1] {
        Value::Number(_) => {
            let n = need_integer(D, &args[1], line, col)?;
            vec![n; items.len()]
        }
        Value::List(cs) => {
            let mut v = Vec::new();
            for c in cs {
                v.push(need_integer(D, c, line, col)?);
            }
            v
        }
        other => {
            return Err(RioError::typ(
                format!("الدالة {} تتوقع عددا أو قائمة أعداد لكن أعطيت {}", D, other.type_name()),
                line,
                col,
            ))
        }
    };
    if counts.len() != items.len() {
        return Err(RioError::runtime(
            format!(
                "قوائم.كرر_كل تتطلب أعدادا بطول القائمة ({} مقابل {})",
                counts.len(),
                items.len()
            ),
            line,
            col,
        ));
    }
    for n in &counts {
        if *n < 0 {
            return Err(RioError::runtime(
                "عدد تكرار سالب في قوائم.كرر_كل".to_string(),
                line,
                col,
            ));
        }
    }
    let total: i128 = counts.iter().map(|n| *n as i128).sum();
    if total > 1_000_000 {
        return Err(RioError::runtime(
            "تكرار كبير جدا في قوائم.كرر_كل".to_string(),
            line,
            col,
        ));
    }
    let mut out = Vec::new();
    for (it, n) in items.iter().zip(counts.iter()) {
        for _ in 0..*n {
            out.push(it.clone());
        }
    }
    Ok(Value::List(out))
}


/// قوائم.فهرس_أول(قائمة، عنصر) — فهرس أول ظهور (البند 37):
/// -١ إن غاب العنصر؛ والمساواة عميقة كما في تطابق.
fn lists_index_first(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فهرس_أول";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?;
    for (i, it) in items.iter().enumerate() {
        if *it == args[1] {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}


/// قوائم.فهرس_آخر(قائمة، عنصر) — فهرس آخر ظهور (البند 37):
/// -١ إن غاب العنصر.
fn lists_index_last(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فهرس_آخر";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?;
    for (i, it) in items.iter().enumerate().rev() {
        if *it == args[1] {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}


/// قوائم.فهرس_فترات(حدود، قيم) — في أي فترة يقع كل عدد
/// (البند 38): النتيجة عدد الحدود التي هي ≤ العدد؛
/// والحدود مرتبة تصاعديا وإلا خطأ عربي.
fn lists_interval_index(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فهرس_فترات";
    need_args(D, args, 2, line, col)?;
    let bounds_v = need_list(D, &args[0], line, col)?.clone();
    let mut bounds = Vec::new();
    for b in &bounds_v {
        bounds.push(need_number(D, b, line, col)?);
    }
    for w in bounds.windows(2) {
        if w[1] < w[0] {
            return Err(RioError::runtime(
                "حدود قوائم.فهرس_فترات غير مرتبة تصاعديا".to_string(),
                line,
                col,
            ));
        }
    }
    let vals = need_list(D, &args[1], line, col)?.clone();
    let mut out = Vec::new();
    for v in &vals {
        let x = need_number(D, v, line, col)?;
        out.push(Value::Number(bounds.iter().filter(|b| **b <= x).count() as f64));
    }
    Ok(Value::List(out))
}


/// قوائم.فهرس_تبديل(تبديل) — رتبة تبديل في الترتيب المعجمي
/// (البند 39): التبديل أعداد ٠ إلى ن-١ بلا تكرار؛ والرتبة
/// صفرية؛ ون > ٢٠ خطأ (٢٠! آخر عاملي في صحيح ٦٤ بت).
fn lists_perm_rank(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فهرس_تبديل";
    need_args(D, args, 1, line, col)?;
    let p_v = need_list(D, &args[0], line, col)?.clone();
    let n = p_v.len();
    if n > 20 {
        return Err(RioError::runtime(
            "تبديل طويل جدا في قوائم.فهرس_تبديل (الأقصى ٢٠)".to_string(),
            line,
            col,
        ));
    }
    let p = j2_need_perm(D, &p_v, line, col)?;
    let mut fact: Vec<i64> = vec![1; n + 1];
    for i in 1..=n {
        fact[i] = fact[i - 1] * i as i64;
    }
    let mut rank: i64 = 0;
    let mut used = vec![false; n];
    for i in 0..n {
        let smaller = (0..p[i]).filter(|j| !used[*j]).count() as i64;
        rank += smaller * fact[n - 1 - i];
        used[p[i]] = true;
    }
    Ok(Value::Number(rank as f64))
}


/// قوائم.طبّق_تبديل(قائمة، تبديل) — إعادة ترتيب قائمة
/// بتبديل (البند 39): النتيجة[ن] = القائمة[تبديل[ن]]؛
/// والتبديل يجب أن يطابق طول القائمة تماما بلا تكرار.
fn lists_apply_perm(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.طبق_تبديل";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let p_v = need_list(D, &args[1], line, col)?.clone();
    if p_v.len() != items.len() {
        return Err(RioError::runtime(
            format!(
                "قوائم.طبق_تبديل تتطلب تبديلا بطول القائمة ({} مقابل {})",
                p_v.len(),
                items.len()
            ),
            line,
            col,
        ));
    }
    let p = j2_need_perm(D, &p_v, line, col)?;
    let mut out = Vec::new();
    for i in p {
        out.push(items[i].clone());
    }
    Ok(Value::List(out))
}


/// قوائم.اعكس(قيمة) — عكس ترتيب قائمة أو محارف نص (البند 40).
fn lists_reverse(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.اعكس";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::List(items) => Ok(Value::List(items.iter().rev().cloned().collect())),
        Value::Str(s) => Ok(Value::Str(s.chars().rev().collect())),
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// قوائم.دوّر(قيمة، عدد) — تدوير قائمة أو نص (البند 40):
/// الموجب يسار (الأوائل إلى النهاية)، والسالب يمين؛
/// والعدد يُؤخذ باقي الطول؛ والفارغ يعيد فارغا.
fn lists_rotate(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.دور";
    need_args(D, args, 2, line, col)?;
    let n = need_integer(D, &args[1], line, col)?;
    match &args[0] {
        Value::List(items) => {
            let len = items.len();
            if len == 0 {
                return Ok(Value::List(vec![]));
            }
            let k = (((n % len as i64) + len as i64) % len as i64) as usize;
            let mut out = items[k..].to_vec();
            out.extend_from_slice(&items[..k]);
            Ok(Value::List(out))
        }
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len();
            if len == 0 {
                return Ok(Value::Str(String::new()));
            }
            let k = (((n % len as i64) + len as i64) % len as i64) as usize;
            let mut out: Vec<char> = chars[k..].to_vec();
            out.extend_from_slice(&chars[..k]);
            Ok(Value::Str(out.iter().collect()))
        }
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع قائمة أو نصا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// قوائم.بدّل_المحاور(مصفوفة) — تبديل صفوف مصفوفة بأعمدتها
/// (البند 41): الصفوف قوائم متساوية الطول وإلا خطأ عربي؛
/// والمصفوفة الفارغة تعيد فارغة.
fn lists_transpose(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.بدل_المحاور";
    need_args(D, args, 1, line, col)?;
    let rows_v = need_list(D, &args[0], line, col)?.clone();
    if rows_v.is_empty() {
        return Ok(Value::List(vec![]));
    }
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for r in &rows_v {
        rows.push(j2_need_row(D, r, line, col)?.clone());
    }
    let w = rows[0].len();
    for r in &rows {
        if r.len() != w {
            return Err(RioError::runtime(
                "صفوف غير متساوية في قوائم.بدل_المحاور".to_string(),
                line,
                col,
            ));
        }
    }
    let mut out = Vec::new();
    for j in 0..w {
        let mut col = Vec::new();
        for r in &rows {
            col.push(r[j].clone());
        }
        out.push(Value::List(col));
    }
    Ok(Value::List(out))
}


/// قوائم.علّب(قيمة) — وضع أي قيمة في علبة (البند 42):
/// قاموس بمفتاح واحد «علبة»؛ فتبقى القائمة المغلّفة
/// عنصرا واحدا في التسطيح والتفكيك.
fn lists_box(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.علب";
    need_args(D, args, 1, line, col)?;
    Ok(Value::Dict(vec![("علبة".to_string(), args[0].clone())]))
}


/// قوائم.افتح(علبة) — فتح العلبة وإرجاع قيمتها (البند 42):
/// ما ليس قاموسا بمفتاح واحد «علبة» خطأ نوع عربي.
fn lists_unbox(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.افتح";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::Dict(pairs) if pairs.len() == 1 && pairs[0].0 == "علبة" => Ok(pairs[0].1.clone()),
        _ => Err(RioError::typ(
            "الدالة قوائم.افتح تتوقع علبة من قوائم.علب".to_string(),
            line,
            col,
        )),
    }
}


/// قوائم.فريد(قائمة) — العناصر الفريدة بلا تكرار (البند 45):
/// بترتيب أول ظهور؛ والمساواة عميقة كما في تطابق.
fn lists_unique(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فريد";
    need_args(D, args, 1, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut out: Vec<Value> = Vec::new();
    for it in items {
        if !out.iter().any(|x| x == &it) {
            out.push(it);
        }
    }
    Ok(Value::List(out))
}


/// قوائم.قناع_فريد(قائمة) — قناع يميز أول ظهور (البند 45):
/// صحيح عند أول ظهور للعنصر وخطأ عند تكراره.
fn lists_unique_mask(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.قناع_فريد";
    need_args(D, args, 1, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut seen: Vec<Value> = Vec::new();
    let mut out = Vec::new();
    for it in items {
        let first = !seen.iter().any(|x| x == &it);
        if first {
            seen.push(it);
        }
        out.push(Value::Bool(first));
    }
    Ok(Value::List(out))
}


/// قوائم.قارن_الكل_بالكل(قائمة) — مصفوفة منطقية تبين
/// تساوي كل عنصر مع كل عنصر (البند 46): النتيجة[س][ص]
/// صحيح إن تساوى العنصران؛ والفارغة تعيد فارغة.
fn lists_self_compare(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.قارن_الكل_بالكل";
    need_args(D, args, 1, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut out = Vec::new();
    for a in &items {
        let mut row = Vec::new();
        for b in &items {
            row.push(Value::Bool(a == b));
        }
        out.push(Value::List(row));
    }
    Ok(Value::List(out))
}


/// قوائم.فهرس_مسار(قيمة، مسار) — جلب عنصر من بنية متداخلة
/// بمسار فهارس (البند 52): عدد للقائمة (والسالب من
/// النهاية)، ونص لمفتاح القاموس؛ والمسار الفارغ يعيد
/// القيمة نفسها؛ والخطوة الخاطئة خطأ عربي.
fn lists_path_index(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.فهرس_مسار";
    need_args(D, args, 2, line, col)?;
    let mut cur = args[0].clone();
    let path = need_list(D, &args[1], line, col)?.clone();
    for step in &path {
        match step {
            Value::Number(_) => {
                let i = need_integer(D, step, line, col)?;
                match &cur {
                    Value::List(items) => {
                        let pos = j2_resolve_index(D, items.len() as i64, i, line, col)?;
                        cur = items[pos].clone();
                    }
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتطلب قائمة للفهرس العددي لكن أعطيت {}",
                                D,
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                }
            }
            Value::Str(key) => match &cur {
                Value::Dict(pairs) => match pairs.iter().find(|(k, _)| k == key) {
                    Some((_, v)) => cur = v.clone(),
                    None => {
                        return Err(RioError::runtime(
                            format!("المفتاح {} غائب في قوائم.فهرس_مسار", key),
                            line,
                            col,
                        ))
                    }
                },
                other => {
                    return Err(RioError::typ(
                        format!(
                            "الدالة {} تتطلب قاموسا للمفتاح النصي لكن أعطيت {}",
                            D,
                            other.type_name()
                        ),
                        line,
                        col,
                    ))
                }
            },
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتطلب عددا أو نصا في المسار لكن أعطيت {}",
                        D,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(cur)
}

// END J2-GEN fns-lists
// BEGIN J5-GEN fns-lists
// ---------- J5 — الرتبة والخلايا (الدفعة الأخيرة من الـ579، الخطوة 4 §88) ----------

/// عملية بث ثنائية على عددين (J5): جمع، طرح، ضرب،
/// قسمة، باقي، أكبر، أصغر؛ والقسمة أو الباقي على
/// صفر خطأ عربي؛ وغيرها خطأ عملية غير معروفة.
fn j5_apply_op(display: &str, op: &str, x: f64, y: f64, line: usize, col: usize) -> RioResult<f64> {
    match op {
        "جمع" => Ok(x + y),
        "طرح" => Ok(x - y),
        "ضرب" => Ok(x * y),
        "قسمة" => {
            if y == 0.0 {
                return Err(RioError::runtime(
                    format!("قسمة على صفر في {}", display),
                    line,
                    col,
                ));
            }
            Ok(x / y)
        }
        "باقي" => {
            if y == 0.0 {
                return Err(RioError::runtime(
                    format!("باقي القسمة على صفر في {}", display),
                    line,
                    col,
                ));
            }
            Ok(x % y)
        }
        "أكبر" => Ok(x.max(y)),
        "أصغر" => Ok(x.min(y)),
        _ => Err(RioError::runtime(
            format!(
                "عملية بث غير معروفة: {} — المتاح: جمع، طرح، ضرب، قسمة، باقي، أكبر، أصغر",
                op
            ),
            line,
            col,
        )),
    }
}

/// هل نص العملية من عمليات البث السبع (J5)؟
fn j5_known_op(op: &str) -> bool {
    matches!(
        op,
        "جمع" | "طرح" | "ضرب" | "قسمة" | "باقي" | "أكبر" | "أصغر"
    )
}

/// تطبيق عملية بث واحدة على زوج (J5): نص عملية على
/// عددين، أو دالة نيفا تستلم زوجا [أ، ب].
fn j5_broadcast_one(
    ctx: &mut dyn HandlerCaller,
    display: &str,
    op: &Value,
    x: Value,
    y: Value,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    match op {
        Value::Str(s) => {
            let a = need_number(display, &x, line, col)?;
            let b = need_number(display, &y, line, col)?;
            Ok(Value::Number(j5_apply_op(display, s, a, b, line, col)?))
        }
        _ => j3_call_pair(ctx, op, x, y, line, col),
    }
}

/// قوائم.طبق_على_كل_عنصر(قائمة، دالة) — تطبيق دالة
/// على كل عنصر وإرجاع قائمة النتائج بالترتيب (البند 93:
/// رتبة الفعل بصيغة عربية صريحة)؛ والدالة تستلم
/// العنصر وحده بمعامل واحد؛ والقائمة الفارغة تعيد
/// فارغة؛ وغير الدالة أو غير القائمة خطأ عربي.
/// المفتاح المخزن «طبق_على_كل_عنصر» (تجريد الشدة من طبّق).
fn lists_apply_each(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.طبق_على_كل_عنصر";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    j3_need_func(D, &args[1], line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        out.push(j3_call_single(ctx, &args[1], it, line, col)?);
    }
    Ok(Value::List(out))
}


/// قوائم.طبق_على_الصفوف(مصفوفة، دالة) — تطبيق دالة
/// على كل صف (قائمة) وإرجاع قائمة النتائج بالترتيب
/// (البند 93)؛ والدالة تستلم الصف كاملا بمعامل واحد؛
/// وصف ليس قائمة خطأ عربي؛ والمصفوفة الفارغة تعيد فارغة.
/// المفتاح المخزن «طبق_على_الصفوف» (تجريد الشدة من طبّق).
fn lists_apply_rows(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.طبق_على_الصفوف";
    need_args(D, args, 2, line, col)?;
    let rows = need_list(D, &args[0], line, col)?.clone();
    j3_need_func(D, &args[1], line, col)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let row = j2_need_row(D, &r, line, col)?.clone();
        out.push(j3_call_single(ctx, &args[1], Value::List(row), line, col)?);
    }
    Ok(Value::List(out))
}


/// قوائم.بث(قائمة، قيمة، عملية) — الاتفاق/البث (البند 95)
/// بصيغة صريحة: قيمة عددية تُبث على كل عنصر، أو قائمة
/// بنفس الطول تُجمع عنصرا بعنصر؛ والعملية نص من السبع
/// (جمع، طرح، ضرب، قسمة، باقي، أكبر، أصغر — أعداد فقط)
/// أو دالة نيفا تستلم زوجا [أ، ب] (أي قيمتين)؛ وأطوال
/// مختلفة خطأ عربي؛ والقائمة الفارغة تعيد فارغة قبل
/// أي فحص (للطول أو العملية أو القسمة) — قاعدة البند 97.
/// لا بث في المعاملات نفسها: `+` على قائمتين دمج لا بث.
fn lists_broadcast(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.بث";
    need_args(D, args, 3, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    if items.is_empty() {
        return Ok(Value::List(vec![]));
    }
    if let Value::Str(op) = &args[2] {
        if !j5_known_op(op) {
            return Err(RioError::runtime(
                format!(
                    "عملية بث غير معروفة: {} — المتاح: جمع، طرح، ضرب، قسمة، باقي، أكبر، أصغر",
                    op
                ),
                line,
                col,
            ));
        }
    } else {
        j3_need_func(D, &args[2], line, col)?;
    }
    let mut out = Vec::with_capacity(items.len());
    match &args[1] {
        Value::List(other) => {
            if other.len() != items.len() {
                return Err(RioError::runtime(
                    format!(
                        "البث في قوائم.بث يتطلب أطوالا متساوية ({} مقابل {})",
                        items.len(),
                        other.len()
                    ),
                    line,
                    col,
                ));
            }
            for (x, y) in items.iter().zip(other.iter()) {
                out.push(j5_broadcast_one(ctx, D, &args[2], x.clone(), y.clone(), line, col)?);
            }
        }
        scalar => {
            for x in items {
                out.push(j5_broadcast_one(ctx, D, &args[2], x, scalar.clone(), line, col)?);
            }
        }
    }
    Ok(Value::List(out))
}


/// قوائم.بث_بمالئ(قائمة، قائمة، مالئ، عملية) — الحشو
/// عند الجمع (البند 96) بمعامل قيمة مالئة صريح: الأقصر
/// من القائمتين يُكمل بالمالئ حتى طول الأطول، ثم تُطبق
/// العملية عنصرا بعنصر؛ والعملية نص من السبع (أعداد
/// فقط، والمالئ حينها عدد) أو دالة زوج [أ، ب] (أي قيمتين)؛
/// والفارغتان تعيدان فارغة؛ وفارغة مع غير فارغة تُكمل
/// كلها بالمالئ.
fn lists_broadcast_fill(ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "قوائم.بث_بمالئ";
    need_args(D, args, 4, line, col)?;
    let a = need_list(D, &args[0], line, col)?.clone();
    let b = need_list(D, &args[1], line, col)?.clone();
    if a.is_empty() && b.is_empty() {
        return Ok(Value::List(vec![]));
    }
    if let Value::Str(op) = &args[3] {
        if !j5_known_op(op) {
            return Err(RioError::runtime(
                format!(
                    "عملية بث غير معروفة: {} — المتاح: جمع، طرح، ضرب، قسمة، باقي، أكبر، أصغر",
                    op
                ),
                line,
                col,
            ));
        }
    } else {
        j3_need_func(D, &args[3], line, col)?;
    }
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or_else(|| args[2].clone());
        let y = b.get(i).cloned().unwrap_or_else(|| args[2].clone());
        out.push(j5_broadcast_one(ctx, D, &args[3], x, y, line, col)?);
    }
    Ok(Value::List(out))
}

// END J5-GEN fns-lists
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

// BEGIN J2-GEN fns-text
// ---------- J2 — يونيكود والأبجدية (الدفعة الأخيرة من الـ579) ----------

/// نص.رمز_يونيكود(قيمة) — تحويل بين حرف ورمز يونيكود
/// (البند 50): نص من محرف واحد يعيد رمزه؛ وعدد صحيح
/// صالح يعيد المحرف نصا؛ وغيرهما خطأ عربي.
fn text_unicode(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "نص.رمز_يونيكود";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::Str(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(Value::Number(c as u32 as f64)),
                _ => Err(RioError::runtime(
                    "الدالة نص.رمز_يونيكود تتطلب نصا من محرف واحد".to_string(),
                    line,
                    col,
                )),
            }
        }
        Value::Number(_) => {
            let n = need_integer(D, &args[0], line, col)?;
            if !(0..=0x10FFFF).contains(&n) || (0xD800..0xE000).contains(&n) {
                return Err(RioError::runtime(
                    "رمز يونيكود غير صالح في نص.رمز_يونيكود".to_string(),
                    line,
                    col,
                ));
            }
            match char::from_u32(n as u32) {
                Some(c) => Ok(Value::Str(c.to_string())),
                None => Err(RioError::runtime(
                    "رمز يونيكود غير صالح في نص.رمز_يونيكود".to_string(),
                    line,
                    col,
                )),
            }
        }
        other => Err(RioError::typ(
            format!("الدالة {} تتوقع نصا أو عددا لكن أعطيت {}", D, other.type_name()),
            line,
            col,
        )),
    }
}


/// نص.أبجدية() — قائمة الحروف العربية الثمانية والعشرين
/// (البند 51): كل حرف نص من محرف واحد، بالترتيب الأبجدي.
fn text_alphabet(_ctx: &mut dyn HandlerCaller, args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const D: &str = "نص.أبجدية";
    need_args(D, args, 0, line, col)?;
    let letters = "ابتثجحخدذرزسشصضطظعغفقكلمنهوي";
    Ok(Value::List(
        letters.chars().map(|c| Value::Str(c.to_string())).collect(),
    ))
}

// END J2-GEN fns-text
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

// ---------- وحدة تدفق: دفعة لينكس 2، البنّاء اليدوي H2 ----------
// البنود 129–133 (آلية عنوان) و145 (عدل_في_مكانه) و146 (نسخة_احتياطية).
// آلية العنوان داخلية (بلا دالة مستخدم بعد): تحلّ قيمة
// عنوان إلى أرقام أسطر (1-based) مرتبة في نص معطى؛
// ويستدعيها مولّد الدفعة (G) في أوامر تدفق. والمرآة C
// (flow_address_lines_c في niva_std.c) بالدلالة نفسها
// حرفيا — فأي اختلاف بينهما عيب تكافؤ. أشكال العنوان:
// - عدد: سطر واحد؛ و-1 آخر سطر؛ و0 أو سالب آخر ← خطأ عربي.
// - قائمة عددين [بداية، نهاية]: مدى شامل؛ وبداية > نهاية ← فارغة.
// - نص: نمط — الأسطر المطابقة له عبر محرك نمط؛ والنمط غير
//   الصالح ← خطأ المحرك نفسه.
// - قائمة نصين [نمط1، نمط2]: من أول سطر يطابق نمط1 حتى أول
//   سطر يطابق نمط2 بعده شاملا؛ وإن لم يطابق نمط2 أبدا فإلى النهاية.
// - لاشيء: كل الأسطر.
// والحد -1 يعني آخر سطر في حدّي المدى أيضا؛ والعدد الموجب
// خارج النص لا يخطئ — مجموعة فارغة أو مدى مقصوص؛ ونمط2
// يُبحث بعد سطر نمط1 حتما (فإن طابق السطر نفسه لم ينه المدى).
// و`display` اسم الدالة المتصلة في رسائل الخطأ (فالآلية
// داخلية بلا اسم مستخدم ثابت).

/// أسطر نص بمعنى lines() — بلا سطر أخير فارغ.
fn flow_lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// هل يطابق السطر النمط المجمّع (بحث لا مطابقة كاملة)؟
fn flow_line_matches(
    prog: &RxProg,
    line_text: &str,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    let chars = rx_check_text(line_text).map_err(|e| rx_err(e, line, col))?;
    Ok(rx_find_from(prog, &chars, 0)
        .map_err(|e| rx_err(e, line, col))?
        .is_some())
}

/// حدّ مدى (بداية أو نهاية): عدد صحيح؛ و-1 آخر سطر؛
/// و0 أو سالب آخر ← خطأ عربي.
fn flow_address_bound(
    display: &str,
    v: &Value,
    total: i64,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    let n = need_integer(display, v, line, col)?;
    if n == -1 {
        Ok(total)
    } else if n <= 0 {
        Err(RioError::runtime(
            format!(
                "الدالة {} عنوان السطر {} غير صالح (الصالح 1 أو أكثر، و-1 لآخر سطر)",
                display, n
            ),
            line,
            col,
        ))
    } else {
        Ok(n)
    }
}

/// حلّ عنوان تدفق إلى أرقام أسطر (1-based) مرتبة.
/// والطيّ (تجاهل_الحالة) يطوي أنماط العنوان النصية
/// عند صحيح؛ وبلا طيّ تبقى حساسة للحالة.
fn flow_address_lines(
    display: &str,
    address: &Value,
    lines: &[&str],
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<Vec<usize>> {
    let total = lines.len() as i64;
    match address {
        Value::Nil => Ok((1..=total).map(|n| n as usize).collect()),
        Value::Number(_) => {
            let n = flow_address_bound(display, address, total, line, col)?;
            if total == 0 || n == 0 || n > total {
                Ok(Vec::new())
            } else {
                Ok(vec![n as usize])
            }
        }
        Value::Str(pattern) => {
            let prog = rx_compile_fold(pattern, false, fold).map_err(|e| rx_err(e, line, col))?;
            let mut out: Vec<usize> = Vec::new();
            for (idx, raw) in lines.iter().enumerate() {
                if flow_line_matches(&prog, raw, line, col)? {
                    out.push(idx + 1);
                }
            }
            Ok(out)
        }
        Value::List(items) => {
            if items.len() != 2 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع قائمة عنوان من عنصرين لكن أعطيت {}",
                        display,
                        items.len()
                    ),
                    line,
                    col,
                ));
            }
            match (&items[0], &items[1]) {
                (Value::Number(_), Value::Number(_)) => {
                    let start = flow_address_bound(display, &items[0], total, line, col)?;
                    let end = flow_address_bound(display, &items[1], total, line, col)?;
                    if total == 0 || start == 0 || start > total || start > end {
                        Ok(Vec::new())
                    } else {
                        let end = end.min(total);
                        Ok((start..=end).map(|n| n as usize).collect())
                    }
                }
                (Value::Str(p1), Value::Str(p2)) => {
                    let prog1 =
                        rx_compile_fold(p1, false, fold).map_err(|e| rx_err(e, line, col))?;
                    let prog2 =
                        rx_compile_fold(p2, false, fold).map_err(|e| rx_err(e, line, col))?;
                    let mut start: Option<usize> = None;
                    for (idx, raw) in lines.iter().enumerate() {
                        if flow_line_matches(&prog1, raw, line, col)? {
                            start = Some(idx);
                            break;
                        }
                    }
                    let Some(s) = start else {
                        return Ok(Vec::new());
                    };
                    // نمط2 بعد سطر نمط1 حتما؛ وإن لم يطابق
                    // أبدا فالمدى إلى النهاية (دلالة sed).
                    let mut end = lines.len();
                    for (idx, raw) in lines.iter().enumerate().skip(s + 1) {
                        if flow_line_matches(&prog2, raw, line, col)? {
                            end = idx + 1;
                            break;
                        }
                    }
                    Ok((s + 1..=end).collect())
                }
                _ => Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع قائمة عنوان من عددين أو نصين",
                        display
                    ),
                    line,
                    col,
                )),
            }
        }
        other => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عنوانا: عددا أو قائمة عددين أو نصا أو قائمة نصين أو لاشيء لكن أعطيت {}",
                display,
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// حلّ عنوان في نص مباشرة (يقسمه أسطر أولا).
#[allow(dead_code)]
fn flow_address_lines_in_text(
    display: &str,
    address: &Value,
    text: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<Vec<usize>> {
    let lines = flow_lines(text);
    flow_address_lines(display, address, &lines, fold, line, col)
}

/// تدفق.استبدل_الكل(نص، نمط، بديل، خيارات؟) — البند
/// 117: استبدل كل مطابقات النمط؛ وخياراتها عنوان
/// وتجاهل_الحالة (يسري على نمط الاستبدال وعلى نمط
/// العنوان معا)؛ وبلا عنوان ≡ نمط.استبدل حرفيا.
fn flow_replace_all(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.استبدل_الكل";
    if args.len() < 3 || args.len() > 4 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع ثلاثة معاملات أو أربعة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let pattern = need_string(DISPLAY, &args[1], line, col)?;
    let repl = need_string(DISPLAY, &args[2], line, col)?;
    let mut address: Option<Value> = None;
    let mut fold = false;
    if args.len() == 4 {
        let pairs = need_dict(DISPLAY, &args[3], line, col)?;
        let opts = flow_parse_opts(DISPLAY, pairs, true, false, line, col)?;
        address = opts.address;
        fold = opts.ignore_case;
    }
    Ok(Value::Str(flow_replace_all_core(
        DISPLAY,
        &text,
        &pattern,
        &repl,
        address.as_ref(),
        fold,
        line,
        col,
    )?))
}

/// تدفق.احذف_أسطر(نص، عنوان؟) — البند 124: احذف
/// الأسطر المعنونة؛ والافتراضي لاشيء = كل الأسطر.
fn flow_delete_lines(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.احذف_أسطر";
    if args.len() < 1 || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملا أو معاملين لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let address = if args.len() == 2 { Some(&args[1]) } else { None };
    Ok(Value::Str(flow_delete_lines_core(
        DISPLAY, &text, address, false, line, col,
    )?))
}

/// تدفق.اطبع_المطابق(نص، عنوان؟) — البند 125:
/// ترجع قائمة الأسطر المعنونة، لا تطبع.
fn flow_print_matching(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.اطبع_المطابق";
    if args.len() < 1 || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملا أو معاملين لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let address = if args.len() == 2 { Some(&args[1]) } else { None };
    let found = flow_matching_lines_core(DISPLAY, &text, address, false, line, col)?;
    Ok(Value::List(
        found.into_iter().map(Value::Str).collect(),
    ))
}

/// تدفق.توقف_عند(نص، عدد) — البند 127: الأسطر
/// 1..N شاملة؛ وN أكبر من المجموع ← النص كله؛
/// و0 أو سالب ← خطأ عربي؛ و-1 ← آخر سطر.
fn flow_take_lines(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.توقف_عند";
    need_args(DISPLAY, args, 2, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    Ok(Value::Str(flow_take_lines_core(
        DISPLAY, &text, n, line, col,
    )?))
}

/// تدفق.ألحق_بعد(نص، عنوان، نص_ملحق) — البند 135:
/// أدخل أسطر النص الملحق بعد كل سطر معنون.
fn flow_append_after(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.ألحق_بعد";
    need_args(DISPLAY, args, 3, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let appended = need_string(DISPLAY, &args[2], line, col)?;
    Ok(Value::Str(flow_append_after_core(
        DISPLAY,
        &text,
        Some(&args[1]),
        &appended,
        false,
        line,
        col,
    )?))
}

/// تدفق.أدرج_قبل(نص، عنوان، نص_مدرج) — البند 136:
/// أدخل أسطر النص المدرج قبل كل سطر معنون.
fn flow_insert_before(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.أدرج_قبل";
    need_args(DISPLAY, args, 3, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let inserted = need_string(DISPLAY, &args[2], line, col)?;
    Ok(Value::Str(flow_insert_before_core(
        DISPLAY,
        &text,
        Some(&args[1]),
        &inserted,
        false,
        line,
        col,
    )?))
}

/// تدفق.غير_أسطر(نص، عنوان، نص_جديد) — البند 137:
/// استبدال كل امتداد متصل من الأسطر المعنونة
/// بالنص الجديد مرة واحدة (sed المخلصة).
fn flow_change_lines(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.غير_أسطر";
    need_args(DISPLAY, args, 3, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let new_text = need_string(DISPLAY, &args[2], line, col)?;
    Ok(Value::Str(flow_change_lines_core(
        DISPLAY,
        &text,
        Some(&args[1]),
        &new_text,
        false,
        line,
        col,
    )?))
}

/// تدفق.عالج(نص، نمط، إجراء، خيارات؟) — البنود
/// 151 و152 و153 (يدوي: نموذج جديد): معالجة الأسطر
/// بنمط وإجراء؛ والإجراء دالة تستلم سجلا واحدا
/// {سطر، رقم، حقول} (رقم السجل 157) وتعيد قيمة أو
/// لاشيء (فتُتخطى)؛ والخيارات: قبل (BEGIN — دالة
/// تستلم {عدد_الأسطر}) وبعد (END — دالة تستلم
/// {عدد_الأسطر، عدد_المطابق}) وفاصل (159 — الفاصل
/// نفسه علم -F في 163). والنمط الفارغ ≡ كل الأسطر.
/// والنتيجة قائمة [قبل؟، نتائج الإجراء...، بعد؟].
/// تقسيم الحقول من المساعد المشترك b3_split_fields
/// (يولّده المولّد)؛ والنمط يُجمَّع قبل أي إجراء.
fn flow_process(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.عالج";
    if args.len() < 3 || args.len() > 4 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع ثلاثة معاملات أو أربعة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let pattern = need_string(DISPLAY, &args[1], line, col)?;
    j3_need_func(DISPLAY, &args[2], line, col)?;
    let mut before: Option<Value> = None;
    let mut after: Option<Value> = None;
    let mut sep: Option<String> = None;
    if args.len() == 4 {
        let pairs = need_dict(DISPLAY, &args[3], line, col)?;
        for (k, v) in pairs {
            match k.as_str() {
                "قبل" => {
                    j3_need_func(DISPLAY, v, line, col)?;
                    before = Some(v.clone());
                }
                "بعد" => {
                    j3_need_func(DISPLAY, v, line, col)?;
                    after = Some(v.clone());
                }
                "فاصل" => {
                    let s = need_string(DISPLAY, v, line, col)?;
                    if s.is_empty() {
                        return Err(RioError::runtime(
                            format!("الدالة {} فاصل الحقول فارغ", DISPLAY),
                            line,
                            col,
                        ));
                    }
                    sep = Some(s);
                }
                other => {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", DISPLAY, other),
                        line,
                        col,
                    ));
                }
            }
        }
    }
    let prog = if pattern.is_empty() {
        None
    } else {
        Some(rx_compile(&pattern, false).map_err(|e| rx_err(e, line, col))?)
    };
    let lines = flow_lines(&text);
    let total = lines.len() as i64;
    let mut out: Vec<Value> = Vec::new();
    if let Some(f) = &before {
        let info = Value::Dict(vec![(
            "عدد_الأسطر".to_string(),
            Value::Number(total as f64),
        )]);
        let r = j3_call_single(ctx, f, info, line, col)?;
        if r != Value::Nil {
            out.push(r);
        }
    }
    let mut matched = 0i64;
    for (i, l) in lines.iter().enumerate() {
        let m = match &prog {
            None => true,
            Some(p) => flow_line_matches(p, l, line, col)?,
        };
        if !m {
            continue;
        }
        matched += 1;
        let fields = b3_split_fields(l, sep.as_deref());
        let rec = Value::Dict(vec![
            ("سطر".to_string(), Value::Str(l.to_string())),
            ("رقم".to_string(), Value::Number((i + 1) as f64)),
            (
                "حقول".to_string(),
                Value::List(fields.into_iter().map(Value::Str).collect()),
            ),
        ]);
        let r = j3_call_single(ctx, &args[2], rec, line, col)?;
        if r != Value::Nil {
            out.push(r);
        }
    }
    if let Some(f) = &after {
        let info = Value::Dict(vec![
            ("عدد_الأسطر".to_string(), Value::Number(total as f64)),
            (
                "عدد_المطابق".to_string(),
                Value::Number(matched as f64),
            ),
        ]);
        let r = j3_call_single(ctx, f, info, line, col)?;
        if r != Value::Nil {
            out.push(r);
        }
    }
    Ok(Value::List(out))
}

// BEGIN لينكس3-نص مساعدات Rust
/// مساعدات مشتركة لدفعة لينكس 3، موجة النص (مولّدة).

/// تقسيم سطر إلى حقول: بلا فاصل ← مسافات بيضاء
/// متتابعة (awk الافتراضي)؛ وبفاصل حرفي ← تقسيم
/// به مع حفظ الفارغ.
fn b3_split_fields(line: &str, sep: Option<&str>) -> Vec<String> {
    match sep {
        None => line.split_whitespace().map(|s| s.to_string()).collect(),
        Some(s) => line.split(s).map(|x| x.to_string()).collect(),
    }
}

/// تحويل awk العددي لحقل: بادئة عددية أولى وإلا 0
/// (و"12abc" ← 12، و"abc" ← 0).
fn b3_field_number(field: &str) -> f64 {
    let t = field.trim_start();
    if t.is_empty() {
        return 0.0;
    }
    let b = t.as_bytes();
    let mut i = 0usize;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = false;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits = true;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits = true;
        }
    }
    let mut end = i;
    if digits && i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let mut ed = false;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            ed = true;
        }
        if ed {
            end = j;
        }
    }
    if !digits || end == 0 {
        return 0.0;
    }
    t[..end].parse::<f64>().unwrap_or(0.0)
}

/// خيار فاصل تدفق: قاموس اختياري بمفتاح فاصل فقط
/// (غير فارغ)؛ وبلا قاموس ← بلا فاصل.
fn b3_flow_sep_opt(
    display: &str,
    opt: Option<&Value>,
    line: usize,
    col: usize,
) -> RioResult<Option<String>> {
    match opt {
        None => Ok(None),
        Some(v) => {
            let pairs = need_dict(display, v, line, col)?;
            let mut sep: Option<String> = None;
            for (k, val) in pairs {
                match k.as_str() {
                    "فاصل" => {
                        let s = need_string(display, val, line, col)?;
                        if s.is_empty() {
                            return Err(RioError::runtime(
                                format!("الدالة {} فاصل الحقول فارغ", display),
                                line,
                                col,
                            ));
                        }
                        sep = Some(s);
                    }
                    other => {
                        return Err(RioError::runtime(
                            format!("الدالة {} لا تقبل الخيار {}", display, other),
                            line,
                            col,
                        ));
                    }
                }
            }
            Ok(sep)
        }
    }
}

/// ترتيب JSON الكلي (jq): لاشيء < منطقي < عدد <
/// نص < قائمة < قاموس؛ والأعداد عدديا، والنصوص
/// معجميا، والقوائم والقواميس معجميا بمفاتيح مرتبة.
fn b3_json_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let rank = |v: &Value| match v {
        Value::Nil => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::Str(_) => 3,
        Value::List(_) => 4,
        Value::Dict(_) => 5,
        _ => 6,
    };
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Equal),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::List(x), Value::List(y)) => {
            for (u, v) in x.iter().zip(y.iter()) {
                let c = b3_json_cmp(u, v);
                if c != Equal {
                    return c;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Dict(x), Value::Dict(y)) => {
            let mut xs: Vec<&(String, Value)> = x.iter().collect();
            let mut ys: Vec<&(String, Value)> = y.iter().collect();
            xs.sort_by(|p, q| p.0.cmp(&q.0));
            ys.sort_by(|p, q| p.0.cmp(&q.0));
            for (p, q) in xs.iter().zip(ys.iter()) {
                match p.0.cmp(&q.0) {
                    Equal => {
                        let c = b3_json_cmp(&p.1, &q.1);
                        if c != Equal {
                            return c;
                        }
                    }
                    other => return other,
                }
            }
            xs.len().cmp(&ys.len())
        }
        _ => Equal,
    }
}

/// قيمة JSON إلى نصها (إلى_نص): النص نفسه، وإلا
/// JSON مضغوط غربي الأرقام.
fn b3_json_to_text(v: &Value) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        _ => {
            let mut out = String::new();
            json_write_value(v, &mut out, 0)?;
            Ok(out)
        }
    }
}

/// نسخة بمفاتيح مرتبة تكراريا في كل مستوى.
fn b3_json_sorted(v: &Value) -> Value {
    match v {
        Value::Dict(pairs) => {
            let mut ps: Vec<(String, Value)> = pairs
                .iter()
                .map(|(k, val)| (k.clone(), b3_json_sorted(val)))
                .collect();
            ps.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Dict(ps)
        }
        Value::List(items) => Value::List(items.iter().map(b3_json_sorted).collect()),
        other => other.clone(),
    }
}

/// القيمة وكل ما تحتها تكراريا (النزول ..) — القيمة
/// نفسها أولا ثم الأبناء بترتيبهم.
fn b3_json_recurse(v: &Value, out: &mut Vec<Value>) {
    out.push(v.clone());
    match v {
        Value::List(items) => {
            for it in items {
                b3_json_recurse(it, out);
            }
        }
        Value::Dict(pairs) => {
            for (_, val) in pairs {
                b3_json_recurse(val, out);
            }
        }
        _ => {}
    }
}

// END لينكس3-نص مساعدات Rust
// BEGIN لينكس3-نص دوال تدفق Rust
/// تدفق.حقل(سطر، رقم، خيارات؟) — البند 154:
/// الحقل رقم ن (1-based)؛ و0 (155) = السطر الكامل؛
/// وخارج النطاق ← نص فارغ؛ والفاصل خيار (159).
fn flow_field(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.حقل";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات أو 3 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(2), line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            format!("الدالة {} رقم الحقل {} غير صالح (الصالح 0 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    if n == 0 {
        return Ok(Value::Str(text));
    }
    let fields = b3_split_fields(&text, sep.as_deref());
    Ok(Value::Str(
        fields.get((n - 1) as usize).cloned().unwrap_or_default(),
    ))
}

/// تدفق.عدد_الحقول(سطر، خيارات؟) — البند 156:
/// عدد الحقول في السطر (والفارغ ← 0).
fn flow_field_count(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.عدد_الحقول";
    if args.len() < 1 || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معاملا أو 2 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(1), line, col)?;
    Ok(Value::Number(b3_split_fields(&text, sep.as_deref()).len() as f64))
}

/// تدفق.اربط_حقول(حقول، خيارات؟) — البند 161:
/// اربط قائمة حقول بفاصل الإخراج (الافتراضي مسافة).
fn flow_join_fields(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.اربط_حقول";
    if args.len() < 1 || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معاملا أو 2 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let items = need_list(DISPLAY, &args[0], line, col)?;
    let mut out_sep = " ".to_string();
    if let Some(v) = args.get(1) {
        let pairs = need_dict(DISPLAY, v, line, col)?;
        for (k, val) in pairs {
            match k.as_str() {
                "فاصل_الإخراج" => {
                    out_sep = need_string(DISPLAY, val, line, col)?;
                }
                other => {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", DISPLAY, other),
                        line,
                        col,
                    ));
                }
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for it in items {
        match it {
            Value::Str(s) => parts.push(s.clone()),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع قائمة نصوص لكن أعطيت {}",
                        DISPLAY,
                        other.type_name()
                    ),
                    line,
                    col,
                ));
            }
        }
    }
    Ok(Value::Str(parts.join(&out_sep)))
}

/// تدفق.موضع_المطابقة(نمط، نص) — البند 169:
/// قاموس {موضع، طول} لأول مطابقة (محارف 1-based)
/// أو لاشيء عند الغياب.
fn flow_match_pos(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.موضع_المطابقة";
    need_args(DISPLAY, args, 2, line, col)?;
    let pattern = need_string(DISPLAY, &args[0], line, col)?;
    let text = need_string(DISPLAY, &args[1], line, col)?;
    match rx_find_first(&pattern, &text).map_err(|e| rx_err(e, line, col))? {
        Some((s, e, _, _)) => Ok(Value::Dict(vec![
            ("موضع".to_string(), Value::Number((s + 1) as f64)),
            ("طول".to_string(), Value::Number((e - s) as f64)),
        ])),
        None => Ok(Value::Nil),
    }
}

/// تدفق.فهرس_نص(نص، نص_فرعي) — البند 170: موضع
/// أول ظهور (محارف 1-based كما awk) أو 0 عند
/// الغياب؛ والفرعي الفارغ ← 1.
fn flow_text_index(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.فهرس_نص";
    need_args(DISPLAY, args, 2, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let sub = need_string(DISPLAY, &args[1], line, col)?;
    if sub.is_empty() {
        return Ok(Value::Number(1.0));
    }
    let h: Vec<char> = text.chars().collect();
    let n: Vec<char> = sub.chars().collect();
    let pos = h.windows(n.len()).position(|w| w == n.as_slice());
    Ok(Value::Number(pos.map(|p| p as f64 + 1.0).unwrap_or(0.0)))
}

/// تدفق.اجمع_عمود(نص، رقم_عمود، خيارات؟) — البند 175:
/// تجميع عددي على عمود (والحقل غير العددي ← 0 كما
/// awk؛ وبلا أسطر ← لاشيء إلا الجمع ← 0).
fn flow_col_sum(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.اجمع_عمود";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات أو 3 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(2), line, col)?;
    if n < 1 {
        return Err(RioError::runtime(
            format!("الدالة {} رقم العمود {} غير صالح (الصالح 1 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    let lines = flow_lines(&text);
    let mut sum = 0.0;
    for l in lines {
        let fields = b3_split_fields(l, sep.as_deref());
        let f = fields.get((n - 1) as usize).map(|s| s.as_str()).unwrap_or("");
        sum += b3_field_number(f);
    }
    Ok(Value::Number(sum))
}

/// تدفق.عد_المطابق(نص، نمط) — البند 176: عد
/// الأسطر المطابقة لنمط بحث.
fn flow_count_match(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.عد_المطابق";
    need_args(DISPLAY, args, 2, line, col)?;
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let pattern = need_string(DISPLAY, &args[1], line, col)?;
    let prog = rx_compile(&pattern, false).map_err(|e| rx_err(e, line, col))?;
    let mut count = 0i64;
    for l in flow_lines(&text) {
        if flow_line_matches(&prog, l, line, col)? {
            count += 1;
        }
    }
    Ok(Value::Number(count as f64))
}

/// تدفق.أكبر_عمود(نص، رقم_عمود، خيارات؟) — البند 177:
/// تجميع عددي على عمود (والحقل غير العددي ← 0 كما
/// awk؛ وبلا أسطر ← لاشيء إلا الجمع ← 0).
fn flow_col_max(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.أكبر_عمود";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات أو 3 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(2), line, col)?;
    if n < 1 {
        return Err(RioError::runtime(
            format!("الدالة {} رقم العمود {} غير صالح (الصالح 1 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    let lines = flow_lines(&text);
    let mut best: Option<f64> = None;
    for l in lines {
        let fields = b3_split_fields(l, sep.as_deref());
        let f = fields.get((n - 1) as usize).map(|s| s.as_str()).unwrap_or("");
        let num = b3_field_number(f);
        best = Some(best.map_or(num, |b| b.max(num)));
    }
    Ok(best.map(Value::Number).unwrap_or(Value::Nil))
}

/// تدفق.أصغر_عمود(نص، رقم_عمود، خيارات؟) — البند 178:
/// تجميع عددي على عمود (والحقل غير العددي ← 0 كما
/// awk؛ وبلا أسطر ← لاشيء إلا الجمع ← 0).
fn flow_col_min(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.أصغر_عمود";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات أو 3 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(2), line, col)?;
    if n < 1 {
        return Err(RioError::runtime(
            format!("الدالة {} رقم العمود {} غير صالح (الصالح 1 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    let lines = flow_lines(&text);
    let mut best: Option<f64> = None;
    for l in lines {
        let fields = b3_split_fields(l, sep.as_deref());
        let f = fields.get((n - 1) as usize).map(|s| s.as_str()).unwrap_or("");
        let num = b3_field_number(f);
        best = Some(best.map_or(num, |b| b.min(num)));
    }
    Ok(best.map(Value::Number).unwrap_or(Value::Nil))
}

/// تدفق.متوسط_عمود(نص، رقم_عمود، خيارات؟) — البند 179:
/// تجميع عددي على عمود (والحقل غير العددي ← 0 كما
/// awk؛ وبلا أسطر ← لاشيء إلا الجمع ← 0).
fn flow_col_avg(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.متوسط_عمود";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات أو 3 لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let n = need_integer(DISPLAY, &args[1], line, col)?;
    let sep = b3_flow_sep_opt(DISPLAY, args.get(2), line, col)?;
    if n < 1 {
        return Err(RioError::runtime(
            format!("الدالة {} رقم العمود {} غير صالح (الصالح 1 أو أكثر)", DISPLAY, n),
            line,
            col,
        ));
    }
    let lines = flow_lines(&text);
    let mut sum = 0.0;
    let mut count = 0i64;
    for l in lines {
        let fields = b3_split_fields(l, sep.as_deref());
        let f = fields.get((n - 1) as usize).map(|s| s.as_str()).unwrap_or("");
        sum += b3_field_number(f);
        count += 1;
    }
    if count == 0 {
        Ok(Value::Nil)
    } else {
        Ok(Value::Number(sum / count as f64))
    }
}
// END لينكس3-نص دوال تدفق Rust
fn flow_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("عدل_في_مكانه", native("عدل_في_مكانه", flow_edit_in_place)),
        ("استبدل_الكل", native("استبدل_الكل", flow_replace_all)),
        ("احذف_أسطر", native("احذف_أسطر", flow_delete_lines)),
        ("اطبع_المطابق", native("اطبع_المطابق", flow_print_matching)),
        ("توقف_عند", native("توقف_عند", flow_take_lines)),
        ("ألحق_بعد", native("ألحق_بعد", flow_append_after)),
        ("أدرج_قبل", native("أدرج_قبل", flow_insert_before)),
        ("غير_أسطر", native("غير_أسطر", flow_change_lines)),
        ("نفذ_أوامر", native("نفذ_أوامر", flow_run_commands)),
        ("نفذ_من_ملف", native("نفذ_من_ملف", flow_run_from_file)),// BEGIN لينكس3-نص مداخل تدفق
        ("عالج", native("عالج", flow_process)),
        ("حقل", native("حقل", flow_field)),
        ("عدد_الحقول", native("عدد_الحقول", flow_field_count)),
        ("اربط_حقول", native("اربط_حقول", flow_join_fields)),
        ("موضع_المطابقة", native("موضع_المطابقة", flow_match_pos)),
        ("فهرس_نص", native("فهرس_نص", flow_text_index)),
        ("اجمع_عمود", native("اجمع_عمود", flow_col_sum)),
        ("عد_المطابق", native("عد_المطابق", flow_count_match)),
        ("أكبر_عمود", native("أكبر_عمود", flow_col_max)),
        ("أصغر_عمود", native("أصغر_عمود", flow_col_min)),
        ("متوسط_عمود", native("متوسط_عمود", flow_col_avg)),
// END لينكس3-نص مداخل تدفق

    ]
}

/// تدفق.عدل_في_مكانه(مسار، محول، خيارات؟) — البند 145:
/// اقرأ الملف نصا، وطبّق عليه دالة التحويل `محول`
/// (دالة نص→نص)، واكتب الناتج في الملف نفسه؛ ويرجع
/// لاشيء (كاكتب_ملف). والمسار الغائب أو غير المقروء
/// أو غير UTF-8 ← خطأ عربي؛ والكتابة الفاشلة ← خطأ
/// عربي. والتحويل قبل أي كتابة — فشل المحول (خطأ
/// أو إرجاع غير نصي) يبقي الملف الأصلي كما هو، بلا
/// كتابة جزئية. والكتابة ذرية (كما اكتب_ملف).
/// والخيار نسخة_احتياطية (البند 146): نص بامتداد
/// (مثل «.bak» — نسخة بالمسار + الامتداد قبل
/// الكتابة)، أو كاذب بلا نسخة (الافتراضي)، أو صحيح
/// بامتداد افتراضي «.bak»؛ وغير النص/المنطقي ← خطأ
/// عربي؛ والامتداد الفارغ أو الحامل فاصلا ← خطأ عربي.
fn flow_edit_in_place(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.عدل_في_مكانه";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = need_string(DISPLAY, &args[0], line, col)?;
    let func = match &args[1] {
        Value::Function { .. } | Value::BoundMethod { .. } | Value::NativeFunction { .. } => {
            &args[1]
        }
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع دالة محولة لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let mut backup: Option<String> = None;
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        for (k, v) in pairs {
            match k.as_str() {
                "نسخة_احتياطية" => match v {
                    Value::Bool(false) => backup = None,
                    Value::Bool(true) => backup = Some(".bak".to_string()),
                    Value::Str(s) => {
                        if s.is_empty() || s.contains('/') || s.contains('\\') {
                            return Err(RioError::runtime(
                                "الدالة تدفق.عدل_في_مكانه الخيار نسخة_احتياطية يتوقع امتدادا غير فارغ بلا فاصل مسار".to_string(),
                                line,
                                col,
                            ));
                        }
                        backup = Some(s.clone());
                    }
                    other => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} الخيار نسخة_احتياطية يتوقع نصا أو قيمة منطقية لكن أعطيت {}",
                                DISPLAY,
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                },
                other => {
                    return Err(RioError::runtime(
                        format!("الدالة {} خيار غير معروف: {}", DISPLAY, other),
                        line,
                        col,
                    ))
                }
            }
        }
    }
    // المسار النسبي يُحل مقابل مجلد الجلسة (كما في
    // بحث.أنماط_من_ملف) لا مقابل مجلد العملية.
    let full = search_resolve_path(ctx, &path);
    let content = match std::fs::read(&full) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return Err(RioError::runtime(
                    format!("الملف {} ليس نصا صالحا (UTF-8)", path),
                    line,
                    col,
                ))
            }
        },
        Err(_) => {
            return Err(RioError::runtime(
                format!("تعذر قراءة الملف {}", path),
                line,
                col,
            ))
        }
    };
    // التحويل قبل أي كتابة.
    let (res, produced) = ctx.call_handler(func, Value::Str(content), line, col);
    if !produced.is_empty() {
        print!("{}", produced);
    }
    let new_text = match res? {
        Value::Str(s) => s,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع أن يرجع المحول نصا لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    // النسخة الاحتياطية قبل الكتابة (المسار + الامتداد).
    if let Some(ext) = &backup {
        let mut backup_path = full.clone().into_os_string();
        backup_path.push(ext);
        let backup_path = std::path::PathBuf::from(backup_path);
        if std::fs::copy(&full, &backup_path).is_err() {
            return Err(RioError::runtime(
                format!("تعذر كتابة النسخة الاحتياطية {}{}", path, ext),
                line,
                col,
            ));
        }
    }
    // كتابة ذرية — فلا ملف نصف مكتوب.
    crate::install::write_atomic(&full, new_text.as_bytes()).map_err(|_| {
        RioError::runtime(format!("تعذر كتابة الملف {}", path), line, col)
    })?;
    Ok(Value::Nil)
}

// ---------- وحدة تدفق: نوى أوامر يدوية (دفعة لينكس 2، المرحلة 2) ----------
// قسم يدوي معلَّم: نوى مشتركة بين دوال تدفق العامة
// (استبدل_الكل واحذف_أسطر واطبع_المطابق وتوقف_عند
// وألحق_بعد وأدرج_قبل وغير_أسطر — يولّدها مولّد
// الدفعة فوقها) وبين مشغّل الأوامر (نفذ_أوامر
// ونفذ_من_ملف — البندان 149 و150)؛ فالمشغّل يطبّق
// الأوامر نفسها بالنوى نفسها، بلا منطق مكرر.
// قاعدة إعادة الربط العامة: النص المنتهي بـ`\n`
// يعيد نصا منتهيا بـ`\n`، والنص الفارغ يعيد فارغا.

/// خيارات تدفق: عنوان (لاستبدل_الكل) وطيّ حالة
/// وصامت (للمشغّل). والمحلل يقبل مفاتيح محددة
/// فقط — فالمفتاح الزائد ← خطأ عربي يسمّيه.
struct FlowOpts {
    address: Option<Value>,
    ignore_case: bool,
    quiet: bool,
}

fn flow_parse_opts(
    display: &str,
    pairs: &[(String, Value)],
    allow_address: bool,
    allow_quiet: bool,
    line: usize,
    col: usize,
) -> RioResult<FlowOpts> {
    let mut out = FlowOpts {
        address: None,
        ignore_case: false,
        quiet: false,
    };
    for (k, v) in pairs {
        match k.as_str() {
            "عنوان" => {
                if !allow_address {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                out.address = Some(v.clone());
            }
            "تجاهل_الحالة" => {
                if !allow_address {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                out.ignore_case = search_need_bool(display, k, v, line, col)?;
            }
            "صامت" => {
                if !allow_quiet {
                    return Err(RioError::runtime(
                        format!("الدالة {} لا تقبل الخيار {}", display, k),
                        line,
                        col,
                    ));
                }
                out.quiet = search_need_bool(display, k, v, line, col)?;
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
    Ok(out)
}

/// قسّم نصا أسطر مملوكة + هل انتهى بـ`\n`؟
fn flow_split_owned(text: &str) -> (Vec<String>, bool) {
    let trailing = text.ends_with('\n');
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    (lines, trailing)
}

/// أعد ربط الأسطر بالقاعدة العامة.
fn flow_rejoin(lines: &[String], trailing: bool) -> String {
    let mut out = lines.join("\n");
    if trailing && !lines.is_empty() {
        out.push('\n');
    }
    out
}

/// استبدال نمط في نص واحد يرجع نصا (لا قيمة).
fn flow_regex_replace_str(
    pattern: &str,
    text: &str,
    repl: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    match regex_replace_fold(pattern, text, repl, fold, line, col)? {
        Value::Str(s) => Ok(s),
        // لا يحدث: الاستبدال يرجع نصا دائما.
        _ => Ok(String::new()),
    }
}

/// حلّ عنوان إلى مجموعة أسطر (1-based)؛ ولاشيء
/// (أو غائب) = كل الأسطر.
fn flow_resolve_set(
    display: &str,
    address: Option<&Value>,
    lines: &[String],
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<std::collections::HashSet<usize>> {
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let addr = match address {
        Some(a) => a,
        None => &Value::Nil,
    };
    let nums = flow_address_lines(display, addr, &refs, fold, line, col)?;
    Ok(nums.into_iter().collect())
}

/// نواة استبدل_الكل: بلا عنوان ≡ نمط.استبدل حرفيا
/// على النص كله؛ ومع عنوان يستبدل داخل الأسطر
/// المعنونة وحدها سطرا سطرا.
fn flow_replace_all_core(
    display: &str,
    text: &str,
    pattern: &str,
    repl: &str,
    address: Option<&Value>,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    match address {
        None => flow_regex_replace_str(pattern, text, repl, fold, line, col),
        Some(addr) => {
            let (lines, trailing) = flow_split_owned(text);
            let set = flow_resolve_set(display, Some(addr), &lines, fold, line, col)?;
            let mut out: Vec<String> = Vec::with_capacity(lines.len());
            for (idx, raw) in lines.iter().enumerate() {
                if set.contains(&(idx + 1)) {
                    out.push(flow_regex_replace_str(pattern, raw, repl, fold, line, col)?);
                } else {
                    out.push(raw.clone());
                }
            }
            Ok(flow_rejoin(&out, trailing))
        }
    }
}

/// نواة احذف_أسطر: احذف الأسطر المعنونة؛ والغائب
/// (لاشيء) = كل الأسطر.
fn flow_delete_lines_core(
    display: &str,
    text: &str,
    address: Option<&Value>,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let (lines, trailing) = flow_split_owned(text);
    let set = flow_resolve_set(display, address, &lines, fold, line, col)?;
    let kept: Vec<String> = lines
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !set.contains(&(i + 1)))
        .map(|(_, l)| l)
        .collect();
    Ok(flow_rejoin(&kept, trailing))
}

/// نواة اطبع_المطابق: الأسطر المعنونة نصوصا.
fn flow_matching_lines_core(
    display: &str,
    text: &str,
    address: Option<&Value>,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<Vec<String>> {
    let (lines, _trailing) = flow_split_owned(text);
    let set = flow_resolve_set(display, address, &lines, fold, line, col)?;
    Ok(lines
        .into_iter()
        .enumerate()
        .filter(|(i, _)| set.contains(&(i + 1)))
        .map(|(_, l)| l)
        .collect())
}

/// نواة توقف_عند: الأسطر 1..N شاملة؛ وN أكبر من
/// المجموع ← النص كله؛ و0 أو سالب (عدا -1) ← خطأ
/// عربي؛ و-1 ← آخر سطر (أي النص كله).
fn flow_take_lines_core(
    display: &str,
    text: &str,
    n: i64,
    line: usize,
    col: usize,
) -> RioResult<String> {
    if n == -1 {
        return Ok(text.to_string());
    }
    if n <= 0 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} عدد الأسطر {} غير صالح (الصالح 1 أو أكثر، و-1 لآخر سطر)",
                display, n
            ),
            line,
            col,
        ));
    }
    let (lines, _trailing) = flow_split_owned(text);
    let total = lines.len() as i64;
    if n >= total {
        return Ok(text.to_string());
    }
    let mut out = lines[..n as usize].join("\n");
    out.push('\n');
    Ok(out)
}

/// نواة ألحق_بعد: أدخل أسطر النص الملحق بعد كل
/// سطر معنون؛ والملحق الفارغ بلا أثر.
fn flow_append_after_core(
    display: &str,
    text: &str,
    address: Option<&Value>,
    appended: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let (lines, trailing) = flow_split_owned(text);
    let set = flow_resolve_set(display, address, &lines, fold, line, col)?;
    let extra: Vec<String> = appended.lines().map(|l| l.to_string()).collect();
    let mut out: Vec<String> = Vec::new();
    for (idx, raw) in lines.iter().enumerate() {
        out.push(raw.clone());
        if set.contains(&(idx + 1)) {
            out.extend(extra.iter().cloned());
        }
    }
    Ok(flow_rejoin(&out, trailing))
}

/// نواة أدرج_قبل: أدخل أسطر النص المدرج قبل كل
/// سطر معنون؛ والمدرج الفارغ بلا أثر.
fn flow_insert_before_core(
    display: &str,
    text: &str,
    address: Option<&Value>,
    inserted: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let (lines, trailing) = flow_split_owned(text);
    let set = flow_resolve_set(display, address, &lines, fold, line, col)?;
    let extra: Vec<String> = inserted.lines().map(|l| l.to_string()).collect();
    let mut out: Vec<String> = Vec::new();
    for (idx, raw) in lines.iter().enumerate() {
        if set.contains(&(idx + 1)) {
            out.extend(extra.iter().cloned());
        }
        out.push(raw.clone());
    }
    Ok(flow_rejoin(&out, trailing))
}

/// نواة غير_أسطر: استبدال كل امتداد متصل من الأسطر
/// المعنونة بالنص الجديد مرة واحدة (sed المخلصة)،
/// لا تكرار النص الجديد على كل سطر.
fn flow_change_lines_core(
    display: &str,
    text: &str,
    address: Option<&Value>,
    new_text: &str,
    fold: bool,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let (lines, trailing) = flow_split_owned(text);
    let set = flow_resolve_set(display, address, &lines, fold, line, col)?;
    let replacement: Vec<String> = new_text.lines().map(|l| l.to_string()).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if set.contains(&(i + 1)) {
            out.extend(replacement.iter().cloned());
            while i < lines.len() && set.contains(&(i + 1)) {
                i += 1;
            }
        } else {
            out.push(lines[i].clone());
            i += 1;
        }
    }
    Ok(flow_rejoin(&out, trailing))
}

/// أمر واحد من قائمة أوامر: يتحقق من مفاتيحه
/// وينفذه على النص الحالي؛ ويرجع (نص جديد، أوقف
/// كل ما بعده؟). و`اطبع_المطابق` يُلحق أسطره
/// بقائمة مطبوع (إلا مع صامت).
fn flow_run_one_command(
    display: &str,
    text: &str,
    cmd: &Value,
    printed: &mut Vec<Value>,
    quiet: bool,
    line: usize,
    col: usize,
) -> RioResult<(String, bool)> {
    let pairs = match cmd {
        Value::Dict(p) => p,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قاموسا لكل أمر لكن أعطيت {}",
                    display,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let name = match dict_get(pairs, "أمر") {
        Some(Value::Str(s)) => s.clone(),
        Some(other) => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع مفتاح «أمر» نصا لكن أعطيت {}",
                    display,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
        None => {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع مفتاح «أمر» في كل أمر", display),
                line,
                col,
            ))
        }
    };
    let allowed: &[&str] = match name.as_str() {
        "استبدل_الكل" => &["أمر", "نمط", "بديل", "عنوان"],
        "احذف_أسطر" => &["أمر", "عنوان"],
        "اطبع_المطابق" => &["أمر", "عنوان"],
        "توقف_عند" => &["أمر", "عدد"],
        "ألحق_بعد" => &["أمر", "نص", "عنوان"],
        "أدرج_قبل" => &["أمر", "نص", "عنوان"],
        "غير_أسطر" => &["أمر", "نص", "عنوان"],
        _ => {
            return Err(RioError::runtime(
                format!("الدالة {} أمر غير معروف: {}", display, name),
                line,
                col,
            ))
        }
    };
    for (k, _) in pairs {
        if !allowed.iter().any(|a| *a == k.as_str()) {
            return Err(RioError::runtime(
                format!("الدالة {} الأمر {} لا يقبل المفتاح {}", display, name, k),
                line,
                col,
            ));
        }
    }
    let need = |key: &str| -> RioResult<&Value> {
        dict_get(pairs, key).ok_or_else(|| {
            RioError::runtime(
                format!("الدالة {} الأمر {} ينقصه المفتاح {}", display, name, key),
                line,
                col,
            )
        })
    };
    let address = dict_get(pairs, "عنوان");
    match name.as_str() {
        "استبدل_الكل" => {
            let pattern = need_string(display, need("نمط")?, line, col)?;
            let repl = need_string(display, need("بديل")?, line, col)?;
            Ok((
                flow_replace_all_core(display, text, &pattern, &repl, address, false, line, col)?,
                false,
            ))
        }
        "احذف_أسطر" => Ok((
            flow_delete_lines_core(display, text, address, false, line, col)?,
            false,
        )),
        "اطبع_المطابق" => {
            let found = flow_matching_lines_core(display, text, address, false, line, col)?;
            if !quiet {
                for l in found {
                    printed.push(Value::Str(l));
                }
            }
            Ok((text.to_string(), false))
        }
        "توقف_عند" => {
            let n = need_integer(display, need("عدد")?, line, col)?;
            Ok((
                flow_take_lines_core(display, text, n, line, col)?,
                true,
            ))
        }
        "ألحق_بعد" => {
            let appended = need_string(display, need("نص")?, line, col)?;
            Ok((
                flow_append_after_core(display, text, address, &appended, false, line, col)?,
                false,
            ))
        }
        "أدرج_قبل" => {
            let inserted = need_string(display, need("نص")?, line, col)?;
            Ok((
                flow_insert_before_core(display, text, address, &inserted, false, line, col)?,
                false,
            ))
        }
        "غير_أسطر" => {
            let new_text = need_string(display, need("نص")?, line, col)?;
            Ok((
                flow_change_lines_core(display, text, address, &new_text, false, line, col)?,
                false,
            ))
        }
        // لا يحدث: الاسم المجهول رُفض أعلاه.
        _ => unreachable!(),
    }
}

/// نواة المشغّل: طبّق قائمة أوامر تسلسليا على نص؛
/// و`اطبع_المطابق` يُلحق أسطره بقائمة مطبوع؛
/// و`توقف_عند` يقطع النص ويوقف كل ما بعده (دلالة
/// sed `q`)؛ والإرجاع سجل واحد {نص، مطبوع} — بلا
/// طباعة جانبية؛ ومع صامت تبقى مطبوع فارغة دائما.
fn flow_run_commands_core(
    display: &str,
    text: &str,
    commands: &[Value],
    quiet: bool,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let mut cur = text.to_string();
    let mut printed: Vec<Value> = Vec::new();
    for cmd in commands {
        let (next, stop) =
            flow_run_one_command(display, &cur, cmd, &mut printed, quiet, line, col)?;
        cur = next;
        if stop {
            break;
        }
    }
    Ok(Value::Dict(vec![
        ("نص".to_string(), Value::Str(cur)),
        ("مطبوع".to_string(), Value::List(printed)),
    ]))
}

/// عدد صحيح من نص: لاتيني أو عربي مشرقي، مع
/// إشارة سالبة اختيارية؛ والفائض أو التالف ← لا شيء.
fn flow_parse_int_text(s: &str) -> Option<i64> {
    let mut out: i64 = 0;
    let mut neg = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '-' if !any && !neg => neg = true,
            '0'..='9' => {
                any = true;
                out = out.checked_mul(10)?.checked_add(c as i64 - '0' as i64)?;
            }
            '٠'..='٩' => {
                any = true;
                out = out.checked_mul(10)?.checked_add(c as i64 - '٠' as i64)?;
            }
            _ => return None,
        }
    }
    if !any {
        return None;
    }
    Some(if neg { -out } else { out })
}

/// قسّم سطر أوامر على `|` مع هروب `\|` و`\\`.
fn flow_split_command_fields(line: &str) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('|') => {
                    cur.push('|');
                    chars.next();
                }
                Some('\\') => {
                    cur.push('\\');
                    chars.next();
                }
                _ => cur.push('\\'),
            },
            '|' => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// حدّ عنوان صالح في صيغة الملف: -1 أو موجب.
fn flow_address_bound_ok(n: i64) -> bool {
    n == -1 || n > 0
}

/// حلّ حقل عنوان نصي (صيغة ملف الأوامر) إلى قيمة
/// عنوان؛ والفارغ = كل الأسطر (لاشيء). الأشكال:
/// عدد، أو `آخر` لـ-1، أو `أ-ب` مدى، أو `/نمط/`،
/// أو `/نمط1/:/نمط2/`. والتالف ← خطأ عربي يسمّي
/// رقم السطر في الملف.
fn flow_parse_address_text(
    field: &str,
    file_line: usize,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let bad = || {
        RioError::runtime(
            format!(
                "السطر {} في الملف: عنوان غير صالح «{}»",
                file_line, field
            ),
            line,
            col,
        )
    };
    if field.is_empty() {
        return Ok(Value::Nil);
    }
    if field == "آخر" {
        return Ok(Value::Number(-1.0));
    }
    if let Some(n) = flow_parse_int_text(field) {
        if flow_address_bound_ok(n) {
            return Ok(Value::Number(n as f64));
        }
        return Err(bad());
    }
    // `/نمط1/:/نمط2/` قبل `/نمط/` المنفرد، وقبلهما
    // قبل المدى — فالنمط قد يحوي `-`.
    if field.starts_with('/') && field.ends_with('/') && field.len() >= 2 {
        let inner = &field[1..field.len() - 1];
        if let Some((p1, p2)) = inner.split_once("/:/") {
            return Ok(Value::List(vec![
                Value::Str(p1.to_string()),
                Value::Str(p2.to_string()),
            ]));
        }
        return Ok(Value::Str(inner.to_string()));
    }
    // مدى `أ-ب` (عدد-عدد).
    if let Some((a, b)) = field.split_once('-') {
        if let (Some(x), Some(y)) = (flow_parse_int_text(a), flow_parse_int_text(b)) {
            if flow_address_bound_ok(x) && flow_address_bound_ok(y) {
                return Ok(Value::List(vec![
                    Value::Number(x as f64),
                    Value::Number(y as f64),
                ]));
            }
        }
        return Err(bad());
    }
    Err(bad())
}

/// حلّل محتوى ملف أوامر (صيغة الإصدار 1) إلى
/// قائمة قواميس الأوامر نفسها التي يقبلها
/// نفذ_أوامر — فـنفذ_من_ملف غلاف قراءة فوق نواة
/// نفذ_أوامر. والفارغ وسطور `#` تُتخطى؛ والسطر
/// السيئ ← خطأ عربي يسمّي رقم السطر في الملف.
fn flow_parse_command_file(
    content: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<Value>> {
    let mut commands: Vec<Value> = Vec::new();
    for (idx, raw) in content.lines().enumerate() {
        let no = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields = flow_split_command_fields(raw);
        let name = fields[0].as_str();
        let bad_fields = |want: &str| {
            RioError::runtime(
                format!(
                    "السطر {} في الملف: الأمر {} يتوقع {} لكن أعطيت {} حقول",
                    no,
                    name,
                    want,
                    fields.len()
                ),
                line,
                col,
            )
        };
        let mut dict: Vec<(String, Value)> =
            vec![("أمر".to_string(), Value::Str(name.to_string()))];
        match name {
            "استبدل_الكل" => {
                if fields.len() != 3 && fields.len() != 4 {
                    return Err(bad_fields("٣ أو ٤ حقول"));
                }
                dict.push(("نمط".to_string(), Value::Str(fields[1].clone())));
                dict.push(("بديل".to_string(), Value::Str(fields[2].clone())));
                if fields.len() == 4 {
                    let addr = flow_parse_address_text(&fields[3], no, line, col)?;
                    if !matches!(addr, Value::Nil) {
                        dict.push(("عنوان".to_string(), addr));
                    }
                }
            }
            "احذف_أسطر" | "اطبع_المطابق" => {
                if fields.len() != 1 && fields.len() != 2 {
                    return Err(bad_fields("حقلا أو حقلين"));
                }
                if fields.len() == 2 {
                    let addr = flow_parse_address_text(&fields[1], no, line, col)?;
                    if !matches!(addr, Value::Nil) {
                        dict.push(("عنوان".to_string(), addr));
                    }
                }
            }
            "توقف_عند" => {
                if fields.len() != 2 {
                    return Err(bad_fields("حقلين"));
                }
                let n = flow_parse_int_text(&fields[1]).ok_or_else(|| {
                    RioError::runtime(
                        format!("السطر {} في الملف: عدد غير صالح «{}»", no, fields[1]),
                        line,
                        col,
                    )
                })?;
                dict.push(("عدد".to_string(), Value::Number(n as f64)));
            }
            "ألحق_بعد" | "أدرج_قبل" | "غير_أسطر" => {
                if fields.len() != 2 && fields.len() != 3 {
                    return Err(bad_fields("حقلين أو ثلاثة حقول"));
                }
                dict.push(("نص".to_string(), Value::Str(fields[1].clone())));
                if fields.len() == 3 {
                    let addr = flow_parse_address_text(&fields[2], no, line, col)?;
                    if !matches!(addr, Value::Nil) {
                        dict.push(("عنوان".to_string(), addr));
                    }
                }
            }
            _ => {
                return Err(RioError::runtime(
                    format!("السطر {} في الملف: أمر غير معروف «{}»", no, name),
                    line,
                    col,
                ));
            }
        }
        commands.push(Value::Dict(dict));
    }
    Ok(commands)
}

/// تدفق.نفذ_أوامر(نص، أوامر، خيارات؟) — البند 150:
/// طبّق قائمة قواميس أوامر تسلسليا على نص وارجع
/// سجلا واحدا {نص، مطبوع}. والأوامر السبعة أولا:
/// استبدل_الكل واحذف_أسطر واطبع_المطابق وتوقف_عند
/// وألحق_بعد وأدرج_قبل وغير_أسطر. والخيار صامت
/// (البند 126): صحيحا تبقى مطبوع فارغة دائما.
fn flow_run_commands(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.نفذ_أوامر";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let commands = match &args[1] {
        Value::List(l) => l.clone(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قائمة أوامر لكن أعطيت {}",
                    DISPLAY,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let mut quiet = false;
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        let opts = flow_parse_opts(DISPLAY, pairs, false, true, line, col)?;
        quiet = opts.quiet;
    }
    flow_run_commands_core(DISPLAY, &text, &commands, quiet, line, col)
}

/// تدفق.نفذ_من_ملف(نص، مسار، خيارات؟) — البند 149:
/// غلاف قراءة فوق نواة نفذ_أوامر: اقرأ ملف الأوامر
/// (المسار النسبي يُحل مقابل مجلد الجلسة)، وحوّل
/// أسطره إلى قواميس الأوامر نفسها، وطبّقها بالنواة
/// نفسها. والغائب/غير المقروء/غير UTF-8 ← «تعذر
/// قراءة الملف X» كـأنماط_من_ملف.
fn flow_run_from_file(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const DISPLAY: &str = "تدفق.نفذ_من_ملف";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع معاملين أو ثلاثة لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = need_string(DISPLAY, &args[0], line, col)?;
    let path = need_string(DISPLAY, &args[1], line, col)?;
    let mut quiet = false;
    if args.len() == 3 {
        let pairs = need_dict(DISPLAY, &args[2], line, col)?;
        let opts = flow_parse_opts(DISPLAY, pairs, false, true, line, col)?;
        quiet = opts.quiet;
    }
    let full = search_resolve_path(ctx, &path);
    let content = std::fs::read_to_string(&full).map_err(|_| {
        RioError::runtime(format!("تعذر قراءة الملف {}", path), line, col)
    })?;
    let commands = flow_parse_command_file(&content, line, col)?;
    flow_run_commands_core(DISPLAY, &text, &commands, quiet, line, col)
}

#[cfg(test)]
mod flow_address_tests {
    use super::*;

    const DISPLAY: &str = "تدفق.عنوان";

    fn num(n: i64) -> Value {
        Value::Number(n as f64)
    }

    fn lines_of(text: &str) -> Vec<&str> {
        flow_lines(text)
    }

    fn resolve(address: &Value, text: &str) -> RioResult<Vec<usize>> {
        let lines = lines_of(text);
        flow_address_lines(DISPLAY, address, &lines, false, 0, 0)
    }

    fn err_msg(address: &Value, text: &str) -> String {
        resolve(address, text).unwrap_err().message
    }

    #[test]
    fn flow_address_number_forms() {
        let text = "ألف\nباء\nجيم\n";
        assert_eq!(resolve(&num(1), text).unwrap(), vec![1]);
        assert_eq!(resolve(&num(3), text).unwrap(), vec![3]);
        assert_eq!(resolve(&num(-1), text).unwrap(), vec![3]);
        assert_eq!(resolve(&num(9), text).unwrap(), Vec::<usize>::new());
        assert!(err_msg(&num(0), text).contains("غير صالح"));
        assert!(err_msg(&num(-2), text).contains("غير صالح"));
        assert!(resolve(&Value::Number(1.5), text).is_err());
        assert!(resolve(&Value::Bool(true), text).is_err());
    }

    #[test]
    fn flow_address_range_forms() {
        let text = "ألف\nباء\nجيم\n";
        let range = |a: i64, b: i64| Value::List(vec![num(a), num(b)]);
        assert_eq!(resolve(&range(1, 3), text).unwrap(), vec![1, 2, 3]);
        assert_eq!(resolve(&range(2, 2), text).unwrap(), vec![2]);
        assert_eq!(resolve(&range(3, 1), text).unwrap(), Vec::<usize>::new());
        assert_eq!(resolve(&range(2, 99), text).unwrap(), vec![2, 3]);
        assert_eq!(resolve(&range(1, -1), text).unwrap(), vec![1, 2, 3]);
        assert_eq!(resolve(&range(-1, -1), text).unwrap(), vec![3]);
        assert_eq!(resolve(&range(9, 10), text).unwrap(), Vec::<usize>::new());
        assert!(err_msg(&range(0, 2), text).contains("غير صالح"));
        assert!(err_msg(&range(-2, 2), text).contains("غير صالح"));
        assert!(resolve(&Value::List(vec![num(1)]), text).is_err());
        assert!(resolve(&Value::List(vec![num(1), num(2), num(3)]), text).is_err());
        assert!(resolve(
            &Value::List(vec![num(1), Value::Str("جيم".to_string())]),
            text
        )
        .is_err());
    }

    #[test]
    fn flow_address_pattern_forms() {
        let text = "ألف\nباء\nجيم\n";
        let pat = |p: &str| Value::Str(p.to_string());
        assert_eq!(resolve(&pat("باء"), text).unwrap(), vec![2]);
        assert_eq!(resolve(&pat("ألف|جيم"), text).unwrap(), vec![1, 3]);
        assert_eq!(resolve(&pat("غائب"), text).unwrap(), Vec::<usize>::new());
        assert_eq!(resolve(&pat(""), text).unwrap(), vec![1, 2, 3]);
        assert!(err_msg(&pat("["), text).contains("نمط غير صالح"));
    }

    #[test]
    fn flow_address_pattern_range_forms() {
        let text = "ألف\nباء\nباء\nجيم\n";
        let prange = |a: &str, b: &str| {
            Value::List(vec![Value::Str(a.to_string()), Value::Str(b.to_string())])
        };
        assert_eq!(
            resolve(&prange("ألف", "جيم"), text).unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(resolve(&prange("باء", "غائب"), text).unwrap(), vec![2, 3, 4]);
        assert_eq!(
            resolve(&prange("غائب", "جيم"), text).unwrap(),
            Vec::<usize>::new()
        );
        // نمط2 بعد سطر نمط1 حتما: «باء» الثانية تنهي المدى لا الأولى.
        assert_eq!(resolve(&prange("باء", "باء"), text).unwrap(), vec![2, 3]);
        assert!(err_msg(&prange("ألف", "["), text).contains("نمط غير صالح"));
    }

    #[test]
    fn flow_address_nil_and_empty_text() {
        let text = "ألف\nباء\nجيم\n";
        assert_eq!(resolve(&Value::Nil, text).unwrap(), vec![1, 2, 3]);
        assert_eq!(resolve(&Value::Nil, "").unwrap(), Vec::<usize>::new());
        assert_eq!(resolve(&num(1), "").unwrap(), Vec::<usize>::new());
        assert_eq!(resolve(&num(-1), "").unwrap(), Vec::<usize>::new());
        assert_eq!(
            flow_address_lines_in_text(DISPLAY, &num(2), "ألف\nباء\n", false, 0, 0).unwrap(),
            vec![2]
        );
    }
}

// ---------- J11: البيانات والجداول (وحدة جداول جديدة) ----------
// البنود 168 و170 و171 و173 و174. الجدول قائمة صفوف،
// والصف قاموس (عمود ← قيمة) يحفظ ترتيب إدخال مفاتيحه.
// 172 (SQLite وODBC) مؤجل: يحتاج اعتمادا أصليا جديدا
// (مكتبة SQLite) وقرار نموذج، فلا واجهة وهمية هنا.
// التسجيلات في المنطقة المعلمة أدناه مولدة من
// ~/workspace/j11gen/spec.json — لا تعدلها يدويا.



// BEGIN J12-GEN rust-trig
/// J12: جدول جيب صحيح (مولّد): sin(2πi/1024) * 32767،
/// i16 في [-32767, 32767] — نفس القيم حرفيا في مرآة C،
/// فكل هندسة المخططات والموجات صحيحة بلا عائم.
static J12_SIN_TABLE: [i16; 1024] = [
    0, 201, 402, 603, 804, 1005, 1206, 1407, 1608, 1809, 2009, 2210, 2410, 2611, 2811, 3012,
    3212, 3412, 3612, 3811, 4011, 4210, 4410, 4609, 4808, 5007, 5205, 5404, 5602, 5800, 5998, 6195,
    6393, 6590, 6786, 6983, 7179, 7375, 7571, 7767, 7962, 8157, 8351, 8545, 8739, 8933, 9126, 9319,
    9512, 9704, 9896, 10087, 10278, 10469, 10659, 10849, 11039, 11228, 11417, 11605, 11793, 11980, 12167, 12353,
    12539, 12725, 12910, 13094, 13279, 13462, 13645, 13828, 14010, 14191, 14372, 14553, 14732, 14912, 15090, 15269,
    15446, 15623, 15800, 15976, 16151, 16325, 16499, 16673, 16846, 17018, 17189, 17360, 17530, 17700, 17869, 18037,
    18204, 18371, 18537, 18703, 18868, 19032, 19195, 19357, 19519, 19680, 19841, 20000, 20159, 20317, 20475, 20631,
    20787, 20942, 21096, 21250, 21403, 21554, 21705, 21856, 22005, 22154, 22301, 22448, 22594, 22739, 22884, 23027,
    23170, 23311, 23452, 23592, 23731, 23870, 24007, 24143, 24279, 24413, 24547, 24680, 24811, 24942, 25072, 25201,
    25329, 25456, 25582, 25708, 25832, 25955, 26077, 26198, 26319, 26438, 26556, 26674, 26790, 26905, 27019, 27133,
    27245, 27356, 27466, 27575, 27683, 27790, 27896, 28001, 28105, 28208, 28310, 28411, 28510, 28609, 28706, 28803,
    28898, 28992, 29085, 29177, 29268, 29358, 29447, 29534, 29621, 29706, 29791, 29874, 29956, 30037, 30117, 30195,
    30273, 30349, 30424, 30498, 30571, 30643, 30714, 30783, 30852, 30919, 30985, 31050, 31113, 31176, 31237, 31297,
    31356, 31414, 31470, 31526, 31580, 31633, 31685, 31736, 31785, 31833, 31880, 31926, 31971, 32014, 32057, 32098,
    32137, 32176, 32213, 32250, 32285, 32318, 32351, 32382, 32412, 32441, 32469, 32495, 32521, 32545, 32567, 32589,
    32609, 32628, 32646, 32663, 32678, 32692, 32705, 32717, 32728, 32737, 32745, 32752, 32757, 32761, 32765, 32766,
    32767, 32766, 32765, 32761, 32757, 32752, 32745, 32737, 32728, 32717, 32705, 32692, 32678, 32663, 32646, 32628,
    32609, 32589, 32567, 32545, 32521, 32495, 32469, 32441, 32412, 32382, 32351, 32318, 32285, 32250, 32213, 32176,
    32137, 32098, 32057, 32014, 31971, 31926, 31880, 31833, 31785, 31736, 31685, 31633, 31580, 31526, 31470, 31414,
    31356, 31297, 31237, 31176, 31113, 31050, 30985, 30919, 30852, 30783, 30714, 30643, 30571, 30498, 30424, 30349,
    30273, 30195, 30117, 30037, 29956, 29874, 29791, 29706, 29621, 29534, 29447, 29358, 29268, 29177, 29085, 28992,
    28898, 28803, 28706, 28609, 28510, 28411, 28310, 28208, 28105, 28001, 27896, 27790, 27683, 27575, 27466, 27356,
    27245, 27133, 27019, 26905, 26790, 26674, 26556, 26438, 26319, 26198, 26077, 25955, 25832, 25708, 25582, 25456,
    25329, 25201, 25072, 24942, 24811, 24680, 24547, 24413, 24279, 24143, 24007, 23870, 23731, 23592, 23452, 23311,
    23170, 23027, 22884, 22739, 22594, 22448, 22301, 22154, 22005, 21856, 21705, 21554, 21403, 21250, 21096, 20942,
    20787, 20631, 20475, 20317, 20159, 20000, 19841, 19680, 19519, 19357, 19195, 19032, 18868, 18703, 18537, 18371,
    18204, 18037, 17869, 17700, 17530, 17360, 17189, 17018, 16846, 16673, 16499, 16325, 16151, 15976, 15800, 15623,
    15446, 15269, 15090, 14912, 14732, 14553, 14372, 14191, 14010, 13828, 13645, 13462, 13279, 13094, 12910, 12725,
    12539, 12353, 12167, 11980, 11793, 11605, 11417, 11228, 11039, 10849, 10659, 10469, 10278, 10087, 9896, 9704,
    9512, 9319, 9126, 8933, 8739, 8545, 8351, 8157, 7962, 7767, 7571, 7375, 7179, 6983, 6786, 6590,
    6393, 6195, 5998, 5800, 5602, 5404, 5205, 5007, 4808, 4609, 4410, 4210, 4011, 3811, 3612, 3412,
    3212, 3012, 2811, 2611, 2410, 2210, 2009, 1809, 1608, 1407, 1206, 1005, 804, 603, 402, 201,
    0, -201, -402, -603, -804, -1005, -1206, -1407, -1608, -1809, -2009, -2210, -2410, -2611, -2811, -3012,
    -3212, -3412, -3612, -3811, -4011, -4210, -4410, -4609, -4808, -5007, -5205, -5404, -5602, -5800, -5998, -6195,
    -6393, -6590, -6786, -6983, -7179, -7375, -7571, -7767, -7962, -8157, -8351, -8545, -8739, -8933, -9126, -9319,
    -9512, -9704, -9896, -10087, -10278, -10469, -10659, -10849, -11039, -11228, -11417, -11605, -11793, -11980, -12167, -12353,
    -12539, -12725, -12910, -13094, -13279, -13462, -13645, -13828, -14010, -14191, -14372, -14553, -14732, -14912, -15090, -15269,
    -15446, -15623, -15800, -15976, -16151, -16325, -16499, -16673, -16846, -17018, -17189, -17360, -17530, -17700, -17869, -18037,
    -18204, -18371, -18537, -18703, -18868, -19032, -19195, -19357, -19519, -19680, -19841, -20000, -20159, -20317, -20475, -20631,
    -20787, -20942, -21096, -21250, -21403, -21554, -21705, -21856, -22005, -22154, -22301, -22448, -22594, -22739, -22884, -23027,
    -23170, -23311, -23452, -23592, -23731, -23870, -24007, -24143, -24279, -24413, -24547, -24680, -24811, -24942, -25072, -25201,
    -25329, -25456, -25582, -25708, -25832, -25955, -26077, -26198, -26319, -26438, -26556, -26674, -26790, -26905, -27019, -27133,
    -27245, -27356, -27466, -27575, -27683, -27790, -27896, -28001, -28105, -28208, -28310, -28411, -28510, -28609, -28706, -28803,
    -28898, -28992, -29085, -29177, -29268, -29358, -29447, -29534, -29621, -29706, -29791, -29874, -29956, -30037, -30117, -30195,
    -30273, -30349, -30424, -30498, -30571, -30643, -30714, -30783, -30852, -30919, -30985, -31050, -31113, -31176, -31237, -31297,
    -31356, -31414, -31470, -31526, -31580, -31633, -31685, -31736, -31785, -31833, -31880, -31926, -31971, -32014, -32057, -32098,
    -32137, -32176, -32213, -32250, -32285, -32318, -32351, -32382, -32412, -32441, -32469, -32495, -32521, -32545, -32567, -32589,
    -32609, -32628, -32646, -32663, -32678, -32692, -32705, -32717, -32728, -32737, -32745, -32752, -32757, -32761, -32765, -32766,
    -32767, -32766, -32765, -32761, -32757, -32752, -32745, -32737, -32728, -32717, -32705, -32692, -32678, -32663, -32646, -32628,
    -32609, -32589, -32567, -32545, -32521, -32495, -32469, -32441, -32412, -32382, -32351, -32318, -32285, -32250, -32213, -32176,
    -32137, -32098, -32057, -32014, -31971, -31926, -31880, -31833, -31785, -31736, -31685, -31633, -31580, -31526, -31470, -31414,
    -31356, -31297, -31237, -31176, -31113, -31050, -30985, -30919, -30852, -30783, -30714, -30643, -30571, -30498, -30424, -30349,
    -30273, -30195, -30117, -30037, -29956, -29874, -29791, -29706, -29621, -29534, -29447, -29358, -29268, -29177, -29085, -28992,
    -28898, -28803, -28706, -28609, -28510, -28411, -28310, -28208, -28105, -28001, -27896, -27790, -27683, -27575, -27466, -27356,
    -27245, -27133, -27019, -26905, -26790, -26674, -26556, -26438, -26319, -26198, -26077, -25955, -25832, -25708, -25582, -25456,
    -25329, -25201, -25072, -24942, -24811, -24680, -24547, -24413, -24279, -24143, -24007, -23870, -23731, -23592, -23452, -23311,
    -23170, -23027, -22884, -22739, -22594, -22448, -22301, -22154, -22005, -21856, -21705, -21554, -21403, -21250, -21096, -20942,
    -20787, -20631, -20475, -20317, -20159, -20000, -19841, -19680, -19519, -19357, -19195, -19032, -18868, -18703, -18537, -18371,
    -18204, -18037, -17869, -17700, -17530, -17360, -17189, -17018, -16846, -16673, -16499, -16325, -16151, -15976, -15800, -15623,
    -15446, -15269, -15090, -14912, -14732, -14553, -14372, -14191, -14010, -13828, -13645, -13462, -13279, -13094, -12910, -12725,
    -12539, -12353, -12167, -11980, -11793, -11605, -11417, -11228, -11039, -10849, -10659, -10469, -10278, -10087, -9896, -9704,
    -9512, -9319, -9126, -8933, -8739, -8545, -8351, -8157, -7962, -7767, -7571, -7375, -7179, -6983, -6786, -6590,
    -6393, -6195, -5998, -5800, -5602, -5404, -5205, -5007, -4808, -4609, -4410, -4210, -4011, -3811, -3612, -3412,
    -3212, -3012, -2811, -2611, -2410, -2210, -2009, -1809, -1608, -1407, -1206, -1005, -804, -603, -402, -201,
];
// END J12-GEN rust-trig

// BEGIN J13-GEN rust-impl
// ---------- J13: التوسعات الشبكية (البنود 191 و195 و202 و203 و204) ----------
// طبقة بروتوكول نقية حتمية فوق المقابس القائمة — بلا مقبس حي داخل
// أي دالة؛ والمرآة C في niva_std.c مطابقة حرفيا في الحساب والعرض
// ورسائل الأخطاء. التطبيقات هنا مصدرها ~/workspace/j13gen/rust_impl.txt
// ويحقنها المولّد — لا تعدلها في هذا الملف مباشرة.

// ---------- مساعدات J13 المشتركة ----------

fn j13_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn j13_need_str_list(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<String>> {
    let items = need_list(display, v, line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Str(s) => out.push(s.clone()),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع نصوصا لكن أعطيت {}",
                        display,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(out)
}

fn j13_no_newline(s: &str) -> bool {
    !s.contains('\n') && !s.contains('\r')
}

fn j13_no_space_or_newline(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().any(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r')
}

fn j13_reply_parts(
    display: &str,
    reply: &str,
    line: usize,
    col: usize,
) -> RioResult<(i64, String)> {
    let first = reply.split('\n').next().unwrap_or("");
    let first = first.strip_suffix('\r').unwrap_or(first);
    let bytes = first.as_bytes();
    if bytes.len() < 3 || !bytes[..3].iter().all(|b| b.is_ascii_digit()) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ردا يبدأ بثلاثة أرقام", display),
            line,
            col,
        ));
    }
    let code: i64 = first[..3].parse().unwrap_or(0);
    let text = first[3..].trim_start_matches([' ', '-']).to_string();
    Ok((code, text))
}

fn j13_chat_name_valid(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_') {
        return false;
    }
    if name.chars().count() > 32 {
        return false;
    }
    name.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

// ---------- صفحات (191) ----------

fn pages_escape(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.هرب";
    need_args(D, args, 1, line, col)?;
    let s = need_string(D, &args[0], line, col)?;
    Ok(Value::Str(j13_escape(&s)))
}

fn pages_element(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.عنصر";
    need_args(D, args, 3, line, col)?;
    let tag = need_string(D, &args[0], line, col)?;
    let attrs = need_dict(D, &args[1], line, col)?;
    let content = need_string(D, &args[2], line, col)?;
    let tb = tag.as_bytes();
    let tag_ok = !tb.is_empty()
        && tb[0].is_ascii_alphabetic()
        && tb.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-');
    if !tag_ok {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع وسما من حروف وأرقام وشرطات", D),
            line,
            col,
        ));
    }
    let void_tags = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
        "source", "track", "wbr",
    ];
    let is_void = void_tags.contains(&tag.as_str());
    if is_void && !content.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع محتوى فارغا للوسم الفارغ {}", D, tag),
            line,
            col,
        ));
    }
    let mut attr_text = String::new();
    for (k, v) in attrs {
        match v {
            Value::Str(val) => {
                attr_text.push(' ');
                attr_text.push_str(k);
                attr_text.push_str("=\"");
                attr_text.push_str(&j13_escape(val));
                attr_text.push('"');
            }
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع سمات نصية لكن أعطيت {}",
                        D,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    if is_void {
        Ok(Value::Str(format!("<{}{} />", tag, attr_text)))
    } else {
        Ok(Value::Str(format!(
            "<{}{}>{}</{}>",
            tag, attr_text, content, tag
        )))
    }
}

fn pages_page(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.صفحة";
    need_args(D, args, 2, line, col)?;
    let title = need_string(D, &args[0], line, col)?;
    let body = need_string(D, &args[1], line, col)?;
    Ok(Value::Str(format!(
        "<!DOCTYPE html>\n<html lang=\"ar\" dir=\"rtl\">\n<head>\n<meta charset=\"utf-8\" />\n<title>{}</title>\n</head>\n<body>\n{}\n</body>\n</html>\n",
        j13_escape(&title),
        body
    )))
}

fn pages_list_inner(
    display: &str,
    tag: &str,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args(display, args, 1, line, col)?;
    let items = j13_need_str_list(display, &args[0], line, col)?;
    let mut out = String::new();
    out.push('<');
    out.push_str(tag);
    out.push_str(">\n");
    for item in items {
        out.push_str("<li>");
        out.push_str(&j13_escape(&item));
        out.push_str("</li>\n");
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Ok(Value::Str(out))
}

fn pages_list(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    pages_list_inner("صفحات.قائمة", "ul", args, line, col)
}

fn pages_ordered_list(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    pages_list_inner("صفحات.قائمة_مرتبة", "ol", args, line, col)
}

fn pages_table(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.جدول";
    need_args(D, args, 2, line, col)?;
    let headers = j13_need_str_list(D, &args[0], line, col)?;
    let rows_raw = need_list(D, &args[1], line, col)?;
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(rows_raw.len());
    for row in rows_raw {
        rows.push(j13_need_str_list(D, row, line, col)?);
    }
    for row in &rows {
        if row.len() != headers.len() {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل صف بطول الترويسة نفسها", D),
                line,
                col,
            ));
        }
    }
    let mut out = String::new();
    out.push_str("<table>\n<thead>\n<tr>");
    for h in &headers {
        out.push_str("<th>");
        out.push_str(&j13_escape(h));
        out.push_str("</th>");
    }
    out.push_str("</tr>\n</thead>\n<tbody>\n");
    for row in &rows {
        out.push_str("<tr>");
        for cell in row {
            out.push_str("<td>");
            out.push_str(&j13_escape(cell));
            out.push_str("</td>");
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>");
    Ok(Value::Str(out))
}

fn pages_link(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.رابط";
    need_args(D, args, 2, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let href = need_string(D, &args[1], line, col)?;
    Ok(Value::Str(format!(
        "<a href=\"{}\">{}</a>",
        j13_escape(&href),
        j13_escape(&text)
    )))
}

fn pages_image(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صفحات.صورة";
    need_args(D, args, 2, line, col)?;
    let src = need_string(D, &args[0], line, col)?;
    let alt = need_string(D, &args[1], line, col)?;
    Ok(Value::Str(format!(
        "<img src=\"{}\" alt=\"{}\" />",
        j13_escape(&src),
        j13_escape(&alt)
    )))
}

// ---------- بريد (195) ----------

fn mail_build(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.كون_رسالة";
    need_args(D, args, 4, line, col)?;
    let from = need_string(D, &args[0], line, col)?;
    let to = need_string(D, &args[1], line, col)?;
    let subject = need_string(D, &args[2], line, col)?;
    let body = need_string(D, &args[3], line, col)?;
    for h in [&from, &to, &subject] {
        if !j13_no_newline(h) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع رأسا بلا أسطر جديدة", D),
                line,
                col,
            ));
        }
    }
    Ok(Value::Str(format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\n\r\n{}",
        from, to, subject, body
    )))
}

fn mail_parse(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.حلل_رسالة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut from = String::new();
    let mut to = String::new();
    let mut subject = String::new();
    let mut body_start = lines.len();
    for (i, raw) in lines.iter().enumerate() {
        let ln = raw.strip_suffix('\r').unwrap_or(raw);
        if ln.is_empty() {
            body_start = i + 1;
            break;
        }
        if let Some(v) = ln.strip_prefix("From:") {
            from = v.trim().to_string();
        } else if let Some(v) = ln.strip_prefix("To:") {
            to = v.trim().to_string();
        } else if let Some(v) = ln.strip_prefix("Subject:") {
            subject = v.trim().to_string();
        }
    }
    let body_lines: Vec<String> = lines[body_start..]
        .iter()
        .map(|raw| raw.strip_suffix('\r').unwrap_or(raw).to_string())
        .collect();
    Ok(Value::Dict(vec![
        ("من".to_string(), Value::Str(from)),
        ("إلى".to_string(), Value::Str(to)),
        ("موضوع".to_string(), Value::Str(subject)),
        ("جسم".to_string(), Value::Str(body_lines.join("\n"))),
    ]))
}

fn mail_cmd_from(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.أمر_من";
    need_args(D, args, 1, line, col)?;
    let addr = need_string(D, &args[0], line, col)?;
    if !j13_no_space_or_newline(&addr) || addr.contains('<') || addr.contains('>') {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع عنوانا بلا مسافات ولا أقواس", D),
            line,
            col,
        ));
    }
    Ok(Value::Str(format!("MAIL FROM:<{}>\r\n", addr)))
}

fn mail_cmd_to(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.أمر_إلى";
    need_args(D, args, 1, line, col)?;
    let addr = need_string(D, &args[0], line, col)?;
    if !j13_no_space_or_newline(&addr) || addr.contains('<') || addr.contains('>') {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع عنوانا بلا مسافات ولا أقواس", D),
            line,
            col,
        ));
    }
    Ok(Value::Str(format!("RCPT TO:<{}>\r\n", addr)))
}

fn mail_cmd_data(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.أمر_بيانات";
    need_args(D, args, 0, line, col)?;
    Ok(Value::Str("DATA\r\n".to_string()))
}

fn mail_parse_reply(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.حلل_رد";
    need_args(D, args, 1, line, col)?;
    let reply = need_string(D, &args[0], line, col)?;
    let (code, text) = j13_reply_parts(D, &reply, line, col)?;
    Ok(Value::Dict(vec![
        ("رمز".to_string(), Value::Number(code as f64)),
        ("نص".to_string(), Value::Str(text)),
    ]))
}

fn mail_parse_pop3(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بريد.حلل_رد_pop3";
    need_args(D, args, 1, line, col)?;
    let reply = need_string(D, &args[0], line, col)?;
    let first = reply.split('\n').next().unwrap_or("");
    let first = first.strip_suffix('\r').unwrap_or(first);
    let (state, rest) = if let Some(r) = first.strip_prefix("+OK") {
        ("موافق", r)
    } else if let Some(r) = first.strip_prefix("-ERR") {
        ("خطأ", r)
    } else {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ردا يبدأ بـ +OK أو -ERR", D),
            line,
            col,
        ));
    };
    Ok(Value::Dict(vec![
        ("حالة".to_string(), Value::Str(state.to_string())),
        (
            "نص".to_string(),
            Value::Str(rest.trim_start_matches(' ').to_string()),
        ),
    ]))
}

// ---------- محادثة (202) ----------

fn chat_build(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "محادثة.كون_رسالة";
    need_args(D, args, 2, line, col)?;
    let sender = need_string(D, &args[0], line, col)?;
    let text = need_string(D, &args[1], line, col)?;
    if !j13_no_newline(&sender) || !j13_no_newline(&text) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع نصا بلا أسطر جديدة", D),
            line,
            col,
        ));
    }
    Ok(Value::Str(format!("[{}] {}\n", sender, text)))
}

fn chat_parse(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "محادثة.حلل_رسالة";
    need_args(D, args, 1, line, col)?;
    let line_s = need_string(D, &args[0], line, col)?;
    let s = line_s
        .strip_suffix('\n')
        .unwrap_or(&line_s)
        .strip_suffix('\r')
        .unwrap_or(line_s.strip_suffix('\n').unwrap_or(&line_s));
    let bad = || {
        RioError::runtime(
            format!("الدالة {} تتوقع رسالة بصيغة [مرسل] نص", D),
            line,
            col,
        )
    };
    if !s.starts_with('[') {
        return Err(bad());
    }
    let close = s.find(']').ok_or_else(bad)?;
    if close < 2 || s.as_bytes().get(close + 1) != Some(&b' ') {
        return Err(bad());
    }
    let sender = &s[1..close];
    let text = &s[close + 2..];
    Ok(Value::Dict(vec![
        ("مرسل".to_string(), Value::Str(sender.to_string())),
        ("نص".to_string(), Value::Str(text.to_string())),
    ]))
}

fn chat_join_inner(
    display: &str,
    verb: &str,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    need_args(display, args, 2, line, col)?;
    let room = need_string(display, &args[0], line, col)?;
    let user = need_string(display, &args[1], line, col)?;
    if !j13_chat_name_valid(&room) || !j13_chat_name_valid(&user) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع اسما صالحا", display),
            line,
            col,
        ));
    }
    Ok(Value::Str(format!("{} #{} {}\n", verb, room, user)))
}

fn chat_join(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    chat_join_inner("محادثة.أمر_انضمام", "JOIN", args, line, col)
}

fn chat_leave(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    chat_join_inner("محادثة.أمر_مغادرة", "LEAVE", args, line, col)
}

fn chat_name_ok(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "محادثة.اسم_صالح";
    need_args(D, args, 1, line, col)?;
    let name = need_string(D, &args[0], line, col)?;
    Ok(Value::Bool(j13_chat_name_valid(&name)))
}

// ---------- نقل_ملفات (203) ----------

fn ftp_login(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نقل_ملفات.أمر_دخول";
    need_args(D, args, 2, line, col)?;
    let user = need_string(D, &args[0], line, col)?;
    let pass = need_string(D, &args[1], line, col)?;
    if !j13_no_space_or_newline(&user) || !j13_no_space_or_newline(&pass) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع اسما وكلمة بلا مسافات ولا أسطر جديدة", D),
            line,
            col,
        ));
    }
    Ok(Value::Str(format!("USER {}\r\nPASS {}\r\n", user, pass)))
}

fn ftp_path_cmd(
    display: &str,
    verb: &str,
    args: &[Value],
    line: usize,
    col: usize,
    allow_empty: bool,
) -> RioResult<Value> {
    need_args(display, args, 1, line, col)?;
    let path = need_string(display, &args[0], line, col)?;
    if (!allow_empty && path.is_empty()) || !j13_no_newline(&path) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع مسارا غير فارغ بلا أسطر جديدة", display),
            line,
            col,
        ));
    }
    if path.is_empty() {
        Ok(Value::Str(format!("{}\r\n", verb)))
    } else {
        Ok(Value::Str(format!("{} {}\r\n", verb, path)))
    }
}

fn ftp_retr(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    ftp_path_cmd("نقل_ملفات.أمر_استرجاع", "RETR", args, line, col, false)
}

fn ftp_stor(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    ftp_path_cmd("نقل_ملفات.أمر_تخزين", "STOR", args, line, col, false)
}

fn ftp_list_cmd(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    ftp_path_cmd("نقل_ملفات.أمر_قائمة", "LIST", args, line, col, true)
}

fn ftp_quit(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نقل_ملفات.أمر_إنهاء";
    need_args(D, args, 0, line, col)?;
    Ok(Value::Str("QUIT\r\n".to_string()))
}

fn ftp_parse_reply(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نقل_ملفات.حلل_رد";
    need_args(D, args, 1, line, col)?;
    let reply = need_string(D, &args[0], line, col)?;
    let (code, text) = j13_reply_parts(D, &reply, line, col)?;
    Ok(Value::Dict(vec![
        ("رمز".to_string(), Value::Number(code as f64)),
        ("نص".to_string(), Value::Str(text)),
    ]))
}

fn ftp_parse_list(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نقل_ملفات.حلل_قائمة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    let mut n = 0usize;
    for raw in text.split('\n') {
        let ln = raw.strip_suffix('\r').unwrap_or(raw);
        if ln.is_empty() {
            continue;
        }
        n += 1;
        let fields: Vec<&str> = ln.split_whitespace().collect();
        if fields.len() < 9 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع سطر قائمة من 9 حقول على الأقل في السطر {}", D, n),
                line,
                col,
            ));
        }
        let perms = fields[0];
        let is_dir = perms.starts_with('d');
        let size_s = fields[4];
        let size_ok = !size_s.is_empty() && size_s.bytes().all(|b| b.is_ascii_digit());
        let size: i64 = if size_ok {
            size_s.parse().map_err(|_| {
                RioError::runtime(
                    format!("الدالة {} تتوقع حجما عدديا في السطر {}", D, n),
                    line,
                    col,
                )
            })?
        } else {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع حجما عدديا في السطر {}", D, n),
                line,
                col,
            ));
        };
        let name = fields[8..].join(" ");
        out.push(Value::Dict(vec![
            ("اسم".to_string(), Value::Str(name)),
            ("حجم".to_string(), Value::Number(size as f64)),
            ("دليل".to_string(), Value::Bool(is_dir)),
        ]));
    }
    Ok(Value::List(out))
}

// ---------- وقت_شبكة (204) ----------

fn ntp_packet(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وقت_شبكة.كون_حزمة";
    need_args(D, args, 1, line, col)?;
    let version = need_integer(D, &args[0], line, col)?;
    if !(1..=4).contains(&version) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع إصدارا بين 1 و 4", D),
            line,
            col,
        ));
    }
    let mut pkt: Vec<Value> = Vec::with_capacity(48);
    pkt.push(Value::Number((((version as i64) << 3) | 3) as f64));
    for _ in 1..48 {
        pkt.push(Value::Number(0.0));
    }
    Ok(Value::List(pkt))
}

fn ntp_parse(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وقت_شبكة.حلل_حزمة";
    need_args(D, args, 1, line, col)?;
    let bytes = need_bytes(D, &args[0], line, col)?;
    if bytes.len() != 48 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع حزمة من 48 بايتا", D),
            line,
            col,
        ));
    }
    let version = ((bytes[0] >> 3) & 7) as i64;
    let stratum = bytes[1] as i64;
    let secs = u32::from_be_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as i64
        - 2208988800;
    let frac = u32::from_be_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]) as i64;
    Ok(Value::Dict(vec![
        ("إصدار".to_string(), Value::Number(version as f64)),
        ("طبقة".to_string(), Value::Number(stratum as f64)),
        ("وقت".to_string(), Value::Number(secs as f64)),
        ("جزء".to_string(), Value::Number(frac as f64)),
    ]))
}

fn ntp_to_unix(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وقت_شبكة.حول_ثواني";
    need_args(D, args, 1, line, col)?;
    let secs = need_integer(D, &args[0], line, col)?;
    if !(0..=4294967295).contains(&secs) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ثواني بين 0 و 4294967295", D),
            line,
            col,
        ));
    }
    Ok(Value::Number((secs - 2208988800) as f64))
}

fn ntp_valid(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وقت_شبكة.حزمة_صحيحة";
    need_args(D, args, 1, line, col)?;
    let bytes = need_bytes(D, &args[0], line, col)?;
    let ok = bytes.len() == 48
        && ((bytes[0] >> 3) & 7) >= 3
        && ((bytes[0] >> 3) & 7) <= 4
        && (bytes[0] & 7) >= 3
        && (bytes[0] & 7) <= 4;
    Ok(Value::Bool(ok))
}
// END J13-GEN rust-impl

// BEGIN J15-GEN rust-impl
// ---------- J15: أنماط الاستخدام (225 و228 و230 و240) ----------
// تطبيقات فوق اللغة لا فيها — بلا صياغة جديدة وبلا كلمة
// مفتاحية جديدة وبلا تغيير في المحلل الدلالي؛ دوال مكتبة
// في وحدات قائمة (جداول ونص وقوائم) ووحدة جديدة (لغات).
// مصدر هذا القسم ~/workspace/j15gen/rust_impl.txt ويحقنه
// المولّد — لا تعدله في هذا الملف مباشرة.
// المرآة C في ~/workspace/j15gen/c_impl.txt مطابقة حرفيا
// في الحساب والعرض ورسائل الأخطاء.

// ---------- مساعدات J15 المشتركة ----------

/// خطأ محلل التعبيرات الصغير (مشترك بين 228 و240).
enum J15ExprErr {
    Parse,
    Unknown(String),
    DivZero,
}

/// رمز في تعبير: عدد أو اسم أو عامل أو قوس.
#[derive(Clone, Debug)]
enum J15Tok {
    Num(f64, String),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

/// شجرة تعبير: عدد أو متغير أو نفي أو عملية ثنائية.
enum J15Ast {
    Num(f64),
    Var(String),
    Neg(Box<J15Ast>),
    Bin(char, Box<J15Ast>, Box<J15Ast>),
}

/// هل المحرف فاصل تعبير (مسافة بيضاء أو عامل أو قوس)؟
fn j15_is_expr_delim(c: char) -> bool {
    c.is_whitespace() || "+-*/%()".contains(c)
}

/// رمّز تعبيرا إلى رموز؛ `=` محرف مرفوض (يفصل القاعدة عن
/// تعبيرها فلا يدخل التعبير أبدا).
fn j15_tokenize(expr: &str) -> Result<Vec<J15Tok>, J15ExprErr> {
    let chars: Vec<char> = expr.chars().collect();
    let mut toks: Vec<J15Tok> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '=' {
            return Err(J15ExprErr::Parse);
        }
        if c == '(' {
            toks.push(J15Tok::LParen);
            i += 1;
            continue;
        }
        if c == ')' {
            toks.push(J15Tok::RParen);
            i += 1;
            continue;
        }
        if "+-*/%".contains(c) {
            toks.push(J15Tok::Op(c));
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let mut j = i;
            let mut text = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() {
                text.push(chars[j]);
                j += 1;
            }
            if j < chars.len() && chars[j] == '.' {
                text.push('.');
                j += 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    text.push(chars[j]);
                    j += 1;
                }
            }
            let v: f64 = text.parse().map_err(|_| J15ExprErr::Parse)?;
            toks.push(J15Tok::Num(v, text));
            i = j;
            continue;
        }
        let mut j = i;
        let mut text = String::new();
        while j < chars.len() && !j15_is_expr_delim(chars[j]) && chars[j] != '=' {
            text.push(chars[j]);
            j += 1;
        }
        if text.is_empty() {
            return Err(J15ExprErr::Parse);
        }
        toks.push(J15Tok::Ident(text));
        i = j;
    }
    Ok(toks)
}

/// نص الرمز كما كُتب (للرموز في حلل_تعبير).
fn j15_tok_text(t: &J15Tok) -> String {
    match t {
        J15Tok::Num(_, s) => s.clone(),
        J15Tok::Ident(s) => s.clone(),
        J15Tok::Op(c) => c.to_string(),
        J15Tok::LParen => "(".to_string(),
        J15Tok::RParen => ")".to_string(),
    }
}

/// محلل تنازلي تكراري: جمع/طرح ثم ضرب/قسمة/باقي ثم عامل.
struct J15Parser {
    toks: Vec<J15Tok>,
    pos: usize,
}

impl J15Parser {
    fn peek(&self) -> Option<&J15Tok> {
        self.toks.get(self.pos)
    }

    fn parse_expr(&mut self) -> Result<J15Ast, J15ExprErr> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Some(J15Tok::Op('+')) | Some(J15Tok::Op('-')) => {
                    let op = match self.peek().unwrap() {
                        J15Tok::Op(c) => *c,
                        _ => unreachable!(),
                    };
                    self.pos += 1;
                    let right = self.parse_term()?;
                    left = J15Ast::Bin(op, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<J15Ast, J15ExprErr> {
        let mut left = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(J15Tok::Op('*')) | Some(J15Tok::Op('/')) | Some(J15Tok::Op('%')) => {
                    let op = match self.peek().unwrap() {
                        J15Tok::Op(c) => *c,
                        _ => unreachable!(),
                    };
                    self.pos += 1;
                    let right = self.parse_factor()?;
                    left = J15Ast::Bin(op, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<J15Ast, J15ExprErr> {
        match self.peek().cloned() {
            Some(J15Tok::Num(v, _)) => {
                self.pos += 1;
                Ok(J15Ast::Num(v))
            }
            Some(J15Tok::Ident(name)) => {
                self.pos += 1;
                Ok(J15Ast::Var(name))
            }
            Some(J15Tok::Op('-')) => {
                self.pos += 1;
                let inner = self.parse_factor()?;
                Ok(J15Ast::Neg(Box::new(inner)))
            }
            Some(J15Tok::LParen) => {
                self.pos += 1;
                let inner = self.parse_expr()?;
                match self.peek() {
                    Some(J15Tok::RParen) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    _ => Err(J15ExprErr::Parse),
                }
            }
            _ => Err(J15ExprErr::Parse),
        }
    }
}

/// حلّل تعبيرا إلى شجرة؛ ورفض أي رموز زائدة بعده.
fn j15_parse(expr: &str) -> Result<J15Ast, J15ExprErr> {
    let toks = j15_tokenize(expr)?;
    if toks.is_empty() {
        return Err(J15ExprErr::Parse);
    }
    let mut p = J15Parser { toks, pos: 0 };
    let ast = p.parse_expr()?;
    if p.pos != p.toks.len() {
        return Err(J15ExprErr::Parse);
    }
    Ok(ast)
}

/// متغيرات الشجرة بترتيب الظهور الأول، بلا تكرار.
fn j15_ast_vars(ast: &J15Ast, out: &mut Vec<String>) {
    match ast {
        J15Ast::Num(_) => {}
        J15Ast::Var(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        J15Ast::Neg(inner) => j15_ast_vars(inner, out),
        J15Ast::Bin(_, a, b) => {
            j15_ast_vars(a, out);
            j15_ast_vars(b, out);
        }
    }
}

/// قيّم شجرة بمتغيرات (اسم، قيمة)؛ والمتغير الغائب خطأ.
fn j15_eval(ast: &J15Ast, vars: &[(String, f64)]) -> Result<f64, J15ExprErr> {
    match ast {
        J15Ast::Num(v) => Ok(*v),
        J15Ast::Var(name) => vars
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| J15ExprErr::Unknown(name.clone())),
        J15Ast::Neg(inner) => Ok(-j15_eval(inner, vars)?),
        J15Ast::Bin(op, a, b) => {
            let x = j15_eval(a, vars)?;
            let y = j15_eval(b, vars)?;
            match op {
                '+' => Ok(x + y),
                '-' => Ok(x - y),
                '*' => Ok(x * y),
                '/' => {
                    if y == 0.0 {
                        Err(J15ExprErr::DivZero)
                    } else {
                        Ok(x / y)
                    }
                }
                '%' => {
                    if y == 0.0 {
                        Err(J15ExprErr::DivZero)
                    } else {
                        Ok(x % y)
                    }
                }
                _ => Err(J15ExprErr::Parse),
            }
        }
    }
}

/// اسم قاعدة صالح: أوله حرف أو _، وباقيه حرف أو رقم أو _.
fn j15_name_valid(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// حلّل سطر قاعدة «اسم = تعبير» إلى (الاسم، التعبير
/// مجردا من المسافات، المتغيرات). أي عيب صيغة خطأ تحليل.
fn j15_parse_rule(text: &str) -> Result<(String, String, Vec<String>), J15ExprErr> {
    let eq = text.find('=').ok_or(J15ExprErr::Parse)?;
    let name = text[..eq].trim().to_string();
    let expr = text[eq + 1..].trim().to_string();
    if name.is_empty() || !j15_name_valid(&name) || expr.is_empty() {
        return Err(J15ExprErr::Parse);
    }
    let ast = j15_parse(&expr)?;
    let mut vars = Vec::new();
    j15_ast_vars(&ast, &mut vars);
    Ok((name, expr, vars))
}

/// قاموس قاعدة من (الاسم، التعبير، المتغيرات).
fn j15_rule_value(name: &str, expr: &str, vars: &[String]) -> Value {
    Value::Dict(vec![
        ("الاسم".to_string(), Value::Str(name.to_string())),
        ("التعبير".to_string(), Value::Str(expr.to_string())),
        (
            "المتغيرات".to_string(),
            Value::List(vars.iter().map(|v| Value::Str(v.clone())).collect()),
        ),
    ])
}

/// قيّم قاعدة (أزواج قاموسها) بقيم (أزواج قاموس)؛ ورسائل
/// الخطأ بصيغة العارض الممرر (لغات.قيم_قاعدة أو نفذ_قاعدة).
fn j15_eval_rule_pairs(
    display: &str,
    rule: &[(String, Value)],
    values: &[(String, Value)],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let expr = match tables_row_get(rule, "التعبير") {
        Some(Value::Str(s)) => s.clone(),
        _ => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع قاعدة من عرف_قاعدة لكن «التعبير» غائب أو ليس نصا",
                    display
                ),
                line,
                col,
            ))
        }
    };
    let mut vars: Vec<(String, f64)> = Vec::new();
    for (k, v) in values {
        vars.push((k.clone(), need_number(display, v, line, col)?));
    }
    let ast = j15_parse(&expr).map_err(|_| {
        RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", display, expr),
            line,
            col,
        )
    })?;
    match j15_eval(&ast, &vars) {
        Ok(v) => Ok(Value::Number(v)),
        Err(J15ExprErr::Unknown(name)) => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع متغيرا معروفا لكن «{}» غير معروف",
                display, name
            ),
            line,
            col,
        )),
        Err(J15ExprErr::DivZero) => Err(RioError::runtime(
            format!("الدالة {} تتوقع مقسوما عليه غير صفر في «{}»", display, expr),
            line,
            col,
        )),
        Err(J15ExprErr::Parse) => Err(RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", display, expr),
            line,
            col,
        )),
    }
}

/// مقارنة عامل استعلام: يساوي، لا_يساوي، أكبر، أصغر،
/// أكبر_أو_يساوي، أصغر_أو_يساوي، يحتوي (نصان فقط).
fn j15_cmp_op(
    display: &str,
    op: &str,
    a: &Value,
    b: &Value,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    match op {
        "يساوي" | "لا_يساوي" | "أكبر" | "أصغر" | "أكبر_أو_يساوي" | "أصغر_أو_يساوي" => {
            match (a, b) {
                (Value::Number(x), Value::Number(y)) => Ok(match op {
                    "يساوي" => x == y,
                    "لا_يساوي" => x != y,
                    "أكبر" => x > y,
                    "أصغر" => x < y,
                    "أكبر_أو_يساوي" => x >= y,
                    _ => x <= y,
                }),
                (Value::Str(x), Value::Str(y)) => Ok(match op {
                    "يساوي" => x == y,
                    "لا_يساوي" => x != y,
                    "أكبر" => x > y,
                    "أصغر" => x < y,
                    "أكبر_أو_يساوي" => x >= y,
                    _ => x <= y,
                }),
                _ => Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع عددين أو نصين للعامل «{}» لكن أعطيت {} و{}",
                        display,
                        op,
                        a.type_name(),
                        b.type_name()
                    ),
                    line,
                    col,
                )),
            }
        }
        "يحتوي" => match (a, b) {
            (Value::Str(x), Value::Str(y)) => Ok(x.contains(y.as_str())),
            _ => Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع نصين لعامل «يحتوي» لكن أعطيت {} و{}",
                    display,
                    a.type_name(),
                    b.type_name()
                ),
                line,
                col,
            )),
        },
        _ => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عاملا معروفا لكن «{}» غير معروف — المتاح: يساوي، لا_يساوي، أكبر، أصغر، أكبر_أو_يساوي، أصغر_أو_يساوي، يحتوي",
                display, op
            ),
            line,
            col,
        )),
    }
}

/// هل المحرف نهاية جملة؟ نقطة أو تعجب أو استفهام (غربي
/// أو عربي ؟) أو علامة حذف ….
fn j15_is_term(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '؟' | '…')
}

/// قسّم نصا إلى جمل: تنتهي الجملة عند سلسلة نهايات
/// متتالية (فتصير «...» نهاية واحدة)؛ والنقطة بين
/// رقمين لا تنهي جملة (3.5)؛ وتُسقَط المسافات حول كل
/// جملة؛ والجملة بلا نهاية تُحفظ كما هي.
fn j15_split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let decimal_dot = c == '.'
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit();
        if j15_is_term(c) && !decimal_dot {
            while i < chars.len() && j15_is_term(chars[i]) {
                let t = chars[i];
                let dd = t == '.'
                    && i > 0
                    && i + 1 < chars.len()
                    && chars[i - 1].is_ascii_digit()
                    && chars[i + 1].is_ascii_digit();
                if dd {
                    break;
                }
                cur.push(t);
                i += 1;
            }
            let s = cur.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            cur = String::new();
        } else {
            cur.push(c);
            i += 1;
        }
    }
    let s = cur.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// قيمة خلية: لاشيء أو غائبة تُعامل كغائبة (كـ رتب_جدولا).
fn j15_cell<'a>(row: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    match tables_row_get(row, key) {
        Some(Value::Nil) => None,
        other => other,
    }
}

// ---------- 225: لغة استعلام (جداول وقوائم) ----------

/// 225: حيث(جدول، عمود، عامل، قيمة) — صفوف تحقق شرط
/// العمود بالعامل؛ والعمود الغائب أو الفارغ خطأ.
fn tables_where(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.حيث";
    need_args(D, args, 4, line, col)?;
    let column = need_string(D, &args[1], line, col)?;
    let op = need_string(D, &args[2], line, col)?;
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let cell = match j15_cell(row, &column) {
            Some(v) => v,
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                        D,
                        column,
                        i + 1
                    ),
                    line,
                    col,
                ))
            }
        };
        if j15_cmp_op(D, &op, cell, &args[3], line, col)? {
            out.push(Value::Dict(row.clone()));
        }
    }
    Ok(Value::List(out))
}

/// 225: اختر(جدول، أعمدة) — إسقاط الأعمدة المطلوبة
/// وحدها، بالترتيب المطلوب؛ والعمود الغائب خطأ.
fn tables_select(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.اختر";
    need_args(D, args, 2, line, col)?;
    let cols = need_list(D, &args[1], line, col)?;
    let mut names: Vec<String> = Vec::new();
    for c in cols {
        names.push(need_string(D, c, line, col)?);
    }
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let mut new_row: Vec<(String, Value)> = Vec::new();
        for name in &names {
            match j15_cell(row, name) {
                Some(v) => new_row.push((name.clone(), v.clone())),
                None => {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                            D,
                            name,
                            i + 1
                        ),
                        line,
                        col,
                    ))
                }
            }
        }
        out.push(Value::Dict(new_row));
    }
    Ok(Value::List(out))
}

/// 225: جمع_حسب(جدول، عمود_تجميع، عمود_قيمة، عملية) —
/// تجميع الصفوف بمفتاح عمود التجميع (بترتيب الظهور
/// الأول) ثم تجميع عمود القيمة: عدد أو مجموع أو متوسط
/// أو أدنى أو أقصى. لعملية عدد يجوز أن يكون عمود
/// القيمة لاشيء. صف النتيجة {عمود_التجميع، العملية}.
fn tables_group_agg(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.جمع_حسب";
    need_args(D, args, 4, line, col)?;
    let group_col = need_string(D, &args[1], line, col)?;
    let op = need_string(D, &args[3], line, col)?;
    if !matches!(op.as_str(), "عدد" | "مجموع" | "متوسط" | "أدنى" | "أقصى") {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عملية تجميع معروفة لكن «{}» غير معروفة — المتاح: عدد، مجموع، متوسط، أدنى، أقصى",
                D, op
            ),
            line,
            col,
        ));
    }
    let val_col: Option<String> = match &args[2] {
        Value::Nil => None,
        Value::Str(s) => Some(s.clone()),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع عمود قيمة نصا أو لاشيء لكن أعطيت {}",
                    D,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    if op != "عدد" && val_col.is_none() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عمود قيمة لعملية «{}» لكن أعطيت لاشيء",
                D, op
            ),
            line,
            col,
        ));
    }
    let rows = tables_need_table(D, &args[0], line, col)?;
    // مجموعات بترتيب الظهور الأول: (المفتاح، فهارس الصفوف).
    let mut groups: Vec<(Value, Vec<usize>)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let key = match j15_cell(row, &group_col) {
            Some(v @ Value::Number(_)) | Some(v @ Value::Str(_)) => v.clone(),
            Some(other) => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع قيما عددية أو نصية في عمود التجميع لكن الصف {} هو {}",
                        D,
                        i + 1,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                        D,
                        group_col,
                        i + 1
                    ),
                    line,
                    col,
                ))
            }
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, idxs)) => idxs.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for (key, idxs) in &groups {
        let agg = if op == "عدد" {
            idxs.len() as f64
        } else {
            let vc = val_col.as_ref().unwrap();
            let mut vals: Vec<f64> = Vec::new();
            for &i in idxs {
                match j15_cell(&rows[i], vc) {
                    Some(Value::Number(x)) => vals.push(*x),
                    Some(other) => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتوقع عددا في العمود «{}» في الصف {} لكن أعطيت {}",
                                D,
                                vc,
                                i + 1,
                                other.type_name()
                            ),
                            line,
                            col,
                        ))
                    }
                    None => {
                        return Err(RioError::runtime(
                            format!(
                                "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                                D,
                                vc,
                                i + 1
                            ),
                            line,
                            col,
                        ))
                    }
                }
            }
            match op.as_str() {
                "مجموع" => vals.iter().sum(),
                "متوسط" => vals.iter().sum::<f64>() / vals.len() as f64,
                "أدنى" => vals.iter().cloned().fold(f64::INFINITY, f64::min),
                _ => vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            }
        };
        out.push(Value::Dict(vec![
            (group_col.clone(), key.clone()),
            (op.clone(), Value::Number(agg)),
        ]));
    }
    Ok(Value::List(out))
}

/// 225: رشح_بدالة(جدول، دالة) — صفوف يعيد شرطها
/// (دالة تستلم الصف) صحيحا.
fn tables_filter_fn(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.رشح_بدالة";
    need_args(D, args, 2, line, col)?;
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for row in &rows {
        if j3_call_bool(ctx, &args[1], Value::Dict(row.clone()), D, line, col)? {
            out.push(Value::Dict(row.clone()));
        }
    }
    Ok(Value::List(out))
}

/// 225: أول_صفوف(جدول، عدد) — أول عدد من الصفوف؛
/// والعدد السالب خطأ.
fn tables_head(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.أول_صفوف";
    need_args(D, args, 2, line, col)?;
    let n = need_integer(D, &args[1], line, col)?;
    if n < 0 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع عددا غير سالب لكن أعطيت {}", D, n),
            line,
            col,
        ));
    }
    let rows = tables_need_table(D, &args[0], line, col)?;
    let take = (n as usize).min(rows.len());
    let mut out: Vec<Value> = Vec::new();
    for row in rows.iter().take(take) {
        out.push(Value::Dict(row.clone()));
    }
    Ok(Value::List(out))
}

/// 225: قوائم.رشح_بقيمة(قائمة، عامل، قيمة) — عناصر
/// تحقق العامل مع القيمة.
fn lists_filter_value(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "قوائم.رشح_بقيمة";
    need_args(D, args, 3, line, col)?;
    let op = need_string(D, &args[1], line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut out: Vec<Value> = Vec::new();
    for item in &items {
        if j15_cmp_op(D, &op, item, &args[2], line, col)? {
            out.push(item.clone());
        }
    }
    Ok(Value::List(out))
}

/// 225: قوائم.رشح_بدالة(قائمة، دالة) — عناصر يعيد
/// شرطها (دالة تستلم العنصر) صحيحا.
fn lists_filter_fn(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "قوائم.رشح_بدالة";
    need_args(D, args, 2, line, col)?;
    let items = need_list(D, &args[0], line, col)?.clone();
    let mut out: Vec<Value> = Vec::new();
    for item in items {
        if j3_call_bool(ctx, &args[1], item.clone(), D, line, col)? {
            out.push(item);
        }
    }
    Ok(Value::List(out))
}

// ---------- 228: برمجة بالجداول (جداول محسوبة) ----------

/// جدول محسوب بعد التحقق: أعمدة وأصفاف وصيغ.
struct J15Comp {
    cols: Vec<String>,
    rows: Vec<Vec<(String, Value)>>,
    /// (العمود، الصيغة، المتغيرات) بترتيب الصيغ.
    formulas: Vec<(String, String, Vec<String>)>,
    /// ترتيب تقييم الصيغ (بلا دورة).
    order: Vec<usize>,
}

/// ترتيب تقييم الصيغ: صيغة تُقيَّم بعد كل صيغة تعتمد
/// عليها؛ والدورة خطأ.
fn j15_topo_order(formulas: &[(String, String, Vec<String>)]) -> Result<Vec<usize>, ()> {
    let mut order: Vec<usize> = Vec::new();
    let mut rest: Vec<usize> = (0..formulas.len()).collect();
    loop {
        if rest.is_empty() {
            return Ok(order);
        }
        let mut progressed = false;
        let mut i = 0;
        while i < rest.len() {
            let f = rest[i];
            let deps_ok = formulas[f].2.iter().all(|v| {
                !formulas.iter().any(|(n, _, _)| n == v) || order.contains(&formulas.iter().position(|(n, _, _)| n == v).unwrap())
            });
            if deps_ok {
                order.push(f);
                rest.remove(i);
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            return Err(());
        }
    }
}

/// اقرأ جدولا محسوبا وتحقق من بنيته؛ والصيغ تُحلَّل
/// وتُرتَّب هنا مرة واحدة.
fn j15_read_comp(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<J15Comp> {
    let pairs = match v {
        Value::Dict(p) => p,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع جدولا محسوبا لكن أعطيت {}",
                    display,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let get = |k: &str| tables_row_get(pairs, k);
    let cols_v = match get("أعمدة") {
        Some(v) => v,
        None => {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع جدولا محسوبا بمفاتيح «أعمدة» و«صفوف» و«صيغ»",
                    display
                ),
                line,
                col,
            ))
        }
    };
    let rows_v = get("صفوف").ok_or_else(|| {
        RioError::runtime(
            format!(
                "الدالة {} تتوقع جدولا محسوبا بمفاتيح «أعمدة» و«صفوف» و«صيغ»",
                display
            ),
            line,
            col,
        )
    })?;
    let formulas_v = get("صيغ").ok_or_else(|| {
        RioError::runtime(
            format!(
                "الدالة {} تتوقع جدولا محسوبا بمفاتيح «أعمدة» و«صفوف» و«صيغ»",
                display
            ),
            line,
            col,
        )
    })?;
    let cols_list = need_list(display, cols_v, line, col)?;
    let mut cols: Vec<String> = Vec::new();
    for c in cols_list {
        let s = need_string(display, c, line, col)?;
        if s.is_empty() {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع أسماء أعمدة غير فارغة", display),
                line,
                col,
            ));
        }
        if cols.contains(&s) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع أسماء أعمدة بلا تكرار لكن «{}» مكرر", display, s),
                line,
                col,
            ));
        }
        cols.push(s);
    }
    if cols.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع قائمة أعمدة غير فارغة", display),
            line,
            col,
        ));
    }
    let rows_list = need_list(display, rows_v, line, col)?;
    let mut rows: Vec<Vec<(String, Value)>> = Vec::new();
    for (i, item) in rows_list.iter().enumerate() {
        let row = match item {
            Value::Dict(p) => p.clone(),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع جدولا (قائمة قواميس) لكن الصف {} هو {}",
                        display,
                        i + 1,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        };
        for (k, _) in &row {
            if !cols.contains(k) {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع أعمدة معروفة في الصفوف لكن الصف {} فيه عمود «{}» غير معروف",
                        display,
                        i + 1,
                        k
                    ),
                    line,
                    col,
                ));
            }
        }
        rows.push(row);
    }
    let formulas_dict = need_dict(display, formulas_v, line, col)?;
    let mut formulas: Vec<(String, String, Vec<String>)> = Vec::new();
    for (name, fexpr) in formulas_dict {
        if !cols.contains(name) {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع صيغا لأعمدة موجودة لكن «{}» غير موجود",
                    display, name
                ),
                line,
                col,
            ));
        }
        let expr = match fexpr {
            Value::Str(s) => s.clone(),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع صيغة نصا لكن صيغة «{}» هي {}",
                        display,
                        name,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        };
        let ast = j15_parse(&expr).map_err(|_| {
            RioError::runtime(
                format!(
                    "الدالة {} تتوقع صيغة صحيحة لكن «{}» غير صالحة",
                    display, expr
                ),
                line,
                col,
            )
        })?;
        let mut vars = Vec::new();
        j15_ast_vars(&ast, &mut vars);
        for var in &vars {
            if !cols.contains(var) {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع عمودا موجودا لكن «{}» غير موجود في الصيغة",
                        display, var
                    ),
                    line,
                    col,
                ));
            }
            if var == name {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع صيغة بلا اعتماد على نفسها لكن «{}» تعتمد على نفسها",
                        display, name
                    ),
                    line,
                    col,
                ));
            }
        }
        formulas.push((name.clone(), expr, vars));
    }
    let order = j15_topo_order(&formulas).map_err(|_| {
        RioError::runtime(
            format!(
                "الدالة {} تتوقع صيغا بلا اعتماد دائري لكن الصيغ تحوي دورة",
                display
            ),
            line,
            col,
        )
    })?;
    Ok(J15Comp { cols, rows, formulas, order })
}

/// احسب كل خلايا الصيغ في كل الصفوف (بالترتيب
/// الطوبولوجي)؛ والخلية المرجعية غير العددية خطأ.
fn j15_compute_rows(
    display: &str,
    comp: &J15Comp,
    line: usize,
    col: usize,
) -> RioResult<Vec<Vec<(String, Value)>>> {
    let formula_names: Vec<&str> = comp.formulas.iter().map(|(n, _, _)| n.as_str()).collect();
    let mut out: Vec<Vec<(String, Value)>> = Vec::new();
    for (ri, row) in comp.rows.iter().enumerate() {
        // الخلايا الأساس (كل ما ليس عمود صيغة).
        let mut new_row: Vec<(String, Value)> = row
            .iter()
            .filter(|(k, _)| !formula_names.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // قيّم الصيغ بالترتيب؛ وكل صيغة تُلحق فور حسابها.
        for &fi in &comp.order {
            let (name, expr, vars) = &comp.formulas[fi];
            let ast = j15_parse(expr).map_err(|_| {
                RioError::runtime(
                    format!(
                        "الدالة {} تتوقع صيغة صحيحة لكن «{}» غير صالحة",
                        display, expr
                    ),
                    line,
                    col,
                )
            })?;
            let mut num_vars: Vec<(String, f64)> = Vec::new();
            for var in vars {
                match j15_cell(&new_row, var) {
                    Some(Value::Number(x)) => num_vars.push((var.clone(), *x)),
                    _ => {
                        return Err(RioError::runtime(
                            format!(
                                "الدالة {} تتوقع قيمة عددية في العمود «{}» في الصف {}",
                                display,
                                var,
                                ri + 1
                            ),
                            line,
                            col,
                        ))
                    }
                }
            }
            match j15_eval(&ast, &num_vars) {
                Ok(v) => new_row.push((name.clone(), Value::Number(v))),
                Err(J15ExprErr::DivZero) => {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع مقسوما عليه غير صفر في «{}»",
                            display, expr
                        ),
                        line,
                        col,
                    ))
                }
                Err(_) => {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع صيغة صحيحة لكن «{}» غير صالحة",
                            display, expr
                        ),
                        line,
                        col,
                    ))
                }
            }
        }
        out.push(new_row);
    }
    Ok(out)
}

/// قيمة جدول محسوب (قاموس) من مكوناته بعد الحساب.
fn j15_comp_value(comp: &J15Comp, rows: Vec<Vec<(String, Value)>>) -> Value {
    Value::Dict(vec![
        (
            "أعمدة".to_string(),
            Value::List(comp.cols.iter().map(|c| Value::Str(c.clone())).collect()),
        ),
        (
            "صفوف".to_string(),
            Value::List(rows.into_iter().map(Value::Dict).collect()),
        ),
        (
            "صيغ".to_string(),
            Value::Dict(
                comp.formulas
                    .iter()
                    .map(|(n, e, _)| (n.clone(), Value::Str(e.clone())))
                    .collect(),
            ),
        ),
    ])
}

/// 228: جدول_محسوب(أعمدة، صفوف، صيغ) — أنشئ جدولا
/// خلاياه المحسوبة بصيغ تُعاد عند كل تحديث.
fn tables_computed(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.جدول_محسوب";
    need_args(D, args, 3, line, col)?;
    let draft = Value::Dict(vec![
        ("أعمدة".to_string(), args[0].clone()),
        ("صفوف".to_string(), args[1].clone()),
        ("صيغ".to_string(), args[2].clone()),
    ]);
    let comp = j15_read_comp(D, &draft, line, col)?;
    let rows = j15_compute_rows(D, &comp, line, col)?;
    Ok(j15_comp_value(&comp, rows))
}

/// 228: حدث_خلية(جدول_محسوب، رقم_صف، عمود، قيمة) —
/// حدّث خلية أساس وأعد حساب صيغ صفوف الجدول كلها.
/// رقم الصف من 1؛ وعمود الصيغة لا يُحدث مباشرة.
fn tables_update_cell(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.حدث_خلية";
    need_args(D, args, 4, line, col)?;
    let comp = j15_read_comp(D, &args[0], line, col)?;
    let row_no = need_integer(D, &args[1], line, col)?;
    if row_no < 1 || row_no as usize > comp.rows.len() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع رقما بين 1 و {} لكن أعطيت {}",
                D,
                comp.rows.len(),
                row_no
            ),
            line,
            col,
        ));
    }
    let column = need_string(D, &args[2], line, col)?;
    if !comp.cols.contains(&column) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عمودا موجودا لكن «{}» غير موجود",
                D, column
            ),
            line,
            col,
        ));
    }
    if comp.formulas.iter().any(|(n, _, _)| n == &column) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عمودا أساسيا لكن «{}» عمود محسوب",
                D, column
            ),
            line,
            col,
        ));
    }
    let mut comp = comp;
    let idx = (row_no - 1) as usize;
    match comp.rows[idx].iter_mut().find(|(k, _)| k == &column) {
        Some((_, v)) => *v = args[3].clone(),
        None => comp.rows[idx].push((column, args[3].clone())),
    }
    let rows = j15_compute_rows(D, &comp, line, col)?;
    Ok(j15_comp_value(&comp, rows))
}

/// 228: أعد_حساب(جدول_محسوب) — أعد حساب كل خلايا
/// الصيغ في كل الصفوف.
fn tables_recompute(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.أعد_حساب";
    need_args(D, args, 1, line, col)?;
    let comp = j15_read_comp(D, &args[0], line, col)?;
    let rows = j15_compute_rows(D, &comp, line, col)?;
    Ok(j15_comp_value(&comp, rows))
}

/// 228: صفوف_محسوبة(جدول_محسوب) — صفوف الجدول
/// العادي (قائمة قواميس) لاستعمال دوال جداول عليها.
fn tables_computed_rows(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.صفوف_محسوبة";
    need_args(D, args, 1, line, col)?;
    let comp = j15_read_comp(D, &args[0], line, col)?;
    let rows = j15_compute_rows(D, &comp, line, col)?;
    Ok(Value::List(rows.into_iter().map(Value::Dict).collect()))
}

/// 228: صيغة_عمود(جدول_محسوب، عمود) — صيغة العمود
/// أو نص فارغ إن كان أساسيا.
fn tables_formula_of(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.صيغة_عمود";
    need_args(D, args, 2, line, col)?;
    let comp = j15_read_comp(D, &args[0], line, col)?;
    let column = need_string(D, &args[1], line, col)?;
    if !comp.cols.contains(&column) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عمودا موجودا لكن «{}» غير موجود",
                D, column
            ),
            line,
            col,
        ));
    }
    match comp.formulas.iter().find(|(n, _, _)| n == &column) {
        Some((_, expr, _)) => Ok(Value::Str(expr.clone())),
        None => Ok(Value::Str(String::new())),
    }
}

/// 228: اعتماديات(جدول_محسوب، عمود) — أعمدة تعتمد
/// عليها صيغة العمود بترتيب الظهور؛ والأساسي بلا
/// اعتماديات.
fn tables_deps(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.اعتماديات";
    need_args(D, args, 2, line, col)?;
    let comp = j15_read_comp(D, &args[0], line, col)?;
    let column = need_string(D, &args[1], line, col)?;
    if !comp.cols.contains(&column) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عمودا موجودا لكن «{}» غير موجود",
                D, column
            ),
            line,
            col,
        ));
    }
    match comp.formulas.iter().find(|(n, _, _)| n == &column) {
        Some((_, _, vars)) => Ok(Value::List(
            vars.iter().map(|v| Value::Str(v.clone())).collect(),
        )),
        None => Ok(Value::List(Vec::new())),
    }
}

// ---------- 230: معالجة نصوص بالجمل (نص) ----------

/// 230: قسم_جمل(نص) — قسم إلى جمل.
fn text_split_sentences(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.قسم_جمل";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    Ok(Value::List(
        j15_split_sentences(&text)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

/// 230: عدد_الجمل(نص) — عدد الجمل.
fn text_sentence_count(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.عدد_الجمل";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    Ok(Value::Number(j15_split_sentences(&text).len() as f64))
}

/// 230: جملة(نص، رقم) — الجملة رقمها من 1.
fn text_sentence_at(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.جملة";
    need_args(D, args, 2, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let n = need_integer(D, &args[1], line, col)?;
    let sentences = j15_split_sentences(&text);
    if n < 1 || n as usize > sentences.len() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع رقما بين 1 و {} لكن أعطيت {}",
                D,
                sentences.len(),
                n
            ),
            line,
            col,
        ));
    }
    Ok(Value::Str(sentences[(n - 1) as usize].clone()))
}

/// 230: طبق_على_جمل(نص، دالة) — طبق الدالة على كل
/// جملة وأرجع قائمة النتائج.
fn text_apply_sentences(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.طبق_على_جمل";
    need_args(D, args, 2, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for s in j15_split_sentences(&text) {
        out.push(j3_call_single(ctx, &args[1], Value::Str(s), line, col)?);
    }
    Ok(Value::List(out))
}

/// 230: رشح_جمل(نص، كلمة) — جمل تحوي الكلمة؛
/// والكلمة الفارغة خطأ.
fn text_filter_sentences(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.رشح_جمل";
    need_args(D, args, 2, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let word = need_string(D, &args[1], line, col)?;
    if word.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع كلمة غير فارغة", D),
            line,
            col,
        ));
    }
    Ok(Value::List(
        j15_split_sentences(&text)
            .into_iter()
            .filter(|s| s.contains(word.as_str()))
            .map(Value::Str)
            .collect(),
    ))
}

/// 230: أطول_جملة(نص) — أطول جملة (الأولى عند
/// التعادل)؛ والنص بلا جمل يعطي نصا فارغا.
fn text_longest_sentence(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.أطول_جملة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let mut best: Option<String> = None;
    for s in j15_split_sentences(&text) {
        let longer = match &best {
            None => true,
            Some(b) => s.chars().count() > b.chars().count(),
        };
        if longer {
            best = Some(s);
        }
    }
    Ok(Value::Str(best.unwrap_or_default()))
}

/// 230: أقصر_جملة(نص) — أقصر جملة (الأولى عند
/// التعادل)؛ والنص بلا جمل يعطي نصا فارغا.
fn text_shortest_sentence(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "نص.أقصر_جملة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let mut best: Option<String> = None;
    for s in j15_split_sentences(&text) {
        let shorter = match &best {
            None => true,
            Some(b) => s.chars().count() < b.chars().count(),
        };
        if shorter {
            best = Some(s);
        }
    }
    Ok(Value::Str(best.unwrap_or_default()))
}

// ---------- 240: برمجة لغوية — لغات مصغرة (وحدة لغات) ----------

/// 240: عرف_قاعدة(نص) — حلّل «اسم = تعبير» إلى قاموس
/// {الاسم، التعبير، المتغيرات}.
fn langs_define_rule(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.عرف_قاعدة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    match j15_parse_rule(&text) {
        Ok((name, expr, vars)) => Ok(j15_rule_value(&name, &expr, &vars)),
        Err(_) => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع قاعدة «اسم = تعبير» صحيحة لكن «{}» غير صالحة",
                D, text
            ),
            line,
            col,
        )),
    }
}

/// 240: قيم_قاعدة(قاعدة، قيم) — قيّم القاعدة بقاموس
/// المتغيرات.
fn langs_eval_rule(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.قيم_قاعدة";
    need_args(D, args, 2, line, col)?;
    let rule = need_dict(D, &args[0], line, col)?;
    let values = need_dict(D, &args[1], line, col)?;
    j15_eval_rule_pairs(D, rule, values, line, col)
}

/// 240: عرف_لغة(نص_قواعد) — قواعد سطرا بسطر (السطر
/// الفارغ يُتخطى، و# تعليق) إلى قاموس اسم ← قاعدة.
fn langs_define_lang(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.عرف_لغة";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let mut out: Vec<(String, Value)> = Vec::new();
    for raw in text.lines() {
        let ln = raw.trim();
        if ln.is_empty() || ln.starts_with('#') {
            continue;
        }
        match j15_parse_rule(ln) {
            Ok((name, expr, vars)) => {
                if out.iter().any(|(n, _)| n == &name) {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع أسماء قواعد بلا تكرار لكن «{}» مكرر",
                            D, name
                        ),
                        line,
                        col,
                    ));
                }
                out.push((name.clone(), j15_rule_value(&name, &expr, &vars)));
            }
            Err(_) => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع قاعدة «اسم = تعبير» صحيحة لكن السطر «{}» غير صالح",
                        D, ln
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(Value::Dict(out))
}

/// 240: نفذ_قاعدة(لغة، اسم، قيم) — قيّم القاعدة
/// المسماة في اللغة.
fn langs_run_rule(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.نفذ_قاعدة";
    need_args(D, args, 3, line, col)?;
    let lang = need_dict(D, &args[0], line, col)?;
    let name = need_string(D, &args[1], line, col)?;
    let values = need_dict(D, &args[2], line, col)?;
    let rule = match tables_row_get(lang, &name) {
        Some(Value::Dict(p)) => p.clone(),
        _ => {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع قاعدة موجودة لكن «{}» غير موجودة",
                    D, name
                ),
                line,
                col,
            ))
        }
    };
    j15_eval_rule_pairs(D, &rule, values, line, col)
}

/// 240: أسماء_القواعد(لغة) — أسماء القواعد بترتيب
/// تعريفها.
fn langs_rule_names(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.أسماء_القواعد";
    need_args(D, args, 1, line, col)?;
    let lang = need_dict(D, &args[0], line, col)?;
    Ok(Value::List(
        lang.iter().map(|(k, _)| Value::Str(k.clone())).collect(),
    ))
}

/// 240: حلل_تعبير(نص) — تحقق من التعبير وأرجع قاموس
/// {الرموز، المتغيرات}.
fn langs_parse_expr(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "لغات.حلل_تعبير";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    let toks = j15_tokenize(&text).map_err(|_| {
        RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", D, text),
            line,
            col,
        )
    })?;
    if toks.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", D, text),
            line,
            col,
        ));
    }
    let mut p = J15Parser { toks: toks.clone(), pos: 0 };
    let ast = p.parse_expr().map_err(|_| {
        RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", D, text),
            line,
            col,
        )
    })?;
    if p.pos != p.toks.len() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع تعبيرا صحيحا لكن «{}» غير صالح", D, text),
            line,
            col,
        ));
    }
    let mut vars = Vec::new();
    j15_ast_vars(&ast, &mut vars);
    Ok(Value::Dict(vec![
        (
            "الرموز".to_string(),
            Value::List(toks.iter().map(|t| Value::Str(j15_tok_text(t))).collect()),
        ),
        (
            "المتغيرات".to_string(),
            Value::List(vars.iter().map(|v| Value::Str(v.clone())).collect()),
        ),
    ]))
}
// END J15-GEN rust-impl

fn tables_entries() -> Vec<(&'static str, Value)> {
    vec![
        // BEGIN J11-GEN rust-entries
        ("رتب_جدولا", native("رتب_جدولا", tables_sort_by_column)),
        ("رتب_بأعمدة", native("رتب_بأعمدة", tables_sort_by_columns)),
        ("رتب_الأعمدة", native("رتب_الأعمدة", tables_reorder_columns)),
        ("اشتق_عمود", native("اشتق_عمود", tables_derive_column)),
        ("حدث_عمود", native("حدث_عمود", tables_update_column)),
        ("قارن_قائمتين", native("قارن_قائمتين", tables_compare_lists)),
        ("قارن_جدولين", native("قارن_جدولين", tables_compare_tables)),
        ("حلل_csv", native("حلل_csv", tables_parse_csv)),
        ("كون_csv", native("كون_csv", tables_build_csv)),
        ("اقرأ_csv", native("اقرأ_csv", tables_read_csv)),
        ("اكتب_csv", native("اكتب_csv", tables_write_csv)),
        ("طبق_قواعد", native("طبق_قواعد", tables_apply_rules)),
        ("جدول_قرارات", native("جدول_قرارات", tables_decision_table)),
// END J11-GEN rust-entries
// BEGIN J15-GEN rust-entries-tables

// J15 (225 و228): استعلام وجداول محسوبة.
        ("حيث", native("حيث", tables_where)),
        ("اختر", native("اختر", tables_select)),
        ("جمع_حسب", native("جمع_حسب", tables_group_agg)),
        ("رشح_بدالة", native("رشح_بدالة", tables_filter_fn)),
        ("أول_صفوف", native("أول_صفوف", tables_head)),
        ("جدول_محسوب", native("جدول_محسوب", tables_computed)),
        ("حدث_خلية", native("حدث_خلية", tables_update_cell)),
        ("أعد_حساب", native("أعد_حساب", tables_recompute)),
        ("صفوف_محسوبة", native("صفوف_محسوبة", tables_computed_rows)),
        ("صيغة_عمود", native("صيغة_عمود", tables_formula_of)),
        ("اعتماديات", native("اعتماديات", tables_deps)),
// END J15-GEN rust-entries-tables

    ]
}
// BEGIN J12-GEN rust-entries

fn charts_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("ارسم_مخططا", native("ارسم_مخططا", charts_plot)),
        ("ارسم_مخططا_متراكما", native("ارسم_مخططا_متراكما", charts_stacked)),
        ("ارسم_مخططا_شمسيا", native("ارسم_مخططا_شمسيا", charts_sunburst)),
        ("ارسم_شبكة", native("ارسم_شبكة", charts_grid)),
        ("ارسم_مدرجا", native("ارسم_مدرجا", charts_histogram)),
        ("ارسم_صندوقا", native("ارسم_صندوقا", charts_boxplot)),
    ]
}

fn images_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("اكتب_bmp", native("اكتب_bmp", images_write_bmp)),
        ("اقرأ_bmp", native("اقرأ_bmp", images_read_bmp)),
        ("اكتب_png", native("اكتب_png", images_write_png)),
        ("حول_bmp_إلى_png", native("حول_bmp_إلى_png", images_bmp_to_png)),
        ("أنشئ_gif", native("أنشئ_gif", images_make_gif)),
        ("بصمة_ملف", native("بصمة_ملف", images_file_hash)),
    ]
}

fn media_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("ولد_صوتا", native("ولد_صوتا", media_make_tone)),
        ("حول_صوتا", native("حول_صوتا", media_convert_tone)),
        ("اقرأ_صوتا", native("اقرأ_صوتا", media_read_tone)),
    ]
}

fn sim_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("حاك_حياة", native("حاك_حياة", sim_life)),
    ]
}

// END J12-GEN rust-entries
// BEGIN J13-GEN rust-entries

fn pages_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("هرب", native("هرب", pages_escape)),
        ("عنصر", native("عنصر", pages_element)),
        ("صفحة", native("صفحة", pages_page)),
        ("قائمة", native("قائمة", pages_list)),
        ("قائمة_مرتبة", native("قائمة_مرتبة", pages_ordered_list)),
        ("جدول", native("جدول", pages_table)),
        ("رابط", native("رابط", pages_link)),
        ("صورة", native("صورة", pages_image)),
    ]
}

fn mail_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("كون_رسالة", native("كون_رسالة", mail_build)),
        ("حلل_رسالة", native("حلل_رسالة", mail_parse)),
        ("أمر_من", native("أمر_من", mail_cmd_from)),
        ("أمر_إلى", native("أمر_إلى", mail_cmd_to)),
        ("أمر_بيانات", native("أمر_بيانات", mail_cmd_data)),
        ("حلل_رد", native("حلل_رد", mail_parse_reply)),
        ("حلل_رد_pop3", native("حلل_رد_pop3", mail_parse_pop3)),
    ]
}

fn chat_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("كون_رسالة", native("كون_رسالة", chat_build)),
        ("حلل_رسالة", native("حلل_رسالة", chat_parse)),
        ("أمر_انضمام", native("أمر_انضمام", chat_join)),
        ("أمر_مغادرة", native("أمر_مغادرة", chat_leave)),
        ("اسم_صالح", native("اسم_صالح", chat_name_ok)),
    ]
}

fn ftp_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("أمر_دخول", native("أمر_دخول", ftp_login)),
        ("أمر_استرجاع", native("أمر_استرجاع", ftp_retr)),
        ("أمر_تخزين", native("أمر_تخزين", ftp_stor)),
        ("أمر_قائمة", native("أمر_قائمة", ftp_list_cmd)),
        ("أمر_إنهاء", native("أمر_إنهاء", ftp_quit)),
        ("حلل_رد", native("حلل_رد", ftp_parse_reply)),
        ("حلل_قائمة", native("حلل_قائمة", ftp_parse_list)),
    ]
}

fn ntp_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("كون_حزمة", native("كون_حزمة", ntp_packet)),
        ("حلل_حزمة", native("حلل_حزمة", ntp_parse)),
        ("حول_ثواني", native("حول_ثواني", ntp_to_unix)),
        ("حزمة_صحيحة", native("حزمة_صحيحة", ntp_valid)),
    ]
}

// END J13-GEN rust-entries
// BEGIN J15-GEN rust-entries-langs

fn langs_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("عرف_قاعدة", native("عرف_قاعدة", langs_define_rule)),
        ("قيم_قاعدة", native("قيم_قاعدة", langs_eval_rule)),
        ("عرف_لغة", native("عرف_لغة", langs_define_lang)),
        ("نفذ_قاعدة", native("نفذ_قاعدة", langs_run_rule)),
        ("أسماء_القواعد", native("أسماء_القواعد", langs_rule_names)),
        ("حلل_تعبير", native("حلل_تعبير", langs_parse_expr)),
    ]
}

// END J15-GEN rust-entries-langs

/// جدول: قائمة صفوف، كل صف قاموس. القائمة الفارغة جدول فارغ.
fn tables_need_table(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<Vec<(String, Value)>>> {
    let items = need_list(display, v, line, col)?;
    let mut rows = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Dict(pairs) => rows.push(pairs.clone()),
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع جدولا (قائمة قواميس) لكن الصف {} هو {}",
                        display,
                        i + 1,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    Ok(rows)
}

fn tables_row_get<'a>(
    row: &'a [(String, Value)],
    key: &str,
) -> Option<&'a Value> {
    row.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// مساواة عميقة تتجاهل ترتيب مفاتيح القواميس وعناصر المجموعات.
fn tables_value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Dict(pa), Value::Dict(pb)) => {
            if pa.len() != pb.len() {
                return false;
            }
            pa.iter().all(|(k, v)| match tables_row_get(pb, k) {
                Some(w) => tables_value_eq(v, w),
                None => false,
            })
        }
        (Value::List(xa), Value::List(xb))
        | (Value::Tuple(xa), Value::Tuple(xb)) => {
            xa.len() == xb.len()
                && xa.iter().zip(xb.iter()).all(|(x, y)| tables_value_eq(x, y))
        }
        (Value::Set(xa), Value::Set(xb)) => {
            xa.len() == xb.len()
                && xa.iter().all(|x| xb.iter().any(|y| tables_value_eq(x, y)))
        }
        _ => a == b,
    }
}

/// مقارنة قيمتي عمود للترتيب: عددان عدديا، ونصان معجميا؛
/// وغير ذلك خطأ عربي. القيمة الفارغة (لاشيء) خطأ.
fn tables_cmp_cell(
    display: &str,
    a: &Value,
    b: &Value,
    line: usize,
    col: usize,
) -> RioResult<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            Ok(x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        }
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع قيما من نوع واحد في العمود (أعداد أو نصوص) لكن أعطيت {} و{}",
                display,
                a.type_name(),
                b.type_name()
            ),
            line,
            col,
        )),
    }
}

fn tables_dir(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    // صحيح = تنازلي.
    let s = need_string(display, v, line, col)?;
    match s.as_str() {
        "تصاعدي" => Ok(false),
        "تنازلي" => Ok(true),
        _ => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع اتجاها «تصاعدي» أو «تنازلي» لكن أعطيت «{}»",
                display, s
            ),
            line,
            col,
        )),
    }
}

/// 168: رتب_جدولا(جدول، عمود، اتجاه؟) — ترتيب مستقر
/// لصفوف الجدول بقيمة عمود واحد؛ والاتجاه اختياري
/// (تصاعدي افتراضا). العمود الغائب أو الفارغ خطأ.
fn tables_sort_by_column(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.رتب_جدولا";
    if args.len() != 2 && args.len() != 3 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 2 أو 3 معاملات لكن أعطيت {}", D, args.len()),
            line,
            col,
        ));
    }
    let column = need_string(D, &args[1], line, col)?;
    let desc = if args.len() == 3 {
        tables_dir(D, &args[2], line, col)?
    } else {
        false
    };
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut keys: Vec<Value> = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        match tables_row_get(row, &column) {
            Some(Value::Nil) | None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                        D, column, i + 1
                    ),
                    line,
                    col,
                ))
            }
            Some(v) => keys.push(v.clone()),
        }
    }
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let mut err: Option<RioError> = None;
    idx.sort_by(|&a, &b| {
        if err.is_some() {
            return std::cmp::Ordering::Equal;
        }
        match tables_cmp_cell(D, &keys[a], &keys[b], line, col) {
            Ok(o) => {
                if desc {
                    o.reverse()
                } else {
                    o
                }
            }
            Err(e) => {
                err = Some(e);
                std::cmp::Ordering::Equal
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    // أعيد ترتيب الصفوف الأصلية حسب idx (مستقر لأن sort_by مستقر).
    let mut out: Vec<Value> = Vec::with_capacity(rows.len());
    for i in idx {
        out.push(Value::Dict(rows[i].clone()));
    }
    Ok(Value::List(out))
}

/// 168: رتب_بأعمدة(جدول، أعمدة) — ترتيب بمفاتيح متعددة:
/// كل مواصفة نص (عمود تصاعديا) أو قاموس {عمود، اتجاه؟}.
fn tables_sort_by_columns(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.رتب_بأعمدة";
    need_args(D, args, 2, line, col)?;
    let specs = need_list(D, &args[1], line, col)?;
    if specs.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع قائمة أعمدة غير فارغة", D),
            line,
            col,
        ));
    }
    let mut keys: Vec<(String, bool)> = Vec::new();
    for spec in specs {
        match spec {
            Value::Str(name) => keys.push((name.clone(), false)),
            Value::Dict(pairs) => {
                let name = match tables_row_get(pairs, "عمود") {
                    Some(Value::Str(s)) => s.clone(),
                    _ => {
                        return Err(RioError::typ(
                            format!(
                                "الدالة {} تتوقع مواصفة فيها «عمود» نصا",
                                D
                            ),
                            line,
                            col,
                        ))
                    }
                };
                let desc = match tables_row_get(pairs, "اتجاه") {
                    Some(v) => tables_dir(D, v, line, col)?,
                    None => false,
                };
                keys.push((name, desc));
            }
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع مواصفات نصا أو قاموسا لكن أعطيت {}",
                        D,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        }
    }
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let mut err: Option<RioError> = None;
    idx.sort_by(|&a, &b| {
        if err.is_some() {
            return std::cmp::Ordering::Equal;
        }
        for (name, desc) in &keys {
            let va = tables_row_get(&rows[a], name);
            let vb = tables_row_get(&rows[b], name);
            match (va, vb) {
                (Some(Value::Nil) | None, _) | (_, Some(Value::Nil) | None) => {
                    err = Some(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع العمود «{}» في كل صف",
                            D, name
                        ),
                        line,
                        col,
                    ));
                    return std::cmp::Ordering::Equal;
                }
                (Some(x), Some(y)) => match tables_cmp_cell(D, x, y, line, col) {
                    Ok(std::cmp::Ordering::Equal) => continue,
                    Ok(o) => {
                        return if *desc { o.reverse() } else { o };
                    }
                    Err(e) => {
                        err = Some(e);
                        return std::cmp::Ordering::Equal;
                    }
                },
            }
        }
        std::cmp::Ordering::Equal
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(Value::List(
        idx.into_iter().map(|i| Value::Dict(rows[i].clone())).collect(),
    ))
}

/// 168: رتب_الأعمدة(جدول، أعمدة) — إعادة ترتيب أعمدة
/// الجدول (واختيارها): صفوف جديدة بمفاتيحها بالترتيب المعطى.
fn tables_reorder_columns(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.رتب_الأعمدة";
    need_args(D, args, 2, line, col)?;
    let cols = need_list(D, &args[1], line, col)?;
    let mut names: Vec<String> = Vec::new();
    for c in cols {
        let s = need_string(D, c, line, col)?;
        if names.contains(&s) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع أعمدة بلا تكرار لكن «{}» مكرر", D, s),
                line,
                col,
            ));
        }
        names.push(s);
    }
    let rows = tables_need_table(D, &args[0], line, col)?;
    let mut out: Vec<Value> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let mut new_row: Vec<(String, Value)> = Vec::new();
        for name in &names {
            match tables_row_get(row, name) {
                Some(v) => new_row.push((name.clone(), v.clone())),
                None => {
                    return Err(RioError::runtime(
                        format!(
                            "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                            D, name, i + 1
                        ),
                        line,
                        col,
                    ))
                }
            }
        }
        out.push(Value::Dict(new_row));
    }
    Ok(Value::List(out))
}

/// 170: اشتق_عمود(جدول، اسم، دالة) — عمود مشتق جديد:
/// الدالة تستلم الصف (قاموسا) وتعيد قيمة الخلية.
/// العمود الموجود أصلا خطأ (له حدث_عمود).
fn tables_derive_column(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.اشتق_عمود";
    need_args(D, args, 3, line, col)?;
    let name = need_string(D, &args[1], line, col)?;
    let rows = tables_need_table(D, &args[0], line, col)?;
    for row in &rows {
        if tables_row_get(row, &name).is_some() {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع عمودا جديدا لكن «{}» موجود أصلا",
                    D, name
                ),
                line,
                col,
            ));
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for row in &rows {
        let v = j3_call_single(ctx, &args[2], Value::Dict(row.clone()), line, col)?;
        let mut new_row = row.clone();
        new_row.push((name.clone(), v));
        out.push(Value::Dict(new_row));
    }
    Ok(Value::List(out))
}

/// 170: حدث_عمود(جدول، اسم، دالة) — إعادة حساب عمود
/// موجود: الدالة تستلم الصف وتعيد القيمة الجديدة.
/// العمود الغائب خطأ.
fn tables_update_column(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.حدث_عمود";
    need_args(D, args, 3, line, col)?;
    let name = need_string(D, &args[1], line, col)?;
    let rows = tables_need_table(D, &args[0], line, col)?;
    for (i, row) in rows.iter().enumerate() {
        if tables_row_get(row, &name).is_none() {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع العمود «{}» في كل صف لكن الصف {} بلا قيمة",
                    D, name, i + 1
                ),
                line,
                col,
            ));
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for row in &rows {
        let v = j3_call_single(ctx, &args[2], Value::Dict(row.clone()), line, col)?;
        let new_row: Vec<(String, Value)> = row
            .iter()
            .map(|(k, old)| {
                if k == &name {
                    (k.clone(), v.clone())
                } else {
                    (k.clone(), old.clone())
                }
            })
            .collect();
        out.push(Value::Dict(new_row));
    }
    Ok(Value::List(out))
}

/// 171: قارن_قائمتين(قديمة، جديدة) — فروق موضعية:
/// قائمة قواميس {نوع، فهرس، قديم، جديد}؛ والنوع
/// مغير أو مضاف أو محذوف، والغائب لاشيء.
fn tables_compare_lists(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.قارن_قائمتين";
    need_args(D, args, 2, line, col)?;
    let old = need_list(D, &args[0], line, col)?;
    let new = need_list(D, &args[1], line, col)?;
    let n = old.len().max(new.len());
    let mut out: Vec<Value> = Vec::new();
    for i in 0..n {
        let o = old.get(i);
        let w = new.get(i);
        match (o, w) {
            (Some(x), Some(y)) => {
                if !tables_value_eq(x, y) {
                    out.push(Value::Dict(vec![
                        ("نوع".to_string(), Value::Str("مغير".to_string())),
                        ("فهرس".to_string(), Value::Number(i as f64)),
                        ("قديم".to_string(), x.clone()),
                        ("جديد".to_string(), y.clone()),
                    ]));
                }
            }
            (Some(x), None) => {
                out.push(Value::Dict(vec![
                    ("نوع".to_string(), Value::Str("محذوف".to_string())),
                    ("فهرس".to_string(), Value::Number(i as f64)),
                    ("قديم".to_string(), x.clone()),
                    ("جديد".to_string(), Value::Nil),
                ]));
            }
            (None, Some(y)) => {
                out.push(Value::Dict(vec![
                    ("نوع".to_string(), Value::Str("مضاف".to_string())),
                    ("فهرس".to_string(), Value::Number(i as f64)),
                    ("قديم".to_string(), Value::Nil),
                    ("جديد".to_string(), y.clone()),
                ]));
            }
            (None, None) => {}
        }
    }
    Ok(Value::List(out))
}

/// 171: قارن_جدولين(قديم، جديد، مفتاح) — فروق بين
/// نسختي جدول بمطابقة الصفوف على عمود المفتاح:
/// قائمة قواميس {نوع، مفتاح، قديم، جديد}؛ والمفتاح
/// المكرر في أي جدول خطأ، وقيمته عدد أو نص.
fn tables_compare_tables(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.قارن_جدولين";
    need_args(D, args, 3, line, col)?;
    let key_col = need_string(D, &args[2], line, col)?;
    let old_rows = tables_need_table(D, &args[0], line, col)?;
    let new_rows = tables_need_table(D, &args[1], line, col)?;
    let mut old_keys: Vec<(String, Value)> = Vec::new();
    for (i, row) in old_rows.iter().enumerate() {
        let k = tables_key_value(D, row, &key_col, i, line, col)?;
        if old_keys.iter().any(|(s, _)| *s == k.0) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع مفتاحا بلا تكرار لكن «{}» مكرر في الجدول القديم", D, k.0),
                line,
                col,
            ));
        }
        old_keys.push(k);
    }
    let mut new_keys: Vec<(String, Value)> = Vec::new();
    for (i, row) in new_rows.iter().enumerate() {
        let k = tables_key_value(D, row, &key_col, i, line, col)?;
        if new_keys.iter().any(|(s, _)| *s == k.0) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع مفتاحا بلا تكرار لكن «{}» مكرر في الجدول الجديد", D, k.0),
                line,
                col,
            ));
        }
        new_keys.push(k);
    }
    let mut out: Vec<Value> = Vec::new();
    for (oi, (ks, kv)) in old_keys.iter().enumerate() {
        match new_keys.iter().position(|(s, _)| s == ks) {
            None => {
                out.push(tables_diff("محذوف", kv.clone(), Value::Dict(old_rows[oi].clone()), Value::Nil));
            }
            Some(ni) => {
                let old_row = Value::Dict(old_rows[oi].clone());
                let new_row = Value::Dict(new_rows[ni].clone());
                if !tables_value_eq(&old_row, &new_row) {
                    out.push(tables_diff("مغير", kv.clone(), old_row, new_row));
                }
            }
        }
    }
    for (ni, (ks, kv)) in new_keys.iter().enumerate() {
        if !old_keys.iter().any(|(s, _)| s == ks) {
            out.push(tables_diff("مضاف", kv.clone(), Value::Nil, Value::Dict(new_rows[ni].clone())));
        }
    }
    Ok(Value::List(out))
}

/// قيمة مفتاح صف: نصها المطبوع للفهرسة + القيمة نفسها.
/// المفتاح الغائب أو الفارغ أو غير العدد والنص خطأ.
fn tables_key_value(
    display: &str,
    row: &[(String, Value)],
    key_col: &str,
    i: usize,
    line: usize,
    col: usize,
) -> RioResult<(String, Value)> {
    match tables_row_get(row, key_col) {
        Some(Value::Number(_)) | Some(Value::Str(_)) => {
            let v = tables_row_get(row, key_col).unwrap().clone();
            Ok((v.fmt_value(), v))
        }
        _ => Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع المفتاح «{}» عددا أو نصا في كل صف لكن الصف {} بلا قيمة صالحة",
                display, key_col, i + 1
            ),
            line,
            col,
        )),
    }
}

fn tables_diff(kind: &str, key: Value, old: Value, new: Value) -> Value {
    Value::Dict(vec![
        ("نوع".to_string(), Value::Str(kind.to_string())),
        ("مفتاح".to_string(), key),
        ("قديم".to_string(), old),
        ("جديد".to_string(), new),
    ])
}

// ---------- 173: CSV ----------
// التحليل صيغة RFC 4180 المبسطة: اقتباس بمضاعفة
// علامة الاقتباس، وفاصل فاصلة، وسطر جديد \n أو \r\n،
// وسطور فارغة تُتخطى. الصف الأول ترويسة (مفاتيح
// بلا تكرار وبلا اسم فارغ)، وعدد حقول كل صف يجب
// أن يساوي عدد الترويسة. قيمة الحقل: فارغ ← نص
// فارغ؛ صحيح/خطأ ← منطقي؛ وعدد (غربي أو عربي
// الأرقام مع ٫) ← عدد؛ وغير ذلك نص.
// الكتابة بالتنسيق المطبوع نفسه (fmt_value) فيرد
// التحليل الأعداد والمنطقيات كما كانت؛ والفارغ
// (لاشيء) ← حقل فارغ؛ والقيمة المركبة خطأ.

/// حلل نص CSV إلى قائمة صفوف (قواميس بمفاتيح الترويسة).
fn tables_parse_csv_text(
    display: &str,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<Value>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut field = String::new();
    let mut row: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut field_quoted = false;
    let mut field_started = false;
    // حالة سطر فارغ: سطر بلا أي حرف يُتخطى؛ أما
    // السطر «,» فصف من حقلين فارغين فيُحفظ.
    let mut line_has_content = false;
    macro_rules! end_field {
        () => {
            row.push(std::mem::take(&mut field));
            field_quoted = false;
            field_started = false;
        };
    }
    macro_rules! end_row {
        () => {
            if line_has_content {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
            line_has_content = false;
        };
    }
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if !field_started => {
                in_quotes = true;
                field_quoted = true;
                field_started = true;
                line_has_content = true;
            }
            '"' => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع علامة اقتباس في أول الحقل لا وسطه",
                        display
                    ),
                    line,
                    col,
                ));
            }
            ',' => {
                end_field!();
                line_has_content = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                end_field!();
                end_row!();
            }
            '\n' => {
                end_field!();
                end_row!();
            }
            _ => {
                field.push(c);
                field_started = true;
                line_has_content = true;
            }
        }
    }
    if in_quotes {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع إغلاق الاقتباس في نهاية النص", display),
            line,
            col,
        ));
    }
    if field_started || field_quoted || !row.is_empty() {
        end_field!();
        end_row!();
    }
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let header = rows[0].clone();
    for (i, h) in header.iter().enumerate() {
        if h.is_empty() {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع ترويسة بلا اسم فارغ لكن العمود {} فارغ",
                    display,
                    i + 1
                ),
                line,
                col,
            ));
        }
        if header[..i].contains(h) {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع ترويسة بلا تكرار لكن «{}» مكرر",
                    display, h
                ),
                line,
                col,
            ));
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for (ri, r) in rows.iter().skip(1).enumerate() {
        if r.len() != header.len() {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع {} حقولا في الصف {} لكن أعطيت {}",
                    display,
                    header.len(),
                    ri + 2,
                    r.len()
                ),
                line,
                col,
            ));
        }
        let mut pairs: Vec<(String, Value)> = Vec::new();
        for (h, f) in header.iter().zip(r.iter()) {
            pairs.push((h.clone(), tables_csv_field_value(f)));
        }
        out.push(Value::Dict(pairs));
    }
    Ok(out)
}

/// قيمة حقل CSV نصية إلى قيمة نيفا.
fn tables_csv_field_value(s: &str) -> Value {
    if s.is_empty() {
        return Value::Str(String::new());
    }
    if s == "صحيح" {
        return Value::Bool(true);
    }
    if s == "خطأ" {
        return Value::Bool(false);
    }
    if let Some(n) = tables_parse_csv_number(s) {
        return Value::Number(n);
    }
    Value::Str(s.to_string())
}

/// عدد CSV: أرقام غربية أو عربية (٠-٩) مع نقطة
/// غربية أو ٫، وإشارة سالبة وأس اختياري.
fn tables_parse_csv_number(s: &str) -> Option<f64> {
    let norm: String = s
        .chars()
        .map(|c| match c {
            '٠'..='٩' => ((c as u32 - '٠' as u32) as u8 + b'0') as char,
            '٫' => '.',
            other => other,
        })
        .collect();
    let b = norm.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0;
    if b[i] == b'-' {
        i += 1;
        if i >= b.len() {
            return None;
        }
    }
    let mut digits = 0;
    let mut dot = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == b'.' && !dot {
            dot = true;
        } else {
            break;
        }
        i += 1;
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut edigits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            edigits += 1;
            i += 1;
        }
        if edigits == 0 {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let n: f64 = norm.parse().ok()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// خلية إلى نص CSV (مع الاقتباس عند الحاجة).
fn tables_csv_cell_text(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<String> {
    let raw = match v {
        Value::Nil => String::new(),
        Value::Str(s) => s.clone(),
        Value::Number(_) | Value::Bool(_) => v.fmt_value(),
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {} تتوقع خلايا عددا أو نصا أو منطقيا أو فارغا لكن أعطيت {}",
                    display,
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    if raw.contains(',') || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
        Ok(format!("\"{}\"", raw.replace('"', "\"\"")))
    } else {
        Ok(raw)
    }
}

/// جدول إلى نص CSV: الترويسة مفاتيح الصف الأول
/// بالترتيب، وكل صف بعده بمفاتيح المجموعة نفسها.
fn tables_build_csv_text(
    display: &str,
    rows: &[Vec<(String, Value)>],
    line: usize,
    col: usize,
) -> RioResult<String> {
    if rows.is_empty() {
        return Ok(String::new());
    }
    let header: Vec<String> = rows[0].iter().map(|(k, _)| k.clone()).collect();
    let mut out = String::new();
    out.push_str(&tables_csv_line(&header)?);
    out.push('\n');
    for (i, row) in rows.iter().enumerate() {
        if row.len() != header.len()
            || header.iter().any(|h| tables_row_get(row, h).is_none())
        {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع كل صف بمفاتيح الترويسة نفسها لكن الصف {} يخالفها",
                    display,
                    i + 1
                ),
                line,
                col,
            ));
        }
        let cells: Vec<String> = header
            .iter()
            .map(|h| tables_csv_cell_text(display, tables_row_get(row, h).unwrap(), line, col))
            .collect::<RioResult<Vec<String>>>()?;
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    Ok(out)
}

fn tables_csv_line(cells: &[String]) -> RioResult<String> {
    let mut parts: Vec<String> = Vec::new();
    for raw in cells {
        if raw.contains(',') || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
            parts.push(format!("\"{}\"", raw.replace('"', "\"\"")));
        } else {
            parts.push(raw.clone());
        }
    }
    Ok(parts.join(","))
}

/// 173: حلل_csv(نص) — نص CSV إلى جدول.
fn tables_parse_csv(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.حلل_csv";
    need_args(D, args, 1, line, col)?;
    let text = need_string(D, &args[0], line, col)?;
    Ok(Value::List(tables_parse_csv_text(D, &text, line, col)?))
}

/// 173: كون_csv(جدول) — جدول إلى نص CSV.
fn tables_build_csv(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.كون_csv";
    need_args(D, args, 1, line, col)?;
    let rows = tables_need_table(D, &args[0], line, col)?;
    Ok(Value::Str(tables_build_csv_text(D, &rows, line, col)?))
}

/// 173: اقرأ_csv(مسار) — قراءة ملف CSV وتحليله.
/// المسار النسبي مقابل مجلد الجلسة (كأنماط_من_ملف).
fn tables_read_csv(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.اقرأ_csv";
    need_args(D, args, 1, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let full = search_resolve_path(ctx, &path);
    let content = std::fs::read_to_string(&full).map_err(|_| {
        RioError::runtime(format!("تعذر قراءة الملف {}", path), line, col)
    })?;
    Ok(Value::List(tables_parse_csv_text(D, &content, line, col)?))
}

/// 173: اكتب_csv(مسار، جدول) — كتابة جدول ملف CSV.
/// كتابة ذرية (كاكتب_ملف).
fn tables_write_csv(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.اكتب_csv";
    need_args(D, args, 2, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let rows = tables_need_table(D, &args[1], line, col)?;
    let text = tables_build_csv_text(D, &rows, line, col)?;
    let full = search_resolve_path(ctx, &path);
    crate::install::write_atomic(&full, text.as_bytes()).map_err(|e| {
        RioError::runtime(format!("تعذر كتابة الملف {}: {}", path, e), line, col)
    })?;
    Ok(Value::Nil)
}

// ---------- 174: قواعد الأعمال ----------

/// 174: طبق_قواعد(قواعد، سجل) — أول قاعدة يصدق
/// شرطها على السجل يعيد نتيجتها؛ وبلا مطابقة لاشيء.
/// القاعدة قاموس {شرط: دالة(سجل)→منطقي، نتيجة: قيمة}.
fn tables_apply_rules(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.طبق_قواعد";
    need_args(D, args, 2, line, col)?;
    let rules = need_list(D, &args[0], line, col)?;
    let record = need_dict(D, &args[1], line, col)?.clone();
    for (i, rule) in rules.iter().enumerate() {
        let pairs = match rule {
            Value::Dict(p) => p,
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع قواعد قواميس لكن القاعدة {} هي {}",
                        D,
                        i + 1,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        };
        let cond = match tables_row_get(pairs, "شرط") {
            Some(v) => v.clone(),
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع «شرط» في كل قاعدة لكن القاعدة {} بلا شرط",
                        D,
                        i + 1
                    ),
                    line,
                    col,
                ))
            }
        };
        let result = match tables_row_get(pairs, "نتيجة") {
            Some(v) => v.clone(),
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع «نتيجة» في كل قاعدة لكن القاعدة {} بلا نتيجة",
                        D,
                        i + 1
                    ),
                    line,
                    col,
                ))
            }
        };
        if j3_call_bool(ctx, &cond, Value::Dict(record.clone()), D, line, col)? {
            return Ok(result);
        }
    }
    Ok(Value::Nil)
}

/// 174: جدول_قرارات(قرارات، سجل) — جدول قرارات:
/// كل صف قاموس فيه «نتيجة» وشروط (باقي المفاتيح)؛
/// والصف يطابق حين تساوي كل شروطه قيم السجل؛
/// والصف بلا شروط (نتيجة وحدها) افتراضي يطابق دائما.
/// أول مطابقة تفوز؛ وبلا مطابقة لاشيء.
fn tables_decision_table(
    _ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "جداول.جدول_قرارات";
    need_args(D, args, 2, line, col)?;
    let rows = need_list(D, &args[0], line, col)?;
    let record = need_dict(D, &args[1], line, col)?;
    for (i, row) in rows.iter().enumerate() {
        let pairs = match row {
            Value::Dict(p) => p,
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع صفوف قرارات قواميس لكن الصف {} هو {}",
                        D,
                        i + 1,
                        other.type_name()
                    ),
                    line,
                    col,
                ))
            }
        };
        let result = match tables_row_get(pairs, "نتيجة") {
            Some(v) => v.clone(),
            None => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع «نتيجة» في كل صف لكن الصف {} بلا نتيجة",
                        D,
                        i + 1
                    ),
                    line,
                    col,
                ))
            }
        };
        let mut matches = true;
        for (k, v) in pairs {
            if k == "نتيجة" {
                continue;
            }
            match tables_row_get(record, k) {
                Some(w) if tables_value_eq(v, w) => {}
                _ => {
                    matches = false;
                    break;
                }
            }
        }
        if matches {
            return Ok(result);
        }
    }
    Ok(Value::Nil)
}

// ================= J12 (يدوي): الرسوميات والوسائط — الدفعة الأخيرة من الـ579 (§88) =================
// مخرجات ملفات فقط (SVG/BMP/PNG/GIF/WAV) بلا أي نافذة تفاعلية أبدا؛
// وكل الهندسة والموجات بأعداد صحيحة (جدول الجيب المولّد J12_SIN_TABLE)
// فتطابق المرآة C بايتًا بايتًا. بلا صناديق خارجية إطلاقا.
// المؤجل بقرار موثق: 176 و183 (OpenGL)، و185 (تشغيل صوت — لا مسار
// تشغيل في المحرك)، وJPEG كتابة/قراءة، وقراءة PNG (تحتاج مفكك DEFLATE).

const J12_PALETTE: [&str; 10] = [
    "#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2", "#7f7f7f",
    "#bcbd22", "#17becf",
];

fn j12_milli(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

fn j12_need_numbers(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<f64>> {
    let items = need_list(display, v, line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let x = need_number(display, item, line, col)?;
        if !x.is_finite() || x.abs() > 1.0e9 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع أعدادا محدودة ضمن المدى", display),
                line,
                col,
            ));
        }
        out.push(x);
    }
    Ok(out)
}

fn j12_need_millis(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<i64>> {
    Ok(j12_need_numbers(display, v, line, col)?
        .iter()
        .map(|&x| j12_milli(x))
        .collect())
}

fn j12_need_labels(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<String>> {
    let items = need_list(display, v, line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(need_string(display, item, line, col)?);
    }
    Ok(out)
}

fn j12_need_int(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    need_integer(display, v, line, col)
}

fn j12_svg_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn j12_write_file(
    ctx: &mut dyn HandlerCaller,
    path: &str,
    bytes: &[u8],
    line: usize,
    col: usize,
) -> RioResult<()> {
    let full = search_resolve_path(ctx, path);
    crate::install::write_atomic(&full, bytes).map_err(|e| {
        RioError::runtime(format!("تعذر كتابة الملف {}: {}", path, e), line, col)
    })
}

fn j12_read_file(
    ctx: &mut dyn HandlerCaller,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Vec<u8>> {
    let full = search_resolve_path(ctx, path);
    std::fs::read(&full)
        .map_err(|_| RioError::runtime(format!("تعذر قراءة الملف {}", path), line, col))
}

fn j12_sin1024(a: i64) -> i64 {
    J12_SIN_TABLE[(a & 1023) as usize] as i64
}

fn j12_cos1024(a: i64) -> i64 {
    J12_SIN_TABLE[((a + 256) & 1023) as usize] as i64
}

fn j12_point(cx: i64, cy: i64, r: i64, a: i64) -> (i64, i64) {
    (
        cx + r * j12_cos1024(a) / 32767,
        cy - r * j12_sin1024(a) / 32767,
    )
}

fn j12_svg_open(w: i64, h: i64, title: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n<text x=\"{}\" y=\"24\" text-anchor=\"middle\" font-size=\"18\">{}</text>\n",
        w,
        h,
        w,
        h,
        w / 2,
        j12_svg_escape(title)
    )
}

fn j12_svg_axes(s: &mut String) {
    s.push_str("<line x1=\"60\" y1=\"370\" x2=\"620\" y2=\"370\" stroke=\"#000000\"/>\n");
    s.push_str("<line x1=\"60\" y1=\"20\" x2=\"60\" y2=\"370\" stroke=\"#000000\"/>\n");
}

fn j12_line_x(i: i64, n: i64) -> i64 {
    if n == 1 {
        340
    } else {
        60 + i * 560 / (n - 1)
    }
}

fn j12_line_y(v: i64, mn: i64, mx: i64) -> i64 {
    if mx == mn {
        195
    } else {
        370 - (v - mn) * 330 / (mx - mn)
    }
}

fn j12_pie_slice(
    cx: i64,
    cy: i64,
    r: i64,
    a0: i64,
    span: i64,
    color: &str,
    out: &mut String,
) {
    if span == 1024 {
        out.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>\n",
            cx, cy, r, color
        ));
        return;
    }
    let (x0, y0) = j12_point(cx, cy, r, a0);
    let (x1, y1) = j12_point(cx, cy, r, a0 + span);
    let large = if span > 512 { 1 } else { 0 };
    out.push_str(&format!(
        "<path d=\"M {} {} L {} {} A {} {} 0 {} 1 {} {} Z\" fill=\"{}\"/>\n",
        cx, cy, x0, y0, r, r, large, x1, y1, color
    ));
}

fn j12_ring_sector(
    cx: i64,
    cy: i64,
    r0: i64,
    r1: i64,
    a0: i64,
    span: i64,
    color: &str,
    out: &mut String,
) {
    if span == 1024 {
        j12_ring_sector(cx, cy, r0, r1, a0, 512, color, out);
        j12_ring_sector(cx, cy, r0, r1, a0 + 512, 512, color, out);
        return;
    }
    let (ox0, oy0) = j12_point(cx, cy, r1, a0);
    let (ox1, oy1) = j12_point(cx, cy, r1, a0 + span);
    let (ix1, iy1) = j12_point(cx, cy, r0, a0 + span);
    let (ix0, iy0) = j12_point(cx, cy, r0, a0);
    let large = if span > 512 { 1 } else { 0 };
    out.push_str(&format!(
        "<path d=\"M {} {} A {} {} 0 {} 1 {} {} L {} {} A {} {} 0 {} 0 {} {} Z\" fill=\"{}\"/>\n",
        ox0, oy0, r1, r1, large, ox1, oy1, ix1, iy1, r0, r0, large, ix0, iy0, color
    ));
}

fn j12_plot_line(title: &str, labels: &[String], values: &[i64]) -> String {
    let n = values.len() as i64;
    let mn = *values.iter().min().unwrap();
    let mx = *values.iter().max().unwrap();
    let mut s = j12_svg_open(640, 420, title);
    j12_svg_axes(&mut s);
    let mut pts = String::new();
    for (i, &v) in values.iter().enumerate() {
        if i > 0 {
            pts.push(' ');
        }
        pts.push_str(&format!(
            "{},{}",
            j12_line_x(i as i64, n),
            j12_line_y(v, mn, mx)
        ));
    }
    s.push_str(&format!(
        "<polyline points=\"{}\" fill=\"none\" stroke=\"#1f77b4\" stroke-width=\"2\"/>\n",
        pts
    ));
    for (i, &v) in values.iter().enumerate() {
        s.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"#1f77b4\"/>\n",
            j12_line_x(i as i64, n),
            j12_line_y(v, mn, mx)
        ));
    }
    for (i, label) in labels.iter().enumerate() {
        s.push_str(&format!(
            "<text x=\"{}\" y=\"395\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            j12_line_x(i as i64, n),
            j12_svg_escape(label)
        ));
    }
    s.push_str("</svg>\n");
    s
}

fn j12_plot_bars(title: &str, labels: &[String], values: &[i64]) -> String {
    let n = values.len() as i64;
    let mn = *values.iter().min().unwrap();
    let mx = *values.iter().max().unwrap();
    let lo = mn.min(0);
    let hi = mx.max(0);
    let slot = 560 / n;
    let bw = slot * 7 / 10;
    let mut s = j12_svg_open(640, 420, title);
    j12_svg_axes(&mut s);
    for (i, &v) in values.iter().enumerate() {
        let x = 60 + (i as i64) * slot + slot * 3 / 20;
        let h = if hi == lo {
            0
        } else {
            (v - lo) * 330 / (hi - lo)
        };
        s.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            x,
            370 - h,
            bw,
            h,
            J12_PALETTE[i % 10]
        ));
    }
    for (i, label) in labels.iter().enumerate() {
        let x = 60 + (i as i64) * slot + slot / 2;
        s.push_str(&format!(
            "<text x=\"{}\" y=\"395\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            x,
            j12_svg_escape(label)
        ));
    }
    s.push_str("</svg>\n");
    s
}

fn j12_plot_pie(
    display: &str,
    title: &str,
    labels: &[String],
    values: &[i64],
    line: usize,
    col: usize,
) -> RioResult<String> {
    for &v in values {
        if v < 0 {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع قيما غير سالبة للمخطط الدائري",
                    display
                ),
                line,
                col,
            ));
        }
    }
    let total: i64 = values.iter().sum();
    if total <= 0 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع مجموع قيم موجبا للمخطط الدائري", display),
            line,
            col,
        ));
    }
    let mut s = j12_svg_open(640, 420, title);
    let mut start: i64 = 0;
    let mut acc: i64 = 0;
    for (i, &v) in values.iter().enumerate() {
        let span = if i == values.len() - 1 {
            1024 - acc
        } else {
            1024 * v / total
        };
        acc += span;
        if span == 0 {
            continue;
        }
        j12_pie_slice(320, 210, 150, start, span, J12_PALETTE[i % 10], &mut s);
        let mid = start + span / 2;
        let (lx, ly) = j12_point(320, 210, 185, mid);
        s.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            lx,
            ly,
            j12_svg_escape(&labels[i])
        ));
        start += span;
    }
    s.push_str("</svg>\n");
    Ok(s)
}
fn j12_dict_int(
    display: &str,
    pairs: &[(String, Value)],
    key: &str,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    match dict_get(pairs, key) {
        Some(v) => need_integer(display, v, line, col),
        None => Err(RioError::runtime(
            format!("الدالة {} تتوقع «{}» في كل شكل", display, key),
            line,
            col,
        )),
    }
}

fn j12_dict_color(pairs: &[(String, Value)]) -> String {
    match dict_get(pairs, "لون") {
        Some(Value::Str(s)) => s.clone(),
        _ => "#000000".to_string(),
    }
}

/// 181: ارسم_شبكة(مسار، عرض، ارتفاع، أشكال) — شبكة 20 بكسل +
/// أشكال قواميس: دائرة {س، ص، نق}، مستطيل {س، ص، عرض، ارتفاع}،
/// خط {س، ص، س٢، ص٢}، نقطة {س، ص}، نص {س، ص، نص}؛ واللون اختياري.
fn charts_grid(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_شبكة";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let w = j12_need_int(D, &args[1], line, col)?;
    let h = j12_need_int(D, &args[2], line, col)?;
    if !(1..=2000).contains(&w) || !(1..=2000).contains(&h) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع العرض والارتفاع ضمن 1..2000", D),
            line,
            col,
        ));
    }
    let shapes = need_list(D, &args[3], line, col)?;
    if shapes.len() > 10000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ≤ 10000 شكل", D),
            line,
            col,
        ));
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        w, h, w, h
    );
    let mut x = 20i64;
    while x < w {
        svg.push_str(&format!(
            "<line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"#e0e0e0\"/>\n",
            x, x, h
        ));
        x += 20;
    }
    let mut y = 20i64;
    while y < h {
        svg.push_str(&format!(
            "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#e0e0e0\"/>\n",
            y, w, y
        ));
        y += 20;
    }
    svg.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#000000\"/>\n",
        w, h
    ));
    for item in shapes {
        let pairs = match item {
            Value::Dict(p) => p,
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع أشكالا قواميس لكن أعطيت {}",
                        D,
                        other.type_name()
                    ),
                    line,
                    col,
                ));
            }
        };
        let kind = match dict_get(pairs, "شكل") {
            Some(Value::Str(s)) => s.clone(),
            _ => {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع «شكل» نصا في كل شكل", D),
                    line,
                    col,
                ));
            }
        };
        let color = j12_dict_color(pairs);
        match kind.as_str() {
            "دائرة" => {
                let cx = j12_dict_int(D, pairs, "س", line, col)?;
                let cy = j12_dict_int(D, pairs, "ص", line, col)?;
                let r = j12_dict_int(D, pairs, "نق", line, col)?;
                if r < 0 {
                    return Err(RioError::runtime(
                        format!("الدالة {} تتوقع نصف قطر غير سالب", D),
                        line,
                        col,
                    ));
                }
                svg.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>\n",
                    cx, cy, r, color
                ));
            }
            "مستطيل" => {
                let x = j12_dict_int(D, pairs, "س", line, col)?;
                let y = j12_dict_int(D, pairs, "ص", line, col)?;
                let ww = j12_dict_int(D, pairs, "عرض", line, col)?;
                let hh = j12_dict_int(D, pairs, "ارتفاع", line, col)?;
                if ww < 0 || hh < 0 {
                    return Err(RioError::runtime(
                        format!("الدالة {} تتوقع أبعادا غير سالبة", D),
                        line,
                        col,
                    ));
                }
                svg.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
                    x, y, ww, hh, color
                ));
            }
            "خط" => {
                let x1 = j12_dict_int(D, pairs, "س", line, col)?;
                let y1 = j12_dict_int(D, pairs, "ص", line, col)?;
                let x2 = j12_dict_int(D, pairs, "س٢", line, col)?;
                let y2 = j12_dict_int(D, pairs, "ص٢", line, col)?;
                svg.push_str(&format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                    x1, y1, x2, y2, color
                ));
            }
            "نقطة" => {
                let cx = j12_dict_int(D, pairs, "س", line, col)?;
                let cy = j12_dict_int(D, pairs, "ص", line, col)?;
                svg.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"{}\"/>\n",
                    cx, cy, color
                ));
            }
            "نص" => {
                let x = j12_dict_int(D, pairs, "س", line, col)?;
                let y = j12_dict_int(D, pairs, "ص", line, col)?;
                let text = match dict_get(pairs, "نص") {
                    Some(Value::Str(s)) => s.clone(),
                    _ => {
                        return Err(RioError::runtime(
                            format!("الدالة {} تتوقع «نص» نصا لشكل النص", D),
                            line,
                            col,
                        ));
                    }
                };
                svg.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-size=\"14\" fill=\"{}\">{}</text>\n",
                    x,
                    y,
                    color,
                    j12_svg_escape(&text)
                ));
            }
            _ => {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع شكلا من (دائرة، مستطيل، خط، نقطة، نص) لكن أعطيت {}",
                        D, kind
                    ),
                    line,
                    col,
                ));
            }
        }
    }
    svg.push_str("</svg>\n");
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

/// 182: ارسم_مدرجا(مسار، عنوان، قيم، فئات) — مدرج تكراري.
fn charts_histogram(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_مدرجا";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let title = need_string(D, &args[1], line, col)?;
    let values = j12_need_millis(D, &args[2], line, col)?;
    let bins = j12_need_int(D, &args[3], line, col)?;
    if values.is_empty() || values.len() > 100000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..100000 قيمة", D),
            line,
            col,
        ));
    }
    if !(1..=100).contains(&bins) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع الفئات ضمن 1..100", D),
            line,
            col,
        ));
    }
    let mn = *values.iter().min().unwrap();
    let mx = *values.iter().max().unwrap();
    let mut counts = vec![0i64; bins as usize];
    for &v in &values {
        let idx = if mx == mn {
            0
        } else {
            (v - mn) * bins / (mx - mn + 1)
        };
        counts[idx as usize] += 1;
    }
    let mc = *counts.iter().max().unwrap();
    let slot = 560 / bins;
    let bw = slot * 7 / 10;
    let mut svg = j12_svg_open(640, 420, &title);
    j12_svg_axes(&mut svg);
    for (i, &c) in counts.iter().enumerate() {
        let x = 60 + (i as i64) * slot + slot * 3 / 20;
        let h = if mc == 0 { 0 } else { c * 330 / mc };
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            x,
            370 - h,
            bw,
            h,
            J12_PALETTE[i % 10]
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"395\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            x + bw / 2,
            i
        ));
    }
    svg.push_str("</svg>\n");
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

fn j12_median(sorted: &[i64]) -> i64 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    }
}

/// 182: ارسم_صندوقا(مسار، عنوان، تسميات، مجموعات) — مخطط صندوقي
/// (وسيط وربيعان وطرفان) لكل مجموعة.
fn charts_boxplot(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_صندوقا";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let title = need_string(D, &args[1], line, col)?;
    let labels = j12_need_labels(D, &args[2], line, col)?;
    let raw = need_list(D, &args[3], line, col)?;
    if labels.is_empty() || labels.len() > 50 || labels.len() != raw.len() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..50 مجموعة بتسميات مساوية لها", D),
            line,
            col,
        ));
    }
    let mut groups: Vec<Vec<i64>> = Vec::with_capacity(raw.len());
    for g in raw {
        let vals = j12_need_millis(D, g, line, col)?;
        if vals.is_empty() || vals.len() > 10000 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل مجموعة 1..10000 قيمة", D),
                line,
                col,
            ));
        }
        groups.push(vals);
    }
    let mut gmn = i64::MAX;
    let mut gmx = i64::MIN;
    for g in &groups {
        for &v in g {
            if v < gmn {
                gmn = v;
            }
            if v > gmx {
                gmx = v;
            }
        }
    }
    let y_of = |v: i64| -> i64 {
        if gmx == gmn {
            195
        } else {
            370 - (v - gmn) * 330 / (gmx - gmn)
        }
    };
    let g = groups.len() as i64;
    let slot = 560 / g;
    let bw = slot * 6 / 10;
    let mut svg = j12_svg_open(640, 420, &title);
    j12_svg_axes(&mut svg);
    for (i, grp) in groups.iter().enumerate() {
        let mut s = grp.clone();
        s.sort_unstable();
        let n = s.len();
        let med = j12_median(&s);
        let q1 = j12_median(&s[..n / 2]);
        let q3 = j12_median(&s[(n + 1) / 2..]);
        let (q1, q3) = if n == 1 { (med, med) } else { (q1, q3) };
        let cx = 60 + (i as i64) * slot + slot / 2;
        let x0 = cx - bw / 2;
        let x1 = x0 + bw;
        svg.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000000\"/>\n",
            cx,
            y_of(s[n - 1]),
            cx,
            y_of(s[0])
        ));
        svg.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000000\"/>\n",
            x0,
            y_of(s[n - 1]),
            x1,
            y_of(s[n - 1])
        ));
        svg.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000000\"/>\n",
            x0,
            y_of(s[0]),
            x1,
            y_of(s[0])
        ));
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"#000000\"/>\n",
            x0,
            y_of(q3),
            bw,
            y_of(q1) - y_of(q3),
            J12_PALETTE[i % 10]
        ));
        svg.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#000000\" stroke-width=\"2\"/>\n",
            x0,
            y_of(med),
            x1,
            y_of(med)
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"395\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            cx,
            j12_svg_escape(&labels[i])
        ));
    }
    svg.push_str("</svg>\n");
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

/// 177: ارسم_مخططا(مسار، نوع، عنوان، تسميات، قيم) — نوع خط/أعمدة/دائري.
/// مخرج SVG نصي حتمي بإحداثيات صحيحة.
fn charts_plot(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_مخططا";
    need_args(D, args, 5, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let kind = need_string(D, &args[1], line, col)?;
    let title = need_string(D, &args[2], line, col)?;
    let labels = j12_need_labels(D, &args[3], line, col)?;
    let values = j12_need_millis(D, &args[4], line, col)?;
    if values.is_empty() || values.len() > 10000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..10000 قيمة", D),
            line,
            col,
        ));
    }
    if labels.len() != values.len() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع عدد التسميات مساويا لعدد القيم", D),
            line,
            col,
        ));
    }
    let svg = match kind.as_str() {
        "خط" => j12_plot_line(&title, &labels, &values),
        "أعمدة" => j12_plot_bars(&title, &labels, &values),
        "دائري" => j12_plot_pie(D, &title, &labels, &values, line, col)?,
        _ => {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع نوعا من (خط، أعمدة، دائري) لكن أعطيت {}",
                    D, kind
                ),
                line,
                col,
            ));
        }
    };
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

/// 179: ارسم_مخططا_متراكما(مسار، عنوان، تسميات، أسماء، سلاسل) —
/// أعمدة متراكمة؛ كل سلسلة قائمة أعداد بطول التسميات؛ القيم غير سالبة.
fn charts_stacked(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_مخططا_متراكما";
    need_args(D, args, 5, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let title = need_string(D, &args[1], line, col)?;
    let labels = j12_need_labels(D, &args[2], line, col)?;
    let names = j12_need_labels(D, &args[3], line, col)?;
    let series_raw = need_list(D, &args[4], line, col)?;
    if labels.is_empty() || labels.len() > 1000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..1000 تسمية", D),
            line,
            col,
        ));
    }
    if names.is_empty() || names.len() > 10 || names.len() != series_raw.len() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..10 سلاسل بأسماء مساوية لها", D),
            line,
            col,
        ));
    }
    let mut series: Vec<Vec<i64>> = Vec::with_capacity(series_raw.len());
    for s in series_raw {
        let vals = j12_need_millis(D, s, line, col)?;
        if vals.len() != labels.len() {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل سلسلة بطول التسميات", D),
                line,
                col,
            ));
        }
        for &v in &vals {
            if v < 0 {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع قيما غير سالبة", D),
                    line,
                    col,
                ));
            }
        }
        series.push(vals);
    }
    let m = labels.len() as i64;
    let mut totals = vec![0i64; labels.len()];
    for s in &series {
        for (i, &v) in s.iter().enumerate() {
            totals[i] += v;
        }
    }
    let mx = *totals.iter().max().unwrap();
    let slot = 410 / m;
    let bw = slot * 7 / 10;
    let mut svg = j12_svg_open(640, 420, &title);
    svg.push_str("<line x1=\"60\" y1=\"370\" x2=\"470\" y2=\"370\" stroke=\"#000000\"/>\n");
    svg.push_str("<line x1=\"60\" y1=\"20\" x2=\"60\" y2=\"370\" stroke=\"#000000\"/>\n");
    for (i, _) in labels.iter().enumerate() {
        let x = 60 + (i as i64) * slot + slot * 3 / 20;
        let mut ycur: i64 = 370;
        for (j, s) in series.iter().enumerate() {
            let h = if mx == 0 { 0 } else { s[i] * 330 / mx };
            svg.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
                x,
                ycur - h,
                bw,
                h,
                J12_PALETTE[j % 10]
            ));
            ycur -= h;
        }
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"395\" text-anchor=\"middle\" font-size=\"12\">{}</text>\n",
            x + bw / 2,
            j12_svg_escape(&labels[i])
        ));
    }
    for (j, name) in names.iter().enumerate() {
        let y = 30 + (j as i64) * 24;
        svg.push_str(&format!(
            "<rect x=\"480\" y=\"{}\" width=\"14\" height=\"14\" fill=\"{}\"/>\n",
            y,
            J12_PALETTE[j % 10]
        ));
        svg.push_str(&format!(
            "<text x=\"500\" y=\"{}\" font-size=\"12\">{}</text>\n",
            y + 12,
            j12_svg_escape(name)
        ));
    }
    svg.push_str("</svg>\n");
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

struct J12Node {
    name: String,
    value: i64,
    parent: String,
}

fn j12_sunburst_totals(
    nodes: &[J12Node],
    children: &[Vec<usize>],
    idx: usize,
    totals: &mut [i64],
) -> i64 {
    let mut t = nodes[idx].value;
    for &c in &children[idx] {
        t += j12_sunburst_totals(nodes, children, c, totals);
    }
    totals[idx] = t;
    t
}

fn j12_sunburst_draw(
    nodes: &[J12Node],
    children: &[Vec<usize>],
    totals: &[i64],
    idx: usize,
    depth: usize,
    a0: i64,
    span: i64,
    single_root: bool,
    out: &mut String,
) {
    let color = J12_PALETTE[idx % 10];
    if depth == 0 && single_root {
        out.push_str(&format!(
            "<circle cx=\"320\" cy=\"210\" r=\"28\" fill=\"{}\"/>\n",
            color
        ));
        out.push_str(&format!(
            "<text x=\"320\" y=\"214\" text-anchor=\"middle\" font-size=\"11\">{}</text>\n",
            j12_svg_escape(&nodes[idx].name)
        ));
    } else {
        let inner = 28 + (depth.saturating_sub(1) as i64) * 28;
        let outer = inner + 28;
        j12_ring_sector(320, 210, inner, outer, a0, span, color, out);
        if span >= 96 {
            let mid = a0 + span / 2;
            let (lx, ly) = j12_point(320, 210, (inner + outer) / 2, mid);
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\">{}</text>\n",
                lx,
                ly,
                j12_svg_escape(&nodes[idx].name)
            ));
        }
    }
    let mut acc: i64 = 0;
    let n = children[idx].len();
    for (k, &c) in children[idx].iter().enumerate() {
        let cspan = if k == n - 1 {
            span - acc
        } else if totals[idx] == 0 {
            0
        } else {
            span * totals[c] / totals[idx]
        };
        acc += cspan;
        if cspan == 0 {
            continue;
        }
        j12_sunburst_draw(
            nodes,
            children,
            totals,
            c,
            depth + 1,
            a0 + acc - cspan,
            cspan,
            single_root,
            out,
        );
    }
}

/// 180: ارسم_مخططا_شمسيا(مسار، عنوان، عقد) — عقد قواميس
/// {اسم، قيمة، أب}؛ الأب "" للجذر؛ العمق ≤ 5؛ القيم غير سالبة.
fn charts_sunburst(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مخططات.ارسم_مخططا_شمسيا";
    need_args(D, args, 3, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let title = need_string(D, &args[1], line, col)?;
    let raw = need_list(D, &args[2], line, col)?;
    if raw.is_empty() || raw.len() > 1000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..1000 عقدة", D),
            line,
            col,
        ));
    }
    let mut nodes: Vec<J12Node> = Vec::with_capacity(raw.len());
    for item in raw {
        let pairs = match item {
            Value::Dict(p) => p,
            other => {
                return Err(RioError::typ(
                    format!(
                        "الدالة {} تتوقع عقدا قواميس لكن أعطيت {}",
                        D,
                        other.type_name()
                    ),
                    line,
                    col,
                ));
            }
        };
        let name = match dict_get(pairs, "اسم") {
            Some(Value::Str(s)) => s.clone(),
            _ => {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع «اسم» نصا في كل عقدة", D),
                    line,
                    col,
                ));
            }
        };
        let value = match dict_get(pairs, "قيمة") {
            Some(v) => need_number(D, v, line, col)?,
            None => {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع «قيمة» في كل عقدة", D),
                    line,
                    col,
                ));
            }
        };
        if !value.is_finite() || value.abs() > 1.0e9 || value < 0.0 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع قيما غير سالبة محدودة", D),
                line,
                col,
            ));
        }
        let parent = match dict_get(pairs, "أب") {
            Some(Value::Str(s)) => s.clone(),
            _ => {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع «أب» نصا في كل عقدة", D),
                    line,
                    col,
                ));
            }
        };
        nodes.push(J12Node {
            name,
            value: j12_milli(value),
            parent,
        });
    }
    for (i, n) in nodes.iter().enumerate() {
        for (j, m) in nodes.iter().enumerate() {
            if i != j && n.name == m.name {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع أسماء عقد فريدة", D),
                    line,
                    col,
                ));
            }
        }
        if n.parent == n.name {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع الأب غير العقدة نفسها", D),
                line,
                col,
            ));
        }
        if !n.parent.is_empty() && !nodes.iter().any(|m| m.name == n.parent) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل أب عقدة موجودة", D),
                line,
                col,
            ));
        }
    }
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.parent.is_empty() {
            roots.push(i);
        } else {
            let p = nodes.iter().position(|m| m.name == n.parent).unwrap();
            children[p].push(i);
        }
    }
    if roots.is_empty() {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع جذرا واحدا على الأقل (أب فارغ)", D),
            line,
            col,
        ));
    }
    for i in 0..nodes.len() {
        let mut depth = 0usize;
        let mut cur = i;
        loop {
            if nodes[cur].parent.is_empty() {
                break;
            }
            depth += 1;
            if depth > nodes.len() {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع عقدا بلا دورة", D),
                    line,
                    col,
                ));
            }
            cur = nodes.iter().position(|m| m.name == nodes[cur].parent).unwrap();
        }
        if depth > 5 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع عمق عقد ≤ 5", D),
                line,
                col,
            ));
        }
    }
    let mut totals = vec![0i64; nodes.len()];
    for &r in &roots {
        j12_sunburst_totals(&nodes, &children, r, &mut totals);
    }
    let grand: i64 = roots.iter().map(|&r| totals[r]).sum();
    if grand <= 0 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع مجموع قيم موجبا", D),
            line,
            col,
        ));
    }
    let mut svg = j12_svg_open(640, 420, &title);
    if roots.len() > 1 {
        svg.push_str("<circle cx=\"320\" cy=\"210\" r=\"28\" fill=\"#ffffff\" stroke=\"#000000\"/>\n");
    }
    let single_root = roots.len() == 1;
    let mut acc: i64 = 0;
    for (k, &r) in roots.iter().enumerate() {
        let span = if k == roots.len() - 1 {
            1024 - acc
        } else {
            1024 * totals[r] / grand
        };
        acc += span;
        if span == 0 {
            continue;
        }
        j12_sunburst_draw(
            &nodes,
            &children,
            &totals,
            r,
            0,
            acc - span,
            span,
            single_root,
            &mut svg,
        );
    }
    svg.push_str("</svg>\n");
    j12_write_file(ctx, &path, svg.as_bytes(), line, col)?;
    Ok(Value::Nil)
}

// ---------- J12: صور (178 جزئيا، 188) ----------

fn j12_need_pixels(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<Vec<[u8; 3]>> {
    let items = need_list(display, v, line, col)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let px = need_list(display, item, line, col)?;
        if px.len() != 3 {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع كل بكسل قائمة [أحمر، أخضر، أزرق]",
                    display
                ),
                line,
                col,
            ));
        }
        let mut rgb = [0u8; 3];
        for (k, c) in px.iter().enumerate() {
            let n = need_number(display, c, line, col)?;
            if n.fract() != 0.0 || !(0.0..=255.0).contains(&n) {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع كل قناة لون عددا صحيحا 0..255",
                        display
                    ),
                    line,
                    col,
                ));
            }
            rgb[k] = n as u8;
        }
        out.push(rgb);
    }
    Ok(out)
}

fn j12_check_dims(display: &str, w: i64, h: i64, line: usize, col: usize) -> RioResult<()> {
    if !(1..=2000).contains(&w) || !(1..=2000).contains(&h) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع العرض والارتفاع ضمن 1..2000", display),
            line,
            col,
        ));
    }
    if w * h > 4_000_000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع عدد البكسلات ≤ 4000000", display),
            line,
            col,
        ));
    }
    Ok(())
}

fn j12_bmp_bytes(w: i64, h: i64, px: &[[u8; 3]]) -> Vec<u8> {
    let row = ((w * 3 + 3) / 4) * 4;
    let data_size = row * h;
    let mut out = Vec::with_capacity((54 + data_size) as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + data_size) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&(h as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..h).rev() {
        for x in 0..w {
            let p = px[(y * w + x) as usize];
            out.push(p[2]);
            out.push(p[1]);
            out.push(p[0]);
        }
        for _ in 0..(row - w * 3) {
            out.push(0);
        }
    }
    out
}

fn j12_u16(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() {
        return None;
    }
    Some(data[off] as u16 | ((data[off + 1] as u16) << 8))
}

fn j12_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(
        data[off] as u32
            | ((data[off + 1] as u32) << 8)
            | ((data[off + 2] as u32) << 16)
            | ((data[off + 3] as u32) << 24),
    )
}

fn j12_parse_bmp(
    display: &str,
    data: &[u8],
    line: usize,
    col: usize,
) -> RioResult<(i64, i64, Vec<[u8; 3]>)> {
    let bad = || {
        RioError::runtime(format!("الدالة {} تتوقع ملف BMP صالحا", display), line, col)
    };
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Err(bad());
    }
    let off = j12_u32(data, 10).ok_or_else(bad)? as usize;
    let hsize = j12_u32(data, 14).ok_or_else(bad)?;
    let w = j12_u32(data, 18).ok_or_else(bad)? as i64;
    let h = j12_u32(data, 22).ok_or_else(bad)? as i64;
    let planes = j12_u16(data, 26).ok_or_else(bad)?;
    let bpp = j12_u16(data, 28).ok_or_else(bad)?;
    let comp = j12_u32(data, 30).ok_or_else(bad)?;
    if hsize != 40 || planes != 1 || bpp != 24 || comp != 0 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ملف BMP غير مضغوط 24-بت", display),
            line,
            col,
        ));
    }
    if !(1..=2000).contains(&w) || !(1..=2000).contains(&h) || w * h > 4_000_000 {
        return Err(bad());
    }
    let row = ((w * 3 + 3) / 4) * 4;
    if off + (row * h) as usize > data.len() {
        return Err(bad());
    }
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in (0..h).rev() {
        for x in 0..w {
            let p = off + (y * row + x * 3) as usize;
            px.push([data[p + 2], data[p + 1], data[p]]);
        }
    }
    Ok((w, h, px))
}

/// 178: اكتب_bmp(مسار، عرض، ارتفاع، بكسلات) — بكسلات قوائم [ر، غ، ب].
fn images_write_bmp(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.اكتب_bmp";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let w = j12_need_int(D, &args[1], line, col)?;
    let h = j12_need_int(D, &args[2], line, col)?;
    j12_check_dims(D, w, h, line, col)?;
    let px = j12_need_pixels(D, &args[3], line, col)?;
    if px.len() as i64 != w * h {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عدد البكسلات مساويا للعرض × الارتفاع",
                D
            ),
            line,
            col,
        ));
    }
    let bytes = j12_bmp_bytes(w, h, &px);
    j12_write_file(ctx, &path, &bytes, line, col)?;
    Ok(Value::Nil)
}

/// 178: اقرأ_bmp(مسار) — قاموس {عرض، ارتفاع، بكسلات}.
fn images_read_bmp(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.اقرأ_bmp";
    need_args(D, args, 1, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let data = j12_read_file(ctx, &path, line, col)?;
    let (w, h, px) = j12_parse_bmp(D, &data, line, col)?;
    let pixels = Value::List(
        px.iter()
            .map(|p| {
                Value::List(vec![
                    Value::Number(p[0] as f64),
                    Value::Number(p[1] as f64),
                    Value::Number(p[2] as f64),
                ])
            })
            .collect(),
    );
    Ok(Value::Dict(vec![
        ("عرض".to_string(), Value::Number(w as f64)),
        ("ارتفاع".to_string(), Value::Number(h as f64)),
        ("بكسلات".to_string(), pixels),
    ]))
}

fn j12_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

fn j12_adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn j12_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&j12_crc32(&crc_data).to_be_bytes());
}

fn j12_png_bytes(w: i64, h: i64, px: &[[u8; 3]]) -> Vec<u8> {
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    j12_png_chunk(&mut out, b"IHDR", &ihdr);
    let mut raw = Vec::with_capacity((1 + w as usize * 3) * h as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = px[(y * w + x) as usize];
            raw.push(p[0]);
            raw.push(p[1]);
            raw.push(p[2]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut off = 0usize;
    while off < raw.len() {
        let chunk = std::cmp::min(65535, raw.len() - off);
        let last = off + chunk >= raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(chunk as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk as u16)).to_le_bytes());
        z.extend_from_slice(&raw[off..off + chunk]);
        off += chunk;
    }
    z.extend_from_slice(&j12_adler32(&raw).to_be_bytes());
    j12_png_chunk(&mut out, b"IDAT", &z);
    j12_png_chunk(&mut out, b"IEND", &[]);
    out
}

/// 178: اكتب_png(مسار، عرض، ارتفاع، بكسلات) — PNG بكتل zlib مخزنة.
fn images_write_png(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.اكتب_png";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let w = j12_need_int(D, &args[1], line, col)?;
    let h = j12_need_int(D, &args[2], line, col)?;
    j12_check_dims(D, w, h, line, col)?;
    let px = j12_need_pixels(D, &args[3], line, col)?;
    if px.len() as i64 != w * h {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع عدد البكسلات مساويا للعرض × الارتفاع",
                D
            ),
            line,
            col,
        ));
    }
    let bytes = j12_png_bytes(w, h, &px);
    j12_write_file(ctx, &path, &bytes, line, col)?;
    Ok(Value::Nil)
}

/// 186: حول_bmp_إلى_png(مصدر، هدف) — قراءة BMP وكتابته PNG.
fn images_bmp_to_png(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.حول_bmp_إلى_png";
    need_args(D, args, 2, line, col)?;
    let src = need_string(D, &args[0], line, col)?;
    let dst = need_string(D, &args[1], line, col)?;
    let data = j12_read_file(ctx, &src, line, col)?;
    let (w, h, px) = j12_parse_bmp(D, &data, line, col)?;
    let bytes = j12_png_bytes(w, h, &px);
    j12_write_file(ctx, &dst, &bytes, line, col)?;
    Ok(Value::Nil)
}

fn j12_lzw_emit(out: &mut Vec<u8>, bitbuf: &mut u32, bitcnt: &mut u32, code: u32, size: u32) {
    *bitbuf |= code << *bitcnt;
    *bitcnt += size;
    while *bitcnt >= 8 {
        out.push((*bitbuf & 0xFF) as u8);
        *bitbuf >>= 8;
        *bitcnt -= 8;
    }
}

fn j12_gif_lzw(pixels: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut bitbuf: u32 = 0;
    let mut bitcnt: u32 = 0;
    let mut code_size: u32 = 9;
    let mut next_code: u32 = 258;
    let mut dict: Vec<(u32, u8)> = Vec::new();
    j12_lzw_emit(&mut out, &mut bitbuf, &mut bitcnt, 256, code_size);
    let mut buffer: u32 = pixels[0] as u32;
    for &k in &pixels[1..] {
        let k = k as u32;
        let mut found: Option<u32> = None;
        for (i, &(p, s)) in dict.iter().enumerate() {
            if p == buffer && s as u32 == k {
                found = Some(258 + i as u32);
                break;
            }
        }
        if let Some(code) = found {
            buffer = code;
        } else {
            j12_lzw_emit(&mut out, &mut bitbuf, &mut bitcnt, buffer, code_size);
            if next_code == 4096 {
                j12_lzw_emit(&mut out, &mut bitbuf, &mut bitcnt, 256, code_size);
                dict.clear();
                next_code = 258;
                code_size = 9;
            } else {
                dict.push((buffer, k as u8));
                next_code += 1;
                if next_code == (1 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
            buffer = k;
        }
    }
    j12_lzw_emit(&mut out, &mut bitbuf, &mut bitcnt, buffer, code_size);
    j12_lzw_emit(&mut out, &mut bitbuf, &mut bitcnt, 257, code_size);
    if bitcnt > 0 {
        out.push((bitbuf & 0xFF) as u8);
    }
    out
}

fn j12_gif_bytes(w: i64, h: i64, frames: &[Vec<u8>], delay: i64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.extend_from_slice(&[0xF7, 0, 0]);
    for g in 0..256u16 {
        out.push(g as u8);
        out.push(g as u8);
        out.push(g as u8);
    }
    out.extend_from_slice(&[0x21, 0xFF, 11]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[3, 1, 0, 0, 0]);
    for frame in frames {
        out.extend_from_slice(&[0x21, 0xF9, 4, 0x00]);
        out.extend_from_slice(&(delay as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.push(0x2C);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(w as u16).to_le_bytes());
        out.extend_from_slice(&(h as u16).to_le_bytes());
        out.push(0);
        out.push(8);
        let comp = j12_gif_lzw(frame);
        let mut off = 0usize;
        while off < comp.len() {
            let n = std::cmp::min(255, comp.len() - off);
            out.push(n as u8);
            out.extend_from_slice(&comp[off..off + n]);
            off += n;
        }
        out.push(0);
    }
    out.push(0x3B);
    out
}

/// 188: أنشئ_gif(مسار، عرض، ارتفاع، إطارات، تأخير) — كل إطار قائمة
/// أعداد 0..255 (رمادي) بطول العرض × الارتفاع؛ والتأخير بالمئة من الثانية.
fn images_make_gif(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.أنشئ_gif";
    need_args(D, args, 5, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let w = j12_need_int(D, &args[1], line, col)?;
    let h = j12_need_int(D, &args[2], line, col)?;
    j12_check_dims(D, w, h, line, col)?;
    let raw_frames = need_list(D, &args[3], line, col)?;
    let delay = j12_need_int(D, &args[4], line, col)?;
    if raw_frames.is_empty() || raw_frames.len() > 64 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع 1..64 إطارا", D),
            line,
            col,
        ));
    }
    if !(0..=6000).contains(&delay) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع التأخير ضمن 0..6000", D),
            line,
            col,
        ));
    }
    let mut frames: Vec<Vec<u8>> = Vec::with_capacity(raw_frames.len());
    for f in raw_frames {
        let items = need_list(D, f, line, col)?;
        if items.len() as i64 != w * h {
            return Err(RioError::runtime(
                format!(
                    "الدالة {} تتوقع كل إطار بطول العرض × الارتفاع",
                    D
                ),
                line,
                col,
            ));
        }
        let mut frame = Vec::with_capacity(items.len());
        for v in items {
            let n = need_number(D, v, line, col)?;
            if n.fract() != 0.0 || !(0.0..=255.0).contains(&n) {
                return Err(RioError::runtime(
                    format!("الدالة {} تتوقع كل بكسل عددا صحيحا 0..255", D),
                    line,
                    col,
                ));
            }
            frame.push(n as u8);
        }
        frames.push(frame);
    }
    let bytes = j12_gif_bytes(w, h, &frames, delay);
    j12_write_file(ctx, &path, &bytes, line, col)?;
    Ok(Value::Nil)
}

/// بصمة ملف (SHA-256 ست عشريا) — للتحقق من المخرجات الثنائية
/// بلا تضمين بايتات في الحالات.
fn images_file_hash(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "صور.بصمة_ملف";
    need_args(D, args, 1, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let data = j12_read_file(ctx, &path, line, col)?;
    Ok(Value::Str(crate::crypto::sha256_hex(&data)))
}

// ---------- J12: وسائط (184، 186) ومحاكاة (187) ----------

fn j12_wave_step(freq: f64) -> i64 {
    (freq * 1048576.0 / 44100.0).round() as i64
}

fn j12_wave_sample(wave: &str, step: i64, i: i64) -> i64 {
    let phase = i * step;
    let idx = ((phase >> 10) & 1023) as usize;
    let frac = phase & 1023;
    match wave {
        "جيب" => {
            let a = J12_SIN_TABLE[idx] as i64;
            let b = J12_SIN_TABLE[(idx + 1) & 1023] as i64;
            let s = (a * (1024 - frac) + b * frac) / 1024;
            s * 20000 / 32767
        }
        "مربع" => {
            if idx < 512 {
                20000
            } else {
                -20000
            }
        }
        "منشار" => (idx as i64 * 40000) / 1024 - 20000,
        "مثلث" => {
            if idx < 256 {
                (idx as i64 * 20000) / 256
            } else if idx < 768 {
                20000 - ((idx - 256) as i64 * 20000) / 256
            } else {
                -20000 + ((idx - 768) as i64 * 20000) / 256
            }
        }
        _ => 0,
    }
}

fn j12_wav_bytes(rate: u32, channels: u16, bits: u16, samples: &[i32]) -> Vec<u8> {
    let bytes_per_sample = (bits / 8) as usize;
    let frame_bytes = channels as usize * bytes_per_sample;
    let frames = samples.len() / channels as usize;
    let data_size = frames * frame_bytes;
    let mut out = Vec::with_capacity(44 + data_size);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_size) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * frame_bytes as u32).to_le_bytes());
    out.extend_from_slice(&(frame_bytes as u16).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    for &s in samples {
        if bits == 16 {
            out.extend_from_slice(&(s as i16).to_le_bytes());
        } else {
            out.push((s + 128) as u8);
        }
    }
    out
}

struct J12Wav {
    rate: u32,
    channels: u16,
    bits: u16,
    samples: Vec<i32>,
}

fn j12_parse_wav(
    display: &str,
    data: &[u8],
    line: usize,
    col: usize,
) -> RioResult<J12Wav> {
    let bad = || RioError::runtime(format!("الدالة {} تتوقع ملف WAV صالحا", display), line, col);
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(bad());
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data_off: Option<usize> = None;
    let mut data_len: usize = 0;
    let mut off = 12usize;
    while off + 8 <= data.len() {
        let id = &data[off..off + 4];
        let size = j12_u32(data, off + 4).ok_or_else(bad)? as usize;
        let start = off + 8;
        if start + size > data.len() {
            break;
        }
        if id == b"fmt " {
            if size < 16 {
                return Err(bad());
            }
            let format = j12_u16(data, start).ok_or_else(bad)?;
            let channels = j12_u16(data, start + 2).ok_or_else(bad)?;
            let rate = j12_u32(data, start + 4).ok_or_else(bad)?;
            let bits = j12_u16(data, start + 14).ok_or_else(bad)?;
            fmt = Some((format, channels, rate, bits));
        } else if id == b"data" {
            data_off = Some(start);
            data_len = size;
        }
        off = start + size + (size % 2);
    }
    let (format, channels, rate, bits) = fmt.ok_or_else(bad)?;
    let start = data_off.ok_or_else(bad)?;
    if format != 1 || !(1..=2).contains(&channels) || (bits != 8 && bits != 16) {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع ملف WAV بنبضة PCM أحادية أو ثنائية 8 أو 16 بت",
                display
            ),
            line,
            col,
        ));
    }
    if rate == 0 || rate > 384000 {
        return Err(bad());
    }
    let frame_bytes = channels as usize * (bits as usize / 8);
    if data_len % frame_bytes != 0 {
        return Err(bad());
    }
    let total = data_len / (bits as usize / 8);
    if total > 1_000_000 {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع ملف صوت ≤ 1000000 عينة", display),
            line,
            col,
        ));
    }
    let mut samples = Vec::with_capacity(total);
    if bits == 16 {
        for i in 0..total {
            let v = j12_u16(data, start + i * 2).ok_or_else(bad)? as i16;
            samples.push(v as i32);
        }
    } else {
        for i in 0..total {
            samples.push(data[start + i] as i32 - 128);
        }
    }
    Ok(J12Wav {
        rate,
        channels,
        bits,
        samples,
    })
}

/// 184: ولد_صوتا(مسار، تردد، مدة، موجة) — WAV أحادي 16-بت 44100 هرتز؛
/// الموجة جيب/مربع/منشار/مثلث؛ والمدة 0.01..60 ثانية؛ والتردد 20..20000.
fn media_make_tone(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وسائط.ولد_صوتا";
    need_args(D, args, 4, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let freq = need_number(D, &args[1], line, col)?;
    let duration = need_number(D, &args[2], line, col)?;
    let wave = need_string(D, &args[3], line, col)?;
    if !matches!(wave.as_str(), "جيب" | "مربع" | "منشار" | "مثلث") {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع موجة من (جيب، مربع، منشار، مثلث) لكن أعطيت {}",
                D, wave
            ),
            line,
            col,
        ));
    }
    if !freq.is_finite() || !(20.0..=20000.0).contains(&freq) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع التردد ضمن 20..20000", D),
            line,
            col,
        ));
    }
    if !duration.is_finite() || !(0.01..=60.0).contains(&duration) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع المدة ضمن 0.01..60 ثانية", D),
            line,
            col,
        ));
    }
    let n = (duration * 44100.0).round() as i64;
    let step = j12_wave_step(freq);
    let mut samples = Vec::with_capacity(n as usize);
    for i in 0..n {
        samples.push(j12_wave_sample(&wave, step, i) as i32);
    }
    let bytes = j12_wav_bytes(44100, 1, 16, &samples);
    j12_write_file(ctx, &path, &bytes, line, col)?;
    Ok(Value::Nil)
}

/// 186: حول_صوتا(مصدر، هدف، معدل) — تغيير معدل عينات WAV
/// (إعادة أخذ بالأقرب) مع بقاء القنوات والبتات.
fn media_convert_tone(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وسائط.حول_صوتا";
    need_args(D, args, 3, line, col)?;
    let src = need_string(D, &args[0], line, col)?;
    let dst = need_string(D, &args[1], line, col)?;
    let new_rate = j12_need_int(D, &args[2], line, col)?;
    if !(8000..=192000).contains(&new_rate) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع المعدل ضمن 8000..192000", D),
            line,
            col,
        ));
    }
    let data = j12_read_file(ctx, &src, line, col)?;
    let wav = j12_parse_wav(D, &data, line, col)?;
    let ch = wav.channels as usize;
    let frames = wav.samples.len() / ch;
    let out_frames = (frames as i64 * new_rate / wav.rate as i64) as usize;
    let mut out = Vec::with_capacity(out_frames * ch);
    for j in 0..out_frames {
        let s = j * wav.rate as usize / new_rate as usize;
        for c in 0..ch {
            out.push(wav.samples[s * ch + c]);
        }
    }
    let bytes = j12_wav_bytes(new_rate as u32, wav.channels, wav.bits, &out);
    j12_write_file(ctx, &dst, &bytes, line, col)?;
    Ok(Value::Nil)
}

/// اقرأ_صوتا(مصدر) — قاموس {معدل، قنوات، بتات، عينات}.
fn media_read_tone(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "وسائط.اقرأ_صوتا";
    need_args(D, args, 1, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let data = j12_read_file(ctx, &path, line, col)?;
    let wav = j12_parse_wav(D, &data, line, col)?;
    Ok(Value::Dict(vec![
        ("معدل".to_string(), Value::Number(wav.rate as f64)),
        ("قنوات".to_string(), Value::Number(wav.channels as f64)),
        ("بتات".to_string(), Value::Number(wav.bits as f64)),
        (
            "عينات".to_string(),
            Value::List(
                wav.samples
                    .iter()
                    .map(|&s| Value::Number(s as f64))
                    .collect(),
            ),
        ),
    ]))
}

/// 187: حاك_حياة(مسار، عرض، ارتفاع، خلايا، خطوات) — لعبة الحياة
/// (كونواي) بلا التفاف؛ والمخرج BMP بخلية 10 بكسل (حي أسود).
fn sim_life(
    ctx: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "محاكاة.حاك_حياة";
    need_args(D, args, 5, line, col)?;
    let path = need_string(D, &args[0], line, col)?;
    let w = j12_need_int(D, &args[1], line, col)?;
    let h = j12_need_int(D, &args[2], line, col)?;
    if !(1..=500).contains(&w) || !(1..=500).contains(&h) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع العرض والارتفاع ضمن 1..500", D),
            line,
            col,
        ));
    }
    let cells = need_list(D, &args[3], line, col)?;
    let steps = j12_need_int(D, &args[4], line, col)?;
    if !(0..=10000).contains(&steps) {
        return Err(RioError::runtime(
            format!("الدالة {} تتوقع الخطوات ضمن 0..10000", D),
            line,
            col,
        ));
    }
    let mut grid = vec![false; (w * h) as usize];
    for cell in cells {
        let xy = need_list(D, cell, line, col)?;
        if xy.len() != 2 {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل خلية قائمة [س، ص]", D),
                line,
                col,
            ));
        }
        let x = j12_need_int(D, &xy[0], line, col)?;
        let y = j12_need_int(D, &xy[1], line, col)?;
        if !(0..w).contains(&x) || !(0..h).contains(&y) {
            return Err(RioError::runtime(
                format!("الدالة {} تتوقع كل خلية داخل الشبكة", D),
                line,
                col,
            ));
        }
        grid[(y * w + x) as usize] = true;
    }
    let mut next = vec![false; (w * h) as usize];
    for _ in 0..steps {
        for y in 0..h {
            for x in 0..w {
                let mut n = 0i64;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        if (0..w).contains(&nx)
                            && (0..h).contains(&ny)
                            && grid[(ny * w + nx) as usize]
                        {
                            n += 1;
                        }
                    }
                }
                let alive = grid[(y * w + x) as usize];
                next[(y * w + x) as usize] = n == 3 || (alive && n == 2);
            }
        }
        std::mem::swap(&mut grid, &mut next);
    }
    let cell = 10i64;
    let mut px = Vec::with_capacity((w * cell * h * cell) as usize);
    for y in 0..(h * cell) {
        for x in 0..(w * cell) {
            let alive = grid[((y / cell) * w + (x / cell)) as usize];
            px.push(if alive { [0u8, 0, 0] } else { [255u8, 255, 255] });
        }
    }
    let bytes = j12_bmp_bytes(w * cell, h * cell, &px);
    j12_write_file(ctx, &path, &bytes, line, col)?;
    Ok(Value::Nil)
}

// ============================================================
// ج6 (الدفعة الأخيرة من الـ579 — الأنواع والأعداد والدقة):
// البنود 99 (بت) و101 (مركب) و102 (دقة ممتدة) و103 (كسر)
// و105 (معلب) و106 (جمع دقيق). الأنواع الجديدة قيم
// حقيقية (Value::BigInt/Rational/Complex/Bits/Boxed) لا
// نصوص — والعمليات عليها في interpreter.rs (المساعدات
// المشتركة add_values/arith_values/neg_value/compare_values).
// f64 الافتراضية بلا أي تغيير في دلالتها.
// ============================================================

use crate::value as j6v;

fn j6_need_args_range(
    display: &str,
    args: &[Value],
    min: usize,
    max: usize,
    line: usize,
    col: usize,
) -> RioResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع {} إلى {} معاملات لكن أعطيت {}",
                display,
                min,
                max,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

fn j6_need_bigint(display: &str, v: &Value, line: usize, col: usize) -> RioResult<String> {
    match v {
        Value::BigInt(s) => Ok(s.clone()),
        Value::Number(n) => j6v::bigint_from_f64(*n).ok_or_else(|| {
            RioError::typ(
                format!("{} تتطلب عددا صحيحا، وليس كسرا", display),
                line,
                col,
            )
        }),
        Value::Str(s) => j6v::bigint_norm(s).ok_or_else(|| {
            RioError::typ(format!("عدد كبير غير صالح: {}", s), line, col)
        }),
        other => Err(RioError::typ(
            format!("{} تتطلب عددا كبيرا، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

fn j6_need_rational(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<(String, String)> {
    match v {
        Value::Rational(n, d) => Ok((n.clone(), d.clone())),
        Value::BigInt(s) => Ok((s.clone(), "1".to_string())),
        Value::Number(n) => j6v::rat_from_f64(*n).ok_or_else(|| {
            RioError::typ(
                format!("{} تتطلب عددا منتهيا", display),
                line,
                col,
            )
        }),
        other => Err(RioError::typ(
            format!("{} تتطلب كسرا، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

fn j6_need_complex(
    display: &str,
    v: &Value,
    line: usize,
    col: usize,
) -> RioResult<(f64, f64)> {
    j6v::value_to_complex(v).ok_or_else(|| {
        RioError::typ(
            format!("{} تتطلب عددا مركبا، وليس {}", display, other_type(v)),
            line,
            col,
        )
    })
}

fn other_type(v: &Value) -> &'static str {
    v.type_name()
}

fn j6_need_bits(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Vec<bool>> {
    match v {
        Value::Bits(b) => Ok(b.clone()),
        other => Err(RioError::typ(
            format!("{} تتطلب بت، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

fn j6_need_boxed(display: &str, v: &Value, line: usize, col: usize) -> RioResult<Value> {
    match v {
        Value::Boxed(inner) => Ok((**inner).clone()),
        other => Err(RioError::typ(
            format!("{} تتطلب معلبا، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

fn j6_need_count(display: &str, v: &Value, line: usize, col: usize) -> RioResult<usize> {
    match v {
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => Ok(*n as usize),
        Value::BigInt(s) => {
            let f = j6v::bigint_to_f64(s);
            if f.is_finite() && f >= 0.0 && f <= (usize::MAX as f64) {
                Ok(f as usize)
            } else {
                Err(RioError::typ(
                    format!("{} تتطلب عددا صحيحا غير سالب", display),
                    line,
                    col,
                ))
            }
        }
        other => Err(RioError::typ(
            format!("{} تتطلب عددا صحيحا غير سالب، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

fn j6_need_f64(display: &str, v: &Value, line: usize, col: usize) -> RioResult<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::BigInt(s) => Ok(j6v::bigint_to_f64(s)),
        Value::Rational(n, d) => Ok(j6v::rat_to_f64(&(n.clone(), d.clone()))),
        Value::Complex(re, im) => {
            if *im == 0.0 {
                Ok(*re)
            } else {
                Err(RioError::typ(
                    format!("{} تتطلب عددا حقيقيا، وليس مركبا بتخيلي", display),
                    line,
                    col,
                ))
            }
        }
        other => Err(RioError::typ(
            format!("{} تتطلب عددا، وليس {}", display, other.type_name()),
            line,
            col,
        )),
    }
}

// ---------- دقة (102 + 106) ----------

fn precision_big(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "دقة.كبير";
    need_args(D, args, 1, line, col)?;
    Ok(Value::BigInt(j6_need_bigint(D, &args[0], line, col)?))
}

fn precision_to_number(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "دقة.إلى_عدد";
    need_args(D, args, 1, line, col)?;
    Ok(Value::Number(j6_need_f64(D, &args[0], line, col)?))
}

fn precision_to_text(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "دقة.إلى_نص";
    need_args(D, args, 1, line, col)?;
    Ok(Value::Str(args[0].fmt_value()))
}

fn precision_pow(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "دقة.أس";
    need_args(D, args, 2, line, col)?;
    let base = j6_need_bigint(D, &args[0], line, col)?;
    let exp = match &args[1] {
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 => *n as i64,
        Value::BigInt(s) => {
            let f = j6v::bigint_to_f64(s);
            if f.abs() > 1_000_000.0 {
                return Err(RioError::typ(
                    "دقة.أس تتطلب أسا في حدود المعقول".to_string(),
                    line,
                    col,
                ));
            }
            f as i64
        }
        other => {
            return Err(RioError::typ(
                format!("دقة.أس تتطلب أسا صحيحا، وليس {}", other.type_name()),
                line,
                col,
            ));
        }
    };
    if exp < 0 {
        // أس سالب = كسر دقيق (1 / أساس^|أس|).
        let mut acc = "1".to_string();
        let mut b = base.clone();
        let mut e = (-exp) as u64;
        while e > 0 {
            if e % 2 == 1 {
                acc = j6v::bigint_mul(&acc, &b);
            }
            e /= 2;
            if e > 0 {
                b = j6v::bigint_mul(&b, &b);
            }
        }
        let (n, d) = j6v::rat_norm("1", &acc)
            .unwrap_or_else(|| ("0".to_string(), "1".to_string()));
        return Ok(Value::Rational(n, d));
    }
    let mut acc = "1".to_string();
    let mut b = base;
    let mut e = exp as u64;
    while e > 0 {
        if e % 2 == 1 {
            acc = j6v::bigint_mul(&acc, &b);
        }
        e /= 2;
        if e > 0 {
            b = j6v::bigint_mul(&b, &b);
        }
    }
    Ok(Value::BigInt(acc))
}

fn precision_exact_sum(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "دقة.جمع_دقيق";
    need_args(D, args, 1, line, col)?;
    let items = match &args[0] {
        Value::List(items) => items.clone(),
        other => {
            return Err(RioError::typ(
                format!("دقة.جمع_دقيق يتطلب قائمة، وليس {}", other.type_name()),
                line,
                col,
            ));
        }
    };
    if items.is_empty() {
        return Ok(Value::Number(0.0));
    }
    // مركب أولا (بـf64)؛ ثم دقيق (كبير/كسر)؛ ثم عائم معوض (نويمر).
    if items.iter().any(|v| matches!(v, Value::Complex(..))) {
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for v in &items {
            match j6v::value_to_complex(v) {
                Some((a, b)) => {
                    re += a;
                    im += b;
                }
                None => {
                    return Err(RioError::typ(
                        format!(
                            "دقة.جمع_دقيق يتطلب قائمة أعداد، وجد {}",
                            v.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        return Ok(Value::Complex(re, im));
    }
    if items
        .iter()
        .any(|v| matches!(v, Value::BigInt(_) | Value::Rational(_, _)))
    {
        if items.iter().all(|v| matches!(v, Value::BigInt(_))) {
            let mut acc = "0".to_string();
            for v in &items {
                if let Value::BigInt(s) = v {
                    acc = j6v::bigint_add(&acc, s);
                }
            }
            return Ok(Value::BigInt(acc));
        }
        let mut acc = ("0".to_string(), "1".to_string());
        for v in &items {
            match j6v::value_to_rational(v) {
                Some(r) => acc = j6v::rat_add(&acc, &r),
                None => {
                    return Err(RioError::typ(
                        format!(
                            "دقة.جمع_دقيق يتطلب قائمة أعداد، وجد {}",
                            v.type_name()
                        ),
                        line,
                        col,
                    ));
                }
            }
        }
        let (n, d) = acc;
        return Ok(Value::Rational(n, d));
    }
    // عائم معوض (خوارزمية نويمر): 0.1 عشر مرات = 1 لا 0.999...
    let mut sum = 0.0f64;
    let mut c = 0.0f64;
    for v in &items {
        let x = match v {
            Value::Number(n) => *n,
            other => {
                return Err(RioError::typ(
                    format!(
                        "دقة.جمع_دقيق يتطلب قائمة أعداد، وجد {}",
                        other.type_name()
                    ),
                    line,
                    col,
                ));
            }
        };
        let t = sum + x;
        if sum.abs() >= x.abs() {
            c += (sum - t) + x;
        } else {
            c += (x - t) + sum;
        }
        sum = t;
    }
    Ok(Value::Number(sum + c))
}

// ---------- كسر (103) ----------

fn rational_new(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.جديد";
    j6_need_args_range(D, args, 1, 2, line, col)?;
    let num = j6_need_bigint(D, &args[0], line, col)?;
    let den = if args.len() == 2 {
        j6_need_bigint(D, &args[1], line, col)?
    } else {
        "1".to_string()
    };
    match j6v::rat_norm(&num, &den) {
        Some((n, d)) => Ok(Value::Rational(n, d)),
        None => Err(RioError::typ(
            "مقام الكسر لا يمكن أن يكون صفرا".to_string(),
            line,
            col,
        )),
    }
}

fn rational_num(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.بسط";
    need_args(D, args, 1, line, col)?;
    let (n, _) = j6_need_rational(D, &args[0], line, col)?;
    Ok(Value::BigInt(n))
}

fn rational_den(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.مقام";
    need_args(D, args, 1, line, col)?;
    let (_, d) = j6_need_rational(D, &args[0], line, col)?;
    Ok(Value::BigInt(d))
}

fn rational_to_number(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.إلى_عدد";
    need_args(D, args, 1, line, col)?;
    let r = j6_need_rational(D, &args[0], line, col)?;
    Ok(Value::Number(j6v::rat_to_f64(&r)))
}

fn rational_to_text(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.إلى_نص";
    need_args(D, args, 1, line, col)?;
    let _ = j6_need_rational(D, &args[0], line, col)?;
    Ok(Value::Str(args[0].fmt_value()))
}

fn rational_from_number(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "كسر.من_عدد";
    need_args(D, args, 1, line, col)?;
    match &args[0] {
        Value::Number(n) => match j6v::rat_from_f64(*n) {
            Some((a, b)) => Ok(Value::Rational(a, b)),
            None => Err(RioError::typ(
                "كسر.من_عدد يتطلب عددا منتهيا".to_string(),
                line,
                col,
            )),
        },
        other => {
            let r = j6_need_rational(D, other, line, col)?;
            Ok(Value::Rational(r.0, r.1))
        }
    }
}

// ---------- مركب (101) ----------

fn complex_new(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.جديد";
    j6_need_args_range(D, args, 1, 2, line, col)?;
    let re = j6_need_f64(D, &args[0], line, col)?;
    let im = if args.len() == 2 {
        j6_need_f64(D, &args[1], line, col)?
    } else {
        0.0
    };
    Ok(Value::Complex(re, im))
}

fn complex_real(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.حقيقي";
    need_args(D, args, 1, line, col)?;
    let (re, _) = j6_need_complex(D, &args[0], line, col)?;
    Ok(Value::Number(re))
}

fn complex_imag(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.تخيلي";
    need_args(D, args, 1, line, col)?;
    let (_, im) = j6_need_complex(D, &args[0], line, col)?;
    Ok(Value::Number(im))
}

fn complex_abs(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.مقدار";
    need_args(D, args, 1, line, col)?;
    let (re, im) = j6_need_complex(D, &args[0], line, col)?;
    Ok(Value::Number((re * re + im * im).sqrt()))
}

fn complex_conj(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.مرافق";
    need_args(D, args, 1, line, col)?;
    let (re, im) = j6_need_complex(D, &args[0], line, col)?;
    Ok(Value::Complex(re, -im))
}

fn complex_to_text(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "مركب.إلى_نص";
    need_args(D, args, 1, line, col)?;
    let _ = j6_need_complex(D, &args[0], line, col)?;
    Ok(Value::Str(args[0].fmt_value()))
}

// ---------- بت (99) ----------

fn bits_from_number(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.من_عدد";
    j6_need_args_range(D, args, 1, 2, line, col)?;
    let n = j6_need_count(D, &args[0], line, col)?;
    let len = if args.len() == 2 {
        Some(j6_need_count(D, &args[1], line, col)?)
    } else {
        None
    };
    let mut bits: Vec<bool> = Vec::new();
    let mut x = n;
    if x == 0 {
        bits.push(false);
    } else {
        while x > 0 {
            bits.push(x % 2 == 1);
            x /= 2;
        }
        bits.reverse();
    }
    if let Some(want) = len {
        if bits.len() > want {
            return Err(RioError::typ(
                format!(
                    "العدد لا يتسع في {} بت (يحتاج {})",
                    want,
                    bits.len()
                ),
                line,
                col,
            ));
        }
        while bits.len() < want {
            bits.insert(0, false);
        }
    }
    Ok(Value::Bits(bits))
}

fn bits_from_text(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.من_نص";
    need_args(D, args, 1, line, col)?;
    let s = need_string(D, &args[0], line, col)?;
    if s.is_empty() {
        return Err(RioError::typ(
            "بت.من_نص يتطلب نصا غير فارغ من 0 و1".to_string(),
            line,
            col,
        ));
    }
    let mut bits = Vec::new();
    for c in s.chars() {
        match c {
            '0' | '٠' => bits.push(false),
            '1' | '١' => bits.push(true),
            _ => {
                return Err(RioError::typ(
                    "نص البت يتطلب 0 و1 فقط".to_string(),
                    line,
                    col,
                ));
            }
        }
    }
    Ok(Value::Bits(bits))
}

fn bits_to_number(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.إلى_عدد";
    need_args(D, args, 1, line, col)?;
    let bits = j6_need_bits(D, &args[0], line, col)?;
    let mut v = 0.0f64;
    for b in &bits {
        v = v * 2.0 + if *b { 1.0 } else { 0.0 };
    }
    Ok(Value::Number(v))
}

fn bits_to_text(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.إلى_نص";
    need_args(D, args, 1, line, col)?;
    let bits = j6_need_bits(D, &args[0], line, col)?;
    let s: String = bits.iter().map(|b| if *b { '1' } else { '0' }).collect();
    Ok(Value::Str(s))
}

fn bits_len(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.طول";
    need_args(D, args, 1, line, col)?;
    let bits = j6_need_bits(D, &args[0], line, col)?;
    Ok(Value::Number(bits.len() as f64))
}

fn bits_binop(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
    f: fn(bool, bool) -> bool,
) -> RioResult<Value> {
    need_args(display, args, 2, line, col)?;
    let a = j6_need_bits(display, &args[0], line, col)?;
    let b = j6_need_bits(display, &args[1], line, col)?;
    if a.len() != b.len() {
        return Err(RioError::typ(
            format!(
                "عمليات البت تتطلب أطوالا متساوية ({} مقابل {})",
                a.len(),
                b.len()
            ),
            line,
            col,
        ));
    }
    Ok(Value::Bits(
        a.iter().zip(b.iter()).map(|(x, y)| f(*x, *y)).collect(),
    ))
}

fn bits_and(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_binop("بت.و", args, line, col, |a, b| a && b)
}

fn bits_or(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_binop("بت.أو", args, line, col, |a, b| a || b)
}

fn bits_xor(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_binop("بت.حصري", args, line, col, |a, b| a != b)
}

fn bits_not(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "بت.ليس";
    need_args(D, args, 1, line, col)?;
    let bits = j6_need_bits(D, &args[0], line, col)?;
    Ok(Value::Bits(bits.iter().map(|b| !*b).collect()))
}

fn bits_rotate(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
    left: bool,
) -> RioResult<Value> {
    need_args(display, args, 2, line, col)?;
    let bits = j6_need_bits(display, &args[0], line, col)?;
    let n = j6_need_count(display, &args[1], line, col)?;
    if bits.is_empty() {
        return Ok(Value::Bits(bits));
    }
    let k = n % bits.len();
    let mut out = bits.clone();
    if left {
        out.rotate_left(k);
    } else {
        out.rotate_right(k);
    }
    Ok(Value::Bits(out))
}

fn bits_rotl(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_rotate("بت.أدر_يسار", args, line, col, true)
}

fn bits_rotr(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_rotate("بت.أدر_يمين", args, line, col, false)
}

fn bits_shift(
    display: &str,
    args: &[Value],
    line: usize,
    col: usize,
    left: bool,
) -> RioResult<Value> {
    need_args(display, args, 2, line, col)?;
    let bits = j6_need_bits(display, &args[0], line, col)?;
    let n = j6_need_count(display, &args[1], line, col)?;
    let len = bits.len();
    let mut out = vec![false; len];
    if n < len {
        if left {
            out[..len - n].copy_from_slice(&bits[n..]);
        } else {
            out[n..].copy_from_slice(&bits[..len - n]);
        }
    }
    Ok(Value::Bits(out))
}

fn bits_shl(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_shift("بت.أزح_يسار", args, line, col, true)
}

fn bits_shr(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    bits_shift("بت.أزح_يمين", args, line, col, false)
}

// ---------- معلب (105) ----------

fn boxed_new(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "معلب.جديد";
    need_args(D, args, 1, line, col)?;
    Ok(Value::Boxed(Box::new(args[0].clone())))
}

fn boxed_open(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "معلب.افتح";
    need_args(D, args, 1, line, col)?;
    j6_need_boxed(D, &args[0], line, col)
}

fn boxed_is(
    _: &mut dyn HandlerCaller,
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<Value> {
    const D: &str = "معلب.هو_معلب";
    need_args(D, args, 1, line, col)?;
    Ok(Value::Bool(matches!(args[0], Value::Boxed(_))))
}

// ---------- جداول التسجيل (توليد جدولي — انظر j6gen) ----------
// BEGIN J6-GEN entries-precision
fn precision_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("كبير", native("كبير", precision_big)),
        ("إلى_عدد", native("إلى_عدد", precision_to_number)),
        ("إلى_نص", native("إلى_نص", precision_to_text)),
        ("أس", native("أس", precision_pow)),
        ("جمع_دقيق", native("جمع_دقيق", precision_exact_sum)),
    ]
}
// END J6-GEN entries-precision
// BEGIN J6-GEN entries-rational
fn rational_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("جديد", native("جديد", rational_new)),
        ("بسط", native("بسط", rational_num)),
        ("مقام", native("مقام", rational_den)),
        ("إلى_عدد", native("إلى_عدد", rational_to_number)),
        ("إلى_نص", native("إلى_نص", rational_to_text)),
        ("من_عدد", native("من_عدد", rational_from_number)),
    ]
}
// END J6-GEN entries-rational
// BEGIN J6-GEN entries-complex
fn complex_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("جديد", native("جديد", complex_new)),
        ("حقيقي", native("حقيقي", complex_real)),
        ("تخيلي", native("تخيلي", complex_imag)),
        ("مقدار", native("مقدار", complex_abs)),
        ("مرافق", native("مرافق", complex_conj)),
        ("إلى_نص", native("إلى_نص", complex_to_text)),
    ]
}
// END J6-GEN entries-complex
// BEGIN J6-GEN entries-bits
fn bits_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("من_عدد", native("من_عدد", bits_from_number)),
        ("من_نص", native("من_نص", bits_from_text)),
        ("إلى_عدد", native("إلى_عدد", bits_to_number)),
        ("إلى_نص", native("إلى_نص", bits_to_text)),
        ("طول", native("طول", bits_len)),
        ("و", native("و", bits_and)),
        ("او", native("او", bits_or)),
        ("حصري", native("حصري", bits_xor)),
        ("ليس", native("ليس", bits_not)),
        ("أدر_يسار", native("أدر_يسار", bits_rotl)),
        ("أدر_يمين", native("أدر_يمين", bits_rotr)),
        ("أزح_يسار", native("أزح_يسار", bits_shl)),
        ("أزح_يمين", native("أزح_يمين", bits_shr)),
    ]
}
// END J6-GEN entries-bits
// BEGIN J6-GEN entries-boxed
fn boxed_entries() -> Vec<(&'static str, Value)> {
    vec![
        ("جديد", native("جديد", boxed_new)),
        ("افتح", native("افتح", boxed_open)),
        ("هو_معلب", native("هو_معلب", boxed_is)),
    ]
}
// END J6-GEN entries-boxed
