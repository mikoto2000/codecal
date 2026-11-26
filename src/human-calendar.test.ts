import { expect, it } from "vitest";
import { indexHumanActivity, summarizeHumanDays } from "./human-calendar";
const ms = (value: string) => Date.parse(value);
it("clips long estimates before splitting and keeps zero-second points", () => {
  const index = indexHumanActivity(
    [
      { start: 0, end: ms("2999-01-01T00:00Z"), kind: "unknown" },
      {
        start: ms("2026-11-21T00:00Z"),
        end: ms("2026-11-21T00:00Z"),
        kind: "entertainment",
      },
    ],
    ["2026-11-20", "2026-11-21"],
    "UTC",
  );
  expect(index.get("2026-11-20")?.segments).toHaveLength(1);
  expect(index.get("2026-11-21")?.segments).toHaveLength(2);
  const summary = summarizeHumanDays(index, ["2026-11-20", "2026-11-21"]);
  expect(summary.total_ms).toBe(2 * 86400000);
  expect(summary.kinds.entertainment).toBe(0);
  expect(summary.kinds.unknown).toBe(2 * 86400000);
});
it("unions same-kind overlaps and keeps different-kind totals independent", () => {
  const start = ms("2026-11-20T00:00Z");
  const index = indexHumanActivity(
    [
      { start, end: start + 2000, kind: "entertainment" },
      { start: start + 1000, end: start + 3000, kind: "entertainment" },
      { start: start + 1000, end: start + 4000, kind: "non_entertainment" },
    ],
    ["2026-11-20"],
    "UTC",
  );
  expect(summarizeHumanDays(index, ["2026-11-20"])).toEqual({
    total_ms: 4000,
    kinds: { entertainment: 3000, non_entertainment: 3000, unknown: 0 },
  });
  expect(
    summarizeHumanDays(index, ["2026-11-20"], ["non_entertainment"]).total_ms,
  ).toBe(3000);
});
it("uses real DST day lengths for timeline placement and excludes the range end", () => {
  for (const [date, from, to] of [
    ["2026-03-08", "2026-03-08T05:00Z", "2026-03-09T04:00Z"],
    ["2026-11-01", "2026-11-01T04:00Z", "2026-11-02T05:00Z"],
  ]) {
    const index = indexHumanActivity(
      [
        { start: ms(from), end: ms(to), kind: "unknown" },
        { start: ms(to), end: ms(to), kind: "unknown" },
      ],
      [date],
      "America/New_York",
    );
    expect(index.get(date)?.segments).toHaveLength(1);
    expect(index.get(date)?.segments[0].height).toBe(1440);
    expect(summarizeHumanDays(index, [date]).total_ms).toBe(ms(to) - ms(from));
  }
});
