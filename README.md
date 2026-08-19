# arch-kit

`arch-kit` is a CLI for generating Arch keys, deploying programs, and
publishing canonical on-chain IDLs.

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

## Commands

| Command | Usage | Description |
| --- | --- | --- |
| [`keygen`](#generate-keys) | `arch-kit keygen [OPTIONS] <PATH>...` | Generate one or more secp256k1 key files, with optional public key prefixes (vanity). |
| [`deploy`](#deploy-a-program) | `arch-kit deploy [OPTIONS]` | Deploy or update a program and its IDL. |

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
