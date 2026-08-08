use std::time::{Duration, Instant};

use qingzhou_ssh_lib::{
    core::{
        sftp::{
            download, list_remote_directory, sha256_local_file, upload, BrowserEntryKind,
            DownloadRequest, TransferPhase, UploadRequest, VerificationPolicy,
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
    let payload_len = std::env::var("QZ_SFTP_LARGE_TEST_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|bytes| (1..=512 * 1024 * 1024).contains(bytes))
        .unwrap_or(128 * 1024 + 17);
    let mut source = tokio::fs::File::create(&local_source).await.unwrap();
    let block = vec![0x5a; 256 * 1024];
    let mut generated = 0_u64;
    while generated < payload_len {
        let len = (payload_len - generated).min(block.len() as u64) as usize;
        tokio::io::AsyncWriteExt::write_all(&mut source, &block[..len])
            .await
            .unwrap();
        generated += len as u64;
    }
    tokio::io::AsyncWriteExt::flush(&mut source).await.unwrap();
    drop(source);
    let source_hash = sha256_local_file(&local_source).await.unwrap();
    let remote_path = format!("/tmp/qingzhou-sftp-{}.bin", uuid::Uuid::new_v4());
    let hashes_before = fixture_counter("hash-command-count");
    let reads_before = fixture_counter("sftp-read-bytes");

    let mut upload_events = VecEventSink::default();
    let upload_started = Instant::now();
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
    let upload_ms = upload_started.elapsed().as_millis() as u64;
    assert_eq!(uploaded.bytes, payload_len);
    assert_eq!(uploaded.sha256, source_hash);
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
    let download_started = Instant::now();
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
    let download_ms = download_started.elapsed().as_millis() as u64;
    assert_eq!(downloaded.sha256, uploaded.sha256);
    assert_eq!(
        downloaded.verification_level,
        qingzhou_ssh_lib::core::sftp::VerificationLevel::RemoteHash
    );
    assert!(downloaded.remote_hash_compared);
    assert_eq!(fixture_counter("hash-command-count"), hashes_before + 2);
    assert_eq!(
        fixture_counter("sftp-read-bytes") - reads_before,
        payload_len
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
        sha256_local_file(&test_root.join("downloads/downloaded.bin"))
            .await
            .unwrap(),
        source_hash
    );
    assert!(downloaded.pipeline_max_in_flight <= 8);
    assert!(downloaded.pipeline_max_in_flight > 0);
    assert!(downloaded.pipeline_max_buffered_bytes <= 16 * 1024 * 1024);
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
        payload_len * 2
    );
    let upload_progress = upload_events
        .events
        .iter()
        .filter(|event| matches!(event.payload, ExecutionEventPayload::Progress { .. }))
        .count() as u64;
    let download_progress = download_events
        .events
        .iter()
        .filter(|event| matches!(event.payload, ExecutionEventPayload::Progress { .. }))
        .count() as u64;
    assert!(upload_progress <= upload_ms / 50 + 4);
    assert!(download_progress <= download_ms / 50 + 4);
    eprintln!(
        "SFTP live bytes={payload_len} upload_ms={upload_ms} download_ms={download_ms} verification={:?} max_in_flight={} max_buffered_bytes={}",
        downloaded.verification_level,
        downloaded.pipeline_max_in_flight,
        downloaded.pipeline_max_buffered_bytes,
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
