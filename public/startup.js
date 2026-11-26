/* global window, document, performance */
// This small classic script is intentionally independent of Vite/React modules.
(() => {
  const shell = document.getElementById("boot-shell");
  const message = document.getElementById("boot-message");
  const error = document.getElementById("boot-error");
  const reload = document.getElementById("boot-reload");
  const timings = { shell_ms: performance.now() };
  let active = true;
  const failed = () => {
    if (!active) return;
    shell.setAttribute("aria-busy", "false");
    shell.querySelector(".boot-progress").hidden = true;
    error.textContent = "画面を読み込めませんでした。再読み込みしてください。";
    error.hidden = false;
    reload.hidden = false;
  };
  const onError = (event) => {
    if (event.target?.id === "app-module" || event instanceof window.ErrorEvent)
      failed();
  };
  const slow = window.setTimeout(() => {
    if (!active) return;
    message.textContent = "画面の読み込みに時間がかかっています…";
    reload.hidden = false;
  }, 15000);
  reload.addEventListener("click", () => window.location.reload());
  window.addEventListener("error", onError, true);
  window.addEventListener("unhandledrejection", failed);
  window.__CODECAL_STARTUP__ = {
    timings,
    mark(name) {
      timings[name] = performance.now();
    },
    reactReady() {
      if (!active) return;
      active = false;
      timings.react_ms = performance.now();
      window.clearTimeout(slow);
      window.removeEventListener("error", onError, true);
      window.removeEventListener("unhandledrejection", failed);
      shell.remove();
    },
  };
  // The native window stays hidden until this dependency-free shell exists.
  // No React, database, source scan, or module import is required to show it.
  window.__TAURI_INTERNALS__
    ?.invoke("show_startup")
    .then((ms) => {
      timings.native_shell_ms = ms;
    })
    .catch(failed);
})();
