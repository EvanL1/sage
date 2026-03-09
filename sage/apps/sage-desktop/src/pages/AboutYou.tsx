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
  personality: "人格特质",
  identity: "身份认同",
  values: "价值观",
  behavior: "行为模式",
  thinking: "思维方式",
  emotion: "情绪线索",
  growth: "成长方向",
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
      console.error("删除记忆失败:", err);
      setDeleting(false);
    }
  };

  const sourceLabel = memory.source === "chat" ? "对话"
    : memory.source === "assessment" ? "测评"
    : memory.source === "import" ? "导入"
    : "观察";

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
        title="删除这条记忆"
        aria-label="删除"
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
      console.error("加载记忆失败:", err);
    } finally {
      setLoading(false);
    }

    // 后台静默提取新记忆
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

  const handleExport = async () => {
    setExporting(true);
    setExportMsg("");
    try {
      const md = await invoke<string>("export_memories");
      await navigator.clipboard.writeText(md);
      setExportMsg("已复制到剪贴板");
      setTimeout(() => setExportMsg(""), 3000);
    } catch (err) {
      console.error("导出失败:", err);
      setExportMsg("导出失败");
    } finally {
      setExporting(false);
    }
  };

  const handleImportFromClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
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
        setExportMsg(`已导入 ${count} 条记忆`);
        setTimeout(() => setExportMsg(""), 3000);
      }
    } catch (err) {
      console.error("导入失败:", err);
      setExportMsg("导入失败 — 请先复制内容到剪贴板");
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
        <h1 className="about-title">Sage 对你的了解</h1>
        <p className="about-subtitle">
          这些是通过我们的对话和日常观察积累的认识。你可以修正或删除任何不准确的部分。
        </p>
        <div className="about-actions">
          <button className="about-action-btn" onClick={handleExport} disabled={exporting}>
            {exporting ? "导出中..." : "导出记忆"}
          </button>
          <button className="about-action-btn" onClick={handleImportFromClipboard}>
            从剪贴板导入
          </button>
          {exportMsg && <span className="about-action-msg">{exportMsg}</span>}
        </div>
      </div>

      {loading ? (
        <div className="about-empty">
          <p>Sage 正在整理对你的了解...</p>
        </div>
      ) : memories.length === 0 ? (
        <div className="about-empty">
          <p>
            还没有足够的了解。
            <br />
            多和我聊聊，我会慢慢认识你的。
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
              <div className="about-category-title">其他</div>
              {unknownItems.map((memory) => (
                <MemoryItem key={memory.id} memory={memory} onDelete={handleDelete} />
              ))}
            </div>
          )}
        </>
      )}

      <div className="about-footer">
        这些观察可能不完全准确 — 人是复杂的。随时删除你觉得不对的，帮我更好地了解你。
      </div>
    </div>
  );
}

export default AboutYou;
