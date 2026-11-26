import {
  test,
  expect,
  chromium,
  type Browser,
  type Page,
} from "@playwright/test";
import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import {
  appendFileSync,
  mkdirSync,
  writeFileSync,
  existsSync,
  readFileSync,
} from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import type { HumanActivityResponse, Snapshot } from "../../src/types";

test("human settings: isolated IPC, WAL updates, OFF restart and confirmed cache clear", async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  const humanRoot = path.join(root, "human-only");
  mkdirSync(humanRoot, { recursive: true });
  const db = new DatabaseSync(path.join(humanRoot, "storage.db"));
  const marker = "PRIVATE_NATIVE_HUMAN_CONTENT";
  const captured = Date.parse("2026-11-20T01:00:00Z");
  db.exec(
    "PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT)",
  );
  db.prepare("INSERT INTO activity_records VALUES(?,?,?)").run(
    "human-synthetic-id",
    captured,
    JSON.stringify({
      durationEstimate: 120,
      foreground: { title: marker },
      detection: { diagnostics: { entertainmentDisposition: "ENTERTAINMENT" } },
    }),
  );
  const ipc = <T>(command: string, args: Record<string, unknown> = {}) =>
    page.evaluate(
      ({ command, args }) => {
        const host = window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args: Record<string, unknown>,
            ) => Promise<unknown>;
          };
        };
        return host.__TAURI_INTERNALS__.invoke(command, args);
      },
      { command, args },
    ) as Promise<T>;
  const period = () =>
    ipc<HumanActivityResponse>("human_activity", {
      from: captured,
      to: captured + 86400000,
    });
  try {
    await launch();
    await expect(page.getByRole("main")).toHaveAttribute("aria-busy", "false");
    await expect(period()).rejects.toThrow(/human_activity_disabled/);
    const before = await ipc<Snapshot>("snapshot", { timezone: "UTC" });
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("れいの AI 実行履歴を取り込む").uncheck();
    await page.getByLabel("Claude Code の履歴を取り込む").uncheck();
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByLabel("れい データディレクトリ").fill(humanRoot);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect.poll(async () => (await period()).records.length).toBe(1);
    const response = await period();
    expect(response.records[0]).toEqual({
      start: captured,
      end: captured + 120000,
      kind: "entertainment",
    });
    expect(Object.keys(response.records[0]).sort()).toEqual([
      "end",
      "kind",
      "start",
    ]);
    expect(JSON.stringify(response)).not.toContain(marker);
    expect(JSON.stringify(response)).not.toContain("human-synthetic-id");
    expect(
      (await ipc<Snapshot>("snapshot", { timezone: "UTC" })).turns,
    ).toHaveLength(before.turns.length);
    await expect(page.locator("body")).not.toContainText(marker);
    await expect(
      ipc("human_activity", { from: captured, to: captured + 63 * 86400000 }),
    ).rejects.toThrow(/invalid_period/);
    db.exec(
      "UPDATE activity_records SET payload=json_set(payload,'$.durationEstimate',180)",
    );
    await expect
      .poll(async () => (await period()).records[0]?.end, { timeout: 40000 })
      .toBe(captured + 180000);
    const cachePath = path.join(appdata, "codecal.db");
    for (const file of [cachePath, `${cachePath}-wal`, `${cachePath}-shm`]) {
      if (existsSync(file))
        expect(readFileSync(file).includes(Buffer.from(marker))).toBe(false);
    }
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("人間のアクティビティ機能を有効にする").uncheck();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(
      page.getByRole("region", { name: "人間のアクティビティ同期" }),
    ).toHaveCount(0);
    await expect(period()).rejects.toThrow(/human_activity_disabled/);
    await stop();
    await launch();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await expect(
      page.getByLabel("人間のアクティビティ機能を有効にする"),
    ).not.toBeChecked();
    await page
      .getByRole("button", { name: "人間のアクティビティ履歴を削除する" })
      .click();
    const confirmation = page.getByRole("dialog", {
      name: "人間のアクティビティ履歴の削除",
    });
    await confirmation.getByLabel("すべてのソース").check();
    await confirmation
      .getByRole("button", { name: "履歴を削除して機能をOFFにする" })
      .click();
    await expect(confirmation).not.toBeVisible();
    const cache = new DatabaseSync(cachePath);
    try {
      expect(
        cache.prepare("SELECT count(*) AS n FROM human_activity_records").get()
          ?.n,
      ).toBe(0);
    } finally {
      cache.close();
    }
    expect(
      db.prepare("SELECT count(*) AS n FROM activity_records").get()?.n,
    ).toBe(1);
    expect(
      db.prepare("SELECT payload FROM activity_records").get()?.payload,
    ).toContain(marker);
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect.poll(async () => (await period()).records.length).toBe(1);
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("人間のアクティビティ機能を有効にする").uncheck();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
  } finally {
    await stop();
    db.close();
  }
});
declare global {
  interface Window {
    __observedBusy: boolean;
  }
}
const exe = path.resolve(
  globalThis.process.env.CODECAL_E2E_EXE ??
    "src-tauri/target/release/codecal.exe",
);
const root = path.resolve(".local", `native-${Date.now()}`);
const logs = path.join(root, "logs");
const appdata = path.join(root, "appdata");
let process: ChildProcess;
let browser: Browser;
let page: Page;
const event = (type: string, payload: unknown, timestamp: string) =>
  JSON.stringify({ type, payload, timestamp }) + "\n";
