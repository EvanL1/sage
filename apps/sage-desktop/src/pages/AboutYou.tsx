import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import { useLang } from "../LangContext";
import type { Memory } from "../types";

interface TagInfo {
  tag: string;
  count: number;
}

// ─── Depth layer config (visual only — labels come from i18n) ────────────────

const DEPTH_VISUAL = {
  episodic: {
    icon: "·",
    borderColor: "var(--text-tertiary, #9ca3af)",
    background: "transparent",
    borderWidth: 3,
    fontSize: 13,
    defaultExpanded: true,
  },
  semantic: {
    icon: "○",
    borderColor: "#3b82f6",
    background: "rgba(59, 130, 246, 0.06)",
    borderWidth: 3,
    fontSize: 14,
    defaultExpanded: true,
  },
  procedural: {
    icon: "◇",
    borderColor: "#8b5cf6",
    background: "rgba(139, 92, 246, 0.06)",
    borderWidth: 3,
    fontSize: 14,
    defaultExpanded: true,
  },
  axiom: {
    icon: "◆",
    borderColor: "#f59e0b",
    background: "rgba(245, 158, 11, 0.08)",
    borderWidth: 4,
    fontSize: 16,
    defaultExpanded: true,
  },
} as const;

type DepthKey = keyof typeof DEPTH_VISUAL;
const DEPTH_ORDER: DepthKey[] = ["episodic", "semantic", "procedural", "axiom"];

// ─── DepthMemoryItem ─────────────────────────────────────────────────────────

const EXPAND_THRESHOLD = 80;

function DepthMemoryItem({
  memory,
  fontSize,
  onDelete,
  onTagsChange,
  expandLabel,
  collapseLabel,
  deleteLabel,
}: {
  memory: Memory;
  fontSize: number;
  onDelete: (id: number) => void;
  onTagsChange: () => void;
  expandLabel: string;
  collapseLabel: string;
  deleteLabel: string;
}) {
  const [deleting, setDeleting] = useState(false);
  const [tags, setTags] = useState<string[]>([]);
  const [addingTag, setAddingTag] = useState(false);
  const [newTag, setNewTag] = useState("");
  const isLong = memory.content.length > EXPAND_THRESHOLD;
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    invoke<string[]>("get_memory_tags", { memoryId: memory.id })
      .then(setTags)
      .catch(() => {});
  }, [memory.id]);

  const handleDelete = async () => {
    setDeleting(true);
    try {
      await invoke("delete_memory", { memoryId: memory.id });
      onDelete(memory.id);
      onTagsChange();
    } catch (err) {
      console.error("Failed to delete memory:", err);
      setDeleting(false);
    }
  };

  const handleAddTag = async () => {
    const t = newTag.trim().toLowerCase();
    if (!t || tags.includes(t)) {
      setNewTag("");
      setAddingTag(false);
      return;
    }
    try {
      await invoke("add_memory_tag", { memoryId: memory.id, tag: t });
      setTags((prev) => [...prev, t].sort());
      setNewTag("");
      setAddingTag(false);
      onTagsChange();
    } catch (err) {
      console.error("Failed to add tag:", err);
    }
  };

  const handleRemoveTag = async (tag: string) => {
    try {
      await invoke("remove_memory_tag", { memoryId: memory.id, tag });
      setTags((prev) => prev.filter((t) => t !== tag));
      onTagsChange();
    } catch (err) {
      console.error("Failed to remove tag:", err);
    }
  };

  return (
    <div className={`about-memory${deleting ? " about-memory-deleting" : ""}`}>
      <div className="about-memory-content" style={{ fontSize }}>
        <div
          style={{ cursor: isLong ? "pointer" : "default" }}
          onClick={isLong ? () => setExpanded((v) => !v) : undefined}
        >
          <ReactMarkdown
            components={{
              p: ({ children }) => <span style={{ fontSize }}>{children}</span>,
            }}
          >
            {isLong && !expanded
              ? memory.content.slice(0, EXPAND_THRESHOLD) + "..."
              : memory.content}
          </ReactMarkdown>
          {isLong && (
            <span
              style={{
                fontSize: 11,
                color: "var(--text-secondary, #9ca3af)",
                marginLeft: 4,
                userSelect: "none",
              }}
            >
              {expanded ? ` ${collapseLabel}` : ` ${expandLabel}`}
            </span>
          )}
        </div>
        <span className="about-memory-source">{memory.category}</span>
        {/* confidence bar */}
        <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4 }}>
          <div
            style={{
              width: 60,
              height: 3,
              background: "var(--border-subtle, #e5e7eb)",
              borderRadius: 2,
              overflow: "hidden",
            }}
          >
            <div
              style={{
                width: `${Math.round(memory.confidence * 100)}%`,
                height: "100%",
                background: "var(--accent, #6366f1)",
                borderRadius: 2,
              }}
            />
          </div>
          {/* tags */}
          <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
            {tags.map((tag) => (
              <span
                key={tag}
                className="memory-tag"
                style={{ display: "inline-flex", alignItems: "center", gap: 2 }}
              >
                {tag}
                <button
                  className="memory-tag-remove"
                  onClick={() => handleRemoveTag(tag)}
                >
                  ×
                </button>
              </span>
            ))}
            {addingTag ? (
              <input
                className="memory-tag-input"
                value={newTag}
                onChange={(e) => setNewTag(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) handleAddTag();
                  if (e.key === "Escape") {
                    setAddingTag(false);
                    setNewTag("");
                  }
                }}
                onBlur={handleAddTag}
                placeholder="tag..."
                autoFocus
              />
            ) : (
              <button className="memory-tag-add" onClick={() => setAddingTag(true)}>
                +
              </button>
            )}
          </div>
        </div>
      </div>
      <button
        className="about-delete"
        onClick={handleDelete}
        disabled={deleting}
        title={deleteLabel}
        aria-label="Delete"
      >
        ×
      </button>
    </div>
  );
}

