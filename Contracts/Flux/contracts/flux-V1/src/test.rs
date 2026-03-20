#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

use crate::{errors::FluxError, types::AllowanceState, FluxContract, FluxContractClient};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, FluxContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, FluxContract);
    let client = FluxContractClient::new(&env, &contract_id);

    // Initialize with 17_280-ledger retry window and 10 XLM max tip
    client
        .initialize(
            &Address::generate(&env),
            &17_280_u32,
            &100_000_000_i128,
        )
        .unwrap();

    (env, client)
}

fn dummy_asset(env: &Env) -> Address {
    Address::generate(env)
}

// ---------------------------------------------------------------------------
// create_allowance
// ---------------------------------------------------------------------------

#[test]
fn test_create_allowance_stores_record() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);
    let merchant = Address::generate(&env);
    let asset = dummy_asset(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &merchant,
            &asset,
            &12_0000000_i128,  // $12.00 USDC
            &2_592_000_u32,    // ~30 days
            &Some(12_u32),     // 12 cycles
            &50_000_i128,      // 0.005 XLM tip
        )
        .unwrap();

    let record = client.get_allowance(&id).unwrap();

    assert_eq!(record.subscriber, subscriber);
    assert_eq!(record.merchant, merchant);
    assert_eq!(record.max_amount, 12_0000000);
    assert_eq!(record.interval_ledgers, 2_592_000);
    assert_eq!(record.max_cycles, Some(12));
    assert_eq!(record.cycles_completed, 0);
    assert_eq!(record.nonce, 0);
    assert!(matches!(record.state, AllowanceState::Active));
}

#[test]
fn test_create_allowance_sets_next_billing_ledger() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Advance to ledger 1000 before creating the allowance
    env.ledger().with_mut(|l| l.sequence_number = 1000);

    let id = client
        .create_allowance(
            &subscriber,
            &merchant,
            &dummy_asset(&env),
            &10_0000000_i128,
            &500_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    let record = client.get_allowance(&id).unwrap();
    // next_billing_ledger = 1000 (creation) + 500 (interval) = 1500
    assert_eq!(record.next_billing_ledger, 1500);
}

#[test]
fn test_create_allowance_rejects_zero_amount() {
    let (env, client) = setup();

    let err = client
        .create_allowance(
            &Address::generate(&env),
            &Address::generate(&env),
            &dummy_asset(&env),
            &0_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap_err();

    assert_eq!(err, FluxError::InvalidAmount.into());
}

#[test]
fn test_create_allowance_rejects_zero_interval() {
    let (env, client) = setup();

    let err = client
        .create_allowance(
            &Address::generate(&env),
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &0_u32,
            &None,
            &0_i128,
        )
        .unwrap_err();

    assert_eq!(err, FluxError::InvalidInterval.into());
}

#[test]
fn test_create_allowance_unlimited_cycles() {
    let (env, client) = setup();

    let id = client
        .create_allowance(
            &Address::generate(&env),
            &Address::generate(&env),
            &dummy_asset(&env),
            &5_0000000_i128,
            &86_400_u32,
            &None,        // unlimited
            &0_i128,
        )
        .unwrap();

    let record = client.get_allowance(&id).unwrap();
    assert_eq!(record.max_cycles, None);
}

// ---------------------------------------------------------------------------
// revoke_allowance
// ---------------------------------------------------------------------------

#[test]
fn test_revoke_sets_state_to_revoked() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    client.revoke_allowance(&id, &subscriber).unwrap();

    let record = client.get_allowance(&id).unwrap();
    assert!(matches!(record.state, AllowanceState::Revoked));
}

#[test]
fn test_revoke_twice_returns_error() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    client.revoke_allowance(&id, &subscriber).unwrap();

    let err = client.revoke_allowance(&id, &subscriber).unwrap_err();
    assert_eq!(err, FluxError::AllowanceRevoked.into());
}

#[test]
fn test_non_subscriber_cannot_revoke() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);
    let attacker = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    let err = client.revoke_allowance(&id, &attacker).unwrap_err();
    assert_eq!(err, FluxError::Unauthorised.into());
}

// ---------------------------------------------------------------------------
// pause_allowance / resume_allowance
// ---------------------------------------------------------------------------

