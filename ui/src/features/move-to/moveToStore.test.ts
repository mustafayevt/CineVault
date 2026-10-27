import { afterEach, describe, expect, it } from "vitest";

import { currentAnchor, openMoveTo, trackPointer, useMoveToStore } from "./moveToStore";

afterEach(() => {
  document.body.innerHTML = "";
  useMoveToStore.setState({ request: null, running: null });
});

describe("where the Move to menu opens", () => {
  it("opens upwards from a button of the selection bar that was pressed", () => {
    document.body.innerHTML = '<div role="toolbar"><button id="move">Move to…</button></div>';
    const button = document.getElementById("move") as HTMLButtonElement;
    // A click does not always focus a button (WebKit): the press is remembered instead.
    trackPointer({ clientX: 10, clientY: 10, target: button });
    expect(document.activeElement).not.toBe(button);
    expect(currentAnchor().side).toBe("top");
  });

  it("opens below a button that has the keyboard focus", () => {
    document.body.innerHTML = '<div><button id="move">Move</button></div>';
    const button = document.getElementById("move") as HTMLButtonElement;
    button.focus();
    expect(currentAnchor().side).toBe("bottom");
  });

  it("keeps the request with its titles and name", () => {
    openMoveTo(["a", "b"], "movie", "2 titles");
    const request = useMoveToStore.getState().request;
    expect(request?.ids).toEqual(["a", "b"]);
    expect(request?.label).toBe("2 titles");
  });

  it("does nothing for no titles", () => {
    openMoveTo([], "movie", "");
    expect(useMoveToStore.getState().request).toBeNull();
  });
});
