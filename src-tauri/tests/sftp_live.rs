use std::time::Duration;

use qingzhou_ssh_lib::{
    core::{
        sftp::{
            download, list_remote_directory, upload, BrowserEntryKind, DownloadRequest,
            TransferPhase, UploadRequest, VerificationPolicy,
        },
        ssh::{
            executor::VecEventSink,
            transport::{
                connect_authenticated, execute, inspect_host_key, probe_authenticated, SshEndpoint,
            },
        },
        tasks::shell_quote,
    },
    domain::{events::ExecutionEventPayload, server::StoredCredential},
};
use tokio_util::sync::CancellationToken;

fn endpoint() -> SshEndpoint {
    SshEndpoint {
        host: "127.0.0.1".into(),
        port: 2222,
        timeout: Duration::from_secs(10),
    }
}

fn fixture_counter(name: &str) -> u64 {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.local/ssh-fixture/remote-root/run/qingzhou-fixture")
            .join(format!("{name}.state")),
    )
    .unwrap()
    .trim()
    .parse()
    .unwrap()
}

fn progress_phases(events: &VecEventSink) -> Vec<TransferPhase> {
    let mut phases = events
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            ExecutionEventPayload::Progress { phase, .. } => Some(*phase),
            _ => None,
        })
        .collect::<Vec<_>>();
    phases.dedup();
    phases
}

#[tokio::test]
#[ignore = "requires the project-local SSH/SFTP fixture on 127.0.0.1:2222"]
async fn uploads_and_downloads_verified_file_against_fixture() {
    let endpoint = endpoint();
    let observed = inspect_host_key(&endpoint).await.unwrap();
    let credential = StoredCredential::Password {
        password: "testpass".into(),
    };
    let session = connect_authenticated(
        &endpoint,
        "testuser",
        &credential,
        &observed.fingerprint_sha256,
    )
    .await
    .unwrap();
    let capabilities = probe_authenticated(&session).await.unwrap();

    let test_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/test-data/sftp-live");
    tokio::fs::create_dir_all(&test_root).await.unwrap();
    let local_source = test_root.join("payload.bin");
    let payload = vec![0x5a; 128 * 1024 + 17];
    tokio::fs::write(&local_source, &payload).await.unwrap();
    let remote_path = format!("/tmp/qingzhou-sftp-{}.bin", uuid::Uuid::new_v4());
    let hashes_before = fixture_counter("hash-command-count");
    let reads_before = fixture_counter("sftp-read-bytes");

    let mut upload_events = VecEventSink::default();
    let uploaded = upload(
        &session,
        &capabilities,
        &UploadRequest {
            local_path: local_source.clone(),
            remote_path: remote_path.clone(),
            overwrite: false,
            verification: VerificationPolicy::Balanced,
        },
        &mut upload_events,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(uploaded.bytes, payload.len() as u64);
    assert_eq!(
        uploaded.verification_level,
        qingzhou_ssh_lib::core::sftp::VerificationLevel::RemoteHash
    );
    assert!(uploaded.remote_hash_compared);
    assert_eq!(fixture_counter("hash-command-count"), hashes_before + 1);
    assert_eq!(fixture_counter("sftp-read-bytes"), reads_before);
    assert_eq!(
        progress_phases(&upload_events),
        vec![
            TransferPhase::Transferring,
            TransferPhase::Verifying,
            TransferPhase::Finalizing,
        ]
    );

    let listing = list_remote_directory(&session, "/tmp").await.unwrap();
    let uploaded_name = remote_path.rsplit('/').next().unwrap();
    assert_eq!(listing.path, "/tmp");
    assert!(listing
        .entries
        .iter()
        .any(|entry| { entry.name == uploaded_name && entry.kind == BrowserEntryKind::File }));

    let mut download_events = VecEventSink::default();
    let downloaded = download(
        &session,
        &capabilities,
        &test_root,
        &DownloadRequest {
            remote_path: remote_path.clone(),
            suggested_name: "downloaded.bin".into(),
            overwrite: true,
            verification: VerificationPolicy::Balanced,
        },
        &mut download_events,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(downloaded.sha256, uploaded.sha256);
    assert_eq!(
        downloaded.verification_level,
        qingzhou_ssh_lib::core::sftp::VerificationLevel::RemoteHash
    );
    assert!(downloaded.remote_hash_compared);
    assert_eq!(fixture_counter("hash-command-count"), hashes_before + 2);
    assert_eq!(
        fixture_counter("sftp-read-bytes") - reads_before,
        payload.len() as u64
    );
    assert_eq!(
        progress_phases(&download_events),
        vec![
            TransferPhase::Verifying,
            TransferPhase::Transferring,
            TransferPhase::Verifying,
            TransferPhase::Finalizing,
        ]
    );
    assert_eq!(
        tokio::fs::read(test_root.join("downloads/downloaded.bin"))
            .await
            .unwrap(),
        payload
    );
    let reads_after_balanced = fixture_counter("sftp-read-bytes");
    let mut strict_capabilities = capabilities.clone();
    strict_capabilities
        .commands
        .retain(|command| command != "sha256sum");
    let mut strict_events = VecEventSink::default();
    let strict_download = download(
        &session,
        &strict_capabilities,
        &test_root,
        &DownloadRequest {
            remote_path: remote_path.clone(),
            suggested_name: "strict-downloaded.bin".into(),
            overwrite: true,
            verification: VerificationPolicy::Strict,
        },
        &mut strict_events,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(strict_download.sha256, uploaded.sha256);
    assert_eq!(fixture_counter("hash-command-count"), hashes_before + 2);
    assert_eq!(
        fixture_counter("sftp-read-bytes") - reads_after_balanced,
        (payload.len() * 2) as u64
    );
    assert!(upload_events.events.len() >= 3);
    assert!(download_events.events.len() >= 3);
    session.disconnect().await;

    execute(
        &endpoint,
        "testuser",
        &credential,
        &observed.fingerprint_sha256,
        &format!("rm -f -- {}", shell_quote(&remote_path)),
    )
    .await
    .unwrap();
    tokio::fs::remove_file(local_source).await.unwrap();
    tokio::fs::remove_file(test_root.join("downloads/downloaded.bin"))
        .await
        .unwrap();
    tokio::fs::remove_file(test_root.join("downloads/strict-downloaded.bin"))
        .await
        .unwrap();
}
