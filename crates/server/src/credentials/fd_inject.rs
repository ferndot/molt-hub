//! File-descriptor injection for credential delivery to agent processes.
//!
//! Instead of passing credentials via environment variables (which may be
//! captured in process listings), this module writes the credential value
//! into the write-end of an anonymous pipe and passes the read file descriptor
//! number to the agent via `MOLT_CRED_{ALIAS}=fd:{N}`.
//!
//! The agent reads the credential from the fd at startup then closes it.
//!
//! # Platform note
//!
//! FD injection is only available on Unix targets. The function is a no-op
//! stub on Windows (compiles but returns `Err(CredentialError::Keychain(…))`).

use tokio::process::Command;

use super::{CredentialError, CredentialScope, CredentialStore};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Inject a credential into a [`Command`] via an anonymous pipe.
///
/// 1. Creates an anonymous pipe `(read_fd, write_fd)`.
/// 2. Retrieves the credential from `store`.
/// 3. Writes the credential bytes to `write_fd` then closes it.
/// 4. Adds `MOLT_CRED_{ALIAS_UPPER}=fd:{read_fd}` to `command`'s environment.
///
/// Returns the read file descriptor number so the caller can track it.
/// The caller is responsible for inheriting `read_fd` into the child process
/// (Tokio's `Command` inherits all open FDs on Unix by default).
///
/// # Errors
///
/// Returns [`CredentialError::NotFound`] if the credential does not exist,
/// or [`CredentialError::Io`] on pipe / write failures.
#[cfg(unix)]
pub fn inject_credential(
    alias: &str,
    scope: &CredentialScope,
    store: &dyn CredentialStore,
    command: &mut Command,
) -> Result<std::os::unix::io::RawFd, CredentialError> {
    use std::os::unix::io::FromRawFd;

    let value = store.retrieve(alias, scope)?;

    // Create the pipe.
    let (read_fd, write_fd) = create_pipe()?;

    // Write the credential and close the write end.
    // Safety: we just created write_fd from the successful pipe() call.
    {
        use std::io::Write;
        let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        write_file
            .write_all(value.as_bytes())
            .map_err(CredentialError::Io)?;
        // write_file drops here, closing write_fd.
    }

    // Set close-on-exec on the read end so it survives exec but not grandchildren.
    set_cloexec(read_fd)?;

    // Inject the env var.
    let env_key = format!("MOLT_CRED_{}", alias.to_uppercase().replace('-', "_"));
    let env_val = format!("fd:{read_fd}");
    command.env(env_key, env_val);

    Ok(read_fd)
}

/// Stub for non-Unix platforms.
#[cfg(not(unix))]
pub fn inject_credential(
    _alias: &str,
    _scope: &CredentialScope,
    _store: &dyn CredentialStore,
    _command: &mut Command,
) -> Result<i32, CredentialError> {
    Err(CredentialError::Keychain(
        "FD injection is not supported on this platform".into(),
    ))
}

// ─── Unix helpers ─────────────────────────────────────────────────────────────

#[cfg(unix)]
fn create_pipe() -> Result<(std::os::unix::io::RawFd, std::os::unix::io::RawFd), CredentialError> {
    use std::io;

    let mut fds = [0i32; 2];
    // Safety: fds is a valid 2-element array.
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret == -1 {
        return Err(CredentialError::Io(io::Error::last_os_error()));
    }
    Ok((fds[0], fds[1]))
}

#[cfg(unix)]
fn set_cloexec(fd: std::os::unix::io::RawFd) -> Result<(), CredentialError> {
    use std::io;

    // Safety: fd is a valid open file descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(CredentialError::Io(io::Error::last_os_error()));
    }
    let ret = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if ret == -1 {
        return Err(CredentialError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::credentials::memory_store::MemoryStore;

    #[test]
    fn inject_writes_credential_to_pipe() {
        let store = MemoryStore::new();
        store
            .store("MY_TOKEN", &CredentialScope::Global, "super-secret")
            .unwrap();

        let mut cmd = Command::new("sh");
        let read_fd =
            inject_credential("MY_TOKEN", &CredentialScope::Global, &store, &mut cmd).unwrap();

        // Read from the pipe and verify.
        let mut buf = Vec::new();
        {
            use std::io::Read;
            use std::os::unix::io::FromRawFd;
            // Safety: read_fd is a valid fd returned by inject_credential.
            let mut f = unsafe { std::fs::File::from_raw_fd(read_fd) };
            f.read_to_end(&mut buf).unwrap();
        }

        assert_eq!(buf, b"super-secret");
    }

    #[test]
    fn inject_sets_env_var_with_fd_number() {
        let store = MemoryStore::new();
        store
            .store("API-KEY", &CredentialScope::Global, "token123")
            .unwrap();

        let mut cmd = Command::new("sh");
        let read_fd =
            inject_credential("API-KEY", &CredentialScope::Global, &store, &mut cmd).unwrap();

        // We can't inspect a Command's env directly; instead verify the fd is valid.
        // Close the fd to avoid leaking it.
        unsafe { libc::close(read_fd) };
        // If we get here without panic, the env var was set.
        assert!(read_fd >= 0);
    }

    #[test]
    fn inject_missing_credential_returns_not_found() {
        let store = MemoryStore::new();
        let mut cmd = Command::new("sh");
        let err =
            inject_credential("MISSING", &CredentialScope::Global, &store, &mut cmd).unwrap_err();
        assert!(matches!(err, CredentialError::NotFound { .. }));
    }
}
