import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
  type ReactNode,
} from "react";
import { DateTime } from "luxon";
import { api } from "./api";
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
const empty: Snapshot = { projects: [], turns: [], days: [], diagnostics: [] };
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
  useEffect(() => {
    const d = ref.current;
    if (d?.showModal) d.showModal();
    else d?.setAttribute("open", "");
  }, []);
  return (
    <dialog
      ref={ref}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <div className="modal-heading">
        <h2>{title}</h2>
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
  const [name, setName] = useState(project.name),
    [color, setColor] = useState(project.color);
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
        onChange={(e) => setName(e.target.value)}
      />
      <input
        aria-label={`${project.name} の色`}
        type="color"
        value={color}
        onChange={(e) => setColor(e.target.value)}
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
  const [loading, setLoading] = useState(true),
    [syncing, setSyncing] = useState(false),
    [error, setError] = useState(""),
    [progress, setProgress] = useState(""),
    [notice, setNotice] = useState("");
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
    void api
      .boot()
      .then(async (b) => {
        if (!alive) return;
        setConfig(b.config);
        zoneRef.current = b.config.timezone;
        setDbPath(b.db_path);
        setNotice(`解析対象: ${b.config.log_root}  ·  保存先: ${b.db_path}`);
        await reload(b.config.timezone);
        if (alive) {
          setLoading(false);
          await synchronize();
        }
      })
      .catch((e) => {
        if (alive) {
          setError(String(e));
          setLoading(false);
        }
      });
    void api
      .events(
        () => {
          void reload(zoneRef.current).catch((e) => setError(String(e)));
        },
        setProgress,
        setError,
      )
      .then((f) => {
        if (alive) off = f;
        else f();
      })
      .catch(() => {});
    const timer = setInterval(() => setNow(Date.now()), 30000);
    return () => {
      alive = false;
      off?.();
      clearInterval(timer);
    };
  }, [reload, synchronize]);
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
  const openTurn = (t: Turn) => {
    setSelected(t);
    setSelectedDay(null);
    setExcerpt("");
  };
  const save = async () => {
    if (!draft) return;
    try {
      await api.save(draft);
      setConfig(draft);
      zoneRef.current = draft.timezone;
      setShowSettings(false);
      await reload(draft.timezone);
      await synchronize();
    } catch (e) {
      setError(String(e));
    }
  };
  const dateText = (ms: number) =>
    localDate(ms, zone).toFormat("yyyy/MM/dd HH:mm:ss ZZZZ");
  const dayTurns = selectedDay
    ? (activity.get(selectedDay)?.segments.map((s) => s.turn) ?? [])
    : [];
  const daySummary = selectedDay
    ? summarizeDays(activity, [selectedDay])
    : null;
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a className="brand" href="#">
          <span className="brand-icon">▦</span>CodeCal
          <span className="version">LOCAL</span>
        </a>
        <p className="eyebrow">YOUR CODING RHYTHM</p>
        <p className="sidebar-intro">
          開発の時間を、
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
            onClick={() => {
              setDraft(config);
              setShowSettings(true);
            }}
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
            <h1>開発アクティビティ</h1>
          </div>
          <div className="top-actions">
            <span
              className={`sync-state ${syncing ? "busy" : ""}`}
              role="status"
            >
              {loading ? "読み込み中" : syncing ? "同期中" : "同期済み"}
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
        {error && (
          <div role="alert" className="error-banner">
            {error}
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
          {!loading && summary.count === 0 && (
            <p className="empty-state">表示期間の稼働はありません</p>
          )}
          {view !== "month" ? (
            <>
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
              <div className="week-scroll">
                <div
                  className="week-grid"
                  style={{
                    gridTemplateColumns: `60px repeat(${days.length}, minmax(0,1fr))`,
                  }}
                >
                  <div className="time-axis">
                    {Array.from({ length: 25 }, (_, h) => (
                      <span key={h} style={{ top: h * 60 }}>
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
                                style={{ top: ((h * 3600000) / length) * 1440 }}
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
                                ((now - midnight.toMillis()) / length) * 1440,
                            }}
                          >
                            <span />
                          </div>
                        )}
                        {packSegments(segments.filter((s) => s.date === d)).map(
                          (s) => {
                            const p = projects.get(s.turn.project_id);
                            const times = `${localDate(s.start, zone).toFormat("HH:mm")} – ${s.unknown ? "?" : localDate(s.end, zone).toFormat(s.end === next.toMillis() ? "'24:00'" : "HH:mm")}`;
                            return (
                              <button
                                key={`${s.turn.id}:${d}`}
                                className={`activity-block ${s.unknown ? "unknown" : ""}`}
                                aria-label={`${p?.name} ${times} ${statusLabel[s.turn.status]}`}
                                title={`${p?.name}\n${times}\n${statusLabel[s.turn.status]}\n${s.turn.prompt}`}
                                style={{
                                  top: s.top,
                                  height: s.height,
                                  left: `calc(${(s.lane / s.lanes) * 100}% + 3px)`,
                                  width: `calc(${100 / s.lanes}% - 6px)`,
                                  borderColor: p?.color,
                                  background: `${p?.color ?? "#14b8a6"}26`,
                                }}
                                onClick={() => openTurn(s.turn)}
                              >
                                <strong>{p?.name ?? "Unknown"}</strong>
                                <span>
                                  {times} · {statusLabel[s.turn.status]}
                                </span>
                                {s.height > 60 && (
                                  <p>{s.turn.prompt || "依頼内容なし"}</p>
                                )}
                              </button>
                            );
                          },
                        )}
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
          <span title={notice}>
            {notice || "ローカルの Codex ログを読み取り専用で解析します"}
          </span>
          <button onClick={() => setShowDiagnostics(true)}>
            診断{" "}
            {data.diagnostics.length > 0 ? `(${data.diagnostics.length})` : ""}
          </button>
        </footer>
        <div className="sync-progress" aria-live="polite">
          {progress}
        </div>
      </div>
      {selected && (
        <Modal
          title={projects.get(selected.project_id)?.name ?? "作業詳細"}
          close={() => setSelected(null)}
        >
          <span className="status-pill">{statusLabel[selected.status]}</span>
          <dl>
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
            <dd>{selected.session_id}</dd>
            <dt>ターン ID</dt>
            <dd>{selected.turn_id}</dd>
            <dt>トークン使用量</dt>
            <dd>{selected.tokens?.toLocaleString() ?? "取得できません"}</dd>
          </dl>
          <h3>Codex への依頼</h3>
          <pre className="prompt">
            {selected.prompt || "依頼内容を取得できません"}
          </pre>
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
          {excerpt && <pre className="log-excerpt">{excerpt}</pre>}
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
            <label>
              タイムゾーン
              <input
                value={draft.timezone}
                placeholder="Asia/Tokyo"
                onChange={(e) =>
                  setDraft({ ...draft, timezone: e.target.value })
                }
              />
            </label>
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
            <button className="primary">設定を保存</button>
          </form>
          <h3>プロジェクトの表示</h3>
          {data.projects.map((p) => (
            <ProjectEditor
              key={p.id}
              project={p}
              changed={() => void reload(zone)}
              error={setError}
            />
          ))}
        </Modal>
      )}
      {showDiagnostics && (
        <Modal title="取り込み診断" close={() => setShowDiagnostics(false)}>
          <p>
            元のログは変更していません。解析不要のイベントは読み飛ばします。
          </p>
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
