import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DetailImage } from "./DetailImage";

const HASH = "d1690a2d0888887f7787778777788887887f87f887";

afterEach(() => {
  vi.useRealTimers();
});

describe("DetailImage", () => {
  it("removes the blurred preview once the image has faded in, so a clear logo has no box", () => {
    vi.useFakeTimers();
    const { container } = render(
      <DetailImage
        image={{ key: "0".repeat(31) + "1", thumbhash: HASH, ambient: null }}
        size="hero"
        className="cv-hero-logo"
      />,
    );
    expect(container.querySelector("canvas")).not.toBeNull();
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    fireEvent.load(img as HTMLImageElement);
    // The preview stays while the image fades in over it...
    expect(container.querySelector("canvas")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(400);
    });
    // ...and is gone after.
    expect(container.querySelector("canvas")).toBeNull();
    expect(container.querySelector("img")).toHaveClass("opacity-100");
  });

  it("keeps the preview when the image cannot be loaded", () => {
    const { container } = render(
      <DetailImage
        image={{ key: "0".repeat(31) + "2", thumbhash: HASH, ambient: null }}
        size="hero"
      />,
    );
    fireEvent.error(container.querySelector("img") as HTMLImageElement);
    expect(container.querySelector("canvas")).not.toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });
});
