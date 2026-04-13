import { useRef, useEffect, useState, useCallback, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { invokeDeduped } from "../utils/invokeCache";
import { GridLayout, type LayoutItem, type Layout } from "react-grid-layout";
import "react-grid-layout/css/styles.css";
import { DashData, TYPE_COLORS, TYPE_LABEL, reportLabel, preview } from "./types";
import { TaskItem } from "../types";
import { loadPinned, togglePin, isPinned as checkPinned, onPinChange, unpinItem, type PinnedItem } from "./pinStore";
import { useLang } from "../LangContext";
import InteractiveReport from "../components/InteractiveReport";
import CompletionDialog from "../components/CompletionDialog";
import { useDashboard } from "../contexts/DashboardContext";

/* ═══ Widget Registry ═══ */

interface WidgetMeta {
  id: string;
  title: string;
  icon: string;
  defaultLayout: { w: number; h: number; minW: number; minH: number };
  render: (data: DashData) => React.ReactNode;
}

const WIDGET_CATALOG: WidgetMeta[] = [
  { id: "report",      title: "Brief",        icon: "📋", defaultLayout: { w: 7, h: 11, minW: 4, minH: 5 }, render: d => <ReportWidget data={d} /> },
  { id: "tags",        title: "Tags",          icon: "🏷", defaultLayout: { w: 7, h: 4,  minW: 3, minH: 3 }, render: d => <TagsWidget data={d} /> },
  { id: "sessions",    title: "Sessions",      icon: "💬", defaultLayout: { w: 4, h: 5,  minW: 3, minH: 3 }, render: d => <SessionsWidget data={d} /> },
  { id: "memories",    title: "Memories",      icon: "🧠", defaultLayout: { w: 4, h: 5,  minW: 3, minH: 3 }, render: d => <MemoriesWidget data={d} /> },
  { id: "question",    title: "Daily Question", icon: "❓", defaultLayout: { w: 4, h: 3,  minW: 3, minH: 2 }, render: _ => <QuestionWidget /> },
  { id: "connections", title: "Connections",    icon: "🔌", defaultLayout: { w: 4, h: 4,  minW: 3, minH: 3 }, render: _ => <ConnectionsWidget /> },
  { id: "messages",    title: "Messages",       icon: "✉️", defaultLayout: { w: 5, h: 5,  minW: 3, minH: 3 }, render: _ => <MessagesWidget /> },
  { id: "people",      title: "People",         icon: "👥", defaultLayout: { w: 3, h: 4,  minW: 2, minH: 3 }, render: d => <PeopleWidget data={d} /> },
  { id: "news",        title: "News",            icon: "📰", defaultLayout: { w: 5, h: 6,  minW: 3, minH: 4 }, render: _ => <NewsWidget /> },
  { id: "pinned",      title: "Pinned",          icon: "📌", defaultLayout: { w: 5, h: 5,  minW: 3, minH: 3 }, render: d => <PinnedWidget data={d} /> },
  { id: "nebula",      title: "Nebula",           icon: "✨", defaultLayout: { w: 5, h: 7,  minW: 3, minH: 5 }, render: d => <NebulaWidget data={d} /> },
  { id: "tasks",       title: "Tasks",            icon: "☑️", defaultLayout: { w: 5, h: 6,  minW: 3, minH: 4 }, render: _ => <TasksWidget /> },
  { id: "meetings",    title: "Today's Meetings", icon: "📅", defaultLayout: { w: 5, h: 6,  minW: 3, minH: 4 }, render: _ => <MeetingsWidget /> },
];

const CATALOG_MAP = new Map(WIDGET_CATALOG.map(w => [w.id, w]));

/* ═══ Layout Persistence ═══ */

const LAYOUT_KEY = "command_grid_layout";
const VISIBLE_KEY = "command_visible_widgets";
const DEFAULT_VISIBLE = ["report", "meetings", "tasks", "nebula", "news", "tags"];

function loadVisible(): string[] {
  try { const s = localStorage.getItem(VISIBLE_KEY); if (s) return JSON.parse(s); } catch {}
  return DEFAULT_VISIBLE;
}

function loadLayout(visible: string[]): LayoutItem[] {
  try {
    const s = localStorage.getItem(LAYOUT_KEY);
    if (s) {
      const saved: LayoutItem[] = JSON.parse(s);
      const savedMap = new Map(saved.map(l => [l.i, l]));
      return visible.map(id => {
        const meta = CATALOG_MAP.get(id); if (!meta) return null;
        const existing = savedMap.get(id);
        if (existing) return { ...existing, minW: meta.defaultLayout.minW, minH: meta.defaultLayout.minH };
        return makeItem(id, meta);
      }).filter(Boolean) as LayoutItem[];
    }
  } catch {}
  return visible.map(id => { const m = CATALOG_MAP.get(id); return m ? makeItem(id, m) : null; }).filter(Boolean) as LayoutItem[];
}

function makeItem(id: string, meta: WidgetMeta): LayoutItem {
  return { i: id, x: 0, y: 999, w: meta.defaultLayout.w, h: meta.defaultLayout.h,
    minW: meta.defaultLayout.minW, minH: meta.defaultLayout.minH };
}

/* ═══ Built-in Widget Components ═══ */

function ReportWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const btns: [string, string][] = [
    ["morning", t("dashboard.reportAm")],
    ["evening", t("dashboard.reportPm")],
    ["weekly",  t("dashboard.reportWk")],
  ];
  return (<>
    <div className="cmd-drag-handle">
      <span className="cmd-card-title">{data.report ? reportLabel(data.report.type) : "Brief"}</span>
      <span className="cmd-rpt-btns">
        {btns.map(([type, label]) => <button key={type} className="cmd-rpt-btn" onClick={() => data.triggerReport(type)}
          disabled={data.reportLoading === type}>{data.reportLoading === type ? "…" : label}</button>)}
      </span>
    </div>
    <div className="cmd-card-body">
      {data.report ? (
        <div className="cmd-report-md" onClick={(e) => {
          // Only open expanded if clicking outside interactive buttons
          if ((e.target as HTMLElement).closest(".ir-btn, .ir-badge, .ir-correction-input")) return;
          data.openExpanded({ content: data.report!.data.content, category: "report", ref_id: data.report!.type });
        }}>
          <InteractiveReport content={data.report.data.content} reportType={data.report.type} />
        </div>
      ) : <span className="cmd-empty">{t("widget.reportEmpty")}</span>}
    </div>
  </>);
}



function TagsWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const tags = data.stats?.top_tags ?? [];
  return (<div className="cmd-card-body">
    <div className="cmd-tags">
      {tags.slice(0, 20).map(tag => <span key={tag.tag} className="cmd-tag">#{tag.tag}<em>{tag.count}</em></span>)}
      {!tags.length && <span className="cmd-empty">{t("widget.noTags")}</span>}
    </div>
  </div>);
}

function SessionsWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const sessions = data.items.filter(i => i.category === "session").slice(0, 8);
  return (<div className="cmd-card-body">
    {sessions.map((item, i) => (
      <div key={i} className="cmd-feed-row" onClick={() => data.openExpanded(item)}>
        <span className="cmd-feed-dot" style={{ background: "#6366f1" }} />
        <div className="cmd-feed-info">
          <span className="cmd-feed-type">CONV</span>
          <span className="cmd-feed-text">{preview(item.content, 55)}</span>
        </div>
      </div>
    ))}
    {!sessions.length && <span className="cmd-empty">{t("widget.noSessions")}</span>}
  </div>);
}

function MemoriesWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const memories = data.items.filter(i => i.category === "memory").slice(0, 8);
  return (<div className="cmd-card-body">
    {memories.map((item, i) => (
      <div key={i} className="cmd-feed-row" onClick={() => data.openExpanded(item)}>
        <span className="cmd-feed-dot" style={{ background: "#22c55e" }} />
        <div className="cmd-feed-info">
          <span className="cmd-feed-type">MEMORY</span>
          <span className="cmd-feed-text">{preview(item.content, 55)}</span>
        </div>
      </div>
    ))}
    {!memories.length && <span className="cmd-empty">{t("widget.noMemories")}</span>}
  </div>);
}

