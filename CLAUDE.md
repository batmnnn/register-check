# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project: Register Check

A Rust + DataFusion engine that watches Solana Token-2022 mints and flags supply increases with no matching reserve, plus an Anchor program that records check results on-chain. **Hackathon deadline: Tue Oct 13 12:29 PM IST (Oct 12 11:59 PM PT).**

## Stack & Constraints

- **Rust 2021**, pinned to **rustc 1.97.1** by `rust-toolchain.toml` (litesvm 0.18 and the Agave 4.3 runtime crates used by rc-attest tests need it; SBF builds use platform-tools' own compiler)
- **Workspace layout**: pure Rust logic in `crates/rc-*`, Anchor program only in `programs/rc-attest`
- **DataFusion 54** for rule evaluation (MSRV 1.88)
- **Solana client crates pinned `~4.1`** (`solana-rpc-client`, `solana-transaction-status-client-types`) and `spl-token-2022-interface = "3"`. Never take the `4.5.0-alpha` crates crates.io lists as latest: they need rustc 1.98. Two `solana-pubkey` majors coexist, so convert at the boundary and don't mix Pubkey types
- **rc-attest deployed on devnet**: `BNADNdmc5zJF54Tj6o1ctr3PmhjjDFPByPGGU3oLthV3` (upgrade authority = id.json). Program keypair backup is in `.secrets/rc_attest-program-keypair.json`; `cargo clean` deletes target/deploy, so restore it before `anchor build`. Tests use **litesvm 0.18** (0.10 from the Anchor scaffold can't load sBPFv3). IDL ships in `idl/`; on-chain IDL upload needs a JS project and is skipped
- **Anchor 1.2.1** (avm) + **Agave CLI 4.3.0** / cargo-build-sbf 4.3.0 / platform-tools v1.57 for on-chain attestation. Builds sBPFv3 by default (required once SIMD-0500 activates; local `solana-test-validator` 4.3 already enforces it)
- **Wallet**: `~/.config/solana/id.json`, set in `Anchor.toml [provider]` (the global CLI config points to a missing file, so pass `-k ~/.config/solana/id.json` to `solana` commands)
- **Disk is tight** (~15 GB free): one shared workspace `target/`, and run `cargo clean` before SBF builds if space drops
- **Token-2022** extensions: transfer-hook, permanent-delegate, mint/freeze authority
- **Apache Arrow** for columnar data (RecordBatch builders in rc-types)

## Repository Structure

```
register-check/
  Cargo.toml (workspace)  Anchor.toml
  crates/
    rc-types/       event structs, Arrow schemas, RecordBatch builders
    rc-engine/      DataFusion SessionContext, rule SQL loading, violations runner
    rc-ingest/      RPC polling, tx parsing (spl-token-2022 instruction mapping)
    rc-proof/       canonical JSON bundles, sha256 result_hash
    rc-cli/         watch loop, run-once, demo commands
  programs/
    rc-attest/      Anchor program: Config, Rule, Result accounts + instructions
  rules/            R1..R4.sql (4 register rules as SQL files)
  fixtures/         *.json test data (devnet tx, reserve attestations)
  scripts/
    devnet_setup.sh generate Token-2022 mint with extensions
    demo.sh         mint over reserve, show violations
  web/              dashboard (reads JSON proofs from rc-cli)
```

## Build & Test Commands

```bash
# Workspace tests (all crates)
cargo test --workspace

# Single crate test
cargo test -p rc-engine --lib

# Single test
cargo test -p rc-engine --lib test_rule_r1_unbacked_mint

# Build all
cargo build --workspace
anchor build                  # rc-attest program (sBPFv3)

# Format & lint
cargo fmt --all
cargo clippy --workspace --all-targets

# Demo (devnet): one-time setup, then watch / mint / attest
scripts/devnet_setup.sh && scripts/demo.sh init
scripts/demo.sh watch --attest          # proofs/ + status.json, result hashes posted on-chain
scripts/demo.sh mint 100                # -> R1; also: swap-hook, unhook, stranger <n>, reserve <n>
target/debug/rc attest proofs/R1_*.json # attest an existing proof
python3 -I scripts/verify_proof.py proofs/*.json   # independent hash check
```

## Coding Rules

**Spec first**: Before writing code, create a SPEC.md documenting files, types, function signatures, 7 table schemas as Arrow types, 4 SQL rules, and how each will be tested.

**Tests before code**: Each SQL rule has a `pass` fixture (0 violations) and a `fail` fixture (exact expected rows). Unit tests use RecordBatch fixtures, not live RPC.

**Small commits**: One feature or fix per commit. No accumulation.

**No secrets**: Never commit .env, private keys, or attestor signatures. Use mock reserves for tests.

**No unwrap()** in non-test code. Use `thiserror` for error types. Checked math for u128 products.

**Deterministic hashing**: Proof JSON is canonical (sorted keys); sha256 result_hash is reproducible.

**Commitment always "finalized"**: Proof bundles only use finalized txs (not confirmed).

Run `cargo test --workspace` and paste the summary before marking done.

## Data Model (7 Arrow RecordBatches)

- **mints** (mint, slot, signature, amount, supply_after, authority)
- **burns** (mint, slot, signature, amount, supply_after, authority, via_permanent_delegate)
- **transfers** (mint, slot, signature, from_owner, to_owner, amount)
- **balances** (mint, owner, amount, slot)
- **authority_changes** (mint, slot, signature, change_kind, old_value, new_value)
- **reserves** (mint, as_of_ts, reserve_amount, attestor, sig, source_url)
- **allowlist** (mint, owner)

## The 4 SQL Rules

Each lives in `rules/R<N>.sql` and is tested against pass + fail fixtures.

- **R1 unbacked_mint**: `supply_after > reserve_amount`
- **R2 non_allowlisted_holder**: balance > 0 for owner not in allowlist
- **R3 holder_concentration**: any holder > 20% of supply
- **R4 privileged_change**: mint/freeze authority, permanent-delegate, or transfer-hook program changed

## Key Design Decisions

1. **RPC polling over Geyser**: v0 uses finalized `getSignaturesForAddress` pagination; v1 planned for Yellowstone gRPC.
2. **Duplicate tx safety**: Dedup signatures before processing; out-of-order slots are handled by rule tests, not ingester.
3. **Reserve attestation**: Signed JSON from issuer; v1 to add Chainlink/Brevis inputs. Bad signatures rejected.
4. **Proof as evidence**: Result hash stored on-chain; anyone can re-fetch tx and re-run SQL to verify.
5. **Anchor account layout**: Minimal (no token logic); Config, Rule, Result PDAs. Authority is single key (v0) → multisig (v1).

## Edge Cases & Red-Team Scope

High-priority adversarial cases:
- CPI inner instructions (mints via delegate, not direct)
- Permanent-delegate burn vs theft
- Transfer-hook program swaps between polls
- RPC pagination gaps (>1000 sigs)
- Reorg / commitment confusion (finalized vs confirmed)
- Reserve replay / stale / spoof
- SQL injection via config (inputs to rules)
- Decimal places & u64 overflow in math

## Hackathon Timeline

- **Mon 03:20–04:30 IST**: Scaffold, types, SQL fixtures, rc-types
- **Mon 10:30–13:00 IST**: rc-ingest RPC parser
- **Mon 14:00–18:00 IST**: rc-proof + cli watch loop
- **Mon 18:00–19:00 IST**: rc-attest Anchor program
- **Mon 21:00–23:30 IST**: Dashboard (Next.js or static HTML)
- **Mon 23:30–01:00 IST**: README, diagram, threat model
- **Mon 01:00–02:00 IST**: Demo video (≤3 min)
- **Mon 02:00–03:00 IST**: Pitch video (≤3 min)
- **Tue 08:00–10:00 IST**: Final fixes, public repo, form drafts
- **Tue 10:00–11:00 IST**: Submit Colosseum + Superteam Earn

Cut order if late: dashboard → R3 → on-chain attest → webhook.

## Demo Script Outline (3 min)

0:00–0:25: Problem (monthly PDF vs per-slot supply). Dashboard with all 4 rules green.
0:25–0:50: Terminal: `demo.sh mint 100` (supply 1,100 > reserve 1,000).
0:50–1:10: R1 red; open proof JSON; click explorer link.
1:10–1:40: Show SQL of R1. "Any auditor can re-run this."
1:40–2:00: Show on-chain attestation tx (result hash).
2:00–2:20: Bonus: swap transfer-hook program → R4 red.
2:20–2:40: Close: open source, Token-2022 native, v1 roadmap (Yellowstone + mainnet PYUSD/USDG).

## Integration with Claude Code Skills

- Use `/verify` after each feature to test end-to-end.
- Use `/code-review` to audit for correctness, spec gaps, missing tests.
- Use `/run` to launch the demo script.
- Use the custom `anchor-test`, `datafusion-rule`, and `pre-submit-audit` skills (see `.claude/skills/`).

## Success Metrics

- End-to-end R1 alert in < 60s on devnet.
- All 4 rules green in tests.
- 3-min demo + 3-min pitch videos.
- Public GitHub repo with README + license.
