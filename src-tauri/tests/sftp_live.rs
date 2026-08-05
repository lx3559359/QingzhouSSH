use std::time::Duration;

use qingzhou_ssh_lib::{
    core::{
        sftp::{
            download, list_remote_directory, upload, BrowserEntryKind, DownloadRequest,
            UploadRequest,
        },
        ssh::{
            executor::VecEventSink,
            transport::{connect_authenticated, execute, inspect_host_key, SshEndpoint},
        },
        tasks::shell_quote,
    },
    domain::server::StoredCredential,
};
use tokio_util::sync::CancellationToken;

fn endpoint() -> SshEndpoint {
    SshEndpoint {
        host: "127.0.0.1".into(),
        port: 2222,
        timeout: Duration::from_secs(10),
    }
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

    let test_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/test-data/sftp-live");
    tokio::fs::create_dir_all(&test_root).await.unwrap();
    let local_source = test_root.join("payload.bin");
    let payload = vec![0x5a; 128 * 1024 + 17];
    tokio::fs::write(&local_source, &payload).await.unwrap();
    let remote_path = format!("/tmp/qingzhou-sftp-{}.bin", uuid::Uuid::new_v4());

    let mut upload_events = VecEventSink::default();
    let uploaded = upload(
        &session,
        &UploadRequest {
            local_path: local_source.clone(),
            remote_path: remote_path.clone(),
            overwrite: false,
        },
        &mut upload_events,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(uploaded.bytes, payload.len() as u64);

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
        &test_root,
        &DownloadRequest {
            remote_path: remote_path.clone(),
            suggested_name: "downloaded.bin".into(),
            overwrite: true,
        },
        &mut download_events,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(downloaded.sha256, uploaded.sha256);
    assert_eq!(
        tokio::fs::read(test_root.join("downloads/downloaded.bin"))
            .await
            .unwrap(),
        payload
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
}
