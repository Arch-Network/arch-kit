use std::{path::Path, str::FromStr};

use arch_sdk::arch_program::pubkey::Pubkey;
use bitcoin::{
    key::Keypair,
    secp256k1::{Secp256k1, SecretKey},
};

use crate::error::{CliError, Result};

const SECRET_KEY_SIZE: usize = 32;

/// Load an existing secp256k1 key without ever creating or modifying its file.
pub(crate) fn load_existing_key(path: &Path, label: &'static str) -> Result<(Keypair, Pubkey)> {
    if !path.is_file() {
        return Err(CliError::InputNotFile {
            label,
            path: path.to_path_buf(),
        });
    }

    let contents = std::fs::read_to_string(path).map_err(|source| CliError::LoadKey {
        label,
        path: path.to_path_buf(),
        source,
    })?;
    let secret_key = parse_secret_key(&contents).map_err(|source| CliError::LoadKey {
        label,
        path: path.to_path_buf(),
        source,
    })?;
    let keypair = Keypair::from_secret_key(&Secp256k1::new(), &secret_key);
    let pubkey = Pubkey::from_slice(&keypair.x_only_public_key().0.serialize());
    Ok((keypair, pubkey))
}

fn parse_secret_key(contents: &str) -> std::io::Result<SecretKey> {
    if let Ok(secret_key) = SecretKey::from_str(contents) {
        return Ok(secret_key);
    }

    let secret_bytes: Vec<u8> = serde_json::from_str(contents).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file content is neither a valid secret key string nor a JSON byte array",
        )
    })?;
    if secret_bytes.len() < SECRET_KEY_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "secret key byte array is too short: expected at least {}, got {}",
                SECRET_KEY_SIZE,
                secret_bytes.len()
            ),
        ));
    }
    SecretKey::from_slice(&secret_bytes[..SECRET_KEY_SIZE]).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the first 32 bytes are not a valid secp256k1 secret key",
        )
    })
}

pub(crate) fn pubkey_hex(pubkey: &Pubkey) -> String {
    hex::encode(pubkey.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sdk_compatible_secret_key_formats() {
        let hex = "01".repeat(32);
        let from_hex = parse_secret_key(&hex).unwrap();
        let json = format!(
            "[{}]",
            from_hex
                .secret_bytes()
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let from_json = parse_secret_key(&json).unwrap();

        assert_eq!(from_hex, from_json);
    }

    #[test]
    fn rejects_invalid_secret_key_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid.json");
        std::fs::write(&path, "[1, 2, 3]").unwrap();

        assert!(load_existing_key(&path, "test key").is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "[1, 2, 3]");
    }
}
