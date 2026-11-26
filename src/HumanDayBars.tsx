import type { CSSProperties } from "react";
import { formatDuration } from "./calendar";
import {
  humanDisplayKinds,
  humanDisplayLabels,
  humanDisplayMembers,
  humanDisplayColors,
  displayDuration,
} from "./human-display";
import type { HumanActivityKind, HumanDaySummary } from "./types";
export function HumanDayBars({
  day,
  visible,
}: {
  day: HumanDaySummary;
  visible: HumanActivityKind[];
}) {
  if (!visible.some((kind) => day.observed.includes(kind))) return null;
  return (
    <span className="human-day-bars">
      {humanDisplayKinds
        .filter((kind) =>
          humanDisplayMembers[kind].some((k) => visible.includes(k)),
        )
        .map((kind) => {
          const point =
            displayDuration(day, kind) === 0 &&
            humanDisplayMembers[kind].some((k) => day.observed.includes(k));
          return (
            <span
              key={kind}
              className="human-day-bar"
              role="img"
              aria-label={`${humanDisplayLabels[kind]} 推定 ${formatDuration(displayDuration(day, kind))}${point ? " 点の観測" : ""}`}
              style={
                { "--human-color": humanDisplayColors[kind] } as CSSProperties
              }
            >
              <span
                className={point ? "point" : ""}
                style={{
                  width: `${Math.min(100, (100 * displayDuration(day, kind)) / day.day_length_ms)}%`,
                }}
              />
            </span>
          );
        })}
    </span>
  );
}
