# Register Check: PRD
Status: LOCKED by Shubham (Oct 12). [V]/[G] labels follow the dossier.

## 1. Problem
Tokenized dollars and funds on Solana change supply every slot. Reserve evidence is a monthly PDF (Circle, Paxos [V dossier s.2]), and GENIUS makes that monthly cadence law [V]. Between reports, nobody can prove that each mint matched reserves, that only allowed wallets hold a permissioned fund, or that a hook or delegate change was authorized. Existing PoR gives one aggregate number (Chainlink, Chaos [V]).

## 2. Users and jobs to be done
| User | Job |
|---|---|
| Issuer compliance team | "Tell me within a minute if any mint exceeds attested reserves or any non-allowlisted wallet receives tokens, with evidence I can hand to the auditor." |
| Wallets / exchanges / DeFi risk | "Auto-pause listing or collateral if a stablecoin breaks a register rule." |
| Raters (Moody's/S&P-style) | "Continuous evidence between monthly reports." |
| Auditors (registered accounting firms) | "Reproducible queries + signed result history for the month-end exam." |
| AI agents holding stablecoins | "Machine-readable health check before accepting a token." |

## 3. Scope
v0 (hackathon, devnet):
1. Ingest Token-2022 mint/burn/transfer/authority/hook changes for one or more mints (RPC polling, finalized).
2. Arrow tables + DataFusion SQL rules. Ships 4 rules:
   - R1 unbacked_mint: supply after a mint > latest attested reserve.
   - R2 non_allowlisted_holder: balance > 0 for a wallet not in the allowlist.
   - R3 holder_concentration: any holder > X% of supply.
   - R4 privileged_change: mint/freeze authority, permanent delegate, or transfer-hook program changed.
3. Reserve feed: signed JSON attestation posted by the issuer (mock), stored as a table.
4. Alert: JSON proof bundle + webhook/console + on-chain attestation (rule result hash per check).
5. Simple dashboard (read-only).
v1 (paid): Yellowstone gRPC streaming, mainnet USDC/PYUSD/USDG public dashboards, custom rule packs, auditor export, Chainlink/Brevis reserve inputs, SLA.

## 4. SQL rule examples (target syntax)
```sql
-- R1: unbacked mint
SELECT m.signature, m.slot, m.supply_after, r.reserve_amount
FROM mints m JOIN latest_reserve r ON m.mint = r.mint
WHERE m.supply_after > r.reserve_amount;
-- R2
SELECT b.owner, b.amount FROM balances b
LEFT JOIN allowlist a ON b.mint = a.mint AND b.owner = a.owner
WHERE b.amount > 0 AND a.owner IS NULL;
-- R3
SELECT b.owner, b.amount, s.supply FROM balances b JOIN supply s ON b.mint = s.mint
WHERE b.amount * 10000 > s.supply * 2000;  -- > 20%
-- R4
SELECT signature, slot, change_kind, old_value, new_value FROM authority_changes;
```

## 5. Architecture
ingester (Rust, RPC) -> normalized events -> Arrow RecordBatches -> DataFusion SessionContext (tables) -> rule runner (SQL files) -> violations -> proof builder -> sinks (console/webhook, JSON file, Anchor attestation program).
Crates: rc-types, rc-ingest, rc-engine, rc-proof, rc-cli; programs/rc-attest.

## 6. Data model
- mints(mint, slot, signature, amount, supply_after, authority)
- burns(mint, slot, signature, amount, supply_after, authority, via_permanent_delegate bool)
- transfers(mint, slot, signature, from_owner, to_owner, amount)
- balances(mint, owner, amount, slot)
- authority_changes(mint, slot, signature, change_kind, old_value, new_value)
- reserves(mint, as_of_ts, reserve_amount, attestor, sig, source_url)
- allowlist(mint, owner)

## 7. Proof / alert format (v0)
```json
{"rule":"R1_unbacked_mint","mint":"...","slot":123,"signature":"5x...","commitment":"finalized",
 "observed":{"supply_after":1100000000},"expected":{"reserve_amount":1000000000,"reserve_as_of":"...","attestor":"..."},
 "rpc":"https://api.devnet.solana.com","explorer":"https://explorer.solana.com/tx/5x...?cluster=devnet",
 "result_hash":"sha256(canonical json)","attested_tx":"..."}
```
Anyone can re-fetch the tx by signature and re-run the SQL. The result hash goes on-chain via rc-attest.

## 8. Success metrics
Hackathon: end-to-end R1 alert in < 60s on devnet [G target]; 4 rules green in tests; 3-min demo; 2-3 named people (issuer/dev) giving feedback quotes. v1: 3 design partners, public dashboards for USDC/PYUSD/USDG.

## 9. Pricing hypothesis [G]
Open-source engine (free). Hosted: per-mint monitoring tier for issuers (a monthly fee); auditor export add-on; public free dashboards as marketing. Numbers TBD after talks. Don't quote prices in the pitch.

## 10. GTM [G]
1. Public dashboard for PYUSD/USDG (Token-2022, rich extensions) as a hook.
2. Superteam India network and Solana Foundation stablecoin teams.
3. Auditors and raters: offer free monthly evidence packs.

## 11. Risks and threat model
| Threat | Mitigation |
|---|---|
| RPC lies or omits | Multiple RPCs, compare; proofs carry signature + slot so anyone can re-verify; v1 Geyser on own node |
| Reorgs / finality | Only `finalized` for proofs; `confirmed` marked PENDING |
| Attestation lag (monthly) | R1 uses latest attested reserve + a declared intra-month issuance budget; flag "stale reserve" > N days |
| False alarms | Rule severity levels, dry-run mode, allowlist/known-ops lists (e.g. scheduled mints) |
| Spoofed reserves | Reserve JSON must be signed by a registered attestor key; v1 Chainlink/Brevis inputs |
| Confidential transfers | Per-holder rules not possible; supply rules still work; state the limit |
| Permanent delegate seizures look like theft | Separate event class + R4 |
| Legal | We give evidence, not an audit opinion. No "certified" wording. Not investment advice. |
| Key compromise of attest program authority | Authority = multisig in v1; v0 single key, devnet |
