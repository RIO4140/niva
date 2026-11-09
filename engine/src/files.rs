//! Open files for the `ملفات` native module (catalog task 12,
//! from `fs/`): open a file by handle, read its whole content,
//! write (replace) its whole content. One implementation,
//! shared by both engines (interpreter + VM) through the stdlib
//! native functions — never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-03-FILES.md` §1): the policy check happens once,
//! at `افتح`, through the session's own FilePolicy
//! (resolve_in against the session cwd — the same call
//! `اقرأ_ملف` uses, so the outside-folder error is
//! byte-identical to the flat builtins); `اقرأ`/`اكتب` take a
//! numeric handle only, never a path. Handles start at 1 and
//! never repeat in one run. Reads are capped at 5 MiB and
//! writes at the 100 MiB program-dir quota — the same caps and
//! the same Arabic messages as the flat builtins. On WASM all
//! three refuse with the flat builtins' own message. There is
//! no `أغلق` in v1: handles close when the process exits.

use crate::error::{ErrorKind, RioError, RioResult};
use crate::permissions::FilePolicy;
use crate::value::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// One open file: the handle itself plus the display form of
/// the path it was opened with (for error messages only — the
/// resolved filesystem path is never shown to Niva code).
/// `read_only` (Meeting 50 §64): a file opened under the
/// downloads root is read-only — `ملفات.اكتب` on its handle
/// refuses with the read-only Arabic error.
struct OpenFile {
    file: File,
    display: String,
    read_only: bool,
}

/// Open files: handle -> file. One table for the whole OS
/// process, shared by both engines (a Niva program is one OS
/// process, so it has one file table).
static REGISTRY: LazyLock<Mutex<HashMap<u64, OpenFile>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// File handles start at 1 and never repeat in one run.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One writer at a time, across every write path: the
/// flat `اكتب_ملف`, `ملفات.اكتب`, and the async write
/// worker (task 19) all hold this lock around (quota
/// check + write), so the 100 MiB quota can never be
/// overrun by concurrent writers. Before task 19 a Niva
/// program was single-threaded, so no write race was
/// possible at all; the async worker is the first
/// concurrent writer, and this lock is its guard. It is a
/// leaf lock (nothing else is taken while holding it),
/// so no lock ordering cycle exists.
pub(crate) static FILE_WRITE_LOCK: LazyLock<Mutex<()>> =
    LazyLock::new(|| Mutex::new(()));

/// Test-only serialization for the process-wide registry:
/// cargo runs tests in parallel threads, and the registry is
/// a single static. Every test that touches it locks this
/// first (same pattern as MEM_TEST_LOCK in memory.rs).
#[cfg(test)]
pub(crate) static FILE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lock the test serialization lock, recovering from
/// poisoning: if an earlier test panicked while holding it,
/// the next test must still run and report its OWN result
/// instead of failing with PoisonError (CI 2026-11-06: one
/// macOS panic cascaded into four PoisonErrors that hid
/// every other test's verdict). This is cascade-hardening
/// only — the panicking test itself still fails loudly on
/// its own line.
#[cfg(test)]
fn file_test_lock() -> std::sync::MutexGuard<'static, ()> {
    FILE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test-only: drop every open file and close its handle.
/// The caller must hold FILE_TEST_LOCK (via file_test_lock),
/// so no other files test can hold a handle at the time.
#[cfg(test)]
pub(crate) fn test_clear() {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.clear();
    }
}

/// Test-only: how many files are open right now.
#[cfg(test)]
pub(crate) fn test_open_count() -> usize {
    REGISTRY.lock().map(|reg| reg.len()).unwrap_or(0)
}

/// The disk quota for the program dir (100 MiB) — the same
/// constant and the same walk as the engines' check, so the
/// message can never drift from `اكتب_ملف`'s.
/// The disk-quota check (also used by the async write
/// worker, task 19 — same base, same message). Native
/// only: no caller exists on WASM, so it is compiled out
/// there (no dead-code warning).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn check_disk_quota(
    base: &Path,
    extra_bytes: u64,
    line: usize,
    col: usize,
) -> RioResult<()> {
    let mut budget = 10000usize;
    let used = crate::interpreter::Interpreter::dir_size(base, &mut budget);
    if used.saturating_add(extra_bytes) > crate::interpreter::Interpreter::DISK_QUOTA_BYTES
    {
        return Err(RioError::new(
            ErrorKind::Io,
            "تجاوزت حصة التخزين (100 ميغابايت) — احذف ملفات قبل الكتابة",
            line,
            col,
        ));
    }
    Ok(())
}

/// هل المساران الملف نفسه؟ فحص قبل النسخ، كما في
/// `same_file_ok` في لينكس (الميزة 202: `cp` يرفض
/// «are the same file»). على يونكس: الجهاز+العقدة
/// (البيانات الوصفية تتبع الروابط الرمزية، فيلتقط
/// الفحص الاسم نفسه و`./` و`..` والمسار المطلق
/// والرابط الرمزي والرابط الصلب). خارجه: مقارنة
/// المسارين القانونيين الموحّدين. وإن غاب أحد
/// المسارين فلا تطابق — فالهدف غير الموجود ليس
/// الملف نفسه أبدا.
pub(crate) fn paths_same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(ca), Ok(cb)) => ca == cb,
            _ => false,
        }
    }
}

/// نتيجة البحث عن ملف أداة (الدفعة 3 — التشغيل،
/// الاجتماع 57 §72، الميزة 39): تعريف التنفيذي
/// لملف `.نيفا` — موجود + ملف عادي + مقروء
/// (بت القراءة يقوم مقام بت التنفيذ، فالملف نص
/// يقرأه المفسر لا ثنائي تنفذه النواة؛ وبت التنفيذ
/// نفسه لا يُشترط وإلا كُسرت كل أداة مثبتة).
/// المجلد وغير المقروء لا يُعدّان تطابقا أبدا بل
/// يُتخطيان إلى المكان التالي — مرآة `FS_NODIRS`
/// و`FS_EXEC_PREFERRED` في لينكس.
#[derive(Debug)]
pub enum ToolLookup {
    /// وُجد تنفيذي: مساره الكامل، و`في_البيت`
    /// صحيحة إن كان في البيت (`~`) لا في `$نيفا`.
    Found { path: std::path::PathBuf, in_home: bool },
    /// الاسم موجود كمجلد في مكان واحد على الأقل
    /// (ولم يوجد تنفيذي في أي مكان).
    IsDir,
    /// الاسم موجود كملف غير مقروء في مكان واحد
    /// على الأقل (ولم يوجد تنفيذي في أي مكان).
    Unreadable,
    /// الاسم غائب في المكانين معا.
    Missing,
}

/// ابحث عن ملف أداة باسمه الكامل (مع الامتداد)
/// في `$نيفا` أولا ثم في البيت — المثبَّت يفوز
/// عند التعارض (اجتماع 45 §59، البند 35).
/// المرشح المجلد أو غير المقروء يُتخطى إلى المكان
/// التالي؛ وإن لم يوجد تنفيذي يُرجع أول عائق
/// بالترتيب (مجلد قبل غير مقروء إن سبقه).
pub(crate) fn find_tool_file(
    program_dir: &Path,
    file_name: &str,
) -> ToolLookup {
    let mut first_bad: Option<ToolLookup> = None;
    for (in_home, path) in [
        (false, program_dir.join(file_name)),
        (true, program_dir.join("home").join(file_name)),
    ] {
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => {
                if first_bad.is_none() {
                    first_bad = Some(ToolLookup::IsDir);
                }
            }
            Ok(meta) if meta.is_file() => {
                if is_readable_file(&meta, &path) {
                    return ToolLookup::Found { path, in_home };
                }
                if first_bad.is_none() {
                    first_bad = Some(ToolLookup::Unreadable);
                }
            }
            // غير ملف ولا مجلد (أو غائب): يُتخطى —
            // والغائب في المكانين معا «غائب» في النهاية.
            _ => {}
        }
    }
    first_bad.unwrap_or(ToolLookup::Missing)
}

/// هل هذا الملف المقروء فعلا؟ الفحص ببت القراءة
/// (أي بت من 0400/0040/0004 على يونكس — حاسم
/// وقابل للاختبار حتى بصلاحيات الجذر)، ومحاولة
/// فتح خارجه.
fn is_readable_file(meta: &std::fs::Metadata, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o444 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        std::fs::File::open(path).is_ok()
    }
}

