import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import { useLang } from "../LangContext";
import type { Memory } from "../types";

interface TagInfo { tag: string; count: number; }

// ─── Depth config ───────────────────────────────────────────────────────────

const DEPTH_VISUAL = {
  episodic:   { icon: "·", color: "var(--text-tertiary, #9ca3af)", bg: "transparent", label: "about.depth.episodic" },
  semantic:   { icon: "○", color: "#3b82f6", bg: "rgba(59,130,246,0.06)",  label: "about.depth.semantic" },
  procedural: { icon: "◇", color: "#8b5cf6", bg: "rgba(139,92,246,0.06)", label: "about.depth.procedural" },
  axiom:      { icon: "◆", color: "#f59e0b", bg: "rgba(245,158,11,0.08)", label: "about.depth.axiom" },
} as const;

type DepthKey = keyof typeof DEPTH_VISUAL;
const DEPTH_ORDER: DepthKey[] = ["episodic", "semantic", "procedural", "axiom"];

// ─── Category config ────────────────────────────────────────────────────────

const CATEGORY_ORDER = ["identity", "values", "behavior", "thinking", "emotion", "growth"] as const;
type CatKey = typeof CATEGORY_ORDER[number];
const CAT_ICONS: Record<CatKey, string> = {
  identity: "🪞", values: "💎", behavior: "⚡", thinking: "🧠", emotion: "💗", growth: "🌱",
};
const CAT_COLORS: Record<CatKey, string> = {
  identity: "#6366f1", values: "#f59e0b", behavior: "#22c55e",
  thinking: "#3b82f6", emotion: "#ec4899", growth: "#10b981",
};

// ─── Confidence bar ─────────────────────────────────────────────────────────

function ConfidenceBar({ value, t }: { value: number; t: (key: any) => string }) {
  const pct = Math.round(value * 100);
  const color = pct >= 80 ? "#22c55e" : pct >= 50 ? "#f59e0b" : "var(--text-tertiary)";
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
      <div style={{ width: 48, height: 4, background: "var(--border-subtle)", borderRadius: 2, overflow: "hidden" }}>
        <div style={{ width: `${pct}%`, height: "100%", background: color, borderRadius: 2 }} />
      </div>
      <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>{t("about.confidence")} {pct}%</span>
    </div>
  );
}

// ─── Memory Card ────────────────────────────────────────────────────────────

const EXPAND_THRESHOLD = 100;

