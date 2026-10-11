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
    let src = Source {
        rpc: &args.rpc,
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
            #[cfg(feature = "attest")]
            if let Some(att) = attestor {
                // A failed attestation is reported, not fatal: the proof file is already written.
                if let Err(e) = crate::attest::attest_file(att, &path).await {
                    eprintln!("attest {} failed: {e}", path.display());
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
    if let Some(d) = report.drift {
        rule_rows.push(json!({
            "id": "R0_supply_drift",
            "passed": false,
            "violations": 1,
            "detail": {"onchain": d.onchain.to_string(), "computed": d.computed.to_string(), "slot": d.slot},
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
    let stale_after = args.stale_days.saturating_mul(86_400);
    json!({
        "version": 1,
        "mint": args.mint,
        "cluster": args.cluster,
        "rpc": args.rpc,
        "commitment": rc_proof::COMMITMENT,
        "updated_at": now,
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
    let reserve = latest.map_or("none".to_string(), |r| r.reserve_amount.to_string());
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
    for (s, t) in &report.unparsed {
        println!("  WARNING unparsed Token-2022 instruction {t} in {s}");
    }
}
