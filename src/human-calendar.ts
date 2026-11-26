import { DateTime } from "luxon";
import type { HumanActivity, HumanActivityKind } from "./types";
export const humanKinds: HumanActivityKind[] = [
  "entertainment",
  "non_entertainment",
  "unknown",
];
export const humanLabels: Record<HumanActivityKind, string> = {
  entertainment: "娯楽",
  non_entertainment: "非娯楽",
  unknown: "不明",
};
export const humanColors: Record<HumanActivityKind, string> = {
  entertainment: "#d97706",
  non_entertainment: "#8b5cf6",
  unknown: "#64748b",
};
export type HumanSegment = HumanActivity & {
  id: string;
  date: string;
  top: number;
  height: number;
  lane: number;
  lanes: number;
  point: boolean;
};
export type HumanDay = { segments: HumanSegment[] };
export function indexHumanActivity(
  records: HumanActivity[],
  dates: string[],
  zone: string,
): Map<string, HumanDay> {
  const days = [...new Set(dates)].map((date) => {
    const start = DateTime.fromISO(date, { zone }).startOf("day");
    return {
      date,
      start: start.toMillis(),
      end: start.plus({ days: 1 }).toMillis(),
    };
  });
  const index = new Map<string, HumanDay>();
  records.forEach((record, i) => {
    for (const day of days) {
      const point = record.start === record.end;
      if (
        point
          ? record.start < day.start || record.start >= day.end
          : record.start >= day.end || record.end <= day.start
      )
        continue;
      const start = Math.max(record.start, day.start),
        end = Math.min(record.end, day.end);
      const segment: HumanSegment = {
        id: `human-${i}-${day.date}`,
        date: day.date,
        start,
        end,
        kind: record.kind,
        top: ((start - day.start) / (day.end - day.start)) * 1440,
        height: Math.max(2, ((end - start) / (day.end - day.start)) * 1440),
        lane: 0,
        lanes: 1,
        point,
      };
      const value = index.get(day.date) ?? { segments: [] };
      value.segments.push(segment);
      index.set(day.date, value);
    }
  });
  return index;
}
function unionMs(records: HumanActivity[]) {
  let end = -Infinity,
    total = 0;
  for (const record of records
    .filter((r) => r.end > r.start)
    .sort((a, b) => a.start - b.start || a.end - b.end)) {
    if (record.end > end) {
      total += record.end - Math.max(record.start, end);
      end = record.end;
    }
  }
  return total;
}
export function summarizeHumanDays(
  index: Map<string, HumanDay>,
  dates: string[],
  visible: HumanActivityKind[] = humanKinds,
) {
  const kinds: Record<HumanActivityKind, number> = {
    entertainment: 0,
    non_entertainment: 0,
    unknown: 0,
  };
  let total_ms = 0;
  let other_ms = 0;
  for (const date of new Set(dates)) {
    const records = index.get(date)?.segments ?? [];
    other_ms += unionMs(records.filter((r) => r.kind !== "entertainment"));
    total_ms += unionMs(records.filter((r) => visible.includes(r.kind)));
    for (const kind of visible)
      kinds[kind] += unionMs(records.filter((r) => r.kind === kind));
  }
  return { total_ms, kinds, other_ms };
}
