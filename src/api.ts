import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Bootstrap,
  Config,
  Snapshot,
  HumanActivityResponse,
  HumanSyncStatus,
  HumanClearResult,
} from "./types";
export const api = {
  humanActivity: (from: number, to: number) =>
    invoke<HumanActivityResponse>("human_activity", { from, to }),
  humanStatus: () => invoke<HumanSyncStatus>("human_status"),
  humanSync: () => invoke<void>("sync_human"),
  humanClear: (all: boolean) =>
    invoke<HumanClearResult>("clear_human_activity", { all }),
  humanEvents: (changed: (status: HumanSyncStatus) => void) =>
    listen<HumanSyncStatus>("human-status", (event) => changed(event.payload)),
  boot: () =>
    invoke<Bootstrap>("bootstrap", {
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }),
  snapshot: (timezone: string) => invoke<Snapshot>("snapshot", { timezone }),
  sync: () =>
    invoke<{ files: number; bytes_read: number; warnings: string[] }>(
      "sync_logs",
    ),
  save: (value: Config) => invoke<string[]>("save_config", { value }),
  project: (id: string, name: string, color: string) =>
    invoke<void>("update_project", { id, name, color }),
  excerpt: (id: string) => invoke<string>("source_excerpt", { id }),
  events: async (
    changed: () => void,
    progress: (s: string) => void,
    error: (s: string) => void,
    startup: (phase: "migrating") => void = () => {},
    warning: (messages: string[]) => void = () => {},
    watchWarning: (message: string) => void = () => {},
  ) => {
    const offs = await Promise.all([
      listen("data-changed", changed),
      listen<{ done: number; total: number }>("sync-progress", (e) =>
        progress(`${e.payload.done} / ${e.payload.total} ファイル`),
      ),
      listen<string>("sync-error", (e) => error(e.payload)),
      listen<"migrating">("startup-progress", (e) => startup(e.payload)),
      listen<string[]>("sync-warning", (e) => warning(e.payload)),
      listen<string>("watch-warning", (e) => watchWarning(e.payload)),
    ]);
    return () => offs.forEach((off) => off());
  },
};
