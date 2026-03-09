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
