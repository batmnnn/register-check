use std::collections::HashMap;

use rc_types::{AuthorityChange, BurnEvent, ChangeKind, MintEvent, TransferEvent};
use serde_json::Value;

use crate::IngestError;

pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// A register event from one instruction. `supply_after` is 0 until the ingester computes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Mint(MintEvent),
    Burn(BurnEvent),
    Transfer(TransferEvent),
    Authority(AuthorityChange),
}

/// Current privileged settings of the mint, replayed from history in order. Gives
/// `old_value` for authority changes and identifies permanent-delegate burns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MintState {
    pub initialized: bool,
    pub mint_authority: Option<String>,
    pub freeze_authority: Option<String>,
    pub permanent_delegate: Option<String>,
    pub transfer_hook_program: Option<String>,
    pub transfer_hook_authority: Option<String>,
    pub close_authority: Option<String>,
}

/// Everything rc-ingest keeps from one successful transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTx {
    pub signature: String,
    pub slot: u64,
    pub tx_index: u32,
    pub events: Vec<Event>,
    /// Post-tx state of every token account of our mint touched: (account, owner, amount).
    pub balances: Vec<(String, String, u64)>,
    /// Instructions on Token-2022 touching our mint that rc-ingest does not interpret.
    pub unparsed: Vec<String>,
}

/// Instruction types that can't change supply, balances of other owners, or privileged settings.
const BENIGN: &[&str] = &[
    "initializeAccount",
    "initializeAccount2",
    "initializeAccount3",
    "getAccountDataSize",
    "initializeImmutableOwner",
    "approve",
    "approveChecked",
    "revoke",
    "freezeAccount",
    "thawAccount",
    "closeAccount",
    "syncNative",
    "amountToUiAmount",
    "uiAmountToAmount",
    "reallocate",
];

struct Ctx<'a> {
    mint: &'a str,
    signature: &'a str,
    slot: u64,
    tx_index: u32,
    /// token account -> (mint, owner), from pre/post token balances.
    accounts: HashMap<String, (String, String)>,
}

impl Ctx<'_> {
    fn err(&self, reason: impl Into<String>) -> IngestError {
        IngestError::Parse {
            signature: self.signature.to_string(),
            reason: reason.into(),
        }
    }

    fn owner_of(&self, account: &str) -> Option<&str> {
        self.accounts.get(account).map(|(_, o)| o.as_str())
    }

    fn mint_of(&self, account: &str) -> Option<&str> {
        self.accounts.get(account).map(|(m, _)| m.as_str())
    }
}

fn s<'v>(info: &'v Value, key: &str) -> Option<&'v str> {
    info.get(key).and_then(Value::as_str)
}

fn amount(ctx: &Ctx, info: &Value) -> Result<u64, IngestError> {
    let raw = s(info, "amount").or_else(|| info.get("tokenAmount").and_then(|t| s(t, "amount")));
    raw.and_then(|a| a.parse::<u64>().ok())
        .ok_or_else(|| ctx.err("instruction has no u64 amount"))
}

fn authority_kind(t: &str) -> Option<ChangeKind> {
    match t {
        "mintTokens" => Some(ChangeKind::MintAuthority),
        "freezeAccount" => Some(ChangeKind::FreezeAuthority),
        "permanentDelegate" => Some(ChangeKind::PermanentDelegate),
        "transferHookProgramId" => Some(ChangeKind::TransferHookAuthority),
        "closeMint" => Some(ChangeKind::CloseAuthority),
        // Account-level authorities (owner/close of a token account) are not mint settings.
        "accountOwner" | "closeAccount" => None,
        _ => Some(ChangeKind::Other),
    }
}

fn state_slot(state: &mut MintState, kind: ChangeKind) -> Option<&mut Option<String>> {
    match kind {
        ChangeKind::MintAuthority => Some(&mut state.mint_authority),
        ChangeKind::FreezeAuthority => Some(&mut state.freeze_authority),
        ChangeKind::PermanentDelegate => Some(&mut state.permanent_delegate),
        ChangeKind::TransferHookProgram => Some(&mut state.transfer_hook_program),
        ChangeKind::TransferHookAuthority => Some(&mut state.transfer_hook_authority),
        ChangeKind::CloseAuthority => Some(&mut state.close_authority),
        ChangeKind::Other => None,
    }
}

