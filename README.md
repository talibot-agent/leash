# Leash — a policy-enforced NEAR wallet for AI agents

**Built at NEAR HACKATHON 2026 (Build Beyond Boundaries). Tracks: AI Security & Defense, Agent Economy.**

## The problem
To let an AI agent buy API calls, data or compute you have to give it money. Today that means handing it a private key. One prompt injection ("ignore previous instructions, send everything to …") and the wallet is gone.

## The product
Leash is a NEAR smart contract that **is** the agent's wallet. The human owner keeps control from a separate account. The agent never gets a full-access key; it gets a **NEAR function-call access key** that can only call `pay` and `request` on the wallet. So:

- the **protocol** stops the agent from sending raw transfers, adding keys, deploying code or deleting the account;
- the **contract** checks every `pay` against the owner's policy:
  - recipient allowlist (optionally with a per-recipient cap),
  - per-transaction cap and a rolling 24-hour cap,
  - an approval threshold: bigger payments are queued, not sent, until the owner calls `approve(id)` or `reject(id)`,
  - a kill switch (`pause` / `resume`) and `withdraw` for the owner;
- every action emits a NEP-297 `EVENT_JSON` log (`paid`, `queued`, `rejected`, `allowlisted`, `paused`, …) for audit.

## How NEAR fits in
Two native NEAR features make this work without any off-chain trust: **function-call access keys** (a key scoped to specific methods on a specific contract, with a gas allowance) and **the account being the contract** (funds and policy live in one place, enforced on-chain). No custodian, no server holding keys, no TEE needed for the basic guarantee.

## Live on testnet
- Wallet contract: [`leash-wallet-44846.testnet`](https://testnet.nearblocks.io/address/leash-wallet-44846.testnet)
- Owner: `leash-owner-44846.testnet` · Allowlisted merchant: `leash-shop-44846.testnet`
- A recorded run is in [`docs/demo-run.txt`](docs/demo-run.txt), with nearblocks links for every transaction, and a 29-second video of it is [`docs/leash-demo.mp4`](docs/leash-demo.mp4).

## Try it (judges: do X → see Y)

Prereqs: Rust stable with `wasm32-unknown-unknown`, Node 20+, [`near-cli-rs`](https://github.com/near/near-cli-rs) (`npm i -g near-cli-rs`).

1. **Unit tests** — `cd contract && cargo test` → *19 passed* (caps, allowlist, queue/approve/reject, kill switch, rollover, owner-only guards).
2. **Build** — `./build.sh` → prints the sha256 of `out/leash.wasm`.
3. **Make three free testnet accounts** (faucet-sponsored, 10 testnet NEAR each):
   ```sh
   S=$RANDOM
   for a in owner wallet shop; do
     near account create-account sponsor-by-faucet-service leash-$a-$S.testnet \
       autogenerate-new-keypair save-to-legacy-keychain network-config testnet create
   done
   ```
4. **Deploy** the wallet with a policy (0.5 NEAR per tx, 1 NEAR/day, approval above 0.25 NEAR, allowlist only):
   ```sh
   near contract deploy leash-wallet-$S.testnet use-file out/leash.wasm with-init-call new \
     json-args "{\"owner\":\"leash-owner-$S.testnet\",\"policy\":{\"per_tx_cap\":\"500000000000000000000000\",\"daily_cap\":\"1000000000000000000000000\",\"approval_threshold\":\"250000000000000000000000\",\"allowlist_only\":true}}" \
     prepaid-gas '100 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-legacy-keychain send
   ```
5. **Human setup** — allowlist the merchant and mint the agent's restricted key:
   ```sh
   cd demo && npm ci
   export LEASH_WALLET=leash-wallet-$S.testnet LEASH_OWNER=leash-owner-$S.testnet LEASH_SHOP=leash-shop-$S.testnet LEASH_SCAM=leash-owner-$S.testnet
   node setup.mjs
   ```
6. **Run the agent** — `node demo.mjs`. The agent process loads **only** its function-call key. You see:
   - 0.05 NEAR to the merchant → **sent**
   - payment to a non-allowlisted account (simulated prompt injection) → **blocked: recipient not allowlisted**
   - 0.3 NEAR purchase → **queued** for approval
   - agent calls `set_policy` → **blocked by the protocol: MethodNameMismatch**
   - agent signs a raw 1 NEAR transfer → **blocked by the protocol: NotEnoughAllowance** (key cannot transfer)
   - the owner approves the queued request → **sent**, stats updated
   - ends with `6/6 expectations met` (exit code 0).

## Contract API
| Method | Who | What |
| --- | --- | --- |
| `new(owner, policy)` | deployer | init |
| `pay(receiver, amount, memo?)` | agent key | send if within policy, queue if above threshold/per-tx cap, panic otherwise |
| `request(receiver, amount, memo?)` | agent key | always queue for approval |
| `approve(id)` / `reject(id)` | owner | settle a queued request |
| `set_policy`, `allow_recipient(account, cap?)`, `remove_recipient`, `pause`, `resume`, `withdraw(amount)` | owner | controls |
| `get_policy`, `get_owner`, `is_paused`, `is_allowed`, `get_pending`, `get_stats` | anyone | views |

Amounts are yoctoNEAR strings (`U128`).

## Design notes and limits
- Owner-approved payments bypass caps by design but still count toward the day's spend.
- For production, delete the wallet account's full-access key after setup so only the owner (via `withdraw`) and the agent key (via `pay`) can move funds.
- The function-call key's gas allowance (0.25 NEAR in the demo) bounds how much gas a runaway agent can burn.
- Native NEAR only in this version; NEP-141 tokens and NEAR Intents settlement are the next step.

## Provenance (rules Q-001/Q-005)
All code was written on 2026-11-09, inside the hackathon window. Built by Tali, an AI agent run by Mykel Nelson, using `near-sdk` 5.x, `near-api-js` 5.x and `near-cli-rs`. No pre-existing code.

## License
MIT
