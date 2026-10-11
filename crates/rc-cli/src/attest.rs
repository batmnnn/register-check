//! On-chain attestation of proof bundles via the rc-attest program.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anchor_client::anchor_lang::prelude::Pubkey;
use anchor_client::anchor_lang::system_program;
use anchor_client::{Client, Cluster, CommitmentConfig, Signer};
use clap::Args;
use rc_attest::{CheckResult, Config, Rule, CONFIG_SEED, RESULT_SEED, RULE_SEED};
use serde_json::Value;
use solana_keypair::Keypair;

use crate::inputs::{read, write_atomic};
use crate::CliError;

pub const DEFAULT_PROGRAM_ID: &str = "BNADNdmc5zJF54Tj6o1ctr3PmhjjDFPByPGGU3oLthV3";

#[derive(Args, Debug, Clone)]
pub struct AttestArgs {
    /// Proof bundle files to attest.
    #[arg(required = true)]
    pub proofs: Vec<PathBuf>,
    #[command(flatten)]
    pub chain: ChainArgs,
}

#[derive(Args, Debug, Clone)]
pub struct ChainArgs {
    #[arg(long, default_value = DEFAULT_PROGRAM_ID)]
    pub program: String,
    /// Attestation authority keypair (pays fees; must match the Config authority).
    #[arg(long, default_value = "~/.config/solana/id.json")]
    pub keypair: String,
    /// devnet, testnet, mainnet, localnet, or an RPC URL.
    #[arg(long, default_value = "devnet")]
    pub attest_cluster: String,
}

fn invalid(e: impl std::fmt::Display) -> CliError {
    CliError::Invalid(e.to_string())
}

fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(p),
    }
}

