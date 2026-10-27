import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

import { createMemoryBackend, setSettingsBackend } from "../features/settings/settingsBackend";
import { useSettingsStore } from "../features/settings/settingsStore";
import { useToastStore } from "../features/toasts/toastStore";
import { LOCALE_SYSTEM, useLocaleStore } from "../i18n/locale";

// `findBy*` and `waitFor` poll until the screen shows what the test expects; this is only how long
// they keep trying before they call it a failure. The default of one second is too short for a
// machine that is also compiling (the whole suite runs in parallel workers), and a test that
// passes on a rerun is worse than a slow failure. A passing test never waits this long.
configure({ asyncUtilTimeout: 10_000 });

afterEach(() => {
  cleanup();
  // Actions stay undoable after their toast goes; one test's actions are not the next one's.
  useToastStore.setState({ toasts: [], history: [] });
});

// jsdom lacks these browser APIs, which cmdk relies on.
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = ResizeObserverStub;
}
if (typeof Element !== "undefined" && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
window.scrollTo = () => {};

// Settings persist through a backend; tests use memory so nothing leaks between them or into
// localStorage. Each test starts from an unloaded store with no stored values.
beforeEach(() => {
  setSettingsBackend(createMemoryBackend());
  useSettingsStore.setState({ values: {}, status: "idle" });
  useLocaleStore.setState({ preference: LOCALE_SYSTEM, sizeBase: "base10" });
});
