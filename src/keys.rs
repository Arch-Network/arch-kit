use std::{collections::HashSet, fs::OpenOptions, io::Write, path::Path, str::FromStr};

use arch_sdk::{arch_program::pubkey::Pubkey, generate_new_keypair};
use bitcoin::{
    Network,
    key::Keypair,
    secp256k1::{Secp256k1, SecretKey},
};

use crate::{
    cli::KeygenArgs,
    error::{CliError, Result},
    vanity::VanitySearch,
};

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

/// Generate a key with the Arch SDK and persist it without replacing any path.
pub(crate) fn generate_key_file(
    path: &Path,
    label: &'static str,
    network: Network,
) -> Result<(Keypair, Pubkey)> {
    let (keypair, pubkey, _) = generate_new_keypair(network);
    persist_key_file(path, label, &keypair)?;
    Ok((keypair, pubkey))
}

/// Persist a generated keypair without replacing any existing path.
fn persist_key_file(path: &Path, label: &'static str, keypair: &Keypair) -> Result<()> {
    let encoded_secret = hex::encode(keypair.secret_bytes());

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|source| CliError::CreateKey {
        label,
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(encoded_secret.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| CliError::CreateKey {
            label,
            path: path.to_path_buf(),
            source,
        })?;

    Ok(())
}

/// Load a key, generating it only when its path is genuinely absent.
pub(crate) fn load_or_generate_key(
    path: &Path,
    label: &'static str,
    network: Network,
    generate_if_missing: bool,
) -> Result<(Keypair, Pubkey, bool)> {
    if generate_if_missing {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let (keypair, pubkey) = generate_key_file(path, label, network)?;
                return Ok((keypair, pubkey, true));
            }
            Err(source) => {
                return Err(CliError::LoadKey {
                    label,
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }

    let (keypair, pubkey) = load_existing_key(path, label)?;
    Ok((keypair, pubkey, false))
}

pub(crate) fn run_keygen(args: KeygenArgs) -> Result<()> {
    preflight_keygen_paths(&args.outputs)?;

    let vanity_search = args
        .prefix
        .as_deref()
        .map(|prefix| VanitySearch::new(prefix, args.threads))
        .transpose()?;

    for output in args.outputs {
        let (pubkey, vanity_stats) = if let Some(search) = &vanity_search {
            eprintln!(
                "Searching for public-key prefix {:?} for {} using {} thread(s)...",
                search.prefix(),
                output.display(),
                search.thread_count()
            );
            eprintln!(
                "  Rough uniform baseline: {:.3e} attempts",
                search.rough_expected_attempts()
            );
            let outcome = search.run()?;
            persist_key_file(&output, "secret key", &outcome.keypair)?;
            (
                outcome.pubkey,
                Some((outcome.attempts, outcome.elapsed.as_secs_f64())),
            )
        } else {
            // Secret keys are network-independent; the SDK's address result is unused.
            let (_, pubkey) = generate_key_file(&output, "secret key", Network::Bitcoin)?;
            (pubkey, None)
        };

        println!("Secret key generated securely.");
        println!("  Path: {}", output.display());
        println!("  Public key: {pubkey}");
        println!("  Public key (hex): {}", pubkey_hex(&pubkey));
        if let Some((attempts, elapsed_seconds)) = vanity_stats {
            println!("  Vanity attempts: {attempts}");
            println!("  Search time: {elapsed_seconds:.2}s");
        }
    }
    Ok(())
}

fn preflight_keygen_paths(paths: &[std::path::PathBuf]) -> Result<()> {
    let mut requested = HashSet::with_capacity(paths.len());
    for path in paths {
        if !requested.insert(path.as_path()) {
            return Err(CliError::InvalidArgument(format!(
                "duplicate key destination: {}",
                path.display()
            )));
        }

        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(CliError::CreateKey {
                    label: "secret key",
                    path: path.clone(),
                    source: std::io::Error::from(std::io::ErrorKind::AlreadyExists),
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CliError::CreateKey {
                    label: "secret key",
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn parse_secret_key(contents: &str) -> std::io::Result<SecretKey> {
    if let Ok(secret_key) = SecretKey::from_str(contents.trim()) {
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

    #[test]
    fn generates_an_sdk_compatible_key_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated.key");

        let (_, generated_pubkey) = generate_key_file(&path, "test key", Network::Regtest).unwrap();
        let (_, loaded_pubkey) = load_existing_key(&path, "test key").unwrap();
        let original = std::fs::read_to_string(&path).unwrap();

        assert_eq!(generated_pubkey, loaded_pubkey);
        assert_eq!(original.trim().len(), 64);
        assert!(generate_key_file(&path, "test key", Network::Regtest).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn generated_key_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated.key");
        generate_key_file(&path, "test key", Network::Regtest).unwrap();

        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn only_generates_when_the_key_is_missing_and_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated.key");

        assert!(load_or_generate_key(&path, "test key", Network::Regtest, false).is_err());
        let (_, generated_pubkey, generated) =
            load_or_generate_key(&path, "test key", Network::Regtest, true).unwrap();
        let (_, loaded_pubkey, generated_again) =
            load_or_generate_key(&path, "test key", Network::Regtest, true).unwrap();

        assert!(generated);
        assert!(!generated_again);
        assert_eq!(generated_pubkey, loaded_pubkey);
    }

    #[cfg(unix)]
    #[test]
    fn generate_if_missing_does_not_follow_a_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("missing-target.key");
        let path = directory.path().join("key-link");
        symlink(&target, &path).unwrap();

        assert!(load_or_generate_key(&path, "test key", Network::Regtest, true).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn keygen_preflight_rejects_duplicates_and_existing_paths() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.key");
        let second = directory.path().join("second.key");

        assert!(preflight_keygen_paths(&[first.clone(), first]).is_err());
        std::fs::write(&second, "existing").unwrap();
        assert!(preflight_keygen_paths(&[directory.path().join("new.key"), second]).is_err());
    }
}
