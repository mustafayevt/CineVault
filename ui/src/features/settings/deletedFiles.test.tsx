import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { moveSpeedText, timeLeftText } from "../library-ops/moveModel";
import { DeletedFilesSection } from "./DeletedFilesSection";
import { useSettingsStore } from "./settingsStore";

beforeEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});
afterEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});

describe("Settings › Deleted files", () => {
  it("starts on the Recycle Bin and switches to the trash folder at once", async () => {
    const user = userEvent.setup();
    render(<DeletedFilesSection />);
    expect(screen.getByRole("radio", { name: /^Recycle Bin/ })).toBeChecked();
    await user.click(screen.getByRole("radio", { name: /Trash folder on each drive/ }));
    expect(useSettingsStore.getState().values["files.deleted-mode"]).toBe("folder");
  });

  it("switches Ask before deleting, which stays on and locked under Delete permanently", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<DeletedFilesSection />);
    const ask = screen.getByRole("switch", { name: "Ask before deleting" });
    expect(ask).toHaveAttribute("aria-checked", "true");
    await user.click(ask);
    expect(useSettingsStore.getState().values["delete.confirm"]).toBe(false);
    unmount();

    useSettingsStore.setState({
      values: { "files.deleted-mode": "erase", "delete.confirm": false },
      status: "ready",
    });
    render(<DeletedFilesSection />);
    const locked = screen.getByRole("switch", { name: "Ask before deleting" });
    expect(locked).toHaveAttribute("aria-checked", "true");
    expect(locked).toBeDisabled();
  });

  it("chooses Delete permanently only after the typed confirmation", async () => {
    const user = userEvent.setup();
    render(<DeletedFilesSection />);
    expect(
      screen.getByText(
        "Deleted and replaced files are erased at once. Undo can't bring them back.",
      ),
    ).toBeVisible();
    await user.click(screen.getByRole("radio", { name: /Delete permanently/ }));
    const dialog = await screen.findByRole("dialog");
    expect(useSettingsStore.getState().values["files.deleted-mode"]).toBeUndefined();
    // Cancel keeps the Recycle Bin.
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(useSettingsStore.getState().values["files.deleted-mode"]).toBeUndefined();
    await user.click(screen.getByRole("radio", { name: /Delete permanently/ }));
    const again = await screen.findByRole("dialog");
    const confirm = within(again).getByRole("button", { name: "Delete permanently" });
    expect(confirm).toBeDisabled();
    await user.type(within(again).getByRole("textbox"), "DELETE");
    await user.click(confirm);
    expect(useSettingsStore.getState().values["files.deleted-mode"]).toBe("erase");
  });

  it("offers the automatic empty, off by default", () => {
    render(<DeletedFilesSection />);
    expect(
      screen.getByRole("combobox", { name: "Empty trash folders automatically" }),
    ).toHaveTextContent("Off");
  });
});

describe("the speed of a move", () => {
  it("reads as a size per second and the time left", () => {
    const size = (bytes: number) => `${Math.round(bytes / 1e6)} MB`;
    expect(moveSpeedText({ bytes_per_second: 142e6, seconds_left: 240 }, size)).toBe(
      "142 MB/s · 4 min left",
    );
    expect(moveSpeedText({ bytes_per_second: 142e6, seconds_left: null }, size)).toBe("142 MB/s");
    expect(moveSpeedText({ bytes_per_second: 0, seconds_left: null }, size)).toBeNull();
    expect(timeLeftText(30)).toBe("under a minute left");
    expect(timeLeftText(3900)).toBe("1 h 5 min left");
  });
});