/// هل خطأ الإدخال/الإخراج هذا اختلاف جهاز (EXDEV) —
/// أي أن إعادة التسمية مستحيلة بين جهازين، لا أنها
/// مرفوضة لسبب آخر؟ `CrossesDevices` هو ترجمة
/// المكتبة القياسية لـEXDEV (يونكس) و
/// ERROR_NOT_SAME_DEVICE (ويندوز)؛ والرمز الخام
/// احتياط إن لم تترجمه المكتبة.
pub(crate) fn is_cross_device_error(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(18)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// المسار القانوني لمسار قد لا يوجد بعد: أطول بادئة
/// موجودة تُقنَّن، ثم تُعاد اللاحقة كما هي — مرآة
/// `nv_canonical_with_suffix` في الخلفية C.
pub(crate) fn canonical_with_suffix(path: &Path) -> std::path::PathBuf {
    let mut cur = path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = std::fs::canonicalize(&cur) {
            let mut out = c;
            for comp in suffix.iter().rev() {
                out.push(comp);
            }
            return out;
        }
        match cur.file_name() {
            Some(name) => {
                suffix.push(name.to_os_string());
                cur = cur
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();
                if cur.as_os_str().is_empty() {
                    cur = if path.is_absolute() {
                        std::path::PathBuf::from("/")
                    } else {
                        std::path::PathBuf::from(".")
                    };
                }
            }
            None => {
                let mut out = std::fs::canonicalize(&cur).unwrap_or(cur);
                for comp in suffix.iter().rev() {
                    out.push(comp);
                }
                return out;
            }
        }
    }
}

/// هل هدف الحذف هو الجذر؟ فحص قانوني قبل حل
/// الساندبوكس (الميزة 208): يلحق المسار بالمجلد
/// الحالي (مطلقا كان أو نسبيا) ثم يقنّن أطول
/// بادئة موجودة — فيكشف الرابط الرمزي إلى الجذر
/// قبل أن يرفضه الساندبوكس برسالة عامة.
pub(crate) fn delete_target_is_root(cwd: &Path, path: &str) -> bool {
    let joined = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        cwd.join(path)
    };
    is_filesystem_root(&canonical_with_suffix(&joined))
}

/// هل الهدف هو المصدر نفسه أو داخل فرع منه؟ فحص
/// قبل النقل، كما في `copy_into_self` في لينكس
/// (الميزة 204: `mv` يرفض نقل مجلد داخل نفسه).
/// المقارنة مكونات لا نصا، فتلتقط `..` والروابط
/// الرمزية. وإن غاب المصدر فلا تطابق — فإعادة
/// التسمية ستفشل أصلا بخطأ «غير موجود».
pub(crate) fn dst_inside_src(src: &Path, dst: &Path) -> bool {
    let src_c = match std::fs::canonicalize(src) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let dst_c = canonical_with_suffix(dst);
    dst_c == src_c || dst_c.starts_with(&src_c)
}

/// حجم شجرة (ملفاتها فقط، بلا اتباع روابط رمزية)
/// لحساب حصة التخزين قبل نسخ مجلد عبر الأنظمة —
/// مرآة `Interpreter::dir_size` بميزانية المداخل
/// نفسها، بلا حاجة إلى جلسة.
pub(crate) fn tree_size(path: &Path) -> u64 {
    fn walk(p: &Path, budget: &mut usize) -> u64 {
        if *budget == 0 {
            return 0;
        }
        *budget -= 1;
        match std::fs::symlink_metadata(p) {
            Ok(m) if m.file_type().is_symlink() => 0,
            Ok(m) if m.is_dir() => {
                let mut total = 0u64;
                if let Ok(entries) = std::fs::read_dir(p) {
                    for e in entries.flatten() {
                        total += walk(&e.path(), budget);
                        if *budget == 0 {
                            break;
                        }
                    }
                }
                total
            }
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }
    walk(path, &mut 10000usize)
}

/// نسخ شجرة تكراريا: ملف ينسخ محتواه (ويستبدل
/// ملف هدف موجود، كما في إعادة التسمية)، ومجلد
/// ينشأ وينسخ أبناؤه، ورابط رمزي يعاد إنشاؤه
/// رابطا (بلا اتباع هدفه — فلا خروج من الساندبوكس
/// عبر رابط داخل الشجرة). ومجلد هدف موجود فارغ
/// يستخدم كما هو (كاستبدال إعادة التسمية لمجلد
/// فارغ)؛ وغير الفارغ أو الملف خطأ.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(src)?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, dst)?;
        }
        #[cfg(windows)]
        {
            let is_dir = std::fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false);
            if is_dir {
                std::os::windows::fs::symlink_dir(&target, dst)?;
            } else {
                std::os::windows::fs::symlink_file(&target, dst)?;
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = target;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "symlink",
            ));
        }
        return Ok(());
    }
    if meta.is_dir() {
        match std::fs::create_dir(dst) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match std::fs::metadata(dst) {
                Ok(m) if m.is_dir() => {
                    let non_empty =
                        std::fs::read_dir(dst).map(|mut it| it.next().is_some()).unwrap_or(true);
                    if non_empty {
                        return Err(e);
                    }
                }
                _ => return Err(e),
            },
            Err(e) => return Err(e),
        }
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// حذف شجرة: ملف أو رابط يزال وحده (بلا اتباع
/// الهدف)، ومجلد يزال تكراريا.
pub(crate) fn remove_tree(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// نتيجة مسار نسخ-ثم-حذف: فشل النسخ (المصدر سليم
/// — بلا حذف جزئي) مقابل فشل الحذف بعد نسخ ناجح
/// (الملف صار في المكانين، فيبلَّغ بذلك صراحة).
pub(crate) enum MoveError {
    Copy(std::io::Error),
    Remove(std::io::Error),
}

/// مسار نسخ-ثم-حذف عبر الأنظمة (الميزة 204: `mv`
/// حين تفشل إعادة التسمية باختلاف الجهاز): انسخ
/// أولا، واحذف المصدر بعد نجاح النسخ كاملا فقط.
/// وإن فشل النسخ وكان الهدف جديدا، أزل النسخة
/// الجزئية (بأفضل جهد) ولا تمسّ هدفا كان موجودا.
pub(crate) fn move_fallback(src: &Path, dst: &Path) -> Result<(), MoveError> {
    let dst_existed = std::fs::symlink_metadata(dst).is_ok();
    if let Err(e) = copy_tree(src, dst) {
        if !dst_existed {
            let _ = remove_tree(dst);
        }
        return Err(MoveError::Copy(e));
    }
    remove_tree(src).map_err(MoveError::Remove)
}

/// رفض حذف الجذر نصا (الميزة 208، الاجتماع 59
/// §74): قاعدة صريحة قبل أي حل ساندبوكس — لا
/// بالساندبوكس وحده. المكون الأخير `.` أو `..`
/// (بعد تقليم `/` اللاحقة) مرفوض، والجذر `/`
/// مرفوض. والمسار الفارغ ليس جذرا (يتعامل معه
/// المسار العادي). يرجع رسالة الرفض أو لاشيء.
pub(crate) fn root_refusal_text(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return None;
    }
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Some("لا يمكن حذف الجذر: /");
    }
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if last == "." || last == ".." {
        return Some("لا يمكن حذف . أو ..");
    }
    None
}

/// هل هذا المسار المحلول هو جذر نظام الملفات `/`
/// نفسه — بما فيه عبر رابط رمزي؟ المقارنة
/// قانونية (تتبع الروابط) وجهاز+عقدة على يونكس،
/// كما في `ROOT_DEV_INO_CHECK` في لينكس (الميزة
/// 208). وإن غاب المسار فليس جذرا.
pub(crate) fn is_filesystem_root(path: &Path) -> bool {
    if let Ok(c) = std::fs::canonicalize(path) {
        if c.as_os_str() == "/" {
            return true;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(m), Ok(r)) = (std::fs::metadata(path), std::fs::metadata("/")) {
            return m.dev() == r.dev() && m.ino() == r.ino();
        }
    }
    false
}

/// هل خطأ الإدخال/الإخراج هذا «مجلد غير فارغ»؟
/// `DirectoryNotEmpty` القياسي، والرمزان الخامان
/// احتياطا (39 يونكس، و145 ويندوز).
pub(crate) fn is_dir_not_empty_error(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
        return true;
    }
    matches!(e.raw_os_error(), Some(39) | Some(145))
}

/// حلل وضع أذونات ثمانيا («755» أو «0755») —
/// 3 أو 4 أرقام 0–7 فقط (الميزة 211). وكل ما
/// عداه لاشيء (فيبلَّغ بخطأ عربي).
pub(crate) fn parse_octal_mode(s: &str) -> Option<u32> {
    if s.len() < 3 || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for c in s.chars() {
        v = v * 8 + c.to_digit(8)?;
    }
    Some(v)
}

