---
name: datafusion-rule
description: Write, test, or debug the R1–R4 register SQL rules in rules/*.sql against pass/fail RecordBatch fixtures in DataFusion. Use when adding or changing a rule, its fixtures, or when a rule returns unexpected rows.
---

# datafusion-rule

## When to use

Call this skill when you need to write, test, or debug the 4 SQL register rules (R1–R4) in `rules/*.sql` and their test fixtures. Use it for:
- Writing a new SQL rule from a spec
- Testing a rule against pass + fail fixtures
- Debugging why a rule returns unexpected rows (field names, data types, JOIN logic)
- Validating rule coverage (does it catch the cases in the threat model?)
- Optimizing rule performance (column ordering, index hints)

Do NOT use this for general Rust unit tests—use standard `cargo test` for those.

## Step-by-step instructions

### 1. Understand the data model
Each rule queries one or more Arrow RecordBatches registered as DataFusion tables:
- **mints** (mint, slot, signature, amount, supply_after, authority)
- **burns** (mint, slot, signature, amount, supply_after, authority, via_permanent_delegate)
- **transfers** (mint, slot, signature, from_owner, to_owner, amount)
- **balances** (mint, owner, amount, slot)
- **authority_changes** (mint, slot, signature, change_kind, old_value, new_value)
- **reserves** (mint, as_of_ts, reserve_amount, attestor, sig, source_url)
- **allowlist** (mint, owner)

Column types: mint/owner/sig are UTF8 strings, amounts are u128 or i128, slot is u64, sig is [u8; 32] (stored as hex string or binary).

### 2. Read the rule spec
For each rule (R1–R4), read the PRD section 4 and the comments in `rules/R<N>.sql`:
- What condition triggers a violation?
- Which tables are involved?
- What's the JOIN key?
- Are there calculations (e.g., percentage for R3)?

Example: **R1 unbacked_mint** → `supply_after > reserve_amount` after JOINing mints and latest_reserve.

### 3. Write the SQL rule file
Create `rules/R<N>.sql`:
```sql
-- R<N>: <description>
-- Violation: <condition that triggers alert>
SELECT m.signature, m.slot, m.supply_after, r.reserve_amount
FROM mints m
JOIN latest_reserve r ON m.mint = r.mint
WHERE m.supply_after > r.reserve_amount;
```

Key constraints:
- No semicolons within the query (DataFusion parsing issue)
- Use standard SQL syntax (FROM, JOIN, WHERE, GROUP BY)
- Column aliases must match the expected output (signature, slot, etc. for proof JSON)
- Aggregations (COUNT, MAX, SUM) are allowed but test for correctness

### 4. Create test fixtures
In `fixtures/`, create two fixtures per rule:

**Pass fixture** (rule returns 0 rows, data is compliant):
```json
{
  "mints": [
    {"mint": "rUSD", "slot": 100, "signature": "sig1", "amount": 100, "supply_after": 1000, "authority": "issuer"}
  ],
  "reserves": [
    {"mint": "rUSD", "as_of_ts": 1234567890, "reserve_amount": 1500, "attestor": "circle", "sig": "valid_sig", "source_url": "https://circle.com"}
  ]
}
```

**Fail fixture** (rule returns exact expected rows, data violates the rule):
```json
{
  "mints": [
    {"mint": "rUSD", "slot": 100, "signature": "sig1", "amount": 100, "supply_after": 2000, "authority": "issuer"}
  ],
  "reserves": [
    {"mint": "rUSD", "as_of_ts": 1234567890, "reserve_amount": 1500, "attestor": "circle", "sig": "valid_sig", "source_url": "https://circle.com"}
  ]
}
```

For fail fixtures, explicitly list the rows you expect the rule to return.

### 5. Implement the test in `crates/rc-engine/src/lib.rs`
```rust
#[test]
fn test_rule_r<N>_pass() {
    let fixture = load_fixture("fixtures/r<N>_pass.json");
    let batches = build_record_batches_from_fixture(&fixture);
    let violations = run_rule_r<N>(&batches);
    assert_eq!(violations.len(), 0, "Pass fixture should return no violations");
}

#[test]
fn test_rule_r<N>_fail() {
    let fixture = load_fixture("fixtures/r<N>_fail.json");
    let batches = build_record_batches_from_fixture(&fixture);
    let violations = run_rule_r<N>(&batches);
    assert_eq!(violations.len(), 1, "Fail fixture should return exactly 1 violation");
    assert_eq!(violations[0].signature, "sig1");
    // Add more assertions on the violation fields
}
```

### 6. Run the rule test
```bash
cargo test -p rc-engine --lib test_rule_r<N>
```

If the test fails:
- Check the error message for SQL parsing errors (column names, type mismatches)
- Verify fixture data types match the Arrow schema
- Use `println!("violations: {:?}", violations)` to debug output
- Check DataFusion documentation for your pinned version's SQL syntax

### 7. Add edge-case tests
Create additional test cases for:
- Empty input tables (should return 0 rows)
- Large amounts (u128::MAX, overflow in calculations)
- NULL values (if applicable; DataFusion handles NULLs in JOINs)
- Multiple violations in one rule (same mint multiple times)
- Decimals (if amount is fractional; ensure SQL handles it)
- Out-of-order slots (rule should still work correctly)

Example for R1 with large amounts:
```rust
#[test]
fn test_rule_r1_large_amounts() {
    // Mint with supply_after = u128::MAX, reserve < u128::MAX
    // Should trigger R1
}
```

### 8. Verify rule coverage against threat model
Cross-check each rule against the threat model in CLAUDE.md:
- **R1**: Catches unbacked mints (honest mistake or CPI inner mint)
- **R2**: Catches non-allowlisted holders (transfer to sanctioned address)
- **R3**: Catches concentration (large holder risk)
- **R4**: Catches authority/hook changes (detection, not prevention)

If a threat is not covered, add it to the next rule or document as out-of-scope.

## Common patterns

**Loading fixtures and building batches:**
```rust
fn build_record_batches_from_fixture(fixture: &serde_json::Value) -> HashMap<String, RecordBatch> {
    let mut batches = HashMap::new();
    if let Some(mints_data) = fixture["mints"].as_array() {
        batches.insert("mints".to_string(), build_mint_batch(mints_data));
    }
    // repeat for other tables
    batches
}
```

**Running a rule:**
```rust
fn run_rule_r1(batches: &HashMap<String, RecordBatch>) -> Vec<Violation> {
    let ctx = SessionContext::new();
    for (name, batch) in batches {
        ctx.register_batch(name, batch.clone()).unwrap();
    }
    let sql = std::fs::read_to_string("rules/R1.sql").unwrap();
    let results = ctx.sql(&sql).await.unwrap();
    // convert results to Vec<Violation>
}
```

**Canonical JSON for deterministic hashing:**
```rust
let canonical = serde_json::to_value(&violation).unwrap();
let canonical_json = serde_json::to_string(&canonical).unwrap();
let hash = sha2::Sha256::digest(canonical_json.as_bytes());
```

## Debugging tips

- Use `cargo test -p rc-engine --lib -- --nocapture test_rule_r<N>` to see println! output
- Print the SQL query before execution: `println!("SQL: {}", sql)`
- Inspect raw RecordBatch with `println!("{:?}", batch)` (careful with large batches)
- Check fixture JSON is valid: `cat fixtures/r<N>_fail.json | jq .`
- If DataFusion errors, check the pinned version in Cargo.toml and read docs.rs for that version

## Success criteria

- All rule tests pass: `cargo test -p rc-engine --lib rule`
- Pass fixture returns 0 rows; fail fixture returns expected violations
- Edge cases are tested (empty, large amounts, multiple rows, decimals)
- Each rule is documented in `rules/R<N>.sql` with the violation condition
- SQL is deterministic (same input → same output every time)
