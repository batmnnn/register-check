//! Golden test: the demo violation (R1 on the 1,100 mint) must produce exactly
//! fixtures/proof/r1_demo.golden.json, byte for byte, including result_hash.

use std::path::{Path, PathBuf};

use rc_engine::{load_rules, Engine};
use rc_proof::{build_all, canonical_json, result_hash, verify, Source, COMMITMENT};
use rc_types::Register;
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const SRC: Source = Source {
    rpc: "https://api.devnet.solana.com",
    cluster: "devnet",
};

async fn demo_bundles() -> Vec<Value> {
    let reg = Register::from_json(
        &std::fs::read_to_string(root().join("fixtures/demo/register.json")).expect("fixture"),
    )
    .expect("parses");
    let rules = load_rules(&root().join("rules")).expect("rules");
    let engine = Engine::new(&reg, rules.clone()).await.expect("engine");
    let violations = engine.run().await.expect("runs");
    build_all(&rules, &violations, &SRC).expect("bundles")
}

// Bug caught: any change to bundle fields, canonical encoding, or hashing that would make
// previously attested hashes unverifiable. Regenerate deliberately with RC_UPDATE_GOLDEN=1.
#[tokio::test]
async fn demo_r1_bundle_matches_golden_file() {
    let bundles = demo_bundles().await;
    assert_eq!(bundles.len(), 1);
    let got = format!(
        "{}\n",
        serde_json::to_string_pretty(&bundles[0]).expect("ser")
    );
    let path = root().join("fixtures/proof/r1_demo.golden.json");
    if std::env::var_os("RC_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &got).expect("write golden");
    }
    let want = std::fs::read_to_string(&path).expect("golden exists (run with RC_UPDATE_GOLDEN=1)");
    assert_eq!(got, want);
}

#[tokio::test]
async fn bundle_is_finalized_and_self_verifying() {
    let b = &demo_bundles().await[0];
    assert_eq!(b["commitment"], COMMITMENT);
    assert_eq!(b["observed"]["supply_after"], "1100000000");
    assert_eq!(b["expected"]["reserve_amount"], "1000000000");
    assert_eq!(
        b["explorer"],
        "https://explorer.solana.com/tx/demoMintSig6?cluster=devnet"
    );
    assert!(verify(b).expect("object"));
}

// Bug caught: hash depending on key order (non-canonical JSON).
#[tokio::test]
async fn key_order_does_not_change_the_hash() {
    let b = &demo_bundles().await[0];
    let mut entries: Vec<(String, Value)> =
        b.as_object().expect("obj").clone().into_iter().collect();
    entries.reverse();
    let reordered: serde_json::Map<String, Value> = entries.into_iter().collect();
    let reordered = Value::Object(reordered);
    assert_eq!(result_hash(b), result_hash(&reordered));
    assert_eq!(canonical_json(b), canonical_json(&reordered));
}

// Bug caught: the attestation tx (added after hashing) or a tampered field going unnoticed.
#[tokio::test]
async fn attested_tx_is_excluded_but_evidence_tampering_is_detected() {
    let mut b = demo_bundles().await.remove(0);
    b["attested_tx"] = Value::String("5attestSig".into());
    assert!(
        verify(&b).expect("object"),
        "attested_tx must not affect the hash"
    );
    b["observed"]["supply_after"] = Value::String("1000000000".into());
    assert!(!verify(&b).expect("object"), "tampered evidence must fail");
}
