import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BrowserAccessSection } from "./BrowserAccessSection";
import {
  BrowserAccessRequestError,
  browserLabel,
  createSyntheticBrowserAccess,
  setBrowserAccessBackend,
} from "./browserAccess";
import type { BrowserAccess, BrowserAccessBackend } from "./browserAccess";

afterEach(() => {
  setBrowserAccessBackend(null);
  vi.restoreAllMocks();
});

function use(backend: BrowserAccessBackend) {
  setBrowserAccessBackend(backend);
  render(<BrowserAccessSection />);
}

const PASSWORD = "a long enough password";

describe("Settings › Browser access", () => {
  it("is off and cannot be switched on before a password exists", async () => {
    use(createSyntheticBrowserAccess());
    const toggle = await screen.findByRole("switch", { name: "Browser access" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeDisabled();
    expect(screen.getByText("Off")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create password" })).toBeInTheDocument();
    expect(
      screen.getAllByText("Windows will ask to allow CineVault on private networks. Choose Allow.")
        .length,
    ).toBeGreaterThan(0);
    // No address is shown while it does not run.
    expect(screen.queryByText(/http:\/\//)).not.toBeInTheDocument();
  });

  it("creates a password, switches on and shows the addresses with a QR code", async () => {
    const user = userEvent.setup();
    use(createSyntheticBrowserAccess());
    await user.type(
      await screen
        .findByRole("textbox", { name: /New password/ })
        .catch(() => screen.getByLabelText("New password (at least 12 characters)")),
      PASSWORD,
    );
    await user.type(screen.getByLabelText("Confirm the new password"), PASSWORD);
    await user.click(screen.getByRole("button", { name: "Create password" }));
    const toggle = screen.getByRole("switch", { name: "Browser access" });
    await waitFor(() => expect(toggle).toBeEnabled());
    await user.click(toggle);
    await screen.findByText("Running");
    expect(screen.getByText("http://192.168.1.20:8787")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:8787")).toBeInTheDocument();
    const qr = screen.getByRole("img", { name: "QR code for http://192.168.1.20:8787" });
    expect(qr).toHaveAttribute("data-qr", "http://192.168.1.20:8787");
    expect(qr.querySelector("path")?.getAttribute("d")).toMatch(/^M\d+ \d+h1v1h-1z/);
  });

  it("refuses a short password with the reason", async () => {
    const user = userEvent.setup();
    use(createSyntheticBrowserAccess());
    await user.type(await screen.findByLabelText("New password (at least 12 characters)"), "short");
    await user.type(screen.getByLabelText("Confirm the new password"), "short");
    await user.click(screen.getByRole("button", { name: "Create password" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The password must be at least 12 characters.",
    );
    expect(screen.getByRole("switch", { name: "Browser access" })).toBeDisabled();
  });

  it("copies an address and says so", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    use(createSyntheticBrowserAccess({ hasPassword: true, enabled: true }));
    await user.click(await screen.findByRole("button", { name: "Copy http://192.168.1.20:8787" }));
    expect(writeText).toHaveBeenCalledWith("http://192.168.1.20:8787");
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("lists signed-in browsers by name and signs one out", async () => {
    const user = userEvent.setup();
    use(createSyntheticBrowserAccess({ hasPassword: true, enabled: true }));
    const line = (await screen.findByText("Safari on iPhone")).closest("li");
    expect(line).not.toBeNull();
    expect(within(line as HTMLElement).getByText(/192\.168\.1\.31/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Sign out Safari on iPhone" }));
    expect(await screen.findByText("No browser is signed in.")).toBeInTheDocument();
  });

  it("shows the port-in-use sentence and applies a new port", async () => {
    const user = userEvent.setup();
    const base = createSyntheticBrowserAccess({ hasPassword: true });
    let state: BrowserAccess = {
      ...(await base.get()),
      enabled: true,
      state: "portInUse",
      message: "Port 8787 is in use by another program. Choose another port, such as 8788.",
    };
    const setPort = vi.fn((port: number) => {
      state = {
        ...state,
        port,
        state: "running",
        message: null,
        addresses: [`http://192.168.1.20:${port}`, `http://localhost:${port}`],
      };
      return Promise.resolve(state);
    });
    use({ ...base, get: () => Promise.resolve(state), setPort });
    expect(await screen.findByText("Port in use")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose another port, such as 8788.");
    const field = screen.getByRole("textbox", { name: "Port" });
    await user.clear(field);
    await user.type(field, "8788");
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(setPort).toHaveBeenCalledWith(8788);
    expect(await screen.findByText("http://192.168.1.20:8788")).toBeInTheDocument();
  });

  it("refuses a port below 1024 without asking the desktop", async () => {
    const user = userEvent.setup();
    const base = createSyntheticBrowserAccess({ hasPassword: true });
    const setPort = vi.fn(base.setPort);
    use({ ...base, setPort });
    const field = await screen.findByRole("textbox", { name: "Port" });
    await user.clear(field);
    await user.type(field, "80");
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Choose a port from 1024 to 65535.")).toBeInTheDocument();
    expect(setPort).not.toHaveBeenCalled();
  });

  it("warns when home-network-only is turned off", async () => {
    const user = userEvent.setup();
    use(createSyntheticBrowserAccess({ hasPassword: true }));
    const lan = await screen.findByRole("switch", { name: "Only devices on my home network" });
    expect(lan).toHaveAttribute("aria-checked", "true");
    await user.click(lan);
    expect(
      await screen.findByText(/Anyone who can reach this port can try the password/),
    ).toBeInTheDocument();
  });

  it("switches the tray and the start at sign-in", async () => {
    const user = userEvent.setup();
    use(createSyntheticBrowserAccess({ hasPassword: true }));
    const tray = await screen.findByRole("switch", {
      name: "Keep running in the tray when the window is closed",
    });
    const login = screen.getByRole("switch", { name: "Start CineVault when I sign in" });
    await user.click(tray);
    await user.click(login);
    await waitFor(() => expect(tray).toHaveAttribute("aria-checked", "true"));
    expect(login).toHaveAttribute("aria-checked", "true");
  });

  it("shows a refusal from the desktop", async () => {
    const user = userEvent.setup();
    const base = createSyntheticBrowserAccess({ hasPassword: true });
    use({
      ...base,
      setEnabled: () =>
        Promise.reject(new BrowserAccessRequestError("unavailable", "The library is not open.")),
    });
    await user.click(await screen.findByRole("switch", { name: "Browser access" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The library is not open.");
  });

  it("reads the state again while it is shown", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const base = createSyntheticBrowserAccess({ hasPassword: true, enabled: true });
    const get = vi.fn(base.get);
    use({ ...base, get });
    await screen.findByText("Running");
    const before = get.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5100);
    });
    expect(get.mock.calls.length).toBeGreaterThan(before);
    vi.useRealTimers();
  });
});

describe("browser names", () => {
  it("names common browsers and systems", () => {
    expect(
      browserLabel(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
      ),
    ).toEqual({ browser: "Edge", system: "Windows" });
    expect(
      browserLabel(
        "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
      ),
    ).toEqual({ browser: "Chrome", system: "Android" });
    expect(
      browserLabel("Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0"),
    ).toEqual({ browser: "Firefox", system: "Linux" });
    expect(browserLabel("curl/8.9.1")).toBeNull();
  });
});
