import { describe, expect, it } from "vitest";

import { t } from "../../i18n/catalog";
import { binName } from "./binName";

describe("binName", () => {
  it("names the Recycle Bin on Windows", () => {
    expect(binName("windows")).toBe("Recycle Bin");
  });

  it("names the Trash on macOS and on Linux", () => {
    expect(binName("macos")).toBe("Trash");
    expect(binName("linux")).toBe("Trash");
  });

  it("names the Trash on a platform it does not recognise", () => {
    expect(binName("other")).toBe("Trash");
  });

  it("fills the bin into a message the same way on each platform", () => {
    expect(t("delete.toast.one", { title: "Heat", bin: binName("windows") })).toBe(
      "Deleted Heat to the Recycle Bin",
    );
    expect(t("delete.toast.one", { title: "Heat", bin: binName("linux") })).toBe(
      "Deleted Heat to the Trash",
    );
  });
});
