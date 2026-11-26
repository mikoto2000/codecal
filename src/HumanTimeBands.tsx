import type { CSSProperties } from "react";
import { DateTime } from "luxon";
import { humanColors, humanKinds, humanLabels } from "./human-calendar";
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
    <div className="human-time-bands" aria-label="人間の5分集約・固定3列">
      {humanKinds.map((kind) => (
        <div
          className="human-time-lane"
          key={kind}
          data-kind={kind}
          aria-label={humanLabels[kind]}
        >
          {visible.includes(kind) &&
            bins
              .filter((b) => b.kinds[kind] > 0 || b.points[kind])
              .map((bin) => {
                const point = bin.points[kind] && bin.kinds[kind] === 0;
                const times = `${DateTime.fromMillis(bin.start, { zone }).toFormat("HH:mm ZZ")} – ${DateTime.fromMillis(bin.end, { zone }).toFormat("HH:mm ZZ")}`;
                const label = `人間 ${humanLabels[kind]} ${times} 推定 ${formatDuration(bin.kinds[kind])}${point ? " 点の観測" : ""}`;
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
                        "--human-color":
                          kind === "unknown" ? "#a8b2c1" : humanColors[kind],
                        "--coverage": `${Math.max(12, (100 * bin.kinds[kind]) / (bin.end - bin.start))}%`,
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
