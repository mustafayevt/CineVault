import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { handleEscape } from "../shell/escStack";
import { NetworkShareForm } from "./NetworkShareForm";
import type { ShareMode } from "./NetworkShareForm";
import { libraryFolderPath, parseShareAddress } from "./shareAddress";
import { ShareRequestError, setSharesBackend } from "./sharesBackend";
import type { NetworkShare, ShareSupport, SharesBackend } from "./sharesBackend";

const WINDOWS: ShareSupport = { supported: true, system: "windows", mount_base: null };
const MAC: ShareSupport = { supported: true, system: "macos", mount_base: "~/CineVault Shares" };
const LINUX: ShareSupport = { supported: true, system: "linux", mount_base: "/run/user/1000/gvfs" };
const NO_MOUNTING: ShareSupport = { ...LINUX, supported: false };

const ADDED = {
  share: { id: 1, address: "\\\\nas\\movies", user: "alex", remember: true, reason: null },
  source: {
    id: 7,
    kind: "movie" as const,
    path: "\\\\nas\\movies\\Films",
    label: "Films",
    position: 0,
  },
};

const OFFLINE: NetworkShare = {
  id: 3,
  address: "\\\\nas\\movies",
  user: "alex",
  remember: true,
  reason: "wrong_password",
};

function fake(overrides: Partial<SharesBackend> = {}) {
  const backend: SharesBackend = {
    support: () => Promise.resolve(WINDOWS),
    connect: vi.fn(() => Promise.resolve(ADDED)),
    connectAgain: vi.fn(() => Promise.resolve({ ...OFFLINE, reason: null })),
    cancel: vi.fn(() => Promise.resolve(true)),
    list: () => Promise.resolve([OFFLINE]),
    ...overrides,
  };
  setSharesBackend(backend);
  return backend;
}

function renderForm(
  mode: ShareMode = { kind: "connect", library: "movie" },
  support: ShareSupport = WINDOWS,
) {
  const onDone = vi.fn();
  const onCancel = vi.fn();
  const onAddFolder = vi.fn();
  render(
    <NetworkShareForm
      mode={mode}
      support={support}
      onDone={onDone}
      onCancel={onCancel}
      onAddFolder={onAddFolder}
    />,
  );
  return { onDone, onCancel, onAddFolder };
}

/** A connect that stays open until the test settles it. */
function pending() {
  let settle: (outcome: "added" | ShareRequestError) => void = () => undefined;
  const connect = vi.fn(
    () =>
      new Promise<typeof ADDED>((resolve, reject) => {
        settle = (outcome) => {
          if (outcome === "added") resolve(ADDED);
          else reject(outcome);
        };
      }),
  );
  return { connect, settle: (outcome: "added" | ShareRequestError) => settle(outcome) };
}

async function fill(user: ReturnType<typeof userEvent.setup>, address: string, password = "pw") {
  await user.type(screen.getByLabelText("Address"), address);
  await user.type(screen.getByLabelText("User"), "alex");
  await user.type(screen.getByLabelText("Password"), password);
}

afterEach(() => {
  setSharesBackend(null);
});

describe("the address typed on the Network share page", () => {
  it("reads the forms people paste", () => {
    for (const text of ["\\\\nas\\movies\\Films", "//nas/movies/Films", "smb://nas/movies/Films"]) {
      expect(parseShareAddress(text), text).toEqual({
        server: "nas",
        share: "movies",
        folder: ["Films"],
        user: null,
      });
    }
    expect(parseShareAddress("smb://alex@nas/my%20films/A/B/")).toEqual({
      server: "nas",
      share: "my films",
      folder: ["A", "B"],
      user: "alex",
    });
    expect(parseShareAddress("\\\\nas")).toBeNull();
    expect(parseShareAddress("movies")).toBeNull();
  });

  it("shows where the library folder will be on each system", () => {
    const address = parseShareAddress("smb://NAS/Movies/Films") ?? {
      server: "",
      share: "",
      folder: [],
      user: null,
    };
    expect(address.server).toBe("NAS");
    expect(libraryFolderPath(address, WINDOWS)).toBe("\\\\NAS\\Movies\\Films");
    expect(libraryFolderPath(address, MAC)).toBe("~/CineVault Shares/NAS/Movies/Films");
    expect(libraryFolderPath(address, LINUX)).toBe(
      "/run/user/1000/gvfs/smb-share:server=nas,share=movies/Films",
    );
  });
});

