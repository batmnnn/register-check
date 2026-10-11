use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("http error calling {method}: {source}")]
    Http {
        method: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("rpc {method} returned error: {message}")]
    Rpc {
        method: &'static str,
        message: String,
    },
    #[error("rpc {method} still rate-limited after {attempts} attempts")]
    RateLimited { method: &'static str, attempts: u32 },
    #[error("unexpected rpc response for {method}: {reason}")]
    Response {
        method: &'static str,
        reason: String,
    },
    #[error("tx {signature}: {reason}")]
    Parse { signature: String, reason: String },
    #[error("supply overflow at tx {signature}")]
    SupplyOverflow { signature: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReserveError {
    #[error("attestor {0} is not a registered attestor key")]
    UnknownAttestor(String),
    #[error("attestor key is not valid base58 ed25519: {0}")]
    BadKey(String),
    #[error("signature is not valid base58 ed25519")]
    BadSignatureEncoding,
    #[error("signature does not verify")]
    BadSignature,
    #[error("as_of_ts {as_of_ts} is in the future (now {now})")]
    FromFuture { as_of_ts: i64, now: i64 },
    #[error("encoding reserve message: {0}")]
    Encode(String),
}
