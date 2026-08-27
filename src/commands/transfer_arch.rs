use std::path::PathBuf;

use arch_sdk::{
    AccountInfo, Config,
    arch_program::{instruction::Instruction, pubkey::Pubkey, system_instruction, system_program},
    blocking::ArchRpcClient,
};

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
    token::parse_pubkey,
    transaction::send_and_confirm,
    utils::{format_amount, parse_amount},
};

const ARCH_DECIMALS: u8 = 9;
const BASE_FEE_PER_SIGNATURE: u64 = 5_000;

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Destination account as a Base58 or hexadecimal Arch public key.
    #[arg(value_name = "DESTINATION")]
    pub(crate) destination: String,

    /// Human-readable ARCH amount.
    #[arg(value_name = "AMOUNT")]
    pub(crate) amount: String,

    /// Secret key file for the sender and transaction payer.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let (keypair, source) = load_existing_key(&args.key, "transfer key")?;
    let destination = parse_pubkey(&args.destination, "destination")?;
    require_distinct_accounts(source, destination)?;

    let amount = parse_amount(&args.amount, ARCH_DECIMALS)?;
    let required_balance = amount
        .checked_add(BASE_FEE_PER_SIGNATURE)
        .ok_or_else(|| CliError::NativeTransfer("amount plus fee overflows u64".to_string()))?;

    let client = ArchRpcClient::new(config);
    let source_account = client.read_account_info(source)?;
    validate_source_account(source, &source_account, required_balance)?;

    let instruction = transfer_instruction(source, destination, amount);
    let transaction_id = send_and_confirm(
        &client,
        "native ARCH transfer",
        vec![instruction],
        source,
        vec![keypair],
    )?;

    println!("ARCH transfer completed");
    println!("  Transaction: {transaction_id}");
    println!("  Source: {source}");
    println!("  Destination: {destination}");
    println!(
        "  Amount: {} ARCH ({amount} lamports)",
        format_amount(amount, ARCH_DECIMALS)
    );
    println!(
        "  Fee: {} ARCH ({BASE_FEE_PER_SIGNATURE} lamports)",
        format_amount(BASE_FEE_PER_SIGNATURE, ARCH_DECIMALS)
    );
    Ok(())
}

fn require_distinct_accounts(source: Pubkey, destination: Pubkey) -> Result<()> {
    if source != destination {
        return Ok(());
    }
    Err(CliError::NativeTransfer(format!(
        "source and destination are both {source}"
    )))
}

fn transfer_instruction(source: Pubkey, destination: Pubkey, amount: u64) -> Instruction {
    system_instruction::transfer(&source, &destination, amount)
}

fn validate_source_account(
    address: Pubkey,
    account: &AccountInfo,
    required_balance: u64,
) -> Result<()> {
    if account.owner != system_program::SYSTEM_PROGRAM_ID {
        return Err(CliError::NativeTransfer(format!(
            "source account {address} is owned by {}, expected system program {}",
            account.owner,
            system_program::SYSTEM_PROGRAM_ID
        )));
    }
    if !account.data.is_empty() {
        return Err(CliError::NativeTransfer(format!(
            "source account {address} contains data and cannot fund a native ARCH transfer"
        )));
    }
    if account.is_executable {
        return Err(CliError::NativeTransfer(format!(
            "source account {address} is executable and cannot fund a native ARCH transfer"
        )));
    }
    if account.lamports < required_balance {
        return Err(CliError::NativeTransfer(format!(
            "source account {address} has {} lamports, but the transfer and fee require {required_balance}",
            account.lamports
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_transfer_arguments() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "transfer-arch",
            "destination",
            "0.1",
            "--key",
            "owner.key",
        ])
        .unwrap();
        let Command::TransferArch(args) = cli.command else {
            panic!("expected transfer-arch command");
        };
        assert_eq!(args.destination, "destination");
        assert_eq!(args.amount, "0.1");
        assert_eq!(args.key, PathBuf::from("owner.key"));
        assert_eq!(
            parse_amount(&args.amount, ARCH_DECIMALS).unwrap(),
            100_000_000
        );
    }

    #[test]
    fn requires_a_transfer_key() {
        assert!(Cli::try_parse_from(["arch-kit", "transfer-arch", "destination", "0.1"]).is_err());
    }

    #[test]
    fn builds_a_native_system_transfer() {
        let source = Pubkey::from([1; 32]);
        let destination = Pubkey::from([2; 32]);
        assert_eq!(
            transfer_instruction(source, destination, 100_000_000),
            system_instruction::transfer(&source, &destination, 100_000_000)
        );
    }

    #[test]
    fn validates_source_account_and_balance() {
        let address = Pubkey::from([1; 32]);
        let valid = AccountInfo {
            lamports: 100_005_000,
            owner: system_program::SYSTEM_PROGRAM_ID,
            data: Vec::new(),
            utxo: String::new(),
            is_executable: false,
        };
        assert!(validate_source_account(address, &valid, 100_005_000).is_ok());

        let mut invalid = valid.clone();
        invalid.lamports -= 1;
        assert!(validate_source_account(address, &invalid, 100_005_000).is_err());
        invalid = valid.clone();
        invalid.owner = Pubkey::from([2; 32]);
        assert!(validate_source_account(address, &invalid, 100_005_000).is_err());
        invalid = valid.clone();
        invalid.data.push(1);
        assert!(validate_source_account(address, &invalid, 100_005_000).is_err());
        invalid = valid;
        invalid.is_executable = true;
        assert!(validate_source_account(address, &invalid, 100_005_000).is_err());
        assert!(require_distinct_accounts(address, address).is_err());
    }
}
