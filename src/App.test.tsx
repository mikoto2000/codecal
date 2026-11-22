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
import { api } from "./api";
import type { Snapshot } from "./types";
vi.mock("./api", () => ({
  api: {
    boot: vi.fn(),
    snapshot: vi.fn(),
    sync: vi.fn(),
    save: vi.fn(),
    project: vi.fn(),
    excerpt: vi.fn(),
    events: vi.fn(),
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
});
const mount = async () => {
  render(<App initialDate="2026-11-20" />);
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
};
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
  fireEvent.click(screen.getByRole("button", { name: "タイムゾーンを確認" }));
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
  expect(screen.getByRole("status")).toHaveTextContent("読み込み中");
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
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Codex: ログディレクトリが見つかりません",
  );
  expect(screen.getByText("要確認")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "診断を見る" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("過去の診断");
  fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
  fireEvent.click(screen.getByRole("button", { name: /同期/ }));
  await waitFor(() =>
    expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
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
  fireEvent.click(screen.getByLabelText("れいの実行履歴を取り込む"));
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
  fireEvent.click(screen.getByLabelText("れいの実行履歴を取り込む"));
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
it("zooms the daily timeline around the pointer and prevents browser zoom", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
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
      screen.getByLabelText("タイムラインの倍率").textContent?.replace("%", ""),
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
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("100%");
  expect(block.style.height).toBe("120px");
});
it("leaves ordinary wheel scrolling and the weekly timeline unchanged", async () => {
  await mount();
  const scroller = document.querySelector(".week-scroll") as HTMLElement;
  const weekly = new WheelEvent("wheel", {
    ctrlKey: true,
    deltaY: -120,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    scroller.dispatchEvent(weekly);
  });
  expect(weekly.defaultPrevented).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  const ordinary = new WheelEvent("wheel", {
    deltaY: 120,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    scroller.dispatchEvent(ordinary);
  });
  expect(ordinary.defaultPrevented).toBe(false);
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("100%");
});
it("clamps daily zoom and keeps the weekly view at its original scale", async () => {
  await mount();
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
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
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("400%");
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
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("400%");
  fireEvent.click(screen.getByRole("button", { name: "週" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(7),
  );
  expect(
    (document.querySelector(".week-grid") as HTMLElement).style.height,
  ).toBe("1441px");
  fireEvent.click(screen.getByRole("button", { name: "日" }));
  await waitFor(() =>
    expect(document.querySelectorAll(".day-column")).toHaveLength(1),
  );
  expect(screen.getByLabelText("タイムラインの倍率")).toHaveTextContent("400%");
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
  expect(screen.getByLabelText("作業ブロックの表示単位")).toHaveValue("group");
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
  fireEvent.change(screen.getByLabelText("作業ブロックの表示単位"), {
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
  expect(screen.getByLabelText("作業ブロックの表示単位")).toHaveValue("turn");
  fireEvent.change(screen.getByLabelText("作業ブロックの表示単位"), {
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
