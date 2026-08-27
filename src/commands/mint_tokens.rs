use std::path::PathBuf;

use arch_sdk::{
    Config,
    arch_program::{program_option::COption, pubkey::Pubkey},
    blocking::ArchRpcClient,
};

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
    token::{mint_to_user_instructions, parse_pubkey, read_mint},
    transaction::send_and_confirm,
    utils::{format_amount, parse_amount},
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Recipient owner as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "RECIPIENT")]
    pub(crate) recipient: String,

    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,

    /// Human-readable amount, interpreted using the mint's decimals.
    #[arg(value_name = "AMOUNT")]
    pub(crate) amount: String,

    /// Existing payer and mint-authority secret key file.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let (authority_keypair, authority) = load_existing_key(&args.key, "mint authority key")?;
    let recipient = parse_pubkey(&args.recipient, "recipient")?;
    let mint_address = parse_pubkey(&args.mint, "mint")?;
    let client = ArchRpcClient::new(config);
    let mint = read_mint(&client, mint_address)?;
    require_mint_authority(mint_address, &mint.state, authority)?;

    let amount = parse_amount(&args.amount, mint.state.decimals)?;
    let (destination, instructions) = mint_to_user_instructions(
        authority,
        recipient,
        mint_address,
        authority,
        amount,
        mint.state.decimals,
    )?;
    let transaction_id = send_and_confirm(
        &client,
        "token minting",
        instructions,
        authority,
        vec![authority_keypair],
    )?;

    println!("Tokens minted");
    println!("  Transaction: {transaction_id}");
    println!("  Mint: {mint_address}");
    println!("  Recipient: {recipient}");
    println!("  Destination: {destination}");
    println!(
        "  Amount: {} ({amount} raw)",
        format_amount(amount, mint.state.decimals)
    );
    Ok(())
}

fn require_mint_authority(
    mint_address: Pubkey,
    mint: &apl_token::state::Mint,
    authority: Pubkey,
) -> Result<()> {
    if !mint.is_initialized {
        return Err(CliError::MintTokens(format!(
            "mint {mint_address} is not initialized"
        )));
    }
    match mint.mint_authority {
        COption::Some(expected) if expected == authority => Ok(()),
        COption::Some(expected) => Err(CliError::MintTokens(format!(
            "mint {mint_address} has authority {expected}, but the signing key is {authority}"
        ))),
        COption::None => Err(CliError::MintTokens(format!(
            "mint {mint_address} has fixed supply and no mint authority"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_mint_tokens_arguments() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "mint-tokens",
            "recipient",
            "mint",
            "1.25",
            "--key",
            "authority.key",
        ])
        .unwrap();
        let Command::MintTokens(args) = cli.command else {
            panic!("expected mint-tokens command");
        };
        assert_eq!(args.recipient, "recipient");
        assert_eq!(args.mint, "mint");
        assert_eq!(args.amount, "1.25");
        assert_eq!(args.key, PathBuf::from("authority.key"));
    }

    #[test]
    fn requires_a_key() {
        assert!(
            Cli::try_parse_from(["arch-kit", "mint-tokens", "recipient", "mint", "1"]).is_err()
        );
    }

    #[test]
    fn validates_the_mint_authority() {
        let mint_address = Pubkey::from([1; 32]);
        let authority = Pubkey::from([2; 32]);
        let mut mint = apl_token::state::Mint {
            is_initialized: true,
            mint_authority: COption::Some(authority),
            ..apl_token::state::Mint::default()
        };

        assert!(require_mint_authority(mint_address, &mint, authority).is_ok());
        assert!(require_mint_authority(mint_address, &mint, Pubkey::from([3; 32])).is_err());
        mint.mint_authority = COption::None;
        assert!(require_mint_authority(mint_address, &mint, authority).is_err());
        mint.is_initialized = false;
        assert!(require_mint_authority(mint_address, &mint, authority).is_err());
    }
}
