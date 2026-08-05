use std::{
    collections::BTreeMap,
    net::IpAddr,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        sftp::{
            backup_operation_remote_file, sha256_local_file, upload, validate_remote_path,
            OperationFileBackup, RemoteFileMetadata, UploadRequest,
        },
        ssh::executor::VecEventSink,
        ssh::transport::execute_authenticated,
        tasks::{
            elevate_fixed_command, probe_privilege, render_backup_target,
            resolve_task_restore_path, shell_quote, task_restore_dir,
            task_restore_item_relative_path, write_restore_asset_atomic, BackupItemKind,
            BackupPlan, PrivilegeMode, ValidatedParameters,
        },
    },
    domain::execution::now_millis,
    domain::operation_restore::{
        NewOperationRestoreItem, NewOperationRestorePoint, OperationRestoreDetails,
        OperationRestoreItem, OperationRestoreItemStatus, OperationRestorePointStatus,
    },
    error::{AppError, AppResult},
    repositories::operation_restore_repository::OperationRestoreRepository,
    services::{
        remote_recovery_service::RemoteRecoveryService,
        server_connector::{ConnectedServer, ServerConnector},
    },
};

#[derive(Clone)]
pub struct OperationRestoreService {
    data_root: PathBuf,
    repository: OperationRestoreRepository,
    connector: ServerConnector,
}

