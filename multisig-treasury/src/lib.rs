#![no_std]
//! # Multisig treasury (Payable)
//!
//! An M-of-N multisig USDC treasury for Stellar Soroban. Signers propose payments
//! against an on-chain invoice hash; once a configurable threshold of the *current*
//! signer set approves, anyone can execute the payment. An admin may pause new
//! executions but can never move, drain, or reverse funds.
//!
//! ## Auth model
//!
//! Authorization uses Soroban's native `require_auth` on `Address`es rather than manual
//! signature passing: a signer authorizes an `approve`/`propose` call by signing the
//! transaction (Freighter-compatible). The contract additionally checks that the
//! authorized address is a member of the current signer set. This satisfies the domain
//! rule that every state-changing call is verified against the current signer set.
//!
//! ## Note on `init`
//!
//! `init` also takes the USDC token contract address (the asset the treasury pays out).
//! v0.1 is USDC-only; the token is fixed at bootstrap and never changed afterward.

mod error;
mod test;
mod types;

pub use error::Error;
pub use types::{PaymentDetails, Proposal, ProposalKind, SignerUpdate};

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, BytesN, Env, String, Vec};
use types::DataKey;

/// Maximum number of signers (domain rule: N up to 10).
const MAX_SIGNERS: u32 = 10;

// TTL management. Roughly 30 days of ledgers, bumped on every state change.
const DAY_IN_LEDGERS: u32 = 17_280;
const BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const BUMP_THRESHOLD: u32 = BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contract]
pub struct MultisigTreasury;