describe("the Network share page", () => {
  it("starts empty: nothing to connect yet, and the library folder fills in from the address", async () => {
    fake();
    renderForm();
    expect(screen.getByText("Fills in from the address")).toBeVisible();
    expect(screen.getByLabelText("Address")).toHaveFocus();
    expect(screen.getByLabelText("Address")).toHaveAttribute("placeholder", "\\\\nas\\movies");
    expect(screen.getByText(/DOMAIN\\name also works/)).toBeVisible();
    expect(screen.getByRole("checkbox", { name: "Remember the password" })).toBeChecked();
    expect(screen.getByText("Kept in Windows Credential Manager")).toBeVisible();
    expect(screen.getByRole("button", { name: "Connect and add" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.getByText(/adds the folder to Movies/)).toBeVisible();
  });

  it("shows the library folder of a pasted address and takes the user an smb address names", async () => {
    const user = userEvent.setup();
    fake();
    renderForm();
    await user.click(screen.getByLabelText("Address"));
    await user.paste("smb://alex@nas/movies/Films");
    expect(screen.getByText("\\\\nas\\movies\\Films")).toBeVisible();
    expect(screen.getByLabelText("User")).toHaveValue("alex");
  });

  it("connects with what was typed, then reports what was added", async () => {
    const user = userEvent.setup();
    const backend = fake();
    const { onDone } = renderForm({ kind: "connect", library: "tv" });
    await fill(user, "\\\\nas\\movies\\Films", "Example-Pass-42");
    await user.click(screen.getByRole("button", { name: "Connect and add" }));
    await waitFor(() => {
      expect(onDone).toHaveBeenCalledWith({ kind: "added", added: ADDED });
    });
    expect(backend.connect).toHaveBeenCalledWith({
      address: "\\\\nas\\movies\\Films",
      kind: "tv",
      user: "alex",
      password: "Example-Pass-42",
      remember: true,
    });
  });

  it("shows and hides the password on request", async () => {
    const user = userEvent.setup();
    fake();
    renderForm();
    const field = screen.getByLabelText("Password");
    expect(field).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Show password" }));
    expect(field).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "Hide password" }));
    expect(field).toHaveAttribute("type", "password");
  });

  it("names the credential store of each system", () => {
    fake();
    for (const [support, text] of [
      [MAC, "Kept in the macOS Keychain"],
      [LINUX, "Kept in the system keyring"],
    ] as const) {
      const { unmount } = render(
        <NetworkShareForm
          mode={{ kind: "connect", library: null }}
          support={support}
          onDone={vi.fn()}
          onCancel={vi.fn()}
          onAddFolder={vi.fn()}
        />,
      );
      expect(screen.getByText(text)).toBeVisible();
      unmount();
    }
  });

  it("is busy while it connects: Connecting…, read-only fields, and Esc stops the tool", async () => {
    const user = userEvent.setup();
    const wait = pending();
    const backend = fake({ connect: wait.connect });
    renderForm();
    await fill(user, "\\\\nas\\movies");
    await user.click(screen.getByRole("button", { name: "Connect and add" }));

    const busy = await screen.findByRole("button", { name: "Connecting…" });
    expect(busy).toHaveAttribute("aria-busy", "true");
    expect(screen.getByLabelText("Address")).toHaveAttribute("readonly");
    expect(screen.getByLabelText("User")).toHaveAttribute("readonly");
    expect(screen.getByLabelText("Password")).toHaveAttribute("readonly");
    expect(screen.getByRole("checkbox", { name: "Remember the password" })).toBeDisabled();

    // Esc in a field that holds text stops the connect, and nothing typed is lost.
    await user.type(screen.getByLabelText("Address"), "{Escape}");
    expect(backend.cancel).toHaveBeenCalledTimes(1);
    await act(async () => {
      wait.settle(new ShareRequestError("cancelled", "Connecting was stopped."));
      await Promise.resolve();
    });
    expect(await screen.findByRole("button", { name: "Connect and add" })).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByLabelText("Address")).toHaveValue("\\\\nas\\movies");
    expect(screen.getByLabelText("Password")).toHaveValue("pw");
  });

  it("leaves with Cancel, and with Esc through the Esc stack", async () => {
    const user = userEvent.setup();
    fake();
    const { onCancel } = renderForm();
    await user.click(screen.getByRole("button", { name: /^Cancel/ }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    act(() => {
      expect(handleEscape({ defaultPrevented: false, target: document.body })).toBe(true);
    });
    expect(onCancel).toHaveBeenCalledTimes(2);
  });

  it("stops the connect, not the page, when Cancel is pressed while connecting", async () => {
    const user = userEvent.setup();
    const wait = pending();
    const backend = fake({ connect: wait.connect });
    const { onCancel } = renderForm();
    await fill(user, "\\\\nas\\movies");
    await user.click(screen.getByRole("button", { name: "Connect and add" }));
    await screen.findByRole("button", { name: "Connecting…" });
    await user.click(screen.getByRole("button", { name: /^Cancel/ }));
    expect(backend.cancel).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });
});

