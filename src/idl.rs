use std::{
    io::{Read, Write},
    ops::Range,
    path::Path,
};

use arch_sdk::{
    AccountInfo, ArchError, Config, Status,
    arch_program::{
        account::AccountMeta, hash::Hash, instruction::Instruction, pubkey::Pubkey,
        rent::minimum_rent, sanitized::ArchMessage, system_instruction,
    },
    blocking::ArchRpcClient,
    build_and_sign_transaction, generate_new_keypair,
};
use bitcoin::key::Keypair;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde_json::Value;

use crate::{
    error::{CliError, Result},
    keys::pubkey_hex,
};

const IDL_IX_TAG_LE: [u8; 8] = [0x40, 0xf4, 0xbc, 0x78, 0xa7, 0xe9, 0x69, 0x0a];
const IDL_ACCOUNT_DISCRIMINATOR: [u8; 8] = [0x18, 0x46, 0x62, 0xbf, 0x3a, 0x90, 0x7b, 0x9e];
const IDL_SEED: &str = "anchor:idl";
const IDL_HEADER_LEN: usize = 8 + 32 + 4;
const DEFAULT_IDL_ACCOUNT_SIZE: usize = 10_000;
const IDL_RESIZE_INCREMENT: usize = 10_000;
const MAX_WRITE_SIZE: usize = 600;

const TAG_CREATE: u8 = 0;
const TAG_CREATE_BUFFER: u8 = 1;
const TAG_WRITE: u8 = 2;
const TAG_SET_BUFFER: u8 = 3;
const TAG_RESIZE: u8 = 6;

#[derive(Debug)]
pub(crate) struct PreparedIdl {
    expected: Value,
    compressed: Vec<u8>,
    initial_capacity: usize,
    requested_capacity: Option<usize>,
}

pub(crate) fn prepare(
    path: &Path,
    program: Pubkey,
    requested_capacity: Option<usize>,
) -> Result<PreparedIdl> {
    if !path.is_file() {
        return Err(CliError::InputNotFile {
            label: "IDL JSON",
            path: path.to_path_buf(),
        });
    }
    let raw = std::fs::read(path).map_err(|source| CliError::ReadInput {
        label: "IDL JSON",
        path: path.to_path_buf(),
        source,
    })?;
    prepare_bytes(&raw, program, requested_capacity)
}

fn prepare_bytes(
    raw: &[u8],
    program: Pubkey,
    requested_capacity: Option<usize>,
) -> Result<PreparedIdl> {
    let mut expected: Value = serde_json::from_slice(raw)
        .map_err(|error| CliError::InvalidIdl(format!("invalid JSON: {error}")))?;
    let root = expected
        .as_object_mut()
        .ok_or_else(|| CliError::InvalidIdl("JSON root must be an object".to_string()))?;
    root.insert("address".to_string(), Value::String(pubkey_hex(&program)));

    let metadata = root
        .entry("metadata")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::InvalidIdl("metadata must be an object".to_string()))?;
    metadata.insert("spec".to_string(), Value::String("0.1.0".to_string()));

    let normalized = serde_json::to_vec(&expected)?;
    let compressed = compress(&normalized)?;
    if compressed.len() > u32::MAX as usize {
        return Err(CliError::IdlPayloadTooLarge {
            bytes: compressed.len(),
        });
    }

    let required = required_space(compressed.len())?;
    let initial_capacity = match requested_capacity {
        Some(requested) if requested < required => {
            return Err(CliError::IdlSizeTooSmall {
                requested,
                required,
            });
        }
        Some(requested) => requested,
        None => DEFAULT_IDL_ACCOUNT_SIZE.max(required),
    };
    u64::try_from(initial_capacity)
        .map_err(|_| CliError::InvalidIdl("IDL account size exceeds u64".to_string()))?;

    Ok(PreparedIdl {
        expected,
        compressed,
        initial_capacity,
        requested_capacity,
    })
}

