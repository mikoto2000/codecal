import { it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
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
  expect(
    screen.queryByRole("button", { name: /Sample project.*10:00/ }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("表示日"), {
    target: { value: "2026-11-20" },
  });
  expect(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
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
  const cell = screen.getByRole("button", { name: "2026-11-20 2h 0m" });
  expect(cell).toHaveAttribute("data-level", "2");
  fireEvent.click(cell);
  expect(screen.getByRole("dialog")).toHaveTextContent("合成ログから実装");
  fireEvent.click(screen.getByRole("button", { name: "週表示で開く" }));
  expect(
    screen.getByRole("button", { name: /Sample project.*10:00/ }),
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
it("shows import diagnostics", async () => {
  vi.mocked(api.snapshot).mockResolvedValue({
    ...data,
    diagnostics: ["line 5: invalid JSON"],
  });
  await mount();
  fireEvent.click(screen.getByRole("button", { name: /診断/ }));
  expect(screen.getByRole("dialog")).toHaveTextContent("line 5: invalid JSON");
});