impl OperationRestoreService {
    pub fn new(
        data_root: PathBuf,
        repository: OperationRestoreRepository,
        connector: ServerConnector,
    ) -> Self {
        Self {
            data_root,
            repository,
            connector,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn capture(
        &self,
        run_id: Uuid,
        server_id: &str,
        task_id: &str,
        implementation_id: &str,
        backup_plan: &BackupPlan,
        parameters: &ValidatedParameters,
        cancel: CancellationToken,
    ) -> AppResult<OperationRestoreDetails> {
        if backup_plan.items.is_empty() {
            return Err(AppError::Validation("危险任务没有声明恢复项".into()));
        }
        let local_relative_dir = task_restore_dir(run_id)
            .to_string_lossy()
            .replace('\\', "/");
        let creating = self
            .repository
            .create(NewOperationRestorePoint {
                operation_run_id: run_id,
                server_id: server_id.into(),
                task_id: task_id.into(),
                local_relative_dir,
                remote_asset_id: None,
                expires_at: None,
            })
            .await?;
        if cancel.is_cancelled() {
            self.repository.mark_creation_failed(creating.id).await?;
            return Err(AppError::Cancelled);
        }
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.repository.mark_creation_failed(creating.id).await?;
                return Err(error);
            }
        };
        let privilege_mode = match probe_privilege(&connected.session).await {
            Ok(mode) => mode,
            Err(error) => {
                connected.session.disconnect().await;
                self.repository.mark_creation_failed(creating.id).await?;
                return Err(error);
            }
        };
        let result = self
            .capture_connected(
                creating.id,
                run_id,
                task_id,
                implementation_id,
                backup_plan,
                parameters,
                &connected,
                privilege_mode,
                cancel,
            )
            .await;
        connected.session.disconnect().await;
        if let Err(error) = result {
            let _ = self.repository.mark_creation_failed(creating.id).await;
            return Err(error);
        }
        if let Err(error) = self.repository.mark_available(creating.id).await {
            let _ = self.repository.mark_creation_failed(creating.id).await;
            return Err(error);
        }
        self.repository
            .get(creating.id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationRestoreDetails>> {
        self.repository.get(id).await
    }

    pub async fn list_by_run(&self, run_id: Uuid) -> AppResult<Vec<OperationRestoreDetails>> {
        self.repository.list_by_run(run_id).await
    }

    pub async fn cleanup_assets(
        &self,
        restore_point_id: Uuid,
    ) -> AppResult<OperationRestoreDetails> {
        let details = self
            .repository
            .get(restore_point_id)
            .await?
            .ok_or_else(|| AppError::Validation("恢复点不存在".into()))?;
        if details.point.status != OperationRestorePointStatus::CleanupPending {
            self.repository
                .begin_cleanup(restore_point_id, now_millis())
                .await?;
        }

        let expected_relative = task_restore_dir(details.point.operation_run_id)
            .to_string_lossy()
            .replace('\\', "/");
        if details.point.local_relative_dir != expected_relative {
            return Err(AppError::Security("恢复资产目录与运维运行不匹配".into()));
        }

        if let Some(remote_asset_id) = details.point.remote_asset_id.as_deref() {
            let expected_remote = format!("qingzhou-recovery/{}", details.point.operation_run_id);
            if remote_asset_id != expected_remote {
                return Err(AppError::Security(
                    "远程恢复资产标识与运维运行不匹配".into(),
                ));
            }
            RemoteRecoveryService::new(self.connector.clone())
                .cleanup_operation_assets(&details.point.server_id, details.point.operation_run_id)
                .await?;
        }

        let local_dir = resolve_task_restore_path(
            &self.data_root,
            PathBuf::from(&details.point.local_relative_dir).as_path(),
        )?;
        match tokio::fs::symlink_metadata(&local_dir).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(AppError::Security("恢复资产目录类型无效".into()));
                }
                tokio::fs::remove_dir_all(&local_dir).await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.repository.finish_cleanup(restore_point_id).await?;
        self.repository
            .get(restore_point_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_remote_rollback_observed(&self, run_id: Uuid) -> AppResult<()> {
        let points = self.repository.list_by_run(run_id).await?;
        let point = points
            .into_iter()
            .find(|details| details.point.status == OperationRestorePointStatus::Available)
            .ok_or_else(|| AppError::Integrity("远程自动回滚缺少可用恢复点".into()))?;
        self.repository
            .mark_remote_rollback_observed(point.point.id)
            .await
    }

    pub async fn attach_remote_asset(
        &self,
        restore_point_id: Uuid,
        remote_asset_id: &str,
        expires_at: i64,
    ) -> AppResult<()> {
        self.repository
            .attach_remote_asset(restore_point_id, remote_asset_id, expires_at)
            .await
    }

    pub async fn rollback(
        &self,
        restore_point_id: Uuid,
        cancel: CancellationToken,
    ) -> AppResult<OperationRestoreDetails> {
        let details = self
            .repository
            .get(restore_point_id)
            .await?
            .ok_or_else(|| AppError::Validation("运维恢复点不存在".into()))?;
        self.repository.begin_rollback(restore_point_id).await?;
        let connected = match self.connector.connect(&details.point.server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.repository
                    .finish_rollback(
                        restore_point_id,
                        OperationRestorePointStatus::Failed,
                        Some(error.to_string()),
                    )
                    .await?;
                return Err(error);
            }
        };
        let privilege_mode = match probe_privilege(&connected.session).await {
            Ok(mode) => mode,
            Err(error) => {
                connected.session.disconnect().await;
                self.repository
                    .finish_rollback(
                        restore_point_id,
                        OperationRestorePointStatus::Failed,
                        Some(error.to_string()),
                    )
                    .await?;
                return Err(error);
            }
        };

        let mut succeeded = 0_usize;
        let mut failures = Vec::new();
        let mut uncertain_failure = None;
        for item in details.items.iter().rev() {
            if matches!(
                item.status,
                OperationRestoreItemStatus::RolledBack | OperationRestoreItemStatus::Skipped
            ) {
                continue;
            }
            if cancel.is_cancelled() {
                failures.push("用户已取消剩余回滚步骤".to_string());
                break;
            }
            self.repository.mark_item_rolling_back(item.id).await?;
            match self
                .rollback_item(
                    &details.point.task_id,
                    item,
                    &connected,
                    privilege_mode,
                    cancel.child_token(),
                )
                .await
            {
                Ok(()) => {
                    succeeded = succeeded.saturating_add(1);
                    self.repository
                        .finish_item_rollback(item.id, OperationRestoreItemStatus::RolledBack, None)
                        .await?;
                }
                Err(error) => {
                    failures.push(format!("{}：{}", item.ordinal, error));
                    self.repository
                        .finish_item_rollback(
                            item.id,
                            OperationRestoreItemStatus::Failed,
                            Some(error.to_string()),
                        )
                        .await?;
                    if is_connection_uncertain(&error) {
                        uncertain_failure = Some(error.to_string());
                        break;
                    }
                }
            }
        }
        connected.session.disconnect().await;
        let status = if failures.is_empty() {
            OperationRestorePointStatus::RolledBack
        } else if succeeded == 0 {
            OperationRestorePointStatus::Failed
        } else {
            OperationRestorePointStatus::Partial
        };
        self.repository
            .finish_rollback(
                restore_point_id,
                status,
                (!failures.is_empty()).then(|| failures.join("；")),
            )
            .await?;
        if let Some(message) = uncertain_failure {
            return Err(AppError::RemoteStateUncertain(format!(
                "自动回滚期间连接中断：{message}"
            )));
        }
        self.repository
            .get(restore_point_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    #[allow(clippy::too_many_arguments)]
    async fn capture_connected(
        &self,
        restore_point_id: Uuid,
        run_id: Uuid,
        task_id: &str,
        implementation_id: &str,
        backup_plan: &BackupPlan,
        parameters: &ValidatedParameters,
        connected: &ConnectedServer,
        privilege_mode: PrivilegeMode,
        cancel: CancellationToken,
    ) -> AppResult<()> {
        for (ordinal, definition) in backup_plan.items.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let target =
                render_backup_target(&definition.target_template, implementation_id, parameters)?;
            let relative = task_restore_item_relative_path(run_id, ordinal, &target)?;
            let (local_relative_path, sha256, original_metadata) = match definition.kind {
                BackupItemKind::RemoteFile | BackupItemKind::ManagedBlock => {
                    if definition.kind == BackupItemKind::ManagedBlock
                        && (task_id != "service.cron_manage"
                            || target != "/etc/cron.d/qingzhou-managed")
                    {
                        return Err(AppError::Security("受控 Cron 恢复目标无效".into()));
                    }
                    let backup: OperationFileBackup = backup_operation_remote_file(
                        &connected.session,
                        &self.data_root,
                        &target,
                        &relative,
                        cancel.child_token(),
                    )
                    .await?;
                    let metadata: Option<&RemoteFileMetadata> = backup.metadata.as_ref();
                    match backup.transfer {
                        Some(transfer) => {
                            let complete = metadata.is_some_and(|value| {
                                value.uid.is_some()
                                    && value.gid.is_some()
                                    && value.permissions.is_some()
                            });
                            if !complete {
                                if let Ok(path) = resolve_task_restore_path(
                                    &self.data_root,
                                    PathBuf::from(&transfer.location).as_path(),
                                ) {
                                    let _ = tokio::fs::remove_file(path).await;
                                }
                                return Err(AppError::Integrity(
                                    "远程文件备份缺少属主或权限元数据".into(),
                                ));
                            }
                            (
                                Some(transfer.location),
                                Some(transfer.sha256),
                                json!({
                                    "originalExisted": true,
                                    "size": metadata.and_then(|value| value.size),
                                    "uid": metadata.and_then(|value| value.uid),
                                    "gid": metadata.and_then(|value| value.gid),
                                    "permissions": metadata.and_then(|value| value.permissions),
                                    "modifiedAt": metadata.and_then(|value| value.modified_at),
                                }),
                            )
                        }
                        None => (None, None, json!({"originalExisted": false})),
                    }
                }
                BackupItemKind::CommandSnapshot | BackupItemKind::RuntimeState => {
                    let command =
                        privileged_backup_command(definition.kind, &target, privilege_mode)?
                            .ok_or_else(|| {
                                AppError::Validation("命令快照没有可执行的备份命令".into())
                            })?;
                    let output = execute_authenticated(&connected.session, &command).await?;
                    if output.exit_status != 0 {
                        return Err(AppError::ssh_command(
                            output.exit_status,
                            connected.redactor.redact(&output.stderr),
                        ));
                    }
                    let snapshot = format!(
                        "stdout:\n{}\nstderr:\n{}",
                        connected.redactor.redact(&output.stdout),
                        connected.redactor.redact(&output.stderr)
                    );
                    if is_snapshot_rollback_task(task_id) {
                        build_snapshot_rollback_command(task_id, &snapshot)?;
                    }
                    let asset =
                        write_restore_asset_atomic(&self.data_root, &relative, snapshot.as_bytes())
                            .await?;
                    (
                        Some(asset.relative_path),
                        Some(asset.sha256),
                        json!({
                            "originalExisted": true,
                            "exitStatus": output.exit_status,
                            "bytes": asset.bytes,
                        }),
                    )
                }
            };
            self.repository
                .add_item(NewOperationRestoreItem {
                    restore_point_id,
                    ordinal,
                    item_kind: definition.kind,
                    remote_target: target,
                    local_relative_path,
                    sha256,
                    original_metadata,
                    status: OperationRestoreItemStatus::Available,
                    error_summary: None,
                })
                .await?;
        }
        Ok(())
    }

    async fn rollback_item(
        &self,
        task_id: &str,
        item: &OperationRestoreItem,
        connected: &ConnectedServer,
        privilege_mode: PrivilegeMode,
        cancel: CancellationToken,
    ) -> AppResult<()> {
        match item.item_kind {
            BackupItemKind::CommandSnapshot | BackupItemKind::RuntimeState => {
                let snapshot = self.read_verified_asset(item).await?;
                let rollback = build_snapshot_rollback_command(task_id, &snapshot)?;
                execute_checked(
                    connected,
                    &elevate_fixed_command(&rollback.command, privilege_mode)?,
                )
                .await?;
                execute_checked(
                    connected,
                    &elevate_fixed_command(&rollback.verify, privilege_mode)?,
                )
                .await
            }
            BackupItemKind::RemoteFile => {
                self.restore_remote_file(item, connected, privilege_mode, cancel)
                    .await
            }
            BackupItemKind::ManagedBlock => {
                if task_id != "service.cron_manage"
                    || item.remote_target != "/etc/cron.d/qingzhou-managed"
                {
                    return Err(AppError::Security("受控 Cron 恢复目标无效".into()));
                }
                self.restore_remote_file(item, connected, privilege_mode, cancel)
                    .await
            }
        }
    }

    async fn read_verified_asset(&self, item: &OperationRestoreItem) -> AppResult<String> {
        let relative = item
            .local_relative_path
            .as_deref()
            .ok_or_else(|| AppError::Integrity("恢复项缺少本地资产路径".into()))?;
        let expected = item
            .sha256
            .as_deref()
            .ok_or_else(|| AppError::Integrity("恢复项缺少 SHA-256".into()))?;
        let path = resolve_task_restore_path(&self.data_root, PathBuf::from(relative).as_path())?;
        verify_local_restore_asset(&self.data_root, &path, expected).await?;
        let bytes = tokio::fs::read(path).await?;
        if bytes.len() > 1024 * 1024 {
            return Err(AppError::Integrity("恢复项快照超过 1 MiB 安全上限".into()));
        }
        String::from_utf8(bytes).map_err(|_| AppError::Integrity("恢复项快照不是有效 UTF-8".into()))
    }

    async fn restore_remote_file(
        &self,
        item: &OperationRestoreItem,
        connected: &ConnectedServer,
        privilege_mode: PrivilegeMode,
        cancel: CancellationToken,
    ) -> AppResult<()> {
        validate_remote_path(&item.remote_target)?;
        let target = shell_quote(&item.remote_target);
        let original_existed = item
            .original_metadata
            .get("originalExisted")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| AppError::Integrity("恢复项缺少原文件存在状态".into()))?;
        if !original_existed {
            let command = format!(
                "if test -L {target}; then exit 65; fi; rm -f -- {target}; test ! -e {target}"
            );
            return execute_checked(connected, &elevate_fixed_command(&command, privilege_mode)?)
                .await;
        }

        let relative = item
            .local_relative_path
            .as_deref()
            .ok_or_else(|| AppError::Integrity("文件恢复项缺少本地资产路径".into()))?;
        let expected = item
            .sha256
            .as_deref()
            .ok_or_else(|| AppError::Integrity("文件恢复项缺少 SHA-256".into()))?;
        let local_path =
            resolve_task_restore_path(&self.data_root, PathBuf::from(relative).as_path())?;
        verify_local_restore_asset(&self.data_root, &local_path, expected).await?;
        let uid = item
            .original_metadata
            .get("uid")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| AppError::Integrity("文件恢复项缺少原 UID".into()))?;
        let gid = item
            .original_metadata
            .get("gid")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| AppError::Integrity("文件恢复项缺少原 GID".into()))?;
        let permissions = item
            .original_metadata
            .get("permissions")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| AppError::Integrity("文件恢复项缺少原权限".into()))?
            & 0o7777;
        if uid > u64::from(u32::MAX) || gid > u64::from(u32::MAX) {
            return Err(AppError::Integrity("恢复项文件属主超出安全范围".into()));
        }
        let staging = format!("/tmp/.qingzhou-restore-{}", Uuid::new_v4());
        let mut events = VecEventSink::default();
        upload(
            &connected.session,
            &UploadRequest {
                local_path,
                remote_path: staging.clone(),
                overwrite: false,
            },
            &mut events,
            cancel,
        )
        .await?;
        let staging_quoted = shell_quote(&staging);
        let target_temp = shell_quote(&format!("{}.qingzhou.XXXXXX", item.remote_target));
        let command = format!(
            "tmp=''; cleanup() {{ test -z \"$tmp\" || rm -f -- \"$tmp\"; rm -f -- {staging_quoted}; }}; trap cleanup EXIT HUP INT TERM; test ! -L {target} && tmp=$(mktemp {target_temp}) && cat -- {staging_quoted} > \"$tmp\" && chown -- {uid}:{gid} \"$tmp\" && chmod -- {permissions:o} \"$tmp\" && mv -f -- \"$tmp\" {target} && tmp=''; result=$?; cleanup; trap - EXIT HUP INT TERM; exit $result"
        );
        let result =
            execute_checked(connected, &elevate_fixed_command(&command, privilege_mode)?).await;
        if result.is_err() {
            let _ =
                execute_authenticated(&connected.session, &format!("rm -f -- {staging_quoted}"))
                    .await;
            return result;
        }
        let verify = format!(
            "test ! -L {target} && set -- $(sha256sum -- {target}) && test \"$1\" = {}",
            shell_quote(expected)
        );
        execute_checked(connected, &elevate_fixed_command(&verify, privilege_mode)?).await
    }
}

