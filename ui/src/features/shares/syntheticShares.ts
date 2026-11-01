import { ShareRequestError } from "./sharesBackend";
import type { NetworkShare, ShareSupport, SharesBackend } from "./sharesBackend";

/** The shares a plain browser lists with `?synthetic-folders`: one online, one that wants its password. */
export const SYNTHETIC_SHARES: readonly NetworkShare[] = [
  { id: 1, address: "\\\\nas\\movies", user: "alex", remember: true, reason: null },
  { id: 2, address: "\\\\backup\\movies", user: "alex", remember: true, reason: "wrong_password" },
];

/** How long a pretend connect takes, so the busy state can be seen. */
const CONNECT_MS = 600;

function supportFor(system: string | null): ShareSupport {
  if (system === "macos")
    return { supported: true, system: "macos", mount_base: "~/CineVault Shares" };
  if (system === "linux" || system === "linux-unsupported") {
    return {
      supported: system === "linux",
      system: "linux",
      mount_base: "/run/user/1000/gvfs",
    };
  }
  return { supported: true, system: "windows", mount_base: null };
}

/**
 * Shares held in memory, for developing the Network share page in a plain browser. The page asks
 * for what the address says, so a try-out needs no NAS: a password of `wrong` is refused, and an
 * address holding `down`, `noshare`, `nofolder` or `otheruser` fails the way a server that is
 * off, a share that is not there, a missing folder or another Windows login would. `?share-system=`
 * `macos`, `linux` or `linux-unsupported` shows the page of that system.
 */
export function createSyntheticSharesBackend(params: URLSearchParams): SharesBackend {
  const support = supportFor(params.get("share-system"));
  let stopped = false;
  let next = SYNTHETIC_SHARES.length + 1;
  const attempt = <T>(fail: ShareRequestError | null, done: () => T): Promise<T> => {
    stopped = false;
    return new Promise<T>((resolve, reject) => {
      window.setTimeout(() => {
        if (stopped) reject(new ShareRequestError("cancelled", "Connecting was stopped."));
        else if (fail !== null) reject(fail);
        else resolve(done());
      }, CONNECT_MS);
    });
  };
  return {
    support: () => Promise.resolve(support),
    connect: (input) => {
      const address = input.address.toLowerCase();
      const refused = (code: string, message: string) => new ShareRequestError(code, message);
      let fail: ShareRequestError | null = null;
      if (input.password === "wrong") fail = refused("wrong_login", "Wrong user name or password.");
      else if (address.includes("down"))
        fail = refused("unreachable", "The server could not be reached.");
      else if (address.includes("noshare"))
        fail = refused("share_not_found", "There is no share by that name on the server.");
      else if (address.includes("nofolder"))
        fail = refused("folder_not_found", "The folder was not found on the share.");
      else if (address.includes("otheruser"))
        fail = refused(
          "other_user_connected",
          "Windows is already connected to this server as another user.",
        );
      else if (address.includes("nokeyring") && input.remember)
        fail = refused("no_keyring", "No system keyring is running.");
      return attempt(fail, () => {
        const id = next;
        next += 1;
        const unc = input.address
          .trim()
          .replace(/^smb:\/\//i, "\\\\")
          .replaceAll("/", "\\");
        return {
          share: { id, address: unc, user: input.user, remember: input.remember, reason: null },
          source: {
            id: id + 100,
            kind: input.kind ?? "movie",
            path: unc,
            label: unc.split("\\").pop() ?? unc,
            position: 0,
          },
        };
      });
    },
    connectAgain: (input) =>
      attempt(
        input.password === "wrong"
          ? new ShareRequestError("wrong_login", "Wrong user name or password.")
          : null,
        () => ({
          id: input.id,
          address: "\\\\backup\\movies",
          user: input.user,
          remember: input.remember,
          reason: null,
        }),
      ),
    cancel: () => {
      stopped = true;
      return Promise.resolve(true);
    },
    list: () => Promise.resolve([...SYNTHETIC_SHARES]),
  };
}