const start = "2026-11-20T01:00:00Z";
async function launch(waitForReady = true) {
  const startupBegan = Date.now();
  process = spawn(exe, [], {
    windowsHide: true,
    env: {
      ...globalThis.process.env,
      CODEX_HOME: logs,
      CLAUDE_CONFIG_DIR: path.join(root, "unused-claude"),
      REI_DATA_DIR: path.join(root, "rei"),
      APPDATA: appdata,
      CODECAL_DATA_DIR: appdata,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9223",
    },
  });
  let launchError = "";
  let stderr = "";
  process.on("error", (error) => {
    launchError = error.message;
  });
  process.stderr?.on("data", (chunk) => {
    stderr = (stderr + String(chunk)).slice(-8192);
  });
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
      // Allow a bounded budget for cold WebView2 startup on Windows CI.
      // Keep a bounded startup budget; application assertions still run fully.
      { timeout: 60000 },
    )
    .toBe(true)
    .catch((cause) => {
      throw new Error(
        `WebView2 debugging did not start: ${exe}; exit=${process.exitCode}; spawn=${launchError}; stderr=${stderr}`,
        { cause },
      );
    });
  console.info(
    `Native WebView2 endpoint ready after ${Date.now() - startupBegan}ms`,
  );
  browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  await expect
    .poll(() => browser.contexts().flatMap((c) => c.pages()).length)
    .toBeGreaterThan(0);
  page = browser.contexts().flatMap((c) => c.pages())[0];
  await page.waitForLoadState("domcontentloaded");
  if (!waitForReady) return;
  // Fixture times and labels use Japan time; CI runners usually use UTC.
  await page.getByRole("button", { name: "設定", exact: true }).click();
  await page.getByLabel("タイムゾーン", { exact: true }).fill("Asia/Tokyo");
  await page.getByRole("button", { name: "設定を保存", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "設定", exact: true }),
  ).not.toBeVisible();
}
async function stop() {
  if (process?.pid && process.exitCode === null) {
    try {
      // Let WebView2 release the isolated profile before immediately reopening it.
      execFileSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-Command",
          `$app=Get-Process -Id ${process.pid} -ErrorAction SilentlyContinue; if($app){if(!$app.CloseMainWindow()){throw 'Window close failed'}; if(!$app.WaitForExit(10000)){throw 'Application did not exit normally'}}`,
        ],
        { windowsHide: true, stdio: "pipe" },
      );
    } catch {
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
  await browser?.close().catch(() => {});
}
test("native Tauri: import, calendars, detail, watch append and restart SQLite", async () => {
  // Includes a 35-second watcher reconciliation allowance and native restarts.
  test.setTimeout(120000);
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
    await expect(page.getByRole("main")).toHaveAttribute("aria-busy", "false");
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
    // Immediately after restart, creation can precede watcher attachment.
    // The documented 30-second reconciliation scan must also be allowed.
    await expect(page.getByLabel("editor を表示")).toBeVisible({
      timeout: 35000,
    });
    await expect(page.getByTestId("total-elapsed")).toHaveText("4h 0m");
    await expect(page.locator(".summary-cards")).toContainText("5h 0m");
    await page.locator(".week-scroll").evaluate((el) => {
      el.scrollTop = 480;
    });
    await page.screenshot({ path: "docs/screenshots/week.png" });
    await page.getByRole("button", { name: "月", exact: true }).click();
    await expect(page.getByRole("main")).toHaveAttribute("aria-busy", "false");
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

test("month places seven columns left to right with the following week below", async () => {
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  try {
    await launch();
    await page.getByRole("button", { name: "月", exact: true }).click();
    await page.locator(".heatmap button").first().waitFor();
    const cells = await page.locator(".heatmap button").evaluateAll((els) =>
      els.slice(0, 8).map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y };
      }),
    );
    expect(cells[1].y).toBe(cells[0].y);
    expect(cells[1].x).toBeGreaterThan(cells[0].x);
    expect(cells[7].x).toBe(cells[0].x);
    expect(cells[7].y).toBeGreaterThan(cells[0].y);
  } finally {
    await stop();
  }
});

