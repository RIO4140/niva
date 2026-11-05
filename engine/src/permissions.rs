//! File capability policy (Meeting 4, Phase 1A permissions slice).
//!
//! Default: RESTRICTED. A Niva program may read/write files only
//! inside its own program directory (program_dir), including
//! subdirectories. Relative paths resolve against program_dir,
//! NOT against the terminal working directory (this matches
//! module loading, which was already program_dir-only).
//!
//! Trusted mode (--اسمح-بالكل, explicit CLI flag only): no file
//! restriction. Use it for your own programs that must reach
//! outside their folder. It does NOT change module loading:
//! module names are identifiers only, so they can never escape
//! program_dir anyway.
//!
//! Checks in restricted mode, in order:
//! 1. Lexical normalization: "." and ".." are resolved without
//!    touching the filesystem, so ".." escape is caught even
//!    for files that do not exist yet.
//! 2. Prefix check: the normalized candidate must start with
//!    the normalized base. Otherwise: Arabic Io error.
//! 3. Symlink check: the existing prefix of the candidate is
//!    canonicalized, and the result must still start with the
//!    canonical base. A symlink inside the folder pointing
//!    outside is rejected, even if the lexical path looks inside.
//!
//! Network: Niva has no network builtin at all, so there is no
//! network capability to grant. This module documents that
//! fact; no network code is added here.

use crate::error::{ErrorKind, RioError, RioResult};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FilePolicy {
    pub trusted: bool,
    pub base: PathBuf,
}

impl FilePolicy {
    pub fn restricted(base: PathBuf) -> Self {
        Self { trusted: false, base }
    }

    pub fn trusted(base: PathBuf) -> Self {
        Self { trusted: true, base }
    }

