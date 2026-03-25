import React, { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { DisplayItem, DashStats, ReportData, DashData } from "../layouts/types";
import { useLang } from "../LangContext";

import CommandLayout from "../layouts/CommandLayout";
import NebulaLayout from "../layouts/NebulaLayout";
import ClassicLayout from "../layouts/ClassicLayout";
import InteractiveReport from "../components/InteractiveReport";

type Layout = "command" | "nebula" | "classic";

type LayoutKey = "command" | "nebula" | "classic";
const LAYOUT_KEYS: LayoutKey[] = ["command", "nebula", "classic"];

class LayoutErrorBoundary extends React.Component<
  { children: React.ReactNode; onReset: () => void; errorLabel: string; resetLabel: string },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };
  static getDerivedStateFromError(error: Error) { return { error }; }
  render() {
    if (this.state.error) return (
      <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 12, color: "var(--text-secondary)", fontFamily: "var(--font-mono)", fontSize: 12 }}>
        <div style={{ color: "var(--error)", fontSize: 14, fontWeight: 600 }}>{this.props.errorLabel}</div>
        <div style={{ maxWidth: 400, textAlign: "center", opacity: 0.7 }}>{this.state.error.message}</div>
        <button className="btn btn-primary" onClick={() => { this.setState({ error: null }); this.props.onReset(); }}>{this.props.resetLabel}</button>
      </div>
    );
    return this.props.children;
  }
}

const VALID_LAYOUTS = new Set<string>(LAYOUT_KEYS);

