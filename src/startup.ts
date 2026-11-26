declare global {
  interface Window {
    __CODECAL_STARTUP__?: {
      timings: Record<string, number>;
      mark: (name: string) => void;
      reactReady: () => void;
    };
  }
}
export function markStartup(name: string) {
  window.__CODECAL_STARTUP__?.mark(name);
}
export function showReactStartup() {
  window.__CODECAL_STARTUP__?.reactReady();
}
