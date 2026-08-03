use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use zeroize::Zeroizing;

use crate::{
    core::secret_protector::SecretProtector,
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct Vault {
    directory: PathBuf,
    protector: Arc<dyn SecretProtector>,
}

impl Vault {
    pub fn new(root: &Path, protector: Arc<dyn SecretProtector>) -> Self {
        Self {
            directory: root.join("vault"),
            protector,
        }
    }

    fn path_for(&self, id: &str) -> AppResult<PathBuf> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(AppError::Validation("凭据标识格式无效".into()));
        }
        Ok(self.directory.join(format!("{id}.bin")))
    }

    pub fn put(&self, id: &str, secret: &[u8]) -> AppResult<()> {
        let final_path = self.path_for(id)?;
        fs::create_dir_all(&self.directory)?;
        if final_path.exists() {
            return Err(AppError::Validation("凭据标识已经存在".into()));
        }

        let temp_path = self
            .directory
            .join(format!(".{id}.{}.tmp", uuid::Uuid::new_v4()));
        let encrypted = self.protector.protect(secret)?;
        let write_result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(&encrypted)?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temp_path, final_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> AppResult<Zeroizing<Vec<u8>>> {
        let encrypted = fs::read(self.path_for(id)?)?;
        Ok(Zeroizing::new(self.protector.unprotect(&encrypted)?))
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let path = self.path_for(id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    use crate::{core::secret_protector::SecretProtector, error::AppResult};

    struct XorProtector;

    impl SecretProtector for XorProtector {
        fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
            Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
        }

        fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
            Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
        }
    }

    #[test]
    fn writes_atomic_encrypted_blob_and_round_trips() {
        let temp = tempdir().unwrap();
        let vault = Vault::new(temp.path(), Arc::new(XorProtector));
        vault.put("cred-1", b"canary-password").unwrap();
        let stored = std::fs::read(temp.path().join("vault/cred-1.bin")).unwrap();
        assert!(!stored
            .windows(b"canary-password".len())
            .any(|window| window == b"canary-password"));
        assert_eq!(&*vault.get("cred-1").unwrap(), b"canary-password");
        assert!(!std::fs::read_dir(temp.path().join("vault"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|value| value == "tmp")));
    }

    #[test]
    fn rejects_path_traversal_and_preserves_an_existing_secret() {
        let temp = tempdir().unwrap();
        let vault = Vault::new(temp.path(), Arc::new(XorProtector));

        assert!(vault.put("../outside", b"secret").is_err());
        assert!(!temp.path().join("outside.bin").exists());

        vault.put("cred-2", b"first").unwrap();
        assert!(vault.put("cred-2", b"second").is_err());
        assert_eq!(&*vault.get("cred-2").unwrap(), b"first");
    }
}
