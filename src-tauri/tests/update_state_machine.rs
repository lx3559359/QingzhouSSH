use qingzhou_ssh_lib::domain::update::{
    Sha256Digest, UpdateLifecycle, UpdatePhase, UpdateRelease, UpdateReleaseInput, UpdateSource,
    UpdateValidationError, MAX_UPDATE_BYTES,
};

fn release(version: &str) -> UpdateRelease {
    UpdateRelease::new(UpdateReleaseInput {
        version: version.into(),
        notes: "修复与安全更新".into(),
        published_at: Some("2026-08-04T10:00:00Z".into()),
        platform: "windows-x86_64".into(),
        download_url:
            "https://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/QingzhouSSH.exe"
                .into(),
        signature: "trusted-signature".into(),
        sha256: "a".repeat(64),
        size: 12_345,
        build_id: "build-20260804".into(),
        source: UpdateSource::Github,
    })
    .unwrap()
}

#[test]
fn validates_release_metadata_and_version_policy() {
    let candidate = release("0.2.0");
    assert!(candidate.is_newer_than("0.1.0").unwrap());
    assert!(!candidate.is_newer_than("0.2.0").unwrap());
    assert!(!candidate.is_newer_than("0.3.0").unwrap());

    assert_eq!(
        Sha256Digest::parse("ABC").unwrap_err(),
        UpdateValidationError::InvalidSha256
    );
    assert_eq!(
        UpdateRelease::new(UpdateReleaseInput {
            version: "0.2".into(),
            notes: "notes".into(),
            published_at: None,
            platform: "windows-x86_64".into(),
            download_url: "https://example.com/update.exe".into(),
            signature: "sig".into(),
            sha256: "b".repeat(64),
            size: 1,
            build_id: "build".into(),
            source: UpdateSource::Github,
        })
        .unwrap_err(),
        UpdateValidationError::InvalidVersion
    );
    assert_eq!(
        UpdateRelease::new(UpdateReleaseInput {
            version: "0.2.0".into(),
            notes: "notes".into(),
            published_at: None,
            platform: "windows-x86_64".into(),
            download_url: "https://example.com/update.exe".into(),
            signature: "sig".into(),
            sha256: "b".repeat(64),
            size: MAX_UPDATE_BYTES + 1,
            build_id: "build".into(),
            source: UpdateSource::Github,
        })
        .unwrap_err(),
        UpdateValidationError::InvalidSize
    );
}

#[test]
fn enforces_the_update_lifecycle() {
    let mut lifecycle = UpdateLifecycle::default();
    assert_eq!(lifecycle.phase(), UpdatePhase::Idle);
    assert!(lifecycle.begin_download().is_err());

    lifecycle.begin_check().unwrap();
    lifecycle.set_available(release("0.2.0")).unwrap();
    assert_eq!(lifecycle.phase(), UpdatePhase::Available);
    lifecycle.begin_download().unwrap();
    lifecycle.set_downloaded().unwrap();
    assert_eq!(lifecycle.phase(), UpdatePhase::Downloaded);
    lifecycle.begin_install().unwrap();
    assert_eq!(lifecycle.phase(), UpdatePhase::Installing);

    lifecycle.fail("安装器启动失败");
    assert_eq!(lifecycle.phase(), UpdatePhase::Failed);
    assert_eq!(lifecycle.last_error(), Some("安装器启动失败"));
    lifecycle.reset();
    assert_eq!(lifecycle.phase(), UpdatePhase::Idle);
    assert!(lifecycle.release().is_none());
}

#[test]
fn supports_up_to_date_and_failed_checks_without_fake_availability() {
    let mut lifecycle = UpdateLifecycle::default();
    lifecycle.begin_check().unwrap();
    lifecycle.set_up_to_date().unwrap();
    assert_eq!(lifecycle.phase(), UpdatePhase::UpToDate);
    assert!(lifecycle.release().is_none());

    lifecycle.begin_check().unwrap();
    lifecycle.fail("主源和镜像均不可达");
    assert_eq!(lifecycle.phase(), UpdatePhase::Failed);
    assert!(lifecycle.release().is_none());
}
