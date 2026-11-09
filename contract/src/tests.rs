use super::*;
use near_sdk::test_utils::{get_logs, VMContextBuilder};
use near_sdk::testing_env;

const N: u128 = 1_000_000_000_000_000_000_000_000; // 1 NEAR

fn wallet() -> AccountId { "wallet.testnet".parse().unwrap() }
fn owner() -> AccountId { "human.testnet".parse().unwrap() }
fn shop() -> AccountId { "api-shop.testnet".parse().unwrap() }
fn scam() -> AccountId { "free-money.testnet".parse().unwrap() }

fn ctx(pred: AccountId, ts: u64) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(wallet())
        .predecessor_account_id(pred.clone())
        .signer_account_id(pred)
        .account_balance(NearToken::from_yoctonear(10 * N))
        .block_timestamp(ts);
    testing_env!(b.build());
}

fn policy() -> Policy {
    Policy {
        per_tx_cap: U128(N / 2),
        daily_cap: U128(N),
        approval_threshold: U128(N / 4),
        allowlist_only: true,
    }
}

fn setup() -> Leash {
    ctx(owner(), 1);
    let mut c = Leash::new(owner(), policy());
    c.allow_recipient(shop(), None);
    c
}

#[test]
fn agent_pays_allowlisted_within_caps() {
    let mut c = setup();
    ctx(wallet(), 2);
    let r = c.pay(shop(), U128(N / 10), Some("1 GPU-minute".into()));
    assert_eq!(r, PayOutcome::Sent { amount: U128(N / 10), spent_today: U128(N / 10) });
    assert!(get_logs().iter().any(|l| l.contains("\"event\":\"paid\"")));
    assert_eq!(c.get_stats().3, 1);
}

#[test]
#[should_panic(expected = "recipient not allowlisted")]
fn rejects_unknown_recipient() {
    let mut c = setup();
    ctx(wallet(), 2);
    c.pay(scam(), U128(1), None);
}

#[test]
fn open_policy_allows_unknown_recipient() {
    let mut c = setup();
    ctx(owner(), 2);
    let mut p = policy();
    p.allowlist_only = false;
    c.set_policy(p);
    ctx(wallet(), 3);
    assert!(matches!(c.pay(scam(), U128(1), None), PayOutcome::Sent { .. }));
}

#[test]
#[should_panic(expected = "agent key only")]
fn outsiders_cannot_spend() {
    let mut c = setup();
    ctx(scam(), 2);
    c.pay(shop(), U128(1), None);
}

#[test]
#[should_panic(expected = "owner only")]
fn agent_cannot_change_policy() {
    let mut c = setup();
    ctx(wallet(), 2);
    c.set_policy(Policy { per_tx_cap: U128(100 * N), daily_cap: U128(100 * N), approval_threshold: U128(100 * N), allowlist_only: false });
}

#[test]
#[should_panic(expected = "owner only")]
fn agent_cannot_allowlist_itself() {
    let mut c = setup();
    ctx(wallet(), 2);
    c.allow_recipient(scam(), None);
}

#[test]
fn over_threshold_is_queued_then_approved() {
    let mut c = setup();
    ctx(wallet(), 2);
    let r = c.pay(shop(), U128(N / 3), Some("bulk dataset".into()));
    assert_eq!(r, PayOutcome::Queued { id: 0 });
    assert_eq!(c.get_pending().len(), 1);
    assert_eq!(c.get_stats().0, U128(0), "queued payment must not count as spent");
    ctx(owner(), 3);
    assert_eq!(c.approve(0), U128(N / 3));
    assert!(c.get_pending().is_empty());
    assert_eq!(c.get_stats().0, U128(N / 3));
}

#[test]
fn reject_clears_request() {
    let mut c = setup();
    ctx(wallet(), 2);
    let id = c.request(shop(), U128(N / 100), None);
    ctx(owner(), 3);
    c.reject(id);
    assert!(c.get_pending().is_empty());
}

#[test]
#[should_panic(expected = "owner only")]
fn agent_cannot_approve_own_request() {
    let mut c = setup();
    ctx(wallet(), 2);
    let id = c.request(shop(), U128(N), None);
    c.approve(id);
}

#[test]
#[should_panic(expected = "daily cap exceeded")]
fn daily_cap_enforced() {
    let mut c = setup();
    ctx(wallet(), 2);
    for _ in 0..5 {
        c.pay(shop(), U128(N / 5), None); // 5 x 0.2 = 1.0 NEAR, at cap
    }
    c.pay(shop(), U128(1), None);
}

#[test]
fn daily_window_rolls_over() {
    let mut c = setup();
    ctx(wallet(), 2);
    for _ in 0..5 {
        c.pay(shop(), U128(N / 5), None);
    }
    ctx(wallet(), 2 + DAY_NS);
    assert!(matches!(c.pay(shop(), U128(N / 5), None), PayOutcome::Sent { .. }));
    assert_eq!(c.get_stats().0, U128(N / 5));
    assert_eq!(c.get_stats().2, U128(6 * N / 5));
}

#[test]
#[should_panic(expected = "exceeds recipient cap")]
fn per_recipient_cap() {
    let mut c = setup();
    ctx(owner(), 2);
    c.allow_recipient(shop(), Some(U128(N / 100)));
    ctx(wallet(), 3);
    c.pay(shop(), U128(N / 50), None);
}

#[test]
#[should_panic(expected = "wallet is paused by owner")]
fn kill_switch() {
    let mut c = setup();
    ctx(owner(), 2);
    c.pause();
    ctx(wallet(), 3);
    c.pay(shop(), U128(1), None);
}

#[test]
fn resume_after_pause() {
    let mut c = setup();
    ctx(owner(), 2);
    c.pause();
    c.resume();
    ctx(wallet(), 3);
    assert!(!c.is_paused());
    assert!(matches!(c.pay(shop(), U128(1), None), PayOutcome::Sent { .. }));
}

#[test]
#[should_panic(expected = "per_tx_cap must be <= daily_cap")]
fn bad_policy_rejected() {
    ctx(owner(), 1);
    Leash::new(owner(), Policy { per_tx_cap: U128(2), daily_cap: U128(1), approval_threshold: U128(1), allowlist_only: true });
}

#[test]
#[should_panic(expected = "cannot pay self")]
fn cannot_pay_self() {
    let mut c = setup();
    ctx(owner(), 2);
    c.allow_recipient(wallet(), None);
    ctx(wallet(), 3);
    c.pay(wallet(), U128(1), None);
}

#[test]
#[should_panic(expected = "memo too long")]
fn memo_length_bounded() {
    let mut c = setup();
    ctx(wallet(), 2);
    c.pay(shop(), U128(1), Some("x".repeat(300)));
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn zero_amount_rejected() {
    let mut c = setup();
    ctx(wallet(), 2);
    c.pay(shop(), U128(0), None);
}

#[test]
fn delisting_blocks_recipient() {
    let mut c = setup();
    ctx(owner(), 2);
    c.remove_recipient(shop());
    assert!(!c.is_allowed(shop()));
}
