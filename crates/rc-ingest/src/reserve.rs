use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use rc_types::Reserve;
use serde_json::Value;

use crate::ReserveError;

/// Attestations dated more than this far ahead of `now` are rejected.
pub const MAX_FUTURE_SKEW_SECS: i64 = 300;

/// The bytes an attestor signs: compact JSON of every field except `sig`, keys sorted,
/// `reserve_amount` as a decimal string. A BTreeMap keeps key order independent of
/// serde_json's `preserve_order` feature.
pub fn reserve_message(r: &Reserve) -> Result<Vec<u8>, ReserveError> {
    let mut m: BTreeMap<&str, Value> = BTreeMap::new();
    m.insert("as_of_ts", Value::from(r.as_of_ts));
    m.insert("attestor", Value::from(r.attestor.as_str()));
    m.insert("mint", Value::from(r.mint.as_str()));
    m.insert("reserve_amount", Value::from(r.reserve_amount.to_string()));
    m.insert("source_url", Value::from(r.source_url.as_str()));
    serde_json::to_vec(&m).map_err(|e| ReserveError::Encode(e.to_string()))
}

/// Accept a reserve only if the attestor is registered, the base58 ed25519 signature over
/// [`reserve_message`] verifies (strict), and it isn't dated in the future.
pub fn verify_reserve(r: &Reserve, attestors: &[String], now: i64) -> Result<(), ReserveError> {
    if !attestors.iter().any(|a| a == &r.attestor) {
        return Err(ReserveError::UnknownAttestor(r.attestor.clone()));
    }
    let key_bytes: [u8; 32] = bs58::decode(&r.attestor)
        .into_vec()
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ReserveError::BadKey(r.attestor.clone()))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| ReserveError::BadKey(r.attestor.clone()))?;
    let sig_bytes: [u8; 64] = bs58::decode(&r.sig)
        .into_vec()
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(ReserveError::BadSignatureEncoding)?;
    let sig = Signature::from_bytes(&sig_bytes);
    let msg = reserve_message(r)?;
    key.verify_strict(&msg, &sig)
        .map_err(|_| ReserveError::BadSignature)?;
    if r.as_of_ts > now.saturating_add(MAX_FUTURE_SKEW_SECS) {
        return Err(ReserveError::FromFuture {
            as_of_ts: r.as_of_ts,
            now,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(seed: u8, mutate: impl FnOnce(&mut Reserve)) -> (Reserve, String) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let attestor = bs58::encode(sk.verifying_key().to_bytes()).into_string();
        let mut r = Reserve {
            mint: "CceRcJNKkxWuSZxJWeWKunSkHH3i8njpHa7dYoJcBBUp".into(),
            as_of_ts: 1_791_676_800,
            reserve_amount: 1_000_000_000,
            attestor: attestor.clone(),
            sig: String::new(),
            source_url: "https://example.invalid/r.json".into(),
        };
        let msg = reserve_message(&r).expect("encodes");
        r.sig = bs58::encode(sk.sign(&msg).to_bytes()).into_string();
        mutate(&mut r);
        (r, attestor)
    }

    const NOW: i64 = 1_791_700_000;

    #[test]
    fn message_is_sorted_compact_json_with_string_amount() {
        let (r, a) = signed(1, |_| {});
        let msg = String::from_utf8(reserve_message(&r).expect("encodes")).expect("utf8");
        assert_eq!(
            msg,
            format!(
                r#"{{"as_of_ts":1791676800,"attestor":"{a}","mint":"CceRcJNKkxWuSZxJWeWKunSkHH3i8njpHa7dYoJcBBUp","reserve_amount":"1000000000","source_url":"https://example.invalid/r.json"}}"#
            )
        );
    }

    #[test]
    fn valid_signature_from_registered_attestor_is_accepted() {
        let (r, a) = signed(1, |_| {});
        assert_eq!(verify_reserve(&r, &[a], NOW), Ok(()));
    }

    // Bug caught: amount not covered by the signature (replaying a signature on a bigger reserve).
    #[test]
    fn tampered_amount_is_rejected() {
        let (r, a) = signed(1, |r| r.reserve_amount += 1);
        assert_eq!(
            verify_reserve(&r, &[a], NOW),
            Err(ReserveError::BadSignature)
        );
    }

    // Bug caught: a valid signature from a key that is not a registered attestor.
    #[test]
    fn unregistered_attestor_is_rejected() {
        let (r, _) = signed(2, |_| {});
        let (_, registered) = signed(1, |_| {});
        assert!(matches!(
            verify_reserve(&r, &[registered], NOW),
            Err(ReserveError::UnknownAttestor(_))
        ));
    }

    #[test]
    fn garbage_signature_is_rejected() {
        let (r, a) = signed(1, |r| r.sig = "not-base58-!!".into());
        assert_eq!(
            verify_reserve(&r, &[a], NOW),
            Err(ReserveError::BadSignatureEncoding)
        );
    }

    // Bug caught: a pre-dated attestation used to cover future mints.
    #[test]
    fn future_dated_reserve_is_rejected() {
        let (r, a) = signed(1, |_| {});
        let early = r.as_of_ts - MAX_FUTURE_SKEW_SECS - 1;
        assert!(matches!(
            verify_reserve(&r, &[a], early),
            Err(ReserveError::FromFuture { .. })
        ));
    }
}