async fn execute_checked(connected: &ConnectedServer, command: &str) -> AppResult<()> {
    let output = execute_authenticated(&connected.session, command).await?;
    if output.exit_status == 0 {
        Ok(())
    } else {
        Err(AppError::ssh_command(
            output.exit_status,
            connected.redactor.redact(&output.stderr),
        ))
    }
}

async fn verify_local_restore_asset(
    data_root: &Path,
    path: &Path,
    expected_sha256: &str,
) -> AppResult<()> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| AppError::Integrity("恢复项本地资产不存在".into()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Security("恢复项本地资产不是普通文件".into()));
    }
    let canonical_root = tokio::fs::canonicalize(data_root).await?;
    let canonical_path = tokio::fs::canonicalize(path).await?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(AppError::Security("恢复项本地资产逃逸项目数据目录".into()));
    }
    if sha256_local_file(path).await? != expected_sha256 {
        return Err(AppError::Integrity("恢复项本地资产 SHA-256 不一致".into()));
    }
    Ok(())
}

fn is_snapshot_rollback_task(task_id: &str) -> bool {
    matches!(
        task_id,
        "system.hostname_change"
            | "system.timezone_change"
            | "storage.swap_manage"
            | "security.file_permissions"
            | "service.start"
            | "service.stop"
            | "service.restart"
            | "service.boot_policy"
            | "container.action"
            | "security.firewall_open_port"
            | "network.ip_change"
    )
}

