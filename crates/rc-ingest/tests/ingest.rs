//! rc-ingest against real devnet responses saved in fixtures/rpc/ (scripts/fixture_txs.sh +
//! scripts/capture_fixtures.py). The fixtures mint JAk4… went through: create (permanent delegate +
//! transfer hook), mint 500 → A, transfer 50 A → B, permanent-delegate burn 30 from B, owner burn
//! 20 from A, set transfer hook, rotate mint authority, rotate permanent delegate. Supply 450.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use rc_ingest::{
    parse_transaction, Event, IngestError, Ingester, MintState, RpcSource, SigInfo, TokenSupply,
    TOKEN_2022_PROGRAM_ID,
};
use rc_types::{AuthorityChange, Balance, ChangeKind, Register};
use serde_json::{json, Value};

const MINT: &str = "JAk4GmJzpk9qDXXn4t6s7kYXaNsGj3jv38w9NJyQhy8w";
const PAYER: &str = "7K2fYJvQWNjmYEBr58UEoBY13YzhzC9rqRscXnbi3CaL"; // mint authority + permanent delegate
const HOLDER_A: &str = "EXtaAuPQLXb7yimfb88qNSFL5NJ1r139duZEctn9MKbK";
const HOLDER_B: &str = "5HiYhGPRF2R51QXDm5Fcb1jNFMUXHoHJB6qQMk8JNNFH";
const NEW_MINT_AUTH: &str = "DVRSx2igg96PVf3b94mmAEa5SZ6jRFYrGJYA1RjK7SkF";
const NEW_PDELEGATE: &str = "CqFmK9dmTk8dxb5RTCinxh3sTXU9sLTmVKTMqG8oD1xU";
const HOOK_A: &str = "9LamBFt3FeyZaFmVbwfY5XYYTujZkZYzgLF7DJiJEyUD";
const D: u64 = 1_000_000;

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rpc")
        .join(format!("{name}.json"));
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json");
    v["result"].clone()
}

fn sig_of(tx: &Value) -> String {
    tx["transaction"]["signatures"][0]
        .as_str()
        .expect("sig")
        .to_string()
}

const TX_FIXTURES: &[&str] = &[
    "tx_initializePermanentDelegate+initializeTransferHook+initializeMint",
    "tx_getAccountDataSize+initializeImmutableOwner+initializeAccount3",
    "tx_getAccountDataSize+initializeImmutableOwner+initializeAccount3_2",
    "tx_mintToChecked",
    "tx_transferChecked",
    "tx_burnChecked",
    "tx_burnChecked_2",
    "tx_updateTransferHook",
    "tx_setAuthority",
    "tx_setAuthority_2",
];

/// In-memory RpcSource. `sigs` is newest-first like the real RPC.
struct FakeRpc {
    sigs: Vec<SigInfo>,
    txs: HashMap<String, Value>,
    supply: TokenSupply,
    page: usize,
    tx_calls: AtomicUsize,
    /// If non-zero, the Nth `transaction()` call (1-based) fails like an exhausted rate limit.
    fail_at: AtomicUsize,
}

