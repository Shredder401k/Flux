use soroban_sdk::contracterror;

/// All error conditions that the Flux contract may return.
///
/// Variants are assigned stable u32 discriminants — these must never
/// be renumbered once deployed to mainnet, as on-chain tooling and
/// SDKs may rely on numeric codes.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FluxError {
    /// No AllowanceRecord exists for the given AllowanceId.
    AllowanceNotFound = 1,

    /// The allowance has been revoked by the subscriber and cannot be
    /// used for any further operations.
    AllowanceRevoked = 2,

    /// `execute_billing` was called before `next_billing_ledger` has
    /// been reached. The keeper must wait.
    AllowanceNotDue = 3,

    /// The subscriber's token balance was insufficient to cover the
    /// billing amount at the time of execution.
    InsufficientBalance = 4,

    /// The allowance has reached its `max_cycles` limit and is now in
    /// `Completed` state.
    ExceededMaxCycles = 5,

    /// A keeper attempted to execute billing with a nonce that has
    /// already been consumed in this cycle.
    AlreadyExecuted = 6,

    /// The caller is not authorised to perform the requested operation.
    /// Typically raised when a non-subscriber attempts to revoke or
    /// pause an allowance.
    Unauthorised = 7,

    /// The merchant's gas pool has insufficient XLM to pay the keeper
    /// tip for this billing execution.
    GasPoolEmpty = 8,

    /// `execute_billing` was called while the allowance is in
    /// `Paused` state and the resume ledger has not yet arrived.
    AllowancePaused = 9,

    /// The contract has already been initialised. `initialize` may
    /// only be called once.
    AlreadyInitialized = 10,

    /// A required configuration value was missing or invalid during
    /// contract initialization.
    InvalidConfig = 11,

    /// The supplied `max_amount` is zero or negative.
    InvalidAmount = 12,

    /// The supplied `interval_ledgers` is zero.
    InvalidInterval = 13,
}