//! جهاز — حوار تفاعلي + تنبيه + نطق + صوت + حساس
//! + اسمع + بطارية (المهام 20 و21 و23 و24 و25 و26
//! من خطة البناء، قدرات واجهات الجهاز — المرحلة 4).
//!
//! المبدأ: المحرك عملية فرعية في التطبيق، فيطلب
//! الحوار أو التنبيه عبر قناة التحكم المنظمة على
//! stderr (نمط م-6: `\u{0001}نيفا-تحكم:…` حين يكون
//! `NIVA_CONTROL_CHANNEL=stderr` مضبوطا) وينتظر
//! النتيجة سطرا منظما على stdin. لا واجهة في
//! المحرك أبدا — المحرك يطلب فقط، والتطبيق يعرض
//! ويعيد. وخارج أندرويد لا حوار ولا تنبيه إطلاقا
//! (خطأ عربي واضح)، فلا سطر تحكم يُرسَل ولا انتظار
//! يُدخَل.
//!
//! التصميم الكامل — بما فيه بروتوكول الطلب/النتيجة
//! وأنواع الحوار التسعة في v1 وتأجيل «صوتي» وبوابة
//! الإذن وترتيب الفحوص — في CATALOG-12-DEVICE-DIALOG.md؛
//! وتصميم التنبيه (معرّفاته وسقفه وبوابة إذن
//! الإشعارات وترتيب فحوصه) في CATALOG-13-DEVICE-NOTIFY.md.

use crate::error::{RioError, RioResult};
use crate::value::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[cfg(target_arch = "wasm32")]
/// رفض المتصفح المشترك (يستخدمه أيضا ملفات SAF
/// في files.rs — المهمة 28).
pub(crate) const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// بادئة سطر طلب الحوار على stderr (نمط م-6) —
/// أندرويد فقط (لا مرسِل لها في غيره).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:حوار:";
/// بادئة سطر طلب التنبيه على stderr (المهمة 21) —
/// أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const NOTIFY_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:نبّه:";
/// بادئة سطر طلب إزالة التنبيه على stderr (المهمة 21)
/// — أندرويد فقط؛ بلا انتظار (الإلغاء متزامن ولا
/// يفشل فشلا ذا معنى حين يكون المعرّف حيّا).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const REMOVE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:أزل_تنبيه:";
/// بادئة سطر طلب النطق على stderr (المهمة 23) —
/// أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SPEAK_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:انطق:";
/// بادئة سطر طلب سرد محركات النطق على stderr
/// (المهمة 23) — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const ENGINES_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:محركات_النطق:";
/// بادئة سطر طلب تشغيل صوت على stderr (المهمة 24)
/// — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const PLAY_AUDIO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:شغّل_صوت:";
/// بادئة سطر طلب إيقاف صوت على stderr (المهمة 24)
/// — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const STOP_AUDIO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:أوقف_صوت:";
/// بادئة سطر طلب قراءة حساس على stderr (المهمة 25)
/// — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SENSOR_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:حساس:";
/// بادئة سطر طلب استماع على stderr (المهمة 26) —
/// أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const LISTEN_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اسمع:";
/// بادئة سطر طلب قراءة بطارية على stderr (المهمة 26)
/// — أندرويد فقط.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const BATTERY_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:بطارية:";
/// بادئات الدفعة 4 من الكتالوج (البنود 22–34) —
/// أندرويد فقط؛ كلها على النمط نفسه (طلب JSON ثم
/// انتظار نتيجة واحدة).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SHARE_FILE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:شارك_ملف:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SHARE_TEXT_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:شارك_نص:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const OPEN_FILE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:افتح_ملف:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const OPEN_URL_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:افتح_رابط:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CLIPBOARD_SET_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:انسخ_للحافظة:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CLIPBOARD_GET_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:الحافظة:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const PICK_FILE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اختر_ملف:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const LOCATION_LAST_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:موقع:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const LOCATION_ONCE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:موقع_حي:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CAMERAS_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:كاميرات:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const CAPTURE_PHOTO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:التقط_صورة:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const WIFI_INFO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:واي_فاي:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const WIFI_SCAN_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:مسح_واي_فاي:";
/// بادئات الدفعة 4ب من الكتالوج (البنود 61 و62 و66–72
/// و79 و105 و106 و119) — أندرويد فقط؛ النمط نفسه.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const COPY_LAST_ERROR_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:انسخ_آخر_خطأ:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SHARE_SESSION_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:شارك_الجلسة:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const PAUSE_AUDIO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:أوقف_صوت_مؤقتا:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const RESUME_AUDIO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:استأنف_صوت:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const AUDIO_INFO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:معلومات_صوت:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const LIST_SENSORS_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:حساسات:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const VOLUME_LEVEL_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:مستوى_الصوت:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SET_VOLUME_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اضبط_مستوى_الصوت:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const VIBRATE_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اهتز:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const TORCH_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:مصباح:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const RECORD_AUDIO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:سجل_صوت:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const BRIGHTNESS_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:سطوع:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const TOAST_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:توست:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const AUDIO_DEVICE_INFO_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:معلومات_الصوت:";
/// بادئات الدفعة 5 من الكتالوج (البنود 98 و99
/// خطوة 1) — أندرويد فقط؛ النمط نفسه.
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const DOWNLOAD_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:نزل:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const READ_DOCUMENT_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اقرأ_مستند:";
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const LIST_FOLDER_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:اعرض_مجلد:";
/// بادئة سطر نتيجة التحكم على stdin — أندرويد فقط،
/// وتبقى في اختبارات الوحدة (انتظار SAF القابل
/// للإلغاء في files.rs — المهمة 28).
#[cfg(any(all(target_os = "android", not(target_arch = "wasm32")), test))]
pub(crate) const RESULT_PREFIX: &str = "\u{0001}نيفا-نتيجة:";

/// طلب حوار متحقق منه (خالص — يعمل على كل المنصات).
#[derive(Debug, Clone, PartialEq)]
pub struct DialogRequest {
    /// نوع الحوار: تأكيد، نص، رقم، اختيار واحد،
    /// اختيارات، قائمة، عداد، تاريخ، وقت.
    pub kind: String,
    /// سؤال الحوار (غير فارغ أبدا).
    pub title: String,
    /// خيارات أنواع الاختيار الثلاثة فقط.
    pub options: Option<Vec<String>>,
    /// حدّا العداد فقط.
    pub min: Option<i64>,
    /// حدّا العداد فقط.
    pub max: Option<i64>,
    /// افتراضي العداد فقط (إن أُعطي).
    pub default: Option<i64>,
}

fn typ_error(display: &str, what: &str, line: usize, col: usize) -> RioError {
    RioError::typ(format!("الدالة {} {}", display, what), line, col)
}

/// التحقق الخالص من وسائط `جهاز.حوار` — يعمل على كل
/// المنصات (نمط `تشفير`: أخطاء الوسائط قبل فحص المنصة).
pub fn validate_dialog_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<DialogRequest> {
    const DISPLAY: &str = "جهاز.حوار";
    if args.len() < 2 || args.len() > 3 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 أو 3 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let kind = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لنوع الحوار لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    let title = match &args[1] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لعنوان الحوار لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if title.is_empty() {
        return Err(RioError::runtime("عنوان الحوار فارغ", line, col));
    }
    match kind.as_str() {
        // الأنواع ذات المعاملين فقط.
        "تأكيد" | "نص" | "رقم" | "تاريخ" | "وقت" => {
            if args.len() != 2 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 2 معاملات لنوع {} لكن أعطيت {}",
                        DISPLAY,
                        kind,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            Ok(DialogRequest {
                kind,
                title,
                options: None,
                min: None,
                max: None,
                default: None,
            })
        }
        // «صوتي» نوع معروف مؤجل (لا مجهول): يحتاج إذن
        // ميكروفون وقت التشغيل + تعرفا صوتيا.
        "صوتي" => Err(RioError::runtime(
            "الحوار الصوتي غير متاح بعد — يحتاج إذن ميكروفون وتعرفا صوتيا",
            line,
            col,
        )),
        // أنواع الاختيار الثلاثة: قائمة نصوص ثالثة.
        "اختيار واحد" | "اختيارات" | "قائمة" => {
            if args.len() != 3 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 3 معاملات لنوع {} لكن أعطيت {}",
                        DISPLAY,
                        kind,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            let options = match &args[2] {
                Value::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            Value::Str(s) => out.push(s.clone()),
                            other => {
                                return Err(typ_error(
                                    DISPLAY,
                                    &format!(
                                        "تتوقع نصوصا في قائمة الخيارات لكن أعطيت {}",
                                        other.type_name()
                                    ),
                                    line,
                                    col,
                                ))
                            }
                        }
                    }
                    out
                }
                other => {
                    return Err(typ_error(
                        DISPLAY,
                        &format!(
                            "تتوقع قائمة خيارات لكن أعطيت {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    ))
                }
            };
            if options.is_empty() {
                return Err(RioError::runtime("قائمة الخيارات فارغة", line, col));
            }
            Ok(DialogRequest {
                kind,
                title,
                options: Some(options),
                min: None,
                max: None,
                default: None,
            })
        }
        // العداد: قاموس حدود ثالث.
        "عداد" => {
            if args.len() != 3 {
                return Err(RioError::runtime(
                    format!(
                        "الدالة {} تتوقع 3 معاملات لنوع عداد لكن أعطيت {}",
                        DISPLAY,
                        args.len()
                    ),
                    line,
                    col,
                ));
            }
            let dict = match &args[2] {
                Value::Dict(pairs) => pairs,
                other => {
                    return Err(typ_error(
                        DISPLAY,
                        &format!(
                            "تتوقع قاموس حدود للعداد لكن أعطيت {}",
                            other.type_name()
                        ),
                        line,
                        col,
                    ))
                }
            };
            let mut min: Option<i64> = None;
            let mut max: Option<i64> = None;
            let mut default: Option<i64> = None;
            for (key, value) in dict {
                let num = match value {
                    Value::Number(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => *x as i64,
                    _ => {
                        return Err(typ_error(
                            DISPLAY,
                            &format!("تتوقع عددا صحيحا لقيمة «{}» في قاموس العداد", key),
                            line,
                            col,
                        ))
                    }
                };
                match key.as_str() {
                    "أدنى" => min = Some(num),
                    "أقصى" => max = Some(num),
                    "افتراضي" => default = Some(num),
                    _ => {
                        return Err(RioError::runtime(
                            format!("مفتاح غير معروف في قاموس العداد: {}", key),
                            line,
                            col,
                        ))
                    }
                }
            }
            let min = match min {
                Some(v) => v,
                None => {
                    return Err(RioError::runtime(
                        "قاموس العداد يفتقد «أدنى»",
                        line,
                        col,
                    ))
                }
            };
            let max = match max {
                Some(v) => v,
                None => {
                    return Err(RioError::runtime(
                        "قاموس العداد يفتقد «أقصى»",
                        line,
                        col,
                    ))
                }
            };
            if min > max {
                return Err(RioError::runtime(
                    "الحد الأدنى أكبر من الحد الأقصى",
                    line,
                    col,
                ));
            }
            if let Some(d) = default {
                if d < min || d > max {
                    return Err(RioError::runtime(
                        "القيمة الافتراضية خارج النطاق",
                        line,
                        col,
                    ));
                }
            }
            Ok(DialogRequest {
                kind,
                title,
                options: None,
                min: Some(min),
                max: Some(max),
                default,
            })
        }
        _ => Err(RioError::runtime(
            format!("نوع حوار غير معروف: {}", kind),
            line,
            col,
        )),
    }
}

/// نقطة الدخول: `جهاز.حوار(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم الطلب والانتظار.
pub fn show_dialog(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let req = validate_dialog_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        // التحقق جرى أعلاه (أخطاء الوسائط تعمل في
        // المتصفح)؛ والرفض هنا لا يحتاج الطلب نفسه.
        let _ = &req;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        show_dialog_native(&req, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn show_dialog_native(req: &DialogRequest, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = req;
        Err(RioError::runtime(
            "جهاز.حوار غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            request_and_wait(req, line, col)
        } else {
            Err(RioError::runtime("الحوار متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// سقف التنبيهات الحية (المهمة 21 — على نمط سقف
/// المقابس والقنوات: 64).
pub const MAX_LIVE_NOTIFICATIONS: usize = 64;

/// عدّاد معرّفات التنبيه — رتيب من 1، لا يُعاد أبدا
/// حتى بعد الإزالة (على نمط معرّفات المقابس والقنوات).
static NEXT_NOTIFY_ID: AtomicU64 = AtomicU64::new(1);

/// مجموعة المعرّفات الحية — مشتركة بين المحركين
/// (على نمط سجل كتل `ذاكرة`).
fn live_notifications() -> &'static Mutex<HashSet<u64>> {
    static LIVE: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// حجز معرّف جديد (يفشل عند السقف — قبل أي إرسال).
pub fn reserve_notify_id(line: usize, col: usize) -> RioResult<u64> {
    let mut live = live_notifications()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if live.len() >= MAX_LIVE_NOTIFICATIONS {
        return Err(RioError::runtime(
            "وصل عدد التنبيهات الحية إلى الحد (64)",
            line,
            col,
        ));
    }
    let id = NEXT_NOTIFY_ID.fetch_add(1, Ordering::SeqCst);
    live.insert(id);
    Ok(id)
}

/// تحرير معرّف محجوز (عند فشل النشر — وإلا تسرّب
/// المعرّف وعدّ حيّا وهو غير معروض).
pub fn release_notify_id(id: u64) {
    match live_notifications().lock() {
        Ok(mut live) => {
            live.remove(&id);
        }
        Err(poisoned) => {
            poisoned.into_inner().remove(&id);
        }
    }
}

/// استهلاك معرّف حيّ (للإزالة) — يعيد صحيحا إن كان
/// حيّا، وخطأ إن لم يكن.
pub fn consume_notify_id(id: u64) -> bool {
    match live_notifications().lock() {
        Ok(mut live) => live.remove(&id),
        Err(poisoned) => poisoned.into_inner().remove(&id),
    }
}

/// التحقق الخالص من وسائط `جهاز.نبّه` — يعمل على كل
/// المنصات (نمط `تشفير`: أخطاء الوسائط قبل فحص المنصة).
pub fn validate_notify_args(args: &[Value], line: usize, col: usize) -> RioResult<(String, String)> {
    const DISPLAY: &str = "جهاز.نبه";
    if args.len() != 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 2 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let title = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لعنوان التنبيه لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    let text = match &args[1] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لنص التنبيه لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if title.is_empty() {
        return Err(RioError::runtime("عنوان التنبيه فارغ", line, col));
    }
    if text.is_empty() {
        return Err(RioError::runtime("نص التنبيه فارغ", line, col));
    }
    Ok((title, text))
}

/// التحقق الخالص من وسائط `جهاز.أزل_تنبيه` — معامل
/// واحد عدد صحيح موجب.
pub fn validate_remove_args(args: &[Value], line: usize, col: usize) -> RioResult<u64> {
    const DISPLAY: &str = "جهاز.أزل_تنبيه";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    match &args[0] {
        Value::Number(x) if x.fract() == 0.0 && *x >= 1.0 && x.abs() < 9.0e15 => Ok(*x as u64),
        Value::Number(_) => Err(RioError::typ(
            format!("الدالة {} تتوقع عددا صحيحا موجبا لمعرف التنبيه", DISPLAY),
            line,
            col,
        )),
        other => Err(typ_error(
            DISPLAY,
            &format!("تتوقع عددا لمعرف التنبيه لكن أعطيت {}", other.type_name()),
            line,
            col,
        )),
    }
}

/// تسلسل طلب التنبيه إلى JSON (تهريب يدوي — يعيد
/// استخدام `json_escape`).
pub fn serialize_notify_request(title: &str, text: &str, id: u64) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"عنوان\":");
    out.push_str(&json_escape(title));
    out.push_str(",\"نص\":");
    out.push_str(&json_escape(text));
    out.push_str(&format!(",\"معرف\":{}", id));
    out.push('}');
    out
}

/// تسلسل طلب إزالة التنبيه إلى JSON.
pub fn serialize_remove_request(id: u64) -> String {
    format!("{{\"معرف\":{}}}", id)
}

/// التحقق الخالص من وسائط `جهاز.انطق` — معامل واحد
/// نص غير فارغ (المهمة 23).
pub fn validate_speak_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.انطق";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لنص النطق لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if text.is_empty() {
        return Err(RioError::runtime("نص النطق فارغ", line, col));
    }
    Ok(text)
}

/// التحقق الخالص من وسائط `جهاز.محركات_النطق` —
/// بلا معاملات (المهمة 23).
pub fn validate_engines_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.محركات_النطق";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// تسلسل طلب النطق إلى JSON (تهريب يدوي — يعيد
/// استخدام `json_escape`).
pub fn serialize_speak_request(text: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نص\":");
    out.push_str(&json_escape(text));
    out.push('}');
    out
}

/// تسلسل طلب سرد محركات النطق إلى JSON — كائن فارغ
/// (بلا معاملات).
pub fn serialize_engines_request() -> String {
    "{}".to_string()
}

/// التحقق الخالص من وسائط `جهاز.شغّل_صوت` — معامل
/// واحد نص (مسار ملف صوتي) غير فارغ (المهمة 24).
pub fn validate_play_audio_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.شغل_صوت";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار الصوت لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار الصوت فارغ", line, col));
    }
    Ok(path)
}