/// طبق وضع أذونات على مسار (الميزة 211): على
/// يونكس الوضع تماما؛ وعلى ويندوز بت الكتابة
/// للمالك وحده (بلا كتابة → للقراءة فقط) وباقي
/// البتات بلا مقابل — مسجل في §74.
pub(crate) fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let mut perm = std::fs::metadata(path)?.permissions();
        perm.set_readonly(mode & 0o200 == 0);
        std::fs::set_permissions(path, perm)
    }
}

/// اضبط وقتي ملف أو مجلد على `t` (الوصول والتعديل).
/// ويندوز: المقبض المفتوح للقراءة وحده بلا حق كتابة
/// السمات (FILE_WRITE_ATTRIBUTES)، فيرفض SetFileTime
/// به (Access Denied، os error 5) — فافتح بحق السمة
/// وحده (0x100)، مع علم FILE_FLAG_BACKUP_SEMANTICS
/// (0x02000000) ليعمل الفتح نفسه على المجلدات أيضا.
/// يونكس: فتح القراءة يكفي (الملكية وحدها تحدث الوقت).
pub(crate) fn set_file_times(path: &Path, t: std::time::SystemTime) -> std::io::Result<()> {
    set_file_times_both(
        path,
        std::fs::FileTimes::new()
            .set_accessed(t)
            .set_modified(t),
    )
}

/// اضبط وقتي ملف أو مجلد من `FileTimes` جاهزة (قد
/// يختلف الوصول عن التعديل — كما في حفظ وقت المصدر
/// عند النسخ). المنصة نفسها كـ`set_file_times`.
fn set_file_times_both(path: &Path, times: std::fs::FileTimes) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .access_mode(0x100)
            .custom_flags(0x02000000)
            .open(path)?;
        f.set_times(times)
    }
    #[cfg(not(windows))]
    {
        std::fs::File::open(path)?.set_times(times)
    }
}

/// حدّث وقتي الوصول والتعديل إلى الآن بلا مساس
/// بالمحتوى (الميزة 215 — `touch`).
pub(crate) fn touch_times(path: &Path) -> std::io::Result<()> {
    set_file_times(path, std::time::SystemTime::now())
}

/// المسار النسبي من مجلد إلى هدف (الميزة 213 —
/// `ln -r`): مكونات مشتركة ثم `..` لكل مكون زائد
/// في مجلد المصدر. والطرفان مطلقان محلولان عند
/// الاستدعاء؛ وبلا بادئة مشتركة (قرصان مختلفان
/// على ويندوز) يبقى الهدف المطلق كما هو.
// مستدعاة من فروع يونكس/ويندوز وحدهما — على
// WASM مستدعوها مُكيَّفون خارجا فيُسمح بعدم
// الاستعمال هناك.
#[allow(dead_code)]
pub(crate) fn relative_path(from_dir: &Path, to: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let from: Vec<Component<'_>> = from_dir.components().collect();
    let to_c: Vec<Component<'_>> = to.components().collect();
    let mut i = 0;
    while i < from.len() && i < to_c.len() && from[i] == to_c[i] {
        i += 1;
    }
    if i == 0 {
        return to.to_path_buf();
    }
    let mut out = std::path::PathBuf::new();
    for _ in i..from.len() {
        out.push("..");
    }
    for c in &to_c[i..] {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// المكونات الناقصة من مسار محلول (من أول سلف
/// موجود إلى المسار نفسه) — لإعلان `mkdir -v`
/// (الميزة 211): تُحسب قبل الإنشاء، فالقائمة
/// الفارغة تعني «موجود أصلا».
pub(crate) fn missing_components(path: &Path) -> Vec<std::path::PathBuf> {
    let mut missing = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if cur.exists() {
            break;
        }
        missing.push(cur.clone());
        match cur.parent() {
            Some(p) if !p.as_os_str().is_empty() => cur = p.to_path_buf(),
            _ => break,
        }
    }
    missing.reverse();
    missing
}

/// مفتاح تتبع الروابط الصلبة في نسخ الأرشيف
/// (الميزة 200): جهاز+عقدة على يونكس، ومؤشر
/// الملف على ويندوز، وبلا تتبع خارجهما.
#[cfg(unix)]
type LinkKey = (u64, u64);
#[cfg(windows)]
type LinkKey = u64;
#[cfg(not(any(unix, windows)))]
type LinkKey = ();

/// مفتاح الرابط الصلب لملف عادي — لاشيء إن كان
/// للملف رابط واحد فقط (فلا تكرار يتتبع) أو إن
/// كانت المنصة بلا مفتاح.
fn link_key(meta: &std::fs::Metadata) -> Option<LinkKey> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return Some((meta.dev(), meta.ino()));
        }
        None
    }
    // ويندوز/WASM: بلا هوية ملف مستقرة في راست
    // المستقر (`file_index` غير مستقرة) — تُنسخ
    // الروابط الصلبة نسخا مستقلة في الأرشيف
    // (المحتوى محفوظ، والعلاقة لا) — قيد منصة
    // مسجل، لا ادعاء.
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// أنشئ رابطا رمزيا بالنص المعطى (يونكس وويندوز —
/// وعلى ويندوز يفرّق نوع الهدف كما في `copy_tree`).
fn create_symlink(target: &Path, dst: &Path, target_is_dir: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = target_is_dir;
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(windows)]
    {
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, dst)
        } else {
            std::os::windows::fs::symlink_file(target, dst)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, dst, target_is_dir);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "إنشاء الروابط غير مدعوم على هذه المنصة",
        ))
    }
}

/// احفظ أذونات المصدر (يونكس وحده) ووقتي الوصول
/// والتعديل على نسخة — للمجلد بعد أبنائه حتى لا
/// يرفع إنشاؤهم وقته (الميزة 200). أخطاء الحفظ
/// هنا أفضل جهد (لا تفشل نسخا ناجحا)؛ وفشل فتح
/// المجلد على ويندوز متوقع فيتجاوز.
fn preserve_attrs(meta: &std::fs::Metadata, dst: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = set_mode(dst, meta.permissions().mode());
    }
    let mut times = std::fs::FileTimes::new();
    if let Ok(a) = meta.accessed() {
        times = times.set_accessed(a);
    }
    if let Ok(m) = meta.modified() {
        times = times.set_modified(m);
    }
    let _ = set_file_times_both(dst, times);
}

/// نسخ أرشيف (الميزة 200، الاجتماع 59 §74):
/// تكراري مع حفظ الروابط الرمزية روابط والروابط
/// الصلبة روابط صلبة (خريطة المفتاح إلى أول هدف
/// منسوخ) والأذونات (يونكس) ووقتي الوصول
/// والتعديل للملفات والمجلدات. الملكية وسياق
/// الأمان والسمات الموسعة لا تُحفظ (مسجل في §74).
pub(crate) fn copy_archive(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut links: std::collections::HashMap<LinkKey, std::path::PathBuf> =
        std::collections::HashMap::new();
    copy_archive_inner(src, dst, &mut links)
}

fn copy_archive_inner(
    src: &Path,
    dst: &Path,
    links: &mut std::collections::HashMap<LinkKey, std::path::PathBuf>,
) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(src)?;
        let is_dir = std::fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false);
        return create_symlink(&target, dst, is_dir);
    }
    if meta.is_dir() {
        match std::fs::create_dir(dst) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                match std::fs::metadata(dst) {
                    Ok(m) if m.is_dir() => {
                        let non_empty = std::fs::read_dir(dst)
                            .map(|mut it| it.next().is_some())
                            .unwrap_or(true);
                        if non_empty {
                            return Err(e);
                        }
                    }
                    _ => return Err(e),
                }
            }
            Err(e) => return Err(e),
        }
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_archive_inner(&entry.path(), &dst.join(entry.file_name()), links)?;
        }
        preserve_attrs(&meta, dst);
        return Ok(());
    }
    if let Some(key) = link_key(&meta) {
        if let Some(first) = links.get(&key) {
            std::fs::hard_link(first, dst)?;
            return Ok(());
        }
        std::fs::copy(src, dst)?;
        links.insert(key, dst.to_path_buf());
    } else {
        std::fs::copy(src, dst)?;
    }
    preserve_attrs(&meta, dst);
    Ok(())
}

