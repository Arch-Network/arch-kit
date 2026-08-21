use arch_sdk::Config;
use serde_json::json;

use crate::{
    error::Result,
    token::{
        account_state_name, associated_token_address, format_amount, list_token_accounts,
        parse_pubkey,
    },
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Token account owner as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "OWNER")]
    pub(crate) owner: String,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let owner = parse_pubkey(&args.owner, "owner")?;
    let accounts = list_token_accounts(config, owner)?;

    if args.json {
        let accounts = accounts
            .iter()
            .map(|view| {
                let decimals = view.mint.state.decimals;
                json!({
                    "address": view.address.to_string(),
                    "mint": view.state.mint.to_string(),
                    "amount_raw": view.state.amount.to_string(),
                    "amount": format_amount(view.state.amount, decimals),
                    "decimals": decimals,
                    "state": account_state_name(view.state.state),
                    "is_associated": associated_token_address(&owner, &view.state.mint).0 == view.address,
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "owner": owner.to_string(),
                "count": accounts.len(),
                "accounts": accounts,
            }))?
        );
    } else {
        println!("Token accounts for {owner}");
        if accounts.is_empty() {
            println!("No token accounts found.");
        }
        for view in &accounts {
            let decimals = view.mint.state.decimals;
            let is_associated =
                associated_token_address(&owner, &view.state.mint).0 == view.address;
            println!();
            println!("Address: {}", view.address);
            println!("  Mint: {}", view.state.mint);
            println!(
                "  Amount: {} (raw {})",
                format_amount(view.state.amount, decimals),
                view.state.amount
            );
            println!("  Decimals: {decimals}");
            println!("  State: {}", account_state_name(view.state.state));
            println!("  Associated: {is_associated}");
        }
        println!();
        println!("Total token accounts: {}", accounts.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn parses_owner_and_json_flag() {
        let cli = Cli::try_parse_from(["arch-kit", "token-accounts", "owner", "--json"]).unwrap();
        let Command::TokenAccounts(args) = cli.command else {
            panic!("expected token-accounts command");
        };
        assert_eq!(args.owner, "owner");
        assert!(args.json);
    }
}
