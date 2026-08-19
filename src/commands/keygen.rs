use std::{collections::HashSet, path::PathBuf};

use bitcoin::Network;

use crate::{
    error::{CliError, Result},
    keys::{generate_key_file, persist_key_file, pubkey_hex},
    vanity::VanitySearch,
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Require each generated Arch public key to start with this Base58 prefix.
    #[arg(long, value_name = "BASE58_PREFIX")]
    pub(crate) prefix: Option<String>,

    /// Maximum parallel search threads for vanity key generation.
    #[arg(long, requires = "prefix", value_name = "COUNT")]
    pub(crate) threads: Option<usize>,

    /// Destinations for new secret keys. The files must not already exist.
    #[arg(value_name = "PATH", required = true, num_args = 1..)]
    pub(crate) outputs: Vec<PathBuf>,
}

pub(crate) fn run(args: Args) -> Result<()> {
    preflight_paths(&args.outputs)?;

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

fn preflight_paths(paths: &[PathBuf]) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    use super::*;

    #[test]
    fn accepts_one_or_more_positional_paths() {
        let cli = Cli::try_parse_from(["arch-kit", "keygen", "first.key", "second.key"]).unwrap();

        let Command::Keygen(args) = cli.command else {
            panic!("expected keygen command");
        };
        assert_eq!(
            args.outputs,
            [PathBuf::from("first.key"), PathBuf::from("second.key")]
        );
        assert!(args.prefix.is_none());
        assert!(args.threads.is_none());
    }

    #[test]
    fn requires_at_least_one_path() {
        assert!(Cli::try_parse_from(["arch-kit", "keygen"]).is_err());
    }

    #[test]
    fn accepts_a_vanity_prefix_and_thread_limit() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "keygen",
            "--prefix",
            "PAMM",
            "--threads",
            "4",
            "program.key",
        ])
        .unwrap();

        let Command::Keygen(args) = cli.command else {
            panic!("expected keygen command");
        };
        assert_eq!(args.prefix.as_deref(), Some("PAMM"));
        assert_eq!(args.threads, Some(4));
    }

    #[test]
    fn rejects_threads_without_a_prefix() {
        assert!(
            Cli::try_parse_from(["arch-kit", "keygen", "--threads", "2", "program.key"]).is_err()
        );
    }

    #[test]
    fn preflight_rejects_duplicates_and_existing_paths() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.key");
        let second = directory.path().join("second.key");

        assert!(preflight_paths(&[first.clone(), first]).is_err());
        std::fs::write(&second, "existing").unwrap();
        assert!(preflight_paths(&[directory.path().join("new.key"), second]).is_err());
    }
}
