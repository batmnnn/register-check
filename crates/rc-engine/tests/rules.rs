//! Each rule runs against a `pass` fixture (must return 0 rows) and a `fail` fixture
//! (must return exactly `fail.expected.json`). Fixtures live in fixtures/rules/R<N>/.

use std::path::{Path, PathBuf};

use rc_engine::{load_rules, Engine, EngineError};
use rc_types::Register;
use serde_json::{Map, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn register(rel: &str) -> Register {
    let s = std::fs::read_to_string(root().join(rel)).expect("fixture readable");
    Register::from_json(&s).expect("fixture parses")
}

fn expected(rel: &str) -> Vec<Map<String, Value>> {
    let s = std::fs::read_to_string(root().join(rel)).expect("expected readable");
    serde_json::from_str(&s).expect("expected parses")
}

async fn engine(reg: &Register) -> Engine {
    let rules = load_rules(&root().join("rules")).expect("rules load");
    Engine::new(reg, rules).await.expect("engine builds")
}

async fn rows(rule: &str, fixture: &str) -> Vec<Map<String, Value>> {
    engine(&register(fixture))
        .await
        .run_rule(rule)
        .await
        .expect("rule runs")
        .rows
}

async fn check(n: u32) {
    let tag = format!("R{n}");
    let pass = rows(&tag, &format!("fixtures/rules/{tag}/pass.json")).await;
    assert!(
        pass.is_empty(),
        "{tag} pass fixture returned rows: {pass:#?}"
    );
    let fail = rows(&tag, &format!("fixtures/rules/{tag}/fail.json")).await;
    assert_eq!(
        fail,
        expected(&format!("fixtures/rules/{tag}/fail.expected.json")),
        "{tag} fail fixture"
    );
}

// Bug caught: comparing against an older reserve, picking the higher reserve on a tie,
// or reporting mints that have no reserve at all.
#[tokio::test]
async fn test_rule_r1_unbacked_mint() {
    check(1).await;
}

// Bug caught: zero balances reported, or an allowlist entry for another mint counted.
#[tokio::test]
async fn test_rule_r2_non_allowlisted_holder() {
    check(2).await;
}

// Bug caught: exactly-20% flagged, burns ignored in supply, or u64 overflow in amount * 10000.
#[tokio::test]
async fn test_rule_r3_holder_concentration() {
    check(3).await;
}

// Bug caught: a hook swapped and swapped back collapses to one row, or close/other kinds reported.
#[tokio::test]
async fn test_rule_r4_privileged_change() {
    check(4).await;
}

#[tokio::test]
async fn empty_register_passes_every_rule() {
    let results = engine(&Register::default())
        .await
        .run_all()
        .await
        .expect("runs");
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|v| v.passed()), "{results:#?}");
}

// The demo story: 6 allowlisted holders, reserve 1,000, a mint of 100 takes supply to 1,100.
// Only R1 may fire, on exactly the over-reserve mint.
#[tokio::test]
async fn demo_register_fires_only_r1_on_the_over_reserve_mint() {
    let violations = engine(&register("fixtures/demo/register.json"))
        .await
        .run()
        .await
        .expect("runs");
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].rule, "R1_unbacked_mint");
    assert_eq!(violations[0].rows.len(), 1);
    assert_eq!(violations[0].rows[0]["signature"], "demoMintSig6");
    assert_eq!(violations[0].rows[0]["supply_after"], 1_100_000_000u64);
}

#[tokio::test]
async fn unknown_rule_id_is_an_error() {
    let err = engine(&Register::default())
        .await
        .run_rule("R9")
        .await
        .expect_err("no R9");
    assert!(matches!(err, EngineError::UnknownRule(_)));
}
