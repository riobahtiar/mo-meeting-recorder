//! momr-platform: the seams between the core and the operating system
//! (plan 16, D25). One module per seam: paths, processes, files, sockets,
//! and later capture, playback, keep-awake and secrets. `cfg(target_os)` is
//! allowed here and nowhere else; every other crate is portable. macOS is
//! implemented and tested; Windows 11+ and Linux gain their branches when
//! scheduled, and the core already checks on Windows.

/// The app and folder name everywhere: the binary, the helpers, the socket
/// and the folders (D13). One constant so the shell and the seams agree.
pub const APP_NAME: &str = "momr";

pub mod fs;
pub mod paths;
pub mod process;
pub mod sock;
