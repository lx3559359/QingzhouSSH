use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use std::{
    env, fs,
    io::{BufReader, Read},
    path::Path,
};

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if arguments.len() != 3 {
        return Err("usage: verifier <public-key-value> <signature> <artifact>".into());
    }
    let public_key_text = decode_tauri_text(&arguments[0])?;
    let signature_text = decode_tauri_text(&fs::read_to_string(&arguments[1])?)?;
    let public_key = if public_key_text.starts_with("untrusted comment:") {
        PublicKey::decode(&public_key_text)?
    } else {
        PublicKey::from_base64(&public_key_text)?
    };
    let signature = Signature::decode(&signature_text)?;
    let mut verifier = public_key.verify_stream(&signature)?;
    let mut artifact = BufReader::new(fs::File::open(Path::new(&arguments[2]))?);
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = artifact.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
    }
    verifier.finalize()?;
    println!("PASS: updater signature is cryptographically valid");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC_KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";

    #[test]
    fn verifies_the_release_fixture_incrementally() {
        let public_key = PublicKey::from_base64(PUBLIC_KEY).unwrap();
        let signature = Signature::decode(SIGNATURE).unwrap();
        let mut verifier = public_key.verify_stream(&signature).unwrap();
        verifier.update(b"te");
        verifier.update(b"st");
        verifier.finalize().unwrap();
    }
}
