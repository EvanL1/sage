import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import MessageFlow from "./MessageFlow";
import { useLang } from "../LangContext";

interface GraphNode {
  id: number;
  category: string;
  content: string;
  confidence: number;
  depth?: string;
  // computed
  degree: number; // edge count — computed on load
  // simulation state
  x: number;
  y: number;
  vx: number;
  vy: number;
}

interface GraphEdge {
  id: number;
  from: number;
  to: number;
  relation: string;
  weight: number;
}

interface GraphData {
  nodes: Omit<GraphNode, "x" | "y" | "vx" | "vy">[];
  edges: GraphEdge[];
}

const DEPTH_COLORS: Record<string, string> = {
  axiom:      "#d97706",
  procedural: "#7c3aed",
  semantic:   "#2563eb",
  episodic:   "#9ca3af",
};

const DEPTH_LABELS_GRAPH: Record<string, string> = {
  axiom:      "Beliefs",
  procedural: "Judgments",
  semantic:   "Patterns",
  episodic:   "Events",
};

function getColor(node: Pick<GraphNode, "depth" | "category">): string {
  if (node.depth && DEPTH_COLORS[node.depth]) return DEPTH_COLORS[node.depth];
  return "#8B7355";
}

/** Node radius: blend confidence (30%) + degree centrality (70%), range 5–25px */
function nodeRadius(n: Pick<GraphNode, "confidence" | "degree">, maxDegree: number): number {
  const degNorm = maxDegree > 0 ? n.degree / maxDegree : 0;
  const blend = n.confidence * 0.3 + degNorm * 0.7;
  return 5 + blend * 20; // 5px min, 25px max
}

function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}

type TabKey = "graph" | "messages";

