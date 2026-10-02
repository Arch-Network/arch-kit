use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use rand::{RngCore, rngs::OsRng};
use sha2::Sha256;

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
};

const VERSION: u8 = 1;
const NONCE_SIZE: usize = 12;
const TAG_SIZE: usize = 16;
const KEY_CONTEXT: &[u8] = b"arch-kit/message-encryption/v1";

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Existing secret key file (hex or SDK JSON).
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,

    /// Message or encrypted payload; omit to read UTF-8 from stdin.
    #[arg(value_name = "TEXT")]
    pub(crate) input: Option<String>,
}

pub(crate) fn run_encrypt(args: Args, json: bool) -> Result<()> {
    let cipher = load_cipher(&args.key)?;
    let message = read_input(args.input, io::stdin().lock())?;
    let ciphertext = encrypt(&cipher, &message)?;
    write_output("ciphertext", &ciphertext, json, io::stdout().lock())
}

pub(crate) fn run_decrypt(args: Args, json: bool) -> Result<()> {
    let cipher = load_cipher(&args.key)?;
    let payload = read_input(args.input, io::stdin().lock())?;
    let message = decrypt(&cipher, &payload)?;
    write_output("message", &message, json, io::stdout().lock())
}

fn load_cipher(path: &Path) -> Result<Aes256Gcm> {
    let (keypair, _) = load_existing_key(path, "secret key")?;
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, &keypair.secret_bytes())
        .expand(KEY_CONTEXT, &mut key)
        .map_err(|_| CliError::MessageCrypto("key derivation failed".into()))?;
    Aes256Gcm::new_from_slice(&key)
        .map_err(|_| CliError::MessageCrypto("invalid AES key length".into()))
}

fn encrypt(cipher: &Aes256Gcm, message: &str) -> Result<String> {
    let mut nonce = [0u8; NONCE_SIZE];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|error| CliError::MessageCrypto(format!("OS randomness unavailable: {error}")))?;
    encrypt_with_nonce(cipher, message, nonce)
}

fn encrypt_with_nonce(
    cipher: &Aes256Gcm,
    message: &str,
    nonce: [u8; NONCE_SIZE],
) -> Result<String> {
    let ciphertext = cipher
        .encrypt(
            &nonce.into(),
            Payload {
                msg: message.as_bytes(),
                aad: &[VERSION],
            },
        )
        .map_err(|_| CliError::MessageCrypto("encryption failed".into()))?;
    let mut payload = Vec::with_capacity(1 + NONCE_SIZE + ciphertext.len());
    payload.push(VERSION);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(payload))
}

fn decrypt(cipher: &Aes256Gcm, encoded: &str) -> Result<String> {
    let payload = STANDARD
        .decode(encoded.trim())
        .map_err(|_| CliError::MessageCrypto("invalid Base64 payload".into()))?;
    if payload.len() < 1 + NONCE_SIZE + TAG_SIZE {
        return Err(CliError::MessageCrypto("truncated payload".into()));
    }
    if payload[0] != VERSION {
        return Err(CliError::MessageCrypto(
            "unsupported payload version".into(),
        ));
    }
    let mut nonce = [0u8; NONCE_SIZE];
    nonce.copy_from_slice(&payload[1..1 + NONCE_SIZE]);
    let plaintext = cipher
        .decrypt(
            &nonce.into(),
            Payload {
                msg: &payload[1 + NONCE_SIZE..],
                aad: &payload[..1],
            },
        )
        .map_err(|_| {
            CliError::MessageCrypto("authentication failed: wrong key or modified payload".into())
        })?;
    String::from_utf8(plaintext)
        .map_err(|_| CliError::MessageCrypto("decrypted message is not valid UTF-8".into()))
}

fn read_input(input: Option<String>, mut reader: impl Read) -> Result<String> {
    if let Some(input) = input {
        return Ok(input);
    }
    let mut input = String::new();
    reader.read_to_string(&mut input).map_err(|error| {
        CliError::MessageCrypto(format!("cannot read UTF-8 from stdin: {error}"))
    })?;
    Ok(input)
}

