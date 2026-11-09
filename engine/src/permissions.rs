//! File capability policy (Meeting 4, Phase 1A permissions slice).
//!
//! Default: RESTRICTED. A Niva program may read/write files only
//! inside its own program directory (program_dir), including
//! subdirectories. Relative paths resolve against program_dir,
//! NOT against the terminal working directory (this matches
//! module loading, which was already program_dir-only).
//!
//! Layout (Meeting 45 §59, items 35/38): the program folder
//! itself is internal; the user's home is `program_dir/home`.
//! The session starts in home, and `~` means home. The
//! program folder is reachable as `$نيفا` (shortcut) and
//! shown as `$نيفا/...` in displays; under home `~/...`
//! wins the longest match.
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

/// Display a path with forward slashes on every platform.
/// Rust keeps the separator a path was built with, and
/// Windows joins with `\` — but the language shows `/`
/// everywhere (terminal style, owner's order 2026-11-05).
/// Every path the language returns or shows goes through
/// this one function, so the display never depends on the
/// platform that built the path. The replacement runs only
/// where the platform separator IS a backslash: on Unix a
/// `\` inside a file name is a real character and is kept.
pub(crate) fn slash_display(path: &Path) -> String {
    let s = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '\\' {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

#[derive(Debug, Clone)]
pub struct FilePolicy {
    pub trusted: bool,
    /// The program folder (internal): modules, store, settings.
    pub base: PathBuf,
    /// The user's home: `base/home` (Meeting 45 §59).
    pub home: PathBuf,
    /// Read-only external root (Meeting 50 §64, items 94/96):
    /// the shared Downloads folder, exposed to programs as
    /// `تخزين/تنزيلات` — reads only, every write under it is
    /// refused. Set by the launcher (`--تنزيلات <مسار>`) or
    /// live by the host app (`:تنزيلات <مسار>`); `None` means
    /// storage is not exposed yet, and the spelling itself is
    /// an Arabic error naming the fix. In trusted mode
    /// (--اسمح-بالكل) the restriction is lifted, as every
    /// other file restriction is.
    pub downloads: Option<PathBuf>,
}

impl FilePolicy {
    pub fn restricted(base: PathBuf) -> Self {
        let home = base.join("home");
        Self { trusted: false, base, home, downloads: None }
    }

    pub fn trusted(base: PathBuf) -> Self {
        let home = base.join("home");
        Self { trusted: true, base, home, downloads: None }
    }

    /// Set the read-only downloads root (Meeting 50 §64).
    /// The caller validates that the path exists and is a
    /// directory; the policy only stores it.
    pub fn set_downloads(&mut self, path: PathBuf) {
        self.downloads = Some(path);
    }

    /// The spelling names the downloads root
    /// (`تخزين/تنزيلات`, or anything below it) — return the
    /// rest below the root, or `None` for any other spelling.
    fn downloads_rest(path_str: &str) -> Option<&str> {
        if path_str == "تخزين/تنزيلات" {
            Some("")
        } else {
            path_str.strip_prefix("تخزين/تنزيلات/")
        }
    }

    /// Map a downloads spelling to its filesystem path, or
    /// the «not exposed yet» error when the root is unset.
    /// `None` = the spelling does not name downloads.
    fn downloads_path(&self, path_str: &str, line: usize, col: usize) -> Option<RioResult<PathBuf>> {
        let rest = Self::downloads_rest(path_str)?;
        match &self.downloads {
            Some(root) => {
                let mapped = if rest.is_empty() {
                    root.clone()
                } else {
                    root.join(rest)
                };
                Some(self.check_downloads(mapped, path_str, line, col))
            }
            None => Some(Err(RioError::new(
                ErrorKind::Io,
                "التخزين غير مكشوف — امنح إذن التخزين أولا",
                line,
                col,
            ))),
        }
    }

    /// Checks for a mapped downloads path: the lexical form
    /// must stay under the root (`..` cannot climb out of
    /// it), and the canonical form must stay under the
    /// canonical root (a symlink inside Downloads pointing
    /// elsewhere is rejected, like every other symlink
    /// escape in `check`).
    fn check_downloads(&self, mapped: PathBuf, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        let root = self.downloads.as_ref().unwrap();
        let normalized = Self::normalize(root, &mapped);
        let root_norm = Self::normalize(root, root);
        if !normalized.starts_with(&root_norm) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج تخزين/تنزيلات", self.display_str(path_str)),
                line,
                col,
            ));
        }
        let canon_candidate = Self::canonicalize_with_suffix(&normalized);
        let canon_root = Self::canonicalize_with_suffix(&root_norm);
        if !canon_candidate.starts_with(&canon_root) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} خارج تخزين/تنزيلات عبر رابط رمزي", self.display_str(path_str)),
                line,
                col,
            ));
        }
        Ok(normalized)
    }

    /// Is this resolved path under the downloads root?
    /// Used by the write variants to refuse every write
    /// there. `false` when no root is set, or in trusted
    /// mode (the root does not bind a trusted session).
    pub fn is_under_downloads(&self, path: &Path) -> bool {
        if self.trusted {
            return false;
        }
        match &self.downloads {
            Some(root) => {
                let normalized = Self::normalize(root, path);
                let root_norm = Self::normalize(root, root);
                normalized.starts_with(&root_norm)
            }
            None => false,
        }
    }

    /// Refuse a write under the downloads root (Meeting 50
    /// §64): the root is read-only, literally. Any other
    /// path passes through with the same error as `resolve`.
    fn refuse_downloads_write(&self, path: &Path, path_str: &str, line: usize, col: usize) -> RioResult<()> {
        if self.is_under_downloads(path) {
            return Err(RioError::new(
                ErrorKind::Io,
                format!("الملف {} في تخزين/تنزيلات للقراءة فقط", self.display_str(path_str)),
                line,
                col,
            ));
        }
        Ok(())
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

    /// Expand a leading `~` (home) and `$نيفا` (program
    /// folder) like any Unix terminal (Meeting 45 §59,
    /// items 35/38): `~` is home (`base/home`), `~/x`
    /// under it; `$نيفا` is the program folder itself,
    /// `$نيفا/x` under it. Anything else passes through
    /// unchanged.
    fn expand_home(&self, path_str: &str) -> PathBuf {
        if path_str == "~" {
            self.home.clone()
        } else if let Some(rest) = path_str.strip_prefix("~/") {
            self.home.join(rest)
        } else if path_str == "$نيفا" {
            self.base.clone()
        } else if let Some(rest) = path_str.strip_prefix("$نيفا/") {
            self.base.join(rest)
        } else {
            PathBuf::from(path_str)
        }
    }

    /// Display a path the terminal way (owner's order
    /// 2026-11-05 ~22:38: paths look like any Unix terminal):
    /// home is `~`, below it `~/...`; the downloads root is
    /// `تخزين/تنزيلات`, below it `تخزين/تنزيلات/...`
    /// (Meeting 50 §64); the program folder is
    /// `$نيفا`, below it `$نيفا/...` (longest match wins, so
    /// a path under home never shows as `$نيفا/home/...`);
    /// an absolute path inside Android's private /data/ area
    /// never shows its internal segments — only its leaf
    /// name; any other path shows as-is, with `/`
    /// separators on every platform (slash_display). The
    /// package name and the /data/data/... prefix can never
    /// reach the display.
    pub fn display_path(&self, path: &Path) -> String {
        if path == self.home {
            return "~".to_string();
        }
        if let Ok(rel) = path.strip_prefix(&self.home) {
            if rel.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", slash_display(rel));
        }
        if let Some(root) = &self.downloads {
            if path == root {
                return "تخزين/تنزيلات".to_string();
            }
            if let Ok(rel) = path.strip_prefix(root) {
                if rel.as_os_str().is_empty() {
                    return "تخزين/تنزيلات".to_string();
                }
                return format!("تخزين/تنزيلات/{}", slash_display(rel));
            }
        }
        if path == self.base {
            return "$نيفا".to_string();
        }
        if let Ok(rel) = path.strip_prefix(&self.base) {
            if rel.as_os_str().is_empty() {
                return "$نيفا".to_string();
            }
            return format!("$نيفا/{}", slash_display(rel));
        }
        // has_root(), not is_absolute(): on Windows a path
        // like "/data/..." is rooted but has no drive prefix,
        // so is_absolute() is false there — and the Android
        // private-path strip below never ran (CI 2026-11-06:
        // the Windows error message leaked the raw Android
        // path). A rooted path is enough: the check only
        // looks at the first component.
        if path.has_root() {
            if let Some(first) = path.components().nth(1) {
                if first.as_os_str() == "data" {
                    if let Some(leaf) = path.file_name() {
                        return leaf.to_string_lossy().to_string();
                    }
                }
            }
        }
        slash_display(path)
    }

    /// Display-form of a user-typed path string: already-display
    /// forms (`~`, `~/...`, `$نيفا`, `$نيفا/...`) and
    /// relative paths pass through; absolute paths go through
    /// display_path. Rooted paths (has_root, not is_absolute)
    /// count as absolute here for the same Windows reason as
    /// display_path: "/data/..." must reach the Android strip
    /// instead of echoing raw.
    pub fn display_str(&self, s: &str) -> String {
        if s == "~" || s.starts_with("~/") || s == "$نيفا" || s.starts_with("$نيفا/") {
            return s.to_string();
        }
        let p = Path::new(s);
        if p.has_root() {
            return self.display_path(p);
        }
        slash_display(p)
    }

    /// Resolve a user-supplied path string to a filesystem path,
    /// or return an Arabic Io error if it escapes the base.
    pub fn resolve(&self, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        if self.trusted {
            // The downloads spelling names the root in
            // trusted mode too (no restriction is lifted
            // there — only the write refusal is).
            if let Some(rest) = Self::downloads_rest(path_str) {
                if let Some(root) = &self.downloads {
                    let mapped = if rest.is_empty() { root.clone() } else { root.join(rest) };
                    return Ok(Self::normalize(root, &mapped));
                }
            }
            return Ok(self.expand_home(path_str));
        }
        // Meeting 50 §64: the downloads spelling is special
        // in restricted mode — it maps outside the base (or
        // errors when storage is not exposed), never falls
        // through to a local folder of the same name.
        if let Some(mapped) = self.downloads_path(path_str, line, col) {
            return mapped;
        }
        let given = self.expand_home(path_str);
        let joined = if given.is_absolute() {
            given
        } else {
            self.base.join(given)
        };
        self.check(joined, path_str, line, col)
    }

    /// Like `resolve`, but a write is refused under the
    /// downloads root (Meeting 50 §64): the root is
    /// read-only, literally.
    pub fn resolve_for_write(&self, path_str: &str, line: usize, col: usize) -> RioResult<PathBuf> {
        let resolved = self.resolve(path_str, line, col)?;
        self.refuse_downloads_write(&resolved, path_str, line, col)?;
        Ok(resolved)
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
            if let Some(rest) = Self::downloads_rest(path_str) {
                if let Some(root) = &self.downloads {
                    let mapped = if rest.is_empty() { root.clone() } else { root.join(rest) };
                    return Ok(Self::normalize(root, &mapped));
                }
            }
            let given = self.expand_home(path_str);
            if given.is_absolute() {
                return Ok(given);
            }
            return Ok(cwd.join(given));
        }
        if let Some(mapped) = self.downloads_path(path_str, line, col) {
            return mapped;
        }
        let given = self.expand_home(path_str);
        let joined = if given.is_absolute() {
            given
        } else {
            cwd.join(given)
        };
        self.check(joined, path_str, line, col)
    }

    /// Like `resolve_in`, but a write is refused under the
    /// downloads root (Meeting 50 §64).
    pub fn resolve_in_for_write(
        &self,
        cwd: &Path,
        path_str: &str,
        line: usize,
        col: usize,
    ) -> RioResult<PathBuf> {
        let resolved = self.resolve_in(cwd, path_str, line, col)?;
        self.refuse_downloads_write(&resolved, path_str, line, col)?;
        Ok(resolved)
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
        // Meeting 45 §59: home is `base/home`, the program
        // folder itself is `$نيفا` — the longest match wins,
        // so a path under home never shows as `$نيفا/home/...`.
        // The fake home is built per-platform (a temp dir),
        // never a hardcoded Unix path: on Windows "/tmp/..."
        // is a rooted path with no drive. The hardcoded
        // Android "/data/..." strings below stay as they are:
        // they are the test's SUBJECT (the leak the display
        // must strip on every platform), not the home.
        let base = std::env::temp_dir()
            .join(format!("niva-home-test-{}", std::process::id()));
        let p = FilePolicy::restricted(base.clone());
        assert_eq!(p.display_path(&base), "$نيفا");
        assert_eq!(p.display_path(&base.join(".نيفا")), "$نيفا/.نيفا");
        assert_eq!(p.display_path(&base.join("home")), "~");
        assert_eq!(
            p.display_path(&base.join("home/مشاريع")),
            "~/مشاريع"
        );
        assert_eq!(
            p.display_path(&base.join("home/مشاريع/أ.نيفا")),
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
        assert_eq!(p.display_str("$نيفا"), "$نيفا");
        assert_eq!(p.display_str("$نيفا/ملف.txt"), "$نيفا/ملف.txt");
        assert_eq!(
            p.display_str("/data/data/com.rio4140.niva/files/ملف.txt"),
            "ملف.txt"
        );
        // `~` resolves to home, `~/x` under it; `$نيفا`
        // resolves to the program folder, `$نيفا/x` under it.
        assert_eq!(p.resolve("~", 1, 1).unwrap(), base.join("home"));
        assert_eq!(
            p.resolve("~/ملف.txt", 1, 1).unwrap(),
            base.join("home/ملف.txt")
        );
        assert_eq!(p.resolve("$نيفا", 1, 1).unwrap(), base);
        assert_eq!(
            p.resolve("$نيفا/ملف.txt", 1, 1).unwrap(),
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

    // Meeting 50 §64 (items 94/96): the downloads root.
    fn downloads_dirs() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("niva-dl-base-{}", std::process::id()));
        let root = std::env::temp_dir().join(format!("niva-dl-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        (base, root)
    }

    #[test]
    fn downloads_unset_spelling_errors() {
        let base = std::env::temp_dir().join(format!("niva-dl-base-{}", std::process::id()));
        let p = FilePolicy::restricted(base);
        let err = p
            .resolve("تخزين/تنزيلات/ملف.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("التخزين غير مكشوف"), "got: {}", err);
        // The bare root spelling errors the same way.
        assert!(p.resolve("تخزين/تنزيلات", 1, 1).is_err());
        // A plain `تخزين` folder is local, not special.
        assert!(p.resolve("تخزين/شيء.txt", 1, 1).is_ok());
    }

    #[test]
    fn downloads_root_reads_ok_writes_refused() {
        let (base, root) = downloads_dirs();
        std::fs::write(root.join("ملف.txt"), "x").unwrap();
        let mut p = FilePolicy::restricted(base.clone());
        p.set_downloads(root.clone());
        // Read resolve maps under the root.
        assert_eq!(
            p.resolve("تخزين/تنزيلات/ملف.txt", 1, 1).unwrap(),
            root.join("ملف.txt")
        );
        assert_eq!(p.resolve("تخزين/تنزيلات", 1, 1).unwrap(), root);
        // Display shows the terminal spelling.
        assert_eq!(
            p.display_path(&root.join("ملف.txt")),
            "تخزين/تنزيلات/ملف.txt"
        );
        // Every write variant refuses under the root.
        let werr = p
            .resolve_for_write("تخزين/تنزيلات/ملف.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(werr.contains("للقراءة فقط"), "got: {}", werr);
        let werr2 = p
            .resolve_in_for_write(&base.join("home"), "تخزين/تنزيلات/جديد.txt", 1, 1)
            .unwrap_err()
            .to_string();
        assert!(werr2.contains("للقراءة فقط"), "got: {}", werr2);
        // `..` cannot climb out of the root.
        assert!(p.resolve("تخزين/تنزيلات/../خارج.txt", 1, 1).is_err());
        // A local file still resolves and writes fine.
        assert!(p.resolve("ملف.txt", 1, 1).is_ok());
        assert!(p.resolve_for_write("ملف.txt", 1, 1).is_ok());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn downloads_symlink_escape_rejected() {
        let (base, root) = downloads_dirs();
        let outside = std::env::temp_dir().join(format!("niva-dl-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("رابط")).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, root.join("رابط")).is_ok();
        #[cfg(not(any(unix, windows)))]
        let linked = false;
        if !linked {
            let _ = std::fs::remove_dir_all(&base);
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }
        let mut p = FilePolicy::restricted(base.clone());
        p.set_downloads(root.clone());
        let res = p.resolve("تخزين/تنزيلات/رابط/سر.txt", 1, 1);
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(res.is_err(), "symlink escape out of downloads must be rejected");
    }

    #[test]
    fn downloads_trusted_unrestricted() {
        let (base, root) = downloads_dirs();
        let mut p = FilePolicy::trusted(base.clone());
        p.set_downloads(root.clone());
        // Trusted mode lifts every restriction, downloads
        // included — the caller opted out explicitly.
        assert!(!p.is_under_downloads(&root.join("ملف.txt")));
        assert!(p.resolve_for_write("تخزين/تنزيلات/ملف.txt", 1, 1).is_ok());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&root);
    }
}
