import { describe, it, expect } from "vitest";
import { groupActivity } from "./activity-groups";
import { splitTurn } from "./calendar";
import type { Turn } from "./types";
const turn = (
  id: string,
  start: string,
  end: string | null,
  project_id = "p",
  status: Turn["status"] = "completed",
): Turn => ({
  id,
  turn_id: id,
  session_id: "s",
  project_id,
  start: Date.parse(start),
  end: end ? Date.parse(end) : null,
  status,
  prompt: id,
  tokens: null,
});
const segments = (turns: Turn[], zone = "UTC") =>
  turns.flatMap((t) => splitTurn(t, zone));
describe("activity grouping", () => {
  it("combines overlap and gaps up to five minutes without counting gaps as work", () => {
    const input = segments([
      turn("a", "2026-11-20T10:00Z", "2026-11-20T10:30Z"),
      turn("b", "2026-11-20T10:15Z", "2026-11-20T10:45Z", "p", "failed"),
      turn("c", "2026-11-20T10:50Z", "2026-11-20T11:00Z"),
    ]);
    const groups = groupActivity(input);
    expect(groups).toHaveLength(1);
    const g = groups[0];
    expect(g.members.map((m) => m.turn.id)).toEqual(["a", "b", "c"]);
    expect(g.end! - g.start).toBe(60 * 60000);
    expect(g.elapsed_ms).toBe(55 * 60000);
    expect(g.total_ms).toBe(70 * 60000);
    expect(g.statuses).toEqual({ completed: 2, failed: 1 });
    expect(input[0].turn.end).toBe(Date.parse("2026-11-20T10:30Z"));
  });
  it("separates projects and longer gaps while grouping parallel turns of the same project", () => {
    const groups = groupActivity(
      segments([
        turn("a", "2026-11-20T10:00Z", "2026-11-20T10:10Z"),
        turn("b", "2026-11-20T10:05Z", "2026-11-20T10:15Z", "q"),
        turn("c", "2026-11-20T10:15Z", "2026-11-20T10:20Z"),
        turn("d", "2026-11-20T10:26Z", "2026-11-20T10:30Z"),
      ]),
    );
    expect(groups.map((g) => g.members.map((m) => m.turn.id))).toEqual([
      ["a", "c"],
      ["b"],
      ["d"],
    ]);
  });
  it("keeps unknown endings independent and out of confirmed durations", () => {
    const groups = groupActivity(
      segments([
        turn("a", "2026-11-20T10:00Z", "2026-11-20T11:00Z"),
        turn("u", "2026-11-20T10:30Z", null, "p", "unknown"),
      ]),
    );
    expect(groups).toHaveLength(2);
    const unknown = groups.find((g) => g.end === null)!;
    expect(unknown.members).toHaveLength(1);
    expect(unknown.elapsed_ms).toBe(0);
    expect(unknown.unknown).toBe(true);
  });
  it("keeps calendar days separate and calculates only each days clipped work", () => {
    const groups = groupActivity(
      segments([
        turn("a", "2026-11-20T23:50Z", "2026-11-21T00:02Z"),
        turn("b", "2026-11-21T00:05Z", "2026-11-21T00:10Z"),
      ]),
    );
    expect(groups).toHaveLength(2);
    expect(groups[0].date).toBe("2026-11-20");
    expect(groups[0].elapsed_ms).toBe(10 * 60000);
    expect(groups[1].members).toHaveLength(2);
    expect(groups[1].elapsed_ms).toBe(7 * 60000);
  });
  it("retains every original turn in individual mode and has stable grouping for unsorted input", () => {
    const input = segments([
      turn("a", "2026-11-20T10:00Z", "2026-11-20T11:00Z"),
      turn("b", "2026-11-20T10:30Z", "2026-11-20T10:40Z"),
    ]);
    expect(groupActivity(input, "turn")).toHaveLength(2);
    expect(groupActivity([...input].reverse())).toEqual(groupActivity(input));
  });
  it("uses elapsed UTC time across the repeated hour at DST fallback", () => {
    const groups = groupActivity(
      segments(
        [
          turn("a", "2026-11-01T05:00Z", "2026-11-01T06:00Z"),
          turn("b", "2026-11-01T06:05Z", "2026-11-01T06:10Z"),
        ],
        "America/New_York",
      ),
    );
    expect(groups).toHaveLength(1);
    expect(groups[0].elapsed_ms).toBe(65 * 60000);
    expect(groups[0].end! - groups[0].start).toBe(70 * 60000);
  });
});
