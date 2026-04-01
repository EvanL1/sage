import type React from "react";
import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { formatTime } from "../utils/time";

interface CommMessage {
  id: number;
  sender: string;
  channel: string;
  content: string | null;
  source: string;
  message_type: string;
  timestamp: string;
  created_at: string;
  direction: string; // "received" | "sent"
}

interface ChannelInfo {
  channel: string;
  source: string;
  count: number;
}

// ─── Message Graph types ───
interface MsgGraphNode {
  id: number;
  label: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
  degree: number; // number of connections
}

interface MsgGraphEdge {
  from: number;
  to: number;
  shared_channels: number;
  weight: number;
}

interface MsgGraphData {
  nodes: { id: number; label: string }[];
  edges: MsgGraphEdge[];
}

const SOURCE_COLORS: Record<string, string> = {
  teams: "#6264A7",
  email: "#0078D4",
  slack: "#4A154B",
};

function sourceIcon(source: string): string {
  switch (source) {
    case "teams": return "T";
    case "email": return "@";
    case "slack": return "#";
    default: return "?";
  }
}

const dismissBtnStyle: React.CSSProperties = {
  position: "absolute", top: 4, right: 8, background: "none",
  border: "none", fontSize: 14, cursor: "pointer",
  color: "var(--text-tertiary)", padding: 0, lineHeight: 1,
};

const panelHeaderLabelStyle: React.CSSProperties = {
  fontSize: 10, fontWeight: 600, color: "var(--accent)",
  marginBottom: 4, textTransform: "uppercase", letterSpacing: "0.5px",
};

