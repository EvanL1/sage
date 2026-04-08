export interface DisplayItem {
  id?: number;
  ref_id?: string;
  content: string;
  category: string;
  meta?: string;
  about_person?: string;
}

export interface DashStats {
  memories: number;
  edges: number;
  sessions: number;
  messages: number;
  tag_count: number;
  top_tags: { tag: string; count: number }[];
  known_persons: number;
}

export interface ReportData { content: string; created_at: string; }

export interface DashData {
  stats: DashStats | null;
  report: { type: string; data: ReportData } | null;
  items: DisplayItem[];
  curated: DisplayItem[];
  reportLoading: string | null;
  triggerReport: (t: string) => void;
  openExpanded: (item: DisplayItem) => void;
}

export const TYPE_COLORS: Record<string, string> = {
  report: "var(--accent)", suggestion: "var(--accent)", schedule: "var(--accent)",
  session: "#6366f1", memory: "#22c55e", insight: "#6366f1",
  question: "#8b5cf6", people: "#ec4899", channel: "#14b8a6",
  data: "var(--accent)", greeting: "var(--text-tertiary)",
};

export const TYPE_LABEL: Record<string, string> = {
  report: "BRIEF", suggestion: "ADVISE", session: "CONV", memory: "MEMORY",
  insight: "INSIGHT", question: "QUERY", people: "CIRCLE", channel: "CHANNEL",
  schedule: "SCHED", data: "DATA",
};

export const reportLabel = (t: string) =>
  t === "morning" ? "Morning Brief" : t === "evening" ? "Evening Review" : t === "weekly" ? "Weekly Report" : "Week Start";

/** Strip markdown syntax for plain-text preview */
export function stripMd(s: string): string {
  return s
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/^>\s?/gm, "")
    .replace(/^---+$/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/__(.+?)__/g, "$1")
    .replace(/\*(.+?)\*/g, "$1")
    .replace(/_(.+?)_/g, "$1")
    .replace(/!\[.*?\]\(.*?\)/g, "")
    .replace(/\[(.+?)\]\(.*?\)/g, "$1")
    .replace(/`(.+?)`/g, "$1")
    .replace(/^[-*+]\s/gm, "· ")
    .replace(/^\d+\.\s/gm, "")
    .replace(/\n{2,}/g, " · ")
    .replace(/\n/g, " ")
    .trim();
}

export function preview(content: string, maxLen: number): string {
  const clean = stripMd(content);
  return clean.length > maxLen ? clean.slice(0, maxLen) + "…" : clean;
}
