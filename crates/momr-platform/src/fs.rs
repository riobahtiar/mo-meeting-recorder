//! Small file-system operations whose spelling differs per OS: private
//! directories, symlink-free reads and file links. Each is a few lines, but
//! they are the lines a port gets wrong, so they live here once.

use std::fs::File;
use std::io;
use std::path::Path;

/// Creates `path` and any missing parents as private directories: every
/// level it creates gets mode 0700 on Unix, default ACLs on Windows (real
/// ACLs arrive with the Windows shell). Levels that exist are left as they
/// are.
pub fn secure_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

/// Creates `path` as a new private directory, failing when anything is
/// already there: a directory someone else prepared (or a symlink planted
/// under a guessable name) is never reused for an agent's workdir.
pub fn new_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)
    }
}

/// Opens `path` for reading without following a symlink at its end, and
/// without blocking on a FIFO (`O_NONBLOCK` only affects the open; regular
/// files read normally). Windows has no such flags yet and opens plainly,
/// so it follows symlinks until the Windows shell adds a reparse-point check.
pub fn open_no_follow(path: &Path) -> io::Result<File> {
    // The values differ per OS and, on Linux, per architecture (aarch64's
    // O_NOFOLLOW is x86's O_DIRECTORY), so each target spells its own and
    // an unlisted one refuses to build rather than silently following.
    #[cfg(target_os = "macos")]
    const FLAGS: i32 = 0x0100 | 0x0004; // O_NOFOLLOW | O_NONBLOCK
    #[cfg(all(unix, not(target_os = "macos")))]
    compile_error!("open_no_follow needs the O_NOFOLLOW value for this target (plan 16)");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FLAGS)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(path)
    }
}

/// A symlink to `src` at `dest`. On Windows symlinks need a privilege, so
/// the error there is expected until the Windows shell copies instead.
pub fn link(src: &Path, dest: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dest)
    }
    #[cfg(not(unix))]
    {
        std::os::windows::fs::symlink_file(src, dest)
    }
}

/// `link`, only when `src` is a regular file and not itself a symlink; a
/// missing or odd `src` is not an error, just nothing to link.
pub fn link_if_regular(src: &Path, dest: &Path) -> io::Result<()> {
    if !std::fs::symlink_metadata(src).is_ok_and(|m| m.file_type().is_file()) {
        return Ok(());
    }
    link(src, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("momr-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[cfg(unix)]
    #[test]
    fn open_no_follow_refuses_a_symlink_and_opens_a_file() {
        let dir = scratch("nofollow");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("real");
        std::fs::write(&file, "ok").unwrap();
        link(&file, &dir.join("link")).unwrap();
        assert!(open_no_follow(&file).is_ok());
        assert!(open_no_follow(&dir.join("link")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn private_dirs_are_0700_and_new_ones_are_never_reused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let nested = dir.join("a/b");
        secure_dir(&nested).unwrap();
        for level in [&dir, &dir.join("a"), &nested] {
            let mode = std::fs::metadata(level).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", level.display());
        }
        let fresh = dir.join("fresh");
        new_private_dir(&fresh).unwrap();
        assert!(
            new_private_dir(&fresh).is_err(),
            "an existing dir is refused"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_if_regular_skips_what_is_not_a_file() {
        let dir = scratch("links");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(link_if_regular(&dir.join("missing"), &dir.join("a")).is_ok());
        assert!(!dir.join("a").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