fn is_connection_uncertain(error: &AppError) -> bool {
    matches!(
        error,
        AppError::RemoteStateUncertain(_)
            | AppError::Ssh(_)
            | AppError::Io(_)
            | AppError::Transfer(_)
    )
}

pub fn privileged_backup_command(
    kind: BackupItemKind,
    target: &str,
    privilege_mode: PrivilegeMode,
) -> AppResult<Option<String>> {
    match kind {
        BackupItemKind::CommandSnapshot | BackupItemKind::RuntimeState => {
            elevate_fixed_command(target, privilege_mode).map(Some)
        }
        BackupItemKind::RemoteFile | BackupItemKind::ManagedBlock => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRollbackCommand {
    pub command: String,
    pub verify: String,
}

pub fn build_snapshot_rollback_command(
    task_id: &str,
    snapshot: &str,
) -> AppResult<SnapshotRollbackCommand> {
    let values = parse_snapshot_values(snapshot)?;
    match task_id {
        "system.hostname_change" => {
            let hostname = required_snapshot_value(&values, "hostname")?;
            if !is_safe_hostname(hostname) {
                return Err(AppError::Integrity("恢复点中的原主机名无效".into()));
            }
            let hostname = shell_quote(hostname);
            Ok(SnapshotRollbackCommand {
                command: format!("hostnamectl set-hostname -- {hostname}"),
                verify: format!("test \"$(hostname)\" = {hostname}"),
            })
        }
        "system.timezone_change" => {
            let timezone = required_snapshot_value(&values, "timezone")?;
            if !is_safe_timezone(timezone) {
                return Err(AppError::Integrity("恢复点中的原时区无效".into()));
            }
            let timezone = shell_quote(timezone);
            Ok(SnapshotRollbackCommand {
                command: format!("timedatectl set-timezone -- {timezone}"),
                verify: format!("test \"$(timedatectl show -p Timezone --value)\" = {timezone}"),
            })
        }
        "security.file_permissions" => {
            let path = required_snapshot_value(&values, "path")?;
            if !is_safe_permissions_path(path) {
                return Err(AppError::Integrity("恢复点中的权限目标路径无效".into()));
            }
            let uid = parse_bounded_u32(&values, "uid", 60_000)?;
            let gid = parse_bounded_u32(&values, "gid", 60_000)?;
            let mode = required_snapshot_value(&values, "mode")?;
            if !(3..=4).contains(&mode.len())
                || !mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
            {
                return Err(AppError::Integrity("恢复点中的原文件权限无效".into()));
            }
            let command_mode = if mode.len() == 3 {
                format!("0{mode}")
            } else {
                mode.into()
            };
            let path = shell_quote(path);
            Ok(SnapshotRollbackCommand {
                command: format!(
                    "test ! -L {path} && chown -- {uid}:{gid} {path} && chmod -- {command_mode} {path}"
                ),
                verify: format!(
                    "test ! -L {path} && test \"$(stat -Lc '%u:%g:%a' -- {path})\" = '{uid}:{gid}:{mode}'"
                ),
            })
        }
        "storage.swap_manage" => build_swap_rollback(&values),
        "service.start" | "service.stop" | "service.restart" => {
            build_service_action_rollback(&values)
        }
        "service.boot_policy" => build_service_policy_rollback(&values),
        "container.action" => build_container_rollback(&values),
        "security.firewall_open_port" => build_firewall_rollback(&values),
        "network.ip_change" => {
            if values.contains_key("backend") {
                build_network_manager_rollback(&values)
            } else {
                build_network_rollback(&values)
            }
        }
        _ => Err(AppError::Validation("该任务尚未实现受控快照回滚".into())),
    }
}

fn build_network_manager_rollback(
    values: &BTreeMap<String, String>,
) -> AppResult<SnapshotRollbackCommand> {
    if required_snapshot_value(values, "backend")? != "networkmanager" {
        return Err(AppError::Integrity("恢复点中的网络管理后端无效".into()));
    }
    let interface = required_snapshot_value(values, "interface")?;
    if interface.is_empty()
        || interface.len() > 32
        || !interface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
    {
        return Err(AppError::Integrity("恢复点中的网络接口名称无效".into()));
    }
    let connection = decode_snapshot_base64(values, "connectionb", false)?;
    let fields = [
        ("ipv4.method", "ipfourmethodb", false),
        ("ipv4.addresses", "ipfouraddressesb", true),
        ("ipv4.gateway", "ipfourgatewayb", true),
        ("ipv6.method", "ipsixmethodb", false),
        ("ipv6.addresses", "ipsixaddressesb", true),
        ("ipv6.gateway", "ipsixgatewayb", true),
    ]
    .into_iter()
    .map(|(property, key, allow_empty)| {
        Ok((
            property,
            shell_quote(&decode_snapshot_base64(values, key, allow_empty)?),
        ))
    })
    .collect::<AppResult<Vec<_>>>()?;
    let connection = shell_quote(&connection);
    let interface = shell_quote(interface);
    let assignments = fields
        .iter()
        .map(|(property, value)| format!("{property} {value}"))
        .collect::<Vec<_>>()
        .join(" ");
    let checks = fields
        .iter()
        .map(|(property, value)| {
            format!("test \"$(nmcli -g {property} connection show {connection})\" = {value}")
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(SnapshotRollbackCommand {
        command: format!(
            "nmcli connection modify {connection} {assignments}; nmcli device reapply {interface}"
        ),
        verify: checks,
    })
}

fn decode_snapshot_base64(
    values: &BTreeMap<String, String>,
    key: &str,
    allow_empty: bool,
) -> AppResult<String> {
    let encoded = values
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| AppError::Integrity(format!("恢复点缺少字段：{key}")))?;
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| AppError::Integrity(format!("恢复点字段 {key} 编码无效")))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|_| AppError::Integrity(format!("恢复点字段 {key} 文本无效")))?;
    if decoded.len() > 4096
        || decoded.contains(['\0', '\n', '\r'])
        || (!allow_empty && decoded.is_empty())
    {
        return Err(AppError::Integrity(format!("恢复点字段 {key} 内容无效")));
    }
    Ok(decoded)
}

fn build_network_rollback(values: &BTreeMap<String, String>) -> AppResult<SnapshotRollbackCommand> {
    let interface = required_snapshot_value(values, "interface")?;
    if interface.is_empty()
        || interface.len() > 32
        || !interface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
    {
        return Err(AppError::Integrity("恢复点中的网络接口名称无效".into()));
    }
    let addresses = required_snapshot_value(values, "addresses")?;
    let mut parsed_addresses = Vec::new();
    if addresses != "none" {
        for value in addresses.split(',') {
            let (address, prefix) = parse_snapshot_cidr(value)?;
            parsed_addresses.push((address, prefix));
        }
    }
    if parsed_addresses.len() > 32 {
        return Err(AppError::Integrity(
            "恢复点中的原网络地址数量超出上限".into(),
        ));
    }
    let gateway_four = parse_snapshot_gateway(values, "gatewayfour", true)?;
    let gateway_six = parse_snapshot_gateway(values, "gatewaysix", false)?;
    let interface = shell_quote(interface);
    let mut command = format!(
        "ip -4 address flush dev {interface} scope global; ip -6 address flush dev {interface} scope global"
    );
    let mut verify = String::new();
    let mut count_four = 0usize;
    let mut count_six = 0usize;
    for (address, prefix) in &parsed_addresses {
        let family = if address.is_ipv4() {
            count_four += 1;
            "-4"
        } else {
            count_six += 1;
            "-6"
        };
        let cidr = shell_quote(&format!("{address}/{prefix}"));
        command.push_str(&format!("; ip {family} address add {cidr} dev {interface}"));
        verify.push_str(&format!(
            "ip {family} -o address show dev {interface} scope global | awk -v cidr={cidr} '$4 == cidr {{ found=1 }} END {{ exit found ? 0 : 1 }}' && "
        ));
    }
    command.push_str(&format!(
        "; while ip -4 route del default dev {interface} >/dev/null 2>&1; do :; done; while ip -6 route del default dev {interface} >/dev/null 2>&1; do :; done"
    ));
    for (family, gateway) in [("-4", gateway_four), ("-6", gateway_six)] {
        if let Some(gateway) = gateway {
            let gateway = shell_quote(&gateway.to_string());
            command.push_str(&format!(
                "; ip {family} route replace default via {gateway} dev {interface}"
            ));
            verify.push_str(&format!(
                "ip {family} route show default dev {interface} | awk -v gateway={gateway} '$1 == \"default\" {{ for (i=1; i<=NF; i++) if ($i == \"via\" && $(i+1) == gateway) found=1 }} END {{ exit found ? 0 : 1 }}' && "
            ));
        }
    }
    verify.push_str(&format!(
        "test \"$(ip -4 -o address show dev {interface} scope global | awk 'END {{ print NR + 0 }}')\" -eq {count_four} && test \"$(ip -6 -o address show dev {interface} scope global | awk 'END {{ print NR + 0 }}')\" -eq {count_six}"
    ));
    Ok(SnapshotRollbackCommand { command, verify })
}

fn parse_snapshot_cidr(value: &str) -> AppResult<(IpAddr, u8)> {
    let (address, prefix) = value
        .split_once('/')
        .ok_or_else(|| AppError::Integrity("恢复点中的原网络地址无效".into()))?;
    let address = address
        .parse::<IpAddr>()
        .map_err(|_| AppError::Integrity("恢复点中的原网络地址无效".into()))?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| AppError::Integrity("恢复点中的原网络前缀无效".into()))?;
    if prefix > if address.is_ipv4() { 32 } else { 128 } {
        return Err(AppError::Integrity("恢复点中的原网络前缀无效".into()));
    }
    Ok((address, prefix))
}