pub(crate) fn publish(
    config: &Config,
    program: Pubkey,
    authority: Pubkey,
    authority_keypair: Keypair,
    prepared: PreparedIdl,
) -> Result<()> {
    let client = ArchRpcClient::new(config);
    let (_, idl_address) = derive_idl_addresses(&program)?;

    println!("Publishing canonical program IDL");
    println!("  IDL account: {idl_address}");
    println!("  IDL account (hex): {}", pubkey_hex(&idl_address));
    println!(
        "  Normalized JSON: {} bytes; compressed: {} bytes",
        serde_json::to_vec(&prepared.expected)?.len(),
        prepared.compressed.len()
    );

    let outcome = match client.read_account_info(idl_address) {
        Ok(existing) => upgrade_or_skip(
            &client,
            config,
            program,
            idl_address,
            authority,
            authority_keypair,
            existing,
            &prepared,
        )?,
        Err(ArchError::NotFound(_)) => {
            initialize(
                &client,
                program,
                idl_address,
                authority,
                authority_keypair,
                &prepared,
            )?;
            PublishOutcome::Initialized {
                capacity: prepared.initial_capacity,
            }
        }
        Err(error) => return Err(error.into()),
    };

    verify_published(&client, program, authority, &prepared.expected)?;
    match outcome {
        PublishOutcome::Initialized { capacity } => {
            println!("IDL initialized and verified with {capacity} bytes of account capacity.");
        }
        PublishOutcome::Upgraded { capacity } => {
            println!("IDL upgraded and verified within {capacity} bytes of account capacity.");
        }
        PublishOutcome::Unchanged { capacity } => {
            println!("IDL is already current; retained {capacity} bytes of account capacity.");
        }
    }
    Ok(())
}

enum PublishOutcome {
    Initialized { capacity: usize },
    Upgraded { capacity: usize },
    Unchanged { capacity: usize },
}

fn initialize(
    client: &ArchRpcClient,
    program: Pubkey,
    idl_address: Pubkey,
    authority: Pubkey,
    authority_keypair: Keypair,
    prepared: &PreparedIdl,
) -> Result<()> {
    let (base, expected_address) = derive_idl_addresses(&program)?;
    if expected_address != idl_address {
        return Err(CliError::Idl(
            "canonical IDL address changed during initialization".to_string(),
        ));
    }

    let payload_capacity = prepared
        .initial_capacity
        .checked_sub(IDL_HEADER_LEN)
        .ok_or_else(|| CliError::InvalidIdl("IDL account size is below its header".to_string()))?;
    let create = Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(authority, true),
            AccountMeta::new(idl_address, false),
            AccountMeta::new_readonly(base, false),
            AccountMeta::new_readonly(Pubkey::system_program(), false),
            AccountMeta::new_readonly(program, false),
        ],
        data: create_ix_data(to_u64(payload_capacity, "IDL payload capacity")?),
    };
    send(
        client,
        "IDL create",
        vec![create],
        authority,
        vec![authority_keypair],
    )?;

    grow_empty_account(
        client,
        program,
        idl_address,
        authority,
        authority_keypair,
        prepared.initial_capacity.min(IDL_RESIZE_INCREMENT),
        prepared.initial_capacity,
    )?;
    write_chunks(
        client,
        program,
        idl_address,
        authority,
        authority_keypair,
        &prepared.compressed,
    )
}

