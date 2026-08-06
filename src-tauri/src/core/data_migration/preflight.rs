use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    core::data_migration::model::{
        DataMigrationManifest, DataMigrationManifestEntry, DataMigrationPreview, ManifestEntryKind,
        ALLOWED_ROOT_ENTRIES, MIGRATION_COMPLETE_FILE, MIGRATION_JOURNAL_FILE,
    },
    error::{AppError, AppResult},
};

const MINIMUM_SPACE_RESERVE: u64 = 64 * 1024 * 1024;
const PREVIEW_TTL_MILLIS: i64 = 5 * 60 * 1000;

pub trait MigrationEnvironment {
    fn is_reparse_point(&self, path: &Path) -> AppResult<bool>;
    fn probe_writable(&self, directory: &Path) -> AppResult<()>;
    fn available_space(&self, directory: &Path) -> AppResult<u64>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemMigrationEnvironment;

impl MigrationEnvironment for SystemMigrationEnvironment {
    fn is_reparse_point(&self, path: &Path) -> AppResult<bool> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            return Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0);
        }
        #[cfg(not(windows))]
        Ok(false)
    }

    fn probe_writable(&self, directory: &Path) -> AppResult<()> {
        let probe = directory.join(format!(".migration-write-probe-{}", Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)?;
            file.write_all(b"qingzhou-data-migration")?;
            file.sync_all()
        })();
        let cleanup = fs::remove_file(&probe);
        result?;
        cleanup?;
        Ok(())
    }

    fn available_space(&self, directory: &Path) -> AppResult<u64> {
        fs2::available_space(directory).map_err(Into::into)
    }
}

pub fn preflight_data_root_migration(
    source: &Path,
    target: &Path,
    now: i64,
) -> AppResult<(DataMigrationPreview, DataMigrationManifest)> {
    preflight_with(source, target, now, &SystemMigrationEnvironment)
}

pub fn preflight_retry_data_root_migration(
    source: &Path,
    target: &Path,
    now: i64,
) -> AppResult<(DataMigrationPreview, DataMigrationManifest)> {
    preflight_retry_with(source, target, now, &SystemMigrationEnvironment)
}

pub fn preflight_with<E: MigrationEnvironment>(
    source: &Path,
    target: &Path,
    now: i64,
    environment: &E,
) -> AppResult<(DataMigrationPreview, DataMigrationManifest)> {
    preflight_inner(source, target, now, false, environment)
}

pub fn preflight_retry_with<E: MigrationEnvironment>(
    source: &Path,
    target: &Path,
    now: i64,
    environment: &E,
) -> AppResult<(DataMigrationPreview, DataMigrationManifest)> {
    preflight_inner(source, target, now, true, environment)
}

fn preflight_inner<E: MigrationEnvironment>(
    source: &Path,
    target: &Path,
    now: i64,
    retryable: bool,
    environment: &E,
) -> AppResult<(DataMigrationPreview, DataMigrationManifest)> {
    validate_absolute_non_root(source, "当前数据目录")?;
    validate_absolute_non_root(target, "目标数据目录")?;
    if !source.is_dir() {
        return Err(AppError::Validation(
            "当前数据目录不存在或不是文件夹".into(),
        ));
    }
    let normalized_source = fs::canonicalize(source)?;
    let normalized_target = normalize_future_path(target)?;
    reject_related_paths(&normalized_source, &normalized_target)?;
    reject_reparse_ancestors(target, environment)?;
    if environment.is_reparse_point(&normalized_source)? {
        return Err(AppError::Security(
            "当前数据目录不能是链接或重解析点".into(),
        ));
    }

    let manifest = scan_manifest(&normalized_source, environment)?;
    if target.exists() {
        let target_manifest = scan_manifest(target, environment)?;
        if !target_manifest.entries.is_empty() {
            if !retryable {
                return Err(AppError::Validation(
                    "目标目录不是空目录，不能覆盖其中的数据".into(),
                ));
            }
            validate_retry_target(&manifest, &target_manifest)?;
        }
    }
    fs::create_dir_all(target)?;
    if environment.is_reparse_point(target)? {
        return Err(AppError::Security(
            "目标数据目录不能是链接或重解析点".into(),
        ));
    }
    environment.probe_writable(target)?;
    let available_bytes = environment.available_space(target)?;
    let reserve = std::cmp::max(MINIMUM_SPACE_RESERVE, manifest.total_bytes / 10);
    let required_bytes = manifest.total_bytes.saturating_add(reserve);
    if available_bytes < required_bytes {
        return Err(AppError::DiskSpace(format!(
            "目标磁盘至少需要 {required_bytes} 字节，当前仅有 {available_bytes} 字节可用"
        )));
    }

    Ok((
        DataMigrationPreview {
            preview_id: Uuid::new_v4(),
            confirmation_token: Uuid::new_v4(),
            expires_at: now.saturating_add(PREVIEW_TTL_MILLIS),
            source: normalized_source,
            target: fs::canonicalize(target)?,
            file_count: manifest.file_count,
            total_bytes: manifest.total_bytes,
            required_bytes,
            available_bytes,
            old_root_will_be_kept: true,
            retryable,
        },
        manifest,
    ))
}

