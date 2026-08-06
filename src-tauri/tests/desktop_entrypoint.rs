#[test]
fn windows_release_uses_the_gui_subsystem() {
    let source = include_str!("../src/main.rs");
    assert!(
        source.lines().any(|line| {
            line.contains("cfg_attr")
                && line.contains("windows_subsystem")
                && line.contains("windows")
        }),
        "release builds must not open a companion console window"
    );
}

#[test]
fn desktop_entrypoint_routes_the_hidden_migration_mode_before_tauri() {
    let source = include_str!("../src/main.rs");
    assert!(source.contains("run_process_mode(std::env::args_os())"));
    assert!(source.contains("Ok(true) => ()"));
    assert!(source.contains("Ok(false) => qingzhou_ssh_lib::run()"));
}
