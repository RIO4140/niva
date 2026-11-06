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
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// One open file: the handle itself plus the display form of
/// the path it was opened with (for error messages only — the
/// resolved filesystem path is never shown to Niva code).
struct OpenFile {
    file: File,
    display: String,
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
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&resolved)
        .map_err(|_| {
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
            reg.insert(id, OpenFile { file, display });
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
}
