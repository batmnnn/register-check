# Pitch video script (≤ 3:00)

Facts below are the [V] items from dossier.md or measured in this repo. Do not add prices (PRD §9) or unverified numbers.

**0:00 Problem (35 s)**
Tokenized dollars on Solana change supply every slot. The evidence that they're backed is a monthly report: Circle publishes a monthly examination, Paxos monthly attestations for PYUSD and USDG. The GENIUS Act (Public Law 119-27) makes that monthly report law. Between two reports, nobody can prove that each mint matched reserves, that only allowed wallets hold a permissioned token, or that a transfer hook or permanent delegate wasn't changed.

**0:35 What exists (20 s)**
Proof-of-reserve today (Chainlink PoR, Chaos Labs) publishes one aggregate number. It tells you the reserve. It doesn't tell you which transaction broke which rule.

**0:55 Register Check (45 s)**
An open-source Rust engine that ingests a Token-2022 mint's finalized history, puts it in seven Arrow tables, and runs register rules as plain SQL on DataFusion. Rules: unbacked mint against a signed reserve, non-allowlisted holder, holder concentration, privileged change (authority, permanent delegate, transfer hook). Every violation becomes a proof bundle: the transaction, the slot, observed vs expected, the hash of the exact SQL. Its sha256 is recorded on-chain by our Anchor program. Anyone can re-fetch the transaction, re-run the SQL and get the same hash.

**1:40 Proof it works (25 s)**
On devnet, a mint that pushed supply over the reserve was flagged 4 seconds after it was sent, and the proof hash was on-chain moments later. The ingester catches mints hidden in CPI calls or Token-2022 batch instructions. A supply-reconciliation check catches anything we fail to parse. 53 tests, mostly on real devnet transactions.

**2:05 Who pays (30 s)**
Issuers' compliance teams want an alert within a minute with evidence for their auditor. Exchanges and DeFi risk teams want to auto-pause a token that breaks a rule. Raters and auditors want continuous evidence between monthly reports. Model: the engine is free and open source, and hosted per-mint monitoring plus auditor evidence packs are paid. Go-to-market: public dashboards for PYUSD and USDG (Token-2022 mints with rich extensions) as the hook.

**2:35 Ask / close (25 s)**
Next: Yellowstone gRPC streaming, mainnet dashboards, multisig attestation, Chainlink and Brevis reserve inputs. We're looking for design partners among Solana stablecoin and RWA issuers. Register Check: continuous, reproducible evidence that a token's register follows its rules.
