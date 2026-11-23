import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useTransition,
  type ReactNode,
  type MouseEvent,
  type CSSProperties,
} from "react";
import { DateTime } from "luxon";
import { api } from "./api";
import { useTimelineZoom } from "./useTimelineZoom";
import {
  groupActivity,
  type ActivityGroup,
  type GroupingMode,
} from "./activity-groups";
import {
  daysInView,
  type CalendarView,
  formatDuration,
  heatLevel,
  localDate,
  packSegments,
  indexActivity,
  statusLabel,
  summarizeDays,
} from "./calendar";
import type { Config, Project, Snapshot, Turn } from "./types";
const sourceLabel = (turn: Turn) =>
  turn.source === "claude"
    ? "Claude Code"
    : turn.source === "rei"
      ? "れい"
      : "Codex";
const empty: Snapshot = {
  projects: [],
  turns: [],
  days: [],
  diagnostics: [],
  diagnostics_total: 0,
};
function Modal({
  title,
  close,
  children,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const pressedOutside = useRef(false);
  const titleId = useId();
  const isOutside = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return false;
    const bounds = event.currentTarget.getBoundingClientRect();
    return (
      event.clientX < bounds.left ||
      event.clientX > bounds.right ||
      event.clientY < bounds.top ||
      event.clientY > bounds.bottom
    );
  };
  useEffect(() => {
    const trigger = document.activeElement;
    const d = ref.current;
    if (d?.showModal) d.showModal();
    else d?.setAttribute("open", "");
    return () => {
      if (d?.open && d.close) d.close();
      if (trigger instanceof HTMLElement && trigger.isConnected)
        trigger.focus();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onMouseDown={(event) => {
        pressedOutside.current = isOutside(event);
      }}
      onClick={(event) => {
        if (pressedOutside.current && isOutside(event)) close();
        pressedOutside.current = false;
      }}
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <div className="modal-heading">
        <h2 id={titleId}>{title}</h2>
        <button onClick={close} aria-label="閉じる">
          ×
        </button>
      </div>
      {children}
    </dialog>
  );
}
function ProjectEditor({
  project,
  changed,
  error,
}: {
  project: Project;
  changed: () => void;
  error: (s: string) => void;
}) {
  const [nameEdit, setName] = useState<{ base: string; value: string } | null>(
      null,
    ),
    [colorEdit, setColor] = useState<{ base: string; value: string } | null>(
      null,
    );
  // Discard only a draft whose own saved field changed; ordinary reloads and
  // updates to the other field must leave in-progress edits intact.
  if (nameEdit && nameEdit.base !== project.name) setName(null);
  if (colorEdit && colorEdit.base !== project.color) setColor(null);
  const name = nameEdit?.base === project.name ? nameEdit.value : project.name;
  const color =
    colorEdit?.base === project.color ? colorEdit.value : project.color;
  return (
    <form
      className="project-editor"
      onSubmit={(e) => {
        e.preventDefault();
        void api
          .project(project.id, name, color)
          .then(changed)
          .catch((e) => error(String(e)));
      }}
    >
      <input
        aria-label={`${project.name} の表示名`}
        value={name}
        maxLength={120}
        onChange={(e) => setName({ base: project.name, value: e.target.value })}
      />
      <input
        aria-label={`${project.name} の色`}
        type="color"
        value={color}
        onChange={(e) =>
          setColor({ base: project.color, value: e.target.value })
        }
      />
      <button>更新</button>
    </form>
  );
}
export default function App({ initialDate }: { initialDate?: string }) {
  const [config, setConfig] = useState<Config | null>(null),
    [draft, setDraft] = useState<Config | null>(null),
    [data, setData] = useState(empty),
    [dbPath, setDbPath] = useState("");
  const [view, setView] = useState<CalendarView>("week"),
    [anchor, setAnchor] = useState(initialDate ?? DateTime.now().toISODate()!),
    [hidden, setHidden] = useState<Set<string>>(new Set());
  const {
    viewportRef: timelineViewportRef,
    scale: dayScale,
    reset: resetTimelineZoom,
  } = useTimelineZoom(view === "day");
  const timelineScale = view === "day" ? dayScale : 1;
  const [loading, setLoading] = useState(true),
    [syncing, setSyncing] = useState(false),
    [saving, setSaving] = useState(false),
    [settingsError, setSettingsError] = useState(""),
    [settingsWarning, setSettingsWarning] = useState<string[]>([]),
    [error, setError] = useState(""),
    [progress, setProgress] = useState(""),
    [notice, setNotice] = useState("");
  const [startupPhase, setStartupPhase] = useState<"checking" | "migrating">(
    "checking",
  );
  const [bootAttempt, setBootAttempt] = useState(0);
  const [firstRun, setFirstRun] = useState(false);
  const [copyStatus, setCopyStatus] = useState("");
  const [copyFailed, setCopyFailed] = useState(false);
  const [bootNotices, setBootNotices] = useState<string[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [watchWarning, setWatchWarning] = useState("");
  const timezoneChoices = useMemo(() => {
    try {
      return ["UTC", ...Intl.supportedValuesOf("timeZone")];
    } catch {
      return ["UTC", "Asia/Tokyo", "Europe/London", "America/New_York"];
    }
  }, []);
  const [selected, setSelected] = useState<Turn | null>(null),
    [selectedDay, setSelectedDay] = useState<string | null>(null),
    [showSettings, setShowSettings] = useState(false),
    [showDiagnostics, setShowDiagnostics] = useState(false),
    [excerpt, setExcerpt] = useState("");
  const [metric, setMetric] = useState<"elapsed_ms" | "total_ms">("elapsed_ms"),
    [now, setNow] = useState(() => Date.now());
  const zone = config?.timezone ?? "UTC";
  const zoneRef = useRef(zone);
  useEffect(() => {
    zoneRef.current = zone;
  }, [zone]);
  const generation = useRef(0);
  const reload = useCallback(async (timezone: string) => {
    const n = ++generation.current;
    const d = await api.snapshot(timezone);
    if (n === generation.current) setData(d);
  }, []);
  const synchronize = useCallback(async () => {
    setSyncing(true);
    setProgress("ログを走査中");
    try {
      const result = await api.sync();
      await reload(zoneRef.current);
      setProgress(
        `${result.files} ファイル確認 · ${Math.round(result.bytes_read / 1024)} KB 読み込み`,
      );
      setWarnings(result.warnings);
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(false);
    }
  }, [reload]);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const initialize = async () => {
      try {
        const unsubscribe = await api.events(
          () => {
            if (alive)
              void reload(zoneRef.current).catch((e) => setError(String(e)));
          },
          (value) => {
            if (alive) setProgress(value);
          },
          (value) => {
            if (alive) setError(value);
          },
          (phase) => {
            if (alive) setStartupPhase(phase);
          },
          (messages) => {
            if (alive) setWarnings(messages);
          },
          (message) => {
            if (alive) setWatchWarning(message);
          },
        );
        if (!alive) {
          unsubscribe();
          return;
        }
        off = unsubscribe;
        // Paint the startup screen before starting the database worker.
        await new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        );
        if (!alive) return;
        const b = await api.boot();
        if (!alive) return;
        setConfig(b.config);
        setFirstRun(b.first_run);
        setBootNotices(b.notices);
        zoneRef.current = b.config.timezone;
        setDbPath(b.db_path);
        setNotice(
          `解析対象: Codex ${b.config.log_root}${b.config.claude_enabled ? ` / Claude Code ${b.config.claude_root}` : ""}${b.config.rei_enabled ? ` / れい ${b.config.rei_root}` : ""}  ·  保存先: ${b.db_path}`,
        );
        await reload(b.config.timezone);
        if (alive) {
          setLoading(false);
          await synchronize();
        }
      } catch (e) {
        if (alive) {
          setError(String(e));
          setLoading(false);
        }
      }
    };
    void initialize();
    const timer = setInterval(() => setNow(Date.now()), 30000);
    return () => {
      alive = false;
      off?.();
      clearInterval(timer);
    };
  }, [reload, synchronize, bootAttempt]);
  useEffect(() => {
    document.documentElement.dataset.theme = config?.theme ?? "system";
  }, [config?.theme]);
  const days = useMemo(
    () => daysInView(anchor, view, config?.week_start ?? 1),
    [anchor, view, config?.week_start],
  );
  const visibleDays =
    view === "month"
      ? days.filter((d) => d.slice(0, 7) === anchor.slice(0, 7))
      : days;
  const filtered = useMemo(
    () => data.turns.filter((t) => !hidden.has(t.project_id)),
    [data.turns, hidden],
  );
  const activity = useMemo(
    () => indexActivity(filtered, zone),
    [filtered, zone],
  );
  const summary = useMemo(
    () => summarizeDays(activity, visibleDays),
    [activity, visibleDays],
  );
  const segments = useMemo(
    () => days.flatMap((d) => activity.get(d)?.segments ?? []),
    [activity, days],
  );
  const [grouping, setGrouping] = useState<GroupingMode>("group");
  const groups = useMemo(
    () => (view === "month" ? [] : groupActivity(segments, grouping)),
    [segments, grouping, view],
  );
  const [selectedGroup, setSelectedGroup] = useState<ActivityGroup | null>(
    null,
  );
  const [returnGroup, setReturnGroup] = useState<ActivityGroup | null>(null);
  const projects = useMemo(
    () => new Map(data.projects.map((p) => [p.id, p])),
    [data.projects],
  );
  const [isPending, startTransition] = useTransition();
  const [preparingView, setPreparingView] = useState(false);
  const viewFrame = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (viewFrame.current !== null) cancelAnimationFrame(viewFrame.current);
    },
    [],
  );
  const viewBusy = preparingView || isPending;
  const navigate = (nextView: CalendarView, nextAnchor: string) => {
    if (viewFrame.current !== null) cancelAnimationFrame(viewFrame.current);
    setSelectedDay(null);
    setPreparingView(true);
    // Give the busy indicator a painted frame before rendering the next calendar.
    viewFrame.current = requestAnimationFrame(() => {
      viewFrame.current = requestAnimationFrame(() => {
        viewFrame.current = null;
        startTransition(() => {
          setView(nextView);
          setAnchor(nextAnchor);
          setPreparingView(false);
        });
      });
    });
  };
  const changeView = (v: CalendarView) => navigate(v, anchor);
  const move = (n: number) =>
    navigate(
      view,
      DateTime.fromISO(anchor)
        .plus(
          view === "day"
            ? { days: n }
            : view === "week"
              ? { weeks: n }
              : { months: n },
        )
        .toISODate()!,
    );
  const openTurn = (t: Turn, fromGroup: ActivityGroup | null = null) => {
    setReturnGroup(fromGroup);
    setSelectedGroup(null);
    setSelected(t);
    setSelectedDay(null);
    setExcerpt("");
  };
  const openGroup = (group: ActivityGroup) => {
    if (group.members.length === 1) {
      openTurn(group.members[0].turn);
      return;
    }
    setSelectedGroup(group);
    setSelected(null);
    setSelectedDay(null);
    setReturnGroup(null);
  };
  const openSettings = () => {
    setDraft(config);
    setSettingsError("");
    setSettingsWarning([]);
    setShowSettings(true);
  };
  const openDiagnostics = () => {
    setCopyStatus("");
    setCopyFailed(false);
    setShowDiagnostics(true);
  };
  const save = async () => {
    if (!draft || saving) return;
    const sourceChanged =
      !config ||
      draft.log_root !== config.log_root ||
      draft.claude_enabled !== config.claude_enabled ||
      (draft.claude_enabled && draft.claude_root !== config.claude_root) ||
      draft.rei_enabled !== config.rei_enabled ||
      (draft.rei_enabled && draft.rei_root !== config.rei_root);
    setSettingsError("");
    setSettingsWarning([]);
    setSaving(true);
    let warnings: string[];
    try {
      warnings = await api.save(draft);
    } catch (e) {
      setSettingsError(String(e));
      return;
    } finally {
      setSaving(false);
    }
    setConfig(draft);
    setNotice(
      `解析対象: Codex ${draft.log_root}${draft.claude_enabled ? ` / Claude Code ${draft.claude_root}` : ""}${draft.rei_enabled ? ` / れい ${draft.rei_root}` : ""}  ·  保存先: ${dbPath}`,
    );
    zoneRef.current = draft.timezone;
    setSettingsWarning(warnings);
    if (warnings.length === 0) setShowSettings(false);
    try {
      await reload(draft.timezone);
      if (sourceChanged) await synchronize();
    } catch (e) {
      if (warnings.length) setSettingsError(String(e));
      else setError(String(e));
    }
  };
  const dateText = (ms: number) =>
    localDate(ms, zone).toFormat("yyyy/MM/dd HH:mm:ss ZZZZ");
  const diagnosticSummary = `表示 ${data.diagnostics.length} 件 / 全 ${data.diagnostics_total} 件`;
  const omittedDiagnostics = Math.max(
    0,
    data.diagnostics_total - data.diagnostics.length,
  );
  const diagnosticText = `${diagnosticSummary}${omittedDiagnostics ? `（他 ${omittedDiagnostics} 件は省略）` : ""}\n\n${data.diagnostics.join("\n\n")}`;
  const dayTurns = selectedDay
    ? (activity.get(selectedDay)?.segments.map((s) => s.turn) ?? [])
    : [];
  const daySummary = selectedDay
    ? summarizeDays(activity, [selectedDay])
    : null;
  if (!config) {
    return (
      <div className="startup-screen" aria-busy={loading}>
        <h1>CodeCal</h1>
        {loading ? (
          <div className="startup-status" role="status">
            <span className="spinner" aria-hidden="true" />
            <p>
              {startupPhase === "migrating"
                ? "CodeCal がデータベースを移行しています…"
                : "CodeCal がデータベースを確認しています…"}
            </p>
          </div>
        ) : (
          <>
            <p role="alert">
              CodeCal がデータベースを準備できませんでした。{error}
            </p>
            <button
              onClick={() => {
                setError("");
                setLoading(true);
                setStartupPhase("checking");
                setBootAttempt((n) => n + 1);
              }}
            >
              再試行
            </button>
          </>
        )}
      </div>
    );
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-icon">▦</span>CodeCal
          <span className="version">v{__APP_VERSION__}</span>
        </div>
        <p className="eyebrow">YOUR WORK RHYTHM</p>
        <p className="sidebar-intro">
          作業の時間を、
          <br />
          見えるかたちに。
        </p>
        <div className="sidebar-label">
          PROJECTS <span>{data.projects.length}</span>
        </div>
        <div className="project-list">
          {data.projects.map((p) => (
            <label key={p.id} className="project-filter">
              <input
                type="checkbox"
                checked={!hidden.has(p.id)}
                aria-label={`${p.name} を表示`}
                onChange={() =>
                  setHidden((old) => {
                    const next = new Set(old);
                    if (next.has(p.id)) next.delete(p.id);
                    else next.add(p.id);
                    return next;
                  })
                }
              />
              <span className="dot" style={{ background: p.color }} />
              <span className="project-name" title={p.cwd}>
                {p.name}
              </span>
              <small>{formatDuration(summary.projects[p.id] ?? 0)}</small>
            </label>
          ))}
        </div>
        <div className="sidebar-bottom">
          <span className="privacy-dot" /> ローカルで完結
          <p>
            ログはこの端末から
            <br />
            送信されません。
          </p>
          <button
            className="settings-button"
            aria-label="設定"
            onClick={openSettings}
            disabled={!config}
          >
            ⚙ 設定
          </button>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <div>
            <span className="eyebrow">ACTIVITY CALENDAR</span>
            <h1>作業アクティビティ</h1>
          </div>
          <div className="top-actions">
            <span
              className={`sync-state ${syncing ? "busy" : ""}`}
              role="status"
            >
              {loading
                ? "読み込み中"
                : syncing
                  ? "同期中"
                  : error || warnings.length > 0 || watchWarning
                    ? "要確認"
                    : "同期済み"}
            </span>
            <button
              onClick={() => void synchronize()}
              disabled={syncing || !config}
            >
              ↻ 同期
            </button>
            <div className="view-switch" aria-label="カレンダーの表示期間">
              <button
                aria-pressed={view === "day"}
                className={view === "day" ? "active" : ""}
                onClick={() => changeView("day")}
              >
                日
              </button>
              <button
                aria-pressed={view === "week"}
                className={view === "week" ? "active" : ""}
                onClick={() => changeView("week")}
              >
                週
              </button>
              <button
                aria-pressed={view === "month"}
                className={view === "month" ? "active" : ""}
                onClick={() => changeView("month")}
              >
                月
              </button>
            </div>
          </div>
        </header>
        {bootNotices.length > 0 && (
          <div className="setup-notice" role="status">
            {bootNotices.map((message) => (
              <p key={message}>{message}</p>
            ))}
            <button onClick={openSettings}>設定を確認</button>
          </div>
        )}
        {firstRun && config && (
          <section className="setup-notice" aria-label="はじめての設定">
            <strong>
              はじめに、取り込み先とタイムゾーンを確認してください。
            </strong>
            <p>
              Codex: {config.log_root} · タイムゾーン: {config.timezone}
              {config.claude_enabled && ` · Claude Code: ${config.claude_root}`}
              {config.rei_enabled && ` · れい: ${config.rei_root}`}
            </p>
            <p>
              ログの保存先が異なる場合は設定で変更できます。元のログは変更しません。
            </p>
            {config.claude_enabled && (
              <p>
                Claude Code の履歴取り込みが有効です。設定で無効にできます。
              </p>
            )}
            {config.rei_enabled && (
              <p>
                れいの履歴取り込みが有効です。不要な場合は設定で無効にできます。
              </p>
            )}
            <button onClick={openSettings}>初期設定を確認</button>
            <button onClick={() => setFirstRun(false)}>案内を閉じる</button>
          </section>
        )}
        {(warnings.length > 0 || watchWarning) && (
          <section
            role="status"
            className="warning-banner"
            aria-label="同期の警告"
          >
            <div>
              <strong>取り込みの一部に確認が必要です</strong>
              <ul>
                {[
                  ...new Set([
                    ...warnings,
                    ...(watchWarning ? [watchWarning] : []),
                  ]),
                ].map((message) => (
                  <li key={message}>{message}</li>
                ))}
              </ul>
            </div>
            <button onClick={openDiagnostics}>診断を見る</button>
            <button
              aria-label="警告を閉じる"
              onClick={() => {
                setWarnings([]);
                setWatchWarning("");
              }}
            >
              ×
            </button>
          </section>
        )}
        {error && (
          <div role="alert" className="error-banner">
            {error}
            <button onClick={openSettings} disabled={!config}>
              設定を開く
            </button>
            <button onClick={openDiagnostics}>診断を見る</button>
            <button aria-label="エラーを閉じる" onClick={() => setError("")}>
              ×
            </button>
          </div>
        )}
        <section className="summary-cards" aria-label="表示期間の集計">
          <div>
            <span>実稼働時間帯</span>
            <strong data-testid="total-elapsed">
              {formatDuration(summary.elapsed_ms)}
            </strong>
            <small>重複を除いた時間</small>
          </div>
          <div>
            <span>延べ稼働時間</span>
            <strong>{formatDuration(summary.total_ms)}</strong>
            <small>すべてのターンの合計</small>
          </div>
          <div>
            <span>アクティブプロジェクト</span>
            <strong>
              {Object.keys(summary.projects).length}
              <em> projects</em>
            </strong>
            <small>表示期間内のプロジェクト</small>
          </div>
          <div>
            <span>実行ターン</span>
            <strong>
              {summary.count}
              <em> turns</em>
            </strong>
            <small>終了不明のターンも含む</small>
          </div>
        </section>
        <main className="calendar-card" aria-busy={viewBusy}>
          {viewBusy && (
            <div
              className="view-loading"
              role="status"
              aria-label="カレンダーを切り替え中"
            >
              <span className="spinner" aria-hidden="true" />
              <span>カレンダーを切り替え中…</span>
            </div>
          )}
          <div className="calendar-toolbar">
            <div className="period">
              <h2>
                {DateTime.fromISO(anchor).toFormat(
                  view === "day" ? "yyyy年 M月 d日" : "yyyy年 M月",
                )}
              </h2>
              <span>
                {view === "week"
                  ? `${DateTime.fromISO(days[0]).toFormat("M/d")} – ${DateTime.fromISO(days[6]).toFormat("M/d")}`
                  : view === "day"
                    ? "DAILY ACTIVITY"
                    : "MONTHLY ACTIVITY"}
              </span>
            </div>
            <div className="date-controls">
              <button aria-label="前へ" onClick={() => move(-1)}>
                ‹
              </button>
              <button aria-label="次へ" onClick={() => move(1)}>
                ›
              </button>
              <button
                onClick={() =>
                  navigate(view, localDate(Date.now(), zone).toISODate()!)
                }
              >
                {view === "day" ? "今日" : view === "week" ? "今週" : "今月"}
              </button>
              <input
                aria-label="表示日"
                type={view !== "month" ? "date" : "month"}
                value={view !== "month" ? anchor : anchor.slice(0, 7)}
                onChange={(e) => {
                  if (e.target.value)
                    navigate(
                      view,
                      view !== "month"
                        ? e.target.value
                        : e.target.value + "-01",
                    );
                }}
              />
            </div>
          </div>
          {!loading && !syncing && summary.count === 0 && (
            <div className="empty-state">
              {data.turns.length === 0 ? (
                <>
                  <p>取り込まれた稼働履歴がありません</p>
                  <p>ログディレクトリと診断を確認してください。</p>
                  <button onClick={openSettings} disabled={!config}>
                    設定を開く
                  </button>
                  <button onClick={openDiagnostics}>診断を見る</button>
                </>
              ) : filtered.length === 0 ? (
                <>
                  <p>表示するプロジェクトが選択されていません</p>
                  <button onClick={() => setHidden(new Set())}>
                    すべてのプロジェクトを表示
                  </button>
                </>
              ) : (
                <p>表示期間の稼働はありません</p>
              )}
            </div>
          )}
          {view !== "month" ? (
            <>
              <div className="timeline-controls">
                <label>
                  表示単位{" "}
                  <select
                    aria-label="作業ブロックの表示単位"
                    value={grouping}
                    onChange={(e) =>
                      startTransition(() =>
                        setGrouping(e.target.value as GroupingMode),
                      )
                    }
                  >
                    <option value="group">まとめ表示</option>
                    <option value="turn">ターン単位</option>
                  </select>
                </label>
                {grouping === "group" && (
                  <span className="grouping-hint">
                    同じプロジェクト・間隔5分以内
                  </span>
                )}
                {view === "day" && (
                  <div className="timeline-zoom-controls">
                    <span>Ctrl＋ホイールで拡大・縮小</span>
                    <span aria-label="タイムラインの倍率">
                      {Math.round(dayScale * 100)}%
                    </span>
                    <button
                      aria-label="タイムラインの倍率をリセット"
                      onClick={resetTimelineZoom}
                      disabled={dayScale === 1}
                    >
                      リセット
                    </button>
                  </div>
                )}
              </div>
              <div
                className="week-head"
                style={{
                  gridTemplateColumns: `60px repeat(${days.length}, minmax(0,1fr))`,
                }}
              >
                <div className="zone-caption" title={zone}>
                  {zone.split("/").at(-1)}
                </div>
                {days.map((d) => (
                  <div
                    key={d}
                    className={
                      d === localDate(now, zone).toISODate() ? "today" : ""
                    }
                  >
                    <span>
                      {DateTime.fromISO(d).setLocale("ja").toFormat("ccc")}
                    </span>
                    <strong>{Number(d.slice(-2))}</strong>
                  </div>
                ))}
              </div>
              <div className="week-scroll" ref={timelineViewportRef}>
                <div
                  className="week-grid"
                  style={
                    {
                      gridTemplateColumns: `60px repeat(${days.length}, minmax(0,1fr))`,
                      height: 1440 * timelineScale + 1,
                      "--timeline-scale": timelineScale,
                    } as CSSProperties
                  }
                >
                  <div className="time-axis">
                    {Array.from({ length: 25 }, (_, h) => (
                      <span key={h} style={{ top: h * 60 * timelineScale }}>
                        {String(h).padStart(2, "0")}:00
                      </span>
                    ))}
                  </div>
                  {days.map((d) => {
                    const midnight = DateTime.fromISO(d, { zone });
                    const next = midnight.plus({ days: 1 });
                    const length = next.toMillis() - midnight.toMillis();
                    const dst = length !== 86400000;
                    return (
                      <div
                        className={`day-column ${dst ? "dst-day" : ""}`}
                        key={d}
                        aria-label={d}
                      >
                        {dst &&
                          Array.from(
                            { length: Math.round(length / 3600000) },
                            (_, h) => (
                              <span
                                className="dst-tick"
                                key={h}
                                style={{
                                  top:
                                    ((h * 3600000) / length) *
                                    1440 *
                                    timelineScale,
                                }}
                              >
                                {midnight
                                  .plus({ hours: h })
                                  .toFormat("HH:mm ZZ")}
                              </span>
                            ),
                          )}
                        {d === localDate(now, zone).toISODate() && (
                          <div
                            className="now-line"
                            style={{
                              top:
                                ((now - midnight.toMillis()) / length) *
                                1440 *
                                timelineScale,
                            }}
                          >
                            <span />
                          </div>
                        )}
                        {packSegments(
                          groups
                            .filter((g) => g.date === d)
                            .map((g) => ({
                              ...g,
                              top: g.top * timelineScale,
                              height: g.unknown
                                ? 22
                                : Math.max(2, g.height * timelineScale),
                            })),
                        ).map((g) => {
                          const p = projects.get(g.project_id);
                          const sources = [
                            ...new Set(
                              g.members.map((m) => sourceLabel(m.turn)),
                            ),
                          ].join(" / ");
                          const multiple = g.members.length > 1;
                          const states = Object.entries(g.statuses)
                            .map(
                              ([status, count]) =>
                                `${statusLabel[status as Turn["status"]]}${multiple ? ` ${count}` : ""}`,
                            )
                            .join(" · ");
                          const times = `${localDate(g.start, zone).toFormat("HH:mm")} – ${g.unknown ? "?" : localDate(g.end!, zone).toFormat(g.end === next.toMillis() ? "'24:00'" : "HH:mm")}`;
                          const details = multiple
                            ? `${g.members.length}ターン · 実稼働 ${formatDuration(g.elapsed_ms)} · ${states}`
                            : states;
                          const prompt =
                            g.members.find((m) => m.turn.prompt)?.turn.prompt ??
                            "";
                          return (
                            <button
                              key={g.id}
                              className={`activity-block ${g.unknown ? "unknown" : ""} ${multiple ? "grouped" : ""}`}
                              data-turn-count={g.members.length}
                              aria-label={`${p?.name} ${times} ${details} · ${sources}`}
                              title={`${p?.name} · ${sources}\n${times}\n${details}\n${prompt.slice(0, 240)}`}
                              style={{
                                top: g.top,
                                height: g.height,
                                left: `calc(${(g.lane / g.lanes) * 100}% + 3px)`,
                                width: `calc(${100 / g.lanes}% - 6px)`,
                                borderColor: p?.color,
                                background: `${p?.color ?? "#14b8a6"}26`,
                              }}
                              onClick={() => openGroup(g)}
                            >
                              <strong>
                                {p?.name ?? "Unknown"}
                                <small> · {sources}</small>
                                {multiple && ` · ${g.members.length}ターン`}
                              </strong>
                              <span>
                                {times}
                                {!multiple && ` · ${states}`}
                              </span>
                              {multiple && g.height > 42 && (
                                <span>
                                  実稼働 {formatDuration(g.elapsed_ms)} ·{" "}
                                  {states}
                                </span>
                              )}
                              {g.height > (multiple ? 85 : 60) && (
                                <p>{prompt.slice(0, 160) || "依頼内容なし"}</p>
                              )}
                            </button>
                          );
                        })}
                      </div>
                    );
                  })}
                </div>
              </div>
            </>
          ) : (
            <div className="month-view">
              <div className="month-intro">
                <p>日々の積み重ねを振り返る</p>
                <label>
                  集計{" "}
                  <select
                    aria-label="月の集計方式"
                    value={metric}
                    onChange={(e) => setMetric(e.target.value as typeof metric)}
                  >
                    <option value="elapsed_ms">実稼働時間帯</option>
                    <option value="total_ms">延べ稼働時間</option>
                  </select>
                </label>
              </div>
              <div className="heatmap-with-labels">
                <div className="weekday-labels">
                  {Array.from({ length: 7 }, (_, i) => (
                    <span key={i}>
                      {
                        ["日", "月", "火", "水", "木", "金", "土"][
                          ((config?.week_start ?? 1) + i) % 7
                        ]
                      }
                    </span>
                  ))}
                </div>
                <div
                  className="heatmap"
                  style={{
                    gridTemplateRows: `repeat(${days.length / 7}, clamp(48px, calc((100vh - 610px) / ${days.length / 7}), 90px))`,
                  }}
                >
                  {days.map((d) => {
                    const sum = summarizeDays(activity, [d]);
                    const ms = sum[metric];
                    const level = heatLevel(
                      ms,
                      config?.thresholds ?? [1, 3, 6],
                    );
                    return (
                      <button
                        key={d}
                        data-level={level}
                        className={
                          d.slice(0, 7) !== anchor.slice(0, 7) ? "outside" : ""
                        }
                        aria-label={`${d} ${formatDuration(ms)}`}
                        onClick={() => setSelectedDay(d)}
                      >
                        <strong>{Number(d.slice(-2))}</strong>
                        <span>{formatDuration(ms)}</span>
                        {sum.count > 0 && <small>{sum.count} turns</small>}
                      </button>
                    );
                  })}
                </div>
              </div>
              <div className="heat-legend">
                <span>少ない</span>
                {[0, 1, 2, 3, 4].map((l) => (
                  <i key={l} data-level={l} />
                ))}
                <span>多い</span>
                <small>
                  固定しきい値 · {config?.thresholds.slice(0, 3).join(" / ")}{" "}
                  時間
                </small>
              </div>
            </div>
          )}
        </main>
        <footer>
          <span
            title={`${zone} · 現在の時差 UTC${DateTime.now().setZone(zone).toFormat("ZZ")} · ${notice}`}
          >
            {zone} · {notice || "ローカルの実行履歴を読み取り専用で解析します"}
          </span>
          <button onClick={openDiagnostics}>
            診断{" "}
            {data.diagnostics_total > 0 ? `(${data.diagnostics_total})` : ""}
          </button>
        </footer>
        <div className="sync-progress" aria-live="polite">
          {progress}
        </div>
      </div>
      {selectedGroup && (
        <Modal
          title={`${projects.get(selectedGroup.project_id)?.name ?? "Unknown"} の作業のまとまり`}
          close={() => setSelectedGroup(null)}
        >
          <dl>
            <dt>作業ディレクトリ</dt>
            <dd>{projects.get(selectedGroup.project_id)?.cwd}</dd>
            <dt>表示範囲</dt>
            <dd>
              {dateText(selectedGroup.start)} 〜{" "}
              {selectedGroup.end === null
                ? "終了不明"
                : dateText(selectedGroup.end)}
            </dd>
            <dt>実稼働時間帯</dt>
            <dd>{formatDuration(selectedGroup.elapsed_ms)}</dd>
            <dt>延べ稼働時間</dt>
            <dd>{formatDuration(selectedGroup.total_ms)}</dd>
            <dt>ターン数</dt>
            <dd>{selectedGroup.members.length}ターン</dd>
            <dt>実行状態</dt>
            <dd>
              {Object.entries(selectedGroup.statuses)
                .map(
                  ([status, count]) =>
                    `${statusLabel[status as Turn["status"]]} ${count}`,
                )
                .join(" · ")}
            </dd>
          </dl>
          <p className="hint">
            表示範囲内の空白は、実稼働時間・延べ稼働時間には含めていません。
          </p>
          <h3>元のターン</h3>
          <div className="day-turns group-turns">
            {selectedGroup.members.map((m) => (
              <button
                key={m.turn.id}
                onClick={() => openTurn(m.turn, selectedGroup)}
              >
                <span>
                  {localDate(m.start, zone).toFormat("HH:mm")} 〜{" "}
                  {m.unknown
                    ? "終了不明"
                    : localDate(m.end, zone).toFormat("HH:mm")}{" "}
                  · {statusLabel[m.turn.status]}
                </span>
                <small>{sourceLabel(m.turn)}</small>
                <strong>{m.turn.prompt || "依頼内容なし"}</strong>
              </button>
            ))}
          </div>
        </Modal>
      )}
      {selected && (
        <Modal
          title={projects.get(selected.project_id)?.name ?? "作業詳細"}
          close={() => {
            setSelected(null);
            setReturnGroup(null);
          }}
        >
          {returnGroup && (
            <button
              className="group-back"
              onClick={() => {
                setSelected(null);
                setSelectedGroup(returnGroup);
                setReturnGroup(null);
              }}
            >
              作業のまとまりに戻る
            </button>
          )}
          <span className="status-pill">{statusLabel[selected.status]}</span>
          <h3>
            {sourceLabel(selected)}{" "}
            {selected.source === "claude" ? "の活動内容" : "への依頼"}
          </h3>
          <pre className="prompt">
            {selected.prompt || "依頼内容を取得できません"}
          </pre>
          <h3>作業情報</h3>
          <dl>
            {selected.details?.git_branch && (
              <>
                <dt>Git ブランチ</dt>
                <dd>{selected.details.git_branch}</dd>
              </>
            )}
            {selected.details?.parent_session_id && (
              <>
                <dt>親セッション</dt>
                <dd>{selected.details.parent_session_id}</dd>
              </>
            )}
            <dt>取得元</dt>
            <dd>{sourceLabel(selected)}</dd>
            <dt>作業ディレクトリ</dt>
            <dd>{projects.get(selected.project_id)?.cwd}</dd>
            <dt>開始日時</dt>
            <dd>{dateText(selected.start)}</dd>
            <dt>終了日時</dt>
            <dd>
              {selected.end === null
                ? "不明（稼働時間の集計対象外）"
                : dateText(selected.end)}
            </dd>
            <dt>稼働時間</dt>
            <dd>
              {selected.end === null
                ? "未確定"
                : formatDuration(selected.end - selected.start)}
            </dd>
            <dt>セッション ID</dt>
            <dd>
              {selected.source === "rei"
                ? (selected.native_session_id ?? "記録なし")
                : selected.session_id}
            </dd>
            <dt>{selected.source === "rei" ? "実行 ID" : "ターン ID"}</dt>
            <dd>{selected.turn_id}</dd>
            {selected.source === "rei" && selected.native_turn_id && (
              <>
                <dt>ターン ID</dt>
                <dd>{selected.native_turn_id}</dd>
              </>
            )}
            <dt>トークン使用量</dt>
            <dd>{selected.tokens?.toLocaleString() ?? "取得できません"}</dd>
          </dl>
          <button
            onClick={() =>
              void api
                .excerpt(selected.id)
                .then(setExcerpt)
                .catch((e) => setError(String(e)))
            }
          >
            ログの該当箇所を表示
          </button>
          {excerpt && (
            <>
              <p className="hint">
                ログ抜粋には依頼文やローカルのパスなどの機密情報が含まれ得ます。共有前に内容を確認してください。
              </p>
              <pre className="log-excerpt">{excerpt}</pre>
            </>
          )}
        </Modal>
      )}
      {selectedDay && daySummary && (
        <Modal
          title={`${selectedDay} のアクティビティ`}
          close={() => setSelectedDay(null)}
        >
          <div className="day-stats">
            <p>
              実稼働時間帯{" "}
              <strong>{formatDuration(daySummary.elapsed_ms)}</strong>
            </p>
            <p>
              延べ稼働時間{" "}
              <strong>{formatDuration(daySummary.total_ms)}</strong>
            </p>
            <p>
              実行ターン数 <strong>{daySummary.count}</strong>
            </p>
          </div>
          {Object.entries(daySummary.projects).map(([id, ms]) => (
            <p key={id}>
              {projects.get(id)?.name} · {formatDuration(ms)}
            </p>
          ))}
          <div className="day-turns">
            {dayTurns.map((t) => (
              <button key={t.id} onClick={() => openTurn(t)}>
                <span>
                  {projects.get(t.project_id)?.name} · {statusLabel[t.status]}
                </span>
                <small>{sourceLabel(t)}</small>
                <strong>{t.prompt || "依頼内容なし"}</strong>
              </button>
            ))}
          </div>
          <button
            className="primary"
            onClick={() => {
              navigate("week", selectedDay);
            }}
          >
            週表示で開く
          </button>
          <button
            className="primary"
            onClick={() => navigate("day", selectedDay)}
          >
            1日表示で開く
          </button>
        </Modal>
      )}
      {showSettings && draft && (
        <Modal title="設定" close={() => setShowSettings(false)}>
          {settingsError && <p role="alert">{settingsError}</p>}
          {settingsWarning.length > 0 && (
            <div role="alert">
              <p>設定は保存しましたが、取り込み先を確認してください。</p>
              <ul>
                {settingsWarning.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
              <p>
                パスを修正するか、ディレクトリを用意してから同期してください。
              </p>
            </div>
          )}
          <form
            className="settings-form"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <label>
              Codex ログディレクトリ
              <input
                value={draft.log_root}
                onChange={(e) =>
                  setDraft({ ...draft, log_root: e.target.value })
                }
              />
            </label>
            <p className="hint">
              sessions と archived_sessions を含む Codex
              ホームを指定してください。保存先: {dbPath}
            </p>
            <label className="inline-check">
              <input
                type="checkbox"
                checked={draft.claude_enabled ?? false}
                onChange={(e) =>
                  setDraft({ ...draft, claude_enabled: e.target.checked })
                }
              />
              Claude Code の履歴を取り込む
            </label>
            <label>
              Claude Code projects ディレクトリ
              <input
                value={draft.claude_root ?? ""}
                disabled={!draft.claude_enabled}
                onChange={(e) =>
                  setDraft({ ...draft, claude_root: e.target.value })
                }
              />
            </label>
            <p className="hint">
              ~/.claude/projects または CLAUDE_CONFIG_DIR 配下の projects
              を指定します。会話本文・ツール入出力は保存しません。
            </p>
            <label className="inline-check">
              <input
                type="checkbox"
                checked={draft.rei_enabled}
                onChange={(e) =>
                  setDraft({ ...draft, rei_enabled: e.target.checked })
                }
              />
              れいの実行履歴を取り込む
            </label>
            <label>
              れい データディレクトリ
              <input
                value={draft.rei_root}
                disabled={!draft.rei_enabled}
                onChange={(e) =>
                  setDraft({ ...draft, rei_root: e.target.value })
                }
              />
            </label>
            <p className="hint">
              storage.db または projects
              を含むディレクトリを指定してください。開発以外の実行も取り込みます。
            </p>
            <label>
              タイムゾーン
              <input
                value={draft.timezone}
                placeholder="Asia/Tokyo"
                title="候補にないIANAタイムゾーン名も直接入力できます"
                list="timezone-choices"
                aria-describedby="timezone-preview"
                onChange={(e) =>
                  setDraft({ ...draft, timezone: e.target.value })
                }
              />
            </label>
            <datalist id="timezone-choices">
              {Array.from(new Set([draft.timezone, ...timezoneChoices])).map(
                (value) => (
                  <option key={value} value={value} />
                ),
              )}
            </datalist>
            <p className="hint" id="timezone-preview">
              {DateTime.now().setZone(draft.timezone).isValid
                ? `${draft.timezone} · 現在の時差 UTC${DateTime.now().setZone(draft.timezone).toFormat("ZZ")}`
                : "タイムゾーンを確認してください（例: Asia/Tokyo、UTC）"}
            </p>
            <label>
              週の開始曜日
              <select
                value={draft.week_start}
                onChange={(e) =>
                  setDraft({ ...draft, week_start: Number(e.target.value) })
                }
              >
                <option value={1}>月曜日</option>
                <option value={0}>日曜日</option>
              </select>
            </label>
            <label>
              テーマ
              <select
                value={draft.theme}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    theme: e.target.value as Config["theme"],
                  })
                }
              >
                <option value="system">OS に合わせる</option>
                <option value="light">ライト</option>
                <option value="dark">ダーク</option>
              </select>
            </label>
            <fieldset>
              <legend>ヒートマップのしきい値（時間・昇順）</legend>
              <div className="thresholds">
                {draft.thresholds.map((v, i) => (
                  <input
                    key={i}
                    aria-label={`しきい値 ${i + 1}`}
                    type="number"
                    min="0.1"
                    step="0.1"
                    value={v}
                    onChange={(e) => {
                      const thresholds = [
                        ...draft.thresholds,
                      ] as Config["thresholds"];
                      thresholds[i] = Number(e.target.value);
                      setDraft({ ...draft, thresholds });
                    }}
                  />
                ))}
              </div>
            </fieldset>
            <label className="inline-check">
              <input
                type="checkbox"
                checked={draft.auto_refresh}
                onChange={(e) =>
                  setDraft({ ...draft, auto_refresh: e.target.checked })
                }
              />
              自動更新を有効にする
            </label>
            <button className="primary" disabled={saving}>
              {saving ? "保存中…" : "設定を保存"}
            </button>
          </form>
          <h3>プロジェクトの表示</h3>
          {data.projects.map((p) => (
            <ProjectEditor
              key={p.id}
              project={p}
              changed={() => void reload(zone)}
              error={setSettingsError}
            />
          ))}
        </Modal>
      )}
      {showDiagnostics && (
        <Modal title="取り込み診断" close={() => setShowDiagnostics(false)}>
          <p>
            元のログは変更していません。解析不要のイベントは読み飛ばします。
          </p>
          <p>
            {diagnosticSummary}
            {omittedDiagnostics > 0 && `（他 ${omittedDiagnostics} 件は省略）`}
          </p>
          <p className="hint">
            診断にはローカルのパスが含まれます。共有前に内容を確認してください。
          </p>
          <button
            disabled={data.diagnostics.length === 0}
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(diagnosticText);
                setCopyFailed(false);
                setCopyStatus("表示中の診断をコピーしました。");
              } catch {
                setCopyFailed(true);
                setCopyStatus(
                  "コピーできませんでした。下のテキストを選択してコピーしてください。",
                );
              }
            }}
          >
            診断をコピー
          </button>
          {copyStatus && <p role="status">{copyStatus}</p>}
          {copyFailed && (
            <textarea
              className="diagnostic-copy"
              aria-label="コピー用の診断"
              readOnly
              value={diagnosticText}
              onFocus={(e) => e.currentTarget.select()}
            />
          )}
          {data.diagnostics.length ? (
            data.diagnostics.map((s, i) => (
              <pre className="diagnostic" key={i}>
                {s}
              </pre>
            ))
          ) : (
            <p>診断情報はありません。</p>
          )}
        </Modal>
      )}
    </div>
  );
}
