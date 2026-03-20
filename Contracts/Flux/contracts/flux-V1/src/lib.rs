#![no_std]

mod errors;
mod storage;
mod types;

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

use errors::FluxError;
use storage::{
    allowance_exists, gas_pool_balance, is_initialized, load_allowance,
    load_config, record_id, save_allowance, save_config, set_gas_pool_balance,
};
use types::{
    AllowanceId, AllowanceRecord, AllowanceState, BillingResult,
    ContractConfig, EVENT_ALLOWANCE_CREATED, EVENT_ALLOWANCE_PAUSED,
    EVENT_ALLOWANCE_RESUMED, EVENT_ALLOWANCE_REVOKED,
    EVENT_GAS_POOL_DEPOSITED, EVENT_GAS_POOL_WITHDRAWN,
};

// ---------------------------------------------------------------------------
// Contract declaration
// ---------------------------------------------------------------------------

#[contract]
pub struct FluxContract;

#[contractimpl]
impl FluxContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the Flux contract.  Must be called exactly once after
    /// deployment.  Sets the admin address, retry window, and max keeper tip.
    ///
    /// # Panics
    /// - `FluxError::AlreadyInitialized` if called more than once.
    /// - `FluxError::InvalidConfig` if `retry_window_ledgers` is zero.
    pub fn initialize(
        env: Env,
        admin: Address,
        retry_window_ledgers: u32,
        max_keeper_tip: i128,
    ) -> Result<(), FluxError> {
        if is_initialized(&env) {
            return Err(FluxError::AlreadyInitialized);
        }
        if retry_window_ledgers == 0 {
            return Err(FluxError::InvalidConfig);
        }

        admin.require_auth();

        let config = ContractConfig {
            admin,
            retry_window_ledgers,
            max_keeper_tip,
        };
        save_config(&env, &config);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Allowance lifecycle
    // -----------------------------------------------------------------------

    /// Create a new billing allowance.
    ///
    /// The subscriber signs this transaction once at checkout, authorising
    /// the Flux contract to pull up to `max_amount` every `interval_ledgers`
    /// ledgers for `max_cycles` cycles (or indefinitely if `max_cycles` is
    /// `None`).
    ///
    /// The subscriber's funds are **not** moved at this point — they remain
    /// in the subscriber's wallet until a keeper calls `execute_billing`.
    ///
    /// # Parameters
    /// - `subscriber`       — wallet that will be debited each cycle.
    /// - `merchant`         — wallet that receives each payment.
    /// - `asset`            — Stellar token contract address (e.g. USDC).
    /// - `max_amount`       — maximum debit per cycle (7-decimal precision).
    /// - `interval_ledgers` — billing cadence in ledgers (must be > 0).
    /// - `max_cycles`       — optional cap; `None` = unlimited.
    /// - `keeper_tip`       — XLM tip (in stroops) paid to the executing keeper.
    ///
    /// # Returns
    /// The newly created `AllowanceId`.
    ///
    /// # Errors
    /// - `FluxError::InvalidAmount`   — `max_amount` is zero or negative.
    /// - `FluxError::InvalidInterval` — `interval_ledgers` is zero.
    /// - `FluxError::Unauthorised`    — caller did not supply subscriber auth.
    ///
    /// # Events
    /// Emits `AllowanceCreated` with the new `AllowanceId`.
    pub fn create_allowance(
        env: Env,
        subscriber: Address,
        merchant: Address,
        asset: Address,
        max_amount: i128,
        interval_ledgers: u32,
        max_cycles: Option<u32>,
        keeper_tip: i128,
    ) -> Result<AllowanceId, FluxError> {
        // --- Input validation ------------------------------------------------
        if max_amount <= 0 {
            return Err(FluxError::InvalidAmount);
        }
        if interval_ledgers == 0 {
            return Err(FluxError::InvalidInterval);
        }

        // Subscriber must sign this transaction.
        subscriber.require_auth();

        // --- Build the record ------------------------------------------------
        let current_ledger = env.ledger().sequence();

        let record = AllowanceRecord {
            subscriber: subscriber.clone(),
            merchant: merchant.clone(),
            asset,
            max_amount,
            interval_ledgers,
            next_billing_ledger: current_ledger
                .checked_add(interval_ledgers)
                .expect("ledger overflow"),
            max_cycles,
            cycles_completed: 0,
            nonce: 0,
            keeper_tip,
            state: AllowanceState::Active,
        };

        let id = record_id(&env, &record);
        save_allowance(&env, &record);

        // --- Emit event ------------------------------------------------------
        env.events().publish(
            (Symbol::new(&env, EVENT_ALLOWANCE_CREATED),),
            (id.clone(), subscriber, merchant),
        );

        Ok(id)
    }

    /// Revoke an allowance immediately.
    ///
    /// Only the `subscriber` on the record may call this function.  Once
    /// revoked, all future `execute_billing` calls will be rejected and the
    /// state can never be changed back to `Active`.
    ///
    /// # Errors
    /// - `FluxError::AllowanceNotFound` — unknown id.
    /// - `FluxError::Unauthorised`      — caller is not the subscriber.
    /// - `FluxError::AllowanceRevoked`  — already revoked.
    ///
    /// # Events
    /// Emits `AllowanceRevoked`.
    pub fn revoke_allowance(
        env: Env,
        allowance_id: AllowanceId,
        subscriber: Address,
    ) -> Result<(), FluxError> {
        subscriber.require_auth();

        let mut record = load_allowance(&env, &allowance_id)?;

        if record.subscriber != subscriber {
            return Err(FluxError::Unauthorised);
        }
        if matches!(record.state, AllowanceState::Revoked) {
            return Err(FluxError::AllowanceRevoked);
        }

        record.state = AllowanceState::Revoked;
        save_allowance(&env, &record);

        env.events().publish(
            (Symbol::new(&env, EVENT_ALLOWANCE_REVOKED),),
            (allowance_id, subscriber),
        );

        Ok(())
    }

    /// Pause an allowance, optionally providing a ledger at which it should
    /// automatically resume.
    ///
    /// While paused, keeper calls to `execute_billing` are rejected without
    /// consuming the nonce.  The subscriber may call `resume_allowance` at
    /// any time to re-activate immediately.
    ///
    /// # Parameters
    /// - `resume_ledger` — `Some(n)` to auto-resume at ledger n; `None` to
    ///   pause indefinitely until `resume_allowance` is called.
    ///
    /// # Errors
    /// - `FluxError::AllowanceNotFound` — unknown id.
    /// - `FluxError::Unauthorised`      — caller is not the subscriber.
    /// - `FluxError::AllowanceRevoked`  — cannot pause a revoked allowance.
    ///
    /// # Events
    /// Emits `AllowancePaused`.
    pub fn pause_allowance(
        env: Env,
        allowance_id: AllowanceId,
        subscriber: Address,
        resume_ledger: Option<u32>,
    ) -> Result<(), FluxError> {
        subscriber.require_auth();

        let mut record = load_allowance(&env, &allowance_id)?;

        if record.subscriber != subscriber {
            return Err(FluxError::Unauthorised);
        }
        if matches!(record.state, AllowanceState::Revoked) {
            return Err(FluxError::AllowanceRevoked);
        }

        record.state = AllowanceState::Paused { resume_ledger };
        save_allowance(&env, &record);

        env.events().publish(
            (Symbol::new(&env, EVENT_ALLOWANCE_PAUSED),),
            (allowance_id, subscriber, resume_ledger),
        );

        Ok(())
    }

    /// Resume a paused allowance.
    ///
    /// # Errors
    /// - `FluxError::AllowanceNotFound` — unknown id.
    /// - `FluxError::Unauthorised`      — caller is not the subscriber.
    ///
    /// # Events
    /// Emits `AllowanceResumed`.
    pub fn resume_allowance(
        env: Env,
        allowance_id: AllowanceId,
        subscriber: Address,
    ) -> Result<(), FluxError> {
        subscriber.require_auth();

        let mut record = load_allowance(&env, &allowance_id)?;

        if record.subscriber != subscriber {
            return Err(FluxError::Unauthorised);
        }

        record.state = AllowanceState::Active;
        save_allowance(&env, &record);

        env.events().publish(
            (Symbol::new(&env, EVENT_ALLOWANCE_RESUMED),),
            (allowance_id, subscriber),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // View functions
    // -----------------------------------------------------------------------

    /// Retrieve the full `AllowanceRecord` for a given id.
    ///
    /// No auth required — allowance data is public.
    pub fn get_allowance(
        env: Env,
        allowance_id: AllowanceId,
    ) -> Result<AllowanceRecord, FluxError> {
        load_allowance(&env, &allowance_id)
    }

    /// Returns `true` if the allowance exists and is currently due for
    /// billing (i.e. `Active` and `next_billing_ledger <= current_ledger`).
    pub fn is_billing_due(env: Env, allowance_id: AllowanceId) -> bool {
        match load_allowance(&env, &allowance_id) {
            Ok(record) => {
                matches!(record.state, AllowanceState::Active)
                    && env.ledger().sequence() >= record.next_billing_ledger
            }
            Err(_) => false,
        }
    }

    /// Return the current gas pool balance (in stroops) for a merchant.
    pub fn get_gas_pool_balance(env: Env, merchant: Address) -> i128 {
        gas_pool_balance(&env, &merchant)
    }

    // -----------------------------------------------------------------------
    // Gas pool management
    // -----------------------------------------------------------------------

    /// Deposit XLM (in stroops) into the merchant's gas pool.
    /// The pool is used to pay keeper tips on each billing execution.
    ///
    /// # Events
    /// Emits `GasPoolDeposited`.
    pub fn deposit_gas_pool(
        env: Env,
        merchant: Address,
        amount: i128,
    ) -> Result<(), FluxError> {
        if amount <= 0 {
            return Err(FluxError::InvalidAmount);
        }
        merchant.require_auth();

        let current = gas_pool_balance(&env, &merchant);
        set_gas_pool_balance(&env, &merchant, current + amount);

        env.events().publish(
            (Symbol::new(&env, EVENT_GAS_POOL_DEPOSITED),),
            (merchant, amount),
        );

        Ok(())
    }

    /// Withdraw XLM from the merchant's gas pool.
    ///
    /// # Errors
    /// - `FluxError::GasPoolEmpty`  — withdrawal amount exceeds pool balance.
    /// - `FluxError::Unauthorised`  — caller did not supply merchant auth.
    ///
    /// # Events
    /// Emits `GasPoolWithdrawn`.
    pub fn withdraw_gas_pool(
        env: Env,
        merchant: Address,
        amount: i128,
    ) -> Result<(), FluxError> {
        merchant.require_auth();

        let current = gas_pool_balance(&env, &merchant);
        if amount > current {
            return Err(FluxError::GasPoolEmpty);
        }

        set_gas_pool_balance(&env, &merchant, current - amount);

        env.events().publish(
            (Symbol::new(&env, EVENT_GAS_POOL_WITHDRAWN),),
            (merchant, amount),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Stubs — remaining functions are tracked in GitHub issues
    // -----------------------------------------------------------------------

    // execute_billing  — issue #6  (happy path) + issue #7 (insufficient funds)
    // update_allowance_amount — issue #39
    // update_keeper_tip       — issue #40
    // get_contract_config     — issue #41
    // get_version             — issue #42
    // get_billing_history     — issue #43
    // get_due_allowances      — issue #47
    // upgrade                 — issue #48
    // transfer_admin          — issue #49
}