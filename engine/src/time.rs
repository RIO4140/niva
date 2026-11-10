//! Calendar for the `وقت` native module (catalog task 15,
//! from `kernel/time`): Unix seconds <-> civil date/time,
//! formatting, and basic calendar arithmetic. One
//! implementation, shared by both engines (interpreter +
//! VM) through the stdlib native functions — never
//! implemented twice.
//!
//! Design (see `CATALOG-07-TIME.md` §1): the existing
//! `وقت` module is extended in place — no new
//! user-facing module. Pure civil-date math (Howard
//! Hinnant's algorithms) works everywhere, including the
//! browser; the local zone comes from the system TZif file
//! (`/etc/localtime`, honoring `TZ` as a zone name) on
//! Unix, is honestly refused on Windows (no TZif there),
//! and is refused in the browser (no OS zone there).
//! No new dependencies: std only.

use crate::error::{RioError, RioResult};
use crate::value::Value;

/// ساعة بدء الجلسة لـ`بيئة("ثواني")` — ثوان منذ بدء الجلسة.
/// أصليا `Instant` (ساعة أحادية)؛ وعلى WASM `js_sys::Date::now()`
/// (فـ`Instant::now()` يذعر على wasm32-unknown-unknown — بلا ساعة
/// أحادية في std هناك — كما في `next_u64` في stdlib.rs؛ والذعر
/// كان يكسر كل تشغيل `run_source` على WASM منذ الدفعة 4، فلا
/// برنامج يعمل في الملعب أصلا). على WASM الساعة ساعة حائط
/// فتُقيد النتيجة بلا سالب. المعنى واحد على المنصتين: ثوان
/// صحيحة منذ بدء الجلسة.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionClock {
    #[cfg(target_arch = "wasm32")]
    start_ms: f64,
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl SessionClock {
    pub(crate) fn now() -> Self {
        #[cfg(target_arch = "wasm32")]
        return Self {
            start_ms: js_sys::Date::now(),
        };
        #[cfg(not(target_arch = "wasm32"))]
        return Self {
            start: std::time::Instant::now(),
        };
    }

    pub(crate) fn elapsed_secs(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        return ((js_sys::Date::now() - self.start_ms) / 1000.0).max(0.0) as u64;
        #[cfg(not(target_arch = "wasm32"))]
        return self.start.elapsed().as_secs();
    }
}

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// A broken-down date/time plus its Unix epoch and which
/// clock built it. The `utc` flag is what `أضف` uses to
/// recompute the fields in the same clock the dict came
/// from — a UTC dict never silently becomes local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = الأحد .. 6 = السبت.
    pub weekday: u32,
    pub epoch: i64,
    pub utc: bool,
}

/// Days since 1970-01-01 -> (year, month, day).
/// Howard Hinnant's civil-from-days algorithm — the same
/// math `Interpreter::format_unix_utc` used to inline;
/// this is now the single source of civil math in the
/// engine (that builtin is a thin wrapper over it).
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d)
}

/// (year, month, day) -> days since 1970-01-01.
/// Hinnant's days-from-civil; the exact inverse of
/// `civil_from_days` for every representable date.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Weekday of a day count: 0 = الأحد .. 6 = السبت.
/// 1970-01-01 was a Thursday (4).
pub(crate) fn weekday_from_days(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0) && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if m == 2 && is_leap(y) {
        29
    } else {
        DAYS[(m - 1) as usize]
    }
}

// --- Local zone (TZif) ---

fn be_u32(d: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(off..off + 4)?.try_into().ok()?))
}

fn be_i32(d: &[u8], off: usize) -> Option<i32> {
    Some(i32::from_be_bytes(d.get(off..off + 4)?.try_into().ok()?))
}

fn be_i64(d: &[u8], off: usize) -> Option<i64> {
    Some(i64::from_be_bytes(d.get(off..off + 8)?.try_into().ok()?))
}

