import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BACKDROP_SLOT_ID } from "./backdropSlot";
import { TitleBackdrop } from "./TitleBackdrop";

describe("TitleBackdrop", () => {
  it("stays in place when no shell layer exists", () => {
    const { container } = render(<TitleBackdrop height={400}>x</TitleBackdrop>);
    expect(container.querySelector(".cv-title-backdrop")).not.toBeNull();
    expect(container.querySelector("[data-in-shell]")).toBeNull();
  });

  it("is drawn into the shell's backdrop layer and follows the page scroll", () => {
    const slot = document.createElement("div");
    slot.id = BACKDROP_SLOT_ID;
    const scroller = document.createElement("main");
    scroller.setAttribute("data-route-content", "");
    document.body.append(slot, scroller);
    const { container } = render(<TitleBackdrop height={400}>x</TitleBackdrop>);
    expect(container.querySelector(".cv-title-backdrop")).toBeNull();
    const band = slot.querySelector<HTMLElement>(".cv-title-backdrop[data-in-shell]");
    expect(band).not.toBeNull();
    scroller.scrollTop = 120;
    scroller.dispatchEvent(new Event("scroll"));
    expect(band?.style.transform).toBe("translateY(-120px)");
    slot.remove();
    scroller.remove();
  });
});
