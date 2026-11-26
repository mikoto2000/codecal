export type Project = { id: string; cwd: string; name: string; color: string };
export type Turn = {
  source?: "codex" | "rei" | "claude";
  details?: {
    parent_session_id: string | null;
    git_branch: string | null;
  } | null;
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
  days?: Day[];
  diagnostics: string[];
  diagnostics_total: number;
};
export type Config = {
  log_root: string;
  claude_enabled?: boolean;
  claude_root?: string;
  rei_enabled: boolean;
  human_activity_enabled?: boolean;
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
export type HumanActivityKind =
  "entertainment" | "non_entertainment" | "unknown";
export type HumanActivity = {
  start: number;
  end: number;
  kind: HumanActivityKind;
};
export type HumanActivityDiagnostic = {
  code:
    | "invalid_json"
    | "payload_too_large"
    | "invalid_payload_type"
    | "invalid_duration_type"
    | "invalid_duration"
    | "invalid_timestamp"
    | "invalid_id"
    | "long_estimate"
    | "simultaneous_conflict";
  count: number;
};
export type HumanActivityResponse = {
  bins?: (Omit<import("./human-time-bands").HumanTimeBin, "top" | "height"> & {
    day_start: number;
    day_end: number;
  })[];
  records: HumanActivity[];
  diagnostics: HumanActivityDiagnostic[];
};
export type HumanDaySummary = {
  date: string;
  day_length_ms: number;
  total_ms: number;
  kinds: Record<HumanActivityKind, number>;
  observed: HumanActivityKind[];
};
export type HumanSummaryResponse = {
  days: HumanDaySummary[];
  diagnostics: HumanActivityDiagnostic[];
};
export type HumanSyncStatus = {
  generation: number;
  busy: boolean;
  revision: number;
  error: string | null;
  last_success: number | null;
};
export type HumanClearResult = { compacted: boolean; staging_cleared: boolean };