struct TzCounts {
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

fn read_counts(d: &[u8], hdr: usize) -> Option<TzCounts> {
    if d.len() < hdr + 44 || &d[hdr..hdr + 4] != b"TZif" {
        return None;
    }
    Some(TzCounts {
        isutcnt: be_u32(d, hdr + 20)? as usize,
        isstdcnt: be_u32(d, hdr + 24)? as usize,
        leapcnt: be_u32(d, hdr + 28)? as usize,
        timecnt: be_u32(d, hdr + 32)? as usize,
        typecnt: be_u32(d, hdr + 36)? as usize,
        charcnt: be_u32(d, hdr + 40)? as usize,
    })
}

/// The UTC offset (seconds) in effect at Unix time
/// `secs`, from a TZif file (v1/v2/v3). The last
/// transition at or before `secs` wins; before the
/// first transition the first transition's type applies
/// (the tzcode rule); with no transitions the first
/// type applies.
pub(crate) fn offset_in_file(
    path: &std::path::Path,
    secs: i64,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    let data = std::fs::read(path).map_err(|_| {
        RioError::runtime("تعذر قراءة ملف المنطقة الزمنية", line, col)
    })?;
    let corrupt = || RioError::runtime("ملف المنطقة الزمنية تالف", line, col);
    let c1 = read_counts(&data, 0).ok_or_else(corrupt)?;
    // v1 data block: transitions (4 bytes each) + type
    // indices (1) + ttinfo (6) + abbreviations + leap
    // records (8) + std/wall + UT indicators.
    let v1_len = 4 * c1.timecnt
        + c1.timecnt
        + 6 * c1.typecnt
        + c1.charcnt
        + 8 * c1.leapcnt
        + c1.isstdcnt
        + c1.isutcnt;
    let version = data[4];
    let (trans, idx, ttinfo): (Vec<i64>, Vec<u8>, Vec<i32>);
    if version == b'2' || version == b'3' || version == b'4' {
        let hdr2 = 44usize.checked_add(v1_len).ok_or_else(corrupt)?;
        let c2 = read_counts(&data, hdr2).ok_or_else(corrupt)?;
        let base = hdr2 + 44;
        let need = 8 * c2.timecnt
            + c2.timecnt
            + 6 * c2.typecnt
            + c2.charcnt
            + 12 * c2.leapcnt
            + c2.isstdcnt
            + c2.isutcnt;
        if data.len() < base.checked_add(need).ok_or_else(corrupt)? {
            return Err(corrupt());
        }
        let mut t = Vec::with_capacity(c2.timecnt);
        for i in 0..c2.timecnt {
            t.push(be_i64(&data, base + 8 * i).ok_or_else(corrupt)?);
        }
        let mut ix = Vec::with_capacity(c2.timecnt);
        for i in 0..c2.timecnt {
            ix.push(*data.get(base + 8 * c2.timecnt + i).ok_or_else(corrupt)?);
        }
        let mut tt = Vec::with_capacity(c2.typecnt);
        let tt_base = base + 8 * c2.timecnt + c2.timecnt;
        for i in 0..c2.typecnt {
            tt.push(be_i32(&data, tt_base + 6 * i).ok_or_else(corrupt)?);
        }
        trans = t;
        idx = ix;
        ttinfo = tt;
    } else if version == 0 {
        let base: usize = 44;
        let need = 4 * c1.timecnt
            + c1.timecnt
            + 6 * c1.typecnt
            + c1.charcnt
            + 8 * c1.leapcnt
            + c1.isstdcnt
            + c1.isutcnt;
        if data.len() < base.checked_add(need).ok_or_else(corrupt)? {
            return Err(corrupt());
        }
        let mut t = Vec::with_capacity(c1.timecnt);
        for i in 0..c1.timecnt {
            t.push(be_i32(&data, base + 4 * i).ok_or_else(corrupt)? as i64);
        }
        let mut ix = Vec::with_capacity(c1.timecnt);
        for i in 0..c1.timecnt {
            ix.push(*data.get(base + 4 * c1.timecnt + i).ok_or_else(corrupt)?);
        }
        let mut tt = Vec::with_capacity(c1.typecnt);
        let tt_base = base + 4 * c1.timecnt + c1.timecnt;
        for i in 0..c1.typecnt {
            tt.push(be_i32(&data, tt_base + 6 * i).ok_or_else(corrupt)?);
        }
        trans = t;
        idx = ix;
        ttinfo = tt;
    } else {
        return Err(corrupt());
    }
    if ttinfo.is_empty() {
        return Err(corrupt());
    }
    for &i in &idx {
        if (i as usize) >= ttinfo.len() {
            return Err(corrupt());
        }
    }
    let chosen: usize = if trans.is_empty() {
        0
    } else {
        match trans.binary_search(&secs) {
            Ok(i) => idx[i] as usize,
            Err(0) => idx[0] as usize,
            Err(i) => idx[i - 1] as usize,
        }
    };
    Ok(ttinfo[chosen] as i64)
}

/// Resolve the local zone file: `TZ` names a zone
/// (absolute path, or a name under the system zoneinfo
/// directories); an empty `TZ` means UTC; without `TZ`
/// the system file `/etc/localtime` is used. A `TZ`
/// POSIX rule string is not parsed in v1 — the system
/// file is used instead (see CATALOG-07-TIME.md §1).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
fn local_zone_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.strip_prefix(':').unwrap_or(&tz);
        if tz.is_empty() {
            return None; // UTC
        }
        let p = PathBuf::from(tz);
        if p.is_absolute() && p.exists() {
            return Some(p);
        }
        for dir in [
            "/usr/share/zoneinfo",
            "/system/usr/share/zoneinfo",
            "/data/misc/zoneinfo",
        ] {
            let cand = PathBuf::from(dir).join(tz);
            if cand.exists() {
                return Some(cand);
            }
        }
        // POSIX rule string (or unknown name): fall
        // through to the system file, documented in §1.
    }
    Some(PathBuf::from("/etc/localtime"))
}