for (const view of ["日", "週"]) {
  test(`${view} view zooms around the pointer and navigates its date range`, async () => {
    mkdirSync(path.join(logs, "sessions"), { recursive: true });
    mkdirSync(appdata, { recursive: true });
    try {
      await launch();
      await page.getByLabel("表示日").fill("2026-11-20");
      await expect(page.getByRole("main")).toHaveAttribute(
        "aria-busy",
        "false",
      );
      await page.getByRole("button", { name: "月", exact: true }).click();
      await expect(page.getByRole("main")).toHaveAttribute(
        "aria-busy",
        "false",
      );
      await page.evaluate(() => {
        window.__observedBusy = false;
        const main = document.querySelector("main")!;
        new MutationObserver((records) => {
          if (
            records.some((r) => r.oldValue === "true") ||
            main.getAttribute("aria-busy") === "true"
          )
            window.__observedBusy = true;
        }).observe(main, {
          attributes: true,
          attributeOldValue: true,
          attributeFilter: ["aria-busy"],
        });
      });
      await page.getByRole("button", { name: view, exact: true }).click();
      await expect(page.locator(".day-column")).toHaveCount(
        view === "日" ? 1 : 7,
      );
      await expect(page.getByLabel("表示日")).toHaveValue("2026-11-20");
      expect(await page.evaluate(() => window.__observedBusy)).toBe(true);
      await expect(page.getByRole("main")).toHaveAttribute(
        "aria-busy",
        "false",
      );
      await page.locator(".week-scroll").evaluate((el) => {
        el.scrollTop = 480;
      });
      await page.screenshot({
        path:
          view === "日" ? "docs/screenshots/day.png" : "test-results/week.png",
      });
      const scroller = page.locator(".week-scroll");
      const box = (await scroller.boundingBox())!;
      const pointerY = 120;
      const beforeZoom = await page.evaluate(() => ({
        scrollTop: document.querySelector(".week-scroll")!.scrollTop,
        width: window.innerWidth,
        dpr: window.devicePixelRatio,
      }));
      const originalBlockHeight = await page
        .locator(".activity-block")
        .first()
        .evaluate((el) => el.getBoundingClientRect().height);
      await page.mouse.move(box.x + box.width * 0.75, box.y + pointerY);
      await page.keyboard.down("Control");
      try {
        await page.mouse.wheel(0, -120);
      } finally {
        await page.keyboard.up("Control");
      }
      await expect(
        page.getByLabel("タイムラインの倍率", { exact: true }),
      ).not.toHaveText("100%");
      const afterZoom = await page.evaluate(() => ({
        scrollTop: document.querySelector(".week-scroll")!.scrollTop,
        height: document.querySelector(".week-grid")!.getBoundingClientRect()
          .height,
        width: window.innerWidth,
        dpr: window.devicePixelRatio,
      }));
      const scale = (afterZoom.height - 1) / 1440;
      expect(scale).toBeGreaterThan(1);
      expect(
        Math.abs(
          (afterZoom.scrollTop + pointerY) / scale -
            (beforeZoom.scrollTop + pointerY),
        ),
      ).toBeLessThan(1);
      expect(afterZoom.width).toBe(beforeZoom.width);
      expect(afterZoom.dpr).toBe(beforeZoom.dpr);
      expect(
        await page
          .locator(".activity-block")
          .first()
          .evaluate((el) => el.getBoundingClientRect().height),
      ).toBeGreaterThan(originalBlockHeight);
      await page.screenshot({
        path:
          view === "日"
            ? "docs/screenshots/day-zoom.png"
            : "test-results/week-zoom.png",
      });
      const zoomLabel = await page
        .getByLabel("タイムラインの倍率", { exact: true })
        .textContent();
      await page.mouse.wheel(0, 120);
      await expect
        .poll(() => scroller.evaluate((el) => el.scrollTop))
        .toBeGreaterThan(afterZoom.scrollTop);
      await expect(
        page.getByLabel("タイムラインの倍率", { exact: true }),
      ).toHaveText(zoomLabel!);
      await page
        .getByRole("button", {
          name: "タイムラインの倍率をリセット",
          exact: true,
        })
        .click();
      await expect(
        page.getByLabel("タイムラインの倍率", { exact: true }),
      ).toHaveText("100%");
      await page.getByRole("button", { name: "次へ", exact: true }).click();
      await expect(page.getByLabel("表示日")).toHaveValue(
        view === "日" ? "2026-11-21" : "2026-11-27",
      );
      await page.getByRole("button", { name: "前へ", exact: true }).click();
      await expect(page.getByLabel("表示日")).toHaveValue("2026-11-20");
    } finally {
      await stop();
    }
  });
}
test("grouped activity keeps gaps out of totals and opens every original turn", async () => {
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  let fixture = event(
    "session_meta",
    { id: "grouped-session", cwd: "C:/sample/Grouped" },
    start,
  );
  for (const [id, from, to, status, prompt] of [
    [
      "batch-a",
      "2026-11-20T01:00:00Z",
      "2026-11-20T01:30:00Z",
      "task_complete",
      "作業A",
    ],
    [
      "batch-b",
      "2026-11-20T01:15:00Z",
      "2026-11-20T01:45:00Z",
      "task_failed",
      "作業B",
    ],
    [
      "batch-c",
      "2026-11-20T01:50:00Z",
      "2026-11-20T02:00:00Z",
      "turn_aborted",
      "作業C",
    ],
  ])
    fixture +=
      event("event_msg", { type: "task_started", turn_id: id }, from) +
      event("event_msg", { type: "user_message", message: prompt }, from) +
      event("event_msg", { type: status, turn_id: id }, to);
  writeFileSync(path.join(logs, "sessions", "grouped.jsonl"), fixture);
  try {
    await launch();
    await expect(page.getByLabel("grouped を表示")).toBeVisible();
    for (const label of ["codecal を表示", "editor を表示"]) {
      const checkbox = page.getByLabel(label);
      if (await checkbox.count()) await checkbox.uncheck();
    }
    await page.getByLabel("表示日").fill("2026-11-20");
    await page.getByRole("button", { name: "日", exact: true }).click();
    await expect(page.locator(".day-column")).toHaveCount(1);
    await expect(page.locator(".activity-block")).toHaveCount(1);
    await expect(
      page.getByRole("combobox", {
        name: "作業ブロックの表示単位",
        exact: true,
      }),
    ).toHaveValue("group");
    await expect(page.getByTestId("total-elapsed")).toHaveText("0h 55m");
    await expect(page.locator(".summary-cards")).toContainText("1h 10m");
    await page.locator(".week-scroll").evaluate((el) => {
      el.scrollTop = 480;
    });
    await page.screenshot({ path: "docs/screenshots/day-grouped.png" });
    await page.getByRole("button", { name: /grouped.*3ターン/ }).click();
    await expect(page.getByRole("dialog")).toContainText("0h 55m");
    await expect(page.getByRole("dialog")).toContainText("1h 10m");
    await expect(page.getByRole("dialog")).toContainText("完了 1");
    await expect(page.getByRole("dialog")).toContainText("失敗 1");
    await expect(page.getByRole("dialog")).toContainText("中断 1");
    await page.screenshot({
      path: "docs/screenshots/activity-group-details.png",
    });
    await page.getByRole("button", { name: /作業B/ }).click();
    await expect(page.getByRole("dialog")).toContainText("batch-b");
    await expect(page.getByRole("dialog")).toContainText("失敗");
    await page
      .getByRole("button", { name: "作業のまとまりに戻る", exact: true })
      .click();
    await expect(page.getByRole("dialog")).toContainText("作業C");
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    await page
      .getByRole("combobox", { name: "作業ブロックの表示単位", exact: true })
      .selectOption("turn");
    await expect(page.locator(".activity-block")).toHaveCount(3);
    await expect(page.getByTestId("total-elapsed")).toHaveText("0h 55m");
    await page.getByRole("button", { name: "月", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "2026-11-20 0h 55m", exact: true }),
    ).toBeVisible();
  } finally {
    await stop();
  }
});

