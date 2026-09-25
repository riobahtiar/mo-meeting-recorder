//! Small file-system operations whose spelling differs per OS: private
//! directories, symlink-free reads and file links. Each is a few lines, but
//! they are the lines a port gets wrong, so they live here once.

use std::fs::File;
use std::io;
use std::path::Path;

/// Creates `path` and parents as a private directory: mode 0700 on Unix,
/// default ACLs on Windows (real ACLs arrive with the Windows shell).
pub fn secure_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn open_no_follow(path: &Path) -> io::Result<File> {
    #[cfg(target_os = "macos")]
    const FLAGS: i32 = 0x0100 | 0x0004; // O_NOFOLLOW | O_NONBLOCK
    #[cfg(target_os = "linux")]
    const FLAGS: i32 = 0o400000 | 0o4000; // O_NOFOLLOW | O_NONBLOCK
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
    compile_error!("open_no_follow needs the O_NOFOLLOW value for this OS (plan 16)");
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

/// A symlink to `src` at `dest`, only when `src` is a regular file and not
/// itself a symlink. Best effort on Windows (which needs a privilege for
/// symlinks): a missed link only costs the agent its config file.
pub fn link_if_regular(src: &Path, dest: &Path) {
    if !std::fs::symlink_metadata(src).is_ok_and(|m| m.file_type().is_file()) {
        return;
    }
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(src, dest);
    }
    #[cfg(not(unix))]
    {
        let _ = std::os::windows::fs::symlink_file(src, dest);
    }
}