#[test]
fn test_pause_and_resume() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    // Pause with no auto-resume
    client
        .pause_allowance(&id, &subscriber, &None)
        .unwrap();

    let record = client.get_allowance(&id).unwrap();
    assert!(matches!(
        record.state,
        AllowanceState::Paused { resume_ledger: None }
    ));

    // Resume manually
    client.resume_allowance(&id, &subscriber).unwrap();

    let record = client.get_allowance(&id).unwrap();
    assert!(matches!(record.state, AllowanceState::Active));
}

#[test]
fn test_pause_with_auto_resume_ledger() {
    let (env, client) = setup();
    let subscriber = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &100_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    client
        .pause_allowance(&id, &subscriber, &Some(5000_u32))
        .unwrap();

    let record = client.get_allowance(&id).unwrap();
    assert!(matches!(
        record.state,
        AllowanceState::Paused {
            resume_ledger: Some(5000)
        }
    ));
}

// ---------------------------------------------------------------------------
// is_billing_due
// ---------------------------------------------------------------------------

#[test]
fn test_is_billing_due_before_interval() {
    let (env, client) = setup();
    env.ledger().with_mut(|l| l.sequence_number = 100);

    let id = client
        .create_allowance(
            &Address::generate(&env),
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &500_u32,       // next_billing_ledger = 600
            &None,
            &0_i128,
        )
        .unwrap();

    // Still at ledger 100 — not due
    assert!(!client.is_billing_due(&id));
}

#[test]
fn test_is_billing_due_after_interval() {
    let (env, client) = setup();
    env.ledger().with_mut(|l| l.sequence_number = 100);

    let id = client
        .create_allowance(
            &Address::generate(&env),
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &500_u32,       // next_billing_ledger = 600
            &None,
            &0_i128,
        )
        .unwrap();

    // Advance past the billing ledger
    env.ledger().with_mut(|l| l.sequence_number = 601);
    assert!(client.is_billing_due(&id));
}

#[test]
fn test_is_billing_due_returns_false_for_revoked() {
    let (env, client) = setup();
    env.ledger().with_mut(|l| l.sequence_number = 100);
    let subscriber = Address::generate(&env);

    let id = client
        .create_allowance(
            &subscriber,
            &Address::generate(&env),
            &dummy_asset(&env),
            &10_0000000_i128,
            &50_u32,
            &None,
            &0_i128,
        )
        .unwrap();

    client.revoke_allowance(&id, &subscriber).unwrap();

    // Advance past the billing ledger — still false because revoked
    env.ledger().with_mut(|l| l.sequence_number = 200);
    assert!(!client.is_billing_due(&id));
}

// ---------------------------------------------------------------------------
// Gas pool
// ---------------------------------------------------------------------------

#[test]
fn test_gas_pool_deposit_and_balance() {
    let (env, client) = setup();
    let merchant = Address::generate(&env);

    assert_eq!(client.get_gas_pool_balance(&merchant), 0);

    client.deposit_gas_pool(&merchant, &50_000_000_i128).unwrap();
    assert_eq!(client.get_gas_pool_balance(&merchant), 50_000_000);

    client.deposit_gas_pool(&merchant, &25_000_000_i128).unwrap();
    assert_eq!(client.get_gas_pool_balance(&merchant), 75_000_000);
}

#[test]
fn test_gas_pool_withdrawal() {
    let (env, client) = setup();
    let merchant = Address::generate(&env);

    client.deposit_gas_pool(&merchant, &100_000_000_i128).unwrap();
    client.withdraw_gas_pool(&merchant, &40_000_000_i128).unwrap();

    assert_eq!(client.get_gas_pool_balance(&merchant), 60_000_000);
}

#[test]
fn test_gas_pool_overdraft_rejected() {
    let (env, client) = setup();
    let merchant = Address::generate(&env);

    client.deposit_gas_pool(&merchant, &10_000_000_i128).unwrap();

    let err = client
        .withdraw_gas_pool(&merchant, &20_000_000_i128)
        .unwrap_err();

    assert_eq!(err, FluxError::GasPoolEmpty.into());
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

#[test]
fn test_double_initialize_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, FluxContract);
    let client = FluxContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &17_280_u32, &100_000_000_i128).unwrap();

    let err = client
        .initialize(&admin, &17_280_u32, &100_000_000_i128)
        .unwrap_err();

    assert_eq!(err, FluxError::AlreadyInitialized.into());
}