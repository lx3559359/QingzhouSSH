use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use sha2::{Digest, Sha256};

pub fn sha256_fingerprint(raw_key: &[u8]) -> String {
    format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(raw_key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_openssh_style_sha256_fingerprint() {
        assert_eq!(
            sha256_fingerprint(b"host-key"),
            "SHA256:CfEOS9w3pHE4KlqjcQFwWyWMmyRvvPoehydyMhTxpzg"
        );
    }
}
