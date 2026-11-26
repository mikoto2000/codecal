import type { CSSProperties } from "react";
import { DateTime } from "luxon";
import {
  displayDuration,
  humanDisplayKinds,
  humanDisplayLabels,
  humanDisplayMembers,
  humanDisplayColors,
} from "./human-display";
import { formatDuration } from "./calendar";
import type { HumanTimeBin } from "./human-time-bands";
import type { HumanActivityKind } from "./types";

export function HumanTimeBands({
  bins,
  visible,
  scale,
  zone,
  open,
}: {
  bins: HumanTimeBin[];
  visible: HumanActivityKind[];
  scale: number;
  zone: string;
  open: (bin: HumanTimeBin) => void;
}) {
  return (
    <div className="human-time-bands" aria-label="人間の5分集約・固定2列">
      {humanDisplayKinds.map((kind) => (
        <div
          className="human-time-lane"
          key={kind}
          data-kind={kind}
          aria-label={humanDisplayLabels[kind]}
        >
          {humanDisplayMembers[kind].some((k) => visible.includes(k)) &&
            bins
              .filter(
                (b) =>
                  displayDuration(b, kind) > 0 ||
                  humanDisplayMembers[kind].some((k) => b.points[k]),
              )
              .map((bin) => {
                const duration = displayDuration(bin, kind);
                const hasPoint = humanDisplayMembers[kind].some(
                  (k) => bin.points[k],
                );
                const point = hasPoint && duration === 0;
                const times = `${DateTime.fromMillis(bin.start, { zone }).toFormat("HH:mm ZZ")} – ${DateTime.fromMillis(bin.end, { zone }).toFormat("HH:mm ZZ")}`;
                const label = `人間 ${humanDisplayLabels[kind]} ${times} 推定 ${formatDuration(duration)}${hasPoint ? (point ? " 点の観測" : " 点の観測を含む") : ""}`;
                return (
                  <button
                    key={bin.start}
                    className={`human-block human-time-cell ${point ? "point" : ""}`}
                    aria-label={label}
                    title={label}
                    style={
                      {
                        top: bin.top * scale,
                        height: bin.height * scale,
                        "--human-color": humanDisplayColors[kind],
                        "--coverage": `${Math.max(12, (100 * duration) / (bin.end - bin.start))}%`,
                      } as CSSProperties
                    }
                    onClick={() => open(bin)}
                  />
                );
              })}
        </div>
      ))}
    </div>
  );
}
