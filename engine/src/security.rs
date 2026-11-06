//! Security queries for the `أمن` native module
//! (catalog task 18, from `security/`): the read-only
//! Linux security surface — the current process's
//! capability sets, the active Linux Security Modules,
//! and an access(2)-style permission check for a file
//! inside the program folder. One implementation,
//! shared by both engines (interpreter + VM) through
//! the stdlib native functions — never implemented
//! twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-10-SECURITY.md` §1): read-only only. No
//! capability is ever dropped, no privilege is ever
//! raised, no process state is ever changed from Niva
//! code in v1 (`أسقط_قدرة` is deferred: it needs
//! capset, and libc is not in the tree). The queries
//! read /proc/self/status and /sys files on Linux
//! (+Android), and check permission bits. Everywhere
//! else they refuse with an honest Arabic error — no
//! invented value, no crash. No new dependencies
//! (no libc): everything here is std plus /proc and
//! /sys reads.

use crate::error::{RioError, RioResult};
#[cfg(any(target_os = "linux", target_os = "android"))]
use crate::error::ErrorKind;
use crate::permissions::FilePolicy;
use crate::value::Value;
use std::path::Path;

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// The 41 Linux capabilities (bit, name without the
/// `CAP_` prefix), from include/uapi/linux/capability.h
/// in the local kernel tree (`CAP_LAST_CAP` =
/// `CAP_CHECKPOINT_RESTORE` = 40). Names are kernel
/// identifiers, shown as they are — the same precedent
/// as `نظام.اسم_النداء` returning `read` and `exit`.
/// (Production on Linux/Android; everywhere in tests.)
#[cfg(any(test, any(target_os = "linux", target_os = "android")))]
static CAPABILITIES: &[(u32, &str)] = &[
    (0, "CHOWN"),
    (1, "DAC_OVERRIDE"),
    (2, "DAC_READ_SEARCH"),
    (3, "FOWNER"),
    (4, "FSETID"),
    (5, "KILL"),
    (6, "SETGID"),
    (7, "SETUID"),
    (8, "SETPCAP"),
    (9, "LINUX_IMMUTABLE"),
    (10, "NET_BIND_SERVICE"),
    (11, "NET_BROADCAST"),
    (12, "NET_ADMIN"),
    (13, "NET_RAW"),
    (14, "IPC_LOCK"),
    (15, "IPC_OWNER"),
    (16, "SYS_MODULE"),
    (17, "SYS_RAWIO"),
    (18, "SYS_CHROOT"),
    (19, "SYS_PTRACE"),
    (20, "SYS_PACCT"),
    (21, "SYS_ADMIN"),
    (22, "SYS_BOOT"),
    (23, "SYS_NICE"),
    (24, "SYS_RESOURCE"),
    (25, "SYS_TIME"),
    (26, "SYS_TTY_CONFIG"),
    (27, "MKNOD"),
    (28, "LEASE"),
    (29, "AUDIT_WRITE"),
    (30, "AUDIT_CONTROL"),
    (31, "SETFCAP"),
    (32, "MAC_OVERRIDE"),
    (33, "MAC_ADMIN"),
    (34, "SYSLOG"),
    (35, "WAKE_ALARM"),
    (36, "BLOCK_SUSPEND"),
    (37, "AUDIT_READ"),
    (38, "PERFMON"),
    (39, "BPF"),
    (40, "CHECKPOINT_RESTORE"),
];

/// Decode a capability bitmask into names, in bit
/// order. A set bit above 40 (a future kernel with a
/// new capability) is listed by its decimal number —
/// no invented name, no silent loss.
/// (Production on Linux/Android; everywhere in tests.)
#[cfg(any(test, any(target_os = "linux", target_os = "android")))]
fn decode_capabilities(mask: u64) -> Vec<String> {
    let mut out = Vec::new();
    for bit in 0..64 {
        if (mask >> bit) & 1 == 1 {
            match CAPABILITIES.iter().find(|&&(b, _)| b == bit) {
                Some(&(_, name)) => out.push(name.to_string()),
                None => out.push(bit.to_string()),
            }
        }
    }
    out
}