/// The local UTC offset (seconds) at Unix time `secs`.
/// Unix: the system TZif file. Windows: honestly
/// refused (no TZif there). WASM: refused with the
/// browser message (no OS zone in the playground).
pub fn local_offset_at(
    secs: i64,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (secs, display);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
    {
        let _ = secs;
        Err(RioError::runtime(
            format!("{display} غير مدعوم على هذه المنصة"),
            line,
            col,
        ))
    }
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
    {
        // `display` names the caller in the Windows
        // refusal; the Unix path needs no name.
        let _ = display;
        match local_zone_path() {
            None => Ok(0), // TZ empty: UTC
            Some(path) => offset_in_file(&path, secs, line, col),
        }
    }
}

/// Unix seconds -> broken-down date/time. `utc` false
/// means the system local zone (which may refuse on
/// Windows / in the browser — see `local_offset_at`).
pub fn datetime_from_epoch(
    secs: i64,
    utc: bool,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<DateTime> {
    let offset = if utc {
        0
    } else {
        local_offset_at(secs, display, line, col)?
    };
    let local = secs + offset;
    let days = local.div_euclid(86400);
    let sod = local.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    Ok(DateTime {
        year,
        month,
        day,
        hour: (sod / 3600) as u32,
        minute: ((sod % 3600) / 60) as u32,
        second: (sod % 60) as u32,
        weekday: weekday_from_days(days),
        epoch: secs,
        utc,
    })
}

/// Civil components -> `DateTime`, validating every
/// field with an Arabic error. Local components resolve
/// their offset iteratively: the offset at the naive
/// time, then one correction at the corrected time.
/// Ambiguous local times (the repeated autumn hour)
/// resolve to the first occurrence; nonexistent ones
/// (the skipped spring hour) resolve with the
/// pre-transition offset — the standard rule, noted
/// in CATALOG-07-TIME.md §1.
pub fn datetime_from_components(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    utc: bool,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<DateTime> {
    if !(1..=9999).contains(&year) {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع سنة بين 1 و 9999"),
            line,
            col,
        ));
    }
    if !(1..=12).contains(&month) {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع شهرا بين 1 و 12"),
            line,
            col,
        ));
    }
    let dim = days_in_month(year, month as u32);
    if day < 1 || day > dim as i64 {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع يوما بين 1 و {dim} في هذا الشهر"),
            line,
            col,
        ));
    }
    if !(0..=23).contains(&hour) {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع ساعة بين 0 و 23"),
            line,
            col,
        ));
    }
    if !(0..=59).contains(&minute) {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع دقيقة بين 0 و 59"),
            line,
            col,
        ));
    }
    if !(0..=59).contains(&second) {
        return Err(RioError::runtime(
            format!("الدالة {display} تتوقع ثانية بين 0 و 59"),
            line,
            col,
        ));
    }
    let days = days_from_civil(year, month as u32, day as u32);
    let naive = days * 86400 + hour * 3600 + minute * 60 + second;
    let epoch = if utc {
        naive
    } else {
        let off1 = local_offset_at(naive, display, line, col)?;
        let e1 = naive - off1;
        let off2 = local_offset_at(e1, display, line, col)?;
        if off2 == off1 {
            e1
        } else {
            naive - off2
        }
    };
    datetime_from_epoch(epoch, utc, display, line, col)
}

