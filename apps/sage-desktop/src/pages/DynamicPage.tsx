import { useState, useEffect, useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import { parsePage, serializeNodes, type PageNode } from "../dynpage/parser";
import { renderNodes } from "../dynpage/ComponentRenderer";
import type { CustomPage } from "../types";

export default function DynamicPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { t } = useLang();
  const [page, setPage] = useState<CustomPage | null>(null);
  const [blocks, setBlocks] = useState<PageNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [overIdx, setOverIdx] = useState<number | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout>>();

  const load = useCallback(() => {
    if (!id) return;
    setLoading(true);
    setError(null);
    invoke<CustomPage>("get_custom_page", { id: parseInt(id, 10) })
      .then(p => {
        setPage(p);
        setBlocks(parsePage(p.markdown ?? ""));
        setLoading(false);
      })
      .catch(err => { setError(String(err)); setLoading(false); });
  }, [id]);

  useEffect(() => { load(); }, [load]);

  // Debounced save after reorder
  const saveBlocks = useCallback((newBlocks: PageNode[], pg: CustomPage) => {
    clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      const md = serializeNodes(newBlocks);
      invoke("update_custom_page", { id: pg.id, title: pg.title, markdown: md }).catch(console.error);
    }, 500);
  }, []);

  const handleDragStart = (idx: number) => setDragIdx(idx);
  const handleDragOver = (e: React.DragEvent, idx: number) => {
    e.preventDefault();
    if (idx !== overIdx) setOverIdx(idx);
  };
  const handleDragEnd = () => {
    if (dragIdx !== null && overIdx !== null && dragIdx !== overIdx && page) {
      const newBlocks = [...blocks];
      const [moved] = newBlocks.splice(dragIdx, 1);
      newBlocks.splice(overIdx, 0, moved);
      setBlocks(newBlocks);
      saveBlocks(newBlocks, page);
    }
    setDragIdx(null);
    setOverIdx(null);
  };

  const handleDelete = async () => {
    if (!page || deleting) return;
    setDeleting(true);
    try {
      await invoke("delete_custom_page", { id: page.id });
      navigate("/pages");
    } catch (err) {
      console.error(err);
      setDeleting(false);
    }
  };

  if (loading) {
    return <div style={{ padding: 24, color: "var(--text-secondary)" }}>Loading...</div>;
  }
  if (error || !page) {
    return (
      <div style={{ padding: 24 }}>
        <div style={{ color: "var(--error)", marginBottom: 12 }}>{error || "Page not found"}</div>
        <button onClick={() => navigate("/pages")} style={S.ghostBtn}>← {t("pages.backToList")}</button>
      </div>
    );
  }

  return (
    <div style={{ padding: "16px 24px", overflowY: "auto" }}>
      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 16 }}>
        <button onClick={() => navigate("/pages")} style={S.ghostBtn}>← {t("pages.backToList")}</button>
        <div style={{ flex: 1 }} />
        <button onClick={handleDelete} disabled={deleting} style={S.deleteBtn}>
          {t("pages.delete")}
        </button>
      </div>

      {/* Title */}
      <h1 style={{ fontSize: 20, fontWeight: 700, color: "var(--text)", marginBottom: 4 }}>{page.title}</h1>
      <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 20 }}>{page.created_at.slice(0, 10)}</div>

      {/* Blocks */}
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {blocks.map((node, idx) => {
          const isDragging = dragIdx === idx;
          const isOver = overIdx === idx;
          return (
            <div
              key={idx}
              draggable
              onDragStart={() => handleDragStart(idx)}
              onDragOver={(e) => handleDragOver(e, idx)}
              onDragEnd={handleDragEnd}
              style={{
                position: "relative",
                opacity: isDragging ? 0.4 : 1,
                borderTop: isOver && dragIdx !== null && dragIdx > idx ? "2px solid var(--accent)" : "2px solid transparent",
                borderBottom: isOver && dragIdx !== null && dragIdx < idx ? "2px solid var(--accent)" : "2px solid transparent",
                borderRadius: 6,
                transition: "opacity 0.15s",
              }}
            >
              {/* Drag handle — visible on hover */}
              <div
                style={{
                  position: "absolute", left: -24, top: 4,
                  width: 20, height: 20,
                  display: "flex", alignItems: "center", justifyContent: "center",
                  cursor: "grab", opacity: 0, transition: "opacity 0.15s",
                  color: "var(--text-tertiary, var(--text-secondary))",
                  fontSize: 14,
                }}
                className="drag-handle"
              >
                ⠿
              </div>
              <div style={{ lineHeight: 1.7, color: "var(--text)" }}>
                {renderNodes([node])}
              </div>
            </div>
          );
        })}
      </div>

      {/* Hover style for drag handles */}
      <style>{`
        div[draggable]:hover .drag-handle { opacity: 1 !important; }
        div[draggable]:active { cursor: grabbing; }
      `}</style>
    </div>
  );
}

const S = {
  ghostBtn: {
    padding: "5px 12px", borderRadius: 6, border: "1px solid var(--border)",
    background: "transparent", color: "var(--text-secondary)", fontSize: 12, cursor: "pointer",
  } as React.CSSProperties,
  deleteBtn: {
    padding: "5px 12px", borderRadius: 6, border: "1px solid var(--error, #ef4444)",
    background: "transparent", color: "var(--error, #ef4444)", fontSize: 12, cursor: "pointer",
  } as React.CSSProperties,
};
