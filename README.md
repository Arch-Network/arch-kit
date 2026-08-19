# arch-kit

`arch-kit` is a small CLI for deploying Arch Network programs and publishing
their canonical on-chain IDLs.

## Build

```bash
cargo build --release
```

## Deploy a program

The program identity and authority keypair files must already exist. Key files
may contain either a secp256k1 secret-key string or a JSON byte array accepted
by the Arch SDK.

```bash
arch-kit \
  --rpc-url http://127.0.0.1:9002 \
  --bitcoin-network regtest \
  deploy \
  --elf ./target/deploy/example.so \
  --program-key ./keys/example-program.json \
  --authority ./keys/authority.json
```

`--rpc-url` and `--bitcoin-network` can instead be supplied through
`ARCH_RPC_URL` and `ARCH_BITCOIN_NETWORK`. Supported signing networks are
`mainnet`, `testnet`, `testnet4`, `signet`, and `regtest`.

The authority must already have enough lamports to pay deployment rent and
transaction fees. On a faucet-enabled network, opt into authority funding with:

```bash
arch-kit \
  --rpc-url http://127.0.0.1:9002 \
  --bitcoin-network regtest \
  deploy \
  --elf ./target/deploy/example.so \
  --program-key ./keys/example-program.json \
  --authority ./keys/authority.json \
  --fund-authority
```

Faucet funding is rejected when `--bitcoin-network mainnet` is selected. The
SDK's standard program-authority faucet grants are requested once; `arch-kit`
does not estimate the deployment cost or repeatedly top up the authority.

## Publish an IDL with deployment

Pass `--idl` to publish the IDL after the program deployment succeeds:

```bash
arch-kit \
  --rpc-url http://127.0.0.1:9002 \
  --bitcoin-network regtest \
  deploy \
  --elf ./target/deploy/example.so \
  --program-key ./keys/example-program.json \
  --authority ./keys/authority.json \
  --idl ./target/idl/example.json \
  --idl-size 20000
```

The initial canonical IDL account defaults to at least 10,000 bytes.
`--idl-size` selects a different minimum total account size, including the
44-byte canonical IDL header. It requires `--idl` and must fit the compressed
IDL. Account creation is followed by as many pre-write resize transactions as
needed.

On later runs, `arch-kit` skips an identical IDL or upgrades changed content
through the standard buffer flow. A populated Satellite IDL account cannot be
grown, so a replacement or explicitly requested `--idl-size` must fit the
existing capacity. Reserve enough initial capacity for expected IDL growth.

Before publication, `arch-kit` sets the IDL's `address` to the deployed program
ID in hex and `metadata.spec` to `0.1.0`. The target program must include the
canonical Satellite IDL dispatch handlers (or a compatible implementation).

Deployment and IDL publication are sequential. If IDL publication fails, the
program remains deployed and the error includes its program ID.