/// `أضف`: shift a date by `delta` seconds, keeping
/// the dict's own clock (UTC stays UTC, local stays
/// local — the fields are recomputed in that clock).
pub fn shift(
    dt: &DateTime,
    delta: i64,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<DateTime> {
    let epoch = dt.epoch.checked_add(delta).ok_or_else(|| {
        RioError::runtime(
            format!("الدالة {display} تجاوزت نطاق الثواني"),
            line,
            col,
        )
    })?;
    datetime_from_epoch(epoch, dt.utc, display, line, col)
}

/// `فرق`: seconds from `b` to `a` (a - b).
pub fn diff(a: &DateTime, b: &DateTime, display: &str, line: usize, col: usize) -> RioResult<i64> {
    a.epoch.checked_sub(b.epoch).ok_or_else(|| {
        RioError::runtime(
            format!("الدالة {display} تجاوز الفرق نطاق الثواني"),
            line,
            col,
        )
    })
}

// --- Formatting (Eastern Arabic digits) ---

const ARABIC_DIGITS: [char; 10] = ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];

/// حوّل الأرقام اللاتينية في نص إلى أرقام مشرقية.
pub(crate) fn eastern(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0'..='9' => ARABIC_DIGITS[c as usize - '0' as usize],
            other => other,
        })
        .collect()
}

/// `نص`: «٢٠٢٦-١١-٠٦ ١٩:٤٥:٣٠» — the same layout as
/// the flat `التاريخ_الآن` builtin, with Eastern
/// Arabic digits (numbers display Eastern everywhere
/// in the language; strings print raw, so the
/// conversion happens here).
pub fn format_datetime(dt: &DateTime) -> String {
    eastern(&format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
    ))
}

/// `نص_التاريخ`: «٢٠٢٦-١١-٠٦».
pub fn format_date(dt: &DateTime) -> String {
    eastern(&format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day))
}

/// `نص_الوقت`: «١٩:٤٥:٣٠».
pub fn format_time(dt: &DateTime) -> String {
    eastern(&format!(
        "{:02}:{:02}:{:02}",
        dt.hour, dt.minute, dt.second
    ))
}

/// `اسم_اليوم`: 0 = الأحد .. 6 = السبت.
pub fn weekday_name(weekday: u32) -> &'static str {
    const NAMES: [&str; 7] = [
        "الأحد", "الاثنين", "الثلاثاء", "الأربعاء", "الخميس", "الجمعة", "السبت",
    ];
    NAMES[weekday as usize]
}

