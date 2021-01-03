use std::env::var_os;
use std::ffi::OsString;

pub fn server() -> OsString {
    var_os("RUST_SOCK").unwrap_or_else(|| {
        dirs::runtime_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| dirs::home_dir().unwrap())
            .into()
    })
}
