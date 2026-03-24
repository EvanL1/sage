import { useNavigate } from "react-router-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { DashData, TYPE_LABEL, reportLabel } from "./types";

export default function ClassicLayout({ data, navigate }: { data: DashData; navigate: ReturnType<typeof useNavigate> }) {
  const allItems = [...data.curated, ...data.items];

  return (
    <div className="classic-scroll">
      <div className="dashboard-grid">
        <div className="stat-card"><div className="stat-label">Memories</div><div className="stat-value">{data.stats?.memories ?? "—"}</div></div>
        <div className="stat-card"><div className="stat-label">Links</div><div className="stat-value">{data.stats?.edges ?? "—"}</div></div>
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
