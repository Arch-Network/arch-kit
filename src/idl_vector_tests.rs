//! Vectors from Arch mainnet, read once with `read_account_info` (no keys):
//! the canonical IDL accounts of PropAMM and ausd-bridge, and the transactions
//! that first published PropAMM's IDL (blocks 110290-110351).
use arch_sdk::RuntimeTransaction;
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use serde_json::json;

use super::*;

const PROPAMM: &str = "4022dec7e57f757682712b45ddf13a8f324710f4637cefc0ebacc6343caab24c";
const PROPAMM_BASE: &str = "4ac274080926f77aa376f5e6bf9b27e6581155f5710d46276efbcd0a66955986";
const PROPAMM_IDL: &str = "c90520a685fcf5e553f266d9505cf9f6c307b897d4022d63e6444ceb93ada91f";
const PROPAMM_AUTHORITY: &str = "5f4a60eba551dd057c64e38a9be8370daeb465be0fb4e276c5e9ff778132d203";
const AUSD_BRIDGE: &str = "d14535a471647c7246c9ff0d79097ebef3f914b9dce2bf6cee428893b2b5c98b";
const AUSD_BRIDGE_IDL: &str = "504db03c87e465287a33e869d86e62701d47d56fa279bcab4644f22a94258a08";
const AUSD_BRIDGE_AUTHORITY: &str =
    "47a7f482480d97a4b870f3744b3720ef912076f036580580813b7444f391c23f";

/// Header plus payload; the zero-filled remainder of each account is trimmed.
const PROPAMM_ACCOUNT: &[u8] = include_bytes!("idl_vectors/propamm_idl_account.bin");
const AUSD_BRIDGE_ACCOUNT: &[u8] = include_bytes!("idl_vectors/ausd_bridge_idl_account.bin");
/// `<txid> <instruction data hex>`: the IDL Create, then every Write in order.
const PROPAMM_PUBLISH: &str = include_str!("idl_vectors/propamm_publish.txt");

fn key(hex: &str) -> Pubkey {
    Pubkey::from(<[u8; 32]>::try_from(hex::decode(hex).unwrap()).unwrap())
}

fn mainnet_account(program: &str, bytes: &[u8], capacity: usize) -> AccountInfo {
    let mut data = bytes.to_vec();
    data.resize(capacity, 0);
    AccountInfo {
        lamports: minimum_rent(capacity),
        owner: key(program),
        data,
        is_executable: false,
    }
}

fn propamm_account() -> AccountInfo {
    mainnet_account(PROPAMM, PROPAMM_ACCOUNT, 10_000)
}

fn propamm_publication() -> Vec<Vec<u8>> {
    PROPAMM_PUBLISH
        .lines()
        .map(|line| hex::decode(line.split_once(' ').unwrap().1).unwrap())
        .collect()
}

#[test]
fn derives_the_mainnet_idl_and_base_addresses() {
    assert_eq!(
        derive_idl_addresses(&key(PROPAMM)).unwrap(),
        (key(PROPAMM_BASE), key(PROPAMM_IDL))
    );
    assert_eq!(
        idl_address(&key(AUSD_BRIDGE)).unwrap(),
        key(AUSD_BRIDGE_IDL)
    );
}

