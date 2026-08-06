pub mod copy;
pub mod journal;
pub mod model;
pub mod preflight;
pub mod worker;

pub use copy::{copy_and_verify, verify_manifest, VerifiedMigration};
pub use journal::{DataMigrationJournal, DataMigrationPhase, MigrationJournalStore};
pub use model::{
    DataMigrationManifest, DataMigrationManifestEntry, DataMigrationPreview, ManifestEntryKind,
    ALLOWED_ROOT_ENTRIES, MIGRATION_COMPLETE_FILE, MIGRATION_JOURNAL_FILE,
};
pub use preflight::{
    hash_file, preflight_data_root_migration, preflight_retry_data_root_migration,
    preflight_retry_with, preflight_with, scan_manifest, MigrationEnvironment,
    SystemMigrationEnvironment,
};
pub use worker::{
    completion_marker_is_valid, run_process_mode, run_system_worker, run_worker_with,
    DataRootPointer, MigrationCompleteMarker, ParentProcessWaiter, ProcessLauncher,
    RuntimeDataRootPointer, SystemParentProcessWaiter, SystemProcessLauncher,
};
