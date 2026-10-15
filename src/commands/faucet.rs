use std::path::PathBuf;

use arch_sdk::{Config, blocking::ArchRpcClient};

use crate::{error::Result, keys::load_existing_key, utils::format_amount};

const ARCH_DECIMALS: u8 = 9;

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Secret key file for the account to create or fund.
    #[arg(long, value_name = "PATH")]
    pub(crate) key: PathBuf,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let (keypair, pubkey) = load_existing_key(&args.key, "faucet account key")?;
    let client = ArchRpcClient::new(config);

    println!("Requesting faucet funding for {pubkey}...");
    client.create_and_fund_account_with_faucet(&keypair)?;

    let balance = client.read_account_info(pubkey)?.lamports;
    println!("Faucet funding completed");
    println!("  Account: {pubkey}");
    println!(
        "  Balance: {} ARCH ({balance} lamports)",
        format_amount(balance, ARCH_DECIMALS)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use arch_sdk::{
        ACCOUNT_FUNDING_AMOUNT, AccountInfo, RuntimeTransaction,
        arch_program::{
            hash::Hash, pubkey::Pubkey, sanitized::ArchMessage, system_instruction, system_program,
        },
        build_and_sign_transaction,
    };
    use bitcoin::{
        Network,
        key::Keypair,
        secp256k1::{Secp256k1, SecretKey},
    };
    use clap::Parser;
    use serde_json::json;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_faucet_key() {
        let cli = Cli::try_parse_from(["arch-kit", "faucet", "--key", "sender.key"]).unwrap();
        let Command::Faucet(args) = cli.command else {
            panic!("expected faucet command");
        };
        assert_eq!(args.key, PathBuf::from("sender.key"));
    }

    #[test]
    fn requires_a_key() {
        assert!(Cli::try_parse_from(["arch-kit", "faucet"]).is_err());
    }

    #[test]
    fn mainnet_faucet_creates_and_tops_up_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let key = directory.path().join("account.key");
        std::fs::write(&key, "01".repeat(32)).unwrap();
        let (keypair, pubkey) = load_existing_key(&key, "test key").unwrap();
        let funder =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[2; 32]).unwrap());
        let funder_pubkey = Pubkey::from_slice(&funder.x_only_public_key().0.serialize());
        let message = ArchMessage::new(
            &[system_instruction::create_account(
                &funder_pubkey,
                &pubkey,
                ACCOUNT_FUNDING_AMOUNT,
                0,
                &system_program::SYSTEM_PROGRAM_ID,
            )],
            Some(funder_pubkey),
            Hash::from([0; 32]),
        );
        let transaction =
            build_and_sign_transaction(message, vec![funder, keypair], Network::Bitcoin).unwrap();
        let mut partial = transaction.clone();
        // The node signs as the faucet; the SDK must append the account signature.
        assert_eq!(partial.message.account_keys[1], pubkey);
        partial.signatures.pop().unwrap();
        let receipt = json!({"result": {
            "status": {"type": "processed"},
            "runtime_transaction": transaction,
            "bitcoin_txid": null, "logs": [], "inner_instructions_list": [],
        }});
        let read_balance = |lamports| {
            (
                "read_account_info",
                json!({"result": AccountInfo {
                    lamports,
                    owner: system_program::SYSTEM_PROGRAM_ID,
                    data: vec![],
                    is_executable: false,
                }}),
            )
        };

        for exists in [false, true] {
            let mut replies = if exists {
                vec![
                    read_balance(ACCOUNT_FUNDING_AMOUNT),
                    ("request_airdrop", json!({"result": "11".repeat(32)})),
                ]
            } else {
                vec![
                    (
                        "read_account_info",
                        json!({"error": {"code": 404, "message": "Account not found"}}),
                    ),
                    ("create_account_with_faucet", json!({"result": partial})),
                    ("send_transaction", json!({"result": "11".repeat(32)})),
                ]
            };
            let balance = if exists {
                2 * ACCOUNT_FUNDING_AMOUNT
            } else {
                ACCOUNT_FUNDING_AMOUNT
            };
            replies.extend([
                ("get_processed_transaction", receipt.clone()),
                read_balance(balance),
                read_balance(balance),
            ]);
            let (result, requests) =
                crate::test_rpc::run(replies, |config| run(config, Args { key: key.clone() }));
            result.unwrap();
            assert_eq!(requests[0]["params"], json!(pubkey));
            assert_eq!(requests[1]["params"], json!(pubkey));
            if !exists {
                let submitted: RuntimeTransaction =
                    serde_json::from_value(requests[2]["params"].clone()).unwrap();
                assert_eq!(submitted.message, transaction.message);
                assert_eq!(submitted.signatures.len(), 2);
                submitted.verify_sigs(Network::Bitcoin).unwrap();
            }
        }
    }
}
