import { DateTime } from "luxon";
import type { Turn } from "./types";
export type CalendarView = "day" | "week" | "month";
export type Segment = {
  turn: Turn;
  date: string;
  start: number;
  end: number;
  top: number;
  height: number;
  unknown: boolean;
  lane: number;
  lanes: number;
};
export const localDate = (ms: number, zone: string) =>
  DateTime.fromMillis(ms, { zone });
export const formatDuration = (ms: number) => {
  const safe = Number.isFinite(ms) ? Math.max(0, ms) : 0;
  return `${Math.floor(safe / 3600000)}h ${Math.floor((safe % 3600000) / 60000)}m`;
};
export const statusLabel = {
  completed: "完了",
  failed: "失敗",
  interrupted: "中断",
  running: "実行中",
  unknown: "終了不明",
};
export function daysInView(
  anchor: string,
  view: CalendarView,
  weekStart: number,
): string[] {
  if (view === "day") return [anchor];
  const a = DateTime.fromISO(anchor, { zone: "UTC" });
  const base = view === "month" ? a.startOf("month") : a;
  const first = base.minus({ days: ((base.weekday % 7) - weekStart + 7) % 7 });
  const last = view === "month" ? a.endOf("month") : first.plus({ days: 6 });
  const count =
    view === "month"
      ? Math.ceil((last.startOf("day").diff(first, "days").days + 1) / 7) * 7
      : 7;
  return Array.from({ length: count }, (_, i) =>
    first.plus({ days: i }).toISODate()!,
  );
}
export function splitTurn(turn: Turn, zone: string): Segment[] {
  let start = turn.start;
  const end = turn.end ?? turn.start;
  const out: Segment[] = [];
  do {
    const day = localDate(start, zone).startOf("day"),
      next = day.plus({ days: 1 });
    const stop = Math.min(end, next.toMillis());
    const length = next.toMillis() - day.toMillis();
    out.push({
      turn,
      date: day.toISODate()!,
      start,
      end: stop,
      top: ((start - day.toMillis()) / length) * 1440,
      height: Math.max(
        turn.end === null ? 22 : 2,
        ((stop - start) / length) * 1440,
      ),
      unknown: turn.end === null,
      lane: 0,
      lanes: 1,
    });
    start = stop;
  } while (start < end);
  return out;
}
export type TimelinePlacement = {
  top: number;
  height: number;
  lane: number;
  lanes: number;
};
export function packSegments<T extends TimelinePlacement>(segments: T[]): T[] {
  const sorted = segments
    .map((s) => ({ ...s }))
    .sort((a, b) => a.top - b.top || b.height - a.height);
  let group: T[] = [];
  let ends: number[] = [];
  let boundary = -1;
  const finish = () => {
    for (const s of group) s.lanes = ends.length;
    group = [];
    ends = [];
    boundary = -1;
  };
  for (const s of sorted) {
    if (s.top >= boundary) finish();
    let lane = ends.findIndex((e) => e <= s.top);
    if (lane < 0) lane = ends.length;
    s.lane = lane;
    ends[lane] = s.top + s.height;
    boundary = Math.max(boundary, s.top + s.height);
    group.push(s);
  }
  finish();
  return sorted;
}
export function totals(turns: Turn[], days: string[], zone: string) {
  return summarizeDays(indexActivity(turns, zone), days);
}
export function heatLevel(ms: number, thresholds: number[]) {
  if (ms <= 0) return 0;
  return Math.min(4, 1 + thresholds.filter((h) => ms >= h * 3600000).length);
}
export type IndexedDay = {
  segments: Segment[];
  elapsed_ms: number;
  total_ms: number;
  projects: Record<string, number>;
  turnIds: Set<string>;
};
export function indexActivity(
  turns: Turn[],
  zone: string,
): Map<string, IndexedDay> {
  const index = new Map<string, IndexedDay>();
  for (const turn of turns)
    for (const segment of splitTurn(turn, zone)) {
      let day = index.get(segment.date);
      if (!day) {
        day = {
          segments: [],
          elapsed_ms: 0,
          total_ms: 0,
          projects: {},
          turnIds: new Set(),
        };
        index.set(segment.date, day);
      }
      day.segments.push(segment);
      day.turnIds.add(turn.id);
      if (turn.end !== null) {
        const duration = segment.end - segment.start;
        day.total_ms += duration;
        day.projects[turn.project_id] =
          (day.projects[turn.project_id] ?? 0) + duration;
      }
    }
  for (const day of index.values()) {
    const ranges = day.segments
      .filter((s) => !s.unknown)
      .sort((a, b) => a.start - b.start);
    let current: { start: number; end: number } | null = null;
    for (const s of ranges) {
      if (!current) {
        current = { start: s.start, end: s.end };
      } else if (s.start <= current.end) {
        current.end = Math.max(current.end, s.end);
      } else {
        day.elapsed_ms += current.end - current.start;
        current = { start: s.start, end: s.end };
      }
    }
    if (current) day.elapsed_ms += current.end - current.start;
  }
  return index;
}
export function summarizeDays(index: Map<string, IndexedDay>, dates: string[]) {
  let elapsed_ms = 0,
    total_ms = 0;
  const projects: Record<string, number> = {};
  const ids = new Set<string>();
  for (const date of new Set(dates)) {
    const day = index.get(date);
    if (!day) continue;
    elapsed_ms += day.elapsed_ms;
    total_ms += day.total_ms;
    for (const id of day.turnIds) ids.add(id);
    for (const [id, ms] of Object.entries(day.projects))
      projects[id] = (projects[id] ?? 0) + ms;
  }
  return { elapsed_ms, total_ms, count: ids.size, projects };
}
