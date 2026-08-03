use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use qingzhou_ssh_lib::{
    core::updates::{
        ManifestDecision, SourceCheckError, SourceSelection, UpdateChecker, UpdateStateStore,
    },
    domain::update::{UpdatePhase, UpdateRelease, UpdateReleaseInput, UpdateSource},
    services::update_service::{
        ProgressCallback, SignedUpdateAdapter, UpdateAdapterError, UpdateManager, UpdateService,
    },
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

struct FakeChecker {
    responses: Mutex<VecDeque<Result<SourceSelection, SourceCheckError>>>,
    calls: Arc<Mutex<usize>>,
}

impl UpdateChecker for FakeChecker {
    fn check<'a>(
        &'a self,
        _current_version: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<SourceSelection, SourceCheckError>> + Send + 'a>> {
        Box::pin(async move {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing checker response")
        })
    }
}

#[derive(Clone)]
struct FakeAdapter {
    bytes: Vec<u8>,
}

impl SignedUpdateAdapter for FakeAdapter {
    fn download<'a>(
        &'a self,
        _release: &'a UpdateRelease,
        mut progress: ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async move {
            progress(4, Some(self.bytes.len() as u64));
            progress(self.bytes.len() as u64, Some(self.bytes.len() as u64));
            Ok(self.bytes.clone())
        })
    }

    fn install<'a>(
        &'a self,
        _release: &'a UpdateRelease,
        _bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

fn release(bytes: &[u8]) -> UpdateRelease {
    UpdateRelease::new(UpdateReleaseInput {
        version: "0.2.0".into(),
        notes: "镜像安全更新".into(),
        published_at: Some("2026-08-04T10:00:00Z".into()),
        platform: "windows-x86_64".into(),
        download_url: "https://modelscope.cn/api/v1/studios/demo/QingzhouSSH/repo?Revision=master&FilePath=releases%2Fv0.2.0%2Fupdate.exe".into(),
        signature: "secret-signature-must-not-cross-ipc".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        size: bytes.len() as u64,
        build_id: "build-20260804".into(),
        source: UpdateSource::Modelscope,
    })
    .unwrap()
}

#[tokio::test]
async fn persists_checked_status_and_rate_limits_automatic_checks() {
    let root = tempdir().unwrap();
    let bytes = b"verified update bytes".to_vec();
    let calls = Arc::new(Mutex::new(0));
    let checker = Arc::new(FakeChecker {
        responses: Mutex::new(VecDeque::from([Ok(SourceSelection {
            source: UpdateSource::Modelscope,
            decision: ManifestDecision::Available(Box::new(release(&bytes))),
            fallback_reason: Some("GitHub 超时，已切换国内镜像".into()),
        })])),
        calls: calls.clone(),
    });
    let download = UpdateService::new(
        root.path(),
        Arc::new(FakeAdapter {
            bytes: bytes.clone(),
        }),
    )
    .unwrap();
    let manager = UpdateManager::new_with_components(
        "0.1.0",
        UpdateStateStore::new(root.path()).unwrap(),
        checker,
        download,
    )
    .unwrap();

    let status = manager.check(true, 1_000).await.unwrap();
    assert_eq!(status.phase, UpdatePhase::Available);
    assert_eq!(status.release.unwrap().source, UpdateSource::Modelscope);
    assert_eq!(
        status.fallback_reason.as_deref(),
        Some("GitHub 超时，已切换国内镜像")
    );
    let limited = manager.check(false, 1_001).await.unwrap();
    assert_eq!(limited.phase, UpdatePhase::Available);
    assert_eq!(*calls.lock().unwrap(), 1);

    let stored = UpdateStateStore::new(root.path()).unwrap().load().unwrap();
    assert_eq!(stored.last_checked_at, Some(1_000));
}

#[tokio::test]
async fn emits_monotonic_progress_and_never_serializes_urls_or_signatures() {
    let root = tempdir().unwrap();
    let bytes = b"verified update bytes".to_vec();
    let checker = Arc::new(FakeChecker {
        responses: Mutex::new(VecDeque::from([Ok(SourceSelection {
            source: UpdateSource::Modelscope,
            decision: ManifestDecision::Available(Box::new(release(&bytes))),
            fallback_reason: Some("GitHub unavailable".into()),
        })])),
        calls: Arc::new(Mutex::new(0)),
    });
    let manager = UpdateManager::new_with_components(
        "0.1.0",
        UpdateStateStore::new(root.path()).unwrap(),
        checker,
        UpdateService::new(
            root.path(),
            Arc::new(FakeAdapter {
                bytes: bytes.clone(),
            }),
        )
        .unwrap(),
    )
    .unwrap();
    manager.check(true, 1_000).await.unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let status = manager
        .download(Box::new(move |event| captured.lock().unwrap().push(event)))
        .await
        .unwrap();
    assert_eq!(status.phase, UpdatePhase::Downloaded);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    assert!(events[1].downloaded_bytes >= events[0].downloaded_bytes);

    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains("modelscope.cn"));
    assert!(!json.contains("secret-signature"));
}

#[tokio::test]
async fn rejects_conflicting_actions_and_requires_explicit_install_confirmation() {
    let root = tempdir().unwrap();
    let bytes = b"verified update bytes".to_vec();
    let checker = Arc::new(FakeChecker {
        responses: Mutex::new(VecDeque::from([Ok(SourceSelection {
            source: UpdateSource::Github,
            decision: ManifestDecision::Available(Box::new(release(&bytes))),
            fallback_reason: None,
        })])),
        calls: Arc::new(Mutex::new(0)),
    });
    let manager = UpdateManager::new_with_components(
        "0.1.0",
        UpdateStateStore::new(root.path()).unwrap(),
        checker,
        UpdateService::new(root.path(), Arc::new(FakeAdapter { bytes })).unwrap(),
    )
    .unwrap();

    assert!(manager.download(Box::new(|_| {})).await.is_err());
    assert_eq!(manager.status().await.unwrap().phase, UpdatePhase::Idle);

    manager.check(true, 1_000).await.unwrap();
    manager.download(Box::new(|_| {})).await.unwrap();
    assert!(manager.install(false).await.is_err());
    assert_eq!(
        manager.status().await.unwrap().phase,
        UpdatePhase::Downloaded
    );
}
