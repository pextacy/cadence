import casper from "casper-js-sdk";
import { writeFileSync, mkdirSync } from "node:fs";
const { PrivateKey, KeyAlgorithm } = casper;
const toHex = (u8) => Buffer.from(u8).toString("hex");

async function makeWallet(label) {
  const k = await PrivateKey.generate(KeyAlgorithm.SECP256K1);
  const privHex = toHex(k.priv.toBytes());              // raw 32-byte secp256k1
  const pubHex = k.publicKey.toHex();
  const accountHash = k.publicKey.accountHash().toPrefixedString();
  const pem = k.toPem();
  // round-trip: hex must reload to the same key (this is what the agent/scripts do)
  const reloaded = PrivateKey.fromHex(privHex, KeyAlgorithm.SECP256K1);
  const ok = reloaded.publicKey.toHex() === pubHex
    && reloaded.publicKey.accountHash().toPrefixedString() === accountHash;
  if (privHex.length !== 64 || !ok) throw new Error(`${label}: key invalid (len=${privHex.length}, roundtrip=${ok})`);
  return { label, privHex, pubHex, accountHash, pem };
}

mkdirSync("../secrets", { recursive: true });
const treasury = await makeWallet("treasury");
const agent = await makeWallet("agent");

writeFileSync("../secrets/treasury.pem", treasury.pem, { mode: 0o600 });
writeFileSync("../secrets/agent.pem", agent.pem, { mode: 0o600 });
writeFileSync("../secrets/wallets.testnet.json", JSON.stringify({
  network: "casper-test",
  generatedFor: "Cadence testnet go-live",
  treasury: { privateKeyHex: treasury.privHex, publicKeyHex: treasury.pubHex, accountHash: treasury.accountHash },
  agent:    { privateKeyHex: agent.privHex,    publicKeyHex: agent.pubHex,    accountHash: agent.accountHash },
}, null, 2) + "\n", { mode: 0o600 });

const envBlock =
`# --- Cadence testnet wallets (secp256k1) — generated, KEEP SECRET ---
TREASURY_PRIVATE_KEY=${treasury.privHex}
TREASURY_ACCOUNT_HASH=${treasury.accountHash}
AGENT_PRIVATE_KEY=${agent.privHex}
AGENT_ACCOUNT_HASH=${agent.accountHash}
`;
writeFileSync("../secrets/wallets.env", envBlock, { mode: 0o600 });

console.log("OK — both keypairs valid, round-trip verified, 64-hex raw keys.\n");
console.log("Treasury account hash:", treasury.accountHash);
console.log("Treasury public key  :", treasury.pubHex);
console.log("Agent    account hash:", agent.accountHash);
console.log("Agent    public key  :", agent.pubHex);
console.log("\nWrote: secrets/wallets.testnet.json, secrets/wallets.env, secrets/treasury.pem, secrets/agent.pem");
