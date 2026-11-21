import type { Segment, TimelinePlacement } from "./calendar";
import type { Turn } from "./types";
export type ActivityGroup = TimelinePlacement & {
  id: string;
  project_id: string;
  date: string;
  start: number;
  end: number | null;
  members: Segment[];
  elapsed_ms: number;
  total_ms: number;
  statuses: Partial<Record<Turn["status"], number>>;
  unknown: boolean;
};
export type GroupingMode = "group" | "turn";
const GAP_MS = 5 * 60 * 1000;
function makeGroup(members: Segment[]): ActivityGroup {
  const first = members[0];
  let elapsed_ms = 0,
    total_ms = 0;
  let range: { start: number; end: number } | null = null;
  const statuses: ActivityGroup["statuses"] = {};
  for (const member of members) {
    const status = member.turn.status;
    statuses[status] = (statuses[status] ?? 0) + 1;
    if (member.unknown) continue;
    total_ms += member.end - member.start;
    if (!range) {
      range = { start: member.start, end: member.end };
    } else if (member.start <= range.end) {
      range.end = Math.max(range.end, member.end);
    } else {
      elapsed_ms += range.end - range.start;
      range = { start: member.start, end: member.end };
    }
  }
  if (range) elapsed_ms += range.end - range.start;
  const top = Math.min(...members.map((s) => s.top));
  return {
    id: `${first.date}:${first.turn.project_id}:${first.turn.id}`,
    project_id: first.turn.project_id,
    date: first.date,
    start: first.start,
    end: first.unknown ? null : Math.max(...members.map((s) => s.end)),
    members,
    elapsed_ms,
    total_ms,
    statuses,
    top,
    height: Math.max(...members.map((s) => s.top + s.height)) - top,
    unknown: first.unknown,
    lane: 0,
    lanes: 1,
  };
}
export function groupActivity(
  segments: Segment[],
  mode: GroupingMode = "group",
): ActivityGroup[] {
  const sorted = [...segments].sort(
    (a, b) =>
      a.start - b.start || a.end - b.end || a.turn.id.localeCompare(b.turn.id),
  );
  const result: ActivityGroup[] = [];
  const buckets = new Map<string, Segment[]>();
  for (const segment of sorted) {
    if (mode === "turn" || segment.unknown) {
      result.push(makeGroup([segment]));
      continue;
    }
    const key = JSON.stringify([segment.date, segment.turn.project_id]);
    const bucket = buckets.get(key) ?? [];
    bucket.push(segment);
    buckets.set(key, bucket);
  }
  for (const bucket of buckets.values()) {
    let members: Segment[] = [];
    let end = -Infinity;
    for (const segment of bucket) {
      if (members.length && segment.start > end + GAP_MS) {
        result.push(makeGroup(members));
        members = [];
        end = -Infinity;
      }
      members.push(segment);
      end = Math.max(end, segment.end);
    }
    if (members.length) result.push(makeGroup(members));
  }
  return result.sort((a, b) => a.start - b.start || a.id.localeCompare(b.id));
}
