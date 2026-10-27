import { render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import {
  selectBottomInset,
  useBottomInsetStore,
  useReportBottomInset,
} from "../../stores/bottomInset";

function Bar({ id }: { id: string }) {
  const ref = useReportBottomInset(id);
  return <div ref={ref}>bar</div>;
}

afterEach(() => useBottomInsetStore.setState({ insets: {} }));

describe("bottom inset store", () => {
  it("takes the tallest bar and forgets a bar that unmounts", () => {
    const first = render(<Bar id="a" />);
    render(<Bar id="b" />);
    expect(Object.keys(useBottomInsetStore.getState().insets).sort()).toEqual(["a", "b"]);
    first.unmount();
    expect(Object.keys(useBottomInsetStore.getState().insets)).toEqual(["b"]);
    useBottomInsetStore.getState().set("b", 40);
    useBottomInsetStore.getState().set("c", 90);
    expect(selectBottomInset(useBottomInsetStore.getState())).toBe(90);
  });
});
