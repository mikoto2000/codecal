import { it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
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
    first_run: true,
  });
  vi.mocked(api.snapshot).mockResolvedValue(data);
  vi.mocked(api.sync).mockResolvedValue({ files: 1, bytes_read: 10 });
  vi.mocked(api.events).mockResolvedValue(() => {});
  vi.mocked(api.save).mockResolvedValue();
});
const mount = async () => {
  render(<App initialDate="2026-11-20" />);
  await screen.findByRole("button", { name: /Sample project.*10:00/ });
};
it("renders weekly activity and confirmed totals", async () => {
  await mount();
  expect(screen.getByTestId("total-elapsed")).toHaveTextContent("2h 0m");
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
  fireEvent.click(screen.getByLabelText("Sample project を表示"));
  expect(
    screen.queryByRole("button", { name: /Sample project.*10:00/ }),
  ).not.toBeInTheDocument();
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
    await screen.findByText("表示期間の稼働はありません"),
  ).toBeInTheDocument();
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
  });
  await mount();
  fireEvent.click(screen.getByRole("button", { name: /診断/ }));
  expect(screen.getByRole("dialog")).toHaveTextContent("line 5: invalid JSON");
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
