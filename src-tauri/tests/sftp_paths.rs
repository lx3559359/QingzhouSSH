use qingzhou_ssh_lib::core::sftp::{
    download_destination, local_partial_path, remote_partial_path, sha256_local_file,
    validate_remote_path,
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
