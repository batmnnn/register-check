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
    /// Token accounts whose on-chain balance disagreed with history in two consecutive polls:
    /// a token movement the signature scan missed (e.g. an unchecked `transfer` that doesn't
    /// reference the mint). (account, onchain, computed); onchain `None` = account closed.
    pub balance_drift: Vec<BalanceDrift>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceDrift {
    pub account: String,
    pub onchain: Option<u64>,
    pub computed: u64,
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
    /// Token accounts of the mint seen in history (from pre/post token balances).
    known_accounts: Vec<String>,
    /// Accounts that mismatched last poll; a mismatch is reported only if it repeats.
    suspect_accounts: HashSet<String>,
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
            known_accounts: Vec::new(),
            suspect_accounts: HashSet::new(),
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
        // Read supply and known token-account balances first: anything finalized after these
        // reads is excluded from reconciliation.
        let supply = self.rpc.token_supply(&self.mint).await?;
        let onchain_accounts = if self.known_accounts.is_empty() {
            None
        } else {
            let accounts = self.known_accounts.clone();
            let (slot, amounts) = self.rpc.token_account_amounts(&accounts).await?;
            Some((slot, accounts, amounts))
        };

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

        // Every unprocessed signature becomes pending *before* any fetch. If a fetch fails mid-poll
        // (rate limit, timeout), the remaining signatures are retried next poll instead of being
        // hidden behind newer, already-seen signatures.
        let mut failed_skipped = 0;
        for sig in fresh {
            if sig.failed {
                if self.seen.insert(sig.signature) {
                    failed_skipped += 1;
                }
            } else if !self.seen.contains(&sig.signature) {
                self.pending.insert(sig.signature);
            }
        }
        let mut report_new = 0;
        let mut todo: Vec<String> = self.pending.iter().cloned().collect();
        todo.sort();
        for signature in todo {
            // On error, return with `pending` intact.
            if let Some(tx) = self.rpc.transaction(&signature).await? {
                let slot = tx["slot"].as_u64().unwrap_or(0);
                let idx = tx["transactionIndex"]
                    .as_u64()
                    .and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(0);
                self.txs.insert((slot, idx, signature.clone()), tx);
                self.pending.remove(&signature);
                self.seen.insert(signature);
                report_new += 1;
            }
            // `None`: finalized but not served yet; stays pending.
        }

        let acct_slot = onchain_accounts.as_ref().map_or(0, |(s, _, _)| *s);
        let (register, unparsed, computed_upto, accounts_upto, known) =
            self.rebuild(supply.context_slot, acct_slot);
        self.register = register;
        self.known_accounts = known;

        let mut mismatched: HashSet<String> = HashSet::new();
        let mut balance_drift = Vec::new();
        if let (Some((_, accounts, amounts)), true) = (onchain_accounts, self.pending.is_empty()) {
            for (account, onchain) in accounts.into_iter().zip(amounts) {
                let computed = accounts_upto.get(&account).copied().unwrap_or(0);
                if onchain.unwrap_or(0) != computed {
                    if self.suspect_accounts.contains(&account) {
                        balance_drift.push(BalanceDrift {
                            account: account.clone(),
                            onchain,
                            computed,
                        });
                    }
                    mismatched.insert(account);
                }
            }
        }
        self.suspect_accounts = mismatched;
        let computed_supply = last_supply(&self.register);
        // While transactions are pending the computed supply is incomplete by construction, so
        // drift is only reported on a complete history (pending count is reported instead).
        let drift =
            (self.pending.is_empty() && computed_upto != supply.amount).then_some(SupplyDrift {
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
            balance_drift,
        })
    }

    /// Replay all kept transactions in (slot, tx_index) order. Returns the register, unparsed
    /// instructions / problems, and the supply computed from events at or before `upto_slot`.
    /// Never fails: a transaction that can't be parsed is reported in `unparsed` (and will show up
    /// as supply drift if it changed supply) instead of stopping monitoring.
    /// Also returns each token account's amount as of `acct_slot` and the list of known accounts.
    #[allow(clippy::type_complexity)]
    fn rebuild(
        &self,
        upto_slot: u64,
        acct_slot: u64,
    ) -> (
        Register,
        Vec<(String, String)>,
        u64,
        HashMap<String, u64>,
        Vec<String>,
    ) {
        let mut state = MintState::default();
        let mut reg = Register::default();
        let mut unparsed = Vec::new();
        let mut supply: u64 = 0;
        let mut supply_upto: u64 = 0;
        // token account -> (owner, amount, slot); last write wins in replay order.
        let mut accounts: HashMap<String, (String, u64, u64)> = HashMap::new();
        let mut accounts_upto: HashMap<String, u64> = HashMap::new();

        for ((_, _, sig), tx) in &self.txs {
            if tx["transactionIndex"].as_u64().is_none() {
                unparsed.push((sig.clone(), "missing_transaction_index".to_string()));
            }
            let p = match parse_transaction(tx, &self.mint, &mut state) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(e) => {
                    unparsed.push((sig.clone(), format!("parse_error: {e}")));
                    continue;
                }
            };
            unparsed.extend(p.unparsed.iter().map(|t| (p.signature.clone(), t.clone())));
            for (account, owner, amount) in &p.balances {
                accounts.insert(account.clone(), (owner.clone(), *amount, p.slot));
                if p.slot <= acct_slot {
                    accounts_upto.insert(account.clone(), *amount);
                }
            }
            for ev in p.events {
                match ev {
                    Event::Mint(mut m) => match supply.checked_add(m.amount) {
                        Some(s) => {
                            supply = s;
                            m.supply_after = supply;
                            reg.mints.push(m);
                        }
                        None => unparsed.push((p.signature.clone(), "supply_overflow".into())),
                    },
                    Event::Burn(mut b) => match supply.checked_sub(b.amount) {
                        Some(s) => {
                            supply = s;
                            b.supply_after = supply;
                            reg.burns.push(b);
                        }
                        None => unparsed.push((p.signature.clone(), "supply_underflow".into())),
                    },
                    Event::Transfer(t) => reg.transfers.push(t),
                    Event::Authority(a) => reg.authority_changes.push(a),
                }
            }
            if p.slot <= upto_slot {
                supply_upto = supply;
            }
        }

        let mut known: Vec<String> = accounts.keys().cloned().collect();
        known.sort();
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
        (reg, unparsed, supply_upto, accounts_upto, known)
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