#[allow(clippy::too_many_arguments)]
fn upgrade_or_skip(
    client: &ArchRpcClient,
    config: &Config,
    program: Pubkey,
    idl_address: Pubkey,
    authority: Pubkey,
    authority_keypair: Keypair,
    existing: AccountInfo,
    prepared: &PreparedIdl,
) -> Result<PublishOutcome> {
    let payload_range = validate_idl_account(&existing, program, Some(authority))?;
    let capacity = existing.data.len();

    if let Some(requested) = prepared.requested_capacity
        && requested > capacity
    {
        return Err(CliError::Idl(format!(
            "--idl-size requests {requested} bytes, but the populated canonical IDL account has fixed capacity {capacity}; publish initially with a larger size"
        )));
    }

    let existing_json = decompress_json(&existing.data[payload_range])?;
    if existing_json == prepared.expected {
        return Ok(PublishOutcome::Unchanged { capacity });
    }

    let required = required_space(prepared.compressed.len())?;
    if required > capacity {
        return Err(CliError::Idl(format!(
            "new compressed IDL requires {required} account bytes, but the populated canonical IDL account has fixed capacity {capacity}; redeploy the IDL account with a larger --idl-size"
        )));
    }

    let (buffer_keypair, buffer, _) = generate_new_keypair(config.network);
    println!("  Upgrade buffer: {buffer}");
    let create_account = system_instruction::create_account(
        &authority,
        &buffer,
        minimum_rent(required),
        to_u64(required, "IDL buffer size")?,
        &program,
    );
    let create_buffer = Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(buffer, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data: unit_ix_data(TAG_CREATE_BUFFER),
    };
    send(
        client,
        "IDL create upgrade buffer",
        vec![create_account, create_buffer],
        authority,
        vec![authority_keypair, buffer_keypair],
    )?;

    write_chunks(
        client,
        program,
        buffer,
        authority,
        authority_keypair,
        &prepared.compressed,
    )?;
    let set_buffer = Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(buffer, false),
            AccountMeta::new(idl_address, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data: unit_ix_data(TAG_SET_BUFFER),
    };
    send(
        client,
        "IDL set upgrade buffer",
        vec![set_buffer],
        authority,
        vec![authority_keypair],
    )?;

    Ok(PublishOutcome::Upgraded { capacity })
}

#[allow(clippy::too_many_arguments)]
fn grow_empty_account(
    client: &ArchRpcClient,
    program: Pubkey,
    idl_address: Pubkey,
    authority: Pubkey,
    authority_keypair: Keypair,
    initial_space: usize,
    target_space: usize,
) -> Result<()> {
    let mut current_space = initial_space;
    while current_space < target_space {
        let resize = Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new(idl_address, false),
                AccountMeta::new(authority, true),
                AccountMeta::new_readonly(Pubkey::system_program(), false),
            ],
            data: resize_ix_data(to_u64(target_space, "IDL account size")?),
        };
        let next_space = current_space
            .checked_add((target_space - current_space).min(IDL_RESIZE_INCREMENT))
            .ok_or_else(|| CliError::InvalidIdl("IDL resize overflow".to_string()))?;
        send(
            client,
            format!("IDL resize {next_space}/{target_space}"),
            vec![resize],
            authority,
            vec![authority_keypair],
        )?;
        current_space = next_space;
    }
    Ok(())
}

fn write_chunks(
    client: &ArchRpcClient,
    program: Pubkey,
    target: Pubkey,
    authority: Pubkey,
    authority_keypair: Keypair,
    compressed: &[u8],
) -> Result<()> {
    for (index, chunk) in compressed.chunks(MAX_WRITE_SIZE).enumerate() {
        let start = index * MAX_WRITE_SIZE;
        let end = start + chunk.len();
        let write = Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new(target, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: write_ix_data(chunk),
        };
        send(
            client,
            format!("IDL write {start}-{end}/{}", compressed.len()),
            vec![write],
            authority,
            vec![authority_keypair],
        )?;
    }
    Ok(())
}

fn send(
    client: &ArchRpcClient,
    action: impl Into<String>,
    instructions: Vec<Instruction>,
    payer: Pubkey,
    signers: Vec<Keypair>,
) -> Result<Hash> {
    let action = action.into();
    let message = ArchMessage::new(
        &instructions,
        Some(payer),
        client.get_best_finalized_block_hash()?,
    );
    let transaction = build_and_sign_transaction(message, signers, client.config.network)?;
    let transaction_id = client.send_transaction(transaction)?;
    let processed = client.wait_for_processed_transaction(&transaction_id)?;
    if processed.status != Status::Processed {
        return Err(CliError::TransactionFailed {
            action,
            status: format!("{:?}", processed.status),
        });
    }
    println!("  {action}: {transaction_id}");
    Ok(transaction_id)
}