function Dashboard() {
  const navigate = useNavigate();
  const { t } = useLang();
  const [layout, setLayout] = useState<Layout>(() => {
    const saved = localStorage.getItem("dash_layout") as Layout;
    return VALID_LAYOUTS.has(saved) ? saved : "command";
  });
  const [stats, setStats] = useState<DashStats | null>(null);
  // depthCounts 暂不在 topbar 展示，数据仍加载以备后用
  const [, setDepthCounts] = useState<Record<string, number>>({});
  const [report, setReport] = useState<{ type: string; data: ReportData } | null>(null);
  const [items, setItems] = useState<DisplayItem[]>([]);
  const [curated, setCurated] = useState<DisplayItem[]>([]);
  const [expanded, setExpanded] = useState<DisplayItem | null>(null);
  const [expandedFull, setExpandedFull] = useState<string | null>(null);
  const [reportLoading, setReportLoading] = useState<string | null>(null);
  const [showCorrection, setShowCorrection] = useState(false);
  const [corrWrong, setCorrWrong] = useState("");
  const [corrFact, setCorrFact] = useState("");
  const [corrHint, setCorrHint] = useState("");
  const [zoom, setZoom] = useState(() => {
    const saved = parseFloat(localStorage.getItem("dash_zoom") || "1");
    return saved >= 0.5 && saved <= 1.5 ? saved : 1;
  });
  const adjustZoom = (delta: number) => {
    setZoom(prev => {
      const next = Math.round((prev + delta) * 100) / 100;
      const clamped = Math.max(0.5, Math.min(1.5, next));
      localStorage.setItem("dash_zoom", String(clamped));
      return clamped;
    });
  };

  useEffect(() => {
    invoke<{ status: string; has_profile: boolean }>("get_system_status")
      .then((s) => { if (!s.has_profile) navigate("/welcome", { replace: true }); })
      .catch(() => navigate("/welcome", { replace: true }));
  }, [navigate]);

  useEffect(() => {
    invoke<DashStats>("get_dashboard_stats").then(setStats).catch(() => {});
    invoke<{ depth?: string }[]>("get_memories").then((mems) => {
      const counts: Record<string, number> = {};
      for (const m of mems) {
        const d = m.depth ?? "episodic";
        counts[d] = (counts[d] ?? 0) + 1;
      }
      setDepthCounts(counts);
    }).catch(() => {});
    invoke<Record<string, ReportData>>("get_latest_reports")
      .then((r) => { for (const rt of ["morning", "evening", "weekly", "week_start"]) { if (r[rt]) { setReport({ type: rt, data: r[rt] }); break; } } })
      .catch(() => {});
    invoke<DisplayItem[]>("get_dashboard_snapshot")
      .then((snap) => setItems(snap.filter((i) => i.category !== "report")))
      .catch(() => {});
    invoke<DisplayItem[]>("curate_homepage")
      .then((c) => { if (c?.length) setCurated(c.filter((i) => i.category !== "greeting")); })
      .catch(() => {});
  }, []);

  const triggerReport = useCallback(async (reportType: string) => {
    setReportLoading(reportType);
    try {
      const content = await invoke<string>("trigger_report", { reportType });
      setReport({ type: reportType, data: { content, created_at: new Date().toISOString() } });
      invoke<DashStats>("get_dashboard_stats").then(setStats).catch(() => {});
    } catch (err) {
      setReport({ type: reportType, data: { content: `## Error\n\n${err}`, created_at: new Date().toISOString() } });
    } finally {
      setReportLoading(null);
    }
  }, []);

  const openExpanded = useCallback(async (item: DisplayItem) => {
    setExpanded(item); setExpandedFull(null);
    try {
      if (item.category === "session" && item.ref_id) {
        const msgs = await invoke<{ role: string; content: string; created_at: string }[]>("get_chat_history", { sessionId: item.ref_id });
        setExpandedFull(msgs.map((m) => `**${m.role === "user" ? "You" : "Sage"}** _${m.created_at.slice(11, 16)}_\n\n${m.content}`).join("\n\n---\n\n"));
      } else if (item.category === "report" && item.ref_id) {
        const reports = await invoke<Record<string, ReportData>>("get_latest_reports");
        if (reports[item.ref_id]) setExpandedFull(reports[item.ref_id].content);
      } else if (item.category === "memory" && item.id) {
        // Fetch full memory content by finding it in all memories
        const mems = await invoke<{ id: number; content: string; category: string; confidence: number }[]>("get_memories");
        const found = mems.find(m => m.id === item.id);
        if (found) setExpandedFull(found.content);
      } else if (item.category === "suggestion" && item.id) {
        const suggestions = await invoke<{ id: number; response: string; prompt: string }[]>("get_suggestions", { limit: 20 });
        const found = suggestions.find(s => s.id === item.id);
        if (found) setExpandedFull(`${found.response}\n\n---\n_Context: ${found.prompt}_`);
      }
    } catch {}
  }, []);

  const data: DashData = { stats, report, items, curated, reportLoading, triggerReport, openExpanded };

  const layoutLabels: Record<LayoutKey, string> = {
    command: t("dashboard.layoutCommand"),
    nebula:  t("dashboard.layoutNebula"),
    classic: t("dashboard.layoutClassic"),
  };

  const reportBtnLabel = (rt: string) => {
    if (reportLoading === rt) return "...";
    if (rt === "morning") return t("dashboard.reportAm");
    if (rt === "evening") return t("dashboard.reportPm");
    return t("dashboard.reportWk");
  };

  return (
    <div className="tl-page">
      <div className="tl-topbar">
        <div className="tl-actions">
          <div className="dash-layout-switcher">
            {LAYOUT_KEYS.map((key) => (
              <button key={key} className={`dash-layout-btn${layout === key ? " active" : ""}`}
                onClick={() => { setLayout(key); localStorage.setItem("dash_layout", key); }}
                title={key}>
                {layoutLabels[key]}
              </button>
            ))}
          </div>
          {(["morning", "evening", "weekly"] as const).map((rt) => (
            <button key={rt} className={`tl-trigger${reportLoading === rt ? " loading" : ""}`}
              onClick={() => triggerReport(rt)} disabled={reportLoading !== null}>
              {reportBtnLabel(rt)}
            </button>
          ))}
          <span className="dash-zoom">
            <button className="dash-zoom-btn" onClick={() => adjustZoom(-0.1)} disabled={zoom <= 0.5}>−</button>
            <span className="dash-zoom-label">{Math.round(zoom * 100)}%</span>
            <button className="dash-zoom-btn" onClick={() => adjustZoom(0.1)} disabled={zoom >= 1.5}>+</button>
          </span>
          <button className="tl-trigger tl-chat-btn" onClick={() => navigate("/chat")}>{t("dashboard.chat")}</button>
        </div>
      </div>

      <LayoutErrorBoundary key={layout} onReset={() => { setLayout("command"); localStorage.setItem("dash_layout", "command"); }}
        errorLabel={t("dashboard.layoutError")} resetLabel={t("dashboard.resetLayout")}>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", zoom, minHeight: 0 }}>
          {layout === "command" && <CommandLayout data={data} />}
          {layout === "nebula" && <NebulaLayout data={data} />}
          {layout === "classic" && <ClassicLayout data={data} navigate={navigate} />}
        </div>
      </LayoutErrorBoundary>

      {/* Expanded overlay */}
      {expanded && createPortal(
        <div className="subpage-overlay" onClick={() => { setExpanded(null); setShowCorrection(false); }}>
          <div className="subpage" onClick={(e) => e.stopPropagation()}>
            <div className="subpage-header">
              <span className="subpage-label">{expanded.category.toUpperCase()}</span>
              {expanded.meta && <span className="subpage-meta">{expanded.meta}</span>}
              <div style={{ flex: 1 }} />
              {expanded.category === "report" && (
                <button className="subpage-action" onClick={() => setShowCorrection(v => !v)}>
                  {showCorrection ? t("dashboard.cancelCorrection") : t("dashboard.correct")}
                </button>
              )}
              <button className="subpage-action primary" onClick={() => {
                const ctx = (expandedFull || expanded.content).slice(0, 300);
                setExpanded(null); setShowCorrection(false); navigate("/chat", { state: { quote: ctx } });
              }}>{t("dashboard.discuss")}</button>
              <button className="subpage-close" onClick={() => { setExpanded(null); setShowCorrection(false); }}>✕</button>
            </div>
            {showCorrection && (
              <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)", display: "flex", flexDirection: "column", gap: 8 }}>
                <input style={{ padding: "6px 10px", borderRadius: 6, border: "1px solid var(--border)", background: "var(--surface)", color: "var(--text)", fontSize: 12 }}
                  placeholder={t("dashboard.corrWrongPlaceholder")} value={corrWrong} onChange={e => setCorrWrong(e.target.value)} maxLength={200} />
                <input style={{ padding: "6px 10px", borderRadius: 6, border: "1px solid var(--border)", background: "var(--surface)", color: "var(--text)", fontSize: 12 }}
                  placeholder={t("dashboard.corrFactPlaceholder")} value={corrFact} onChange={e => setCorrFact(e.target.value)} maxLength={200} />
                <input style={{ padding: "6px 10px", borderRadius: 6, border: "1px solid var(--border)", background: "var(--surface)", color: "var(--text)", fontSize: 12 }}
                  placeholder={t("dashboard.corrHintPlaceholder")} value={corrHint} onChange={e => setCorrHint(e.target.value)} />
                <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                  <button className="btn btn-primary" disabled={corrWrong.length < 5 || corrFact.length < 5}
                    onClick={async () => {
                      const rt = expanded.ref_id ?? "morning";
                      await invoke("save_report_correction", { reportType: rt, wrongClaim: corrWrong, correctFact: corrFact, contextHint: corrHint });
                      setShowCorrection(false); setCorrWrong(""); setCorrFact(""); setCorrHint("");
                    }}>{t("dashboard.submitCorrection")}</button>
                </div>
              </div>
            )}
            <div className="subpage-body">
              {expanded.category === "report" ? (
                <InteractiveReport content={expandedFull || expanded.content} reportType={expanded.ref_id ?? "evening"} />
              ) : (
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{expandedFull || expanded.content}</ReactMarkdown>
              )}
            </div>
          </div>
        </div>, document.body
      )}
    </div>
  );
}

export default Dashboard;
