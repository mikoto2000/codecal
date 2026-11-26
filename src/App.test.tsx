import { it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  within,
} from "@testing-library/react";
import App from "./App";
import { DateTime } from "luxon";
import {
  indexHumanActivity,
  summarizeHumanDays,
  humanKinds,
} from "./human-calendar";
import { api } from "./api";
import type { Snapshot, HumanActivity } from "./types";
vi.mock("./api", () => ({
  api: {
    boot: vi.fn(),
    snapshot: vi.fn(),
    sync: vi.fn(),
    save: vi.fn(),
    project: vi.fn(),
    excerpt: vi.fn(),
    events: vi.fn(),
    humanActivity: vi.fn(),
    humanSummary: vi.fn(),
    humanStatus: vi.fn(),
    humanEvents: vi.fn(),
    humanSync: vi.fn(),
    humanClear: vi.fn(),
  },
}));
const data: Snapshot = {
  projects: [
    { id: "p", name: "Sample project", cwd: "/sample", color: "#14b8a6" },
  ],
  turns: [
    {
      id: "s:t",
      session_id: "s",
      turn_id: "t",
      project_id: "p",
      start: Date.parse("2026-11-20T10:00Z"),
      end: Date.parse("2026-11-20T12:00Z"),
      status: "completed",
      prompt: "合成ログから実装",
      tokens: 123,
    },
  ],
  days: [
    {
      date: "2026-11-20",
      elapsed_ms: 7200000,
      total_ms: 7200000,
      projects: { p: 7200000 },
    },
  ],
  diagnostics: [],
  diagnostics_total: 0,
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.boot).mockResolvedValue({
    config: {
      log_root: "/logs",
      rei_enabled: false,
      rei_root: "/rei",
      timezone: "UTC",
      week_start: 1,
      theme: "system",
      thresholds: [1, 3, 6],
      auto_refresh: true,
    },
    db_path: "/data/codecal.db",
    first_run: false,
    notices: [],
  });
  vi.mocked(api.snapshot).mockResolvedValue(data);
  vi.mocked(api.sync).mockResolvedValue({
    files: 1,
    bytes_read: 10,
    warnings: [],
  });
  vi.mocked(api.events).mockResolvedValue(() => {});
  vi.mocked(api.save).mockResolvedValue([]);
  vi.mocked(api.humanActivity).mockResolvedValue({
    records: [],
    diagnostics: [],
  });
  vi.mocked(api.humanSummary).mockResolvedValue({ days: [], diagnostics: [] });
  vi.mocked(api.humanStatus).mockResolvedValue({
    generation: 2,
    busy: false,
    revision: 1,
    error: null,
    last_success: null,
  });
  vi.mocked(api.humanEvents).mockResolvedValue(() => {});
  vi.mocked(api.humanSync).mockResolvedValue(undefined);
  vi.mocked(api.humanClear).mockResolvedValue({
    compacted: true,
    staging_cleared: true,
  });
});
const mount = async () => {
  render(<App initialDate="2026-11-20" />);
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
};
const humanRecords: HumanActivity[] = [
  {
    start: Date.parse("2026-11-20T10:30Z"),
    end: Date.parse("2026-11-20T11:30Z"),
    kind: "entertainment",
  },
  {
    start: Date.parse("2026-11-20T11:30Z"),
    end: Date.parse("2026-11-20T12:00Z"),
    kind: "non_entertainment",
  },
  {
    start: Date.parse("2026-11-20T12:45Z"),
    end: Date.parse("2026-11-20T12:45Z"),
    kind: "unknown",
  },
];
async function withHuman(records = humanRecords) {
  const boot = await api.boot();
  vi.mocked(api.boot).mockResolvedValue({
    ...boot,
    config: { ...boot.config, human_activity_enabled: true },
  });
  vi.mocked(api.humanActivity).mockResolvedValue({ records, diagnostics: [] });
  vi.mocked(api.humanSummary).mockImplementation(
    async (from, to, zone, visible) => {
      const dates: string[] = [];
      for (
        let day = DateTime.fromMillis(from, { zone });
        day.toMillis() < to;
        day = day.plus({ days: 1 })
      )
        dates.push(day.toISODate()!);
      const index = indexHumanActivity(records, dates, zone);
      return {
        days: dates
          .filter((date) => index.has(date))
          .map((date) => ({
            date,
            day_length_ms:
              DateTime.fromISO(date, { zone }).plus({ days: 1 }).toMillis() -
              DateTime.fromISO(date, { zone }).toMillis(),
            total_ms: summarizeHumanDays(index, [date], visible).total_ms,
            kinds: summarizeHumanDays(index, [date], humanKinds).kinds,
            observed: [
              ...new Set(index.get(date)!.segments.map((s) => s.kind)),
            ],
          })),
        diagnostics: [],
      };
    },
  );
}
it("keeps human observed totals independent of AI and explains overlapping kind totals", async () => {
  await withHuman([
    humanRecords[0],
    { ...humanRecords[1], start: Date.parse("2026-11-20T11:00Z") },
  ]);
  await mount();
  const region = screen.getByRole("region", { name: "人間の観測推定時間" });
  await waitFor(() =>
    expect(within(region).getByTestId("human-total")).toHaveTextContent(
      "1h 30m",
    ),
  );
  expect(within(region).getByLabelText("娯楽の推定時間")).toHaveTextContent(
    "1h 0m",
  );
  expect(within(region).getByLabelText("非娯楽の推定時間")).toHaveTextContent(
    "1h 0m",
  );
  expect(region).toHaveTextContent("種別間で重なる時間");
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 0m");
  fireEvent.click(screen.getByRole("checkbox", { name: "人間の娯楽を表示" }));
  await waitFor(() =>
    expect(within(region).getByTestId("human-total")).toHaveTextContent(
      "1h 0m",
    ),
  );
  expect(
    within(
      screen
        .getByRole("checkbox", { name: "人間の娯楽を表示" })
        .closest("label")!,
    ).getByText("1h 0m"),
  ).toBeVisible();
});
it("adds independent human month bars while preserving AI heat levels and prefilter hours", async () => {
  await withHuman();
  await mount();
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  const cell = await screen.findByRole("button", { name: /^2026-11-20 / });
  expect(cell).toHaveAttribute("data-level", "2");
  await act(async () => {});
  expect(cell.querySelectorAll(".human-day-bar")).toHaveLength(3);
  const bar = within(cell).getByRole("img", { name: "娯楽 推定 1h 0m" });
  expect(
    parseFloat((bar.firstElementChild as HTMLElement).style.width),
  ).toBeCloseTo(100 / 24);
  expect(
    within(cell).getByRole("img", { name: "不明 推定 0h 0m 点の観測" }),
  ).toBeVisible();
  expect(
    screen.getByText(/実際の日長に対する種別ごとの独立した比率/),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("checkbox", { name: "人間の娯楽を表示" }));
  expect(
    within(cell).queryByRole("img", { name: /^娯楽 / }),
  ).not.toBeInTheDocument();
  expect(
    within(
      screen
        .getByRole("checkbox", { name: "人間の娯楽を表示" })
        .closest("label")!,
    ).getByText("1h 0m"),
  ).toBeVisible();
  expect(cell).toHaveAttribute("data-level", "2");
  expect(api.humanActivity).toHaveBeenCalledTimes(1);
});
it("opens human-only monthly day details with a separate AI list and a zero-second observation", async () => {
  await withHuman();
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    projects: [],
    turns: [],
  });
  render(<App initialDate="2026-11-20" />);
  await screen.findByRole("button", { name: /^人間 娯楽 / });
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  const cell = await screen.findByRole("button", { name: /^2026-11-20 / });
  await act(async () => {});
  fireEvent.click(cell);
  const dialog = screen.getByRole("dialog", {
    name: "2026-11-20 のアクティビティ",
  });
  expect(
    within(dialog).getByRole("region", { name: "AIの実行履歴" }),
  ).toHaveTextContent("AI の記録はありません");
  const human = within(dialog).getByRole("region", {
    name: "人間のアクティビティ（推定）",
  });
  const point = await within(human).findByRole("button", {
    name: /^人間 不明 /,
  });
  expect(within(human).getByTestId("human-total")).toHaveTextContent("1h 30m");
  fireEvent.click(point);
  const detail = screen.getByRole("dialog", {
    name: "人間のアクティビティ（推定）",
  });
  expect(detail).toHaveTextContent("0h 0m 0.000s");
  expect(detail).not.toHaveTextContent("/rei");
  expect(detail).not.toHaveTextContent("Sample project");
});
it("keeps independent filter hours and targets bulk operations to their own lists", async () => {
  await withHuman();
  await mount();
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  const humanCheckbox = screen.getByRole("checkbox", {
    name: "人間の娯楽を表示",
  });
  expect(
    within(humanCheckbox.closest("label")!).getByText("1h 0m"),
  ).toBeVisible();
  fireEvent.click(humanCheckbox);
  expect(
    within(humanCheckbox.closest("label")!).getByText("1h 0m"),
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: /^人間 娯楽 / }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
  ).toBeVisible();
  const project = screen.getByRole("checkbox", {
    name: "Sample project を表示",
  });
  fireEvent.click(project);
  expect(within(project.closest("label")!).getByText("2h 0m")).toBeVisible();
  expect(screen.getByRole("button", { name: /^人間 非娯楽 / })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "人間の種別をすべてON" }));
  expect(humanCheckbox).toBeChecked();
  expect(project).not.toBeChecked();
  fireEvent.click(
    screen.getByRole("button", { name: "プロジェクトをすべてON" }),
  );
  expect(project).toBeChecked();
  fireEvent.click(
    screen.getByRole("button", { name: "人間の種別をすべてOFF" }),
  );
  expect(
    screen.queryByRole("button", { name: /^人間 (娯楽|非娯楽|不明) / }),
  ).not.toBeInTheDocument();
  expect(api.humanActivity).toHaveBeenCalledTimes(1);
});
it("shows human-only records with zero projects and preserves filters through view changes", async () => {
  await withHuman();
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    projects: [],
    turns: [],
  });
  render(<App initialDate="2026-11-20" />);
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  expect(
    screen.getByRole("button", { name: "プロジェクトをすべてON" }),
  ).toBeDisabled();
  expect(
    await screen.findByRole("button", { name: /^人間 娯楽 / }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("checkbox", { name: "人間の娯楽を表示" }));
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  expect(
    screen.getByRole("checkbox", { name: "人間の娯楽を表示" }),
  ).not.toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  expect(
    screen.getByRole("checkbox", { name: "人間の娯楽を表示" }),
  ).not.toBeChecked();
});
it("limits human detail fields and closes it permanently after the feature is disabled", async () => {
  const marker = "PRIVATE_RENDER_MARKER";
  const record = {
    ...humanRecords[0],
    native_id: marker,
    source_path: marker,
    raw_disposition: marker,
    foreground: { title: marker },
  } as HumanActivity;
  await withHuman([record]);
  await mount();
  const block = await screen.findByRole("button", { name: /^人間 娯楽 / });
  expect(block.getAttribute("title")).not.toContain(marker);
  fireEvent.click(block);
  const detail = screen.getByRole("dialog", {
    name: "人間のアクティビティ（推定）",
  });
  expect(detail).toHaveTextContent("推定終了時刻");
  expect(detail).toHaveTextContent("Rei の観測に基づく推定");
  expect(detail).not.toHaveTextContent(marker);
  expect(
    within(detail).queryByRole("button", { name: "ログの該当箇所を表示" }),
  ).not.toBeInTheDocument();
  // Exercise a saved config response arriving while a human selection exists.
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(
    screen.getByLabelText("人間のアクティビティ機能を有効にする"),
  );
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("dialog", { name: "人間のアクティビティ（推定）" }),
    ).not.toBeInTheDocument(),
  );
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(
    screen.getByLabelText("人間のアクティビティ機能を有効にする"),
  );
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await screen.findByRole("button", { name: /^人間 娯楽 / });
  expect(
    screen.queryByRole("dialog", { name: "人間のアクティビティ（推定）" }),
  ).not.toBeInTheDocument();
});
it("opens a zero-second human point and clears its detail when the date changes", async () => {
  await withHuman();
  await mount();
  fireEvent.click(
    await screen.findByRole("button", { name: /^人間 不明 .*点の観測/ }),
  );
  const detail = screen.getByRole("dialog", {
    name: "人間のアクティビティ（推定）",
  });
  expect(detail).toHaveTextContent("0h 0m 0.000s");
  fireEvent.change(screen.getByLabelText("表示日"), {
    target: { value: "2026-11-21" },
  });
  await waitFor(() =>
    expect(
      screen.queryByRole("dialog", { name: "人間のアクティビティ（推定）" }),
    ).not.toBeInTheDocument(),
  );
});
it("enables human activity independently only after successful settings save", async () => {
  await mount();
  expect(api.humanActivity).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  const checkbox =
    screen.getByLabelText("人間のアクティビティ機能を有効にする");
  expect(checkbox).not.toBeChecked();
  fireEvent.click(checkbox);
  expect(screen.getByLabelText("れい データディレクトリ")).toBeEnabled();
  expect(api.humanActivity).not.toHaveBeenCalled();
  vi.mocked(api.save).mockRejectedValueOnce(
    new Error("synthetic settings failure"),
  );
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await screen.findByText("Error: synthetic settings failure");
  expect(api.humanActivity).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalled());
  expect(api.save).toHaveBeenLastCalledWith(
    expect.objectContaining({
      rei_enabled: false,
      human_activity_enabled: true,
    }),
  );
});
it("cancels human settings edits and confirms cache deletion while OFF", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(
    screen.getByLabelText("人間のアクティビティ機能を有効にする"),
  );
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  expect(
    screen.getByLabelText("人間のアクティビティ機能を有効にする"),
  ).not.toBeChecked();
  fireEvent.change(screen.getByLabelText("タイムゾーン", { exact: true }), {
    target: { value: "Asia/Tokyo" },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "人間のアクティビティ履歴を削除する" }),
  );
  expect(api.humanClear).not.toHaveBeenCalled();
  fireEvent.click(screen.getByLabelText("すべてのソース"));
  fireEvent.click(
    screen.getByRole("button", { name: "履歴を削除して機能をOFFにする" }),
  );
  await waitFor(() => expect(api.humanClear).toHaveBeenCalledWith(true));
  await waitFor(() =>
    expect(
      screen.queryByRole("dialog", { name: "人間のアクティビティ履歴の削除" }),
    ).not.toBeInTheDocument(),
  );
  expect(screen.getByLabelText("タイムゾーン", { exact: true })).toHaveValue(
    "Asia/Tokyo",
  );
  expect(
    screen.getByLabelText("人間のアクティビティ機能を有効にする"),
  ).not.toBeChecked();
  expect(api.humanActivity).not.toHaveBeenCalled();
});
it("returns focus to the settings trigger and associates the visible dialog title", async () => {
  await mount();
  const trigger = screen.getByRole("button", { name: "設定" });
  trigger.focus();
  fireEvent.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "設定" });
  expect(dialog).toHaveAttribute(
    "aria-labelledby",
    within(dialog).getByRole("heading", { name: "設定" }).id,
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "閉じる" }));
  expect(trigger).toHaveFocus();
});
it.each([
  [99, 150],
  [401, 150],
  [150, 99],
  [150, 301],
])(
  "closes settings on an outside click at (%i, %i)",
  async (clientX, clientY) => {
    await mount();
    const trigger = screen.getByRole("button", { name: "設定" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "設定" });
    vi.spyOn(dialog, "getBoundingClientRect").mockReturnValue({
      left: 100,
      right: 400,
      top: 100,
      bottom: 300,
      x: 100,
      y: 100,
      width: 300,
      height: 200,
      toJSON: () => ({}),
    });
    fireEvent.mouseDown(dialog, { clientX, clientY });
    fireEvent.click(dialog, { clientX, clientY });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  },
);
it("keeps settings open for content, padding and drags starting inside", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  const dialog = screen.getByRole("dialog", { name: "設定" });
  vi.spyOn(dialog, "getBoundingClientRect").mockReturnValue({
    left: 100,
    right: 400,
    top: 100,
    bottom: 300,
    x: 100,
    y: 100,
    width: 300,
    height: 200,
    toJSON: () => ({}),
  });
  const heading = within(dialog).getByRole("heading", { name: "設定" });
  fireEvent.mouseDown(heading, { clientX: 150, clientY: 150 });
  fireEvent.click(heading, { clientX: 150, clientY: 150 });
  fireEvent.mouseDown(dialog, { clientX: 105, clientY: 105 });
  fireEvent.click(dialog, { clientX: 105, clientY: 105 });
  expect(dialog).toBeInTheDocument();
  fireEvent.mouseDown(heading, { clientX: 150, clientY: 150 });
  fireEvent.mouseUp(dialog, { clientX: 50, clientY: 50 });
  fireEvent.click(dialog, { clientX: 50, clientY: 50 });
  expect(dialog).toBeInTheDocument();
});
it("keeps project drafts on ordinary reloads and reflects externally changed fields", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("Sample project の表示名"), {
    target: { value: "My draft" },
  });
  const events = vi.mocked(api.events).mock.calls.at(-1)!;
  await act(async () => {
    events[0]();
  });
  expect(screen.getByLabelText("Sample project の表示名")).toHaveValue(
    "My draft",
  );
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    projects: [{ ...data.projects[0], color: "#ffffff" }],
  });
  await act(async () => {
    events[0]();
  });
  expect(screen.getByLabelText("Sample project の表示名")).toHaveValue(
    "My draft",
  );
  expect(screen.getByLabelText("Sample project の色")).toHaveValue("#ffffff");
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    projects: [{ ...data.projects[0], name: "External name" }],
  });
  await act(async () => {
    events[0]();
  });
  expect(screen.getByLabelText("External name の表示名")).toHaveValue(
    "External name",
  );
  vi.mocked(api.snapshot).mockResolvedValue(data);
  await act(async () => {
    events[0]();
  });
  expect(screen.getByLabelText("Sample project の表示名")).toHaveValue(
    "Sample project",
  );
});
it("keeps background warnings separate from failures and clears resolved warnings", async () => {
  await mount();
  const events = vi.mocked(api.events).mock.calls.at(-1)!;
  await act(async () => {
    events[4]?.(["一部のログを読み飛ばしました"]);
    events[5]?.("監視を開始できません");
  });
  const warning = screen.getByRole("status", { name: "同期の警告" });
  expect(warning).toHaveTextContent("一部のログを読み飛ばしました");
  expect(warning).toHaveTextContent("監視を開始できません");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  await act(async () => {
    events[2]("同期に失敗しました");
    events[4]?.([]);
  });
  expect(screen.getByRole("alert")).toHaveTextContent("同期に失敗しました");
  expect(
    screen.getByRole("status", { name: "同期の警告" }),
  ).not.toHaveTextContent("一部のログを読み飛ばしました");
});
it("offers detected settings on first launch without blocking the calendar", async () => {
  const boot = await api.boot();
  vi.mocked(api.boot).mockResolvedValue({ ...boot, first_run: true });
  await mount();
  const setup = screen.getByRole("region", { name: "はじめての設定" });
  expect(setup).toHaveTextContent("/logs");
  expect(setup).toHaveTextContent("UTC");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.click(
    within(setup).getByRole("button", { name: "初期設定を確認" }),
  );
  expect(screen.getByRole("dialog", { name: "設定" })).toBeInTheDocument();
  expect(screen.getByLabelText("Codex ログディレクトリ")).toHaveValue("/logs");
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  fireEvent.click(within(setup).getByRole("button", { name: "案内を閉じる" }));
  expect(
    screen.queryByRole("region", { name: "はじめての設定" }),
  ).not.toBeInTheDocument();
});
it("does not show first-launch guidance for a saved configuration", async () => {
  await mount();
  expect(
    screen.queryByRole("region", { name: "はじめての設定" }),
  ).not.toBeInTheDocument();
});
it("makes enabled Rei import explicit in first-launch guidance", async () => {
  const boot = await api.boot();
  vi.mocked(api.boot).mockResolvedValue({
    ...boot,
    first_run: true,
    config: { ...boot.config, rei_enabled: true },
  });
  await mount();
  const setup = screen.getByRole("region", { name: "はじめての設定" });
  expect(setup).toHaveTextContent("れい: /rei");
  expect(setup).toHaveTextContent("れいの履歴取り込みが有効です");
});
it("renders weekly activity and confirmed totals", async () => {
  await mount();
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 0m");
});
it("shows timezone candidates and a resolved offset, including invalid input", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  const input = screen.getByLabelText("タイムゾーン", { exact: true });
  expect(input).toHaveAttribute("list", "timezone-choices");
  fireEvent.change(input, { target: { value: "Asia/Tokyo" } });
  expect(screen.getByText(/現在の時差 UTC\+09:00/)).toBeInTheDocument();
  fireEvent.change(input, { target: { value: "invalid/zone" } });
  expect(
    screen.getByText("タイムゾーンを確認してください（例: Asia/Tokyo、UTC）"),
  ).toBeInTheDocument();
});
it("keeps a timezone fallback notice visible and links to settings", async () => {
  const boot = await api.boot();
  vi.mocked(api.boot).mockResolvedValue({
    ...boot,
    notices: [
      "端末のタイムゾーンを認識できなかったため UTC を使用しています。",
    ],
  });
  await mount();
  expect(screen.getByText(/認識できなかったため UTC/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "設定を確認" }));
  expect(screen.getByRole("dialog", { name: "設定" })).toBeInTheDocument();
});
it("opens turn detail and reads local source only on request", async () => {
  await mount();
  fireEvent.click(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
  );
  expect(screen.getByRole("dialog")).toHaveTextContent("合成ログから実装");
  expect(screen.getByRole("dialog")).toHaveTextContent("123");
  vi.mocked(api.excerpt).mockResolvedValue("synthetic source");
  fireEvent.click(screen.getByRole("button", { name: "ログの該当箇所を表示" }));
  expect(await screen.findByText("synthetic source")).toBeInTheDocument();
});
it("navigates weeks and returns to a chosen date", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "次へ" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: /Sample project.*10:00/ }),
    ).not.toBeInTheDocument(),
  );
  fireEvent.change(screen.getByLabelText("表示日"), {
    target: { value: "2026-11-20" },
  });
  expect(
    await screen.findByRole("button", { name: /Sample project.*10:00/ }),
  ).toBeInTheDocument();
});
it("filters projects", async () => {
  await mount();
  await screen.findByText("同期済み");
  fireEvent.click(screen.getByLabelText("Sample project を表示"));
  expect(
    screen.queryByRole("button", { name: /Sample project.*10:00/ }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByText("表示するプロジェクトが選択されていません"),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "すべてのプロジェクトを表示" }),
  );
  expect(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
  ).toBeInTheDocument();
});
it("renders heatmap and navigates from a selected day to week", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  const cell = await screen.findByRole("button", { name: "2026-11-20 2h 0m" });
  expect(cell).toHaveAttribute("data-level", "2");
  fireEvent.click(cell);
  expect(screen.getByRole("dialog")).toHaveTextContent("合成ログから実装");
  fireEvent.click(screen.getByRole("button", { name: "週表示で開く" }));
  expect(
    await screen.findByRole("button", { name: /Sample project.*10:00/ }),
  ).toBeInTheDocument();
});
it("shows empty state", async () => {
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    turns: [],
    days: [],
    projects: [],
  });
  render(<App initialDate="2026-11-20" />);
  expect(
    await screen.findByText("取り込まれた稼働履歴がありません"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "診断を見る" }));
  expect(
    screen.getByRole("dialog", { name: "取り込み診断" }),
  ).toBeInTheDocument();
});
it("distinguishes a period with no activity from an empty import", async () => {
  render(<App initialDate="2026-12-20" />);
  expect(
    await screen.findByText("表示期間の稼働はありません"),
  ).toBeInTheDocument();
  expect(
    screen.queryByText("取り込まれた稼働履歴がありません"),
  ).not.toBeInTheDocument();
});
it("shows loading state", () => {
  vi.mocked(api.boot).mockReturnValue(new Promise(() => {}));
  render(<App />);
  expect(screen.getByRole("status")).toHaveTextContent(
    "CodeCal がデータベースを確認しています…",
  );
});
it("shows recoverable backend errors", async () => {
  vi.mocked(api.boot).mockRejectedValue(new Error("synthetic error"));
  render(<App />);
  expect(await screen.findByRole("alert")).toHaveTextContent("synthetic error");
});
it("shows partial import failures and clears them after a successful retry", async () => {
  vi.mocked(api.sync).mockResolvedValueOnce({
    files: 1,
    bytes_read: 10,
    warnings: ["Codex: ログディレクトリが見つかりません"],
  });
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    diagnostics: ["過去の診断"],
  });
  await mount();
  expect(
    await screen.findByRole("status", { name: "同期の警告" }),
  ).toHaveTextContent("Codex: ログディレクトリが見つかりません");
  expect(screen.getByText("要確認")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "診断を見る" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("過去の診断");
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  fireEvent.click(screen.getByRole("button", { name: /同期/ }));
  await waitFor(() =>
    expect(
      screen.queryByRole("status", { name: "同期の警告" }),
    ).not.toBeInTheDocument(),
  );
  expect(screen.getByText("同期済み")).toBeInTheDocument();
});
it("saves theme and timezone configuration", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("テーマ"), {
    target: { value: "dark" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({ theme: "dark" }),
    ),
  );
  await waitFor(() =>
    expect(document.documentElement.dataset.theme).toBe("dark"),
  );
});
it("keeps save failures inside settings and allows correction and retry", async () => {
  vi.mocked(api.save).mockRejectedValueOnce(
    new Error("タイムゾーンが不正です"),
  );
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("タイムゾーン"), {
    target: { value: "invalid" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  const dialog = screen.getByRole("dialog", { name: "設定" });
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "タイムゾーンが不正です",
  );
  expect(screen.getByLabelText("タイムゾーン")).toHaveValue("invalid");
  fireEvent.change(screen.getByLabelText("タイムゾーン"), {
    target: { value: "UTC" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});
it("shows refresh failures after a successful settings save in the workspace", async () => {
  await mount();
  await screen.findByText("同期済み");
  vi.mocked(api.snapshot).mockRejectedValueOnce(
    new Error("更新に失敗しました"),
  );
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "更新に失敗しました",
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
it("shows missing-directory warnings inside settings after saving", async () => {
  vi.mocked(api.save).mockResolvedValueOnce([
    "Codex のログディレクトリが見つかりません: /missing",
  ]);
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("Codex ログディレクトリ"), {
    target: { value: "/missing" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  const dialog = screen.getByRole("dialog", { name: "設定" });
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "設定は保存しました",
  );
  expect(dialog).toHaveTextContent("/missing");
  expect(screen.getByLabelText("Codex ログディレクトリ")).toHaveValue(
    "/missing",
  );
  fireEvent.change(screen.getByLabelText("Codex ログディレクトリ"), {
    target: { value: "/logs" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});
it("saves the Rei source independently of Codex", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  const root = screen.getByLabelText("れい データディレクトリ");
  expect(root).toBeDisabled();
  fireEvent.click(screen.getByLabelText("れいの AI 実行履歴を取り込む"));
  fireEvent.change(root, { target: { value: "/rei-data" } });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        log_root: "/logs",
        rei_enabled: true,
        rei_root: "/rei-data",
      }),
    ),
  );
});
it.each([
  ["テーマ", "dark"],
  ["週の開始曜日", "0"],
  ["タイムゾーン", "Asia/Tokyo"],
  ["しきい値 1", "0.5"],
])(
  "refreshes display settings without scanning logs: %s",
  async (label, value) => {
    await mount();
    await screen.findByText("同期済み");
    vi.mocked(api.sync).mockClear();
    vi.mocked(api.snapshot).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "設定" }));
    fireEvent.change(screen.getByLabelText(label, { exact: true }), {
      target: { value },
    });
    fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
    await waitFor(() => expect(api.snapshot).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(api.sync).not.toHaveBeenCalled();
  },
);
it("does not scan for auto-refresh or unchanged saves, but does scan for source changes", async () => {
  await mount();
  await screen.findByText("同期済み");
  vi.mocked(api.sync).mockClear();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByLabelText("自動更新を有効にする"));
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(api.sync).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(api.sync).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("Codex ログディレクトリ"), {
    target: { value: "/other" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() => expect(api.sync).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByLabelText("れいの AI 実行履歴を取り込む"));
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() => expect(api.sync).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.change(screen.getByLabelText("れい データディレクトリ"), {
    target: { value: "/other-rei" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() => expect(api.sync).toHaveBeenCalledTimes(3));
});
it("shows Rei attribution and its request in the activity detail", async () => {
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    turns: data.turns.map((t) => ({ ...t, source: "rei" })),
  });
  await mount();
  fireEvent.click(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
  );
  expect(screen.getByRole("dialog")).toHaveTextContent("れい への依頼");
  expect(screen.getByRole("dialog")).toHaveTextContent("実行 ID");
});
it("shows import diagnostics", async () => {
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    diagnostics: ["line 5: invalid JSON"],
    diagnostics_total: 1,
  });
  await mount();
  fireEvent.click(screen.getByRole("button", { name: /診断/ }));
  expect(screen.getByRole("dialog")).toHaveTextContent("line 5: invalid JSON");
});
it("copies displayed diagnostics with their total and omitted count", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  try {
    vi.mocked(api.snapshot).mockResolvedValue({
      ...data,
      diagnostics: ["synthetic diagnostic"],
      diagnostics_total: 201,
    });
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "診断 (201)" }));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "表示 1 件 / 全 201 件（他 200 件は省略）",
    );
    fireEvent.click(screen.getByRole("button", { name: "診断をコピー" }));
    await screen.findByText("表示中の診断をコピーしました。");
    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining("synthetic diagnostic"),
    );
    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining("他 200 件は省略"),
    );
  } finally {
    vi.unstubAllGlobals();
  }
});
it("offers selectable diagnostic text when clipboard writing fails", async () => {
  vi.stubGlobal("navigator", {
    ...navigator,
    clipboard: {
      writeText: vi.fn().mockRejectedValue(new Error("unavailable")),
    },
  });
  try {
    vi.mocked(api.snapshot).mockResolvedValue({
      ...data,
      diagnostics: ["synthetic diagnostic"],
      diagnostics_total: 1,
    });
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "診断 (1)" }));
    fireEvent.click(screen.getByRole("button", { name: "診断をコピー" }));
    expect(await screen.findByLabelText("コピー用の診断")).toHaveValue(
      "表示 1 件 / 全 1 件\n\nsynthetic diagnostic",
    );
    fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
    fireEvent.click(screen.getByRole("button", { name: "診断 (1)" }));
    expect(screen.queryByLabelText("コピー用の診断")).not.toBeInTheDocument();
  } finally {
    vi.unstubAllGlobals();
  }
});

