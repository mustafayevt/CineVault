import { t } from "../../i18n/catalog";

/** Formats a text setting can require. An empty text is left to the setting's own rules. */
export type TextFormat = "https-url" | "url" | "signing-key";

/** Hosts plain http may be used for: this computer, where the local test channel runs. */
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** Why `text` is not a web address the update checker would request, or null when it is. */
function httpsUrlError(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return t("settings.control.format.httpsUrl");
  }
  if (url.protocol === "https:" && url.hostname !== "") return null;
  if (url.protocol === "http:" && LOCAL_HOSTS.has(url.hostname)) return null;
  return t("settings.control.format.httpsUrl");
}

/** Why `text` is not a web address starting with http:// or https://, or null when it is. */
function webUrlError(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return t("settings.control.format.url");
  }
  const web = url.protocol === "https:" || url.protocol === "http:";
  const plain = url.username === "" && url.password === "" && url.search === "" && url.hash === "";
  return web && plain && url.hostname !== "" ? null : t("settings.control.format.url");
}

/** An Ed25519 public key in base64: 32 bytes, 44 characters ending in one `=`. */
const SIGNING_KEY = /^[A-Za-z0-9+/]{43}=$/;

/** Why `text` breaks `format`, or null when it is fine. The text is trimmed first. */
export function textFormatError(format: TextFormat, text: string): string | null {
  const value = text.trim();
  if (value === "") return null;
  switch (format) {
    case "https-url":
      return httpsUrlError(value);
    case "url":
      return webUrlError(value);
    case "signing-key":
      return SIGNING_KEY.test(value) ? null : t("settings.control.format.signingKey");
  }
}
