#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use winreg::HKCU;

#[cfg(windows)]
use crate::error::AppResult;

#[cfg(windows)]
const KEY: &str = r"Software\QingzhouSSH";
#[cfg(windows)]
const VALUE: &str = "DataRoot";

#[cfg(windows)]
pub fn load_data_root() -> AppResult<Option<PathBuf>> {
    match HKCU.open_subkey(KEY) {
        Ok(key) => Ok(key.get_value::<String, _>(VALUE).ok().map(PathBuf::from)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
pub fn save_data_root(path: &Path) -> AppResult<()> {
    let (key, _) = HKCU.create_subkey(KEY)?;
    key.set_value(VALUE, &path.to_string_lossy().to_string())?;
    Ok(())
}

#[cfg(windows)]
pub fn clear_data_root() -> AppResult<()> {
    match HKCU.open_subkey_with_flags(KEY, winreg::enums::KEY_SET_VALUE) {
        Ok(key) => match key.delete_value(VALUE) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