/// `اسم_الشهر`: 1 = يناير .. 12 = ديسمبر.
pub fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "يناير", "فبراير", "مارس", "أبريل", "مايو", "يونيو", "يوليو", "أغسطس", "سبتمبر",
        "أكتوبر", "نوفمبر", "ديسمبر",
    ];
    NAMES[(month - 1) as usize]
}

// --- Dict conversions ---

/// A date dict: سنة، شهر، يوم، ساعة، دقيقة، ثانية،
/// يوم_الأسبوع (0 = الأحد)، ثواني (يونكس)، عالمي
/// (صحيح = UTC، خطأ = محلي).
pub fn to_dict(dt: &DateTime) -> Value {
    Value::Dict(vec![
        ("سنة".to_string(), Value::Number(dt.year as f64)),
        ("شهر".to_string(), Value::Number(dt.month as f64)),
        ("يوم".to_string(), Value::Number(dt.day as f64)),
        ("ساعة".to_string(), Value::Number(dt.hour as f64)),
        ("دقيقة".to_string(), Value::Number(dt.minute as f64)),
        ("ثانية".to_string(), Value::Number(dt.second as f64)),
        ("يوم_الأسبوع".to_string(), Value::Number(dt.weekday as f64)),
        ("ثواني".to_string(), Value::Number(dt.epoch as f64)),
        ("عالمي".to_string(), Value::Bool(dt.utc)),
    ])
}

fn dict_number(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<i64> {
    match pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v) {
        Some(Value::Number(x)) if x.fract() == 0.0 && x.abs() < 9.0e15 => Ok(*x as i64),
        _ => Err(RioError::typ(
            format!("الدالة {display} تتوقع قاموس تاريخ فيه {key} عددا صحيحا"),
            line,
            col,
        )),
    }
}

fn dict_bool(
    pairs: &[(String, Value)],
    key: &str,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<bool> {
    match pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(RioError::typ(
            format!("الدالة {display} تتوقع قاموس تاريخ فيه {key} منطقيا"),
            line,
            col,
        )),
    }
}

