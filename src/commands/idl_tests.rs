//! `idl` commands against a scripted local RPC serving the mainnet PropAMM IDL.
use std::io::Read;

use arch_sdk::AccountInfo;
use serde_json::json;

use super::*;

const PROPAMM: &str = "4022dec7e57f757682712b45ddf13a8f324710f4637cefc0ebacc6343caab24c";
const PROPAMM_ACCOUNT: &[u8] = include_bytes!("../idl_vectors/propamm_idl_account.bin");

fn propamm_idl_account() -> AccountInfo {
    let mut data = PROPAMM_ACCOUNT.to_vec();
    data.resize(10_000, 0);
    AccountInfo {
        lamports: 0,
        owner: parse_pubkey(PROPAMM, "test").unwrap(),
        data,
        is_executable: false,
    }
}

fn propamm_json() -> Value {
    let mut json = Vec::new();
    flate2::read::ZlibDecoder::new(&PROPAMM_ACCOUNT[44..])
        .read_to_end(&mut json)
        .unwrap();
    serde_json::from_slice(&json).unwrap()
}

fn idl_file(json: &Value) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), serde_json::to_vec(json).unwrap()).unwrap();
    file
}

fn run_check(json: &Value, reply: (&'static str, Value)) -> Result<()> {
    let file = idl_file(json);
    let (result, requests) = crate::test_rpc::run(vec![reply], |config| {
        check(
            config,
            CheckArgs {
                idl: file.path().to_path_buf(),
                program_id: PROPAMM.to_string(),
            },
        )
    });
    assert_eq!(requests.len(), 1);
    result
}

#[test]
fn check_accepts_the_published_idl_and_normalizes_address_and_spec() {
    let reply = || {
        (
            "read_account_info",
            json!({"result": propamm_idl_account()}),
        )
    };
    run_check(&propamm_json(), reply()).unwrap();

    // Publication rewrites these two fields, so the file need not carry them.
    let mut unnormalized = propamm_json();
    unnormalized["address"] = json!("placeholder");
    unnormalized["metadata"]["spec"] = json!("0.31.5");
    run_check(&unnormalized, reply()).unwrap();
}

#[test]
fn check_fails_on_any_difference_or_a_missing_idl() {
    let reply = || {
        (
            "read_account_info",
            json!({"result": propamm_idl_account()}),
        )
    };
    let mut changed = propamm_json();
    changed["errors"][0]["msg"] = json!("changed");
    changed["events"] = json!([{"name": "Traded", "discriminator": [0, 1, 2, 3, 4, 5, 6, 7]}]);
    let error = run_check(&changed, reply()).unwrap_err().to_string();
    assert!(error.ends_with("in: errors, events"), "{error}");

    let missing = (
        "read_account_info",
        json!({"error": {"code": 404, "message": "Account not found"}}),
    );
    let error = run_check(&propamm_json(), missing).unwrap_err().to_string();
    assert!(error.contains("has no canonical IDL account"), "{error}");
}

#[test]
fn fetch_decodes_the_on_chain_idl() {
    let (result, requests) = crate::test_rpc::run(
        vec![(
            "read_account_info",
            json!({"result": propamm_idl_account()}),
        )],
        |config| {
            fetch(
                config,
                FetchArgs {
                    program_id: PROPAMM.to_string(),
                },
            )
        },
    );
    result.unwrap();
    let address: Pubkey = serde_json::from_value(requests[0]["params"].clone()).unwrap();
    assert_eq!(
        pubkey_hex(&address),
        "c90520a685fcf5e553f266d9505cf9f6c307b897d4022d63e6444ceb93ada91f"
    );
}

#[test]
fn publish_rejects_a_program_id_that_is_not_a_deployed_program() {
    let authority = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(authority.path(), "02".repeat(32)).unwrap();
    let idl = idl_file(&propamm_json());
    let mut not_executable = propamm_idl_account();
    not_executable.data.clear();
    for (reply, error) in [
        (
            json!({"error": {"code": 404, "message": "Account not found"}}),
            "does not exist",
        ),
        (json!({"result": not_executable}), "not an executable"),
    ] {
        let (result, requests) =
            crate::test_rpc::run(vec![("read_account_info", reply)], |config| {
                publish(
                    config,
                    PublishArgs {
                        program_id: PROPAMM.to_string(),
                        idl: idl.path().to_path_buf(),
                        authority: authority.path().to_path_buf(),
                        mode: PublishMode::Auto,
                        dry_run: true,
                        idl_size: None,
                        allow_idl_resize: false,
                    },
                )
            });
        assert!(result.unwrap_err().to_string().contains(error));
        let program: Pubkey = serde_json::from_value(requests[0]["params"].clone()).unwrap();
        assert_eq!(pubkey_hex(&program), PROPAMM);
    }
}
