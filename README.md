# arch-kit

`arch-kit` is a CLI for generating Arch keys, deploying programs, publishing
canonical on-chain IDLs, and inspecting APL tokens.

## Install

Install from this repository with Cargo:

```bash
cargo install --path .
```

Cargo installs the binary to `$CARGO_HOME/bin` (usually `~/.cargo/bin`). If it
is not already available on your `PATH`, add this to your shell profile:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

After pulling changes, update the installed binary with:

```bash
cargo install --path . --force
```

## Program development

Programs using Satellite require Rust nightly during the IDL-building step.
Install it with `rustup toolchain install nightly`; Satellite's
[IDL builder explicitly selects the nightly toolchain](https://docs.rs/arch-satellite-lang-idl/0.31.5/src/arch_satellite_lang_idl/build.rs.html#141-146).

## Commands

| Command | Usage | Description |
| --- | --- | --- |
| [`init`](#initialize-a-program) | `arch-kit init <PATH> --program-key <PATH>` | Initialize a new Satellite Hello World program. |
| [`keygen`](#generate-keys) | `arch-kit keygen [OPTIONS] <PATH>...` | Generate one or more secp256k1 key files, with optional public key prefixes (vanity). |
| [`pubkey`](#derive-a-public-key) | `arch-kit pubkey <PATH>` | Derive a Base58 Arch public key from a secret key file. |
| [`deploy`](#deploy-a-program) | `arch-kit deploy [OPTIONS]` | Deploy or update a program and its IDL. |
| [`health`](#check-node-health) | `arch-kit health` | Check validator readiness and block progression. |
| [`ata`](#inspect-tokens) | `arch-kit ata <OWNER> <MINT>` | Derive an associated token account address. |
| [`token-balance`](#inspect-tokens) | `arch-kit token-balance <OWNER> <MINT>` | Read an owner's ATA balance for a mint. |
| [`token-account`](#inspect-tokens) | `arch-kit token-account <ADDRESS>` | Inspect one APL token account. |
| [`token-accounts`](#inspect-tokens) | `arch-kit token-accounts <OWNER>` | List every APL token account owned by an address. |
| [`mint-info`](#inspect-tokens) | `arch-kit mint-info <MINT>` | Inspect an APL token mint. |
| [`create-mint`](#create-a-mint) | `arch-kit create-mint --mint-key <PATH> --key <PATH>` | Create an APL token mint with optional initial supply. |
| [`mint-tokens`](#mint-tokens) | `arch-kit mint-tokens <RECIPIENT> <MINT> <AMOUNT> --key <PATH>` | Mint tokens to a user's ATA. |
| [`token-transfer`](#transfer-tokens) | `arch-kit token-transfer <RECIPIENT> <MINT> <AMOUNT> --key <PATH>` | Transfer tokens to a user's ATA, creating it idempotently. |
| [`token-transfer-to-account`](#transfer-tokens) | `arch-kit token-transfer-to-account <DESTINATION> <MINT> <AMOUNT> --key <PATH>` | Transfer tokens directly to an APL token account. |
| [`faucet`](#fund-an-account) | `arch-kit faucet --key <PATH>` | Create or fund an account using a non-mainnet faucet. |
| [`transfer-arch`](#transfer-native-arch) | `arch-kit transfer-arch <DESTINATION> <AMOUNT> --key <PATH>` | Transfer native ARCH to an account. |

Run `arch-kit <COMMAND> --help` for the complete option list.

## Network configuration

Networked commands share these top-level settings:

| Setting | CLI argument | Environment variable | Default |
| --- | --- | --- | --- |
| Arch RPC | `--rpc-url <URL>` | `ARCH_RPC_URL` | `https://rpc.testnet.arch.network` |
| Bitcoin network | `--bitcoin-network <NETWORK>` | `ARCH_BITCOIN_NETWORK` | `testnet` |

Explicit arguments override environment variables and defaults. Place them
before the command, for example `arch-kit --bitcoin-network regtest deploy ...`.
Supported networks are `mainnet`, `testnet`, `testnet4`, `signet`, and
`regtest`.

## Initialize a program

```bash
arch-kit init ./hello-world --program-key ./keys/program.key
```

The destination must not exist. The command creates a Satellite program whose
declared ID is derived from the supplied program key. Its `say_hello`
instruction requires a user signer and logs `Hello <USER_BASE58_PUBKEY>`; the
signature constraint uses the custom error defined in `src/error.rs`. The
secret key is read only and is not copied into the project.

Build the generated program from its project directory, or pass its manifest
path explicitly:

```bash
cargo build-sbf --manifest-path ./hello-world/Cargo.toml
```

## Check node health

```bash
arch-kit health
```

The command checks validator readiness, reports RPC latency, and samples the
block height twice. It exits successfully only when the node is ready and the
height increases during its two-second observation window.

## Generate keys

Generate one or several independent keys:

```bash
arch-kit keygen ./keys/program.key ./keys/authority.key
```

Search for a Base58 Arch public-key prefix, optionally limiting CPU threads:

```bash
arch-kit keygen --prefix PAMM --threads 8 ./keys/vanity-program.key
```

Parent directories must exist, and destination paths must not. Existing paths
are never replaced. Secrets are stored as SDK-compatible hex, never printed,
and created with `0600` permissions on Unix.

Vanity search uses all available CPU parallelism by default. Each additional
Base58 character increases the rough expected work by about 58 times; the
estimate is only a baseline because first characters are not uniformly
distributed.

## Derive a public key

```bash
arch-kit pubkey ./keys/authority.key
```

The command reads either supported secret-key file format and writes only the
derived Base58 Arch public key to standard output.

## Inspect tokens

Derive an ATA locally without contacting an RPC node:

```bash
arch-kit ata <OWNER> <MINT>
```

Read its balance or inspect token state:

```bash
arch-kit token-balance <OWNER> <MINT>
arch-kit token-account <TOKEN_ACCOUNT>
arch-kit token-accounts <OWNER>
arch-kit mint-info <MINT>
```

Public keys may be Base58 or 64-character hex. Amounts include both raw and
decimal-formatted values. RPC-backed token commands accept `--json`; raw token
amounts are encoded as strings in JSON to preserve full `u64` precision. A
missing ATA has a zero balance and `exists: false`, while malformed or
incorrectly owned accounts are errors.

## Create a mint

Create a mint using existing mint and authority key files:

```bash
arch-kit create-mint \
  --mint-key ./keys/mint.key \
  --key ./keys/authority.key \
  --decimals 6 \
  --initial-supply 1000000
```

Decimals default to `9`. The payer also becomes the mint authority. Mints are
non-freezable by default; pass `--freeze-authority <PUBKEY>` to set one. Initial
supply is minted to the authority's ATA in the same transaction. Add
`--fixed-supply` to permanently revoke mint authority after the initial mint.

## Mint tokens

Mint additional tokens to a user's ATA, creating it idempotently when needed:

```bash
arch-kit mint-tokens <RECIPIENT> <MINT> 100 --key ./keys/authority.key
```

Amounts are interpreted using the mint's decimals. Fixed-supply mints reject
this operation because they no longer have a mint authority.

## Transfer tokens

Transfer tokens from the signing key's ATA to another user's ATA. The recipient
ATA is derived and idempotently created in the same transaction:

```bash
arch-kit token-transfer <RECIPIENT> <MINT> 1.25 --key ./keys/owner.key
```

Transfer directly to an existing token account, including a non-ATA account:

```bash
arch-kit token-transfer-to-account <TOKEN_ACCOUNT> <MINT> 1.25 \
  --key ./keys/owner.key
```

Amounts are human-readable decimals interpreted using the mint's configured
decimals. Both commands derive the source ATA from the signing key by default;
pass `--source <TOKEN_ACCOUNT>` to use another token account owned by the same
signer.

## Fund an account

Create or top up an account through the configured network's faucet:

```bash
arch-kit faucet --key ./keys/owner.key
```

The command is unavailable on mainnet. It waits for funding to be processed
and reports the resulting native ARCH balance.

## Transfer native ARCH

Transfer native ARCH using a local secret key file:

```bash
arch-kit transfer-arch <DESTINATION> 0.1 --key ./keys/owner.key
```

ARCH uses nine decimal places. The command validates the sender's system
account and requires enough balance for both the amount and the network's
5,000-lamport base fee before submitting a native system transfer. For mainnet,
place the shared network arguments before the command:

```bash
arch-kit --rpc-url https://rpc.mainnet.arch.network \
  --bitcoin-network mainnet \
  transfer-arch <DESTINATION> 0.1 --key ./keys/owner.key
```

## Deploy a program

```bash
arch-kit deploy \
  --elf ./target/deploy/example.so \
  --program-key ./keys/program.key \
  --authority ./keys/authority.key
```

Key files may contain a secp256k1 secret-key string or an SDK-compatible JSON
byte array.

Useful deployment options:

- `--generate-if-missing` securely creates missing program or authority keys.
- `--fund-authority` requests faucet funding before deployment; it is rejected
  on mainnet.
- `--idl <PATH>` publishes or upgrades an IDL after deployment.
- `--idl-size <BYTES>` sets the initial IDL account size and requires `--idl`.

IDL accounts default to at least 10,000 bytes, including the 44-byte header.
Reserve enough capacity for future upgrades because a populated IDL account
cannot be grown. The target program must include compatible canonical Satellite
IDL handlers. If IDL publication fails, the deployed program remains deployed
and its program ID is included in the error.