test("Rei SQLite: settings, WAL import, source detail, automatic append and restart", async () => {
  const reiRoot = path.join(root, "rei");
  mkdirSync(reiRoot, { recursive: true });
  writeFileSync(
    path.join(reiRoot, "projects.json"),
    JSON.stringify({
      projects: [
        { id: "rei-sample", name: "れいサンプル", path: "C:/sample/Rei" },
      ],
    }),
  );
  const db = new DatabaseSync(path.join(reiRoot, "storage.db"));
  db.exec(
    "PRAGMA journal_mode=WAL; CREATE TABLE agent_events(project_id TEXT,sequence INTEGER,id TEXT,event_type TEXT,record TEXT,PRIMARY KEY(project_id,sequence)); CREATE TABLE turns(project_id TEXT,run_id TEXT,record TEXT)",
  );
  const add = (
    sequence: number,
    id: string,
    runId: string,
    type: string,
    timestamp: string,
  ) =>
    db.prepare("INSERT INTO agent_events VALUES(?,?,?,?,?)").run(
      "rei-sample",
      sequence,
      id,
      type,
      JSON.stringify({
        id,
        projectId: "rei-sample",
        sessionId: "rei-session",
        turnId: runId,
        runId,
        type,
        timestamp,
        version: 1,
        payload: {},
      }),
    );
  add(1, "rei-start", "rei-run", "AGENT_RUN_STARTED", "2026-11-20T06:00:00Z");
  add(2, "rei-end", "rei-run", "AGENT_RUN_COMPLETED", "2026-11-20T06:30:00Z");
  db.prepare("INSERT INTO turns VALUES(?,?,?)").run(
    "rei-sample",
    "rei-run",
    JSON.stringify({ request: "れいの履歴を確認する" }),
  );
  try {
    await launch();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("れいの AI 実行履歴を取り込む").check();
    await page.getByLabel("れい データディレクトリ").fill(reiRoot);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(page.getByLabel("れいサンプル を表示")).toBeVisible();
    for (const label of ["codecal を表示", "editor を表示", "grouped を表示"]) {
      const checkbox = page.getByLabel(label);
      if (await checkbox.count()) await checkbox.uncheck();
    }
    await page.getByLabel("表示日").fill("2026-11-20");
    await expect(page.getByTestId("total-elapsed")).toHaveText("0h 30m");
    await page.getByRole("button", { name: /れいサンプル.*15:00/ }).click();
    await expect(page.getByRole("dialog")).toContainText("れい への依頼");
    await expect(page.getByRole("dialog")).toContainText(
      "れいの履歴を確認する",
    );
    await page.getByRole("button", { name: "ログの該当箇所を表示" }).click();
    await expect(page.locator(".log-excerpt")).toContainText("rei-start");
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    add(
      3,
      "rei-start-2",
      "rei-run-2",
      "AGENT_RUN_STARTED",
      "2026-11-20T06:40:00Z",
    );
    add(
      4,
      "rei-end-2",
      "rei-run-2",
      "AGENT_RUN_CANCELLED",
      "2026-11-20T07:10:00Z",
    );
    await expect(page.getByTestId("total-elapsed")).toHaveText("1h 0m", {
      timeout: 15000,
    });
    await stop();
    await launch();
    await expect(page.getByLabel("れいサンプル を表示")).toBeVisible();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await expect(page.getByLabel("れいの AI 実行履歴を取り込む")).toBeChecked();
    await expect(page.getByLabel("れい データディレクトリ")).toHaveValue(
      reiRoot,
    );
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
  } finally {
    await stop();
    db.close();
  }
});
test("Claude synthetic transcripts: settings, week/month, append and restart", async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  const projects = path.join(root, "unused-claude", "projects");
  const project = path.join(projects, "C--sample-Claude");
  mkdirSync(project, { recursive: true });
  const sid = "550e8400-e29b-41d4-a716-446655440000";
  const file = path.join(project, `${sid}.jsonl`);
  const message = (uuid: string, timestamp: string, type: string) =>
    JSON.stringify({
      uuid,
      timestamp,
      type,
      sessionId: sid,
      cwd: "C:/sample/Claude",
      gitBranch: "synthetic",
      message: { content: "SECRET-not-persisted" },
    }) + "\n";
  writeFileSync(
    file,
    message("a", "2026-11-20T14:00:00Z", "user") +
      message("b", "2026-11-20T14:02:00Z", "assistant"),
  );
  try {
    await launch();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page
      .getByLabel("Claude Code の履歴を取り込む", { exact: true })
      .check();
    await page
      .getByLabel("Claude Code projects ディレクトリ", { exact: true })
      .fill(projects);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "設定", exact: true }),
    ).not.toBeVisible();
    await expect(page.getByLabel("claude を表示")).toBeVisible();
    for (const checkbox of await page
      .getByRole("checkbox", { name: / を表示$/ })
      .all()) {
      if ((await checkbox.getAttribute("aria-label")) !== "claude を表示")
        await checkbox.uncheck();
    }
    await page.getByLabel("表示日").fill("2026-11-20");
    await expect(page.getByTestId("total-elapsed")).toHaveText("0h 2m");
    const block = page
      .locator(".activity-block")
      .filter({ hasText: "Claude Code" });
    await expect(block).toHaveCount(1);
    await block.click();
    await expect(page.getByRole("dialog")).toContainText("synthetic");
    await expect(page.getByRole("dialog")).not.toContainText(
      "SECRET-not-persisted",
    );
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    await page.getByRole("button", { name: "月", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "2026-11-20 0h 2m", exact: true }),
    ).toBeVisible();
    appendFileSync(
      file,
      message("c", "2026-11-20T14:04:00Z", "user") +
        message("d", "2026-11-20T14:05:00Z", "assistant"),
    );
    await expect(page.getByTestId("total-elapsed")).toHaveText("0h 3m", {
      timeout: 35000,
    });
    await stop();
    await launch();
    await expect(page.getByLabel("claude を表示")).toBeVisible();
    const db = new DatabaseSync(path.join(appdata, "codecal.db"), {
      readOnly: true,
    });
    try {
      const records = db
        .prepare("SELECT event FROM events WHERE session_id LIKE 'claude:%'")
        .all();
      expect(records.length).toBe(4);
      expect(JSON.stringify(records)).not.toContain("SECRET-not-persisted");
    } finally {
      db.close();
    }
  } finally {
    await stop();
  }
});
test("database startup migrates v2 with backup and reopens compatible legacy v3", async () => {
  test.setTimeout(90000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  try {
    await launch();
    await stop();
    const dbPath = path.join(appdata, "codecal.db");
    const db = new DatabaseSync(dbPath);
    db.exec(
      "INSERT OR REPLACE INTO settings VALUES('migration-test','preserved'); DELETE FROM schema_migrations WHERE version>2; PRAGMA user_version=2;",
    );
    db.close();
    await launch();
    await stop();
    const current = new DatabaseSync(dbPath);
    expect(current.prepare("PRAGMA user_version").get()).toEqual({
      user_version: 4,
    });
    expect(
      current
        .prepare("SELECT value FROM settings WHERE key='migration-test'")
        .get(),
    ).toEqual({ value: "preserved" });
    const backup = new DatabaseSync(`${dbPath}.before-v4.bak`, {
      readOnly: true,
    });
    try {
      expect(backup.prepare("PRAGMA user_version").get()).toEqual({
        user_version: 2,
      });
      expect(
        backup.prepare("SELECT count(*) AS n FROM human_activity_records").get()
          ?.n,
      ).toBe(0);
      expect(
        backup.prepare("SELECT count(*) AS n FROM human_activity_sources").get()
          ?.n,
      ).toBe(0);
      expect(
        backup
          .prepare("SELECT value FROM settings WHERE key='migration-test'")
          .get(),
      ).toEqual({ value: "preserved" });
    } finally {
      backup.close();
    }
    current.exec(
      "CREATE TABLE IF NOT EXISTS claude_files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL); INSERT OR REPLACE INTO claude_files VALUES('/synthetic/legacy.jsonl','preserved'); PRAGMA user_version=3;",
    );
    current.close();
    await launch();
    await expect(
      page.getByRole("button", { name: "設定", exact: true }),
    ).toBeVisible();
    await stop();
    const reopened = new DatabaseSync(dbPath, { readOnly: true });
    try {
      expect(
        reopened
          .prepare(
            "SELECT fingerprint FROM claude_files WHERE path='/synthetic/legacy.jsonl'",
          )
          .get(),
      ).toEqual({ fingerprint: "preserved" });
    } finally {
      reopened.close();
    }
  } finally {
    await stop();
  }
});
test("migration startup shows an animated status while SQLite is busy", async () => {
  test.setTimeout(90000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  await launch();
  await stop();
  const db = new DatabaseSync(path.join(appdata, "codecal.db"));
  db.exec(
    "DELETE FROM schema_migrations WHERE version>2; PRAGMA user_version=2; BEGIN IMMEDIATE;",
  );
  let locked = true;
  try {
    await launch(false);
    const status = page.getByRole("status");
    await expect(status).toHaveText("CodeCal がデータベースを移行しています…");
    await expect(status.locator(".spinner")).toBeVisible();
    const frames = await page.evaluate(
      () =>
        new Promise<number>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve(2))),
        ),
    );
    expect(frames).toBe(2);
    await page.screenshot({ path: "test-results/migration-progress.png" });
    db.exec("ROLLBACK;");
    locked = false;
    await expect(
      page.getByRole("button", { name: "設定", exact: true }),
    ).toBeEnabled();
    await expect(
      page.getByText("CodeCal がデータベースを移行しています…"),
    ).not.toBeVisible();
  } finally {
    if (locked) db.exec("ROLLBACK;");
    db.close();
    await stop();
  }
});

