import { useEffect, useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import FeedbackButtons, { actionToFeedback } from "../components/FeedbackButtons";
import type { Suggestion } from "../types";
import { formatDate, formatTime } from "../utils/time";
import { sourceLabel } from "../utils/labels";
import { useLang } from "../LangContext";

function History() {
  const { t } = useLang();
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
      state: { initialMessage: `${t("history.talkMsg")}${preview}${t("history.talkMsgSuffix")}` },
    });
  };

  return (
    <div className="page">
      <div className="page-header">
        <h1>{t("history.title")}</h1>
        <p>{t("history.subtitle")}</p>
      </div>

      <div className="search-bar">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("history.searchPlaceholder")} />
      </div>

      {filtered.length === 0 ? (
        <div className="card">
          <div className="empty-state">
            <h3>{search ? t("history.noResults") : t("history.noHistory")}</h3>
            <p>{search ? t("history.tryKeywords") : t("history.sageSuggestions")}</p>
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
                        title={t("history.talkAboutThis")}
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                        </svg>
                      </button>
                      <button
                        className="suggestion-action-btn suggestion-delete-btn"
                        onClick={() => handleDelete(s.id)}
                        title={t("delete")}
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
