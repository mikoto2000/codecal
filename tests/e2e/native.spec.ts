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
import { DatabaseSync } from "node:sqlite";
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
async function launch() {
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
      { timeout: 20000 },
    )
    .toBe(true)
    .catch((cause) => {
      throw new Error(
        `WebView2 debugging did not start: ${exe}; exit=${process.exitCode}; spawn=${launchError}; stderr=${stderr}`,
        { cause },
      );
    });
  browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  await expect
    .poll(() => browser.contexts().flatMap((c) => c.pages()).length)
    .toBeGreaterThan(0);
  page = browser.contexts().flatMap((c) => c.pages())[0];
  await page.waitForLoadState("domcontentloaded");
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

test("day view shows a spinner, zooms around the pointer and navigates one day", async () => {
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  mkdirSync(appdata, { recursive: true });
  try {
    await launch();
    await page.getByLabel("表示日").fill("2026-11-20");
    await expect(page.getByRole("main")).toHaveAttribute("aria-busy", "false");
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
    await page.getByRole("button", { name: "日", exact: true }).click();
    await expect(page.locator(".day-column")).toHaveCount(1);
    await expect(page.getByLabel("表示日")).toHaveValue("2026-11-20");
    expect(await page.evaluate(() => window.__observedBusy)).toBe(true);
    await expect(page.getByRole("main")).toHaveAttribute("aria-busy", "false");
    await page.locator(".week-scroll").evaluate((el) => {
      el.scrollTop = 480;
    });
    await page.screenshot({ path: "docs/screenshots/day.png" });
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
    await page.screenshot({ path: "docs/screenshots/day-zoom.png" });
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
    await expect(page.getByLabel("表示日")).toHaveValue("2026-11-21");
    await page.getByRole("button", { name: "前へ", exact: true }).click();
    await expect(page.getByLabel("表示日")).toHaveValue("2026-11-20");
  } finally {
    await stop();
  }
});
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
    await page.getByLabel("れいの実行履歴を取り込む").check();
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
    await expect(page.getByLabel("れいの実行履歴を取り込む")).toBeChecked();
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
