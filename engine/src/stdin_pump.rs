//! مضخة stdin (المهمة 22 — شبكة.نزّل + إلغاء أمر معلق).
//!
//! خيط خلفية واحد يملك stdin ويقرأ أسطره في قناة؛ وكل
//! قرّاء stdin في المحرك يمرون به (لا قراءة مباشرة بعد
//! اليوم): حلقة REPL و`ادخل` في المحركين وانتظار نتيجة
//! التحكم وحلقة انتظار الجلب. والسبب: الجلب القابل
//! للإلغاء يشغّل العمل على خيط عامل، والخيط المنفذ
//! ينتظر نتيجته — فإن قرأ stdin مباشرة علق، وإن قرأه
//! خيط ثانٍ سرق أسطر الموجّه. المضخة تحلها: قارئ
//! واحد، وقناة واحدة، وطابور معلّق للأسطر العادية
//! التي تصل أثناء انتظار جلب (فتُقرأ بعده — لا يُفقد
//! سطر كُتب أثناء الجلب).
//!
//! سطر الإلغاء (`\u{0001}نيفا-إلغاء`) يراه مستهلك
//! واحد فقط — حلقة انتظار الجلب؛ وكل مستهلك آخر
//! يتخطاه بصمت (فالضغط على «ألغِ الأمر» بلا جلب جارٍ
//! لا أثر له ولا يأكل إدخالا). ونهاية الإدخال علم
//! منفصل (لا حدث في القناة — فالحقن الاختباري يبقي
//! مرسِلا حيّا بعدها).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

/// سطر الإلغاء كاملا (يرسله التطبيق عبر stdin حين
/// يضغط المستخدم «ألغِ الأمر»).
pub(crate) const CANCEL_LINE: &str = "\u{0001}نيفا-إلغاء";

/// هل هذا السطر سطر إلغاء؟ (البادئة `\u{0001}` تجعله
/// مميزا عن أي سطر مستخدم — كما إشارات التحكم في م-6.)
pub(crate) fn is_cancel_line(text: &str) -> bool {
    text.trim_end_matches(['\n', '\r']) == CANCEL_LINE
}

/// نتيجة قراءة خام واحدة (بلا تخطي).
pub(crate) enum RawLine {
    /// سطر كامل (بالسطر الجديد كما قرأه `read_line`).
    Line(String),
    /// لا سطر في المهلة.
    Timeout,
    /// انتهى الإدخال (EOF) — لا stdin بعد.
    Eof,
}

struct PumpState {
    /// مستقبل القناة (خلف قفل — المستهلكون على الخيط
    /// المنفذ وحده، واحدا بعد الآخر، فلا تزامن).
    rx: Mutex<mpsc::Receiver<String>>,
    /// مرسِل محفوظ للحقن الاختباري (يبقى حيّا بعد
    /// خروج خيط المضخة عند EOF — وإلا انقطعت القناة
    /// وتعذّر الحقن). اختبار فقط — فلا قارئ له في
    /// الإنتاج.
    #[cfg(test)]
    tx: Mutex<mpsc::Sender<String>>,
    /// أسطر عادية وصلت أثناء انتظار جلب — تُقرأ أولا
    /// قبل القناة (لا سطور إلغاء هنا أبدا).
    pending: Mutex<VecDeque<String>>,
    /// نهاية الإدخال: ضبطها خيط المضخة عند EOF.
    eof: AtomicBool,
}

static PUMP: OnceLock<PumpState> = OnceLock::new();

/// ابدأ خيط المضخة كسولا (مرة واحدة لكل عملية)،
/// وأعد الحالة العامة.
fn ensure_pump() -> &'static PumpState {
    PUMP.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        let state = PumpState {
            rx: Mutex::new(rx),
            #[cfg(test)]
            tx: Mutex::new(tx.clone()),
            pending: Mutex::new(VecDeque::new()),
            eof: AtomicBool::new(false),
        };
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            loop {
                let mut line = String::new();
                // قفل لكل قراءة لا لكل الحلقة (نمط المهمة
                // 20): المخزن المؤقت يعيش في Stdin العام
                // لا في الحارس، فلا يُفقد سطر مخزّن.
                let n = {
                    let mut input = stdin.lock();
                    use std::io::BufRead;
                    input.read_line(&mut line)
                };
                match n {
                    Ok(0) => {
                        // EOF: لا إدخال بعد — والعلم (لا
                        // حدث في القناة) يبقي الحقن ممكنا.
                        PUMP.get().map(|s| s.eof.store(true, Ordering::SeqCst));
                        return;
                    }
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        PUMP.get().map(|s| s.eof.store(true, Ordering::SeqCst));
                        return;
                    }
                }
            }
        });
        state
    })
}

/// ادفع سطرا عاديا إلى الطابور المعلق (تستخدمه حلقة
/// انتظار الجلب للأسطر غير الإلغاء — فتُقرأ بعد عودة
/// الجلب). أصلي فقط — فحلقة الجلب نفسها لا تُبنى
/// في المتصفح.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn push_pending(line: String) {
    let state = ensure_pump();
    if let Ok(mut q) = state.pending.lock() {
        q.push_back(line);
    }
}

/// تجاهل سطور إلغاء قديمة في القناة (أُرسلت قبل بدء
/// جلب): تُسقَط بصمت، والأسطر العادية تُدفع إلى
/// الطابور المعلق. بلا حجب (مهلة صفر) — تُستدعى في
/// بداية كل جلب قابل للإلغاء، فلا يلغي سطر قديم
/// جلبا لم يبدأ بعد. أصلي فقط — كـpush_pending.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn discard_stale_cancel_lines() {
    loop {
        match next_channel_line_timeout(Duration::from_millis(0)) {
            RawLine::Line(l) => {
                if is_cancel_line(&l) {
                    continue;
                } else {
                    push_pending(l);
                }
            }
            _ => break,
        }
    }
}

