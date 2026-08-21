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
