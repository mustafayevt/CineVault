// The update-manifest signature, shared by release-keys.mjs and release-publish.mjs
// (checked in the app by cv_app::updates::verify_manifest).
//
// The envelope is { "payload": <base64 of the exact payload bytes>, "signature": <base64 ed25519> }.
import { createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Whether `path` is inside this repository (a private key must never be). */
export const insideRepo = (path) => {
  const rel = relative(repoRoot, resolve(path));
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
};

/** The raw 32-byte ed25519 public key of a KeyObject. */
export const rawPublicKey = (publicKey) =>
  Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");

/** Reads a PKCS#8 PEM private key. The key itself is never returned as text or logged. */
export const loadPrivateKey = (path) => createPrivateKey(readFileSync(path));

/** The base64 public key that belongs to a private KeyObject. */
export const publicKeyText = (privateKey) =>
  rawPublicKey(createPublicKey(privateKey)).toString("base64");

/** Signs the exact payload bytes and returns the envelope text (no trailing newline). */
export function signPayload(payload, privateKey) {
  const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload, "utf8");
  JSON.parse(bytes.toString("utf8")); // must be valid JSON
  const signature = sign(null, bytes, privateKey);
  return JSON.stringify({ payload: bytes.toString("base64"), signature: signature.toString("base64") });
}

/** A public KeyObject from base64 of the raw 32 bytes, as BUILT_IN_PUBLIC_KEY holds it. */
export function publicKeyFromText(text) {
  const raw = Buffer.from(text.trim(), "base64");
  if (raw.length !== 32) throw new Error("the public key is not 32 bytes of base64");
  return createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: raw.toString("base64url") }, format: "jwk" });
}

/**
 * Checks an envelope the way the app does: the signature over the payload bytes first, then the
 * payload as JSON. Returns the parsed payload; throws when anything is wrong.
 */
export function verifyEnvelope(envelopeText, publicKeyBase64) {
  const envelope = JSON.parse(envelopeText);
  const payload = Buffer.from(String(envelope.payload ?? ""), "base64");
  const signature = Buffer.from(String(envelope.signature ?? ""), "base64");
  if (signature.length !== 64 || !verify(null, payload, publicKeyFromText(publicKeyBase64), signature)) {
    throw new Error("the manifest signature does not verify with the public key");
  }
  return JSON.parse(payload.toString("utf8"));
}

/** The public key built into the app (crates/cv-app/src/updates/manifest.rs), or null. */
export function builtInPublicKey(root = repoRoot) {
  const source = readFileSync(resolve(root, "crates/cv-app/src/updates/manifest.rs"), "utf8");
  const match = /pub const BUILT_IN_PUBLIC_KEY: Option<&str> = Some\("([A-Za-z0-9+/=]+)"\)/.exec(source);
  return match ? match[1] : null;
}