// ─── DepthLayer ──────────────────────────────────────────────────────────────

function DepthLayer({
  depthKey,
  label,
  memories,
  onDelete,
  onTagsChange,
  expandLabel,
  collapseLabel,
  deleteLabel,
}: {
  depthKey: DepthKey;
  label: string;
  memories: Memory[];
  onDelete: (id: number) => void;
  onTagsChange: () => void;
  expandLabel: string;
  collapseLabel: string;
  deleteLabel: string;
}) {
  const cfg = DEPTH_VISUAL[depthKey];
  const [expanded, setExpanded] = useState<boolean>(cfg.defaultExpanded);

  if (memories.length === 0) return null;

  const sectionStyle: React.CSSProperties = {
    borderLeft: `${cfg.borderWidth}px solid ${cfg.borderColor}`,
    background: cfg.background,
    borderRadius: "0 6px 6px 0",
    marginBottom: 16,
    overflow: "hidden",
  };

  const headerStyle: React.CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "10px 14px",
    cursor: "pointer",
    userSelect: "none",
    borderBottom: expanded ? `1px solid ${cfg.borderColor}22` : "none",
  };

  const iconStyle: React.CSSProperties = {
    color: cfg.borderColor,
    fontSize: 14,
    fontWeight: 700,
    lineHeight: 1,
  };

  const titleStyle: React.CSSProperties = {
    fontWeight: 600,
    fontSize: 13,
    color: cfg.borderColor,
    flex: 1,
  };

  const countStyle: React.CSSProperties = {
    fontSize: 12,
    color: "var(--text-secondary, #9ca3af)",
    marginLeft: 4,
    fontWeight: 400,
  };

  const toggleStyle: React.CSSProperties = {
    fontSize: 10,
    color: "var(--text-secondary, #9ca3af)",
  };

  const itemsStyle: React.CSSProperties = {
    padding: "4px 14px 8px",
  };

  return (
    <section style={sectionStyle}>
      <div
        style={headerStyle}
        onClick={() => setExpanded((v) => !v)}
      >
        <span style={iconStyle}>{cfg.icon}</span>
        <span style={titleStyle}>
          {label}
          <span style={countStyle}>({memories.length})</span>
        </span>
        <span style={toggleStyle}>{expanded ? "▼" : "▶"}</span>
      </div>
      {expanded && (
        <div style={itemsStyle}>
          {memories.map((m) => (
            <DepthMemoryItem
              key={m.id}
              memory={m}
              fontSize={cfg.fontSize}
              onDelete={onDelete}
              onTagsChange={onTagsChange}
              expandLabel={expandLabel}
              collapseLabel={collapseLabel}
              deleteLabel={deleteLabel}
            />
          ))}
        </div>
      )}
    </section>
  );
}