describe("what the page says when a share does not connect", () => {
  async function refused(code: string, address = "\\\\nas\\movies\\Films", message = "system") {
    const user = userEvent.setup();
    const backend = fake({
      connect: vi.fn(() => Promise.reject(new ShareRequestError(code, message))),
    });
    renderForm();
    await fill(user, address, "secret");
    await user.click(screen.getByRole("button", { name: "Connect and add" }));
    await screen.findByRole("button", { name: "Connect and add" });
    // What was typed stays.
    expect(screen.getByLabelText("Address")).toHaveValue(address);
    expect(screen.getByLabelText("User")).toHaveValue("alex");
    expect(screen.getByLabelText("Password")).toHaveValue("secret");
    return { user, backend };
  }

  it("says a wrong user name or password under the Password field", async () => {
    await refused("wrong_login");
    expect(screen.getByLabelText("Password")).toBeInvalid();
    expect(screen.getByText("Wrong user name or password.")).toBeVisible();
  });

  it("says a server that does not answer under the Address field", async () => {
    await refused("unreachable");
    expect(screen.getByLabelText("Address")).toBeInvalid();
    expect(
      screen.getByText(
        "nas could not be reached. Check the name or address and that it is switched on.",
      ),
    ).toBeVisible();
  });

  it("says a share that is not on the server", async () => {
    await refused("share_not_found");
    expect(screen.getByText("There is no share named movies on nas.")).toBeVisible();
  });

  it("says a folder that is not on the share", async () => {
    await refused("folder_not_found");
    expect(screen.getByText("Films was not found on \\\\nas\\movies.")).toBeVisible();
  });

  it("explains another Windows login to the same server in a banner", async () => {
    await refused("other_user_connected");
    const banner = screen.getByRole("alert");
    expect(
      within(banner).getByText("Windows is already connected to nas as another user."),
    ).toBeVisible();
    expect(within(banner).getByText(/Windows allows one login per server/)).toBeVisible();
  });

  it("tells a leftover Windows session from another user and says to restart", async () => {
    await refused("session_in_use");
    const banner = screen.getByRole("alert");
    expect(
      within(banner).getByText("Windows is still signed in to nas with the earlier password."),
    ).toBeVisible();
    expect(
      within(banner).getByText(
        "Restart CineVault (or sign out of Windows and back in), then connect again.",
      ),
    ).toBeVisible();
    expect(within(banner).queryByText(/another user/)).toBeNull();
  });

  it("shows the folder rules' refusal and the system's own message in a banner", async () => {
    await refused("inside_existing", "\\\\nas\\movies\\Films", "Films is inside D:\\Movies.");
    expect(
      within(screen.getByRole("alert")).getByText("Films is inside D:\\Movies."),
    ).toBeVisible();
  });

  it("offers to connect without remembering when no keyring is running", async () => {
    const user = userEvent.setup();
    const connect = vi
      .fn()
      .mockRejectedValueOnce(new ShareRequestError("no_keyring", "No system keyring is running."))
      .mockResolvedValueOnce(ADDED);
    fake({ connect });
    const { onDone } = renderForm();
    await fill(user, "\\\\nas\\movies");
    await user.click(screen.getByRole("button", { name: "Connect and add" }));
    expect(
      await screen.findByText("No system keyring is running, so the password can't be remembered."),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Connect without remembering" }));
    await waitFor(() => {
      expect(onDone).toHaveBeenCalled();
    });
    expect(connect).toHaveBeenLastCalledWith(expect.objectContaining({ remember: false }));
  });
});

describe("connecting a share that is offline", () => {
  it("fixes the address, fills in the user and asks for the password", async () => {
    const user = userEvent.setup();
    const backend = fake();
    const { onDone } = renderForm({ kind: "again", share: OFFLINE });
    expect(screen.getByLabelText("Address")).toHaveValue("\\\\nas\\movies");
    expect(screen.getByLabelText("Address")).toHaveAttribute("readonly");
    expect(screen.getByLabelText("User")).toHaveValue("alex");
    expect(screen.getByLabelText("User")).not.toHaveAttribute("readonly");
    expect(screen.getByLabelText("Password")).toHaveFocus();
    expect(
      screen.getByText(/The saved password for alex no longer works on \\\\nas\\movies/),
    ).toBeVisible();

    await user.type(screen.getByLabelText("Password"), "new-secret");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => {
      expect(onDone).toHaveBeenCalledWith({
        kind: "again",
        share: { ...OFFLINE, reason: null },
      });
    });
    expect(backend.connectAgain).toHaveBeenCalledWith({
      id: 3,
      user: "alex",
      password: "new-secret",
      remember: true,
    });
  });

  it("says the password was not kept when it was not remembered", () => {
    fake();
    renderForm({
      kind: "again",
      share: { ...OFFLINE, remember: false, reason: "password_needed" },
    });
    expect(
      screen.getByText(/does not keep the password for alex on \\\\nas\\movies/),
    ).toBeVisible();
    expect(screen.getByRole("checkbox", { name: "Remember the password" })).not.toBeChecked();
  });
});

describe("a Linux desktop that cannot connect shares", () => {
  it("explains the system's own way instead of asking for a password", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    fake();
    const { onAddFolder } = renderForm({ kind: "connect", library: "movie" }, NO_MOUNTING);
    expect(screen.getByText("This desktop can't connect network shares for apps")).toBeVisible();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(screen.queryByLabelText("Address")).toBeNull();
    expect(screen.getByText(/sudo mount -t cifs \/\//)).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Copy the command" }));
    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining("sudo mount -t cifs //SERVER/SHARE /mnt/SHARE -o username=USER"),
    );
    await user.click(screen.getByRole("button", { name: "Add folder…" }));
    expect(onAddFolder).toHaveBeenCalledTimes(1);
  });
});
