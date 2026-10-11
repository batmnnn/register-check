# Register Check: research dossier
Built Mon Oct 12 2026, 03:20-04:00 IST. Labels: [V] = verified at the cited source today. [G] = guess or not verified. Web pages were treated as untrusted. No numbers are invented. If a number isn't here, I didn't find it.

## 1. Token-2022 extensions that matter for a register
| Extension | What it does | Why Register Check cares | Source |
|---|---|---|---|
| Mint authority | Key that can mint new supply | Every supply increase is a mint ix signed by this key. That's our core event. [V basic SPL] | https://solana.com/docs/tokens/extensions |
| Freeze authority | Can freeze any token account | A "frozen account moved" rule; compliance actions | [V] PYUSD has one: https://solana.com/pyusd |
| Permanent delegate | Can transfer OR burn from ANY account | Seizure/burn changes supply and balances with no holder signature. We must log it as a separate event class. | [V] PYUSD active: https://solana.com/pyusd , https://developer.paypal.com/community/blog/pyusd-solana-token-extensions/ |
| Transfer hook | Every transfer CPIs into a hook program; extra accounts in the ExtraAccountMetaList PDA; the hook program can be changed by UpdateTransferHook | Allowlist enforcement lives here. A hook-program CHANGE is a high-risk event we should alert on. | [V] https://solana.com/docs/tokens/extensions/transfer-hook-integration . PYUSD hook initialized with a null program: [V] solana.com/pyusd |
| Transfer fee | Fee withheld on transfer | Supply accounting: withheld amounts | [V] listed for PYUSD (initialized) solana.com/pyusd |
| Confidential transfers / balances | Encrypted balances and amounts | Per-holder rules break. Only supply (mint/burn) stays public. Limit stated in PRD. | [V] named on solana.com/docs/payments/how-payments-work ; details [G] |
| Metadata pointer / token metadata | On-chain name/symbol/URI | We can store the reserve-attestation URL in metadata. Display only. | [G] details not re-read today |
| Default account state, CPI guard, non-transferable | Listed in the ExtensionType enum | Default-frozen = KYC-gated tokens (RWAs) | [V] enum on https://solana.com/docs/tokens/extensions |
Note: an extension's state lives in the account's TLV data, so the ingester must parse TLV. [V] solana.com/docs/tokens/extensions

## 2. Solana stablecoins and tokenized assets
| Asset | Issuer | Mainnet mint | Program | Reserve evidence + cadence | Source |
|---|---|---|---|---|---|
| USDC | Circle | EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v | Token (legacy) | Monthly Deloitte examination report [V] | mint: https://solana.com/docs/payments/how-payments-work ; https://www.circle.com/en/transparency |
| USDT | Tether | Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB | Token (legacy) | Quarterly attestation [G not checked] | solana.com payments doc |
| PYUSD | Paxos (for PayPal) | 2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo | Token-2022 | Monthly; KPMG after Feb 28 2025 [V per search synthesis of paxos.com page] | https://www.paxos.com/pyusd-transparency |
| USDG | Paxos (Global Dollar) | 2u1tszSeqZ3qBWF3uNGPFc8TzMk2tdiwknnRMWGWjGWH | Token-2022 | Monthly; KPMG for reports after Feb 27 2026 [V-ish: search synthesis, re-open page before quoting] | https://www.paxos.com/usdg-transparency |
| BUIDL (Solana share class) | BlackRock via Securitize | GyWgeqpy5GueU2YbkE8xqUeVEokCMMCEeUrfbtMw6phr | [G] | Fund NAV/reports [G] | https://investors.securitize.io/news/news-details/2025/BlackRock-and-Securitize-Debut-New-BUIDL-Share-Class-on-Solana-Network-03-25-2025/default.aspx |
| USDY | Ondo (Reg S yieldcoin, not a stablecoin) | A1KLoBrKBde8Ty9qtNQUtq3C2ortoC3u7twggz7sEto6 | [G] | [G] | https://docs.ondo.finance/addresses |
| OUSG | Ondo (tokenized T-bills) | i7u4r16TcsJTgq1kAG8opmVZyVnAKBwLKu6ZPMwzxNc | [G] | Ethereum oracle bounded +/-74 bps vs SHV [V for ETH] | https://docs.ondo.finance/addresses |
| CASH | Phantom (Colosseum pays prizes in it) | [G] not looked up | [G] | [G] | Rules s.14 |
Key insight [V from the table]: reserve evidence is MONTHLY and off-chain (PDF). Supply changes per slot. That gap is the product.