function MemoryCard({ memory, onDelete, onTagsChange, t }: {
  memory: Memory;
  onDelete: (id: number) => void;
  onTagsChange: () => void;
  t: (key: any) => string;
}) {
  const [deleting, setDeleting] = useState(false);
  const [tags, setTags] = useState<string[]>([]);
  const [addingTag, setAddingTag] = useState(false);
  const [newTag, setNewTag] = useState("");
  const [expanded, setExpanded] = useState(false);
  const [showSources, setShowSources] = useState(false);
  const [sources, setSources] = useState<Memory[]>([]);
  const isLong = memory.content.length > EXPAND_THRESHOLD;
  const cat = memory.category as CatKey;
  const catColor = CAT_COLORS[cat] || "var(--text-tertiary)";

  // 解析 derived_from
  const derivedIds = useMemo(() => {
    if (!memory.derived_from) return [];
    try { return JSON.parse(memory.derived_from) as number[]; } catch { return []; }
  }, [memory.derived_from]);

  useEffect(() => {
    invoke<string[]>("get_memory_tags", { memoryId: memory.id }).then(setTags).catch(() => {});
  }, [memory.id]);

  const handleDelete = async () => {
    setDeleting(true);
    try {
      await invoke("delete_memory", { memoryId: memory.id });
      onDelete(memory.id);
      onTagsChange();
    } catch { setDeleting(false); }
  };

  const handleAddTag = async () => {
    const tag = newTag.trim().toLowerCase();
    if (!tag || tags.includes(tag)) { setNewTag(""); setAddingTag(false); return; }
    try {
      await invoke("add_memory_tag", { memoryId: memory.id, tag });
      setTags(prev => [...prev, tag].sort());
      setNewTag(""); setAddingTag(false); onTagsChange();
    } catch {}
  };

  const handleRemoveTag = async (tag: string) => {
    try {
      await invoke("remove_memory_tag", { memoryId: memory.id, tag });
      setTags(prev => prev.filter(t => t !== tag));
      onTagsChange();
    } catch {}
  };

  const loadSources = async () => {
    if (showSources) { setShowSources(false); return; }
    if (sources.length > 0) { setShowSources(true); return; }
    try {
      const mems = await invoke<Memory[]>("get_memories_by_ids", { ids: derivedIds });
      setSources(mems);
      setShowSources(true);
    } catch { setShowSources(true); }
  };

  return (
    <div className={`about-memory${deleting ? " about-memory-deleting" : ""}`}
      style={{ borderLeft: `3px solid ${catColor}20`, marginBottom: 8, padding: "10px 14px" }}>
      {/* Content */}
      <div style={{ cursor: isLong ? "pointer" : "default", marginBottom: 8 }}
        onClick={isLong ? () => setExpanded(v => !v) : undefined}>
        <ReactMarkdown components={{ p: ({ children }) => <span style={{ fontSize: 13 }}>{children}</span> }}>
          {isLong && !expanded ? memory.content.slice(0, EXPAND_THRESHOLD) + "..." : memory.content}
        </ReactMarkdown>
        {isLong && (
          <span style={{ fontSize: 11, color: "var(--text-secondary)", marginLeft: 4 }}>
            {expanded ? t("about.collapse") : t("about.expand")}
          </span>
        )}
      </div>

      {/* Meta row: category chip + confidence + time + delete */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <span style={{
          fontSize: 10, padding: "1px 6px", borderRadius: 4,
          background: `${catColor}15`, color: catColor, fontWeight: 500,
        }}>
          {CAT_ICONS[cat] || ""} {t(`about.cat.${cat}`) || cat}
        </span>
        <ConfidenceBar value={memory.confidence} t={t} />
        <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>
          {memory.created_at?.slice(0, 10)}
        </span>
        {/* Evidence link */}
        {derivedIds.length > 0 && (
          <button onClick={loadSources} style={{
            background: "none", border: "none", cursor: "pointer",
            fontSize: 10, color: "var(--accent)", padding: 0,
          }}>
            {t("about.basedOn")} {derivedIds.length} {t("about.memories")} {showSources ? "▼" : "▶"}
          </button>
        )}
        <div style={{ flex: 1 }} />
        {/* Tags */}
        <div style={{ display: "flex", flexWrap: "wrap", gap: 3, alignItems: "center" }}>
          {tags.map(tag => (
            <span key={tag} className="memory-tag" style={{ display: "inline-flex", alignItems: "center", gap: 2 }}>
              {tag}
              <button className="memory-tag-remove" onClick={() => handleRemoveTag(tag)}>×</button>
            </span>
          ))}
          {addingTag ? (
            <input className="memory-tag-input" value={newTag}
              onChange={e => setNewTag(e.target.value)}
              onKeyDown={e => {
                if (e.key === "Enter" && !e.nativeEvent.isComposing) handleAddTag();
                if (e.key === "Escape") { setAddingTag(false); setNewTag(""); }
              }}
              onBlur={handleAddTag} placeholder="tag..." autoFocus />
          ) : (
            <button className="memory-tag-add" onClick={() => setAddingTag(true)}>+</button>
          )}
        </div>
        <button className="about-delete" onClick={handleDelete} disabled={deleting} title={t("about.deleteMemory")}>×</button>
      </div>

      {/* Evidence sources (expandable) */}
      {showSources && (
        <div style={{ marginTop: 8, paddingLeft: 12, borderLeft: "2px solid var(--border-subtle)" }}>
          {sources.length === 0 && <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>{t("about.sourcesNotFound")}</span>}
          {sources.map(s => (
            <div key={s.id} style={{ fontSize: 11, color: "var(--text-secondary)", padding: "4px 0", borderBottom: "1px solid var(--border-subtle)" }}>
              <span style={{ color: CAT_COLORS[s.category as CatKey] || "var(--text-tertiary)", marginRight: 4 }}>
                {CAT_ICONS[s.category as CatKey] || "·"}
              </span>
              {s.content.slice(0, 80)}{s.content.length > 80 ? "..." : ""}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── AboutYou ───────────────────────────────────────────────────────────────

function AboutYou() {
  const { t } = useLang();
  const [memories, setMemories] = useState<Memory[]>([]);
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState("");
  const [userInput, setUserInput] = useState("");
  const [saving, setSaving] = useState(false);
  const [aiImportText, setAiImportText] = useState("");
  const [importing, setImporting] = useState(false);
  const [allTags, setAllTags] = useState<TagInfo[]>([]);
  const [filterTag, setFilterTag] = useState<string | null>(null);
  const [filteredIds, setFilteredIds] = useState<Set<number> | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeDepth, setActiveDepth] = useState<DepthKey>("semantic");
  const [filterCat, setFilterCat] = useState<CatKey | null>(null);

  const showMsg = useCallback((msg: string, ms = 3000) => {
    setExportMsg(msg);
    setTimeout(() => setExportMsg(""), ms);
  }, []);

  const fetchTags = useCallback(async () => {
    try { const tags = await invoke<TagInfo[]>("get_all_tags"); setAllTags(tags); } catch {}
  }, []);

  const fetchMemories = useCallback(async () => {
    try {
      const result = await invoke<Memory[]>("get_memories");
      setMemories(result);
    } catch (err) { console.error("Failed to load memories:", err); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { setLoading(true); fetchMemories(); fetchTags(); }, [fetchMemories, fetchTags]);

  const handleDelete = useCallback((id: number) => {
    setMemories(prev => prev.filter(m => m.id !== id));
  }, []);

  const handleFilterByTag = async (tag: string | null) => {
    if (!tag || tag === filterTag) { setFilterTag(null); setFilteredIds(null); return; }
    setFilterTag(tag);
    try {
      const ids = await invoke<number[]>("get_memories_by_tag", { tag });
      setFilteredIds(new Set(ids));
    } catch { setFilteredIds(null); }
  };

  const handleSaveUserInput = async () => {
    const text = userInput.trim();
    if (!text) return;
    setSaving(true);
    try {
      await invoke("add_user_memory", { content: text });
      setUserInput("");
      const refreshed = await invoke<Memory[]>("get_memories");
      setMemories(refreshed);
      showMsg(t("about.saved"), 2000);
    } catch { showMsg(t("about.saveFailed"), 2000); }
    finally { setSaving(false); }
  };

  const handleExport = async () => {
    setExporting(true); setExportMsg("");
    try { await invoke<string>("export_memories"); showMsg(t("about.copiedToClipboard")); }
    catch { setExportMsg(t("about.exportFailed")); }
    finally { setExporting(false); }
  };

  const readFromClipboard = async (): Promise<string> => {
    try { return await navigator.clipboard.readText(); }
    catch { return window.prompt(t("about.pastePrompt")) || ""; }
  };

  const handleImportFromClipboard = async () => {
    try {
      const text = await readFromClipboard();
      let entries: { category: string; content: string; confidence?: number; source?: string }[];
      try { entries = JSON.parse(text); if (!Array.isArray(entries)) throw new Error("not array"); }
      catch { entries = [{ category: "identity", content: text.trim(), confidence: 0.8, source: "import" }]; }
      const count = await invoke<number>("import_memories", { entries });
      if (count > 0) {
        const refreshed = await invoke<Memory[]>("get_memories");
        setMemories(refreshed);
        showMsg(t("about.importedCount").replace("{n}", String(count)));
      }
    } catch { showMsg(t("about.importFailed")); }
  };

  const handleAiImport = async () => {
    const text = aiImportText.trim();
    if (!text) return;
    setImporting(true);
    try {
      const count = await invoke<number>("import_raw_memories", { text });
      setAiImportText("");
      if (count > 0) {
        const refreshed = await invoke<Memory[]>("get_memories");
        setMemories(refreshed);
        showMsg(t("about.importedFromAi").replace("{n}", String(count)));
      } else { showMsg(t("about.noMemoriesExtracted")); }
    } catch (err) { showMsg(t("about.importAiFailed") + String(err)); }
    finally { setImporting(false); }
  };

  // ── Filter pipeline ────────────────────────────────────────────────────

  const q = searchQuery.trim().toLowerCase();
  const displayMemories = memories
    .filter(m => !filteredIds || filteredIds.has(m.id))
    .filter(m => !q || m.content.toLowerCase().includes(q) || m.category.toLowerCase().includes(q));

  // Group by depth
  const byDepth = useMemo(() => {
    const groups: Record<DepthKey, Memory[]> = { episodic: [], semantic: [], procedural: [], axiom: [] };
    for (const m of displayMemories) {
      const d = (m.depth ?? "episodic") as DepthKey;
      if (groups[d]) groups[d].push(m);
    }
    // episodic 按时间倒序，其余按 confidence 降序
    groups.episodic.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
    for (const d of ["semantic", "procedural", "axiom"] as DepthKey[]) {
      groups[d].sort((a, b) => b.confidence - a.confidence);
    }
    return groups;
  }, [displayMemories]);

  // Active tab memories with category filter
  const activeMemories = useMemo(() => {
    let items = byDepth[activeDepth] || [];
    if (filterCat) items = items.filter(m => m.category === filterCat);
    return items;
  }, [byDepth, activeDepth, filterCat]);

  // Category counts for current depth tab
  const catCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const m of (byDepth[activeDepth] || [])) {
      counts[m.category] = (counts[m.category] || 0) + 1;
    }
    return counts;
  }, [byDepth, activeDepth]);

  const depthLabels: Record<DepthKey, string> = {
    episodic: t("about.depth.episodic"),
    semantic: t("about.depth.semantic"),
    procedural: t("about.depth.procedural"),
    axiom: t("about.depth.axiom"),
  };

  const depthDescriptions: Record<DepthKey, string> = {
    episodic: t("about.depthDesc.episodic"),
    semantic: t("about.depthDesc.semantic"),
    procedural: t("about.depthDesc.procedural"),
    axiom: t("about.depthDesc.axiom"),
  };

  return (
    <div className="about-page">
      <div className="about-header">
        <h1 className="about-title">{t("about.title")}</h1>
        <p className="about-subtitle">{t("about.subtitle")}</p>
        <div className="about-actions">
          <button className="about-action-btn" onClick={handleExport} disabled={exporting}>
            {exporting ? t("about.exporting") : t("about.exportMemories")}
          </button>
          <button className="about-action-btn" onClick={handleImportFromClipboard}>
            {t("about.importFromClipboard")}
          </button>
          <button className="about-action-btn" onClick={async () => {
            try { const result = await invoke<string>("sync_memory"); showMsg(result); }
            catch (err) { showMsg(t("about.syncFailed") + String(err)); }
          }}>
            {t("about.syncToClaudeCode")}
          </button>
          {exportMsg && <span className="about-action-msg">{exportMsg}</span>}
        </div>
      </div>

      {loading ? (
        <div className="about-empty"><p>{t("about.loadingState")}</p></div>
      ) : memories.length === 0 ? (
        <div className="about-empty"><p>{t("about.emptyState1")}<br />{t("about.emptyState2")}</p></div>
      ) : (
        <div className="about-layout">
          {/* ── Sidebar ── */}
          <div className="about-sidebar">
            {/* Depth navigation */}
            <div className="tag-cloud">
              <div className="tag-cloud-title">{t("about.cognitiveDepth")}</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                {DEPTH_ORDER.map(d => {
                  const active = activeDepth === d;
                  return (
                    <div key={d} onClick={() => { setActiveDepth(d); setFilterCat(null); }} style={{
                      display: "flex", alignItems: "center", gap: 6,
                      padding: "8px 10px", borderRadius: 8, cursor: "pointer",
                      background: active ? `${DEPTH_VISUAL[d].color}15` : "transparent",
                      borderLeft: active ? `3px solid ${DEPTH_VISUAL[d].color}` : "3px solid transparent",
                      transition: "all 0.15s",
                    }}>
                      <span style={{ fontSize: 14, color: DEPTH_VISUAL[d].color, fontWeight: 700 }}>{DEPTH_VISUAL[d].icon}</span>
                      <span style={{ fontSize: 13, fontWeight: active ? 600 : 400, color: active ? DEPTH_VISUAL[d].color : "var(--text)" }}>
                        {depthLabels[d]}
                      </span>
                      <span style={{ marginLeft: "auto", fontSize: 12, fontWeight: active ? 600 : 400, color: active ? DEPTH_VISUAL[d].color : "var(--text-secondary)" }}>
                        {byDepth[d].length}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Category filter chips */}
            <div className="tag-cloud">
              <div className="tag-cloud-title">{t("about.categoryDimension")}</div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
                {CATEGORY_ORDER.map(c => {
                  const count = catCounts[c] || 0;
                  const active = filterCat === c;
                  return (
                    <button key={c} onClick={() => setFilterCat(active ? null : c)} style={{
                      display: "inline-flex", alignItems: "center", gap: 3,
                      padding: "3px 8px", borderRadius: 12, border: "1px solid",
                      borderColor: active ? CAT_COLORS[c] : "var(--border-subtle)",
                      background: active ? `${CAT_COLORS[c]}15` : "transparent",
                      color: active ? CAT_COLORS[c] : "var(--text-secondary)",
                      fontSize: 11, cursor: "pointer", transition: "all 0.15s",
                      opacity: count === 0 ? 0.4 : 1,
                    }}>
                      {CAT_ICONS[c]} {t(`about.cat.${c}`)} <span style={{ fontWeight: 500 }}>{count}</span>
                    </button>
                  );
                })}
                {filterCat && (
                  <button onClick={() => setFilterCat(null)} style={{
                    fontSize: 11, padding: "3px 8px", borderRadius: 12,
                    border: "1px solid var(--border)", background: "transparent",
                    color: "var(--accent)", cursor: "pointer",
                  }}>
                    {t("about.clearFilter")}
                  </button>
                )}
              </div>
            </div>

            {/* Tag cloud */}
            {allTags.length > 0 && (
              <div className="tag-cloud">
                <div className="tag-cloud-title">{t("about.tags")}</div>
                <div className="tag-cloud-list">
                  {allTags.map(({ tag, count }) => (
                    <button key={tag} className={`tag-chip${filterTag === tag ? " tag-chip-active" : ""}`}
                      onClick={() => handleFilterByTag(tag)}>
                      {tag} <span className="tag-chip-count">{count}</span>
                    </button>
                  ))}
                  {filterTag && (
                    <button className="tag-chip tag-chip-clear" onClick={() => handleFilterByTag(null)}>
                      {t("about.clearFilter")}
                    </button>
                  )}
                </div>
              </div>
            )}
          </div>

          {/* ── Main content ── */}
          <div className="about-content">
            {/* Depth header */}
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
              <span style={{ fontSize: 15, color: DEPTH_VISUAL[activeDepth].color, fontWeight: 700 }}>
                {DEPTH_VISUAL[activeDepth].icon} {depthLabels[activeDepth]}
              </span>
              <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
                {depthDescriptions[activeDepth]}
              </span>
            </div>

            {/* Search bar */}
            <div style={{ marginBottom: 12 }}>
              <input type="search" placeholder={t("about.searchPlaceholder")}
                value={searchQuery} onChange={e => setSearchQuery(e.target.value)}
                style={{
                  width: "100%", padding: "7px 12px", borderRadius: 8,
                  border: "1px solid var(--border)", background: "var(--surface)",
                  color: "var(--text)", fontSize: 13, outline: "none", boxSizing: "border-box",
                }} />
            </div>

            {/* Filter info */}
            {(filterTag || filterCat || q) && (
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10, display: "flex", gap: 8, alignItems: "center" }}>
                {filterCat && <span style={{ color: CAT_COLORS[filterCat] }}>{CAT_ICONS[filterCat]} {t(`about.cat.${filterCat}`)}</span>}
                {filterTag && <span>{t("about.tagLabel")}<strong>{filterTag}</strong></span>}
                {q && <span>{t("about.searchLabel")}<strong>{q}</strong></span>}
                <span>({activeMemories.length})</span>
              </div>
            )}

            {/* Memory list */}
            {activeMemories.length === 0 ? (
              <div style={{ textAlign: "center", padding: "40px 0", color: "var(--text-tertiary)", fontSize: 13 }}>
                {t("about.noMemoriesInDepth")}
              </div>
            ) : (
              activeMemories.map(m => (
                <MemoryCard key={m.id} memory={m} onDelete={handleDelete} onTagsChange={fetchTags} t={t} />
              ))
            )}
          </div>
        </div>
      )}

      {/* Tell Sage something */}
      <div className="about-user-input">
        <div className="about-user-input-label">{t("about.tellSageLabel")}</div>
        <textarea className="about-user-textarea" placeholder={t("about.tellSagePlaceholder")}
          value={userInput} onChange={e => setUserInput(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing && (e.ctrlKey || e.metaKey)) { e.preventDefault(); handleSaveUserInput(); } }}
          rows={3} disabled={saving} />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">{t("about.saveHint")}</span>
          <button className="about-action-btn" onClick={handleSaveUserInput} disabled={saving || !userInput.trim()}>
            {saving ? t("about.saving") : t("save")}
          </button>
        </div>
      </div>

      {/* Import from AI */}
      <div className="about-user-input" style={{ marginTop: "var(--spacing-md)" }}>
        <div className="about-user-input-label">{t("about.importAiLabel")}</div>
        <textarea className="about-user-textarea" placeholder={t("about.importAiPlaceholder")}
          value={aiImportText} onChange={e => setAiImportText(e.target.value)}
          rows={4} disabled={importing} />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">{t("about.importAiHint")}</span>
          <button className="about-action-btn" onClick={handleAiImport} disabled={importing || !aiImportText.trim()}>
            {importing ? t("about.importing") : t("about.import")}
          </button>
        </div>
      </div>

      <div className="about-footer">{t("about.footer")}</div>
    </div>
  );
}

export default AboutYou;
