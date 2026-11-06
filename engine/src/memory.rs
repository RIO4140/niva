//! Memory allocation for the `ذاكرة` native module
//! (catalog task 11, from `mm/`): reserve a block, free it,
//! free memory now, and the page size. One implementation,
//! shared by both engines (interpreter + VM) through the stdlib
//! native functions — never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-02-MEMORY.md` §1): no raw pointer ever reaches
//! Niva code — `احجز` returns a numeric handle only; a
//! single allocation is capped at 64 MiB and all live blocks
//! at 256 MiB in total (a Rust allocation failure aborts the
//! process, so the caps are what keep a runaway loop from
//! eating the machine); blocks are zeroed; freeing consumes
//! the handle; `المتاح` reads MemAvailable from
//! `/proc/meminfo` and `حجم_الصفحة` reads AT_PAGESZ from
//! `/proc/self/auxv` on Linux (+Android, a Linux kernel
//! underneath); on macOS/Windows/WASM the two queries are
//! refused with an honest Arabic error — no invented value,
//! no crash. Allocation itself is pure Rust, so `احجز` and
//! `حرّر` work on every target including WASM.

use crate::error::{RioError, RioResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// Maximum bytes in one allocation: 64 MiB — big enough for
/// any real buffer, small enough that one bad call is harmless.
pub const MAX_SINGLE_ALLOCATION: u64 = 64 * 1024 * 1024;

/// Maximum bytes live at once (reserved, not yet freed):
/// 256 MiB — a runaway reserve-without-free loop cannot
/// out-of-memory a real machine.
pub const MAX_TOTAL_LIVE: u64 = 256 * 1024 * 1024;

/// Live blocks: handle -> zeroed bytes. One table for the
/// whole OS process, shared by both engines (a Niva program
/// is one OS process, so it has one block table).
static REGISTRY: LazyLock<Mutex<HashMap<u64, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Block handles start at 1 and never repeat in one run.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Live bytes in the registry (O(1) cap check — summing the
/// table on every allocation would be quadratic in a hostile
/// many-tiny-blocks loop). Raised only after a successful
/// insert, lowered only after a successful remove.
static TOTAL_LIVE: AtomicU64 = AtomicU64::new(0);

/// Test-only serialization for the process-wide registry:
/// cargo runs tests in parallel threads, and the registry is
/// a single static. Every test that touches it locks this
/// first (same pattern as PROC_TEST_LOCK in processes.rs).
#[cfg(test)]
pub(crate) static MEM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lock the test serialization lock, recovering from
/// poisoning: if an earlier test panicked while holding it,
/// the next test must still run and report its OWN result
/// instead of failing with PoisonError (CI 2026-11-06: one
/// macOS panic cascaded into four PoisonErrors that hid
/// every other test's verdict). This is cascade-hardening
/// only — the panicking test itself still fails loudly on
/// its own line.
#[cfg(test)]
fn mem_test_lock() -> std::sync::MutexGuard<'static, ()> {
    MEM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Test-only: drop every live block and reset the live total.
/// The caller must hold MEM_TEST_LOCK (via mem_test_lock),
/// so no other memory test can hold a handle at the time.
#[cfg(test)]
pub(crate) fn test_clear() {
    if let Ok(mut reg) = REGISTRY.lock() {
        reg.clear();
    }
    TOTAL_LIVE.store(0, Ordering::SeqCst);
}

/// Test-only: live bytes right now.
#[cfg(test)]
pub(crate) fn test_live_total() -> u64 {
    TOTAL_LIVE.load(Ordering::SeqCst)
}

/// Reserve a block: `ذاكرة.احجز حجم` — returns the block
/// handle. The block is zeroed (no stale memory leaks into
/// another program). The handle is consumed by `حرّر`.
pub fn allocate_block(size: i64, line: usize, col: usize) -> RioResult<u64> {
    if size <= 0 {
        return Err(RioError::typ(
            "الدالة ذاكرة.احجز تتوقع حجما موجبا",
            line,
            col,
        ));
    }
    let size = size as u64;
    if size > MAX_SINGLE_ALLOCATION {
        return Err(RioError::runtime(
            "حجم الكتلة تجاوز الحد (٦٤ ميبيبايت) — احجز كتلا أصغر",
            line,
            col,
        ));
    }
    if TOTAL_LIVE.load(Ordering::SeqCst) + size > MAX_TOTAL_LIVE {
        return Err(RioError::runtime(
            "مجموع الذاكرة المحجوزة تجاوز الحد (٢٥٦ ميبيبايت) — حرّر بعض الكتل",
            line,
            col,
        ));
    }
    let block = vec![0u8; size as usize];
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    match REGISTRY.lock() {
        Ok(mut reg) => {
            // Strict cap at insert time (two threads may both
            // have passed the fast path): refuse instead of
            // letting the total creep past the cap.
            let live: u64 = reg.values().map(|b| b.len() as u64).sum();
            if live + size > MAX_TOTAL_LIVE {
                return Err(RioError::runtime(
                    "مجموع الذاكرة المحجوزة تجاوز الحد (٢٥٦ ميبيبايت) — حرّر بعض الكتل",
                    line,
                    col,
                ));
            }
            reg.insert(id, block);
        }
        Err(_) => {
            return Err(RioError::runtime(
                "تعذّر تسجيل الكتلة الجديدة",
                line,
                col,
            ));
        }
    }
    TOTAL_LIVE.fetch_add(size, Ordering::SeqCst);
    Ok(id)
}

/// Take a live block out of the registry, or an Arabic error
/// if the handle is unknown (already freed, or never was).
fn take_block(id: u64, line: usize, col: usize) -> RioResult<Vec<u8>> {
    match REGISTRY.lock() {
        Ok(mut reg) => reg.remove(&id).ok_or_else(|| {
            RioError::runtime(format!("كتلة غير معروفة: {}", id), line, col)
        }),
        Err(_) => Err(RioError::runtime(
            format!("كتلة غير معروفة: {}", id),
            line,
            col,
        )),
    }
}

/// Free a block: `ذاكرة.حرّر معرف` — the handle is consumed,
/// so freeing twice is an error.
pub fn free_block(id: u64, line: usize, col: usize) -> RioResult<()> {
    let block = take_block(id, line, col)?;
    TOTAL_LIVE.fetch_sub(block.len() as u64, Ordering::SeqCst);
    Ok(())
}

/// Free memory now: `ذاكرة.المتاح()` — bytes. Linux (+Android)
/// reads MemAvailable from /proc/meminfo: the memory actually
/// usable, not just MemFree. Other platforms refuse honestly.
pub fn available_memory(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        available_memory_linux(line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = line;
        let _ = col;
        Err(RioError::runtime(
            "ذاكرة.المتاح غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn available_memory_linux(line: usize, col: usize) -> RioResult<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").map_err(|_| {
        RioError::runtime("تعذّر قراءة ذاكرة النظام", line, col)
    })?;
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("MemAvailable:") {
            let num: String = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            if let Ok(kb) = num.parse::<u64>() {
                return Ok(kb * 1024);
            }
            break;
        }
    }
    Err(RioError::runtime("تعذّر قراءة ذاكرة النظام", line, col))
}

/// The page size: `ذاكرة.حجم_الصفحة()` — bytes. Linux
/// (+Android) reads AT_PAGESZ (type 6) from /proc/self/auxv.
/// Other platforms refuse honestly — no invented 4096.
pub fn page_size(line: usize, col: usize) -> RioResult<u64> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        page_size_linux(line, col)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = line;
        let _ = col;
        Err(RioError::runtime(
            "ذاكرة.حجم_الصفحة غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn page_size_linux(line: usize, col: usize) -> RioResult<u64> {
    let bytes = std::fs::read("/proc/self/auxv").map_err(|_| {
        RioError::runtime("تعذّر قراءة ذاكرة النظام", line, col)
    })?;
    // auxv entries are (type, value) pairs of native words;
    // every target here is 64-bit, so 16 bytes per entry.
    for chunk in bytes.chunks_exact(16) {
        let kind = u64::from_ne_bytes(chunk[0..8].try_into().unwrap());
        let value = u64::from_ne_bytes(chunk[8..16].try_into().unwrap());
        if kind == 0 {
            break;
        }
        if kind == 6 && value > 0 {
            return Ok(value);
        }
    }
    Err(RioError::runtime("تعذّر قراءة ذاكرة النظام", line, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_free_roundtrip() {
        let _guard = mem_test_lock();
        test_clear();
        let id = allocate_block(1024, 1, 1).unwrap();
        assert!(id > 0);
        assert_eq!(test_live_total(), 1024);
        let id2 = allocate_block(2048, 1, 1).unwrap();
        assert_ne!(id2, id);
        assert_eq!(test_live_total(), 3072);
        free_block(id, 1, 1).unwrap();
        assert_eq!(test_live_total(), 2048);
        free_block(id2, 1, 1).unwrap();
        assert_eq!(test_live_total(), 0);
    }

    #[test]
    fn alloc_zero_is_zeroed() {
        let _guard = mem_test_lock();
        test_clear();
        let id = allocate_block(16, 1, 1).unwrap();
        {
            let reg = REGISTRY.lock().unwrap();
            let block = reg.get(&id).expect("block is registered");
            assert!(block.iter().all(|&b| b == 0));
        }
        free_block(id, 1, 1).unwrap();
    }

    #[test]
    fn alloc_bad_size_is_arabic() {
        let _guard = mem_test_lock();
        test_clear();
        for bad in [0, -1, -1024] {
            let err = allocate_block(bad, 1, 1).unwrap_err().to_string();
            assert!(err.contains("حجما موجبا"), "got: {}", err);
        }
    }

    #[test]
    fn free_unknown_and_double_free_error() {
        let _guard = mem_test_lock();
        test_clear();
        let err = free_block(999_999, 1, 1).unwrap_err().to_string();
        assert!(err.contains("كتلة غير معروفة"), "got: {}", err);
        let id = allocate_block(8, 1, 1).unwrap();
        free_block(id, 1, 1).unwrap();
        let err = free_block(id, 1, 1).unwrap_err().to_string();
        assert!(err.contains("كتلة غير معروفة"), "got: {}", err);
    }

    #[test]
    fn single_cap_enforced() {
        let _guard = mem_test_lock();
        test_clear();
        let err = allocate_block(MAX_SINGLE_ALLOCATION as i64 + 1, 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("تجاوز الحد"), "got: {}", err);
        assert_eq!(test_live_total(), 0);
    }

    #[test]
    fn total_cap_enforced() {
        let _guard = mem_test_lock();
        test_clear();
        // Four 64 MiB blocks fill the 256 MiB total exactly;
        // one more byte must be refused.
        let mut ids = Vec::new();
        for _ in 0..4 {
            ids.push(allocate_block(MAX_SINGLE_ALLOCATION as i64, 1, 1).unwrap());
        }
        assert_eq!(test_live_total(), MAX_TOTAL_LIVE);
        let err = allocate_block(1, 1, 1).unwrap_err().to_string();
        assert!(err.contains("تجاوز الحد"), "got: {}", err);
        // After freeing one block, allocating works again.
        free_block(ids.pop().unwrap(), 1, 1).unwrap();
        let id = allocate_block(1024, 1, 1).unwrap();
        assert_eq!(test_live_total(), 3 * MAX_SINGLE_ALLOCATION + 1024);
        free_block(id, 1, 1).unwrap();
        for id in ids {
            free_block(id, 1, 1).unwrap();
        }
        assert_eq!(test_live_total(), 0);
    }

    #[test]
    fn system_queries_positive_on_linux() {
        // Linux (+Android): the real values, positive and sane.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let _guard = mem_test_lock();
            let free = available_memory(1, 1).unwrap();
            assert!(free > 0, "available must be positive");
            let ps = page_size(1, 1).unwrap();
            assert!(ps >= 1024 && ps <= 1024 * 1024, "page size sane: {}", ps);
            assert_eq!(ps & (ps - 1), 0, "page size is a power of two");
        }
    }

    #[test]
    fn available_matches_meminfo() {
        // The value must match the system, not just be
        // positive: parse /proc/meminfo directly in the test
        // and compare within 1 MiB (MemAvailable drifts
        // slowly; the two reads are microseconds apart).
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let _guard = mem_test_lock();
            let text = std::fs::read_to_string("/proc/meminfo").unwrap();
            let mut expected = 0u64;
            for l in text.lines() {
                if let Some(rest) = l.strip_prefix("MemAvailable:") {
                    let num: String = rest
                        .trim()
                        .trim_end_matches("kB")
                        .trim()
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect();
                    expected = num.parse::<u64>().unwrap() * 1024;
                    break;
                }
            }
            assert!(expected > 0, "test could not parse meminfo");
            let got = available_memory(1, 1).unwrap();
            let diff = got.abs_diff(expected);
            assert!(
                diff < 1024 * 1024,
                "available {} vs meminfo {} (diff {} bytes)",
                got,
                expected,
                diff
            );
        }
    }

    #[test]
    fn page_size_matches_auxv() {
        // Exact match with a direct /proc/self/auxv parse.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let _guard = mem_test_lock();
            let bytes = std::fs::read("/proc/self/auxv").unwrap();
            let mut expected = 0u64;
            for chunk in bytes.chunks_exact(16) {
                let kind = u64::from_ne_bytes(chunk[0..8].try_into().unwrap());
                let value = u64::from_ne_bytes(chunk[8..16].try_into().unwrap());
                if kind == 0 {
                    break;
                }
                if kind == 6 {
                    expected = value;
                    break;
                }
            }
            assert!(expected > 0, "test could not parse auxv");
            assert_eq!(page_size(1, 1).unwrap(), expected);
        }
    }

    #[test]
    fn system_queries_refused_elsewhere() {
        // macOS / Windows / WASM: an honest Arabic refusal,
        // never an invented value and never a crash.
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            let _guard = mem_test_lock();
            let err = available_memory(1, 1).unwrap_err().to_string();
            assert!(err.contains("غير مدعوم"), "got: {}", err);
            let err = page_size(1, 1).unwrap_err().to_string();
            assert!(err.contains("غير مدعوم"), "got: {}", err);
        }
    }
}
