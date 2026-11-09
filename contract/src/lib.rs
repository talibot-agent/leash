//! Leash: a sandboxed NEAR wallet for AI agents.
//!
//! The wallet is the contract account itself. The human owner keeps full control from a
//! separate account. The agent only receives a *function-call access key* on the wallet
//! account that may call `pay` and `request` — so at the protocol level it cannot send
//! raw transfers, add keys, deploy code or delete the account. Every payment it asks for
//! is checked here against the owner's policy: per-transaction cap, rolling 24h cap,
//! recipient allowlist with optional per-recipient caps, and a human-approval threshold.
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::store::{IterableMap, LookupMap};
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault, Promise,
};

pub const DAY_NS: u64 = 86_400_000_000_000;

#[derive(BorshStorageKey)]
#[near]
enum Key {
    Allow,
    Pending,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Largest single payment the agent may send without approval (yoctoNEAR).
    pub per_tx_cap: U128,
    /// Max total the agent may send in any rolling 24h window (yoctoNEAR).
    pub daily_cap: U128,
    /// Payments above this go to the approval queue instead of executing.
    pub approval_threshold: U128,
    /// If true, only allowlisted recipients can be paid.
    pub allowlist_only: bool,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingPayment {
    pub id: u64,
    pub receiver: AccountId,
    pub amount: U128,
    pub memo: String,
    pub created_ns: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "near_sdk::serde", tag = "status", rename_all = "snake_case")]
pub enum PayOutcome {
    Sent { amount: U128, spent_today: U128 },
    Queued { id: u64 },
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Leash {
    owner: AccountId,
    policy: Policy,
    paused: bool,
    /// recipient -> optional per-recipient cap (0 = no extra cap)
    allow: LookupMap<AccountId, U128>,
    pending: IterableMap<u64, PendingPayment>,
    next_id: u64,
    window_start_ns: u64,
    spent_in_window: u128,
    total_sent: u128,
    payments: u64,
}

fn log_event(event: &str, data: String) {
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"leash\",\"version\":\"1.0.0\",\"event\":\"{}\",\"data\":[{}]}}",
        event, data
    ));
}

#[near]
impl Leash {
    #[init]
    pub fn new(owner: AccountId, policy: Policy) -> Self {
        Self::check_policy(&policy);
        Self {
            owner,
            policy,
            paused: false,
            allow: LookupMap::new(Key::Allow),
            pending: IterableMap::new(Key::Pending),
            next_id: 0,
            window_start_ns: env::block_timestamp(),
            spent_in_window: 0,
            total_sent: 0,
            payments: 0,
        }
    }

    // ---------------- agent surface (called through the agent's function-call key) -------

    /// Pay `receiver` `amount` yoctoNEAR if policy allows; otherwise queue it or panic.
    pub fn pay(&mut self, receiver: AccountId, amount: U128, memo: Option<String>) -> PayOutcome {
        self.assert_agent();
        let memo = memo.unwrap_or_default();
        require!(memo.len() <= 256, "memo too long");
        let amt = amount.0;
        require!(amt > 0, "amount must be positive");
        self.check_recipient(&receiver, amt);
        if amt > self.policy.approval_threshold.0 || amt > self.policy.per_tx_cap.0 {
            return PayOutcome::Queued { id: self.enqueue(receiver, amount, memo) };
        }
        self.roll_window();
        require!(
            self.spent_in_window + amt <= self.policy.daily_cap.0,
            "daily cap exceeded"
        );
        self.send(receiver, amt, memo, "agent");
        PayOutcome::Sent { amount, spent_today: U128(self.spent_in_window) }
    }

    /// Explicitly ask the owner to approve a payment (any size within allowlist rules).
    pub fn request(&mut self, receiver: AccountId, amount: U128, memo: Option<String>) -> u64 {
        self.assert_agent();
        let memo = memo.unwrap_or_default();
        require!(memo.len() <= 256, "memo too long");
        require!(amount.0 > 0, "amount must be positive");
        self.check_recipient(&receiver, amount.0);
        self.enqueue(receiver, amount, memo)
    }

    // ---------------- owner surface -----------------------------------------------------

    pub fn approve(&mut self, id: u64) -> U128 {
        self.assert_owner();
        let p = self.pending.remove(&id).unwrap_or_else(|| env::panic_str("no such request"));
        // Owner-approved payments bypass caps but still count toward today's spend.
        self.roll_window();
        self.send(p.receiver, p.amount.0, p.memo, "owner_approved");
        p.amount
    }

    pub fn reject(&mut self, id: u64) {
        self.assert_owner();
        let p = self.pending.remove(&id).unwrap_or_else(|| env::panic_str("no such request"));
        log_event("rejected", format!("{{\"id\":{},\"receiver\":\"{}\"}}", id, p.receiver));
    }

