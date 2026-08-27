use std::path::PathBuf;

use apl_token::{instruction::initialize_mint, state::Mint};
use arch_sdk::{
    Config,
    arch_program::{
        instruction::Instruction, program_pack::Pack, pubkey::Pubkey, rent::minimum_rent,
        system_instruction,
    },
    blocking::ArchRpcClient,
};

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
    token::parse_pubkey,
    transaction::send_and_confirm,
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Existing secret key file for the new mint account.
    #[arg(long, value_name = "PATH")]
    pub(crate) mint_key: PathBuf,

    /// Existing payer key; this account also becomes the mint authority.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,

    /// Number of base-10 digits after the decimal point.
    #[arg(long, default_value_t = 9, value_name = "COUNT")]
    pub(crate) decimals: u8,

    /// Optional freeze authority as a Base58 or hexadecimal Arch public key.
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) freeze_authority: Option<String>,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let (authority_keypair, authority) = load_existing_key(&args.key, "mint authority key")?;
    let (mint_keypair, mint) = load_existing_key(&args.mint_key, "mint key")?;
    require_distinct_accounts(authority, mint)?;

    let freeze_authority = args
        .freeze_authority
        .as_deref()
        .map(|value| parse_pubkey(value, "freeze authority"))
        .transpose()?;
    let instructions = create_mint_instructions(mint, authority, freeze_authority, args.decimals)?;
    let transaction_id = send_and_confirm(
        &ArchRpcClient::new(config),
        "mint creation",
        instructions,
        authority,
        vec![authority_keypair, mint_keypair],
    )?;

    println!("Mint created");
    println!("  Transaction: {transaction_id}");
    println!("  Mint: {mint}");
    println!("  Decimals: {}", args.decimals);
    println!("  Supply: 0");
    println!("  Mint authority: {authority}");
    println!(
        "  Freeze authority: {}",
        freeze_authority
            .map(|pubkey| pubkey.to_string())
            .as_deref()
            .unwrap_or("none")
    );
    Ok(())
}

fn require_distinct_accounts(authority: Pubkey, mint: Pubkey) -> Result<()> {
    if authority == mint {
        return Err(CliError::MintCreation(format!(
            "payer and mint account are both {mint}"
        )));
    }
    Ok(())
}

fn create_mint_instructions(
    mint: Pubkey,
    authority: Pubkey,
    freeze_authority: Option<Pubkey>,
    decimals: u8,
) -> Result<Vec<Instruction>> {
    let create_account = system_instruction::create_account(
        &authority,
        &mint,
        minimum_rent(Mint::LEN),
        Mint::LEN as u64,
        &apl_token::id(),
    );
    let initialize = initialize_mint(
        &apl_token::id(),
        &mint,
        &authority,
        freeze_authority.as_ref(),
        decimals,
    )
    .map_err(|error| {
        CliError::MintCreation(format!("could not build initialize instruction: {error}"))
    })?;
    Ok(vec![create_account, initialize])
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_keys_and_defaults_to_nine_decimals() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "create-mint",
            "--mint-key",
            "mint.key",
            "--key",
            "authority.key",
        ])
        .unwrap();
        let Command::CreateMint(args) = cli.command else {
            panic!("expected create-mint command");
        };
        assert_eq!(args.mint_key, PathBuf::from("mint.key"));
        assert_eq!(args.key, PathBuf::from("authority.key"));
        assert_eq!(args.decimals, 9);
        assert!(args.freeze_authority.is_none());
    }

    #[test]
    fn parses_explicit_decimals_and_freeze_authority() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "create-mint",
            "--mint-key",
            "mint.key",
            "--key",
            "authority.key",
            "--decimals",
            "6",
            "--freeze-authority",
            "freeze",
        ])
        .unwrap();
        let Command::CreateMint(args) = cli.command else {
            panic!("expected create-mint command");
        };
        assert_eq!(args.decimals, 6);
        assert_eq!(args.freeze_authority.as_deref(), Some("freeze"));
    }

    #[test]
    fn requires_both_keys() {
        assert!(
            Cli::try_parse_from(["arch-kit", "create-mint", "--mint-key", "mint.key"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["arch-kit", "create-mint", "--key", "authority.key"]).is_err()
        );
    }

    #[test]
    fn builds_atomic_create_and_initialize_instructions() {
        let mint = Pubkey::from([1; 32]);
        let authority = Pubkey::from([2; 32]);
        let freeze_authority = Pubkey::from([3; 32]);

        let instructions =
            create_mint_instructions(mint, authority, Some(freeze_authority), 6).unwrap();

        assert_eq!(
            instructions,
            vec![
                system_instruction::create_account(
                    &authority,
                    &mint,
                    minimum_rent(Mint::LEN),
                    Mint::LEN as u64,
                    &apl_token::id(),
                ),
                initialize_mint(
                    &apl_token::id(),
                    &mint,
                    &authority,
                    Some(&freeze_authority),
                    6,
                )
                .unwrap(),
            ]
        );
        assert!(require_distinct_accounts(authority, authority).is_err());
        assert!(require_distinct_accounts(authority, mint).is_ok());
    }
}
