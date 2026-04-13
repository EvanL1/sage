import { useMemo } from "react";
import { useNavigate } from "react-router-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { DashData, TYPE_LABEL, reportLabel } from "./types";
import { useDashboard } from "../contexts/DashboardContext";

export default function ClassicLayout({ data, navigate }: { data: DashData; navigate: ReturnType<typeof useNavigate> }) {
  const { state } = useDashboard();
  const meetings = state.events;

  const statusColor = (s: string) =>
    s === "now" ? "var(--success, #22c55e)" : s === "upcoming" ? "var(--accent)" : "var(--text-tertiary)";
  const statusLabel = (s: string) =>
    s === "now" ? "进行中" : s === "upcoming" ? "即将" : "已结束";

  const allItems = useMemo(() => {
    const raw = [...data.curated, ...data.items].filter(
      i => i.category !== "report" && i.category !== "question"
    );
    const seen = new Set<string | number>();
    return raw.filter(item => {
      const key = item.id ?? item.ref_id ?? item.content.slice(0, 50);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  }, [data.curated, data.items]);

  return (
    <div className="classic-scroll">
      <div className="dashboard-grid">
        <div className="stat-card"><div className="stat-label">Memories</div><div className="stat-value">{data.stats?.memories ?? "—"}</div></div>
        <div className="stat-card"><div className="stat-label">Links</div><div className="stat-value">{data.stats?.edges ?? "—"}</div></div>
      </div>

      {/* 今日会议 */}
      <div className="card">
        <div className="card-title" style={{ marginBottom: 8 }}>今日会议</div>
        {meetings.length > 0 ? meetings.map((m, i) => (
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
        )) : (
          <p style={{ color: "var(--text-tertiary)", fontSize: 12, margin: 0 }}>今日暂无会议</p>
        )}
      </div>

      {data.report && (
        <div className="card" style={{ cursor: "pointer" }} onClick={() => data.openExpanded({ content: data.report!.data.content, category: "report", ref_id: data.report!.type })}>
          <div className="card-title">{reportLabel(data.report.type)}</div>
          <div className="md-content" style={{ fontSize: 13, lineHeight: 1.6, color: "var(--text-secondary)" }}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{data.report.data.content.slice(0, 500)}</ReactMarkdown>
          </div>
        </div>
      )}
      <div className="card-title" style={{ marginTop: 8 }}>Recent activity</div>
      <div className="suggestion-stream">
        {allItems.slice(0, 10).map((item, i) => (
          <div key={i} className="suggestion-bubble" onClick={() => data.openExpanded(item)}>
            <div className="suggestion-header">
              <span className="suggestion-source">{TYPE_LABEL[item.category] ?? "INFO"}</span>
              {item.meta && <span className="suggestion-time">{item.meta}</span>}
            </div>
            <div className="suggestion-body"><ReactMarkdown remarkPlugins={[remarkGfm]}>{item.content}</ReactMarkdown></div>
          </div>
        ))}
        {allItems.length === 0 && (
          <div className="card" style={{ textAlign: "center", padding: 20 }}>
            <p style={{ color: "var(--text-secondary)" }}>暂无数据</p>
            <button className="btn btn-primary" style={{ marginTop: 8 }} onClick={() => navigate("/chat")}>Chat</button>
          </div>
        )}
      </div>
    </div>
  );
}
