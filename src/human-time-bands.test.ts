import { expect, it } from "vitest";
import { buildHumanTimeBins } from "./human-time-bands";
const start = Date.parse("2026-11-20T00:00Z");
it("unions repeated observations per kind without filling gaps or adding counts", () => {
  const bins = buildHumanTimeBins(
    [
      { start, end: start + 120000, kind: "entertainment" },
      { start: start + 60000, end: start + 180000, kind: "entertainment" },
      { start: start + 240000, end: start + 360000, kind: "unknown" },
    ],
    ["2026-11-20"],
    "UTC",
  ).get("2026-11-20")!;
  expect(bins).toHaveLength(2);
  expect(bins[0].kinds).toEqual({
    entertainment: 180000,
    non_entertainment: 0,
    unknown: 60000,
  });
  expect(bins[0].total_ms).toBe(240000);
  expect(bins[1].kinds.unknown).toBe(60000);
  expect(bins[0].height).toBe(5);
});
it("keeps zero-time observations visible without assigning duration or including the range end", () => {
  const bins = buildHumanTimeBins(
    [
      { start, end: start, kind: "unknown" },
      { start: start + 86400000, end: start + 86400000, kind: "unknown" },
    ],
    ["2026-11-20"],
    "UTC",
  ).get("2026-11-20")!;
  expect(bins).toHaveLength(1);
  expect(bins[0].points.unknown).toBe(true);
  expect(bins[0].total_ms).toBe(0);
});
it("clips cross-midnight observations and uses elapsed five-minute slots on DST days", () => {
  for (const [date, from, to, count] of [
    ["2026-03-08", "2026-03-08T05:00Z", "2026-03-09T04:00Z", 276],
    ["2026-11-01", "2026-11-01T04:00Z", "2026-11-02T05:00Z", 300],
  ] as const) {
    const bins = buildHumanTimeBins(
      [
        {
          start: Date.parse(from) - 1000,
          end: Date.parse(to) + 1000,
          kind: "unknown",
        },
      ],
      [date],
      "America/New_York",
    ).get(date)!;
    expect(bins).toHaveLength(count);
    expect(bins.reduce((n, b) => n + b.total_ms, 0)).toBe(
      Date.parse(to) - Date.parse(from),
    );
    expect(bins.at(-1)!.top + bins.at(-1)!.height).toBeCloseTo(1440);
  }
});
it("bounds dense data by time slots and preserves independent overlapping kinds", () => {
  const records = Array.from({ length: 10000 }, () => ({
    start,
    end: start + 600000,
    kind: "entertainment" as const,
  }));
  const bins = buildHumanTimeBins(
    [...records, { start, end: start + 600000, kind: "non_entertainment" }],
    ["2026-11-20"],
    "UTC",
  ).get("2026-11-20")!;
  expect(bins).toHaveLength(2);
  expect(bins[0].total_ms).toBe(300000);
  expect(bins[0].kinds.entertainment).toBe(300000);
  expect(bins[0].kinds.non_entertainment).toBe(300000);
});
