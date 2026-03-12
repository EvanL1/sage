import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Memory {
  id: number;
  category: string;
  content: string;
  source: string;
  confidence: number;
  created_at: string;
  updated_at: string;
}

const CATEGORY_LABELS: Record<string, string> = {
  personality: "Personality",
  identity: "Identity",
  values: "Values",
  behavior: "Behavior",
  thinking: "Thinking style",
  emotion: "Emotional cues",
  growth: "Growth direction",
  preference: "Preferences",
  skill: "Skills",
  goal: "Goals",
  relationship: "Relationships",
  // 用户主动告诉 Sage 的内容
  user_input: "From you",
};

const CATEGORY_ORDER = [
  "personality", "identity", "values", "behavior", "thinking", "emotion",
  "growth", "preference", "skill", "goal", "relationship", "user_input",
];

function MemoryItem({ memory, onDelete }: { memory: Memory; onDelete: (id: number) => void }) {
  const [deleting, setDeleting] = useState(false);

  const handleDelete = async () => {
    setDeleting(true);
    try {
      await invoke("delete_memory", { memoryId: memory.id });
      onDelete(memory.id);
    } catch (err) {
      console.error("Failed to delete memory:", err);
      setDeleting(false);
    }
  };

  const sourceLabel = memory.source === "chat" ? "Chat"
    : memory.source === "assessment" ? "Assessment"
    : memory.source === "import" ? "Import"
    : memory.source === "user" ? "You"
    : "Observation";

  return (
    <div className={`about-memory${deleting ? " about-memory-deleting" : ""}`}>
      <div className="about-memory-content">
        <div>{memory.content}</div>
        <div className="about-memory-meta">
          <div className="about-confidence-bar">
            <div
              className="about-confidence-fill"
              style={{ width: `${Math.round(memory.confidence * 100)}%` }}
            />
          </div>
          <span className="about-memory-source">{sourceLabel}</span>
        </div>
      </div>
      <button
        className="about-delete"
        onClick={handleDelete}
        disabled={deleting}
        title="Delete this memory"
        aria-label="Delete"
      >
        ×
      </button>
    </div>
  );
}