test("database startup failures stop the spinner and allow retry", async () => {
  test.setTimeout(90000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  await launch();
  await stop();
  const dbPath = path.join(appdata, "codecal.db");
  const failed = new DatabaseSync(dbPath);
  failed.exec("PRAGMA user_version=99;");
  failed.close();
  try {
    await launch(false);
    await expect(page.getByRole("alert")).toContainText(
      "CodeCal がデータベースを準備できませんでした",
    );
    await expect(page.locator(".startup-screen .spinner")).toHaveCount(0);
    const repaired = new DatabaseSync(dbPath);
    repaired.exec("PRAGMA user_version=3;");
    repaired.close();
    await page.getByRole("button", { name: "再試行", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "設定", exact: true }),
    ).toBeEnabled();
    await expect(page.locator(".startup-screen")).toHaveCount(0);
  } finally {
    await stop();
  }
});

test("dialogs close on backdrop clicks and preserve inside interactions", async () => {
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  try {
    await launch();
    for (const name of ["設定", "診断"]) {
      const trigger = page.getByRole("button", { name, exact: true });
      for (const side of ["left", "right", "top", "bottom"]) {
        await trigger.click();
        const dialog = page.getByRole("dialog");
        await expect(dialog).toBeVisible();
        const bounds = (await dialog.boundingBox())!;
        const inside = { x: bounds.x + 8, y: bounds.y + 8 };
        const outside = {
          x:
            side === "left"
              ? bounds.x - 8
              : side === "right"
                ? bounds.x + bounds.width + 8
                : bounds.x + bounds.width / 2,
          y:
            side === "top"
              ? bounds.y - 8
              : side === "bottom"
                ? bounds.y + bounds.height + 8
                : bounds.y + bounds.height / 2,
        };
        await dialog.getByRole("heading").first().click();
        await page.mouse.click(inside.x, inside.y);
        await expect(dialog).toBeVisible();
        await page.mouse.move(inside.x, inside.y);
        await page.mouse.down();
        await page.mouse.move(outside.x, outside.y);
        await page.mouse.up();
        await expect(dialog).toBeVisible();
        await page.mouse.click(outside.x, outside.y);
        await expect(dialog).not.toBeVisible();
        await expect(trigger).toBeFocused();
      }
    }
  } finally {
    await stop();
  }
});

