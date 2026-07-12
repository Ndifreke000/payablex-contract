use soroban_sdk::{contracttype, Address, BytesN, String, Vec};

/// Storage keys. Config lives in instance storage; proposals in persistent storage
/// keyed by id (they grow unbounded).
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Signers,
    Threshold,
    Paused,
    ProposalCount,
    Proposal(u32),
}

/// A payment proposal's on-chain, immutable-once-proposed details.
///
/// The `invoice_hash` binds this payment to an off-chain invoice; the backend's stored
/// metadata must hash to exactly this value. `amount` is USDC in stroops (7 decimals).
#[derive(Clone)]
#[contracttype]
pub struct PaymentDetails {
    pub invoice_hash: BytesN<32>,
    pub recipient: Address,
    pub amount: i128,
    pub memo: String,
}

/// A proposed change to the signer set and/or threshold. Applying it requires the
/// *current* signer set to approve, so governance is itself multisig-gated.
#[derive(Clone)]
#[contracttype]
pub struct SignerUpdate {
    pub new_signers: Vec<Address>,
    pub new_threshold: u32,
}

/// What a proposal will do once it reaches threshold.
#[derive(Clone)]
#[contracttype]
pub enum ProposalKind {
    Payment(PaymentDetails),
    SignerUpdate(SignerUpdate),
}

/// A pending or executed multisig proposal.
#[derive(Clone)]
#[contracttype]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    pub kind: ProposalKind,
    /// Addresses that have approved. The proposer is included at creation.
    /// Membership is re-validated against the current signer set at execution time,
    /// so an approval from a since-removed signer does not count toward the threshold.
    pub approvals: Vec<Address>,
    pub executed: bool,
}
