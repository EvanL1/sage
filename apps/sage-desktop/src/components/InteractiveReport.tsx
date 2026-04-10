import { useState, useCallback, useMemo, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useLang } from "../LangContext";

// ─── Persistence ────────────────────────────────────────────────────────────

function contentKey(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return "ir_" + (h >>> 0).toString(36);
}

interface Persisted {
  states: Record<string, string>;  // "t0_3" → "confirmed", "i1_2" → "accurate"
  taskIds: Record<string, number>; // "t0_3" → 42
}

function loadPersisted(key: string): Persisted {
  try { const r = localStorage.getItem(key); if (r) return JSON.parse(r); } catch {}
  return { states: {}, taskIds: {} };
}

function savePersisted(key: string, data: Persisted) {
  try { localStorage.setItem(key, JSON.stringify(data)); } catch {}
}

// ─── Types ──────────────────────────────────────────────────────────────────

interface TableRow { cells: string[]; }
interface ParsedTable { headers: string[]; rows: TableRow[]; }
interface ListItem { priority: "high" | "optional"; text: string; }

interface Section {
  title: string;
  tables: ParsedTable[];
  items: ListItem[];
  prose: string[];
}

interface ParsedReport {
  title: string;
  preamble: string;
  sections: Section[];
}

// ─── Parser ─────────────────────────────────────────────────────────────────

const SECTION_RE = /^#{0,3}\s*(\d+)[.．]\s*/;
const CIRCLED_RE = /^[①②③④⑤⑥⑦⑧⑨⑩]/;
const NUMBERED_RE = /^\d+[.．]\s*/;
const BULLET_RE = /^[-*•]\s+/;

