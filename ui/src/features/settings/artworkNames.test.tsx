import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useToastStore } from "../toasts/toastStore";
import { ArtworkNamesRow } from "./ArtworkNamesRow";
import {
  artworkType,
  artworkTypes,
  customFrom,
  examplesOf,
  isCustom,
  listOf,
  styleChanges,
  tableTypes,
  toggled,
  visibleChoices,
} from "./artworkNames";
import type { ArtworkNameType } from "./artworkNames";
import { settleSettingSaves, useSettingsStore } from "./settingsStore";

function type(key: string): ArtworkNameType {
  const found = artworkType(key);
  if (found === undefined) throw new Error(`no type ${key}`);
  return found;
}

const values = () => useSettingsStore.getState().values;

beforeEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
  useToastStore.setState({ toasts: [], history: [] });
});
afterEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});

describe("artwork names model", () => {
  it("reads the style's lists until a type has names of its own", () => {
    const poster = type("movie.poster");
    expect(listOf(poster, {})).toEqual(["FILENAME_POSTER"]);
    expect(listOf(poster, { "metadata.artwork-names": "short" })).toEqual(["POSTER"]);
    expect(listOf(type("movie.fanart"), { "metadata.artwork-names": "jellyfin" })).toEqual([
      "BACKDROP",
    ]);
    const stored = { "artwork.names.movie.poster": ["poster", "folder"] };
    expect(listOf(poster, stored)).toEqual(["POSTER", "FOLDER"]);
    expect(isCustom(stored)).toBe(true);
    expect(isCustom({ "metadata.artwork-names": "both" })).toBe(false);
    // A list the backend would ignore reads as the style's list.
    expect(listOf(poster, { "artwork.names.movie.poster": ["BOGUS"] })).toEqual([
      "FILENAME_POSTER",
    ]);
  });

  it("starts custom names from the style in use, for every type", () => {
    const filled = customFrom({ "metadata.artwork-names": "jellyfin" });
    expect(Object.keys(filled)).toHaveLength(artworkTypes().length);
    expect(filled["artwork.names.movie.fanart"]).toEqual(["BACKDROP"]);
    expect(filled["artwork.names.movie.poster"]).toEqual(["POSTER"]);
    expect(filled["artwork.names.tv.episode-thumb"]).toEqual(["FILENAME_THUMB"]);
  });

  it("picks a style by clearing every custom list", () => {
    const changes = styleChanges("short");
    expect(changes["metadata.artwork-names"]).toBe("short");
    expect(Object.hasOwn(changes, "artwork.names.movie.poster")).toBe(true);
    expect(changes["artwork.names.movie.poster"]).toBeUndefined();
  });

  it("never leaves a type without a name, and keeps the catalogue order", () => {
    const poster = type("movie.poster");
    expect(toggled(poster, ["POSTER"], "POSTER")).toBeNull();
    expect(toggled(poster, ["FOLDER"], "POSTER")).toEqual(["POSTER", "FOLDER"]);
    expect(toggled(poster, ["POSTER", "FOLDER"], "FOLDER")).toEqual(["POSTER"]);
    // Extra fanart uses its first name only: a choice replaces it.
    expect(toggled(type("movie.extra-fanart"), ["FILENAME_EXTRAFANART"], "EXTRABACKDROP")).toEqual([
      "EXTRABACKDROP",
    ]);
  });

  it("previews the files for the example movie and hides rare names until used", () => {
    const poster = type("movie.poster");
    expect(examplesOf(poster, ["FILENAME_POSTER", "FOLDER"])).toEqual([
      "Example Movie (2020)-poster.jpg",
      "folder.jpg",
    ]);
    expect(examplesOf(type("movie.extra-fanart"), ["FOLDER_EXTRAFANART", "EXTRAFANART"])).toEqual([
      "extrafanart/fanart1.jpg",
    ]);
    const fanart = type("movie.fanart");
    expect(visibleChoices(fanart, ["FANART"]).map((c) => c.id)).not.toContain("BACKGROUND");
    expect(visibleChoices(fanart, ["BACKGROUND"]).map((c) => c.id)).toContain("BACKGROUND");
    // A show's banner has one name only and gets no row.
    expect(tableTypes("tv").map((t) => t.key)).not.toContain("tv.banner");
    expect(tableTypes("tv").map((t) => t.key)).toContain("tv.season-poster");
  });
});