fn parse_snapshot_gateway(
    values: &BTreeMap<String, String>,
    key: &str,
    ipv4: bool,
) -> AppResult<Option<IpAddr>> {
    let value = required_snapshot_value(values, key)?;
    if value == "none" {
        return Ok(None);
    }
    let gateway = value
        .parse::<IpAddr>()
        .map_err(|_| AppError::Integrity("恢复点中的原默认网关无效".into()))?;
    if gateway.is_ipv4() != ipv4 {
        return Err(AppError::Integrity(
            "恢复点中的原默认网关协议版本无效".into(),
        ));
    }
    Ok(Some(gateway))
}

fn build_firewall_rollback(
    values: &BTreeMap<String, String>,
) -> AppResult<SnapshotRollbackCommand> {
    let backend = required_snapshot_value(values, "backend")?;
    let entry_id = required_snapshot_value(values, "entryid")?;
    let parsed_id = Uuid::parse_str(entry_id)
        .map_err(|_| AppError::Integrity("恢复点中的防火墙规则标识无效".into()))?;
    if parsed_id.is_nil() || parsed_id.hyphenated().to_string() != entry_id.to_ascii_lowercase() {
        return Err(AppError::Integrity("恢复点中的防火墙规则标识无效".into()));
    }
    let port = parse_bounded_u32(values, "port", 65_535)?;
    if port == 0 {
        return Err(AppError::Integrity("恢复点中的防火墙端口无效".into()));
    }
    let protocol = required_snapshot_value(values, "protocol")?;
    if !matches!(protocol, "tcp" | "udp") {
        return Err(AppError::Integrity("恢复点中的防火墙协议无效".into()));
    }
    let present = parse_snapshot_bool(values, "present")?;
    let marker = format!("qingzhou:{entry_id}");

    match backend {
        "firewalld" => {
            let rule =
                format!("0 -p {protocol} --dport {port} -m comment --comment {marker} -j ACCEPT");
            let mutation = if present {
                format!(
                    "test -n \"$qz_owned\" || firewall-cmd --permanent --direct --add-rule ipv4 filter INPUT 0 -p {protocol} --dport {port} -m comment --comment {marker} -j ACCEPT"
                )
            } else {
                format!(
                    "if test -n \"$qz_owned\"; then firewall-cmd --permanent --direct --remove-rule ipv4 filter INPUT 0 -p {protocol} --dport {port} -m comment --comment {marker} -j ACCEPT || exit; fi"
                )
            };
            let expected = if present { &rule } else { "" };
            Ok(SnapshotRollbackCommand {
                command: format!(
                    "qz_marker='{marker}'; qz_rule='{rule}'; qz_owned=$(firewall-cmd --permanent --direct --get-rules ipv4 filter INPUT | grep --fixed-strings -- \"$qz_marker\" || true); test -z \"$qz_owned\" || test \"$qz_owned\" = \"$qz_rule\"; {mutation}; firewall-cmd --reload"
                ),
                verify: format!(
                    "qz_owned=$(firewall-cmd --permanent --direct --get-rules ipv4 filter INPUT | grep --fixed-strings -- '{marker}' || true); test \"$qz_owned\" = '{expected}'"
                ),
            })
        }
        "ufw" => {
            let mutation = if present {
                format!("test -n \"$qz_rows\" || ufw allow '{port}/{protocol}' comment '{marker}'")
            } else {
                "if test -n \"$qz_rows\"; then qz_number=$(printf '%s\\n' \"$qz_rows\" | sed -n 's/^\\[[[:space:]]*\\([0-9][0-9]*\\)\\].*/\\1/p'); test -n \"$qz_number\" && ufw --force delete \"$qz_number\"; fi".into()
            };
            let expected_count = if present { 1 } else { 0 };
            Ok(SnapshotRollbackCommand {
                command: format!(
                    "qz_rows=$(ufw status numbered | awk -v marker='{marker}' 'index($0, marker) {{ print }}'); test \"$(printf '%s\\n' \"$qz_rows\" | awk 'NF {{ count++ }} END {{ print count + 0 }}')\" -le 1; {mutation}"
                ),
                verify: format!(
                    "qz_rows=$(ufw status numbered | awk -v marker='{marker}' 'index($0, marker) {{ print }}'); test \"$(printf '%s\\n' \"$qz_rows\" | awk 'NF {{ count++ }} END {{ print count + 0 }}')\" -eq {expected_count}"
                ),
            })
        }
        "nftables" => {
            let mutation = if present {
                format!(
                    "if test -z \"$qz_rows\"; then nft list table inet qingzhou >/dev/null 2>&1 || nft add table inet qingzhou; nft list chain inet qingzhou input >/dev/null 2>&1 || nft 'add chain inet qingzhou input {{ type filter hook input priority 0; }}'; nft add rule inet qingzhou input {protocol} dport {port} counter accept comment '{marker}'; fi"
                )
            } else {
                "if test -n \"$qz_rows\"; then qz_handle=$(printf '%s\\n' \"$qz_rows\" | awk '{ for (i=1; i<=NF; i++) if ($i == \"handle\") { print $(i+1); exit } }'); test -n \"$qz_handle\" && nft delete rule inet qingzhou input handle \"$qz_handle\"; fi".into()
            };
            let expected_count = if present { 1 } else { 0 };
            Ok(SnapshotRollbackCommand {
                command: format!(
                    "qz_rows=$(nft -a list chain inet qingzhou input 2>/dev/null | awk -v marker='{marker}' 'index($0, marker) {{ print }}'); test \"$(printf '%s\\n' \"$qz_rows\" | awk 'NF {{ count++ }} END {{ print count + 0 }}')\" -le 1; {mutation}"
                ),
                verify: format!(
                    "qz_rows=$(nft -a list chain inet qingzhou input 2>/dev/null | awk -v marker='{marker}' 'index($0, marker) {{ print }}'); test \"$(printf '%s\\n' \"$qz_rows\" | awk 'NF {{ count++ }} END {{ print count + 0 }}')\" -eq {expected_count}"
                ),
            })
        }
        "iptables" => {
            let rule =
                format!("-p {protocol} --dport {port} -m comment --comment '{marker}' -j ACCEPT");
            let mutation = if present {
                format!("iptables -C INPUT {rule} >/dev/null 2>&1 || iptables -I INPUT {rule}")
            } else {
                format!(
                    "if iptables -C INPUT {rule} >/dev/null 2>&1; then iptables -D INPUT {rule}; fi"
                )
            };
            let verify = if present {
                format!("iptables -C INPUT {rule}")
            } else {
                format!("! iptables -C INPUT {rule} >/dev/null 2>&1")
            };
            Ok(SnapshotRollbackCommand {
                command: mutation,
                verify,
            })
        }
        _ => Err(AppError::Integrity("恢复点中的防火墙后端无效".into())),
    }
}

