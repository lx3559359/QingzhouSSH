use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableDataRootPointer {
    schema_version: u32,
    data_root: PathBuf,
}

pub fn load(pointer_path: &Path) -> AppResult<Option<PathBuf>> {
    let payload = match fs::read(pointer_path) {
        Ok(payload) => payload,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let pointer: PortableDataRootPointer = serde_json::from_slice(&payload)
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    if pointer.schema_version != SCHEMA_VERSION {
        return Err(AppError::Validation(
            "便携版数据目录指针版本不受支持".into(),
        ));
    }
    validate_absolute(&pointer.data_root)?;
    Ok(Some(pointer.data_root))
}

pub fn save(pointer_path: &Path, data_root: &Path) -> AppResult<()> {
    validate_absolute(data_root)?;
    let payload = serde_json::to_vec_pretty(&PortableDataRootPointer {
        schema_version: SCHEMA_VERSION,
        data_root: data_root.to_path_buf(),
    })
    .map_err(|error| AppError::Serialization(error.to_string()))?;
    AtomicFile::new(pointer_path, AllowOverwrite)
        .write(|file| {
            let mut writer = BufWriter::new(file);
            writer.write_all(&payload)?;
            writer.flush()?;
            writer.get_ref().sync_all()
        })
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

pub fn clear(pointer_path: &Path) -> AppResult<()> {
    match fs::remove_file(pointer_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn validate_absolute(path: &Path) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::Validation(
            "便携版数据目录指针必须是绝对路径".into(),
        ));
    }
    Ok(())
}
