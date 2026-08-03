use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use std::{env, fs, path::Path};

fn decode_tauri_text(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let trimmed = value.trim();
    if trimmed.starts_with("untrusted comment:") {
        return Ok(trimmed.to_owned());
    }

    if let Ok(decoded) = STANDARD.decode(trimmed) {
        if let Ok(text) = String::from_utf8(decoded) {
            if text.starts_with("untrusted comment:") {
                return Ok(text);
            }
        }
    }

    Ok(trimmed.to_owned())
}

fn verify_files(
    public_key_value: &str,
    signature_path: &Path,
    artifact_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let public_key_text = decode_tauri_text(public_key_value)?;
    let signature_text = decode_tauri_text(&fs::read_to_string(signature_path)?)?;
    let public_key = if public_key_text.starts_with("untrusted comment:") {
        PublicKey::decode(&public_key_text)?
    } else {
        PublicKey::from_base64(&public_key_text)?
    };
    let signature = Signature::decode(&signature_text)?;
    let artifact = fs::read(artifact_path)?;
    public_key.verify(&artifact, &signature, false)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if arguments.len() != 3 {
        return Err(
            "usage: verify_release_signature <public-key-value> <signature> <artifact>".into(),
        );
    }

    verify_files(
        &arguments[0],
        Path::new(&arguments[1]),
        Path::new(&arguments[2]),
    )?;
    println!("PASS: updater signature is cryptographically valid");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_tauri_wrapped_minisign_text() {
        let plain =
            "untrusted comment: fixture\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let wrapped = STANDARD.encode(plain.as_bytes());

        assert_eq!(decode_tauri_text(plain).unwrap(), plain);
        assert_eq!(decode_tauri_text(&wrapped).unwrap(), plain);
        assert_eq!(
            decode_tauri_text("RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3").unwrap(),
            "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"
        );
    }
}
