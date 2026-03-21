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

// ─── Inline styles ──────────────────────────────────────
const S = {
  input: {
    padding: "6px 10px", borderRadius: 6, border: "1px solid var(--border)",
    background: "var(--surface)", color: "var(--text)", fontSize: 12,
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
    background: "var(--surface)", marginBottom: 8,
  } as React.CSSProperties,
  section: { marginBottom: 12 } as React.CSSProperties,
  btn: (primary = false) => ({
    padding: "6px 14px", borderRadius: 6, border: "1px solid var(--border)",
    background: primary ? "var(--accent)" : "var(--surface)",
    color: primary ? "#fff" : "var(--text)",
    fontSize: 12, cursor: "pointer", whiteSpace: "nowrap",
  }) as React.CSSProperties,
  ghostBtn: {
    padding: "4px 10px", borderRadius: 5, border: "none",
    background: "transparent", color: "var(--text-secondary)",
    fontSize: 11, cursor: "pointer",
  } as React.CSSProperties,
};

// ─── Toggle Switch ──────────────────────────────────────
function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button onClick={() => onChange(!on)} style={{
      width: 36, height: 20, borderRadius: 10, border: "none", cursor: "pointer",
      background: on ? "var(--accent)" : "var(--border)",
      position: "relative", transition: "background 0.2s", flexShrink: 0,
    }}>
      <span style={{
        width: 16, height: 16, borderRadius: 8, background: "#fff",
        position: "absolute", top: 2, left: on ? 18 : 2,
        transition: "left 0.2s", boxShadow: "0 1px 2px rgba(0,0,0,0.2)",
      }} />
    </button>
  );
}

// ─── Tag Input ──────────────────────────────────────────
function TagInput({ values, onChange, placeholder }: {
  values: string[]; onChange: (v: string[]) => void; placeholder: string;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const v = draft.trim();
    if (v && !values.includes(v)) onChange([...values, v]);
    setDraft("");
  };
  return (
    <div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: values.length ? 6 : 0 }}>
        {values.map((v) => (
          <span key={v} style={{
            display: "inline-flex", alignItems: "center", gap: 4,
            padding: "2px 8px", borderRadius: 10, fontSize: 11,
            background: "var(--accent-light)", color: "var(--accent)",
            border: "1px solid var(--border)",
          }}>
            {v}
            <span style={{ cursor: "pointer", opacity: 0.6 }} onClick={() => onChange(values.filter(x => x !== v))}>x</span>
          </span>
        ))}
      </div>
      <div style={{ display: "flex", gap: 4 }}>
        <input style={S.input} value={draft} onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); add(); } }}
          placeholder={placeholder} />
        <button style={S.btn()} onClick={add}>+</button>
      </div>
    </div>
  );
}

// ─── Score Badge ────────────────────────────────────────
function ScoreBadge({ score, size = 24 }: { score: number; size?: number }) {
  const filled = score >= 4;
  const gold = score >= 5;
  const color = gold ? "#f59e0b" : filled ? "var(--accent)" : score >= 3 ? "var(--text-secondary)" : "var(--border)";
  return (
    <div style={{
      width: size, height: size, borderRadius: "50%",
      display: "flex", alignItems: "center", justifyContent: "center",
      fontSize: size * 0.46, fontWeight: 700,
      background: filled ? color : "transparent",
      color: filled ? "#fff" : color,
      border: filled ? "none" : `1.5px solid ${color}`,
      flexShrink: 0,
    }}>
      {score}
    </div>
  );
}

// ─── Hostname helper ────────────────────────────────────
function hostname(url: string): string {
  try { return new URL(url).hostname.replace("www.", ""); } catch { return ""; }
}

