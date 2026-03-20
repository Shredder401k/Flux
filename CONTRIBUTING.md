# Contributing to Flux

Thank you for your interest in contributing. Flux is an open protocol — contributions from the Stellar community are what make it stronger. This document covers everything you need to get involved, from filing your first issue to submitting a production-ready pull request.

---

## Table of contents

- [Code of conduct](#code-of-conduct)
- [Ways to contribute](#ways-to-contribute)
- [Getting started](#getting-started)
- [Project structure](#project-structure)
- [Development workflow](#development-workflow)
- [Writing Soroban contracts](#writing-soroban-contracts)
- [Writing the SDK](#writing-the-sdk)
- [Testing](#testing)
- [Pull request process](#pull-request-process)
- [Issue guidelines](#issue-guidelines)
- [Security vulnerabilities](#security-vulnerabilities)
- [Community](#community)

---

## Code of conduct

Flux follows the [Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) Code of Conduct. By participating you agree to uphold it. Instances of unacceptable behaviour can be reported to the maintainers via the email in the repository's security policy.

---

## Ways to contribute

You do not need to write code to contribute meaningfully to Flux.

**Code**
- Implement features from the roadmap
- Fix bugs raised in open issues
- Improve test coverage
- Optimise Soroban resource usage (compute units, ledger entries)

**Documentation**
- Improve or clarify the README, this file, or inline code comments
- Write tutorials or integration guides for Flux.js
- Translate documentation

**Research**
- Propose and model keeper incentive mechanisms
- Audit the threat model and suggest improvements
- Benchmark path payment performance across asset pairs

**Community**
- Answer questions in issues and discussions
- Review open pull requests
- Report bugs with clear reproduction steps

---

## Getting started

### Prerequisites

| Tool | Version | Purpose |
|---|---|---|
| Rust | `>=1.74` | Soroban contract development |
| `soroban-cli` | latest | Contract build, deploy, invoke |
| Node.js | `>=18` | Flux.js SDK development |
| pnpm | `>=8` | SDK package management |
| Docker | any | Local Stellar testnet via `stellar-quickstart` |

### Install Rust and Soroban toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
cargo install --locked soroban-cli
```

### Clone and build

```bash
git clone https://github.com/your-org/flux
cd flux
cargo build
```

### Start a local testnet

```bash
docker run --rm -it \
  -p 8000:8000 \
  stellar/quickstart:latest \
  --testnet \
  --enable-soroban-rpc
```

### Run the full test suite

```bash
cargo test
```

---

## Project structure

```
flux/
├── contracts/
│   └── flux/
│       ├── src/
│       │   ├── lib.rs            # Contract entry point
│       │   ├── allowance.rs      # AllowanceRecord type + storage
│       │   ├── billing.rs        # BillingExecutor logic
│       │   ├── events.rs         # Event definitions
│       │   ├── errors.rs         # FluxError enum
│       │   └── types.rs          # Shared types
│       ├── Cargo.toml
│       └── tests/
│           ├── create_allowance.rs
│           ├── execute_billing.rs
│           ├── revoke_allowance.rs
│           └── pause_resume.rs
├── keeper/
│   ├── src/
│   │   ├── main.rs               # Keeper bot entry point
│   │   ├── scanner.rs            # On-chain event poller
│   │   ├── queue.rs              # Priority queue of due subscriptions
│   │   └── executor.rs           # Transaction submission
│   ├── Cargo.toml
│   └── .env.example
├── sdk/
│   ├── src/
│   │   ├── client.ts             # FluxClient (subscriber-facing)
│   │   ├── merchant.ts           # FluxMerchant (merchant-facing)
│   │   ├── types.ts              # Shared TypeScript types
│   │   └── utils.ts              # Asset formatting, ledger helpers
│   ├── package.json
│   └── tsconfig.json
├── docs/
├── CONTRIBUTING.md
├── README.md
└── LICENSE
```

---

## Development workflow

Flux uses a standard fork-and-branch workflow.

### 1. Fork the repository

Click **Fork** on GitHub, then clone your fork:

```bash
git clone https://github.com/YOUR_USERNAME/flux
cd flux
git remote add upstream https://github.com/your-org/flux
```

### 2. Create a branch

Branch names should follow this convention:

```
feat/short-description        # new feature
fix/short-description         # bug fix
docs/short-description        # documentation only
refactor/short-description    # code change with no behaviour change
test/short-description        # adding or improving tests
```

```bash
git checkout -b feat/pause-allowance-ledger-validation
```

### 3. Make your changes

Keep commits atomic and focused. Each commit should represent one logical change. Write commit messages in the imperative mood:

```
# Good
Add ledger validation to pause_allowance

# Bad
fixed stuff
updated pause function and also changed some types and fixed a test
```

### 4. Sync with upstream regularly

```bash
git fetch upstream
git rebase upstream/main
```

### 5. Open a pull request

Push your branch and open a PR against `main`. Fill in the PR template completely. Link the issue your PR resolves using `Closes #123` in the description.

---

## Writing Soroban contracts

### Style conventions

- Use `snake_case` for all function and variable names
- Use `PascalCase` for all types and enums
- Keep public contract functions thin — delegate logic to internal modules (`allowance.rs`, `billing.rs`)
- Every public function must have a doc comment explaining its purpose, panics, and events emitted
- Use `soroban_sdk::panic_with_error!` with a typed `FluxError` — never use bare `panic!`

### Resource awareness

Soroban charges for compute units and ledger entry reads/writes. When adding contract logic, be mindful of:

- Minimising the number of ledger entries read per invocation
- Avoiding unbounded loops over contract storage
- Caching repeated storage reads in local variables within a function

Run `soroban contract build` with `--profile release` and inspect the WASM size. Aim to keep the compiled contract under 100KB.

### Error handling

All errors must be variants of `FluxError`:

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FluxError {
    AllowanceNotFound     = 1,
    AllowanceRevoked      = 2,
    AllowanceNotDue       = 3,
    InsufficientBalance   = 4,
    ExceededMaxCycles     = 5,
    AlreadyExecuted       = 6,
    Unauthorised          = 7,
    GasPoolEmpty          = 8,
    AllowancePaused       = 9,
}
```

Never add a new error without a corresponding test that verifies the error is returned in the correct scenario.

### Events

Every state-changing function must emit a typed event. Define events in `events.rs` and document the topic and data fields. Event schemas are considered part of the public API — once on mainnet, they are immutable.

---

## Writing the SDK

The Flux.js SDK lives in `/sdk` and is written in TypeScript with strict mode enabled.

### Style conventions

- Use `camelCase` for functions and variables, `PascalCase` for classes and types
- All public functions and types must have JSDoc comments
- Prefer explicit return types over inference on public API surfaces
- No `any` — use `unknown` and narrow explicitly

### Adding a new SDK method

1. Define the input and output types in `types.ts`
2. Implement the method in the appropriate class (`FluxClient` for subscriber-facing, `FluxMerchant` for merchant-facing)
3. Write a unit test covering the happy path and at least one error case
4. Update the README SDK section if the method is user-facing

### Building the SDK

```bash
cd sdk
pnpm install
pnpm build
pnpm test
```

---

## Testing

### Contract tests

Soroban contract tests live in `contracts/flux/tests/`. Each public function has its own test file. Tests use the Soroban test environment (`soroban_sdk::testutils`) — no deployed contract or network connection is needed.

Every pull request that touches contract logic must include tests for:

- The happy path
- All relevant `FluxError` variants
- Edge cases (zero amounts, max cycle boundary, ledger boundary conditions)

Run contract tests:

```bash
cargo test -p flux
```

### SDK tests

SDK tests use [Vitest](https://vitest.dev/). Mocked Soroban RPC responses live in `sdk/src/__mocks__/`.

```bash
cd sdk
pnpm test
```

### Integration tests

Integration tests deploy the contract to a local testnet and exercise the full flow end-to-end, including the keeper bot. They require Docker running.

```bash
cargo test --test integration
```

Integration tests are not required for documentation-only PRs but are strongly encouraged for any contract or keeper changes.

---

## Pull request process

1. Ensure `cargo test` and `pnpm test` both pass locally before opening a PR.
2. Fill in the PR template. Incomplete PRs will be marked `needs-info` and may be closed if unresponsive for 14 days.
3. All PRs require at least one approving review from a maintainer before merging.
4. PRs that touch contract logic require two approving reviews.
5. Maintainers may request changes. Address feedback with new commits — do not force-push during review as it makes re-review harder.
6. Once approved, a maintainer will squash-merge your PR into `main`.

### PR checklist

Before marking your PR as ready for review, confirm:

- [ ] Code compiles without warnings (`cargo build` / `pnpm build`)
- [ ] All existing tests pass
- [ ] New tests added for new behaviour
- [ ] Doc comments updated for any changed public API
- [ ] `CONTRIBUTING.md` or `README.md` updated if the change affects developer workflow
- [ ] PR description links the issue it resolves

---

*Built on [Stellar](https://stellar.org) and [Soroban](https://soroban.stellar.org).*