#[contractimpl]
impl MultisigTreasury {
    /// Bootstrap the treasury with founding signers, a threshold, an admin, and the
    /// USDC token address. Callable exactly once; requires the admin's authorization.
    pub fn init(
        env: Env,
        admin: Address,
        token: Address,
        signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        validate_signer_set(&signers, threshold)?;

        let store = env.storage().instance();
        store.set(&DataKey::Admin, &admin);
        store.set(&DataKey::Token, &token);
        store.set(&DataKey::Signers, &signers);
        store.set(&DataKey::Threshold, &threshold);
        store.set(&DataKey::Paused, &false);
        store.set(&DataKey::ProposalCount, &0u32);
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("init"),), (admin, threshold, signers.len()));
        Ok(())
    }

    /// Propose a payment. The proposer must be a current signer and is recorded as the
    /// first approver. Allowed even while paused (pause only blocks execution).
    pub fn propose_payment(
        env: Env,
        proposer: Address,
        invoice_hash: BytesN<32>,
        recipient: Address,
        amount: i128,
        memo: String,
    ) -> Result<u32, Error> {
        require_initialized(&env)?;
        proposer.require_auth();
        require_signer(&env, &proposer)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let details = PaymentDetails {
            invoice_hash,
            recipient,
            amount,
            memo,
        };
        let id = create_proposal(&env, &proposer, ProposalKind::Payment(details));
        env.events()
            .publish((symbol_short!("propose"), proposer), id);
        Ok(id)
    }

    /// Propose a change to the signer set and/or threshold. Applying it later is itself
    /// gated by the current signer set reaching threshold.
    ///
    /// Semantics for in-flight proposals (payment or signer-update alike): the
    /// *target* of a proposal (payment details, or the new signer set/threshold) is
    /// fixed at proposal time and never changes. The *approvals* on it, however, are
    /// always counted against whichever signer set is current at execution time — so
    /// if a rotation lands while another proposal is still pending, an approval from a
    /// signer removed by that rotation stops counting (see `Proposal::approvals`).
    /// This is deliberately stricter than proposal-time semantics: a since-removed
    /// signer can never push something across the line after they've been rotated out.
    pub fn propose_signer_update(
        env: Env,
        proposer: Address,
        new_signers: Vec<Address>,
        new_threshold: u32,
    ) -> Result<u32, Error> {
        require_initialized(&env)?;
        proposer.require_auth();
        require_signer(&env, &proposer)?;
        validate_signer_set(&new_signers, new_threshold)?;

        let update = SignerUpdate {
            new_signers,
            new_threshold,
        };
        let id = create_proposal(&env, &proposer, ProposalKind::SignerUpdate(update));
        env.events()
            .publish((symbol_short!("sgn_prop"), proposer), id);
        Ok(id)
    }

    /// Approve a proposal (payment or signer update). The caller must be a current
    /// signer and must not have already approved this proposal.
    pub fn approve(env: Env, signer: Address, proposal_id: u32) -> Result<(), Error> {
        require_initialized(&env)?;
        signer.require_auth();
        require_signer(&env, &signer)?;

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(Error::AlreadyExecuted);
        }
        if contains_addr(&proposal.approvals, &signer) {
            return Err(Error::DuplicateApproval);
        }
        proposal.approvals.push_back(signer.clone());
        write_proposal(&env, &proposal);

        env.events()
            .publish((symbol_short!("approve"), signer), proposal_id);
        Ok(())
    }

    /// Execute an approved payment. Permissionless once the threshold is met, but blocked
    /// while paused. Marks the proposal executed *before* transferring, so it can never
    /// be executed twice. Approvals from since-removed signers do not count.
    pub fn execute_payment(env: Env, proposal_id: u32) -> Result<(), Error> {
        require_initialized(&env)?;
        if read_paused(&env) {
            return Err(Error::Paused);
        }

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(Error::AlreadyExecuted);
        }
        let details = match &proposal.kind {
            ProposalKind::Payment(d) => d.clone(),
            ProposalKind::SignerUpdate(_) => return Err(Error::WrongProposalKind),
        };
        require_threshold(&env, &proposal.approvals)?;

        // Effects before interaction: mark executed, then transfer.
        proposal.executed = true;
        write_proposal(&env, &proposal);

        let token = read_token(&env);
        token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &details.recipient,
            &details.amount,
        );

        env.events().publish(
            (symbol_short!("execute"),),
            (proposal_id, details.recipient, details.amount),
        );
        Ok(())
    }

    /// Apply an approved signer update. Not blocked by pause, so a compromised signer can
    /// be removed during an emergency. Re-validates the new set at execution time.
    pub fn execute_signer_update(env: Env, proposal_id: u32) -> Result<(), Error> {
        require_initialized(&env)?;

        let mut proposal = read_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(Error::AlreadyExecuted);
        }
        let update = match &proposal.kind {
            ProposalKind::SignerUpdate(u) => u.clone(),
            ProposalKind::Payment(_) => return Err(Error::WrongProposalKind),
        };
        require_threshold(&env, &proposal.approvals)?;
        validate_signer_set(&update.new_signers, update.new_threshold)?;

        proposal.executed = true;
        write_proposal(&env, &proposal);

        let store = env.storage().instance();
        store.set(&DataKey::Signers, &update.new_signers);
        store.set(&DataKey::Threshold, &update.new_threshold);
        bump_instance(&env);

        env.events().publish(
            (symbol_short!("sgn_exec"),),
            (proposal_id, update.new_threshold, update.new_signers.len()),
        );
        Ok(())
    }

    /// Admin-only: pause new payment executions. Cannot touch funds.
    pub fn pause(env: Env) -> Result<(), Error> {
        require_initialized(&env)?;
        read_admin(&env).require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        bump_instance(&env);
        env.events().publish((symbol_short!("pause"),), ());
        Ok(())
    }

    /// Admin-only: resume payment executions.
    pub fn unpause(env: Env) -> Result<(), Error> {
        require_initialized(&env)?;
        read_admin(&env).require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        bump_instance(&env);
        env.events().publish((symbol_short!("unpause"),), ());
        Ok(())
    }

    // ---- Views ----

    pub fn version(_env: Env) -> u32 {
        1
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        require_initialized(&env)?;
        Ok(read_admin(&env))
    }

    pub fn get_token(env: Env) -> Result<Address, Error> {
        require_initialized(&env)?;
        Ok(read_token(&env))
    }

    pub fn get_signers(env: Env) -> Result<Vec<Address>, Error> {
        require_initialized(&env)?;
        Ok(read_signers(&env))
    }

    pub fn get_threshold(env: Env) -> Result<u32, Error> {
        require_initialized(&env)?;
        Ok(read_threshold(&env))
    }

    pub fn is_paused(env: Env) -> Result<bool, Error> {
        require_initialized(&env)?;
        Ok(read_paused(&env))
    }

    pub fn get_proposal(env: Env, proposal_id: u32) -> Result<Proposal, Error> {
        require_initialized(&env)?;
        read_proposal(&env, proposal_id)
    }

    /// Number of approvals on a proposal that are *still* valid signers.
    pub fn valid_approvals(env: Env, proposal_id: u32) -> Result<u32, Error> {
        require_initialized(&env)?;
        let proposal = read_proposal(&env, proposal_id)?;
        Ok(count_valid_approvals(
            &read_signers(&env),
            &proposal.approvals,
        ))
    }
}

