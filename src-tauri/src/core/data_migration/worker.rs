use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};

use crate::{
    core::{
        data_migration::{
            copy_and_verify, DataMigrationJournal, DataMigrationPhase, MigrationJournalStore,
            SystemMigrationEnvironment, VerifiedMigration, MIGRATION_COMPLETE_FILE,
            MIGRATION_JOURNAL_FILE,
        },
        data_root::DataRootSource,
        portable_root, root_registry,
    },
    domain::execution::now_millis,
    error::{AppError, AppResult},
};

const INTERNAL_MIGRATION_FLAG: &str = "--migrate-data-root";

pub trait DataRootPointer {
    fn commit(&self, verified: &VerifiedMigration) -> AppResult<()>;
}

pub trait ParentProcessWaiter {
    fn wait_for_exit(&self, parent_pid: u32) -> AppResult<()>;
}

pub trait ProcessLauncher {
    fn restart(&self, executable: &Path) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub enum RuntimeDataRootPointer {
    Registry,
    Portable {
        pointer_path: PathBuf,
        default_root: PathBuf,
    },
}

impl DataRootPointer for RuntimeDataRootPointer {
    fn commit(&self, verified: &VerifiedMigration) -> AppResult<()> {
        match self {
            Self::Registry => root_registry::save_data_root(verified.target()),
            Self::Portable {
                pointer_path,
                default_root,
            } => {
                if same_path(verified.target(), default_root) {
                    portable_root::clear(pointer_path)
                } else {
                    portable_root::save(pointer_path, verified.target())
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemParentProcessWaiter;

impl ParentProcessWaiter for SystemParentProcessWaiter {
    fn wait_for_exit(&self, parent_pid: u32) -> AppResult<()> {
        if parent_pid == 0 || parent_pid == std::process::id() {
            return Err(AppError::Security("数据迁移父进程标识无效".into()));
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::{
                Foundation::{CloseHandle, WAIT_OBJECT_0},
                System::Threading::{OpenProcess, WaitForSingleObject, INFINITE},
            };
            const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
            let handle = OpenProcess(SYNCHRONIZE_ACCESS, 0, parent_pid);
            if handle.is_null() {
                return Err(AppError::Io(std::io::Error::last_os_error()));
            }
            let result = WaitForSingleObject(handle, INFINITE);
            CloseHandle(handle);
            if result != WAIT_OBJECT_0 {
                return Err(AppError::Io(std::io::Error::last_os_error()));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        Err(AppError::Compatibility(
            "数据目录迁移工作器目前仅支持 Windows".into(),
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcessLauncher;

impl ProcessLauncher for SystemProcessLauncher {
    fn restart(&self, executable: &Path) -> AppResult<()> {
        Command::new(executable).spawn()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationCompleteMarker {
    pub migration_id: uuid::Uuid,
    pub previous_root: PathBuf,
    pub file_count: u64,
    pub total_bytes: u64,
    pub completed_at: i64,
}

pub fn run_process_mode<I, S>(args: I) -> AppResult<bool>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    if args.get(1).and_then(|value| value.to_str()) != Some(INTERNAL_MIGRATION_FLAG) {
        return Ok(false);
    }
    if args.len() != 3 {
        return Err(AppError::Security("数据迁移内部参数无效".into()));
    }
    let journal_path = PathBuf::from(&args[2]);
    if !journal_path.is_absolute() {
        return Err(AppError::Security("数据迁移日志必须是绝对路径".into()));
    }
    match run_system_worker(&journal_path) {
        Ok(_) => Ok(true),
        Err(error) => {
            let executable = std::env::current_exe()?;
            let _ = Command::new(executable).spawn();
            Err(error)
        }
    }
}

pub fn run_system_worker(journal_path: &Path) -> AppResult<DataMigrationPhase> {
    let executable = std::env::current_exe()?;
    let journal = validate_worker_journal(journal_path)?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| AppError::Validation("无法确定程序目录".into()))?;
    let pointer = match journal.source_mode {
        DataRootSource::Registry => RuntimeDataRootPointer::Registry,
        DataRootSource::PortableCustom | DataRootSource::PortableDefault => {
            RuntimeDataRootPointer::Portable {
                pointer_path: executable_directory.join("data-root.json"),
                default_root: executable_directory.join("data"),
            }
        }
        DataRootSource::Environment | DataRootSource::NeedsSelection => {
            return Err(AppError::Security("当前数据目录来源不允许迁移".into()))
        }
    };
    run_worker_with(
        journal_path,
        &executable,
        &pointer,
        &SystemParentProcessWaiter,
        &SystemProcessLauncher,
        &SystemMigrationEnvironment,
        |_| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_worker_with<P, W, L, E, F>(
    journal_path: &Path,
    executable: &Path,
    pointer: &P,
    waiter: &W,
    launcher: &L,
    environment: &E,
    before_verify: F,
) -> AppResult<DataMigrationPhase>
where
    P: DataRootPointer,
    W: ParentProcessWaiter,
    L: ProcessLauncher,
    E: crate::core::data_migration::MigrationEnvironment,
    F: FnOnce(&Path) -> AppResult<()>,
{
    let mut journal = validate_worker_journal(journal_path)?;
    let store = MigrationJournalStore::new(&journal.source, &journal.target);
    let work = (|| -> AppResult<()> {
        waiter.wait_for_exit(journal.parent_pid)?;
        let (preview, manifest) = crate::core::data_migration::preflight_with(
            &journal.source,
            &journal.target,
            now_millis(),
            environment,
        )
        .or_else(|_| {
            crate::core::data_migration::preflight_retry_with(
                &journal.source,
                &journal.target,
                now_millis(),
                environment,
            )
        })?;
        journal.file_count = manifest.file_count;
        journal.total_bytes = manifest.total_bytes;
        journal.copied_files = 0;
        journal.copied_bytes = 0;
        journal.updated_at = now_millis();
        store.save(&journal)?;
        let verified = copy_and_verify(
            &preview.source,
            &preview.target,
            &manifest,
            preview.retryable,
            &mut journal,
            &store,
            environment,
            before_verify,
        )?;
        write_complete_marker(&verified)?;
        pointer.commit(&verified)?;
        journal.transition(DataMigrationPhase::Switched, now_millis());
        store.save(&journal)?;
        journal.transition(DataMigrationPhase::Completed, now_millis());
        store.save(&journal)?;
        Ok(())
    })();

    let phase = match work {
        Ok(()) => DataMigrationPhase::Completed,
        Err(error) => {
            journal.fail(&error, now_millis());
            let _ = store.save(&journal);
            DataMigrationPhase::Failed
        }
    };
    launcher.restart(executable)?;
    Ok(phase)
}

pub fn validate_worker_journal(journal_path: &Path) -> AppResult<DataMigrationJournal> {
    if !journal_path.is_absolute()
        || journal_path.file_name() != Some(OsStr::new(MIGRATION_JOURNAL_FILE))
    {
        return Err(AppError::Security("数据迁移日志路径无效".into()));
    }
    let journal = MigrationJournalStore::load(journal_path)?;
    let parent = journal_path
        .parent()
        .ok_or_else(|| AppError::Security("数据迁移日志没有父目录".into()))?;
    if !same_path(parent, &journal.source) || journal.phase != DataMigrationPhase::Prepared {
        return Err(AppError::Security(
            "数据迁移日志不属于当前源目录或状态无效".into(),
        ));
    }
    if journal.parent_pid == 0 || journal.parent_pid == std::process::id() {
        return Err(AppError::Security("数据迁移父进程标识无效".into()));
    }
    Ok(journal)
}

pub fn completion_marker_is_valid(root: &Path, journal: &DataMigrationJournal) -> bool {
    let payload = match std::fs::read(root.join(MIGRATION_COMPLETE_FILE)) {
        Ok(payload) => payload,
        Err(_) => return false,
    };
    let marker: MigrationCompleteMarker = match serde_json::from_slice(&payload) {
        Ok(marker) => marker,
        Err(_) => return false,
    };
    marker.migration_id == journal.migration_id && same_path(&marker.previous_root, &journal.source)
}

fn write_complete_marker(verified: &VerifiedMigration) -> AppResult<()> {
    let marker = MigrationCompleteMarker {
        migration_id: verified.migration_id(),
        previous_root: verified.source().to_path_buf(),
        file_count: verified.file_count(),
        total_bytes: verified.total_bytes(),
        completed_at: now_millis(),
    };
    let payload = serde_json::to_vec_pretty(&marker)
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    AtomicFile::new(
        verified.target().join(MIGRATION_COMPLETE_FILE),
        AllowOverwrite,
    )
    .write(|file| {
        use std::io::Write;
        file.write_all(&payload)?;
        file.sync_all()
    })
    .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}
