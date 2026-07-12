# payablex-contract

**Core Soroban (Rust) smart contracts powering Payable — the accounts-payable workflow for Soroban organizations on Stellar.**

This Cargo workspace holds the two contracts that make Payable trustless: an M-of-N
**multisig treasury** that gates every USDC payment behind on-chain approvals, and an
**invoice registry** that commits an immutable hash of each approved invoice so its
amount, recipient, and memo can be verified byte-for-byte against the chain.

## Contracts

### `multisig-treasury`
`init` · `propose_payment` · `approve_payment` · `execute_payment`, plus admin pause and
multisig-gated signer-set updates. The M-of-N threshold is **enforced on-chain**, never
gated only by a backend. Emergency pause can halt new executions but **cannot drain
funds or reverse approved payments** — there is no admin backdoor to funds.

### `invoice-registry`
Commits invoice hashes on-chain so off-chain metadata stored by the backend is
verifiable against the chain at approval time.

## Domain rules (non-negotiable)

1. Multisig is M-of-N with N up to 10; M is configurable per org, enforced on-chain.
2. Every invoice has an immutable on-chain hash committed at approval time.
3. USDC is 7 decimals on Stellar — all amount math goes through one Amount module; no
   raw `i128` arithmetic elsewhere.
4. All state-changing calls require signature verification against the current signer
   set; signer-set changes are themselves multisig-approved.

## Deployments (testnet)

Deployed **2026-07-05** with Stellar CLI 27.0.0 (target `wasm32v1-none`). Network
passphrase: `Test SDF Network ; September 2015`.

| Contract | Version | Contract ID |
|---|---|---|
| `multisig-treasury` | v0.1.0 | `CAX6THSV346KF6E5QZTYQJKABDBBWUU4JY6FJ34GPHY4JB3VHOEFZ6OQ` |
| `invoice-registry` | v0.1.0 | `CDY4S22DMAPKM7PKFQHMLJ4VHLWZLHZMRZB2D2ISFR53LX73MGUNFW4T` |

Explorer:
[multisig-treasury](https://stellar.expert/explorer/testnet/contract/CAX6THSV346KF6E5QZTYQJKABDBBWUU4JY6FJ34GPHY4JB3VHOEFZ6OQ)
·
[invoice-registry](https://stellar.expert/explorer/testnet/contract/CDY4S22DMAPKM7PKFQHMLJ4VHLWZLHZMRZB2D2ISFR53LX73MGUNFW4T)

> Status: deployed but **not yet initialized** — `multisig-treasury.init` still needs
> the founding signer addresses, threshold, and USDC token contract address.

## Getting started

Requires the **Rust/Cargo** toolchain (1.94+) and the **Stellar CLI** to build and
deploy.

```bash
cargo build            # build the workspace
cargo test             # run contract tests
stellar contract build # build Wasm (target wasm32v1-none)
```

## Related repositories

- [`payablex-frontend`](https://github.com/payableX/payablex-frontend) — Next.js 15 dashboard
- [`payablex-backend`](https://github.com/payableX/payablex-backend) — Fastify + GraphQL API and Soroban indexer

## License

[Apache-2.0](./LICENSE)