/// طريقة النسخ الخفيف المرجعة نصا (الميزة 201):
/// مستنسخ أو متناثر أو عادي — فلا ادعاء بلا دليل.
// `Cloned` و`Sparse` تُبنيان على لينكس وحده
// (على ويندوز/WASM الطريقة «عادي» دائما) —
// فالتحذير `dead_code` على تلك المنصات متوقع.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum LightMethod {
    Cloned,
    Sparse,
    Plain,
}

impl LightMethod {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Cloned => "مستنسخ",
            Self::Sparse => "متناثر",
            Self::Plain => "عادي",
        }
    }
}

/// نسخ خفيف (الميزة 201، الاجتماع 59 §74): على
/// لينكس/أندرويد يحاول استنساخ الكتل `FICLONE`
/// ثم نسخا متناثرا بـ`SEEK_HOLE`/`SEEK_DATA` ثم
/// نسخا عاديا؛ وخارجهما نسخ عادي و«عادي».
pub(crate) fn copy_light(src: &Path, dst: &Path) -> std::io::Result<LightMethod> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return copy_light_linux(src, dst);
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        std::fs::copy(src, dst)?;
        Ok(LightMethod::Plain)
    }
}

/// احفظ وضع ملف المصدر على نسخته (يونكس — كما
/// يفعل `std::fs::copy` نفسه).
#[cfg(unix)]
fn preserve_file_mode(dst: &Path, meta: &std::fs::Metadata) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(
        dst,
        std::fs::Permissions::from_mode(meta.permissions().mode()),
    );
}

/// النسخ الخفيف على لينكس/أندرويد (الميزة 201):
/// `FICLONE` أولا (فشله تراجع لا خطأ)، ثم مسح
/// الثقوب، ثم النسخ العادي.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn copy_light_linux(src: &Path, dst: &Path) -> std::io::Result<LightMethod> {
    use std::os::unix::io::AsRawFd;
    let src_file = std::fs::File::open(src)?;
    let src_meta = src_file.metadata()?;
    let src_len = src_meta.len();
    {
        let dst_file = std::fs::File::create(dst)?;
        // الاستنساخ: `ioctl(FICLONE)` عملية O(1)
        // بلا نسخ بيانات حيث يدعمها نظام الملفات.
        let rc = unsafe { libc::ioctl(dst_file.as_raw_fd(), libc::FICLONE, src_file.as_raw_fd()) };
        if rc == 0 {
            preserve_file_mode(dst, &src_meta);
            return Ok(LightMethod::Cloned);
        }
        // فشل الاستنساخ — أغلق الهدف المقطوع
        // وانتقل إلى النسخ المتناثر.
        drop(dst_file);
    }
    match copy_sparse(&src_file, dst, src_len) {
        Ok(sparse) => {
            preserve_file_mode(dst, &src_meta);
            Ok(if sparse { LightMethod::Sparse } else { LightMethod::Plain })
        }
        Err(_) => {
            std::fs::copy(src, dst)?;
            Ok(LightMethod::Plain)
        }
    }
}

/// انسخ ملفا متناثرا بـ`SEEK_DATA`/`SEEK_HOLE`
/// (الميزة 201): الامتدادات الحقيقية فقط تنسخ،
/// والثقوب تبقى ثقوبا في الهدف. يرجع صحيحا إن
/// وجد ثقبا واحدا على الأقل (وإلا فالنسخ عادي
/// الشكل)، وخطأ إن كان نظام الملفات بلا دعم
/// للمسح فيتراجع المتصل إلى النسخ العادي.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn copy_sparse(
    src_file: &std::fs::File,
    dst: &Path,
    src_len: u64,
) -> std::io::Result<bool> {
    use std::os::unix::fs::FileExt;
    use std::os::unix::io::AsRawFd;
    let fd = src_file.as_raw_fd();
    let dst_file = std::fs::File::create(dst)?;
    if src_len == 0 {
        return Ok(false);
    }
    let mut pos: libc::off_t = 0;
    let mut had_hole = false;
    let mut buf = vec![0u8; 65536];
    loop {
        let data = unsafe { libc::lseek(fd, pos, libc::SEEK_DATA) };
        if data < 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ENXIO) {
                // لا بيانات بعد هذا الموضع: الباقي ثقب
                // (أو الملف كله ثقوب) — ينهي النسخ.
                break;
            }
            return Err(e);
        }
        if data > pos {
            had_hole = true;
        }
        let hole = unsafe { libc::lseek(fd, data, libc::SEEK_HOLE) };
        if hole < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut off = data;
        while off < hole {
            let want = std::cmp::min(buf.len() as u64, (hole - off) as u64) as usize;
            let n = src_file.read_at(&mut buf[..want], off as u64)?;
            if n == 0 {
                break;
            }
            dst_file.write_at(&buf[..n], off as u64)?;
            off += n as libc::off_t;
        }
        pos = hole;
        if pos >= src_len as libc::off_t {
            break;
        }
    }
    dst_file.set_len(src_len)?;
    Ok(had_hole || pos < src_len as libc::off_t)
}

/// Open a file: `ملفات.افتح مسار` — returns the file handle.
/// Read+write, creating the file when it does not exist yet.
/// The policy check is the session's own FilePolicy, so the
/// outside-folder error is identical to `اقرأ_ملف`'s.
pub fn open_file(
    policy: &FilePolicy,
    cwd: &Path,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (policy, cwd, path);
        Err(RioError::runtime(
            "هذه الدالة غير مدعومة في متصفح الويب",
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        open_file_native(policy, cwd, path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_file_native(
    policy: &FilePolicy,
    cwd: &Path,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<u64> {
    let resolved = policy.resolve_in(cwd, path, line, col)?;
    let display = policy.display_str(path);
    // A directory is not a file (same wording as `احذف_ملف`).
    if let Ok(m) = std::fs::metadata(&resolved) {
        if m.is_dir() {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("ليس ملفا: {}", display),
                line,
                col,
            ));
        }
    }
    // Meeting 50 §64: under the downloads root the file
    // opens read-only (no write, no create) — the handle
    // remembers it, so `ملفات.اكتب` refuses in Arabic.
    let read_only = policy.is_under_downloads(&resolved);
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    if !read_only {
        opts.write(true).create(true);
    }
    let file = opts.open(&resolved).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر فتح الملف {}", display),
            line,
            col,
        )
    })?;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    match REGISTRY.lock() {
        Ok(mut reg) => {
            reg.insert(id, OpenFile { file, display, read_only });
        }
        Err(_) => {
            return Err(RioError::runtime("تعذّر تسجيل الملف الجديد", line, col));
        }
    }
    Ok(id)
}

/// Read a whole open file: `ملفات.اقرأ معرف` — returns the
/// content as text. Seeks to the start first, so a repeated
/// read returns the same content. Same 5 MiB cap and same
/// messages as `اقرأ_ملف`.
pub fn read_file(id: i64, line: usize, col: usize) -> RioResult<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = id;
        Err(RioError::runtime(
            "هذه الدالة غير مدعومة في متصفح الويب",
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        read_file_native(id, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file_native(id: i64, line: usize, col: usize) -> RioResult<String> {
    if id < 1 {
        return Err(RioError::runtime(
            format!("ملف غير معروف: {}", id),
            line,
            col,
        ));
    }
    let cap = crate::interpreter::Interpreter::MAX_READ_BYTES;
    let mut guard = REGISTRY.lock().map_err(|_| {
        RioError::runtime(format!("ملف غير معروف: {}", id), line, col)
    })?;
    let entry = guard.get_mut(&(id as u64)).ok_or_else(|| {
        RioError::runtime(format!("ملف غير معروف: {}", id), line, col)
    })?;
    let display = entry.display.clone();
    entry.file.seek(SeekFrom::Start(0)).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر قراءة الملف {}", display),
            line,
            col,
        )
    })?;
    let mut buf = Vec::new();
    (&mut entry.file)
        .take((cap + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|_| {
            RioError::new(
                ErrorKind::Io,
                format!("تعذر قراءة الملف {}", display),
                line,
                col,
            )
        })?;
    if buf.len() > cap {
        return Err(RioError::new(
            ErrorKind::Io,
            format!("الملف {} أكبر من الحد الأقصى (5 ميغابايت)", display),
            line,
            col,
        ));
    }
    String::from_utf8(buf).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("الملف {} ليس نصا صالحا (UTF-8)", display),
            line,
            col,
        )
    })
}

