use soroban_sdk::{contracttype, Address, Symbol};

/// Unique identifier for an AllowanceRecord.
/// Generated as SHA-256 of (subscriber, merchant, creation_ledger).
pub type AllowanceId = soroban_sdk::BytesN<32>;

/// Unique identifier for a billing cycle nonce entry.
pub type NonceKey = (AllowanceId, u64);

// ---------------------------------------------------------------------------
// Core record
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowanceRecord {
    /// Address of the subscriber (wallet that pays).
    pub subscriber: Address,
    /// Address of the merchant (wallet that receives).
    pub merchant: Address,
    /// Stellar token contract address for the billing asset (e.g. USDC).
    pub asset: Address,
    /// Maximum amount the contract may debit per billing cycle.
    /// Uses 7-decimal Stellar precision (e.g. 12_0000000 = $12.00).
    pub max_amount: i128,
    /// Billing interval expressed in ledgers.
    /// ~86400 ledgers ≈ 1 day; ~2_592_000 ledgers ≈ 30 days.
    pub interval_ledgers: u32,
    /// Ledger at which the first billing cycle may execute.
    pub next_billing_ledger: u32,
    /// Optional cap on total billing cycles. None = unlimited.
    pub max_cycles: Option<u32>,
    /// Number of successfully completed billing cycles so far.
    pub cycles_completed: u32,
    /// Monotonically-incrementing nonce — prevents double-execution
    /// within the same billing window.
    pub nonce: u64,
    /// XLM tip paid to the keeper that executes billing (in stroops).
    pub keeper_tip: i128,
    /// Current lifecycle state of this allowance.
    pub state: AllowanceState,
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllowanceState {
    /// Allowance is active — keepers may execute billing on schedule.
    Active,
    /// Billing is temporarily suspended.
    /// If `resume_ledger` is Some(n), the allowance auto-resumes at ledger n.
    Paused { resume_ledger: Option<u32> },
    /// Subscriber cancelled — no further billing possible.
    Revoked,
    /// max_cycles reached — allowance completed naturally.
    Completed,
    /// Subscriber had insufficient funds for billing and the retry window
    /// closed without a successful execution.
    Lapsed,
}

// ---------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BillingResult {
    /// Billing executed successfully.
    Success {
        amount: i128,
        ledger: u32,
        new_nonce: u64,
    },
    /// Subscriber balance was insufficient.
    /// The contract stores a retry window; keepers may re-attempt.
    InsufficientFunds { retry_window_closes: u32 },
}

// ---------------------------------------------------------------------------
// Storage key schema
// ---------------------------------------------------------------------------

/// All keys used in Flux contract persistent storage.
#[contracttype]
pub enum DataKey {
    /// AllowanceRecord keyed by AllowanceId.
    Allowance(AllowanceId),
    /// Used-nonce guard: (AllowanceId, nonce) → true once consumed.
    UsedNonce(AllowanceId, u64),
    /// Per-merchant gas pool balance (in stroops).
    GasPool(Address),
    /// Contract-level configuration (admin, retry window, etc.).
    Config,
}

// ---------------------------------------------------------------------------
// Contract configuration
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug)]
pub struct ContractConfig {
    /// Account authorised to register reporters and upgrade the contract.
    pub admin: Address,
    /// How long (in ledgers) a billing retry window stays open.
    /// Default: 17_280 ledgers ≈ 72 hours at 5-second close time.
    pub retry_window_ledgers: u32,
    /// Maximum XLM keeper tip any allowance may specify (in stroops).
    pub max_keeper_tip: i128,
}

impl Default for ContractConfig {
    fn default() -> Self {
        ContractConfig {
            admin: soroban_sdk::Address::from_str(
                &soroban_sdk::Env::default(),
                "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
            )
            .unwrap(),
            retry_window_ledgers: 17_280,
            max_keeper_tip: 10_000_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Event topics (used with env.events().publish())
// ---------------------------------------------------------------------------

pub const EVENT_ALLOWANCE_CREATED: &str   = "AllowanceCreated";
pub const EVENT_BILLING_EXECUTED: &str    = "BillingExecuted";
pub const EVENT_BILLING_FAILED: &str      = "BillingFailed";
pub const EVENT_ALLOWANCE_REVOKED: &str   = "AllowanceRevoked";
pub const EVENT_ALLOWANCE_PAUSED: &str    = "AllowancePaused";
pub const EVENT_ALLOWANCE_RESUMED: &str   = "AllowanceResumed";
pub const EVENT_ALLOWANCE_COMPLETED: &str = "AllowanceCompleted";
pub const EVENT_ALLOWANCE_LAPSED: &str    = "AllowanceLapsed";
pub const EVENT_GAS_POOL_DEPOSITED: &str  = "GasPoolDeposited";
pub const EVENT_GAS_POOL_WITHDRAWN: &str  = "GasPoolWithdrawn";
