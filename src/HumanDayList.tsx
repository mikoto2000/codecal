import { useMemo } from "react";
import { DateTime } from "luxon";
import { useHumanActivity } from "./useHumanActivity";
import { HumanTotals } from "./HumanTotals";
import { formatDuration } from "./calendar";
import {
  humanLabels,
  indexHumanActivity,
  type HumanSegment,
} from "./human-calendar";
import type { HumanActivityKind, HumanDaySummary } from "./types";
import type { GroupingMode } from "./activity-groups";

export function HumanDayList({
  date,
  root,
  zone,
  visible,
  grouping,
  changeGrouping,
  open,
  summary,
}: {
  date: string;
  root: string;
  zone: string;
  visible: HumanActivityKind[];
  grouping: GroupingMode;
  changeGrouping: (mode: GroupingMode) => void;
  open: (segment: HumanSegment) => void;
  summary: {
    days: HumanDaySummary[];
    loading: boolean;
    error: string;
    retry: () => void;
  };
}) {
  const start = DateTime.fromISO(date, { zone }).startOf("day");
  const query = useHumanActivity({
    enabled: visible.length > 0,
    root,
    from: start.toMillis(),
    to: start.plus({ days: 1 }).toMillis(),
    zone,
    grouped: grouping === "group",
  });
  const segments = useMemo(
    () =>
      indexHumanActivity(query.records, [date], zone).get(date)?.segments ?? [],
    [query.records, date, zone],
  );
  const records = segments.filter((record) => visible.includes(record.kind));
  return (
    <section
      className="human-day-detail"
      aria-label="人間のアクティビティ（推定）"
    >
      <h3>人間のアクティビティ（推定）</h3>
      <HumanTotals {...summary} dates={[date]} visible={visible} />
      <label>
        表示単位{" "}
        <select
          aria-label="人間の日別表示単位"
          value={grouping}
          onChange={(event) =>
            changeGrouping(event.target.value as GroupingMode)
          }
        >
          <option value="group">まとめ表示</option>
          <option value="turn">記録単位</option>
        </select>
      </label>
      {query.loading ? (
        <p>人間の日別記録を読み込み中…</p>
      ) : query.error ? (
        <p role="alert">
          {query.error === "period_too_dense"
            ? "人間の記録が多すぎます。まとめ表示に変更してください。"
            : "人間の日別記録を取得できません。"}
          <button onClick={query.retry}>人間の日別記録を再試行</button>
        </p>
      ) : records.length === 0 ? (
        <p>選択中の種別の記録はありません。</p>
      ) : (
        <div className="day-turns human-day-records">
          {records.map((record) => {
            const times = `${DateTime.fromMillis(record.start, { zone }).toFormat("HH:mm:ss")} – ${DateTime.fromMillis(record.end, { zone }).toFormat(record.end === start.plus({ days: 1 }).toMillis() ? "'24:00:00'" : "HH:mm:ss")}`;
            return (
              <button
                key={record.id}
                aria-label={`人間 ${humanLabels[record.kind]} ${times}${record.point ? " 点の観測" : " 推定区間"}`}
                onClick={() => open(record)}
              >
                <strong>人間 · {humanLabels[record.kind]}</strong>
                <span>
                  {times} · 推定 {formatDuration(record.end - record.start)}
                  {record.point && " · 点の観測"}
                </span>
              </button>
            );
          })}
        </div>
      )}
      <p className="hint">
        Rei の観測に基づく推定時間です。観測のない空白は含みません。
      </p>
    </section>
  );
}
