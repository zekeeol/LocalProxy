//! Paths and constants shared by the core and its privileged WireGuard helper.
//!
//! Both the core (running as the desktop user) and the root helper need to
//! agree on where the control socket, the helper pid file and the helper
//! single-instance lock live. Deriving them from one place keeps the helper
//! takeover logic in sync with the launcher.

use std::path::{Path, PathBuf};

/// Unix socket the helper listens on.
pub const HELPER_SOCKET_FILE: &str = "localproxy-wireguard.sock";
/// Pid of the helper that currently owns the socket.
pub const HELPER_PID_FILE: &str = "localproxy-wireguard.pid";
/// Advisory lock (`flock`) that guarantees a single helper per socket.
pub const HELPER_LOCK_FILE: &str = "localproxy-wireguard.lock";
/// Bumped whenever the helper protocol or its lifecycle guarantees change:
/// the core only reuses a helper that answers with this exact version.
pub const HELPER_PROTOCOL_VERSION: u32 = 4;

/// Directory holding the socket, pid file and lock file.
pub fn socket_dir() -> PathBuf {
    std::env::temp_dir().join("localproxy-wireguard")
}

/// Control socket path used by the core.
pub fn socket_path() -> PathBuf {
    socket_dir().join(HELPER_SOCKET_FILE)
}

/// Pid file path used by the core.
pub fn pid_path() -> PathBuf {
    socket_dir().join(HELPER_PID_FILE)
}

/// Sibling of `socket` with the given file name.
///
/// The helper receives only `--socket <path>`; deriving the pid/lock paths from
/// that argument keeps it independent of the helper's own `TMPDIR`, which can
/// differ from the desktop user's when the process is started as root.
pub fn sibling(socket: &Path, file_name: &str) -> PathBuf {
    match socket.parent() {
        Some(parent) => parent.join(file_name),
        None => PathBuf::from(file_name),
    }
}

/// Record file for helper lifecycle events, written by the core.
pub fn log_path() -> Option<PathBuf> {
    dirs::data_dir().map(|base| base.join("LocalProxy").join("wireguard-helper.log"))
}