fn build_service_action_rollback(
    values: &BTreeMap<String, String>,
) -> AppResult<SnapshotRollbackCommand> {
    let manager = required_snapshot_value(values, "manager")?;
    let service = required_snapshot_value(values, "service")?;
    let active = required_snapshot_value(values, "active")?;
    let enabled = required_snapshot_value(values, "enabled")?;
    if !is_safe_service_name(service) {
        return Err(AppError::Integrity("恢复点中的服务名无效".into()));
    }
    if !matches!(active, "active" | "inactive") {
        return Err(AppError::Integrity(
            "服务原状态无法被可靠恢复，已在修改前阻止".into(),
        ));
    }
    let service = shell_quote(service);
    let action = if active == "active" { "start" } else { "stop" };
    match manager {
        "systemd" => {
            if !is_known_systemd_policy(enabled) {
                return Err(AppError::Integrity("恢复点中的服务开机状态无效".into()));
            }
            let enabled = shell_quote(enabled);
            Ok(SnapshotRollbackCommand {
                command: format!("systemctl {action} -- {service}"),
                verify: format!(
                    "test \"$(systemctl is-active -- {service} 2>/dev/null || true)\" = '{active}' && test \"$(systemctl is-enabled -- {service} 2>/dev/null || true)\" = {enabled}"
                ),
            })
        }
        "service" if enabled == "unsupported" => Ok(SnapshotRollbackCommand {
            command: format!("service {service} {action}"),
            verify: if active == "active" {
                format!("service {service} status >/dev/null 2>&1")
            } else {
                format!("! service {service} status >/dev/null 2>&1")
            },
        }),
        _ => Err(AppError::Integrity("恢复点中的服务管理器无效".into())),
    }
}