#[test]
fn decodes_the_mainnet_idl_accounts() {
    let propamm = decode_account(&propamm_account(), key(PROPAMM), key(PROPAMM_IDL)).unwrap();
    assert_eq!(propamm.authority, key(PROPAMM_AUTHORITY));
    assert_eq!(propamm.capacity, 10_000);
    assert_eq!(propamm.json["address"], PROPAMM);
    assert_eq!(propamm.json["metadata"]["name"], "PropAMM");
    assert_eq!(propamm.json["metadata"]["spec"], "0.1.0");
    assert_eq!(propamm.json["instructions"].as_array().unwrap().len(), 6);
    assert_eq!(propamm.json["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(propamm.json["errors"].as_array().unwrap().len(), 198);

    let bridge = mainnet_account(AUSD_BRIDGE, AUSD_BRIDGE_ACCOUNT, 20_000);
    let bridge = decode_account(&bridge, key(AUSD_BRIDGE), key(AUSD_BRIDGE_IDL)).unwrap();
    assert_eq!(bridge.authority, key(AUSD_BRIDGE_AUTHORITY));
    assert_eq!(bridge.capacity, 20_000);
    assert_eq!(bridge.json["address"], AUSD_BRIDGE);
    assert_eq!(bridge.json["metadata"]["name"], "Arch Bridge");
    assert_eq!(bridge.json["instructions"].as_array().unwrap().len(), 16);
    assert_eq!(bridge.json["accounts"].as_array().unwrap().len(), 6);
    assert_eq!(bridge.json["events"].as_array().unwrap().len(), 14);
    assert_eq!(bridge.json["errors"].as_array().unwrap().len(), 57);

    // Another program's IDL never decodes as this program's.
    assert!(decode_account(&propamm_account(), key(AUSD_BRIDGE), key(PROPAMM_IDL)).is_err());
}

#[test]
fn normalization_and_compression_reproduce_the_mainnet_payloads() {
    for (program, bytes) in [
        (PROPAMM, PROPAMM_ACCOUNT),
        (AUSD_BRIDGE, AUSD_BRIDGE_ACCOUNT),
    ] {
        let json = decompress_json(&bytes[IDL_HEADER_LEN..]).unwrap();
        let prepared =
            prepare_bytes(&serde_json::to_vec(&json).unwrap(), key(program), None).unwrap();
        assert_eq!(prepared.expected, json);
        assert_eq!(prepared.compressed, &bytes[IDL_HEADER_LEN..]);
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize,
            prepared.compressed.len()
        );
    }
}

#[test]
fn chunking_and_encoding_match_the_mainnet_propamm_publication() {
    let recorded = propamm_publication();
    let payload = &PROPAMM_ACCOUNT[IDL_HEADER_LEN..];
    assert_eq!(recorded[0], create_ix_data(10_000 - IDL_HEADER_LEN as u64));

    let writes = write_instructions(
        key(PROPAMM),
        key(PROPAMM_IDL),
        key(PROPAMM_AUTHORITY),
        payload,
    );
    assert_eq!(writes.len(), 8);
    assert_eq!(
        writes.iter().map(|ix| ix.data.clone()).collect::<Vec<_>>(),
        recorded[1..]
    );
    for write in &writes {
        assert_eq!(
            write.accounts,
            [
                AccountMeta::new(key(PROPAMM_IDL), false),
                AccountMeta::new_readonly(key(PROPAMM_AUTHORITY), true),
            ]
        );
    }
}

fn test_keypair() -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[2; 32]).unwrap())
}

fn successful_transaction() -> Vec<(&'static str, Value)> {
    vec![
        (
            "get_best_finalized_block_hash",
            json!({"result": "00".repeat(32)}),
        ),
        ("send_transaction", json!({"result": "11".repeat(32)})),
        (
            "get_processed_transaction",
            json!({"result": {
                "status": {"type": "processed"},
                "bitcoin_txid": null, "logs": [], "inner_instructions_list": [],
            }}),
        ),
    ]
}

#[test]
fn init_publish_replays_the_mainnet_propamm_transactions() {
    let program = key(PROPAMM);
    let keypair = test_keypair();
    let authority = Pubkey::from_slice(&keypair.x_only_public_key().0.serialize());
    let json = decompress_json(&PROPAMM_ACCOUNT[IDL_HEADER_LEN..]).unwrap();
    let prepared = prepare_bytes(&serde_json::to_vec(&json).unwrap(), program, None).unwrap();

    let recorded = propamm_publication();
    let mut replies = vec![(
        "read_account_info",
        json!({"error": {"code": 404, "message": "Account not found"}}),
    )];
    for _ in &recorded {
        replies.extend(successful_transaction());
    }
    let mut published = propamm_account();
    published.data[8..40].copy_from_slice(authority.as_ref());
    replies.push(("read_account_info", json!({"result": published})));

    let (result, requests) = crate::test_rpc::run(replies, |config| {
        publish(
            config,
            program,
            authority,
            keypair,
            prepared,
            PublishOptions {
                mode: PublishMode::Init,
                allow_resize: false,
                dry_run: false,
            },
        )
    });
    result.unwrap();
    let transactions: Vec<RuntimeTransaction> = requests
        .into_iter()
        .filter(|request| request["method"] == "send_transaction")
        .map(|request| serde_json::from_value(request["params"].clone()).unwrap())
        .collect();
    assert_eq!(transactions.len(), recorded.len());
    for (transaction, data) in transactions.iter().zip(&recorded) {
        let [instruction] = transaction.message.instructions.as_slice() else {
            panic!("each mainnet IDL transaction carries one instruction");
        };
        assert_eq!(&instruction.data, data);
        let accounts: Vec<Pubkey> = instruction
            .accounts
            .iter()
            .map(|index| transaction.message.account_keys[*index as usize])
            .collect();
        let expected = if data[8] == TAG_CREATE {
            vec![
                authority,
                key(PROPAMM_IDL),
                key(PROPAMM_BASE),
                Pubkey::system_program(),
                program,
            ]
        } else {
            vec![key(PROPAMM_IDL), authority]
        };
        assert_eq!(accounts, expected);
    }
}
