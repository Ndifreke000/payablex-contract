# Contributing to payablex-contract

Thanks for your interest in contributing. These are the Soroban (Rust) smart contracts
for **Payable**, the accounts-payable workflow for Soroban organizations: the
`multisig-treasury` and `invoice-registry`. This is the most security-sensitive part of
the project — funds live here — so the bar for changes is high.

## Prerequisites

- **Rust / Cargo 1.94+** with the `wasm32v1-none` target for building Wasm
- **Stellar CLI** (27.0.0+) for building and deploying contracts

## Setup

```bash
git clone https://github.com/payableX/payablex-contract.git
cd payablex-contract
cargo build
cargo test
```

## Before you open a PR

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
stellar contract build   # Wasm must build (target wasm32v1-none)
```

PRs must be green.

## Ground rules

These are non-negotiable. Changes that violate them will not be merged:

1. **The threshold is enforced on-chain.** M-of-N approval is checked in the contract,
   never assumed to be handled elsewhere.
2. **No admin backdoor to funds.** Pause may stop new executions; it must never enable
   draining or reversing approved payments.
3. **Every state-changing call verifies signatures against the current signer set.**
   Signer-set changes are themselves multisig-approved.
4. **Amounts are 7-decimal USDC** and handled through the contract's amount handling — no
   ad-hoc `i128` arithmetic that can silently overflow or misplace decimals.
5. **The invoice hash is immutable once committed.** Approved amount, recipient, and memo
   must match it byte-for-byte.

## Tests

Adversarial tests are expected for every behavioral change, not just happy-path:

- replay of an already-executed proposal
- approval or execution by a non-signer
- execution below threshold
- attempts to drain, reverse, or double-spend
- signer-set changes without sufficient approvals

## Scope of changes

- Keep PRs focused. One issue, one PR where possible.
- New features should reference an issue with a context paragraph, acceptance criteria,
  files to touch, and test expectations.

## Commit and PR conventions

- Write clear, imperative commit messages ("Guard execute_payment against replay", not "fixes").
- Link the issue your PR closes.

## Reporting security issues

**Do not open a public issue for a vulnerability.** A flaw here can put real funds at
risk. Report it privately to the maintainers first, and give us time to ship a fix before
any disclosure.

## License

By contributing, you agree that your contributions are licensed under Apache-2.0.
