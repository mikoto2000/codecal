import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { HumanSyncTimes } from "./HumanSyncTimes";
import type { HumanSyncStatus } from "./types";
const status: HumanSyncStatus = {
  generation: 1,
  revision: 1,
  busy: false,
  error: null,
  last_success: Date.parse("2026-11-26T14:00Z"),
  next_auto_sync: Date.parse("2026-11-26T15:00Z"),
};
it("formats both synchronization times in the configured timezone across midnight", () => {
  render(<HumanSyncTimes status={status} zone="Asia/Tokyo" autoRefresh />);
  expect(screen.getByText("2026/11/26 23:00:00")).toHaveAttribute(
    "datetime",
    "2026-11-26T14:00:00.000Z",
  );
  expect(screen.getByText("2026/11/27 00:00:00")).toBeVisible();
});
it.each([
  [false, false, "自動同期OFF"],
  [true, true, "同期完了後に予定"],
  [true, false, "初回同期待ち"],
])("explains missing next time", (autoRefresh, busy, text) => {
  render(
    <HumanSyncTimes
      status={{ ...status, busy, next_auto_sync: null, last_success: null }}
      zone="UTC"
      autoRefresh={autoRefresh}
    />,
  );
  expect(screen.getByText(text as string)).toBeVisible();
  expect(screen.getByText("未同期")).toBeVisible();
});
