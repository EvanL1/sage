import { useEffect, useState, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import FeedbackButtons, { FeedbackValue, actionToFeedback } from "../components/FeedbackButtons";

interface Suggestion {
  id: number;
  event_source: string;
  response: string;
  timestamp: string;
  feedback: FeedbackValue | null;
}

function formatDate(ts: string): string {
  try {
    const d = new Date(ts);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    if (d.toDateString() === today.toDateString()) return "今天";
    if (d.toDateString() === yesterday.toDateString()) return "昨天";
    return d.toLocaleDateString("zh-CN", { month: "long", day: "numeric" });
  } catch {
    return ts;
  }
}

function formatTime(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}

function sourceLabel(source: string): string {
  const map: Record<string, string> = { email: "邮件", calendar: "日历", heartbeat: "定时", hook: "Hook" };
  return map[source] ?? source;
}

function History() {
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [search, setSearch] = useState("");

  useEffect(() => {
    invoke<Suggestion[]>("get_suggestions", { limit: 50 }).then(setSuggestions).catch(console.error);
  }, []);

  const filtered = useMemo(() => {
    if (!search.trim()) return suggestions;
    const q = search.toLowerCase();
    return suggestions.filter(
      (s) => s.response.toLowerCase().includes(q) || s.event_source.toLowerCase().includes(q)
    );
  }, [suggestions, search]);

  const grouped = useMemo(() => {
    const groups: { label: string; items: Suggestion[] }[] = [];
    for (const s of filtered) {
      const label = formatDate(s.timestamp);
      const last = groups[groups.length - 1];
      if (last && last.label === label) {
        last.items.push(s);
      } else {
        groups.push({ label, items: [s] });
      }
    }
    return groups;
  }, [filtered]);

  const handleFeedback = async (id: number, action: string) => {
    await invoke("submit_feedback", { suggestionId: id, action });
    setSuggestions((prev) =>
      prev.map((s) => (s.id === id ? { ...s, feedback: actionToFeedback(action) } : s))
    );
  };

  return (
    <div className="page">
      <div className="page-header">
        <h1>建议历史</h1>
        <p>查看和管理 Sage 的所有建议</p>
      </div>

      <div className="search-bar">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="搜索建议内容..." />
      </div>

      {filtered.length === 0 ? (
        <div className="card">
          <div className="empty-state">
            <h3>{search ? "未找到匹配结果" : "暂无历史记录"}</h3>
            <p>{search ? "尝试其他关键词" : "Sage 的建议将显示在这里"}</p>
          </div>
        </div>
      ) : (
        grouped.map((group) => (
          <div key={group.label} className="date-group">
            <div className="date-group-label">{group.label}</div>
            <div className="suggestion-stream">
              {group.items.map((s) => (
                <div key={s.id} className="suggestion-bubble">
                  <div className="suggestion-header">
                    <span className="suggestion-source">{sourceLabel(s.event_source)}</span>
                    <span className="suggestion-time">{formatTime(s.timestamp)}</span>
                  </div>
                  <div className="suggestion-body"><ReactMarkdown>{s.response}</ReactMarkdown></div>
                  <FeedbackButtons suggestionId={s.id} feedback={s.feedback} onSubmit={handleFeedback} />
                </div>
              ))}
            </div>
          </div>
        ))
      )}
    </div>
  );
}

export default History;
