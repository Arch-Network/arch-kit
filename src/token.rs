use std::collections::{BTreeSet, HashMap};

use apl_associated_token_account::get_associated_token_address_and_bump_seed;
use apl_token::state::{Account as TokenAccount, AccountState, Mint};
use arch_sdk::{
    AccountFilter, AccountInfo, ArchError, Config,
    arch_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey},
    blocking::ArchRpcClient,
};

use crate::error::{CliError, Result};

const RPC_ACCOUNT_BATCH_SIZE: usize = 100;

#[derive(Clone, Debug)]
pub(crate) struct MintView {
    pub(crate) address: Pubkey,
    pub(crate) state: Mint,
}

#[derive(Clone, Debug)]
pub(crate) struct TokenAccountView {
    pub(crate) address: Pubkey,
    pub(crate) state: TokenAccount,
    pub(crate) mint: MintView,
}

pub(crate) fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey> {
    let decoded = bs58::decode(value).into_vec();
    if let Ok(bytes) = decoded
        && let Ok(bytes) = <[u8; 32]>::try_from(bytes)
    {
        return Ok(Pubkey::from(bytes));
    }

    let decoded = hex::decode(value);
    if let Ok(bytes) = decoded
        && let Ok(bytes) = <[u8; 32]>::try_from(bytes)
    {
        return Ok(Pubkey::from(bytes));
    }

    Err(CliError::InvalidArgument(format!(
        "{label} must be a Base58 or 64-character hexadecimal Arch public key"
    )))
}

pub(crate) fn associated_token_address(owner: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    get_associated_token_address_and_bump_seed(owner, mint, &apl_associated_token_account::id())
}

pub(crate) fn read_mint(client: &ArchRpcClient, address: Pubkey) -> Result<MintView> {
    let info = client.read_account_info(address)?;
    decode_mint(address, &info)
}

pub(crate) fn read_token_account(
    client: &ArchRpcClient,
    address: Pubkey,
) -> Result<TokenAccountView> {
    let state = read_token_account_state(client, address)?;
    let mint = read_mint(client, state.mint)?;
    Ok(TokenAccountView {
        address,
        state,
        mint,
    })
}

pub(crate) fn read_token_account_state(
    client: &ArchRpcClient,
    address: Pubkey,
) -> Result<TokenAccount> {
    let info = client.read_account_info(address)?;
    decode_token_account(address, &info)
}

pub(crate) fn read_associated_balance(
    client: &ArchRpcClient,
    owner: Pubkey,
    mint_address: Pubkey,
) -> Result<(Pubkey, MintView, Option<TokenAccount>)> {
    let mint = read_mint(client, mint_address)?;
    let (address, _) = associated_token_address(&owner, &mint_address);
    let account = match client.read_account_info(address) {
        Ok(info) => {
            let account = decode_token_account(address, &info)?;
            if account.mint != mint_address || account.owner != owner {
                return Err(CliError::TokenAccountIdentityMismatch {
                    address: address.to_string(),
                    expected_mint: mint_address.to_string(),
                    actual_mint: account.mint.to_string(),
                    expected_owner: owner.to_string(),
                    actual_owner: account.owner.to_string(),
                });
            }
            Some(account)
        }
        Err(ArchError::NotFound(_)) => None,
        Err(source) => return Err(source.into()),
    };
    Ok((address, mint, account))
}

pub(crate) fn list_token_accounts(config: &Config, owner: Pubkey) -> Result<Vec<TokenAccountView>> {
    let client = ArchRpcClient::new(config);
    let mut accounts = client.get_program_accounts(
        &apl_token::id(),
        Some(vec![
            AccountFilter::DataSize(TokenAccount::LEN),
            AccountFilter::DataContent {
                offset: 32,
                bytes: owner.serialize().to_vec(),
            },
        ]),
    )?;
    accounts.sort_by_key(|account| account.pubkey);

    let decoded = accounts
        .into_iter()
        .map(|account| {
            let address = account.pubkey;
            decode_token_account(address, &account.account).map(|state| (address, state))
        })
        .collect::<Result<Vec<_>>>()?;

    let mint_addresses = decoded
        .iter()
        .map(|(_, account)| account.mint)
        .collect::<BTreeSet<_>>();
    let mut mints = HashMap::with_capacity(mint_addresses.len());
    let mint_addresses = mint_addresses.into_iter().collect::<Vec<_>>();
    for batch in mint_addresses.chunks(RPC_ACCOUNT_BATCH_SIZE) {
        let infos = client.get_multiple_accounts(batch.to_vec())?;
        for (address, info) in batch.iter().copied().zip(infos) {
            let info = info.ok_or_else(|| CliError::InvalidTokenMint {
                address: address.to_string(),
                detail: "account does not exist".to_string(),
            })?;
            mints.insert(address, decode_mint(address, &info.into())?);
        }
    }

    decoded
        .into_iter()
        .map(|(address, state)| {
            let mint =
                mints
                    .get(&state.mint)
                    .cloned()
                    .ok_or_else(|| CliError::InvalidTokenMint {
                        address: state.mint.to_string(),
                        detail: "mint was not returned by the RPC".to_string(),
                    })?;
            Ok(TokenAccountView {
                address,
                state,
                mint,
            })
        })
        .collect()
}

