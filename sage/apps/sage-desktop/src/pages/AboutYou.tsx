import { useState, useEffect, useCallback } from "react";
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
};

const CATEGORY_ORDER = ["personality", "identity", "values", "behavior", "thinking", "emotion", "growth"];

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

  const copyToClipboard = async (text: string): Promise<boolean> => {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Fallback for Tauri webview where clipboard API may be restricted
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(ta);
      return ok;
    }
  };

  const handleExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      const md = await invoke<string>("export_memories");
      const ok = await copyToClipboard(md);
      setExportMsg(ok ? "Copied to clipboard" : "Export failed");
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

  const grouped = CATEGORY_ORDER.reduce<Record<string, Memory[]>>((acc, cat) => {
    const items = memories.filter((m) => m.category === cat);
    if (items.length > 0) {
      acc[cat] = items;
    }
    return acc;
  }, {});

  const unknownItems = memories.filter((m) => !CATEGORY_ORDER.includes(m.category));

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
        <>
          {Object.entries(grouped).map(([cat, items]) => (
            <div key={cat} className="about-category">
              <div className="about-category-title">
                {CATEGORY_LABELS[cat] ?? cat}
              </div>
              {items.map((memory) => (
                <MemoryItem key={memory.id} memory={memory} onDelete={handleDelete} />
              ))}
            </div>
          ))}

          {unknownItems.length > 0 && (
            <div className="about-category">
              <div className="about-category-title">Other</div>
              {unknownItems.map((memory) => (
                <MemoryItem key={memory.id} memory={memory} onDelete={handleDelete} />
              ))}
            </div>
          )}
        </>
      )}

      <div className="about-footer">
        These observations may not be fully accurate — people are complex. Delete anything that doesn't feel right to help me understand you better.
      </div>
    </div>
  );
}

export default AboutYou;