// ---- Internal helpers ----

fn require_initialized(env: &Env) -> Result<(), Error> {
    if env.storage().instance().has(&DataKey::Admin) {
        Ok(())
    } else {
        Err(Error::NotInitialized)
    }
}

fn require_signer(env: &Env, who: &Address) -> Result<(), Error> {
    if contains_addr(&read_signers(env), who) {
        Ok(())
    } else {
        Err(Error::NotAuthorized)
    }
}

fn require_threshold(env: &Env, approvals: &Vec<Address>) -> Result<(), Error> {
    let valid = count_valid_approvals(&read_signers(env), approvals);
    if valid >= read_threshold(env) {
        Ok(())
    } else {
        Err(Error::ThresholdNotMet)
    }
}

fn validate_signer_set(signers: &Vec<Address>, threshold: u32) -> Result<(), Error> {
    let n = signers.len();
    if signers.is_empty() || n > MAX_SIGNERS {
        return Err(Error::InvalidSigners);
    }
    if threshold == 0 || threshold > n {
        return Err(Error::InvalidThreshold);
    }
    // Reject duplicate addresses.
    let mut i = 0u32;
    while i < n {
        let a = signers.get(i).unwrap();
        let mut j = i + 1;
        while j < n {
            if signers.get(j).unwrap() == a {
                return Err(Error::DuplicateSigner);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

fn contains_addr(list: &Vec<Address>, who: &Address) -> bool {
    for a in list.iter() {
        if &a == who {
            return true;
        }
    }
    false
}

fn count_valid_approvals(current_signers: &Vec<Address>, approvals: &Vec<Address>) -> u32 {
    let mut count = 0u32;
    for a in approvals.iter() {
        if contains_addr(current_signers, &a) {
            count += 1;
        }
    }
    count
}

fn create_proposal(env: &Env, proposer: &Address, kind: ProposalKind) -> u32 {
    let id: u32 = env
        .storage()
        .instance()
        .get(&DataKey::ProposalCount)
        .unwrap_or(0)
        + 1;
    env.storage().instance().set(&DataKey::ProposalCount, &id);
    bump_instance(env);

    let mut approvals = Vec::new(env);
    approvals.push_back(proposer.clone());
    let proposal = Proposal {
        id,
        proposer: proposer.clone(),
        kind,
        approvals,
        executed: false,
    };
    write_proposal(env, &proposal);
    id
}

fn read_proposal(env: &Env, id: u32) -> Result<Proposal, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(id))
        .ok_or(Error::ProposalNotFound)
}

fn write_proposal(env: &Env, proposal: &Proposal) {
    let key = DataKey::Proposal(proposal.id);
    env.storage().persistent().set(&key, proposal);
    env.storage()
        .persistent()
        .extend_ttl(&key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(BUMP_THRESHOLD, BUMP_AMOUNT);
}

fn read_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

fn read_token(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Token).unwrap()
}

fn read_signers(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::Signers).unwrap()
}

fn read_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::Threshold).unwrap()
}

fn read_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap()
}
