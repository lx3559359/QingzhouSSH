use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

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
    domain::operation_restore::{
        NewOperationRestoreItem, NewOperationRestorePoint, OperationRestoreDetails,
        OperationRestoreItem, OperationRestoreItemStatus, OperationRestorePointStatus,
    },
    error::{AppError, AppResult},
    repositories::operation_restore_repository::OperationRestoreRepository,
    services::server_connector::{ConnectedServer, ServerConnector},
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
                    if is_task5_dangerous_task(task_id) {
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
            BackupItemKind::ManagedBlock => Err(AppError::Validation(
                "该任务的受控文本块回滚尚未启用".into(),
            )),
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

fn is_task5_dangerous_task(task_id: &str) -> bool {
    matches!(
        task_id,
        "system.hostname_change"
            | "system.timezone_change"
            | "storage.swap_manage"
            | "security.file_permissions"
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
        _ => Err(AppError::Validation("该任务尚未实现受控快照回滚".into())),
    }
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
