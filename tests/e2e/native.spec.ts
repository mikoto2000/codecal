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
import type {
  HumanActivityResponse,
  HumanSummaryResponse,
  Snapshot,
} from "../../src/types";

test("human settings: legacy memory database, isolated IPC, WAL updates, OFF restart and confirmed cache clear", async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  const humanRoot = path.join(root, "human-only");
  mkdirSync(humanRoot, { recursive: true });
  const aiDatabase = new DatabaseSync(path.join(humanRoot, "storage.db"));
  aiDatabase.exec("CREATE TABLE agent_events(sequence INTEGER)");
  aiDatabase.close();
  const db = new DatabaseSync(path.join(humanRoot, "memory.db"));
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
    await expect(
      page.getByRole("dialog", { name: "設定", exact: true }),
    ).not.toBeVisible();
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
    const humanSidebar = page.getByRole("region", {
      name: "人間のアクティビティ",
      exact: true,
    });
    const humanSync = humanSidebar.getByRole("region", {
      name: "人間のアクティビティ同期",
      exact: true,
    });
    await expect(humanSync).toContainText("1時間ごと");
    expect((await period()).records[0].end).toBe(captured + 120000);
    await humanSync
      .getByRole("button", {
        name: "人間のアクティビティを再同期",
        exact: true,
      })
      .click();
    await expect
      .poll(async () => (await period()).records[0]?.end, { timeout: 40000 })
      .toBe(captured + 180000);
    const cachePath = path.join(appdata, "codecal.db");
    const insert = db.prepare("INSERT INTO activity_records VALUES(?,?,?)");
    for (const [id, offset, duration, kind] of [
      ["human-touch", 180000, 60, "ENTERTAINMENT"],
      ["human-gap", 241000, 60, "NON_ENTERTAINMENT"],
      ["human-point", 400000, 0, "UNCERTAIN"],
    ] as const) {
      insert.run(
        id,
        captured + offset,
        JSON.stringify({
          durationEstimate: duration,
          foreground: { title: marker },
          detection: { diagnostics: { entertainmentDisposition: kind } },
        }),
      );
    }
    const fixture = new DatabaseSync(cachePath);
    const fixtureProjects = [
      "human-test-project",
      ...Array.from({ length: 100 }, (_, i) => `human-layout-${i}`),
    ];
    fixture.exec("BEGIN");
    const projectInsert = fixture.prepare(
      "INSERT INTO projects VALUES(?,?,?,?,?)",
    );
    for (const id of fixtureProjects)
      projectInsert.run(
        id,
        `C:/synthetic/${id}`,
        id === "human-test-project" ? "AI overlap" : id,
        "#14b8a6",
        captured,
      );
    fixture
      .prepare("INSERT INTO sessions VALUES(?,?,?,?)")
      .run(
        "human-test-session",
        "human-test-project",
        captured,
        captured + 600000,
      );
    fixture
      .prepare(
        "INSERT INTO turns(id,session_id,turn_id,project_id,start,end,status,prompt,source) VALUES(?,?,?,?,?,?,?,?,?)",
      )
      .run(
        "human-test-session:turn",
        "human-test-session",
        "turn",
        "human-test-project",
        captured,
        captured + 600000,
        "completed",
        "synthetic AI overlap",
        "codex",
      );
    fixture
      .prepare(
        "INSERT INTO turns(id,session_id,turn_id,project_id,start,end,status,prompt,source) VALUES(?,?,?,?,?,?,?,?,?)",
      )
      .run(
        "human-test-session:short",
        "human-test-session",
        "short",
        "human-test-project",
        captured + 241000,
        captured + 242000,
        "completed",
        "synthetic short AI overlap",
        "codex",
      );
    fixture.exec("COMMIT");
    try {
      await ipc("sync_logs");
      await page.getByLabel("表示日").fill("2026-11-20");
      await expect(page.locator(".human-block")).toHaveCount(3);
      await expect(page.locator(".human-time-lane")).toHaveCount(14);
      const ai = page.getByRole("button", { name: /^AI overlap 10:00/ });
      const entertainment = page.getByRole("button", { name: /^人間 娯楽 / });
      await expect(ai).toBeVisible();
      await page.locator(".week-scroll").evaluate((el) => {
        el.scrollTop = 480;
      });
      const [aiBox, humanBox] = await Promise.all([
        ai.boundingBox(),
        entertainment.boundingBox(),
      ]);
      expect(aiBox && humanBox).toBeTruthy();
      expect(
        aiBox!.x + aiBox!.width <= humanBox!.x ||
          humanBox!.x + humanBox!.width <= aiBox!.x,
      ).toBe(true);
      await expect(page.getByTestId("total-elapsed")).toHaveText("0h 10m");
      await ai.click();
      await expect(page.getByRole("dialog")).toContainText(
        "synthetic AI overlap",
      );
      await page.getByRole("button", { name: "閉じる", exact: true }).click();
      await entertainment.click();
      const detail = page.getByRole("dialog", {
        name: "人間のアクティビティ（推定）",
      });
      await expect(detail).toContainText("時間帯の終了");
      await expect(detail).not.toContainText(marker);
      await expect(detail).not.toContainText(humanRoot);
      await expect(
        detail.getByRole("button", { name: "ログの該当箇所を表示" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await page
        .getByRole("checkbox", { name: "人間の娯楽を表示", exact: true })
        .uncheck();
      await expect(entertainment).toHaveCount(0);
      await expect(ai).toBeVisible();
      await expect(
        page
          .getByRole("checkbox", { name: "人間の娯楽を表示", exact: true })
          .locator(".."),
      ).toContainText("0h 4m");
      await page
        .getByRole("button", { name: "人間の種別をすべてON", exact: true })
        .click();
      await page.getByLabel("ブロックの表示単位").selectOption("turn");
      await expect(
        page.getByRole("button", { name: /^人間 娯楽 / }),
      ).toHaveCount(1);
      await page.getByRole("button", { name: /^AI overlap 10:04/ }).click();
      await expect(page.getByRole("dialog")).toContainText(
        "synthetic short AI overlap",
      );
      await page.keyboard.press("Escape");
      await page.getByLabel("ブロックの表示単位").selectOption("group");
      await expect(entertainment).toHaveCount(1);
      await page
        .getByRole("button", { name: /^人間 非娯楽（不明含む） .*点の観測/ })
        .click();
      await expect(
        page.getByRole("dialog", { name: "人間のアクティビティ（推定）" }),
      ).toContainText("0h 0m 1.000s");
      await page.keyboard.press("Escape");
      await page
        .getByRole("button", { name: "プロジェクトをすべてOFF", exact: true })
        .click();
      await expect(ai).toHaveCount(0);
      await expect(entertainment).toBeVisible();
      await page
        .getByRole("button", { name: "プロジェクトをすべてON", exact: true })
        .click();
      const viewport = await page.evaluate(() => ({
        width: innerWidth,
        height: innerHeight,
      }));
      await page.setViewportSize({ width: 920, height: 640 });
      await page
        .getByRole("button", { name: "案内を閉じる", exact: true })
        .click();
      const list = await page
        .getByRole("region", { name: "人間のアクティビティ", exact: true })
        .boundingBox();
      const settings = await page
        .getByRole("button", { name: "設定", exact: true })
        .boundingBox();
      expect(list!.y + list!.height).toBeLessThanOrEqual(640);
      expect(settings!.y + settings!.height).toBeLessThanOrEqual(640);
      const unknown = page.getByRole("checkbox", {
        name: "人間の非娯楽（不明含む）を表示",
        exact: true,
      });
      await unknown.focus();
      await page.keyboard.press("Space");
      await expect(unknown).not.toBeChecked();
      await page
        .getByRole("button", { name: "人間の種別をすべてON", exact: true })
        .click();
      await page.getByRole("button", { name: "日", exact: true }).click();
      await expect(page.locator(".human-time-lane")).toHaveCount(2);
      const dayCell = page.getByRole("button", { name: /^人間 娯楽 / });
      await dayCell.scrollIntoViewIfNeeded();
      await dayCell.focus();
      await page.keyboard.press("Enter");
      await expect(page.getByRole("dialog")).toContainText("集約枠の末尾");
      await page.keyboard.press("Escape");
      await page.screenshot({ path: "test-results/human-bands-day.png" });
      await page.getByRole("button", { name: "週", exact: true }).click();
      await expect(page.locator(".human-time-lane")).toHaveCount(14);
      for (const theme of ["light", "dark"]) {
        await page.getByRole("button", { name: "設定", exact: true }).click();
        await page.getByLabel("テーマ", { exact: true }).selectOption(theme);
        await page
          .getByRole("button", { name: "設定を保存", exact: true })
          .click();
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect(entertainment).toBeVisible();
        await entertainment.scrollIntoViewIfNeeded();
        await entertainment.click();
        await expect(
          page.getByRole("dialog", { name: "人間のアクティビティ（推定）" }),
        ).toBeVisible();
        await page.keyboard.press("Escape");
        await page.screenshot({
          path: path.join("test-results", `human-${theme}.png`),
        });
      }
      await page.setViewportSize(viewport);
      await expect(page.locator("body")).not.toContainText(marker);
    } finally {
      fixture
        .prepare("DELETE FROM turns WHERE id=?")
        .run("human-test-session:turn");
      fixture
        .prepare("DELETE FROM turns WHERE id=?")
        .run("human-test-session:short");
      fixture
        .prepare("DELETE FROM sessions WHERE id=?")
        .run("human-test-session");
      const removeProject = fixture.prepare("DELETE FROM projects WHERE id=?");
      for (const id of fixtureProjects) removeProject.run(id);
      fixture.close();
      const beforeCleanup = (await ipc<{ revision: number }>("human_status"))
        .revision;
      await ipc("sync_logs");
      await expect
        .poll(async () => {
          const status = await ipc<{ revision: number; busy: boolean }>(
            "human_status",
          );
          return status.revision > beforeCleanup && !status.busy;
        })
        .toBe(true);
    }
    for (const file of [cachePath, `${cachePath}-wal`, `${cachePath}-shm`]) {
      let bytes: Buffer = Buffer.alloc(0);
      await expect
        .poll(() => {
          try {
            bytes = existsSync(file) ? readFileSync(file) : Buffer.alloc(0);
            return true;
          } catch (error) {
            if (
              ["EBUSY", "ENOENT"].includes(
                (error as NodeJS.ErrnoException).code ?? "",
              )
            )
              return false;
            throw error;
          }
        })
        .toBe(true);
      expect(bytes.includes(Buffer.from(marker))).toBe(false);
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
    ).toBe(4);
    expect(
      db.prepare("SELECT payload FROM activity_records").get()?.payload,
    ).toContain(marker);
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect.poll(async () => (await period()).records.length).toBe(4);
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
  page.on("pageerror", (error) =>
    console.error(`Native renderer error: ${error.message}`),
  );
  await page.waitForLoadState("domcontentloaded");
  if (!waitForReady) return;
  // Fixture times and labels use Japan time; CI runners usually use UTC.
  try {
    const settings = page.getByRole("button", { name: "設定", exact: true });
    await settings.waitFor({
      state: "visible",
      timeout: Math.max(1, 60000 - (Date.now() - startupBegan)),
    });
    console.info(`Native UI ready after ${Date.now() - startupBegan}ms`);
    await settings.click();
  } catch (error) {
    console.error(
      `Native startup document: ${await page.locator("body").innerText()}`,
    );
    await page.screenshot({ path: "test-results/native-startup-debug.png" });
    throw error;
  }
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
test("human month: dense observations, independent bars and filtered day records", async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  const humanRoot = path.join(root, "dense-human-only");
  mkdirSync(humanRoot, { recursive: true });
  const writer = new DatabaseSync(path.join(humanRoot, "storage.db"));
  const marker = "PRIVATE_DENSE_MONTH_CONTENT";
  const captured = Date.parse("2026-11-20T01:00:00Z");
  writer.exec(
    "PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); BEGIN",
  );
  const insert = writer.prepare("INSERT INTO activity_records VALUES(?,?,?)");
  const payload = (duration: number, kind: string) =>
    JSON.stringify({
      durationEstimate: duration,
      foreground: { title: marker },
      detection: { diagnostics: { entertainmentDisposition: kind } },
    });
  for (let i = 0; i < 50_001; i++)
    insert.run(
      `dense-${String(i).padStart(6, "0")}`,
      Date.parse("2026-11-01T00:00Z") + i,
      payload(1, "ENTERTAINMENT"),
    );
  for (const [id, start, duration, kind] of [
    ["a", captured, 180, "ENTERTAINMENT"],
    ["b", captured + 180000, 60, "ENTERTAINMENT"],
    ["c", captured + 241000, 60, "NON_ENTERTAINMENT"],
    ["d", captured + 400000, 0, "UNCERTAIN"],
    ["long-ent", captured + 86400000, 14400, "ENTERTAINMENT"],
    ["long-non", captured + 86400000 + 14400000, 7200, "NON_ENTERTAINMENT"],
  ] as const)
    insert.run(id, start, payload(duration, kind));
  writer.exec("COMMIT");
  writer.close();
  const ipc = <T>(command: string, args: Record<string, unknown> = {}) =>
    page.evaluate(
      ({ command, args }) =>
        (
          window as unknown as {
            __TAURI_INTERNALS__: {
              invoke: (
                command: string,
                args: Record<string, unknown>,
              ) => Promise<unknown>;
            };
          }
        ).__TAURI_INTERNALS__.invoke(command, args),
      { command, args },
    ) as Promise<T>;
  const args = {
    from: Date.parse("2026-11-01T00:00Z"),
    to: Date.parse("2026-12-01T00:00Z"),
    timezone: "Asia/Tokyo",
    visible: ["entertainment", "non_entertainment", "unknown"],
  };
  try {
    await launch();
    await expect(ipc("human_summary", args)).rejects.toThrow(
      /human_activity_disabled/,
    );
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("れいの AI 実行履歴を取り込む").uncheck();
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByLabel("れい データディレクトリ").fill(humanRoot);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "設定", exact: true }),
    ).not.toBeVisible();
    await expect
      .poll(
        async () =>
          (await ipc<HumanSummaryResponse>("human_summary", args)).days.length,
        { timeout: 40000 },
      )
      .toBe(3);
    const response = await ipc<HumanSummaryResponse>("human_summary", args);
    const day = response.days.find((day) => day.date === "2026-11-20")!;
    expect(day.total_ms).toBe(300000);
    expect(day.kinds).toEqual({
      entertainment: 240000,
      non_entertainment: 60000,
      unknown: 0,
    });
    expect(day.day_length_ms).toBe(86400000);
    expect(day.observed.sort()).toEqual([
      "entertainment",
      "non_entertainment",
      "unknown",
    ]);
    const wire = JSON.stringify(response);
    expect(wire.length).toBeLessThan(4096);
    for (const forbidden of [
      marker,
      humanRoot,
      "source_id",
      "native_id",
      "foreground",
      "UNCERTAIN",
    ])
      expect(wire).not.toContain(forbidden);
    await expect(
      ipc("human_activity", { from: args.from, to: args.to }),
    ).rejects.toThrow(/period_too_dense/);
    await page.getByLabel("表示日").fill("2026-11-20");
    const guide = page.getByRole("button", {
      name: "案内を閉じる",
      exact: true,
    });
    if (await guide.isVisible()) await guide.click();
    await page.getByRole("button", { name: "月", exact: true }).click();
    const cell = page.getByRole("button", { name: /^2026-11-20 / });
    await expect(cell).toHaveAttribute("data-level", "0");
    await expect(
      cell.getByRole("img", { name: "娯楽 推定 0h 4m", exact: true }),
    ).toBeVisible();
    await expect(
      cell.getByRole("img", {
        name: "非娯楽（不明含む） 推定 0h 1m",
        exact: true,
      }),
    ).toBeVisible();
    await expect(cell.getByRole("img")).toHaveCount(2);
    const longCell = page.getByRole("button", { name: /^2026-11-21 / });
    await expect(
      longCell.getByRole("img", { name: "娯楽 推定 4h 0m", exact: true }),
    ).toBeVisible();
    await expect(
      longCell.getByRole("img", {
        name: "非娯楽（不明含む） 推定 2h 0m",
        exact: true,
      }),
    ).toBeVisible();
    await expect(page.locator(".empty-state")).toHaveCount(0);
    await cell.click();
    let dialog = page.getByRole("dialog", {
      name: "2026-11-20 のアクティビティ",
      exact: true,
    });
    await expect(
      dialog.getByRole("region", { name: "AIの実行履歴", exact: true }),
    ).toContainText("AI の記録はありません");
    let human = dialog.getByRole("region", {
      name: "人間のアクティビティ（推定）",
      exact: true,
    });
    await expect(
      human.getByRole("button", { name: /^人間 娯楽 / }),
    ).toHaveCount(1);
    await expect(human.getByTestId("human-total")).toHaveText("0h 5m");
    await human
      .getByLabel("人間の日別表示単位", { exact: true })
      .selectOption("turn");
    await expect(
      human.getByRole("button", { name: /^人間 娯楽 / }),
    ).toHaveCount(2);
    await dialog.getByRole("button", { name: "閉じる", exact: true }).click();
    await page
      .getByRole("checkbox", {
        name: "人間の非娯楽（不明含む）を表示",
        exact: true,
      })
      .uncheck();
    await expect(
      cell.getByRole("img", { name: /^非娯楽（不明含む） / }),
    ).toHaveCount(0);
    await cell.click();
    dialog = page.getByRole("dialog", {
      name: "2026-11-20 のアクティビティ",
      exact: true,
    });
    human = dialog.getByRole("region", {
      name: "人間のアクティビティ（推定）",
      exact: true,
    });
    await expect(
      human.getByRole("button", { name: /^人間 娯楽 / }),
    ).toHaveCount(2);
    await expect(
      human.getByRole("button", { name: /^人間 非娯楽（不明含む） / }),
    ).toHaveCount(0);
    await expect(human.getByTestId("human-total")).toHaveText("0h 4m");
    await dialog.getByRole("button", { name: "閉じる", exact: true }).click();
    await page
      .getByRole("button", { name: "人間の種別をすべてON", exact: true })
      .click();
    await cell.click();
    await human
      .getByRole("button", { name: /^人間 非娯楽（不明含む） .*点の観測/ })
      .click();
    const detail = page.getByRole("dialog", {
      name: "人間のアクティビティ（推定）",
      exact: true,
    });
    await expect(detail).toContainText("0h 0m 0.000s");
    for (const forbidden of [marker, humanRoot, "dense-", "UNCERTAIN"])
      await expect(detail).not.toContainText(forbidden);
    await page.keyboard.press("Escape");
    const viewport = await page.evaluate(() => ({
      width: innerWidth,
      height: innerHeight,
    }));
    await page.setViewportSize({ width: 920, height: 640 });
    const monthArea = await page.locator(".month-view").boundingBox();
    expect(monthArea!.height).toBeGreaterThanOrEqual(180);
    for (const theme of ["light", "dark"]) {
      await page.getByRole("button", { name: "設定", exact: true }).click();
      await page.getByLabel("テーマ", { exact: true }).selectOption(theme);
      await page
        .getByRole("button", { name: "設定を保存", exact: true })
        .click();
      await expect(
        cell.getByRole("img", { name: "娯楽 推定 0h 4m", exact: true }),
      ).toBeVisible();
      await cell.click();
      await expect(
        page.getByRole("dialog", {
          name: "2026-11-20 のアクティビティ",
          exact: true,
        }),
      ).toBeVisible();
      await page.keyboard.press("Escape");
      await page.screenshot({
        path: path.join("test-results", `human-month-${theme}.png`),
      });
    }
    await page.setViewportSize(viewport);
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("人間のアクティビティ機能を有効にする").uncheck();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(page.locator(".human-day-bars")).toHaveCount(0);
    await expect(
      page.getByRole("region", { name: "人間の観測推定時間", exact: true }),
    ).toHaveCount(0);
    await expect(ipc("human_summary", args)).rejects.toThrow(
      /human_activity_disabled/,
    );
    await expect(page.locator("body")).not.toContainText(marker);
  } finally {
    await stop();
  }
});

