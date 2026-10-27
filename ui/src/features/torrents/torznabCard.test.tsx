import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createMemoryBackend, setSettingsBackend } from "../settings/settingsBackend";
import { settleSettingSaves, useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import { spiedBackend } from "./torrentHarness";
import type { SpiedBackend } from "./torrentHarness";
import { TorznabCard } from "./TorznabCard";

let backend: SpiedBackend;

beforeEach(() => {
  backend = spiedBackend();
  setTorrentBackend(backend);
  setSettingsBackend(createMemoryBackend({}));
  useSettingsStore.setState({ values: {}, status: "ready" });
});

afterEach(() => {
  setTorrentBackend(null);
  setSettingsBackend(null);
});

function show(props: { savedUrl?: string; keySaved?: boolean; onChanged?: () => void } = {}) {
  const onChanged = props.onChanged ?? vi.fn();
  render(
    <TorznabCard
      savedUrl={props.savedUrl ?? ""}
      keySaved={props.keySaved ?? false}
      onChanged={onChanged}
    />,
  );
  return { onChanged };
}

const address = () => screen.getByRole("textbox", { name: "Address" });
const key = () => screen.getByLabelText("API key");
const status = () => screen.getByRole("status");

describe("Test connection", () => {
  it("is off until there is an address", () => {
    show();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("says how many indexers answered", async () => {
    const user = userEvent.setup();
    show();
    await user.type(address(), "http://localhost:9117/torznab");
    await user.type(key(), "abc123");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(status()).toHaveTextContent("Connected · 12 indexers"));
    expect(backend.calls.torznab).toEqual([
      { url: "http://localhost:9117/torznab", apiKey: "abc123" },
    ]);
  });

  const ERRORS: [string, string][] = [
    ["http://rejected.test/api", "The key was rejected"],
    ["http://nothing.test/api", "Nothing answers at this address"],
    ["http://html.test/api", "This is not a Torznab feed"],
  ];
  for (const [url, message] of ERRORS) {
    it(`says it plainly: ${message}`, async () => {
      const user = userEvent.setup();
      show();
      await user.type(address(), url);
      await user.click(screen.getByRole("button", { name: "Test connection" }));
      await waitFor(() => expect(status()).toHaveTextContent(message));
      expect(status()).toHaveAttribute("data-tone", "problem");
    });
  }

  it("tests with the saved key when the key field is left empty", async () => {
    const user = userEvent.setup();
    show({ savedUrl: "http://localhost:9117/torznab", keySaved: true });
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(backend.calls.torznab).toHaveLength(1));
    expect(backend.calls.torznab[0]).toEqual({
      url: "http://localhost:9117/torznab",
      apiKey: null,
    });
  });

  it("shows a failed call as a sentence", async () => {
    const user = userEvent.setup();
    backend.testTorznab = () => Promise.reject(new Error("The feature is off."));
    show();
    await user.type(address(), "http://x.test/api");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => expect(status()).toHaveTextContent("The feature is off."));
  });
});

describe("the key", () => {
  it("is masked, with a button to look at what you are typing", async () => {
    const user = userEvent.setup();
    show();
    await user.type(key(), "secret-key");
    expect(key()).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Show the key you are typing" }));
    expect(key()).toHaveAttribute("type", "text");
    expect(key()).toHaveValue("secret-key");
    await user.click(screen.getByRole("button", { name: "Hide the key you are typing" }));
    expect(key()).toHaveAttribute("type", "password");
  });

  it("is sent once on Save and never shown again", async () => {
    const user = userEvent.setup();
    const { onChanged } = show();
    await user.type(address(), "  http://localhost:9117/torznab ");
    await user.type(key(), "secret-key");
    await user.click(screen.getByRole("button", { name: "Show the key you are typing" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(status()).toHaveTextContent("Saved."));
    expect(backend.calls.torznab).toEqual([
      { url: "http://localhost:9117/torznab", apiKey: "secret-key" },
    ]);
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(key()).toHaveValue("");
    expect(key()).toHaveAttribute("type", "password");
    expect(document.body.textContent).not.toContain("secret-key");
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["torrents.torznab.url"]).toBe(
      "http://localhost:9117/torznab",
    );
  });

  it("is kept when only the address changes", async () => {
    const user = userEvent.setup();
    show({ savedUrl: "http://localhost:9117/torznab", keySaved: true });
    expect(key()).toHaveAttribute("placeholder", "A key is saved. Type to replace it.");
    await user.type(address(), "2");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(backend.calls.torznab).toHaveLength(1));
    expect(backend.calls.torznab[0]).toEqual({
      url: "http://localhost:9117/torznab2",
      apiKey: null,
    });
  });
});

describe("Remove", () => {
  it("is not offered before anything is saved", () => {
    show();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  });

  it("clears the address and the key", async () => {
    const user = userEvent.setup();
    useSettingsStore.setState({
      values: { "torrents.torznab.url": "http://localhost:9117/torznab" },
      status: "ready",
    });
    const { onChanged } = show({ savedUrl: "http://localhost:9117/torznab", keySaved: true });
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(status()).toHaveTextContent("Removed."));
    expect(backend.calls.torznab).toEqual([{ url: "", apiKey: null }]);
    expect(address()).toHaveValue("");
    expect(key()).toHaveValue("");
    expect(onChanged).toHaveBeenCalledTimes(1);
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["torrents.torznab.url"]).toBeUndefined();
  });
});

describe("accessibility", () => {
  it("has no violations", async () => {
    show({ savedUrl: "http://localhost:9117/torznab", keySaved: true });
    const result = await axe.run(document.body, {
      rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
    });
    expect(result.violations.map((v) => v.id)).toEqual([]);
  });
});
