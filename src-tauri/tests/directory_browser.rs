use qingzhou_ssh_lib::core::sftp::{remote_parent, validate_remote_directory_path};

#[test]
fn remote_directory_navigation_is_absolute_and_cannot_escape() {
    assert_eq!(remote_parent("/var/log"), Some("/var".to_string()));
    assert_eq!(remote_parent("/"), None);
    assert!(validate_remote_directory_path("/var/log").is_ok());
    assert!(validate_remote_directory_path("relative/path").is_err());
    assert!(validate_remote_directory_path("/var/../root").is_err());
}
