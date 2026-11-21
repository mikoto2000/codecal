import { describe, it, expect } from "vitest";
import { indexActivity, summarizeDays } from "./calendar";
import type { Turn } from "./types";
const turn = (id: string, start: string, end: string | null): Turn => ({
  id,
  turn_id: id,
  session_id: "s",
  project_id: id,
  start: Date.parse(start),
  end: end ? Date.parse(end) : null,
  status: end ? "completed" : "unknown",
  prompt: "synthetic",
  tokens: null,
});
describe("indexed calendar activity", () => {
  it("prepares every day once and combines overlapping work and unknown markers", () => {
    const a = turn("a", "2026-11-20T23:00Z", "2026-11-21T02:00Z");
    const b = turn("b", "2026-11-20T23:30Z", "2026-11-21T01:00Z");
    const unknown = turn("c", "2026-11-20T20:00Z", null);
    const index = indexActivity([a, b, unknown], "UTC");
    expect(index.get("2026-11-20")?.segments).toHaveLength(3);
    expect(summarizeDays(index, ["2026-11-20"])).toMatchObject({
      elapsed_ms: 3600000,
      total_ms: 5400000,
      count: 3,
    });
    expect(summarizeDays(index, ["2026-11-20", "2026-11-21"])).toMatchObject({
      elapsed_ms: 10800000,
      total_ms: 16200000,
      count: 3,
      projects: { a: 10800000, b: 5400000 },
    });
    expect(summarizeDays(index, ["2026-11-22"])).toMatchObject({
      elapsed_ms: 0,
      total_ms: 0,
      count: 0,
    });
  });
  it("uses actual elapsed time on both DST transition days", () => {
    const a = turn("a", "2026-03-08T05:00Z", "2026-03-09T04:00Z");
    const b = turn("b", "2026-11-01T04:00Z", "2026-11-02T05:00Z");
    const index = indexActivity([a, b], "America/New_York");
    expect(summarizeDays(index, ["2026-03-08"]).elapsed_ms).toBe(23 * 3600000);
    expect(summarizeDays(index, ["2026-11-01"]).elapsed_ms).toBe(25 * 3600000);
  });
});