/// سطر خام واحد بحجب كامل (لا تخطي لسطور الإلغاء —
/// المتصل يقرر). `None` تعني نهاية الإدخال.
pub(crate) fn next_raw_line() -> Option<String> {
    loop {
        match next_raw_line_timeout(Duration::from_secs(3600)) {
            RawLine::Line(l) => return Some(l),
            RawLine::Timeout => continue,
            RawLine::Eof => return None,
        }
    }
}

/// سطر خام واحد بمهلة (لا تخطي لسطور الإلغاء —
/// المتصل يقرر).
pub(crate) fn next_raw_line_timeout(dur: Duration) -> RawLine {
    let state = ensure_pump();
    // الطابور المعلق أولا (أسطر عادية فقط).
    if let Ok(mut q) = state.pending.lock() {
        if let Some(l) = q.pop_front() {
            return RawLine::Line(l);
        }
    }
    next_channel_line_timeout(dur)
}

/// سطر خام من القناة وحدها (بلا الطابور المعلق) —
/// لحلقة انتظار الجلب وتجاهل القديم: فالطابور أسطر
/// سبق أن استلمتها الحلقة نفسها، وإعادة قراءتها
/// منه تدور إلى الأبد (تُسحب ثم تُدفع ثانية).
pub(crate) fn next_channel_line_timeout(dur: Duration) -> RawLine {
    let state = ensure_pump();
    let deadline = std::time::Instant::now() + dur;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        // شريحة 50ms: لفحص علم EOF دوريا — فبعد EOF
        // لا يأتي سطر أبدا، والانتظار الكامل كان سيعلق.
        let slice = left.min(Duration::from_millis(50));
        let got = {
            let guard = state.rx.lock().unwrap_or_else(|e| e.into_inner());
            guard.recv_timeout(slice)
        };
        match got {
            Ok(l) => return RawLine::Line(l),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if state.eof.load(Ordering::SeqCst) {
                    return RawLine::Eof;
                }
                if std::time::Instant::now() >= deadline {
                    return RawLine::Timeout;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return RawLine::Eof,
        }
    }
}

/// سطر stdin التالي للمستهلكين العاديين (REPL وادخل
/// وانتظار النتيجة): حاجب، يتخطى سطور الإلغاء بصمت.
/// `None` تعني نهاية الإدخال (كما `read_line` حين
/// تعيد 0 — فـ `ادخل` يرجع نصا فارغا كما كان).
pub(crate) fn stdin_line() -> Option<String> {
    loop {
        match next_raw_line() {
            Some(l) if is_cancel_line(&l) => continue,
            other => return other,
        }
    }
}

/// حقن اختباري: ادفع سطرا إلى القناة مباشرة (بلا
/// خيط مضخة وبلا stdin حقيقي) — لاختبارات الإلغاء
/// والطابور المعلق. السطر كما يُرسل (بالسطر الجديد
/// عادة — كما يقرأه `read_line`).
#[cfg(test)]
pub(crate) fn inject_stdin_line(line: String) {
    let state = ensure_pump();
    if let Ok(tx) = state.tx.lock() {
        let _ = tx.send(line);
    }
}

/// تسلسل اختبارات المضخة: القناة والطابور عامّان
/// لكل العملية، فالاختبارات التي تحقن أسطرا أو تقرأ
/// من المضخة تتسلسل بهذا القفل (نمط NET_TEST_LOCK
/// في network.rs).
#[cfg(test)]
static PUMP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn pump_test_lock() -> std::sync::MutexGuard<'static, ()> {
    PUMP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_line_recognized() {
        assert!(is_cancel_line("\u{0001}نيفا-إلغاء\n"));
        assert!(is_cancel_line("\u{0001}نيفا-إلغاء"));
        assert!(!is_cancel_line("ألغِ الأمر\n"));
        assert!(!is_cancel_line("\u{0001}نيفا-نتيجة:{}\n"));
        assert!(!is_cancel_line(""));
    }

    #[test]
    fn injected_line_arrives_and_cancel_is_skipped() {
        let _guard = pump_test_lock();
        inject_stdin_line("\u{0001}نيفا-إلغاء\n".to_string());
        inject_stdin_line("محمود\n".to_string());
        // stdin_line يتخطى الإلغاء ويرجع السطر العادي.
        assert_eq!(stdin_line().as_deref(), Some("محمود\n"));
    }

    #[test]
    fn pending_lines_read_first() {
        let _guard = pump_test_lock();
        push_pending("الأول\n".to_string());
        inject_stdin_line("الثاني\n".to_string());
        assert_eq!(stdin_line().as_deref(), Some("الأول\n"));
        assert_eq!(stdin_line().as_deref(), Some("الثاني\n"));
    }

    #[test]
    fn raw_timeout_returns_timeout() {
        let _guard = pump_test_lock();
        // بلا سطر في المهلة: Timeout عادة — أو Eof
        // حين يكون stdin مغلقا أصلا (/dev/null في CI):
        // كلاهما يعني «لا سطر وصل» (لا حجب أبدا).
        assert!(matches!(
            next_raw_line_timeout(Duration::from_millis(120)),
            RawLine::Timeout | RawLine::Eof
        ));
    }
}
