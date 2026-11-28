//! The token, the endpoint file `agent.json` and the marker a running window holds.
use crate::core::actions::Dirs;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

/// Where a running endpoint says how to reach it. Readable by the current user only.
#[derive(Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub url: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub headless: bool,
    pub version: String,
}

// The token must not end up in a log through a stray `{:?}`.
impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Endpoint {{ port: {}, pid: {}, headless: {} }}",
            self.port, self.pid, self.headless
        )
    }
}

pub fn endpoint_file(dirs: &Dirs) -> PathBuf {
    dirs.data.join("agent.json")
}

fn window_file(dirs: &Dirs) -> PathBuf {
    dirs.data.join("window.lock")
}

/// 32 random bytes as hex.
pub fn new_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("No random numbers from the system: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Hash first, then compare the fixed-size digests without stopping at the first difference.
pub fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    let difference = a
        .iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(difference) == 0
}

/// Writes `agent.json` through a new file that only this user may open.
pub fn write_endpoint(dirs: &Dirs, endpoint: &Endpoint) -> Result<()> {
    std::fs::create_dir_all(&dirs.data)?;
    let target = endpoint_file(dirs);
    let staged = dirs
        .data
        .join(format!(".agent-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&staged);
    let mut file = private_file(&staged).context("Could not create the endpoint file")?;
    file.write_all(&serde_json::to_vec_pretty(endpoint)?)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&staged, &target).context("Could not publish the endpoint file")?;
    Ok(())
}

pub fn read_endpoint(dirs: &Dirs) -> Option<Endpoint> {
    serde_json::from_slice(&std::fs::read(endpoint_file(dirs)).ok()?).ok()
}

/// Removes `agent.json` if it still describes the endpoint with this token.
pub fn remove_endpoint(dirs: &Dirs, token: &str) {
    if read_endpoint(dirs).is_some_and(|e| e.token == token) {
        let _ = std::fs::remove_file(endpoint_file(dirs));
    }
}

/// Held by every launcher window while it is open, so that `--mcp-stdio` can tell that a window with
/// agent control switched off is running and must not go around it.
pub fn hold_window_marker(dirs: &Dirs) -> Option<File> {
    std::fs::create_dir_all(&dirs.data).ok()?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(window_file(dirs))
        .ok()?;
    file.lock_shared().ok()?;
    Some(file)
}

pub fn window_is_open(dirs: &Dirs) -> bool {
    let Ok(file) = File::options()
        .read(true)
        .write(true)
        .open(window_file(dirs))
    else {
        return false;
    };
    // The lock is released again when `file` is dropped.
    file.try_lock().is_err()
}

#[cfg(unix)]
fn private_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

/// A new file whose access list names this user and the system, and inherits nothing from the folder.
#[cfg(windows)]
fn private_file(path: &Path) -> Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree},
            Security::{
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    SDDL_REVISION_1,
                },
                GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
                TOKEN_USER, TokenUser,
            },
            Storage::FileSystem::{
                CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
        core::{PCWSTR, PWSTR},
    };

    // The SID of the user this process runs as, in text form.
    let sid = unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        // u64 elements keep the structure aligned.
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        let read = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        read?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let sid = text.to_string();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid?
    };

    let sddl: Vec<u16> = format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both strings are NUL terminated and live through the calls; the descriptor is freed once
    // the file exists, and the returned handle is owned by the `File`.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let handle = CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            Some(&attributes),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        );
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        Ok(File::from_raw_handle(handle?.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_and_compared_whole() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(same_secret(&a, &a.clone()));
        for other in [b.as_str(), "", &a[..63], &format!("{a}0")] {
            assert!(!same_secret(&a, other));
        }
    }

    #[test]
    fn endpoint_file_is_private_and_removed_only_by_its_owner() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: temp.path().join("data"),
            app: temp.path().into(),
        };
        let endpoint = Endpoint {
            url: "u".into(),
            port: 1,
            token: "secret-token".into(),
            pid: 2,
            headless: true,
            version: "v".into(),
        };
        write_endpoint(&dirs, &endpoint).unwrap();
        write_endpoint(&dirs, &endpoint).unwrap();
        assert_eq!(read_endpoint(&dirs).unwrap().token, "secret-token");
        assert!(!format!("{endpoint:?}").contains("secret-token"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(endpoint_file(&dirs))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        remove_endpoint(&dirs, "another");
        assert!(read_endpoint(&dirs).is_some());
        remove_endpoint(&dirs, "secret-token");
        assert!(!endpoint_file(&dirs).exists());
    }

    #[test]
    fn a_window_marker_is_seen_while_held() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: temp.path().join("data"),
            app: temp.path().into(),
        };
        assert!(!window_is_open(&dirs));
        let first = hold_window_marker(&dirs).unwrap();
        let second = hold_window_marker(&dirs).unwrap();
        assert!(window_is_open(&dirs));
        drop(first);
        assert!(window_is_open(&dirs));
        drop(second);
        assert!(!window_is_open(&dirs));
    }
}