/// The four capability masks of this process, from
/// `/proc/self/status` (CapEff, CapPrm, CapBnd,
/// CapInh), parsed as hex.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_status_caps(line: usize, col: usize) -> RioResult<(u64, u64, u64, u64)> {
    let text = std::fs::read_to_string("/proc/self/status")
        .map_err(|_| RioError::runtime("تعذّر قراءة معلومات الأمن", line, col))?;
    let mut eff = None;
    let mut prm = None;
    let mut bnd = None;
    let mut inh = None;
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("CapEff:") {
            eff = u64::from_str_radix(rest.trim(), 16).ok();
        } else if let Some(rest) = l.strip_prefix("CapPrm:") {
            prm = u64::from_str_radix(rest.trim(), 16).ok();
        } else if let Some(rest) = l.strip_prefix("CapBnd:") {
            bnd = u64::from_str_radix(rest.trim(), 16).ok();
        } else if let Some(rest) = l.strip_prefix("CapInh:") {
            inh = u64::from_str_radix(rest.trim(), 16).ok();
        }
    }
    match (eff, prm, bnd, inh) {
        (Some(e), Some(p), Some(b), Some(i)) => Ok((e, p, b, i)),
        _ => Err(RioError::runtime("تعذّر قراءة معلومات الأمن", line, col)),
    }
}

/// `أمن.قدرات_العملية()` — the four capability sets
/// as a dict {فعالة، مسموحة، مقيدة، موروثة}, each a
/// list of capability names.
pub fn process_capabilities(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        process_capabilities_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn process_capabilities_native(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let (eff, prm, bnd, inh) = read_status_caps(line, col)?;
        let list = |mask: u64| {
            Value::List(
                decode_capabilities(mask)
                    .into_iter()
                    .map(Value::Str)
                    .collect(),
            )
        };
        Ok(Value::Dict(vec![
            ("فعالة".to_string(), list(eff)),
            ("مسموحة".to_string(), list(prm)),
            ("مقيدة".to_string(), list(bnd)),
            ("موروثة".to_string(), list(inh)),
        ]))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "أمن.قدرات_العملية غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

/// Split the `/sys/kernel/security/lsm` content
/// (comma-separated) into names; empty pieces dropped.
/// (Production on Linux/Android; everywhere in tests.)
#[cfg(any(test, any(target_os = "linux", target_os = "android")))]
fn parse_lsm_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The active LSMs, from the real files: the
/// `/sys/kernel/security/lsm` list first; if that file
/// is missing or unreadable, the individual indicators
/// (a present file proves the module is active — a
/// bare directory does not). An empty result means "no
/// LSM detected", not "no LSM exists" (landlock, for
/// instance, has no individual indicator in /sys and
/// is only visible through the `lsm` file).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_lsm_list() -> Vec<String> {
    if let Ok(text) = std::fs::read_to_string("/sys/kernel/security/lsm") {
        let names = parse_lsm_list(&text);
        if !names.is_empty() {
            return names;
        }
    }
    let mut out = Vec::new();
    if Path::new("/sys/fs/selinux/enforce").exists() {
        out.push("selinux".to_string());
    }
    if Path::new("/sys/kernel/security/apparmor/profiles").exists() {
        out.push("apparmor".to_string());
    }
    if Path::new("/sys/kernel/security/tomoyo").exists() {
        out.push("tomoyo".to_string());
    }
    if Path::new("/sys/kernel/security/smackfs").exists() {
        out.push("smack".to_string());
    }
    out
}

/// `أمن.الوضع()` — the active LSMs as a list of
/// names (an empty list when none was detected).
pub fn security_mode(line: usize, col: usize) -> RioResult<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        security_mode_native(line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn security_mode_native(_line: usize, _col: usize) -> RioResult<Value> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Ok(Value::List(
            read_lsm_list().into_iter().map(Value::Str).collect(),
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Err(RioError::runtime(
            "أمن.الوضع غير مدعوم على هذه المنصة",
            _line,
            _col,
        ))
    }
}

/// The pure access-bit logic (access(2)-style): given
/// a file's mode/owner/group and the process's ids,
/// which of read/write/execute does the process have?
/// The applicable class is the owner bits when the
/// file's uid is the process's uid, else the group
/// bits when the file's gid is the process's gid or
/// one of its supplementary groups, else the other
/// bits. Root (uid 0) reads and writes anything;
/// execute still needs one execute bit set anywhere
/// (as access(2) does). Mount flags are not consulted
/// (a read-only mount still reports writable).
/// (Production on Linux/Android; everywhere in tests.)
#[cfg(any(test, any(target_os = "linux", target_os = "android")))]
fn access_bits(
    mode: u32,
    file_uid: u32,
    file_gid: u32,
    uid: u32,
    gid: u32,
    groups: &[u32],
) -> (bool, bool, bool) {
    if uid == 0 {
        let any_x = mode & 0o111 != 0;
        return (true, true, any_x);
    }
    let bits = if file_uid == uid {
        (mode >> 6) & 0o7
    } else if file_gid == gid || groups.contains(&file_gid) {
        (mode >> 3) & 0o7
    } else {
        mode & 0o7
    };
    (bits & 0o4 != 0, bits & 0o2 != 0, bits & 0o1 != 0)
}

/// The process's ids from `/proc/self/status`: the
/// real uid, the real gid, and the supplementary
/// groups (the first value on each line is the real
/// id — the same reading as `نظام.معرف_المستخدم`).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_self_ids(line: usize, col: usize) -> RioResult<(u32, u32, Vec<u32>)> {
    let text = std::fs::read_to_string("/proc/self/status")
        .map_err(|_| RioError::runtime("تعذّر قراءة معلومات الأمن", line, col))?;
    let mut uid = None;
    let mut gid = None;
    let mut groups = Vec::new();
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("Uid:") {
            uid = rest.split_whitespace().next().and_then(|s| s.parse::<u32>().ok());
        } else if let Some(rest) = l.strip_prefix("Gid:") {
            gid = rest.split_whitespace().next().and_then(|s| s.parse::<u32>().ok());
        } else if let Some(rest) = l.strip_prefix("Groups:") {
            groups = rest
                .split_whitespace()
                .filter_map(|s| s.parse::<u32>().ok())
                .collect();
        }
    }
    match (uid, gid) {
        (Some(u), Some(g)) => Ok((u, g, groups)),
        _ => Err(RioError::runtime("تعذّر قراءة معلومات الأمن", line, col)),
    }
}

