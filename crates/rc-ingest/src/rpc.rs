use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::IngestError;

/// One entry from `getSignaturesForAddress` (newest first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigInfo {
    pub signature: String,
    pub slot: u64,
    pub failed: bool,
}

/// `getTokenSupply` amount plus the slot it was read at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSupply {
    pub amount: u64,
    pub context_slot: u64,
}

/// Every request is made at `finalized` commitment.
#[async_trait]
pub trait RpcSource: Send + Sync {
    /// Up to 1000 signatures older than `before` (newest first).
    async fn signatures(
        &self,
        address: &str,
        before: Option<&str>,
    ) -> Result<Vec<SigInfo>, IngestError>;
    /// `getTransaction` result (jsonParsed, maxSupportedTransactionVersion 0); `None` if unknown.
    async fn transaction(&self, signature: &str) -> Result<Option<Value>, IngestError>;
    async fn token_supply(&self, mint: &str) -> Result<TokenSupply, IngestError>;
}

/// JSON-RPC over HTTP with backoff on HTTP 429 (public devnet rate-limits bursts).
pub struct HttpRpc {
    url: String,
    client: reqwest::Client,
    max_attempts: u32,
}

impl HttpRpc {
    pub fn new(url: impl Into<String>) -> Result<Self, IngestError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|source| IngestError::Http {
                method: "client",
                source,
            })?;
        Ok(Self {
            url: url.into(),
            client,
            max_attempts: 6,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    async fn call(&self, method: &'static str, params: Value) -> Result<Value, IngestError> {
        let body =
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        let http = |source| IngestError::Http { method, source };
        for attempt in 0..self.max_attempts {
            let resp = self
                .client
                .post(&self.url)
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await
                .map_err(http)?;
            if resp.status().as_u16() == 429 {
                tokio::time::sleep(Duration::from_millis(500u64 << attempt)).await;
                continue;
            }
            let text = resp
                .error_for_status()
                .map_err(http)?
                .text()
                .await
                .map_err(http)?;
            let mut v: Value = serde_json::from_str(&text).map_err(|e| IngestError::Response {
                method,
                reason: e.to_string(),
            })?;
            if let Some(err) = v.get("error") {
                return Err(IngestError::Rpc {
                    method,
                    message: err.to_string(),
                });
            }
            return Ok(v["result"].take());
        }
        Err(IngestError::RateLimited {
            method,
            attempts: self.max_attempts,
        })
    }
}

fn bad(method: &'static str, reason: &str) -> IngestError {
    IngestError::Response {
        method,
        reason: reason.to_string(),
    }
}

pub(crate) fn parse_signatures(result: &Value) -> Result<Vec<SigInfo>, IngestError> {
    const M: &str = "getSignaturesForAddress";
    let arr = result
        .as_array()
        .ok_or_else(|| bad(M, "result is not an array"))?;
    arr.iter()
        .map(|e| {
            Ok(SigInfo {
                signature: e["signature"]
                    .as_str()
                    .ok_or_else(|| bad(M, "missing signature"))?
                    .to_string(),
                slot: e["slot"].as_u64().ok_or_else(|| bad(M, "missing slot"))?,
                failed: !e["err"].is_null(),
            })
        })
        .collect()
}

pub(crate) fn parse_token_supply(result: &Value) -> Result<TokenSupply, IngestError> {
    const M: &str = "getTokenSupply";
    let amount = result["value"]["amount"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| bad(M, "missing value.amount"))?;
    let context_slot = result["context"]["slot"]
        .as_u64()
        .ok_or_else(|| bad(M, "missing context.slot"))?;
    Ok(TokenSupply {
        amount,
        context_slot,
    })
}

#[async_trait]
impl RpcSource for HttpRpc {
    async fn signatures(
        &self,
        address: &str,
        before: Option<&str>,
    ) -> Result<Vec<SigInfo>, IngestError> {
        let mut cfg = json!({"commitment": "finalized", "limit": 1000});
        if let Some(b) = before {
            cfg["before"] = json!(b);
        }
        let r = self
            .call("getSignaturesForAddress", json!([address, cfg]))
            .await?;
        parse_signatures(&r)
    }

    async fn transaction(&self, signature: &str) -> Result<Option<Value>, IngestError> {
        let cfg = json!({"commitment": "finalized", "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0});
        let r = self.call("getTransaction", json!([signature, cfg])).await?;
        Ok(if r.is_null() { None } else { Some(r) })
    }

    async fn token_supply(&self, mint: &str) -> Result<TokenSupply, IngestError> {
        let r = self
            .call("getTokenSupply", json!([mint, {"commitment": "finalized"}]))
            .await?;
        parse_token_supply(&r)
    }
}
