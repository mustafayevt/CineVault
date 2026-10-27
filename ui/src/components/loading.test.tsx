import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { readStyle } from "../test/tokenCss";
import { CachedImage } from "../features/images/CachedImage";
import { posterArt, posterLines } from "./posterArt";
import { PlaceholderPoster } from "./PlaceholderPoster";
import { PosterTile } from "./PosterTile";
import { Skeleton } from "./Skeleton";

const KEY = "0123456789abcdef0123456789abcdef";

describe("Skeleton", () => {
  it("has the shapes of the content it stands in for", () => {
    const { container } = render(
      <>
        <Skeleton shape="poster" />
        <Skeleton shape="row" />
        <Skeleton shape="line" width="60%" />
      </>,
    );
    const [poster, row, line] = [...container.querySelectorAll<HTMLElement>("[data-shape]")];
    expect(poster).toHaveClass("aspect-[2/3]", "w-full");
    expect(row).toHaveClass("h-(--h-row)");
    expect(line).toHaveStyle({ width: "60%" });
  });

  it("shifts no layout when the content arrives: same box as the poster and the row", () => {
    // The poster box is a 2:3 aspect ratio at full width, and a skeleton claims the same.
    render(<PosterTile title="Heat" stage="scraped" />);
    expect(screen.getByTestId("poster-frame")).toHaveClass("aspect-[2/3]", "w-full");
    render(<Skeleton shape="poster" />);
    expect(document.querySelector("[data-shape='poster']")).toHaveClass("aspect-[2/3]");
  });

  it("is hidden from assistive technology and shimmers only through the shimmer attribute", () => {
    const { container } = render(<Skeleton shape="poster" />);
    const skeleton = container.firstElementChild;
    expect(skeleton).toHaveAttribute("aria-hidden", "true");
    expect(skeleton).toHaveAttribute("data-cv-skeleton");
    // Static under reduced motion (skeleton.css), and only the pseudo-element moves.
    const css = readStyle("../components/skeleton.css");
    expect(css).toContain(':root[data-motion="reduced"] [data-cv-skeleton]::after');
    // Flat: one band moved by transform and a dashed frame on poster skeletons, no gradient.
    expect(css).toContain('[data-cv-skeleton][data-shape="poster"]::before');
    expect(css).toMatch(
      /@keyframes cv-skeleton-slide\s*\{\s*from\s*\{\s*transform: translateX\(-100%\)/,
    );
  });
});

describe("PlaceholderPoster", () => {
  it("draws a typographic poster named by the title, with the title printed on it", () => {
    render(<PlaceholderPoster title="Arrival (2016)" />);
    const poster = screen.getByRole("img", { name: "Arrival" });
    expect(poster).toHaveAttribute("data-testid", "placeholder-poster");
    expect(poster.querySelector("text")?.textContent).toBe("Arrival");
  });

  it("gives the same title the same palette and motif every time", () => {
    expect(posterArt("Heat")).toEqual(posterArt("Heat"));
    expect(posterArt("Heat (1995)")).toEqual(posterArt("Heat"));
    const looks = new Set(
      ["Heat", "Drive", "Arrival", "Parasite", "Dune", "Aftersun"].map((title) => {
        const art = posterArt(title);
        return `${art.palette[0]}-${art.motif}`;
      }),
    );
    expect(looks.size).toBeGreaterThan(3);
  });

  it("breaks long titles into balanced lines and hyphenates long words", () => {
    expect(posterLines("Blade Runner 2049").lines).toEqual(["Blade", "Runner 2049"]);
    expect(posterLines("Supercalifragilisticexpialidocious").lines).toEqual([
      "Supercalifragilis-",
      "ticexpialidocious",
    ]);
    expect(posterLines("The Assassination of Jesse James by the Coward").lines).toHaveLength(3);
  });
});

describe("CachedImage placeholder and crossfade", () => {
  it("paints the ambient colour first, then fades the image in over dur-page", () => {
    render(<CachedImage imageKey={KEY} size="grid" alt="Poster" ambient="#c8702e" />);
    const frame = screen.getByTestId("cached-image");
    expect(frame).toHaveStyle({ backgroundColor: "#c8702e" });
    const img = screen.getByRole("img", { name: "Poster" });
    expect(img).toHaveClass("opacity-0", "duration-(--dur-page)", "ease-(--ease-out)");
    expect(img).toHaveAttribute("loading", "lazy");
    fireEvent.load(img);
    expect(img).toHaveClass("opacity-100");
    expect(frame).toHaveAttribute("data-loaded", "true");
  });

  it("shows the ThumbHash under the image until it has loaded", () => {
    const { container } = render(
      <CachedImage imageKey={KEY} size="grid" alt="Poster" thumbhash="3f8f071f8b1e3c8a1000" />,
    );
    expect(container.querySelector("canvas")).toBeInTheDocument();
    fireEvent.load(screen.getByRole("img", { name: "Poster" }));
    expect(container.querySelector("canvas")).toBeNull();
  });
});
