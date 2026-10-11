---
name: anchor-test
description: Write, debug, or verify Anchor tests for programs/rc-attest (Config/Rule/Result PDA init, authority checks, account sizing, instruction validation, failure cases). Use when touching the on-chain attestation program; not for plain Rust crate tests.
---

# anchor-test

## When to use

Call this skill when you need to write, debug, or verify Anchor program unit tests for `programs/rc-attest/`. Use it for:
- Account initialization tests (Config, Rule, Result PDAs)
- Authority validation (only authority can post results)
- Account size checks (account space is sufficient for all fields)
- CPI / cross-program invocation tests (e.g., rc-attest called from rc-cli)
- Instruction parameter validation (slot, result_hash bounds)
- Failure cases (malformed accounts, unauthorized signers)

Do NOT use this for general Rust unit tests—use standard `cargo test` for those.

## Step-by-step instructions

### 1. Understand the Anchor program structure
Read `programs/rc-attest/src/lib.rs` and `programs/rc-attest/src/state.rs` to map out:
- Anchor modules and instruction handlers
- Account PDAs and their derivation paths
- Constraints (mut, signer, authority checks)
- Account space requirements (use `#[account(init, space = ...)]`)

### 2. Write the test skeleton
In `programs/rc-attest/tests/integration_tests.rs`, create a new test function:
```rust
#[tokio::test]
async fn test_<instruction>_<scenario>() {
    let mut context = /* setup program context */;
    // Arrange: create accounts, set state
    // Act: invoke instruction
    // Assert: check account state or expect error
}
```

### 3. Set up the Anchor test context
Use `anchor_client::Client` (if available) or the Solana test utilities:
- Create a keypair for the authority (payer)
- Initialize a `ProgramTest` with your program
- Use `process_transaction()` to send instructions
- Extract returned account data with `get_account()`

### 4. Test account initialization
For `init_config`, verify:
- Config PDA is created at the correct address
- Config.authority is set to the payer
- Config account space is sufficient (should be `8 + std::mem::size_of::<Config>()`)
- Only the authority can invoke init_config (negative test: non-authority invocation fails)

### 5. Test authority-gated instructions
For `register_rule` and `post_result`:
- Verify the instruction fails if invoked by a non-authority signer
- Verify the instruction succeeds if authority signs
- Check that accounts are created/modified correctly
- Test boundary conditions (max rule count, oversized result_hash, etc.)

### 6. Run the test
```bash
cargo test -p rc-attest --test integration_tests
```

If the test fails:
- Check Anchor error codes in the program output
- Verify account constraints (rent-exempt, space, signer)
- Ensure PDAs are derived with the correct seeds
- Check that the authority keypair is passed correctly

### 7. Add edge-case tests
Create tests for:
- Duplicate rule registration (same rule ID registered twice)
- Posting a result before the rule is registered
- Result_hash is 32 bytes (sha256); test with wrong length
- Slot overflow (if slot is u64, test u64::MAX)
- Multiple authority key rotations (if supported)

### 8. Verify test isolation
Each test should:
- Use its own Config PDA (different seed if needed)
- Not depend on state from other tests
- Clean up any created accounts
- Be deterministic (no randomness in test data)

## Common patterns

**Generating a valid result_hash:**
```rust
let result_hash: [u8; 32] = sha2::Sha256::digest(&canonical_json)
    .as_slice()
    .try_into()
    .unwrap();
```

**Creating a PDA for tests:**
```rust
let (config_pda, _bump) = Pubkey::find_program_address(
    &[b"config", authority.as_ref()],
    &program_id,
);
```

**Asserting an instruction fails:**
```rust
let result = send_instruction(&mut context, instruction).await;
assert!(result.is_err());
assert_eq!(result.unwrap_err().code, AnchorError::ConstraintViolated);
```

## Debugging tips

- Use `solana-cli` to inspect account state on devnet/testnet if needed
- Print Anchor error messages with `println!("Error: {:?}", error.error_code)`
- Check that the Anchor.toml `cluster` is set correctly (should match your test environment)
- If account space is too small, the transaction will fail with `InsufficientFunds` or `AccountDataTooSmall`

## Success criteria

- All anchor tests pass: `cargo test -p rc-attest --test '*'`
- Account sizes are correct (no wasted space, all fields fit)
- Authority checks are enforced
- Edge cases (duplicates, overflow, malformed) are handled
