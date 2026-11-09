import { connect, keyStores, KeyPair, utils } from "near-api-js";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";

export const NETWORK = process.env.NEAR_NETWORK || "testnet";
export const WALLET = process.env.LEASH_WALLET; // contract account = the agent's wallet
export const OWNER = process.env.LEASH_OWNER;   // human owner account
export const SHOP = process.env.LEASH_SHOP;     // an allowlisted merchant
export const SCAM = process.env.LEASH_SCAM || "near"; // a non-allowlisted account
export const AGENT_KEY_FILE = process.env.LEASH_AGENT_KEY || path.join(os.homedir(), ".leash", `agent-${WALLET}.key`);
export const NEAR = (x) => utils.format.parseNearAmount(String(x));
export const fmt = (y) => utils.format.formatNearAmount(y, 4) + " NEAR";
export const GAS = "50000000000000";

export function need(...names) {
  const miss = names.filter((n) => !process.env[n]);
  if (miss.length) {
    console.error(`Set ${miss.join(", ")} (see README).`);
    process.exit(2);
  }
}

/** Full-access keys come from the standard ~/.near-credentials keychain (near-cli legacy keychain). */
export async function nearWithKeychain() {
  const keyStore = new keyStores.UnencryptedFileSystemKeyStore(path.join(os.homedir(), ".near-credentials"));
  return connect({ networkId: NETWORK, nodeUrl: `https://rpc.${NETWORK}.near.org`, keyStore });
}

/** The agent process sees ONLY its function-call key; nothing else is in its keystore. */
export async function nearAsAgent() {
  const keyStore = new keyStores.InMemoryKeyStore();
  const secret = fs.readFileSync(AGENT_KEY_FILE, "utf8").trim();
  await keyStore.setKey(NETWORK, WALLET, KeyPair.fromString(secret));
  return connect({ networkId: NETWORK, nodeUrl: `https://rpc.${NETWORK}.near.org`, keyStore });
}

export function txUrl(hash) {
  return `https://testnet.nearblocks.io/txns/${hash}`;
}
