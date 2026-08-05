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
