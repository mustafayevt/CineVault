import { describe, expect, it } from "vitest";

import { episodeName, seasonTitle } from "./tvFormat";

describe("season and episode names", () => {
  it("writes a year-numbered season without a thousands separator", () => {
    expect(seasonTitle(2021)).toBe("Season 2021");
  });

  it("writes a long absolute episode number without a thousands separator", () => {
    expect(episodeName({ title: "", number: 1100 })).toBe("Episode 1100");
  });
});
