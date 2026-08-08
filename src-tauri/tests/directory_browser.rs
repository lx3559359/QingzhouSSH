use qingzhou_ssh_lib::core::sftp::{
    remote_child_path, remote_parent, validate_remote_directory_path, validate_remote_entry_name,
};

#[test]
fn remote_directory_navigation_is_absolute_and_cannot_escape() {
    assert_eq!(remote_parent("/var/log"), Some("/var".to_string()));
    assert_eq!(remote_parent("/"), None);
    assert!(validate_remote_directory_path("/var/log").is_ok());
    assert!(validate_remote_directory_path("relative/path").is_err());
    assert!(validate_remote_directory_path("/var/../root").is_err());

    assert_eq!(remote_parent("C:/Users/ops"), Some("C:/Users".to_string()));
    assert_eq!(remote_parent("C:/Users"), Some("C:/".to_string()));
    assert_eq!(remote_parent("C:/"), None);
    assert!(validate_remote_directory_path("C:/").is_ok());
}

#[test]
fn remote_mutation_names_are_single_safe_path_components() {
    for name in ["reports", "报告 2026", "archive.tar.gz"] {
        assert!(validate_remote_entry_name(name).is_ok(), "{name}");
    }
    for name in [
        "",
        "   ",
        ".",
        "..",
        "child/name",
        "child\\name",
        "bad\0name",
    ] {
        assert!(validate_remote_entry_name(name).is_err(), "{name:?}");
    }
    assert!(validate_remote_entry_name(&"a".repeat(256)).is_err());
}

#[test]
fn remote_child_paths_preserve_posix_and_windows_roots() {
    assert_eq!(remote_child_path("/", "logs").unwrap(), "/logs");
    assert_eq!(remote_child_path("/srv", "logs").unwrap(), "/srv/logs");
    assert_eq!(remote_child_path("C:/", "logs").unwrap(), "C:/logs");
    assert_eq!(
        remote_child_path("C:/Users", "logs").unwrap(),
        "C:/Users/logs"
    );
    assert!(remote_child_path("relative", "logs").is_err());
    assert!(remote_child_path("/srv", "../root").is_err());
}
