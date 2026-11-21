import {
  test,
  expect,
  chromium,
  type Browser,
  type Page,
} from "@playwright/test";
import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { appendFileSync, mkdirSync, writeFileSync, existsSync } from "node:fs";
import path from "node:path";
const exe = path.resolve("src-tauri/target/release/codecal.exe");
const root = path.resolve(".local", `native-${Date.now()}`);
const logs = path.join(root, "logs");
const appdata = path.join(root, "appdata");
let process: ChildProcess;
let browser: Browser;
let page: Page;
const event = (type: string, payload: unknown, timestamp: string) =>
  JSON.stringify({ type, payload, timestamp }) + "\n";
const start = "2026-11-20T01:00:00Z";
async function launch() {
  process = spawn(exe, [], {
    windowsHide: true,
    env: {
      ...globalThis.process.env,
      CODEX_HOME: logs,
      APPDATA: appdata,
      CODECAL_DATA_DIR: appdata,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9223",
    },
  });
  process.on("error", () => {});
  await expect
    .poll(
      async () => {
        try {
          const r = await fetch("http://127.0.0.1:9223/json/version");
          return r.ok;
        } catch {
          return false;
        }
      },
      { timeout: 20000 },
    )
    .toBe(true);
  browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  await expect
    .poll(() => browser.contexts().flatMap((c) => c.pages()).length)
    .toBeGreaterThan(0);
  page = browser.contexts().flatMap((c) => c.pages())[0];
  await page.waitForLoadState("domcontentloaded");
}
async function stop() {
  await browser?.close();
  if (process?.pid) {
    try {
      execFileSync("taskkill", ["/pid", String(process.pid), "/t", "/f"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } catch {
      /* Window may already have closed. */
    }
  }
}
test("native Tauri: import, calendars, detail, watch append and restart SQLite", async () => {
  expect(
    existsSync(exe),
    "Build the native app first: npm run tauri build",
  ).toBe(true);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  const logfile = path.join(logs, "sessions", "synthetic.jsonl");
  writeFileSync(
    logfile,
    event(
      "session_meta",
      { id: "sample-session", cwd: "C:/sample/CodeCal" },
      start,
    ) +
      event("event_msg", { type: "task_started", turn_id: "turn-1" }, start) +
      event(
        "event_msg",
        { type: "user_message", message: "カレンダー表示を実装する" },
        start,
      ) +
      event(
        "token_usage_record",
        { turn_id: "turn-1", turn_token_usage: { total_tokens: 1200 } },
        "2026-11-20T02:59:00Z",
      ) +
      event(
        "event_msg",
        { type: "task_complete", turn_id: "turn-1" },
        "2026-11-20T03:00:00Z",
      ),
  );
  try {
    await launch();
    await expect(page.getByLabel("codecal を表示")).toBeVisible();
    await page.getByLabel("表示日").fill("2026-11-20");
    await expect(page.getByTestId("total-elapsed")).toHaveText("2h 0m");
    await page.getByRole("button", { name: /codecal.*10:00/ }).click();
    await expect(page.getByRole("dialog")).toContainText(
      "カレンダー表示を実装する",
    );
    await expect(page.getByRole("dialog")).toContainText("1,200");
    await page.getByRole("button", { name: "ログの該当箇所を表示" }).click();
    await expect(page.locator(".log-excerpt")).toContainText("sample-session");
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    await page.locator(".week-scroll").evaluate((el) => {
      el.scrollTop = 480;
    });
    mkdirSync("docs/screenshots", { recursive: true });
    await page.screenshot({ path: "docs/screenshots/week.png" });
    await page.getByRole("button", { name: "月", exact: true }).click();
    await page
      .getByRole("button", { name: "2026-11-20 2h 0m", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toContainText("2h 0m");
    await page.getByRole("button", { name: "週表示で開く" }).click();
    appendFileSync(
      logfile,
      event(
        "event_msg",
        { type: "task_started", turn_id: "turn-2" },
        "2026-11-20T04:00:00Z",
      ) +
        event(
          "event_msg",
          { type: "user_message", message: "増分同期の確認" },
          "2026-11-20T04:00:00Z",
        ) +
        event(
          "event_msg",
          { type: "task_complete", turn_id: "turn-2" },
          "2026-11-20T05:00:00Z",
        ),
    );
    await expect(page.getByTestId("total-elapsed")).toHaveText("3h 0m", {
      timeout: 15000,
    });
    await page.getByRole("button", { name: "月", exact: true }).click();
    await page.screenshot({ path: "docs/screenshots/month.png" });
    await stop();
    await launch();
    await page.getByLabel("表示日").fill("2026-11-20");
    await expect(page.getByTestId("total-elapsed")).toHaveText("3h 0m");
    // A new parallel session must appear without a manual refresh.
    writeFileSync(
      path.join(logs, "sessions", "parallel.jsonl"),
      event(
        "session_meta",
        { id: "parallel", cwd: "C:/sample/Editor" },
        "2026-11-20T02:00:00Z",
      ) +
        event(
          "event_msg",
          { type: "task_started", turn_id: "parallel-turn" },
          "2026-11-20T02:00:00Z",
        ) +
        event(
          "event_msg",
          { type: "user_message", message: "検索パネルのテストを追加する" },
          "2026-11-20T02:00:00Z",
        ) +
        event(
          "event_msg",
          { type: "task_complete", turn_id: "parallel-turn" },
          "2026-11-20T04:00:00Z",
        ),
    );
    await expect(page.getByLabel("editor を表示")).toBeVisible();
    await expect(page.getByTestId("total-elapsed")).toHaveText("4h 0m");
    await expect(page.locator(".summary-cards")).toContainText("5h 0m");
    await page.locator(".week-scroll").evaluate((el) => {
      el.scrollTop = 480;
    });
    await page.screenshot({ path: "docs/screenshots/week.png" });
    await page.getByRole("button", { name: "月", exact: true }).click();
    await page.screenshot({ path: "docs/screenshots/month.png" });
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page
      .getByRole("combobox", { name: "テーマ", exact: true })
      .selectOption("dark");
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  } finally {
    await stop();
  }
});