afterEach(() => vi.restoreAllMocks());
it("paints a loading spinner before changing calendar views", async () => {
  await mount();
  const frames: FrameRequestCallback[] = [];
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.push(callback);
    return frames.length;
  });
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  expect(
    screen.getByRole("status", { name: "カレンダーを切り替え中" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("main")).toHaveAttribute("aria-busy", "true");
  act(() => frames.shift()!(0));
  expect(
    screen.getByRole("status", { name: "カレンダーを切り替え中" }),
  ).toBeInTheDocument();
  act(() => frames.shift()!(16));
  expect(
    screen.queryByRole("status", { name: "カレンダーを切り替え中" }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("main")).toHaveAttribute("aria-busy", "false");
  expect(
    screen.getByRole("button", { name: "2026-11-20 2h 0m" }),
  ).toBeInTheDocument();
});
it("renders one day and moves between adjacent dates", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  expect(screen.getByLabelText("表示日")).toHaveValue("2026-11-20");
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 0m");
  fireEvent.click(screen.getByRole("button", { name: "次へ" }));
  await waitFor(() =>
    expect(screen.getByLabelText("表示日")).toHaveValue("2026-11-21"),
  );
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("0h 0m");
  fireEvent.click(screen.getByRole("button", { name: "前へ" }));
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
});
it("opens a selected monthly date in day view", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "2026-11-20 2h 0m" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "1日表示で開く" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  expect(screen.getByLabelText("表示日")).toHaveValue("2026-11-20");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
it.each(["日", "週"])(
  "zooms the %s timeline around the pointer and prevents browser zoom",
  async (view) => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: view }));
    await waitFor(() =>
      expect(document.querySelectorAll(".day-column")).toHaveLength(
        view === "日" ? 1 : 7,
      ),
    );
    const scroller = document.querySelector(".week-scroll") as HTMLElement;
    Object.defineProperty(scroller, "clientHeight", {
      configurable: true,
      value: 400,
    });
    vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue({
      top: 100,
      bottom: 500,
      left: 0,
      right: 1000,
      x: 0,
      y: 100,
      width: 1000,
      height: 400,
      toJSON: () => ({}),
    });
    scroller.scrollTop = 480;
    const wheel = new WheelEvent("wheel", {
      ctrlKey: true,
      deltaY: -120,
      clientY: 220,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      scroller.dispatchEvent(wheel);
    });
    expect(wheel.defaultPrevented).toBe(true);
    const scale =
      Number(
        screen
          .getByLabelText("タイムラインの倍率")
          .textContent?.replace("%", ""),
      ) / 100;
    expect(scale).toBeGreaterThan(1);
    const exactScale =
      (parseFloat(
        (document.querySelector(".week-grid") as HTMLElement).style.height,
      ) -
        1) /
      1440;
    expect((scroller.scrollTop + 120) / exactScale).toBeCloseTo(600, 0);
    const block = screen.getByRole("button", { name: /Sample project.*10:00/ });
    expect(parseFloat(block.style.height)).toBeGreaterThan(120);
    fireEvent.click(
      screen.getByRole("button", { name: "タイムラインの倍率をリセット" }),
    );
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "100%",
    );
    expect(block.style.height).toBe("120px");
  },
);
it.each(["日", "週"])(
  "leaves ordinary wheel scrolling unchanged in %s view",
  async (view) => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: view }));
    await waitFor(() =>
      expect(document.querySelectorAll(".day-column")).toHaveLength(
        view === "日" ? 1 : 7,
      ),
    );
    const scroller = document.querySelector(".week-scroll") as HTMLElement;
    const ordinary = new WheelEvent("wheel", {
      deltaY: 120,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      scroller.dispatchEvent(ordinary);
    });
    expect(ordinary.defaultPrevented).toBe(false);
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "100%",
    );
  },
);
it.each(["日", "週"])(
  "clamps zoom in %s view and shares the scale across timeline views",
  async (view) => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: view }));
    await waitFor(() =>
      expect(document.querySelectorAll(".day-column")).toHaveLength(
        view === "日" ? 1 : 7,
      ),
    );
    const scroller = document.querySelector(".week-scroll")!;
    act(() => {
      scroller.dispatchEvent(
        new WheelEvent("wheel", {
          ctrlKey: true,
          deltaY: -10000,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "400%",
    );
    const limited = new WheelEvent("wheel", {
      ctrlKey: true,
      deltaY: -120,
      bubbles: true,
      cancelable: true,
    });
    act(() => {
      scroller.dispatchEvent(limited);
    });
    expect(limited.defaultPrevented).toBe(true);
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "400%",
    );
    fireEvent.click(
      screen.getByRole("button", { name: view === "日" ? "週" : "日" }),
    );
    await waitFor(() =>
      expect(document.querySelectorAll(".day-column")).toHaveLength(
        view === "日" ? 7 : 1,
      ),
    );
    expect(
      (document.querySelector(".week-grid") as HTMLElement).style.height,
    ).toBe("5761px");
    fireEvent.click(screen.getByRole("button", { name: "日" }));
    await waitFor(() =>
      expect(document.querySelectorAll(".day-column")).toHaveLength(1),
    );
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "400%",
    );
    act(() => {
      scroller.dispatchEvent(
        new WheelEvent("wheel", {
          ctrlKey: true,
          deltaY: 10000,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent(
      "50%",
    );
  },
);
it("reattaches weekly zoom after returning from the month view", async () => {
  await mount();
  const originalScroller = document.querySelector(".week-scroll")!;
  fireEvent.wheel(originalScroller, { ctrlKey: true, deltaY: -10000 });
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("400%");
  fireEvent.click(screen.getByRole("button", { name: "月" }));
  await waitFor(() =>
    expect(document.querySelector(".week-scroll")).toBeNull(),
  );
  expect(screen.queryByLabelText("タイムラインの倍率")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "週" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(7),
  );
  const scroller = document.querySelector(".week-scroll")!;
  expect(scroller).not.toBe(originalScroller);
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("400%");
  const wheel = new WheelEvent("wheel", {
    ctrlKey: true,
    deltaY: 10000,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    scroller.dispatchEvent(wheel);
  });
  expect(wheel.defaultPrevented).toBe(true);
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("50%");
});
const groupedData: Snapshot = {
  ...data,
  turns: [
    data.turns[0],
    {
      ...data.turns[0],
      id: "s:t2",
      turn_id: "t2",
      start: Date.parse("2026-11-20T12:05Z"),
      end: Date.parse("2026-11-20T12:20Z"),
      status: "interrupted",
      prompt: "次の依頼",
    },
    {
      ...data.turns[0],
      id: "s:t3",
      turn_id: "t3",
      start: Date.parse("2026-11-20T11:00Z"),
      end: Date.parse("2026-11-20T11:30Z"),
      prompt: "並列の依頼",
    },
  ],
};
it("defaults to grouped blocks and preserves exact totals and every turn in detail", async () => {
  vi.mocked(api.snapshot).mockResolvedValue(groupedData);
  await mount();
  expect(document.querySelectorAll(".activity-block")).toHaveLength(1);
  expect(screen.getByLabelText("ブロックの表示単位")).toHaveValue("group");
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 15m");
  fireEvent.click(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog).toHaveTextContent("作業のまとまり");
  expect(dialog).toHaveTextContent("2h 15m");
  expect(dialog).toHaveTextContent("2h 45m");
  expect(dialog).toHaveTextContent("完了 2");
  expect(dialog).toHaveTextContent("中断 1");
  fireEvent.click(screen.getByRole("button", { name: /次の依頼/ }));
  expect(screen.getByRole("dialog")).toHaveTextContent("t2");
  expect(screen.getByRole("dialog")).toHaveTextContent("次の依頼");
  fireEvent.click(screen.getByRole("button", { name: "作業のまとまりに戻る" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("2h 45m");
});
it("switches between grouped and individual blocks without changing the summaries", async () => {
  vi.mocked(api.snapshot).mockResolvedValue(groupedData);
  await mount();
  fireEvent.change(screen.getByLabelText("ブロックの表示単位"), {
    target: { value: "turn" },
  });
  await waitFor(() =>
    expect(document.querySelectorAll(".activity-block")).toHaveLength(3),
  );
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 15m");
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  expect(screen.getByLabelText("ブロックの表示単位")).toHaveValue("turn");
  fireEvent.change(screen.getByLabelText("ブロックの表示単位"), {
    target: { value: "group" },
  });
  await waitFor(() =>
    expect(document.querySelectorAll(".activity-block")).toHaveLength(1),
  );
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 15m");
});
it("shows Claude source in timeline and details and saves import settings", async () => {
  const claude = {
    ...data,
    turns: [
      { ...data.turns[0], source: "claude" as const, prompt: "ツール呼び出し" },
    ],
  };
  vi.mocked(api.snapshot).mockResolvedValue(claude);
  await mount();
  const block = screen.getByRole("button", { name: /Sample project.*10:00/ });
  expect(block).toHaveTextContent("Claude Code");
  expect(block).toHaveAccessibleName(/Claude Code/);
  expect(block).toHaveAttribute(
    "title",
    expect.stringContaining("Claude Code"),
  );
  fireEvent.click(block);
  expect(screen.getByRole("dialog")).toHaveTextContent("Claude Code");
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  fireEvent.click(screen.getByRole("button", { name: "設定" }));
  fireEvent.click(screen.getByLabelText("Claude Code の履歴を取り込む"));
  fireEvent.change(screen.getByLabelText("Claude Code projects ディレクトリ"), {
    target: { value: "/claude/projects" },
  });
  fireEvent.click(screen.getByRole("button", { name: "設定を保存" }));
  await waitFor(() =>
    expect(api.save).toHaveBeenCalledWith(
      expect.objectContaining({
        claude_enabled: true,
        claude_root: "/claude/projects",
      }),
    ),
  );
});
it("shows a spinner and migration status until startup completes", async () => {
  let finish!: (value: Awaited<ReturnType<typeof api.boot>>) => void;
  const boot = await api.boot();
  vi.mocked(api.boot).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  render(<App initialDate="2026-11-20" />);
  expect(screen.getByRole("status")).toHaveTextContent(
    "CodeCal がデータベースを確認しています…",
  );
  expect(
    screen.getByRole("status").querySelector(".spinner"),
  ).toBeInTheDocument();
  await waitFor(() => expect(finish).toBeDefined());
  const onStartup = vi.mocked(api.events).mock.calls[0][3];
  act(() => onStartup?.("migrating"));
  expect(screen.getByRole("status")).toHaveTextContent(
    "CodeCal がデータベースを移行しています…",
  );
  await act(async () => finish(boot));
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
  expect(
    screen.queryByText("CodeCal がデータベースを移行しています…"),
  ).not.toBeInTheDocument();
});
it("stops the startup spinner on failure and retries database initialization", async () => {
  vi.mocked(api.boot).mockRejectedValueOnce(
    new Error("synthetic migration failure"),
  );
  render(<App initialDate="2026-11-20" />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "synthetic migration failure",
  );
  expect(
    document.querySelector(".startup-screen .spinner"),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "再試行" }));
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
});
