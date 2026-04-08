import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { DashData, TYPE_LABEL, preview } from "./types";
import { pinItem } from "./pinStore";

interface FloatCard {
  id: number;
  content: string;
  category: string;
  x: number;
  y: number;
  phase: "in" | "hold" | "out";
  raw: Parameters<DashData["openExpanded"]>[0];
}

function hexRgba(hex: string, a: number) {
  const r = parseInt(hex.slice(1, 3), 16), g = parseInt(hex.slice(3, 5), 16), b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${a})`;
}

/** Read accent color from CSS variables at runtime */
function getThemeColors(): string[] {
  const style = getComputedStyle(document.documentElement);
  const accent = style.getPropertyValue("--accent").trim() || "#d97706";
  // Generate palette around accent
  return [accent, accent, accent, accent];
}

function getGlowColor(): string {
  return getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#d97706";
}

export default function NebulaLayout({ data }: { data: DashData }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const particlesRef = useRef<{ x: number; y: number; vx: number; vy: number; angle: number; orbit: number; size: number; color: string; alpha: number }[]>([]);
  const [cards, setCards] = useState<FloatCard[]>([]);
  const nextId = useRef(0);
  const idx = useRef(0);

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

  // Init particles with theme colors
  useEffect(() => {
    const colors = getThemeColors();
    const ps = [];
    for (let i = 0; i < 120; i++) {
      ps.push({
        x: 0, y: 0, vx: (Math.random() - 0.5) * 0.3, vy: (Math.random() - 0.5) * 0.3,
        angle: Math.random() * Math.PI * 2, orbit: (Math.random() + Math.random()) * 0.5,
        size: 1 + Math.random() * 2, color: colors[Math.floor(Math.random() * colors.length)], alpha: 0.3 + Math.random() * 0.7,
      });
    }
    particlesRef.current = ps;
  }, []);

  // Animation
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const glowColor = getGlowColor();
    const draw = () => {
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      const dpr = devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();
      if (canvas.width !== rect.width * dpr) { canvas.width = rect.width * dpr; canvas.height = rect.height * dpr; ctx.scale(dpr, dpr); }
      const w = rect.width, h = rect.height, cx = w / 2, cy = h / 2, r = Math.min(w, h) * 0.22;
      ctx.clearRect(0, 0, w, h);
      for (const p of particlesRef.current) {
        p.angle += 0.003; p.vx += (Math.random() - 0.5) * 0.04; p.vy += (Math.random() - 0.5) * 0.04; p.vx *= 0.95; p.vy *= 0.95;
        const tx = cx + Math.cos(p.angle) * p.orbit * r, ty = cy + Math.sin(p.angle) * p.orbit * r;
        p.x += (tx - p.x) * 0.02 + p.vx; p.y += (ty - p.y) * 0.02 + p.vy;
        ctx.beginPath(); ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
        ctx.fillStyle = hexRgba(p.color, p.alpha * 0.5); ctx.fill();
      }
      const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, r * 0.8);
      glow.addColorStop(0, hexRgba(glowColor, 0.08)); glow.addColorStop(1, "transparent");
      ctx.fillStyle = glow; ctx.fillRect(0, 0, w, h);
      animRef.current = requestAnimationFrame(draw);
    };
    animRef.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(animRef.current);
  }, []);

  // Spawn floating cards
  useEffect(() => {
    if (allItems.length === 0) return;
    const spawn = () => {
      setCards((prev) => {
        const visibleIds = new Set(prev.map(c => c.raw.id ?? c.raw.ref_id));
        let item = null;
        for (let attempt = 0; attempt < allItems.length; attempt++) {
          const candidate = allItems[idx.current % allItems.length];
          idx.current++;
          const cid = candidate.id ?? candidate.ref_id;
          if (cid == null || !visibleIds.has(cid)) { item = candidate; break; }
        }
        if (!item) return prev;
        const x = 5 + Math.random() * 60, y = 10 + Math.random() * 70;
        const card: FloatCard = { id: nextId.current++, content: preview(item.content, 60), category: item.category, x, y, phase: "in", raw: item };
        const next = [...prev, card];
        return next.length > 6 ? next.slice(-6) : next;
      });
    };
    spawn();
    const timer = setInterval(spawn, 3000);
    return () => clearInterval(timer);
  }, [allItems.length]);

  // Lifecycle
  useEffect(() => {
    const timers: number[] = [];
    for (const c of cards) {
      if (c.phase === "in") timers.push(window.setTimeout(() => setCards((p) => p.map((x) => x.id === c.id ? { ...x, phase: "hold" } : x)), 600));
      else if (c.phase === "hold") timers.push(window.setTimeout(() => setCards((p) => p.map((x) => x.id === c.id ? { ...x, phase: "out" } : x)), 6000));
      else if (c.phase === "out") timers.push(window.setTimeout(() => setCards((p) => p.filter((x) => x.id !== c.id)), 500));
    }
    return () => timers.forEach(clearTimeout);
  }, [cards]);

  const [pinFlash, setPinFlash] = useState<number | null>(null);

  const handlePin = useCallback((c: FloatCard, e: React.MouseEvent) => {
    e.stopPropagation();
    pinItem(c.raw);
    setPinFlash(c.id);
    setTimeout(() => setPinFlash(null), 800);
  }, []);

  return (
    <div className="nebula-page">
      <canvas ref={canvasRef} className="nebula-bg" />
      {cards.map((c) => (
        <div key={c.id} className={`nebula-card nebula-phase-${c.phase}${pinFlash === c.id ? " nebula-pinned" : ""}`}
          style={{ left: `${c.x}%`, top: `${c.y}%` }} onClick={() => data.openExpanded(c.raw)}>
          <div className="nebula-card-header">
            <span className="nebula-card-label">{TYPE_LABEL[c.category] ?? "INFO"}</span>
            <button className="nebula-pin-btn" onClick={(e) => handlePin(c, e)} title="Pin to Command">📌</button>
          </div>
          <div className="nebula-card-text md-content"><ReactMarkdown remarkPlugins={[remarkGfm]}>{c.content}</ReactMarkdown></div>
        </div>
      ))}
    </div>
  );
}
