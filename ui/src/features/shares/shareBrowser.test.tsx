import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NetworkShareButton } from "./NetworkShareButton";

const transport = window as unknown as Record<string, unknown>;

afterEach(() => {
  delete transport.__CINEVAULT_TRANSPORT__;
});

describe("Network share… in a browser", () => {
  it("opens the page in the desktop app", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<NetworkShareButton onOpen={onOpen}>Network share…</NetworkShareButton>);
    await user.click(screen.getByRole("button", { name: "Network share…" }));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("explains the installer in a popover, with the command to copy and no password field", async () => {
    transport.__CINEVAULT_TRANSPORT__ = "server";
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onOpen = vi.fn();
    render(<NetworkShareButton onOpen={onOpen}>Network share…</NetworkShareButton>);
    await user.click(screen.getByRole("button", { name: "Network share…" }));

    const popover = await screen.findByRole("dialog", { name: "Network shares in a browser" });
    expect(onOpen).not.toHaveBeenCalled();
    expect(within(popover).getByText("Network shares are connected on the server")).toBeVisible();
    expect(within(popover).getByText(/the server can't connect a share by itself/i)).toBeVisible();
    expect(within(popover).getByText(/install\.sh \| sh/)).toBeVisible();
    expect(within(popover).getByText("Then add the folder with Add folder….")).toBeVisible();
    expect(screen.queryByLabelText("Password")).toBeNull();

    await user.click(within(popover).getByRole("button", { name: "Copy the command" }));
    expect(writeText).toHaveBeenCalledWith(
      "curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh",
    );
  });
});