fn build_service_policy_rollback(
    values: &BTreeMap<String, String>,
) -> AppResult<SnapshotRollbackCommand> {
    if required_snapshot_value(values, "manager")? != "systemd" {
        return Err(AppError::Integrity("开机策略恢复只支持 systemd".into()));
    }
    let service = required_snapshot_value(values, "service")?;
    if !is_safe_service_name(service) {
        return Err(AppError::Integrity("恢复点中的服务名无效".into()));
    }
    if !matches!(
        required_snapshot_value(values, "active")?,
        "active" | "inactive" | "failed"
    ) {
        return Err(AppError::Integrity("恢复点中的服务运行状态无效".into()));
    }
    let enabled = required_snapshot_value(values, "enabled")?;
    let service = shell_quote(service);
    let command = match enabled {
        "enabled" => format!(
            "systemctl unmask -- {service} >/dev/null 2>&1 || true; systemctl enable -- {service}"
        ),
        "enabled-runtime" => format!(
            "systemctl unmask --runtime -- {service} >/dev/null 2>&1 || true; systemctl enable --runtime -- {service}"
        ),
        "disabled" => format!(
            "systemctl unmask -- {service} >/dev/null 2>&1 || true; systemctl disable -- {service}"
        ),
        "masked" => format!("systemctl mask -- {service}"),
        "masked-runtime" => format!("systemctl mask --runtime -- {service}"),
        _ => {
            return Err(AppError::Integrity(
                "服务原开机策略无法被可靠恢复，已在修改前阻止".into(),
            ))
        }
    };
    let enabled = shell_quote(enabled);
    Ok(SnapshotRollbackCommand {
        command,
        verify: format!(
            "test \"$(systemctl is-enabled -- {service} 2>/dev/null || true)\" = {enabled}"
        ),
    })
}

