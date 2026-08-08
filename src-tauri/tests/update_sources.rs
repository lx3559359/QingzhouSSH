use std::{cell::Cell, collections::VecDeque, future::Future, pin::Pin, sync::Mutex};

use qingzhou_ssh_lib::{
    core::updates::{
        choose_source, current_platform_key, parse_manifest, parse_manifest_for_platform,
        DualSourceChecker, ManifestDecision, ManifestTransport, SourceCheckError,
        SourceFailureKind, TrustedSourcePolicy,
    },
    domain::update::UpdateSource,
};

fn github_manifest(url: &str) -> Vec<u8> {
    platform_manifest(current_platform_key(), url)
}

fn platform_manifest(platform: &str, url: &str) -> Vec<u8> {
    format!(
        r#"{{
          "version":"0.2.0",
          "notes":"安全更新",
          "pub_date":"2026-08-04T10:00:00Z",
          "platforms":{{
            "{platform}":{{
              "url":"{url}",
              "signature":"trusted-signature",
              "sha256":"{}",
              "size":12345,
              "build_id":"build-20260804"
            }}
          }}
        }}"#,
        "a".repeat(64)
    )
    .into_bytes()
}

#[test]
fn selects_only_the_requested_os_architecture_and_package() {
    let policy = TrustedSourcePolicy::new("lx3559359", "domestic-user").unwrap();
    let url = "https://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/QingzhouSSH.dmg";
    let manifest = platform_manifest("macos-aarch64-dmg", url);
    let decision = parse_manifest_for_platform(
        &policy,
        UpdateSource::Github,
        "0.1.0",
        &manifest,
        "macos-aarch64-dmg",
    )
    .unwrap();
    let ManifestDecision::Available(release) = decision else {
        panic!("expected available release");
    };
    assert_eq!(release.platform, "macos-aarch64-dmg");
    assert!(parse_manifest_for_platform(
        &policy,
        UpdateSource::Github,
        "0.1.0",
        &manifest,
        "linux-aarch64-appimage",
    )
    .is_err());
}

#[test]
fn parses_only_https_urls_from_the_fixed_github_repository() {
    let policy = TrustedSourcePolicy::new("lx3559359", "domestic-user").unwrap();
    let valid = github_manifest(
        "https://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/QingzhouSSH_0.2.0_x64-setup.exe",
    );
    let decision = parse_manifest(&policy, UpdateSource::Github, "0.1.0", &valid).unwrap();
    let ManifestDecision::Available(release) = decision else {
        panic!("expected available release");
    };
    assert_eq!(release.version, "0.2.0");
    assert_eq!(release.source, UpdateSource::Github);

    let http = github_manifest(
        "http://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/update.exe",
    );
    assert!(parse_manifest(&policy, UpdateSource::Github, "0.1.0", &http).is_err());

    let other_repo = github_manifest(
        "https://github.com/attacker/QingzhouSSH/releases/download/v0.2.0/update.exe",
    );
    assert!(parse_manifest(&policy, UpdateSource::Github, "0.1.0", &other_repo).is_err());
}

#[test]
fn accepts_only_the_declared_modelscope_release_path() {
    let policy = TrustedSourcePolicy::new("lx3559359", "domestic-user").unwrap();
    assert_eq!(
        policy.manifest_endpoint(UpdateSource::Modelscope),
        "https://modelscope.cn/api/v1/models/domestic-user/QingzhouSSH/repo?Revision=master&FilePath=releases%2Flatest.json"
    );
    let url = "https://modelscope.cn/api/v1/models/domestic-user/QingzhouSSH/repo?Revision=master&FilePath=releases%2Fv0.2.0%2FQingzhouSSH_0.2.0_x64-setup.exe";
    let decision = parse_manifest(
        &policy,
        UpdateSource::Modelscope,
        "0.1.0",
        &github_manifest(url),
    )
    .unwrap();
    assert!(matches!(decision, ManifestDecision::Available(_)));

    let escaped = "https://modelscope.cn/api/v1/models/domestic-user/QingzhouSSH/repo?Revision=master&FilePath=..%2Fprivate.key";
    assert!(parse_manifest(
        &policy,
        UpdateSource::Modelscope,
        "0.1.0",
        &github_manifest(escaped),
    )
    .is_err());
}

#[test]
fn reports_up_to_date_without_inventing_an_update() {
    let policy = TrustedSourcePolicy::new("lx3559359", "domestic-user").unwrap();
    let manifest = github_manifest(
        "https://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/update.exe",
    );
    assert_eq!(
        parse_manifest(&policy, UpdateSource::Github, "0.2.0", &manifest).unwrap(),
        ManifestDecision::UpToDate
    );
}

#[test]
fn falls_back_only_for_availability_failures() {
    let calls = Cell::new(0);
    let selected = choose_source::<&str, _>(
        Err(SourceCheckError::new(
            SourceFailureKind::Network,
            "GitHub 暂时不可达",
        )),
        || {
            calls.set(calls.get() + 1);
            Ok("modelscope")
        },
    )
    .unwrap();
    assert_eq!(selected, "modelscope");
    assert_eq!(calls.get(), 1);

    let security_calls = Cell::new(0);
    let error = choose_source::<&str, _>(
        Err(SourceCheckError::new(
            SourceFailureKind::Security,
            "主源清单越过固定仓库",
        )),
        || {
            security_calls.set(security_calls.get() + 1);
            Ok("must-not-run")
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, SourceFailureKind::Security);
    assert_eq!(security_calls.get(), 0);
}

struct FakeTransport {
    responses: Mutex<VecDeque<Result<Vec<u8>, SourceCheckError>>>,
    endpoints: Mutex<Vec<String>>,
}

impl ManifestTransport for FakeTransport {
    fn fetch<'a>(
        &'a self,
        endpoint: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, SourceCheckError>> + Send + 'a>> {
        Box::pin(async move {
            self.endpoints.lock().unwrap().push(endpoint.to_owned());
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing fake response")
        })
    }
}

#[tokio::test]
async fn injectable_transport_checks_github_then_modelscope() {
    let modelscope_url = "https://modelscope.cn/api/v1/models/domestic-user/QingzhouSSH/repo?Revision=master&FilePath=releases%2Fv0.2.0%2Fupdate.exe";
    let transport = FakeTransport {
        responses: Mutex::new(VecDeque::from([
            Err(SourceCheckError::new(
                SourceFailureKind::Network,
                "GitHub 超时",
            )),
            Ok(github_manifest(modelscope_url)),
        ])),
        endpoints: Mutex::new(Vec::new()),
    };
    let checker = DualSourceChecker::new(
        TrustedSourcePolicy::new("lx3559359", "domestic-user").unwrap(),
        transport,
    );
    let selected = checker.check("0.1.0").await.unwrap();
    assert_eq!(selected.source, UpdateSource::Modelscope);
    assert_eq!(selected.fallback_reason.as_deref(), Some("GitHub 超时"));
}
