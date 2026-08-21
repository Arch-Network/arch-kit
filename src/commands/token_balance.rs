use arch_sdk::{Config, blocking::ArchRpcClient};
use serde_json::json;

use crate::{
    error::Result,
    token::{format_amount, parse_pubkey, read_associated_balance},
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Token account owner as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "OWNER")]
    pub(crate) owner: String,

    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let owner = parse_pubkey(&args.owner, "owner")?;
    let mint_address = parse_pubkey(&args.mint, "mint")?;
    let (address, mint, account) =
        read_associated_balance(&ArchRpcClient::new(config), owner, mint_address)?;
    let amount = account.map_or(0, |account| account.amount);
    let display_amount = format_amount(amount, mint.state.decimals);

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "owner": owner.to_string(),
                "mint": mint_address.to_string(),
                "token_account": address.to_string(),
                "exists": account.is_some(),
                "decimals": mint.state.decimals,
                "amount_raw": amount.to_string(),
                "amount": display_amount,
            }))?
        );
    } else {
        println!("Owner: {owner}");
        println!("Mint: {mint_address}");
        println!("Token account: {address}");
        println!("Exists: {}", account.is_some());
        println!("Amount: {display_amount}");
        println!("Raw amount: {amount}");
        println!("Decimals: {}", mint.state.decimals);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn parses_json_output_flag() {
        let cli =
            Cli::try_parse_from(["arch-kit", "token-balance", "owner", "mint", "--json"]).unwrap();
        let Command::TokenBalance(args) = cli.command else {
            panic!("expected token-balance command");
        };
        assert!(args.json);
    }
}
