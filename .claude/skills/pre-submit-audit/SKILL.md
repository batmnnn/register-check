---
name: pre-submit-audit
description: Final red-team gate before Colosseum/Superteam Earn submission — enumerate detection bypasses, confirm adversarial tests from prompts.md step 8, check the devnet demo end-to-end, catch spec gaps. Use only after feature freeze.
---

# pre-submit-audit

## When to use

Call this skill as the FINAL step before Colosseum/Superteam Earn submission, after feature freeze. Use it to:
- Red-team the engine for bypasses an attacker could use to mint unbacked tokens or evade detection
- Verify all adversarial test cases from prompts.md step 8 are covered
- Check the demo script works end-to-end on devnet
- Catch missing tests, spec gaps, or false-alarm conditions before judges see the repo

Do NOT use this mid-development — it's a final gate, not a continuous check. Run `/code-review` and `cargo test --workspace` first; this skill assumes those already pass.

## Step-by-step instructions

### 1. Confirm feature freeze state
Verify:
```bash
cargo test --workspace  # must be green
cargo clippy --workspace --all-targets  # no warnings on core logic
git status  # no uncommitted changes
```
If any fail, stop and fix before auditing — don't audit a moving target.

### 2. Run the adversarial audit prompt
Use the fresh-context pattern (prompts.md step 8). Spawn a fresh subagent (no implementation context) and give it this exact framing:

> "Act as an attacker who wants to mint unbacked tokens or move tokens to a sanctioned wallet WITHOUT Register Check alerting, or to make it alert falsely. Using the repo, enumerate concrete bypasses: (a) mint via CPI inner instruction or a different instruction variant we don't parse, (b) mint authority rotated to a new key then mint, (c) burn+remint in the same slot, (d) supply moved through a second mint / wrapper token, (e) confidential transfer extension, (f) permanent-delegate transfer to a non-allowlisted wallet, (g) transfer-hook program swapped, then swapped back between polls, (h) RPC returning a truncated signature list (pagination gaps, >1000 sigs), (i) reorg/commitment confusion, (j) reserve JSON replay/stale or attestor key spoof, (k) decimals/u64 overflow in SQL, (l) rule SQL injection via config, (m) clock/slot ordering, (n) dashboard showing stale green. For each: is it exploitable today (yes/no with file:line), impact, and the smallest fix + a test. Rank by severity."

### 3. Triage findings
For each finding reported:
- **Critical** (exploitable today, breaks R1 core promise): must fix before submission
- **High** (exploitable but requires specific conditions): fix if time allows, document as known limitation otherwise
- **Medium/Low** (theoretical or v1-scope): document in README's "Known Limitations" section, do not fix now

Cross-reference against PRD section 11 (Risks and threat model) — if a finding is already listed there with a stated mitigation, verify the mitigation is actually implemented.

### 4. Fix critical findings only
For each critical finding:
- Write the smallest fix (don't refactor beyond the bug)
- Add a regression test that fails without the fix and passes with it
- Re-run `cargo test --workspace` to confirm no regressions

### 5. Document unfixed findings
For high/medium findings not fixed due to time, add a "Known Limitations" section to README.md:
```markdown
## Known Limitations (v0 hackathon scope)
- Confidential transfers: per-holder rules (R2, R3) cannot inspect confidential balances. Supply rules (R1) still work.
- RPC pagination: >1000 signatures per poll window may miss events; v1 uses Yellowstone gRPC streaming.
- [other findings...]
```
This is REQUIRED — judges value honesty about scope over false completeness. PRD section 11 already models this pattern.

### 6. Verify the E2E demo path
Run the actual demo script on devnet:
```bash
./scripts/devnet_setup.sh   # create Token-2022 mint with extensions
./scripts/demo.sh mint 100  # mint over reserve
```
Confirm within 60 seconds:
- R1 violation appears in console/webhook
- Proof JSON is written with correct signature, slot, result_hash
- Explorer link in proof JSON resolves to the actual tx
- On-chain attestation tx is confirmed (if rc-attest is deployed)

If any step fails, this is a CRITICAL blocker — fix immediately, it's the core demo.

### 7. Run the pitch/judge-scoring check
Use prompts.md step 9 framing in a fresh subagent:

> "Read README and the demo script. As a Colosseum judge scoring functionality, impact, novelty, UX, open source, business plan: give 1-5 each with one sentence why and the single highest-leverage change in the next 2 hours."

Act on the single highest-leverage suggestion if time remains.

### 8. Final submission checklist
Verify against poc_plan.md submission checklist:
- [ ] `cargo test --workspace` green
- [ ] No `unwrap()` in non-test code (grep check: `grep -rn "unwrap()" crates/*/src | grep -v test`)
- [ ] README has build steps, license (Apache-2.0), known limitations
- [ ] All 4 rules have pass + fail fixture tests
- [ ] Demo video ≤3 min recorded
- [ ] Pitch video ≤3 min recorded
- [ ] Repo is public on GitHub
- [ ] No secrets committed (`git log -p | grep -i "private_key\|secret\|api_key"`)
- [ ] Reused code disclosed (Endgame scaffold, if used)

## Common pitfalls to check

- **Stale green dashboard**: does the dashboard poll frequently enough, or could it show green after a violation due to caching?
- **SQL injection via config**: if rule SQL is loaded from a file, ensure no user input is interpolated into SQL strings directly (all rules are static files, not templated — verify this holds)
- **Decimals mismatch**: Token-2022 mints can have different decimal places; verify rule math doesn't assume a fixed decimals value
- **Finalized vs confirmed**: double check every code path producing a proof bundle filters on `commitment: finalized`, not `confirmed`

## Success criteria

- Zero critical findings remain unfixed
- All high/medium findings are documented in README
- E2E demo works live on devnet within 60s
- Submission checklist is 100% complete
- Judge-scoring check has been run and highest-leverage change addressed (if time allows)