    fn normalize(base: &Path, path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                Component::RootDir => out.push("/"),
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                Component::Normal(s) => out.push(s),
                Component::Prefix(p) => out.push(p.as_os_str()),
            }
        }
        // Keep the base absolute for the prefix check: if the
        // input was relative, it was already joined to base
        // before this call, so out is absolute here in practice.
        let _ = base;
        out
    }

    fn canonicalize_with_suffix(path: &Path) -> PathBuf {
        let mut existing = path.to_path_buf();
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            match existing.file_name() {
                Some(name) => {
                    suffix.push(name.to_os_string());
                    existing.pop();
                }
                None => break,
            }
        }
        match std::fs::canonicalize(&existing) {
            Ok(canon) => {
                let mut rebuilt = canon;
                for part in suffix.iter().rev() {
                    rebuilt.push(part);
                }
                rebuilt
            }
            Err(_) => path.to_path_buf(),
        }
    }

    /// Expand a leading `~` (home) like any Unix terminal:
    /// `~` is the program dir (home), `~/x` is under it.
    /// Anything else passes through unchanged.
    fn expand_home(&self, path_str: &str) -> PathBuf {
        if path_str == "~" {
            self.base.clone()
        } else if let Some(rest) = path_str.strip_prefix("~/") {
            self.base.join(rest)
        } else {
            PathBuf::from(path_str)
        }
    }

    /// Display a path the terminal way (owner's order
    /// 2026-11-05 ~22:38: paths look like any Unix terminal):
    /// the program dir (home) is `~`, below it `~/...`; an
    /// absolute path inside Android's private /data/ area never
    /// shows its internal segments — only its leaf name; any
    /// other path shows as-is. The package name and the
    /// /data/data/... prefix can never reach the display.
    pub fn display_path(&self, path: &Path) -> String {
        if path == self.base {
            return "~".to_string();
        }
        if let Ok(rel) = path.strip_prefix(&self.base) {
            if rel.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rel.to_string_lossy());
        }
        if path.is_absolute() {
            if let Some(first) = path.components().nth(1) {
                if first.as_os_str() == "data" {
                    if let Some(leaf) = path.file_name() {
                        return leaf.to_string_lossy().to_string();
                    }
                }
            }
        }
        path.to_string_lossy().to_string()
    }

    /// Display-form of a user-typed path string: already-display
    /// forms (`~`, `~/...`) and relative paths pass through;
    /// absolute paths go through display_path.
    pub fn display_str(&self, s: &str) -> String {
        if s == "~" || s.starts_with("~/") {
            return s.to_string();
        }
        let p = Path::new(s);
        if p.is_absolute() {
            return self.display_path(p);
        }
        s.to_string()
    }

    /// Resolve a user-supplied path string to a filesystem path,
    /// or return an Arabic Io error if it escapes the base.
    pub fn resolve(&self, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        if self.trusted {
            return Ok(self.expand_home(path_str));
        }
        let given = self.expand_home(path_str);
        let joined = if given.is_absolute() {
            given
        } else {
            self.base.join(given)
        };
        self.check(joined, path_str, line, col)
    }

    /// Like resolve(), but relative paths start from `cwd`
    /// (an absolute directory — the session's current directory
    /// set by انتقل) instead of the base. Absolute paths ignore
    /// cwd. In trusted mode relative paths also start from cwd,
    /// so cd works there too.
    pub fn resolve_in(
        &self,
        cwd: &Path,
        path_str: &str,
        line: usize,
        col: usize,
    ) -> RioResult<PathBuf> {
        if self.trusted {
            let given = self.expand_home(path_str);
            if given.is_absolute() {
                return Ok(given);
            }
            return Ok(cwd.join(given));
        }
        let given = self.expand_home(path_str);
        let joined = if given.is_absolute() {
            given
        } else {
            cwd.join(given)
        };
        self.check(joined, path_str, line, col)
    }

    /// Shared restricted-mode checks for a joined candidate:
    /// lexical normalization, then the prefix check, then the
    /// symlink check. Returns the lexical path the program
    /// named (canonical forms are for comparison only).
    fn check(&self, joined: PathBuf, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        // Lexical normalization (no filesystem access).
        let normalized = Self::normalize(&self.base, &joined);
        let base_norm = Self::normalize(&self.base, &self.base);
        if !normalized.starts_with(&base_norm) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج مجلد البرنامج، والوصول مقيد بمجلد البرنامج افتراضيا (استخدم --اسمح-بالكل للبرامج الموثوقة)", self.display_str(path_str)),
                line,
                col,
            ));
        }
        // Symlink check: canonicalize BOTH sides the same way
        // (longest existing prefix + remaining suffix), because
        // the base itself may not exist yet, or the file may be
        // in a new subdirectory. Checking only the candidate's
        // existing prefix against a non-canonical base would
        // wrongly reject files in a not-yet-existing folder.
        // The canonical forms are used ONLY for this security
        // comparison. The returned path is the lexical one the
        // program named: canonicalize rewrites it (macOS /tmp
        // -> /private/tmp, Windows -> \\?\D:\...), which broke
        // the caller's path and the tests on those platforms
        // (CI 2026-11-05: relative_inside_base_ok failed on
        // macOS and Windows, passed on Linux).
        let canon_candidate = Self::canonicalize_with_suffix(&normalized);
        let canon_base = Self::canonicalize_with_suffix(&base_norm);
        if !canon_candidate.starts_with(&canon_base) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج مجلد البرنامج عبر رابط رمزي، والوصول مقيد بمجلد البرنامج افتراضيا", self.display_str(path_str)),
                line,
                col,
            ));
        }
        Ok(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_inside_base_ok() {
        let base = PathBuf::from("/tmp/niva-perm-test");
        let p = FilePolicy::restricted(base.clone());
        let out = p.resolve("out.txt", 1, 1).unwrap();
        assert_eq!(out, PathBuf::from("/tmp/niva-perm-test/out.txt"));
    }

    #[test]
    fn dotdot_escape_rejected() {
        let base = PathBuf::from("/tmp/niva-perm-test");
        let p = FilePolicy::restricted(base);
        assert!(p.resolve("../escape.txt", 1, 1).is_err());
        assert!(p.resolve("sub/../../escape.txt", 1, 1).is_err());
    }

    #[test]
    fn absolute_outside_rejected_absolute_inside_ok() {
        let base = PathBuf::from("/tmp/niva-perm-test");
        let p = FilePolicy::restricted(base);
        assert!(p.resolve("/etc/passwd", 1, 1).is_err());
        assert!(p.resolve("/tmp/niva-perm-test/inner.txt", 1, 1).is_ok());
    }

    #[test]
    fn trusted_allows_outside() {
        let base = PathBuf::from("/tmp/niva-perm-test");
        let p = FilePolicy::trusted(base);
        assert!(p.resolve("/etc/passwd", 1, 1).is_ok());
        assert!(p.resolve("../escape.txt", 1, 1).is_ok());
    }

    #[test]
    fn resolve_returns_lexical_path_not_canonical() {
        // Regression (CI 2026-11-05): resolve used to return
        // the canonicalized path, so on macOS (/tmp is a
        // symlink to /private/tmp) and Windows (\\?\ prefix)
        // the caller got a rewritten path. The canonical forms
        // are for the security check only; the returned path
        // must be the lexical one the program named. This test
        // reproduces the bug on Linux too, via a symlinked base.
        #[cfg(unix)]
        {
            let real =
                std::env::temp_dir().join(format!("niva-perm-real-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&real);
            std::fs::create_dir_all(&real).unwrap();
            let link =
                std::env::temp_dir().join(format!("niva-perm-link-{}", std::process::id()));
            let _ = std::fs::remove_file(&link);
            if std::os::unix::fs::symlink(&real, &link).is_err() {
                let _ = std::fs::remove_dir_all(&real);
                return;
            }
            let p = FilePolicy::restricted(link.clone());
            let out = p.resolve("out.txt", 1, 1).unwrap();
            let _ = std::fs::remove_file(&link);
            let _ = std::fs::remove_dir_all(&real);
            assert_eq!(out, link.join("out.txt"));
        }
    }

    #[test]
    fn home_displays_as_tilde_and_expands() {
        // Owner's order 2026-11-05 ~22:38: paths look like any
        // Unix terminal — home is `~`, below it `~/...`.
        let base = PathBuf::from("/tmp/niva-home-test");
        let p = FilePolicy::restricted(base.clone());
        assert_eq!(p.display_path(&base), "~");
        assert_eq!(
            p.display_path(&base.join("مشاريع")),
            "~/مشاريع"
        );
        assert_eq!(
            p.display_path(&base.join("مشاريع/أ.نيفا")),
            "~/مشاريع/أ.نيفا"
        );
        // Absolute Android private paths never show internals.
        assert_eq!(
            p.display_path(Path::new("/data/data/com.rio4140.niva/cache/x")),
            "x"
        );
        // Other absolute paths show as-is (a real user path).
        assert_eq!(
            p.display_path(Path::new("/sdcard/Download/x.txt")),
            "/sdcard/Download/x.txt"
        );
        // Relative paths pass through.
        assert_eq!(p.display_str("ملف.txt"), "ملف.txt");
        assert_eq!(p.display_str("~"), "~");
        assert_eq!(p.display_str("~/ملف.txt"), "~/ملف.txt");
        assert_eq!(
            p.display_str("/data/data/com.rio4140.niva/files/ملف.txt"),
            "ملف.txt"
        );
        // `~` resolves to home, `~/x` under it.
        assert_eq!(p.resolve("~", 1, 1).unwrap(), base);
        assert_eq!(
            p.resolve("~/ملف.txt", 1, 1).unwrap(),
            base.join("ملف.txt")
        );
        // The escape error never leaks the internal path.
        let err = p
            .resolve("/data/data/com.rio4140.niva/files/خارج.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("com.rio4140.niva"), "got: {}", err);
        assert!(!err.contains("/data/"), "got: {}", err);
    }

    #[test]
    fn symlink_escape_rejected() {
        let dir = std::env::temp_dir().join(format!("niva-perm-sym-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let outside = std::env::temp_dir().join(format!("niva-perm-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        // CI 2026-11-05: on Windows the old #[cfg(unix)]-only
        // symlink creation was skipped, so no symlink existed
        // and the test asserted a rejection of nothing — it
        // failed vacuously. Create the link on Windows too;
        // if the machine forbids symlinks (no privilege), the
        // test has nothing to exercise and skips instead of
        // failing on an absence it did not create.
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, dir.join("link")).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, dir.join("link")).is_ok();
        #[cfg(not(any(unix, windows)))]
        let linked = false;
        if !linked {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }
        let p = FilePolicy::restricted(dir.clone());
        let res = p.resolve("link/secret.txt", 1, 1);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(res.is_err(), "symlink escape must be rejected");
    }
}
