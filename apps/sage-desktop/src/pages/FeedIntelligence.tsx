import { useEffect, useState, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { formatTime } from "../utils/time";
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
  action: string;   // "" | "archived" | "learning" | "learned"
  category: string | null;
}

interface FeedConfig {
  user_interests: string[];
  reddit: { enabled: boolean; subreddits: string[]; poll_interval_secs: number };
  hackernews: { enabled: boolean; min_score: number; poll_interval_secs: number };
  github: { enabled: boolean; trending_languages: string[]; poll_interval_secs: number };
  arxiv: { enabled: boolean; categories: string[]; keywords: string[]; poll_interval_secs: number };
  rss: { enabled: boolean; feeds: string[]; poll_interval_secs: number };
}

// ─── Shared styles ─────────────────────────────────────
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

// ─── Sub-components ────────────────────────────────────

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

function hostname(url: string): string {
  try { return new URL(url).hostname.replace("www.", ""); } catch { return ""; }
}

// ─── Score color helper ────────────────────────────────

function scoreColor(score: number): string {
  if (score >= 5) return "#f59e0b";
  if (score >= 4) return "var(--accent)";
  if (score >= 3) return "var(--text-secondary)";
  return "var(--text-tertiary)";
}


// ─── Main Component ────────────────────────────────────

