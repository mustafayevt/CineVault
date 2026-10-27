import { describe, expect, it } from "vitest";
import { t } from "../../i18n";
import { fitKey } from "./artworkModel";

describe("the fit of a candidate in words", () => {
  it("says Best, Good or Loose fit instead of a bare number", () => {
    expect(t(fitKey(90))).toBe("Best fit");
    expect(t(fitKey(50))).toBe("Good fit");
    expect(t(fitKey(10))).toBe("Loose fit");
  });

  it("explains the score in a hint", () => {
    expect(t("artwork.fit.hint", { score: 90 })).toContain("90 of 90");
  });
});