fn decode_token_account(address: Pubkey, info: &AccountInfo) -> Result<TokenAccount> {
    require_token_program_owner(address, info.owner)?;
    if info.data.len() != TokenAccount::LEN {
        return Err(CliError::InvalidTokenAccount {
            address: address.to_string(),
            detail: format!(
                "expected {} bytes, received {}",
                TokenAccount::LEN,
                info.data.len()
            ),
        });
    }
    TokenAccount::unpack(&info.data).map_err(|source| CliError::InvalidTokenAccount {
        address: address.to_string(),
        detail: source.to_string(),
    })
}

fn decode_mint(address: Pubkey, info: &AccountInfo) -> Result<MintView> {
    require_token_program_owner(address, info.owner)?;
    if info.data.len() != Mint::LEN {
        return Err(CliError::InvalidTokenMint {
            address: address.to_string(),
            detail: format!("expected {} bytes, received {}", Mint::LEN, info.data.len()),
        });
    }
    let state = Mint::unpack(&info.data).map_err(|source| CliError::InvalidTokenMint {
        address: address.to_string(),
        detail: source.to_string(),
    })?;
    Ok(MintView { address, state })
}

fn require_token_program_owner(address: Pubkey, owner: Pubkey) -> Result<()> {
    let expected = apl_token::id();
    if owner == expected {
        return Ok(());
    }
    Err(CliError::TokenProgramOwnerMismatch {
        address: address.to_string(),
        expected: expected.to_string(),
        actual: owner.to_string(),
    })
}

pub(crate) fn account_state_name(state: AccountState) -> &'static str {
    match state {
        AccountState::Uninitialized => "uninitialized",
        AccountState::Initialized => "initialized",
        AccountState::Frozen => "frozen",
    }
}

pub(crate) fn optional_pubkey(value: COption<Pubkey>) -> Option<String> {
    match value {
        COption::Some(pubkey) => Some(pubkey.to_string()),
        COption::None => None,
    }
}

pub(crate) fn optional_u64(value: COption<u64>) -> Option<u64> {
    match value {
        COption::Some(value) => Some(value),
        COption::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_info(owner: Pubkey, data: Vec<u8>) -> AccountInfo {
        AccountInfo {
            lamports: 0,
            owner,
            data,
            utxo: String::new(),
            is_executable: false,
        }
    }

    #[test]
    fn parses_base58_and_hex_pubkeys() {
        let key = Pubkey::from([7; 32]);
        assert_eq!(parse_pubkey(&key.to_string(), "key").unwrap(), key);
        assert_eq!(
            parse_pubkey(&hex::encode(key.serialize()), "key").unwrap(),
            key
        );
        assert!(parse_pubkey("not-a-key", "key").is_err());
    }

    #[test]
    fn derives_the_official_associated_token_address() {
        let owner = Pubkey::from([1; 32]);
        let mint = Pubkey::from([2; 32]);
        let derived = associated_token_address(&owner, &mint);
        let expected = get_associated_token_address_and_bump_seed(
            &owner,
            &mint,
            &apl_associated_token_account::id(),
        );
        assert_eq!(derived, expected);
    }

    #[test]
    fn decodes_valid_token_accounts_and_mints() {
        let address = Pubkey::from([3; 32]);
        let mint_address = Pubkey::from([4; 32]);
        let owner = Pubkey::from([5; 32]);
        let state = TokenAccount {
            mint: mint_address,
            owner,
            amount: 123,
            state: AccountState::Initialized,
            ..TokenAccount::default()
        };
        let mut data = vec![0; TokenAccount::LEN];
        TokenAccount::pack(state, &mut data).unwrap();
        let decoded = decode_token_account(address, &account_info(apl_token::id(), data)).unwrap();
        assert_eq!(decoded, state);

        let state = Mint {
            supply: 456,
            decimals: 6,
            is_initialized: true,
            ..Mint::default()
        };
        let mut data = vec![0; Mint::LEN];
        Mint::pack(state, &mut data).unwrap();
        let decoded = decode_mint(mint_address, &account_info(apl_token::id(), data)).unwrap();
        assert_eq!(decoded.state, state);
    }

    #[test]
    fn rejects_wrong_owners_and_invalid_lengths() {
        let address = Pubkey::from([6; 32]);
        let wrong_owner = account_info(Pubkey::system_program(), vec![0; TokenAccount::LEN]);
        assert!(matches!(
            decode_token_account(address, &wrong_owner),
            Err(CliError::TokenProgramOwnerMismatch { .. })
        ));

        let wrong_length = account_info(apl_token::id(), vec![0; TokenAccount::LEN - 1]);
        assert!(matches!(
            decode_token_account(address, &wrong_length),
            Err(CliError::InvalidTokenAccount { .. })
        ));
    }
}
