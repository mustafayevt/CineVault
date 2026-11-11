import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setEditBackend } from "../edit/editBackend";
import type { EditBackend } from "../edit/editBackend";
import { useToastStore } from "../toasts/toastStore";
import { ORIGINAL_TITLE_LANGUAGES_KEY, OriginalTitleLanguages } from "./OriginalTitleLanguages";
import { useSettingsStore } from "./settingsStore";

function renderRow() {
  render(
    <div>
      <OriginalTitleLanguages />
    </div>,
  );
}

const stored = () => useSettingsStore.getState().values[ORIGINAL_TITLE_LANGUAGES_KEY];

beforeEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});
afterEach(() => {
  setEditBackend(null);
  useToastStore.setState({ toasts: [], history: [] });
});

describe("Keep the original title", () => {
  it("starts empty, adds and removes languages by name and stores their codes", async () => {
    const user = userEvent.setup();
    renderRow();
    expect(screen.getByText("Keep the original title")).toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: "Languages that keep their original title" }),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Use for titles I have" })).toBeDisabled();

    await user.selectOptions(screen.getByRole("combobox", { name: "Add a language" }), "fr");
    await user.selectOptions(screen.getByRole("combobox", { name: "Add a language" }), "es");
    expect(stored()).toBe("fr,es");
    const list = screen.getByRole("list", { name: "Languages that keep their original title" });
    expect(within(list).getByText("French")).toBeInTheDocument();
    expect(within(list).getByText("Spanish")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove French" }));
    expect(stored()).toBe("es");
  });

  it("switches the titles already in the library, with Undo", async () => {
    useSettingsStore.setState({
      values: { [ORIGINAL_TITLE_LANGUAGES_KEY]: "es" },
      status: "ready",
    });
    const apply = vi.fn(() =>
      Promise.resolve({ changed: ["4", "9"], unchanged: 0, nfo_problem: null }),
    );
    const setTitleChoice = vi.fn(() =>
      Promise.resolve({ changed: ["4", "9"], unchanged: 0, nfo_problem: null }),
    );
    setEditBackend({
      applyOriginalTitleLanguages: apply,
      setTitleChoice,
    } as unknown as EditBackend);
    const user = userEvent.setup();
    renderRow();
    await user.click(screen.getByRole("button", { name: "Use for titles I have" }));
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]?.message).toBe(
        "Now using the original title for 2 titles",
      ),
    );
    await useToastStore.getState().toasts[0]?.undo?.();
    expect(setTitleChoice).toHaveBeenCalledWith(["4", "9"], false);
  });
});