/// التحقق الخالص من وسائط `جهاز.أوقف_صوت` —
/// بلا معاملات (المهمة 24).
pub fn validate_stop_audio_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.أوقف_صوت";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// تسلسل طلب تشغيل صوت إلى JSON — المسار المطلق
/// المحلول (تهريب يدوي — يعيد استخدام `json_escape`).
pub fn serialize_play_audio_request(abs_path: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push('}');
    out
}

/// تسلسل طلب إيقاف صوت إلى JSON — كائن فارغ
/// (بلا معاملات).
pub fn serialize_stop_audio_request() -> String {
    "{}".to_string()
}

/// أسماء الحساسات الثمانية في v1 (المهمة 25) —
/// القائمة الكاملة؛ وأي اسم خارجها «غير معروف»
/// (فحص وسائط خالص قبل المنصة — كأنواع الحوار
/// التسعة). المستثنى عمدا: نبض القلب (إذن جسدي)
/// وعدّاد الخطوات (تراكمي) وحساسات الزناد.
pub const SENSOR_NAMES: [&str; 8] = [
    "تسارع", "جيروسكوب", "ضوء", "قرب", "مغناطيسي", "ضغط", "رطوبة", "حرارة",
];

/// التحقق الخالص من وسائط `جهاز.حساس` — معامل
/// واحد نص (اسم حساس من الثمانية) غير فارغ
/// (المهمة 25).
pub fn validate_sensor_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.حساس";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let name = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لاسم الحساس لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if name.is_empty() {
        return Err(RioError::runtime("اسم الحساس فارغ", line, col));
    }
    if !SENSOR_NAMES.contains(&name.as_str()) {
        return Err(RioError::runtime(
            format!("حساس غير معروف: {}", name),
            line,
            col,
        ));
    }
    Ok(name)
}

/// تسلسل طلب قراءة حساس إلى JSON (تهريب يدوي —
/// يعيد استخدام `json_escape`).
pub fn serialize_sensor_request(name: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"اسم\":");
    out.push_str(&json_escape(name));
    out.push('}');
    out
}

/// التحقق الخالص من وسائط `جهاز.اسمع` —
/// بلا معاملات (المهمة 26).
pub fn validate_listen_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.اسمع";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.بطارية` —
/// بلا معاملات (المهمة 26).
pub fn validate_battery_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.بطارية";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// تسلسل طلب استماع إلى JSON — كائن فارغ
/// (بلا معاملات).
pub fn serialize_listen_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب قراءة بطارية إلى JSON — كائن فارغ
/// (بلا معاملات).
pub fn serialize_battery_request() -> String {
    "{}".to_string()
}

