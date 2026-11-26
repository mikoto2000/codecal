import { test, expect } from "@playwright/test";
import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import path from "node:path";

test("native startup spinner can be closed before WebView or database initialization", async () => {
  test.setTimeout(60000);
  const root = path.resolve(".local", `startup-cancel-${Date.now()}`);
  mkdirSync(root, { recursive: true });
  const ready = path.join(root, "ready");
  const pidFile = path.join(root, "pid");
  const result = path.join(root, "result.json");
  mkdirSync(path.resolve("test-results"), { recursive: true });
  const screenshot = path.resolve("test-results/startup-native.png");
  const script = path.join(root, "watch.ps1");
  // Warm up the monitor before launching the app, so a fast startup cannot
  // hide the splash while a new PowerShell process is still initializing.
  writeFileSync(
    script,
    "\ufeff" +
      `param($PidFile,$ReadyFile,$ResultFile,$ScreenshotFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class StartupCapture {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
  private delegate bool Visitor(IntPtr window,IntPtr unused);
  [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor visitor,IntPtr unused);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr window,StringBuilder text,int count);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window,uint message,IntPtr w,IntPtr l);
  public static IntPtr Find(int process) {
    IntPtr found=IntPtr.Zero;
    EnumWindows((window,unused) => {
      uint owner; GetWindowThreadProcessId(window,out owner);
      var name=new StringBuilder(128); GetClassName(window,name,name.Capacity);
      if(owner==(uint)process && IsWindowVisible(window) && name.ToString()=="CodeCalStartup") {
        found=window; return false;
      }
      return true;
    },IntPtr.Zero);
    return found;
  }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
Set-Content -LiteralPath $ReadyFile -Value 'ready'
$timer=[Diagnostics.Stopwatch]::StartNew()
while (!(Test-Path -LiteralPath $PidFile) -and $timer.Elapsed.TotalSeconds -lt 15) { Start-Sleep -Milliseconds 20 }
$targetProcessId=[int](Get-Content -LiteralPath $PidFile)
$observed=$false
$exited=$false
while ($timer.Elapsed.TotalSeconds -lt 25) {
  $targetProcess=Get-Process -Id $targetProcessId -ErrorAction SilentlyContinue
  if (!$targetProcess) { break }
  $startupWindow=[StartupCapture]::Find($targetProcessId)
  if ($startupWindow -ne [IntPtr]::Zero) {
    $rect=New-Object StartupCapture+Rect
    if ([StartupCapture]::GetClientRect( $startupWindow,[ref]$rect)) {
      $bitmap=New-Object Drawing.Bitmap($rect.Right,$rect.Bottom)
      $graphics=[Drawing.Graphics]::FromImage($bitmap)
      $dc=$graphics.GetHdc()
      try { [void][StartupCapture]::PrintWindow( $startupWindow,$dc,1) }
      finally { $graphics.ReleaseHdc($dc) }
      try { $bitmap.Save($ScreenshotFile,[Drawing.Imaging.ImageFormat]::Png) }
      finally { $graphics.Dispose();$bitmap.Dispose() }
    }
    $observed=[StartupCapture]::PostMessage($startupWindow,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    $exited=$targetProcess.WaitForExit(10000)
    break
  }
  Start-Sleep -Milliseconds 20
}
@{observed=$observed;exited=$exited} | ConvertTo-Json | Set-Content -LiteralPath $ResultFile
`,
    "utf8",
  );
  const monitor = spawn(
    "powershell.exe",
    [
      "-NoProfile",
      "-File",
      script,
      "-PidFile",
      pidFile,
      "-ReadyFile",
      ready,
      "-ResultFile",
      result,
      "-ScreenshotFile",
      screenshot,
    ],
    { windowsHide: true },
  );
  let child: ChildProcess | undefined;
  try {
    await expect.poll(() => existsSync(ready)).toBe(true);
    child = spawn(
      path.resolve(
        process.env.CODECAL_E2E_EXE ?? "src-tauri/target/release/codecal.exe",
      ),
      [],
      {
        windowsHide: true,
        env: {
          ...process.env,
          CODECAL_DATA_DIR: path.join(root, "data"),
          CODEX_HOME: path.join(root, "unused-codex"),
          REI_DATA_DIR: path.join(root, "unused-rei"),
          CLAUDE_CONFIG_DIR: path.join(root, "unused-claude"),
          WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview"),
        },
      },
    );
    expect(child.pid).toBeDefined();
    writeFileSync(pidFile, String(child.pid));
    await expect.poll(() => existsSync(result), { timeout: 30000 }).toBe(true);
    expect(
      JSON.parse(readFileSync(result, "utf8").replace(/^\uFEFF/, "")),
    ).toEqual({ observed: true, exited: true });
    await expect.poll(() => child!.exitCode).toBe(0);
    expect(existsSync(path.join(root, "data", "codecal.db"))).toBe(false);
  } finally {
    for (const task of [child, monitor]) {
      if (task?.pid && task.exitCode === null) {
        try {
          execFileSync("taskkill", ["/pid", String(task.pid), "/t", "/f"], {
            windowsHide: true,
            stdio: "ignore",
          });
        } catch {
          /* already exited */
        }
      }
    }
  }
});