## 3. Proof-of-reserve prior art (competition)
| Who | What | Gap we exploit | Source |
|---|---|---|---|
| Chainlink PoR + Secure Mint | Reserve feeds on-chain; can gate minting; circuit breakers; mentions Solana | Gives one reserve NUMBER per feed. No register-level SQL rules (holders, hooks, delegates), no tx-level proof bundle | [V] https://chain.link/proof-of-reserve , https://docs.chain.link/data-feeds/proof-of-reserve |
| Chaos Labs PoR | 3 streams: locked reserves, issued supply, collateralization; on-chain attestations; Ethena, AUSD | Same: aggregate-level | [V] https://chaoslabs.xyz/posts/proof-of-reserve-as-risk-infrastructure |
| Ethena USDe PoR | Weekly proof by Chaos, Chainlink, Harris & Trotter, LlamaRisk | Weekly cadence | [V] https://paragraph.com/@ethena-labs/usde-proof-of-reserves-launch |
| Brevis + Primus + Perena | zkTLS/TEE/zkVM reserve proofs for USD* on Solana | Reserve side only; complements us (an input source) | [V] https://blog.brevis.network/2026/04/13/brevis-primus-and-perena-verifiable-proof-of-reserves-for-usd/ |
| TRES (acquired by Fireblocks, Jan 2026) | Continuous reconciliation / proof of funds | Closed SaaS, enterprise | [V] https://www.fireblocks.com/blog/fireblocks-acquires-tres |
| OKX / Binance | ZK Merkle-sum proof of solvency (exchanges) | Exchange liabilities, not token registers | [V] github.com/okx/proof-of-reserves-v2 , github.com/binance/zkmerkle-proof-of-solvency |
| Pyth, Switchboard, Accountable, Cryptio | [G] Not verified today. Don't claim anything about them in the pitch. | | |
Positioning: "Chainlink PoR tells you the reserve number. Register Check tells you WHICH tx broke WHICH rule, with proof, in open-source SQL."

## 4. Regulation
1. GENIUS Act = Public Law 119-27. Monthly reserve report (outstanding supply, reserve amount, composition, tenor, custody). Examined monthly by a registered public accounting firm. CEO and CFO certify. [V] https://www.govinfo.gov/content/pkg/PLAW-119publ27/pdf/PLAW-119publ27.pdf (read via search excerpts; quote the section number only after opening the PDF).
2. Treasury NPRM (Apr 3 2026, FR 2026-06489): state regimes must keep monthly reserve disclosures as a "uniform requirement". [V] https://www.govinfo.gov/content/pkg/FR-2026-04-03/html/2026-06489.htm ; summary https://www.consumerfinancialserviceslawmonitor.com/2026/04/treasury-proposes-genius-act-principles-for-acceptable-state-stablecoin-regimes/
3. Treasury proposed rule on issuance/offer/sale (section 3), FR Aug 18 2026. [V] https://www.federalregister.gov/documents/2026/08/18/2026-16796/genius-act-regulations-on-payment-stablecoin-issuance-offer-and-sale
4. MiCA (Reg (EU) 2023/1114): EMTs hold >=30% in bank deposits (60% if significant). ART reserve is segregated and full. Supervisors expect daily reconciliation of on-chain supply vs ledger (per a vendor's guide, so treat it as an opinion). [V secondary] https://safeheld.com/resources/mica-reserve-of-assets
5. Moody's: B3 issuer rating for Sky Protocol, Oct 7 2026. First stablecoin-protocol rating. About $90M tangible common equity vs about $10B managed assets. [V] https://www.prnewswire.com/news-releases/moodys-ratings-assigns-first-issuer-rating-to-sky-protocol-302900578.html , https://news.bitcoin.com/stablecoins/moodys-gives-sky-protocol-first-stablecoin-credit-rating/
6. UBS onchain MMF built as a GENIUS reserve fund: seen only as a Zeitgeist Signal in Notion ("UBS files two onchain money market funds..."). A web search did not confirm it. [G until a primary filing is opened]
Pitch line: the law makes reserves a monthly audit; chains move every 400ms [G: slot-time figure, check it]. Raters and auditors need continuous evidence.

## 5. Solana data access
| Option | Fit | Source |
|---|---|---|
| JSON-RPC (getSignaturesForAddress + getTransaction, getTokenSupply, getProgramAccounts) | Easiest for the hackathon. Polling. Use commitment=finalized. | [G standard API] |
| WebSocket logsSubscribe / accountSubscribe | Low latency on devnet, no infra | [G] |
| Geyser plugin / Yellowstone gRPC | Streams slots, blocks, txs and accounts with filters and a commitment level. v1 production path. | [V] https://github.com/rpcpool/yellowstone-grpc |
| Helius webhooks / gRPC; pre/post token balances | Hosted. Easy token-balance deltas. | [V] https://www.helius.dev/docs/grpc/transaction-monitoring |
| Snapshots / ledger tool | Backfill and replay | [G] |
Finality: process only `finalized` for proofs, and `confirmed` for early "pending" alerts. [G design choice]

## 6. DataFusion integration
1. Custom TableProvider: implement scan() and return an ExecutionPlan. [V pattern, docs.rs]
2. StreamingTable takes PartitionStreams for unbounded RecordBatch streams. [V] https://docs.rs/datafusion/latest/datafusion/catalog/streaming/struct.StreamingTable.html
3. Hackathon choice: MemTable re-registered per batch window (simple, deterministic tests). StreamingTable in v1. [G design]
4. Rules = SQL that returns violating rows. Empty = pass.

## 7. Past Colosseum winners and judging
1. Criteria (Rules s.8) [V]: functionality/code, impact, novelty, UX, open source/composability, business plan.
2. Colosseum guidance (secondary, Superteam Turkiye) [V secondary]: founder fit, insight, execution, market, 2-min comms, business, traction. Pitch and demo videos max 3 min each, public repo. https://tr.superteam.fun/colosseum/what-judges-score , https://tr.superteam.fun/colosseum
3. Winners (from earlier research, blog.colosseum.com) [V]: Grand Champions were Ore, Reflect (hedge-backed stablecoin FX), TapeDrive (storage infra), Unruggable (hardware wallet). Infra and new primitives win.
4. Past compliance / data-infra winners specifically: [G] NOT verified. I didn't find a winner list with a compliance category. Txtx (Radar, infra) appears in the Radar post. Run Colosseum Copilot for "proof of reserves", "compliance", "indexer".
