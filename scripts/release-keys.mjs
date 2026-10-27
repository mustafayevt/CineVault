// Maintainer tool for the update-signing key pair (ed25519; the public half is built into the
// app, the private half signs the update manifest).
//
//   node scripts/release-keys.mjs keygen --out <private-key.pem>
//       Creates an ed25519 key pair. The private key is written to <private-key.pem>, which must
//       be OUTSIDE this repository. The public key (base64, 32 bytes) is printed: paste it into
//       BUILT_IN_PUBLIC_KEY in crates/cv-app/src/updates/manifest.rs.
//
//   node scripts/release-keys.mjs sign --key <private-key.pem> --payload <payload.json> [--out <manifest.json>]
//       Wraps the exact bytes of the payload file in the signed envelope the app expects.
//       `bun run release:publish` does this (and everything around it) for a whole release.
import { generateKeyPairSync } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { insideRepo, loadPrivateKey, publicKeyText, rawPublicKey, signPayload } from "./update-signing.mjs";

const [command, ...rest] = process.argv.slice(2);
const options = {};
for (let i = 0; i < rest.length; i += 2) options[rest[i].replace(/^--/, "")] = rest[i + 1];

const die = (message) => {
  console.error(message);
  process.exit(1);
};

if (command === "keygen") {
  if (!options.out) die("Usage: keygen --out <private-key.pem>");
  if (insideRepo(options.out)) die("Refusing to write the private key inside the repository.");
  if (existsSync(options.out)) die(`${options.out} already exists; it is not overwritten.`);
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  writeFileSync(options.out, privateKey.export({ format: "pem", type: "pkcs8" }), { mode: 0o600 });
  console.log(`Private key written to ${options.out}. Keep it secret and back it up.`);
  console.log(`Public key (BUILT_IN_PUBLIC_KEY): ${rawPublicKey(publicKey).toString("base64")}`);
} else if (command === "sign") {
  if (!options.key || !options.payload) die("Usage: sign --key <private-key.pem> --payload <payload.json> [--out <manifest.json>]");
  const privateKey = loadPrivateKey(options.key);
  const envelope = signPayload(readFileSync(options.payload), privateKey);
  if (options.out) {
    writeFileSync(options.out, envelope + "\n");
    console.log(`Signed manifest written to ${options.out}`);
    console.log(`Public key of this signature: ${publicKeyText(privateKey)}`);
  } else {
    console.log(envelope);
  }
} else {
  die("Commands: keygen, sign");
}