function parseReport(md: string): ParsedReport | null {
  const lines = md.split("\n");
  let title = "";
  let preamble = "";
  const sections: Section[] = [];
  let cur: Section | null = null;
  let inTable = false, headerDone = false;
  let curTable: ParsedTable | null = null;
  let inOptional = false;

  const flushTable = () => {
    if (curTable && cur) cur.tables.push(curTable);
    curTable = null; inTable = false; headerDone = false;
  };

  for (const line of lines) {
    if (/^#\s+/.test(line) && !title && !cur) { title = line.replace(/^#\s+/, ""); continue; }

    const secMatch = line.match(SECTION_RE);
    // Only start a new section if: (a) not yet in a section, or (b) line has # prefix,
    // or (c) remaining text is short (title-like). Otherwise treat as a numbered item.
    const hasHashPrefix = /^#{1,3}\s/.test(line);
    const afterNum = secMatch ? line.replace(SECTION_RE, "").trim() : "";
    const looksLikeTitle = afterNum.length < 50 && !/\*\*/.test(afterNum);
    if (secMatch && (!cur || hasHashPrefix || looksLikeTitle)) {
      flushTable();
      cur = { title: line.replace(/^#{0,3}\s*/, ""), tables: [], items: [], prose: [] };
      sections.push(cur); inOptional = false;
      continue;
    }

    if (!cur) { preamble += line + "\n"; continue; }

    if (line.trimStart().startsWith("|")) {
      if (!inTable) {
        inTable = true; headerDone = false;
        curTable = { headers: line.split("|").map(c => c.trim()).filter(Boolean), rows: [] };
        continue;
      }
      if (!headerDone) { headerDone = true; continue; }
      const cells = line.split("|").map(c => c.trim()).filter(Boolean);
      if (curTable && cells.length > 0) curTable.rows.push({ cells });
      continue;
    }
    if (inTable) flushTable();

    const trimmed = line.trim();
    if (!trimmed) continue;
    if (/可选关注|optional/i.test(trimmed)) { inOptional = true; continue; }
    if (/优先级排序|priority|推荐方案/i.test(trimmed)) continue;

    if (CIRCLED_RE.test(trimmed)) {
      const idx = trimmed.match(/^[①②③④⑤⑥⑦⑧⑨⑩]+/)?.[0] || "";
      cur.items.push({ priority: "high", text: trimmed.slice(idx.length).trim() });
      continue;
    }
    const numMatch = trimmed.match(NUMBERED_RE);
    if (numMatch) { cur.items.push({ priority: inOptional ? "optional" : "high", text: trimmed.slice(numMatch[0].length) }); continue; }
    const bulMatch = trimmed.match(BULLET_RE);
    if (bulMatch) { cur.items.push({ priority: inOptional ? "optional" : "high", text: trimmed.slice(bulMatch[0].length) }); continue; }

    // Continuation of last item OR bold-prefixed sub-lines (e.g. **理由：** ...)
    if (cur.items.length > 0 && !trimmed.startsWith("#")) {
      cur.items[cur.items.length - 1].text += "\n" + trimmed;
      continue;
    }
    cur.prose.push(line);
  }
  flushTable();
  if (sections.length < 2) return null;
  return { title, preamble: preamble.trim(), sections };
}

// ─── Interactive Table ──────────────────────────────────────────────────────

function InteractiveTable({ table, reportType, sectionIdx, tableIdx, persisted, onPersist, onCorrect }: {
  table: ParsedTable;
  reportType: string;
  sectionIdx: number;
  tableIdx: number;
  persisted: Persisted;
  onPersist: (p: Persisted) => void;
  onCorrect: (summary: string, correct: string) => void;
}) {
  const { t } = useLang();
  const [corrText, setCorrText] = useState("");
  const [corrIdx, setCorrIdx] = useState<number | null>(null);

  const k = (i: number) => `t${sectionIdx}_${tableIdx}_${i}`;
  const getState = (i: number) => persisted.states[k(i)] || "";
  const getTaskId = (i: number) => persisted.taskIds[k(i)];

  const setState = (i: number, val: string, taskId?: number) => {
    const next = { ...persisted, states: { ...persisted.states, [k(i)]: val }, taskIds: { ...persisted.taskIds } };
    if (taskId !== undefined) next.taskIds[k(i)] = taskId;
    onPersist(next);
  };

  const handleConfirm = async (i: number, row: TableRow) => {
    setState(i, "confirmed");
    try {
      await invoke("save_report_correction", {
        reportType, wrongClaim: row.cells.join(" | "),
        correctFact: "✓", contextHint: "positive_confirm",
      });
    } catch {}
  };

  const startCorrect = (i: number) => { setCorrIdx(i); setCorrText(""); };

  const submitCorrect = (i: number, row: TableRow) => {
    if (corrText.trim().length < 2) return;
    onCorrect(row.cells.join(" | "), corrText.trim());
    setState(i, "corrected");
    setCorrIdx(null); setCorrText("");
  };

  const createTask = async (i: number, row: TableRow) => {
    try {
      const text = row.cells.slice(0, 2).join(": ").replace(/\*\*/g, "");
      const taskId = await invoke<number>("create_task", {
        content: text, source: reportType, sourceId: null,
        priority: "high", dueDate: null, description: row.cells.join(" — "),
      });
      setState(i, "tasked", taskId);
    } catch (err) { console.error("Failed to create task:", err); }
  };

  return (
    <div className="ir-table">
      <div className="ir-table-header">
        {table.headers.map((h, i) => <div key={i} className="ir-table-hcell">{h}</div>)}
        <div className="ir-table-hcell ir-table-hcell-actions"></div>
      </div>
      {table.rows.map((row, i) => {
        const st = getState(i);
        return (
          <div key={i} className={`ir-table-row ${st}`}>
            {row.cells.map((cell, j) => (
              <div key={j} className="ir-table-cell">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ p: ({ children }) => <span>{children}</span> }}>{cell}</ReactMarkdown>
              </div>
            ))}
            <div className="ir-table-cell ir-table-cell-actions">
              {!st && (
                <>
                  <button className="ir-btn ir-btn-confirm" onClick={() => handleConfirm(i, row)} title={t("report.confirmTimeline")}>✓</button>
                  <button className="ir-btn ir-btn-correct" onClick={() => startCorrect(i)} title={t("report.correctTimeline")}>✗</button>
                  <button className="ir-btn ir-btn-task" onClick={() => createTask(i, row)} title={t("report.createTask")}>+任务</button>
                </>
              )}
              {st === "confirmed" && <><span className="ir-badge ir-badge-ok">{t("report.confirmed")}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
              {st === "corrected" && <><span className="ir-badge ir-badge-warn">{t("report.corrected")}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
              {st === "tasked" && <><span className="ir-badge ir-badge-ok">✓ 任务 #{getTaskId(i)}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
            </div>
            {corrIdx === i && !st && (
              <div className="ir-correction-inline" style={{ flexBasis: "100%" }}>
                <input className="ir-correction-input" value={corrText}
                  onChange={e => setCorrText(e.target.value)}
                  placeholder={t("report.correctionPlaceholder")}
                  onKeyDown={e => {
                    if (e.key === "Enter" && !e.nativeEvent.isComposing) submitCorrect(i, row);
                    if (e.key === "Escape") setCorrIdx(null);
                  }}
                  autoFocus />
                <button className="ir-btn ir-btn-submit" onClick={() => submitCorrect(i, row)}
                  disabled={corrText.trim().length < 2}>{t("report.submitCorrection")}</button>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ─── Interactive List Items ─────────────────────────────────────────────────

function InteractiveItems({ items, reportType, sectionIdx, persisted, onPersist }: {
  items: ListItem[];
  reportType: string;
  sectionIdx: number;
  persisted: Persisted;
  onPersist: (p: Persisted) => void;
}) {
  const { t } = useLang();
  const k = (i: number) => `i${sectionIdx}_${i}`;
  const getState = (i: number) => persisted.states[k(i)] || "";
  const getTaskId = (i: number) => persisted.taskIds[k(i)];

  const setState = (i: number, val: string, taskId?: number) => {
    const next = { ...persisted, states: { ...persisted.states, [k(i)]: val }, taskIds: { ...persisted.taskIds } };
    if (taskId !== undefined) next.taskIds[k(i)] = taskId;
    onPersist(next);
  };

  const handleFeedback = async (i: number, accurate: boolean, text: string) => {
    setState(i, accurate ? "accurate" : "inaccurate");
    try {
      await invoke("save_report_correction", {
        reportType, wrongClaim: text,
        correctFact: accurate ? "✓" : "用户标记此项不准确",
        contextHint: accurate ? "positive_accurate" : "negative_inaccurate",
      });
    } catch {}
  };

  const createTask = async (i: number, item: ListItem) => {
    try {
      const clean = item.text.replace(/\*\*/g, "").replace(/\s*—\s*/, ": ");
      const taskId = await invoke<number>("create_task", {
        content: clean, source: reportType, sourceId: null,
        priority: item.priority === "high" ? "high" : "medium",
        dueDate: null, description: item.text,
      });
      setState(i, "created", taskId);
    } catch {
      // silently fail
    }
  };

  return (
    <div className="ir-items">
      {items.map((item, i) => {
        const st = getState(i);
        return (
          <div key={i} className={`ir-item ${item.priority} ${st}`}>
            <div className="ir-item-content">
              {item.priority === "optional" && <span className="ir-action-optional">可选</span>}
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.text}</ReactMarkdown>
            </div>
            <div className="ir-item-actions">
              {!st && (
                <>
                  <button className="ir-btn ir-btn-confirm" onClick={() => handleFeedback(i, true, item.text)} title={t("report.patternAccurate")}>👍</button>
                  <button className="ir-btn ir-btn-correct" onClick={() => handleFeedback(i, false, item.text)} title={t("report.patternInaccurate")}>👎</button>
                  <button className="ir-btn ir-btn-task" onClick={() => createTask(i, item)}>+ {t("report.createTask")}</button>
                </>
              )}
              {st === "accurate" && <><span className="ir-badge ir-badge-ok">{t("report.patternAccurate")}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
              {st === "inaccurate" && <><span className="ir-badge ir-badge-warn">{t("report.patternInaccurate")}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
              {st === "created" && <><span className="ir-badge ir-badge-ok">✓ {t("report.taskCreated")} #{getTaskId(i)}</span><button className="ir-btn ir-btn-undo" onClick={() => setState(i, "")} title={t("report.undo")}>↩</button></>}
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ─── Main Component ─────────────────────────────────────────────────────────

export default function InteractiveReport({ content, reportType }: {
  content: string;
  reportType: string;
}) {
  const parsed = useMemo(() => parseReport(content), [content]);
  const key = useMemo(() => contentKey(content), [content]);
  const [persisted, setPersisted] = useState<Persisted>(() => loadPersisted(key));

  // Save on every change
  useEffect(() => { savePersisted(key, persisted); }, [key, persisted]);

  const handlePersist = useCallback((next: Persisted) => setPersisted(next), []);

  const handleCorrect = useCallback(async (summary: string, correct: string) => {
    try {
      await invoke("save_report_correction", {
        reportType, wrongClaim: summary,
        correctFact: correct, contextHint: "interactive_correction",
      });
    } catch (err) {
      console.error("Failed to save correction:", err);
    }
  }, [reportType]);

  if (!parsed) {
    return <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>;
  }

  return (
    <div className="ir-report">
      {parsed.title && <h1 className="ir-title">{parsed.title}</h1>}
      {parsed.preamble && (
        <div className="ir-preamble"><ReactMarkdown remarkPlugins={[remarkGfm]}>{parsed.preamble}</ReactMarkdown></div>
      )}
      {parsed.sections.map((sec, si) => (
        <div key={si} className="ir-section">
          <h2 className="ir-section-title" dangerouslySetInnerHTML={{ __html: sec.title.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>") }} />
          {sec.tables.map((table, ti) => (
            <InteractiveTable key={ti} table={table} reportType={reportType}
              sectionIdx={si} tableIdx={ti} persisted={persisted} onPersist={handlePersist} onCorrect={handleCorrect} />
          ))}
          {sec.items.length > 0 && (
            <InteractiveItems items={sec.items} reportType={reportType}
              sectionIdx={si} persisted={persisted} onPersist={handlePersist} />
          )}
          {sec.prose.length > 0 && (
            <div className="ir-prose"><ReactMarkdown remarkPlugins={[remarkGfm]}>{sec.prose.join("\n")}</ReactMarkdown></div>
          )}
        </div>
      ))}
    </div>
  );
}
