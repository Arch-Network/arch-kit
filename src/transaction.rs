use arch_sdk::{
    RuntimeTransaction, Status,
    arch_program::{hash::Hash, instruction::Instruction, pubkey::Pubkey, sanitized::ArchMessage},
    blocking::ArchRpcClient,
};

use crate::{
    arch_signer::ArchSigner,
    error::{CliError, Result},
};

pub(crate) fn send_and_confirm(
    client: &ArchRpcClient,
    action: impl Into<String>,
    instructions: Vec<Instruction>,
    payer: Pubkey,
    signers: &[&dyn ArchSigner],
) -> Result<Hash> {
    let action = action.into();
    let message = ArchMessage::new(
        &instructions,
        Some(payer),
        client.get_best_finalized_block_hash()?,
    );
    let transaction = sign_transaction(message, signers)?;
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

fn sign_transaction(
    message: ArchMessage,
    signers: &[&dyn ArchSigner],
) -> Result<RuntimeTransaction> {
    let required = usize::from(message.header.num_required_signatures);
    let mut signatures = Vec::with_capacity(required);
    for pubkey in message.account_keys.iter().take(required) {
        let signer = signers
            .iter()
            .find(|signer| signer.pubkey() == *pubkey)
            .ok_or_else(|| CliError::Signer(format!("no signer configured for {pubkey}")))?;
        signatures.push(signer.sign_message(&message)?);
    }
    Ok(RuntimeTransaction {
        version: 0,
        signatures,
        message,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use arch_sdk::{
        Signature,
        arch_program::{hash::Hash, system_instruction},
    };

    use super::*;

    struct RecordingSigner {
        pubkey: Pubkey,
        signature: Signature,
        calls: Mutex<usize>,
    }

    impl ArchSigner for RecordingSigner {
        fn pubkey(&self) -> Pubkey {
            self.pubkey
        }

        fn sign_message(&self, _: &ArchMessage) -> Result<Signature> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.signature.clone())
        }
    }

    #[test]
    fn signs_in_required_account_order() {
        let payer = Pubkey::from([1; 32]);
        let account = Pubkey::from([2; 32]);
        let message = ArchMessage::new(
            &[system_instruction::create_account(
                &payer,
                &account,
                1,
                0,
                &Pubkey::system_program(),
            )],
            Some(payer),
            Hash::from([3; 32]),
        );
        let payer_signer = RecordingSigner {
            pubkey: payer,
            signature: Signature([1; 64]),
            calls: Mutex::new(0),
        };
        let account_signer = RecordingSigner {
            pubkey: account,
            signature: Signature([2; 64]),
            calls: Mutex::new(0),
        };

        let transaction = sign_transaction(message, &[&account_signer, &payer_signer]).unwrap();

        assert_eq!(transaction.signatures[0].0, [1; 64]);
        assert_eq!(transaction.signatures[1].0, [2; 64]);
        assert_eq!(*payer_signer.calls.lock().unwrap(), 1);
        assert_eq!(*account_signer.calls.lock().unwrap(), 1);
    }

    #[test]
    fn rejects_a_missing_required_signer() {
        let payer = Pubkey::from([1; 32]);
        let destination = Pubkey::from([2; 32]);
        let message = ArchMessage::new(
            &[system_instruction::transfer(&payer, &destination, 1)],
            Some(payer),
            Hash::from([3; 32]),
        );

        assert!(sign_transaction(message, &[]).is_err());
    }
}