// ─── Parse digest markdown into sections ────────────────
function parseDigest(text: string): { headlines: string[]; patterns: string[]; ideas: string[] } {
  const sections = { headlines: [] as string[], patterns: [] as string[], ideas: [] as string[] };
  let current: "headlines" | "patterns" | "ideas" | null = null;
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (/^##?\s*(要闻|Headlines)/i.test(trimmed)) { current = "headlines"; continue; }
    if (/^##?\s*(趋势|Patterns)/i.test(trimmed)) { current = "patterns"; continue; }
    if (/^##?\s*(灵感|Ideas)/i.test(trimmed)) { current = "ideas"; continue; }
    if (current && trimmed && /^[-*•\d]/.test(trimmed)) {
      sections[current].push(trimmed.replace(/^[-*•]\s*/, "").replace(/^\d+\.\s*/, ""));
    }
  }
  return sections;
}

// ─── Main Component ─────────────────────────────────────
function FeedIntelligence() {
  const { t } = useLang();
  const [items, setItems] = useState<FeedItem[]>([]);
  const [config, setConfig] = useState<FeedConfig | null>(null);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"time" | "score">("time");
  const [polling, setPolling] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  // Digest
  const [digest, setDigest] = useState<string | null>(null);
  const [digestLoading, setDigestLoading] = useState(false);
  const [briefingOpen, setBriefingOpen] = useState(true);
  // Expand
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set());

  const loadItems = useCallback(() => {
    invoke<FeedItem[]>("get_feed_items", { limit: 100 }).then(setItems).catch(console.error);
  }, []);
  const loadConfig = useCallback(() => {
    invoke<FeedConfig>("get_feed_config").then((c) => { setConfig(c); setDirty(false); }).catch(console.error);
  }, []);
  const fetchDigest = useCallback(async () => {
    setDigestLoading(true);
    try {
      const d = await invoke<string>("get_feed_digest");
      setDigest(d);
    } catch { setDigest(null); }
    setDigestLoading(false);
  }, []);

  useEffect(() => { loadItems(); loadConfig(); }, [loadItems, loadConfig]);

  const update = <K extends keyof FeedConfig>(key: K, val: FeedConfig[K]) => {
    if (!config) return;
    setConfig({ ...config, [key]: val });
    setDirty(true);
  };
  const updateSource = <K extends keyof FeedConfig>(key: K, field: string, val: unknown) => {
    if (!config) return;
    setConfig({ ...config, [key]: { ...config[key] as object, [field]: val } });
    setDirty(true);
  };
  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try { await invoke("save_feed_config", { feedConfig: config }); setDirty(false); } catch {}
    setSaving(false);
  };
  const handlePoll = async () => {
    setPolling(true);
    try {
      await invoke<string>("trigger_feed_poll");
      setTimeout(() => { loadItems(); setPolling(false); }, 5000);
    } catch { setPolling(false); }
  };
  const toggleExpand = (id: number) => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  const filtered = useMemo(() => {
    if (!search.trim()) return items;
    const q = search.toLowerCase();
    return items.filter(it =>
      it.title.toLowerCase().includes(q) || it.insight.toLowerCase().includes(q) ||
      (it.summary || "").toLowerCase().includes(q) || (it.idea || "").toLowerCase().includes(q)
    );
  }, [items, search]);

  const sorted = useMemo(() => {
    if (sortBy === "score") return [...filtered].sort((a, b) => b.score - a.score);
    return filtered;
  }, [filtered, sortBy]);

  const featured = useMemo(() => sorted.filter(it => it.score >= 4).slice(0, 8), [sorted]);
  const featuredIds = useMemo(() => new Set(featured.map(it => it.id)), [featured]);

  const grouped = useMemo(() => {
    const groups: { label: string; items: FeedItem[] }[] = [];
    const list = sortBy === "time" ? sorted.filter(it => !featuredIds.has(it.id)) : sorted;
    for (const it of list) {
      const label = sortBy === "score" ? t("feed.byScore") : formatDate(it.created_at);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.items.push(it);
      else groups.push({ label, items: [it] });
    }
    return groups;
  }, [sorted, sortBy, featuredIds, t]);

  const enabledCount = config
    ? [config.reddit.enabled, config.hackernews.enabled, config.github.enabled, config.arxiv.enabled, config.rss.enabled].filter(Boolean).length
    : 0;

  const digestParsed = useMemo(() => digest ? parseDigest(digest) : null, [digest]);

  return (
    <div style={{ padding: "0 24px 32px" }}>
      {/* ── Toolbar ── */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
        <input type="text" placeholder={t("feed.searchPlaceholder")}
          value={search} onChange={(e) => setSearch(e.target.value)}
          style={{ ...S.input, flex: 1, padding: "8px 12px", fontSize: 13 }} />
        <button onClick={() => setSortBy(s => s === "time" ? "score" : "time")}
          style={{ ...S.btn(), fontSize: 11, padding: "6px 10px" }}>
          {sortBy === "time" ? t("feed.byScore") : t("feed.byTime")}
        </button>
        <button onClick={handlePoll} disabled={polling || enabledCount === 0}
          style={{ ...S.btn(enabledCount > 0 && !polling), opacity: enabledCount === 0 ? 0.5 : 1 }}>
          {polling ? t("feed.fetching") : t("feed.fetchNow")}
        </button>
        <button onClick={() => setShowConfig(!showConfig)}
          style={{ ...S.btn(), background: showConfig ? "var(--accent)" : "var(--surface)", color: showConfig ? "#fff" : "var(--text)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
        </button>
      </div>

      {/* ── Config Panel ── */}
      {showConfig && config && (
        <div style={{ marginBottom: 16, padding: 16, borderRadius: 10, border: "1px solid var(--border)", background: "var(--surface)" }}>
          <div style={S.section}>
            <label style={S.label}>{t("feed.userInterestsLabel")}</label>
            <input style={S.input} value={config.user_interests}
              onChange={(e) => update("user_interests", e.target.value)}
              placeholder={t("feed.userInterestsPlaceholder")} />
          </div>
          {/* Reddit */}
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>Reddit</span>
              <Toggle on={config.reddit.enabled} onChange={(v) => updateSource("reddit", "enabled", v)} /></div>
            {config.reddit.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.redditSubredditsLabel")}</label>
              <TagInput values={config.reddit.subreddits} onChange={(v) => updateSource("reddit", "subreddits", v)} placeholder="e.g. rust" /></div>)}
          </div>
          {/* HackerNews */}
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>Hacker News</span>
              <Toggle on={config.hackernews.enabled} onChange={(v) => updateSource("hackernews", "enabled", v)} /></div>
            {config.hackernews.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.hnMinScoreLabel")}</label>
              <input style={{ ...S.input, width: 80 }} type="number" value={config.hackernews.min_score}
                onChange={(e) => updateSource("hackernews", "min_score", parseInt(e.target.value) || 50)} /></div>)}
          </div>
          {/* GitHub */}
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>GitHub Trending</span>
              <Toggle on={config.github.enabled} onChange={(v) => updateSource("github", "enabled", v)} /></div>
            {config.github.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.githubLangLabel")}</label>
              <input style={{ ...S.input, width: 120 }} value={config.github.trending_language}
                onChange={(e) => updateSource("github", "trending_language", e.target.value)} placeholder="e.g. rust" /></div>)}
          </div>
          {/* arXiv */}
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>arXiv</span>
              <Toggle on={config.arxiv.enabled} onChange={(v) => updateSource("arxiv", "enabled", v)} /></div>
            {config.arxiv.enabled && (<div style={{ marginTop: 8 }}>
              <label style={S.label}>{t("feed.arxivCategoriesLabel")}</label>
              <TagInput values={config.arxiv.categories} onChange={(v) => updateSource("arxiv", "categories", v)} placeholder="e.g. cs.AI" />
              <div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.arxivKeywordsLabel")}</label>
                <TagInput values={config.arxiv.keywords} onChange={(v) => updateSource("arxiv", "keywords", v)} placeholder="e.g. transformer" /></div>
            </div>)}
          </div>
          {/* RSS */}
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>RSS / Atom</span>
              <Toggle on={config.rss.enabled} onChange={(v) => updateSource("rss", "enabled", v)} /></div>
            {config.rss.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.rssFeedsLabel")}</label>
              <TagInput values={config.rss.feeds} onChange={(v) => updateSource("rss", "feeds", v)} placeholder="https://blog.rust-lang.org/feed.xml" /></div>)}
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 12 }}>
            {dirty && <span style={{ fontSize: 11, color: "var(--accent)", alignSelf: "center" }}>{t("feed.unsavedChanges")}</span>}
            <button onClick={handleSave} disabled={!dirty || saving} style={{ ...S.btn(dirty), opacity: dirty ? 1 : 0.5 }}>
              {saving ? t("feed.saving") : t("feed.saveApply")}</button>
          </div>
          <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 8 }}>{t("feed.configFootnote")}</div>
        </div>
      )}

      {/* ── Empty State ── */}
      {items.length === 0 && !polling && (
        <div style={{ textAlign: "center", padding: "48px 24px", color: "var(--text-secondary)", fontSize: 13 }}>
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.3, marginBottom: 12 }}>
            <path d="M4 11a9 9 0 019 9" /><path d="M4 4a16 16 0 0116 16" /><circle cx="5" cy="19" r="1" />
          </svg>
          <p>{t("feed.noItems")}</p>
          <p style={{ fontSize: 12 }}>{enabledCount === 0 ? t("feed.configureHint") : t("feed.fetchHint")}</p>
        </div>
      )}

      {items.length > 0 && (
        <>
          {/* ══ Section 1: Daily Briefing ══ */}
          <div style={{
            marginBottom: 20, borderRadius: 10,
            border: "1px solid var(--border)", background: "var(--surface)",
            overflow: "hidden",
          }}>
            <div style={{
              display: "flex", alignItems: "center", justifyContent: "space-between",
              padding: "10px 16px", borderBottom: briefingOpen ? "1px solid var(--border)" : "none",
              cursor: "pointer",
            }} onClick={() => setBriefingOpen(!briefingOpen)}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ fontSize: 14 }}>📋</span>
                <span style={{ fontSize: 11, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.8, color: "var(--text-secondary)" }}>
                  {t("feed.dailyBriefing")}
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <button onClick={(e) => { e.stopPropagation(); fetchDigest(); }}
                  disabled={digestLoading}
                  style={{ ...S.ghostBtn, color: digestLoading ? "var(--text-tertiary)" : "var(--accent)" }}>
                  {digestLoading ? t("feed.digestLoading") : t("feed.regenerate")}
                </button>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"
                  style={{ transition: "transform 0.2s", transform: briefingOpen ? "rotate(180deg)" : "rotate(0)" }}>
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </div>
            </div>

            {briefingOpen && (
              <div style={{ padding: "12px 16px 16px" }}>
                {!digest && !digestLoading && (
                  <div style={{ fontSize: 12, color: "var(--text-tertiary)", textAlign: "center", padding: 16 }}>
                    {t("feed.digestEmpty")}
                  </div>
                )}
                {digestLoading && (
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16 }}>
                    {[0, 1, 2].map(i => (
                      <div key={i} style={{ height: 80, borderRadius: 6, background: "var(--surface-hover)", animation: "pulse 1.5s infinite" }} />
                    ))}
                  </div>
                )}
                {digestParsed && (
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16 }}>
                    {/* Headlines */}
                    <div>
                      <div style={{ fontSize: 10, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.6, color: "var(--text-tertiary)", marginBottom: 8 }}>
                        {t("feed.headlines")}
                      </div>
                      <ol style={{ margin: 0, paddingLeft: 16, fontSize: 12, lineHeight: 1.7, color: "var(--text)" }}>
                        {digestParsed.headlines.map((h, i) => <li key={i} style={{ marginBottom: 4 }}>{h}</li>)}
                      </ol>
                      {digestParsed.headlines.length === 0 && <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>—</div>}
                    </div>
                    {/* Patterns */}
                    <div>
                      <div style={{ fontSize: 10, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.6, color: "var(--text-tertiary)", marginBottom: 8 }}>
                        {t("feed.patterns")}
                      </div>
                      <ul style={{ margin: 0, paddingLeft: 0, listStyle: "none", fontSize: 12, lineHeight: 1.7, color: "var(--text)" }}>
                        {digestParsed.patterns.map((p, i) => (
                          <li key={i} style={{ marginBottom: 4 }}>
                            <span style={{ color: "var(--accent)", marginRight: 6 }}>→</span>{p}
                          </li>
                        ))}
                      </ul>
                      {digestParsed.patterns.length === 0 && <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>—</div>}
                    </div>
                    {/* Ideas */}
                    <div>
                      <div style={{ fontSize: 10, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.6, color: "var(--text-tertiary)", marginBottom: 8 }}>
                        {t("feed.ideasForYou")}
                      </div>
                      <ul style={{ margin: 0, paddingLeft: 0, listStyle: "none", fontSize: 12, lineHeight: 1.7, color: "var(--text)" }}>
                        {digestParsed.ideas.map((idea, i) => (
                          <li key={i} style={{
                            marginBottom: 6, paddingLeft: 10,
                            borderLeft: "3px solid var(--accent)",
                            background: "var(--accent-light)", borderRadius: "0 4px 4px 0",
                            padding: "4px 8px 4px 10px",
                          }}>{idea}</li>
                        ))}
                      </ul>
                      {digestParsed.ideas.length === 0 && <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>—</div>}
                    </div>
                  </div>
                )}
                {/* Raw digest fallback when parsing finds nothing */}
                {digest && !digestParsed?.headlines.length && !digestParsed?.patterns.length && !digestParsed?.ideas.length && (
                  <div style={{ fontSize: 13, lineHeight: 1.7, color: "var(--text)", whiteSpace: "pre-wrap" }}>{digest}</div>
                )}
              </div>
            )}
          </div>

          {/* ══ Section 2: Featured (score >= 4) ══ */}
          {featured.length > 0 && sortBy === "time" && (
            <div style={{ marginBottom: 20 }}>
              <div style={{
                fontSize: 11, fontWeight: 700, color: "var(--text-secondary)",
                textTransform: "uppercase", letterSpacing: 0.6,
                marginBottom: 10, display: "flex", alignItems: "center", gap: 8,
              }}>
                {t("feed.featured")}
                <span style={{ fontSize: 10, color: "var(--text-tertiary)", fontWeight: 400, letterSpacing: 0 }}>
                  {featured.length} {t("feed.items")}
                </span>
              </div>
              <div style={{
                display: "flex", gap: 10, overflowX: "auto",
                paddingBottom: 4, scrollbarWidth: "none",
              }}>
                {featured.map(it => (
                  <div key={it.id} style={{
                    width: 240, flexShrink: 0, padding: 14,
                    borderRadius: 8, border: `1px solid ${it.score >= 5 ? "rgba(245,158,11,0.3)" : "var(--border)"}`,
                    background: "var(--surface)", cursor: "pointer",
                    transition: "all 0.15s",
                    boxShadow: it.score >= 5 ? "0 0 0 1px rgba(245,158,11,0.08)" : "none",
                  }}
                    onClick={() => it.url ? open(it.url).catch(console.error) : toggleExpand(it.id)}
                    onMouseEnter={e => { (e.currentTarget as HTMLDivElement).style.background = "var(--surface-hover)"; (e.currentTarget as HTMLDivElement).style.transform = "translateY(-1px)"; }}
                    onMouseLeave={e => { (e.currentTarget as HTMLDivElement).style.background = "var(--surface)"; (e.currentTarget as HTMLDivElement).style.transform = "none"; }}
                  >
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                      <ScoreBadge score={it.score} size={26} />
                      <div style={{ fontSize: 10, color: "var(--text-tertiary)" }}>
                        {hostname(it.url)} · {formatTime(it.created_at)}
                      </div>
                    </div>
                    <div style={{
                      fontSize: 13, fontWeight: 600, lineHeight: 1.4, color: "var(--text)",
                      display: "-webkit-box", WebkitLineClamp: 3, WebkitBoxOrient: "vertical" as const,
                      overflow: "hidden", marginBottom: 6,
                    }}>{it.title}</div>
                    {it.insight && (
                      <div style={{
                        fontSize: 11, color: "var(--text-secondary)", lineHeight: 1.4,
                        display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical" as const,
                        overflow: "hidden",
                      }}>{it.insight}</div>
                    )}
                    {it.idea && (
                      <div style={{
                        fontSize: 11, marginTop: 8, padding: "4px 8px",
                        borderLeft: "2px solid var(--accent)", background: "var(--accent-light)",
                        borderRadius: "0 4px 4px 0", color: "var(--accent-text, var(--accent))",
                        lineHeight: 1.4,
                      }}>
                        💡 {it.idea}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* ══ Section 3: Chronological Feed ══ */}
          {grouped.map((group) => (
            <div key={group.label} style={{ marginBottom: 20 }}>
              <div style={{
                fontSize: 11, fontWeight: 600, color: "var(--text-secondary)",
                textTransform: "uppercase", letterSpacing: 0.5,
                marginBottom: 8, paddingBottom: 4, borderBottom: "1px solid var(--border)",
                display: "flex", justifyContent: "space-between",
              }}>
                <span>{group.label}</span>
                <span style={{ fontWeight: 400, color: "var(--text-tertiary)" }}>{group.items.length} {t("feed.items")}</span>
              </div>
              {group.items.map((it) => {
                const expanded = expandedIds.has(it.id);
                const hasSummary = !!it.summary;
                const hasIdea = !!it.idea;
                const expandable = hasSummary || hasIdea;
                return (
                  <div key={it.id}>
                    <div style={{
                      display: "flex", alignItems: "stretch",
                      padding: "8px 10px", borderRadius: 6,
                      transition: "background 0.1s", cursor: "default",
                    }}
                      onMouseEnter={e => { (e.currentTarget as HTMLDivElement).style.background = "var(--surface-hover)"; }}
                      onMouseLeave={e => { (e.currentTarget as HTMLDivElement).style.background = "transparent"; }}
                    >
                      {/* Score gutter */}
                      <div style={{ width: 32, display: "flex", flexDirection: "column", alignItems: "center", flexShrink: 0, marginRight: 8 }}>
                        <ScoreBadge score={it.score} />
                        <div style={{ width: 1, flex: 1, background: "var(--border)", marginTop: 4, opacity: 0.5 }} />
                      </div>
                      {/* Content */}
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 8 }}>
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{
                              fontSize: 13, fontWeight: 500, color: "var(--text)",
                              overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
                              cursor: expandable ? "pointer" : "default",
                            }} onClick={() => expandable && toggleExpand(it.id)}>
                              {it.title}
                            </div>
                            <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 2 }}>
                              {it.url && (
                                <span style={{ fontSize: 11, color: "var(--accent)", opacity: 0.8, cursor: "pointer" }}
                                  onClick={(e) => { e.stopPropagation(); open(it.url).catch(console.error); }}>
                                  {hostname(it.url)} ↗
                                </span>
                              )}
                              <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>·</span>
                              <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>{formatTime(it.created_at)}</span>
                            </div>
                            {it.insight && (
                              <div style={{
                                fontSize: 12, color: "var(--text-secondary)", marginTop: 3,
                                overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
                              }}>{it.insight}</div>
                            )}
                          </div>
                          {/* Expand chevron */}
                          {expandable && (
                            <button onClick={() => toggleExpand(it.id)} style={{
                              ...S.ghostBtn, padding: 4, flexShrink: 0,
                              transform: expanded ? "rotate(180deg)" : "rotate(0)",
                              transition: "transform 0.2s",
                            }}>
                              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <polyline points="6 9 12 15 18 9" />
                              </svg>
                            </button>
                          )}
                        </div>
                      </div>
                    </div>
                    {/* Expanded content */}
                    {expanded && (
                      <div style={{ marginLeft: 40, marginBottom: 8 }}>
                        {hasSummary && (
                          <div style={{
                            fontSize: 12, lineHeight: 1.6, padding: "8px 12px", marginTop: 4,
                            background: "var(--surface)", border: "1px solid var(--border)",
                            borderRadius: 6, color: "var(--text)",
                          }}>
                            <div style={{ fontSize: 10, fontWeight: 600, textTransform: "uppercase", letterSpacing: 0.5, color: "var(--text-tertiary)", marginBottom: 4 }}>
                              {t("feed.summary")}
                            </div>
                            {it.summary}
                          </div>
                        )}
                        {hasIdea && (
                          <div style={{
                            fontSize: 12, lineHeight: 1.6, padding: "8px 12px", marginTop: 4,
                            background: "var(--accent-light)",
                            borderLeft: "3px solid var(--accent)",
                            borderRadius: "0 6px 6px 0", color: "var(--text)",
                          }}>
                            <div style={{ fontSize: 10, fontWeight: 600, textTransform: "uppercase", letterSpacing: 0.5, color: "var(--accent)", marginBottom: 4 }}>
                              {t("feed.idea")}
                            </div>
                            {it.idea}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </>
      )}
    </div>
  );
}

export default FeedIntelligence;
