use std::collections::{BTreeMap, HashMap, HashSet};

use rc_types::{Balance, Register};
use serde_json::Value;

use crate::parse::{parse_transaction, Event, MintState};
use crate::rpc::{RpcSource, TokenSupply};
use crate::IngestError;

/// Computed supply disagrees with `getTokenSupply`: some supply change was not parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupplyDrift {
    pub onchain: u64,
    pub computed: u64,
    pub slot: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollReport {
    pub new_signatures: usize,
    pub failed_skipped: usize,
    /// Finalized signatures whose transaction the RPC did not return yet; retried next poll.
    pub pending: usize,
    /// (signature, instruction type) on Token-2022 touching the mint that we did not interpret.
    pub unparsed: Vec<(String, String)>,
    pub supply: TokenSupply,
    pub computed_supply: u64,
    pub drift: Option<SupplyDrift>,
}

/// Follows one mint. Keeps every successful transaction and rebuilds all events from the full
/// history on each poll, so out-of-order arrival can't corrupt supply or authority state.
pub struct Ingester<R: RpcSource> {
    rpc: R,
    mint: String,
    seen: HashSet<String>,
    pending: HashSet<String>,
    /// (slot, tx_index, signature) -> raw transaction.
    txs: BTreeMap<(u64, u32, String), Value>,
    register: Register,
}

impl<R: RpcSource> Ingester<R> {
    pub fn new(rpc: R, mint: impl Into<String>) -> Self {
        Self {
            rpc,
            mint: mint.into(),
            seen: HashSet::new(),
            pending: HashSet::new(),
            txs: BTreeMap::new(),
            register: Register::default(),
        }
    }

    pub fn mint(&self) -> &str {
        &self.mint
    }

    pub fn rpc(&self) -> &R {
        &self.rpc
    }

    /// Events and balances from the last poll. Reserves and allowlist are added by the caller.
    pub fn register(&self) -> &Register {
        &self.register
    }

    pub async fn poll(&mut self) -> Result<PollReport, IngestError> {
        // Read supply first: anything finalized after this slot is excluded from reconciliation.
        let supply = self.rpc.token_supply(&self.mint).await?;

        // Page newest -> oldest until we reach signatures already seen (or the start of history).
        let mut fresh = Vec::new();
        let mut before: Option<String> = None;
        'pages: loop {
            let page = self.rpc.signatures(&self.mint, before.as_deref()).await?;
            let Some(last) = page.last() else { break };
            before = Some(last.signature.clone());
            let full = page.len() >= 1000;
            for sig in page {
                if self.seen.contains(&sig.signature) {
                    break 'pages;
                }
                if !self.pending.contains(&sig.signature) {
                    fresh.push(sig);
                }
            }
            if !full {
                break;
            }
        }

        let mut report_new = 0;
        let mut failed_skipped = 0;
        let mut batch: HashSet<String> = HashSet::new();
        let retry: Vec<String> = self.pending.drain().collect();
        let todo = fresh
            .into_iter()
            .map(|s| (s.signature, s.failed))
            .chain(retry.into_iter().map(|s| (s, false)));
        for (signature, failed) in todo {
            if !batch.insert(signature.clone()) || self.seen.contains(&signature) {
                continue; // duplicate within this poll
            }
            if failed {
                self.seen.insert(signature);
                failed_skipped += 1;
                continue;
            }
            match self.rpc.transaction(&signature).await? {
                Some(tx) => {
                    let slot = tx["slot"].as_u64().unwrap_or(0);
                    let idx = tx["transactionIndex"]
                        .as_u64()
                        .and_then(|i| u32::try_from(i).ok())
                        .unwrap_or(0);
                    self.txs.insert((slot, idx, signature.clone()), tx);
                    self.seen.insert(signature);
                    report_new += 1;
                }
                None => {
                    self.pending.insert(signature);
                }
            }
        }

        let (register, unparsed, computed_upto) = self.rebuild(supply.context_slot)?;
        self.register = register;
        let computed_supply = last_supply(&self.register);
        let drift = (computed_upto != supply.amount).then_some(SupplyDrift {
            onchain: supply.amount,
            computed: computed_upto,
            slot: supply.context_slot,
        });
        Ok(PollReport {
            new_signatures: report_new,
            failed_skipped,
            pending: self.pending.len(),
            unparsed,
            supply,
            computed_supply,
            drift,
        })
    }

    /// Replay all kept transactions in (slot, tx_index) order. Returns the register, unparsed
    /// instructions, and the supply computed from events at or before `upto_slot`.
    #[allow(clippy::type_complexity)]
    fn rebuild(
        &self,
        upto_slot: u64,
    ) -> Result<(Register, Vec<(String, String)>, u64), IngestError> {
        let mut state = MintState::default();
        let mut reg = Register::default();
        let mut unparsed = Vec::new();
        let mut supply: u64 = 0;
        let mut supply_upto: u64 = 0;
        // token account -> (owner, amount, slot); last write wins in replay order.
        let mut accounts: HashMap<String, (String, u64, u64)> = HashMap::new();

        for tx in self.txs.values() {
            let Some(p) = parse_transaction(tx, &self.mint, &mut state)? else {
                continue;
            };
            unparsed.extend(p.unparsed.iter().map(|t| (p.signature.clone(), t.clone())));
            for (account, owner, amount) in &p.balances {
                accounts.insert(account.clone(), (owner.clone(), *amount, p.slot));
            }
            let overflow = || IngestError::SupplyOverflow {
                signature: p.signature.clone(),
            };
            for ev in p.events {
                match ev {
                    Event::Mint(mut m) => {
                        supply = supply.checked_add(m.amount).ok_or_else(overflow)?;
                        m.supply_after = supply;
                        reg.mints.push(m);
                    }
                    Event::Burn(mut b) => {
                        supply = supply.checked_sub(b.amount).ok_or_else(overflow)?;
                        b.supply_after = supply;
                        reg.burns.push(b);
                    }
                    Event::Transfer(t) => reg.transfers.push(t),
                    Event::Authority(a) => reg.authority_changes.push(a),
                }
            }
            if p.slot <= upto_slot {
                supply_upto = supply;
            }
        }

        // Balances per owner (an owner may hold several token accounts of the mint).
        let mut by_owner: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for (owner, amount, slot) in accounts.into_values() {
            let e = by_owner.entry(owner).or_insert((0, 0));
            e.0 = e.0.saturating_add(amount);
            e.1 = e.1.max(slot);
        }
        reg.balances = by_owner
            .into_iter()
            .map(|(owner, (amount, slot))| Balance {
                mint: self.mint.clone(),
                owner,
                amount,
                slot,
            })
            .collect();
        Ok((reg, unparsed, supply_upto))
    }
}

fn last_supply(reg: &Register) -> u64 {
    let m = reg
        .mints
        .iter()
        .map(|e| ((e.slot, e.tx_index, e.ix_index), e.supply_after));
    let b = reg
        .burns
        .iter()
        .map(|e| ((e.slot, e.tx_index, e.ix_index), e.supply_after));
    m.chain(b)
        .max_by_key(|(k, _)| *k)
        .map(|(_, s)| s)
        .unwrap_or(0)
}