/// Write a whole open file: `ملفات.اكتب معرف نص` — replaces
/// the content (seek to start, truncate, write all). The net
/// growth counts against the same 100 MiB program-dir quota
/// as `اكتب_ملف`, with the same message.
pub fn write_file(
    policy: &FilePolicy,
    id: i64,
    content: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (policy, id, content);
        Err(RioError::runtime(
            "هذه الدالة غير مدعومة في متصفح الويب",
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        write_file_native(policy, id, content, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file_native(
    policy: &FilePolicy,
    id: i64,
    content: &str,
    line: usize,
    col: usize,
) -> RioResult<()> {
    if id < 1 {
        return Err(RioError::runtime(
            format!("ملف غير معروف: {}", id),
            line,
            col,
        ));
    }
    let mut guard = REGISTRY.lock().map_err(|_| {
        RioError::runtime(format!("ملف غير معروف: {}", id), line, col)
    })?;
    let entry = guard.get_mut(&(id as u64)).ok_or_else(|| {
        RioError::runtime(format!("ملف غير معروف: {}", id), line, col)
    })?;
    let display = entry.display.clone();
    // Meeting 50 §64: a handle opened under the downloads
    // root is read-only — refuse before any seek or write.
    if entry.read_only {
        return Err(RioError::new(
            ErrorKind::Io,
            format!("الملف {} في تخزين/تنزيلات للقراءة فقط", display),
            line,
            col,
        ));
    }
    // Net growth only (rewriting counts its size difference),
    // against the same 100 MiB quota as `اكتب_ملف`. The
    // quota check and the write are atomic under the shared
    // file-write lock (task 19): an async write worker can
    // no longer slip between this check and this write.
    let _write_guard = FILE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let existing = entry.file.metadata().map(|m| m.len()).unwrap_or(0);
    let extra = (content.len() as u64).saturating_sub(existing);
    check_disk_quota(&policy.base, extra, line, col)?;
    entry.file.seek(SeekFrom::Start(0)).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر كتابة الملف {}", display),
            line,
            col,
        )
    })?;
    entry.file.set_len(0).map_err(|_| {
        RioError::new(
            ErrorKind::Io,
            format!("تعذر كتابة الملف {}", display),
            line,
            col,
        )
    })?;
    entry
        .file
        .write_all(content.as_bytes())
        .map_err(|_| {
            RioError::new(
                ErrorKind::Io,
                format!("تعذر كتابة الملف {}", display),
                line,
                col,
            )
        })?;
    Ok(())
}

// ===== عمليات SAF — ملفات في شجرة ممنوحة (المهمة 28، CATALOG-19) =====
// التسع دوال في وحدة `ملفات` القائمة — لا وحدة جديدة.
// المبدأ الحاكم: التطبيق ينفذ والمحرك يطلب فقط — شجرة
// SAF عناوين `content://` لا يفتحها `std::fs` إطلاقا،
// فكل دالة ترسل طلبا واحدا عبر قناة التحكم وتنتظر
// النتيجة عبر `wait_for_result` المشترك (نمط قدرات
// `جهاز` حرفيا). البادئة واحدة لكل المسار
// (`ملف-ممنوح:`) والتفريع على حقل `عملية` في
// التطبيق. ترتيب الفحوص: وسائط (عدد ونوع) ← منصة
// (WASM ← رسالة المتصفح؛ غير أندرويد ← رفض باسم
// الدالة) ← قناة (أندرويد بلا قناة ← «الملفات
// الممنوحة متاحة داخل التطبيق فقط») ← إرسال وانتظار.
// المسارات نسبية إلى جذر الشجرة بفاصل `/`، والمسار
// الفارغ هو الجذر نفسه؛ وفحص `..` المعجمي في
// التطبيق قبل أي وصول (CATALOG-19 §2). قراءة SAF
// نص UTF-8 بسقف 5 ميغابايت (يفرضه التطبيق)؛ وحصة
// القرص 100 ميغابايت لا تسري على كتابة SAF (§1).
// العمليات الصامتة (كتابة/إنشاء/حذف) ترجع `صحيح`
// لا null — لأن `json_to_value` يرفض null (§2).

/// بادئة طلبات SAF على stderr (قناة التحكم —
/// أندرويد فقط؛ على غيرها لا يرسل المحرك شيئا).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SAF_CONTROL_PREFIX: &str = "\u{0001}نيفا-تحكم:ملف-ممنوح:";

/// رفض أندرويد بلا قناة تحكم (CATALOG-19 §2 —
/// أندرويد فقط).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const SAF_OUTSIDE_APP: &str = "الملفات الممنوحة متاحة داخل التطبيق فقط";

/// خطأ نوع ببادئة اسم الدالة المنقّط (نمط `جهاز`).
fn saf_typ_error(display: &str, what: &str, line: usize, col: usize) -> RioError {
    RioError::typ(format!("الدالة {} {}", display, what), line, col)
}

/// عدد المعاملات (نمط `need_args` في stdlib).
fn saf_need_args(
    display: &str,
    args: &[Value],
    expected: usize,
    line: usize,
    col: usize,
) -> RioResult<()> {
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

/// معرف شجرة: عدد صحيح موجب (1 فأكثر، دون 9e15 —
/// نمط معرف التنبيه في `جهاز`).
fn saf_need_tree_id(display: &str, v: &Value, line: usize, col: usize) -> RioResult<u64> {
    match v {
        Value::Number(x) if x.fract() == 0.0 && *x >= 1.0 && x.abs() < 9.0e15 => Ok(*x as u64),
        Value::Number(_) => Err(RioError::typ(
            format!(
                "الدالة {} تتوقع عددا صحيحا موجبا لمعرف الشجرة",
                display
            ),
            line,
            col,
        )),
        other => Err(saf_typ_error(
            display,
            &format!("تتوقع عددا لمعرف الشجرة لكن أعطيت {}", other.type_name()),
            line,
            col,
        )),
    }
}

/// مسار نصي (فارغ = جذر الشجرة — CATALOG-19 §2).
fn saf_need_path(display: &str, v: &Value, line: usize, col: usize) -> RioResult<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(saf_typ_error(
            display,
            &format!("تتوقع نصا للمسار لكن أعطيت {}", other.type_name()),
            line,
            col,
        )),
    }
}

/// نص المحتوى (فارغ مسموح — كتابة محتوى فارغ).
fn saf_need_text(display: &str, v: &Value, line: usize, col: usize) -> RioResult<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(saf_typ_error(
            display,
            &format!("تتوقع نصا للنص لكن أعطيت {}", other.type_name()),
            line,
            col,
        )),
    }
}

/// تسلسل طلب بلا وسائط (تهريب يدوي — `json_escape` المشترك).
/// يبنى على أندرويد فقط، ويبقى في اختبارات الوحدة.
#[cfg(any(all(target_os = "android", not(target_arch = "wasm32")), test))]
fn saf_json_no_args(op: &str) -> String {
    format!("{{\"عملية\":{}}}", crate::device::json_escape(op))
}

/// تسلسل طلب بمعرف شجرة ومسار.
#[cfg(any(all(target_os = "android", not(target_arch = "wasm32")), test))]
fn saf_json_tree_path(op: &str, id: u64, path: &str) -> String {
    format!(
        "{{\"عملية\":{},\"معرف\":{},\"مسار\":{}}}",
        crate::device::json_escape(op),
        id,
        crate::device::json_escape(path)
    )
}

/// تسلسل طلب كتابة (معرف ومسار ونص).
#[cfg(any(all(target_os = "android", not(target_arch = "wasm32")), test))]
fn saf_json_write(id: u64, path: &str, text: &str) -> String {
    format!(
        "{{\"عملية\":{},\"معرف\":{},\"مسار\":{},\"نص\":{}}}",
        crate::device::json_escape("اكتب_ممنوح"),
        id,
        crate::device::json_escape(path),
        crate::device::json_escape(text)
    )
}

/// إرسال طلب SAF واحد وانتظار نتيجته (أندرويد فقط —
/// الانتظار القابل للإلغاء أدناه).
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
fn saf_request(json: &str, line: usize, col: usize) -> RioResult<Value> {
    eprintln!("{}{}", SAF_CONTROL_PREFIX, json);
    saf_wait_for_result(line, col)
}

/// انتظار نتيجة SAF — كـ`wait_for_result` المشترك
/// لكن سطر الإلغاء يجهض الانتظار («أُلغي الأمر»)
/// بدلا من تجاهله: قراءة SAF جارية قابلة للإلغاء
/// بزر «ألغِ الأمر» (CATALOG-19 §9) كما الجلب
/// (مهمة 22)؛ أما الحوار فيبقى غير قابل للإلغاء
/// عمدا (قرار مهمة 22 المسجل في `wait_for_result`).
/// والأسطر الغريبة تُتخطى كما في المشترك. يبقى في
/// اختبارات الوحدة لاختبار الإلغاء بالحقن.
#[cfg(any(all(target_os = "android", not(target_arch = "wasm32")), test))]
fn saf_wait_for_result(line: usize, col: usize) -> RioResult<Value> {
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
        if crate::stdin_pump::is_cancel_line(text) {
            return Err(RioError::runtime("أُلغي الأمر", line, col));
        }
        if let Some(payload) = text.strip_prefix(crate::device::RESULT_PREFIX) {
            return crate::device::parse_control_result(payload, line, col);
        }
        // سطر آخر أثناء الانتظار — يُتجاهل (كما المشترك).
    }
}

