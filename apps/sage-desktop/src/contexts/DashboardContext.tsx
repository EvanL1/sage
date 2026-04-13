import { createContext, useContext, useEffect, useReducer, useCallback, ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { invokeDeduped, invalidateCachePrefix } from "../utils/invokeCache";
import type { DashStats, ReportData, DisplayItem } from "../layouts/types";
import type { TaskItem } from "../types";

/* ═══ State Shape ═══ */

interface MessageItem {
  sender?: string;
  channel?: string;
  content?: string;
  created_at?: string;
  direction?: string;
}

interface TaskSignalLight {
  id: number;
  signalType: string;
  title: string;
}

interface ConnectionStatus {
  [key: string]: { status: string; label: string };
}

interface DailyQuestion {
  response: string;
}

export interface MeetingEvent {
  subject: string;
  start: string;
  end: string;
  location: string;
  attendees: string;
  organizer: string;
  status: "past" | "now" | "upcoming";
}

export interface DashboardState {
  stats: DashStats | null;
  report: { type: string; data: ReportData } | null;
  items: DisplayItem[];
  curated: DisplayItem[];
  question: DailyQuestion | null;
  tasks: TaskItem[];
  taskSignals: TaskSignalLight[];
  messages: MessageItem[];
  connections: ConnectionStatus;
  events: MeetingEvent[];
  loading: boolean;
}

/* ═══ Actions ═══ */

type Action =
  | { type: "SET_STATS"; payload: DashStats }
  | { type: "SET_REPORT"; payload: { type: string; data: ReportData } | null }
  | { type: "SET_ITEMS"; payload: DisplayItem[] }
  | { type: "SET_CURATED"; payload: DisplayItem[] }
  | { type: "SET_QUESTION"; payload: DailyQuestion | null }
  | { type: "SET_TASKS"; payload: TaskItem[] }
  | { type: "SET_TASK_SIGNALS"; payload: TaskSignalLight[] }
  | { type: "SET_MESSAGES"; payload: MessageItem[] }
  | { type: "SET_CONNECTIONS"; payload: ConnectionStatus }
  | { type: "SET_EVENTS"; payload: MeetingEvent[] }
  | { type: "SET_LOADING"; payload: boolean };

const initial: DashboardState = {
  stats: null,
  report: null,
  items: [],
  curated: [],
  question: null,
  tasks: [],
  taskSignals: [],
  messages: [],
  connections: {},
  events: [],
  loading: true,
};

function reducer(state: DashboardState, action: Action): DashboardState {
  switch (action.type) {
    case "SET_STATS":       return { ...state, stats: action.payload };
    case "SET_REPORT":      return { ...state, report: action.payload };
    case "SET_ITEMS":       return { ...state, items: action.payload };
    case "SET_CURATED":     return { ...state, curated: action.payload };
    case "SET_QUESTION":    return { ...state, question: action.payload };
    case "SET_TASKS":       return { ...state, tasks: action.payload };
    case "SET_TASK_SIGNALS":return { ...state, taskSignals: action.payload };
    case "SET_MESSAGES":    return { ...state, messages: action.payload };
    case "SET_CONNECTIONS": return { ...state, connections: action.payload };
    case "SET_EVENTS":      return { ...state, events: action.payload };
    case "SET_LOADING":     return { ...state, loading: action.payload };
    default:                return state;
  }
}

/* ═══ Context ═══ */

interface DashboardContextValue {
  state: DashboardState;
  refresh: (domain?: string) => Promise<void>;
}

const DashboardContext = createContext<DashboardContextValue | null>(null);

/* ═══ Fetch helpers ═══ */

async function fetchStats(dispatch: (a: Action) => void) {
  try {
    const s = await invokeDeduped<DashStats>("get_dashboard_stats");
    dispatch({ type: "SET_STATS", payload: s });
  } catch {}
}

async function fetchReport(dispatch: (a: Action) => void) {
  try {
    const r = await invokeDeduped<Record<string, ReportData>>("get_latest_reports");
    let found: { type: string; data: ReportData } | null = null;
    for (const rt of ["morning", "evening", "weekly", "week_start"]) {
      if (r[rt]) { found = { type: rt, data: r[rt] }; break; }
    }
    dispatch({ type: "SET_REPORT", payload: found });
  } catch {}
}

async function fetchItems(dispatch: (a: Action) => void) {
  try {
    const snap = await invokeDeduped<DisplayItem[]>("get_dashboard_snapshot");
    dispatch({ type: "SET_ITEMS", payload: snap.filter(i => i.category !== "report") });
  } catch {}
}

async function fetchCurated(dispatch: (a: Action) => void) {
  try {
    const c = await invokeDeduped<DisplayItem[]>("curate_homepage");
    if (c?.length) {
      dispatch({ type: "SET_CURATED", payload: c.filter(i => i.category !== "greeting" && i.category !== "report") });
    }
  } catch {}
}

async function fetchQuestion(dispatch: (a: Action) => void) {
  try {
    const q = await invokeDeduped<DailyQuestion | null>("get_daily_question");
    dispatch({ type: "SET_QUESTION", payload: q });
  } catch {}
}

async function fetchTasks(dispatch: (a: Action) => void) {
  try {
    const tasks = await invokeDeduped<TaskItem[]>("list_tasks", { status: "open", limit: 8 });
    dispatch({ type: "SET_TASKS", payload: tasks });
  } catch {}
}

async function fetchTaskSignals(dispatch: (a: Action) => void) {
  try {
    const signals = await invokeDeduped<TaskSignalLight[]>("get_task_signals");
    dispatch({ type: "SET_TASK_SIGNALS", payload: Array.isArray(signals) ? signals : [] });
  } catch {}
}

async function fetchMessages(dispatch: (a: Action) => void) {
  try {
    const msgs = await invokeDeduped<MessageItem[]>("get_messages", { limit: 8 });
    dispatch({ type: "SET_MESSAGES", payload: msgs });
  } catch {}
}

async function fetchConnections(dispatch: (a: Action) => void) {
  try {
    const status = await invokeDeduped<ConnectionStatus>("get_connections_status");
    dispatch({ type: "SET_CONNECTIONS", payload: status });
  } catch {}
}

async function fetchEvents(dispatch: (a: Action) => void) {
  try {
    const events = await invokeDeduped<MeetingEvent[]>("get_today_events");
    dispatch({ type: "SET_EVENTS", payload: Array.isArray(events) ? events : [] });
  } catch {}
}

/* ═══ Provider ═══ */

export function DashboardProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initial);

  const fetchAll = useCallback((): Promise<void> => {
    return Promise.all([
      fetchStats(dispatch),
      fetchReport(dispatch),
      fetchItems(dispatch),
      fetchCurated(dispatch),
      fetchQuestion(dispatch),
      fetchTasks(dispatch),
      fetchTaskSignals(dispatch),
      fetchMessages(dispatch),
      fetchConnections(dispatch),
      fetchEvents(dispatch),
    ]).then(() => {}).finally(() => dispatch({ type: "SET_LOADING", payload: false }));
  }, []);

  // Single mount fetch
  useEffect(() => { fetchAll(); }, [fetchAll]);

  // Tauri event listeners for targeted re-fetches
  useEffect(() => {
    let aborted = false;
    const unsubs: (() => void)[] = [];

    const reg = async (event: string, handler: () => void) => {
      const unsub = await listen(event, handler);
      if (aborted) { unsub(); return; }
      unsubs.push(unsub);
    };

    reg("sage:data:reports", () => {
      invalidateCachePrefix("get_latest_reports");
      fetchReport(dispatch);
    });
    reg("sage:data:tasks", () => {
      invalidateCachePrefix("list_tasks");
      invalidateCachePrefix("get_task_signals");
      fetchTasks(dispatch);
      fetchTaskSignals(dispatch);
    });
    reg("sage:data:memories", () => {
      invalidateCachePrefix("get_dashboard_snapshot");
      invalidateCachePrefix("curate_homepage");
      invalidateCachePrefix("get_dashboard_stats");
      fetchStats(dispatch);
      fetchItems(dispatch);
      fetchCurated(dispatch);
    });
    reg("sage:data:messages", () => {
      invalidateCachePrefix("get_messages");
      fetchMessages(dispatch);
    });
    reg("sage:data:refresh", () => {
      invalidateCachePrefix("");
      fetchAll();
    });

    return () => {
      aborted = true;
      unsubs.forEach(fn => fn());
    };
  }, [fetchAll]);

  const refresh = useCallback((domain?: string): Promise<void> => {
    if (!domain || domain === "all") {
      invalidateCachePrefix("");
      return fetchAll();
    }
    if (domain === "tasks") {
      invalidateCachePrefix("list_tasks");
      invalidateCachePrefix("get_task_signals");
      return Promise.all([fetchTasks(dispatch), fetchTaskSignals(dispatch)]).then(() => {});
    } else if (domain === "memories") {
      invalidateCachePrefix("get_dashboard_snapshot");
      invalidateCachePrefix("curate_homepage");
      invalidateCachePrefix("get_dashboard_stats");
      return Promise.all([fetchStats(dispatch), fetchItems(dispatch), fetchCurated(dispatch)]).then(() => {});
    } else if (domain === "messages") {
      invalidateCachePrefix("get_messages");
      return fetchMessages(dispatch);
    } else if (domain === "connections") {
      invalidateCachePrefix("get_connections_status");
      return fetchConnections(dispatch);
    } else if (domain === "report") {
      invalidateCachePrefix("get_latest_reports");
      return fetchReport(dispatch);
    } else if (domain === "question") {
      invalidateCachePrefix("get_daily_question");
      return fetchQuestion(dispatch);
    } else if (domain === "events") {
      invalidateCachePrefix("get_today_events");
      return fetchEvents(dispatch);
    }
    return Promise.resolve();
  }, [fetchAll]);

  return (
    <DashboardContext.Provider value={{ state, refresh }}>
      {children}
    </DashboardContext.Provider>
  );
}

/* ═══ Hook ═══ */

export function useDashboard(): DashboardContextValue {
  const ctx = useContext(DashboardContext);
  if (!ctx) throw new Error("useDashboard must be used inside DashboardProvider");
  return ctx;
}

/* ═══ Re-exports for consumers ═══ */

export type { MessageItem, TaskSignalLight, ConnectionStatus, DailyQuestion };
