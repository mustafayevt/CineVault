import { act, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { FileSize } from "../../components/FileSize";
import { useFormat } from "../../i18n/useFormat";
import { useSettingsLoader } from "./appearance";
import { useLocaleSettings } from "./localeSettings";
import { settingDef } from "./schema";
import { useSettingsStore } from "./settingsStore";

const MOVIE_BYTES = 4.7 * 1024 ** 3;

function Probes() {
  const { date } = useFormat();
  return (
    <ul aria-label="Probes">
      <li data-testid="size">
        <FileSize bytes={MOVIE_BYTES} />
      </li>
      <li data-testid="date">{date(Date.UTC(2026, 9, 20, 12), { timeZone: "UTC" })}</li>
    </ul>
  );
}

/** A count in a landmark, which changes with the regional format. */
function LibraryCount() {
  const { number } = useFormat();
  return (
    <nav aria-label="Library">
      <span>{number(12345)}</span>
    </nav>
  );
}

function Harness() {
  useSettingsLoader();
  useLocaleSettings();
  return (
    <>
      <Probes />
      <LibraryCount />
    </>
  );
}

async function renderProbes() {
  render(<Harness />);
  await waitFor(() => expect(useSettingsStore.getState().status).toBe("ready"));
}

const size = () => screen.getByTestId("size").textContent;

describe("file size units setting", () => {
  it("is a base 10 / base 2 choice that defaults to base 10", () => {
    const def = settingDef("advanced.size-units");
    expect(def?.default).toBe("base10");
    expect(def?.control.type === "choice" && def.control.options.map((o) => o.value)).toEqual([
      "base10",
      "base2",
    ]);
  });

  it("switches every size between GB and GiB as the stored choice changes", async () => {
    await renderProbes();
    expect(size()).toBe("5.05 GB");

    act(() => useSettingsStore.getState().set("advanced.size-units", "base2"));
    expect(size()).toBe("4.7 GiB");

    act(() => useSettingsStore.getState().reset(["advanced.size-units"]));
    expect(size()).toBe("5.05 GB");
    expect(useSettingsStore.getState().values).not.toHaveProperty("advanced.size-units");
  });

  it("applies a stored base 2 choice after the settings load", async () => {
    useSettingsStore.setState({ values: { "advanced.size-units": "base2" } });
    await renderProbes();
    expect(size()).toBe("4.7 GiB");
  });
});

describe("regional format setting", () => {
  it("changes number, size and date formatting at once", async () => {
    await renderProbes();
    const nav = screen.getByRole("navigation", { name: "Library" });
    expect(within(nav).getByText("12,345")).toBeInTheDocument();
    expect(screen.getByTestId("date")).toHaveTextContent("Oct 20, 2026");

    act(() => useSettingsStore.getState().set("general.locale", "de-DE"));
    expect(within(nav).getByText("12.345")).toBeInTheDocument();
    expect(screen.getByTestId("date")).toHaveTextContent("20.10.2026");
    expect(size()).toBe("5,05 GB");

    act(() => useSettingsStore.getState().reset(["general.locale"]));
    expect(within(nav).getByText("12,345")).toBeInTheDocument();
  });

  it("keeps the base 2 unit while the region changes", async () => {
    await renderProbes();
    act(() => {
      useSettingsStore.getState().set("advanced.size-units", "base2");
      useSettingsStore.getState().set("general.locale", "fr-FR");
    });
    expect(size()).toBe("4,7 GiB");
  });
});