fn build_container_rollback(
    values: &BTreeMap<String, String>,
) -> AppResult<SnapshotRollbackCommand> {
    let runtime = required_snapshot_value(values, "runtime")?;
    if !matches!(runtime, "docker" | "podman") {
        return Err(AppError::Integrity("恢复点中的容器运行时无效".into()));
    }
    let container = required_snapshot_value(values, "container")?;
    if !is_safe_container_name(container) {
        return Err(AppError::Integrity("恢复点中的容器名无效".into()));
    }
    let state = match required_snapshot_value(values, "state")? {
        "exited" | "stopped" => "stopped",
        state @ ("running" | "paused") => state,
        _ => {
            return Err(AppError::Integrity(
                "容器原状态无法被可靠恢复，已在修改前阻止".into(),
            ))
        }
    };
    let container = shell_quote(container);
    let inspect = format!(
        "qz_format=$(printf '{{%s}}' '{{.State.Status}}'); {runtime} inspect --format \"$qz_format\" -- {container}"
    );
    let command = match state {
        "running" => format!(
            "qz_state=$({inspect}) || exit; case \"$qz_state\" in running) :;; paused) {runtime} unpause -- {container};; *) {runtime} start -- {container};; esac"
        ),
        "paused" => format!(
            "qz_state=$({inspect}) || exit; case \"$qz_state\" in paused) :;; running) {runtime} pause -- {container};; *) {runtime} start -- {container} && {runtime} pause -- {container};; esac"
        ),
        _ => format!(
            "qz_state=$({inspect}) || exit; case \"$qz_state\" in exited|stopped) :;; paused) {runtime} unpause -- {container} && {runtime} stop -- {container};; *) {runtime} stop -- {container};; esac"
        ),
    };
    let verify = if state == "stopped" {
        format!("qz_state=$({inspect}) || exit; test \"$qz_state\" = exited || test \"$qz_state\" = stopped")
    } else {
        format!("test \"$({inspect})\" = '{state}'")
    };
    Ok(SnapshotRollbackCommand { command, verify })
}

fn is_known_systemd_policy(value: &str) -> bool {
    matches!(
        value,
        "enabled"
            | "enabled-runtime"
            | "disabled"
            | "masked"
            | "masked-runtime"
            | "static"
            | "indirect"
            | "generated"
            | "transient"
            | "alias"
    )
}

fn is_safe_service_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@'))
}

fn is_safe_container_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn build_swap_rollback(values: &BTreeMap<String, String>) -> AppResult<SnapshotRollbackCommand> {
    let path = required_snapshot_value(values, "path")?;
    if !is_safe_swap_path(path) {
        return Err(AppError::Integrity("恢复点中的 Swap 路径无效".into()));
    }
    let existed = parse_snapshot_bool(values, "exists")?;
    let active = parse_snapshot_bool(values, "active")?;
    let size = required_snapshot_value(values, "size")?
        .parse::<u64>()
        .map_err(|_| AppError::Integrity("恢复点中的 Swap 大小无效".into()))?;
    const MAX_SWAP_BYTES: u64 = 32 * 1024 * 1024 * 1024;
    if (existed && !(1..=MAX_SWAP_BYTES).contains(&size)) || (!existed && size != 0) {
        return Err(AppError::Integrity(
            "恢复点中的 Swap 大小超出安全范围".into(),
        ));
    }
    let path = shell_quote(path);
    if !existed {
        return Ok(SnapshotRollbackCommand {
            command: format!(
                "test ! -L {path} && (swapoff -- {path} 2>/dev/null || true) && rm -f -- {path}"
            ),
            verify: format!(
                "test ! -e {path} && ! swapon --show=NAME --noheadings | grep -Fx -- {path}"
            ),
        });
    }
    let active_command = if active {
        format!("swapon -- {path}")
    } else {
        format!("swapoff -- {path} 2>/dev/null || true")
    };
    let active_verify = if active {
        format!("swapon --show=NAME --noheadings | grep -Fx -- {path}")
    } else {
        format!("! swapon --show=NAME --noheadings | grep -Fx -- {path}")
    };
    Ok(SnapshotRollbackCommand {
        command: format!(
            "test ! -L {path} && (swapoff -- {path} 2>/dev/null || true) && fallocate -l {size} -- {path} && chmod 0600 -- {path} && mkswap -- {path} >/dev/null && {active_command}"
        ),
        verify: format!(
            "test ! -L {path} && test -f {path} && test \"$(stat -Lc %s -- {path})\" = '{size}' && {active_verify}"
        ),
    })
}

fn parse_snapshot_values(snapshot: &str) -> AppResult<BTreeMap<String, String>> {
    let stdout = snapshot
        .strip_prefix("stdout:\n")
        .and_then(|value| value.split_once("\nstderr:\n").map(|(stdout, _)| stdout))
        .ok_or_else(|| AppError::Integrity("恢复点快照格式无效".into()))?;
    let mut values = BTreeMap::new();
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| AppError::Integrity("恢复点快照字段格式无效".into()))?;
        if key.is_empty()
            || !key.bytes().all(|byte| byte.is_ascii_lowercase())
            || values.insert(key.into(), value.into()).is_some()
        {
            return Err(AppError::Integrity("恢复点快照包含重复或无效字段".into()));
        }
    }
    Ok(values)
}

fn required_snapshot_value<'a>(
    values: &'a BTreeMap<String, String>,
    key: &str,
) -> AppResult<&'a str> {
    values
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| AppError::Integrity(format!("恢复点缺少字段：{key}")))
}

fn parse_bounded_u32(values: &BTreeMap<String, String>, key: &str, max: u32) -> AppResult<u32> {
    let value = required_snapshot_value(values, key)?
        .parse::<u32>()
        .map_err(|_| AppError::Integrity(format!("恢复点字段 {key} 无效")))?;
    if value > max {
        return Err(AppError::Integrity(format!(
            "恢复点字段 {key} 超出安全范围"
        )));
    }
    Ok(value)
}

fn parse_snapshot_bool(values: &BTreeMap<String, String>, key: &str) -> AppResult<bool> {
    match required_snapshot_value(values, key)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AppError::Integrity(format!("恢复点字段 {key} 无效"))),
    }
}

fn is_safe_hostname(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && value.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn is_safe_timezone(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-'))
        })
}

fn is_safe_swap_path(value: &str) -> bool {
    value == "/swapfile"
        || value
            .strip_prefix("/var/lib/qingzhou/swap/")
            .is_some_and(is_safe_relative_path)
}

fn is_safe_permissions_path(value: &str) -> bool {
    const PROTECTED_PATHS: &[&str] = &[
        "/", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/proc", "/run", "/sbin",
        "/sys", "/usr", "/var",
    ];
    !PROTECTED_PATHS.contains(&value) && is_normalized_absolute_path(value)
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn is_normalized_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value[1..]
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}