// ─── Message Graph sub-component ───
function MessageGraph() {
  const { t } = useLang();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const nodesRef = useRef<MsgGraphNode[]>([]);
  const edgesRef = useRef<MsgGraphEdge[]>([]);
  const animRef = useRef<number>(0);
  const panRef = useRef({ x: 0, y: 0 });
  const zoomRef = useRef(1);
  const dragRef = useRef<{
    type: "node" | "pan" | null;
    nodeIdx: number;
    startX: number;
    startY: number;
  }>({ type: null, nodeIdx: -1, startX: 0, startY: 0 });
  const [selectedNode, setSelectedNode] = useState<MsgGraphNode | null>(null);
  const [loading, setLoading] = useState(true);
  const [stats, setStats] = useState({ persons: 0, edges: 0 });

  const loadGraph = useCallback(async () => {
    try {
      const data = await invoke<MsgGraphData>("get_message_graph");
      // Compute degree for each node (number of edges connected)
      const degreeMap = new Map<number, number>();
      for (const e of data.edges) {
        degreeMap.set(e.from, (degreeMap.get(e.from) || 0) + 1);
        degreeMap.set(e.to, (degreeMap.get(e.to) || 0) + 1);
      }
      const cx = 400, cy = 300;
      nodesRef.current = data.nodes.map((n, i) => {
        const angle = (i / data.nodes.length) * Math.PI * 2;
        const radius = 180;
        return {
          ...n,
          degree: degreeMap.get(n.id) || 0,
          x: cx + Math.cos(angle) * radius + (Math.random() - 0.5) * 40,
          y: cy + Math.sin(angle) * radius + (Math.random() - 0.5) * 40,
          vx: 0,
          vy: 0,
        };
      });
      edgesRef.current = data.edges;
      setStats({ persons: data.nodes.length, edges: data.edges.length });
    } catch (e) {
      console.error("Failed to load message graph:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  const screenToWorld = useCallback((sx: number, sy: number) => {
    const z = zoomRef.current, p = panRef.current;
    return { x: (sx - p.x) / z, y: (sy - p.y) / z };
  }, []);

  const hitTest = useCallback((sx: number, sy: number): number => {
    const { x, y } = screenToWorld(sx, sy);
    for (let i = nodesRef.current.length - 1; i >= 0; i--) {
      const n = nodesRef.current[i];
      const r = 10 + Math.min(n.degree, 10) * 2;
      const dx = n.x - x, dy = n.y - y;
      if (dx * dx + dy * dy < r * r) return i;
    }
    return -1;
  }, [screenToWorld]);

  const simulate = useCallback(() => {
    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    if (nodes.length === 0) return;
    const nodeMap = new Map<number, number>();
    nodes.forEach((n, i) => nodeMap.set(n.id, i));

    // Repulsion
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = 1200 / (dist * dist);
        const fx = (dx / dist) * force, fy = (dy / dist) * force;
        nodes[i].vx += fx; nodes[i].vy += fy;
        nodes[j].vx -= fx; nodes[j].vy -= fy;
      }
    }
    // Attraction along edges
    const maxWeight = Math.max(1, ...edges.map((e) => e.weight));
    for (const e of edges) {
      const si = nodeMap.get(e.from), ti = nodeMap.get(e.to);
      if (si === undefined || ti === undefined) continue;
      const dx = nodes[ti].x - nodes[si].x;
      const dy = nodes[ti].y - nodes[si].y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const w = e.weight / maxWeight;
      const force = (dist - 100) * 0.02 * (0.3 + w * 0.7);
      const fx = (dx / dist) * force, fy = (dy / dist) * force;
      nodes[si].vx += fx; nodes[si].vy += fy;
      nodes[ti].vx -= fx; nodes[ti].vy -= fy;
    }
    // Center gravity
    const cx = 400, cy = 300;
    for (const n of nodes) {
      n.vx += (cx - n.x) * 0.001;
      n.vy += (cy - n.y) * 0.001;
      n.vx *= 0.85; n.vy *= 0.85;
      n.x += n.vx; n.y += n.vy;
    }
  }, []);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
      canvas.width = w * dpr; canvas.height = h * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const z = zoomRef.current, p = panRef.current;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.scale(z, z);

    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    const nodeMap = new Map<number, MsgGraphNode>();
    nodes.forEach((n) => nodeMap.set(n.id, n));
    const maxWeight = Math.max(1, ...edges.map((e) => e.weight));

    // Draw edges
    for (const e of edges) {
      const s = nodeMap.get(e.from), t = nodeMap.get(e.to);
      if (!s || !t) continue;
      const w = e.weight / maxWeight;
      ctx.beginPath();
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);
      ctx.strokeStyle = "#8B7355";
      ctx.globalAlpha = 0.12 + w * 0.5;
      ctx.lineWidth = 1 + w * 4;
      ctx.stroke();
      ctx.globalAlpha = 1;
      // Shared channels label
      if (z > 0.5 && e.shared_channels > 0) {
        const mx = (s.x + t.x) / 2, my = (s.y + t.y) / 2;
        ctx.font = `${9 / z}px system-ui`;
        ctx.fillStyle = "#888";
        ctx.textAlign = "center";
        ctx.fillText(`${e.shared_channels} ch`, mx, my - 4 / z);
      }
    }

    // Draw nodes — circle size based on degree (connections)
    const textColor = getComputedStyle(canvas).getPropertyValue("--text") || "#3D2C1E";
    const hueStep = nodes.length > 1 ? 360 / nodes.length : 0;
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      const r = 10 + Math.min(n.degree, 10) * 2;
      const hue = (i * hueStep) % 360;
      const color = `hsl(${hue}, 55%, 55%)`;

      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
      ctx.fillStyle = `hsla(${hue}, 55%, 55%, 0.15)`;
      ctx.fill();
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.stroke();

      // Initial inside
      ctx.font = `bold ${Math.max(12, r) / z}px system-ui`;
      ctx.fillStyle = color;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(n.label.charAt(0).toUpperCase(), n.x, n.y);
      ctx.textBaseline = "alphabetic";

      // Name below
      if (z > 0.4) {
        const label = n.label.length > 16 ? n.label.slice(0, 16) + ".." : n.label;
        ctx.font = `${10 / z}px system-ui`;
        ctx.fillStyle = textColor;
        ctx.fillText(label, n.x, n.y + r + 12 / z);
      }
    }

    // Highlight selected
    if (selectedNode) {
      const n = nodes.find((nd) => nd.id === selectedNode.id);
      if (n) {
        const r = 10 + Math.min(n.degree, 10) * 2 + 4;
        ctx.beginPath();
        ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
        ctx.strokeStyle = "#3b82f6";
        ctx.lineWidth = 2.5;
        ctx.setLineDash([4, 2]);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
    ctx.restore();
  }, [selectedNode]);

  useEffect(() => { loadGraph(); }, [loadGraph]);
  useEffect(() => {
    let running = true;
    const loop = () => { if (!running) return; simulate(); draw(); animRef.current = requestAnimationFrame(loop); };
    loop();
    return () => { running = false; cancelAnimationFrame(animRef.current); };
  }, [simulate, draw]);

  // Mouse handlers
  const handleMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const sx = e.clientX - rect.left, sy = e.clientY - rect.top;
    const idx = hitTest(sx, sy);
    if (idx >= 0) {
      dragRef.current = { type: "node", nodeIdx: idx, startX: sx, startY: sy };
      setSelectedNode({ ...nodesRef.current[idx] });
    } else {
      dragRef.current = { type: "pan", nodeIdx: -1, startX: e.clientX, startY: e.clientY };
      setSelectedNode(null);
    }
  }, [hitTest]);

  const handleMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    if (!d.type) return;
    if (d.type === "node") {
      const rect = canvasRef.current!.getBoundingClientRect();
      const { x, y } = screenToWorld(e.clientX - rect.left, e.clientY - rect.top);
      nodesRef.current[d.nodeIdx].x = x;
      nodesRef.current[d.nodeIdx].y = y;
      nodesRef.current[d.nodeIdx].vx = 0;
      nodesRef.current[d.nodeIdx].vy = 0;
    } else {
      panRef.current.x += e.clientX - d.startX;
      panRef.current.y += e.clientY - d.startY;
      d.startX = e.clientX; d.startY = e.clientY;
    }
  }, [screenToWorld]);

  const handleMouseUp = useCallback(() => {
    dragRef.current = { type: null, nodeIdx: -1, startX: 0, startY: 0 };
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left, my = e.clientY - rect.top;
      const factor = e.deltaY > 0 ? 0.9 : 1.1;
      const newZoom = Math.max(0.2, Math.min(5, zoomRef.current * factor));
      panRef.current.x = mx - (mx - panRef.current.x) * (newZoom / zoomRef.current);
      panRef.current.y = my - (my - panRef.current.y) * (newZoom / zoomRef.current);
      zoomRef.current = newZoom;
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [loading]);

  if (loading) {
    return (
      <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center" }}>
        <span style={{ color: "var(--text-secondary)" }}>{t("msg.loadingGraph")}</span>
      </div>
    );
  }

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {/* Stats bar */}
      <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-md)", padding: "var(--spacing-sm) var(--spacing-md)", flexShrink: 0 }}>
        <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
          {stats.persons} {t("msg.people")}, {stats.edges} {t("msg.relationships")}
        </span>
        <div style={{ flex: 1 }} />
        <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
          {t("msg.nodeSizeHint")}
        </span>
      </div>
      {/* Canvas */}
      <div style={{ flex: 1, position: "relative", overflow: "hidden", border: "1px solid var(--border)", borderRadius: "var(--radius)" }}>
        <canvas
          ref={canvasRef}
          style={{ width: "100%", height: "100%", cursor: dragRef.current.type ? "grabbing" : "grab" }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        />
        {selectedNode && (
          <div style={{
            position: "absolute", bottom: "var(--spacing-md)", left: "var(--spacing-md)",
            background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius)",
            padding: "var(--spacing-md)", maxWidth: 300, backdropFilter: "blur(8px)",
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text)" }}>{selectedNode.label}</div>
            <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>
              {selectedNode.degree} {selectedNode.degree !== 1 ? t("msg.connectionsPlural") : t("msg.connection")}
            </div>
          </div>
        )}
      </div>
      {stats.persons === 0 && !loading && (
        <div style={{ textAlign: "center", padding: "var(--spacing-md)", color: "var(--text-tertiary)", fontSize: 13 }}>
          {t("msg.noCommData")}
        </div>
      )}
    </div>
  );
}

