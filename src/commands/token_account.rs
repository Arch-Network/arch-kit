use arch_sdk::{Config, arch_program::program_option::COption, blocking::ArchRpcClient};
use serde_json::json;

use crate::{
    error::Result,
    token::{
        account_state_name, format_amount, optional_pubkey, optional_u64, parse_pubkey,
        read_token_account,
    },
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Token account as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "ADDRESS")]
    pub(crate) address: String,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let address = parse_pubkey(&args.address, "token account")?;
    let view = read_token_account(&ArchRpcClient::new(config), address)?;
    let account = view.state;
    let decimals = view.mint.state.decimals;
    let amount = format_amount(account.amount, decimals);
    let delegate = optional_pubkey(account.delegate);
    let close_authority = optional_pubkey(account.close_authority);
    let native_reserve = optional_u64(account.is_native);

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "address": view.address.to_string(),
                "mint": account.mint.to_string(),
                "owner": account.owner.to_string(),
                "state": account_state_name(account.state),
                "decimals": decimals,
                "amount_raw": account.amount.to_string(),
                "amount": amount,
                "delegate": delegate,
                "delegated_amount_raw": account.delegated_amount.to_string(),
                "is_native": native_reserve.is_some(),
                "native_reserve_raw": native_reserve.map(|value| value.to_string()),
                "close_authority": close_authority,
            }))?
        );
    } else {
        println!("Token account: {}", view.address);
        println!("Mint: {}", account.mint);
        println!("Owner: {}", account.owner);
        println!("State: {}", account_state_name(account.state));
        println!("Amount: {amount}");
        println!("Raw amount: {}", account.amount);
        println!("Decimals: {decimals}");
        println!("Delegate: {}", delegate.as_deref().unwrap_or("none"));
        println!("Delegated raw amount: {}", account.delegated_amount);
        match account.is_native {
            COption::Some(reserve) => println!("Native reserve: {reserve}"),
            COption::None => println!("Native reserve: none"),
        }
        println!(
            "Close authority: {}",
            close_authority.as_deref().unwrap_or("none")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn accepts_an_account_address() {
        let cli = Cli::try_parse_from(["arch-kit", "token-account", "account"]).unwrap();
        let Command::TokenAccount(args) = cli.command else {
            panic!("expected token-account command");
        };
        assert_eq!(args.address, "account");
        assert!(!args.json);
    }
}
