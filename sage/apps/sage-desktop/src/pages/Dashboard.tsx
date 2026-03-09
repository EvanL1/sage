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

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return ts;
  }
}

function sourceLabel(source: string): string {
  const map: Record<string, string> = {
    email: "邮件",
    calendar: "日历",
    heartbeat: "定时",
    hook: "Hook",
  };
  return map[source] ?? source;
}

function Dashboard() {
  const navigate = useNavigate();
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);

  // Status: 只 mount 时加载
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

  // Suggestions: mount + 每 30 秒轮询
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

  const handleFeedback = async (id: number, action: string) => {
    await invoke("submit_feedback", { suggestionId: id, action });
    setSuggestions((prev) =>
      prev.map((s) => (s.id === id ? { ...s, feedback: actionToFeedback(action) } : s))
    );
  };

  // 等待 status 加载，避免 Dashboard → Welcome 闪烁
  if (!status) return null;

  const statusText = status.status === "ready"
    ? "运行中"
    : status.status === "needs_onboarding"
      ? "待设置"
      : status.status ?? "加载中";
  const isOnline = status.status === "ready";

  return (
    <div className="page">
      <div className="page-header">
        <h1>仪表板</h1>
        <p>Sage 个人参谋系统</p>
      </div>

      <div className="dashboard-grid">
        <div className="stat-card">
          <div className="stat-label">系统状态</div>
          <div className={`status-badge ${isOnline ? "online" : "idle"}`}>
            <span className="status-dot" />
            {statusText}
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">SOP 版本</div>
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
            <h3>开始使用 Sage</h3>
            <p>完成初始设置后，Sage 将根据你的角色和工作节奏提供个性化建议。</p>
            <Link to="/welcome" className="btn btn-primary">开始设置</Link>
          </div>
        </div>
      )}

      <div style={{ marginBottom: 12, display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span className="card-title" style={{ margin: 0 }}>最近建议</span>
        {suggestions.length > 0 && (
          <Link to="/history" className="btn btn-ghost btn-sm">查看全部</Link>
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
            <h3>还没有建议，慢慢来</h3>
            <p style={{ marginBottom: 8 }}>
              Sage 会根据你的工作节奏自动生成建议 — 早间简报、邮件摘要、晚间回顾。
            </p>
            <p style={{ marginBottom: 16, opacity: 0.7, fontSize: "0.9em" }}>
              不想等？随时可以找我聊聊。
            </p>
            <Link to="/chat" className="btn btn-primary">和 Sage 聊天</Link>
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
