import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Bootstrap, Config, Snapshot } from "./types";
export const api = {
  boot: () =>
    invoke<Bootstrap>("bootstrap", {
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }),
  snapshot: (timezone: string) => invoke<Snapshot>("snapshot", { timezone }),
  sync: () => invoke<{ files: number; bytes_read: number }>("sync_logs"),
  save: (value: Config) => invoke<void>("save_config", { value }),
  project: (id: string, name: string, color: string) =>
    invoke<void>("update_project", { id, name, color }),
  excerpt: (id: string) => invoke<string>("source_excerpt", { id }),
  events: async (
    changed: () => void,
    progress: (s: string) => void,
    error: (s: string) => void,
  ) => {
    const offs = await Promise.all([
      listen("data-changed", changed),
      listen<{ done: number; total: number }>("sync-progress", (e) =>
        progress(`${e.payload.done} / ${e.payload.total} ファイル`),
      ),
      listen<string>("sync-error", (e) => error(e.payload)),
    ]);
    return () => offs.forEach((off) => off());
  },
};
