//! `rc watch`: poll finalized history, run the rules, write proofs and `status.json`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Args;
use rc_engine::{load_rules, Engine, Rule, Violation};
use rc_ingest::{HttpRpc, Ingester, PollReport};
use rc_proof::{build_all, Source};
use rc_types::Reserve;
use serde_json::{json, Value};

use crate::inputs::{load_allowlist, load_attestors, load_reserves, now_ts, write_atomic};
use crate::sink::{post_webhook, print_alert, write_proof};
use crate::CliError;

#[derive(Args, Debug, Clone)]
pub struct WatchArgs {
    #[arg(long)]
    pub mint: String,
    #[arg(long, default_value = "https://api.devnet.solana.com")]
    pub rpc: String,
    #[arg(long, default_value = "devnet")]
    pub cluster: String,
    #[arg(long, default_value = "rules")]
    pub rules: PathBuf,
    /// Signed reserve attestations (JSON array); re-read every poll.
    #[arg(long)]
    pub reserves: PathBuf,
    /// Registered attestor public keys (JSON array of base58).
    #[arg(long)]
    pub attestors: PathBuf,
    /// Allowlist entries (JSON array of {mint, owner}); re-read every poll.
    #[arg(long)]
    pub allowlist: PathBuf,
    /// Directory for proof bundles and status.json.
    #[arg(long, default_value = "proofs")]
    pub out: PathBuf,
    #[arg(long, default_value_t = 5)]
    pub interval: u64,
    /// Token decimals, for display only (amounts stay in base units everywhere else).
    #[arg(long, default_value_t = 6)]
    pub decimals: u8,
    /// Reserve older than this many days is flagged stale.
    #[arg(long, default_value_t = 7)]
    pub stale_days: i64,
    /// POST each new proof bundle here.
    #[arg(long)]
    pub webhook: Option<String>,
    /// Poll once and exit (non-zero on error).
    #[arg(long)]
    pub once: bool,
    /// Post each new proof's result hash on-chain (rc-attest).
    #[cfg(feature = "attest")]
    #[arg(long)]
    pub attest: bool,
    #[cfg(feature = "attest")]
    #[command(flatten)]
    pub chain: crate::attest::ChainArgs,
}

/// Attestation is optional; without the feature or the flag, proofs stay off-chain.
#[cfg(feature = "attest")]
type MaybeAttestor = Option<crate::attest::Attestor>;
#[cfg(not(feature = "attest"))]
type MaybeAttestor = Option<()>;

fn attestor(args: &WatchArgs) -> Result<MaybeAttestor, CliError> {
    #[cfg(feature = "attest")]
    if args.attest {
        return Ok(Some(crate::attest::Attestor::new(&args.chain)?));
    }
    let _ = args;
    Ok(None)
}

pub async fn run(args: WatchArgs) -> Result<(), CliError> {
    let rules = load_rules(&args.rules)?;
    let attestors = load_attestors(&args.attestors)?;
    let rpc = HttpRpc::new(args.rpc.clone())?;
    let mut ingester = Ingester::new(rpc, args.mint.clone());
    let webhook_client = reqwest::Client::new();
    let attestor = attestor(&args)?;
    println!(
        "watching {} on {} ({} rules, finalized, every {}s)",
        args.mint,
        args.cluster,
        rules.len(),
        args.interval
    );
    loop {
        let started = Instant::now();
        match tick(
            &args,
            &rules,
            &attestors,
            &mut ingester,
            &webhook_client,
            &attestor,
        )
        .await
        {
            Ok(()) => {}
            Err(e) if !args.once => eprintln!("poll failed, retrying: {e}"),
            Err(e) => return Err(e),
        }
        if args.once {
            return Ok(());
        }
        let wait = Duration::from_secs(args.interval).saturating_sub(started.elapsed());
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = tokio::signal::ctrl_c() => { println!("stopped"); return Ok(()); }
        }
    }
}

