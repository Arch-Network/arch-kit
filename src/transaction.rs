use arch_sdk::{
    Status,
    arch_program::{hash::Hash, instruction::Instruction, pubkey::Pubkey, sanitized::ArchMessage},
    blocking::ArchRpcClient,
    build_and_sign_transaction,
};
use bitcoin::key::Keypair;

use crate::error::{CliError, Result};

pub(crate) fn send_and_confirm(
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
    Ok(transaction_id)
}
