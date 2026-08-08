use qingzhou_ssh_lib::core::sftp::{
    download_destination, local_partial_path, parse_sha256_output, remote_hash_command,
    remote_partial_path, select_verification, sha256_local_file, validate_remote_path,
    DownloadRequest, UploadRequest, VerificationPolicy, VerificationStrategy,
};
use qingzhou_ssh_lib::core::system_probe::{
    RemoteOsFamily, RemotePathStyle, RemoteShell, SystemCapabilities,
};

#[test]
fn remote_paths_are_absolute_and_partial_files_stay_in_the_same_directory() {
    assert!(validate_remote_path("/var/tmp/release.zip").is_ok());
    for invalid in ["relative/file", "", "/tmp/bad\0name"] {
        assert!(
            validate_remote_path(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    let partial = remote_partial_path("/var/tmp/release.zip").unwrap();
    assert!(partial.starts_with("/var/tmp/.qingzhou-release.zip."));
    assert!(partial.ends_with(".partial"));
}

#[test]
fn windows_sftp_paths_are_absolute_forward_slash_paths() {
    for valid in ["C:/Users/ops/report.log", "/C:/Users/ops/report.log"] {
        assert!(validate_remote_path(valid).is_ok(), "rejected {valid:?}");
    }
    for invalid in [
        "C:relative.txt",
        "C:\\Users\\ops\\report.log",
        "C:/Users/../Admin/file",
    ] {
        assert!(
            validate_remote_path(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    let partial = remote_partial_path("C:/Users/ops/report.log").unwrap();
    assert!(partial.starts_with("C:/Users/ops/.qingzhou-report.log."));
}

#[test]
fn downloads_are_confined_to_the_data_root_downloads_directory() {
    let root = tempfile::tempdir().unwrap();
    let destination = download_destination(root.path(), "service.log").unwrap();
    assert_eq!(destination, root.path().join("downloads/service.log"));
    assert_eq!(
        local_partial_path(&destination),
        root.path().join("downloads/service.log.partial")
    );

    for invalid in ["../escape.log", "folder/file.log", "", "bad\0name"] {
        assert!(download_destination(root.path(), invalid).is_err());
    }
}

#[tokio::test]
async fn local_hash_is_streamed_and_matches_sha256() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("payload.bin");
    tokio::fs::write(&file, b"abc").await.unwrap();
    assert_eq!(
        sha256_local_file(&file).await.unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn parses_only_unambiguous_sha256sum_output() {
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(
        parse_sha256_output(&format!("{digest}  /tmp/payload.bin\n")).unwrap(),
        digest
    );
    assert_eq!(parse_sha256_output(&format!("{digest}\n")).unwrap(), digest);
    for invalid in [
        "abc  /tmp/payload.bin\n",
        "z123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /tmp/payload.bin\n",
        "prefix ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  /tmp/payload.bin\n",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  /tmp/a\nextra\n",
    ] {
        assert!(parse_sha256_output(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn builds_only_capability_selected_remote_hash_commands() {
    let mut capabilities = SystemCapabilities::default();
    assert_eq!(
        remote_hash_command(&capabilities, "/tmp/a b").unwrap(),
        None
    );
    capabilities.commands.push("sha256sum".into());
    assert_eq!(
        remote_hash_command(&capabilities, "/tmp/a b").unwrap(),
        Some("sha256sum -- '/tmp/a b'".into())
    );

    capabilities.commands.clear();
    capabilities.platform_family = RemoteOsFamily::Bsd;
    capabilities.remote_shell = RemoteShell::PosixSh;
    capabilities.path_style = RemotePathStyle::Posix;
    capabilities.commands.push("sha256".into());
    assert_eq!(
        remote_hash_command(&capabilities, "/tmp/a b").unwrap(),
        Some("sha256 -q '/tmp/a b'".into())
    );

    capabilities.platform_family = RemoteOsFamily::Windows;
    capabilities.remote_shell = RemoteShell::PowerShell;
    capabilities.path_style = RemotePathStyle::WindowsSftp;
    capabilities.commands = vec!["get-filehash".into()];
    assert_eq!(
        remote_hash_command(&capabilities, "C:/Users/ops/a.bin").unwrap(),
        None
    );
}

#[test]
fn selects_verification_without_unnecessary_second_reads() {
    assert_eq!(
        select_verification(VerificationPolicy::Balanced, true),
        VerificationStrategy::RemoteHash
    );
    assert_eq!(
        select_verification(VerificationPolicy::Balanced, false),
        VerificationStrategy::TransportAndSize
    );
    assert_eq!(
        select_verification(VerificationPolicy::Strict, false),
        VerificationStrategy::SftpReread
    );
    assert_eq!(
        select_verification(VerificationPolicy::TransportOnly, true),
        VerificationStrategy::TransportAndSize
    );
}

#[test]
fn missing_verification_policy_defaults_to_balanced() {
    let upload: UploadRequest = serde_json::from_value(serde_json::json!({
        "localPath": "D:\\payload.bin",
        "remotePath": "/tmp/payload.bin",
        "overwrite": false
    }))
    .unwrap();
    let download: DownloadRequest = serde_json::from_value(serde_json::json!({
        "remotePath": "/tmp/payload.bin",
        "suggestedName": "payload.bin",
        "overwrite": false
    }))
    .unwrap();

    assert_eq!(upload.verification, VerificationPolicy::Balanced);
    assert_eq!(download.verification, VerificationPolicy::Balanced);
}
