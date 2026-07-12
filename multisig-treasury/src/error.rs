use soroban_sdk::contracterror;

/// Typed contract errors. Returned as `Result::Err` so callers (and tests via the
/// generated `try_*` client methods) get a structured error instead of an opaque trap.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// `init` was already called.
    AlreadyInitialized = 1,
    /// A call was made before `init`.
    NotInitialized = 2,
    /// The caller is not a member of the current signer set.
    NotAuthorized = 3,
    /// Signer set is empty or exceeds the maximum.
    InvalidSigners = 4,
    /// Threshold is zero or greater than the number of signers.
    InvalidThreshold = 5,
    /// The same address appears twice in a signer set.
    DuplicateSigner = 6,
    /// This signer has already approved this proposal.
    DuplicateApproval = 7,
    /// No proposal exists with the given id.
    ProposalNotFound = 8,
    /// The proposal has already been executed.
    AlreadyExecuted = 9,
    /// Current valid approvals are below the threshold.
    ThresholdNotMet = 10,
    /// New payment executions are paused.
    Paused = 11,
    /// Payment amount is not strictly positive.
    InvalidAmount = 12,
    /// Tried to execute a proposal through the wrong entrypoint for its kind.
    WrongProposalKind = 13,
}
