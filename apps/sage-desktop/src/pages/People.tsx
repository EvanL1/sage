import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import { PersonMemory } from "../types";

function People() {
  const { t } = useLang();
  const [persons, setPersons] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [memories, setMemories] = useState<PersonMemory[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [extracting, setExtracting] = useState(false);
  // 合并相关
  const [mergeMode, setMergeMode] = useState(false);
  const [mergeSelection, setMergeSelection] = useState<Set<string>>(new Set());
  // mergeTarget = last-clicked person; that person is kept; others are merged into it
  const [mergeTarget, setMergeTarget] = useState<string | null>(null);
  const [merging, setMerging] = useState(false);
  const [mergeMsg, setMergeMsg] = useState<string | null>(null);
  const mergeMsgTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refreshPersons = useCallback(async () => {
    const list = await invoke<string[]>("get_known_persons");
    setPersons(list);
    return list;
  }, []);

  useEffect(() => {
    refreshPersons()
      .then((list) => { if (list.length > 0) setSelected((prev) => prev ?? list[0]); })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [refreshPersons]);

  const loadMemories = useCallback(async (name: string) => {
    setMemories([]);
    try {
      const mems = await invoke<PersonMemory[]>("get_memories_about_person", { name });
      setMemories(mems);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (selected && !mergeMode) loadMemories(selected);
  }, [selected, loadMemories, mergeMode]);

  const handleExtract = async () => {
    setExtracting(true);
    try {
      await invoke("trigger_person_extract");
      setTimeout(async () => {
        await refreshPersons();
        if (selected) loadMemories(selected);
        setExtracting(false);
      }, 3000);
    } catch (e) {
      setError(String(e));
      setExtracting(false);
    }
  };

  const toggleMergeSelect = (name: string) => {
    setMergeSelection((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
        // If we deselect the current target, pick any remaining as new target
        setMergeTarget((t) => t === name ? (next.size > 0 ? Array.from(next)[next.size - 1] : null) : t);
      } else {
        next.add(name);
        // Last clicked becomes the merge target (the person that is kept)
        setMergeTarget(name);
      }
      return next;
    });
  };

  const handleMerge = async () => {
    const names = Array.from(mergeSelection);
    if (names.length < 2 || !mergeTarget) return;
    // mergeTarget (last-clicked) is kept; all others are merged into it
    const target = mergeTarget;
    setMerging(true);
    setMergeMsg(null);
    try {
      let totalMoved = 0;
      for (const name of names) {
        if (name === target) continue;
        const moved = await invoke<number>("merge_persons", { target, source: name });
        totalMoved += moved;
      }
      const msg = t("people.mergeSuccess").replace("{0}", String(totalMoved));
      setMergeMsg(msg);
      // Auto-dismiss after 3 seconds
      if (mergeMsgTimerRef.current) clearTimeout(mergeMsgTimerRef.current);
      mergeMsgTimerRef.current = setTimeout(() => setMergeMsg(null), 3000);
      const list = await refreshPersons();
      setSelected(target);
      setMergeMode(false);
      setMergeSelection(new Set());
      setMergeTarget(null);
      if (list.includes(target)) loadMemories(target);
    } catch (e) {
      setError(String(e));
    } finally {
      setMerging(false);
    }
  };

  // Cleanup auto-dismiss timer on unmount
  useEffect(() => {
    return () => { if (mergeMsgTimerRef.current) clearTimeout(mergeMsgTimerRef.current); };
  }, []);

  const exitMergeMode = () => {
    setMergeMode(false);
    setMergeSelection(new Set());
    setMergeTarget(null);
    setMergeMsg(null);
  };

  const categoryLabel: Record<string, string> = {
    behavior: "Behavior", personality: "Personality", values: "Values",
    thinking: "Thinking", emotion: "Emotion", identity: "Identity", growth: "Growth",
    role: "Role",
  };

  if (loading) return <div style={{ padding: 24, color: "var(--text-tertiary)" }}>{t("loading")}</div>;

  if (error) {
    return (
      <div style={{ padding: 24 }}>
        <div className="page-header"><h1>{t("people.title")}</h1></div>
        <div className="card" style={{ padding: 24, color: "var(--warning-text)" }}>{error}</div>
      </div>
    );
  }

  if (persons.length === 0) {
    return (
      <div style={{ padding: 24 }}>
        <div className="page-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h1>{t("people.title")}</h1>
          <button onClick={handleExtract} disabled={extracting} style={{
            padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
            borderRadius: "var(--radius-md)", background: "var(--surface)",
            color: "var(--text-secondary)", cursor: extracting ? "not-allowed" : "pointer",
            opacity: extracting ? 0.6 : 1,
          }}>{extracting ? t("people.extracting") : t("people.extractNow")}</button>
        </div>
        <div className="card" style={{ padding: 32, textAlign: "center", color: "var(--text-secondary)" }}>
          <p style={{ fontSize: 14 }}>{t("people.noData")}</p>
          <p style={{ fontSize: 12, marginTop: 8, color: "var(--text-tertiary)" }}>
            {t("people.noDataHint")}
          </p>
        </div>
      </div>
    );
  }

  const grouped = memories.reduce<Record<string, PersonMemory[]>>((acc, m) => {
    (acc[m.category] ??= []).push(m);
    return acc;
  }, {});

  const selectedNames = Array.from(mergeSelection);

  return (
    <div style={{ padding: 24 }}>
      <div className="page-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1>{t("people.title")}</h1>
        <div style={{ display: "flex", gap: 8 }}>
          {mergeMode ? (
            <>
              <button onClick={exitMergeMode} style={{
                padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
                borderRadius: "var(--radius-md)", background: "var(--surface)",
                color: "var(--text-secondary)", cursor: "pointer",
              }}>{t("cancel")}</button>
              <button onClick={handleMerge} disabled={mergeSelection.size < 2 || merging} style={{
                padding: "6px 14px", fontSize: 12, border: "none",
                borderRadius: "var(--radius-md)",
                background: mergeSelection.size >= 2 ? "var(--accent)" : "var(--surface)",
                color: mergeSelection.size >= 2 ? "#fff" : "var(--text-tertiary)",
                cursor: mergeSelection.size >= 2 && !merging ? "pointer" : "not-allowed",
                opacity: merging ? 0.6 : 1,
              }}>
                {mergeSelection.size >= 2
                  ? `${t("people.mergeConfirm")} "${mergeTarget ?? selectedNames[0]}"`
                  : t("people.merge")}
              </button>
            </>
          ) : (
            <>
              <button onClick={() => setMergeMode(true)} style={{
                padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
                borderRadius: "var(--radius-md)", background: "var(--surface)",
                color: "var(--text-secondary)", cursor: "pointer",
              }}>{t("people.merge")}</button>
              <button onClick={handleExtract} disabled={extracting} style={{
                padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
                borderRadius: "var(--radius-md)", background: "var(--surface)",
                color: "var(--text-secondary)", cursor: extracting ? "not-allowed" : "pointer",
                opacity: extracting ? 0.6 : 1,
              }}>{extracting ? t("people.extracting") : t("people.extractNow")}</button>
            </>
          )}
        </div>
      </div>

      {mergeMode && (
        <div style={{
          marginBottom: 12, padding: "8px 14px", fontSize: 12,
          borderRadius: "var(--radius-md)", background: "var(--accent-light)",
          color: "var(--accent-text)",
        }}>
          {t("people.mergeTip")}
          {selectedNames.length >= 2 && mergeTarget && (
            <span style={{ marginLeft: 8, fontWeight: 600 }}>
              {selectedNames.filter(n => n !== mergeTarget).join(", ")} → {mergeTarget}
            </span>
          )}
        </div>
      )}

      {mergeMsg && (
        <div style={{
          marginBottom: 12, padding: "8px 14px", fontSize: 12,
          borderRadius: "var(--radius-md)", background: "var(--success-bg, #e8f5e9)",
          color: "var(--success-text, #2e7d32)",
        }}>{mergeMsg}</div>
      )}

      <div style={{ display: "flex", gap: 16 }}>
        <div style={{ width: 180, flexShrink: 0 }}>
          <div className="card" style={{ padding: 8 }}>
            {persons.map((name) => {
              const isChecked = mergeSelection.has(name);
              const isActive = !mergeMode && selected === name;
              return (
                <button key={name} onClick={() => mergeMode ? toggleMergeSelect(name) : setSelected(name)} style={{
                  display: "flex", alignItems: "center", gap: 8,
                  width: "100%", padding: "8px 12px", border: "none",
                  borderRadius: "var(--radius-md)",
                  background: isActive ? "var(--accent-light)" : isChecked ? "var(--accent-light)" : "transparent",
                  color: isActive ? "var(--accent-text)" : isChecked ? "var(--accent-text)" : "var(--text)",
                  fontWeight: isActive || isChecked ? 600 : 400, fontSize: 13, textAlign: "left",
                  cursor: "pointer", transition: "all 0.15s ease",
                }}>
                  {mergeMode && (
                    <span style={{
                      width: 16, height: 16, borderRadius: 4, flexShrink: 0,
                      border: isChecked ? "none" : "1.5px solid var(--border)",
                      background: isChecked ? "var(--accent)" : "transparent",
                      display: "flex", alignItems: "center", justifyContent: "center",
                      color: "#fff", fontSize: 11, lineHeight: 1,
                    }}>{isChecked ? "✓" : ""}</span>
                  )}
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
                </button>
              );
            })}
          </div>
          <div style={{ marginTop: 8, fontSize: 11, color: "var(--text-tertiary)", textAlign: "center" }}>
            {persons.length} {t("people.people")}
          </div>
        </div>

        <div style={{ flex: 1, minWidth: 0 }}>
          {!mergeMode && selected && (
            <>
              <div style={{ marginBottom: 12 }}>
                <span style={{ fontSize: 15, fontWeight: 600 }}>{selected}</span>
                <span style={{ fontSize: 12, color: "var(--text-tertiary)", marginLeft: 8 }}>
                  {memories.length} {t("people.memories")}
                </span>
              </div>
              {Object.keys(grouped).length === 0 ? (
                <div className="card" style={{ padding: 24, textAlign: "center", color: "var(--text-tertiary)", fontSize: 13 }}>
                  {t("people.noMemories")}
                </div>
              ) : (
                Object.entries(grouped).map(([cat, mems]) => (
                  <div key={cat} style={{ marginBottom: 16 }}>
                    <div style={{ fontSize: 11, fontWeight: 600, color: "var(--text-tertiary)", textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: 6 }}>
                      {categoryLabel[cat] ?? cat}
                    </div>
                    <div className="card" style={{ padding: 0 }}>
                      {mems.map((m, i) => (
                        <div key={m.id} style={{
                          padding: "10px 14px",
                          borderBottom: i < mems.length - 1 ? "1px solid var(--border-subtle)" : "none",
                          display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12,
                        }}>
                          <span style={{ fontSize: 13, color: "var(--text)" }}>{m.content}</span>
                          <span style={{ fontSize: 11, color: "var(--text-tertiary)", flexShrink: 0 }}>
                            {Math.round(m.confidence * 100)}%
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                ))
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

export default People;