fn write_output(field: &str, value: &str, json: bool, mut writer: impl Write) -> Result<()> {
    let output = if json {
        format!("{}\n", serde_json::json!({ (field): value }))
    } else if field == "ciphertext" {
        format!("{value}\n")
    } else {
        value.to_owned()
    };
    writer
        .write_all(output.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|error| CliError::MessageCrypto(format!("cannot write stdout: {error}")))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    use super::*;

    // Independently generated with Python cryptography's HKDF(SHA256) and AESGCM:
    // secret = bytes([1]) * 32, salt = None, info = KEY_CONTEXT,
    // nonce = bytes(range(12)), AAD = b"\x01", message = b"Hello, Arch!\n".
    const FIXTURE: &str = "AQABAgMEBQYHCAkKC8Zs1xInwoOttzy6nwpkRBdnxtg9Cowl6kjvbK5n";

    fn cipher() -> Aes256Gcm {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret.key");
        std::fs::write(&path, "01".repeat(32)).unwrap();
        load_cipher(&path).unwrap()
    }

    #[test]
    fn matches_independent_fixture() {
        let cipher = cipher();
        assert_eq!(decrypt(&cipher, FIXTURE).unwrap(), "Hello, Arch!\n");
        assert_eq!(
            encrypt_with_nonce(&cipher, "Hello, Arch!\n", std::array::from_fn(|i| i as u8))
                .unwrap(),
            FIXTURE
        );
    }

    #[test]
    fn round_trips_messages_with_both_key_formats() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret.key");
        // SDK key files can also contain public-key bytes after the secret.
        let sdk_key = serde_json::to_string(&vec![1u8; 64]).unwrap();
        for contents in ["01".repeat(32), sdk_key] {
            std::fs::write(&path, &contents).unwrap();
            let cipher = load_cipher(&path).unwrap();
            assert_eq!(decrypt(&cipher, FIXTURE).unwrap(), "Hello, Arch!\n");
            for message in ["", "Hello", "Hello 🌍 مرحبا", " \tfirst\nsecond\r\n "] {
                let encrypted = encrypt(&cipher, message).unwrap();
                assert_eq!(decrypt(&cipher, &encrypted).unwrap(), message);
            }
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn generates_a_fresh_nonce_for_each_message() {
        let cipher = cipher();
        let first = STANDARD.decode(encrypt(&cipher, "Hello").unwrap()).unwrap();
        let second = STANDARD.decode(encrypt(&cipher, "Hello").unwrap()).unwrap();
        assert_ne!(&first[1..13], &second[1..13]);
    }

    #[test]
    fn rejects_wrong_keys_and_modified_payloads() {
        let cipher = cipher();
        let wrong_cipher = Aes256Gcm::new_from_slice(&[2; 32]).unwrap();
        let expected = decrypt(&wrong_cipher, FIXTURE).unwrap_err().to_string();
        assert!(expected.contains("authentication failed"));
        let payload = STANDARD.decode(FIXTURE).unwrap();
        // Cover every byte of the nonce, ciphertext, and authentication tag.
        for index in 1..payload.len() {
            let mut changed = payload.clone();
            changed[index] ^= 1;
            assert_eq!(
                decrypt(&cipher, &STANDARD.encode(changed))
                    .unwrap_err()
                    .to_string(),
                expected
            );
        }
    }

    #[test]
    fn rejects_malformed_truncated_and_unknown_payloads() {
        let cipher = cipher();
        assert!(
            decrypt(&cipher, "%%%")
                .unwrap_err()
                .to_string()
                .contains("Base64")
        );
        for length in 0..1 + NONCE_SIZE + TAG_SIZE {
            assert!(
                decrypt(&cipher, &STANDARD.encode(vec![VERSION; length]))
                    .unwrap_err()
                    .to_string()
                    .contains("truncated")
            );
        }
        let mut payload = STANDARD.decode(FIXTURE).unwrap();
        payload[0] = 2;
        assert!(
            decrypt(&cipher, &STANDARD.encode(payload))
                .unwrap_err()
                .to_string()
                .contains("unsupported payload version")
        );
        assert_eq!(
            decrypt(&cipher, &format!(" \n{FIXTURE}\r\n")).unwrap(),
            "Hello, Arch!\n"
        );
    }

    #[test]
    fn rejects_authenticated_non_utf8_messages() {
        // Same independent Python fixture parameters, plaintext = b"\xff".
        let payload = "AQABAgMEBQYHCAkKC3EPxr6c4IIqkeNT8g17HPe6";
        assert!(
            decrypt(&cipher(), payload)
                .unwrap_err()
                .to_string()
                .contains("not valid UTF-8")
        );
    }

    #[test]
    fn rejects_missing_and_invalid_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret.key");
        assert!(load_cipher(&path).is_err());
        assert!(!path.exists());
        std::fs::write(&path, "invalid key").unwrap();
        assert!(load_cipher(&path).is_err());
    }

    #[test]
    fn accepts_commands_with_arguments_or_stdin() {
        for command in ["encrypt", "decrypt"] {
            for text in [None, Some(""), Some("hello")] {
                let mut argv = vec!["arch-kit", command, "--key", "authority.key", "--json"];
                argv.extend(text);
                let cli = Cli::try_parse_from(argv).unwrap();
                assert!(cli.json);
                let args = match cli.command {
                    Command::Encrypt(args) if command == "encrypt" => args,
                    Command::Decrypt(args) if command == "decrypt" => args,
                    _ => panic!("unexpected command"),
                };
                assert_eq!(args.key, PathBuf::from("authority.key"));
                assert_eq!(args.input.as_deref(), text);
            }
            assert!(Cli::try_parse_from(["arch-kit", command, "hello"]).is_err());
        }
    }

    #[test]
    fn preserves_input_and_rejects_non_utf8_stdin() {
        let message = " \nHello 🌍\r\n ";
        assert_eq!(read_input(None, message.as_bytes()).unwrap(), message);
        assert_eq!(
            read_input(Some(String::new()), message.as_bytes()).unwrap(),
            ""
        );
        assert!(read_input(None, &[0xff][..]).is_err());
    }

    #[test]
    fn writes_exact_plaintext_and_json_output() {
        for (field, value) in [("message", " \nHello 🌍\r\n "), ("ciphertext", FIXTURE)] {
            let mut output = Vec::new();
            write_output(field, value, false, &mut output).unwrap();
            let expected = if field == "ciphertext" {
                format!("{value}\n")
            } else {
                value.to_owned()
            };
            assert_eq!(output, expected.as_bytes());
            output.clear();
            write_output(field, value, true, &mut output).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output).unwrap(),
                serde_json::json!({ (field): value })
            );
        }
    }
}
