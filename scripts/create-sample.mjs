import process from "node:process";
import console from "node:console";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
const root = path.resolve(process.argv[2] ?? ".local/sample-codex");
const line = (type, payload, timestamp) =>
  JSON.stringify({ type, payload, timestamp }) + "\n";
mkdirSync(path.join(root, "sessions"), { recursive: true });
let created = 0;
let skipped = 0;
for (const [id, cwd, start, end, prompt] of [
  [
    "calendar",
    "C:/sample/CodeCal",
    "2026-11-20T01:00:00Z",
    "2026-11-20T03:00:00Z",
    "週カレンダーの表示を実装する",
  ],
  [
    "editor",
    "C:/sample/Editor",
    "2026-11-20T02:00:00Z",
    "2026-11-20T04:00:00Z",
    "検索パネルのテストを追加する",
  ],
  [
    "overnight",
    "C:/sample/CodeCal",
    "2026-11-20T14:00:00Z",
    "2026-11-20T17:00:00Z",
    "日付をまたぐビルドを検証する",
  ],
]) {
  const data =
    line("session_meta", { id, cwd }, start) +
    line("event_msg", { type: "task_started", turn_id: id + "-turn" }, start) +
    line("event_msg", { type: "user_message", message: prompt }, start) +
    line("event_msg", { type: "task_complete", turn_id: id + "-turn" }, end);
  try {
    writeFileSync(path.join(root, "sessions", id + ".jsonl"), data, {
      flag: "wx",
    });
    created++;
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    skipped++;
  }
}
console.log(
  `合成ログ: ${root} (2026-11-20 / Asia/Tokyo) · 作成 ${created} 件 · 既存のためスキップ ${skipped} 件`,
);
