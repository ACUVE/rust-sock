use std::env::var_os;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::{fs, io};
use tempfile::{tempdir, TempDir};

pub fn server() -> Option<Box<[OsString]>> {
    var_os("RUST_SOCK").map(|s| {
        s.as_bytes()
            .split(|v| v == &b","[0])
            .map(|t| <OsStr as OsStrExt>::from_bytes(t).to_os_string())
            .collect()
    })
}

pub fn new_unix_path() -> io::Result<(TempDir, OsString)> {
    let dir = tempdir()?;
    let mut path = dir.path().to_owned();
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions)?;
    path.push("sock");
    Ok((dir, path.into()))
}

pub fn application_dir() -> PathBuf {
    let mut app_name = env!("CARGO_PKG_NAME");
    if app_name.is_empty() {
        app_name = "rust-sock";
    }
    match dirs::config_dir().or_else(dirs::runtime_dir) {
        Some(mut buf) => {
            buf.push(app_name);
            buf
        }
        None => {
            let mut buf = dirs::home_dir().unwrap();
            buf.push(String::from(".") + app_name);
            buf
        }
    }
}
