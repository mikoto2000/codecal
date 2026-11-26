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
  const startupBegan = Date.now();
  child = spawn(exe, [], {
    windowsHide: true,
    env: {
      ...globalThis.process.env,
      CODEX_HOME: logs,
      CLAUDE_CONFIG_DIR: path.join(root, "unused-claude"),
      REI_DATA_DIR: path.join(root, "unused-rei"),
      CODECAL_DATA_DIR: data,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9223",
    },
  });
  let launchError = "";
  let stderr = "";
  child.on("error", (error) => {
    launchError = error.message;
  });
  child.stderr?.on("data", (chunk) => {
    stderr = (stderr + String(chunk)).slice(-8192);
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
      // Use the same bounded cold WebView2 startup budget as native.spec.ts.
      { timeout: 60000 },
    )
    .toBe(true)
    .catch((cause) => {
      throw new Error(
        `WebView2 debugging did not start: ${exe}; exit=${child.exitCode}; spawn=${launchError}; stderr=${stderr}`,
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
  await page.getByLabel("表示日").waitFor({
    state: "visible",
    timeout: Math.max(1, 60000 - (Date.now() - startupBegan)),
  });
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
  test.setTimeout(120000);
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
    const availableWidth = await page.evaluate(() =>
      Math.round(screen.availWidth * devicePixelRatio),
    );
    // A maximized window can be narrower than a saved window on a small monitor.
    expect(Math.abs(maximized.width - availableWidth)).toBeLessThanOrEqual(2);
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
  test.setTimeout(120000);
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
