import { formatDuration } from "./calendar";
import {
  humanDisplayKinds,
  humanDisplayLabels,
  humanDisplayMembers,
  displayDuration,
  type HumanDisplayKind,
} from "./human-display";
import type { HumanActivityKind, HumanDaySummary } from "./types";

export function HumanTotals({
  days,
  dates,
  visible,
  loading,
  error,
  retry,
}: {
  days: HumanDaySummary[];
  dates: string[];
  visible: HumanActivityKind[];
  loading: boolean;
  error: string;
  retry: () => void;
}) {
  const selected = days.filter((day) => dates.includes(day.date));
  const total = selected.reduce((sum, day) => sum + day.total_ms, 0);
  const kinds = Object.fromEntries(
    humanDisplayKinds.map((kind) => [
      kind,
      humanDisplayMembers[kind].some((k) => visible.includes(k))
        ? selected.reduce((sum, day) => sum + displayDuration(day, kind), 0)
        : 0,
    ]),
  ) as Record<HumanDisplayKind, number>;
  const unavailable = loading || !!error;
  return (
    <section
      className="human-totals"
      aria-label="人間の観測推定時間"
      aria-busy={loading}
    >
      <h2>人間の観測推定時間</h2>
      <div>
        <span>合計</span>
        <strong data-testid="human-total">
          {unavailable ? "—" : formatDuration(total)}
        </strong>
      </div>
      {humanDisplayKinds.map((kind) => (
        <div
          key={kind}
          className={
            !humanDisplayMembers[kind].some((k) => visible.includes(k))
              ? "unselected"
              : ""
          }
        >
          <span>{humanDisplayLabels[kind]}</span>
          <strong aria-label={`${humanDisplayLabels[kind]}の推定時間`}>
            {unavailable
              ? "—"
              : humanDisplayMembers[kind].some((k) => visible.includes(k))
                ? formatDuration(kinds[kind])
                : "非表示"}
          </strong>
        </div>
      ))}
      {unavailable ? (
        <p>
          {error ? "人間の集計を取得できません。" : "人間の集計を読み込み中…"}
          {error && <button onClick={retry}>人間の集計を再試行</button>}
        </p>
      ) : (
        Object.values(kinds).reduce((sum, ms) => sum + ms, 0) > total && (
          <p>
            種別間で重なる時間は合計で一度だけ数えるため、内訳の合計が総時間を超える場合があります。
          </p>
        )
      )}
    </section>
  );
}
