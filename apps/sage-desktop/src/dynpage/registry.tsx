import React, { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePageData } from "./usePageData";
import { pickColumns } from "./filterUtils";

// ─── Stat ─────────────────────────────────────────────────────────────────────

interface StatProps { label: string; value: string; color?: string; }

function Stat({ label, value, color }: StatProps) {
  return (
    <div style={{
      padding: "12px 16px",
      borderRadius: "var(--radius, 8px)",
      border: "1px solid var(--border)",
      background: "var(--surface)",
      flex: 1,
      minWidth: 100,
    }}>
      <div style={{ fontSize: 22, fontWeight: 700, color: color || "var(--accent)" }}>{value}</div>
      <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>{label}</div>
    </div>
  );
}

// ─── StatRow ──────────────────────────────────────────────────────────────────

interface StatRowProps { children?: React.ReactNode; }

function StatRow({ children }: StatRowProps) {
  return (
    <div style={{ display: "flex", gap: 12, flexWrap: "wrap", marginBottom: 16 }}>
      {children}
    </div>
  );
}

// ─── DataTable ────────────────────────────────────────────────────────────────

interface DataTableProps {
  source?: string;
  filter?: string;
  columns?: string;
}

function DataTable({ source, filter, columns }: DataTableProps) {
  const { data, loading, error } = usePageData(source || "", filter);
  const rows = pickColumns(data, columns);

  if (loading) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading...</div>;
  if (error) return <div style={{ color: "var(--error)", fontSize: 13 }}>Error: {error}</div>;
  if (rows.length === 0) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>No data</div>;

  const headers = Object.keys(rows[0] || {});

  return (
    <div style={{ overflowX: "auto", marginBottom: 16 }}>
      <table style={{
        width: "100%", borderCollapse: "collapse",
        fontSize: 12, color: "var(--text)",
      }}>
        <thead>
          <tr>
            {headers.map(h => (
              <th key={h} style={{
                textAlign: "left", padding: "6px 10px",
                borderBottom: "1px solid var(--border)",
                color: "var(--text-secondary)", fontWeight: 600,
                background: "var(--surface)",
              }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} style={{ borderBottom: "1px solid var(--border)" }}>
              {headers.map(h => (
                <td key={h} style={{ padding: "5px 10px" }}>
                  {row[h] == null ? "" : String(row[h])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ─── Chart ────────────────────────────────────────────────────────────────────

interface ChartProps {
  source?: string;
  filter?: string;
  type?: string; // pie | bar | line
  field?: string; // field to group/plot
  label?: string;
}

function groupData(data: Record<string, unknown>[], field: string): Record<string, number> {
  const groups: Record<string, number> = {};
  for (const row of data) {
    const key = row[field] != null ? String(row[field]) : "(none)";
    groups[key] = (groups[key] || 0) + 1;
  }
  return groups;
}

function getAccentColors(canvas: HTMLCanvasElement, count: number): string[] {
  const style = getComputedStyle(canvas);
  const accent = style.getPropertyValue("--accent").trim() || "#6366f1";
  const palette = [accent, "#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#ec4899"];
  return Array.from({ length: count }, (_, i) => palette[i % palette.length]);
}

function drawPie(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, groups: Record<string, number>) {
  const entries = Object.entries(groups);
  const total = entries.reduce((s, [, v]) => s + v, 0);
  if (total === 0) return;

  const cx = canvas.width / 2, cy = canvas.height / 2;
  const r = Math.min(cx, cy) - 30;
  const colors = getAccentColors(canvas, entries.length);
  let startAngle = -Math.PI / 2;

  entries.forEach(([label, val], i) => {
    const slice = (val / total) * 2 * Math.PI;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.arc(cx, cy, r, startAngle, startAngle + slice);
    ctx.closePath();
    ctx.fillStyle = colors[i];
    ctx.fill();

    const midAngle = startAngle + slice / 2;
    const lx = cx + (r * 0.65) * Math.cos(midAngle);
    const ly = cy + (r * 0.65) * Math.sin(midAngle);
    ctx.fillStyle = "#fff";
    ctx.font = "11px sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(`${label} (${val})`, lx, ly);
    startAngle += slice;
  });
}

function drawBar(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, groups: Record<string, number>) {
  const entries = Object.entries(groups);
  if (entries.length === 0) return;
  const maxVal = Math.max(...entries.map(([, v]) => v));
  const colors = getAccentColors(canvas, entries.length);
  const style = getComputedStyle(canvas);
  const textColor = style.getPropertyValue("--text").trim() || "#e5e7eb";

  const padLeft = 40, padBottom = 50, padTop = 20, padRight = 20;
  const chartW = canvas.width - padLeft - padRight;
  const chartH = canvas.height - padBottom - padTop;
  const barW = Math.max(4, chartW / entries.length - 8);

  entries.forEach(([label, val], i) => {
    const x = padLeft + i * (chartW / entries.length) + (chartW / entries.length - barW) / 2;
    const barH = maxVal === 0 ? 0 : (val / maxVal) * chartH;
    const y = padTop + chartH - barH;
    ctx.fillStyle = colors[i];
    ctx.fillRect(x, y, barW, barH);
    ctx.fillStyle = textColor;
    ctx.font = "10px sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(label.slice(0, 8), x + barW / 2, canvas.height - padBottom + 14);
    ctx.fillText(String(val), x + barW / 2, y - 4);
  });
}

function drawLine(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, groups: Record<string, number>) {
  const entries = Object.entries(groups);
  if (entries.length < 2) { drawBar(ctx, canvas, groups); return; }
  const maxVal = Math.max(...entries.map(([, v]) => v));
  const style = getComputedStyle(canvas);
  const accent = style.getPropertyValue("--accent").trim() || "#6366f1";
  const textColor = style.getPropertyValue("--text").trim() || "#e5e7eb";

  const padLeft = 40, padBottom = 50, padTop = 20, padRight = 20;
  const chartW = canvas.width - padLeft - padRight;
  const chartH = canvas.height - padBottom - padTop;

  const points = entries.map(([, val], i) => ({
    x: padLeft + (i / (entries.length - 1)) * chartW,
    y: padTop + (maxVal === 0 ? chartH : (1 - val / maxVal) * chartH),
  }));

  ctx.strokeStyle = accent;
  ctx.lineWidth = 2;
  ctx.beginPath();
  points.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
  ctx.stroke();

  points.forEach((p, i) => {
    ctx.fillStyle = accent;
    ctx.beginPath();
    ctx.arc(p.x, p.y, 4, 0, 2 * Math.PI);
    ctx.fill();
    ctx.fillStyle = textColor;
    ctx.font = "10px sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(entries[i][0].slice(0, 8), p.x, canvas.height - padBottom + 14);
  });
}

function Chart({ source, filter, type = "bar", field = "status", label }: ChartProps) {
  const { data, loading, error } = usePageData(source || "", filter);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!canvasRef.current || loading || error || data.length === 0) return;
    const canvas = canvasRef.current;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const style = getComputedStyle(canvas);
    const bg = style.getPropertyValue("--surface").trim() || "transparent";
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const groups = groupData(data, field);
    if (type === "pie") drawPie(ctx, canvas, groups);
    else if (type === "line") drawLine(ctx, canvas, groups);
    else drawBar(ctx, canvas, groups);
  }, [data, loading, error, type, field]);

  if (loading) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading...</div>;
  if (error) return <div style={{ color: "var(--error)", fontSize: 13 }}>Error: {error}</div>;

  return (
    <div style={{ marginBottom: 16 }}>
      {label && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 6 }}>{label}</div>}
      <canvas ref={canvasRef} width={480} height={280}
        style={{ borderRadius: "var(--radius, 8px)", border: "1px solid var(--border)", background: "var(--surface)", maxWidth: "100%" }} />
    </div>
  );
}

// ─── KanbanBoard ──────────────────────────────────────────────────────────────

interface KanbanBoardProps { source?: string; filter?: string; groupBy?: string; titleField?: string; }

function KanbanBoard({ source, filter, groupBy = "status", titleField = "content" }: KanbanBoardProps) {
  const { data, loading, error } = usePageData(source || "", filter);

  if (loading) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading...</div>;
  if (error) return <div style={{ color: "var(--error)", fontSize: 13 }}>Error: {error}</div>;

  const columns: Record<string, Record<string, unknown>[]> = {};
  for (const row of data) {
    const col = row[groupBy] != null ? String(row[groupBy]) : "—";
    if (!columns[col]) columns[col] = [];
    columns[col].push(row);
  }

  return (
    <div style={{ display: "flex", gap: 12, overflowX: "auto", marginBottom: 16 }}>
      {Object.entries(columns).map(([col, rows]) => (
        <div key={col} style={{
          minWidth: 180, flex: "0 0 auto",
          background: "var(--surface)", borderRadius: "var(--radius, 8px)",
          border: "1px solid var(--border)", padding: 10,
        }}>
          <div style={{ fontWeight: 600, fontSize: 12, color: "var(--accent)", marginBottom: 8 }}>
            {col} ({rows.length})
          </div>
          {rows.map((row, i) => (
            <div key={i} style={{
              padding: "6px 8px", marginBottom: 6,
              background: "var(--surface)", border: "1px solid var(--border)",
              borderRadius: 6, fontSize: 12, color: "var(--text)",
            }}>
              {String(row[titleField] ?? "")}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

// ─── Timeline ────────────────────────────────────────────────────────────────

interface TimelineProps { source?: string; filter?: string; dateField?: string; titleField?: string; }

function Timeline({ source, filter, dateField = "created_at", titleField = "content" }: TimelineProps) {
  const { data, loading, error } = usePageData(source || "", filter);
  const sorted = [...data].sort((a, b) => String(b[dateField] ?? "").localeCompare(String(a[dateField] ?? "")));

  if (loading) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading...</div>;
  if (error) return <div style={{ color: "var(--error)", fontSize: 13 }}>Error: {error}</div>;
  if (sorted.length === 0) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>No data</div>;

  return (
    <div style={{ marginBottom: 16 }}>
      {sorted.map((row, i) => (
        <div key={i} style={{ display: "flex", gap: 12, marginBottom: 12 }}>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
            <div style={{ width: 10, height: 10, borderRadius: "50%", background: "var(--accent)", flexShrink: 0 }} />
            {i < sorted.length - 1 && <div style={{ width: 1, flex: 1, background: "var(--border)", marginTop: 4 }} />}
          </div>
          <div style={{ paddingBottom: 8 }}>
            <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>
              {String(row[dateField] ?? "").slice(0, 10)}
            </div>
            <div style={{ fontSize: 13, color: "var(--text)" }}>{String(row[titleField] ?? "")}</div>
          </div>
        </div>
      ))}
    </div>
  );
}

// ─── Progress ─────────────────────────────────────────────────────────────────

interface ProgressProps { value?: string; max?: string; label?: string; color?: string; }

function Progress({ value = "0", max = "100", label, color }: ProgressProps) {
  const pct = Math.min(100, Math.max(0, (Number(value) / Number(max)) * 100));
  return (
    <div style={{ marginBottom: 12 }}>
      {label && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 4 }}>{label}</div>}
      <div style={{ background: "var(--border)", borderRadius: 4, height: 8, overflow: "hidden" }}>
        <div style={{
          width: `${pct}%`, height: "100%",
          background: color || "var(--accent)", borderRadius: 4,
          transition: "width 0.3s ease",
        }} />
      </div>
      <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>{pct.toFixed(0)}%</div>
    </div>
  );
}

// ─── Pomodoro ─────────────────────────────────────────────────────────────────

interface PomodoroProps { duration?: string; work?: string; breakTime?: string; breakDuration?: string; }

function Pomodoro({ duration, work, breakTime, breakDuration }: PomodoroProps) {
  const workMins = Number(duration || work || "25");
  const breakMins = Number(breakDuration || breakTime || "5");
  const workSecs = workMins * 60;
  const breakSecs = breakMins * 60;
  const [phase, setPhase] = useState<"work" | "break">("work");
  const [remaining, setRemaining] = useState(workSecs);
  const [running, setRunning] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const total = phase === "work" ? workSecs : breakSecs;
  const pct = remaining / total;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const style = getComputedStyle(canvas);
    const accent = style.getPropertyValue("--accent").trim() || "#6366f1";
    const border = style.getPropertyValue("--border").trim() || "#374151";

    ctx.clearRect(0, 0, 80, 80);
    ctx.strokeStyle = border;
    ctx.lineWidth = 6;
    ctx.beginPath();
    ctx.arc(40, 40, 32, 0, 2 * Math.PI);
    ctx.stroke();
    ctx.strokeStyle = accent;
    ctx.beginPath();
    ctx.arc(40, 40, 32, -Math.PI / 2, -Math.PI / 2 + pct * 2 * Math.PI);
    ctx.stroke();
  }, [remaining, pct]);

  const tick = useCallback(() => {
    setRemaining(prev => {
      if (prev <= 1) {
        setRunning(false);
        const nextPhase = phase === "work" ? "break" : "work";
        setPhase(nextPhase);
        return nextPhase === "work" ? workSecs : breakSecs;
      }
      return prev - 1;
    });
  }, [phase, workSecs, breakSecs]);

  useEffect(() => {
    if (running) {
      intervalRef.current = setInterval(tick, 1000);
    } else {
      if (intervalRef.current) clearInterval(intervalRef.current);
    }
    return () => { if (intervalRef.current) clearInterval(intervalRef.current); };
  }, [running, tick]);

  const reset = () => {
    setRunning(false);
    setPhase("work");
    setRemaining(workSecs);
  };

  const mm = String(Math.floor(remaining / 60)).padStart(2, "0");
  const ss = String(remaining % 60).padStart(2, "0");

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 16, padding: 12,
      border: "1px solid var(--border)", borderRadius: "var(--radius, 8px)",
      background: "var(--surface)", marginBottom: 16, width: "fit-content" }}>
      <canvas ref={canvasRef} width={80} height={80} />
      <div>
        <div style={{ fontSize: 24, fontWeight: 700, fontVariantNumeric: "tabular-nums", color: "var(--text)" }}>
          {mm}:{ss}
        </div>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 8 }}>
          {phase === "work" ? "Focus" : "Break"}
        </div>
        <div style={{ display: "flex", gap: 6 }}>
          <button onClick={() => setRunning(r => !r)} style={{
            padding: "4px 10px", borderRadius: 5, border: "1px solid var(--border)",
            background: "var(--accent)", color: "#fff", fontSize: 12, cursor: "pointer",
          }}>{running ? "Pause" : "Start"}</button>
          <button onClick={reset} style={{
            padding: "4px 10px", borderRadius: 5, border: "1px solid var(--border)",
            background: "var(--surface)", color: "var(--text-secondary)", fontSize: 12, cursor: "pointer",
          }}>Reset</button>
        </div>
      </div>
    </div>
  );
}

// ─── MemoryCloud ──────────────────────────────────────────────────────────────

interface MemoryCloudProps { limit?: string; }

interface MemoryItem { id: number; content: string; confidence: number; category: string; }

const CATEGORY_COLORS: Record<string, string> = {
  belief: "var(--warning)",
  pattern: "var(--accent)",
  preference: "var(--success)",
  fact: "var(--text-secondary)",
};

function MemoryCloud({ limit = "30" }: MemoryCloudProps) {
  const [memories, setMemories] = useState<MemoryItem[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<MemoryItem[]>("get_all_memories", { limit: Number(limit) })
      .then(m => { setMemories(m); setLoading(false); })
      .catch(() => setLoading(false));
  }, [limit]);

  if (loading) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>Loading...</div>;
  if (memories.length === 0) return <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>No memories yet</div>;

  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 16 }}>
      {memories.map(m => {
        const size = 11 + Math.round(m.confidence * 6);
        const color = CATEGORY_COLORS[m.category] || "var(--text-secondary)";
        return (
          <span key={m.id} title={`${m.category} · confidence: ${m.confidence}`} style={{
            fontSize: size, color, cursor: "default",
            padding: "2px 6px", borderRadius: 4,
            background: "var(--surface)", border: "1px solid var(--border)",
          }}>
            {m.content.length > 40 ? m.content.slice(0, 40) + "…" : m.content}
          </span>
        );
      })}
    </div>
  );
}

// ─── Registry ─────────────────────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const REGISTRY: Record<string, React.ComponentType<any>> = {
  Stat,
  StatRow,
  DataTable,
  Chart,
  KanbanBoard,
  Timeline,
  Progress,
  Pomodoro,
  MemoryCloud,
};
