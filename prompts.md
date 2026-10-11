# Register Check: Cursor prompt pack
Sources for the patterns:
- Claude Code best practices (spec first, plan mode, fresh-context review, verify with tests): https://code.claude.com/docs/en/best-practices
- Cursor, "Best practices for coding with agents" (short rules, plan before code, reference existing files): https://cursor.com/blog/agent-best-practices
- Adversarial self-review via a fresh sub-agent: https://github.com/anthropics/claude-code/issues/72206 , https://github.com/voodootikigod/adversarial-review
The prompts below are my adaptation of those patterns.

## 0. Project rule (put in .cursor/rules/register-check.mdc)
```
- Rust 2021, workspace in /register-check. Pure logic in crates/*, Anchor only in programs/*.
- Never use unwrap() in non-test code. Use thiserror. Checked math (u128 for products).
- Every SQL rule lives in rules/*.sql and has a pass + fail fixture test.
- Commitment for proofs is always "finalized".
- Run `cargo test --workspace` before saying done. Paste the summary line.
- Do not add dependencies without saying why; pin versions.
```

## 1. Spec first (once)
"Read poc_plan.md and prd.md. Write SPEC.md: files to create, public types and function signatures, the 7 table schemas as Arrow types, the 4 SQL rules, out-of-scope items, and how each will be verified by a test. Do not write code yet. Ask me up to 3 questions only if they block you."

## 2. Types + fixtures
"Implement crates/rc-types per SPEC.md: event structs, Arrow schemas, and RecordBatch builders. Add fixtures/ JSON for a 1,000 reserve, 3 holders, 1 mint that takes supply to 1,100. Tests: round-trip JSON -> RecordBatch -> row count. Run the tests."

## 3. DataFusion engine
"Implement crates/rc-engine: build a SessionContext, register MemTables from rc-types batches, load rules/*.sql, run each, and return Vec<Violation{rule, rows as JSON}>. Follow the TableProvider/MemTable docs for the pinned datafusion version only; if an API is unsure, check docs.rs for that version. Tests: each rule pass + fail fixture."

## 4. Ingester
"Implement crates/rc-ingest: given a mint and an RPC URL, page getSignaturesForAddress, then getTransaction (jsonParsed, finalized, maxSupportedTransactionVersion 0). Map spl-token-2022 parsed ix types (mintTo, mintToChecked, burn, burnChecked, transfer, transferChecked, setAuthority, transfer-hook update) to rc-types events. Put the HTTP behind a trait so tests use saved JSON. Handle: duplicate sigs, failed txs (meta.err != null => skip), inner instructions (CPI mints)."

## 5. Proof + attest
"Implement crates/rc-proof: a canonical JSON proof bundle (fields in prd.md s.7) + sha256 result_hash; golden test. Then programs/rc-attest (Anchor 0.31): Config{authority}, Rule{id, sql_hash}, Result{rule, slot, result_hash, violated}. Instructions init_config, register_rule, post_result (authority only). No token logic."

## 6. Fresh-context review (run in a NEW chat)
"You did not write this code. Review the diff against SPEC.md. Report only correctness bugs, spec gaps, and missing tests, each with file:line and a concrete failing input. No style comments. If nothing is wrong, say so."

## 7. Test generation
"For rc-engine and rc-ingest, list edge cases first (empty tables, u64 max amounts, out-of-order slots, duplicate signatures, CPI inner mints, failed txs, decimals != 6, permanent-delegate burn). Then write one test per case. Each test must fail if the bug it targets is introduced: state that bug in a comment."

## 8. Red team / loophole audit (final)
"Act as an attacker who wants to mint unbacked tokens or move tokens to a sanctioned wallet WITHOUT Register Check alerting, or to make it alert falsely. Using the repo, enumerate concrete bypasses: (a) mint via CPI inner instruction or a different instruction variant we don't parse, (b) mint authority rotated to a new key then mint, (c) burn+remint in the same slot, (d) supply moved through a second mint / wrapper token, (e) confidential transfer extension, (f) permanent-delegate transfer to a non-allowlisted wallet, (g) transfer-hook program swapped, then swapped back between polls, (h) RPC returning a truncated signature list (pagination gaps, >1000 sigs), (i) reorg/commitment confusion, (j) reserve JSON replay/stale or attestor key spoof, (k) decimals/u64 overflow in SQL, (l) rule SQL injection via config, (m) clock/slot ordering, (n) dashboard showing stale green. For each: is it exploitable today (yes/no with file:line), impact, and the smallest fix + a test. Rank by severity."

## 9. Pitch check
"Read README and the demo script. As a Colosseum judge scoring functionality, impact, novelty, UX, open source, business plan: give 1-5 each with one sentence why and the single highest-leverage change in the next 2 hours."
