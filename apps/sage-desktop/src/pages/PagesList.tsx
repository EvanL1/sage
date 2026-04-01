import { useState, useEffect, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import type { CustomPage } from "../types";

export default function PagesList() {
  const navigate = useNavigate();
  const { t } = useLang();
  const [pages, setPages] = useState<CustomPage[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(() => {
    invoke<CustomPage[]>("list_custom_pages")
      .then(p => { setPages(p); setLoading(false); })
      .catch(err => { console.error("list_custom_pages:", err); setLoading(false); });
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async (id: number, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke("delete_custom_page", { id });
      load();
    } catch (err) {
      console.error("delete_custom_page:", err);
    }
  };

  return (
    <div className="chat-page" style={{ padding: "16px 24px" }}>
      <h1 style={{ fontSize: 20, fontWeight: 700, color: "var(--text)", marginBottom: 20 }}>
        {t("pages.title")}
      </h1>

      {loading ? (
        <div style={{ color: "var(--text-secondary)" }}>{t("loading")}</div>
      ) : pages.length === 0 ? (
        <div style={{ color: "var(--text-secondary)", fontSize: 14 }}>
          {t("pages.empty")}
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {pages.map(page => (
            <div
              key={page.id}
              onClick={() => navigate(`/pages/${page.id}`)}
              style={{
                padding: "12px 16px",
                borderRadius: "var(--radius, 8px)",
                border: "1px solid var(--border)",
                background: "var(--surface)",
                cursor: "pointer",
                display: "flex",
                alignItems: "center",
                gap: 12,
                transition: "border-color 0.15s",
              }}
              onMouseEnter={e => (e.currentTarget.style.borderColor = "var(--accent)")}
              onMouseLeave={e => (e.currentTarget.style.borderColor = "var(--border)")}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text)", marginBottom: 2 }}>
                  {page.title}
                </div>
                <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>
                  {page.created_at.slice(0, 10)}
                </div>
              </div>
              <button
                onClick={e => handleDelete(page.id, e)}
                style={{
                  padding: "4px 10px", borderRadius: 5,
                  border: "1px solid var(--border)",
                  background: "transparent", color: "var(--text-secondary)",
                  fontSize: 11, cursor: "pointer", flexShrink: 0,
                }}
              >
                {t("pages.delete")}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