fn load_keypair(path: &str) -> Result<Keypair, CliError> {
    let path = expand_home(path);
    let bytes: Vec<u8> = serde_json::from_str(&read(&path)?).map_err(|e| CliError::File {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    Keypair::try_from(bytes.as_slice()).map_err(|e| CliError::File {
        path,
        reason: e.to_string(),
    })
}

fn cluster(s: &str) -> Result<Cluster, CliError> {
    if s.starts_with("http") {
        let ws = s.replacen("http", "ws", 1);
        return Ok(Cluster::Custom(s.to_string(), ws));
    }
    Cluster::from_str(s).map_err(invalid)
}

fn hex32(s: &str, what: &str) -> Result<[u8; 32], CliError> {
    let mut out = [0u8; 32];
    if s.len() != 64 {
        return Err(CliError::Invalid(format!("{what} must be 64 hex chars")));
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16)
            .map_err(|_| CliError::Invalid(format!("{what} is not hex")))?;
    }
    Ok(out)
}

/// Rule id as stored on-chain: UTF-8, zero-padded to 32 bytes.
pub fn rule_id_bytes(id: &str) -> Result<[u8; 32], CliError> {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > 32 {
        return Err(CliError::Invalid(format!(
            "rule id {id:?} must be 1-32 bytes"
        )));
    }
    let mut out = [0u8; 32];
    out[..b.len()].copy_from_slice(b);
    Ok(out)
}

pub struct Attestor {
    program: anchor_client::Program<Arc<Keypair>>,
    authority: Pubkey,
    cluster_name: String,
}

impl Attestor {
    pub fn new(args: &ChainArgs) -> Result<Self, CliError> {
        let payer = Arc::new(load_keypair(&args.keypair)?);
        let authority = payer.pubkey();
        let client = Client::new_with_options(
            cluster(&args.attest_cluster)?,
            payer,
            CommitmentConfig::confirmed(),
        );
        let program_id = Pubkey::from_str(&args.program).map_err(invalid)?;
        let program = client.program(program_id).map_err(invalid)?;
        Ok(Self {
            program,
            authority,
            cluster_name: args.attest_cluster.clone(),
        })
    }

    fn pda(&self, seeds: &[&[u8]]) -> Pubkey {
        Pubkey::find_program_address(seeds, &self.program.id()).0
    }

    async fn exists<T: anchor_client::anchor_lang::AccountDeserialize>(&self, key: Pubkey) -> bool {
        self.program.account::<T>(key).await.is_ok()
    }

    /// Create the Config (authority = our keypair) if it doesn't exist; refuse a foreign authority.
    pub async fn ensure_config(&self) -> Result<Pubkey, CliError> {
        let config = self.pda(&[CONFIG_SEED]);
        match self.program.account::<Config>(config).await {
            Ok(c) if c.authority == self.authority => return Ok(config),
            Ok(c) => {
                return Err(CliError::Invalid(format!(
                    "config authority is {}, not our key {}",
                    c.authority, self.authority
                )))
            }
            Err(_) => {}
        }
        let sig = self
            .program
            .request()
            .accounts(rc_attest::accounts::InitConfig {
                config,
                authority: self.authority,
                system_program: system_program::ID,
            })
            .args(rc_attest::instruction::InitConfig {})
            .send()
            .await
            .map_err(invalid)?;
        println!("init_config {sig}");
        Ok(config)
    }

    pub async fn ensure_rule(
        &self,
        config: Pubkey,
        id: [u8; 32],
        sql_hash: [u8; 32],
    ) -> Result<Pubkey, CliError> {
        let rule = self.pda(&[RULE_SEED, &id, &sql_hash]);
        if self.exists::<Rule>(rule).await {
            return Ok(rule);
        }
        let sig = self
            .program
            .request()
            .accounts(rc_attest::accounts::RegisterRule {
                config,
                rule,
                authority: self.authority,
                system_program: system_program::ID,
            })
            .args(rc_attest::instruction::RegisterRule { id, sql_hash })
            .send()
            .await
            .map_err(invalid)?;
        println!("register_rule {sig}");
        Ok(rule)
    }

    /// Post the bundle's result hash; returns the tx signature (or None if already on-chain).
    pub async fn attest_bundle(&self, bundle: &Value) -> Result<Option<String>, CliError> {
        if !rc_proof::verify(bundle)? {
            return Err(CliError::Invalid(
                "bundle result_hash does not match contents".into(),
            ));
        }
        let field = |k: &str| {
            bundle[k]
                .as_str()
                .ok_or_else(|| CliError::Invalid(format!("bundle has no {k}")))
        };
        let id = rule_id_bytes(field("rule")?)?;
        let sql_hash = hex32(field("rule_sql_sha256")?, "rule_sql_sha256")?;
        let result_hash = hex32(field("result_hash")?, "result_hash")?;
        let slot = bundle["slot"]
            .as_u64()
            .or_else(|| bundle["slot"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);

        let config = self.ensure_config().await?;
        let rule = self.ensure_rule(config, id, sql_hash).await?;
        let result = self.pda(&[RESULT_SEED, rule.as_ref(), &result_hash]);
        if self.exists::<CheckResult>(result).await {
            return Ok(None);
        }
        let sig = self
            .program
            .request()
            .accounts(rc_attest::accounts::PostResult {
                config,
                rule,
                result,
                authority: self.authority,
                system_program: system_program::ID,
            })
            .args(rc_attest::instruction::PostResult {
                slot,
                result_hash,
                violated: true,
            })
            .send()
            .await
            .map_err(invalid)?;
        Ok(Some(sig.to_string()))
    }

    pub fn explorer(&self, sig: &str) -> String {
        match self.cluster_name.as_str() {
            "mainnet" | "mainnet-beta" => format!("https://explorer.solana.com/tx/{sig}"),
            c if c.starts_with("http") => {
                format!("https://explorer.solana.com/tx/{sig}?cluster=custom")
            }
            c => format!("https://explorer.solana.com/tx/{sig}?cluster={c}"),
        }
    }
}

/// Attest a proof file and record `attested_tx` in it (excluded from the hash, so it still verifies).
pub async fn attest_file(att: &Attestor, path: &Path) -> Result<(), CliError> {
    let mut bundle: Value = serde_json::from_str(&read(path)?).map_err(|e| CliError::File {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if !bundle["attested_tx"].is_null() {
        println!(
            "already attested {} ({})",
            path.display(),
            bundle["attested_tx"]
        );
        return Ok(());
    }
    match att.attest_bundle(&bundle).await? {
        Some(sig) => {
            println!(
                "ATTESTED {} tx={sig}\n      {}",
                path.display(),
                att.explorer(&sig)
            );
            bundle["attested_tx"] = Value::String(sig);
            let body = serde_json::to_string_pretty(&bundle).map_err(invalid)?;
            write_atomic(path, &format!("{body}\n"))?;
        }
        None => println!("result for {} already on-chain", path.display()),
    }
    Ok(())
}

pub async fn run(args: AttestArgs) -> Result<(), CliError> {
    let att = Attestor::new(&args.chain)?;
    for p in &args.proofs {
        attest_file(&att, p).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_id_is_zero_padded_and_bounded() {
        let id = rule_id_bytes("R1_unbacked_mint").expect("ok");
        assert_eq!(&id[..16], b"R1_unbacked_mint");
        assert!(id[16..].iter().all(|b| *b == 0));
        assert!(rule_id_bytes("").is_err());
        assert!(rule_id_bytes(&"x".repeat(33)).is_err());
    }

    #[test]
    fn hex32_round_trip_and_rejects_bad_input() {
        let h = "44b3f86a92801aab24617944266b19662c51264b73fa79e74d0c8234ceaadccc";
        let b = hex32(h, "h").expect("ok");
        assert_eq!(b[0], 0x44);
        assert_eq!(b[31], 0xcc);
        assert!(hex32(&h[..62], "h").is_err());
        assert!(hex32(&h.replace('4', "z"), "h").is_err());
    }
}
