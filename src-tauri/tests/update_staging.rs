use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use qingzhou_ssh_lib::{
    domain::update::{UpdateRelease, UpdateReleaseInput, UpdateSource},
    services::update_service::{
        ProgressCallback, SignedUpdateAdapter, StagedUpdate, UpdateAdapterError, UpdateService,
    },
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[derive(Clone)]
struct FakeAdapter {
    result: Result<Vec<u8>, UpdateAdapterError>,
    installs: Arc<Mutex<usize>>,
}

impl SignedUpdateAdapter for FakeAdapter {
    fn download<'a>(
        &'a self,
        _release: &'a UpdateRelease,
        mut progress: ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async move {
            if let Ok(bytes) = &self.result {
                progress(bytes.len() as u64, Some(bytes.len() as u64));
            }
            self.result.clone()
        })
    }

    fn install<'a>(
        &'a self,
        _release: &'a UpdateRelease,
        _bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async move {
            *self.installs.lock().unwrap() += 1;
            Ok(())
        })
    }
}

fn release_for(bytes: &[u8]) -> UpdateRelease {
    release_for_platform(bytes, "windows-x86_64")
}

fn release_for_platform(bytes: &[u8], platform: &str) -> UpdateRelease {
    let digest = format!("{:x}", Sha256::digest(bytes));
    UpdateRelease::new(UpdateReleaseInput {
        version: "0.2.0".into(),
        notes: "安全更新".into(),
        published_at: Some("2026-08-04T10:00:00Z".into()),
        platform: platform.into(),
        download_url:
            "https://github.com/lx3559359/QingzhouSSH/releases/download/v0.2.0/update.exe".into(),
        signature: "trusted-signature".into(),
        sha256: digest,
        size: bytes.len() as u64,
        build_id: "build-20260804".into(),
        source: UpdateSource::Github,
    })
    .unwrap()
}

#[tokio::test]
async fn stages_with_the_selected_platform_package_extension() {
    let root = tempdir().unwrap();
    let bytes = b"signed macOS package".to_vec();
    let service = UpdateService::new(
        root.path(),
        Arc::new(FakeAdapter {
            result: Ok(bytes.clone()),
            installs: Arc::new(Mutex::new(0)),
        }),
    )
    .unwrap();

    let staged = service
        .download(
            release_for_platform(&bytes, "macos-aarch64-dmg"),
            Box::new(|_, _| {}),
        )
        .await
        .unwrap();
    assert_eq!(
        staged.relative_path,
        "staged/0.2.0/QingzhouSSH-0.2.0-macos-aarch64-dmg.dmg"
    );
}

#[tokio::test]
async fn stages_only_signature_and_hash_verified_bytes() {
    let root = tempdir().unwrap();
    let bytes = b"signed update package".to_vec();
    let installs = Arc::new(Mutex::new(0));
    let service = UpdateService::new(
        root.path(),
        Arc::new(FakeAdapter {
            result: Ok(bytes.clone()),
            installs: installs.clone(),
        }),
    )
    .unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let captured = progress.clone();
    let staged = service
        .download(
            release_for(&bytes),
            Box::new(move |downloaded, total| {
                captured.lock().unwrap().push((downloaded, total));
            }),
        )
        .await
        .unwrap();

    assert_eq!(
        staged,
        StagedUpdate {
            version: "0.2.0".into(),
            relative_path: "staged/0.2.0/QingzhouSSH-0.2.0-windows-x86_64.nsis".into(),
            sha256: release_for(&bytes).sha256.as_str().into(),
            size: bytes.len() as u64,
        }
    );
    let final_path = root.path().join("updates").join(&staged.relative_path);
    assert_eq!(std::fs::read(final_path).unwrap(), bytes);
    assert!(!root
        .path()
        .join("updates/staged/0.2.0/package.partial")
        .exists());
    assert_eq!(progress.lock().unwrap().as_slice(), &[(21, Some(21))]);
    assert!(service.install(false).await.is_err());
    assert_eq!(*installs.lock().unwrap(), 0);
    service.install(true).await.unwrap();
    assert_eq!(*installs.lock().unwrap(), 1);
}

#[tokio::test]
async fn deletes_partial_data_when_signature_or_hash_verification_fails() {
    let root = tempdir().unwrap();
    let expected = b"expected package".to_vec();
    let installs = Arc::new(Mutex::new(0));
    let signature_failure = UpdateService::new(
        root.path(),
        Arc::new(FakeAdapter {
            result: Err(UpdateAdapterError::Signature),
            installs: installs.clone(),
        }),
    )
    .unwrap();
    assert!(signature_failure
        .download(release_for(&expected), Box::new(|_, _| {}))
        .await
        .is_err());

    let hash_failure = UpdateService::new(
        root.path(),
        Arc::new(FakeAdapter {
            result: Ok(b"tampered package".to_vec()),
            installs,
        }),
    )
    .unwrap();
    assert!(hash_failure
        .download(release_for(&expected), Box::new(|_, _| {}))
        .await
        .is_err());

    let updates = root.path().join("updates");
    let leftovers: Vec<_> = walk_files(&updates)
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "partial" || ext == "nsis")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "unexpected staged files: {leftovers:?}"
    );
}

fn walk_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk_files(&path));
            } else {
                files.push(path);
            }
        }
    }
    files
}