fn verify_published(
    client: &ArchRpcClient,
    program: Pubkey,
    authority: Pubkey,
    expected: &Value,
) -> Result<()> {
    let (_, address) = derive_idl_addresses(&program)?;
    let account = client.read_account_info(address)?;
    let payload_range = validate_idl_account(&account, program, Some(authority))?;
    let actual = decompress_json(&account.data[payload_range])?;
    if &actual != expected {
        return Err(CliError::Idl(
            "on-chain IDL differs from the normalized local IDL after publication".to_string(),
        ));
    }
    Ok(())
}

fn validate_idl_account(
    account: &AccountInfo,
    program: Pubkey,
    expected_authority: Option<Pubkey>,
) -> Result<Range<usize>> {
    if account.owner != program {
        return Err(CliError::Idl(format!(
            "IDL owner mismatch: expected {program}, got {}",
            account.owner
        )));
    }
    if account.data.len() < IDL_HEADER_LEN {
        return Err(CliError::Idl(format!(
            "IDL account is only {} bytes, shorter than its {IDL_HEADER_LEN}-byte header",
            account.data.len()
        )));
    }
    if account.data[..8] != IDL_ACCOUNT_DISCRIMINATOR {
        return Err(CliError::Idl(
            "IDL account discriminator is invalid".to_string(),
        ));
    }

    let authority = Pubkey::from_slice(&account.data[8..40]);
    if let Some(expected) = expected_authority
        && authority != expected
    {
        return Err(CliError::Idl(format!(
            "IDL authority mismatch: expected {expected}, got {authority}"
        )));
    }

    let declared_len = u32::from_le_bytes(
        account.data[40..44]
            .try_into()
            .map_err(|_| CliError::Idl("IDL data length is invalid".to_string()))?,
    ) as usize;
    let end = IDL_HEADER_LEN
        .checked_add(declared_len)
        .filter(|end| *end <= account.data.len())
        .ok_or_else(|| {
            CliError::Idl(format!(
                "IDL payload length {declared_len} exceeds account capacity {}",
                account.data.len() - IDL_HEADER_LEN
            ))
        })?;
    Ok(IDL_HEADER_LEN..end)
}

fn decompress_json(compressed: &[u8]) -> Result<Value> {
    let mut decoder = ZlibDecoder::new(compressed);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .map_err(|error| CliError::Idl(format!("cannot inflate on-chain IDL: {error}")))?;
    serde_json::from_slice(&json)
        .map_err(|error| CliError::Idl(format!("on-chain IDL is invalid JSON: {error}")))
}

fn compress(json: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(json)
        .map_err(|error| CliError::InvalidIdl(format!("cannot compress JSON: {error}")))?;
    encoder
        .finish()
        .map_err(|error| CliError::InvalidIdl(format!("cannot finish compression: {error}")))
}

fn required_space(payload_len: usize) -> Result<usize> {
    IDL_HEADER_LEN
        .checked_add(payload_len)
        .ok_or_else(|| CliError::InvalidIdl("IDL account size overflow".to_string()))
}

fn derive_idl_addresses(program: &Pubkey) -> Result<(Pubkey, Pubkey)> {
    let (base, _) = Pubkey::find_program_address(&[], program);
    let idl = Pubkey::create_with_seed(&base, IDL_SEED, program)
        .map_err(|error| CliError::Idl(format!("cannot derive IDL account: {error:?}")))?;
    Ok((base, idl))
}

fn to_u64(value: usize, label: &str) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| CliError::InvalidIdl(format!("{label} exceeds the protocol's u64 limit")))
}

fn create_ix_data(data_len: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(17);
    data.extend_from_slice(&IDL_IX_TAG_LE);
    data.push(TAG_CREATE);
    data.extend_from_slice(&data_len.to_le_bytes());
    data
}

