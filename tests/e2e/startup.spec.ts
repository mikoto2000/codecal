import {
  test,
  expect,
  chromium,
  type Browser,
  type Page,
} from "@playwright/test";
import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";

const root = path.resolve(".local", `startup-${Date.now()}`);
const exe = path.resolve(
  process.env.CODECAL_E2E_EXE ?? "src-tauri/target/release/codecal.exe",
);
let child: ChildProcess;
let browser: Browser;
let page: Page;
test.beforeEach(async () => {
  test.setTimeout(120000);
  mkdirSync(path.join(root, "logs", "sessions"), { recursive: true });
  const began = Date.now();
  child = spawn(exe, [], {
    windowsHide: true,
    env: {
      ...process.env,
      CODEX_HOME: path.join(root, "logs"),
      CLAUDE_CONFIG_DIR: path.join(root, "unused-claude"),
      REI_DATA_DIR: path.join(root, "unused-rei"),
      CODECAL_DATA_DIR: path.join(root, "data"),
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9223",
    },
  });
  await expect
    .poll(
      async () => {
        try {
          return (await fetch("http://127.0.0.1:9223/json/version")).ok;
        } catch {
          return false;
        }
      },
      { timeout: 60000 },
    )
    .toBe(true);
  browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
  await expect
    .poll(() => browser.contexts().flatMap((c) => c.pages()).length)
    .toBeGreaterThan(0);
  page = browser.contexts().flatMap((c) => c.pages())[0];
  await page
    .getByRole("button", { name: "設定", exact: true })
    .waitFor({ timeout: 60000 });
  console.info("Synthetic startup ready_ms", Date.now() - began);
});
test.afterEach(async () => {
  await browser?.close().catch(() => {});
  if (child?.pid) {
    try {
      execFileSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } catch {
      /* already exited */
    }
  }
});

test("startup shell paints without JavaScript and hands off to the existing React spinner", async () => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setScriptExecutionDisabled", { value: true });
  try {
    await page.reload({ waitUntil: "load" });
    console.info(
      "JavaScript disabled DOM",
      await page.evaluate(() => ({
        rootChildren: document.getElementById("root")?.childElementCount,
        firstPaint: performance
          .getEntriesByName("first-contentful-paint")
          .map((e) => e.startTime),
      })),
    );
    await page.screenshot({ path: "test-results/startup-held.png" });
    const shell = page.getByRole("status", { name: "CodeCalを起動しています" });
    await expect(shell).toBeVisible();
    const spinner = shell.locator(".boot-spinner");
    const a = await spinner.evaluate((el) => getComputedStyle(el).transform);
    await expect
      .poll(() => spinner.evaluate((el) => getComputedStyle(el).transform))
      .not.toBe(a);
    await expect(page.locator("#root")).toBeEmpty();
    await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
    await expect(spinner).toHaveCSS("animation-name", "none");
    await expect(shell).toHaveCSS("background-color", "rgb(24, 31, 44)");
    await page.screenshot({ path: "test-results/startup-held-dark.png" });
  } finally {
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: false });
  }
  await page.reload();
  await page.getByRole("button", { name: "設定", exact: true }).waitFor();
  await expect(page.locator("#boot-shell")).toHaveCount(0);
  const timings = await page.evaluate(
    () => window.__CODECAL_STARTUP__?.timings,
  );
  console.info("Synthetic startup phases", timings);
  expect(timings).toBeDefined();
  expect(timings!.shell_ms).toBeLessThanOrEqual(timings!.module_ms);
  expect(timings!.react_ms).toBeLessThanOrEqual(timings!.database_start_ms);
  expect(timings!.database_ready_ms).toBeGreaterThanOrEqual(
    timings!.database_start_ms,
  );
  expect(timings!.native_shell_ms).toBeGreaterThan(0);
});
