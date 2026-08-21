use crate::{
    error::Result,
    token::{associated_token_address, parse_pubkey},
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Token account owner as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "OWNER")]
    pub(crate) owner: String,

    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,
}

pub(crate) fn run(args: Args) -> Result<()> {
    let owner = parse_pubkey(&args.owner, "owner")?;
    let mint = parse_pubkey(&args.mint, "mint")?;
    println!("{}", associated_token_address(&owner, &mint).0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn accepts_owner_and_mint() {
        let cli = Cli::try_parse_from(["arch-kit", "ata", "owner", "mint"]).unwrap();
        let Command::Ata(args) = cli.command else {
            panic!("expected ata command");
        };
        assert_eq!(args.owner, "owner");
        assert_eq!(args.mint, "mint");
    }
}
