//! Account preparation against a local JSON-RPC server; no chain or real keys.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

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

// A finite response script makes extra RPC calls or uploads fail the test.
fn run_rpc(replies: Vec<(&'static str, Value)>) -> (Result<()>, Vec<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        let mut submitted = Value::Null;
        for (method, mut response) in replies {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing RPC call {method}");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("RPC accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], method);
            if method == "send_transaction" {
                submitted = request["params"].clone();
            }
            if method == "get_processed_transaction" {
                response["result"]["runtime_transaction"] = submitted.clone();
            }
            response["jsonrpc"] = json!("2.0");
            response["id"] = request["id"].clone();
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            requests.push(request);
        }
        requests
    });
    let config = Config {
        arch_node_url: endpoint,
        network: bitcoin::Network::Bitcoin,
        ..Config::localnet()
    };
    let result = prepare_program_account(&config, keypair(1), keypair(2));
    (result, server.join().unwrap())
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