// ─── AboutYou ────────────────────────────────────────────────────────────────

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

  const showMsg = useCallback((msg: string, ms = 3000) => {
    setExportMsg(msg);
    setTimeout(() => setExportMsg(""), ms);
  }, []);

  const fetchTags = useCallback(async () => {
    try {
      const tags = await invoke<TagInfo[]>("get_all_tags");
      setAllTags(tags);
    } catch { /* silent */ }
  }, []);

  const fetchMemories = useCallback(async () => {
    try {
      const result = await invoke<Memory[]>("get_memories");
      setMemories(result);
    } catch (err) {
      console.error("Failed to load memories:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    fetchMemories();
    fetchTags();
  }, [fetchMemories, fetchTags]);

  const handleDelete = useCallback((id: number) => {
    setMemories((prev) => prev.filter((m) => m.id !== id));
  }, []);

  const handleFilterByTag = async (tag: string | null) => {
    if (!tag || tag === filterTag) {
      setFilterTag(null);
      setFilteredIds(null);
      return;
    }
    setFilterTag(tag);
    try {
      const ids = await invoke<number[]>("get_memories_by_tag", { tag });
      setFilteredIds(new Set(ids));
    } catch {
      setFilteredIds(null);
    }
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
    } catch (err) {
      console.error("Failed to save user memory:", err);
      showMsg(t("about.saveFailed"), 2000);
    } finally {
      setSaving(false);
    }
  };

  const handleExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      await invoke<string>("export_memories");
      showMsg(t("about.copiedToClipboard"));
    } catch (err) {
      console.error("Export failed:", err);
      setExportMsg(t("about.exportFailed"));
    } finally {
      setExporting(false);
    }
  };

  const readFromClipboard = async (): Promise<string> => {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return window.prompt(t("about.pastePrompt")) || "";
    }
  };

  const handleImportFromClipboard = async () => {
    try {
      const text = await readFromClipboard();
      let entries: { category: string; content: string; confidence?: number; source?: string }[];
      try {
        entries = JSON.parse(text);
        if (!Array.isArray(entries)) throw new Error("not array");
      } catch {
        entries = [{ category: "identity", content: text.trim(), confidence: 0.8, source: "import" }];
      }
      const count = await invoke<number>("import_memories", { entries });
      if (count > 0) {
        const refreshed = await invoke<Memory[]>("get_memories");
        setMemories(refreshed);
        showMsg(t("about.importedCount").replace("{n}", String(count)));
      }
    } catch (err) {
      console.error("Import failed:", err);
      showMsg(t("about.importFailed"));
    }
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
      } else {
        showMsg(t("about.noMemoriesExtracted"));
      }
    } catch (err) {
      console.error("AI import failed:", err);
      showMsg(t("about.importAiFailed") + String(err));
    } finally {
      setImporting(false);
    }
  };

  // ── Filter pipeline ──────────────────────────────────────────────────────

  const q = searchQuery.trim().toLowerCase();
  const displayMemories = memories
    .filter((m) => !filteredIds || filteredIds.has(m.id))
    .filter((m) => !q || m.content.toLowerCase().includes(q) || m.category.toLowerCase().includes(q));

  // Group by depth, episodic sorted newest-first
  const byDepth = DEPTH_ORDER.reduce<Record<DepthKey, Memory[]>>(
    (acc, d) => {
      let items = displayMemories.filter((m) => (m.depth ?? "episodic") === d);
      if (d === "episodic") {
        items = items.sort(
          (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        );
      }
      acc[d] = items;
      return acc;
    },
    { axiom: [], procedural: [], semantic: [], episodic: [] }
  );

  const totalVisible = displayMemories.length;

  // Depth labels via i18n
  const depthLabels: Record<DepthKey, string> = {
    episodic: t("about.depth.episodic"),
    semantic: t("about.depth.semantic"),
    procedural: t("about.depth.procedural"),
    axiom: t("about.depth.axiom"),
  };

  // ── Sidebar ──────────────────────────────────────────────────────────────

  const sidebarDepthStyle = (d: DepthKey): React.CSSProperties => ({
    display: "flex",
    alignItems: "center",
    gap: 6,
    padding: "4px 8px",
    borderRadius: 6,
    fontSize: 13,
    cursor: "default",
    color: DEPTH_VISUAL[d].borderColor,
    fontWeight: 500,
  });

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
          <button
            className="about-action-btn"
            onClick={async () => {
              try {
                const result = await invoke<string>("sync_memory");
                showMsg(result);
              } catch (err) {
                showMsg(t("about.syncFailed") + String(err));
              }
            }}
          >
            {t("about.syncToClaudeCode")}
          </button>
          {exportMsg && <span className="about-action-msg">{exportMsg}</span>}
        </div>
      </div>

      {loading ? (
        <div className="about-empty">
          <p>{t("about.loadingState")}</p>
        </div>
      ) : memories.length === 0 ? (
        <div className="about-empty">
          <p>
            {t("about.emptyState1")}
            <br />
            {t("about.emptyState2")}
          </p>
        </div>
      ) : (
        <div className="about-layout">
          {/* ── Sidebar ── */}
          <div className="about-sidebar">
            {/* Depth summary */}
            <div className="tag-cloud">
              <div className="tag-cloud-title">{t("about.cognitiveDepth")}</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                {DEPTH_ORDER.map((d) => (
                  <div key={d} style={sidebarDepthStyle(d)}>
                    <span style={{ fontSize: 14 }}>{DEPTH_VISUAL[d].icon}</span>
                    <span>{depthLabels[d]}</span>
                    <span
                      style={{
                        marginLeft: "auto",
                        fontSize: 12,
                        color: "var(--text-secondary, #9ca3af)",
                        fontWeight: 400,
                      }}
                    >
                      {byDepth[d].length}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Tag cloud */}
            {allTags.length > 0 && (
              <div className="tag-cloud">
                <div className="tag-cloud-title">{t("about.tags")}</div>
                <div className="tag-cloud-list">
                  {allTags.map(({ tag, count }) => (
                    <button
                      key={tag}
                      className={`tag-chip${filterTag === tag ? " tag-chip-active" : ""}`}
                      onClick={() => handleFilterByTag(tag)}
                    >
                      {tag} <span className="tag-chip-count">{count}</span>
                    </button>
                  ))}
                  {filterTag && (
                    <button
                      className="tag-chip tag-chip-clear"
                      onClick={() => handleFilterByTag(null)}
                    >
                      {t("about.clearFilter")}
                    </button>
                  )}
                </div>
              </div>
            )}
          </div>

          {/* ── Main content ── */}
          <div className="about-content">
            {/* Search bar */}
            <div style={{ marginBottom: 16 }}>
              <input
                type="search"
                placeholder={t("about.searchPlaceholder")}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                style={{
                  width: "100%",
                  padding: "7px 12px",
                  borderRadius: 8,
                  border: "1px solid var(--border, #e5e7eb)",
                  background: "var(--surface, #fff)",
                  color: "var(--text-primary, #111)",
                  fontSize: 13,
                  outline: "none",
                  boxSizing: "border-box",
                }}
              />
            </div>

            {/* Active filter info */}
            {(filterTag || q) && (
              <div
                style={{
                  fontSize: 13,
                  color: "var(--text-secondary)",
                  marginBottom: 12,
                }}
              >
                {filterTag && (
                  <>
                    {t("about.tagLabel")}<strong>{filterTag}</strong>
                    {"  "}
                  </>
                )}
                {q && (
                  <>
                    {t("about.searchLabel")}<strong>{q}</strong>
                    {"  "}
                  </>
                )}
                <span style={{ color: "var(--text-secondary)" }}>
                  ({totalVisible} {t("about.memoriesCount")})
                </span>
              </div>
            )}

            {/* Depth layers */}
            {DEPTH_ORDER.map((d) => (
              <DepthLayer
                key={d}
                depthKey={d}
                label={depthLabels[d]}
                memories={byDepth[d]}
                onDelete={handleDelete}
                onTagsChange={fetchTags}
                expandLabel={t("about.expand")}
                collapseLabel={t("about.collapse")}
                deleteLabel={t("about.deleteMemory")}
              />
            ))}
          </div>
        </div>
      )}

      {/* Tell Sage something */}
      <div className="about-user-input">
        <div className="about-user-input-label">{t("about.tellSageLabel")}</div>
        <textarea
          className="about-user-textarea"
          placeholder={t("about.tellSagePlaceholder")}
          value={userInput}
          onChange={(e) => setUserInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.nativeEvent.isComposing && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              handleSaveUserInput();
            }
          }}
          rows={3}
          disabled={saving}
        />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">{t("about.saveHint")}</span>
          <button
            className="about-action-btn"
            onClick={handleSaveUserInput}
            disabled={saving || !userInput.trim()}
          >
            {saving ? t("about.saving") : t("save")}
          </button>
        </div>
      </div>

      {/* Import from AI */}
      <div className="about-user-input" style={{ marginTop: "var(--spacing-md)" }}>
        <div className="about-user-input-label">{t("about.importAiLabel")}</div>
        <textarea
          className="about-user-textarea"
          placeholder={t("about.importAiPlaceholder")}
          value={aiImportText}
          onChange={(e) => setAiImportText(e.target.value)}
          rows={4}
          disabled={importing}
        />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">{t("about.importAiHint")}</span>
          <button
            className="about-action-btn"
            onClick={handleAiImport}
            disabled={importing || !aiImportText.trim()}
          >
            {importing ? t("about.importing") : t("about.import")}
          </button>
        </div>
      </div>

      <div className="about-footer">
        {t("about.footer")}
      </div>
    </div>
  );
}

export default AboutYou;