/// Read a date dict back. `ثواني` is the source of
/// truth when present (fields are recomputed from it,
/// so a dict is always self-consistent); without it
/// the components are used (and validated) — so a
/// hand-built dict of سنة/شهر/يوم/... works too.
/// `عالمي` defaults to local when absent.
pub fn from_dict(
    v: &Value,
    display: &str,
    line: usize,
    col: usize,
) -> RioResult<DateTime> {
    let pairs = match v {
        Value::Dict(p) => p,
        other => {
            return Err(RioError::typ(
                format!(
                    "الدالة {display} تتوقع قاموس تاريخ لكن أعطيت {}",
                    other.type_name()
                ),
                line,
                col,
            ))
        }
    };
    let has = |k: &str| pairs.iter().any(|(key, _)| key == k);
    let utc = if has("عالمي") {
        dict_bool(pairs, "عالمي", display, line, col)?
    } else {
        false
    };
    if has("ثواني") {
        let epoch = dict_number(pairs, "ثواني", display, line, col)?;
        datetime_from_epoch(epoch, utc, display, line, col)
    } else {
        let year = dict_number(pairs, "سنة", display, line, col)?;
        let month = dict_number(pairs, "شهر", display, line, col)?;
        let day = dict_number(pairs, "يوم", display, line, col)?;
        let hour = dict_number(pairs, "ساعة", display, line, col)?;
        let minute = dict_number(pairs, "دقيقة", display, line, col)?;
        let second = dict_number(pairs, "ثانية", display, line, col)?;
        datetime_from_components(year, month, day, hour, minute, second, utc, display, line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(secs: i64) -> DateTime {
        datetime_from_epoch(secs, true, "اختبار", 1, 1).unwrap()
    }

    #[test]
    fn civil_roundtrip_known_dates() {
        // (year, month, day, weekday 0=Sunday)
        for (y, m, d, wd) in [
            (1970, 1, 1, 4),   // Thursday
            (1969, 12, 31, 3), // Wednesday
            (2000, 2, 29, 2),  // Tuesday, leap day
            (1900, 3, 1, 4),   // Thursday (1900 not a leap year)
            (2026, 11, 6, 5),  // Friday
            (1, 1, 1, 1),      // Monday
            (9999, 12, 31, 5), // Friday
            (2024, 2, 29, 4),  // Thursday, leap day
        ] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d), "roundtrip {y}-{m}-{d}");
            assert_eq!(weekday_from_days(days), wd, "weekday {y}-{m}-{d}");
        }
    }

    #[test]
    fn leap_and_month_lengths() {
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(is_leap(2024));
        assert!(!is_leap(2100));
        assert!(!is_leap(2026));
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn utc_epoch_zero_and_known_moments() {
        let e0 = utc(0);
        assert_eq!(
            (e0.year, e0.month, e0.day, e0.hour, e0.minute, e0.second),
            (1970, 1, 1, 0, 0, 0)
        );
        assert_eq!(e0.weekday, 4);
        assert_eq!(format_datetime(&e0), "١٩٧٠-٠١-٠١ ٠٠:٠٠:٠٠");
        assert_eq!(format_date(&e0), "١٩٧٠-٠١-٠١");
        assert_eq!(format_time(&e0), "٠٠:٠٠:٠٠");
        // 2026-11-06 12:00:00 UTC is a Friday in November.
        let d = utc(days_from_civil(2026, 11, 6) * 86400 + 12 * 3600);
        assert_eq!((d.year, d.month, d.day), (2026, 11, 6));
        assert_eq!(d.weekday, 5);
        assert_eq!(weekday_name(d.weekday), "الجمعة");
        assert_eq!(month_name(d.month), "نوفمبر");
        assert_eq!(format_datetime(&d), "٢٠٢٦-١١-٠٦ ١٢:٠٠:٠٠");
        // One second before the epoch: 1969-12-31 23:59:59.
        let neg = utc(-1);
        assert_eq!(
            (neg.year, neg.month, neg.day, neg.hour, neg.minute, neg.second),
            (1969, 12, 31, 23, 59, 59)
        );
    }

    #[test]
    fn components_validation_errors() {
        let bad = |y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64| {
            datetime_from_components(y, m, d, h, mi, s, true, "اختبار", 1, 1)
        };
        assert!(bad(2026, 13, 1, 0, 0, 0).is_err());
        assert!(bad(2026, 0, 1, 0, 0, 0).is_err());
        assert!(bad(2026, 2, 30, 0, 0, 0).is_err());
        assert!(bad(2023, 2, 29, 0, 0, 0).is_err());
        assert!(bad(2026, 11, 6, 24, 0, 0).is_err());
        assert!(bad(2026, 11, 6, 0, 60, 0).is_err());
        assert!(bad(2026, 11, 6, 0, 0, 60).is_err());
        assert!(bad(0, 1, 1, 0, 0, 0).is_err());
        assert!(bad(10000, 1, 1, 0, 0, 0).is_err());
        // Leap day is fine in a leap year.
        assert!(bad(2024, 2, 29, 23, 59, 59).is_ok());
        // Round-trip through components <-> epoch.
        let dt = datetime_from_components(2026, 11, 6, 19, 45, 30, true, "اختبار", 1, 1)
            .unwrap();
        assert_eq!(utc(dt.epoch), dt);
    }

    #[test]
    fn dict_roundtrip_and_handbuilt() {
        let dt = utc(days_from_civil(2026, 11, 6) * 86400 + 12 * 3600);
        let v = to_dict(&dt);
        assert_eq!(from_dict(&v, "اختبار", 1, 1).unwrap(), dt);
        // Hand-built dict without ثواني: components rule.
        // (عالمي: صحيح keeps the test on the UTC path,
        // which works on every platform; the local path
        // is covered by the integration tests.)
        let hand = Value::Dict(vec![
            ("سنة".to_string(), Value::Number(2026.0)),
            ("شهر".to_string(), Value::Number(11.0)),
            ("يوم".to_string(), Value::Number(6.0)),
            ("ساعة".to_string(), Value::Number(12.0)),
            ("دقيقة".to_string(), Value::Number(0.0)),
            ("ثانية".to_string(), Value::Number(0.0)),
            ("عالمي".to_string(), Value::Bool(true)),
        ]);
        let back = from_dict(&hand, "اختبار", 1, 1);
        assert!(back.is_ok(), "hand-built dict: {:?}", back);
        let back = back.unwrap();
        assert_eq!((back.year, back.month, back.day), (2026, 11, 6));
        assert_eq!(
            back.epoch,
            days_from_civil(2026, 11, 6) * 86400 + 12 * 3600
        );
        // Missing a field: honest error.
        let missing = Value::Dict(vec![("سنة".to_string(), Value::Number(2026.0))]);
        assert!(from_dict(&missing, "اختبار", 1, 1).is_err());
        // Not a dict at all.
        assert!(from_dict(&Value::Number(5.0), "اختبار", 1, 1).is_err());
    }

    #[test]
    fn diff_and_shift() {
        let a = utc(days_from_civil(2026, 11, 6) * 86400);
        let b = utc(days_from_civil(2026, 11, 5) * 86400);
        assert_eq!(diff(&a, &b, "اختبار", 1, 1).unwrap(), 86400);
        assert_eq!(diff(&b, &a, "اختبار", 1, 1).unwrap(), -86400);
        let c = shift(&a, 3600, "اختبار", 1, 1).unwrap();
        assert_eq!((c.hour, c.minute), (1, 0));
        assert_eq!(c.epoch, a.epoch + 3600);
        assert!(c.utc);
        let d = shift(&a, -86400, "اختبار", 1, 1).unwrap();
        assert_eq!((d.year, d.month, d.day), (2026, 11, 5));
    }

    // TZif parsing against real zone files (Unix only:
    // Windows has no zoneinfo tree).
    #[test]
    #[cfg(unix)]
    fn tzif_offsets_real_zones() {
        use std::path::Path;
        let line = 1;
        let col = 1;
        // Cairo: +2 in winter, +3 in DST summer (DST
        // 2026: 24 Apr – 29 Oct).
        let cairo = Path::new("/usr/share/zoneinfo/Africa/Cairo");
        let summer = days_from_civil(2026, 7, 1) * 86400 + 12 * 3600;
        assert_eq!(offset_in_file(cairo, summer, line, col).unwrap(), 10800);
        let winter = days_from_civil(2026, 11, 6) * 86400 + 12 * 3600;
        assert_eq!(offset_in_file(cairo, winter, line, col).unwrap(), 7200);
        let old = days_from_civil(1970, 1, 1) * 86400;
        assert_eq!(offset_in_file(cairo, old, line, col).unwrap(), 7200);
        // UTC zone: always zero.
        let utc_zone = Path::new("/usr/share/zoneinfo/Etc/UTC");
        assert_eq!(offset_in_file(utc_zone, summer, line, col).unwrap(), 0);
        assert_eq!(offset_in_file(utc_zone, old, line, col).unwrap(), 0);
        // Not a TZif file: honest corruption error.
        assert!(offset_in_file(Path::new("/etc/hostname"), summer, line, col).is_err());
        // Missing file: honest read error.
        assert!(offset_in_file(
            Path::new("/usr/share/zoneinfo/لا_يوجد/أبدا"),
            summer,
            line,
            col
        )
        .is_err());
    }

    // On Windows the local functions refuse honestly.
    #[test]
    #[cfg(windows)]
    fn local_refused_on_windows() {
        let err = local_offset_at(0, "وقت.التاريخ", 1, 1).unwrap_err();
        assert!(err.to_string().contains("غير مدعوم على هذه المنصة"));
    }
}
