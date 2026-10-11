//! `rc`: the Register Check command line.

#[cfg(feature = "attest")]
mod attest;
mod inputs;
mod sink;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rc_engine::{load_rules, Engine};
use rc_proof::{build_all, Source};
use rc_types::Register;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{path}: {reason}")]
    File { path: PathBuf, reason: String },
    #[error(transparent)]
    Engine(#[from] rc_engine::EngineError),
    #[error(transparent)]
    Ingest(#[from] rc_ingest::IngestError),
    #[error(transparent)]
    Proof(#[from] rc_proof::ProofError),
    #[error(transparent)]
    Types(#[from] rc_types::TypesError),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Parser)]
#[command(
    name = "rc",
    version,
    about = "Register Check: continuous register rules for Token-2022 mints"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the rules once over a register JSON file (offline).
    Check {
        #[arg(long)]
        register: PathBuf,
        #[arg(long, default_value = "rules")]
        rules: PathBuf,
        #[arg(long, default_value = "devnet")]
        cluster: String,
    },
    /// Sign a reserve attestation with an attestor keypair and append it to a reserves file.
    SignReserve {
        /// Solana keypair JSON of the attestor.
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        mint: String,
        /// Reserve in whole tokens (e.g. 1000).
        #[arg(long)]
        amount: u64,
        #[arg(long, default_value_t = 6)]
        decimals: u32,
        #[arg(long, default_value = "https://example.invalid/reserve-report.json")]
        source_url: String,
        /// Unix seconds; defaults to now.
        #[arg(long)]
        as_of: Option<i64>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Follow a mint: poll finalized txs, run rules, write proofs + status.json.
    Watch(watch::WatchArgs),
    /// Post proof bundles' result hashes to the rc-attest program.
    #[cfg(feature = "attest")]
    Attest(attest::AttestArgs),
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Check {
            register,
            rules,
            cluster,
        } => check(register, rules, cluster).await,
        Cmd::SignReserve {
            keypair,
            mint,
            amount,
            decimals,
            source_url,
            as_of,
            out,
        } => inputs::sign_reserve(&keypair, mint, amount, decimals, source_url, as_of, &out),
        Cmd::Watch(args) => watch::run(args).await,
        #[cfg(feature = "attest")]
        Cmd::Attest(args) => attest::run(args).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn check(register: PathBuf, rules: PathBuf, cluster: String) -> Result<(), CliError> {
    let reg = Register::from_json(&inputs::read(&register)?)?;
    let rules = load_rules(&rules)?;
    let engine = Engine::new(&reg, rules.clone()).await?;
    let results = engine.run_all().await?;
    for v in &results {
        sink::print_rule_line(v);
    }
    let violations: Vec<_> = results.into_iter().filter(|v| !v.passed()).collect();
    let src = Source {
        rpc: "offline",
        cluster: &cluster,
    };
    for b in build_all(&rules, &violations, &src)? {
        println!("{}", serde_json::to_string_pretty(&b).unwrap_or_default());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    // Bug caught: conflicting argument names (e.g. two `cluster` flags via flatten) that only
    // panic when the binary starts.
    #[test]
    fn cli_definition_is_consistent() {
        super::Cli::command().debug_assert();
    }
}