test("corrupt settings recover with a persistent backup and dialogs restore focus", async () => {
  test.setTimeout(90000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  await launch();
  await stop();
  const dbPath = path.join(appdata, "codecal.db");
  const db = new DatabaseSync(dbPath);
  const original = "{synthetic-invalid-config";
  db.prepare("UPDATE settings SET value=? WHERE key='config'").run(original);
  db.close();
  try {
    await launch(false);
    await expect(
      page.getByText(/保存済みの設定を読み込めなかったため/),
    ).toBeVisible();
    const trigger = page.getByRole("button", {
      name: "設定を確認",
      exact: true,
    });
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: "設定", exact: true });
    await expect(dialog).toBeVisible();
    const name = page.getByLabel("Codex ログディレクトリ", { exact: true });
    await expect(name).toHaveValue(logs);
    const updateButtons = dialog.getByRole("button", {
      name: "更新",
      exact: true,
    });
    const last =
      (await updateButtons.count()) > 0
        ? updateButtons.last()
        : dialog.getByRole("button", { name: "設定を保存", exact: true });
    await last.focus();
    // Modal background controls are inert, including for programmatic focus.
    await page.evaluate(() =>
      document.querySelector<HTMLButtonElement>(".settings-button")?.focus(),
    );
    await expect(last).toBeFocused();
    await page.keyboard.press("Tab");
    // WebView2 can transfer focus to its native host for one Tab stop. It must
    // never move to a background control, and the next Tab returns to the dialog.
    expect(
      await page.evaluate(
        () =>
          Boolean(document.activeElement?.closest("dialog")) ||
          (document.activeElement === document.body && !document.hasFocus()),
      ),
    ).toBe(true);
    await page.keyboard.press("Tab");
    await expect
      .poll(() =>
        page.evaluate(() => Boolean(document.activeElement?.closest("dialog"))),
      )
      .toBe(true);
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    await expect(trigger).toBeFocused();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("button", { name: "設定", exact: true }),
    ).toBeFocused();
    await stop();
    const saved = new DatabaseSync(dbPath, { readOnly: true });
    try {
      expect(
        saved
          .prepare(
            "SELECT value FROM settings WHERE key GLOB '_config_backup_*'",
          )
          .all(),
      ).toEqual([{ value: original }]);
      const row = saved
        .prepare("SELECT value FROM settings WHERE key='config'")
        .get()!;
      expect(JSON.parse(String(row.value)).log_root).toBe(logs);
    } finally {
      saved.close();
    }
    await launch(false);
    await expect(
      page.getByRole("button", { name: "設定", exact: true }),
    ).toBeEnabled();
    await expect(
      page.getByText(/保存済みの設定を読み込めなかったため/),
    ).toHaveCount(0);
  } finally {
    await stop();
  }
});
