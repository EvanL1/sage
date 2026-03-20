import { useEffect, useState, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { formatDate, formatTime } from "../utils/time";
import { useLang } from "../LangContext";

interface FeedItem {
  id: number;
  title: string;
  url: string;
  score: number;
  insight: string;
  summary: string;
  idea: string;
  created_at: string;
}

interface FeedConfig {
  user_interests: string;
  reddit: { enabled: boolean; subreddits: string[]; poll_interval_secs: number };
  hackernews: { enabled: boolean; min_score: number; poll_interval_secs: number };
  github: { enabled: boolean; trending_language: string; poll_interval_secs: number };
  arxiv: { enabled: boolean; categories: string[]; keywords: string[]; poll_interval_secs: number };
  rss: { enabled: boolean; feeds: string[]; poll_interval_secs: number };
}

// ─── Styles ──────────────────────────────────────────────

const S = {
  input: {
    padding: "6px 10px", borderRadius: 6, border: "1px solid var(--border)",
    background: "var(--bg-primary)", color: "var(--text-primary)", fontSize: 12,
    outline: "none", width: "100%",
  } as React.CSSProperties,
  label: {
    fontSize: 11, color: "var(--text-secondary)", marginBottom: 4, display: "block",
  } as React.CSSProperties,
  toggle: {
    display: "flex", alignItems: "center", justifyContent: "space-between",
    padding: "8px 0", borderBottom: "1px solid var(--border)",
  } as React.CSSProperties,
  sourceCard: {
    padding: 12, borderRadius: 8, border: "1px solid var(--border)",
    background: "var(--bg-primary)", marginBottom: 8,
  } as React.CSSProperties,
  section: { marginBottom: 12 } as React.CSSProperties,
  btn: (primary = false) => ({
    padding: "6px 14px", borderRadius: 6, border: "1px solid var(--border)",
    background: primary ? "var(--accent)" : "var(--bg-secondary)",
    color: primary ? "#fff" : "var(--text-primary)",
    fontSize: 12, cursor: "pointer", whiteSpace: "nowrap",
  }) as React.CSSProperties,
};

// ─── Score Indicator ─────────────────────────────────────

function ScoreIndicator({ score }: { score: number }) {
  const color = score >= 5 ? '#f59e0b' : score >= 4 ? 'var(--accent)' : score >= 3 ? 'var(--text-secondary)' : 'var(--border)';
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', marginRight: 10, flexShrink: 0 }}>
      <div style={{
        width: 22, height: 22, borderRadius: '50%',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 11, fontWeight: 600, color: color,
        border: `1.5px solid ${color}`,
        marginBottom: 4,
      }}>
        {score}
      </div>
      <div style={{
        width: 2, flex: 1, borderRadius: 1,
        background: color, opacity: 0.4,
      }} />
    </div>
  );
}

// ─── Toggle Switch ───────────────────────────────────────

function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!on)}
      style={{
        width: 36, height: 20, borderRadius: 10, border: "none", cursor: "pointer",
        background: on ? "var(--accent)" : "var(--border)",
        position: "relative", transition: "background 0.2s", flexShrink: 0,
      }}
    >
      <span style={{
        width: 16, height: 16, borderRadius: 8, background: "#fff",
        position: "absolute", top: 2, left: on ? 18 : 2,
        transition: "left 0.2s", boxShadow: "0 1px 2px rgba(0,0,0,0.2)",
      }} />
    </button>
  );
}

// ─── Tag Input (for arrays) ──────────────────────────────

function TagInput({ values, onChange, placeholder }: {
  values: string[]; onChange: (v: string[]) => void; placeholder: string;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const v = draft.trim();
    if (v && !values.includes(v)) { onChange([...values, v]); }
    setDraft("");
  };
  return (
    <div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: values.length ? 6 : 0 }}>
        {values.map((v) => (
          <span key={v} style={{
            display: "inline-flex", alignItems: "center", gap: 4,
            padding: "2px 8px", borderRadius: 10, fontSize: 11,
            background: "var(--accent-muted, var(--bg-secondary))",
            color: "var(--accent)", border: "1px solid var(--border)",
          }}>
            {v}
            <span style={{ cursor: "pointer", opacity: 0.6 }} onClick={() => onChange(values.filter(x => x !== v))}>x</span>
          </span>
        ))}
      </div>
      <div style={{ display: "flex", gap: 4 }}>
        <input
          style={S.input}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); add(); } }}
          placeholder={placeholder}
        />
        <button style={S.btn()} onClick={add}>+</button>
      </div>
    </div>
  );
}