/// `ملفات.امنح_مجلد()` — يفتح منتقي النظام ويعيد
/// معرف الشجرة (عدد صحيح من 1).
pub fn saf_grant_tree(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    saf_need_args("ملفات.امنح_مجلد", args, 0, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_grant_tree_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_grant_tree_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "ملفات.امنح_مجلد غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_no_args("امنح_مجلد"), line, col)? {
                v @ Value::Number(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.سرد_ممنوح(معرف، مسار)` — قائمة أسماء.
pub fn saf_list_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.سرد_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_list_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_list_granted_native(id: u64, path: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.سرد_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_tree_path("سرد_ممنوح", id, path), line, col)? {
                v @ Value::List(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.اقرأ_ممنوح(معرف، مسار)` — نص الملف.
pub fn saf_read_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.اقرأ_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_read_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_read_granted_native(id: u64, path: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.اقرأ_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_tree_path("اقرأ_ممنوح", id, path), line, col)? {
                v @ Value::Str(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.اكتب_ممنوح(معرف، مسار، نص)` — استبدال
/// كامل للمحتوى، وترجع `صحيح` عند النجاح.
pub fn saf_write_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.اكتب_ممنوح";
    saf_need_args(DISPLAY, args, 3, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    let text = saf_need_text(DISPLAY, &args[2], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path, text);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_write_granted_native(id, &path, &text, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_write_granted_native(
    id: u64,
    path: &str,
    text: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path, text);
        Err(RioError::runtime(
            "ملفات.اكتب_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_write(id, path, text), line, col)? {
                v @ Value::Bool(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.أنشئ_ملف_ممنوح(معرف، مسار)` — ينشئ ملفا
/// نصيا فارغا، ويرجع `صحيح` عند النجاح.
pub fn saf_create_file_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.أنشئ_ملف_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_create_file_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_create_file_granted_native(
    id: u64,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.أنشئ_ملف_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(
                &saf_json_tree_path("أنشئ_ملف_ممنوح", id, path),
                line,
                col,
            )? {
                v @ Value::Bool(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.أنشئ_مجلد_ممنوح(معرف، مسار)` — ينشئ
/// مجلدا، ويرجع `صحيح` عند النجاح.
pub fn saf_create_dir_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.أنشئ_مجلد_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_create_dir_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_create_dir_granted_native(
    id: u64,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.أنشئ_مجلد_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(
                &saf_json_tree_path("أنشئ_مجلد_ممنوح", id, path),
                line,
                col,
            )? {
                v @ Value::Bool(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.تفاصيل_ممنوح(معرف، مسار)` — قاموس
/// {نوع، حجم، تاريخ} (التاريخ نص UTC).
pub fn saf_stat_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.تفاصيل_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_stat_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_stat_granted_native(id: u64, path: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.تفاصيل_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_tree_path("تفاصيل_ممنوح", id, path), line, col)? {
                v @ Value::Dict(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.احذف_ممنوح(معرف، مسار)` — يحذف ملفا
/// (ومجلدا فارغا) بلا حوار — البرنامج هو الطالب —
/// ويرجع `صحيح` عند النجاح.
pub fn saf_delete_granted(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    const DISPLAY: &str = "ملفات.احذف_ممنوح";
    saf_need_args(DISPLAY, args, 2, line, col)?;
    let id = saf_need_tree_id(DISPLAY, &args[0], line, col)?;
    let path = saf_need_path(DISPLAY, &args[1], line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_delete_granted_native(id, &path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_delete_granted_native(id: u64, path: &str, line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, path);
        Err(RioError::runtime(
            "ملفات.احذف_ممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_tree_path("احذف_ممنوح", id, path), line, col)? {
                v @ Value::Bool(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

/// `ملفات.الممنوح()` — قائمة قواميس {معرف، اسم}
/// للشجرات الممنوحة الحية.
pub fn saf_list_trees(args: &[Value], line: usize, col: usize) -> RioResult<Value> {
    saf_need_args("ملفات.الممنوح", args, 0, line, col)?;
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(
            crate::device::WASM_REFUSAL,
            line,
            col,
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        saf_list_trees_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn saf_list_trees_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(not(target_os = "android"))]
    {
        Err(RioError::runtime(
            "ملفات.الممنوح غير مدعوم على هذه المنصة",
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
            match saf_request(&saf_json_no_args("الممنوح"), line, col)? {
                v @ Value::List(_) => Ok(v),
                _ => Err(RioError::runtime("نتيجة تحكم غير صالحة", line, col)),
            }
        } else {
            Err(RioError::runtime(SAF_OUTSIDE_APP, line, col))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niva-files-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn restricted(dir: &Path) -> (FilePolicy, PathBuf) {
        (FilePolicy::restricted(dir.to_path_buf()), dir.to_path_buf())
    }

    // الميزة 204: مسار نسخ-ثم-حذف عبر جهازين حقيقيين
    // (/tmp ذاكرة مؤقتة tmpfs مقابل /dev/shm ذاكرة
    // مؤقتة أخرى — جهازان مختلفان على لينكس).
    #[test]
    #[cfg(target_os = "linux")]
    fn move_fallback_cross_device_file() {
        let _guard = file_test_lock();
        let dir = test_dir("xdev-file");
        let src = dir.join("أ.txt");
        std::fs::write(&src, "محتوى مهم").unwrap();
        let other = PathBuf::from("/dev/shm").join(format!("niva-xdev-{}-file", std::process::id()));
        let _ = std::fs::remove_dir_all(&other);
        std::fs::create_dir_all(&other).unwrap();
        let dst = other.join("ب.txt");
        // إثبات أن الجهازين مختلفان فعلا: إعادة
        // التسمية وحدها تفشل باختلاف الجهاز.
        let r = std::fs::rename(&src, &dst);
        assert!(r.is_err(), "expected EXDEV between /tmp and /dev/shm");
        assert!(is_cross_device_error(&r.unwrap_err()));
        assert!(move_fallback(&src, &dst).is_ok());
        assert!(!src.exists(), "source must be gone after move");
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "محتوى مهم");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn move_fallback_cross_device_dir() {
        let _guard = file_test_lock();
        let dir = test_dir("xdev-dir");
        let src = dir.join("مجلد");
        std::fs::create_dir_all(src.join("داخلي")).unwrap();
        std::fs::write(src.join("داخلي/أ.txt"), "أ").unwrap();
        std::fs::write(src.join("ب.txt"), "ب").unwrap();
        std::os::unix::fs::symlink("ب.txt", src.join("رابط.txt")).unwrap();
        let other = PathBuf::from("/dev/shm").join(format!("niva-xdev-{}-dir", std::process::id()));
        let _ = std::fs::remove_dir_all(&other);
        std::fs::create_dir_all(&other).unwrap();
        let dst = other.join("منقول");
        let r = std::fs::rename(&src, &dst);
        assert!(r.is_err());
        assert!(is_cross_device_error(&r.unwrap_err()));
        assert!(move_fallback(&src, &dst).is_ok());
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(dst.join("داخلي/أ.txt")).unwrap(), "أ");
        assert_eq!(std::fs::read_to_string(dst.join("ب.txt")).unwrap(), "ب");
        // الرابط يعاد رابطا بلا اتباع هدفه.
        let link = dst.join("رابط.txt");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_link(&link).unwrap().to_str().unwrap(),
            "ب.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn move_fallback_copy_failure_keeps_source() {
        let _guard = file_test_lock();
        let dir = test_dir("xdev-fail");
        let src = dir.join("أ.txt");
        std::fs::write(&src, "محتوى مهم").unwrap();
        // الهدف تحت ملف لا مجلد: النسخ يستحيل.
        let blocker = dir.join("حاجز");
        std::fs::write(&blocker, "x").unwrap();
        let dst = blocker.join("ب.txt");
        let res = move_fallback(&src, &dst);
        assert!(matches!(res, Err(MoveError::Copy(_))));
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "محتوى مهم");
        assert!(!dst.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dst_inside_src_cases() {
        let _guard = file_test_lock();
        let dir = test_dir("inside-src");
        let src = dir.join("مجلد");
        std::fs::create_dir_all(&src).unwrap();
        assert!(dst_inside_src(&src, &src));
        assert!(dst_inside_src(&src, &src.join("فرعي")));
        assert!(dst_inside_src(&src, &dir.join("مجلد/../مجلد/فرعي")));
        assert!(dst_inside_src(&src, &dir.join("./مجلد")));
        assert!(!dst_inside_src(&src, &dir.join("آخر")));
        assert!(!dst_inside_src(&src, &dir));
        assert!(!dst_inside_src(&src, &dir.join("مجلد٢")));
        // مصدر غائب: لا تطابق.
        assert!(!dst_inside_src(&dir.join("غائب"), &dir.join("غائب/فرعي")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_write_replace_roundtrip() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("roundtrip");
        let (policy, cwd) = restricted(&dir);
        let id = open_file(&policy, &cwd, "أ.txt", 1, 1).unwrap();
        assert!(id > 0);
        assert_eq!(test_open_count(), 1);
        write_file(&policy, id as i64, "مرحبا", 1, 1).unwrap();
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "مرحبا");
        // A second write REPLACES (never appends).
        write_file(&policy, id as i64, "جديد", 1, 1).unwrap();
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "جديد");
        // Repeated reads are identical.
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "جديد");
        // Empty write empties the file.
        write_file(&policy, id as i64, "", 1, 1).unwrap();
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_creates_missing_file() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("create");
        let (policy, cwd) = restricted(&dir);
        assert!(!dir.join("جديد.txt").exists());
        let id = open_file(&policy, &cwd, "جديد.txt", 1, 1).unwrap();
        assert!(dir.join("جديد.txt").exists());
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_outside_rejected_with_policy_error() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("outside");
        let (policy, cwd) = restricted(&dir);
        for bad in ["../خارج.txt", "فرعي/../../خارج.txt", "/etc/passwd"] {
            let err = open_file(&policy, &cwd, bad, 1, 1)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("خارج مجلد البرنامج"),
                "path {}: got: {}",
                bad,
                err
            );
        }
        assert_eq!(test_open_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_dir_rejected() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("dir");
        let (policy, cwd) = restricted(&dir);
        let err = open_file(&policy, &cwd, ".", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ليس ملفا"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_handle_is_arabic() {
        let _guard = file_test_lock();
        test_clear();
        for bad in [0, -1, 999_999] {
            let err = read_file(bad, 1, 1).unwrap_err().to_string();
            assert!(err.contains("ملف غير معروف"), "read {}: got: {}", bad, err);
            let dir = test_dir("unknown");
            let (policy, _) = restricted(&dir);
            let err = write_file(&policy, bad, "نص", 1, 1)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("ملف غير معروف"),
                "write {}: got: {}",
                bad,
                err
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn handles_are_unique() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("unique");
        let (policy, cwd) = restricted(&dir);
        let a = open_file(&policy, &cwd, "أ.txt", 1, 1).unwrap();
        let b = open_file(&policy, &cwd, "ب.txt", 1, 1).unwrap();
        assert_ne!(a, b);
        assert_eq!(test_open_count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_cap_is_five_megabytes() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("cap");
        let (policy, cwd) = restricted(&dir);
        let cap = crate::interpreter::Interpreter::MAX_READ_BYTES;
        let big = vec![b'x'; cap + 1];
        std::fs::write(dir.join("كبير.txt"), &big).unwrap();
        let id = open_file(&policy, &cwd, "كبير.txt", 1, 1).unwrap();
        let err = read_file(id as i64, 1, 1).unwrap_err().to_string();
        assert!(err.contains("أكبر من الحد الأقصى (5 ميغابايت)"), "got: {}", err);
        // Exactly at the cap reads fine.
        let ok = vec![b'x'; cap];
        std::fs::write(dir.join("تمام.txt"), &ok).unwrap();
        let id2 = open_file(&policy, &cwd, "تمام.txt", 1, 1).unwrap();
        assert_eq!(read_file(id2 as i64, 1, 1).unwrap().len(), cap);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_invalid_utf8_is_arabic() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("utf8");
        let (policy, cwd) = restricted(&dir);
        std::fs::write(dir.join("ثنائي.bin"), [0xFFu8, 0xFE, 0x41]).unwrap();
        let id = open_file(&policy, &cwd, "ثنائي.bin", 1, 1).unwrap();
        let err = read_file(id as i64, 1, 1).unwrap_err().to_string();
        assert!(err.contains("ليس نصا صالحا"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trusted_policy_allows_outside() {
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("trusted");
        let outside = std::env::temp_dir().join(format!(
            "niva-files-outside-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&outside);
        let policy = FilePolicy::trusted(dir.clone());
        let id = open_file(&policy, &dir, outside.to_str().unwrap(), 1, 1).unwrap();
        write_file(&policy, id as i64, "خارج", 1, 1).unwrap();
        assert_eq!(read_file(id as i64, 1, 1).unwrap(), "خارج");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_quota_refuses_when_program_dir_full() {
        // A sparse file stands in for 100 MB of content
        // (metadata length counts, no real disk is used) —
        // same trick as the `اكتب_ملف` quota test.
        let _guard = file_test_lock();
        test_clear();
        let dir = test_dir("quota");
        let (policy, cwd) = restricted(&dir);
        let f = std::fs::File::create(dir.join("كبير.bin")).unwrap();
        f.set_len(crate::interpreter::Interpreter::DISK_QUOTA_BYTES + 1)
            .unwrap();
        drop(f);
        let id = open_file(&policy, &cwd, "جديد.txt", 1, 1).unwrap();
        let err = write_file(&policy, id as i64, "نص", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("حصة التخزين"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wasm_refusal() {
        // WASM: all three refuse honestly — proven by
        // compilation on the host, executed only on wasm32.
        #[cfg(target_arch = "wasm32")]
        {
            let _guard = file_test_lock();
            let dir = std::env::temp_dir();
            let policy = FilePolicy::restricted(dir.clone());
            let err = open_file(&policy, &dir, "أ.txt", 1, 1)
                .unwrap_err()
                .to_string();
            assert!(err.contains("غير مدعومة في متصفح الويب"), "got: {}", err);
            let err = read_file(1, 1, 1).unwrap_err().to_string();
            assert!(err.contains("غير مدعومة في متصفح الويب"), "got: {}", err);
            let err = write_file(&policy, 1, "نص", 1, 1)
                .unwrap_err()
                .to_string();
            assert!(err.contains("غير مدعومة في متصفح الويب"), "got: {}", err);
        }
    }

    // ===== عمليات SAF (المهمة 28) =====
    // الفحص قبل المنصة: الوسائط الخاطئة ترفض
    // برسالتها قبل رفض المنصة (ترتيب CATALOG-19 §2).

    fn saf_num(x: f64) -> Value {
        Value::Number(x)
    }

    fn saf_str(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    #[test]
    fn saf_arg_validation_before_platform() {
        // كل دالة: عدد معاملات خاطئ ← رسالة العدد؛
        // نوع خاطئ ← رسالة النوع؛ معرف غير موجب ←
        // رسالة العدد الصحيح الموجب. كلها قبل أي
        // فحص منصة (على لينكس هنا: لو وصلنا إلى
        // رفض المنصة لكان الترتيب مكسورا).
        let err = saf_grant_tree(&[saf_num(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);

        let err = saf_list_trees(&[saf_num(1.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 0 معاملات"), "got: {}", err);

        for bad_id in [saf_num(0.0), saf_num(-1.0), saf_num(1.5), saf_str("١")] {
            for f in [
                saf_list_granted as fn(&[Value], usize, usize) -> RioResult<Value>,
                saf_read_granted,
                saf_create_file_granted,
                saf_create_dir_granted,
                saf_stat_granted,
                saf_delete_granted,
            ] {
                let err = f(&[bad_id.clone(), saf_str("")], 1, 1)
                    .unwrap_err()
                    .to_string();
                assert!(
                    err.contains("معرف الشجرة"),
                    "bad id {:?}: got: {}",
                    bad_id,
                    err
                );
            }
        }
        // مسار غير نصي.
        let err = saf_list_granted(&[saf_num(1.0), saf_num(2.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للمسار"), "got: {}", err);
        // نص غير نصي في الكتابة.
        let err = saf_write_granted(&[saf_num(1.0), saf_str("أ"), saf_num(3.0)], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع نصا للنص"), "got: {}", err);
        // الكتابة بمعاملين فقط.
        let err = saf_write_granted(&[saf_num(1.0), saf_str("أ")], 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تتوقع 3 معاملات"), "got: {}", err);
    }

    #[test]
    fn saf_platform_refusal_names_function() {
        // وسائط صحيحة على لينكس (غير أندرويد): رفض
        // المنصة باسم الدالة حرفيا — لكل دالة من التسع.
        let cases: [(&str, RioResult<Value>); 9] = [
            ("امنح_مجلد", saf_grant_tree(&[], 1, 1)),
            ("سرد_ممنوح", saf_list_granted(&[saf_num(1.0), saf_str("")], 1, 1)),
            ("اقرأ_ممنوح", saf_read_granted(&[saf_num(1.0), saf_str("أ")], 1, 1)),
            (
                "اكتب_ممنوح",
                saf_write_granted(&[saf_num(1.0), saf_str("أ"), saf_str("نص")], 1, 1),
            ),
            (
                "أنشئ_ملف_ممنوح",
                saf_create_file_granted(&[saf_num(1.0), saf_str("أ")], 1, 1),
            ),
            (
                "أنشئ_مجلد_ممنوح",
                saf_create_dir_granted(&[saf_num(1.0), saf_str("أ")], 1, 1),
            ),
            (
                "تفاصيل_ممنوح",
                saf_stat_granted(&[saf_num(1.0), saf_str("أ")], 1, 1),
            ),
            (
                "احذف_ممنوح",
                saf_delete_granted(&[saf_num(1.0), saf_str("أ")], 1, 1),
            ),
            ("الممنوح", saf_list_trees(&[], 1, 1)),
        ];
        for (name, result) in cases {
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains(&format!("ملفات.{} غير مدعوم على هذه المنصة", name)),
                "{}: got: {}",
                name,
                err
            );
        }
    }

    #[test]
    fn saf_wait_cancel_aborts_and_result_returns() {
        // انتظار SAF قابل للإلغاء (CATALOG-19 §9):
        // سطر الإلغاء يجهضه بـ«أُلغي الأمر»، وسطر
        // النتيجة يعيد القيمة، والسطر الغريب يُتخطى.
        let _guard = crate::stdin_pump::pump_test_lock();
        crate::stdin_pump::inject_stdin_line("\u{0001}نيفا-إلغاء\n".to_string());
        let err = saf_wait_for_result(1, 1).unwrap_err().to_string();
        assert!(err.contains("أُلغي الأمر"), "got: {}", err);
        crate::stdin_pump::inject_stdin_line(
            "\u{0001}نيفا-نتيجة:{\"قيمة\": 3}\n".to_string(),
        );
        match saf_wait_for_result(1, 1).unwrap() {
            Value::Number(n) => assert_eq!(n, 3.0),
            _ => panic!("unexpected value"),
        }
        crate::stdin_pump::inject_stdin_line("سطر غريب\n".to_string());
        crate::stdin_pump::inject_stdin_line(
            "\u{0001}نيفا-نتيجة:{\"قيمة\": true}\n".to_string(),
        );
        match saf_wait_for_result(1, 1).unwrap() {
            Value::Bool(true) => {}
            _ => panic!("unexpected value"),
        }
    }

    #[test]
    fn saf_request_json_serialization() {
        assert_eq!(
            saf_json_no_args("امنح_مجلد"),
            "{\"عملية\":\"امنح_مجلد\"}"
        );
        assert_eq!(
            saf_json_no_args("الممنوح"),
            "{\"عملية\":\"الممنوح\"}"
        );
        assert_eq!(
            saf_json_tree_path("سرد_ممنوح", 3, "مجلد/ملف"),
            "{\"عملية\":\"سرد_ممنوح\",\"معرف\":3,\"مسار\":\"مجلد/ملف\"}"
        );
        assert_eq!(
            saf_json_tree_path("اقرأ_ممنوح", 1, ""),
            "{\"عملية\":\"اقرأ_ممنوح\",\"معرف\":1,\"مسار\":\"\"}"
        );
        assert_eq!(
            saf_json_write(2, "أ.txt", "نص\nجديد"),
            "{\"عملية\":\"اكتب_ممنوح\",\"معرف\":2,\"مسار\":\"أ.txt\",\"نص\":\"نص\\nجديد\"}"
        );
        // علامة تنصيص داخل المسار تُهرب ولا تكسر JSON.
        let json = saf_json_tree_path("تفاصيل_ممنوح", 1, "م\"لف");
        assert!(json.contains("م\\\"لف"), "got: {}", json);
        assert!(json.starts_with("{\"عملية\":\"تفاصيل_ممنوح\""), "got: {}", json);
    }

    // الدفعة 3 — التشغيل (اجتماع 57 §72، الميزة 39):
    // تعريف الملف التنفيذي — موجود + ملف عادي +
    // مقروء؛ والمجلد وغير المقروء يُتخطيان إلى المكان
    // التالي، وأول عائق يحدد النتيجة حين لا تنفيذي.
    fn batch3_tool_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "niva-tool-lookup-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(dir.join("home")).unwrap();
        dir
    }

    #[test]
    fn batch3_tool_lookup_found_and_home_fallback() {
        let dir = batch3_tool_dir();
        std::fs::write(dir.join("أداة.نيفا"), "اطبع \"أداة\"\n").unwrap();
        std::fs::write(dir.join("home").join("بيت.نيفا"), "اطبع \"بيت\"\n").unwrap();
        // المثبَّت يفوز، والبيت بديله.
        match find_tool_file(&dir, "أداة.نيفا") {
            ToolLookup::Found { path, in_home } => {
                assert!(!in_home);
                assert_eq!(path, dir.join("أداة.نيفا"));
            }
            other => panic!("expected Found, got {:?}", other),
        }
        match find_tool_file(&dir, "بيت.نيفا") {
            ToolLookup::Found { path, in_home } => {
                assert!(in_home);
                assert_eq!(path, dir.join("home").join("بيت.نيفا"));
            }
            other => panic!("expected Found in home, got {:?}", other),
        }
        // الغائب غائب.
        assert!(matches!(
            find_tool_file(&dir, "غائب.نيفا"),
            ToolLookup::Missing
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch3_tool_lookup_dir_skipped_to_next() {
        let dir = batch3_tool_dir();
        // مجلد في `$نيفا` يحجب ملفا في البيت في الكود
        // القديم — هنا يُتخطى ويعمل ملف البيت.
        std::fs::create_dir_all(dir.join("أداة.نيفا")).unwrap();
        std::fs::write(dir.join("home").join("أداة.نيفا"), "اطبع \"بيت\"\n").unwrap();
        match find_tool_file(&dir, "أداة.نيفا") {
            ToolLookup::Found { path, in_home } => {
                assert!(in_home);
                assert_eq!(path, dir.join("home").join("أداة.نيفا"));
            }
            other => panic!("expected Found in home, got {:?}", other),
        }
        // مجلد في المكانين معا → عائق مجلد (126).
        std::fs::create_dir_all(dir.join("home").join("مجلد.نيفا")).unwrap();
        std::fs::create_dir_all(dir.join("مجلد.نيفا")).unwrap();
        assert!(matches!(
            find_tool_file(&dir, "مجلد.نيفا"),
            ToolLookup::IsDir
        ));
        // مجلد وحده بلا بديل → عائق مجلد أيضا.
        assert!(matches!(
            find_tool_file(&dir, "مجلد.نيفا"),
            ToolLookup::IsDir
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn batch3_tool_lookup_unreadable_skipped_to_next() {
        use std::os::unix::fs::PermissionsExt;
        let dir = batch3_tool_dir();
        // غير مقروء في `$نيفا` + مقروء في البيت →
        // يعمل ملف البيت.
        let blocked = dir.join("مقفل.نيفا");
        std::fs::write(&blocked, "اطبع \"محجوب\"\n").unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000))
            .unwrap();
        std::fs::write(dir.join("home").join("مقفل.نيفا"), "اطبع \"بيت\"\n").unwrap();
        match find_tool_file(&dir, "مقفل.نيفا") {
            ToolLookup::Found { in_home, .. } => assert!(in_home),
            other => panic!("expected Found in home, got {:?}", other),
        }
        // غير مقروء في المكانين معا → عائق قراءة (126).
        let blocked_home = dir.join("home").join("مقفل.نيفا");
        std::fs::set_permissions(&blocked_home, std::fs::Permissions::from_mode(0o000))
            .unwrap();
        assert!(matches!(
            find_tool_file(&dir, "مقفل.نيفا"),
            ToolLookup::Unreadable
        ));
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644))
            .unwrap();
        std::fs::set_permissions(
            &blocked_home,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