function FeedIntelligence() {
  const { t } = useLang();
  const [items, setItems] = useState<FeedItem[]>([]);
  const [config, setConfig] = useState<FeedConfig | null>(null);
  const [search, setSearch] = useState("");
  const [polling, setPolling] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [digest, setDigest] = useState<string | null>(null);
  const [digestLoading, setDigestLoading] = useState(false);
  const [briefingOpen, setBriefingOpen] = useState(false);
  const [sortBy, setSortBy] = useState<"score" | "time">("score");
  const [showArchived, setShowArchived] = useState(false);
  const [learningIds, setLearningIds] = useState<Set<number>>(new Set());
  const [nlInput, setNlInput] = useState("");
  const [nlBusy, setNlBusy] = useState(false);
  const [topicMode, setTopicMode] = useState(true);
  const [topicQuery, setTopicQuery] = useState("");
  const [topicSearching, setTopicSearching] = useState(false);
  const [learnedResults, setLearnedResults] = useState<Record<number, string[]>>({});
  const [noteContents, setNoteContents] = useState<Record<number, string>>({});
  const [expandedNotes, setExpandedNotes] = useState<Set<number>>(new Set());

  const loadItems = useCallback(() => {
    invoke<FeedItem[]>("get_feed_items", { limit: 100 }).then(setItems).catch(console.error);
  }, []);
  const loadConfig = useCallback(() => {
    invoke<FeedConfig>("get_feed_config").then((c) => { setConfig(c); setDirty(false); }).catch(console.error);
  }, []);
  const loadDigest = useCallback(() => {
    invoke<string>("get_feed_digest").then((d) => { if (d) setDigest(d); }).catch(console.error);
  }, []);

  useEffect(() => { loadItems(); loadConfig(); loadDigest(); }, [loadItems, loadConfig, loadDigest]);

  // 加载已学习条目的阅读笔记
  useEffect(() => {
    const learnedItems = items.filter(it => it.action === "learned");
    if (!learnedItems.length) return;
    learnedItems.forEach(it => {
      if (noteContents[it.id] !== undefined) return; // 已加载
      invoke<string>("get_feed_note", { observationId: it.id }).then(note => {
        if (note) setNoteContents(prev => ({ ...prev, [it.id]: note }));
      }).catch(() => {});
    });
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps

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
      setTimeout(() => { loadItems(); loadDigest(); setPolling(false); }, 8000);
    } catch { setPolling(false); }
  };
  const handleRegenerate = async () => {
    setDigestLoading(true);
    try {
      const d = await invoke<string>("regenerate_feed_digest");
      setDigest(d);
    } catch {}
    setDigestLoading(false);
  };

  const handleArchive = async (id: number) => {
    await invoke("archive_feed_item", { observationId: id }).catch(console.error);
    setItems(prev => prev.map(it => it.id === id ? { ...it, action: "archived" } : it));
  };
  const handleUnarchive = async (id: number) => {
    await invoke("unarchive_feed_item", { observationId: id }).catch(console.error);
    setItems(prev => prev.map(it => it.id === id ? { ...it, action: "" } : it));
  };
  const handleDeepLearn = async (it: FeedItem) => {
    setLearningIds(prev => new Set(prev).add(it.id));
    try {
      const raw = await invoke<string>("deep_learn_feed_item", {
        observationId: it.id, url: it.url, title: it.title,
      });
      const result = JSON.parse(raw) as { count: number; items: string[]; note?: string };
      setItems(prev => prev.map(x => x.id === it.id ? { ...x, action: "learned" } : x));
      if (result.items?.length) {
        setLearnedResults(prev => ({ ...prev, [it.id]: result.items }));
      }
      if (result.note) {
        setNoteContents(prev => ({ ...prev, [it.id]: result.note! }));
        setExpandedNotes(prev => new Set(prev).add(it.id)); // 新学习的默认展开
      }
    } catch (e) {
      alert(`学习失败: ${e}`);
    }
    setLearningIds(prev => { const n = new Set(prev); n.delete(it.id); return n; });
  };

  const enabledCount = config
    ? [config.reddit.enabled, config.hackernews.enabled, config.github.enabled, config.arxiv.enabled, config.rss.enabled].filter(Boolean).length
    : 0;

  const filtered = useMemo(() => {
    let list = items;
    // 搜索过滤
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(it =>
        it.title.toLowerCase().includes(q) || it.insight.toLowerCase().includes(q) ||
        (it.summary || "").toLowerCase().includes(q)
      );
    }
    // 归档过滤（learned 留在活跃列表，不算归档）
    if (!showArchived) {
      list = list.filter(it => !it.action || it.action === "learning" || it.action === "learned");
    } else {
      list = list.filter(it => it.action === "archived");
    }
    return list;
  }, [items, search, showArchived]);

  // 按分数降序排列
  const sorted = useMemo(() => [...filtered].sort((a, b) =>
    sortBy === "score" ? b.score - a.score : b.created_at.localeCompare(a.created_at)
  ), [filtered, sortBy]);

  const stats = useMemo(() => {
    const active = items.filter(it => !it.action || it.action === "learning" || it.action === "learned");
    const archived = items.filter(it => it.action === "archived");
    const high = active.filter(it => it.score >= 4).length;
    const learned = items.filter(it => it.action === "learned").length;
    const avgScore = active.length > 0 ? (active.reduce((s, it) => s + it.score, 0) / active.length).toFixed(1) : "—";
    return { active: active.length, archived: archived.length, high, learned, avgScore };
  }, [items]);

  // Split items into two columns (interleaved by rank to keep both columns balanced)
  const [leftItems, rightItems] = useMemo(() => {
    const left: FeedItem[] = [];
    const right: FeedItem[] = [];
    sorted.forEach((it, i) => (i % 2 === 0 ? left : right).push(it));
    return [left, right];
  }, [sorted]);

  return (
    <div style={{ padding: "0 24px 32px" }}>

      {/* ── Topic search + NL config ── */}
      <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
        <input
          type="text"
          placeholder={topicMode
            ? "搜索主题，如 energy storage、AI agent、Rust async..."
            : "Tell Sage what to follow, e.g. \"Add energy storage and AIDC topics\""}
          value={topicMode ? topicQuery : nlInput}
          onChange={(e) => topicMode ? setTopicQuery(e.target.value) : setNlInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.nativeEvent.isComposing) {
              if (topicMode && topicQuery.trim()) {
                setTopicSearching(true);
                invoke<number>("search_feed_topic", { query: topicQuery.trim() })
                  .then((n) => { loadItems(); if (n > 0) setTopicQuery(""); })
                  .catch(console.error)
                  .finally(() => setTopicSearching(false));
              } else if (!topicMode && nlInput.trim()) {
                setNlBusy(true);
                invoke<FeedConfig>("update_feed_natural", { text: nlInput })
                  .then((newCfg) => { setConfig(newCfg); setNlInput(""); })
                  .catch(console.error)
                  .finally(() => setNlBusy(false));
              }
            }
          }}
          disabled={topicSearching || nlBusy}
          style={{ ...S.input, flex: 1, padding: "8px 12px", fontSize: 13, opacity: (topicSearching || nlBusy) ? 0.6 : 1 }}
        />
        <button
          onClick={() => setTopicMode(!topicMode)}
          title={topicMode ? "切换到订阅配置" : "切换到主题搜索"}
          style={{
            ...S.btn(topicMode), padding: "6px 10px", fontSize: 12,
          }}
        >
          {topicMode ? "搜索" : "配置"}
        </button>
        {topicSearching && <span style={{ fontSize: 12, color: "var(--accent)", alignSelf: "center", whiteSpace: "nowrap" }}>搜索 + AI 分析中...</span>}
        {nlBusy && <span style={{ fontSize: 12, color: "var(--text-tertiary)", alignSelf: "center" }}>Updating...</span>}
      </div>

      {/* ── Toolbar ── */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
        <input type="text" placeholder={t("feed.searchPlaceholder")}
          value={search} onChange={(e) => setSearch(e.target.value)}
          style={{ ...S.input, flex: 1, padding: "8px 12px", fontSize: 13 }} />
        <button onClick={() => setShowArchived(!showArchived)}
          style={{
            ...S.btn(showArchived), fontSize: 11, padding: "6px 10px",
            opacity: stats.archived > 0 ? 1 : 0.5,
          }}>
          {showArchived ? `返回新闻` : `已归档 (${stats.archived})`}
        </button>
        <span style={{ display: "inline-flex", border: "1px solid var(--border)", borderRadius: 6, overflow: "hidden" }}>
          <button onClick={() => setSortBy("score")}
            style={{ fontSize: 10, padding: "4px 8px", border: "none", cursor: "pointer",
              background: sortBy === "score" ? "var(--accent)" : "transparent",
              color: sortBy === "score" ? "#fff" : "var(--text-secondary)",
            }}>分数</button>
          <button onClick={() => setSortBy("time")}
            style={{ fontSize: 10, padding: "4px 8px", border: "none", cursor: "pointer",
              background: sortBy === "time" ? "var(--accent)" : "transparent",
              color: sortBy === "time" ? "#fff" : "var(--text-secondary)",
            }}>时间</button>
        </span>
        <button onClick={handlePoll} disabled={polling}
          style={{ ...S.btn(!polling), opacity: polling ? 0.5 : 1 }}
          title={enabledCount === 0 ? "请先在设置中启用至少一个数据源" : undefined}>
          {polling ? t("feed.fetching") : t("feed.fetchNow")}
        </button>
        <button onClick={() => setShowConfig(!showConfig)}
          style={{
            ...S.btn(), padding: "6px 8px",
            background: showConfig ? "var(--accent)" : "var(--surface)",
            color: showConfig ? "#fff" : "var(--text)",
          }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
        </button>
      </div>

      {/* ── Config Panel ── */}
      {showConfig && config && (
        <div style={{ marginBottom: 20, padding: 16, borderRadius: 10, border: "1px solid var(--border)", background: "var(--surface)" }}>
          <div style={S.section}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <label style={S.label}>{t("feed.userInterestsLabel")}</label>
              <button onClick={async () => {
                try {
                  const items = await invoke<string[]>("summarize_user_interests");
                  if (items.length) update("user_interests", items);
                } catch (e) { console.error(e); }
              }} style={{ ...S.btn(), fontSize: 10, padding: "2px 8px" }}>从记忆总结</button>
            </div>
            <TagInput values={config.user_interests} onChange={(v) => update("user_interests", v)}
              placeholder={t("feed.userInterestsPlaceholder")} />
          </div>
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>Reddit</span>
              <Toggle on={config.reddit.enabled} onChange={(v) => updateSource("reddit", "enabled", v)} /></div>
            {config.reddit.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.redditSubredditsLabel")}</label>
              <TagInput values={config.reddit.subreddits} onChange={(v) => updateSource("reddit", "subreddits", v)} placeholder="e.g. rust" /></div>)}
          </div>
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>Hacker News</span>
              <Toggle on={config.hackernews.enabled} onChange={(v) => updateSource("hackernews", "enabled", v)} /></div>
            {config.hackernews.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.hnMinScoreLabel")}</label>
              <input style={{ ...S.input, width: 80 }} type="number" value={config.hackernews.min_score}
                onChange={(e) => updateSource("hackernews", "min_score", parseInt(e.target.value) || 50)} /></div>)}
          </div>
          <div style={S.sourceCard}>
            <div style={S.toggle}><span style={{ fontSize: 13, fontWeight: 500 }}>GitHub Trending</span>
              <Toggle on={config.github.enabled} onChange={(v) => updateSource("github", "enabled", v)} /></div>
            {config.github.enabled && (<div style={{ marginTop: 8 }}><label style={S.label}>{t("feed.githubLangLabel")}</label>
              <TagInput values={config.github.trending_languages}
                onChange={(v) => updateSource("github", "trending_languages", v)} placeholder="e.g. Rust" /></div>)}
          </div>
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
        </div>
      )}

      {/* ══ Dashboard Stats Bar ══ */}
      <div style={{
        display: "grid", gridTemplateColumns: "repeat(4, 1fr)",
        gap: 12, marginBottom: 16,
      }}>
        {[
          { label: "未读条目", value: stats.active, icon: "📊", color: "var(--text)" },
          { label: "高分推荐", value: stats.high, icon: "⭐", color: "var(--accent)" },
          { label: "已学习", value: stats.learned, icon: "🧠", color: "#10b981" },
          { label: "平均分", value: stats.avgScore, icon: "📈", color: "var(--text-secondary)" },
        ].map((s) => (
          <div key={s.label} style={{
            padding: "16px 18px", borderRadius: 10,
            border: "1px solid var(--border)", background: "var(--surface)",
            display: "flex", alignItems: "center", gap: 14,
          }}>
            <span style={{ fontSize: 24 }}>{s.icon}</span>
            <div>
              <div style={{ fontSize: 24, fontWeight: 700, color: s.color, lineHeight: 1.2 }}>{s.value}</div>
              <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 2 }}>{s.label}</div>
            </div>
          </div>
        ))}
      </div>

      {/* ══ Daily Briefing (click to expand) ══ */}
      <div style={{
        borderRadius: 10, border: "1px solid var(--border)",
        background: "var(--surface)", overflow: "hidden", marginBottom: 20,
      }}>
        <div style={{
          display: "flex", alignItems: "center", justifyContent: "space-between",
          padding: "10px 16px", cursor: "pointer",
          borderBottom: briefingOpen ? "1px solid var(--border)" : "none",
        }} onClick={() => setBriefingOpen(!briefingOpen)}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 14 }}>📋</span>
            <span style={{ fontSize: 12, fontWeight: 700, color: "var(--text-secondary)" }}>
              {t("feed.dailyBriefing")}
            </span>
            {!briefingOpen && digest && (
              <span style={{ fontSize: 11, color: "var(--text-tertiary)", marginLeft: 4 }}>
                点击展开完整简报
              </span>
            )}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <button onClick={(e) => { e.stopPropagation(); handleRegenerate(); }}
              disabled={digestLoading}
              style={{ ...S.ghostBtn, color: digestLoading ? "var(--text-tertiary)" : "var(--accent)" }}>
              {digestLoading ? t("feed.digestLoading") : t("feed.regenerate")}
            </button>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--text-tertiary)" strokeWidth="2.5"
              style={{ transition: "transform 0.2s", transform: briefingOpen ? "rotate(180deg)" : "rotate(0)" }}>
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </div>
        </div>

        {briefingOpen && (
          <div style={{ padding: "14px 18px 18px" }}>
            {digestLoading && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                {[0, 1, 2].map(i => (
                  <div key={i} style={{ height: 40, borderRadius: 6, background: "var(--surface-hover)", animation: "pulse 1.5s infinite" }} />
                ))}
              </div>
            )}
            {digest ? (
              <div className="feed-digest-markdown" style={{ fontSize: 14, lineHeight: 1.8, color: "var(--text)" }}>
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{digest}</ReactMarkdown>
              </div>
            ) : !digestLoading && (
              <div style={{ fontSize: 12, color: "var(--text-tertiary)", textAlign: "center", padding: 20 }}>
                {t("feed.digestEmpty")}
              </div>
            )}
          </div>
        )}
      </div>

      {/* ══ BOTTOM: News Items — Two Columns ══ */}
      {items.length === 0 && !polling && (
        <div style={{ textAlign: "center", padding: "48px 24px", color: "var(--text-secondary)", fontSize: 13 }}>
          <div style={{ fontSize: 40, marginBottom: 16, opacity: 0.3 }}>📡</div>
          <p>{t("feed.noItems")}</p>
          <p style={{ fontSize: 12 }}>{enabledCount === 0 ? t("feed.configureHint") : t("feed.fetchHint")}</p>
        </div>
      )}

      {sorted.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8, alignItems: "start" }}>
          {[leftItems, rightItems].map((col, ci) => (
            <div key={ci} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {col.map((it) => (
                <div key={it.id} style={{
                  padding: "12px 14px", borderRadius: 8,
                  border: "1px solid var(--border)", background: "var(--surface)",
                  transition: "box-shadow 0.15s",
                }}
                  onMouseEnter={e => { (e.currentTarget as HTMLDivElement).style.boxShadow = "0 2px 8px rgba(0,0,0,0.06)"; }}
                  onMouseLeave={e => { (e.currentTarget as HTMLDivElement).style.boxShadow = "none"; }}
                >
                  {/* Title row */}
                  <div style={{ display: "flex", alignItems: "flex-start", gap: 8, marginBottom: 4 }}>
                    <span style={{
                      fontSize: 11, fontWeight: 700, minWidth: 20, textAlign: "center",
                      color: scoreColor(it.score), flexShrink: 0, marginTop: 2,
                      padding: "1px 3px", borderRadius: 4,
                      background: it.score >= 4 ? `${scoreColor(it.score)}18` : "transparent",
                    }}>
                      {it.score}
                    </span>
                    <div style={{
                      fontSize: 13, fontWeight: 600, lineHeight: 1.4, color: "var(--text)",
                      cursor: it.url ? "pointer" : "default",
                      overflow: "hidden", wordBreak: "break-word",
                    }}
                      onClick={() => it.url && open(it.url).catch(console.error)}
                    >
                      {it.title}
                      {it.url && (
                        <span style={{ fontSize: 10, color: "var(--accent)", opacity: 0.7, marginLeft: 6, fontWeight: 400 }}>
                          {hostname(it.url)} ↗
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Insight */}
                  {it.insight && (
                    <div style={{
                      fontSize: 12, lineHeight: 1.6, color: "var(--text-secondary)",
                      marginBottom: it.summary || it.idea ? 6 : 0,
                      wordBreak: "break-word",
                    }}>
                      {it.insight}
                    </div>
                  )}

                  {/* Summary (markdown) — with overflow fix */}
                  {it.summary && (
                    <div className="feed-summary" style={{
                      fontSize: 12, lineHeight: 1.5, padding: "6px 10px",
                      background: "var(--background, #fafafa)", border: "1px solid var(--border)",
                      borderRadius: 6, color: "var(--text)", marginBottom: it.idea ? 6 : 0,
                      overflow: "hidden", wordBreak: "break-word",
                    }}>
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>{it.summary}</ReactMarkdown>
                    </div>
                  )}

                  {/* Idea */}
                  {it.idea && (
                    <div style={{
                      fontSize: 11, lineHeight: 1.5, padding: "5px 8px",
                      borderLeft: "3px solid var(--accent)", background: "var(--accent-light)",
                      borderRadius: "0 6px 6px 0", color: "var(--text)", wordBreak: "break-word",
                    }}>
                      💡 {it.idea}
                    </div>
                  )}

                  {/* Learned results */}
                  {learnedResults[it.id] && (
                    <div style={{
                      marginTop: 6, padding: "6px 10px",
                      background: "rgba(16, 185, 129, 0.08)",
                      border: "1px solid rgba(16, 185, 129, 0.2)",
                      borderRadius: 6, fontSize: 11, lineHeight: 1.5,
                    }}>
                      <div style={{ fontWeight: 600, color: "#10b981", marginBottom: 4 }}>🧠 学习结果</div>
                      {learnedResults[it.id].map((mem, i) => (
                        <div key={i} style={{
                          color: "var(--text-secondary)", paddingLeft: 8,
                          borderLeft: "2px solid rgba(16, 185, 129, 0.3)",
                          marginBottom: 3,
                        }}>
                          {mem}
                        </div>
                      ))}
                    </div>
                  )}

                  {/* Reading note */}
                  {noteContents[it.id] && (
                    <div style={{ marginTop: 6 }}>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setExpandedNotes(prev => {
                            const n = new Set(prev);
                            n.has(it.id) ? n.delete(it.id) : n.add(it.id);
                            return n;
                          });
                        }}
                        style={{
                          ...S.ghostBtn, fontSize: 10, padding: "2px 6px",
                          color: "#10b981", display: "flex", alignItems: "center", gap: 4,
                        }}
                      >
                        <span style={{ transform: expandedNotes.has(it.id) ? "rotate(90deg)" : "none", transition: "transform 0.15s" }}>▶</span>
                        📖 阅读笔记
                      </button>
                      {expandedNotes.has(it.id) && (
                        <div className="md-content" style={{
                          marginTop: 4, padding: "8px 10px",
                          background: "rgba(16, 185, 129, 0.05)",
                          border: "1px solid rgba(16, 185, 129, 0.15)",
                          borderRadius: 6, fontSize: 12, lineHeight: 1.6,
                          color: "var(--text-secondary)",
                        }}>
                          <ReactMarkdown remarkPlugins={[remarkGfm]}>{noteContents[it.id]}</ReactMarkdown>
                        </div>
                      )}
                    </div>
                  )}

                  {/* Action bar */}
                  <div style={{
                    display: "flex", alignItems: "center", justifyContent: "space-between",
                    marginTop: 6, paddingTop: 4, borderTop: "1px solid var(--border)",
                  }}>
                    <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>
                      {formatTime(it.created_at)}
                      {it.action === "learned" && <span style={{ marginLeft: 6, color: "#10b981" }}>🧠 已学习</span>}
                      {it.action === "archived" && <span style={{ marginLeft: 6 }}>📦 已归档</span>}
                    </span>
                    <div style={{ display: "flex", gap: 4 }}>
                      {/* 深入学习 */}
                      {it.url && it.action !== "learned" && (
                        <button
                          onClick={(e) => { e.stopPropagation(); handleDeepLearn(it); }}
                          disabled={learningIds.has(it.id)}
                          style={{
                            ...S.ghostBtn, fontSize: 10, padding: "2px 6px",
                            color: learningIds.has(it.id) ? "var(--text-tertiary)" : "#10b981",
                          }}
                        >
                          {learningIds.has(it.id) ? "学习中..." : "🔬 深入学习"}
                        </button>
                      )}
                      {/* 归档/取消归档 */}
                      {!showArchived ? (
                        <button
                          onClick={(e) => { e.stopPropagation(); handleArchive(it.id); }}
                          style={{ ...S.ghostBtn, fontSize: 10, padding: "2px 6px" }}
                        >
                          ✓ 已知
                        </button>
                      ) : (
                        <button
                          onClick={(e) => { e.stopPropagation(); handleUnarchive(it.id); }}
                          style={{ ...S.ghostBtn, fontSize: 10, padding: "2px 6px", color: "var(--accent)" }}
                        >
                          ↩ 恢复
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default FeedIntelligence;
