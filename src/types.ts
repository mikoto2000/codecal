export type Project = { id: string; cwd: string; name: string; color: string };
export type Turn = {
  source?: "codex" | "rei";
  native_session_id?: string | null;
  native_turn_id?: string | null;
  id: string;
  session_id: string;
  turn_id: string;
  project_id: string;
  start: number;
  end: number | null;
  status: "completed" | "failed" | "interrupted" | "running" | "unknown";
  prompt: string;
  tokens: number | null;
};
export type Day = {
  date: string;
  elapsed_ms: number;
  total_ms: number;
  projects: Record<string, number>;
};
export type Snapshot = {
  projects: Project[];
  turns: Turn[];
  days: Day[];
  diagnostics: string[];
  diagnostics_total: number;
};
export type Config = {
  log_root: string;
  rei_enabled: boolean;
  rei_root: string;
  timezone: string;
  week_start: number;
  theme: "system" | "light" | "dark";
  thresholds: [number, number, number];
  auto_refresh: boolean;
};
export type Bootstrap = {
  config: Config;
  db_path: string;
  first_run: boolean;
  notices: string[];
};