fn validate_retry_target(
    source: &DataMigrationManifest,
    target: &DataMigrationManifest,
) -> AppResult<()> {
    for existing in &target.entries {
        let Some(expected) = source
            .entries
            .iter()
            .find(|candidate| candidate.relative_path == existing.relative_path)
        else {
            return Err(AppError::Security(
                "失败迁移目标中存在不属于源数据的文件".into(),
            ));
        };
        if expected.kind != existing.kind {
            return Err(AppError::Security(
                "失败迁移目标中的文件类型发生变化".into(),
            ));
        }
    }
    Ok(())
}

pub fn scan_manifest<E: MigrationEnvironment>(
    source: &Path,
    environment: &E,
) -> AppResult<DataMigrationManifest> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if !ALLOWED_ROOT_ENTRIES.contains(&name_text.as_ref()) {
            return Err(AppError::Security(format!(
                "数据目录包含未声明的根级项目：{name_text}"
            )));
        }
        if name_text == MIGRATION_JOURNAL_FILE || name_text == MIGRATION_COMPLETE_FILE {
            continue;
        }
        scan_entry(source, &entry.path(), environment, &mut entries)?;
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let file_count = entries
        .iter()
        .filter(|entry| entry.kind == ManifestEntryKind::File)
        .count() as u64;
    let total_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    Ok(DataMigrationManifest {
        entries,
        file_count,
        total_bytes,
    })
}

fn scan_entry<E: MigrationEnvironment>(
    root: &Path,
    path: &Path,
    environment: &E,
    entries: &mut Vec<DataMigrationManifestEntry>,
) -> AppResult<()> {
    if environment.is_reparse_point(path)? {
        return Err(AppError::Security(format!(
            "数据目录包含链接或重解析项：{}",
            path.strip_prefix(root).unwrap_or(path).display()
        )));
    }
    let metadata = fs::symlink_metadata(path)?;
    let relative_path = path
        .strip_prefix(root)
        .map_err(|_| AppError::Security("数据清单路径越界".into()))?
        .to_path_buf();
    if metadata.is_dir() {
        entries.push(DataMigrationManifestEntry {
            relative_path,
            kind: ManifestEntryKind::Directory,
            size_bytes: 0,
            sha256: None,
        });
        for child in fs::read_dir(path)? {
            scan_entry(root, &child?.path(), environment, entries)?;
        }
    } else if metadata.is_file() {
        let (size_bytes, sha256) = hash_file(path)?;
        entries.push(DataMigrationManifestEntry {
            relative_path,
            kind: ManifestEntryKind::File,
            size_bytes,
            sha256: Some(sha256),
        });
    } else {
        return Err(AppError::Security("数据目录包含不受支持的文件类型".into()));
    }
    Ok(())
}

pub fn hash_file(path: &Path) -> AppResult<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn validate_absolute_non_root(path: &Path, label: &str) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::Validation(format!("{label}必须是绝对路径")));
    }
    if path.parent().is_none() {
        return Err(AppError::Validation(format!("{label}不能是盘符根目录")));
    }
    Ok(())
}

fn reject_related_paths(source: &Path, target: &Path) -> AppResult<()> {
    if source == target {
        return Err(AppError::Validation(
            "目标目录不能与当前数据目录相同".into(),
        ));
    }
    if target.starts_with(source) || source.starts_with(target) {
        return Err(AppError::Validation(
            "目标目录不能是当前数据目录的父目录或子目录".into(),
        ));
    }
    Ok(())
}

fn normalize_future_path(path: &Path) -> AppResult<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).map_err(Into::into);
    }
    let mut cursor = path;
    let mut suffix = Vec::<OsString>::new();
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or_else(|| AppError::Validation("无法解析目标数据目录".into()))?;
        suffix.push(name.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| AppError::Validation("无法解析目标数据目录".into()))?;
    }
    let mut normalized = fs::canonicalize(cursor)?;
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

fn reject_reparse_ancestors<E: MigrationEnvironment>(
    target: &Path,
    environment: &E,
) -> AppResult<()> {
    let mut cursor = Some(target);
    while let Some(path) = cursor {
        if path.exists() && environment.is_reparse_point(path)? {
            return Err(AppError::Security(
                "目标目录或其上级目录不能是链接或重解析点".into(),
            ));
        }
        cursor = path.parent();
    }
    Ok(())
}
