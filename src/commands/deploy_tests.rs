//! Account preparation against a local JSON-RPC server; no chain or real keys.
use arch_sdk::{AccountInfo, RuntimeTransaction, arch_program::hash::Hash};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use serde_json::{Value, json};

use super::*;

fn keypair(byte: u8) -> Keypair {
    Keypair::from_secret_key(
        &Secp256k1::new(),
        &SecretKey::from_slice(&[byte; 32]).unwrap(),
    )
}

fn account(owner: Pubkey) -> AccountInfo {
    AccountInfo {
        lamports: 500_000_000,
        owner,
        data: vec![],
        utxo: format!("{}:0", "00".repeat(32)),
        is_executable: false,
    }
}

fn processed(status: Value, rollback: Value) -> Value {
    json!({"result": {
        "status": status, "rollback_status": rollback,
        "bitcoin_txid": null, "logs": [], "inner_instructions_list": [],
    }})
}

fn assignment_replies(status: Value, rollback: Value) -> Vec<(&'static str, Value)> {
    vec![
        (
            "read_account_info",
            json!({"result": account(system_program::SYSTEM_PROGRAM_ID)}),
        ),
        (
            "get_best_finalized_block_hash",
            json!({"result": "00".repeat(32)}),
        ),
        ("send_transaction", json!({"result": "11".repeat(32)})),
        ("get_processed_transaction", processed(status, rollback)),
    ]
}

