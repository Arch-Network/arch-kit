use std::path::PathBuf;

use crate::{error::Result, keys::load_existing_key};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Secret key file to read.
    #[arg(value_name = "PATH")]
    pub(crate) secret_key: PathBuf,
}

pub(crate) fn run(args: Args) -> Result<()> {
    let (_, pubkey) = load_existing_key(&args.secret_key, "secret key")?;
    println!("{pubkey}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    use super::*;

    #[test]
    fn accepts_a_secret_key_path() {
        let cli = Cli::try_parse_from(["arch-kit", "pubkey", "authority.key"]).unwrap();

        let Command::Pubkey(args) = cli.command else {
            panic!("expected pubkey command");
        };
        assert_eq!(args.secret_key, PathBuf::from("authority.key"));
    }

    #[test]
    fn requires_exactly_one_path() {
        assert!(Cli::try_parse_from(["arch-kit", "pubkey"]).is_err());
        assert!(Cli::try_parse_from(["arch-kit", "pubkey", "first.key", "second.key"]).is_err());
    }
}