/// Parse one `getTransaction` (jsonParsed) result. Returns `Ok(None)` for failed transactions
/// (`meta.err != null`): they change nothing on-chain. Walks outer instructions, their inner (CPI)
/// instructions, and `batch`-wrapped instructions in execution order; `ix_index` counts that order.
/// Only instructions whose `programId` is Token-2022 count (the jsonParsed label is shared with
/// legacy SPL Token).
pub fn parse_transaction(
    tx: &Value,
    mint: &str,
    state: &mut MintState,
) -> Result<Option<ParsedTx>, IngestError> {
    let signature =
        tx["transaction"]["signatures"][0]
            .as_str()
            .ok_or_else(|| IngestError::Parse {
                signature: "?".into(),
                reason: "missing transaction.signatures[0]".into(),
            })?;
    let meta = &tx["meta"];
    if meta.is_null() {
        return Err(IngestError::Parse {
            signature: signature.into(),
            reason: "missing meta".into(),
        });
    }
    if !meta["err"].is_null() {
        return Ok(None);
    }
    let slot = tx["slot"].as_u64().ok_or_else(|| IngestError::Parse {
        signature: signature.into(),
        reason: "missing slot".into(),
    })?;
    // Agave 4.x returns transactionIndex; without it, same-slot order falls back to signature order.
    let tx_index = tx["transactionIndex"]
        .as_u64()
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0);

    let message = &tx["transaction"]["message"];
    let keys: Vec<&str> = message["accountKeys"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|k| {
                    k.get("pubkey")
                        .and_then(Value::as_str)
                        .or(k.as_str())
                        .unwrap_or("")
                })
                .collect()
        })
        .unwrap_or_default();

    let mut ctx = Ctx {
        mint,
        signature,
        slot,
        tx_index,
        accounts: HashMap::new(),
    };
    let mut balances = Vec::new();
    for (list, is_post) in [
        (&meta["preTokenBalances"], false),
        (&meta["postTokenBalances"], true),
    ] {
        for b in list.as_array().into_iter().flatten() {
            let Some(account) = b["accountIndex"]
                .as_u64()
                .and_then(|i| keys.get(i as usize).copied())
            else {
                continue;
            };
            let (Some(bm), Some(owner)) = (s(b, "mint"), s(b, "owner")) else {
                continue;
            };
            ctx.accounts
                .insert(account.to_string(), (bm.to_string(), owner.to_string()));
            if is_post && bm == mint && s(b, "programId") == Some(TOKEN_2022_PROGRAM_ID) {
                let amt = b["uiTokenAmount"]["amount"]
                    .as_str()
                    .and_then(|a| a.parse::<u64>().ok())
                    .ok_or_else(|| ctx.err("postTokenBalances amount is not u64"))?;
                balances.push((account.to_string(), owner.to_string(), amt));
            }
        }
    }

    // Execution order: outer i, then inner instructions recorded for outer i.
    let mut inner_by_outer: HashMap<u64, Vec<&Value>> = HashMap::new();
    for group in meta["innerInstructions"].as_array().into_iter().flatten() {
        if let Some(i) = group["index"].as_u64() {
            inner_by_outer
                .entry(i)
                .or_default()
                .extend(group["instructions"].as_array().into_iter().flatten());
        }
    }
    let mut ordered: Vec<&Value> = Vec::new();
    for (i, ix) in message["instructions"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        ordered.push(ix);
        if let Some(inner) = inner_by_outer.get(&(i as u64)) {
            ordered.extend(inner.iter().copied());
        }
    }

    let mut out = ParsedTx {
        signature: signature.to_string(),
        slot,
        tx_index,
        events: Vec::new(),
        balances,
        unparsed: Vec::new(),
    };
    let mut ix_index: u32 = 0;
    for ix in ordered {
        let program = s(ix, "programId");
        visit(&ctx, ix, program, state, &mut ix_index, &mut out)?;
    }
    Ok(Some(out))
}

