# Register Check: PoC plan (Mon Oct 12 03:20 IST to Tue Oct 13 12:29 IST)
SUBMIT TARGET: Tue 11:00 IST. HARD DEADLINE 12:29 IST (= Oct 12 11:59 PM PT).
Current state: /workspace/colosseum/register-check does NOT exist yet. Endgame workspace (/workspace/colosseum/endgame) exists: reuse its layout (crates/* pure Rust + programs/* thin Anchor), Cargo.lock MSRV trick (CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback), and test style. Box has Rust 1.85.1 and no rustup, so the SBF build is blocked until rustup is installed (it needs approval). DataFusion versions that support Rust 1.85 must be pinned [G: check with fallback resolver].

## The one end-to-end rule (R1)
Issuer posts a signed reserve JSON (reserve = 1,000 tokens). The demo script mints 1,100 on a devnet Token-2022 mint. The ingester sees the mint tx (finalized). DataFusion R1 returns 1 row. The proof bundle includes the tx signature + explorer link + result hash. The rc-attest program stores the hash. The dashboard turns red.

## Hour-by-hour (IST)
| When | Task | Done when |
|---|---|---|
| Mon 03:20-04:30 | Scaffold workspace; pin datafusion; rc-types + fixtures; R1-R4 SQL over MemTable fixtures; tests | `cargo test` green |
| 04:30-10:00 | SLEEP | |
| 10:00-10:30 | Register on colosseum.com (if not done) + Superteam Earn India listing check | Registered (Shubham does this) |
| 10:30-13:00 | rc-ingest: RPC poll getSignaturesForAddress(mint) -> getTransaction(jsonParsed, finalized) -> events (mintTo, burn, transfer, setAuthority, hook update) | Unit tests on saved devnet tx JSON fixtures |
| 13:00-14:00 | Lunch | |
| 14:00-16:00 | Devnet setup script (spl-token CLI): Token-2022 mint with transfer-hook/permanent-delegate fields; mint to 3 wallets; save fixtures | Real devnet mint exists |
| 16:00-18:00 | rc-proof JSON + sha256; rc-cli `watch` loop; console + webhook sink | R1 fires on live devnet |
| 18:00-19:00 | rc-attest Anchor: init_config, register_rule, post_result | `cargo test` + build-sbf if rustup available |
| 19:00-21:00 | Gym (short) / dinner | |
| 21:00-23:30 | Dashboard (Next.js or a static HTML page reading proofs JSON) | Red/green per rule |
| 23:30-01:00 | README, architecture diagram, threat model section | |
| 01:00-02:00 | FEATURE FREEZE. Record demo video (<=3 min) | mp4 |
| 02:00-03:00 | Pitch video (<=3 min) | mp4 |
| Tue 03:00-08:00 | SLEEP | |
| 08:00-10:00 | Re-watch, fix README, make repo public, fill the form drafts | |
| 10:00-11:00 | SUBMIT Colosseum + Superteam Earn | Confirmation screens |
| 11:00-12:29 | BUFFER only | |
Cut order if late: dashboard -> R3 -> on-chain attest (keep hash in JSON) -> webhook.

## Repo layout
```
register-check/
  Cargo.toml (workspace)  Anchor.toml
  crates/rc-types   (events, Arrow schemas)
  crates/rc-engine  (DataFusion ctx, rules/*.sql, runner)  <- tests on fixtures
  crates/rc-ingest  (RPC client, tx parser)
  crates/rc-proof   (bundle + hash)
  crates/rc-cli     (watch, run-once, demo)
  programs/rc-attest (Anchor)
  rules/R1..R4.sql
  fixtures/*.json
  scripts/devnet_setup.sh, demo.sh
  web/ (dashboard)
```

## Test plan
1. Engine: each rule has a pass fixture (0 rows) and a fail fixture (exact rows).
2. Ingest: parse 5 saved real devnet tx JSONs (mintTo, burn, transfer, setAuthority, permanent-delegate burn).
3. Proof: hash is deterministic (canonical JSON); a golden-file test.
4. Attest program: account sizes, authority checks (Rust unit tests; anchor tests optional).
5. E2E script: devnet mint over reserve -> violation file appears within 60s.
6. Adversarial: duplicate signatures, out-of-order slots, reserve JSON with a bad signature (must be rejected), supply decreasing via permanent delegate (R4 not R1).

## Demo script (3 min)
1. 0:00 Problem: monthly PDF vs per-slot supply (show the Circle/Paxos transparency pages).
2. 0:25 Dashboard: devnet "rUSD" Token-2022 mint, all 4 rules green, latest reserve 1,000.
3. 0:50 Terminal: `demo.sh mint 100` -> supply 1,100.
4. 1:10 Within seconds: R1 red; open the proof JSON; click the explorer link to the tx.
5. 1:40 Show the SQL of R1 (8 lines). "Any auditor can re-run this."
6. 2:00 Show the on-chain attestation tx with the result hash.
7. 2:20 Bonus: change the transfer-hook program -> R4 red.
8. 2:40 Close: open source, Token-2022 native, next: Yellowstone + mainnet PYUSD/USDG dashboards.

## Submission checklist
- [ ] Shubham registered on colosseum.com before Oct 12 11:59 PM PT (Rules s.6a) [V]
- [ ] Project in the Solana track on arena.colosseum.org
- [ ] Pitch video <= 3 min, English [V secondary: Superteam Turkiye guide]
- [ ] Demo video <= 3 min, English [V secondary]
- [ ] Public GitHub repo (batmnnn/register-check) with README, license (Apache-2.0), build steps
- [ ] Business plan text in the form
- [ ] Superteam Earn India track: submit the same project there too; India-based; Solana [V from HH row, Oct 9; re-check the listing]
- [ ] Disclose any reused code (Endgame scaffold is from Oct 10, inside the window)
- [ ] All submit actions are done by Shubham, not agents
