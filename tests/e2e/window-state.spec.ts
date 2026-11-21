import {
  test,
  expect,
  chromium,
  type Browser,
  type Page,
} from "@playwright/test";
import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import {
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  unlinkSync,
} from "node:fs";
import path from "node:path";
const exe = path.resolve(
  globalThis.process.env.CODECAL_E2E_EXE ??
    "src-tauri/target/release/codecal.exe",
);
const root = path.resolve(".local", `window-state-${Date.now()}`);
const data = path.join(root, "data");
const logs = path.join(root, "logs");
const stateFile = path.join(data, "window-state.json");
let child: ChildProcess;
let browser: Browser;
let page: Page;
const state = {
  width: 1040,
  height: 680,
  x: 84,
  y: 56,
  prev_x: 84,
  prev_y: 56,
  maximized: false,
  visible: true,
  decorated: true,
  fullscreen: false,
};
function saveFixture(value: unknown) {
  writeFileSync(stateFile, JSON.stringify({ main: value }));
}
async function launch() {
  child = spawn(exe, [], {
    windowsHide: true,
    env: {
      ...globalThis.process.env,
      CODEX_HOME: logs,
      REI_DATA_DIR: path.join(root, "unused-rei"),
      CODECAL_DATA_DIR: data,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9223",
    },
  });
  child.on("error", () => {});
  await expect
    .poll(
      async () => {
        try {
          return (await fetch("http://127.0.0.1:9223/json/version")).ok;
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
  await page.getByLabel("表示日").waitFor();
}
async function closeNormally() {
  // Closing the native main window triggers Tauri's normal exit lifecycle.
  execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-Command",
      `$app=Get-Process -Id ${child.pid}; if(!$app.CloseMainWindow()){throw 'Window close failed'}; if(!$app.WaitForExit(10000)){throw 'Application did not exit normally'}`,
    ],
    { windowsHide: true, stdio: "pipe" },
  );
  await browser.close().catch(() => {});
}
async function cleanUp() {
  await browser?.close().catch(() => {});
  if (child?.pid) {
    try {
      execFileSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } catch {
      /* Already closed. */
    }
  }
}
async function geometry() {
  return page.evaluate(() => ({
    x: Math.round(window.screenX * devicePixelRatio),
    y: Math.round(window.screenY * devicePixelRatio),
    width: Math.round(innerWidth * devicePixelRatio),
    height: Math.round(innerHeight * devicePixelRatio),
    visible: document.visibilityState,
  }));
}
test("window state restores position size and maximization after normal restart", async () => {
  mkdirSync(data, { recursive: true });
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  saveFixture(state);
  try {
    await launch();
    await expect
      .poll(async () => {
        const g = await geometry();
        return (
          Math.abs(g.width - state.width) + Math.abs(g.height - state.height)
        );
      })
      .toBeLessThanOrEqual(2);
    const first = await geometry();
    // WebView screen coordinates include the native frame offset on Windows.
    // Check native outer x/y in the saved plugin state below, and compare the
    // WebView geometry to itself across the restart.
    expect(first.visible).toBe("visible");
    // Removing the fixture proves that a normal close actually writes state.
    unlinkSync(stateFile);
    await closeNormally();
    expect(existsSync(stateFile)).toBe(true);
    const saved = JSON.parse(readFileSync(stateFile, "utf-8")).main;
    expect(saved).toMatchObject({
      width: state.width,
      height: state.height,
      x: state.x,
      y: state.y,
      maximized: false,
    });
    await launch();
    expect(await geometry()).toEqual(first);
    await closeNormally();
    saveFixture({ ...saved, maximized: true });
    await launch();
    const maximized = await geometry();
    expect(maximized.width).toBeGreaterThan(first.width);
    await closeNormally();
    expect(JSON.parse(readFileSync(stateFile, "utf-8")).main.maximized).toBe(
      true,
    );
    await launch();
    expect((await geometry()).width).toBe(maximized.width);
    await closeNormally();
  } finally {
    await cleanUp();
  }
});
test("window state safely opens with missing corrupt and disconnected-monitor state", async () => {
  mkdirSync(data, { recursive: true });
  mkdirSync(path.join(logs, "sessions"), { recursive: true });
  try {
    if (existsSync(stateFile)) unlinkSync(stateFile);
    await launch();
    expect((await geometry()).visible).toBe("visible");
    await closeNormally();
    writeFileSync(stateFile, "{broken");
    await launch();
    expect((await geometry()).visible).toBe("visible");
    await closeNormally();
    saveFixture({
      ...state,
      x: 100000,
      y: 100000,
      prev_x: 100000,
      prev_y: 100000,
    });
    await launch();
    const g = await geometry();
    expect(g.visible).toBe("visible");
    expect(Math.abs(g.x)).toBeLessThan(100000);
    expect(Math.abs(g.y)).toBeLessThan(100000);
    await closeNormally();
  } finally {
    await cleanUp();
  }
});