/// نقطة الدخول: `جهاز.انطق(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم القناة، ثم الإرسال
/// والانتظار حتى ينتهي النطق؛ وترجع `صحيح` عند
/// الانتهاء (المهمة 23).
pub fn speak_text(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let text = validate_speak_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = text;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        speak_text_native(&text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn speak_text_native(text: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = text;
        Err(RioError::runtime(
            "جهاز.انطق غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                SPEAK_CONTROL_PREFIX,
                serialize_speak_request(text)
            );
            wait_for_result(line, col)
        } else {
            Err(RioError::runtime("النطق متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.محركات_النطق(...)` — التحقق
/// أولا، ثم بوابة المنصة، ثم القناة، ثم الإرسال
/// والانتظار؛ وترجع قائمة قواميس {اسم، وصف} (وقائمة
/// فارغة إجابة صادقة لا خطأ — المهمة 23).
pub fn list_speech_engines(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_engines_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        list_speech_engines_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_speech_engines_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.محركات_النطق غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                ENGINES_CONTROL_PREFIX,
                serialize_engines_request()
            );
            wait_for_result(line, col)
        } else {
            Err(RioError::runtime("النطق متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.شغّل_صوت(...)` — التحقق أولا
/// (كل المنصات)، ثم بوابة المنصة، ثم القناة، ثم حلّ
/// المسار وفحص الوجود، ثم الإرسال والانتظار حتى
/// يبدأ التشغيل؛ وترجع `صحيح` عند البدء (المهمة 24).
/// التشغيل غير حاجب: يرجع فورا ويكمل الصوت في
/// الخلفية (عكس انطق) — وإلا ما استطاع البرنامج
/// استدعاء `أوقف_صوت` أبدا.
pub fn play_audio(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let path = validate_play_audio_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&path, policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        play_audio_native(&path, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn play_audio_native(
    path: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, policy, cwd);
        Err(RioError::runtime(
            "جهاز.شغل_صوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            // حلّ المسار عبر سياسة الملفات نفسها
            // (كـ نزّل) قبل الإرسال — فالـ JSON يحمل
            // المسار المطلق الجاهز لـ MediaPlayer،
            // والتطبيق لا يعرف مجلد البرنامج.
            let resolved = policy.resolve_in(cwd, path, line, col)?;
            if !resolved.is_file() {
                return Err(RioError::runtime(
                    format!("ملف الصوت غير موجود: {}", policy.display_path(&resolved)),
                    line,
                    col,
                ));
            }
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                PLAY_AUDIO_CONTROL_PREFIX,
                serialize_play_audio_request(&abs)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.أوقف_صوت(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة، ثم الإرسال والانتظار؛
/// وترجع لاشيء. وبلا صوت جارٍ نجاح صامت (لا خطأ) —
/// فالبرنامج لا يعرف متى انتهى الصوت طبيعيا (بلا
/// حدث اكتمال في v1)، فإجبار كل إيقاف على
/// حاول/ماعدا عدائي بلا فائدة (المهمة 24).
pub fn stop_audio(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_stop_audio_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        stop_audio_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn stop_audio_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.أوقف_صوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                STOP_AUDIO_CONTROL_PREFIX,
                serialize_stop_audio_request()
            );
            match wait_for_result(line, col) {
                Ok(_) => Ok(Value::Nil),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.حساس(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم القناة، ثم الإرسال
/// والانتظار حتى تصل القراءة؛ وترجع قاموس {قيم،
/// دقة} (قيم قائمة أعداد: 1 أو 3 حسب الحساس؛ ودقة
/// عدد صحيح 0–3). القراءة حاجبة في المحرك (كالنطق
/// والحوار) — والتطبيق يحدّ جانبه بمهلة 5 ثوان
/// فيجيب دائما (المهمة 25).
pub fn read_sensor(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let name = validate_sensor_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        read_sensor_native(&name, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_sensor_native(name: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = name;
        Err(RioError::runtime(
            "جهاز.حساس غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                SENSOR_CONTROL_PREFIX,
                serialize_sensor_request(name)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الحساس متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.اسمع(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم القناة، ثم الإرسال
/// والانتظار حتى يصل النص المسموع؛ وترجع نصا
/// (المهمة 26). الاستماع حاجب في المحرك (كالحوار
/// والنطق والحساس) — والتطبيق يجيب دائما بنتيجة
/// واحدة (نص أو خطأ عربي: إلغاء إفصاح / رفض إذن /
/// بلا خدمة / بلا كلام واضح).
pub fn listen_speech(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_listen_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        listen_speech_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn listen_speech_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.اسمع غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                LISTEN_CONTROL_PREFIX,
                serialize_listen_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Str(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الاستماع متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.بطارية(...)` — التحقق أولا
/// (كل المنصات)، ثم بوابة المنصة، ثم القناة، ثم
/// الإرسال والانتظار حتى يصل القاموس؛ وترجع قاموس
/// {مستوى، حالة، صحة، حرارة، جهد، مصدر} (المهمة
/// 26). القراءة لحظية عمليا (بث لاصق جاهز) لكنها
/// عبر القناة نفسها كالكل — فلا استثناء بروتوكولي.
pub fn read_battery(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_battery_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        read_battery_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_battery_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.بطارية غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                BATTERY_CONTROL_PREFIX,
                serialize_battery_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("البطارية متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.نبّه(...)` — التحقق أولا (كل
/// المنصات)، ثم بوابة المنصة، ثم القناة، ثم السقف
/// والحجز والإرسال والانتظار؛ وعند خطأ التطبيق
/// يُحرَّر المعرّف المحجوز.
pub fn show_notification(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let (title, text) = validate_notify_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&title, &text);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        show_notification_native(&title, &text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn show_notification_native(title: &str, text: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (title, text);
        Err(RioError::runtime(
            "جهاز.نبه غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            let id = reserve_notify_id(line, col)?;
            eprintln!(
                "{}{}",
                NOTIFY_CONTROL_PREFIX,
                serialize_notify_request(title, text, id)
            );
            match wait_for_result(line, col) {
                Ok(Value::Number(n)) if n.fract() == 0.0 && n == id as f64 => {
                    Ok(Value::Number(n))
                }
                Ok(_) => {
                    // نتيجة قيمة لا تطابق المعرّف المحجوز —
                    // خرق بروتوكول من التطبيق.
                    release_notify_id(id);
                    Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col))
                }
                Err(e) => {
                    release_notify_id(id);
                    Err(e)
                }
            }
        } else {
            Err(RioError::runtime("التنبيه متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.أزل_تنبيه(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة، ثم الحياة، ثم
/// الاستهلاك والإرسال (بلا انتظار) و`لاشيء`.
pub fn remove_notification(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let id = validate_remove_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        remove_notification_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn remove_notification_native(id: u64, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
        Err(RioError::runtime(
            "جهاز.أزل_تنبيه غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            if !consume_notify_id(id) {
                return Err(RioError::runtime(
                    format!("تنبيه غير معروف: {}", id),
                    line,
                    col,
                ));
            }
            eprintln!(
                "{}{}",
                REMOVE_CONTROL_PREFIX,
                serialize_remove_request(id)
            );
            Ok(Value::Nil)
        } else {
            Err(RioError::runtime("التنبيه متاح داخل التطبيق فقط", line, col))
        }
    }
}

// ===== الدفعة 4أ من الكتالوج (البنود 22–28:
// مشاركة وفتح وحافظة ومنتقي ملف) =====
// كل دالة على النمط نفسه: تحقق خالص (كل المنصات)،
// ثم بوابة المنصة، ثم القناة والانتظار؛ والنتيجة
// المتوقعة تُفحص نوعا قبل إرجاعها (فخرق بروتوكول
// التطبيق خطأ عربي لا قيمة مشوهة).

/// التحقق الخالص من وسائط `جهاز.شارك_ملف` — معامل
/// واحد نص (مسار ملف) غير فارغ (البند 22).
pub fn validate_share_file_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.شارك_ملف";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار الملف لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار الملف فارغ", line, col));
    }
    Ok(path)
}

/// التحقق الخالص من وسائط `جهاز.شارك_نص` — معامل
/// واحد نص غير فارغ (البند 22، الخطوة 2).
pub fn validate_share_text_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.شارك_نص";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا للنص المشارك لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if text.is_empty() {
        return Err(RioError::runtime("النص المشارك فارغ", line, col));
    }
    Ok(text)
}

/// التحقق الخالص من وسائط `جهاز.افتح_ملف` — معامل
/// واحد نص (مسار ملف) غير فارغ (البند 23).
pub fn validate_open_file_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.افتح_ملف";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار الملف لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار الملف فارغ", line, col));
    }
    Ok(path)
}

/// التحقق الخالص من وسائط `جهاز.افتح_رابط` — معامل
/// واحد نص رابط http أو https (البند 23).
pub fn validate_open_url_args(args: &[Value], line: usize, col: usize) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.افتح_رابط";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let url = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا للرابط لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if url.is_empty() {
        return Err(RioError::runtime("الرابط فارغ", line, col));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.contains(' ') {
        return Err(RioError::runtime(
            "الرابط يجب أن يبدأ بـ http:// أو https://",
            line,
            col,
        ));
    }
    Ok(url)
}

/// التحقق الخالص من وسائط `جهاز.انسخ_للحافظة` —
/// معامل واحد نص غير فارغ (البند 24).
pub fn validate_clipboard_set_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.انسخ_للحافظة";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا للنص المنسوخ لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if text.is_empty() {
        return Err(RioError::runtime("النص المنسوخ فارغ", line, col));
    }
    Ok(text)
}

/// التحقق الخالص من وسائط `جهاز.الحافظة` —
/// بلا معاملات (البند 25).
pub fn validate_clipboard_get_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.الحافظة";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.اختر_ملف` —
/// بلا معاملات (البند 28).
pub fn validate_pick_file_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.اختر_ملف";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// تسلسل طلب مشاركة ملف إلى JSON — المسار المطلق
/// المحلول.
pub fn serialize_share_file_request(abs_path: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push('}');
    out
}

/// تسلسل طلب مشاركة نص إلى JSON.
pub fn serialize_share_text_request(text: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نص\":");
    out.push_str(&json_escape(text));
    out.push('}');
    out
}

/// تسلسل طلب فتح ملف إلى JSON — المسار المطلق
/// المحلول.
pub fn serialize_open_file_request(abs_path: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push('}');
    out
}

/// تسلسل طلب فتح رابط إلى JSON.
pub fn serialize_open_url_request(url: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"رابط\":");
    out.push_str(&json_escape(url));
    out.push('}');
    out
}

/// تسلسل طلب كتابة الحافظة إلى JSON.
pub fn serialize_clipboard_set_request(text: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نص\":");
    out.push_str(&json_escape(text));
    out.push('}');
    out
}

/// تسلسل طلب قراءة الحافظة إلى JSON — كائن فارغ.
pub fn serialize_clipboard_get_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب منتقي الملف إلى JSON — كائن فارغ.
pub fn serialize_pick_file_request() -> String {
    "{}".to_string()
}

/// نقطة الدخول: `جهاز.شارك_ملف(...)` — التحقق أولا،
/// ثم حلّ المسار وفحص الوجود ورفض ملفات الخصائص
/// (البند 22: لا مشاركة لملفات الخصائص)، ثم بوابة
/// المنصة، ثم القناة والانتظار؛ وترجع `صحيح` عند
/// عرض المختار. النتيجة تُرسَل فور عرض المختار
/// (بلا انتظار اختيار المستخدم — فالمشاركة بلا
/// نتيجة نظامية).
pub fn share_file(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let path = validate_share_file_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&path, policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        share_file_native(&path, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn share_file_native(
    path: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, policy, cwd);
        Err(RioError::runtime(
            "جهاز.شارك_ملف غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            let resolved = policy.resolve_in(cwd, path, line, col)?;
            if !resolved.is_file() {
                return Err(RioError::runtime(
                    format!("ملف المشاركة غير موجود: {}", policy.display_path(&resolved)),
                    line,
                    col,
                ));
            }
            if resolved
                .extension()
                .map(|e| e == "properties")
                .unwrap_or(false)
            {
                return Err(RioError::runtime(
                    "لا مشاركة لملفات الخصائص",
                    line,
                    col,
                ));
            }
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                SHARE_FILE_CONTROL_PREFIX,
                serialize_share_file_request(&abs)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("المشاركة متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.شارك_نص(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار؛ وترجع
/// `صحيح` عند عرض المختار (بلا انتظار اختيار
/// المستخدم — كالمشاركة بالملف).
pub fn share_text(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let text = validate_share_text_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = text;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        share_text_native(&text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn share_text_native(text: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = text;
        Err(RioError::runtime(
            "جهاز.شارك_نص غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                SHARE_TEXT_CONTROL_PREFIX,
                serialize_share_text_request(text)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("المشاركة متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.افتح_ملف(...)` — التحقق أولا،
/// ثم حلّ المسار وفحص الوجود، ثم بوابة المنصة، ثم
/// القناة والانتظار؛ وترجع `صحيح` عند إطلاق العارض
/// الخارجي (بلا انتظار إغلاقه).
pub fn open_file(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let path = validate_open_file_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&path, policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        open_file_native(&path, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_file_native(
    path: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, policy, cwd);
        Err(RioError::runtime(
            "جهاز.افتح_ملف غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            let resolved = policy.resolve_in(cwd, path, line, col)?;
            if !resolved.is_file() {
                return Err(RioError::runtime(
                    format!("ملف العرض غير موجود: {}", policy.display_path(&resolved)),
                    line,
                    col,
                ));
            }
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                OPEN_FILE_CONTROL_PREFIX,
                serialize_open_file_request(&abs)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الفتح متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.افتح_رابط(...)` — التحقق أولا
/// (الرابط http/https فقط)، ثم بوابة المنصة، ثم
/// القناة والانتظار؛ وترجع `صحيح` عند إطلاق المتصفح.
pub fn open_url(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let url = validate_open_url_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = url;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        open_url_native(&url, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_url_native(url: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = url;
        Err(RioError::runtime(
            "جهاز.افتح_رابط غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                OPEN_URL_CONTROL_PREFIX,
                serialize_open_url_request(url)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الفتح متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.انسخ_للحافظة(...)` — التحقق
/// أولا، ثم بوابة المنصة، ثم القناة والانتظار؛
/// وترجع `صحيح` عند النسخ. بلا إذن نظام (الكتابة
/// بلا تنبيه).
pub fn clipboard_set(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let text = validate_clipboard_set_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = text;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        clipboard_set_native(&text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn clipboard_set_native(text: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = text;
        Err(RioError::runtime(
            "جهاز.انسخ_للحافظة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                CLIPBOARD_SET_CONTROL_PREFIX,
                serialize_clipboard_set_request(text)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الحافظة متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.الحافظة(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار؛ وترجع نص
/// الحافظة (وقد يكون فارغا — إجابة صادقة لا خطأ).
/// قيد نظام: أندرويد 12+ يُظهر تنبيها عند قراءة
/// التطبيقات للحافظة — سلوك نظام لا عيب في نيفا.
pub fn clipboard_get(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_clipboard_get_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        clipboard_get_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn clipboard_get_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.الحافظة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                CLIPBOARD_GET_CONTROL_PREFIX,
                serialize_clipboard_get_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Str(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الحافظة متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.اختر_ملف(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار حتى يختار
/// المستخدم ملفا؛ وترجع اسم الملف المنسوخ إلى مجلد
/// البرنامج (نسبيا — كما يعرضه الترمنال). إلغاء
/// المنتقي خطأ عربي «ألغى المستخدم اختيار الملف».
pub fn pick_file(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_pick_file_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        pick_file_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_file_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.اختر_ملف غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                PICK_FILE_CONTROL_PREFIX,
                serialize_pick_file_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Str(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("المنتقي متاح داخل التطبيق فقط", line, col))
        }
    }
}

// ===== الدفعة 4أ — البنود 29–34: موقع وكاميرا
// وواي فاي =====

/// التحقق الخالص من وسائط `جهاز.موقع` —
/// بلا معاملات (البند 29: آخر موقع معروف فقط).
pub fn validate_location_last_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.موقع";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.موقع_حي` —
/// بلا معاملات (البند 30: قراءة واحدة حية بمهلة
/// 10 ثوان في التطبيق).
pub fn validate_location_once_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.موقع_حي";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.كاميرات` —
/// بلا معاملات (البند 31: سرد معلومات الكاميرات).
pub fn validate_cameras_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.كاميرات";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.التقط_صورة` —
/// معامل واحد نص (مسار الصورة) غير فارغ (البند 32).
/// بلا معرّف كاميرا: الالتقاط عبر نية كاميرا النظام
/// (أبسط وأوثق)، وتطبيق الكاميرا يختار الكاميرا من
/// واجهته — فمعامل بلا أثر مرفوض عمدا.
pub fn validate_capture_photo_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.التقط_صورة";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار الصورة لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار الصورة فارغ", line, col));
    }
    Ok(path)
}

/// التحقق الخالص من وسائط `جهاز.واي_فاي` —
/// بلا معاملات (البند 33: معلومات الاتصال الحالي).
pub fn validate_wifi_info_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.واي_فاي";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.مسح_واي_فاي` —
/// بلا معاملات (البند 34: مسح الشبكات القريبة).
pub fn validate_wifi_scan_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.مسح_واي_فاي";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// تسلسل طلب آخر موقع معروف إلى JSON — كائن فارغ.
pub fn serialize_location_last_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب قراءة موقع حية إلى JSON — كائن فارغ.
pub fn serialize_location_once_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب سرد الكاميرات إلى JSON — كائن فارغ.
pub fn serialize_cameras_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب التقاط صورة إلى JSON — المسار المطلق
/// المحلول.
pub fn serialize_capture_photo_request(abs_path: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push('}');
    out
}

/// تسلسل طلب معلومات الواي فاي إلى JSON — كائن فارغ.
pub fn serialize_wifi_info_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب مسح الواي فاي إلى JSON — كائن فارغ.
pub fn serialize_wifi_scan_request() -> String {
    "{}".to_string()
}

/// نقطة الدخول: `جهاز.موقع(...)` — التحقق أولا، ثم
/// بوابة المنصة، ثم القناة والانتظار؛ وترجع قاموس
/// {خط_عرض، خط_طول، دقة} + ارتفاع/سرعة/اتجاه حين
/// يبلّغها المزوّد. بلا موقع معروف خطأ عربي واضح.
pub fn location_last(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_location_last_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        location_last_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn location_last_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.موقع غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                LOCATION_LAST_CONTROL_PREFIX,
                serialize_location_last_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الموقع متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.موقع_حي(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار حتى تصل
/// قراءة حية واحدة (مهلة 10 ثوان في التطبيق)؛
/// وترجع القاموس نفسه كالموقع الأخير. بلا قراءة
/// خلال المهلة خطأ عربي واضح. البث المستمر مرفوض
/// — من أراد تتبعا فليكرر القراءة بنفسه.
pub fn location_once(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_location_once_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        location_once_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn location_once_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.موقع_حي غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                LOCATION_ONCE_CONTROL_PREFIX,
                serialize_location_once_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الموقع متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.كاميرات(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار؛ وترجع قائمة
/// قواميس {معرّف، اتجاه} (أمامية/خلفية/خارجية) —
/// وقائمة فارغة إجابة صادقة لا خطأ. بلا إذن نظام
/// (سرد قدرات فقط).
pub fn list_cameras(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_cameras_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        list_cameras_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_cameras_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.كاميرات غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                CAMERAS_CONTROL_PREFIX,
                serialize_cameras_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::List(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الكاميرا متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.التقط_صورة(...)` — التحقق
/// أولا، ثم حلّ المسار (الملف سيُنشأ — فلا فحص
/// وجود)، ثم بوابة المنصة، ثم القناة والانتظار حتى
/// يلتقط المستخدم الصورة؛ وترجع `صحيح` عند الحفظ.
/// بلا معاينة حية داخل الترمنال — الصورة ملف،
/// والمستخدم يعرضه بتطبيق خارجي.
pub fn capture_photo(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let path = validate_capture_photo_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&path, policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        capture_photo_native(&path, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_photo_native(
    path: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, policy, cwd);
        Err(RioError::runtime(
            "جهاز.التقط_صورة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            // Meeting 50 §64: the photo destination is a
            // write — refused under the downloads root.
            let resolved = policy.resolve_in_for_write(cwd, path, line, col)?;
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                CAPTURE_PHOTO_CONTROL_PREFIX,
                serialize_capture_photo_request(&abs)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Bool(true)),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الكاميرا متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.واي_فاي(...)` — التحقق أولا،
/// ثم بوابة المنصة، ثم القناة والانتظار؛ وترجع قاموس
/// {SSID، BSSID، تردد، قوة، سرعة، عنوان}. بلا اتصال
/// واي فاي خطأ عربي واضح. SSID كما يبلّغه النظام
/// (وقد يكون «غير معروف» بلا إذن موقع — سلوك نظام).
pub fn wifi_info(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_wifi_info_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wifi_info_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wifi_info_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.واي_فاي غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                WIFI_INFO_CONTROL_PREFIX,
                serialize_wifi_info_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الواي فاي متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.مسح_واي_فاي(...)` — التحقق
/// أولا، ثم بوابة المنصة، ثم القناة والانتظار؛ وترجع
/// قائمة قواميس {SSID، BSSID، تردد، قوة، قدرات}.
/// المسح بإذن الموقع — يُشرح للمستخدم أنه إذن مسح
/// لا تتبّع. تشغيل/إيقاف الواي فاي مرفوض — أندرويد
/// 10+ يمنع التطبيقات من تبديله أصلا.
pub fn wifi_scan(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_wifi_scan_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wifi_scan_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wifi_scan_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.مسح_واي_فاي غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                WIFI_SCAN_CONTROL_PREFIX,
                serialize_wifi_scan_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::List(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الواي فاي متاح داخل التطبيق فقط", line, col))
        }
    }
}

// ===== الدفعة 4ب من الكتالوج (البنود 61 و62 و66–72
// و79 و105 و106 و119) — النمط نفسه: تحقق خالص
// (كل المنصات) + تسلسل + بوابة منصة + قناة تحكم
// + انتظار نتيجة واحدة. =====

/// مساعد مشترك: التحقق من دالة بلا معاملات —
/// خطأ العدد قبل المنصة (نمط `validate_battery_args`
/// نفسه، مستخرجا للاستخدام المشترك).
fn validate_zero_args(
    args: &[Value],
    line: usize,
    col: usize,
    display: &str,
) -> RioResult<()> {
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                display,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.انسخ_آخر_خطأ` —
/// بلا معاملات (البند 61).
pub fn validate_copy_last_error_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.انسخ_آخر_خطأ")
}

/// التحقق الخالص من وسائط `جهاز.شارك_الجلسة` —
/// بلا معاملات (البند 62).
pub fn validate_share_session_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.شارك_الجلسة")
}

/// التحقق الخالص من وسائط `جهاز.أوقف_صوت_مؤقتا` —
/// بلا معاملات (البند 66).
pub fn validate_pause_audio_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.أوقف_صوت_مؤقتا")
}

/// التحقق الخالص من وسائط `جهاز.استأنف_صوت` —
/// بلا معاملات (البند 66).
pub fn validate_resume_audio_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.استأنف_صوت")
}

/// التحقق الخالص من وسائط `جهاز.معلومات_صوت` —
/// معامل واحد نص (مسار الصوت) غير فارغ (البند 67).
pub fn validate_audio_info_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<String> {
    const DISPLAY: &str = "جهاز.معلومات_صوت";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار الصوت لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار الصوت فارغ", line, col));
    }
    Ok(path)
}

/// التحقق الخالص من وسائط `جهاز.حساسات` —
/// بلا معاملات (البند 68).
pub fn validate_list_sensors_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.حساسات")
}

/// التحقق الخالص من وسائط `جهاز.مستوى_الصوت` —
/// بلا معاملات (البند 69).
pub fn validate_volume_level_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.مستوى_الصوت")
}

/// التحقق الخالص من وسائط `جهاز.اضبط_مستوى_الصوت` —
/// معامل واحد عدد صحيح ≥ 0 (البند 70)؛ والتطبيق
/// يقصّه إلى [0، الأقصى] ويرجع المستوى الجديد.
pub fn validate_set_volume_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<i64> {
    const DISPLAY: &str = "جهاز.اضبط_مستوى_الصوت";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let level = match &args[0] {
        Value::Number(n) => *n,
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع عددا للمستوى لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if level.fract() != 0.0 || level < 0.0 || level > 1_000_000.0 {
        return Err(RioError::runtime(
            "مستوى الصوت يجب أن يكون عددا صحيحا لا يقل عن 0",
            line,
            col,
        ));
    }
    Ok(level as i64)
}

/// التحقق الخالص من وسائط `جهاز.اهتز` — 0 أو 1
/// معامل: مدة بالمللي ثانية 1–10000، الافتراضي
/// 1000 (البند 71).
pub fn validate_vibrate_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<i64> {
    const DISPLAY: &str = "جهاز.اهتز";
    if args.len() > 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 أو 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    if args.is_empty() {
        return Ok(1000);
    }
    let ms = match &args[0] {
        Value::Number(n) => *n,
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع عددا للمدة لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if ms.fract() != 0.0 || ms < 1.0 || ms > 10_000.0 {
        return Err(RioError::runtime(
            "مدة الاهتزاز يجب أن تكون عددا صحيحا بين 1 و10000 مللي ثانية",
            line,
            col,
        ));
    }
    Ok(ms as i64)
}

/// التحقق الخالص من وسائط `جهاز.مصباح` — معامل
/// واحد منطقي (تشغيل/إيقاف) (البند 72).
pub fn validate_torch_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<bool> {
    const DISPLAY: &str = "جهاز.مصباح";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    match &args[0] {
        Value::Bool(b) => Ok(*b),
        other => Err(typ_error(
            DISPLAY,
            &format!("تتوقع منطقيا للحالة لكن أعطيت {}", other.type_name()),
            line,
            col,
        )),
    }
}

/// التحقق الخالص من وسائط `جهاز.سجّل_صوت` — معامل
/// واحد (ملف) أو اثنان (ملف + مدة بالثواني 1–300،
/// الافتراضي 60) (البند 79).
pub fn validate_record_audio_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<(String, i64)> {
    const DISPLAY: &str = "جهاز.سجّل_صوت";
    if args.is_empty() || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 أو 2 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let path = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا لمسار التسجيل لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if path.is_empty() {
        return Err(RioError::runtime("مسار التسجيل فارغ", line, col));
    }
    let seconds = if args.len() == 2 {
        match &args[1] {
            Value::Number(n) => *n,
            other => {
                return Err(typ_error(
                    DISPLAY,
                    &format!("تتوقع عددا للمدة لكن أعطيت {}", other.type_name()),
                    line,
                    col,
                ))
            }
        }
    } else {
        60.0
    };
    if seconds.fract() != 0.0 || seconds < 1.0 || seconds > 300.0 {
        return Err(RioError::runtime(
            "مدة التسجيل يجب أن تكون عددا صحيحا بين 1 و300 ثانية",
            line,
            col,
        ));
    }
    Ok((path, seconds as i64))
}

/// التحقق الخالص من وسائط `جهاز.سطوع` — معامل واحد:
/// عدد 0–100 أو نص «تلقائي» (البند 105)؛ والكتابة
/// في إعدادات النظام إذن خاص — بلا منح يفتح التطبيق
/// شاشة الإعدادات ويرجع خطأ يعيد المحاولة بعد المنح.
pub fn validate_brightness_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<BrightnessArg> {
    const DISPLAY: &str = "جهاز.سطوع";
    if args.len() != 1 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 معامل لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    match &args[0] {
        Value::Number(n) => {
            if n.fract() != 0.0 || *n < 0.0 || *n > 100.0 {
                return Err(RioError::runtime(
                    "مستوى السطوع يجب أن يكون عددا صحيحا بين 0 و100",
                    line,
                    col,
                ));
            }
            Ok(BrightnessArg::Level(*n as i64))
        }
        Value::Str(s) if s == "تلقائي" => Ok(BrightnessArg::Auto),
        Value::Str(_) => Err(RioError::runtime(
            "سطوع النص يجب أن يكون «تلقائي» فقط",
            line,
            col,
        )),
        other => Err(typ_error(
            DISPLAY,
            &format!(
                "تتوقع عددا أو نص «تلقائي» لكن أعطيت {}",
                other.type_name()
            ),
            line,
            col,
        )),
    }
}

/// معامل `جهاز.سطوع` بعد التحقق: مستوى أو تلقائي.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrightnessArg {
    Level(i64),
    Auto,
}

/// التحقق الخالص من وسائط `جهاز.توست` — معامل واحد
/// (نص) أو اثنان (نص + «قصير»/«طويل»، الافتراضي
/// «قصير») (البند 106)؛ وقاعدة التوست 196-1:
/// إلغاء السابق قبل عرض الجديد — في التطبيق.
pub fn validate_toast_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<(String, bool)> {
    const DISPLAY: &str = "جهاز.توست";
    if args.is_empty() || args.len() > 2 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 1 أو 2 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let text = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(typ_error(
                DISPLAY,
                &format!("تتوقع نصا للرسالة لكن أعطيت {}", other.type_name()),
                line,
                col,
            ))
        }
    };
    if text.is_empty() {
        return Err(RioError::runtime("نص التوست فارغ", line, col));
    }
    let long = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) if s == "قصير" => false,
            Value::Str(s) if s == "طويل" => true,
            Value::Str(_) => {
                return Err(RioError::runtime(
                    "مدة التوست يجب أن تكون «قصير» أو «طويل»",
                    line,
                    col,
                ))
            }
            other => {
                return Err(typ_error(
                    DISPLAY,
                    &format!("تتوقع نصا للمدة لكن أعطيت {}", other.type_name()),
                    line,
                    col,
                ))
            }
        }
    } else {
        false
    };
    Ok((text, long))
}

/// التحقق الخالص من وسائط `جهاز.معلومات_الصوت` —
/// بلا معاملات (البند 119).
pub fn validate_audio_device_info_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<()> {
    validate_zero_args(args, line, col, "جهاز.معلومات_الصوت")
}

/// تسلسل طلبات الدفعة 4ب بلا حمولة إلى JSON.
pub fn serialize_empty_request() -> String {
    "{}".to_string()
}

/// تسلسل طلب معلومات صوت إلى JSON — المسار المطلق
/// المحلول.
pub fn serialize_audio_info_request(abs_path: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push('}');
    out
}

/// تسلسل طلب ضبط مستوى الصوت إلى JSON.
pub fn serialize_set_volume_request(level: i64) -> String {
    format!("{{\"مستوى\":{}}}", level)
}

/// تسلسل طلب اهتزاز إلى JSON.
pub fn serialize_vibrate_request(ms: i64) -> String {
    format!("{{\"مدة\":{}}}", ms)
}

/// تسلسل طلب مصباح إلى JSON.
pub fn serialize_torch_request(on: bool) -> String {
    format!("{{\"تشغيل\":{}}}", on)
}

/// تسلسل طلب تسجيل صوت إلى JSON — المسار المطلق
/// المحلول + المدة بالثواني.
pub fn serialize_record_audio_request(abs_path: &str, seconds: i64) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"مسار\":");
    out.push_str(&json_escape(abs_path));
    out.push_str(&format!(",\"مدة\":{}}}", seconds));
    out
}

/// تسلسل طلب سطوع إلى JSON.
pub fn serialize_brightness_request(arg: &BrightnessArg) -> String {
    match arg {
        BrightnessArg::Level(n) => format!("{{\"مستوى\":{}}}", n),
        BrightnessArg::Auto => "{\"تلقائي\":true}".to_string(),
    }
}

/// تسلسل طلب توست إلى JSON.
pub fn serialize_toast_request(text: &str, long: bool) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نص\":");
    out.push_str(&json_escape(text));
    out.push_str(&format!(",\"طويل\":{}}}", long));
    out
}

/// نقطة الدخول: `جهاز.انسخ_آخر_خطأ(...)` — بلا
/// معاملات؛ وترجع نص آخر سطر خطأ في الجلسة (البند
/// 61). والتحقق أولا (كل المنصات)، ثم بوابة المنصة،
/// ثم القناة، ثم الإرسال والانتظار.
pub fn copy_last_error(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_copy_last_error_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        copy_last_error_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn copy_last_error_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.انسخ_آخر_خطأ غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                COPY_LAST_ERROR_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Str(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("نسخ الخطأ متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.شارك_الجلسة(...)` — بلا
/// معاملات؛ وترجع صحيحا بعد إطلاق نافذة المشاركة
/// بنص الجلسة (آخر 100 ألف حرف) (البند 62).
pub fn share_session(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_share_session_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        share_session_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn share_session_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.شارك_الجلسة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                SHARE_SESSION_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(true)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("مشاركة الجلسة متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.أوقف_صوت_مؤقتا(...)` — بلا
/// معاملات؛ وإيقاف مؤقت يبقي المشغّل حيا لا يطلقه
/// (البند 66)؛ وبلا صوت جارٍ نجاح صامت كـ`أوقف_صوت`.
pub fn pause_audio(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_pause_audio_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        pause_audio_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pause_audio_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.أوقف_صوت_مؤقتا غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                PAUSE_AUDIO_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(true)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("إيقاف الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.استأنف_صوت(...)` — بلا
/// معاملات؛ ويعيد تشغيل الصوت الموقوف مؤقتا (البند
/// 66)؛ وبلا صوت موقوف نجاح صامت.
pub fn resume_audio(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_resume_audio_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        resume_audio_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resume_audio_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.استأنف_صوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                RESUME_AUDIO_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(true)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("استئناف الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.معلومات_صوت(...)` — معلومات
/// ملف صوت قبل تشغيله: قاموس {مدة} بالثواني (البند
/// 67)؛ والمسار عبر سياسة الملفات كـ`شغّل_صوت`.
pub fn audio_info(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let path = validate_audio_info_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&path, policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        audio_info_native(&path, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn audio_info_native(
    path: &str,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, policy, cwd);
        Err(RioError::runtime(
            "جهاز.معلومات_صوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            let resolved = policy.resolve_in(cwd, path, line, col)?;
            if !resolved.is_file() {
                return Err(RioError::runtime(
                    "ملف الصوت غير موجود",
                    line,
                    col,
                ));
            }
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                AUDIO_INFO_CONTROL_PREFIX,
                serialize_audio_info_request(&abs)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("معلومات الصوت متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.حساسات(...)` — بلا معاملات؛
/// وترجع قائمة أسماء الحساسات الثمانية الموجودة
/// على الجهاز (البند 68)؛ والقراءة المستمرة مرفوضة
/// (البند 143) — من أراد تكرارا كرر الأمر بنفسه.
pub fn list_sensors(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_list_sensors_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        list_sensors_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_sensors_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.حساسات غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                LIST_SENSORS_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::List(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("سرد الحساسات متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.مستوى_الصوت(...)` — بلا
/// معاملات؛ وترجع قاموس {مستوى، أقصى} لتيار
/// الموسيقى (البند 69).
pub fn volume_level(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_volume_level_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        volume_level_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn volume_level_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.مستوى_الصوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                VOLUME_LEVEL_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("مستوى الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.اضبط_مستوى_الصوت(...)` —
/// معامل واحد عدد صحيح ≥ 0؛ والتطبيق يقصّه إلى
/// [0، الأقصى] ويرجع المستوى الجديد (البند 70).
pub fn set_volume_level(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let level = validate_set_volume_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = level;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        set_volume_level_native(level, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn set_volume_level_native(level: i64, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = level;
        Err(RioError::runtime(
            "جهاز.اضبط_مستوى_الصوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                SET_VOLUME_CONTROL_PREFIX,
                serialize_set_volume_request(level)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Number(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("ضبط مستوى الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.اهتز(...)` — 0 أو 1 معامل:
/// مدة بالمللي ثانية 1–10000، الافتراضي 1000 (البند
/// 71)؛ وبلا إذن عمليا.
pub fn vibrate(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let ms = validate_vibrate_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ms;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        vibrate_native(ms, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn vibrate_native(ms: i64, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = ms;
        Err(RioError::runtime(
            "جهاز.اهتز غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                VIBRATE_CONTROL_PREFIX,
                serialize_vibrate_request(ms)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(true)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("الاهتزاز متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.مصباح(...)` — معامل واحد
/// منطقي (تشغيل/إيقاف)؛ وترجع الحالة الجديدة (البند
/// 72)؛ ويحتاج إذن الكاميرا في الحقيقة (مدير
/// الكاميرا هو من يشغّله) — يُطلب عند أول استعمال.
pub fn torch(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let on = validate_torch_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = on;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        torch_native(on, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn torch_native(on: bool, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = on;
        Err(RioError::runtime(
            "جهاز.مصباح غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                TORCH_CONTROL_PREFIX,
                serialize_torch_request(on)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("المصباح متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.سجّل_صوت(...)` — تسجيل
/// ميكروفون بسيط إلى ملف داخل مجلد البرنامج، بمدة
/// قصوى افتراضية 60 ثانية (1–300) وبلا خيارات
/// مشفّر/صيغة (الافتراضيات تكفي) (البند 79)؛
/// والإذن يُطلب عند أول استعمال، مع تنبيه أن الملف
/// يُحفظ محليا فقط.
pub fn record_audio(
    args: &[Value],
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    let (path, seconds) = validate_record_audio_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ((&path, seconds), policy, cwd);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        record_audio_native(&path, seconds, policy, cwd, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn record_audio_native(
    path: &str,
    seconds: i64,
    policy: &crate::permissions::FilePolicy,
    cwd: &std::path::Path,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, seconds, policy, cwd);
        Err(RioError::runtime(
            "جهاز.سجّل_صوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            // Meeting 50 §64: the recording destination is
            // a write — refused under the downloads root.
            let resolved = policy.resolve_in_for_write(cwd, path, line, col)?;
            let abs = resolved.to_string_lossy().to_string();
            eprintln!(
                "{}{}",
                RECORD_AUDIO_CONTROL_PREFIX,
                serialize_record_audio_request(&abs, seconds)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Bool(true)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("تسجيل الصوت متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.سطوع(...)` — معامل واحد عدد
/// 0–100 أو «تلقائي»؛ وترجع قاموس {مستوى، تلقائي}
/// (البند 105)؛ والكتابة في إعدادات النظام إذن خاص
/// — بلا منح يفتح التطبيق شاشة الإعدادات ويرجع
/// خطأ يعيد المحاولة بعد المنح.
pub fn brightness(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let arg = validate_brightness_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = arg;
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        brightness_native(&arg, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn brightness_native(arg: &BrightnessArg, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = arg;
        Err(RioError::runtime(
            "جهاز.سطوع غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                BRIGHTNESS_CONTROL_PREFIX,
                serialize_brightness_request(arg)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("السطوع متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.توست(...)` — رسالة عائمة
/// قصيرة تظهر فوق الشاشة وتختفي وحدها (البند 106)؛
/// وترجع لا شيء (Nil)؛ والتطبيق يلغي السابق قبل
/// عرض الجديد (قاعدة التوست 196-1). خطوة تالية
/// بعد التنبيه الموجود — لا تُعرض قبله.
pub fn toast(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let (text, long) = validate_toast_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&text, long);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        toast_native(&text, long, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn toast_native(text: &str, long: bool, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (text, long);
        Err(RioError::runtime(
            "جهاز.توست غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                TOAST_CONTROL_PREFIX,
                serialize_toast_request(text, long)
            );
            match wait_for_result(line, col) {
                Ok(Value::Bool(true)) => Ok(Value::Nil),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("التوست متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.معلومات_الصوت(...)` — بلا
/// معاملات؛ وترجع قاموس {معدل_العينات، حجم_المخزن،
/// بلوتوث، سماعة_سلكية} (البند 119).
pub fn audio_device_info(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_audio_device_info_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        audio_device_info_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn audio_device_info_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.معلومات_الصوت غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                AUDIO_DEVICE_INFO_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Dict(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("معلومات الصوت متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// أندرويد فقط: إرسال الطلب على stderr ثم انتظار سطر
/// النتيجة على stdin (آلية `ادخل` نفسها).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
fn request_and_wait(req: &DialogRequest, line: usize, col: usize) -> RioResult<Value> {
    eprintln!("{}{}", CONTROL_PREFIX, serialize_request(req));
    wait_for_result(line, col)
}

/// أندرويد فقط: انتظار سطر نتيجة تحكم واحد على stdin
/// (مشترك بين الحوار والتنبيه — المهمة 21 استخرجته
/// من `request_and_wait` بلا تغيير سلوك).
/// انتظار نتيجة تحكم واحدة على stdin (مشترك بين
/// `جهاز` وملفات SAF في files.rs — المهمة 28).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
pub(crate) fn wait_for_result(line: usize, col: usize) -> RioResult<Value> {
    // مهمة 22: القراءة عبر مضخة stdin (لا مباشرة) —
    // خيط المضخة وحده يملك stdin. سطور الإلغاء هنا
    // سطور غير نتيجة فتُتجاهل كغيرها (الإلغاء أثناء
    // حوار لا أثر له — والمخرج زر «تحكم»).
    loop {
        let buf = match crate::stdin_pump::next_raw_line() {
            Some(b) => b,
            None => {
                return Err(RioError::runtime(
                    "انتهى الإدخال أثناء انتظار نتيجة التحكم",
                    line,
                    col,
                ));
            }
        };
        let text = buf.trim_end_matches(['\n', '\r']);
        if let Some(payload) = text.strip_prefix(RESULT_PREFIX) {
            return parse_control_result(payload, line, col);
        }
        // سطر آخر أثناء الانتظار — يُتجاهل (لا يصل
        // عمليا: لا كتابة في الترمنال أثناء الانتظار —
        // والتجاهل دفاع فقط).
    }
}

/// تسلسل طلب الحوار إلى JSON (تهريب يدوي — المحرك
/// بلا serde ولا تبعية جديدة).
pub fn serialize_request(req: &DialogRequest) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"نوع\":");
    out.push_str(&json_escape(&req.kind));
    out.push_str(",\"عنوان\":");
    out.push_str(&json_escape(&req.title));
    if let Some(options) = &req.options {
        out.push_str(",\"خيارات\":[");
        for (i, opt) in options.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_escape(opt));
        }
        out.push(']');
    }
    if let Some(min) = req.min {
        out.push_str(&format!(",\"أدنى\":{}", min));
    }
    if let Some(max) = req.max {
        out.push_str(&format!(",\"أقصى\":{}", max));
    }
    if let Some(default) = req.default {
        out.push_str(&format!(",\"افتراضي\":{}", default));
    }
    out.push('}');
    out
}

/// تهريب نص إلى سلسلة JSON (مع علامتي التنصيص).
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// قيمة JSON (محلّل صغير مكتوب يدويا — std فقط).
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn new(text: &str) -> Self {
        JsonParser {
            chars: text.chars().collect(),
            pos: 0,
        }
    }

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
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, what: char) -> Result<(), String> {
        match self.next() {
            Some(c) if c == what => Ok(()),
            _ => Err(format!("توقع «{}»", what)),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), String> {
        for expected in word.chars() {
            match self.next() {
                Some(c) if c == expected => {}
                _ => return Err(format!("توقع «{}»", word)),
            }
        }
        Ok(())
    }

    fn parse_value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(Json::Str(self.parse_string()?)),
            Some('t') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some('f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some('n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(format!("حرف غير متوقع «{}»", c)),
            None => Err("انتهى النص فجأة".to_string()),
        }
    }

    fn parse_object(&mut self) -> Result<Json, String> {
        self.expect('{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err("توقع مفتاح نصا".to_string());
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some('}') => break,
                _ => return Err("توقع «،» أو «}»".to_string()),
            }
        }
        Ok(Json::Obj(pairs))
    }

    fn parse_array(&mut self) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_ws();
            match self.next() {
                Some(',') => continue,
                Some(']') => break,
                _ => return Err("توقع «،» أو «]»".to_string()),
            }
        }
        Ok(Json::Arr(items))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err("سلسلة غير مغلقة".to_string()),
                Some('"') => break,
                Some('\\') => match self.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{0008}'),
                    Some('f') => out.push('\u{000C}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let first = self.parse_hex4()?;
                        if (0xD800..0xDC00).contains(&first) {
                            // زوج بديل: يليه \uXXXX للبديل المنخفض.
                            if self.next() != Some('\\') || self.next() != Some('u') {
                                return Err("بديل عالٍ بلا بديل منخفض".to_string());
                            }
                            let second = self.parse_hex4()?;
                            if !(0xDC00..0xE000).contains(&second) {
                                return Err("بديل منخفض غير صالح".to_string());
                            }
                            let code =
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                            match char::from_u32(code) {
                                Some(c) => out.push(c),
                                None => return Err("نقطة شفرة غير صالحة".to_string()),
                            }
                        } else if (0xDC00..0xE000).contains(&first) {
                            return Err("بديل منخفض بلا بديل عالٍ".to_string());
                        } else {
                            match char::from_u32(first) {
                                Some(c) => out.push(c),
                                None => return Err("نقطة شفرة غير صالحة".to_string()),
                            }
                        }
                    }
                    _ => return Err("تهريب غير صالح".to_string()),
                },
                Some(c) if (c as u32) < 0x20 => {
                    return Err("حرف تحكم عارٍ في سلسلة".to_string())
                }
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let digit = match self.next() {
                Some(c) => c.to_digit(16),
                None => None,
            };
            match digit {
                Some(d) => value = value * 16 + d,
                None => return Err("رقم سداسي عشري غير صالح".to_string()),
            }
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err("عدد غير صالح".to_string()),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {}
                _ => return Err("عدد غير صالح".to_string()),
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {}
                _ => return Err("عدد غير صالح".to_string()),
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        match text.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Json::Num(n)),
            _ => Err("عدد غير صالح".to_string()),
        }
    }
}

/// تحليل نص JSON كاملا (بلا بقايا بعد القيمة).
pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut parser = JsonParser::new(text);
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err("بقايا بعد قيمة JSON".to_string());
    }
    Ok(value)
}

/// تحويل قيمة JSON إلى قيمة نيفا (null مرفوض: التطبيق
/// لا يرسله أبدا).
fn json_to_value(json: Json) -> Result<Value, String> {
    match json {
        Json::Null => Err("قيمة null غير متوقعة".to_string()),
        Json::Bool(b) => Ok(Value::Bool(b)),
        Json::Num(n) => Ok(Value::Number(n)),
        Json::Str(s) => Ok(Value::Str(s)),
        Json::Arr(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(item)?);
            }
            Ok(Value::List(out))
        }
        Json::Obj(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                out.push((key, json_to_value(value)?));
            }
            Ok(Value::Dict(out))
        }
    }
}

/// تحليل سطر نتيجة تحكم (الحمولة بعد البادئة) إلى
/// قيمة نيفا أو رفع خطأ عربي (إلغاء أو خطأ تطبيق أو
/// نتيجة مشوهة). مشترك بين الحوار والتنبيه (المهمة
/// 21 عمّمت `parse_dialog_result` — المنطق نفسه
/// حرفيا؛ وشكل «ألغي» لا يصل من التنبيه أبدا).
pub fn parse_control_result(payload: &str, line: usize, col: usize) -> RioResult<Value> {
    let invalid = || RioError::runtime("نتيجة تحكم غير صالحة", line, col);
    let json = parse_json(payload).map_err(|_| invalid())?;
    let pairs = match json {
        Json::Obj(pairs) => pairs,
        _ => return Err(invalid()),
    };
    for (key, value) in &pairs {
        match key.as_str() {
            "قيمة" => return json_to_value(value.clone()).map_err(|_| invalid()),
            "ألغي" => {
                if *value == Json::Bool(true) {
                    return Err(RioError::runtime("ألغى المستخدم الحوار", line, col));
                }
                return Err(invalid());
            }
            "خطأ" => match value {
                Json::Str(text) => {
                    return Err(RioError::runtime(text.clone(), line, col));
                }
                _ => return Err(invalid()),
            },
            _ => return Err(invalid()),
        }
    }
    Err(invalid())
}

// ===== الدفعة 5 من الكتالوج (البنود 98 و99
// خطوة 1): تنزيل نظامي وقراءة مستند وعرض مجلد
// عبر منتقيات لمرة واحدة — قرار الاجتماع 50
// §64 =====

/// تنظيف اسم ملف التنزيل (البند 98): يُؤخذ الجزء
/// بعد آخر فاصل مسار (`/` أو `\`)، فيسقط أي مسار
/// مرفق؛ واسم فارغ أو `.` أو `..` بعد التنظيف
/// مرفوض بخطأ عربي.
fn clean_download_filename(name: &str) -> Result<String, String> {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if base.is_empty() || base == "." || base == ".." {
        return Err("اسم ملف غير صالح".to_string());
    }
    Ok(base.to_string())
}

/// التحقق الخالص من وسائط `جهاز.نزّل` — أربعة
/// نصوص غير فارغة (رابط، عنوان، وصف، اسم_ملف)
/// (البند 98): الرابط `https` فقط، واسم الملف
/// يُنظَّف (بلا `/` وبلا `..`)؛ ويرجع الأربعة
/// (والاسم بعد التنظيف).
pub fn validate_download_args(
    args: &[Value],
    line: usize,
    col: usize,
) -> RioResult<(String, String, String, String)> {
    const DISPLAY: &str = "جهاز.نزّل";
    if args.len() != 4 {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 4 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    let mut texts = Vec::with_capacity(4);
    for (i, arg) in args.iter().enumerate() {
        match arg {
            Value::Str(s) if !s.trim().is_empty() => texts.push(s.clone()),
            Value::Str(_) => {
                return Err(RioError::runtime(
                    format!("الدالة {} المعامل {} نص فارغ", DISPLAY, i + 1),
                    line,
                    col,
                ));
            }
            _ => {
                return Err(typ_error(
                    DISPLAY,
                    "تتوقع نصا في كل معامل",
                    line,
                    col,
                ));
            }
        }
    }
    let url = texts[0].clone();
    if !url.starts_with("https://") {
        return Err(RioError::runtime(
            "رابط التنزيل يجب أن يبدأ بـ https://",
            line,
            col,
        ));
    }
    let filename = clean_download_filename(&texts[3]).map_err(|m| RioError::runtime(m, line, col))?;
    Ok((url, texts[1].clone(), texts[2].clone(), filename))
}

/// تسلسل طلب تنزيل نظامي إلى JSON.
pub fn serialize_download_request(url: &str, title: &str, desc: &str, filename: &str) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"رابط\":");
    out.push_str(&json_escape(url));
    out.push_str(",\"عنوان\":");
    out.push_str(&json_escape(title));
    out.push_str(",\"وصف\":");
    out.push_str(&json_escape(desc));
    out.push_str(",\"اسم_ملف\":");
    out.push_str(&json_escape(filename));
    out.push('}');
    out
}

/// نقطة الدخول: `جهاز.نزّل(...)` — التحقق أولا
/// (رابط https + اسم ملف منظف)، ثم بوابة المنصة،
/// ثم القناة والانتظار حتى يُدرج مدير التنزيلات
/// الطلب؛ ويرجع معرّف التنزيل النظامي عددا
/// (البند 98). بلا إذن تخزين: مدير التنزيلات
/// يكتب بنفسه.
pub fn download(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    let (url, title, desc, filename) = validate_download_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&url, &title, &desc, &filename);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        download_native(&url, &title, &desc, &filename, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download_native(
    url: &str,
    title: &str,
    desc: &str,
    filename: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (url, title, desc, filename);
        Err(RioError::runtime(
            "جهاز.نزّل غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                DOWNLOAD_CONTROL_PREFIX,
                serialize_download_request(url, title, desc, filename)
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Number(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("التنزيل متاح داخل التطبيق فقط", line, col))
        }
    }
}

/// التحقق الخالص من وسائط `جهاز.اقرأ_مستند` —
/// بلا معاملات (البند 99 خطوة 1).
pub fn validate_read_document_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.اقرأ_مستند";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// التحقق الخالص من وسائط `جهاز.اعرض_مجلد` —
/// بلا معاملات (البند 99 خطوة 1).
pub fn validate_list_folder_args(args: &[Value], line: usize, col: usize) -> RioResult<()> {
    const DISPLAY: &str = "جهاز.اعرض_مجلد";
    if !args.is_empty() {
        return Err(RioError::runtime(
            format!(
                "الدالة {} تتوقع 0 معاملات لكن أعطيت {}",
                DISPLAY,
                args.len()
            ),
            line,
            col,
        ));
    }
    Ok(())
}

/// نقطة الدخول: `جهاز.اقرأ_مستند(...)` — التحقق
/// أولا، ثم بوابة المنصة، ثم القناة والانتظار حتى
/// يختار المستخدم مستندا؛ ويرجع نص المستند
/// (UTF-8، حد 5 ميغابايت في التطبيق). منتقٍ
/// لمرة واحدة بلا إذن دائم. إلغاء المنتقي خطأ
/// عربي «ألغى المستخدم الاختيار» (البند 99
/// خطوة 1).
pub fn read_document(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_read_document_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        read_document_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_document_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.اقرأ_مستند غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                READ_DOCUMENT_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::Str(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("قراءة المستند متاحة داخل التطبيق فقط", line, col))
        }
    }
}

/// نقطة الدخول: `جهاز.اعرض_مجلد(...)` — التحقق
/// أولا، ثم بوابة المنصة، ثم القناة والانتظار حتى
/// يختار المستخدم مجلدا؛ ويرجع قائمة أسماء الأبناء
/// (`/` في نهاية المجلدات — كعلامات `ات`). منتقٍ
/// لمرة واحدة بلا إذن دائم. إلغاء المنتقي خطأ
/// عربي «ألغى المستخدم الاختيار» (البند 99
/// خطوة 1).
pub fn list_folder(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    validate_list_folder_args(args, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        list_folder_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn list_folder_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "جهاز.اعرض_مجلد غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
    #[cfg(target_os = "android")]
    {
        if std::env::var("NIVA_CONTROL_CHANNEL")
            .map(|v| v == "stderr")
            .unwrap_or(false)
        {
            eprintln!(
                "{}{}",
                LIST_FOLDER_CONTROL_PREFIX,
                serialize_empty_request()
            );
            match wait_for_result(line, col) {
                Ok(value @ Value::List(_)) => Ok(value),
                Ok(_) => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
                Err(e) => Err(e),
            }
        } else {
            Err(RioError::runtime("عرض المجلد متاح داخل التطبيق فقط", line, col))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_val(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    fn num_val(n: f64) -> Value {
        Value::Number(n)
    }

    fn list_val(items: Vec<Value>) -> Value {
        Value::List(items)
    }

    fn dict_val(pairs: Vec<(&str, Value)>) -> Value {
        Value::Dict(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    fn req(kind: &str, title: &str) -> DialogRequest {
        DialogRequest {
            kind: kind.to_string(),
            title: title.to_string(),
            options: None,
            min: None,
            max: None,
            default: None,
        }
    }

    #[test]
    fn validate_two_arg_kinds_ok() {
        for kind in ["تأكيد", "نص", "رقم", "تاريخ", "وقت"] {
            let got = validate_dialog_args(&[str_val(kind), str_val("س؟")], 1, 1).unwrap();
            assert_eq!(got, req(kind, "س؟"));
        }
    }

    #[test]
    fn validate_wrong_arity() {
        let err = validate_dialog_args(&[str_val("تأكيد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 أو 3 معاملات"), "got: {}", err);
        let err = validate_dialog_args(
            &[str_val("تأكيد"), str_val("س؟"), str_val("زائد"), str_val("رابع")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("2 أو 3 معاملات"), "got: {}", err);
    }

    #[test]
    fn validate_non_string_args() {
        let err = validate_dialog_args(&[num_val(5.0), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لنوع الحوار"), "got: {}", err);
        let err = validate_dialog_args(&[str_val("نص"), Value::Bool(true)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لعنوان الحوار"), "got: {}", err);
    }

    #[test]
    fn validate_empty_title() {
        let err = validate_dialog_args(&[str_val("نص"), str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عنوان الحوار فارغ"), "got: {}", err);
    }

    #[test]
    fn validate_unknown_and_voice() {
        let err = validate_dialog_args(&[str_val("غريب"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نوع حوار غير معروف: غريب"), "got: {}", err);
        let err = validate_dialog_args(&[str_val("صوتي"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("الحوار الصوتي غير متاح بعد"),
            "got: {}",
            err
        );
    }

    #[test]
    fn validate_choice_options() {
        let ok = validate_dialog_args(
            &[
                str_val("اختيار واحد"),
                str_val("اختر"),
                list_val(vec![str_val("أ"), str_val("ب")]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(ok.options, Some(vec!["أ".to_string(), "ب".to_string()]));
        // بلا معامل ثالث.
        let err = validate_dialog_args(&[str_val("اختيار واحد"), str_val("اختر")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("3 معاملات"), "got: {}", err);
        // قائمة فارغة.
        let err = validate_dialog_args(
            &[str_val("قائمة"), str_val("اختر"), list_val(vec![])],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("قائمة الخيارات فارغة"), "got: {}", err);
        // عنصر غير نص.
        let err = validate_dialog_args(
            &[
                str_val("اختيارات"),
                str_val("اختر"),
                list_val(vec![str_val("أ"), num_val(1.0)]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("نصوصا في قائمة الخيارات"), "got: {}", err);
        // معامل ثالث زائد لنوع بمعاملين.
        let err = validate_dialog_args(
            &[str_val("نص"), str_val("س؟"), str_val("زائد")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("2 معاملات لنوع نص"), "got: {}", err);
    }

    #[test]
    fn validate_counter() {
        let ok = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(10.0)),
                    ("افتراضي", num_val(3.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!((ok.min, ok.max, ok.default), (Some(0), Some(10), Some(3)));
        // بلا افتراضي.
        let ok = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(1.0)), ("أقصى", num_val(5.0))]),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(ok.default, None);
        // يفتقد أدنى.
        let err = validate_dialog_args(
            &[str_val("عداد"), str_val("كم؟"), dict_val(vec![("أقصى", num_val(5.0))])],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("يفتقد «أدنى»"), "got: {}", err);
        // مفتاح غير معروف.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(5.0)),
                    ("خطوة", num_val(1.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("مفتاح غير معروف في قاموس العداد: خطوة"), "got: {}", err);
        // أدنى أكبر من أقصى.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(9.0)), ("أقصى", num_val(2.0))]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("الحد الأدنى أكبر من الحد الأقصى"), "got: {}", err);
        // افتراضي خارج النطاق.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![
                    ("أدنى", num_val(0.0)),
                    ("أقصى", num_val(5.0)),
                    ("افتراضي", num_val(9.0)),
                ]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("القيمة الافتراضية خارج النطاق"), "got: {}", err);
        // حد كسري.
        let err = validate_dialog_args(
            &[
                str_val("عداد"),
                str_val("كم؟"),
                dict_val(vec![("أدنى", num_val(0.5)), ("أقصى", num_val(5.0))]),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
    }

    #[test]
    fn json_escape_round_trip() {
        let original = "قال \"مرحبا\"\nسطر جديد \\ شرطة";
        let escaped = json_escape(original);
        let parsed = parse_json(&escaped).unwrap();
        assert_eq!(parsed, Json::Str(original.to_string()));
        // عربي خالص يبقى حرفيا (بلا \u).
        assert_eq!(json_escape("محمود"), "\"محمود\"");
    }

    #[test]
    fn json_parse_values() {
        assert_eq!(parse_json("true").unwrap(), Json::Bool(true));
        assert_eq!(parse_json("false").unwrap(), Json::Bool(false));
        assert_eq!(parse_json("null").unwrap(), Json::Null);
        assert_eq!(parse_json("42").unwrap(), Json::Num(42.0));
        assert_eq!(parse_json("-3.5e2").unwrap(), Json::Num(-350.0));
        assert_eq!(
            parse_json("[\"أ\", ١]").unwrap_err(),
            "حرف غير متوقع «١»".to_string()
        );
        assert_eq!(
            parse_json("[\"أ\", 1]").unwrap(),
            Json::Arr(vec![Json::Str("أ".to_string()), Json::Num(1.0)])
        );
        assert_eq!(
            parse_json("{\"سنة\": 2026, \"شهر\": 11}").unwrap(),
            Json::Obj(vec![
                ("سنة".to_string(), Json::Num(2026.0)),
                ("شهر".to_string(), Json::Num(11.0)),
            ])
        );
        // تهريب \u + زوج بديل (وجه مبتسم).
        assert_eq!(
            parse_json("\"\\u0645\\u062d\\u0645\\u0648\\u062f\"").unwrap(),
            Json::Str("محمود".to_string())
        );
        assert_eq!(
            parse_json("\"\\uD83D\\uDE00\"").unwrap(),
            Json::Str("😀".to_string())
        );
        // مرفوضات.
        assert!(parse_json("{\"أ\": 1,}").is_err());
        assert!(parse_json("[1, 2").is_err());
        assert!(parse_json("\"غير مغلقة").is_err());
        assert!(parse_json("01").is_err());
        assert!(parse_json("true false").is_err());
        assert!(parse_json("nul").is_err());
    }

    #[test]
    fn serialize_request_round_trip() {
        let mut r = req("اختيار واحد", "اختر لونا");
        r.options = Some(vec!["أحمر".to_string(), "أزرق \"غامق\"".to_string()]);
        let text = serialize_request(&r);
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![
                ("نوع".to_string(), Json::Str("اختيار واحد".to_string())),
                ("عنوان".to_string(), Json::Str("اختر لونا".to_string())),
                (
                    "خيارات".to_string(),
                    Json::Arr(vec![
                        Json::Str("أحمر".to_string()),
                        Json::Str("أزرق \"غامق\"".to_string()),
                    ])
                ),
            ])
        );
        let mut c = req("عداد", "كم؟");
        c.min = Some(-5);
        c.max = Some(10);
        c.default = Some(3);
        let text = serialize_request(&c);
        assert!(text.contains("\"أدنى\":-5"), "got: {}", text);
        assert!(text.contains("\"أقصى\":10"), "got: {}", text);
        assert!(text.contains("\"افتراضي\":3"), "got: {}", text);
    }

    #[test]
    fn parse_result_value_kinds() {
        // منطقي.
        assert_eq!(
            parse_control_result("{\"قيمة\": true}", 1, 1).unwrap(),
            Value::Bool(true)
        );
        // عدد.
        assert_eq!(
            parse_control_result("{\"قيمة\": 42}", 1, 1).unwrap(),
            Value::Number(42.0)
        );
        // نص عربي.
        assert_eq!(
            parse_control_result("{\"قيمة\": \"محمود\"}", 1, 1).unwrap(),
            Value::Str("محمود".to_string())
        );
        // قائمة نصوص.
        assert_eq!(
            parse_control_result("{\"قيمة\": [\"أ\", \"ب\"]}", 1, 1).unwrap(),
            Value::List(vec![Value::Str("أ".to_string()), Value::Str("ب".to_string())])
        );
        // قاموس تاريخ.
        assert_eq!(
            parse_control_result("{\"قيمة\": {\"سنة\": 2026, \"شهر\": 11, \"يوم\": 6}}", 1, 1)
                .unwrap(),
            Value::Dict(vec![
                ("سنة".to_string(), Value::Number(2026.0)),
                ("شهر".to_string(), Value::Number(11.0)),
                ("يوم".to_string(), Value::Number(6.0)),
            ])
        );
    }

    #[test]
    fn parse_result_cancel_error_invalid() {
        let err = parse_control_result("{\"ألغي\": true}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ألغى المستخدم الحوار"), "got: {}", err);
        let err = parse_control_result("{\"خطأ\": \"أدخل رقما\"}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("أدخل رقما"), "got: {}", err);
        // مشوه.
        let err = parse_control_result("ليست json", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة تحكم غير صالحة"), "got: {}", err);
        // مفتاح غير معروف.
        let err = parse_control_result("{\"غريب\": 1}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة تحكم غير صالحة"), "got: {}", err);
        // قيمة null مرفوضة.
        let err = parse_control_result("{\"قيمة\": null}", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نتيجة تحكم غير صالحة"), "got: {}", err);
    }

    #[test]
    fn show_dialog_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = show_dialog(&[str_val("تأكيد"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("غير مدعوم على هذه المنصة"), "got: {}", err);
        // والوسائط الخاطئة ترفض قبل المنصة (نمط تشفير).
        let err = show_dialog(&[str_val("غريب"), str_val("س؟")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نوع حوار غير معروف"), "got: {}", err);
    }

    // ===== المهمة 21 (جهاز.نبّه + أزل_تنبيه) =====

    #[test]
    fn validate_notify_args_ok_and_bad() {
        let ok = validate_notify_args(&[str_val("انتهى"), str_val("البناء جاهز")], 1, 1)
            .unwrap();
        assert_eq!(ok, ("انتهى".to_string(), "البناء جاهز".to_string()));
        // عدد معاملات خاطئ.
        let err = validate_notify_args(&[str_val("عنوان")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 2 معاملات"), "got: {}", err);
        let err = validate_notify_args(
            &[str_val("ع"), str_val("ن"), str_val("زائد")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("تتوقع 2 معاملات"), "got: {}", err);
        // عنوان غير نص.
        let err = validate_notify_args(&[num_val(5.0), str_val("نص")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لعنوان التنبيه"), "got: {}", err);
        // نص غير نص.
        let err = validate_notify_args(&[str_val("عنوان"), Value::Bool(true)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نصا لنص التنبيه"), "got: {}", err);
        // عنوان فارغ.
        let err = validate_notify_args(&[str_val(""), str_val("نص")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عنوان التنبيه فارغ"), "got: {}", err);
        // نص فارغ.
        let err = validate_notify_args(&[str_val("عنوان"), str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نص التنبيه فارغ"), "got: {}", err);
    }

    #[test]
    fn validate_remove_args_ok_and_bad() {
        assert_eq!(validate_remove_args(&[num_val(3.0)], 1, 1).unwrap(), 3);
        // بلا معامل / معاملان.
        let err = validate_remove_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_remove_args(&[num_val(1.0), num_val(2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        // غير عدد.
        let err = validate_remove_args(&[str_val("١")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع عددا لمعرف التنبيه"), "got: {}", err);
        // كسري.
        let err = validate_remove_args(&[num_val(1.5)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا موجبا"), "got: {}", err);
        // صفر وسالب.
        let err = validate_remove_args(&[num_val(0.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا موجبا"), "got: {}", err);
        let err = validate_remove_args(&[num_val(-2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا موجبا"), "got: {}", err);
    }

    #[test]
    fn notify_registry_reserve_release_consume() {
        // الحجز رتيب لا يُعاد؛ والتحرير يعيد السقف لا
        // المعرّف؛ والاستهلاك يزيل مرة واحدة فقط.
        let first = reserve_notify_id(1, 1).unwrap();
        assert!(first >= 1);
        let second = reserve_notify_id(1, 1).unwrap();
        assert!(second > first, "ids must grow: {} vs {}", first, second);
        assert!(consume_notify_id(second));
        assert!(!consume_notify_id(second));
        release_notify_id(first);
        assert!(!consume_notify_id(first));
        // حجز بعد تحرير لا يعيد المعرّف المحرَّر.
        let third = reserve_notify_id(1, 1).unwrap();
        assert!(third > second, "ids must never repeat");
        release_notify_id(third);
    }

    #[test]
    fn notify_registry_cap() {
        // املأ السقف كاملا ثم تأكد أن الحجز التالي
        // يرفض — ثم حرّر الكل (فلا يبقى أثر في
        // الاختبارات التالية).
        let mut ids = Vec::with_capacity(MAX_LIVE_NOTIFICATIONS);
        for _ in 0..MAX_LIVE_NOTIFICATIONS {
            ids.push(reserve_notify_id(1, 1).unwrap());
        }
        let err = reserve_notify_id(1, 1).unwrap_err().to_string();
        assert!(err.contains("الحد (64)"), "got: {}", err);
        for id in ids {
            release_notify_id(id);
        }
        // بعد التحرير يعود الحجز.
        let id = reserve_notify_id(1, 1).unwrap();
        release_notify_id(id);
    }

    #[test]
    fn serialize_notify_requests_round_trip() {
        let text = serialize_notify_request("انتهى \"البناء\"", "جاهز\nالآن", 7);
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![
                ("عنوان".to_string(), Json::Str("انتهى \"البناء\"".to_string())),
                ("نص".to_string(), Json::Str("جاهز\nالآن".to_string())),
                ("معرف".to_string(), Json::Num(7.0)),
            ])
        );
        let text = serialize_remove_request(42);
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![("معرف".to_string(), Json::Num(42.0))])
        );
    }

    #[test]
    fn show_notification_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = show_notification(&[str_val("عنوان"), str_val("نص")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("جهاز.نبه غير مدعوم على هذه المنصة"), "got: {}", err);
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = show_notification(&[str_val("عنوان"), str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نص التنبيه فارغ"), "got: {}", err);
        // الإزالة: المنصة قبل الحياة — فمعرف غير حيّ
        // على لينكس يرفض بالمنصة لا بـ«غير معروف».
        let err = remove_notification(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("جهاز.أزل_تنبيه غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الخاطئة للإزالة ترفض قبل المنصة.
        let err = remove_notification(&[str_val("١")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع عددا لمعرف التنبيه"), "got: {}", err);
    }

    #[test]
    fn remove_unknown_id_after_consume() {
        // استهلاك معرف غير محجوز أصلا يعيد خطأ — على
        // مستوى السجل الخالص (بلا منصة).
        assert!(!consume_notify_id(9_999_999));
        let id = reserve_notify_id(1, 1).unwrap();
        assert!(consume_notify_id(id));
        assert!(!consume_notify_id(id));
    }

    #[test]
    fn validate_speak_args_ok_and_bad() {
        // نص صحيح.
        assert_eq!(
            validate_speak_args(&[str_val("مرحبا بالعالم")], 1, 1).unwrap(),
            "مرحبا بالعالم".to_string()
        );
        // بلا معامل / معاملان.
        let err = validate_speak_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_speak_args(&[str_val("أ"), str_val("ب")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        // غير نص.
        let err = validate_speak_args(&[num_val(5.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لنص النطق"), "got: {}", err);
        // نص فارغ.
        let err = validate_speak_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نص النطق فارغ"), "got: {}", err);
    }

    #[test]
    fn validate_engines_args_ok_and_bad() {
        // بلا معاملات.
        assert!(validate_engines_args(&[], 1, 1).is_ok());
        // معامل واحد زائد.
        let err = validate_engines_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn serialize_speak_and_engines_requests_round_trip() {
        // النطق: النص يُهرَّب (تنصيص وسطر جديد).
        let text = serialize_speak_request("قال: \"مرحبا\"\nبالعالم");
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![(
                "نص".to_string(),
                Json::Str("قال: \"مرحبا\"\nبالعالم".to_string())
            )])
        );
        // السرد: كائن فارغ.
        let text = serialize_engines_request();
        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed, Json::Obj(vec![]));
    }

    #[test]
    fn parse_engines_result_list_of_dicts() {
        // نتيجة سرد المحركات: قائمة قواميس {اسم، وصف}.
        let value = parse_control_result(
            "{\"قيمة\": [{\"اسم\": \"com.google.android.tts\", \"وصف\": \"محرك جوجل\"}, {\"اسم\": \"com.samsung.tts\", \"وصف\": \"محرك سامسونج\"}]}",
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            value,
            Value::List(vec![
                dict_val(vec![
                    ("اسم", str_val("com.google.android.tts")),
                    ("وصف", str_val("محرك جوجل")),
                ]),
                dict_val(vec![
                    ("اسم", str_val("com.samsung.tts")),
                    ("وصف", str_val("محرك سامسونج")),
                ]),
            ])
        );
        // وقائمة فارغة إجابة صادقة (لا محرك مثبّت).
        let value = parse_control_result("{\"قيمة\": []}", 1, 1).unwrap();
        assert_eq!(value, Value::List(vec![]));
        // ونجاح النطق: صحيح.
        let value = parse_control_result("{\"قيمة\": true}", 1, 1).unwrap();
        assert_eq!(value, Value::Bool(true));
    }

    #[test]
    fn speak_and_engines_refuse_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = speak_text(&[str_val("مرحبا")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("جهاز.انطق غير مدعوم على هذه المنصة"), "got: {}", err);
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = speak_text(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نص النطق فارغ"), "got: {}", err);
        let err = speak_text(&[num_val(3.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لنص النطق"), "got: {}", err);
        // السرد: المنصة قبل أي شيء (بلا وسائط أصلا).
        let err = list_speech_engines(&[], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("جهاز.محركات_النطق غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الزائدة للسرد ترفض قبل المنصة.
        let err = list_speech_engines(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn validate_play_audio_args_ok_and_bad() {
        // مسار صحيح.
        assert_eq!(
            validate_play_audio_args(&[str_val("موسيقى.mp3")], 1, 1).unwrap(),
            "موسيقى.mp3".to_string()
        );
        // بلا معامل / معاملان.
        let err = validate_play_audio_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_play_audio_args(&[str_val("أ"), str_val("ب")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        // غير نص.
        let err = validate_play_audio_args(&[num_val(5.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار الصوت"), "got: {}", err);
        // مسار فارغ.
        let err = validate_play_audio_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار الصوت فارغ"), "got: {}", err);
    }

    #[test]
    fn validate_stop_audio_args_ok_and_bad() {
        // بلا معاملات.
        assert!(validate_stop_audio_args(&[], 1, 1).is_ok());
        // معامل واحد زائد.
        let err = validate_stop_audio_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn serialize_play_stop_audio_requests_round_trip() {
        // التشغيل: المسار المطلق يُهرَّب (شرطة مائلة
        // ومسافة وعربي).
        let text = serialize_play_audio_request("/data/موسيقى/أغنية 1.mp3");
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![(
                "مسار".to_string(),
                Json::Str("/data/موسيقى/أغنية 1.mp3".to_string())
            )])
        );
        // الإيقاف: كائن فارغ.
        let text = serialize_stop_audio_request();
        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed, Json::Obj(vec![]));
    }

    #[test]
    fn play_and_stop_audio_refuse_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية — قبل حلّ
        // المسار أبدا (فمسار خاطئ على لينكس يرفض
        // بالمنصة لا بالمسار).
        let policy =
            crate::permissions::FilePolicy::restricted(std::path::PathBuf::from("/tmp"));
        let cwd = std::path::PathBuf::from("/tmp");
        let err = play_audio(&[str_val("موسيقى.mp3")], &policy, &cwd, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("جهاز.شغل_صوت غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = play_audio(&[str_val("")], &policy, &cwd, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار الصوت فارغ"), "got: {}", err);
        let err = play_audio(&[num_val(3.0)], &policy, &cwd, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار الصوت"), "got: {}", err);
        // الإيقاف: المنصة قبل أي شيء (بلا وسائط أصلا).
        let err = stop_audio(&[], 1, 1).unwrap_err().to_string();
        assert!(
            err.contains("جهاز.أوقف_صوت غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الزائدة للإيقاف ترفض قبل المنصة.
        let err = stop_audio(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn validate_sensor_args_ok_and_bad() {
        // الأسماء الثمانية كلها صحيحة.
        for name in SENSOR_NAMES {
            assert_eq!(
                validate_sensor_args(&[str_val(name)], 1, 1).unwrap(),
                name.to_string(),
                "name: {}",
                name
            );
        }
        // بلا معامل / معاملان.
        let err = validate_sensor_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_sensor_args(&[str_val("تسارع"), str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        // غير نص.
        let err = validate_sensor_args(&[num_val(5.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لاسم الحساس"), "got: {}", err);
        // اسم فارغ.
        let err = validate_sensor_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("اسم الحساس فارغ"), "got: {}", err);
        // اسم غير معروف (ومنها المستثنى عمدا: نبض
        // القلب وعدّاد الخطوات).
        for bad in ["نبض", "خطوات", "سرعة", "تسارع "] {
            let err = validate_sensor_args(&[str_val(bad)], 1, 1)
                .unwrap_err()
                .to_string();
            assert!(err.contains("حساس غير معروف"), "got: {}", err);
        }
    }

    #[test]
    fn serialize_sensor_request_round_trip() {
        // الاسم يُهرَّب في JSON.
        let text = serialize_sensor_request("تسارع");
        let parsed = parse_json(&text).unwrap();
        assert_eq!(
            parsed,
            Json::Obj(vec![("اسم".to_string(), Json::Str("تسارع".to_string()))])
        );
    }

    #[test]
    fn parse_sensor_result_dict() {
        // نتيجة قراءة ثلاثية المحاور: قاموس {قيم، دقة}.
        let value = parse_control_result(
            "{\"قيمة\": {\"قيم\": [1.5, -2.0, 9.8], \"دقة\": 3}}",
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            value,
            Value::Dict(vec![
                (
                    "قيم".to_string(),
                    Value::List(vec![num_val(1.5), num_val(-2.0), num_val(9.8)]),
                ),
                ("دقة".to_string(), num_val(3.0)),
            ])
        );
        // ونتيجة قراءة أحادية القيمة (ضوء).
        let value = parse_control_result(
            "{\"قيمة\": {\"قيم\": [450.0], \"دقة\": 2}}",
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            value,
            Value::Dict(vec![
                ("قيم".to_string(), Value::List(vec![num_val(450.0)])),
                ("دقة".to_string(), num_val(2.0)),
            ])
        );
        // (قيمة ليست قاموسا يرفضها `read_sensor_native`
        // لا المحلل المشترك — فهي خارج هذا الاختبار.)
    }

    #[test]
    fn sensor_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = read_sensor(&[str_val("تسارع")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("جهاز.حساس غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = read_sensor(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("اسم الحساس فارغ"), "got: {}", err);
        let err = read_sensor(&[str_val("نبض")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("حساس غير معروف"), "got: {}", err);
        let err = read_sensor(&[num_val(3.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لاسم الحساس"), "got: {}", err);
        let err = read_sensor(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
    }

    #[test]
    fn validate_listen_args_ok_and_bad() {
        // بلا معاملات صحيحة.
        assert!(validate_listen_args(&[], 1, 1).is_ok());
        // معامل واحد / معاملان.
        let err = validate_listen_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
        let err = validate_listen_args(&[num_val(1.0), num_val(2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn validate_battery_args_ok_and_bad() {
        // بلا معاملات صحيحة.
        assert!(validate_battery_args(&[], 1, 1).is_ok());
        // معامل واحد.
        let err = validate_battery_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn serialize_listen_request_round_trip() {
        // الطلب كائن فارغ.
        let text = serialize_listen_request();
        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed, Json::Obj(vec![]));
    }

    #[test]
    fn serialize_battery_request_round_trip() {
        // الطلب كائن فارغ.
        let text = serialize_battery_request();
        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed, Json::Obj(vec![]));
    }

    #[test]
    fn parse_listen_result_text() {
        // نتيجة نص الكلام المسموع.
        let value = parse_control_result("{\"قيمة\": \"مرحبا يا نيفا\"}", 1, 1).unwrap();
        assert_eq!(
            value,
            Value::Str("مرحبا يا نيفا".to_string())
        );
        // (قيمة ليست نصا يرفضها `listen_speech_native`
        // لا المحلل المشترك — فهي خارج هذا الاختبار.)
    }

    #[test]
    fn parse_battery_result_dict() {
        // نتيجة قاموس البطارية بستة مفاتيح.
        let value = parse_control_result(
            "{\"قيمة\": {\"مستوى\": 87, \"حالة\": \"يشحن\", \"صحة\": \"جيدة\", \"حرارة\": 31.5, \"جهد\": 4200, \"مصدر\": \"تيار متردد\"}}",
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            value,
            Value::Dict(vec![
                ("مستوى".to_string(), num_val(87.0)),
                ("حالة".to_string(), Value::Str("يشحن".to_string())),
                ("صحة".to_string(), Value::Str("جيدة".to_string())),
                ("حرارة".to_string(), num_val(31.5)),
                ("جهد".to_string(), num_val(4200.0)),
                ("مصدر".to_string(), Value::Str("تيار متردد".to_string())),
            ])
        );
        // (قيمة ليست قاموسا يرفضها `read_battery_native`
        // لا المحلل المشترك — فهي خارج هذا الاختبار.)
    }

    #[test]
    fn listen_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = listen_speech(&[], 1, 1).unwrap_err().to_string();
        assert!(
            err.contains("جهاز.اسمع غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = listen_speech(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn battery_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية.
        let err = read_battery(&[], 1, 1).unwrap_err().to_string();
        assert!(
            err.contains("جهاز.بطارية غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = read_battery(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    // ===== الدفعة 4أ من الكتالوج (البنود 22–34) =====

    #[test]
    fn batch4a_share_file_args() {
        assert_eq!(
            validate_share_file_args(&[str_val("صورة.png")], 1, 1).unwrap(),
            "صورة.png"
        );
        let err = validate_share_file_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_share_file_args(&[str_val("أ"), str_val("ب")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_share_file_args(&[num_val(5.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار الملف"), "got: {}", err);
        let err = validate_share_file_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار الملف فارغ"), "got: {}", err);
    }

    #[test]
    fn batch4a_share_text_and_open_file_args() {
        assert_eq!(
            validate_share_text_args(&[str_val("مرحبا")], 1, 1).unwrap(),
            "مرحبا"
        );
        let err = validate_share_text_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("النص المشارك فارغ"), "got: {}", err);
        let err = validate_share_text_args(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للنص المشارك"), "got: {}", err);
        assert_eq!(
            validate_open_file_args(&[str_val("مستند.pdf")], 1, 1).unwrap(),
            "مستند.pdf"
        );
        let err = validate_open_file_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
    }

    #[test]
    fn batch4a_open_url_args() {
        assert_eq!(
            validate_open_url_args(&[str_val("https://example.com")], 1, 1).unwrap(),
            "https://example.com"
        );
        assert_eq!(
            validate_open_url_args(&[str_val("http://x.y/z")], 1, 1).unwrap(),
            "http://x.y/z"
        );
        let err = validate_open_url_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("الرابط فارغ"), "got: {}", err);
        let err = validate_open_url_args(&[str_val("ftp://x.y")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("يجب أن يبدأ بـ http:// أو https://"), "got: {}", err);
        let err = validate_open_url_args(&[str_val("https://exa mple.com")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("يجب أن يبدأ بـ http:// أو https://"), "got: {}", err);
        let err = validate_open_url_args(&[num_val(3.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للرابط"), "got: {}", err);
    }

    #[test]
    fn batch4a_clipboard_args() {
        assert_eq!(
            validate_clipboard_set_args(&[str_val("انسخني")], 1, 1).unwrap(),
            "انسخني"
        );
        let err = validate_clipboard_set_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("النص المنسوخ فارغ"), "got: {}", err);
        let err = validate_clipboard_set_args(&[Value::Bool(true)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للنص المنسوخ"), "got: {}", err);
        assert!(validate_clipboard_get_args(&[], 1, 1).is_ok());
        let err = validate_clipboard_get_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
        assert!(validate_pick_file_args(&[], 1, 1).is_ok());
        let err = validate_pick_file_args(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    #[test]
    fn batch4a_zero_arg_device_validators() {
        // البنود 29–31 و33–34: بلا معاملات؛ وخطأ العدد
        // قبل المنصة.
        for f in [
            validate_location_last_args as fn(&[Value], usize, usize) -> RioResult<()>,
            validate_location_once_args,
            validate_cameras_args,
            validate_wifi_info_args,
            validate_wifi_scan_args,
        ] {
            assert!(f(&[], 1, 1).is_ok());
            let err = f(&[str_val("زائد")], 1, 1).unwrap_err().to_string();
            assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
        }
    }

    #[test]
    fn batch4a_capture_photo_args() {
        // معامل واحد فقط (مسار الصورة) — بلا معرّف
        // كاميرا: نية كاميرا النظام لا تقبله، فمعامل
        // بلا أثر مرفوض عمدا.
        assert_eq!(
            validate_capture_photo_args(&[str_val("صورة.jpg")], 1, 1).unwrap(),
            "صورة.jpg"
        );
        let err = validate_capture_photo_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_capture_photo_args(
            &[str_val("أ"), str_val("1")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_capture_photo_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار الصورة فارغ"), "got: {}", err);
        let err = validate_capture_photo_args(&[num_val(2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار الصورة"), "got: {}", err);
    }

    #[test]
    fn batch4a_serializers() {
        assert_eq!(
            serialize_share_file_request("/بيت/صورة.png"),
            "{\"مسار\":\"/بيت/صورة.png\"}"
        );
        assert_eq!(
            serialize_share_text_request("مرحبا"),
            "{\"نص\":\"مرحبا\"}"
        );
        assert_eq!(
            serialize_open_file_request("/بيت/مستند.pdf"),
            "{\"مسار\":\"/بيت/مستند.pdf\"}"
        );
        assert_eq!(
            serialize_open_url_request("https://example.com"),
            "{\"رابط\":\"https://example.com\"}"
        );
        assert_eq!(
            serialize_clipboard_set_request("انسخني"),
            "{\"نص\":\"انسخني\"}"
        );
        assert_eq!(serialize_clipboard_get_request(), "{}");
        assert_eq!(serialize_pick_file_request(), "{}");
        assert_eq!(serialize_location_last_request(), "{}");
        assert_eq!(serialize_location_once_request(), "{}");
        assert_eq!(serialize_cameras_request(), "{}");
        assert_eq!(
            serialize_capture_photo_request("/بيت/صورة.jpg"),
            "{\"مسار\":\"/بيت/صورة.jpg\"}"
        );
        assert_eq!(serialize_wifi_info_request(), "{}");
        assert_eq!(serialize_wifi_scan_request(), "{}");
        // تهريب علامتي التنصيص والخط الجديد.
        assert_eq!(
            serialize_share_text_request("قال \"مرحبا\"\nثم سكت"),
            "{\"نص\":\"قال \\\"مرحبا\\\"\\nثم سكت\"}"
        );
    }

    #[test]
    fn batch4a_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية؛ والوسائط
        // الخاطئة ترفض قبل المنصة.
        let policy = crate::permissions::FilePolicy::restricted(
            std::path::PathBuf::from("/tmp/لا-يوجد"),
        );
        let cwd = std::path::Path::new("/tmp/لا-يوجد");
        for (got, want) in [
            (
                share_file(&[str_val("صورة.png")], &policy, cwd, 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.شارك_ملف غير مدعوم على هذه المنصة",
            ),
            (
                share_text(&[str_val("مرحبا")], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.شارك_نص غير مدعوم على هذه المنصة",
            ),
            (
                open_file(&[str_val("مستند.pdf")], &policy, cwd, 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.افتح_ملف غير مدعوم على هذه المنصة",
            ),
            (
                open_url(&[str_val("https://example.com")], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.افتح_رابط غير مدعوم على هذه المنصة",
            ),
            (
                clipboard_set(&[str_val("انسخني")], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.انسخ_للحافظة غير مدعوم على هذه المنصة",
            ),
            (
                clipboard_get(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.الحافظة غير مدعوم على هذه المنصة",
            ),
            (
                pick_file(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.اختر_ملف غير مدعوم على هذه المنصة",
            ),
            (
                location_last(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.موقع غير مدعوم على هذه المنصة",
            ),
            (
                location_once(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.موقع_حي غير مدعوم على هذه المنصة",
            ),
            (
                list_cameras(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.كاميرات غير مدعوم على هذه المنصة",
            ),
            (
                capture_photo(&[str_val("صورة.jpg")], &policy, cwd, 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.التقط_صورة غير مدعوم على هذه المنصة",
            ),
            (
                wifi_info(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.واي_فاي غير مدعوم على هذه المنصة",
            ),
            (
                wifi_scan(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.مسح_واي_فاي غير مدعوم على هذه المنصة",
            ),
        ] {
            assert!(got.contains(want), "got: {}", got);
        }
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = open_url(&[str_val("ftp://x.y")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("يجب أن يبدأ بـ http:// أو https://"), "got: {}", err);
        let err = share_text(&[str_val("")], 1, 1).unwrap_err().to_string();
        assert!(err.contains("النص المشارك فارغ"), "got: {}", err);
    }

    // ===== الدفعة 4ب من الكتالوج (البنود 61 و62 و66–72
    // و79 و105 و106 و119) =====

    #[test]
    fn batch4b_zero_arg_device_validators() {
        // بلا معاملات؛ وخطأ العدد قبل المنصة.
        for f in [
            validate_copy_last_error_args
                as fn(&[Value], usize, usize) -> RioResult<()>,
            validate_share_session_args,
            validate_pause_audio_args,
            validate_resume_audio_args,
            validate_list_sensors_args,
            validate_volume_level_args,
            validate_audio_device_info_args,
        ] {
            assert!(f(&[], 1, 1).is_ok());
            let err = f(&[str_val("زائد")], 1, 1).unwrap_err().to_string();
            assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
        }
    }

    #[test]
    fn batch4b_audio_info_args() {
        assert_eq!(
            validate_audio_info_args(&[str_val("صوت.mp3")], 1, 1).unwrap(),
            "صوت.mp3"
        );
        let err = validate_audio_info_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
        let err = validate_audio_info_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار الصوت فارغ"), "got: {}", err);
        let err = validate_audio_info_args(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار الصوت"), "got: {}", err);
    }

    #[test]
    fn batch4b_set_volume_args() {
        assert_eq!(validate_set_volume_args(&[num_val(5.0)], 1, 1).unwrap(), 5);
        assert_eq!(validate_set_volume_args(&[num_val(0.0)], 1, 1).unwrap(), 0);
        let err = validate_set_volume_args(&[num_val(-1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("لا يقل عن 0"), "got: {}", err);
        let err = validate_set_volume_args(&[num_val(2.5)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
        let err = validate_set_volume_args(&[str_val("عال")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع عددا للمستوى"), "got: {}", err);
        let err = validate_set_volume_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
    }

    #[test]
    fn batch4b_vibrate_args() {
        assert_eq!(validate_vibrate_args(&[], 1, 1).unwrap(), 1000);
        assert_eq!(validate_vibrate_args(&[num_val(250.0)], 1, 1).unwrap(), 250);
        assert_eq!(
            validate_vibrate_args(&[num_val(10_000.0)], 1, 1).unwrap(),
            10_000
        );
        let err = validate_vibrate_args(&[num_val(0.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("بين 1 و10000"), "got: {}", err);
        let err = validate_vibrate_args(&[num_val(10_001.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("بين 1 و10000"), "got: {}", err);
        let err = validate_vibrate_args(&[num_val(1.5)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
        let err = validate_vibrate_args(&[str_val("طويل")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع عددا للمدة"), "got: {}", err);
        let err = validate_vibrate_args(&[num_val(1.0), num_val(2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 أو 1 معامل"), "got: {}", err);
    }

    #[test]
    fn batch4b_torch_args() {
        assert!(validate_torch_args(&[Value::Bool(true)], 1, 1).unwrap());
        assert!(!validate_torch_args(&[Value::Bool(false)], 1, 1).unwrap());
        let err = validate_torch_args(&[str_val("شغل")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع منطقيا للحالة"), "got: {}", err);
        let err = validate_torch_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
    }

    #[test]
    fn batch4b_record_audio_args() {
        // معامل واحد: المدة الافتراضية 60.
        assert_eq!(
            validate_record_audio_args(&[str_val("تسجيل.m4a")], 1, 1).unwrap(),
            ("تسجيل.m4a".to_string(), 60)
        );
        assert_eq!(
            validate_record_audio_args(
                &[str_val("تسجيل.m4a"), num_val(30.0)],
                1,
                1
            )
            .unwrap(),
            ("تسجيل.m4a".to_string(), 30)
        );
        let err = validate_record_audio_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 أو 2 معاملات"), "got: {}", err);
        let err = validate_record_audio_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("مسار التسجيل فارغ"), "got: {}", err);
        let err = validate_record_audio_args(
            &[str_val("أ"), num_val(0.0)],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("بين 1 و300"), "got: {}", err);
        let err = validate_record_audio_args(
            &[str_val("أ"), num_val(301.0)],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("بين 1 و300"), "got: {}", err);
        let err = validate_record_audio_args(
            &[str_val("أ"), num_val(1.5)],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
        let err = validate_record_audio_args(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا لمسار التسجيل"), "got: {}", err);
    }

    #[test]
    fn batch4b_brightness_args() {
        assert_eq!(
            validate_brightness_args(&[num_val(50.0)], 1, 1).unwrap(),
            BrightnessArg::Level(50)
        );
        assert_eq!(
            validate_brightness_args(&[num_val(0.0)], 1, 1).unwrap(),
            BrightnessArg::Level(0)
        );
        assert_eq!(
            validate_brightness_args(&[num_val(100.0)], 1, 1).unwrap(),
            BrightnessArg::Level(100)
        );
        assert_eq!(
            validate_brightness_args(&[str_val("تلقائي")], 1, 1).unwrap(),
            BrightnessArg::Auto
        );
        let err = validate_brightness_args(&[num_val(101.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("بين 0 و100"), "got: {}", err);
        let err = validate_brightness_args(&[num_val(-1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("بين 0 و100"), "got: {}", err);
        let err = validate_brightness_args(&[num_val(50.5)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("عددا صحيحا"), "got: {}", err);
        let err = validate_brightness_args(&[str_val("ساطع")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("«تلقائي» فقط"), "got: {}", err);
        let err = validate_brightness_args(&[Value::Bool(true)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع عددا أو نص «تلقائي»"), "got: {}", err);
        let err = validate_brightness_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 معامل"), "got: {}", err);
    }

    #[test]
    fn batch4b_toast_args() {
        assert_eq!(
            validate_toast_args(&[str_val("تم")], 1, 1).unwrap(),
            ("تم".to_string(), false)
        );
        assert_eq!(
            validate_toast_args(&[str_val("تم"), str_val("طويل")], 1, 1).unwrap(),
            ("تم".to_string(), true)
        );
        assert_eq!(
            validate_toast_args(&[str_val("تم"), str_val("قصير")], 1, 1).unwrap(),
            ("تم".to_string(), false)
        );
        let err = validate_toast_args(&[str_val("")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("نص التوست فارغ"), "got: {}", err);
        let err = validate_toast_args(&[str_val("تم"), str_val("متوسط")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("«قصير» أو «طويل»"), "got: {}", err);
        let err = validate_toast_args(&[], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع 1 أو 2 معاملات"), "got: {}", err);
        let err = validate_toast_args(
            &[str_val("أ"), str_val("قصير"), str_val("زائد")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("تتوقع 1 أو 2 معاملات"), "got: {}", err);
        let err = validate_toast_args(&[num_val(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للرسالة"), "got: {}", err);
    }

    #[test]
    fn batch4b_serializers() {
        assert_eq!(serialize_empty_request(), "{}");
        assert_eq!(
            serialize_audio_info_request("/بيت/صوت.mp3"),
            "{\"مسار\":\"/بيت/صوت.mp3\"}"
        );
        assert_eq!(serialize_set_volume_request(5), "{\"مستوى\":5}");
        assert_eq!(serialize_vibrate_request(250), "{\"مدة\":250}");
        assert_eq!(serialize_torch_request(true), "{\"تشغيل\":true}");
        assert_eq!(serialize_torch_request(false), "{\"تشغيل\":false}");
        assert_eq!(
            serialize_record_audio_request("/بيت/تسجيل.m4a", 30),
            "{\"مسار\":\"/بيت/تسجيل.m4a\",\"مدة\":30}"
        );
        assert_eq!(
            serialize_brightness_request(&BrightnessArg::Level(50)),
            "{\"مستوى\":50}"
        );
        assert_eq!(
            serialize_brightness_request(&BrightnessArg::Auto),
            "{\"تلقائي\":true}"
        );
        assert_eq!(
            serialize_toast_request("تم", false),
            "{\"نص\":\"تم\",\"طويل\":false}"
        );
        assert_eq!(
            serialize_toast_request("تم", true),
            "{\"نص\":\"تم\",\"طويل\":true}"
        );
    }

    #[test]
    fn batch4b_refuses_off_android() {
        // على هذه الآلة (لينكس): الوسائط الصحيحة تصل
        // إلى بوابة المنصة وترفض بالعربية؛ والوسائط
        // الخاطئة ترفض قبل المنصة.
        let policy = crate::permissions::FilePolicy::restricted(
            std::path::PathBuf::from("/tmp/لا-يوجد"),
        );
        let cwd = std::path::Path::new("/tmp/لا-يوجد");
        for (got, want) in [
            (
                copy_last_error(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.انسخ_آخر_خطأ غير مدعوم على هذه المنصة",
            ),
            (
                share_session(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.شارك_الجلسة غير مدعوم على هذه المنصة",
            ),
            (
                pause_audio(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.أوقف_صوت_مؤقتا غير مدعوم على هذه المنصة",
            ),
            (
                resume_audio(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.استأنف_صوت غير مدعوم على هذه المنصة",
            ),
            (
                audio_info(&[str_val("صوت.mp3")], &policy, cwd, 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.معلومات_صوت غير مدعوم على هذه المنصة",
            ),
            (
                list_sensors(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.حساسات غير مدعوم على هذه المنصة",
            ),
            (
                volume_level(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.مستوى_الصوت غير مدعوم على هذه المنصة",
            ),
            (
                set_volume_level(&[num_val(5.0)], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.اضبط_مستوى_الصوت غير مدعوم على هذه المنصة",
            ),
            (
                vibrate(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.اهتز غير مدعوم على هذه المنصة",
            ),
            (
                torch(&[Value::Bool(true)], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.مصباح غير مدعوم على هذه المنصة",
            ),
            (
                record_audio(&[str_val("تسجيل.m4a")], &policy, cwd, 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.سجّل_صوت غير مدعوم على هذه المنصة",
            ),
            (
                brightness(&[num_val(50.0)], 1, 1)
                    .unwrap_err()
                    .to_string(),
                "جهاز.سطوع غير مدعوم على هذه المنصة",
            ),
            (
                toast(&[str_val("تم")], 1, 1).unwrap_err().to_string(),
                "جهاز.توست غير مدعوم على هذه المنصة",
            ),
            (
                audio_device_info(&[], 1, 1).unwrap_err().to_string(),
                "جهاز.معلومات_الصوت غير مدعوم على هذه المنصة",
            ),
        ] {
            assert!(got.contains(want), "got: {}", got);
        }
        // والوسائط الخاطئة ترفض قبل المنصة.
        let err = set_volume_level(&[num_val(-1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("لا يقل عن 0"), "got: {}", err);
        let err = vibrate(&[num_val(0.0)], 1, 1).unwrap_err().to_string();
        assert!(err.contains("بين 1 و10000"), "got: {}", err);
        let err = torch(&[str_val("شغل")], 1, 1).unwrap_err().to_string();
        assert!(err.contains("تتوقع منطقيا للحالة"), "got: {}", err);
        let err = brightness(&[str_val("ساطع")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("«تلقائي» فقط"), "got: {}", err);
        let err = toast(&[str_val("تم"), str_val("متوسط")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("«قصير» أو «طويل»"), "got: {}", err);
        let err = record_audio(&[str_val("أ"), num_val(0.0)], &policy, cwd, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("بين 1 و300"), "got: {}", err);
    }

    // الدفعة 5 (البند 98): التحقق من وسائط
    // `جهاز.نزّل` — العدد والنوع والفراغ وhttps
    // وتنظيف اسم الملف.
    #[test]
    fn validate_download_args_ok_and_bad() {
        let ok = validate_download_args(
            &[
                str_val("https://مثال.موقع/ملف"),
                str_val("عنوان"),
                str_val("وصف"),
                str_val("ملف.نيفا"),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(
            ok,
            (
                "https://مثال.موقع/ملف".to_string(),
                "عنوان".to_string(),
                "وصف".to_string(),
                "ملف.نيفا".to_string()
            )
        );
        // اسم ملف بمسار مرفق يُنظَّف إلى آخر جزء.
        let ok = validate_download_args(
            &[
                str_val("https://مثال.موقع/ملف"),
                str_val("عنوان"),
                str_val("وصف"),
                str_val("مجلد/فرعي/ملف.نيفا"),
            ],
            1,
            1,
        )
        .unwrap();
        assert_eq!(ok.3, "ملف.نيفا".to_string());
        // عدد معاملات خاطئ.
        let err = validate_download_args(
            &[str_val("https://مثال.موقع/ملف"), str_val("عنوان")],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("تتوقع 4 معاملات"), "got: {}", err);
        // معامل غير نص.
        let err = validate_download_args(
            &[
                str_val("https://مثال.موقع/ملف"),
                num_val(5.0),
                str_val("وصف"),
                str_val("ملف.نيفا"),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("تتوقع نصا"), "got: {}", err);
        // رابط غير https.
        let err = validate_download_args(
            &[
                str_val("http://مثال.موقع/ملف"),
                str_val("عنوان"),
                str_val("وصف"),
                str_val("ملف.نيفا"),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("https://"), "got: {}", err);
        // اسم ملف `..` بعد التنظيف مرفوض.
        let err = validate_download_args(
            &[
                str_val("https://مثال.موقع/ملف"),
                str_val("عنوان"),
                str_val("وصف"),
                str_val("../.."),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("اسم ملف غير صالح"), "got: {}", err);
        // اسم ملف فارغ مرفوض.
        let err = validate_download_args(
            &[
                str_val("https://مثال.موقع/ملف"),
                str_val("عنوان"),
                str_val("وصف"),
                str_val("   "),
            ],
            1,
            1,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("فارغ"), "got: {}", err);
    }

    // الدفعة 5 (البند 99 خطوة 1): التحقق من
    // وسائط `جهاز.اقرأ_مستند` و`جهاز.اعرض_مجلد`
    // — بلا معاملات.
    #[test]
    fn validate_read_document_and_list_folder_args_ok_and_bad() {
        assert!(validate_read_document_args(&[], 1, 1).is_ok());
        assert!(validate_list_folder_args(&[], 1, 1).is_ok());
        let err = validate_read_document_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
        let err = validate_list_folder_args(&[str_val("زائد")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);
    }

    // الدفعة 5 (البند 98): تسلسل طلب التنزيل —
    // كل الحقول الأربعة موجودة بقيمها.
    #[test]
    fn serialize_download_request_round_trip() {
        let json = serialize_download_request(
            "https://مثال.موقع/ملف",
            "عنوان",
            "وصف",
            "ملف.نيفا",
        );
        assert!(json.contains("\"رابط\":\"https://مثال.موقع/ملف\""), "got: {}", json);
        assert!(json.contains("\"عنوان\":\"عنوان\""), "got: {}", json);
        assert!(json.contains("\"وصف\":\"وصف\""), "got: {}", json);
        assert!(json.contains("\"اسم_ملف\":\"ملف.نيفا\""), "got: {}", json);
    }
}
