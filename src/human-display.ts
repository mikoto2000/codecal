import type { HumanActivityKind } from "./types";

export const humanDisplayKinds = ["entertainment", "other"] as const;
export type HumanDisplayKind = (typeof humanDisplayKinds)[number];
export const humanDisplayLabels = {
  entertainment: "娯楽",
  other: "非娯楽（不明含む）",
};
export const humanDisplayColors = {
  entertainment: "#d97706",
  other: "#8b5cf6",
};
export const humanDisplayMembers: Record<
  HumanDisplayKind,
  HumanActivityKind[]
> = {
  entertainment: ["entertainment"],
  other: ["non_entertainment", "unknown"],
};
export function displayKind(kind: HumanActivityKind): HumanDisplayKind {
  return kind === "entertainment" ? "entertainment" : "other";
}
export function displayDuration(
  value: { kinds: Record<HumanActivityKind, number>; other_ms: number },
  kind: HumanDisplayKind,
) {
  return kind === "entertainment" ? value.kinds.entertainment : value.other_ms;
}