test("human performance: native synchronization keeps navigation responsive without external requests", async () => {
  test.setTimeout(180000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  const humanRoot = path.join(root, "human-performance");
  mkdirSync(humanRoot, { recursive: true });
  const db = new DatabaseSync(path.join(humanRoot, "storage.db"));
  const rows = Number(globalThis.process.env.CODECAL_NATIVE_PERF_ROWS ?? 10000);
  expect([10000, 100000, 1000000]).toContain(rows);
  const captured = Date.parse("2026-11-01T00:00:00Z");
  const marker = "PRIVATE_NATIVE_PERFORMANCE_CONTENT";
  const payload = JSON.stringify({
    durationEstimate: 60,
    foreground: { title: marker },
    detection: {
      diagnostics: { entertainmentDisposition: "NON_ENTERTAINMENT" },
    },
  });
  db.exec(
    "PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); BEGIN",
  );
  const insert = db.prepare("INSERT INTO activity_records VALUES(?,?,?)");
  for (let i = 0; i < rows; i++)
    insert.run(
      `synthetic-${String(i).padStart(12, "0")}`,
      captured + i * 60000,
      payload,
    );
  db.exec("COMMIT");
  const external: string[] = [];
  const memoryReady = path.join(root, "memory-ready");
  const memoryStop = path.join(root, "memory-stop");
  const memoryResult = path.join(root, "memory-result.json");
  let memoryMonitor: ChildProcess | undefined;
  let memoryError = "";
  try {
    await launch();
    const mainMemory = () =>
      JSON.parse(
        execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            `$p=Get-Process -Id ${process.pid}; if ($p.Path -ne '${exe.replaceAll("'", "''")}') { throw 'Unexpected test process' }; $info=Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId; $owned=[Collections.Generic.List[int]]::new(); $owned.Add(${process.pid}); for($i=0;$i -lt $owned.Count;$i++){foreach($child in $info){if($child.ParentProcessId -eq $owned[$i] -and -not $owned.Contains([int]$child.ProcessId)){$owned.Add([int]$child.ProcessId)}}}; [long]$working=0; [long]$peak=0; foreach($member in $owned){$child=Get-Process -Id $member -ErrorAction SilentlyContinue;if($child -and $child.StartTime -ge $p.StartTime){$working+=$child.WorkingSet64;$peak+=$child.PeakWorkingSet64}}; @{working_set_bytes=$p.WorkingSet64;peak_working_set_bytes=$p.PeakWorkingSet64;tree_working_set_bytes=$working;tree_peak_sum_bytes=$peak}|ConvertTo-Json -Compress`,
          ],
          { encoding: "utf8", windowsHide: true },
        ),
      ) as {
        working_set_bytes: number;
        peak_working_set_bytes: number;
        tree_working_set_bytes: number;
        tree_peak_sum_bytes: number;
      };
    const baselineMemory = mainMemory();
    memoryMonitor = spawn(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        path.resolve("scripts/measure-native-memory.ps1"),
        "-ParentProcessId",
        String(process.pid),
        "-ExpectedExecutable",
        exe,
        "-ReadyFile",
        memoryReady,
        "-StopFile",
        memoryStop,
        "-ResultFile",
        memoryResult,
      ],
      { windowsHide: true, stdio: ["ignore", "ignore", "pipe"] },
    );
    memoryMonitor.on("error", (error) => {
      memoryError = error.message;
    });
    memoryMonitor.stderr?.on("data", (chunk) => {
      memoryError = (memoryError + String(chunk)).slice(-2048);
    });
    await expect
      .poll(() => {
        if (memoryError || memoryMonitor?.exitCode !== null)
          throw new Error(`Native memory monitor failed: ${memoryError}`);
        return existsSync(memoryReady);
      })
      .toBe(true);
    const ipc = <T>(command: string, args: Record<string, unknown> = {}) =>
      page.evaluate(
        ({ command, args }) =>
          (
            window as unknown as {
              __TAURI_INTERNALS__: {
                invoke: (
                  command: string,
                  args: Record<string, unknown>,
                ) => Promise<unknown>;
              };
            }
          ).__TAURI_INTERNALS__.invoke(command, args),
        { command, args },
      ) as Promise<T>;
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (
        ["http:", "https:"].includes(url.protocol) &&
        !["tauri.localhost", "ipc.localhost"].includes(url.hostname)
      )
        external.push(url.origin);
    });
    await ipc("clear_human_activity", { all: true });
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("れいの AI 実行履歴を取り込む").uncheck();
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByLabel("れい データディレクトリ").fill(humanRoot);
    const began = Date.now();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "設定", exact: true }),
    ).not.toBeVisible();
    await expect
      .poll(
        async () =>
          (await ipc<{ busy: boolean; revision: number }>("human_status"))
            .busy ||
          (await ipc<{ revision: number }>("human_status")).revision > 0,
      )
      .toBe(true);
    const navigationResult = await page.evaluate(async () => {
      const samples: number[] = [];
      let duringSync = 0;
      for (let i = 0; i < 30; i++) {
        const host = window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (command: string) => Promise<{ busy: boolean }>;
          };
        };
        if ((await host.__TAURI_INTERNALS__.invoke("human_status")).busy)
          duringSync++;
        const date = document.querySelector(
          'input[aria-label="表示日"]',
        ) as HTMLInputElement;
        const previous = date.value;
        const start = performance.now();
        (
          document.querySelector(
            `button[aria-label="${i % 2 ? "前へ" : "次へ"}"]`,
          ) as HTMLButtonElement
        ).click();
        await new Promise<void>((resolve, reject) => {
          const changed = () => {
            if (
              date.value !== previous &&
              document.querySelector("main")?.getAttribute("aria-busy") ===
                "false"
            )
              resolve();
            else if (performance.now() - start > 5000)
              reject(new Error("Native date navigation did not finish"));
            else requestAnimationFrame(changed);
          };
          requestAnimationFrame(changed);
        });
        samples.push(performance.now() - start);
      }
      return { samples: samples.sort((a, b) => a - b), duringSync };
    });
    const navigation = navigationResult.samples;
    await expect
      .poll(
        async () =>
          await ipc<{ busy: boolean; error: string | null }>("human_status"),
        { timeout: 40000 },
      )
      .toMatchObject({ busy: false, error: null });
    const elapsed = Date.now() - began;
    writeFileSync(memoryStop, "stop");
    await expect.poll(() => memoryMonitor?.exitCode).toBe(0);
    const syncMemory = JSON.parse(
      readFileSync(memoryResult, "utf8").replace(/^\uFEFF/, ""),
    ) as { observed_tree_rss_increment_bytes: number; [key: string]: unknown };
    const status = await ipc<{ revision: number }>("human_status");
    expect(status.revision).toBe(1);
    const summaryStart = Date.now();
    const summary = await ipc<HumanSummaryResponse>("human_summary", {
      from: captured,
      to: captured + 42 * 86400000,
      timezone: "UTC",
      visible: ["entertainment", "non_entertainment", "unknown"],
    });
    const summaryMs = Date.now() - summaryStart;
    expect(summary.days.length).toBeLessThanOrEqual(43);
    expect(JSON.stringify(summary)).not.toContain(marker);
    const periodIpc = await page.evaluate(
      async ({ captured, marker }) => {
        const host = window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args: Record<string, unknown>,
            ) => Promise<unknown>;
          };
        };
        const periods: Record<
          string,
          { samples_ms: number[]; p95_ms: number; response_bytes: number }
        > = {};
        for (const [name, days] of [
          ["day", 1],
          ["week", 7],
          ["month", 42],
        ] as const) {
          const samples: number[] = [];
          let bytes = 0;
          for (let repeat = 0; repeat < 5; repeat++) {
            const start = performance.now();
            const result = await host.__TAURI_INTERNALS__.invoke(
              name === "month" ? "human_summary" : "human_activity",
              name === "month"
                ? {
                    from: captured,
                    to: captured + days * 86400000,
                    timezone: "UTC",
                    visible: ["entertainment", "non_entertainment", "unknown"],
                  }
                : {
                    from: captured,
                    to: captured + days * 86400000,
                    grouped: false,
                    timezone: "UTC",
                  },
            );
            samples.push(performance.now() - start);
            const wire = JSON.stringify(result);
            if (wire.includes(marker))
              throw new Error("Private synthetic marker entered IPC");
            bytes = Math.max(bytes, new TextEncoder().encode(wire).length);
            if (bytes >= 8 * 1024 * 1024)
              throw new Error("IPC payload budget exceeded");
          }
          samples.sort((a, b) => a - b);
          periods[name] = {
            samples_ms: samples,
            p95_ms: samples[Math.ceil(samples.length * 0.95) - 1],
            response_bytes: bytes,
          };
        }
        return periods;
      },
      { captured, marker },
    );
    await page.getByRole("button", { name: "日", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "日", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await page.getByLabel("表示日").fill("2026-11-01");
    await expect(page.locator(".human-time-lane")).toHaveCount(2);
    expect(await page.locator(".human-block").count()).toBeLessThanOrEqual(576);
    await page.locator(".human-block").first().click();
    await expect(
      page.getByRole("dialog", { name: "人間のアクティビティ（推定）" }),
    ).not.toContainText(marker);
    await page.keyboard.press("Escape");
    expect(external).toEqual([]);
    const memory = mainMemory();
    const metrics = {
      rows,
      sync_memory: syncMemory,
      initial_worker_and_ui_ms: elapsed,
      navigation_samples_ms: navigation,
      navigation_during_sync: navigationResult.duringSync,
      navigation_p95_ms: navigation[Math.ceil(navigation.length * 0.95) - 1],
      summary_ipc_ms: summaryMs,
      summary_ipc_bytes: Buffer.byteLength(JSON.stringify(summary)),
      period_ipc: periodIpc,
      external_requests: external.length,
      main_process_baseline_rss: baselineMemory.working_set_bytes,
      main_process_peak_rss: memory.peak_working_set_bytes,
      main_process_rss_increment: Math.max(
        0,
        memory.peak_working_set_bytes - baselineMemory.working_set_bytes,
      ),
      tree_baseline_rss: baselineMemory.tree_working_set_bytes,
      tree_peak_sum_rss: memory.tree_peak_sum_bytes,
      tree_rss_increment: Math.max(
        0,
        memory.tree_peak_sum_bytes - baselineMemory.tree_working_set_bytes,
      ),
      memory_scope:
        "Tauri main plus live descendants, including WebView2; sum of each process peak is conservative for live processes; exited transient subprocesses are excluded",
    };
    mkdirSync("test-results", { recursive: true });
    writeFileSync(
      path.join("test-results", `human-native-performance-${rows}.json`),
      JSON.stringify(metrics, null, 2),
    );
    console.info("Synthetic human native performance", JSON.stringify(metrics));
    if (globalThis.process.env.CODECAL_ENFORCE_PERF === "1") {
      expect(metrics.navigation_p95_ms).toBeLessThanOrEqual(100);
      expect(metrics.navigation_during_sync).toBeGreaterThan(0);
      expect(periodIpc.day.p95_ms).toBeLessThanOrEqual(100);
      expect(periodIpc.week.p95_ms).toBeLessThanOrEqual(200);
      expect(periodIpc.month.p95_ms).toBeLessThanOrEqual(500);
      expect(metrics.main_process_rss_increment).toBeLessThanOrEqual(
        128 * 1024 * 1024,
      );
      expect(syncMemory.observed_tree_rss_increment_bytes).toBeLessThanOrEqual(
        128 * 1024 * 1024,
      );
    }
    await expect(page.locator("body")).not.toContainText(marker);
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("人間のアクティビティ機能を有効にする").uncheck();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await ipc("clear_human_activity", { all: true });
  } finally {
    writeFileSync(memoryStop, "stop");
    db.close();
    await stop();
    if (memoryMonitor && memoryMonitor.exitCode === null)
      await new Promise<void>((resolve) =>
        memoryMonitor!.once("exit", () => resolve()),
      );
  }
});

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
        name: "ブロックの表示単位",
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
      .getByRole("combobox", { name: "ブロックの表示単位", exact: true })
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

