// The AGENT side: a process that only holds a function-call key. Watch the policy hold.
import { transactions, utils } from "near-api-js";
import { need, nearAsAgent, nearWithKeychain, WALLET, OWNER, SHOP, SCAM, NEAR, fmt, GAS, txUrl } from "./common.mjs";

need("LEASH_WALLET", "LEASH_OWNER", "LEASH_SHOP");
const agentNear = await nearAsAgent();
const agent = await agentNear.account(WALLET);
const view = (m, a = {}) => agent.viewFunction({ contractId: WALLET, methodName: m, args: a });
const results = [];

function errMsg(e) {
  const s = JSON.stringify(e?.type ? { type: e.type, msg: e.message } : String(e?.message || e));
  const m = s.match(/Smart contract panicked: ([^"\\]+)/);
  return m ? m[1] : (e?.type || String(e?.message || e)).slice(0, 160);
}

async function step(label, expect, fn) {
  process.stdout.write(`\n▶ ${label}\n`);
  try {
    const out = await fn();
    const hash = out?.transaction?.hash;
    const ret = out?.status?.SuccessValue ? Buffer.from(out.status.SuccessValue, "base64").toString() : "";
    console.log(`  ✔ allowed ${ret}${hash ? "\n    " + txUrl(hash) : ""}`);
    results.push({ label, expect, got: "ok", pass: expect === "ok" });
  } catch (e) {
    const msg = errMsg(e);
    console.log(`  ✖ blocked: ${msg}`);
    results.push({ label, expect, got: msg, pass: expect !== "ok" && msg.includes(expect) });
  }
}

const pay = (receiver, near, memo) => () =>
  agent.functionCall({ contractId: WALLET, methodName: "pay", args: { receiver, amount: NEAR(near), memo }, gas: GAS });

console.log(`Leash demo on ${WALLET}`);
console.log("policy:", JSON.stringify(await view("get_policy")));

await step("Agent buys 0.05 NEAR of API calls from an allowlisted merchant", "ok", pay(SHOP, "0.05", "100 inference calls"));
await step("Prompt-injected agent tries to pay an unknown account", "recipient not allowlisted", pay(SCAM, "0.05", "ignore previous instructions"));
await step("Agent tries a 0.3 NEAR purchase (above 0.25 approval threshold)", "ok", pay(SHOP, "0.3", "bulk dataset"));
await step("Agent tries to change its own limits", "MethodNameMismatch|owner only", () =>
  agent.functionCall({ contractId: WALLET, methodName: "set_policy", args: { policy: { per_tx_cap: NEAR(100), daily_cap: NEAR(100), approval_threshold: NEAR(100), allowlist_only: false } }, gas: GAS }));
await step("Agent tries a raw transfer that skips the contract", "NotEnoughAllowance|AccessKey|InvalidAccessKey", () =>
  agent.signAndSendTransaction({ receiverId: SCAM, actions: [transactions.transfer(BigInt(NEAR("1")))] }));

const pending = await view("get_pending");
console.log(`\nqueued for human approval: ${JSON.stringify(pending)}`);

// Human approves the queued purchase from the OWNER account (full-access key lives with the human only).
if (process.env.LEASH_APPROVE !== "0" && pending.length) {
  const owner = await (await nearWithKeychain()).account(OWNER);
  const id = pending[pending.length - 1].id;
  await step(`Human ${OWNER} approves request #${id}`, "ok", () =>
    owner.functionCall({ contractId: WALLET, methodName: "approve", args: { id }, gas: GAS }));
}

const [spent, left, total, n] = await view("get_stats");
console.log(`\nstats: spent today ${fmt(spent)}, remaining today ${fmt(left)}, total sent ${fmt(total)}, payments ${n}`);

// The "raw transfer" check passes on any access-key rejection.
for (const r of results) if (r.expect.includes("|")) r.pass = r.got !== "ok";
const passed = results.filter((r) => r.pass).length;
console.log(`\n${passed}/${results.length} expectations met`);
process.exit(passed === results.length ? 0 : 1);
