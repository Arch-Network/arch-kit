use std::path::PathBuf;

use apl_associated_token_account::create_associated_token_account_idempotent;
use apl_token::{instruction::transfer_checked, state::AccountState};
use arch_sdk::{
    Config, Status,
    arch_program::{
        hash::Hash, instruction::Instruction, pubkey::Pubkey, sanitized::ArchMessage,
        system_program,
    },
    blocking::ArchRpcClient,
    build_and_sign_transaction,
};
use bitcoin::key::Keypair;

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
    token::{associated_token_address, parse_pubkey, read_mint, read_token_account_state},
    utils::{format_amount, parse_amount},
};

#[derive(Debug, clap::Args)]
pub(crate) struct UserArgs {
    /// Recipient owner as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "RECIPIENT")]
    pub(crate) recipient: String,

    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,

    /// Human-readable token amount, interpreted using the mint's decimals.
    #[arg(value_name = "AMOUNT")]
    pub(crate) amount: String,

    /// Secret key file for the source owner and transaction payer.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,

    /// Source token account; defaults to the signer's ATA for the mint.
    #[arg(long, value_name = "TOKEN_ACCOUNT")]
    pub(crate) source: Option<String>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct AccountArgs {
    /// Destination APL token account as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "DESTINATION")]
    pub(crate) destination: String,

    /// Token mint as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "MINT")]
    pub(crate) mint: String,

    /// Human-readable token amount, interpreted using the mint's decimals.
    #[arg(value_name = "AMOUNT")]
    pub(crate) amount: String,

    /// Secret key file for the source owner and transaction payer.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,

    /// Source token account; defaults to the signer's ATA for the mint.
    #[arg(long, value_name = "TOKEN_ACCOUNT")]
    pub(crate) source: Option<String>,
}

pub(crate) fn run_to_user(config: &Config, args: UserArgs) -> Result<()> {
    let recipient = parse_pubkey(&args.recipient, "recipient")?;
    run(
        config,
        TransferArgs {
            destination: Destination::User(recipient),
            mint: args.mint,
            amount: args.amount,
            key: args.key,
            source: args.source,
        },
    )
}

pub(crate) fn run_to_account(config: &Config, args: AccountArgs) -> Result<()> {
    let destination = parse_pubkey(&args.destination, "destination token account")?;
    run(
        config,
        TransferArgs {
            destination: Destination::Account(destination),
            mint: args.mint,
            amount: args.amount,
            key: args.key,
            source: args.source,
        },
    )
}

enum Destination {
    User(Pubkey),
    Account(Pubkey),
}

struct TransferArgs {
    destination: Destination,
    mint: String,
    amount: String,
    key: PathBuf,
    source: Option<String>,
}

fn run(config: &Config, args: TransferArgs) -> Result<()> {
    let (keypair, authority) = load_existing_key(&args.key, "transfer key")?;
    let mint_address = parse_pubkey(&args.mint, "mint")?;
    let client = ArchRpcClient::new(config);
    let mint = read_mint(&client, mint_address)?;
    if !mint.state.is_initialized {
        return Err(CliError::TokenTransfer(format!(
            "mint {mint_address} is not initialized"
        )));
    }
    let amount = parse_amount(&args.amount, mint.state.decimals)?;

    let source = args
        .source
        .as_deref()
        .map(|value| parse_pubkey(value, "source token account"))
        .transpose()?
        .unwrap_or_else(|| associated_token_address(&authority, &mint_address).0);
    let source_state = read_token_account_state(&client, source)?;
    validate_source(source, &source_state, authority, mint_address, amount)?;

    let (destination, recipient) = match args.destination {
        Destination::User(recipient) => (
            associated_token_address(&recipient, &mint_address).0,
            Some(recipient),
        ),
        Destination::Account(destination) => {
            let destination_state = read_token_account_state(&client, destination)?;
            validate_destination(destination, &destination_state, mint_address)?;
            (destination, None)
        }
    };
    require_distinct_accounts(source, destination)?;

    let instructions = transfer_instructions(
        source,
        destination,
        recipient,
        mint_address,
        authority,
        amount,
        mint.state.decimals,
    )?;
    let transaction_id = send(&client, authority, keypair, instructions)?;

    println!("Token transfer completed");
    println!("  Transaction: {transaction_id}");
    println!("  Mint: {mint_address}");
    println!("  Source: {source}");
    println!("  Destination: {destination}");
    println!(
        "  Amount: {} ({amount} raw)",
        format_amount(amount, mint.state.decimals)
    );
    Ok(())
}

fn validate_source(
    address: Pubkey,
    account: &apl_token::state::Account,
    authority: Pubkey,
    mint: Pubkey,
    amount: u64,
) -> Result<()> {
    if account.mint != mint {
        return Err(CliError::TokenTransfer(format!(
            "source account {address} uses mint {}, expected {mint}",
            account.mint
        )));
    }
    if account.owner != authority {
        return Err(CliError::TokenTransfer(format!(
            "source account {address} is owned by {}, expected signer {authority}",
            account.owner
        )));
    }
    require_initialized(address, account.state, "source")?;
    if account.amount < amount {
        return Err(CliError::TokenTransfer(format!(
            "source account {address} balance {} is smaller than requested amount {amount}",
            account.amount
        )));
    }
    Ok(())
}

fn validate_destination(
    address: Pubkey,
    account: &apl_token::state::Account,
    mint: Pubkey,
) -> Result<()> {
    if account.mint != mint {
        return Err(CliError::TokenTransfer(format!(
            "destination account {address} uses mint {}, expected {mint}",
            account.mint
        )));
    }
    require_initialized(address, account.state, "destination")
}

fn require_initialized(address: Pubkey, state: AccountState, label: &str) -> Result<()> {
    if state == AccountState::Initialized {
        return Ok(());
    }
    Err(CliError::TokenTransfer(format!(
        "{label} account {address} is {}",
        crate::token::account_state_name(state)
    )))
}

fn require_distinct_accounts(source: Pubkey, destination: Pubkey) -> Result<()> {
    if source != destination {
        return Ok(());
    }
    Err(CliError::TokenTransfer(format!(
        "source and destination are both {source}"
    )))
}

#[allow(clippy::too_many_arguments)]
fn transfer_instructions(
    source: Pubkey,
    destination: Pubkey,
    recipient: Option<Pubkey>,
    mint: Pubkey,
    authority: Pubkey,
    amount: u64,
    decimals: u8,
) -> Result<Vec<Instruction>> {
    let mut instructions = Vec::with_capacity(if recipient.is_some() { 2 } else { 1 });
    if let Some(recipient) = recipient {
        instructions.push(create_associated_token_account_idempotent(
            &authority,
            &destination,
            &recipient,
            &mint,
            &apl_token::id(),
            &system_program::SYSTEM_PROGRAM_ID,
        ));
    }
    instructions.push(
        transfer_checked(
            &apl_token::id(),
            &source,
            &mint,
            &destination,
            &authority,
            &[],
            amount,
            decimals,
        )
        .map_err(|error| {
            CliError::TokenTransfer(format!("could not build transfer instruction: {error}"))
        })?,
    );
    Ok(instructions)
}

fn send(
    client: &ArchRpcClient,
    payer: Pubkey,
    keypair: Keypair,
    instructions: Vec<Instruction>,
) -> Result<Hash> {
    let message = ArchMessage::new(
        &instructions,
        Some(payer),
        client.get_best_finalized_block_hash()?,
    );
    let transaction = build_and_sign_transaction(message, vec![keypair], client.config.network)?;
    let transaction_id = client.send_transaction(transaction)?;
    let processed = client.wait_for_processed_transaction(&transaction_id)?;
    if processed.status != Status::Processed {
        return Err(CliError::TransactionFailed {
            action: "token transfer".to_string(),
            status: format!("{:?}", processed.status),
        });
    }
    Ok(transaction_id)
}

#[cfg(test)]
mod tests {
    use apl_token::instruction::TokenInstruction;
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_user_transfer_arguments() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "token-transfer",
            "recipient",
            "mint",
            "1.25",
            "--key",
            "owner.key",
            "--source",
            "source",
        ])
        .unwrap();
        let Command::TokenTransfer(args) = cli.command else {
            panic!("expected token-transfer command");
        };
        assert_eq!(args.recipient, "recipient");
        assert_eq!(args.amount, "1.25");
        assert_eq!(args.key, PathBuf::from("owner.key"));
        assert_eq!(args.source.as_deref(), Some("source"));
    }

    #[test]
    fn parses_direct_account_transfer_arguments() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "token-transfer-to-account",
            "destination",
            "mint",
            "2",
            "--key",
            "owner.key",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::TokenTransferToAccount(_)));
    }

    #[test]
    fn requires_a_transfer_key() {
        assert!(
            Cli::try_parse_from(["arch-kit", "token-transfer", "recipient", "mint", "1"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "arch-kit",
                "token-transfer-to-account",
                "destination",
                "mint",
                "1"
            ])
            .is_err()
        );
    }

    #[test]
    fn builds_user_transfer_with_idempotent_ata_creation() {
        let authority = Pubkey::from([1; 32]);
        let recipient = Pubkey::from([2; 32]);
        let mint = Pubkey::from([3; 32]);
        let source = Pubkey::from([4; 32]);
        let destination = associated_token_address(&recipient, &mint).0;

        let instructions = transfer_instructions(
            source,
            destination,
            Some(recipient),
            mint,
            authority,
            125,
            2,
        )
        .unwrap();

        assert_eq!(instructions.len(), 2);
        assert_eq!(
            instructions[0].program_id,
            apl_associated_token_account::id()
        );
        assert_eq!(instructions[0].data, vec![1]);
        assert_eq!(instructions[0].accounts[0].pubkey, authority);
        assert_eq!(instructions[0].accounts[1].pubkey, destination);
        assert_eq!(instructions[0].accounts[2].pubkey, recipient);
        assert_eq!(instructions[1].program_id, apl_token::id());
        assert_eq!(instructions[1].accounts[0].pubkey, source);
        assert_eq!(instructions[1].accounts[1].pubkey, mint);
        assert_eq!(instructions[1].accounts[2].pubkey, destination);
        assert_eq!(instructions[1].accounts[3].pubkey, authority);
        assert_eq!(
            TokenInstruction::unpack(&instructions[1].data).unwrap(),
            TokenInstruction::TransferChecked {
                amount: 125,
                decimals: 2
            }
        );
    }

    #[test]
    fn builds_direct_transfer_without_ata_creation() {
        let source = Pubkey::from([1; 32]);
        let destination = Pubkey::from([2; 32]);
        let mint = Pubkey::from([3; 32]);
        let authority = Pubkey::from([4; 32]);

        let instructions =
            transfer_instructions(source, destination, None, mint, authority, 7, 0).unwrap();

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].program_id, apl_token::id());
        assert_eq!(instructions[0].accounts[2].pubkey, destination);
    }

    #[test]
    fn validates_source_and_destination_accounts() {
        let mint = Pubkey::from([1; 32]);
        let authority = Pubkey::from([2; 32]);
        let address = Pubkey::from([3; 32]);
        let valid = apl_token::state::Account {
            mint,
            owner: authority,
            amount: 10,
            state: AccountState::Initialized,
            ..apl_token::state::Account::default()
        };
        assert!(validate_source(address, &valid, authority, mint, 10).is_ok());
        assert!(validate_destination(address, &valid, mint).is_ok());

        let mut invalid = valid;
        invalid.owner = Pubkey::from([4; 32]);
        assert!(validate_source(address, &invalid, authority, mint, 1).is_err());
        invalid = valid;
        invalid.mint = Pubkey::from([5; 32]);
        assert!(validate_source(address, &invalid, authority, mint, 1).is_err());
        assert!(validate_destination(address, &invalid, mint).is_err());
        invalid = valid;
        invalid.state = AccountState::Frozen;
        assert!(validate_source(address, &invalid, authority, mint, 1).is_err());
        assert!(validate_destination(address, &invalid, mint).is_err());
        invalid.state = AccountState::Uninitialized;
        assert!(validate_source(address, &invalid, authority, mint, 1).is_err());
        assert!(validate_destination(address, &invalid, mint).is_err());
        assert!(validate_source(address, &valid, authority, mint, 11).is_err());
        assert!(require_distinct_accounts(address, address).is_err());
    }
}