function AboutYou() {
  const [memories, setMemories] = useState<Memory[]>([]);
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState("");
  const [userInput, setUserInput] = useState("");
  const [saving, setSaving] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const categoryRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [aiImportText, setAiImportText] = useState("");
  const [importing, setImporting] = useState(false);

  const fetchMemories = useCallback(async () => {
    try {
      const result = await invoke<Memory[]>("get_memories");
      setMemories(result);
    } catch (err) {
      console.error("Failed to load memories:", err);
    } finally {
      setLoading(false);
    }

    // Silently extract new memories in the background
    try {
      const history = await invoke<{ id: number; session_id: string }[]>("get_chat_history", { limit: 1 });
      if (history.length > 0) {
        const newMemories = await invoke<Memory[]>("extract_memories", { sessionId: history[0].session_id });
        if (newMemories.length > 0) {
          const refreshed = await invoke<Memory[]>("get_memories");
          setMemories(refreshed);
        }
      }
    } catch (err) {
      console.error("Memory extraction failed:", err);
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    fetchMemories();
  }, [fetchMemories]);

  const handleDelete = useCallback((id: number) => {
    setMemories((prev) => prev.filter((m) => m.id !== id));
  }, []);

  // 保存用户主动输入的记忆
  const handleSaveUserInput = async () => {
    const text = userInput.trim();
    if (!text) return;
    setSaving(true);
    try {
      await invoke("add_user_memory", { content: text });
      setUserInput("");
      // 刷新记忆列表，让新条目即时显示
      const refreshed = await invoke<Memory[]>("get_memories");
      setMemories(refreshed);
      setExportMsg("Saved");
      setTimeout(() => setExportMsg(""), 2000);
    } catch (err) {
      console.error("Failed to save user memory:", err);
      setExportMsg("Save failed");
      setTimeout(() => setExportMsg(""), 2000);
    } finally {
      setSaving(false);
    }
  };

  const handleExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      await invoke<string>("export_memories");
      setExportMsg("Copied to clipboard");
      setTimeout(() => setExportMsg(""), 3000);
    } catch (err) {
      console.error("Export failed:", err);
      setExportMsg("Export failed");
    } finally {
      setExporting(false);
    }
  };

  const readFromClipboard = async (): Promise<string> => {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return window.prompt("Paste your content here:") || "";
    }
  };

  const handleImportFromClipboard = async () => {
    try {
      const text = await readFromClipboard();
      // Try to parse as JSON array
      let entries: { category: string; content: string; confidence?: number; source?: string }[];
      try {
        entries = JSON.parse(text);
        if (!Array.isArray(entries)) throw new Error("not array");
      } catch {
        // Treat as plain text — import as single identity entry
        entries = [{ category: "identity", content: text.trim(), confidence: 0.8, source: "import" }];
      }
      const count = await invoke<number>("import_memories", { entries });
      if (count > 0) {
        const refreshed = await invoke<Memory[]>("get_memories");
        setMemories(refreshed);
        setExportMsg(`Imported ${count} memories`);
        setTimeout(() => setExportMsg(""), 3000);
      }
    } catch (err) {
      console.error("Import failed:", err);
      setExportMsg("Import failed — please copy content to clipboard first");
      setTimeout(() => setExportMsg(""), 3000);
    }
  };

  // 从其他 AI 导入记忆（粘贴原始文本，LLM 自动结构化）
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
        setExportMsg(`Imported ${count} memories from AI`);
      } else {
        setExportMsg("No memories extracted — try pasting more content");
      }
      setTimeout(() => setExportMsg(""), 3000);
    } catch (err) {
      console.error("AI import failed:", err);
      setExportMsg("Import failed: " + String(err));
      setTimeout(() => setExportMsg(""), 3000);
    } finally {
      setImporting(false);
    }
  };

  // 只保留已知分类，过滤掉不属于 "Who am I" 的杂项
  const grouped = CATEGORY_ORDER.reduce<Record<string, Memory[]>>((acc, cat) => {
    const items = memories.filter((m) => m.category === cat);
    if (items.length > 0) {
      acc[cat] = items;
    }
    return acc;
  }, {});

  const visibleCategories = Object.keys(grouped);

  const toggleCollapse = (cat: string) => {
    setCollapsed((prev) => ({ ...prev, [cat]: !prev[cat] }));
  };

  const scrollToCategory = (cat: string) => {
    categoryRefs.current[cat]?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="about-page">
      <div className="about-header">
        <h1 className="about-title">What Sage knows about you</h1>
        <p className="about-subtitle">
          These are observations accumulated through our conversations and daily interactions. You can correct or delete anything that's inaccurate.
        </p>
        <div className="about-actions">
          <button className="about-action-btn" onClick={handleExport} disabled={exporting}>
            {exporting ? "Exporting..." : "Export memories"}
          </button>
          <button className="about-action-btn" onClick={handleImportFromClipboard}>
            Import from clipboard
          </button>
          <button
            className="about-action-btn"
            onClick={async () => {
              try {
                const result = await invoke<string>("sync_memory");
                setExportMsg(result);
                setTimeout(() => setExportMsg(""), 3000);
              } catch (err) {
                setExportMsg("Sync failed: " + String(err));
                setTimeout(() => setExportMsg(""), 3000);
              }
            }}
          >
            Sync to Claude Code
          </button>
          {exportMsg && <span className="about-action-msg">{exportMsg}</span>}
        </div>
      </div>

      {loading ? (
        <div className="about-empty">
          <p>Sage is organizing what it knows about you...</p>
        </div>
      ) : memories.length === 0 ? (
        <div className="about-empty">
          <p>
            Not enough to go on yet.<br />
            Chat with me more and I'll get to know you.
          </p>
        </div>
      ) : (
        <div className="about-layout">
          {visibleCategories.length > 1 && (
            <nav className="about-toc">
              {visibleCategories.map((cat) => (
                <button
                  key={cat}
                  className="about-toc-item"
                  onClick={() => scrollToCategory(cat)}
                >
                  {CATEGORY_LABELS[cat] ?? cat}
                  <span className="about-toc-count">{grouped[cat].length}</span>
                </button>
              ))}
            </nav>
          )}
          <div className="about-content">
            {visibleCategories.map((cat) => (
              <div
                key={cat}
                className="about-category"
                ref={(el) => { categoryRefs.current[cat] = el; }}
              >
                <button
                  className="about-category-title"
                  onClick={() => toggleCollapse(cat)}
                >
                  <span className={`about-collapse-arrow${collapsed[cat] ? " collapsed" : ""}`}>
                    &#9662;
                  </span>
                  {CATEGORY_LABELS[cat] ?? cat}
                  <span className="about-category-count">{grouped[cat].length}</span>
                </button>
                {!collapsed[cat] && grouped[cat].map((memory) => (
                  <MemoryItem key={memory.id} memory={memory} onDelete={handleDelete} />
                ))}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 用户主动输入区：让用户告诉 Sage 想被记住的内容 */}
      <div className="about-user-input">
        <div className="about-user-input-label">Tell Sage something about yourself</div>
        <textarea
          className="about-user-textarea"
          placeholder="Tell Sage something about yourself..."
          value={userInput}
          onChange={(e) => setUserInput(e.target.value)}
          onKeyDown={(e) => {
            // Ctrl/Cmd + Enter 快捷提交
            if (e.key === "Enter" && !e.nativeEvent.isComposing && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              handleSaveUserInput();
            }
          }}
          rows={3}
          disabled={saving}
        />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">⌘↵ to save</span>
          <button
            className="about-action-btn"
            onClick={handleSaveUserInput}
            disabled={saving || !userInput.trim()}
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>

      {/* 从其他 AI 导入记忆 */}
      <div className="about-user-input" style={{ marginTop: "var(--spacing-md)" }}>
        <div className="about-user-input-label">Import memories from other AI assistants</div>
        <textarea
          className="about-user-textarea"
          placeholder="Paste your memories from Claude, Gemini, or ChatGPT here. Sage will automatically structure and save them."
          value={aiImportText}
          onChange={(e) => setAiImportText(e.target.value)}
          rows={4}
          disabled={importing}
        />
        <div className="about-user-input-actions">
          <span className="about-user-input-hint">Paste from Claude / Gemini / ChatGPT</span>
          <button
            className="about-action-btn"
            onClick={handleAiImport}
            disabled={importing || !aiImportText.trim()}
          >
            {importing ? "Importing..." : "Import"}
          </button>
        </div>
      </div>

      <div className="about-footer">
        These observations may not be fully accurate — people are complex. Delete anything that doesn't feel right to help me understand you better.
      </div>
    </div>
  );
}

export default AboutYou;
