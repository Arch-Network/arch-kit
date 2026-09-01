use arch_sdk::{Config, blocking::ArchRpcClient};
use serde_json::json;

use crate::{error::Result, token::parse_pubkey, utils::format_amount};

const ARCH_DECIMALS: u8 = 9;

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Account as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "ACCOUNT")]
    pub(crate) account: String,
}

pub(crate) fn run(config: &Config, args: Args, json: bool) -> Result<()> {
    let account = parse_pubkey(&args.account, "account")?;
    let lamports = ArchRpcClient::new(config)
        .read_account_info(account)?
        .lamports;
    let amount = format_amount(lamports, ARCH_DECIMALS);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "account": account.to_string(),
                "lamports": lamports.to_string(),
                "amount": amount,
                "decimals": ARCH_DECIMALS,
            }))?
        );
    } else {
        println!("Account: {account}");
        println!("Balance: {amount} ARCH");
        println!("Lamports: {lamports}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn parses_account_and_json_output() {
        let cli = Cli::try_parse_from(["arch-kit", "arch-balance", "account", "--json"]).unwrap();
        assert!(cli.json);
        let Command::ArchBalance(args) = cli.command else {
            panic!("expected arch-balance command");
        };
        assert_eq!(args.account, "account");
    }

    #[test]
    fn requires_an_account() {
        assert!(Cli::try_parse_from(["arch-kit", "arch-balance"]).is_err());
    }
}
