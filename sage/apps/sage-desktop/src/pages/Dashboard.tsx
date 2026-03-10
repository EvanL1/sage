import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import FeedbackButtons, { FeedbackValue, actionToFeedback } from "../components/FeedbackButtons";

interface SystemStatus {
  status: string;
  has_profile: boolean;
  sop_version: number;
}

interface Suggestion {
  id: number;
  event_source: string;
  response: string;
  timestamp: string;
  feedback: FeedbackValue | null;
}

interface Report {
  id: number;
  report_type: string;
  content: string;
  created_at: string;
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

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return ts;
  }
}

function sourceLabel(source: string): string {
  const map: Record<string, string> = {
    email: "Email",
    calendar: "Calendar",
    heartbeat: "Scheduled",
    hook: "Hook",
  };
  return map[source] ?? source;
}

function Dashboard() {
  const navigate = useNavigate();
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [reports, setReports] = useState<Record<string, Report>>({});
  const [dailyQuestion, setDailyQuestion] = useState<DailyQuestion | null>(null);
  // 触发报告的加载状态：key 为报告类型，value 为是否正在加载
  const [triggeringReport, setTriggeringReport] = useState<Record<string, boolean>>({});

  // Status: load only on mount
  useEffect(() => {
    invoke<SystemStatus>("get_system_status")
      .then((s) => {
        setStatus(s);
        if (!s.has_profile) {
          navigate("/welcome", { replace: true });
        }
      })
      .catch(() => navigate("/welcome", { replace: true }));
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
          <div className="stat-label">SOP version</div>
          <div className="stat-value">{status ? `v${status.sop_version}` : "—"}</div>
        </div>
      </div>

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
                <span className="suggestion-time">{formatTime(s.timestamp)}</span>
              </div>
              <div className="suggestion-body"><ReactMarkdown remarkPlugins={[remarkGfm]}>{s.response}</ReactMarkdown></div>
              <FeedbackButtons suggestionId={s.id} feedback={s.feedback} onSubmit={handleFeedback} />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default Dashboard;