describe("Settings › Naming › Artwork file names", () => {
  it("shows the style with an example and no table", () => {
    render(<ArtworkNamesRow />);
    expect(screen.getByRole("combobox", { name: "Artwork file names" })).toHaveValue("prefixed");
    expect(
      screen.getByText("Example Movie (2020)-poster.jpg, Example Movie (2020)-fanart.jpg"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("artwork-names-table")).toBeNull();
  });

  it("starts Custom from the style in use, so no name changes until a row is edited", async () => {
    const user = userEvent.setup();
    useSettingsStore.setState({
      values: { "metadata.artwork-names": "jellyfin" },
      status: "ready",
    });
    render(<ArtworkNamesRow />);
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Artwork file names" }),
      "custom",
    );
    await settleSettingSaves();

    expect(values()["artwork.names.movie.fanart"]).toEqual(["BACKDROP"]);
    expect(values()["artwork.names.movie.poster"]).toEqual(["POSTER"]);
    const fanart = screen.getByRole("group", { name: "Fanart" });
    expect(within(fanart).getByRole("button", { name: "backdrop" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByTestId("artwork-files-movie.fanart")).toHaveTextContent("backdrop.jpg");
    expect(screen.getByText("Choosing a style replaces these names.")).toBeInTheDocument();
    expect(screen.getByText(/several movies, the names with the movie name/)).toBeInTheDocument();
    // Nothing changed, so there is nothing for Rename to do yet.
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("writes a second name for a type and previews both files", async () => {
    const user = userEvent.setup();
    useSettingsStore.setState({
      values: customFrom({ "metadata.artwork-names": "short" }),
      status: "ready",
    });
    render(<ArtworkNamesRow />);
    const poster = screen.getByRole("group", { name: "Poster" });
    await user.click(within(poster).getByRole("button", { name: "folder" }));

    expect(values()["artwork.names.movie.poster"]).toEqual(["POSTER", "FOLDER"]);
    expect(screen.getByTestId("artwork-files-movie.poster")).toHaveTextContent(
      "poster.jpgfolder.jpg",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Rename moves the artwork you already have to the new names",
    );

    // The movie name in front.
    await user.click(within(poster).getByRole("button", { name: "<name>-poster" }));
    expect(values()["artwork.names.movie.poster"]).toEqual(["FILENAME_POSTER", "POSTER", "FOLDER"]);
    expect(screen.getByTestId("artwork-files-movie.poster")).toHaveTextContent(
      "Example Movie (2020)-poster.jpg",
    );
  });

  it("keeps the last name of a type", async () => {
    const user = userEvent.setup();
    useSettingsStore.setState({ values: customFrom({}), status: "ready" });
    render(<ArtworkNamesRow />);
    const banner = screen.getByRole("group", { name: "Banner" });
    const only = within(banner).getByRole("button", { name: "<name>-banner" });
    expect(only).toHaveAttribute("aria-disabled", "true");
    await user.click(only);
    expect(values()["artwork.names.movie.banner"]).toEqual(["FILENAME_BANNER"]);
  });

  it("replaces the custom names when a style is chosen again, with Undo", async () => {
    const user = userEvent.setup();
    const custom = {
      ...customFrom({}),
      "artwork.names.movie.poster": ["POSTER", "FOLDER"],
    };
    useSettingsStore.setState({ values: custom, status: "ready" });
    render(<ArtworkNamesRow />);
    expect(screen.getByRole("combobox", { name: "Artwork file names" })).toHaveValue("custom");

    await user.selectOptions(screen.getByRole("combobox", { name: "Artwork file names" }), "short");
    await settleSettingSaves();
    expect(values()["metadata.artwork-names"]).toBe("short");
    expect(isCustom(values())).toBe(false);
    expect(screen.queryByTestId("artwork-names-table")).toBeNull();
    const toast = useToastStore.getState().toasts.at(-1);
    expect(toast?.message).toBe("Replaced the custom artwork names with Plain names");

    await useToastStore.getState().undo(toast?.id ?? "");
    await settleSettingSaves();
    expect(values()["artwork.names.movie.poster"]).toEqual(["POSTER", "FOLDER"]);
    expect(values()["metadata.artwork-names"]).toBeUndefined();
  });

  it("offers a stored older style by its name", () => {
    useSettingsStore.setState({ values: { "metadata.artwork-names": "xbmc" }, status: "ready" });
    render(<ArtworkNamesRow />);
    const select = screen.getByRole("combobox", { name: "Artwork file names" });
    expect(select).toHaveValue("xbmc");
    expect(
      within(select).getByRole("option", { name: "Kodi, extra fanart in a folder" }),
    ).toBeTruthy();
  });
});