impl FakeRpc {
    fn devnet() -> Self {
        let sigs = fixture("fx_signatures")
            .as_array()
            .expect("array")
            .iter()
            .map(|e| SigInfo {
                signature: e["signature"].as_str().expect("sig").into(),
                slot: e["slot"].as_u64().expect("slot"),
                failed: !e["err"].is_null(),
            })
            .collect();
        let txs = TX_FIXTURES
            .iter()
            .map(|n| fixture(n))
            .map(|t| (sig_of(&t), t))
            .collect();
        let s = fixture("fx_token_supply");
        Self {
            sigs,
            txs,
            supply: TokenSupply {
                amount: s["value"]["amount"]
                    .as_str()
                    .expect("amt")
                    .parse()
                    .expect("u64"),
                context_slot: s["context"]["slot"].as_u64().expect("slot"),
            },
            page: 1000,
            tx_calls: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl RpcSource for FakeRpc {
    async fn signatures(
        &self,
        _a: &str,
        before: Option<&str>,
    ) -> Result<Vec<SigInfo>, IngestError> {
        let start = match before {
            None => 0,
            Some(b) => self
                .sigs
                .iter()
                .position(|s| s.signature == b)
                .map_or(self.sigs.len(), |i| i + 1),
        };
        Ok(self
            .sigs
            .iter()
            .skip(start)
            .take(self.page)
            .cloned()
            .collect())
    }
    async fn transaction(&self, sig: &str) -> Result<Option<Value>, IngestError> {
        let n = self.tx_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_at.load(Ordering::SeqCst) == n {
            return Err(IngestError::RateLimited {
                method: "getTransaction",
                attempts: 6,
            });
        }
        Ok(self.txs.get(sig).cloned())
    }
    async fn token_supply(&self, _m: &str) -> Result<TokenSupply, IngestError> {
        Ok(self.supply)
    }
}

fn change(tx: &str, kind: ChangeKind, old: Option<&str>, new: &str) -> AuthorityChange {
    let t = fixture(tx);
    AuthorityChange {
        mint: MINT.into(),
        slot: t["slot"].as_u64().expect("slot"),
        tx_index: t["transactionIndex"].as_u64().expect("idx") as u32,
        ix_index: 0,
        signature: sig_of(&t),
        change_kind: kind,
        old_value: old.map(Into::into),
        new_value: Some(new.into()),
    }
}

fn assert_devnet_register(reg: &Register) {
    let mint_sig = sig_of(&fixture("tx_mintToChecked"));
    assert_eq!(reg.mints.len(), 1);
    assert_eq!(reg.mints[0].signature, mint_sig);
    assert_eq!(reg.mints[0].amount, 500 * D);
    assert_eq!(reg.mints[0].supply_after, 500 * D);
    assert_eq!(reg.mints[0].authority, PAYER);

    assert_eq!(reg.transfers.len(), 1);
    assert_eq!(
        (
            reg.transfers[0].from_owner.as_str(),
            reg.transfers[0].to_owner.as_str(),
            reg.transfers[0].amount
        ),
        (HOLDER_A, HOLDER_B, 50 * D)
    );

    let burns: Vec<(u64, u64, &str, bool)> = reg
        .burns
        .iter()
        .map(|b| {
            (
                b.amount,
                b.supply_after,
                b.authority.as_str(),
                b.via_permanent_delegate,
            )
        })
        .collect();
    assert_eq!(
        burns,
        vec![
            (30 * D, 470 * D, PAYER, true),
            (20 * D, 450 * D, HOLDER_A, false)
        ]
    );

    assert_eq!(
        reg.authority_changes,
        vec![
            change(
                "tx_updateTransferHook",
                ChangeKind::TransferHookProgram,
                None,
                HOOK_A
            ),
            change(
                "tx_setAuthority",
                ChangeKind::MintAuthority,
                Some(PAYER),
                NEW_MINT_AUTH
            ),
            change(
                "tx_setAuthority_2",
                ChangeKind::PermanentDelegate,
                Some(PAYER),
                NEW_PDELEGATE
            ),
        ]
    );

    let mut balances: Vec<(String, u64)> = reg
        .balances
        .iter()
        .map(|b: &Balance| (b.owner.clone(), b.amount))
        .collect();
    balances.sort();
    let mut want = vec![
        (HOLDER_A.to_string(), 430 * D),
        (HOLDER_B.to_string(), 20 * D),
    ];
    want.sort();
    assert_eq!(balances, want);
}

// Bug caught: any mis-parsed instruction, wrong owner mapping, wrong supply arithmetic, or a
// permanent-delegate burn not flagged.
#[tokio::test]
async fn full_devnet_history_rebuilds_register_and_reconciles_supply() {
    let mut ing = Ingester::new(FakeRpc::devnet(), MINT);
    let report = ing.poll().await.expect("poll");
    assert_eq!(report.new_signatures, 10);
    assert_eq!(report.computed_supply, 450 * D);
    assert_eq!(report.drift, None, "{report:?}");
    assert!(report.unparsed.is_empty(), "{:?}", report.unparsed);
    assert_devnet_register(ing.register());
}

// Bug caught: re-fetching or double-counting transactions on every poll.
#[tokio::test]
async fn second_poll_fetches_nothing_new() {
    let mut ing = Ingester::new(FakeRpc::devnet(), MINT);
    ing.poll().await.expect("poll 1");
    let calls = ing.rpc().tx_calls.load(Ordering::SeqCst);
    let report = ing.poll().await.expect("poll 2");
    assert_eq!(report.new_signatures, 0);
    assert_eq!(ing.rpc().tx_calls.load(Ordering::SeqCst), calls);
    assert_devnet_register(ing.register());
}

// Bug caught: supply/authority state depending on arrival order (red-team m).
#[tokio::test]
async fn scrambled_signature_order_gives_same_register() {
    let mut rpc = FakeRpc::devnet();
    rpc.sigs.reverse();
    rpc.sigs.swap(2, 7);
    let mut ing = Ingester::new(rpc, MINT);
    ing.poll().await.expect("poll");
    assert_devnet_register(ing.register());
}

// Bug caught: an unparsed supply change goes unnoticed (red-team a).
#[tokio::test]
async fn supply_mismatch_is_reported_as_drift() {
    let mut rpc = FakeRpc::devnet();
    rpc.supply.amount = 999 * D;
    let mut ing = Ingester::new(rpc, MINT);
    let drift = ing.poll().await.expect("poll").drift.expect("drift");
    assert_eq!((drift.onchain, drift.computed), (999 * D, 450 * D));
}

fn synthetic_mint(i: u64) -> (SigInfo, Value) {
    let mut tx = fixture("tx_mintToChecked");
    let sig = format!("synthetic{i:05}");
    tx["transaction"]["signatures"][0] = json!(sig);
    tx["slot"] = json!(1_000_000 + i);
    tx["transactionIndex"] = json!(0);
    let ix = &mut tx["transaction"]["message"]["instructions"];
    for ins in ix.as_array_mut().expect("ixs") {
        if ins["programId"] == TOKEN_2022_PROGRAM_ID {
            ins["parsed"]["info"]["tokenAmount"]["amount"] = json!("1");
        }
    }
    tx["meta"]["postTokenBalances"] = json!([]);
    tx["meta"]["preTokenBalances"] = json!([]);
    (
        SigInfo {
            signature: sig,
            slot: 1_000_000 + i,
            failed: false,
        },
        tx,
    )
}

// Bug caught: stopping after the first 1000-signature page, or processing a signature that
// appears on two pages twice (red-team h).
#[tokio::test]
async fn paginates_past_1000_signatures_and_dedups_page_overlap() {
    let n = 2_500u64;
    let mut sigs = Vec::new();
    let mut txs = HashMap::new();
    for i in (0..n).rev() {
        let (s, t) = synthetic_mint(i);
        txs.insert(s.signature.clone(), t);
        sigs.push(s);
    }
    // The RPC repeats a signature across the page boundary.
    sigs.insert(1000, sigs[999].clone());
    let rpc = FakeRpc {
        sigs,
        txs,
        supply: TokenSupply {
            amount: n,
            context_slot: u64::MAX,
        },
        page: 1000,
        tx_calls: AtomicUsize::new(0),
        fail_at: AtomicUsize::new(0),
    };
    let mut ing = Ingester::new(rpc, MINT);
    let report = ing.poll().await.expect("poll");
    assert_eq!(report.new_signatures, n as usize);
    assert_eq!(ing.rpc().tx_calls.load(Ordering::SeqCst), n as usize);
    assert_eq!(ing.register().mints.len(), n as usize);
    assert_eq!(report.drift, None);
}

// Bug caught: a failed transaction's mint counted as supply.
#[tokio::test]
async fn failed_transactions_change_nothing() {
    let mut tx = fixture("tx_mintToChecked");
    tx["meta"]["err"] = json!({"InstructionError": [0, {"Custom": 1}]});
    assert_eq!(
        parse_transaction(&tx, MINT, &mut MintState::default()).expect("ok"),
        None
    );

    // A signature the RPC already marks failed is never fetched.
    let mut rpc = FakeRpc::devnet();
    let failed_sig = rpc.sigs[0].signature.clone();
    rpc.sigs[0].failed = true;
    rpc.txs.remove(&failed_sig);
    rpc.supply.amount = 450 * D;
    let mut ing = Ingester::new(rpc, MINT);
    let report = ing.poll().await.expect("poll");
    assert_eq!(report.failed_skipped, 1);
    assert_eq!(report.pending, 0);
}

fn mint_events(tx: &Value) -> Vec<Event> {
    parse_transaction(tx, MINT, &mut MintState::default())
        .expect("parses")
        .expect("succeeded")
        .events
        .into_iter()
        .filter(|e| matches!(e, Event::Mint(_)))
        .collect()
}

fn take_token_ix(tx: &mut Value) -> Value {
    let ixs = tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs");
    let pos = ixs
        .iter()
        .position(|i| i["programId"] == TOKEN_2022_PROGRAM_ID)
        .expect("token ix");
    ixs.remove(pos)
}

// Bug caught: mints done through CPI (inner instructions) are missed (red-team a).
#[test]
fn cpi_inner_mint_is_detected() {
    let mut tx = fixture("tx_mintToChecked");
    let token_ix = take_token_ix(&mut tx);
    let outer = json!({"programId": "SomeIssuerProgram1111111111111111111111111", "accounts": [], "data": "", "stackHeight": 1});
    let ixs = tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs");
    ixs.push(outer);
    let idx = ixs.len() - 1;
    tx["meta"]["innerInstructions"] = json!([{"index": idx, "instructions": [token_ix]}]);
    let mints = mint_events(&tx);
    assert_eq!(mints.len(), 1);
    let Event::Mint(m) = &mints[0] else {
        unreachable!()
    };
    assert_eq!(m.amount, 500 * D);
    assert_eq!(
        m.ix_index as usize,
        idx + 1,
        "inner ix numbered right after its outer ix"
    );
}

// Bug caught: a mintTo wrapped in a Token-2022 `batch` instruction is missed.
#[test]
fn batch_wrapped_mint_is_detected() {
    let mut tx = fixture("tx_mintToChecked");
    let token_ix = take_token_ix(&mut tx);
    let batch = json!({
        "programId": TOKEN_2022_PROGRAM_ID,
        "program": "spl-token",
        "parsed": {"type": "batch", "info": {"instructions": [token_ix["parsed"].clone(), token_ix["parsed"].clone()]}}
    });
    tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs")
        .push(batch);
    assert_eq!(mint_events(&tx).len(), 2);
}

// Bug caught: matching on the shared "spl-token" label instead of the program id.
#[test]
fn legacy_spl_token_program_is_ignored() {
    let mut tx = fixture("tx_mintToChecked");
    for ix in tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs")
    {
        if ix["programId"] == TOKEN_2022_PROGRAM_ID {
            ix["programId"] = json!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        }
    }
    assert!(mint_events(&tx).is_empty());
}

// Bug caught: an instruction type we don't interpret is silently dropped.
#[test]
fn unknown_token_2022_instruction_on_our_mint_is_reported() {
    let mut tx = fixture("tx_mintToChecked");
    for ix in tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs")
    {
        if ix["programId"] == TOKEN_2022_PROGRAM_ID {
            ix["parsed"]["type"] = json!("someFutureMintVariant");
        }
    }
    let p = parse_transaction(&tx, MINT, &mut MintState::default())
        .expect("ok")
        .expect("ok");
    assert!(p.events.is_empty());
    assert_eq!(p.unparsed, vec!["someFutureMintVariant".to_string()]);
}

// Live devnet smoke test of HttpRpc against the demo mint from scripts/devnet_setup.sh.
// Run with: RC_LIVE_MINT=<mint> cargo test -p rc-ingest --test ingest live_ -- --ignored
#[tokio::test]
#[ignore = "hits devnet"]
async fn live_devnet_demo_mint_reconciles() {
    let mint = std::env::var("RC_LIVE_MINT").expect("set RC_LIVE_MINT");
    let rpc = rc_ingest::HttpRpc::new("https://api.devnet.solana.com").expect("client");
    let mut ing = Ingester::new(rpc, mint);
    let report = ing.poll().await.expect("poll");
    println!("{report:#?}");
    assert_eq!(report.drift, None);
    assert!(report.unparsed.is_empty());
    assert_eq!(report.computed_supply, report.supply.amount);
    let held: u64 = ing.register().balances.iter().map(|b| b.amount).sum();
    assert_eq!(held, report.supply.amount, "balances add up to supply");
}

// Bug caught: a transient RPC error mid-poll permanently hides the older, unfetched signatures
// behind newer `seen` ones (review finding #1).
#[tokio::test]
async fn rpc_error_mid_poll_loses_nothing() {
    let rpc = FakeRpc::devnet();
    rpc.fail_at.store(5, Ordering::SeqCst);
    let mut ing = Ingester::new(rpc, MINT);
    assert!(ing.poll().await.is_err(), "5th getTransaction fails");
    let report = ing.poll().await.expect("next poll recovers");
    assert_eq!(report.pending, 0);
    assert_eq!(report.drift, None);
    assert_devnet_register(ing.register());
}

// Bug caught: a signature the RPC doesn't serve yet is dropped instead of retried, and its
// absence is reported as supply drift.
#[tokio::test]
async fn unserved_transaction_stays_pending_and_suppresses_drift() {
    let mut rpc = FakeRpc::devnet();
    let mint_sig = sig_of(&fixture("tx_mintToChecked"));
    rpc.txs.remove(&mint_sig).expect("mint tx");
    let mut ing = Ingester::new(rpc, MINT);
    let report = ing.poll().await.expect("poll");
    assert_eq!(report.pending, 1);
    assert_eq!(report.drift, None, "incomplete history is not drift");
    let calls = ing.rpc().tx_calls.load(Ordering::SeqCst);
    let report = ing.poll().await.expect("poll 2");
    assert_eq!(report.pending, 1, "still pending, not dropped");
    assert_eq!(
        ing.rpc().tx_calls.load(Ordering::SeqCst),
        calls + 1,
        "retried exactly once"
    );
}

// Bug caught: setAuthority(transferHookProgramId) treated as a hook *program* change, corrupting
// the old_value of a later updateTransferHook (review finding #11).
#[test]
fn hook_authority_change_does_not_overwrite_hook_program() {
    let mut tx = fixture("tx_setAuthority");
    for ix in tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs")
    {
        if ix["programId"] == TOKEN_2022_PROGRAM_ID {
            ix["parsed"]["info"]["authorityType"] = json!("transferHookProgramId");
        }
    }
    let mut state = MintState {
        transfer_hook_program: Some(HOOK_A.into()),
        transfer_hook_authority: Some(PAYER.into()),
        ..Default::default()
    };
    let p = parse_transaction(&tx, MINT, &mut state)
        .expect("ok")
        .expect("ok");
    let Event::Authority(a) = &p.events[0] else {
        panic!("authority event")
    };
    assert_eq!(a.change_kind, ChangeKind::TransferHookAuthority);
    assert_eq!(a.old_value.as_deref(), Some(PAYER));
    assert_eq!(
        state.transfer_hook_program.as_deref(),
        Some(HOOK_A),
        "program untouched"
    );
    assert_eq!(
        state.transfer_hook_authority.as_deref(),
        Some(NEW_MINT_AUTH)
    );
}

// Bug caught: one malformed transaction makes every later poll fail (monitoring wedged).
#[tokio::test]
async fn malformed_transaction_is_reported_not_fatal() {
    let mut rpc = FakeRpc::devnet();
    let transfer_sig = sig_of(&fixture("tx_transferChecked"));
    let tx = rpc.txs.get_mut(&transfer_sig).expect("tx");
    for ix in tx["transaction"]["message"]["instructions"]
        .as_array_mut()
        .expect("ixs")
    {
        if ix["programId"] == TOKEN_2022_PROGRAM_ID {
            ix["parsed"]["info"]["tokenAmount"]["amount"] = json!("not-a-number");
        }
    }
    let mut ing = Ingester::new(rpc, MINT);
    let report = ing.poll().await.expect("poll still succeeds");
    assert!(
        report
            .unparsed
            .iter()
            .any(|(s, t)| s == &transfer_sig && t.starts_with("parse_error")),
        "{:?}",
        report.unparsed
    );
    assert_eq!(
        ing.register().mints.len(),
        1,
        "rest of history still ingested"
    );
}