test("human bands: dense day/week lanes, duration coverage and keyboard details", async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  const humanRoot = path.join(root, "dense-bands");
  mkdirSync(humanRoot, { recursive: true });
  const db = new DatabaseSync(path.join(humanRoot, "storage.db"));
  const captured = Date.parse("2026-11-19T15:00Z");
  db.exec(
    "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT)",
  );
  db.prepare(
    `WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<60000)
    INSERT INTO activity_records SELECT 'synthetic-dense-'||i,?+i*500,
    json_object('durationEstimate',1,'foreground',json_object('title','PRIVATE_DENSE_BANDS'),
    'detection',json_object('diagnostics',json_object('entertainmentDisposition',
    CASE WHEN i%10<6 THEN 'UNCERTAIN' WHEN i%10<9 THEN 'ENTERTAINMENT' ELSE 'NON_ENTERTAINMENT' END))) FROM n`,
  ).run(captured);
  db.close();
  try {
    await launch();
    const guide = page.getByRole("button", {
      name: "案内を閉じる",
      exact: true,
    });
    if (await guide.count()) await guide.click();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("テーマ", { exact: true }).selectOption("light");
    await page.getByLabel("れいの AI 実行履歴を取り込む").uncheck();
    await page.getByLabel("Claude Code の履歴を取り込む").uncheck();
    await page.getByLabel("人間のアクティビティ機能を有効にする").check();
    await page.getByLabel("れい データディレクトリ").fill(humanRoot);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "設定", exact: true }),
    ).toHaveCount(0);
    await page.getByRole("button", { name: "日", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "日", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await page.getByLabel("表示日").fill("2026-11-20");
    const allOff = page.getByRole("button", {
      name: "プロジェクトをすべてOFF",
      exact: true,
    });
    if (await allOff.count()) await allOff.click();
    const allHumanOn = page.getByRole("button", {
      name: "人間の種別をすべてON",
      exact: true,
    });
    if (await allHumanOn.count()) await allHumanOn.click();
    await expect(page.locator(".human-time-cell")).toHaveCount(201, {
      timeout: 40000,
    });
    await expect(page.locator(".human-time-lane")).toHaveCount(2);
    await expect(
      page.getByRole("region", { name: "表示期間の集計" }),
    ).toHaveCount(0);
    const sidebarSync = page
      .getByRole("region", { name: "人間のアクティビティ", exact: true })
      .getByRole("region", { name: "人間のアクティビティ同期", exact: true });
    await expect(sidebarSync).toContainText("1時間ごと");
    await expect(sidebarSync.locator("time")).toHaveCount(2);
    const syncTimes = await sidebarSync
      .locator("time")
      .evaluateAll((els) =>
        els.map((el) => Date.parse(el.getAttribute("datetime")!)),
      );
    expect(syncTimes[1] - syncTimes[0]).toBeGreaterThan(3595000);
    expect(syncTimes[1] - syncTimes[0]).toBeLessThan(3605000);
    for (const time of await sidebarSync.locator("time").all()) {
      expect((await time.boundingBox())!.height).toBeLessThanOrEqual(20);
    }

    await expect(page.locator(".workspace .human-sync")).toHaveCount(0);
    const normalViewport =
      page.viewportSize() ??
      (await page.evaluate(() => ({
        width: window.innerWidth,
        height: window.innerHeight,
      })));
    await page.setViewportSize({ width: 1100, height: 600 });
    const manual = sidebarSync.getByRole("button", {
      name: "人間のアクティビティを再同期",
      exact: true,
    });
    await manual.scrollIntoViewIfNeeded();
    await expect(manual).toBeVisible();
    await expect(
      sidebarSync.locator("..").locator(".sidebar-label"),
    ).toBeInViewport();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByRole("button", { name: "閉じる", exact: true }).click();
    await page.setViewportSize(normalViewport);
    const first = page
      .locator('.human-time-lane[data-kind="entertainment"] button')
      .first();
    expect(
      await first.evaluate((el) =>
        (el as HTMLElement).style.getPropertyValue("--coverage"),
      ),
    ).toBe("30%");
    await first.scrollIntoViewIfNeeded();
    await first.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("dialog")).toContainText("5分枠");
    await expect(page.getByRole("dialog")).not.toContainText(
      "PRIVATE_DENSE_BANDS",
    );
    await page.keyboard.press("Escape");
    const labels = await page
      .locator(".human-time-cell")
      .evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")));
    await page.screenshot({ path: "test-results/human-bands-dense-day.png" });
    await page.getByRole("button", { name: "週", exact: true }).click();
    await expect(page.locator(".human-time-lane")).toHaveCount(14);
    await expect(page.locator(".human-time-cell")).toHaveCount(201);
    expect(
      await page
        .locator(".human-time-cell")
        .evaluateAll((els) => els.map((el) => el.getAttribute("aria-label"))),
    ).toEqual(labels);
    await page.screenshot({ path: "test-results/human-bands-dense-week.png" });
    const unknown = page.getByRole("checkbox", {
      name: "人間の非娯楽（不明含む）を表示",
      exact: true,
    });
    await unknown.uncheck();
    await expect(page.locator(".human-time-lane")).toHaveCount(14);
    await expect(
      page.locator('.human-time-lane[data-kind="other"] button'),
    ).toHaveCount(0);
    const missingRoot = path.join(root, "missing-human-source");
    mkdirSync(missingRoot, { recursive: true });
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("れい データディレクトリ").fill(missingRoot);
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
    const warnedSettings = page.getByRole("dialog", {
      name: "設定",
      exact: true,
    });
    await expect(warnedSettings).toContainText(
      "前回の正常なキャッシュは保持します",
    );
    await warnedSettings
      .getByRole("button", { name: "閉じる", exact: true })
      .click();
    await expect(warnedSettings).toHaveCount(0);
    await expect(sidebarSync.getByRole("alert")).toContainText(
      "保存先を開けません",
    );
    await page.setViewportSize({ width: 1100, height: 600 });
    await manual.scrollIntoViewIfNeeded();
    await expect(manual).toBeVisible();
    await expect(
      sidebarSync.locator("..").locator(".sidebar-label"),
    ).toBeInViewport();
    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page.getByLabel("人間のアクティビティ機能を有効にする").uncheck();
    await page.getByRole("button", { name: "設定を保存", exact: true }).click();
  } finally {
    await stop();
  }
});
