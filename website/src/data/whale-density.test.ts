import { describe, expect, it } from "vitest";

import { WHALE_DENSITY } from "./whale-density";

describe("whale density artwork", () => {
  it("keeps every scan row on the same 76-cell terminal grid", () => {
    expect(WHALE_DENSITY).toHaveLength(31);
    expect(WHALE_DENSITY.every((row) => row.length === 76)).toBe(true);
  });
});
