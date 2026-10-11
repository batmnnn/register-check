//! Outputs: console lines, proof files, optional webhook.

use std::path::{Path, PathBuf};

use rc_engine::Violation;
use serde_json::Value;

use crate::inputs::write_atomic;
use crate::CliError;

pub fn print_rule_line(v: &Violation) {
    if v.passed() {
        println!("  PASS {}", v.rule);
    } else {
        println!("  FAIL {} ({} row(s))", v.rule, v.rows.len());
    }
}

/// `<rule>-<first 12 hex of result_hash>.json`; same evidence → same file name.
pub fn proof_path(dir: &Path, bundle: &Value) -> PathBuf {
    let rule = bundle["rule"].as_str().unwrap_or("unknown");
    let hash = bundle["result_hash"].as_str().unwrap_or("");
    dir.join(format!("{rule}-{}.json", &hash[..hash.len().min(12)]))
}

/// Write the bundle unless it already exists. Returns true if it is new.
pub fn write_proof(dir: &Path, bundle: &Value) -> Result<(PathBuf, bool), CliError> {
    let path = proof_path(dir, bundle);
    if path.exists() {
        return Ok((path, false));
    }
    let body =
        serde_json::to_string_pretty(bundle).map_err(|e| CliError::Invalid(e.to_string()))?;
    write_atomic(&path, &format!("{body}\n"))?;
    Ok((path, true))
}

pub fn print_alert(bundle: &Value, path: &Path) {
    println!(
        "ALERT {} mint={} slot={} sig={}\n      observed={} expected={}\n      proof={} hash={}\n      {}",
        bundle["rule"].as_str().unwrap_or("?"),
        bundle["mint"].as_str().unwrap_or("?"),
        bundle["slot"],
        bundle["signature"].as_str().unwrap_or("-"),
        bundle["observed"],
        bundle["expected"],
        path.display(),
        bundle["result_hash"].as_str().unwrap_or("?"),
        bundle["explorer"].as_str().unwrap_or(""),
    );
}

/// Best effort: a failing webhook is reported but never stops the watcher.
pub async fn post_webhook(client: &reqwest::Client, url: &str, bundle: &Value) {
    let res = client
        .post(url)
        .header("content-type", "application/json")
        .body(bundle.to_string())
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => eprintln!("webhook {url} returned {}", r.status()),
        Err(e) => eprintln!("webhook {url} failed: {e}"),
    }
}