function MemoryGraph() {
  const { t } = useLang();
  const [activeTab, setActiveTab] = useState<TabKey>("graph");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const nodesRef = useRef<GraphNode[]>([]);
  const edgesRef = useRef<GraphEdge[]>([]);
  const animRef = useRef<number>(0);
  const tempRef = useRef(1.0); // cooling temperature: 1.0 → 0
  const starsRef = useRef<{ x: number; y: number; size: number; opacity: number }[]>([]);
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [loading, setLoading] = useState(true);
  const [linking, setLinking] = useState(false);
  const [stats, setStats] = useState({ nodes: 0, edges: 0 });

  // Pan/zoom state
  const panRef = useRef({ x: 0, y: 0 });
  const zoomRef = useRef(1);
  const dragRef = useRef<{
    type: "node" | "pan" | null;
    nodeIdx: number;
    startX: number;
    startY: number;
  }>({ type: null, nodeIdx: -1, startX: 0, startY: 0 });

  const loadGraph = useCallback(async () => {
    try {
      const data = await invoke<GraphData>("get_memory_graph");
      const cx = 400;
      const cy = 300;
      // Compute degree (edge count) per node
      const degreeMap = new Map<number, number>();
      for (const e of data.edges) {
        degreeMap.set(e.from, (degreeMap.get(e.from) || 0) + 1);
        degreeMap.set(e.to, (degreeMap.get(e.to) || 0) + 1);
      }
      nodesRef.current = data.nodes.map((n, i) => ({
        ...n,
        degree: degreeMap.get(n.id) || 0,
        x: cx + Math.cos((i / data.nodes.length) * Math.PI * 2) * (150 + Math.random() * 150) + (Math.random() - 0.5) * 60,
        y: cy + Math.sin((i / data.nodes.length) * Math.PI * 2) * (150 + Math.random() * 150) + (Math.random() - 0.5) * 60,
        vx: 0,
        vy: 0,
      }));
      edgesRef.current = data.edges;
      tempRef.current = 1.0;
      setStats({ nodes: data.nodes.length, edges: data.edges.length });
    } catch (e) {
      console.error("Failed to load graph:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  // Screen ↔ world coordinate transforms
  const screenToWorld = useCallback((sx: number, sy: number) => {
    const z = zoomRef.current;
    const p = panRef.current;
    return { x: (sx - p.x) / z, y: (sy - p.y) / z };
  }, []);

  // Find node at screen coordinates
  const hitTest = useCallback((sx: number, sy: number): number => {
    const { x, y } = screenToWorld(sx, sy);
    const z = zoomRef.current;
    const maxDeg = nodesRef.current.reduce((m, n) => Math.max(m, n.degree), 0);
    for (let i = nodesRef.current.length - 1; i >= 0; i--) {
      const n = nodesRef.current[i];
      const r = nodeRadius(n, maxDeg) / z + 2;
      const dx = n.x - x;
      const dy = n.y - y;
      if (dx * dx + dy * dy < r * r) return i;
    }
    return -1;
  }, [screenToWorld]);

  // Simulation step with simulated annealing (temperature cools → layout stabilises)
  const simulate = useCallback(() => {
    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    if (nodes.length === 0) return;

    const t = tempRef.current;
    if (t < 0.01) return; // frozen — skip physics entirely
    tempRef.current *= 0.995; // cool down each frame

    const nodeMap = new Map<number, number>();
    nodes.forEach((n, i) => nodeMap.set(n.id, i));

    // Repulsion (Coulomb's law) — stronger repulsion for spacious layout
    const repK = Math.min(3000, 1200 + nodes.length * 8);
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = repK / (dist * dist) * t;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        nodes[i].vx += fx;
        nodes[i].vy += fy;
        nodes[j].vx -= fx;
        nodes[j].vy -= fy;
      }
    }

    // Attraction (Hooke's law) along edges — longer rest length for breathing room
    const restLen = 80 + Math.sqrt(nodes.length) * 12;
    for (const e of edges) {
      const si = nodeMap.get(e.from);
      const ti = nodeMap.get(e.to);
      if (si === undefined || ti === undefined) continue;
      const dx = nodes[ti].x - nodes[si].x;
      const dy = nodes[ti].y - nodes[si].y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const force = (dist - restLen) * 0.02 * e.weight * t;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      nodes[si].vx += fx;
      nodes[si].vy += fy;
      nodes[ti].vx -= fx;
      nodes[ti].vy -= fy;
    }

    // Center gravity
    const cx = 400;
    const cy = 300;
    for (const n of nodes) {
      n.vx += (cx - n.x) * 0.002 * t;
      n.vy += (cy - n.y) * 0.002 * t;
    }

    // Apply velocity with strong damping
    const damping = 0.7 + 0.2 * (1 - t); // 0.7 hot → 0.9 cold
    for (const n of nodes) {
      n.vx *= damping;
      n.vy *= damping;
      n.x += n.vx;
      n.y += n.vy;
    }
  }, []);

  // Draw
  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
      canvas.width = w * dpr;
      canvas.height = h * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    // Theme detection
    const isDark = document.documentElement.getAttribute("data-theme") === "dark";

    // Background
    ctx.fillStyle = isDark ? "#030014" : "#f0f1f8";
    ctx.fillRect(0, 0, w, h);

    // Stars (dark only)
    if (isDark) {
      if (starsRef.current.length === 0) {
        for (let i = 0; i < 60; i++) {
          starsRef.current.push({
            x: Math.random() * 2000,
            y: Math.random() * 1500,
            size: Math.random() * 1.5 + 0.5,
            opacity: Math.random() * 0.6 + 0.2,
          });
        }
      }
      for (const star of starsRef.current) {
        ctx.beginPath();
        ctx.arc(star.x, star.y, star.size, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 255, 255, ${star.opacity})`;
        ctx.fill();
      }
    }

    const z = zoomRef.current;
    const p = panRef.current;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.scale(z, z);

    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    const nodeMap = new Map<number, GraphNode>();
    nodes.forEach((n) => nodeMap.set(n.id, n));

    const maxDeg = nodes.reduce((m, n) => Math.max(m, n.degree), 0);

    // Draw edges — thin, translucent lines that don't compete with nodes
    for (const e of edges) {
      const s = nodeMap.get(e.from);
      const t = nodeMap.get(e.to);
      if (!s || !t) continue;
      ctx.beginPath();
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);
      const alpha = isDark ? 0.06 + e.weight * 0.12 : 0.08 + e.weight * 0.15;
      ctx.strokeStyle = isDark
        ? hexToRgba("#8888aa", alpha)
        : hexToRgba("#6666aa", alpha);
      ctx.lineWidth = 0.3 + e.weight * 0.7;
      ctx.stroke();

      // Relation label at midpoint
      if (z > 0.8) {
        const mx = (s.x + t.x) / 2;
        const my = (s.y + t.y) / 2;
        ctx.font = `${9 / z}px "JetBrains Mono", monospace`;
        ctx.fillStyle = isDark ? "#475569" : "#9ca3af";
        ctx.textAlign = "center";
        ctx.fillText(e.relation, mx, my - 3 / z);
      }
    }

    // Draw nodes — size driven by degree centrality + confidence
    for (const n of nodes) {
      const r = nodeRadius(n, maxDeg);
      const color = getColor(n);

      // Glow halo — scales with node importance
      const glowR = r * 2.5;
      const glowAlpha = isDark ? 0.15 + (r / 25) * 0.2 : 0.06 + (r / 25) * 0.08;
      const glow = ctx.createRadialGradient(n.x, n.y, r * 0.3, n.x, n.y, glowR);
      glow.addColorStop(0, hexToRgba(color, glowAlpha));
      glow.addColorStop(1, "transparent");
      ctx.beginPath();
      ctx.arc(n.x, n.y, glowR, 0, Math.PI * 2);
      ctx.fillStyle = glow;
      ctx.fill();

      // Node body — solid fill with subtle opacity variation
      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.globalAlpha = 0.5 + n.confidence * 0.5;
      ctx.fill();
      ctx.globalAlpha = 1;

      // Label — only for larger nodes or when zoomed in
      const showLabel = r > 8 || z > 0.7;
      if (showLabel) {
        const label = n.content.length > 12 ? n.content.slice(0, 12) + "..." : n.content;
        ctx.font = `${10 / z}px "JetBrains Mono", monospace`;
        ctx.fillStyle = isDark ? "#e2e8f0" : "#1e1b4b";
        ctx.globalAlpha = isDark ? 0.85 : 0.75;
        ctx.textAlign = "center";
        ctx.fillText(label, n.x, n.y + r + 12 / z);
        ctx.globalAlpha = 1;
      }
    }

    // Highlight selected + neighbor glow
    if (selectedNode) {
      const selId = selectedNode.id;

      // 1-hop neighbors glow
      const neighborIds = new Set<number>();
      for (const e of edges) {
        if (e.from === selId) neighborIds.add(e.to);
        if (e.to === selId) neighborIds.add(e.from);
      }
      for (const nid of neighborIds) {
        const nb = nodeMap.get(nid);
        if (!nb) continue;
        const r = nodeRadius(nb, maxDeg) + 4;
        ctx.beginPath();
        ctx.arc(nb.x, nb.y, r, 0, Math.PI * 2);
        ctx.strokeStyle = hexToRgba(getColor(nb), 0.4);
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      // Selected node pulsing ring
      const n = nodes.find((n) => n.id === selId);
      if (n) {
        const r = nodeRadius(n, maxDeg) + 3;
        const pulseR = r + 2 + Math.sin(Date.now() / 300) * 2;
        ctx.beginPath();
        ctx.arc(n.x, n.y, pulseR, 0, Math.PI * 2);
        ctx.strokeStyle = getColor(n);
        ctx.lineWidth = 2;
        ctx.stroke();
      }
    }

    ctx.restore();
  }, [selectedNode]);

  // Animation loop
  useEffect(() => {
    loadGraph();
  }, [loadGraph]);

  useEffect(() => {
    let running = true;
    const loop = () => {
      if (!running) return;
      simulate();
      draw();
      animRef.current = requestAnimationFrame(loop);
    };
    loop();
    return () => {
      running = false;
      cancelAnimationFrame(animRef.current);
    };
  }, [simulate, draw]);

  // Mouse handlers
  const handleMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
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
      const sx = e.clientX - rect.left;
      const sy = e.clientY - rect.top;
      const { x, y } = screenToWorld(sx, sy);
      nodesRef.current[d.nodeIdx].x = x;
      nodesRef.current[d.nodeIdx].y = y;
      nodesRef.current[d.nodeIdx].vx = 0;
      nodesRef.current[d.nodeIdx].vy = 0;
    } else if (d.type === "pan") {
      panRef.current.x += e.clientX - d.startX;
      panRef.current.y += e.clientY - d.startY;
      d.startX = e.clientX;
      d.startY = e.clientY;
    }
  }, [screenToWorld]);

  const handleMouseUp = useCallback(() => {
    if (dragRef.current.type === "node") {
      tempRef.current = Math.max(tempRef.current, 0.3); // reheat after drag
    }
    dragRef.current = { type: null, nodeIdx: -1, startX: 0, startY: 0 };
  }, []);

  const handleBuildLinks = useCallback(async () => {
    setLinking(true);
    try {
      const r = await invoke<{ linked: number }>("trigger_memory_linking");
      if (r.linked > 0) {
        await loadGraph();
      }
    } catch (e) {
      console.error("Link failed:", e);
    } finally {
      setLinking(false);
    }
  }, [loadGraph]);

  // Wheel zoom via native listener (passive: false allows preventDefault)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const factor = e.deltaY > 0 ? 0.9 : 1.1;
      const newZoom = Math.max(0.2, Math.min(5, zoomRef.current * factor));
      panRef.current.x = mx - (mx - panRef.current.x) * (newZoom / zoomRef.current);
      panRef.current.y = my - (my - panRef.current.y) * (newZoom / zoomRef.current);
      zoomRef.current = newZoom;
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [loading]);

  const presentDepths = Object.keys(DEPTH_COLORS).filter((d) =>
    nodesRef.current.some((n) => n.depth === d)
  );

  const tabStyle = (tab: TabKey) => ({
    padding: "6px 16px",
    fontSize: 12,
    fontWeight: activeTab === tab ? 600 : 400,
    border: "none",
    borderBottom: activeTab === tab ? "2px solid var(--accent)" : "2px solid transparent",
    background: "transparent",
    color: activeTab === tab ? "var(--text)" : "var(--text-secondary)",
    cursor: "pointer" as const,
    transition: "all 0.15s",
  });

  return (
    <div className="page-container" style={{ display: "flex", flexDirection: "column", height: "100%", gap: 0 }}>
      {/* Tab bar */}
      <div style={{ display: "flex", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        <button style={tabStyle("graph")} onClick={() => setActiveTab("graph")}>
          {t("graph.memoryGraph")}
        </button>
        <button style={tabStyle("messages")} onClick={() => setActiveTab("messages")}>
          {t("graph.messages")}
        </button>
      </div>

      {activeTab === "messages" ? (
        <div style={{ flex: 1, overflow: "hidden" }}>
          <MessageFlow />
        </div>
      ) : loading ? (
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center" }}>
          <span style={{ color: "var(--text-secondary)" }}>{t("graph.loadingGraph")}</span>
        </div>
      ) : (
        <>
          {/* Toolbar */}
          <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-md)", padding: "var(--spacing-sm) var(--spacing-md)", flexShrink: 0 }}>
            <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
              {stats.nodes} {t("graph.nodes")}, {stats.edges} {t("graph.edges")}
            </span>
            <button
              onClick={handleBuildLinks}
              disabled={linking}
              style={{
                padding: "4px 12px", fontSize: 12, borderRadius: "var(--radius)",
                border: "1px solid var(--border)", background: "var(--surface)",
                color: "var(--text)", cursor: linking ? "wait" : "pointer",
                opacity: linking ? 0.6 : 1,
              }}
            >
              {linking ? t("graph.linking") : t("graph.buildConnections")}
            </button>
            <div style={{ flex: 1 }} />
            {/* Depth legend */}
            <div style={{ display: "flex", gap: "var(--spacing-sm)", flexWrap: "wrap" }}>
              {presentDepths.map((d) => (
                <span key={d} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 11, color: "var(--text-secondary)" }}>
                  <span style={{ width: 8, height: 8, borderRadius: "50%", background: DEPTH_COLORS[d], display: "inline-block" }} />
                  {DEPTH_LABELS_GRAPH[d] ?? d}
                </span>
              ))}
            </div>
          </div>

          {/* Canvas */}
          <div style={{ flex: 1, position: "relative", borderRadius: "var(--radius)", overflow: "hidden", border: "1px solid var(--border)" }}>
            <canvas
              ref={canvasRef}
              style={{ width: "100%", height: "100%", cursor: dragRef.current.type ? "grabbing" : "grab" }}
              onMouseDown={handleMouseDown}
              onMouseMove={handleMouseMove}
              onMouseUp={handleMouseUp}
              onMouseLeave={handleMouseUp}
            />

            {/* Selected node info panel */}
            {selectedNode && (
              <div style={{
                position: "absolute", bottom: "var(--spacing-md)", left: "var(--spacing-md)", right: "var(--spacing-md)",
                background: "var(--surface)", border: "1px solid var(--border)", borderRadius: "var(--radius)",
                padding: "var(--spacing-md)", maxWidth: 360, backdropFilter: "blur(8px)",
              }}>
                <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-sm)", marginBottom: "var(--spacing-xs)" }}>
                  <span style={{ width: 10, height: 10, borderRadius: "50%", background: getColor(selectedNode) }} />
                  <span style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 500 }}>{selectedNode.category}</span>
                  {selectedNode.depth && (
                    <span style={{ fontSize: 10, fontWeight: 500, padding: "1px 5px", borderRadius: 6, background: DEPTH_COLORS[selectedNode.depth] + "33", color: DEPTH_COLORS[selectedNode.depth] }}>
                      {DEPTH_LABELS_GRAPH[selectedNode.depth] ?? selectedNode.depth}
                    </span>
                  )}
                  <span style={{ fontSize: 11, color: "var(--text-tertiary)", marginLeft: "auto" }}>
                    {t("graph.confidence")}: {(selectedNode.confidence * 100).toFixed(0)}%
                  </span>
                </div>
                <div style={{ fontSize: 13, color: "var(--text)", lineHeight: 1.5 }}>{selectedNode.content}</div>
              </div>
            )}
          </div>

          {/* Empty state */}
          {stats.nodes > 0 && stats.edges === 0 && (
            <div style={{ textAlign: "center", padding: "var(--spacing-md)", color: "var(--text-tertiary)", fontSize: 13 }}>
              {t("graph.noEdges")}
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default MemoryGraph;
