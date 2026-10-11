//! Input files: reserves (signed attestations), attestor keys, allowlist.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use rc_ingest::{reserve_message, verify_reserve};
use rc_types::{AllowEntry, Reserve};
use serde_json::Value;

use crate::CliError;

pub fn read(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| CliError::File {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    serde_json::from_str(&read(path)?).map_err(|e| CliError::File {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

/// Write via a temp file + rename so readers (the dashboard) never see a partial file.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), CliError> {
    let err = |e: std::io::Error| CliError::File {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn load_attestors(path: &Path) -> Result<Vec<String>, CliError> {
    parse(path)
}

pub fn load_allowlist(path: &Path, mint: &str) -> Result<Vec<AllowEntry>, CliError> {
    let all: Vec<AllowEntry> = parse(path)?;
    Ok(all.into_iter().filter(|e| e.mint == mint).collect())
}

/// (attestor, reason) for each reserve that failed verification.
pub type Rejected = Vec<(String, String)>;

/// Reserves for `mint` that verify, plus each rejected one.
pub fn load_reserves(
    path: &Path,
    mint: &str,
    attestors: &[String],
    now: i64,
) -> Result<(Vec<Reserve>, Rejected), CliError> {
    let all: Vec<Reserve> = if path.exists() {
        parse(path)?
    } else {
        Vec::new()
    };
    let mut ok = Vec::new();
    let mut rejected = Vec::new();
    for r in all.into_iter().filter(|r| r.mint == mint) {
        match verify_reserve(&r, attestors, now) {
            Ok(()) => ok.push(r),
            Err(e) => rejected.push((r.attestor.clone(), e.to_string())),
        }
    }
    Ok((ok, rejected))
}

/// Solana keypair JSON: 64 bytes, secret seed followed by public key.
fn load_signing_key(path: &Path) -> Result<SigningKey, CliError> {
    let bytes: Vec<u8> = parse(path)?;
    let bad = |reason: &str| CliError::File {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };
    let seed: [u8; 32] = bytes
        .get(..32)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| bad("not a 64-byte Solana keypair"))?;
    let key = SigningKey::from_bytes(&seed);
    if bytes.get(32..64) != Some(key.verifying_key().as_bytes().as_slice()) {
        return Err(bad("public half does not match secret seed"));
    }
    Ok(key)
}

#[allow(clippy::too_many_arguments)]
pub fn sign_reserve(
    keypair: &Path,
    mint: String,
    amount_tokens: u64,
    decimals: u32,
    source_url: String,
    as_of: Option<i64>,
    out: &Path,
) -> Result<(), CliError> {
    let key = load_signing_key(keypair)?;
    let scale = 10u64
        .checked_pow(decimals)
        .ok_or_else(|| CliError::Invalid(format!("decimals {decimals} too large")))?;
    let reserve_amount = amount_tokens
        .checked_mul(scale)
        .ok_or_else(|| CliError::Invalid("reserve amount overflows u64".into()))?;
    let mut r = Reserve {
        mint,
        as_of_ts: as_of.unwrap_or_else(now_ts),
        reserve_amount,
        attestor: bs58::encode(key.verifying_key().to_bytes()).into_string(),
        sig: String::new(),
        source_url,
    };
    let msg = reserve_message(&r).map_err(|e| CliError::Invalid(e.to_string()))?;
    r.sig = bs58::encode(key.sign(&msg).to_bytes()).into_string();

    let mut all: Vec<Value> = if out.exists() {
        parse(out)?
    } else {
        Vec::new()
    };
    all.push(serde_json::to_value(&r).map_err(|e| CliError::Invalid(e.to_string()))?);
    let body = serde_json::to_string_pretty(&all).map_err(|e| CliError::Invalid(e.to_string()))?;
    write_atomic(out, &format!("{body}\n"))?;
    println!(
        "signed reserve {} base units for {} as of {} by {} -> {}",
        r.reserve_amount,
        r.mint,
        r.as_of_ts,
        r.attestor,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair_file(dir: &Path, seed: u8) -> std::path::PathBuf {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let mut bytes = key.to_bytes().to_vec();
        bytes.extend_from_slice(key.verifying_key().as_bytes());
        let p = dir.join(format!("kp{seed}.json"));
        std::fs::write(&p, serde_json::to_string(&bytes).expect("ser")).expect("write");
        p
    }

    // Bug caught: sign-reserve and the watcher disagreeing on the signed message or units.
    #[test]
    fn signed_reserve_verifies_and_scales_by_decimals() {
        let dir = tempfile::tempdir().expect("tmp");
        let kp = keypair_file(dir.path(), 7);
        let out = dir.path().join("reserves.json");
        sign_reserve(&kp, "M".into(), 1000, 6, "u".into(), Some(100), &out).expect("sign");
        sign_reserve(&kp, "Other".into(), 5, 6, "u".into(), Some(100), &out).expect("sign");
        let attestor =
            bs58::encode(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes()).into_string();
        let (ok, rejected) = load_reserves(&out, "M", &[attestor], 200).expect("load");
        assert!(rejected.is_empty());
        assert_eq!(ok.len(), 1, "only the requested mint");
        assert_eq!(ok[0].reserve_amount, 1_000_000_000);
    }

    #[test]
    fn reserve_from_unregistered_key_is_rejected_not_used() {
        let dir = tempfile::tempdir().expect("tmp");
        let kp = keypair_file(dir.path(), 8);
        let out = dir.path().join("reserves.json");
        sign_reserve(&kp, "M".into(), 1000, 6, "u".into(), Some(100), &out).expect("sign");
        let (ok, rejected) = load_reserves(&out, "M", &["SomeOtherKey".into()], 200).expect("load");
        assert!(ok.is_empty());
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn mismatched_keypair_file_is_rejected() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("bad.json");
        std::fs::write(&p, serde_json::to_string(&vec![1u8; 64]).expect("ser")).expect("write");
        assert!(load_signing_key(&p).is_err());
    }
}
