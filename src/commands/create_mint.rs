use apl_token::{
    instruction::{AuthorityType, initialize_mint, set_authority},
    state::Mint,
};
use arch_sdk::{
    Config,
    arch_program::{
        instruction::Instruction, program_pack::Pack, pubkey::Pubkey, rent::minimum_rent,
        system_instruction,
    },
    blocking::ArchRpcClient,
};

use crate::{
    arch_signer::SignerSource,
    error::{CliError, Result},
    token::{mint_to_user_instructions, parse_pubkey},
    transaction::send_and_confirm,
    utils::{format_amount, parse_amount},
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Mint signer source: a path, file:<PATH>, or cosigner:<ENV_PREFIX>.
    #[arg(long, visible_alias = "mint-key", value_name = "SOURCE")]
    pub(crate) mint_signer: SignerSource,

    /// Authority signer source: a path, file:<PATH>, or cosigner:<ENV_PREFIX>.
    #[arg(long, visible_alias = "key", value_name = "SOURCE")]
    pub(crate) signer: SignerSource,

    /// Number of base-10 digits after the decimal point.
    #[arg(long, default_value_t = 9, value_name = "COUNT")]
    pub(crate) decimals: u8,

    /// Optional freeze authority as a Base58 or hexadecimal Arch public key.
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) freeze_authority: Option<String>,

    /// Human-readable amount to mint into the authority's ATA during creation.
    #[arg(long, value_name = "AMOUNT")]
    pub(crate) initial_supply: Option<String>,

    /// Permanently revoke mint authority after issuing the initial supply.
    #[arg(long, requires = "initial_supply")]
    pub(crate) fixed_supply: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let authority_signer =
        args.signer
            .resolve(config.network, "create-mint", "mint authority key")?;
    let mint_signer = args
        .mint_signer
        .resolve(config.network, "create-mint", "mint key")?;
    let authority = authority_signer.pubkey();
    let mint = mint_signer.pubkey();
    require_distinct_accounts(authority, mint)?;

    let freeze_authority = args
        .freeze_authority
        .as_deref()
        .map(|value| parse_pubkey(value, "freeze authority"))
        .transpose()?;
    let initial_supply = args
        .initial_supply
        .as_deref()
        .map(|amount| parse_amount(amount, args.decimals))
        .transpose()?;
    let instructions = create_mint_instructions(
        mint,
        authority,
        freeze_authority,
        args.decimals,
        initial_supply,
        args.fixed_supply,
    )?;
    let transaction_id = send_and_confirm(
        &ArchRpcClient::new(config),
        "mint creation",
        instructions,
        authority,
        &[authority_signer.as_ref(), mint_signer.as_ref()],
    )?;

    println!("Mint created");
    println!("  Transaction: {transaction_id}");
    println!("  Mint: {mint}");
    println!("  Decimals: {}", args.decimals);
    let supply = initial_supply.unwrap_or(0);
    println!(
        "  Supply: {} ({supply} raw)",
        format_amount(supply, args.decimals)
    );
    println!(
        "  Mint authority: {}",
        if args.fixed_supply {
            "none".to_string()
        } else {
            authority.to_string()
        }
    );
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
    initial_supply: Option<u64>,
    fixed_supply: bool,
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
    let mut instructions = vec![create_account, initialize];
    if let Some(amount) = initial_supply {
        let (_, mint_instructions) =
            mint_to_user_instructions(authority, authority, mint, authority, amount, decimals)?;
        instructions.extend(mint_instructions);
    }
    if fixed_supply {
        instructions.push(
            set_authority(
                &apl_token::id(),
                &mint,
                None,
                AuthorityType::MintTokens,
                &authority,
                &[],
            )
            .map_err(|error| {
                CliError::MintCreation(format!("could not build fixed-supply instruction: {error}"))
            })?,
        );
    }
    Ok(instructions)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};
    use crate::token::associated_token_address;

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
        assert_eq!(args.mint_signer, SignerSource::File("mint.key".into()));
        assert_eq!(args.signer, SignerSource::File("authority.key".into()));
        assert_eq!(args.decimals, 9);
        assert!(args.freeze_authority.is_none());
        assert!(args.initial_supply.is_none());
        assert!(!args.fixed_supply);
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
            "--initial-supply",
            "1000.5",
            "--fixed-supply",
        ])
        .unwrap();
        let Command::CreateMint(args) = cli.command else {
            panic!("expected create-mint command");
        };
        assert_eq!(args.decimals, 6);
        assert_eq!(args.freeze_authority.as_deref(), Some("freeze"));
        assert_eq!(args.initial_supply.as_deref(), Some("1000.5"));
        assert!(args.fixed_supply);
    }

    #[test]
    fn requires_both_keys() {
        assert!(
            Cli::try_parse_from(["arch-kit", "create-mint", "--mint-key", "mint.key"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["arch-kit", "create-mint", "--key", "authority.key"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "arch-kit",
                "create-mint",
                "--mint-key",
                "mint.key",
                "--key",
                "authority.key",
                "--fixed-supply",
            ])
            .is_err()
        );
    }

    #[test]
    fn accepts_independent_cosigner_sources() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "create-mint",
            "--mint-signer",
            "cosigner:mint",
            "--signer",
            "cosigner:authority",
        ])
        .unwrap();
        let Command::CreateMint(args) = cli.command else {
            panic!("expected create-mint command");
        };
        assert_eq!(args.mint_signer, SignerSource::Cosigner("MINT".to_string()));
        assert_eq!(args.signer, SignerSource::Cosigner("AUTHORITY".to_string()));
    }

    #[test]
    fn builds_atomic_create_and_initialize_instructions() {
        let mint = Pubkey::from([1; 32]);
        let authority = Pubkey::from([2; 32]);
        let freeze_authority = Pubkey::from([3; 32]);

        let instructions =
            create_mint_instructions(mint, authority, Some(freeze_authority), 6, None, false)
                .unwrap();

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

    #[test]
    fn appends_initial_supply_and_revokes_mint_authority() {
        let mint = Pubkey::from([1; 32]);
        let authority = Pubkey::from([2; 32]);
        let instructions =
            create_mint_instructions(mint, authority, None, 6, Some(1_500_000), true).unwrap();
        let destination = associated_token_address(&authority, &mint).0;

        assert_eq!(instructions.len(), 5);
        assert_eq!(
            instructions[2].program_id,
            apl_associated_token_account::id()
        );
        assert_eq!(instructions[2].accounts[1].pubkey, destination);
        assert_eq!(
            instructions[3],
            apl_token::instruction::mint_to_checked(
                &apl_token::id(),
                &mint,
                &destination,
                &authority,
                &[],
                1_500_000,
                6,
            )
            .unwrap()
        );
        assert_eq!(
            instructions[4],
            set_authority(
                &apl_token::id(),
                &mint,
                None,
                AuthorityType::MintTokens,
                &authority,
                &[],
            )
            .unwrap()
        );
    }
}
