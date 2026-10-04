import { generateKeyPairSync } from "node:crypto";
import { readFileSync, writeFileSync, chmodSync, mkdirSync } from "node:fs";
import { loadEnvFile } from "node:process";
import bs58 from "bs58";

// Creates an unfunded, dedicated wallet. Does not sign or send transactions.
try {
  loadEnvFile(".env");
  if (process.env.ALIGHT_CANARY_KEYPAIR?.trim()) {
    console.log("Existing canary wallet preserved; no key generated.");
  } else {
    const { privateKey, publicKey } = generateKeyPairSync("ed25519");
    const seed = privateKey.export({ type: "pkcs8", format: "der" }).subarray(-32);
    const pubkey = publicKey.export({ type: "spki", format: "der" }).subarray(-32);
    const keypair = bs58.encode(Buffer.concat([seed, pubkey]));
    const address = bs58.encode(pubkey);
    const env = readFileSync(".env", "utf8");
    if (!/^ALIGHT_CANARY_KEYPAIR=.*$/m.test(env)) throw new Error("missing field");
    writeFileSync(".env", env.replace(/^ALIGHT_CANARY_KEYPAIR=.*$/m,
      `ALIGHT_CANARY_KEYPAIR=${JSON.stringify(keypair)}`), { mode: 0o600 });
    chmodSync(".env", 0o600);
    mkdirSync(".alight", { recursive: true, mode: 0o700 });
    writeFileSync(".alight/canary-wallet.json", JSON.stringify({ address, funded: false,
      purpose: "dedicated Alight canary payer", created_at: new Date().toISOString() }, null, 2) + "\n",
      { flag: "wx", mode: 0o600 });
    console.log(`Dedicated canary address: ${address}`);
    console.log("Private key saved only in ignored .env. No funds moved or transactions sent.");
  }
} catch {
  console.error("Wallet setup failed; credential-bearing details withheld.");
  process.exitCode = 2;
}
