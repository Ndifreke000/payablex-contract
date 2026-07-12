#![no_std]
//! # Invoice registry (Payable)
//!
//! Commits invoice hashes on-chain so that backend-held metadata (line items, PDF,
//! description) is verifiable byte-for-byte against the chain. A hash can be committed
//! exactly once — the on-chain record is immutable, satisfying the domain rule that an
//! invoice's committed hash cannot be altered after the fact.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const BUMP_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// This invoice hash has already been committed and cannot be overwritten.
    AlreadyCommitted = 1,
    /// No invoice is committed under the given hash.
    InvoiceNotFound = 2,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Invoice(BytesN<32>),
}

/// The immutable on-chain record for a committed invoice hash.
#[derive(Clone)]
#[contracttype]
pub struct InvoiceRecord {
    /// The address that committed the hash (authorized the call).
    pub committer: Address,
    /// A pointer to the off-chain metadata (e.g. an IPFS/HTTPS URI).
    pub metadata_uri: String,
    /// Ledger sequence at which the invoice was committed.
    pub committed_ledger: u32,
}

#[contract]
pub struct InvoiceRegistry;

#[contractimpl]
impl InvoiceRegistry {
    /// Commit an invoice hash with a pointer to its off-chain metadata. Requires the
    /// committer's authorization. Fails if the hash was already committed (immutable).
    pub fn commit_invoice(
        env: Env,
        committer: Address,
        hash: BytesN<32>,
        metadata_uri: String,
    ) -> Result<(), Error> {
        committer.require_auth();

        let key = DataKey::Invoice(hash.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyCommitted);
        }

        let record = InvoiceRecord {
            committer: committer.clone(),
            metadata_uri,
            committed_ledger: env.ledger().sequence(),
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_AMOUNT);

        env.events()
            .publish((symbol_short!("commit"), committer), hash);
        Ok(())
    }

    /// Fetch a committed invoice record, or error if none exists.
    pub fn get_invoice(env: Env, hash: BytesN<32>) -> Result<InvoiceRecord, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Invoice(hash))
            .ok_or(Error::InvoiceNotFound)
    }

    /// Whether a hash has been committed.
    pub fn has_invoice(env: Env, hash: BytesN<32>) -> bool {
        env.storage().persistent().has(&DataKey::Invoice(hash))
    }

    pub fn version(_env: Env) -> u32 {
        1
    }
}

mod test;