fn run_rpc(replies: Vec<(&'static str, Value)>) -> (Result<()>, Vec<Value>) {
    crate::test_rpc::run(replies, |config| {
        prepare_program_account(config, keypair(1), keypair(2))
    })
}

#[test]
fn funded_system_account_is_assigned_with_both_mainnet_signatures() {
    let mut replies = assignment_replies(
        json!({"type": "processed"}),
        json!({"type": "notRolledback"}),
    );
    replies.push((
        "read_account_info",
        json!({"result": account(BPF_LOADER_ID)}),
    ));
    let (result, requests) = run_rpc(replies);
    result.unwrap();
    let tx: RuntimeTransaction = serde_json::from_value(requests[2]["params"].clone()).unwrap();
    let program = Pubkey::from_slice(&keypair(1).x_only_public_key().0.serialize());
    let authority = Pubkey::from_slice(&keypair(2).x_only_public_key().0.serialize());
    let expected = ArchMessage::new(
        &[system_instruction::assign(&program, &BPF_LOADER_ID)],
        Some(authority),
        Hash::from([0; 32]),
    );
    assert_eq!(tx.message, expected);
    assert_eq!(tx.signatures.len(), 2);
    tx.verify_sigs(bitcoin::Network::Bitcoin).unwrap();
    assert_eq!(requests[0]["params"], json!(program));
    assert_eq!(requests[4]["params"], json!(program));
}

#[test]
fn missing_and_loader_owned_accounts_need_no_assignment() {
    for response in [
        json!({"error": {"code": 404, "message": "Account not found"}}),
        json!({"result": account(BPF_LOADER_ID)}),
    ] {
        run_rpc(vec![("read_account_info", response)]).0.unwrap();
    }
    let mut deployed = account(BPF_LOADER_ID);
    deployed.data = vec![1; 128];
    deployed.is_executable = true;
    run_rpc(vec![("read_account_info", json!({"result": deployed}))])
        .0
        .unwrap();
}

#[test]
fn invalid_accounts_and_rpc_errors_stop_before_submission() {
    let system = account(system_program::SYSTEM_PROGRAM_ID);
    let mut with_data = system.clone();
    with_data.data = vec![0];
    let mut executable = system;
    executable.is_executable = true;
    for invalid in [account(Pubkey::from([7; 32])), with_data, executable] {
        assert!(matches!(
            run_rpc(vec![("read_account_info", json!({"result": invalid}))]).0,
            Err(CliError::InvalidArgument(_)),
        ));
    }
    assert!(matches!(
        run_rpc(vec![(
            "read_account_info",
            json!({"error": {"code": -32603, "message": "RPC unavailable"}})
        )])
        .0,
        Err(CliError::ArchRpc(_)),
    ));
}

#[test]
fn failed_or_rolled_back_assignment_stops_deployment() {
    for (status, rollback) in [
        (
            json!({"type": "failed", "message": "Insufficient funds"}),
            json!({"type": "notRolledback"}),
        ),
        (
            json!({"type": "processed"}),
            json!({"type": "rolledback", "message": "Reverted"}),
        ),
    ] {
        let (result, _) = run_rpc(assignment_replies(status, rollback));
        assert!(matches!(result, Err(CliError::TransactionFailed { .. })));
    }
}

#[test]
fn unchanged_owner_after_processed_assignment_stops_deployment() {
    let mut replies = assignment_replies(
        json!({"type": "processed"}),
        json!({"type": "notRolledback"}),
    );
    replies.push((
        "read_account_info",
        json!({"result": account(system_program::SYSTEM_PROGRAM_ID)}),
    ));
    let (result, _) = run_rpc(replies);
    assert!(result.unwrap_err().to_string().contains("still has owner"));
}

#[test]
fn program_cannot_also_pay_deployment_fees() {
    let result = prepare_program_account(&Config::localnet(), keypair(1), keypair(1));
    assert!(result.unwrap_err().to_string().contains("different keys"));
}

fn deployment_args(expect_program_id: Option<String>) -> (tempfile::TempDir, Args) {
    let directory = tempfile::tempdir().unwrap();
    let elf = directory.path().join("program.so");
    let program_key = directory.path().join("program.key");
    let authority = directory.path().join("authority.key");
    std::fs::write(&elf, b"test ELF; deployment must not be reached").unwrap();
    std::fs::write(&program_key, hex::encode(keypair(1).secret_bytes())).unwrap();
    std::fs::write(&authority, hex::encode(keypair(2).secret_bytes())).unwrap();
    let args = Args {
        elf,
        program_key,
        expect_program_id,
        authority,
        generate_if_missing: false,
        fund_authority: false,
        idl: None,
        idl_size: None,
        allow_idl_resize: false,
    };
    (directory, args)
}

#[test]
fn expected_program_id_mismatch_stops_before_authority_setup_or_funding() {
    let actual = Pubkey::from_slice(&keypair(1).x_only_public_key().0.serialize());
    let expected = Pubkey::from_slice(&keypair(2).x_only_public_key().0.serialize());
    for value in [expected.to_string(), pubkey_hex(&expected)] {
        let (directory, mut args) = deployment_args(Some(value));
        args.authority = directory.path().join("missing-authority.key");
        args.generate_if_missing = true;
        args.fund_authority = true;
        let authority_path = args.authority.clone();
        let (result, requests) = crate::test_rpc::run(vec![], |config| run(config, args));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("program ID mismatch"));
        assert!(error.contains(&expected.to_string()));
        assert!(error.contains(&actual.to_string()));
        assert!(!authority_path.exists());
        assert!(requests.is_empty());
    }
}

#[test]
fn matching_or_omitted_expected_program_id_reaches_account_preparation() {
    let program = Pubkey::from_slice(&keypair(1).x_only_public_key().0.serialize());
    for value in [
        None,
        Some(program.to_string()),
        Some(pubkey_hex(&program)),
        Some(pubkey_hex(&program).to_uppercase()),
    ] {
        let (_directory, args) = deployment_args(value);
        let (result, requests) = crate::test_rpc::run(
            vec![(
                "read_account_info",
                json!({"error": {"code": -32603, "message": "stop before deployment"}}),
            )],
            |config| run(config, args),
        );
        assert!(matches!(result, Err(CliError::ArchRpc(_))));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["params"], json!(program));
    }
}

#[test]
fn malformed_expected_program_id_fails_before_loading_files() {
    for value in [
        "".to_string(),
        "not-a-public-key".to_string(),
        "ab".repeat(31),
        "ab".repeat(33),
    ] {
        let (directory, mut args) = deployment_args(Some(value));
        args.elf = directory.path().join("missing.so");
        let (result, requests) = crate::test_rpc::run(vec![], |config| run(config, args));
        let error = result.unwrap_err();
        assert!(matches!(error, CliError::InvalidArgument(_)));
        assert!(error.to_string().contains("--expect-program-id"));
        assert!(requests.is_empty());
    }
}