// ─── Main MessageFlow component ───
type ViewMode = "list" | "graph";

function MessageFlow() {
  const { t } = useLang();
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [messages, setMessages] = useState<CommMessage[]>([]);
  const [selectedChannel, setSelectedChannel] = useState<string | null>(null);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [directionFilter, setDirectionFilter] = useState<"all" | "received" | "sent">("all");
  const [selectedMsg, setSelectedMsg] = useState<CommMessage | null>(null);
  const [aiInsight, setAiInsight] = useState<string | null>(null);
  const [aiLoading, setAiLoading] = useState(false);
  const [situation, setSituation] = useState<string | null>(null);
  const [situationLoading, setSituationLoading] = useState(false);

  const initDone = useRef(false);

  const loadChannels = useCallback(async () => {
    try {
      const data = await invoke<ChannelInfo[]>("get_message_channels");
      setChannels(data);
      if (data.length > 0 && !initDone.current) {
        initDone.current = true;
        setSelectedChannel(data[0].channel);
        setSelectedSource(data[0].source);
      }
    } catch (e) {
      console.error("Failed to load channels:", e);
    }
  }, []);

  const loadMessages = useCallback(async () => {
    if (!selectedChannel && !selectedSource) {
      setMessages([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setAiInsight(null);
    try {
      const params: Record<string, unknown> = { limit: 100 };
      if (selectedChannel) params.channel = selectedChannel;
      else if (selectedSource) params.source = selectedSource;
      const data = await invoke<CommMessage[]>("get_messages", params);
      setMessages(data);
    } catch (e) {
      console.error("Failed to load messages:", e);
    } finally {
      setLoading(false);
    }
  }, [selectedChannel, selectedSource]);

  useEffect(() => { loadChannels(); }, [loadChannels]);
  useEffect(() => { loadMessages(); }, [loadMessages]);

  const handleSummarize = useCallback(async () => {
    if (messages.length === 0 || !selectedChannel) return;
    setAiLoading(true);
    try {
      type SummarizeResult = { summary: string; actions: { priority: string; description: string }[]; tasks_created: number; message_count: number };
      const result = await invoke<SummarizeResult>("summarize_channel", { channel: selectedChannel, source: selectedSource || "teams", createTasks: true });
      let insight = result.summary;
      if (result.actions.length > 0) {
        insight += `\n\n--- ${t("msg.actionItems")} ---`;
        for (const a of result.actions) insight += `\n[${a.priority}] ${a.description}`;
        if (result.tasks_created > 0) insight += `\n\n\u2713 ${result.tasks_created} ${t("msg.taskCreated")}`;
      }
      setAiInsight(insight);
    } catch (e) {
      console.error("Summarize failed:", e);
      setAiInsight(t("msg.failedSummary"));
    } finally {
      setAiLoading(false);
    }
  }, [messages, selectedChannel, selectedSource, t]);

  const totalMessages = channels.reduce((sum, c) => sum + c.count, 0);
  const sources = [...new Set(channels.map((c) => c.source))];

  const visibleChannels = selectedSource
    ? channels.filter((ch) => ch.source === selectedSource)
    : channels;

  const filteredMessages = messages
    .filter((m) => directionFilter === "all" || m.direction === directionFilter)
    .filter((m) =>
      !searchQuery ||
      m.content?.toLowerCase().includes(searchQuery.toLowerCase()) ||
      m.sender.toLowerCase().includes(searchQuery.toLowerCase()),
    );

  const grouped = new Map<string, CommMessage[]>();
  for (const msg of filteredMessages) {
    const dateKey = msg.timestamp.slice(0, 10);
    if (!grouped.has(dateKey)) grouped.set(dateKey, []);
    grouped.get(dateKey)!.push(msg);
  }

  const yesterday = t("msg.yesterday");

  // Graph view
  if (viewMode === "graph") {
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        {/* View toggle */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-sm)", padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
          <button
            onClick={() => setViewMode("list")}
            style={{ fontSize: 11, padding: "3px 10px", borderRadius: "var(--radius)", border: "1px solid var(--border)", background: "transparent", color: "var(--text-secondary)", cursor: "pointer" }}
          >
            {t("msg.list")}
          </button>
          <button
            style={{ fontSize: 11, padding: "3px 10px", borderRadius: "var(--radius)", border: "1px solid var(--accent)", background: "var(--accent)", color: "#fff", cursor: "default", fontWeight: 600 }}
          >
            {t("msg.graph")}
          </button>
          <span style={{ fontSize: 11, color: "var(--text-tertiary)", marginLeft: "var(--spacing-sm)" }}>
            {t("msg.commRelationships")}
          </span>
        </div>
        <MessageGraph />
      </div>
    );
  }

  // List view
  return (
    <div style={{ display: "flex", height: "100%", gap: 0 }}>
      {/* Channel sidebar */}
      <div
        style={{
          width: 200,
          flexShrink: 0,
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div style={{ padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
            {totalMessages} {t("msg.msgsCount")}, {channels.length} {t("msg.chCount")}
          </span>
          <button
            onClick={() => setViewMode("graph")}
            title={t("msg.graphView")}
            style={{ fontSize: 11, padding: "2px 8px", borderRadius: "var(--radius)", border: "1px solid var(--border)", background: "transparent", color: "var(--text-secondary)", cursor: "pointer" }}
          >
            {t("msg.graph")}
          </button>
        </div>

        {/* Source filter */}
        <div style={{ display: "flex", gap: 4, padding: "var(--spacing-sm) var(--spacing-md)", flexWrap: "wrap" }}>
          <button
            onClick={() => {
              setSelectedChannel(null);
              setSelectedSource(null);
              if (channels.length > 0) {
                setSelectedChannel(channels[0].channel);
                setSelectedSource(channels[0].source);
              }
            }}
            style={{
              fontSize: 10, padding: "2px 8px", borderRadius: 12,
              border: "1px solid var(--border)",
              background: selectedChannel ? "transparent" : "var(--surface-active)",
              color: "var(--text-secondary)", cursor: "pointer", fontWeight: 600,
            }}
          >
            {t("msg.all")}
          </button>
          {sources.map((src) => (
            <button
              key={src}
              onClick={() => { setSelectedChannel(null); setSelectedSource(src); }}
              style={{
                fontSize: 10, padding: "2px 8px", borderRadius: 12,
                border: "1px solid var(--border)",
                background: selectedSource === src && !selectedChannel ? SOURCE_COLORS[src] || "var(--accent)" : "transparent",
                color: selectedSource === src && !selectedChannel ? "#fff" : "var(--text-secondary)",
                cursor: "pointer", textTransform: "uppercase", fontWeight: 600, letterSpacing: "0.5px",
              }}
            >
              {src}
            </button>
          ))}
        </div>

        {/* Channel list */}
        <div style={{ flex: 1, overflowY: "auto" }}>
          {visibleChannels.map((ch) => (
            <button
              key={`${ch.channel}-${ch.source}`}
              onClick={() => {
                if (ch.count === 1) {
                  setSelectedChannel(ch.channel);
                  setSelectedSource(ch.source);
                  invoke<CommMessage[]>("get_messages", { channel: ch.channel, limit: 1 })
                    .then((msgs) => { if (msgs.length > 0) setSelectedMsg(msgs[0]); })
                    .catch(() => {});
                } else {
                  setSelectedChannel(ch.channel);
                  setSelectedSource(ch.source);
                }
              }}
              style={{
                display: "flex", alignItems: "center", gap: "var(--spacing-sm)",
                width: "100%", padding: "6px var(--spacing-md)", border: "none",
                background: selectedChannel === ch.channel && selectedSource === ch.source ? "var(--surface-active)" : "transparent",
                cursor: "pointer", textAlign: "left", fontSize: 12, color: "var(--text)",
              }}
            >
              <span style={{
                width: 20, height: 20, borderRadius: 4,
                display: "flex", alignItems: "center", justifyContent: "center",
                fontSize: 10, fontWeight: 700,
                background: SOURCE_COLORS[ch.source] || "var(--text-tertiary)", color: "#fff", flexShrink: 0,
              }}>
                {sourceIcon(ch.source)}
              </span>
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {ch.channel}
              </span>
              {ch.channel.includes(",") && <span style={{ fontSize: 9, color: "#10b981", fontWeight: 700, flexShrink: 0 }} title={t("msg.group")}>G</span>}
              {!ch.channel.includes(",") && ch.channel.startsWith("#") && <span style={{ fontSize: 10, color: "#6366f1", fontWeight: 700, flexShrink: 0 }} title={t("msg.channel")}>#</span>}
              <span style={{ fontSize: 10, color: "var(--text-tertiary)", flexShrink: 0 }}>{ch.count}</span>
            </button>
          ))}
          {visibleChannels.length === 0 && (
            <div style={{ padding: "var(--spacing-lg)", textAlign: "center", color: "var(--text-tertiary)", fontSize: 12 }}>
              {channels.length === 0
                ? t("msg.noChannels")
                : t("msg.noChannelsForSource")}
            </div>
          )}
        </div>
      </div>

      {/* Message list */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", position: "relative" }}>
        {/* Search + AI bar */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-sm)", padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)" }}>
          <input
            type="text"
            placeholder={t("msg.searchMessages")}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            style={{
              flex: 1, padding: "6px 10px",
              border: "1px solid var(--border)", borderRadius: "var(--radius)",
              background: "var(--bg)", color: "var(--text)", fontSize: 12, outline: "none",
            }}
          />
          {/* Direction filter */}
          {(["all", "received", "sent"] as const).map((d) => (
            <button
              key={d}
              onClick={() => setDirectionFilter(d)}
              style={{
                fontSize: 10, padding: "3px 8px", borderRadius: 12,
                border: "1px solid var(--border)",
                background: directionFilter === d ? (d === "sent" ? "#22c55e" : d === "received" ? "#3b82f6" : "var(--surface-active)") : "transparent",
                color: directionFilter === d ? "#fff" : "var(--text-secondary)",
                cursor: "pointer", fontWeight: 600, textTransform: "uppercase",
              }}
            >
              {d === "all" ? t("msg.all") : d === "received" ? t("msg.in") : t("msg.out")}
            </button>
          ))}
          <button
            onClick={handleSummarize}
            disabled={aiLoading || messages.length === 0 || !selectedChannel}
            style={{
              padding: "5px 10px", fontSize: 11, borderRadius: "var(--radius)",
              border: "1px solid var(--border)", background: "var(--surface)",
              color: "var(--text-secondary)",
              cursor: aiLoading || messages.length === 0 || !selectedChannel ? "not-allowed" : "pointer",
              opacity: aiLoading ? 0.6 : 1, whiteSpace: "nowrap", fontWeight: 500,
            }}
          >
            {aiLoading ? t("msg.thinking") : t("msg.summarize")}
          </button>
        </div>

        {/* Situation card — 处境纵览 */}
        <div style={{
          padding: "var(--spacing-sm) var(--spacing-md)",
          borderBottom: "1px solid var(--border)",
          display: "flex", alignItems: "center", gap: "var(--spacing-sm)",
        }}>
          <button
            onClick={async () => {
              setSituationLoading(true);
              setSituation(null);
              try {
                const result = await invoke<string>("get_situation_summary");
                setSituation(result);
              } catch (e) {
                setSituation("Failed to generate: " + String(e));
              } finally {
                setSituationLoading(false);
              }
            }}
            disabled={situationLoading}
            style={{
              padding: "4px 12px", fontSize: 11, borderRadius: "var(--radius)",
              border: "1px solid var(--accent)", background: "var(--accent-light)",
              color: "var(--accent)", cursor: situationLoading ? "wait" : "pointer",
              fontWeight: 600, whiteSpace: "nowrap",
            }}
          >
            {situationLoading ? "Analyzing..." : "Situation"}
          </button>
          {situation && (
            <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
              Generated just now
            </span>
          )}
        </div>
        {situation && (
          <div style={{
            margin: "0 var(--spacing-md)", padding: "var(--spacing-sm) var(--spacing-md)",
            background: "var(--surface)", borderRadius: "var(--radius)",
            border: "1px solid var(--border)", fontSize: 12, lineHeight: 1.7,
            color: "var(--text)", position: "relative", whiteSpace: "pre-wrap",
            maxHeight: 300, overflowY: "auto",
          }}>
            <button onClick={() => setSituation(null)} style={dismissBtnStyle}>
              x
            </button>
            <div style={panelHeaderLabelStyle}>
              Situation
            </div>
            <div className="md-content"><ReactMarkdown remarkPlugins={[remarkGfm]}>{situation}</ReactMarkdown></div>
          </div>
        )}

        {/* AI Insight panel */}
        {aiInsight && (
          <div style={{
            margin: "var(--spacing-sm) var(--spacing-md)",
            padding: "var(--spacing-sm) var(--spacing-md)",
            background: "var(--accent-light)", borderRadius: "var(--radius)",
            border: "1px solid var(--accent)", fontSize: 12, lineHeight: 1.6,
            color: "var(--text)", position: "relative",
          }}>
            <button onClick={() => setAiInsight(null)} style={dismissBtnStyle}>
              x
            </button>
            <div style={panelHeaderLabelStyle}>
              {t("msg.convSummary")}
            </div>
            <div className="md-content"><ReactMarkdown remarkPlugins={[remarkGfm]}>{aiInsight}</ReactMarkdown></div>
          </div>
        )}

        {/* Messages */}
        <div style={{ flex: 1, overflowY: "auto", padding: "var(--spacing-sm) 0" }}>
          {loading ? (
            <div style={{ textAlign: "center", padding: "var(--spacing-xl)", color: "var(--text-tertiary)" }}>
              {t("loading")}
            </div>
          ) : filteredMessages.length === 0 ? (
            <div style={{ textAlign: "center", padding: "var(--spacing-xl)", color: "var(--text-tertiary)", fontSize: 13 }}>
              {searchQuery ? t("msg.noMatchingMsgs") : t("msg.noMsgsInChannel")}
            </div>
          ) : (
            [...grouped.entries()].map(([date, msgs]) => (
              <div key={date}>
                <div style={{
                  display: "flex", alignItems: "center", gap: "var(--spacing-sm)",
                  padding: "var(--spacing-sm) var(--spacing-md)",
                  position: "sticky", top: 0, background: "var(--bg)", zIndex: 1,
                }}>
                  <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
                  <span style={{ fontSize: 10, color: "var(--text-tertiary)", fontWeight: 500, whiteSpace: "nowrap" }}>{date}</span>
                  <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
                </div>
                {msgs.map((msg) => {
                  const isSent = msg.direction === "sent";
                  return (
                  <div
                    key={msg.id}
                    style={{
                      display: "flex", gap: "var(--spacing-sm)", padding: "4px var(--spacing-md)",
                      transition: "background 0.15s", cursor: "pointer",
                      background: selectedMsg?.id === msg.id ? "var(--surface-active)" : "transparent",
                      flexDirection: isSent ? "row-reverse" : "row",
                    }}
                    onClick={() => setSelectedMsg(selectedMsg?.id === msg.id ? null : msg)}
                    onMouseEnter={(e) => { if (selectedMsg?.id !== msg.id) e.currentTarget.style.background = "var(--surface-hover)"; }}
                    onMouseLeave={(e) => { if (selectedMsg?.id !== msg.id) e.currentTarget.style.background = "transparent"; }}
                  >
                    <div style={{
                      width: 28, height: 28, borderRadius: "50%",
                      background: isSent ? "var(--accent)" : (SOURCE_COLORS[msg.source] ? `${SOURCE_COLORS[msg.source]}22` : "var(--accent-light)"),
                      color: isSent ? "#fff" : (SOURCE_COLORS[msg.source] || "var(--accent)"),
                      display: "flex", alignItems: "center", justifyContent: "center",
                      fontSize: 12, fontWeight: 600, flexShrink: 0, marginTop: 2,
                    }}>
                      {msg.sender ? msg.sender.charAt(0).toUpperCase() : sourceIcon(msg.source)}
                    </div>
                    <div style={{ flex: 1, minWidth: 0, maxWidth: "75%", textAlign: isSent ? "right" : "left" }}>
                      <div style={{ display: "flex", alignItems: "baseline", gap: "var(--spacing-sm)", justifyContent: isSent ? "flex-end" : "flex-start" }}>
                        <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text)" }}>{isSent ? t("msg.me") : (msg.sender || msg.source)}</span>
                        {!isSent && <span
                          onClick={(e) => { e.stopPropagation(); setSelectedChannel(null); setSelectedSource(msg.source); }}
                          style={{
                            fontSize: 9, padding: "1px 5px", borderRadius: 4,
                            background: SOURCE_COLORS[msg.source] || "var(--text-tertiary)",
                            color: "#fff", fontWeight: 600, textTransform: "uppercase",
                            letterSpacing: "0.3px", cursor: "pointer",
                          }}
                        >
                          {msg.source}
                        </span>}
                        {msg.message_type && msg.message_type !== "text" && msg.message_type !== "unknown" && (
                          <span style={{ fontSize: 9, padding: "1px 5px", borderRadius: 4, background: msg.message_type === "group" ? "#10b981" : msg.message_type === "channel" ? "#6366f1" : "#8b5cf6", color: "#fff", fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.3px" }}>
                            {msg.message_type === "group" ? t("msg.group") : msg.message_type === "channel" ? t("msg.channel") : msg.message_type}
                          </span>
                        )}
                        <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>{formatTime(msg.timestamp, yesterday)}</span>
                      </div>
                      <div style={{
                        fontSize: 13, lineHeight: 1.5, wordBreak: "break-word",
                        display: "inline-block", textAlign: "left",
                        padding: "6px 10px", borderRadius: 12, marginTop: 2,
                        background: isSent ? "var(--accent)" : "var(--surface, var(--bg-secondary))",
                        color: isSent ? "#fff" : "var(--text)",
                        borderBottomRightRadius: isSent ? 4 : 12,
                        borderBottomLeftRadius: isSent ? 12 : 4,
                      }}>
                        {msg.content || <span style={{ opacity: 0.5, fontStyle: "italic" }}>{t("msg.noContent")}</span>}
                      </div>
                      {selectedMsg?.id === msg.id && (
                        <button
                          onClick={async (e) => {
                            e.stopPropagation();
                            await invoke("delete_message", { messageId: msg.id, subject: msg.channel }).catch(() => {});
                            setMessages(prev => prev.filter(m => m.id !== msg.id));
                            setSelectedMsg(null);
                          }}
                          style={{
                            fontSize: 10, padding: "2px 6px", borderRadius: 4,
                            border: "1px solid var(--error)", background: "transparent",
                            color: "var(--error-text)", cursor: "pointer", alignSelf: "flex-start",
                            marginTop: 2,
                          }}
                        >
                          delete
                        </button>
                      )}
                    </div>
                  </div>
                  );
                })}
              </div>
            ))
          )}
        </div>

      </div>
    </div>
  );
}

export default MessageFlow;