// ─── Main Component ──────────────────────────────────────

function FeedIntelligence() {
  const { t } = useLang();
  const [items, setItems] = useState<FeedItem[]>([]);
  const [config, setConfig] = useState<FeedConfig | null>(null);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"time" | "score">("time");
  const [polling, setPolling] = useState(false);
  const [layout, setLayout] = useState<"list" | "grid">("list");
  const [showConfig, setShowConfig] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);

  const loadItems = useCallback(() => {
    invoke<FeedItem[]>("get_feed_items", { limit: 100 }).then(setItems).catch(console.error);
  }, []);

  const loadConfig = useCallback(() => {
    invoke<FeedConfig>("get_feed_config").then((c) => { setConfig(c); setDirty(false); }).catch(console.error);
  }, []);

  useEffect(() => { loadItems(); loadConfig(); }, [loadItems, loadConfig]);

  const update = <K extends keyof FeedConfig>(key: K, val: FeedConfig[K]) => {
    if (!config) return;
    setConfig({ ...config, [key]: val });
    setDirty(true);
  };

  const updateSource = <K extends keyof FeedConfig>(
    key: K, field: string, val: unknown
  ) => {
    if (!config) return;
    setConfig({ ...config, [key]: { ...config[key] as object, [field]: val } });
    setDirty(true);
  };

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await invoke("save_feed_config", { feedConfig: config });
      setDirty(false);
    } catch (e) { console.error(e); }
    setSaving(false);
  };

  const handlePoll = async () => {
    setPolling(true);
    try {
      await invoke<string>("trigger_feed_poll");
      setTimeout(() => { loadItems(); setPolling(false); }, 5000);
    } catch { setPolling(false); }
  };

  const filtered = useMemo(() => {
    if (!search.trim()) return items;
    const q = search.toLowerCase();
    return items.filter(
      (it) =>
        it.title.toLowerCase().includes(q) ||
        it.insight.toLowerCase().includes(q) ||
        it.summary.toLowerCase().includes(q) ||
        it.idea.toLowerCase().includes(q)
    );
  }, [items, search]);

  const sorted = useMemo(() => {
    if (sortBy === "score") return [...filtered].sort((a, b) => b.score - a.score);
    return filtered;
  }, [filtered, sortBy]);

  const byScoreLabel = t("feed.byScore");

  const grouped = useMemo(() => {
    const groups: { label: string; items: FeedItem[] }[] = [];
    for (const it of sorted) {
      const label = sortBy === "score" ? byScoreLabel : formatDate(it.created_at);
      const last = groups[groups.length - 1];
      if (last && last.label === label) { last.items.push(it); }
      else { groups.push({ label, items: [it] }); }
    }
    return groups;
  }, [sorted, sortBy, byScoreLabel]);

  const enabledCount = config
    ? [config.reddit.enabled, config.hackernews.enabled, config.github.enabled, config.arxiv.enabled, config.rss.enabled].filter(Boolean).length
    : 0;

  return (
    <div style={{ padding: "0 24px 24px" }}>
      {/* ── Header ── */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
        <input
          type="text" placeholder={t("feed.searchPlaceholder")}
          value={search} onChange={(e) => setSearch(e.target.value)}
          style={{ ...S.input, flex: 1, padding: "8px 12px", fontSize: 13 }}
        />
        <button onClick={() => setSortBy(s => s === "time" ? "score" : "time")}
          style={{ ...S.btn(), fontSize: 11, padding: "6px 10px" }}>
          {sortBy === "time" ? t("feed.byScore") : t("feed.byTime")}
        </button>
        <button onClick={() => setLayout(l => l === "list" ? "grid" : "list")}
          style={{ ...S.btn(), fontSize: 11, padding: "6px 10px" }}>
          {layout === "list" ? t("feed.grid") : t("feed.list")}
        </button>
        <button onClick={handlePoll} disabled={polling || enabledCount === 0}
          style={{ ...S.btn(enabledCount > 0 && !polling), opacity: enabledCount === 0 ? 0.5 : 1 }}>
          {polling ? t("feed.fetching") : t("feed.fetchNow")}
        </button>
        <button onClick={() => setShowConfig(!showConfig)}
          style={{ ...S.btn(), background: showConfig ? "var(--accent)" : "var(--bg-secondary)", color: showConfig ? "#fff" : "var(--text-primary)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
        </button>
      </div>

      {/* ── Config Panel ── */}
      {showConfig && config && (
        <div style={{
          marginBottom: 16, padding: 16, borderRadius: 10,
          border: "1px solid var(--border)", background: "var(--bg-secondary)",
        }}>
          {/* Interests */}
          <div style={S.section}>
            <label style={S.label}>{t("feed.userInterestsLabel")}</label>
            <input style={S.input} value={config.user_interests}
              onChange={(e) => update("user_interests", e.target.value)}
              placeholder={t("feed.userInterestsPlaceholder")} />
          </div>

          {/* ─ Reddit ─ */}
          <div style={S.sourceCard}>
            <div style={S.toggle}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>Reddit</span>
              <Toggle on={config.reddit.enabled} onChange={(v) => updateSource("reddit", "enabled", v)} />
            </div>
            {config.reddit.enabled && (
              <div style={{ marginTop: 8 }}>
                <label style={S.label}>{t("feed.redditSubredditsLabel")}</label>
                <TagInput values={config.reddit.subreddits}
                  onChange={(v) => updateSource("reddit", "subreddits", v)}
                  placeholder="e.g. rust" />
              </div>
            )}
          </div>

          {/* ─ HackerNews ─ */}
          <div style={S.sourceCard}>
            <div style={S.toggle}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>Hacker News</span>
              <Toggle on={config.hackernews.enabled} onChange={(v) => updateSource("hackernews", "enabled", v)} />
            </div>
            {config.hackernews.enabled && (
              <div style={{ marginTop: 8 }}>
                <label style={S.label}>{t("feed.hnMinScoreLabel")}</label>
                <input style={{ ...S.input, width: 80 }} type="number"
                  value={config.hackernews.min_score}
                  onChange={(e) => updateSource("hackernews", "min_score", parseInt(e.target.value) || 50)} />
              </div>
            )}
          </div>

          {/* ─ GitHub ─ */}
          <div style={S.sourceCard}>
            <div style={S.toggle}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>GitHub Trending</span>
              <Toggle on={config.github.enabled} onChange={(v) => updateSource("github", "enabled", v)} />
            </div>
            {config.github.enabled && (
              <div style={{ marginTop: 8 }}>
                <label style={S.label}>{t("feed.githubLangLabel")}</label>
                <input style={{ ...S.input, width: 120 }}
                  value={config.github.trending_language}
                  onChange={(e) => updateSource("github", "trending_language", e.target.value)}
                  placeholder="e.g. rust" />
              </div>
            )}
          </div>

          {/* ─ arXiv ─ */}
          <div style={S.sourceCard}>
            <div style={S.toggle}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>arXiv</span>
              <Toggle on={config.arxiv.enabled} onChange={(v) => updateSource("arxiv", "enabled", v)} />
            </div>
            {config.arxiv.enabled && (
              <div style={{ marginTop: 8 }}>
                <label style={S.label}>{t("feed.arxivCategoriesLabel")}</label>
                <TagInput values={config.arxiv.categories}
                  onChange={(v) => updateSource("arxiv", "categories", v)}
                  placeholder="e.g. cs.AI" />
                <div style={{ marginTop: 8 }}>
                  <label style={S.label}>{t("feed.arxivKeywordsLabel")}</label>
                  <TagInput values={config.arxiv.keywords}
                    onChange={(v) => updateSource("arxiv", "keywords", v)}
                    placeholder="e.g. transformer" />
                </div>
              </div>
            )}
          </div>

          {/* ─ RSS/Atom ─ */}
          <div style={S.sourceCard}>
            <div style={S.toggle}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>RSS / Atom</span>
              <Toggle on={config.rss.enabled} onChange={(v) => updateSource("rss", "enabled", v)} />
            </div>
            {config.rss.enabled && (
              <div style={{ marginTop: 8 }}>
                <label style={S.label}>{t("feed.rssFeedsLabel")}</label>
                <TagInput values={config.rss.feeds}
                  onChange={(v) => updateSource("rss", "feeds", v)}
                  placeholder="https://blog.rust-lang.org/feed.xml" />
              </div>
            )}
          </div>

          {/* Save button */}
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 12 }}>
            {dirty && <span style={{ fontSize: 11, color: "var(--accent)", alignSelf: "center" }}>{t("feed.unsavedChanges")}</span>}
            <button onClick={handleSave} disabled={!dirty || saving}
              style={{ ...S.btn(dirty), opacity: dirty ? 1 : 0.5 }}>
              {saving ? t("feed.saving") : t("feed.saveApply")}
            </button>
          </div>
          <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 8 }}>
            {t("feed.configFootnote")}
          </div>
        </div>
      )}

      {/* ── Empty state ── */}
      {items.length === 0 && !polling && (
        <div style={{ textAlign: "center", padding: "48px 24px", color: "var(--text-secondary)", fontSize: 13 }}>
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.3, marginBottom: 12 }}>
            <path d="M4 11a9 9 0 019 9" /><path d="M4 4a16 16 0 0116 16" /><circle cx="5" cy="19" r="1" />
          </svg>
          <p>{t("feed.noItems")}</p>
          <p style={{ fontSize: 12 }}>
            {enabledCount === 0
              ? <>{t("feed.configureHint")}</>
              : <>{t("feed.fetchHint")}</>}
          </p>
        </div>
      )}

      {/* ── Feed items ── */}
      {grouped.map((group) => (
        <div key={group.label} style={{ marginBottom: 20 }}>
          <div style={{
            fontSize: 11, fontWeight: 600, color: "var(--text-secondary)",
            textTransform: "uppercase", letterSpacing: 0.5,
            marginBottom: 8, paddingBottom: 4, borderBottom: "1px solid var(--border)",
          }}>
            {group.label}
          </div>
          <div style={layout === "grid" ? { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 } : undefined}>
          {group.items.map((it) => (
            <div key={it.id} className="feed-item-row"
              style={{
                padding: layout === "grid" ? "12px" : "10px 12px",
                marginBottom: layout === "grid" ? 0 : 4, borderRadius: 6,
                cursor: it.url ? "pointer" : "default", transition: "background 0.15s",
                border: layout === "grid" ? "1px solid var(--border)" : "none",
                display: "flex", flexDirection: "column",
              }}
              onClick={() => it.url && open(it.url).catch(console.error)}>
              <div style={{ display: "flex", alignItems: layout === "grid" ? "flex-start" : "stretch" }}>
                <ScoreIndicator score={it.score} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 8 }}>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 4,
                        overflow: "hidden" }}>
                        <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text-primary)",
                          overflow: "hidden", textOverflow: "ellipsis",
                          whiteSpace: layout === "grid" ? "normal" : "nowrap",
                          flex: "1 1 0", minWidth: 0,
                          ...(layout === "grid" ? { display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" as const } : {}),
                        }}>
                          {it.title}
                        </div>
                      </div>
                      {it.url && (
                        <div style={{ fontSize: 11, color: "var(--accent)", opacity: 0.7, marginTop: 2 }}>
                          {(() => { try { return new URL(it.url).hostname.replace('www.', ''); } catch { return ''; } })()}
                          <span style={{ fontSize: 10, marginLeft: 2, opacity: 0.5 }}>&uarr;</span>
                        </div>
                      )}
                      {it.insight && (
                        <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4,
                          overflow: "hidden", textOverflow: "ellipsis",
                          whiteSpace: layout === "grid" ? "normal" : "nowrap",
                          ...(layout === "grid" ? { display: "-webkit-box", WebkitLineClamp: 3, WebkitBoxOrient: "vertical" as const } : {}),
                        }}>
                          {it.insight}
                        </div>
                      )}
                      {it.summary && (
                        <div style={{
                          fontSize: 12, color: "var(--text-primary)", marginTop: 6,
                          padding: "6px 10px", borderRadius: 4,
                          background: "var(--bg-secondary)", lineHeight: 1.5,
                          whiteSpace: "normal",
                        }}>
                          <span style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 0.5, color: "var(--text-secondary)", marginRight: 6 }}>{t("feed.insight")}</span>
                          {it.summary}
                        </div>
                      )}
                      {it.idea && (
                        <div style={{
                          fontSize: 12, color: "var(--text-primary)", marginTop: 6,
                          padding: "8px 10px", borderRadius: 6,
                          background: "color-mix(in srgb, var(--accent) 12%, var(--bg-primary))",
                          borderLeft: "3px solid var(--accent)", lineHeight: 1.5,
                          whiteSpace: "normal",
                        }}>
                          <div style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 0.6, color: "var(--accent)", marginBottom: 4 }}>
                            {t("feed.nextStep")}
                          </div>
                          {it.idea}
                        </div>
                      )}
                    </div>
                    <span style={{ fontSize: 11, color: "var(--text-tertiary, var(--text-secondary))", whiteSpace: "nowrap", flexShrink: 0 }}>
                      {formatTime(it.created_at)}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          ))}
          </div>
        </div>
      ))}
    </div>
  );
}

export default FeedIntelligence;
