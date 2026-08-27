use std::path::PathBuf;

use arch_sdk::{Config, blocking::ArchRpcClient};

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
    utils::format_amount,
};

const ARCH_DECIMALS: u8 = 9;

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Secret key file for the account to create or fund.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    ensure_faucet_supported(config.network)?;
    let (keypair, pubkey) = load_existing_key(&args.key, "faucet account key")?;
    let client = ArchRpcClient::new(config);

    println!("Requesting faucet funding for {pubkey}...");
    client.create_and_fund_account_with_faucet(&keypair)?;

    let balance = client.read_account_info(pubkey)?.lamports;
    println!("Faucet funding completed");
    println!("  Account: {pubkey}");
    println!(
        "  Balance: {} ARCH ({balance} lamports)",
        format_amount(balance, ARCH_DECIMALS)
    );
    Ok(())
}

fn ensure_faucet_supported(network: bitcoin::Network) -> Result<()> {
    if network == bitcoin::Network::Bitcoin {
        return Err(CliError::MainnetFaucetUnsupported);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_faucet_key() {
        let cli = Cli::try_parse_from(["arch-kit", "faucet", "--key", "sender.key"]).unwrap();
        let Command::Faucet(args) = cli.command else {
            panic!("expected faucet command");
        };
        assert_eq!(args.key, PathBuf::from("sender.key"));
    }

    #[test]
    fn requires_a_key_and_rejects_mainnet() {
        assert!(Cli::try_parse_from(["arch-kit", "faucet"]).is_err());
        assert!(ensure_faucet_supported(bitcoin::Network::Bitcoin).is_err());
        assert!(ensure_faucet_supported(bitcoin::Network::Testnet).is_ok());
    }
}
