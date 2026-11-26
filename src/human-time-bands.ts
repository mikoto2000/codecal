import { DateTime } from "luxon";
import { humanKinds } from "./human-calendar";
import type { HumanActivity, HumanActivityKind } from "./types";

export type HumanTimeBin = {
  date: string;
  start: number;
  end: number;
  top: number;
  height: number;
  total_ms: number;
  kinds: Record<HumanActivityKind, number>;
  points: Record<HumanActivityKind, boolean>;
};
const step = 300000;
const emptyKinds = () => ({
  entertainment: 0,
  non_entertainment: 0,
  unknown: 0,
});

/** Five elapsed minutes, using interval unions rather than observation counts. */
export function buildHumanTimeBins(
  records: HumanActivity[],
  dates: string[],
  zone: string,
) {
  const result = new Map<string, HumanTimeBin[]>();
  const ordered = [...records].sort(
    (a, b) => a.start - b.start || a.end - b.end,
  );
  for (const date of new Set(dates)) {
    const midnight = DateTime.fromISO(date, { zone }).startOf("day");
    const from = midnight.toMillis(),
      to = midnight.plus({ days: 1 }).toMillis();
    const bins = new Map<number, HumanTimeBin>();
    const get = (time: number) => {
      const i = Math.floor((time - from) / step);
      let bin = bins.get(i);
      if (!bin) {
        const start = from + i * step,
          end = Math.min(to, start + step);
        bin = {
          date,
          start,
          end,
          top: ((start - from) / (to - from)) * 1440,
          height: ((end - start) / (to - from)) * 1440,
          total_ms: 0,
          kinds: emptyKinds(),
          points: {
            entertainment: false,
            non_entertainment: false,
            unknown: false,
          },
        };
        bins.set(i, bin);
      }
      return bin;
    };
    // Each pass keeps its own union endpoint; overlapping kinds remain independent.
    for (const kind of [...humanKinds, null]) {
      let previousEnd = from;
      for (const record of ordered) {
        if (kind !== null && record.kind !== kind) continue;
        if (record.start === record.end) {
          if (kind !== null && record.start >= from && record.start < to)
            get(record.start).points[kind] = true;
          continue;
        }
        let start = Math.max(from, record.start, previousEnd);
        const end = Math.min(to, record.end);
        if (start >= end) continue;
        previousEnd = end;
        while (start < end) {
          const bin = get(start),
            stop = Math.min(end, bin.end);
          if (kind === null) bin.total_ms += stop - start;
          else bin.kinds[kind] += stop - start;
          start = stop;
        }
      }
    }
    result.set(
      date,
      [...bins.values()].sort((a, b) => a.start - b.start),
    );
  }
  return result;
}
