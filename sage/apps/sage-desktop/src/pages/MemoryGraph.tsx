import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import MessageFlow from "./MessageFlow";

interface GraphNode {
  id: number;
  category: string;
  content: string;
  confidence: number;
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

const CATEGORY_COLORS: Record<string, string> = {
  identity: "#3b82f6",
  values: "#8b5cf6",
  behavior: "#f59e0b",
  thinking: "#06b6d4",
  emotion: "#ef4444",
  growth: "#22c55e",
  pattern: "#f97316",
  personality: "#ec4899",
  task: "#6366f1",
  decision: "#14b8a6",
  observer_note: "#64748b",
  report_insight: "#a855f7",
};

function getColor(category: string): string {
  return CATEGORY_COLORS[category] ?? "#8B7355";
}

type TabKey = "graph" | "messages";

function MemoryGraph() {
  const [activeTab, setActiveTab] = useState<TabKey>("graph");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const nodesRef = useRef<GraphNode[]>([]);
  const edgesRef = useRef<GraphEdge[]>([]);
  const animRef = useRef<number>(0);
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
      nodesRef.current = data.nodes.map((n, i) => ({
        ...n,
        x: cx + Math.cos((i / data.nodes.length) * Math.PI * 2) * 200 + (Math.random() - 0.5) * 40,
        y: cy + Math.sin((i / data.nodes.length) * Math.PI * 2) * 200 + (Math.random() - 0.5) * 40,
        vx: 0,
        vy: 0,
      }));
      edgesRef.current = data.edges;
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
    for (let i = nodesRef.current.length - 1; i >= 0; i--) {
      const n = nodesRef.current[i];
      const r = (4 + n.confidence * 6) / z + 2;
      const dx = n.x - x;
      const dy = n.y - y;
      if (dx * dx + dy * dy < r * r) return i;
    }
    return -1;
  }, [screenToWorld]);

  // Simulation step
  const simulate = useCallback(() => {
    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    if (nodes.length === 0) return;

    const nodeMap = new Map<number, number>();
    nodes.forEach((n, i) => nodeMap.set(n.id, i));

    // Repulsion (Coulomb's law)
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = 800 / (dist * dist);
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        nodes[i].vx += fx;
        nodes[i].vy += fy;
        nodes[j].vx -= fx;
        nodes[j].vy -= fy;
      }
    }

    // Attraction (Hooke's law) along edges
    for (const e of edges) {
      const si = nodeMap.get(e.from);
      const ti = nodeMap.get(e.to);
      if (si === undefined || ti === undefined) continue;
      const dx = nodes[ti].x - nodes[si].x;
      const dy = nodes[ti].y - nodes[si].y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const force = (dist - 80) * 0.03 * e.weight;
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
      n.vx += (cx - n.x) * 0.001;
      n.vy += (cy - n.y) * 0.001;
    }

    // Apply velocity with damping
    for (const n of nodes) {
      n.vx *= 0.85;
      n.vy *= 0.85;
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

    const z = zoomRef.current;
    const p = panRef.current;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.scale(z, z);

    const nodes = nodesRef.current;
    const edges = edgesRef.current;
    const nodeMap = new Map<number, GraphNode>();
    nodes.forEach((n) => nodeMap.set(n.id, n));

    // Draw edges
    for (const e of edges) {
      const s = nodeMap.get(e.from);
      const t = nodeMap.get(e.to);
      if (!s || !t) continue;
      ctx.beginPath();
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);
      ctx.strokeStyle = `rgba(139, 115, 85, ${0.1 + e.weight * 0.3})`;
      ctx.lineWidth = 0.5 + e.weight * 1.5;
      ctx.stroke();

      // Relation label at midpoint
      if (z > 0.6) {
        const mx = (s.x + t.x) / 2;
        const my = (s.y + t.y) / 2;
        ctx.font = `${9 / z}px system-ui`;
        ctx.fillStyle = "var(--text-tertiary)";
        ctx.textAlign = "center";
        ctx.fillText(e.relation, mx, my - 3 / z);
      }
    }

    // Draw nodes
    for (const n of nodes) {
      const r = 4 + n.confidence * 6;
      ctx.beginPath();
      ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
      ctx.fillStyle = getColor(n.category);
      ctx.globalAlpha = 0.3 + n.confidence * 0.7;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.strokeStyle = getColor(n.category);
      ctx.lineWidth = 1.5;
      ctx.stroke();

      // Label
      if (z > 0.5) {
        const label = n.content.length > 12 ? n.content.slice(0, 12) + "..." : n.content;
        ctx.font = `${10 / z}px system-ui`;
        ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--text") || "#3D2C1E";
        ctx.textAlign = "center";
        ctx.fillText(label, n.x, n.y + r + 12 / z);
      }
    }

    // Highlight selected
    if (selectedNode) {
      const n = nodes.find((n) => n.id === selectedNode.id);
      if (n) {
        const r = 4 + n.confidence * 6 + 3;
        ctx.beginPath();
        ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
        ctx.strokeStyle = getColor(n.category);
        ctx.lineWidth = 2.5;
        ctx.setLineDash([4, 2]);
        ctx.stroke();
        ctx.setLineDash([]);
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

  const categories = [...new Set(nodesRef.current.map((n) => n.category))];

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
          Memory Graph
        </button>
        <button style={tabStyle("messages")} onClick={() => setActiveTab("messages")}>
          Messages
        </button>
      </div>

      {activeTab === "messages" ? (
        <div style={{ flex: 1, overflow: "hidden" }}>
          <MessageFlow />
        </div>
      ) : loading ? (
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center" }}>
          <span style={{ color: "var(--text-secondary)" }}>Loading graph...</span>
        </div>
      ) : (
        <>
          {/* Toolbar */}
          <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-md)", padding: "var(--spacing-sm) var(--spacing-md)", flexShrink: 0 }}>
            <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
              {stats.nodes} nodes, {stats.edges} edges
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
              {linking ? "Linking..." : "Build Connections"}
            </button>
            <div style={{ flex: 1 }} />
            {/* Legend */}
            <div style={{ display: "flex", gap: "var(--spacing-sm)", flexWrap: "wrap" }}>
              {categories.map((cat) => (
                <span key={cat} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 11, color: "var(--text-secondary)" }}>
                  <span style={{ width: 8, height: 8, borderRadius: "50%", background: getColor(cat), display: "inline-block" }} />
                  {cat}
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
                  <span style={{ width: 10, height: 10, borderRadius: "50%", background: getColor(selectedNode.category) }} />
                  <span style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 500 }}>{selectedNode.category}</span>
                  <span style={{ fontSize: 11, color: "var(--text-tertiary)", marginLeft: "auto" }}>
                    confidence: {(selectedNode.confidence * 100).toFixed(0)}%
                  </span>
                </div>
                <div style={{ fontSize: 13, color: "var(--text)", lineHeight: 1.5 }}>{selectedNode.content}</div>
              </div>
            )}
          </div>

          {/* Empty state */}
          {stats.nodes > 0 && stats.edges === 0 && (
            <div style={{ textAlign: "center", padding: "var(--spacing-md)", color: "var(--text-tertiary)", fontSize: 13 }}>
              Memories loaded but no connections yet. Run Memory Evolution to discover links.
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default MemoryGraph;