async fn tick(
    args: &WatchArgs,
    rules: &[Rule],
    attestors: &[String],
    ingester: &mut Ingester<HttpRpc>,
    webhook: &reqwest::Client,
    attestor: &MaybeAttestor,
) -> Result<(), CliError> {
    let report = ingester.poll().await?;
    let now = now_ts();
    let (reserves, rejected) = load_reserves(&args.reserves, &args.mint, attestors, now)?;
    let mut reg = ingester.register().clone();
    reg.reserves = reserves.clone();
    reg.allowlist = load_allowlist(&args.allowlist, &args.mint)?;

    let engine = Engine::new(&reg, rules.to_vec()).await?;
    let results = engine.run_all().await?;
    let violations: Vec<Violation> = results.iter().filter(|v| !v.passed()).cloned().collect();
    let rpc = rpc_label(&args.rpc);
    let src = Source {
        rpc: &rpc,
        cluster: &args.cluster,
    };
    let bundles = build_all(rules, &violations, &src)?;
    #[cfg(not(feature = "attest"))]
    let _ = attestor;

    let mut proof_files: Vec<(String, String)> = Vec::new();
    for b in &bundles {
        let (path, is_new) = write_proof(&args.out, b)?;
        if is_new {
            print_alert(b, &path);
            if let Some(url) = &args.webhook {
                post_webhook(webhook, url, b).await;
            }
        }
        // Attest new proofs, and retry earlier ones whose attestation failed (attested_tx null).
        #[cfg(feature = "attest")]
        if let Some(att) = attestor {
            if is_new || crate::sink::needs_attestation(&path) {
                // A failed attestation is reported, not fatal: the proof file is already written.
                if let Err(e) = crate::attest::attest_file(att, &path).await {
                    eprintln!(
                        "attest {} failed (will retry next poll): {e}",
                        path.display()
                    );
                }
            }
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        proof_files.push((b["rule"].as_str().unwrap_or_default().to_string(), name));
    }

    let latest = reserves
        .iter()
        .max_by_key(|r| (r.as_of_ts, std::cmp::Reverse(r.reserve_amount)));
    let status = status_json(
        args,
        rules,
        &results,
        &proof_files,
        &report,
        latest,
        &rejected,
        now,
    );
    let body =
        serde_json::to_string_pretty(&status).map_err(|e| CliError::Invalid(e.to_string()))?;
    write_atomic(&args.out.join("status.json"), &format!("{body}\n"))?;
    print_summary(&report, &results, latest, &rejected);
    if let Some(problem) = reserve_problem(
        latest,
        &rejected,
        now,
        args.stale_days.saturating_mul(86_400),
    ) {
        println!("  R0 RESERVE EVIDENCE FAIL: {problem}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn status_json(
    args: &WatchArgs,
    rules: &[Rule],
    results: &[Violation],
    proof_files: &[(String, String)],
    report: &PollReport,
    latest: Option<&Reserve>,
    rejected: &[(String, String)],
    now: i64,
) -> Value {
    let mut rule_rows: Vec<Value> = Vec::new();
    let stale_after = args.stale_days.saturating_mul(86_400);
    // Without valid, fresh reserve evidence R1 would pass vacuously; make that a failing check.
    if let Some(problem) = reserve_problem(latest, rejected, now, stale_after) {
        rule_rows.push(json!({
            "id": "R0_reserve_evidence",
            "passed": false,
            "violations": 1,
            "detail": problem,
            "proofs": [],
        }));
    }
    if let Some(d) = report.drift {
        rule_rows.push(json!({
            "id": "R0_supply_drift",
            "passed": false,
            "violations": 1,
            "detail": {"onchain": d.onchain.to_string(), "computed": d.computed.to_string(), "slot": d.slot},
            "proofs": [],
        }));
    }
    if !report.balance_drift.is_empty() {
        rule_rows.push(json!({
            "id": "R0_balance_drift",
            "passed": false,
            "violations": report.balance_drift.len(),
            "detail": report.balance_drift.iter().map(|d| format!(
                "account {} onchain {} computed {}",
                d.account,
                d.onchain.map_or("closed".to_string(), |a| a.to_string()),
                d.computed
            )).collect::<Vec<_>>().join("; "),
            "proofs": [],
        }));
    }
    for v in results {
        let sql_sha256 = rules
            .iter()
            .find(|r| r.id == v.rule)
            .map(|r| r.sql_hash_hex())
            .unwrap_or_default();
        let proofs: Vec<&str> = proof_files
            .iter()
            .filter(|(r, _)| r == &v.rule)
            .map(|(_, f)| f.as_str())
            .collect();
        rule_rows.push(json!({
            "id": v.rule,
            "passed": v.passed(),
            "violations": v.rows.len(),
            "sql_sha256": sql_sha256,
            "proofs": proofs,
        }));
    }
    json!({
        "version": 1,
        "mint": args.mint,
        "cluster": args.cluster,
        "rpc": rpc_label(&args.rpc),
        "commitment": rc_proof::COMMITMENT,
        "updated_at": now,
        "poll_interval_secs": args.interval,
        "decimals": args.decimals,
        "finalized_slot": report.supply.context_slot,
        "supply": report.supply.amount.to_string(),
        "computed_supply": report.computed_supply.to_string(),
        "pending_signatures": report.pending,
        "unparsed": report.unparsed.iter().map(|(s, t)| json!({"signature": s, "type": t})).collect::<Vec<_>>(),
        "reserve": latest.map(|r| json!({
            "amount": r.reserve_amount.to_string(),
            "as_of_ts": r.as_of_ts,
            "attestor": r.attestor,
            "source_url": r.source_url,
            "stale": now.saturating_sub(r.as_of_ts) > stale_after,
        })),
        "rejected_reserves": rejected.iter().map(|(a, why)| json!({"attestor": a, "reason": why})).collect::<Vec<_>>(),
        "rules": rule_rows,
    })
}

fn print_summary(
    report: &PollReport,
    results: &[Violation],
    latest: Option<&Reserve>,
    rejected: &[(String, String)],
) {
    let rules: Vec<String> = results
        .iter()
        .map(|v| {
            let tag = v.rule.split('_').next().unwrap_or("?");
            if v.passed() {
                format!("{tag} ok")
            } else {
                format!("{tag} FAIL({})", v.rows.len())
            }
        })
        .collect();
    let reserve = latest.map_or("NONE".to_string(), |r| r.reserve_amount.to_string());
    println!(
        "slot {} supply {} reserve {} new_tx {} | {}{}",
        report.supply.context_slot,
        report.supply.amount,
        reserve,
        report.new_signatures,
        rules.join(" | "),
        report
            .drift
            .map(|d| format!(
                " | R0 SUPPLY DRIFT onchain={} computed={}",
                d.onchain, d.computed
            ))
            .unwrap_or_default(),
    );
    for (a, why) in rejected {
        println!("  rejected reserve from {a}: {why}");
    }
    for d in &report.balance_drift {
        println!(
            "  R0 BALANCE DRIFT account {} onchain {:?} computed {} (token movement not in mint history)",
            d.account, d.onchain, d.computed
        );
    }
    for (s, t) in &report.unparsed {
        println!("  WARNING unparsed Token-2022 instruction {t} in {s}");
    }
}

/// RPC URL without query string or credentials: provider API keys (`?api-key=`) must not end up in
/// proofs, status.json or webhooks.
pub fn rpc_label(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or_default();
    match no_query.split_once("://") {
        Some((scheme, rest)) => {
            let host_path = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
            format!("{scheme}://{host_path}")
        }
        None => no_query.to_string(),
    }
}

/// Why the reserve evidence can't back R1, if it can't.
pub fn reserve_problem(
    latest: Option<&Reserve>,
    rejected: &[(String, String)],
    now: i64,
    stale_after: i64,
) -> Option<String> {
    let mut problems = Vec::new();
    match latest {
        None => problems.push("no valid reserve attestation: R1 cannot be evaluated".to_string()),
        Some(r) if now.saturating_sub(r.as_of_ts) > stale_after => problems.push(format!(
            "latest reserve attestation is {} days old",
            now.saturating_sub(r.as_of_ts) / 86_400
        )),
        Some(_) => {}
    }
    if !rejected.is_empty() {
        let reasons: Vec<&str> = rejected.iter().map(|(_, why)| why.as_str()).collect();
        problems.push(format!(
            "{} reserve attestation(s) rejected: {}",
            rejected.len(),
            reasons.join("; ")
        ));
    }
    (!problems.is_empty()).then(|| problems.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_label_strips_api_keys_and_credentials() {
        assert_eq!(
            rpc_label("https://devnet.helius-rpc.com/?api-key=SECRET"),
            "https://devnet.helius-rpc.com/"
        );
        assert_eq!(
            rpc_label("https://user:pw@rpc.example.com/x?k=1#f"),
            "https://rpc.example.com/x"
        );
        assert_eq!(
            rpc_label("https://api.devnet.solana.com"),
            "https://api.devnet.solana.com"
        );
    }

    fn reserve(as_of_ts: i64) -> Reserve {
        Reserve {
            mint: "M".into(),
            as_of_ts,
            reserve_amount: 1,
            attestor: "A".into(),
            sig: "s".into(),
            source_url: "u".into(),
        }
    }

    // Bug caught: R1 passing vacuously (dashboard green) with no, stale, or rejected reserve evidence.
    #[test]
    fn missing_stale_or_rejected_reserve_is_a_problem() {
        let day = 86_400;
        assert!(reserve_problem(None, &[], 0, 7 * day).is_some());
        assert!(reserve_problem(Some(&reserve(0)), &[], 8 * day, 7 * day).is_some());
        assert!(reserve_problem(
            Some(&reserve(0)),
            &[("A".into(), "bad sig".into())],
            0,
            7 * day
        )
        .is_some());
        assert_eq!(reserve_problem(Some(&reserve(0)), &[], day, 7 * day), None);
    }
}
