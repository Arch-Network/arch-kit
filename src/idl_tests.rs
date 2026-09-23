//! IDL publication against a scripted local RPC server; no chain or real keys.
use arch_sdk::RuntimeTransaction;
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use serde_json::json;

use super::*;

fn program() -> Pubkey {
    Pubkey::from([3; 32])
}

fn authority_keypair() -> Keypair {
    Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[2; 32]).unwrap())
}

fn authority() -> Pubkey {
    Pubkey::from_slice(&authority_keypair().x_only_public_key().0.serialize())
}

fn prepared(requested: Option<usize>) -> PreparedIdl {
    prepare_bytes(br#"{"instructions":[]}"#, program(), requested).unwrap()
}

fn updated() -> PreparedIdl {
    let raw = serde_json::to_vec(&json!({
        "instructions": [],
        "docs": (0..500).collect::<Vec<_>>(),
    }))
    .unwrap();
    prepare_bytes(&raw, program(), None).unwrap()
}

fn account(prepared: &PreparedIdl, capacity: usize) -> AccountInfo {
    let mut data = vec![0; capacity];
    data[..8].copy_from_slice(&IDL_ACCOUNT_DISCRIMINATOR);
    data[8..40].copy_from_slice(authority().as_ref());
    data[40..44].copy_from_slice(&(prepared.compressed.len() as u32).to_le_bytes());
    data[44..44 + prepared.compressed.len()].copy_from_slice(&prepared.compressed);
    AccountInfo {
        lamports: minimum_rent(capacity),
        owner: program(),
        data,
        utxo: format!("{}:0", "00".repeat(32)),
        is_executable: false,
    }
}

fn read_reply(account: &AccountInfo) -> (&'static str, Value) {
    ("read_account_info", json!({"result": account}))
}

fn transaction_replies(status: Value, rollback: Value) -> Vec<(&'static str, Value)> {
    vec![
        (
            "get_best_finalized_block_hash",
            json!({"result": "00".repeat(32)}),
        ),
        ("send_transaction", json!({"result": "11".repeat(32)})),
        (
            "get_processed_transaction",
            json!({"result": {
                "status": status, "rollback_status": rollback,
                "bitcoin_txid": null, "logs": [], "inner_instructions_list": [],
            }}),
        ),
    ]
}

fn successful_transaction() -> Vec<(&'static str, Value)> {
    transaction_replies(
        json!({"type": "processed"}),
        json!({"type": "notRolledback"}),
    )
}

fn resize_setup(existing: &AccountInfo) -> Vec<(&'static str, Value)> {
    let mut backup = existing.clone();
    let range = validate_idl_account(existing, program(), Some(authority())).unwrap();
    backup.data.truncate(range.end);
    let mut replies = successful_transaction();
    replies.push(read_reply(&backup));
    replies.extend(successful_transaction());
    replies
}

fn created_buffer(transaction: &RuntimeTransaction) -> Pubkey {
    let index = transaction.message.instructions[1].accounts[0];
    transaction.message.account_keys[index as usize]
}

fn run_publish(
    replies: Vec<(&'static str, Value)>,
    prepared: PreparedIdl,
    allow_resize: bool,
) -> (Result<()>, Vec<RuntimeTransaction>) {
    let (result, requests) = crate::test_rpc::run(replies, |config| {
        publish(
            config,
            program(),
            authority(),
            authority_keypair(),
            prepared,
            allow_resize,
        )
    });
    let transactions = requests
        .into_iter()
        .filter(|request| request["method"] == "send_transaction")
        .map(|request| serde_json::from_value(request["params"].clone()).unwrap())
        .collect();
    (result, transactions)
}

#[test]
fn growth_requires_opt_in_for_both_requested_size_and_larger_payload() {
    let old = prepared(None);
    let existing = account(&old, required_space(old.compressed.len()).unwrap());
    for new in [prepared(Some(20_000)), updated()] {
        let (result, transactions) = run_publish(vec![read_reply(&existing)], new, false);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--allow-idl-resize")
        );
        assert!(transactions.is_empty());
    }
}

#[test]
fn initial_idl_can_still_grow_while_empty_with_either_flag_value() {
    for allow_resize in [false, true] {
        let new = prepared(Some(20_000));
        let mut replies = vec![(
            "read_account_info",
            json!({"error": {"code": 404, "message": "Account not found"}}),
        )];
        replies.extend(successful_transaction());
        replies.extend(successful_transaction());
        for _ in new.compressed.chunks(MAX_WRITE_SIZE) {
            replies.extend(successful_transaction());
        }
        replies.push(read_reply(&account(&new, 20_000)));
        let (result, transactions) = run_publish(replies, new, allow_resize);
        result.unwrap();
        assert_eq!(transactions[0].message.instructions[0].data[8], TAG_CREATE);
        assert_eq!(
            transactions[1].message.instructions[0].data,
            resize_ix_data(20_000)
        );
    }
}

#[test]
fn resize_opt_in_does_not_bypass_account_validation() {
    let existing = account(&prepared(None), 1_000);
    let mut wrong_owner = existing.clone();
    wrong_owner.owner = Pubkey::system_program();
    let mut wrong_authority = existing.clone();
    wrong_authority.data[8..40].fill(0);
    let mut wrong_discriminator = existing;
    wrong_discriminator.data[..8].fill(0);
    for invalid in [wrong_owner, wrong_authority, wrong_discriminator] {
        let (result, transactions) =
            run_publish(vec![read_reply(&invalid)], prepared(Some(20_000)), true);
        assert!(matches!(result, Err(CliError::Idl(_))));
        assert!(transactions.is_empty());
    }
}

#[test]
fn unchanged_idl_does_not_grow_to_the_initial_default_or_shrink() {
    let existing = account(&prepared(None), 1_000);
    for allow_resize in [false, true] {
        for requested in [None, Some(500)] {
            let (result, transactions) = run_publish(
                vec![read_reply(&existing), read_reply(&existing)],
                prepared(requested),
                allow_resize,
            );
            result.unwrap();
            assert!(transactions.is_empty());
        }
    }
}

#[test]
fn explicit_growth_resizes_an_unchanged_idl_in_multiple_steps() {
    let new = prepared(Some(21_001));
    let existing = account(&new, 1_000);
    let resized = account(&new, 21_001);
    let mut replies = vec![read_reply(&existing)];
    replies.extend(resize_setup(&existing));
    for _ in 0..3 {
        replies.extend(successful_transaction());
    }
    replies.extend([read_reply(&resized), read_reply(&resized)]);
    let (result, transactions) = run_publish(replies, new, true);
    result.unwrap();
    assert_eq!(transactions.len(), 5);

    let (_, idl_address) = derive_idl_addresses(&program()).unwrap();
    let backup = created_buffer(&transactions[0]);
    let empty = created_buffer(&transactions[1]);
    let mut accounts = std::collections::BTreeMap::from([
        (idl_address, existing.data.clone()),
        (backup, existing.data.clone()),
        (empty, vec![0; IDL_HEADER_LEN]),
    ]);
    let snapshot = &transactions[0].message.instructions[2];
    assert_eq!(snapshot.data, unit_ix_data(TAG_SET_BUFFER));
    assert_eq!(
        transactions[0].message.account_keys[snapshot.accounts[0] as usize],
        idl_address
    );
    assert_eq!(
        transactions[0].message.account_keys[snapshot.accounts[1] as usize],
        backup
    );
    for (index, transaction) in transactions[2..].iter().enumerate() {
        let mut instructions = vec![
            set_buffer_instruction(program(), empty, idl_address, authority()),
            Instruction {
                program_id: program(),
                accounts: vec![
                    AccountMeta::new(idl_address, false),
                    AccountMeta::new(authority(), true),
                    AccountMeta::new_readonly(Pubkey::system_program(), false),
                ],
                data: resize_ix_data(21_001),
            },
            set_buffer_instruction(program(), backup, idl_address, authority()),
        ];
        if index == 2 {
            instructions.extend([
                close_buffer_instruction(program(), empty, authority()),
                close_buffer_instruction(program(), backup, authority()),
            ]);
        }
        let expected = ArchMessage::new(&instructions, Some(authority()), Hash::from([0; 32]));
        assert_eq!(transaction.message, expected);
        transaction.verify_sigs(bitcoin::Network::Bitcoin).unwrap();

        // Replay the submitted instructions using legacy Satellite's data_len
        // rules: Resize must see an empty IDL, and SetBuffer copies its payload.
        for instruction in &transaction.message.instructions {
            let first = transaction.message.account_keys[instruction.accounts[0] as usize];
            match instruction.data[8] {
                TAG_SET_BUFFER => {
                    let source = &accounts[&first];
                    let len = u32::from_le_bytes(source[40..44].try_into().unwrap()) as usize;
                    let payload = source[44..44 + len].to_vec();
                    let target = transaction.message.account_keys[instruction.accounts[1] as usize];
                    let data = accounts.get_mut(&target).unwrap();
                    data[40..44].copy_from_slice(&(len as u32).to_le_bytes());
                    data[44..44 + len].copy_from_slice(&payload);
                }
                TAG_RESIZE => {
                    let data = accounts.get_mut(&first).unwrap();
                    assert_eq!(
                        &data[40..44],
                        &[0; 4],
                        "legacy handler rejects populated IDLs"
                    );
                    let target =
                        u64::from_le_bytes(instruction.data[9..17].try_into().unwrap()) as usize;
                    data.resize(data.len() + (target - data.len()).min(10_000), 0);
                }
                TAG_CLOSE => {
                    assert_eq!(
                        transaction.message.account_keys[instruction.accounts[2] as usize],
                        authority()
                    );
                    accounts.remove(&first).unwrap();
                }
                tag => panic!("unexpected IDL instruction {tag}"),
            }
        }
        // Every transaction boundary retains the original published bytes.
        assert_eq!(
            &accounts[&idl_address][..existing.data.len()],
            &existing.data
        );
        assert_eq!(
            accounts[&idl_address].len(),
            (1_000 + (index + 1) * 10_000).min(21_001)
        );
    }
    assert_eq!(accounts.len(), 1, "temporary resize buffers must be closed");
}

#[test]
fn larger_payload_grows_before_uploading_and_swapping_the_upgrade_buffer() {
    let old = prepared(None);
    let new = updated();
    let required = required_space(new.compressed.len()).unwrap();
    let existing = account(&old, required_space(old.compressed.len()).unwrap());
    assert!(required > existing.data.len());
    let mut replies = vec![read_reply(&existing)];
    replies.extend(resize_setup(&existing));
    replies.extend(successful_transaction());
    replies.push(read_reply(&account(&old, required)));
    replies.extend(successful_transaction());
    let chunks = new.compressed.chunks(MAX_WRITE_SIZE).count();
    for _ in 0..chunks {
        replies.extend(successful_transaction());
    }
    replies.extend(successful_transaction());
    replies.push(read_reply(&account(&new, required)));

    let (result, transactions) = run_publish(replies, new, true);
    result.unwrap();
    assert_eq!(transactions.len(), chunks + 5);
    assert_eq!(
        transactions[2].message.instructions[1].data,
        resize_ix_data(required as u64)
    );
    assert_eq!(
        transactions[3].message.instructions[1].data,
        unit_ix_data(TAG_CREATE_BUFFER)
    );
    for transaction in &transactions[4..4 + chunks] {
        assert_eq!(transaction.message.instructions[0].data[8], TAG_WRITE);
    }
    assert_eq!(
        transactions.last().unwrap().message.instructions[0].data,
        unit_ix_data(TAG_SET_BUFFER)
    );
}

#[test]
fn upgrades_that_fit_keep_existing_capacity_with_either_flag_value() {
    for allow_resize in [false, true] {
        let new = updated();
        let mut replies = vec![read_reply(&account(&prepared(None), 10_000))];
        replies.extend(successful_transaction());
        for _ in new.compressed.chunks(MAX_WRITE_SIZE) {
            replies.extend(successful_transaction());
        }
        replies.extend(successful_transaction());
        replies.push(read_reply(&account(&new, 10_000)));
        let (result, transactions) = run_publish(replies, new, allow_resize);
        result.unwrap();
        assert_eq!(
            transactions[0].message.instructions[1].data,
            unit_ix_data(TAG_CREATE_BUFFER)
        );
    }
}

#[test]
fn rejected_or_rolled_back_resize_stops_before_uploading_an_upgrade() {
    for (status, rollback) in [
        (
            json!({"type": "failed", "message": "IdlAccountNotEmpty"}),
            json!({"type": "notRolledback"}),
        ),
        (
            json!({"type": "processed"}),
            json!({"type": "rolledback", "message": "Reverted"}),
        ),
    ] {
        let old = prepared(None);
        let existing = account(&old, required_space(old.compressed.len()).unwrap());
        let mut replies = vec![read_reply(&existing)];
        replies.extend(resize_setup(&existing));
        replies.extend(transaction_replies(status, rollback));
        let (result, transactions) = run_publish(replies, updated(), true);
        assert!(matches!(result, Err(CliError::TransactionFailed { .. })));
        assert_eq!(transactions.len(), 3);
    }
}

#[test]
fn resize_must_grow_the_account_and_preserve_the_existing_idl() {
    let old = prepared(None);
    let existing = account(&old, required_space(old.compressed.len()).unwrap());
    let target = required_space(updated().compressed.len()).unwrap();
    for (resized, error) in [
        (existing.clone(), "after resize"),
        (
            account(&updated(), target),
            "contents changed during resize",
        ),
    ] {
        let mut replies = vec![read_reply(&existing)];
        replies.extend(resize_setup(&existing));
        replies.extend(successful_transaction());
        replies.push(read_reply(&resized));
        let (result, transactions) = run_publish(replies, updated(), true);
        assert!(result.unwrap_err().to_string().contains(error));
        assert_eq!(transactions.len(), 3);
    }
}

#[test]
fn invalid_backup_stops_before_clearing_the_canonical_idl() {
    let existing = account(&prepared(None), 1_000);
    let mut replies = vec![read_reply(&existing)];
    replies.extend(successful_transaction());
    replies.push(read_reply(&account(&updated(), 2_000)));
    let (result, transactions) = run_publish(replies, prepared(Some(20_000)), true);
    assert!(result.unwrap_err().to_string().contains("backup differs"));
    assert_eq!(transactions.len(), 1);
    // The only SetBuffer so far copies FROM the canonical IDL into the backup.
    let copy = &transactions[0].message.instructions[2];
    let (_, canonical) = derive_idl_addresses(&program()).unwrap();
    assert_eq!(
        transactions[0].message.account_keys[copy.accounts[0] as usize],
        canonical
    );
    assert_ne!(
        transactions[0].message.account_keys[copy.accounts[1] as usize],
        canonical
    );
}
