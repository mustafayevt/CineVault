import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DetailCreditDto } from "../../bindings";
import { setDetailBackend } from "../detail/detailBackend";
import { createSyntheticDetailBackend } from "../detail/syntheticDetail";
import { CastSection } from "./CastSection";

const { openRatingPage } = vi.hoisted(() => ({
  openRatingPage: vi.fn<(url: string, source: string) => Promise<void>>(() => Promise.resolve()),
}));
vi.mock("../ratings/ratingPage", () => ({ openRatingPage }));

function person(name: string, ids: Partial<DetailCreditDto> = {}): DetailCreditDto {
  return {
    name,
    role: "Role",
    kind: "actor",
    portrait: null,
    imdb_id: null,
    tmdb_id: null,
    ...ids,
  };
}

function useLookup(personImdbId: (tmdbId: string) => Promise<string | null>) {
  setDetailBackend({ ...createSyntheticDetailBackend(), personImdbId });
}

describe("a cast member opens their page", () => {
  beforeEach(() => openRatingPage.mockClear());
  afterEach(() => setDetailBackend(null));

  it("opens the IMDb page of a stored IMDb id without asking", async () => {
    const lookup = vi.fn(() => Promise.resolve(null));
    useLookup(lookup);
    render(
      <CastSection credits={[person("Zendaya", { imdb_id: "nm3918035", tmdb_id: "505710" })]} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Open Zendaya on IMDb" }));
    expect(openRatingPage).toHaveBeenCalledWith("https://www.imdb.com/name/nm3918035/", "IMDb");
    expect(lookup).not.toHaveBeenCalled();
  });

  it("asks for the IMDb id by the TMDB id when none is stored", async () => {
    const lookup = vi.fn(() => Promise.resolve("nm0000244"));
    useLookup(lookup);
    render(<CastSection credits={[person("Sigourney Weaver", { tmdb_id: "10205" })]} />);
    await userEvent.click(screen.getByRole("button", { name: "Open Sigourney Weaver on IMDb" }));
    await waitFor(() =>
      expect(openRatingPage).toHaveBeenCalledWith("https://www.imdb.com/name/nm0000244/", "IMDb"),
    );
    expect(lookup).toHaveBeenCalledWith("10205");
  });

  it("falls back to the TMDB page when IMDb has no id or cannot be asked", async () => {
    useLookup(() => Promise.resolve(null));
    const { unmount } = render(<CastSection credits={[person("Ian Holm", { tmdb_id: "7" })]} />);
    await userEvent.click(screen.getByRole("button", { name: "Open Ian Holm on IMDb" }));
    await waitFor(() =>
      expect(openRatingPage).toHaveBeenCalledWith("https://www.themoviedb.org/person/7", "TMDB"),
    );
    unmount();
    openRatingPage.mockClear();
    useLookup(() => Promise.reject(new Error("offline")));
    render(<CastSection credits={[person("Ian Holm", { tmdb_id: "7" })]} />);
    await userEvent.click(screen.getByRole("button", { name: "Open Ian Holm on IMDb" }));
    await waitFor(() =>
      expect(openRatingPage).toHaveBeenCalledWith("https://www.themoviedb.org/person/7", "TMDB"),
    );
  });

  it("is not clickable without any id", () => {
    render(<CastSection credits={[person("Extra One")]} />);
    expect(screen.getByText("Extra One")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Open Extra One/ })).toBeNull();
  });
});
