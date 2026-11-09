// One-time setup by the HUMAN: allowlist a merchant and mint the agent's restricted key.
import { KeyPair } from "near-api-js";
import fs from "node:fs";
import path from "node:path";
import { need, nearWithKeychain, WALLET, OWNER, SHOP, AGENT_KEY_FILE, NEAR, GAS } from "./common.mjs";

need("LEASH_WALLET", "LEASH_OWNER", "LEASH_SHOP");
const near = await nearWithKeychain();
const owner = await near.account(OWNER);
const wallet = await near.account(WALLET);

console.log(`owner ${OWNER} allowlists merchant ${SHOP}`);
await owner.functionCall({ contractId: WALLET, methodName: "allow_recipient", args: { account: SHOP, cap: null }, gas: GAS });

if (fs.existsSync(AGENT_KEY_FILE)) {
  console.log(`agent key already exists at ${AGENT_KEY_FILE}`);
} else {
  const kp = KeyPair.fromRandom("ed25519");
  // Function-call key: may ONLY call pay/request on the wallet contract, with a 0.25 NEAR gas allowance.
  await wallet.addKey(kp.getPublicKey().toString(), WALLET, ["pay", "request"], NEAR("0.25"));
  fs.mkdirSync(path.dirname(AGENT_KEY_FILE), { recursive: true });
  fs.writeFileSync(AGENT_KEY_FILE, kp.toString(), { mode: 0o600 });
  console.log(`minted agent function-call key ${kp.getPublicKey().toString()} -> ${AGENT_KEY_FILE}`);
}
console.log("setup done");