/* ─── Widgets that read from DashboardContext (useDashboard) ─── */

function QuestionWidget() {
  const { t } = useLang();
  const { state } = useDashboard();
  const q = state.question;
  return (<div className="cmd-card-body">
    {q ? <div className="cmd-question-text">{q.response}</div>
       : <span className="cmd-empty">{t("widget.noQuestion")}</span>}
  </div>);
}

function ConnectionsWidget() {
  const { t } = useLang();
  const { state } = useDashboard();
  const entries = Object.entries(state.connections);
  return (<div className="cmd-card-body">
    {entries.length > 0 ? entries.map(([key, val]) => (
      <div key={key} className="cmd-conn-row">
        <span className={`cmd-conn-dot ${val.status === "connected" ? "on" : ""}`} />
        <span className="cmd-conn-name">{key}</span>
        <span className="cmd-conn-status">{val.label}</span>
      </div>
    )) : <span className="cmd-empty">{t("widget.noConnections")}</span>}
  </div>);
}

function MessagesWidget() {
  const { t } = useLang();
  const { state } = useDashboard();
  const msgs = state.messages;
  const [userName, setUserName] = useState("Me");
  useEffect(() => {
    invokeDeduped<{ identity?: { name?: string } } | null>("get_profile")
      .then(p => { if (p?.identity?.name) setUserName(p.identity.name); }).catch(() => {});
  }, []);
  return (<div className="cmd-card-body">
    {msgs.slice(0, 8).map((m, i) => (
      <div key={i} className="cmd-feed-row">
        <span className="cmd-feed-dot" style={{ background: "#14b8a6" }} />
        <div className="cmd-feed-info">
          <span className="cmd-feed-type">{(!m.sender || m.sender === "Unknown" || m.sender === "unknown") ? (m.direction === "sent" ? userName : (m.channel ?? "MSG")) : m.sender}</span>
          <span className="cmd-feed-text">{(m.content ?? "").slice(0, 60)}</span>
        </div>
      </div>
    ))}
    {!msgs.length && <span className="cmd-empty">{t("widget.noMessages")}</span>}
  </div>);
}

function PeopleWidget({ data }: { data: DashData }) {
  const { state } = useDashboard();
  // Derive person names from items already loaded in context (memories with about_person)
  const people = useMemo(() => {
    const persons = new Set<string>();
    state.items.forEach(m => {
      if (m.about_person) persons.add(m.about_person);
    });
    return [...persons].slice(0, 12);
  }, [state.items]);
  return (<div className="cmd-card-body">
    <div className="cmd-tags">
      {people.map(p => <span key={p} className="cmd-tag cmd-tag-people">{p}</span>)}
      {!people.length && <span className="cmd-empty">{data.stats?.known_persons ?? 0} people tracked</span>}
    </div>
  </div>);
}

/* Compact Tasks widget — quick add + top open tasks + completion dialog */


function TasksWidget() {
  const { t } = useLang();
  const { state, refresh } = useDashboard();
  const tasks = state.tasks;
  const signals = state.taskSignals;
  const [input, setInput] = useState("");
  const [completing, setCompleting] = useState<TaskItem | null>(null);

  const addTask = () => {
    const text = input.trim(); if (!text) return;
    invoke("create_task", { content: text, source: null, sourceId: null, priority: null, dueDate: null })
      .then(() => { setInput(""); refresh("tasks"); }).catch(e => console.error("create_task:", e));
  };

  return (<div className="cmd-card-body">
    <div className="cmd-task-add">
      <input className="cmd-task-input" placeholder={t("widget.taskPlaceholder")} value={input}
        onChange={e => setInput(e.target.value)}
        onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) addTask(); }} />
      <button className="cmd-task-add-btn" onClick={addTask} disabled={!input.trim()}>+</button>
    </div>
    {tasks.map(task => (
      <div key={task.id} className="cmd-task-row">
        <button className="cmd-done-btn" onClick={() => setCompleting(task)} title="Done">✓</button>
        <span className="cmd-task-text">{task.content}</span>
      </div>
    ))}
    {!tasks.length && <span className="cmd-empty">{t("widget.noTasks")}</span>}
    {signals.length > 0 && (
      <a href="#/tasks" className="cmd-task-signals-link">
        {signals.length} {t("widget.suggestionsPending")}
      </a>
    )}
    <a href="#/tasks" className="cmd-task-viewall">{t("widget.viewAllTasks")}</a>

    {completing && <CompletionDialog task={completing} onClose={() => setCompleting(null)} onRefresh={() => refresh("tasks")} />}
  </div>);
}