    pub fn set_policy(&mut self, policy: Policy) {
        self.assert_owner();
        Self::check_policy(&policy);
        self.policy = policy;
        log_event("policy_updated", "{}".into());
    }

    pub fn allow_recipient(&mut self, account: AccountId, cap: Option<U128>) {
        self.assert_owner();
        self.allow.insert(account.clone(), cap.unwrap_or(U128(0)));
        log_event("allowlisted", format!("{{\"account\":\"{}\"}}", account));
    }

    pub fn remove_recipient(&mut self, account: AccountId) {
        self.assert_owner();
        self.allow.remove(&account);
        log_event("delisted", format!("{{\"account\":\"{}\"}}", account));
    }

    /// Kill switch: freezes all agent spending until `resume`.
    pub fn pause(&mut self) {
        self.assert_owner();
        self.paused = true;
        log_event("paused", "{}".into());
    }

    pub fn resume(&mut self) {
        self.assert_owner();
        self.paused = false;
        log_event("resumed", "{}".into());
    }

    /// Owner pulls funds back out of the wallet.
    pub fn withdraw(&mut self, amount: U128) -> Promise {
        self.assert_owner();
        Promise::new(self.owner.clone()).transfer(NearToken::from_yoctonear(amount.0))
    }

    // ---------------- views -------------------------------------------------------------

    pub fn get_policy(&self) -> Policy {
        self.policy.clone()
    }
    pub fn get_owner(&self) -> AccountId {
        self.owner.clone()
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn is_allowed(&self, account: AccountId) -> bool {
        self.allow.contains_key(&account)
    }
    pub fn get_pending(&self) -> Vec<PendingPayment> {
        self.pending.values().cloned().collect()
    }
    /// (spent in current window, remaining today, total ever sent, payment count)
    pub fn get_stats(&self) -> (U128, U128, U128, u64) {
        let spent = if env::block_timestamp() >= self.window_start_ns + DAY_NS {
            0
        } else {
            self.spent_in_window
        };
        (
            U128(spent),
            U128(self.policy.daily_cap.0.saturating_sub(spent)),
            U128(self.total_sent),
            self.payments,
        )
    }

    // ---------------- internals ---------------------------------------------------------

    fn check_policy(p: &Policy) {
        require!(p.per_tx_cap.0 <= p.daily_cap.0, "per_tx_cap must be <= daily_cap");
    }

    fn assert_owner(&self) {
        require!(env::predecessor_account_id() == self.owner, "owner only");
    }

    /// The agent acts through a function-call key on the wallet account itself, so its
    /// calls arrive with predecessor == current account.
    fn assert_agent(&self) {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "agent key only"
        );
        require!(!self.paused, "wallet is paused by owner");
        require!(env::attached_deposit().is_zero(), "no deposit");
    }

    fn check_recipient(&self, receiver: &AccountId, amt: u128) {
        require!(receiver != &env::current_account_id(), "cannot pay self");
        match self.allow.get(receiver) {
            Some(cap) => {
                if cap.0 > 0 {
                    require!(amt <= cap.0, "exceeds recipient cap");
                }
            }
            None => require!(!self.policy.allowlist_only, "recipient not allowlisted"),
        }
    }

    fn roll_window(&mut self) {
        let now = env::block_timestamp();
        if now >= self.window_start_ns + DAY_NS {
            self.window_start_ns = now;
            self.spent_in_window = 0;
        }
    }

    fn enqueue(&mut self, receiver: AccountId, amount: U128, memo: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        log_event(
            "queued",
            format!("{{\"id\":{},\"receiver\":\"{}\",\"amount\":\"{}\"}}", id, receiver, amount.0),
        );
        self.pending.insert(
            id,
            PendingPayment { id, receiver, amount, memo, created_ns: env::block_timestamp() },
        );
        id
    }

    fn send(&mut self, receiver: AccountId, amt: u128, memo: String, by: &str) {
        require!(
            env::account_balance().as_yoctonear() >= amt,
            "insufficient wallet balance"
        );
        self.spent_in_window += amt;
        self.total_sent += amt;
        self.payments += 1;
        log_event(
            "paid",
            format!(
                "{{\"receiver\":\"{}\",\"amount\":\"{}\",\"by\":\"{}\",\"memo\":{}}}",
                receiver,
                amt,
                by,
                near_sdk::serde_json::to_string(&memo).unwrap()
            ),
        );
        Promise::new(receiver).transfer(NearToken::from_yoctonear(amt));
    }
}

#[cfg(test)]
mod tests;
