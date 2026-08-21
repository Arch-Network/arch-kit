use arch_sdk::{Config, blocking::ArchRpcClient};
use serde_json::json;

use crate::{
    error::Result,
    token::{format_amount, optional_pubkey, parse_pubkey, read_mint},
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let address = parse_pubkey(&args.mint, "mint")?;
    let mint = read_mint(&ArchRpcClient::new(config), address)?;
    let state = mint.state;
    let supply = format_amount(state.supply, state.decimals);
    let mint_authority = optional_pubkey(state.mint_authority);
    let freeze_authority = optional_pubkey(state.freeze_authority);

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "mint": mint.address.to_string(),
                "initialized": state.is_initialized,
                "decimals": state.decimals,
                "supply_raw": state.supply.to_string(),
                "supply": supply,
                "mint_authority": mint_authority,
                "freeze_authority": freeze_authority,
            }))?
        );
    } else {
        println!("Mint: {}", mint.address);
        println!("Initialized: {}", state.is_initialized);
        println!("Decimals: {}", state.decimals);
        println!("Supply: {supply}");
        println!("Raw supply: {}", state.supply);
        println!(
            "Mint authority: {}",
            mint_authority.as_deref().unwrap_or("none")
        );
        println!(
            "Freeze authority: {}",
            freeze_authority.as_deref().unwrap_or("none")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn accepts_a_mint_address() {
        let cli = Cli::try_parse_from(["arch-kit", "mint-info", "mint"]).unwrap();
        let Command::MintInfo(args) = cli.command else {
            panic!("expected mint-info command");
        };
        assert_eq!(args.mint, "mint");
        assert!(!args.json);
    }
}