/// `أمن.تحقق(مسار)` — {قراءة، كتابة، تنفيذ} for this
/// process on a file inside the program folder. The
/// FilePolicy gate applies first, exactly like the
/// file functions: a path outside the folder is
/// refused with the policy's own error, never with
/// the system's answer. A missing file is an Arabic
/// error; any other metadata failure is an Arabic
/// error too. Directories work the same way (read =
/// list, write = create, execute = traverse).
pub fn check_access(
    policy: &FilePolicy,
    cwd: &Path,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (policy, cwd, path);
        Err(RioError::runtime(WASM_REFUSAL, line, col))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        check_access_native(policy, cwd, path, line, col)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_access_native(
    policy: &FilePolicy,
    cwd: &Path,
    path: &str,
    line: usize,
    col: usize,
) -> RioResult<Value> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::MetadataExt;
        let resolved = policy.resolve_in(cwd, path, line, col)?;
        let display = policy.display_str(path);
        let m = match std::fs::metadata(&resolved) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!("الملف غير موجود: {}", display),
                    line,
                    col,
                ))
            }
            Err(_) => {
                return Err(RioError::new(
                    ErrorKind::Io,
                    format!("تعذّر الوصول إلى الملف {}", display),
                    line,
                    col,
                ))
            }
        };
        let (uid, gid, groups) = read_self_ids(line, col)?;
        let (r, w, x) = access_bits(m.mode(), m.uid(), m.gid(), uid, gid, &groups);
        Ok(Value::Dict(vec![
            ("قراءة".to_string(), Value::Bool(r)),
            ("كتابة".to_string(), Value::Bool(w)),
            ("تنفيذ".to_string(), Value::Bool(x)),
        ]))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (policy, cwd, path);
        Err(RioError::runtime(
            "أمن.تحقق غير مدعوم على هذه المنصة",
            line,
            col,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_table_has_41_entries() {
        assert_eq!(CAPABILITIES.len(), 41);
        for (i, &(bit, _)) in CAPABILITIES.iter().enumerate() {
            assert_eq!(bit as usize, i, "bit {} out of order", bit);
        }
    }

    #[test]
    fn cap_decode_single_bits() {
        assert_eq!(decode_capabilities(1 << 0), vec!["CHOWN".to_string()]);
        assert_eq!(decode_capabilities(1 << 12), vec!["NET_ADMIN".to_string()]);
        assert_eq!(decode_capabilities(1 << 21), vec!["SYS_ADMIN".to_string()]);
        assert_eq!(
            decode_capabilities(1 << 40),
            vec!["CHECKPOINT_RESTORE".to_string()]
        );
        assert!(decode_capabilities(0).is_empty());
    }

    #[test]
    fn cap_decode_this_machine_mask() {
        // The sandbox mask, fixed here (not read live, so
        // the test is deterministic on every machine):
        // 38 capabilities — all but NET_ADMIN (12),
        // NET_RAW (13) and SYS_PTRACE (19).
        let names = decode_capabilities(0x0000_01ff_fff7_cfff);
        assert_eq!(names.len(), 38);
        assert!(names.contains(&"SYS_ADMIN".to_string()));
        assert!(names.contains(&"CHOWN".to_string()));
        assert!(names.contains(&"CHECKPOINT_RESTORE".to_string()));
        assert!(!names.contains(&"NET_ADMIN".to_string()));
        assert!(!names.contains(&"NET_RAW".to_string()));
        assert!(!names.contains(&"SYS_PTRACE".to_string()));
    }

    #[test]
    fn cap_decode_unknown_bit_listed_by_number() {
        assert_eq!(decode_capabilities(1u64 << 41), vec!["41".to_string()]);
        assert_eq!(
            decode_capabilities((1u64 << 40) | (1u64 << 41)),
            vec!["CHECKPOINT_RESTORE".to_string(), "41".to_string()]
        );
    }

    #[test]
    fn lsm_list_parsing() {
        assert_eq!(
            parse_lsm_list("lockdown,capability,landlock,yama,apparmor"),
            vec![
                "lockdown".to_string(),
                "capability".to_string(),
                "landlock".to_string(),
                "yama".to_string(),
                "apparmor".to_string(),
            ]
        );
        assert_eq!(parse_lsm_list("apparmor"), vec!["apparmor".to_string()]);
        assert!(parse_lsm_list("").is_empty());
        assert_eq!(parse_lsm_list("selinux, "), vec!["selinux".to_string()]);
    }

    #[test]
    fn access_bits_root() {
        // Root reads and writes anything; execute needs
        // one execute bit set anywhere.
        assert_eq!(access_bits(0o644, 1000, 1000, 0, 0, &[]), (true, true, false));
        assert_eq!(access_bits(0o755, 1000, 1000, 0, 0, &[]), (true, true, true));
        assert_eq!(access_bits(0o000, 1000, 1000, 0, 0, &[]), (true, true, false));
        assert_eq!(access_bits(0o001, 1000, 1000, 0, 0, &[]), (true, true, true));
    }

    #[test]
    fn access_bits_owner_group_other() {
        // Owner class (uid 1000 owns the file).
        assert_eq!(
            access_bits(0o640, 1000, 100, 1000, 100, &[]),
            (true, true, false)
        );
        assert_eq!(
            access_bits(0o700, 1000, 100, 1000, 100, &[]),
            (true, true, true)
        );
        // Group class through the primary gid.
        assert_eq!(
            access_bits(0o640, 2000, 100, 1000, 100, &[]),
            (true, false, false)
        );
        // Group class through a supplementary group.
        assert_eq!(
            access_bits(0o640, 2000, 200, 1000, 100, &[200]),
            (true, false, false)
        );
        assert_eq!(
            access_bits(0o640, 2000, 300, 1000, 100, &[200]),
            (false, false, false)
        );
        // Other class.
        assert_eq!(
            access_bits(0o755, 2000, 200, 1000, 100, &[]),
            (true, false, true)
        );
        assert_eq!(
            access_bits(0o000, 2000, 200, 1000, 100, &[]),
            (false, false, false)
        );
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn live_capabilities_match_proc() {
        // The dict has the four keys, each a list, and
        // the effective list matches /proc/self/status
        // read independently here.
        let v = process_capabilities(1, 1).unwrap();
        let pairs = match v {
            Value::Dict(p) => p,
            other => panic!("expected dict, got {:?}", other),
        };
        assert_eq!(pairs.len(), 4);
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["فعالة", "مسموحة", "مقيدة", "موروثة"]);
        let text = std::fs::read_to_string("/proc/self/status").unwrap();
        let eff_hex = text
            .lines()
            .find(|l| l.starts_with("CapEff:"))
            .unwrap()
            .trim_start_matches("CapEff:")
            .trim();
        let eff = u64::from_str_radix(eff_hex, 16).unwrap();
        let expected = decode_capabilities(eff);
        let got = match &pairs[0].1 {
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Str(s) => s.clone(),
                    _ => panic!("expected string"),
                })
                .collect::<Vec<String>>(),
            _ => panic!("expected list"),
        };
        assert_eq!(got, expected);
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn live_security_mode_matches_sys() {
        // A list, equal to an independent re-read of the
        // same files (no machine value is hardcoded —
        // the list differs between hosts).
        let v = security_mode(1, 1).unwrap();
        let got = match v {
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Str(s) => s.clone(),
                    _ => panic!("expected string"),
                })
                .collect::<Vec<String>>(),
            other => panic!("expected list, got {:?}", other),
        };
        assert_eq!(got, read_lsm_list());
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn live_check_access_on_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "niva-security-test-{}-check",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let policy = FilePolicy::restricted(dir.clone());
        let file = dir.join("فحص.txt");
        std::fs::write(&file, "نص").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let v = check_access(&policy, &dir, "فحص.txt", 1, 1).unwrap();
        let pairs = match v {
            Value::Dict(p) => p,
            other => panic!("expected dict, got {:?}", other),
        };
        // The file is owned by this process's user, so
        // the owner bits apply on any machine (root or
        // not): 0644 reads and writes, does not execute.
        let get = |key: &str| match pairs.iter().find(|(k, _)| k == key) {
            Some((_, Value::Bool(b))) => *b,
            _ => panic!("missing key {}", key),
        };
        assert_eq!(get("قراءة"), true);
        assert_eq!(get("كتابة"), true);
        assert_eq!(get("تنفيذ"), false);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        let v = check_access(&policy, &dir, "فحص.txt", 1, 1).unwrap();
        let pairs = match v {
            Value::Dict(p) => p,
            _ => panic!("expected dict"),
        };
        let get = |key: &str| match pairs.iter().find(|(k, _)| k == key) {
            Some((_, Value::Bool(b))) => *b,
            _ => panic!("missing key {}", key),
        };
        assert_eq!(get("تنفيذ"), true);
        let err = check_access(&policy, &dir, "مفقود.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("غير موجود"), "got: {}", err);
        let err = check_access(&policy, &dir, "../خارج.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("خارج مجلد البرنامج"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn security_refused_in_browser() {
        for f in [process_capabilities, security_mode] {
            let err = f(1, 1).unwrap_err().to_string();
            assert!(
                err.contains("غير مدعومة في متصفح الويب"),
                "got: {}",
                err
            );
        }
        let dir = std::path::PathBuf::from("/tmp");
        let policy = FilePolicy::restricted(dir.clone());
        let err = check_access(&policy, &dir, "x", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("غير مدعومة في متصفح الويب"),
            "got: {}",
            err
        );
    }

    #[test]
    #[cfg(all(not(any(target_os = "linux", target_os = "android")), not(target_arch = "wasm32")))]
    fn security_refused_off_linux() {
        for f in [process_capabilities, security_mode] {
            let err = f(1, 1).unwrap_err().to_string();
            assert!(err.contains("غير مدعوم على هذه المنصة"), "got: {}", err);
        }
        let dir = std::path::PathBuf::from("/tmp");
        let policy = FilePolicy::restricted(dir.clone());
        let err = check_access(&policy, &dir, "x", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("غير مدعوم على هذه المنصة"), "got: {}", err);
    }
}
