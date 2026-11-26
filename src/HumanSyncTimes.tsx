import { DateTime } from "luxon";
import type { HumanSyncStatus } from "./types";

export function HumanSyncTimes({
  status,
  zone,
  autoRefresh,
}: {
  status: HumanSyncStatus;
  zone: string;
  autoRefresh: boolean;
}) {
  const time = (value: number | null | undefined, empty: string) => {
    if (value == null || !Number.isFinite(value)) return empty;
    const date = DateTime.fromMillis(value, { zone });
    return date.isValid ? (
      <time dateTime={new Date(value).toISOString()} title={zone}>
        {date.toFormat("yyyy/MM/dd HH:mm:ss")}
      </time>
    ) : (
      empty
    );
  };
  return (
    <dl className="human-sync-times">
      <dt>最終同期時刻</dt>
      <dd>{time(status.last_success, "未同期")}</dd>
      <dt>次回の自動同期予定時刻</dt>
      <dd>
        {!autoRefresh
          ? "自動同期OFF"
          : time(
              status.next_auto_sync,
              status.busy ? "同期完了後に予定" : "初回同期待ち",
            )}
      </dd>
    </dl>
  );
}
