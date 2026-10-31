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

  it("prefers the button just pressed over one that kept the focus", () => {
    document.body.innerHTML =
      '<button id="old">Old</button><div role="toolbar"><button id="move">Move to…</button></div>';
    (document.getElementById("old") as HTMLButtonElement).focus();
    const move = document.getElementById("move") as HTMLButtonElement;
    trackPointer({ clientX: 10, clientY: 10, target: move });
    expect(currentAnchor().element).toBe(move);
    expect(currentAnchor().side).toBe("top");
  });

  it("closes when the control that opened it asks again, and opens on the next press", () => {
    document.body.innerHTML = '<div role="toolbar"><button id="move">Move to…</button></div>';
    const move = document.getElementById("move") as HTMLButtonElement;
    trackPointer({ clientX: 10, clientY: 10, target: move });
    openMoveTo(["a"], "movie", "Heat");
    expect(useMoveToStore.getState().request?.keyboard).toBe(false);
    openMoveTo(["a"], "movie", "Heat");
    expect(useMoveToStore.getState().request).toBeNull();
    openMoveTo(["a"], "movie", "Heat");
    expect(useMoveToStore.getState().request).not.toBeNull();
  });

  it("marks a request with no press before it as a keyboard one", () => {
    document.body.innerHTML = '<div><button id="move">Move</button></div>';
    const move = document.getElementById("move") as HTMLButtonElement;
    trackPointer({ clientX: 0, clientY: 0, target: document.body });
    move.focus();
    openMoveTo(["a"], "movie", "Heat");
    expect(useMoveToStore.getState().request?.keyboard).toBe(true);
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
