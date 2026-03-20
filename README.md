# Flux

**Decentralized subscription billing protocol on Stellar.**

Flux enables non-custodial recurring payments — pull-based, permissionless, and fully on-chain. Users authorize a billing schedule once. Merchants collect reliably. No custodian holds funds between cycles. Built on Soroban smart contracts with native Stellar path payments.



---

## Table of contents

- [Why Flux](#why-flux)
- [How it works](#how-it-works)
- [Architecture](#architecture)
- [Contract interface](#contract-interface)
- [Keeper network](#keeper-network)
- [SDK — Flux.js](#sdk--fluxjs)
- [Fee structure](#fee-structure)
- [Security model](#security-model)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## Why Flux

Web2 subscription billing is centralised by design. Stripe, Braintree, and Recurly act as trusted intermediaries who hold authorisation tokens, execute debits on behalf of merchants, and control the cancellation flow. Users cannot truly self-custody because the payment rail requires a custodian.

On-chain attempts to replicate subscriptions have historically failed for one of two reasons:

1. **Token approvals** — ERC-20 `approve()` gives unlimited or time-unlimited access to a wallet. It is a security hole, not a billing primitive.
2. **Cron-based execution** — centralised servers call the contract on schedule, reintroducing a trusted operator.

Flux solves both. A structured `AllowanceRecord` encodes exactly what a merchant may debit, when, how often, and for how many cycles. A permissionless keeper network handles execution. Stellar's native path payments handle multi-asset conversion atomically. No custodian. No cron. No unlimited approvals.

---

## How it works

### 1. Subscriber signs an allowance

At checkout, the subscriber signs a single transaction that calls `create_allowance()` on the Flux contract. This stores an `AllowanceRecord` on Soroban — not a token transfer, not an approval — a structured permission record the contract enforces on every billing attempt.

```
merchant:       GBTZ…3MWP
max_amount:     12_0000000   (12.00 USDC, 7-decimal Stellar precision)
interval:       2_592_000    (ledgers, ~30 days)
max_cycles:     12
keeper_tip:     50_000       (0.005 XLM per execution)
```

The subscriber's funds remain in their own wallet until the exact moment of billing.

### 2. Keeper detects a due subscription

Any network participant running a keeper bot monitors on-chain `AllowanceRecord` entries. When `current_ledger >= record.next_billing_ledger`, the keeper calls `execute_billing()`.

### 3. Contract validates and executes

The Flux contract checks:

- Allowance is active and not cancelled
- Billing interval has elapsed (prevents early execution)
- Subscriber balance covers the debit
- Cycle count is within `max_cycles`

On success, it executes a Stellar path payment — debiting the subscriber in their held asset, converting via Stellar DEX if needed, and crediting the merchant in their preferred asset. Atomically. In one ledger.

### 4. Keeper receives tip

The executing keeper receives `keeper_tip` XLM from the merchant-funded gas pool, paid atomically in the same transaction. No separate claim step.

### 5. Subscriber cancels instantly

`revoke_allowance()` terminates the subscription immediately on ledger close. The contract rejects all future keeper calls for that record. No retention flows. No grace-period tricks. The code is the policy.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Flux Protocol                     │
│                                                         │
│  ┌──────────────────┐      ┌──────────────────────────┐ │
│  │  PolicyContract  │      │     ReservePool          │ │
│  │                  │      │                          │ │
│  │  AllowanceRecord │      │  Merchant gas pool       │ │
│  │  Nonce tracking  │      │  Keeper tip escrow       │ │
│  │  State machine   │      │  Optional refund logic   │ │
│  └────────┬─────────┘      └──────────────────────────┘ │
│           │                                             │
│  ┌────────▼──────────────────────────────────────────┐  │
│  │              BillingExecutor                      │  │
│  │                                                   │  │
│  │  Validates interval · Checks balance              │  │
│  │  Calls Stellar path_payment_strict_send           │  │
│  │  Increments nonce · Pays keeper tip               │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
         ▲                              ▲
         │ execute_billing()            │ path payment settlement
   Keeper network                  Stellar DEX + Anchors
```

### Core components

**`PolicyContract`** — The primary Soroban contract. Stores all `AllowanceRecord` entries in contract storage. Exposes the four user-facing functions. Enforces all business logic. Emits events for every state transition.

**`BillingExecutor`** — Internal module called by `execute_billing()`. Handles the path payment construction, nonce management, and keeper tip disbursement. Isolated to simplify auditing.

**`ReservePool`** — Merchant-funded pool that covers keeper tips. Merchants deposit XLM on merchant registration. The pool is separate from subscriber funds — merchants cannot access subscriber balances through it.

**Keeper bots** — Off-chain processes (reference implementation in this repo) that poll the contract for due subscriptions and submit `execute_billing()` transactions for profit.

---

## Contract interface

### `create_allowance`

Called once by the subscriber at signup. Creates a new `AllowanceRecord` in contract storage.

```rust
pub fn create_allowance(
    env: Env,
    subscriber: Address,
    merchant: Address,
    asset: Address,           // Stellar asset contract address
    max_amount: i128,         // Maximum debit per cycle (7-decimal precision)
    interval_ledgers: u32,    // Billing interval in ledgers
    max_cycles: Option<u32>,  // None = unlimited until cancelled
    keeper_tip: i128,         // XLM tip paid to executing keeper
) -> AllowanceId;
```

Requires `subscriber` auth. Emits `AllowanceCreated` event.

---

### `execute_billing`

Called by any keeper once per billing interval per allowance. Executes the pull payment.

```rust
pub fn execute_billing(
    env: Env,
    allowance_id: AllowanceId,
    keeper: Address,          // Receives the keeper tip
) -> BillingResult;
```

No auth required — permissionless. Returns `BillingResult::Success` or `BillingResult::InsufficientFunds`. Increments nonce on success. Emits `BillingExecuted` or `BillingFailed` event.

---

### `revoke_allowance`

Called by subscriber to cancel immediately.

```rust
pub fn revoke_allowance(
    env: Env,
    allowance_id: AllowanceId,
    subscriber: Address,
) -> ();
```

Requires `subscriber` auth. Sets record state to `Revoked`. All future `execute_billing()` calls for this ID will panic. Emits `AllowanceRevoked` event.

---

### `pause_allowance`

Temporary suspension without cancellation.

```rust
pub fn pause_allowance(
    env: Env,
    allowance_id: AllowanceId,
    subscriber: Address,
    resume_ledger: Option<u32>, // None = paused indefinitely until resume_allowance()
) -> ();
```

Requires `subscriber` auth. Keeper calls within the pause window are rejected without consuming the nonce. Emits `AllowancePaused` event.

---

### `resume_allowance`

Re-activates a paused allowance. Next billing triggers on the original cadence.

```rust
pub fn resume_allowance(
    env: Env,
    allowance_id: AllowanceId,
    subscriber: Address,
) -> ();
```

---

### `get_allowance`

View function. Returns full `AllowanceRecord` for a given ID.

```rust
pub fn get_allowance(
    env: Env,
    allowance_id: AllowanceId,
) -> AllowanceRecord;
```

---

### Key types

```rust
pub struct AllowanceRecord {
    pub id: AllowanceId,
    pub subscriber: Address,
    pub merchant: Address,
    pub asset: Address,
    pub max_amount: i128,
    pub interval_ledgers: u32,
    pub max_cycles: Option<u32>,
    pub cycles_completed: u32,
    pub nonce: u64,
    pub next_billing_ledger: u32,
    pub keeper_tip: i128,
    pub state: AllowanceState,
}

pub enum AllowanceState {
    Active,
    Paused { resume_ledger: Option<u32> },
    Revoked,
    Completed,          // max_cycles reached
}

pub enum BillingResult {
    Success { amount: i128, ledger: u32 },
    InsufficientFunds { retry_window_closes: u32 },
}
```

---

## Keeper network

Keepers are permissionless off-chain bots that call `execute_billing()` on due subscriptions and earn XLM tips. The reference keeper implementation lives in `/keeper` in this repo.

### How execution is incentivised

Merchants fund a gas pool on registration. Each `execute_billing()` call atomically transfers `keeper_tip` XLM from the pool to the keeper. If the pool is empty, execution reverts — merchants must maintain their pool balance to ensure reliable billing.

### Double-execution prevention

Each `AllowanceRecord` tracks a `nonce`. `execute_billing()` stores `(allowance_id, nonce)` as a used key. A second call with the same nonce panics. The nonce increments on every successful billing cycle.

### Retry logic

On `BillingResult::InsufficientFunds`, the contract does not cancel the subscription. It sets `retry_window_closes = current_ledger + RETRY_WINDOW` (default: ~72 hours). Keepers may re-attempt within this window. On window expiry, the next call marks the subscription `Lapsed` and emits an event — merchant-side dunning logic can subscribe to this event stream.

### Running a keeper

```bash
cd keeper
cp .env.example .env
# Set STELLAR_RPC_URL, KEEPER_SECRET, MIN_TIP_XLM
cargo run --release
```

The reference keeper polls via Stellar's event streaming API, maintains a local priority queue of due subscriptions sorted by next billing ledger, and submits batched transactions to maximise tip income per gas unit.

---

## SDK — Flux.js

Flux.js is a lightweight TypeScript SDK for integrating subscription billing into any Web3 application. Drop-in equivalent to Stripe.js for the Soroban ecosystem.

### Installation

```bash
npm install @flux-protocol/sdk
```

### Create a subscription (frontend)

```typescript
import { FluxClient } from '@flux-protocol/sdk';

const flux = new FluxClient({ network: 'mainnet' });

const allowanceId = await flux.createSubscription({
  wallet,                    // Freighter / xBull / Lobstr wallet adapter
  merchant: 'GBTZ…3MWP',
  asset: 'USDC',
  amount: 12.00,
  interval: '30d',
  maxCycles: 12,
});
```

### Merchant dashboard (backend)

```typescript
import { FluxMerchant } from '@flux-protocol/sdk';

const merchant = new FluxMerchant({
  secretKey: process.env.MERCHANT_SECRET,
  network: 'mainnet',
});

// Listen for billing events
merchant.on('billing:success', (event) => {
  console.log(`Billed ${event.amount} from ${event.subscriber}`);
  await db.extend_subscription(event.subscriber);
});

merchant.on('billing:lapsed', (event) => {
  await sendDunningEmail(event.subscriber);
});
```

---

## Fee structure

Flux is a protocol, not a business. There is no protocol fee on successful billings in v1. The only costs are:

| Cost | Who pays | Amount |
|---|---|---|
| Keeper tip | Merchant (gas pool) | Configurable, default 0.005 XLM |
| Stellar base fee | Keeper (reimbursed via tip) | 0.00001 XLM |
| Soroban resource fee | Keeper (reimbursed via tip) | ~0.001 XLM estimated |
| `create_allowance` tx | Subscriber | ~0.001 XLM once |
| `revoke_allowance` tx | Subscriber | ~0.001 XLM once |

A governance-controlled protocol fee (proposed: 0.1% of billing volume) may be introduced in v2 to fund ongoing development, subject to community vote.

---

## Security model

### What Flux can do

- Debit the subscriber up to `max_amount` per billing interval
- Execute at most once per interval per `AllowanceRecord`
- Pay the keeper tip from the merchant gas pool

### What Flux cannot do

- Debit more than `max_amount` in a single cycle
- Bill more frequently than `interval_ledgers`
- Bill after `revoke_allowance()` has been called
- Access subscriber funds for any purpose other than the authorised billing
- Be paused, upgraded, or shut down by any admin key (no admin key in v1)

### Threat model

**Oracle manipulation** — Not applicable. Flux has no oracle dependency. Billing logic is purely ledger-based.

**Keeper censorship** — A single keeper refusing to execute a subscription has no material impact. Any participant can run a competing keeper. Merchant gas pools incentivise competition.

**Reentrancy** — Soroban's execution model is not susceptible to EVM-style reentrancy. State is committed atomically per contract invocation.

**Overflow** — All arithmetic uses Soroban's checked arithmetic. `i128` provides sufficient range for all asset amounts at 7-decimal precision.

**Upgradability** — v1 contracts are immutable. There is no admin key, proxy pattern, or upgrade mechanism. What is deployed is what runs.

### Audit status

- [ ] Internal review — in progress
- [ ] External audit — planned pre-mainnet
- [ ] Bug bounty — planned post-audit

---

## Roadmap

### v0.1 — Testnet alpha
- [ ] Core `PolicyContract` implementation in Rust/Soroban
- [ ] `create_allowance`, `execute_billing`, `revoke_allowance`, `pause_allowance`
- [ ] Reference keeper bot
- [ ] Testnet deployment

### v0.2 — SDK + devtools
- [ ] Flux.js SDK (TypeScript)
- [ ] Merchant event webhook relay
- [ ] Keeper monitoring dashboard
- [ ] Local testnet environment (`flux-dev`)

### v0.3 — Multi-asset + path payments
- [ ] Full Stellar path payment integration (subscriber pays any anchor asset)
- [ ] Merchant receives preferred asset
- [ ] Anchor registry for supported asset pairs

### v1.0 — Mainnet
- [ ] External security audit
- [ ] Bug bounty program
- [ ] Mainnet deployment
- [ ] Flux.js v1 stable release

### v2.0 — Governance + extensions
- [ ] Protocol fee governance (on-chain vote)
- [ ] Conditional billing (amount varies per cycle based on usage oracle)
- [ ] Native Freighter wallet UI for subscription management

---

## Contributing

Flux is open source and welcomes contributions. See [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.

To get started locally:

```bash
git clone https://github.com/your-org/flux
cd flux
cargo build
cargo test
```

Contract source lives in `/contracts/flux`. Keeper bot in `/keeper`. SDK in `/sdk`.

Issues tagged `good first issue` are a great place to start.

---

## License

MIT — see [LICENSE](./LICENSE).

---

*Built on [Stellar](https://stellar.org) and [Soroban](https://soroban.stellar.org).*
