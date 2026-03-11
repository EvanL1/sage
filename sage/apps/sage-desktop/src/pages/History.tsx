import { useEffect, useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import FeedbackButtons, { actionToFeedback } from "../components/FeedbackButtons";
import type { Suggestion } from "../types";
import { formatDate, formatTime } from "../utils/time";
import { sourceLabel } from "../utils/labels";

function History() {
  const navigate = useNavigate();
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

  const handleDelete = async (id: number) => {
    try {
      await invoke("delete_suggestion", { suggestionId: id });
      setSuggestions((prev) => prev.filter((s) => s.id !== id));
    } catch (err) {
      console.error("Failed to delete suggestion:", err);
    }
  };

  const handleTalk = (s: Suggestion) => {
    const preview = s.response.length > 60 ? s.response.slice(0, 60) + "..." : s.response;
    navigate("/chat", {
      state: { initialMessage: `关于 Sage 之前的建议「${preview}」——我想聊聊这个。` },
    });
  };

  return (
    <div className="page">
      <div className="page-header">
        <h1>Suggestion history</h1>
        <p>View and manage all of Sage's suggestions</p>
      </div>

      <div className="search-bar">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search suggestions..." />
      </div>

      {filtered.length === 0 ? (
        <div className="card">
          <div className="empty-state">
            <h3>{search ? "No matching results" : "No history yet"}</h3>
            <p>{search ? "Try different keywords" : "Sage's suggestions will appear here"}</p>
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
                    <div className="suggestion-header-right">
                      <span className="suggestion-time">{formatTime(s.timestamp)}</span>
                      <button
                        className="suggestion-action-btn"
                        onClick={() => handleTalk(s)}
                        title="聊聊这个"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                        </svg>
                      </button>
                      <button
                        className="suggestion-action-btn suggestion-delete-btn"
                        onClick={() => handleDelete(s.id)}
                        title="Delete"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <line x1="18" y1="6" x2="6" y2="18" />
                          <line x1="6" y1="6" x2="18" y2="18" />
                        </svg>
                      </button>
                    </div>
                  </div>
                  <div className="suggestion-body"><ReactMarkdown remarkPlugins={[remarkGfm]}>{s.response}</ReactMarkdown></div>
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
