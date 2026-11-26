import { afterEach, beforeEach, expect, it, vi } from "vitest";
import html from "../index.html?raw";
import script from "../public/startup.js?raw";
let host: ReturnType<typeof start>;
function start() {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  document.body.innerHTML = parsed.body.innerHTML;
  const fakeWindow = {
    __CODECAL_STARTUP__: undefined as Window["__CODECAL_STARTUP__"],
    __TAURI_INTERNALS__: { invoke: vi.fn().mockResolvedValue(123) },
    location: { reload: vi.fn() },
    ErrorEvent: window.ErrorEvent,
    addEventListener: window.addEventListener.bind(window),
    removeEventListener: window.removeEventListener.bind(window),
    setTimeout,
    clearTimeout,
  };
  new Function("window", "document", "performance", script)(
    fakeWindow,
    document,
    performance,
  );
  return fakeWindow;
}
beforeEach(() => {
  vi.useFakeTimers();
  host = start();
});
afterEach(() => {
  host.__CODECAL_STARTUP__?.reactReady();
  vi.useRealTimers();
});
it("has an HTML spinner before any module executes and presents without database work", async () => {
  expect(document.querySelector("#boot-shell .boot-spinner")).not.toBeNull();
  expect(document.getElementById("root")).toBeEmptyDOMElement();
  expect(host.__TAURI_INTERNALS__.invoke).toHaveBeenCalledExactlyOnceWith(
    "show_startup",
  );
  await Promise.resolve();
  expect(host.__CODECAL_STARTUP__!.timings.native_shell_ms).toBe(123);
});
it("keeps progress and offers reload when module loading is slow", () => {
  vi.advanceTimersByTime(15000);
  expect(document.getElementById("boot-message")).toHaveTextContent(
    "時間がかかっています",
  );
  expect(document.getElementById("boot-reload")).not.toHaveAttribute("hidden");
  document.getElementById("boot-reload")!.click();
  expect(host.location.reload).toHaveBeenCalledOnce();
});
it("shows an actionable error if the module fails", () => {
  document
    .getElementById("app-module")!
    .dispatchEvent(new Event("error", { bubbles: true }));
  expect(document.getElementById("boot-error")).toHaveTextContent(
    "画面を読み込めませんでした",
  );
  expect(document.querySelector(".boot-progress")).toHaveAttribute("hidden");
  expect(document.getElementById("boot-shell")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  document.getElementById("boot-reload")!.click();
  expect(host.location.reload).toHaveBeenCalledOnce();
});
it("removes the shell only after React commits and stops handling later application errors", () => {
  host.__CODECAL_STARTUP__!.mark("module_ms");
  host.__CODECAL_STARTUP__!.reactReady();
  expect(document.getElementById("boot-shell")).toBeNull();
  expect(host.__CODECAL_STARTUP__!.timings.react_ms).toBeGreaterThanOrEqual(
    host.__CODECAL_STARTUP__!.timings.module_ms,
  );
  window.dispatchEvent(new window.ErrorEvent("error"));
  vi.advanceTimersByTime(15000);
  expect(document.getElementById("boot-shell")).toBeNull();
});
