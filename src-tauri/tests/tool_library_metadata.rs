use qingzhou_ssh_lib::core::tasks::{
    built_in_catalog, metadata_for, ToolLibraryCategory, ToolSource,
};

#[test]
fn every_builtin_tool_has_novice_discovery_metadata() {
    for definition in built_in_catalog() {
        let metadata = metadata_for(&definition);
        assert!(
            !metadata.keywords.is_empty(),
            "{} has no search keywords",
            definition.id
        );
        assert!(
            metadata
                .keywords
                .iter()
                .chain(&metadata.novice_aliases)
                .any(|value| value
                    .chars()
                    .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))),
            "{} has no Chinese discovery phrase",
            definition.id
        );
        assert!(matches!(
            metadata.source,
            ToolSource::BuiltInTask | ToolSource::ReviewedCommand
        ));
    }
}

#[test]
fn common_novice_phrases_route_to_the_expected_tools() {
    let expected = [
        (
            "runbook.web.gateway",
            "网站打不开",
            ToolLibraryCategory::WebService,
        ),
        (
            "network.port_process",
            "端口被占用",
            ToolLibraryCategory::Network,
        ),
        (
            "runbook.storage.capacity_io",
            "磁盘满了",
            ToolLibraryCategory::Storage,
        ),
        (
            "runbook.cpu.incident",
            "服务器很慢",
            ToolLibraryCategory::Performance,
        ),
        (
            "security.ssh_events",
            "登录失败",
            ToolLibraryCategory::SecurityLogin,
        ),
    ];

    for (task_id, phrase, category) in expected {
        let definition = built_in_catalog()
            .into_iter()
            .find(|definition| definition.id == task_id)
            .unwrap_or_else(|| panic!("missing task {task_id}"));
        let metadata = metadata_for(&definition);
        assert_eq!(metadata.primary_category, category);
        assert!(
            metadata.novice_aliases.iter().any(|alias| alias == phrase),
            "{task_id} does not include {phrase}"
        );
    }
}
