use serde::{Deserialize, Serialize};

use crate::TypesError;

/// A Token-2022 `mintTo` / `mintToChecked` (outer, inner or batch-wrapped).
/// Events are ordered by `(slot, tx_index, ix_index)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MintEvent {
    pub mint: String,
    pub slot: u64,
    pub tx_index: u32,
    pub ix_index: u32,
    pub signature: String,
    pub amount: u64,
    pub supply_after: u64,
    pub authority: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurnEvent {
    pub mint: String,
    pub slot: u64,
    pub tx_index: u32,
    pub ix_index: u32,
    pub signature: String,
    pub amount: u64,
    pub supply_after: u64,
    pub authority: String,
    pub via_permanent_delegate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferEvent {
    pub mint: String,
    pub slot: u64,
    pub tx_index: u32,
    pub ix_index: u32,
    pub signature: String,
    pub from_owner: String,
    pub to_owner: String,
    pub amount: u64,
}

/// Snapshot balance of one owner at `slot`; one row per (mint, owner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    pub mint: String,
    pub owner: String,
    pub amount: u64,
    pub slot: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    MintAuthority,
    FreezeAuthority,
    PermanentDelegate,
    TransferHookProgram,
    CloseAuthority,
    Other,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::MintAuthority => "mint_authority",
            ChangeKind::FreezeAuthority => "freeze_authority",
            ChangeKind::PermanentDelegate => "permanent_delegate",
            ChangeKind::TransferHookProgram => "transfer_hook_program",
            ChangeKind::CloseAuthority => "close_authority",
            ChangeKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityChange {
    pub mint: String,
    pub slot: u64,
    pub tx_index: u32,
    pub ix_index: u32,
    pub signature: String,
    pub change_kind: ChangeKind,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

/// A reserve attestation that already passed signature verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reserve {
    pub mint: String,
    /// Unix seconds, UTC.
    pub as_of_ts: i64,
    pub reserve_amount: u64,
    pub attestor: String,
    pub sig: String,
    pub source_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowEntry {
    pub mint: String,
    pub owner: String,
}

/// One in-memory snapshot of all 7 register tables. Missing tables in JSON default to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Register {
    pub mints: Vec<MintEvent>,
    pub burns: Vec<BurnEvent>,
    pub transfers: Vec<TransferEvent>,
    pub balances: Vec<Balance>,
    pub authority_changes: Vec<AuthorityChange>,
    pub reserves: Vec<Reserve>,
    pub allowlist: Vec<AllowEntry>,
}

impl Register {
    pub fn from_json(s: &str) -> Result<Self, TypesError> {
        Ok(serde_json::from_str(s)?)
    }
}