interface FeedItem { id: number; title: string; url: string; score: number; insight: string; created_at: string; }

function extractSource(url: string): { label: string; color: string } {
  if (url.includes("github.com")) return { label: "GitHub", color: "#8b5cf6" };
  if (url.includes("reddit.com")) return { label: "Reddit", color: "#ff4500" };
  if (url.includes("news.ycombinator")) return { label: "HN", color: "#ff6600" };
  if (url.includes("arxiv.org")) return { label: "arXiv", color: "#b31b1b" };
  if (url.includes("bbc.com")) return { label: "BBC", color: "#bb1919" };
  return { label: "Web", color: "var(--text-tertiary)" };
}

function timeAgo(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const h = Math.floor(diff / 3600000);
  if (h < 1) return "just now";
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

function NewsWidget() {
  const { t } = useLang();
  const [items, setItems] = useState<FeedItem[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  useEffect(() => {
    invoke<FeedItem[]>("get_feed_items", { limit: 20 }).then(setItems).catch(() => {});
  }, []);
  const refresh = () => {
    setRefreshing(true);
    invoke<string>("trigger_feed_poll")
      .then(() => setTimeout(() => {
        invoke<FeedItem[]>("get_feed_items", { limit: 20 }).then(setItems).catch(() => {});
        setRefreshing(false);
      }, 5000))
      .catch(() => setRefreshing(false));
  };
  return (<div className="cmd-card-body cmd-news-body">
    {items.slice(0, 10).map((item, i) => {
      const src = extractSource(item.url);
      return (
        <a key={item.id ?? i} className="cmd-news-card" href={item.url || undefined}
          onClick={e => { if (!item.url) e.preventDefault(); }}>
          <div className="cmd-news-meta">
            <span className="cmd-news-source" style={{ color: src.color }}>{src.label}</span>
            <span className="cmd-news-dots">{"●".repeat(Math.min(item.score, 5))}</span>
            <span className="cmd-news-time">{timeAgo(item.created_at)}</span>
          </div>
          <div className="cmd-news-title">{item.title || "Untitled"}</div>
          {item.insight && <div className="cmd-news-insight">{item.insight}</div>}
        </a>
      );
    })}
    {!items.length && <span className="cmd-empty">{t("widget.noNews")}</span>}
    <button className="cmd-news-refresh" onClick={refresh} disabled={refreshing}>
      {refreshing ? t("widget.fetching") : t("widget.refresh")}
    </button>
  </div>);
}

/* ─── Nebula Widget: flow layout + pin/unpin ─── */
interface NCard { id: number; content: string; category: string; phase: "in" | "hold" | "out"; raw: Parameters<DashData["openExpanded"]>[0]; }

const NEBULA_SPEED_KEY = "nebula_speed";

type NebulaSpeedKey = "widget.speedFast" | "widget.speedNormal" | "widget.speedSlow";

const NEBULA_SPEEDS: { labelKey: NebulaSpeedKey; spawn: number; hold: number }[] = [
  { labelKey: "widget.speedFast",   spawn: 3000,  hold: 5000 },
  { labelKey: "widget.speedNormal", spawn: 6000,  hold: 10000 },
  { labelKey: "widget.speedSlow",   spawn: 10000, hold: 18000 },
];
function loadNebulaSpeed(): number {
  try { const v = localStorage.getItem(NEBULA_SPEED_KEY); if (v) return Number(v); } catch {}
  return 1; // default Normal
}

function NebulaWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef(0);
  const idxRef = useRef(0);
  const nextIdRef = useRef(0);
  const [cards, setCards] = useState<NCard[]>([]);
  const [pinVer, setPinVer] = useState(0);
  const [speedIdx, setSpeedIdx] = useState(loadNebulaSpeed);
  const speed = NEBULA_SPEEDS[speedIdx] ?? NEBULA_SPEEDS[1];

  const cycleSpeed = () => {
    const next = (speedIdx + 1) % NEBULA_SPEEDS.length;
    setSpeedIdx(next);
    localStorage.setItem(NEBULA_SPEED_KEY, String(next));
  };
  // Curated first (diverse), then non-memory items, memories last — deduplicated by id
  const allItems = useMemo(() => {
    const nonMem = data.items.filter(i => i.category !== "memory");
    const mem = data.items.filter(i => i.category === "memory");
    const raw = [...data.curated, ...nonMem.filter(i => i.category !== "session"), ...mem];
    const seen = new Set<string | number>();
    return raw.filter(item => {
      const key = item.id ?? item.ref_id ?? item.content.slice(0, 50);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }, [data.curated, data.items]);

  // Sync pin state via events
  useEffect(() => onPinChange(() => setPinVer(v => v + 1)), []);

  // Particle canvas
  useEffect(() => {
    const cv = canvasRef.current; if (!cv) return;
    const ctx = cv.getContext("2d")!;
    const pts = Array.from({ length: 30 }, () => ({
      x: Math.random(), y: Math.random(), sz: 0.4 + Math.random(),
      a: 0.1 + Math.random() * 0.3, sp: 0.0001 + Math.random() * 0.0002,
    }));
    const resize = () => { cv.width = cv.offsetWidth; cv.height = cv.offsetHeight; };
    resize();
    const ro = new ResizeObserver(resize); ro.observe(cv);
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#6366f1";
    const [r, g, b] = [parseInt(accent.slice(1, 3), 16) || 99, parseInt(accent.slice(3, 5), 16) || 102, parseInt(accent.slice(5, 7), 16) || 241];
    const draw = () => {
      ctx.clearRect(0, 0, cv.width, cv.height);
      for (const p of pts) {
        p.y -= p.sp * 16; if (p.y < -0.01) p.y = 1.01;
        ctx.beginPath(); ctx.arc(p.x * cv.width, p.y * cv.height, p.sz, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${r},${g},${b},${p.a * 0.25})`; ctx.fill();
      }
      rafRef.current = requestAnimationFrame(draw);
    };
    draw();
    return () => { cancelAnimationFrame(rafRef.current); ro.disconnect(); };
  }, []);

  // Spawn cards
  useEffect(() => {
    if (!allItems.length) return;
    const spawn = () => setCards(prev => {
      const visibleIds = new Set(prev.map(c => c.raw.id ?? c.raw.ref_id));
      let item = null;
      for (let attempt = 0; attempt < allItems.length; attempt++) {
        const candidate = allItems[idxRef.current % allItems.length];
        idxRef.current++;
        const cid = candidate.id ?? candidate.ref_id;
        if (cid == null || !visibleIds.has(cid)) { item = candidate; break; }
      }
      if (!item) return prev;
      const card: NCard = { id: nextIdRef.current++, content: preview(item.content, 45), category: item.category, phase: "in", raw: item };
      const next = [...prev, card];
      return next.length > 5 ? next.slice(-5) : next;
    });
    spawn();
    const timer = setInterval(spawn, speed.spawn);
    return () => clearInterval(timer);
  }, [allItems.length, speed.spawn]);

  // Lifecycle
  useEffect(() => {
    const timers: number[] = [];
    for (const c of cards) {
      if (c.phase === "in") timers.push(window.setTimeout(() => setCards(p => p.map(x => x.id === c.id ? { ...x, phase: "hold" } : x)), 400));
      else if (c.phase === "hold") timers.push(window.setTimeout(() => setCards(p => p.map(x => x.id === c.id ? { ...x, phase: "out" } : x)), speed.hold));
      else if (c.phase === "out") timers.push(window.setTimeout(() => setCards(p => p.filter(x => x.id !== c.id)), 400));
    }
    return () => timers.forEach(clearTimeout);
  }, [cards]);

  const pinnedItems = loadPinned();
  void pinVer; // trigger re-render on pin changes

  return (
    <div className="cmd-nebula-wrap">
      <canvas ref={canvasRef} className="cmd-nebula-canvas" />
      <div className="cmd-nebula-flow">
        {cards.map(c => (
          <div key={c.id} className={`cmd-nebula-card cmd-nebula-${c.phase}`} onClick={() => data.openExpanded(c.raw)}>
            <div className="cmd-nebula-header">
              <span className="cmd-nebula-label">{TYPE_LABEL[c.category] ?? "INFO"}</span>
              <button className={`cmd-nebula-pin${checkPinned(c.raw) ? " pinned" : ""}`}
                onClick={e => { e.stopPropagation(); togglePin(c.raw); }}>📌</button>
            </div>
            <div className="cmd-nebula-text">{c.content}</div>
          </div>
        ))}
      </div>
      <button className="cmd-nebula-speed" onClick={cycleSpeed} title="Change speed">
        {t(speed.labelKey)}
      </button>
      {pinnedItems.length > 0 && (
        <div className="cmd-nebula-pincount">{pinnedItems.length} pinned</div>
      )}
    </div>
  );
}

/* ─── Pinned Widget: event-synced ─── */
function PinnedWidget({ data }: { data: DashData }) {
  const { t } = useLang();
  const [items, setItems] = useState<PinnedItem[]>(loadPinned);
  useEffect(() => onPinChange(() => setItems(loadPinned())), []);
  const handleUnpin = (idx: number) => { unpinItem(idx); /* event fires automatically */ };
  return (<div className="cmd-card-body">
    {items.map((item, i) => (
      <div key={i} className="cmd-pin-row">
        <div className="cmd-feed-row" style={{ flex: 1 }} onClick={() => data.openExpanded(item)}>
          <span className="cmd-feed-dot" style={{ background: TYPE_COLORS[item.category] ?? "var(--accent)" }} />
          <div className="cmd-feed-info">
            <span className="cmd-feed-type">{TYPE_LABEL[item.category] ?? "PIN"}</span>
            <span className="cmd-feed-text">{preview(item.content, 60)}</span>
          </div>
        </div>
        <button className="cmd-unpin-btn" onClick={() => handleUnpin(i)} title="Unpin">×</button>
      </div>
    ))}
    {!items.length && <span className="cmd-empty">{t("widget.noPinned")}</span>}
  </div>);
}

/* ─── Meetings Widget ─── */

function MeetingsWidget() {
  const { t } = useLang();
  const { state } = useDashboard();
  const navigate = useNavigate();
  const meetings = state.events;

  const statusColor = (s: string) =>
    s === "now" ? "var(--success, #22c55e)" : s === "upcoming" ? "var(--accent)" : "var(--text-tertiary)";
  const statusLabel = (s: string) =>
    s === "now" ? t("widget.meetingNow") : s === "upcoming" ? t("widget.meetingUpcoming") : t("widget.meetingPast");

  return (
    <div className="cmd-card-body">
      {meetings.map((m, i) => (
        <div key={i} className="cmd-meeting-row"
          onClick={() => navigate("/chat", { state: { quote: `会议「${m.subject}」${m.start}–${m.end}` } })}>
          <div className="cmd-meeting-time">
            <span className="cmd-meeting-dot" style={{ background: statusColor(m.status) }} />
            <span className="cmd-meeting-hm">{m.start}</span>
          </div>
          <div className="cmd-meeting-info">
            <span className="cmd-meeting-title">{m.subject}</span>
            {m.location && <span className="cmd-meeting-loc">{m.location}</span>}
          </div>
          <span className="cmd-meeting-badge" style={{ color: statusColor(m.status) }}>
            {statusLabel(m.status)}
          </span>
        </div>
      ))}
      {!meetings.length && <span className="cmd-empty">{t("widget.noMeetings")}</span>}
    </div>
  );
}

/* ═══ Main Layout ═══ */

export default function CommandLayout({ data }: { data: DashData }) {
  const { t } = useLang();
  const containerRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState<string[]>(loadVisible);
  const [layout, setLayout] = useState<LayoutItem[]>(() => loadLayout(visible));
  const [width, setWidth] = useState(800);
  const [showPicker, setShowPicker] = useState(false);

  const gridConfig = useMemo(() => ({
    cols: 12, rowHeight: 34, margin: [10, 10] as [number, number], containerPadding: [16, 8] as [number, number],
  }), []);
  const dragConfig = useMemo(() => ({ enabled: true, handle: ".cmd-drag-handle" }), []);
  const resizeConfig = useMemo(() => ({ enabled: true, handles: ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const }), []);

  useEffect(() => {
    const el = containerRef.current; if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    ro.observe(el); return () => ro.disconnect();
  }, []);

  const onLayoutChange = useCallback((nl: Layout) => {
    const items = [...nl]; setLayout(items);
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(items));
  }, []);

  const toggleWidget = useCallback((id: string) => {
    setVisible(prev => {
      const next = prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id];
      localStorage.setItem(VISIBLE_KEY, JSON.stringify(next));
      setLayout(loadLayout(next));
      return next;
    });
  }, []);

  const removeWidget = useCallback((id: string) => {
    setVisible(prev => {
      const next = prev.filter(x => x !== id);
      localStorage.setItem(VISIBLE_KEY, JSON.stringify(next));
      setLayout(cur => { const f = cur.filter(l => l.i !== id); localStorage.setItem(LAYOUT_KEY, JSON.stringify(f)); return f; });
      return next;
    });
  }, []);

  const s = data.stats;

  return (
    <div className="cmd-page" ref={containerRef}>
      {/* KPI Row */}
      <div className="cmd-kpi-row">
        <div className="cmd-greeting">
          <span className="cmd-greeting-dot" />
          <span className="cmd-greeting-text">{t("widget.sageOnline")}</span>
        </div>
        <div className="cmd-kpis">
          {[
            { v: s?.memories ?? 0, l: t("widget.kpiMemories"), c: "var(--accent)" },
            { v: s?.edges ?? 0, l: t("widget.kpiLinks"), c: "var(--success)" },
            { v: s?.sessions ?? 0, l: t("widget.kpiSessions"), c: "var(--warning, #eab308)" },
            { v: s?.known_persons ?? 0, l: t("widget.kpiPeople"), c: "var(--error, #ef4444)" },
          ].map(k => (
            <div key={k.l} className="cmd-kpi">
              <span className="cmd-kpi-val" style={{ color: k.c }}>{k.v}</span>
              <span className="cmd-kpi-lbl">{k.l}</span>
            </div>
          ))}
          <div className="cmd-add-wrap">
            <button className="cmd-add-btn" onClick={() => setShowPicker(p => !p)} title={t("widget.addRemoveWidgets")}>+</button>
            {showPicker && (
              <div className="cmd-picker">
                <div className="cmd-picker-header">{t("widget.pickerTitle")}</div>
                {WIDGET_CATALOG.map(w => (
                  <label key={w.id} className="cmd-picker-item">
                    <input type="checkbox" checked={visible.includes(w.id)} onChange={() => toggleWidget(w.id)} />
                    <span>{w.icon} {w.title}</span>
                  </label>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Bento Grid */}
      <div className="cmd-grid-wrap" onClick={() => showPicker && setShowPicker(false)}>
        <GridLayout layout={layout} width={width} gridConfig={gridConfig}
          dragConfig={dragConfig} resizeConfig={resizeConfig} onLayoutChange={onLayoutChange} autoSize>
          {visible.map(id => {
            const meta = CATALOG_MAP.get(id); if (!meta) return null;
            const isReport = id === "report";
            return (
              <div key={id} className="cmd-card">
                {!isReport && (
                  <div className="cmd-drag-handle">
                    <span className="cmd-card-title">{meta.icon} {meta.title}</span>
                    <button className="cmd-remove-btn" onClick={() => removeWidget(id)} title={t("widget.hideWidget")}>×</button>
                  </div>
                )}
                {meta.render(data)}
              </div>
            );
          })}
        </GridLayout>
      </div>
    </div>
  );
}