fn visit(
    ctx: &Ctx,
    ix: &Value,
    program: Option<&str>,
    state: &mut MintState,
    ix_index: &mut u32,
    out: &mut ParsedTx,
) -> Result<(), IngestError> {
    let idx = *ix_index;
    *ix_index = ix_index.saturating_add(1);
    if program != Some(TOKEN_2022_PROGRAM_ID) {
        return Ok(());
    }
    let Some(parsed) = ix.get("parsed").filter(|p| p.is_object()) else {
        // Token-2022 instruction the RPC could not parse: flag it if it touches our mint.
        let touches = ix["accounts"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|a| a.as_str() == Some(ctx.mint));
        if touches {
            out.unparsed.push("unparsable".into());
        }
        return Ok(());
    };
    let kind = parsed["type"].as_str().unwrap_or("");
    let info = &parsed["info"];

    if kind == "batch" {
        // Agave serializes batch members as bare `{type, info}` (no programId, no `parsed`
        // wrapper); they run in the same program as the batch.
        for inner in info["instructions"].as_array().into_iter().flatten() {
            let wrapped;
            let member = if inner.get("parsed").is_some() {
                inner
            } else {
                wrapped = serde_json::json!({ "parsed": inner });
                &wrapped
            };
            visit(ctx, member, program, state, ix_index, out)?;
        }
        return Ok(());
    }

    let ix_mint = s(info, "mint").or_else(|| s(info, "source").and_then(|a| ctx.mint_of(a)));
    if ix_mint != Some(ctx.mint) {
        return Ok(());
    }
    let base = |m: &str| {
        (
            m.to_string(),
            ctx.slot,
            ctx.tx_index,
            idx,
            ctx.signature.to_string(),
        )
    };

    match kind {
        "mintTo" | "mintToChecked" => {
            let (mint, slot, tx_index, ix_index, signature) = base(ctx.mint);
            out.events.push(Event::Mint(MintEvent {
                mint,
                slot,
                tx_index,
                ix_index,
                signature,
                amount: amount(ctx, info)?,
                supply_after: 0,
                authority: s(info, "mintAuthority")
                    .or_else(|| s(info, "multisigMintAuthority"))
                    .unwrap_or_default()
                    .to_string(),
            }));
        }
        "burn" | "burnChecked" => {
            let authority = s(info, "authority")
                .or_else(|| s(info, "multisigAuthority"))
                .unwrap_or_default()
                .to_string();
            let owner = s(info, "account").and_then(|a| ctx.owner_of(a));
            let via_permanent_delegate = state.permanent_delegate.as_deref() == Some(&authority)
                && owner != Some(authority.as_str());
            let (mint, slot, tx_index, ix_index, signature) = base(ctx.mint);
            out.events.push(Event::Burn(BurnEvent {
                mint,
                slot,
                tx_index,
                ix_index,
                signature,
                amount: amount(ctx, info)?,
                supply_after: 0,
                authority,
                via_permanent_delegate,
            }));
        }
        "transfer" | "transferChecked" | "transferCheckedWithFee" => {
            let src = s(info, "source").ok_or_else(|| ctx.err("transfer without source"))?;
            let dst =
                s(info, "destination").ok_or_else(|| ctx.err("transfer without destination"))?;
            let (mint, slot, tx_index, ix_index, signature) = base(ctx.mint);
            out.events.push(Event::Transfer(TransferEvent {
                mint,
                slot,
                tx_index,
                ix_index,
                signature,
                from_owner: ctx.owner_of(src).unwrap_or(src).to_string(),
                to_owner: ctx.owner_of(dst).unwrap_or(dst).to_string(),
                amount: amount(ctx, info)?,
            }));
        }
        "setAuthority" => {
            let Some(change_kind) = s(info, "authorityType").and_then(authority_kind) else {
                return Ok(());
            };
            let new_value = s(info, "newAuthority").map(str::to_string);
            let old_value = match state_slot(state, change_kind) {
                Some(slot) => std::mem::replace(slot, new_value.clone()),
                None => None,
            };
            push_change(ctx, idx, change_kind, old_value, new_value, out);
        }
        "updateTransferHook" => {
            let new_value = s(info, "programId").map(str::to_string);
            let old_value = std::mem::replace(&mut state.transfer_hook_program, new_value.clone());
            push_change(
                ctx,
                idx,
                ChangeKind::TransferHookProgram,
                old_value,
                new_value,
                out,
            );
        }
        "initializeMint" | "initializeMint2" => {
            state.initialized = true;
            state.mint_authority = s(info, "mintAuthority").map(str::to_string);
            state.freeze_authority = s(info, "freezeAuthority").map(str::to_string);
        }
        "initializePermanentDelegate" => {
            state.permanent_delegate = s(info, "delegate").map(str::to_string);
        }
        "initializeTransferHook" => {
            state.transfer_hook_program = s(info, "programId").map(str::to_string);
            state.transfer_hook_authority = s(info, "authority").map(str::to_string);
        }
        "initializeMintCloseAuthority" => {
            state.close_authority = s(info, "newAuthority").map(str::to_string);
        }
        k if BENIGN.contains(&k) => {}
        other => out.unparsed.push(other.to_string()),
    }
    Ok(())
}

fn push_change(
    ctx: &Ctx,
    ix_index: u32,
    change_kind: ChangeKind,
    old_value: Option<String>,
    new_value: Option<String>,
    out: &mut ParsedTx,
) {
    out.events.push(Event::Authority(AuthorityChange {
        mint: ctx.mint.to_string(),
        slot: ctx.slot,
        tx_index: ctx.tx_index,
        ix_index,
        signature: ctx.signature.to_string(),
        change_kind,
        old_value,
        new_value,
    }));
}
