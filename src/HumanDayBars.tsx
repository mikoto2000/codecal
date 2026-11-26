import type { CSSProperties } from "react";
import { formatDuration } from "./calendar";
import { humanColors, humanLabels } from "./human-calendar";
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
      {visible.map((kind) => {
        const point = day.kinds[kind] === 0 && day.observed.includes(kind);
        return (
          <span
            key={kind}
            className="human-day-bar"
            role="img"
            aria-label={`${humanLabels[kind]} 推定 ${formatDuration(day.kinds[kind])}${point ? " 点の観測" : ""}`}
            style={{ "--human-color": humanColors[kind] } as CSSProperties}
          >
            <span
              className={point ? "point" : ""}
              style={{
                width: `${Math.min(100, (100 * day.kinds[kind]) / day.day_length_ms)}%`,
              }}
            />
          </span>
        );
      })}
    </span>
  );
}
