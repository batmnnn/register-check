//! Register Check proof bundles. One bundle per violating row: the rule, the evidence row, where
//! to re-fetch it, and `result_hash = sha256(canonical JSON)`. Anyone can rebuild the bundle from
//! the finalized tx + rule SQL and get the same hash; the hash is what goes on-chain.

mod canonical;

use rc_engine::{Rule, Violation};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use canonical::{canonical_json, stringify_integers};

/// Proofs only ever describe finalized data.
pub const COMMITMENT: &str = "finalized";

/// Fields excluded from the hash: the hash itself and the attestation tx that records it.
const UNHASHED: [&str; 2] = ["result_hash", "attested_tx"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProofError {
    #[error("violation row for {rule} has no `mint` column")]
    MissingMint { rule: String },
    #[error("unknown cluster {0:?}: expected devnet, testnet or mainnet-beta")]
    Cluster(String),
    #[error("bundle is not a JSON object")]
    NotObject,
}

/// Where the checked data came from.
#[derive(Debug, Clone)]
pub struct Source<'a> {
    pub rpc: &'a str,
    pub cluster: &'a str,
}

fn explorer_url(cluster: &str, kind: &str, id: &str) -> Result<String, ProofError> {
    let suffix = match cluster {
        "mainnet-beta" => String::new(),
        "devnet" | "testnet" => format!("?cluster={cluster}"),
        other => return Err(ProofError::Cluster(other.to_string())),
    };
    Ok(format!("https://explorer.solana.com/{kind}/{id}{suffix}"))
}

fn pick(row: &Map<String, Value>, keys: &[&str]) -> Value {
    let mut m = Map::new();
    for k in keys {
        if let Some(v) = row.get(*k) {
            m.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(m)
}

/// Human-readable observed/expected pair per rule; the full row is always included too.
fn observed_expected(rule: &str, row: &Map<String, Value>) -> (Value, Value) {
    match rule.split('_').next().unwrap_or_default() {
        "R1" => (
            pick(row, &["supply_after"]),
            json!({
                "reserve_amount": row.get("reserve_amount"),
                "reserve_as_of": row.get("as_of_ts"),
                "attestor": row.get("attestor"),
            }),
        ),
        "R2" => (
            pick(row, &["owner", "amount"]),
            json!({"owner_on_allowlist": true}),
        ),
        "R3" => (
            pick(row, &["owner", "amount", "supply"]),
            json!({"max_holder_share_bps": 2000}),
        ),
        "R4" => (
            pick(row, &["change_kind", "old_value", "new_value"]),
            json!({"privileged_settings_unchanged": true}),
        ),
        _ => (Value::Object(row.clone()), Value::Null),
    }
}

/// Build the bundle for one violating row, with `result_hash` filled in and `attested_tx` null.
pub fn build(rule: &Rule, row: &Map<String, Value>, src: &Source) -> Result<Value, ProofError> {
    let mint = row
        .get("mint")
        .and_then(Value::as_str)
        .ok_or_else(|| ProofError::MissingMint {
            rule: rule.id.clone(),
        })?;
    let signature = row.get("signature").and_then(Value::as_str);
    // Event rules link the tx; holder rules (R2/R3) link the holder; anything else the mint.
    let explorer = match (signature, row.get("owner").and_then(Value::as_str)) {
        (Some(sig), _) => explorer_url(src.cluster, "tx", sig)?,
        (None, Some(owner)) => explorer_url(src.cluster, "address", owner)?,
        (None, None) => explorer_url(src.cluster, "address", mint)?,
    };
    let (observed, expected) = observed_expected(&rule.id, row);
    let mut bundle = json!({
        "version": 1,
        "rule": rule.id,
        "rule_sql_sha256": rule.sql_hash_hex(),
        "mint": mint,
        "slot": row.get("slot").cloned().unwrap_or(Value::Null),
        "signature": signature,
        "commitment": COMMITMENT,
        "observed": stringify_integers(&observed),
        "expected": stringify_integers(&expected),
        "row": stringify_integers(&Value::Object(row.clone())),
        "rpc": src.rpc,
        "cluster": src.cluster,
        "explorer": explorer,
        "attested_tx": Value::Null,
    });
    let hash = result_hash(&bundle)?;
    bundle["result_hash"] = Value::String(hex::encode(hash));
    Ok(bundle)
}

/// One bundle per row of every violation, in rule then row order.
pub fn build_all(
    rules: &[Rule],
    violations: &[Violation],
    src: &Source,
) -> Result<Vec<Value>, ProofError> {
    let mut out = Vec::new();
    for v in violations {
        let Some(rule) = rules.iter().find(|r| r.id == v.rule) else {
            continue;
        };
        for row in &v.rows {
            out.push(build(rule, row, src)?);
        }
    }
    Ok(out)
}

/// sha256 of the canonical JSON of `bundle` without `result_hash` and `attested_tx`.
pub fn result_hash(bundle: &Value) -> Result<[u8; 32], ProofError> {
    let mut m = bundle.as_object().cloned().ok_or(ProofError::NotObject)?;
    for k in UNHASHED {
        m.remove(k);
    }
    Ok(Sha256::digest(canonical_json(&Value::Object(m)).as_bytes()).into())
}

/// True if `bundle.result_hash` matches its contents.
pub fn verify(bundle: &Value) -> Result<bool, ProofError> {
    let claimed = bundle.get("result_hash").and_then(Value::as_str);
    Ok(claimed == Some(hex::encode(result_hash(bundle)?).as_str()))
}
