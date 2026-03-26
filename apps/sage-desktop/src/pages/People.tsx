import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface PersonMemory {
  id: number;
  category: string;
  content: string;
  source: string;
  confidence: number;
  created_at: string;
  updated_at: string;
  depth?: string;
  about_person?: string;
}

function People() {
  const [persons, setPersons] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [memories, setMemories] = useState<PersonMemory[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [extracting, setExtracting] = useState(false);

  useEffect(() => {
    invoke<string[]>("get_known_persons")
      .then((list) => {
        setPersons(list);
        if (list.length > 0) setSelected((prev) => prev ?? list[0]);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

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
    if (selected) loadMemories(selected);
  }, [selected, loadMemories]);

  const handleExtract = async () => {
    setExtracting(true);
    try {
      await invoke("trigger_person_extract");
      setTimeout(async () => {
        const list = await invoke<string[]>("get_known_persons");
        setPersons(list);
        if (selected) loadMemories(selected);
        setExtracting(false);
      }, 3000);
    } catch (e) {
      setError(String(e));
      setExtracting(false);
    }
  };

  const categoryLabel: Record<string, string> = {
    behavior: "Behavior", personality: "Personality", values: "Values",
    thinking: "Thinking", emotion: "Emotion", identity: "Identity", growth: "Growth",
  };

  if (loading) return <div style={{ padding: 24, color: "var(--text-tertiary)" }}>Loading...</div>;

  if (error) {
    return (
      <div style={{ padding: 24 }}>
        <div className="page-header"><h1>People</h1></div>
        <div className="card" style={{ padding: 24, color: "var(--warning-text)" }}>{error}</div>
      </div>
    );
  }

  if (persons.length === 0) {
    return (
      <div style={{ padding: 24 }}>
        <div className="page-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h1>People</h1>
          <button onClick={handleExtract} disabled={extracting} style={{
            padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
            borderRadius: "var(--radius-md)", background: "var(--surface)",
            color: "var(--text-secondary)", cursor: extracting ? "not-allowed" : "pointer",
            opacity: extracting ? 0.6 : 1,
          }}>{extracting ? "Extracting..." : "Extract Now"}</button>
        </div>
        <div className="card" style={{ padding: 32, textAlign: "center", color: "var(--text-secondary)" }}>
          <p style={{ fontSize: 14 }}>No people observed yet.</p>
          <p style={{ fontSize: 12, marginTop: 8, color: "var(--text-tertiary)" }}>
            Sage learns about people from your emails, chats, and conversations during evening review.
          </p>
        </div>
      </div>
    );
  }

  const grouped = memories.reduce<Record<string, PersonMemory[]>>((acc, m) => {
    (acc[m.category] ??= []).push(m);
    return acc;
  }, {});

  return (
    <div style={{ padding: 24 }}>
      <div className="page-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1>People</h1>
        <button onClick={handleExtract} disabled={extracting} style={{
          padding: "6px 14px", fontSize: 12, border: "1px solid var(--border)",
          borderRadius: "var(--radius-md)", background: "var(--surface)",
          color: "var(--text-secondary)", cursor: extracting ? "not-allowed" : "pointer",
          opacity: extracting ? 0.6 : 1,
        }}>{extracting ? "Extracting..." : "Extract Now"}</button>
      </div>

      <div style={{ display: "flex", gap: 16 }}>
        <div style={{ width: 180, flexShrink: 0 }}>
          <div className="card" style={{ padding: 8 }}>
            {persons.map((name) => (
              <button key={name} onClick={() => setSelected(name)} style={{
                display: "block", width: "100%", padding: "8px 12px", border: "none",
                borderRadius: "var(--radius-md)",
                background: selected === name ? "var(--accent-light)" : "transparent",
                color: selected === name ? "var(--accent-text)" : "var(--text)",
                fontWeight: selected === name ? 600 : 400, fontSize: 13, textAlign: "left",
                cursor: "pointer", transition: "all 0.15s ease",
              }}>{name}</button>
            ))}
          </div>
          <div style={{ marginTop: 8, fontSize: 11, color: "var(--text-tertiary)", textAlign: "center" }}>
            {persons.length} people
          </div>
        </div>

        <div style={{ flex: 1, minWidth: 0 }}>
          {selected && (
            <>
              <div style={{ marginBottom: 12 }}>
                <span style={{ fontSize: 15, fontWeight: 600 }}>{selected}</span>
                <span style={{ fontSize: 12, color: "var(--text-tertiary)", marginLeft: 8 }}>
                  {memories.length} memories
                </span>
              </div>
              {Object.keys(grouped).length === 0 ? (
                <div className="card" style={{ padding: 24, textAlign: "center", color: "var(--text-tertiary)", fontSize: 13 }}>
                  No memories about {selected} yet.
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
