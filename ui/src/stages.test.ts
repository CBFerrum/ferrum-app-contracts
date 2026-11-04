import { describe, expect, it } from "vitest";
import { byStage } from "./Home";

describe("contracts by stage", () => {
  it("groups the statuses into stages, in order", () => {
    const s = byStage([
      { id: "a", status: "Draft" },
      { id: "b", status: "Out for Signature" },
      { id: "c", status: "Active" },
      { id: "d", status: "Approved" },
      { id: "e", status: "Expired" },
    ]);
    expect(s.map(([stage, l]) => [stage, l.map((c) => c.id)])).toEqual([
      ["In the works", ["a", "d"]],
      ["Out for signature", ["b"]],
      ["Active", ["c"]],
    ]);
  });
});
