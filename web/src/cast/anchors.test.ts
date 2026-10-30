import { describe, expect, it } from "vitest";
import { INGAME_ANCHORS, tabBox } from "./anchors";

describe("tabBox", () => {
  it("blue tab grows leftwards from the inner anchor", () => {
    expect(tabBox("blue")).toEqual({ top: "9.90%", height: "4.60%", maxWidth: "26.00%", right: "60.80%" });
  });
  it("red tab grows rightwards from the inner anchor", () => {
    expect(tabBox("red")).toEqual({ top: "9.90%", height: "4.60%", maxWidth: "26.00%", left: "59.65%" });
  });
  it("accepts overridden anchors (fine offset)", () => {
    const a = { ...INGAME_ANCHORS, blueInnerX: 0.4, redInnerX: 0.6, topY: 0.1 };
    expect(tabBox("blue", a)).toMatchObject({ top: "10.00%", right: "60.00%" });
    expect(tabBox("red", a)).toMatchObject({ left: "60.00%" });
  });
});
