# Demo video script (≤ 3:00)

Record on a fresh all-green mint. Every command below was run on devnet on 2026-10-12.

## Before recording (5 min)

```bash
scripts/demo.sh reset                 # new mint, holders A-E = 200, F = 0, signed reserve 1,000
cargo build -p rc-cli                 # so `watch` starts instantly
```
Terminal layout: **left** `scripts/demo.sh watch --attest`, **right** an empty shell. Browser: `scripts/demo.sh dashboard` → http://127.0.0.1:8080/web/ (all four cards green). Second tab: Solana Explorer (devnet). Font size up and notifications off.

## Script

| Time | Screen | Say (roughly) |
|---|---|---|
| 0:00–0:20 | Circle / Paxos transparency page, then the dashboard (all green) | "Stablecoin reserves are proven by a monthly report. Supply changes every slot. Between reports, nobody can show that each mint was backed. Register Check watches a Token-2022 mint live and checks it against the issuer's signed reserve." |
| 0:20–0:35 | Dashboard: supply 1,000 / reserve 1,000, four green rules | "This is a devnet Token-2022 mint with a permanent delegate and a transfer hook, like PYUSD. Reserve 1,000, signed by the attestor's key. Four rules, all passing on finalized data." |
| 0:35–0:50 | Right terminal: `scripts/demo.sh mint 100` | "Now the issuer mints 100 more tokens without any new reserve." |
| 0:50–1:10 | Left terminal: `ALERT R1_unbacked_mint …`, then `ATTESTED …`. Dashboard turns red, 110% of reserve | "Seconds after finality: R1 fires. Supply 1,100 against a 1,000 reserve. The proof bundle is written and its hash is posted on-chain." (Measured: 4 s from sending the mint.) |
| 1:10–1:30 | Open `proofs/R1_*.json` in the editor; click its `explorer` link | "The proof names the transaction, the slot, finalized commitment, what was observed and what was expected, and the sha256 of the exact rule SQL." |
| 1:30–1:45 | `cat rules/R1.sql` | "The rule is eight lines of SQL running on DataFusion. Open source. Any auditor can re-run it." |
| 1:45–2:00 | `python3 -I scripts/verify_proof.py proofs/*.json` → `OK` | "Twenty lines of plain Python, no shared code, re-derive the same hash." |
| 2:00–2:15 | Dashboard "✓ hash on-chain" link → explorer tx → rc-attest program | "And it's recorded by our Anchor program: rule SQL hash plus result hash, authority-gated, no duplicates." |
| 2:15–2:35 | `scripts/demo.sh swap-hook` → R4 alert | "Bonus: the issuer swaps the transfer-hook program, which governs every transfer. R4 flags it instantly, with the old and new program in the proof." |
| 2:35–3:00 | Dashboard, then the README threat-model table | "CPI mints, batch-wrapped mints, permanent-delegate seizures, pagination gaps, unparsed instructions caught by supply reconciliation: all tested on real devnet transactions. Open source, Token-2022 native. Next: Yellowstone streaming and public dashboards for PYUSD and USDG." |

## If something goes wrong on camera

- Alert slow: public devnet RPC throttles bursts, and the watcher backs off and retries. Wait one more poll; don't restart.
- `swap-hook` makes later transfers fail (the demo hook isn't a deployed program). Run `scripts/demo.sh unhook` before `stranger`.
- Need another clean take: `scripts/demo.sh reset` (old evidence is archived, not deleted).
