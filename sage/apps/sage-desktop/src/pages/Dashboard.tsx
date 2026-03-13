import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import FeedbackButtons, { actionToFeedback } from "../components/FeedbackButtons";
import type { Suggestion, Report } from "../types";
import { formatTime } from "../utils/time";
import { sourceLabel } from "../utils/labels";

interface SystemStatus {
  status: string;
  has_profile: boolean;
}

interface DailyQuestion {
  id: number;
  event_source: string;
  prompt: string;
  response: string;
  timestamp: string;
}

const REPORT_LABELS: Record<string, string> = {
  morning: "Morning Brief",
  evening: "Evening Review",
  weekly: "Weekly Report",
  week_start: "Week Start",
};

function Dashboard() {
  const navigate = useNavigate();
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [reports, setReports] = useState<Record<string, Report>>({});
  const [dailyQuestion, setDailyQuestion] = useState<DailyQuestion | null>(null);
  const [triggeringReport, setTriggeringReport] = useState<Record<string, boolean>>({});
  const [memoryCount, setMemoryCount] = useState<number | null>(null);
  const [topTags, setTopTags] = useState<{ tag: string; count: number }[]>([]);
  const [connections, setConnections] = useState<Record<string, { status: string; label: string }> | null>(null);

  // Status + memory count: load on mount
  useEffect(() => {
    invoke<SystemStatus>("get_system_status")
      .then((s) => {
        setStatus(s);
        if (!s.has_profile) {
          navigate("/welcome", { replace: true });
        }
      })
      .catch(() => navigate("/welcome", { replace: true }));
    invoke<{ id: number }[]>("get_memories")
      .then((m) => setMemoryCount(m.length))
      .catch(() => setMemoryCount(0));
    invoke<{ tag: string; count: number }[]>("get_all_tags")
      .then((tags) => setTopTags(tags.slice(0, 8)))
      .catch(() => {});
    const fetchConnections = () => {
      invoke<Record<string, { status: string; label: string }>>("get_connections_status")
        .then(setConnections)
        .catch(() => {});
    };
    fetchConnections();
    const connTimer = setInterval(fetchConnections, 15_000);
    return () => clearInterval(connTimer);
  }, [navigate]);

  // Suggestions: mount + poll every 30s
  useEffect(() => {
    const fetchSuggestions = () => {
      invoke<Suggestion[]>("get_suggestions", { limit: 5 })
        .then(setSuggestions)
        .catch(console.error);
    };
    fetchSuggestions();
    const timer = setInterval(fetchSuggestions, 30_000);
    return () => clearInterval(timer);
  }, []);

  // Reports: mount + poll every 60s
  useEffect(() => {
    const fetchReports = () => {
      invoke<Record<string, Report>>("get_latest_reports")
        .then(setReports)
        .catch(console.error);
    };
    fetchReports();
    const timer = setInterval(fetchReports, 60_000);
    return () => clearInterval(timer);
  }, []);

  // 今日思考：mount + poll every 60s
  useEffect(() => {
    const fetchDailyQuestion = () => {
      invoke<DailyQuestion | null>("get_daily_question")
        .then(setDailyQuestion)
        .catch(console.error);
    };
    fetchDailyQuestion();
    const timer = setInterval(fetchDailyQuestion, 60_000);
    return () => clearInterval(timer);
  }, []);

  // 手动触发报告生成
  const handleTriggerReport = async (reportType: string) => {
    setTriggeringReport((prev) => ({ ...prev, [reportType]: true }));
    try {
      await invoke("trigger_report", { reportType });
      // 触发后立即刷新报告列表
      const updated = await invoke<Record<string, Report>>("get_latest_reports");
      setReports(updated);
    } catch (err) {
      console.error("触发报告失败:", err);
    } finally {
      setTriggeringReport((prev) => ({ ...prev, [reportType]: false }));
    }
  };

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

  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");

  const startEdit = (s: Suggestion) => {
    setEditingId(s.id);
    setEditText(s.response);
  };

  const saveEdit = async () => {
    if (editingId === null) return;
    try {
      await invoke("update_suggestion", { suggestionId: editingId, response: editText });
      setSuggestions((prev) =>
        prev.map((s) => (s.id === editingId ? { ...s, response: editText } : s))
      );
      setEditingId(null);
    } catch (err) {
      console.error("Failed to update suggestion:", err);
    }
  };

  // Wait for status to load to avoid Dashboard → Welcome flicker
  if (!status) return null;

  const statusText = status.status === "ready"
    ? "Running"
    : status.status === "needs_onboarding"
      ? "Setup required"
      : status.status ?? "Loading";
  const isOnline = status.status === "ready";

  return (
    <div className="page">
      <div className="page-header">
        <h1>Dashboard</h1>
        <p>Sage personal advisor system</p>
      </div>

      <div className="dashboard-grid">
        <div className="stat-card">
          <div className="stat-label">System status</div>
          <div className={`status-badge ${isOnline ? "online" : "idle"}`}>
            <span className="status-dot" />
            {statusText}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Memories</div>
          <div className="stat-value">{memoryCount !== null ? memoryCount : "—"}</div>
        </div>
      </div>

      {connections && (
        <div className="connections-panel" style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
          gap: "var(--spacing-sm)",
          marginBottom: "var(--spacing-lg)",
        }}>
          {[
            { key: "browser_extension", name: "浏览器扩展" },
            { key: "outlook", name: "Outlook" },
            { key: "claude_code", name: "Claude Code" },
            { key: "behavior_tracking", name: "行为追踪" },
          ].map(({ key, name }) => {
            const conn = connections[key];
            if (!conn) return null;
            const dotColor = conn.status === "connected" ? "var(--green, #22c55e)"
              : conn.status === "stale" ? "var(--accent, #d97706)"
              : conn.status === "idle" ? "var(--text-tertiary)"
              : conn.status === "never" ? "var(--text-tertiary)"
              : "var(--red, #ef4444)";
            return (
              <div key={key} style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "8px 12px",
                background: "var(--surface)",
                borderRadius: "var(--radius)",
                border: "1px solid var(--border)",
                fontSize: 13,
              }}>
                <span style={{
                  width: 7,
                  height: 7,
                  borderRadius: "50%",
                  background: dotColor,
                  flexShrink: 0,
                }} />
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontWeight: 500, fontSize: 12, color: "var(--text-secondary)" }}>{name}</div>
                  <div style={{ fontSize: 11, color: "var(--text-tertiary)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{conn.label}</div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {topTags.length > 0 && (
        <div className="dashboard-tags">
          <span className="dashboard-tags-label">Memory tags</span>
          <div className="dashboard-tags-list">
            {topTags.map(({ tag, count }) => (
              <Link key={tag} to="/about" className="tag-chip">
                {tag} <span className="tag-chip-count">{count}</span>
              </Link>
            ))}
          </div>
        </div>
      )}

      {!status?.has_profile && (
        <div className="card">
          <div className="empty-state">
            <div className="empty-state-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 2L2 7l10 5 10-5-10-5z" />
                <path d="M2 17l10 5 10-5" />
                <path d="M2 12l10 5 10-5" />
              </svg>
            </div>
            <h3>Get started with Sage</h3>
            <p>Complete the initial setup and Sage will provide personalized suggestions based on your role and work rhythm.</p>
            <Link to="/welcome" className="btn btn-primary">Start setup</Link>
          </div>
        </div>
      )}

      {Object.keys(reports).length > 0 && (
        <div className="reports-section">
          <div style={{ marginBottom: 10, display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <span className="card-title" style={{ margin: 0 }}>Reports</span>
            {/* 手动触发报告按钮组 */}
            <div style={{ display: "flex", gap: 4 }}>
              {(["morning", "evening"] as const).map((type) => (
                <button
                  key={type}
                  className="btn btn-ghost btn-sm"
                  onClick={() => handleTriggerReport(type)}
                  disabled={triggeringReport[type]}
                  title={`Generate ${REPORT_LABELS[type]}`}
                  style={{ display: "flex", alignItems: "center", gap: 4 }}
                >
                  {triggeringReport[type] ? (
                    /* 旋转加载指示器 */
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
                      style={{ animation: "spin 1s linear infinite" }}>
                      <path d="M21 12a9 9 0 1 1-6.219-8.56" />
                    </svg>
                  ) : (
                    /* 刷新图标 */
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="23 4 23 10 17 10" />
                      <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                    </svg>
                  )}
                  {REPORT_LABELS[type]}
                </button>
              ))}
            </div>
          </div>
          {Object.entries(reports).map(([type, report]) => (
            <details
              key={type}
              className="report-card"
              open={type === "weekly" || type === "morning"}
            >
              <summary>
                <span className="report-type">{REPORT_LABELS[type] ?? type}</span>
                <span className="report-time">{formatTime(report.created_at)}</span>
              </summary>
              <div className="report-body">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{report.content}</ReactMarkdown>
              </div>
            </details>
          ))}
        </div>
      )}

      {/* 今日思考卡片：Questioner 模块生成的苏格拉底式每日问题 */}
      {dailyQuestion && (
        <div className="daily-question-card" style={{
          background: "var(--surface)",
          border: "1px solid var(--accent)",
          borderRadius: "var(--radius-lg)",
          padding: "var(--spacing-lg)",
          marginBottom: "var(--spacing-lg)",
          boxShadow: "0 1px 6px rgba(217, 119, 6, 0.1)",
        }}>
          <div style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginBottom: 10,
          }}>
            {/* 灯泡图标 */}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)"
              strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="9" y1="18" x2="15" y2="18" />
              <line x1="10" y1="22" x2="14" y2="22" />
              <path d="M15.09 14c.18-.98.65-1.74 1.41-2.5A4.65 4.65 0 0 0 18 8 6 6 0 0 0 6 8c0 1 .23 2.23 1.5 3.5A4.61 4.61 0 0 1 8.91 14" />
            </svg>
            <span className="card-title" style={{ margin: 0, color: "var(--accent-text)" }}>
              今日思考
            </span>
          </div>
          <p style={{
            fontSize: 14,
            lineHeight: 1.65,
            color: "var(--text)",
            margin: 0,
            marginBottom: 10,
            fontStyle: "italic",
          }}>
            {dailyQuestion.response}
          </p>
          <div style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}>
            <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
              Sage's daily question · {formatTime(dailyQuestion.timestamp)}
            </span>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => navigate("/chat", {
                state: { initialMessage: `关于你的问题「${dailyQuestion.response}」——我想聊聊这个。` },
              })}
              style={{ fontSize: 12 }}
            >
              聊聊这个 →
            </button>
          </div>
        </div>
      )}

      <div style={{ marginBottom: 12, display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span className="card-title" style={{ margin: 0 }}>Recent suggestions</span>
        {suggestions.length > 0 && (
          <Link to="/history" className="btn btn-ghost btn-sm">View all</Link>
        )}
      </div>

      {suggestions.length === 0 ? (
        <div className="card">
          <div className="empty-state">
            <div className="empty-state-icon">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
                <path d="M8 14s1.5 2 4 2 4-2 4-2" />
                <line x1="9" y1="9" x2="9.01" y2="9" />
                <line x1="15" y1="9" x2="15.01" y2="9" />
              </svg>
            </div>
            <h3>No suggestions yet — take your time</h3>
            <p style={{ marginBottom: 8 }}>
              Sage automatically generates suggestions based on your work rhythm — morning brief, email summaries, evening review.
            </p>
            <p style={{ marginBottom: 16, opacity: 0.7, fontSize: "0.9em" }}>
              Don't want to wait? Feel free to chat with me anytime.
            </p>
            <Link to="/chat" className="btn btn-primary">Chat with Sage</Link>
          </div>
        </div>
      ) : (
        <div className="suggestion-stream">
          {suggestions.map((s) => (
            <div key={s.id} className="suggestion-bubble">
              <div className="suggestion-header">
                <span className="suggestion-source">{sourceLabel(s.event_source)}</span>
                <div className="suggestion-header-right">
                  <span className="suggestion-time">{formatTime(s.timestamp)}</span>
                  <button
                    className="suggestion-action-btn"
                    onClick={() => startEdit(s)}
                    title="Edit"
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
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
      )}

      {editingId !== null && (
        <div className="edit-modal-overlay" onClick={() => setEditingId(null)}>
          <div className="edit-modal" onClick={(e) => e.stopPropagation()}>
            <div className="edit-modal-header">
              <span>Edit suggestion</span>
              <button className="suggestion-action-btn" onClick={() => setEditingId(null)} title="Close">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            </div>
            <textarea
              className="edit-modal-textarea"
              value={editText}
              onChange={(e) => setEditText(e.target.value)}
              autoFocus
            />
            <div className="edit-modal-actions">
              <button className="btn btn-ghost" onClick={() => setEditingId(null)}>Cancel</button>
              <button className="btn btn-primary" onClick={saveEdit}>Save</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Dashboard;