fn write_ix_data(chunk: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(13 + chunk.len());
    data.extend_from_slice(&IDL_IX_TAG_LE);
    data.push(TAG_WRITE);
    data.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    data.extend_from_slice(chunk);
    data
}

fn resize_ix_data(space: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(17);
    data.extend_from_slice(&IDL_IX_TAG_LE);
    data.push(TAG_RESIZE);
    data.extend_from_slice(&space.to_le_bytes());
    data
}

fn unit_ix_data(tag: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(9);
    data.extend_from_slice(&IDL_IX_TAG_LE);
    data.push(tag);
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_address_and_spec_and_uses_default_capacity() {
        let program = Pubkey::new_from_array([0xab; 32]);
        let prepared = prepare_bytes(
            br#"{"address":"placeholder","metadata":{"name":"demo","spec":"0.31.5"},"instructions":[]}"#,
            program,
            None,
        )
        .unwrap();

        assert_eq!(prepared.expected["address"], "ab".repeat(32));
        assert_eq!(prepared.expected["metadata"]["spec"], "0.1.0");
        assert_eq!(prepared.initial_capacity, DEFAULT_IDL_ACCOUNT_SIZE);
        assert!(decompress_json(&prepared.compressed).unwrap() == prepared.expected);
    }

    #[test]
    fn honors_custom_capacity_and_rejects_an_undersized_value() {
        let program = Pubkey::new_from_array([7; 32]);
        let prepared = prepare_bytes(br#"{"instructions":[]}"#, program, Some(20_000)).unwrap();
        assert_eq!(prepared.initial_capacity, 20_000);
        assert_eq!(prepared.requested_capacity, Some(20_000));

        let error = prepare_bytes(br#"{"instructions":[]}"#, program, Some(44)).unwrap_err();
        assert!(matches!(error, CliError::IdlSizeTooSmall { .. }));
    }

    #[test]
    fn instruction_encoding_matches_satellite_protocol() {
        assert_eq!(
            &create_ix_data(9)[..9],
            &[IDL_IX_TAG_LE.as_slice(), &[TAG_CREATE]].concat()
        );
        assert_eq!(
            &write_ix_data(&[1, 2])[..9],
            &[IDL_IX_TAG_LE.as_slice(), &[TAG_WRITE]].concat()
        );
        assert_eq!(unit_ix_data(TAG_CREATE_BUFFER)[8], TAG_CREATE_BUFFER);
        assert_eq!(unit_ix_data(TAG_SET_BUFFER)[8], TAG_SET_BUFFER);
        assert_eq!(resize_ix_data(44)[8], TAG_RESIZE);
    }

    #[test]
    fn validates_and_decodes_an_idl_account() {
        let program = Pubkey::new_from_array([1; 32]);
        let authority = Pubkey::new_from_array([2; 32]);
        let json = serde_json::json!({"address": "demo"});
        let compressed = compress(&serde_json::to_vec(&json).unwrap()).unwrap();
        let mut data = vec![0_u8; DEFAULT_IDL_ACCOUNT_SIZE];
        data[..8].copy_from_slice(&IDL_ACCOUNT_DISCRIMINATOR);
        data[8..40].copy_from_slice(authority.as_ref());
        data[40..44].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
        data[44..44 + compressed.len()].copy_from_slice(&compressed);
        let account = AccountInfo {
            lamports: 0,
            owner: program,
            data,
            utxo: String::new(),
            is_executable: false,
        };

        let range = validate_idl_account(&account, program, Some(authority)).unwrap();
        assert_eq!(decompress_json(&account.data[range]).unwrap(), json);
    }

    #[test]
    fn derives_a_stable_canonical_address() {
        let program = Pubkey::new_from_array([3; 32]);
        let first = derive_idl_addresses(&program).unwrap();
        let second = derive_idl_addresses(&program).unwrap();
        assert_eq!(first, second);
        assert_ne!(first.1, program);
    }
}
