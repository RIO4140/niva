//! File capability policy (Meeting 4, Phase 1A permissions slice).
//!
//! Default: RESTRICTED. A Rio program may read/write files only
//! inside its own program directory (program_dir), including
//! subdirectories. Relative paths resolve against program_dir,
//! NOT against the terminal working directory (this matches
//! module loading, which was already program_dir-only).
//!
//! Trusted mode (--allow-all, explicit CLI flag only): no file
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
//! Network: Rio has no network builtin at all, so there is no
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

    /// Resolve a user-supplied path string to a filesystem path,
    /// or return an Arabic Io error if it escapes the base.
    pub fn resolve(&self, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        if self.trusted {
            return Ok(PathBuf::from(path_str));
        }
        let given = Path::new(path_str);
        let joined = if given.is_absolute() {
            given.to_path_buf()
        } else {
            self.base.join(given)
        };
        // Lexical normalization (no filesystem access).
        let normalized = Self::normalize(&self.base, &joined);
        let base_norm = Self::normalize(&self.base, &self.base);
        if !normalized.starts_with(&base_norm) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج مجلد البرنامج، والوصول مقيد بمجلد البرنامج افتراضيا (استخدم --allow-all للبرامج الموثوقة)", path_str),
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
        let canon_candidate = Self::canonicalize_with_suffix(&normalized);
        let canon_base = Self::canonicalize_with_suffix(&base_norm);
        if !canon_candidate.starts_with(&canon_base) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج مجلد البرنامج عبر رابط رمزي، والوصول مقيد بمجلد البرنامج افتراضيا", path_str),
                line,
                col,
            ));
        }
        Ok(canon_candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_inside_base_ok() {
        let base = PathBuf::from("/tmp/rio-perm-test");
        let p = FilePolicy::restricted(base.clone());
        let out = p.resolve("out.txt", 1, 1).unwrap();
        assert_eq!(out, PathBuf::from("/tmp/rio-perm-test/out.txt"));
    }

    #[test]
    fn dotdot_escape_rejected() {
        let base = PathBuf::from("/tmp/rio-perm-test");
        let p = FilePolicy::restricted(base);
        assert!(p.resolve("../escape.txt", 1, 1).is_err());
        assert!(p.resolve("sub/../../escape.txt", 1, 1).is_err());
    }

    #[test]
    fn absolute_outside_rejected_absolute_inside_ok() {
        let base = PathBuf::from("/tmp/rio-perm-test");
        let p = FilePolicy::restricted(base);
        assert!(p.resolve("/etc/passwd", 1, 1).is_err());
        assert!(p.resolve("/tmp/rio-perm-test/inner.txt", 1, 1).is_ok());
    }

    #[test]
    fn trusted_allows_outside() {
        let base = PathBuf::from("/tmp/rio-perm-test");
        let p = FilePolicy::trusted(base);
        assert!(p.resolve("/etc/passwd", 1, 1).is_ok());
        assert!(p.resolve("../escape.txt", 1, 1).is_ok());
    }

    #[test]
    fn symlink_escape_rejected() {
        let dir = std::env::temp_dir().join(format!("rio-perm-sym-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let outside = std::env::temp_dir().join(format!("rio-perm-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
        let p = FilePolicy::restricted(dir.clone());
        let res = p.resolve("link/secret.txt", 1, 1);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(res.is_err(), "symlink escape must be rejected");
    }
}
