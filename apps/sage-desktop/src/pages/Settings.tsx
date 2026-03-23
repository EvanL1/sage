import { useEffect, useState, useCallback } from "react";
import { Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";

interface UserProfile {
  identity: {
    name: string;
    role: string;
    reporting_line: string[];
    primary_language: string;
    secondary_language: string;
    prompt_language: string;
  };
  work_context: {
    projects: { name: string; description: string; status: string }[];
    stakeholders: { name: string; role: string; relationship: string; email_domain?: string }[];
    tech_stack: string[];
  };
  communication: {
    style: string;
    notification_max_chars: number;
    suggestion_format: string;
  };
  schedule: {
    morning_brief_hour: number;
    evening_review_hour: number;
    weekly_report_day: string;
    weekly_report_hour: number;
    work_start_hour: number;
    work_end_hour: number;
  };
  preferences: {
    urgent_keywords: string[];
    important_sender_domains: string[];
  };
  negative_rules: string[];
  sop_version: number;
}

import type { ProviderInfo, ProviderConfig, MessageSource } from "../types";

type TestState = "idle" | "testing" | "ok" | "fail";

const WEEKDAY_LABELS: Record<string, string> = {
  Mon: "Monday", Tue: "Tuesday", Wed: "Wednesday", Thu: "Thursday",
  Fri: "Friday", Sat: "Saturday", Sun: "Sunday",
};

import { PROVIDER_MODELS } from "../providerModels";

const EMPTY_IMAP = {
  id: 0,
  label: "",
  email: "",
  password: "",
  imap_host: "",
  imap_port: 993,
  smtp_host: "",
  smtp_port: 587,
  use_tls: true,
};

type TestResult = { success: boolean; error?: string } | null;

function MessageSourcesSection() {
  const [sources, setSources] = useState<MessageSource[]>([]);
  const [addOpen, setAddOpen] = useState(false);
  const [form, setForm] = useState(EMPTY_IMAP);
  const [testResults, setTestResults] = useState<Record<number, TestResult>>({});
  const [newTestResult, setNewTestResult] = useState<TestResult>(null);
  const [saving, setSaving] = useState(false);
  const [oauthLoading, setOauthLoading] = useState<string | null>(null);

  const loadSources = useCallback(() => {
    invoke<MessageSource[]>("get_message_sources")
      .then(setSources)
      .catch(() => {});
  }, []);

  useEffect(() => { loadSources(); }, [loadSources]);

  const buildConfig = () => JSON.stringify({
    imap_host: form.imap_host, imap_port: form.imap_port,
    smtp_host: form.smtp_host, smtp_port: form.smtp_port,
    username: form.email, // IMAP username = email address
    password_enc: form.password,
    use_tls: form.use_tls, email: form.email,
  });

  const handleEdit = (src: MessageSource) => {
    try {
      const cfg = JSON.parse(src.config);
      setForm({
        id: src.id, label: src.label, email: cfg.email || cfg.username || "",
        password: "", // don't show existing password
        imap_host: cfg.imap_host || "", imap_port: cfg.imap_port || 993,
        smtp_host: cfg.smtp_host || "", smtp_port: cfg.smtp_port || 587,
        use_tls: cfg.use_tls ?? true,
      });
      setAddOpen(true);
      setNewTestResult(null);
    } catch { /* ignore parse errors */ }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      const config = buildConfig();
      await invoke<number>("save_message_source", {
        source: { id: form.id, label: form.label, source_type: "imap", config, enabled: true, created_at: "" },
      });
      setForm(EMPTY_IMAP);
      setAddOpen(false);
      setNewTestResult(null);
      loadSources();
    } catch (err) {
      alert("Failed to save: " + String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleTestNew = async () => {
    const config = buildConfig();
    try {
      const result = await invoke<{ success: boolean; error?: string }>("test_source_connection", { config, sourceType: "imap" });
      setNewTestResult(result);
    } catch (err) {
      setNewTestResult({ success: false, error: String(err) });
    }
  };

  const handleTestExisting = async (source: MessageSource) => {
    try {
      const result = await invoke<{ success: boolean; error?: string }>("test_source_connection", { config: source.config, sourceType: source.source_type });
      setTestResults((prev) => ({ ...prev, [source.id]: result }));
    } catch (err) {
      setTestResults((prev) => ({ ...prev, [source.id]: { success: false, error: String(err) } }));
    }
  };

  const handleDelete = async (id: number) => {
    if (!confirm("Remove this email source? Cached emails will also be deleted.")) return;
    try {
      await invoke("delete_message_source", { id });
      loadSources();
    } catch (err) {
      alert("Failed to delete: " + String(err));
    }
  };

  const f = (key: keyof typeof EMPTY_IMAP, val: string | number | boolean) =>
    setForm((prev) => ({ ...prev, [key]: val }));

  return (
    <div className="settings-section">
      <div className="settings-section-title">Email / Message Sources</div>
      <div className="card">
        {sources.length === 0 && (
          <p style={{ fontSize: 13, color: "var(--text-tertiary)", marginBottom: "var(--spacing-sm)" }}>No sources configured.</p>
        )}
        {sources.map((src) => (
          <div key={src.id} style={{ display: "flex", alignItems: "center", gap: "var(--spacing-sm)", paddingBottom: 8, marginBottom: 8, borderBottom: "1px solid var(--border-subtle)" }}>
            <div style={{ flex: 1 }}>
              <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{src.label}</span>
              <span style={{ marginLeft: 8, fontSize: 10, background: "var(--surface-active)", color: "var(--text-secondary)", padding: "1px 6px", borderRadius: 999, textTransform: "uppercase" }}>{src.source_type}</span>
            </div>
            {testResults[src.id] && (
              <span style={{ fontSize: 12, color: testResults[src.id]!.success ? "var(--success-text)" : "var(--error-text)" }}>
                {testResults[src.id]!.success ? "✓ OK" : "✗ " + (testResults[src.id]!.error ?? "Failed")}
              </span>
            )}
            {src.source_type === "outlook" ? (
              <span style={{ fontSize: 11, color: "var(--success-text)" }}>AppleScript</span>
            ) : (
              <>
                <button className="btn btn-secondary btn-sm" onClick={() => handleTestExisting(src)}>Test</button>
                <button className="btn btn-secondary btn-sm" onClick={() => handleEdit(src)}>Edit</button>
              </>
            )}
            <button className="btn btn-ghost btn-sm" style={{ color: "var(--error-text)" }} onClick={() => handleDelete(src.id)}>Delete</button>
          </div>
        ))}

        {!addOpen && (
          <div style={{ display: "flex", gap: "var(--spacing-sm)", flexWrap: "wrap" }}>
            <button className="btn btn-primary btn-sm" disabled={oauthLoading !== null} onClick={async () => {
              setOauthLoading("microsoft");
              try {
                // Create source first, then OAuth flow saves tokens into it
                const config = JSON.stringify({ imap_host: "", imap_port: 993, smtp_host: "", smtp_port: 587, username: "", password_enc: "", use_tls: true, email: "", auth_type: "oauth2", oauth_provider: "microsoft" });
                const sourceId = await invoke<number>("save_message_source", { source: { id: 0, label: "Outlook", source_type: "imap", config, enabled: true, created_at: "" } });
                const result = await invoke<{ email?: string }>("start_oauth_flow", { provider: "microsoft", sourceId, clientId: null });
                // Update label with actual email
                if (result.email) {
                  const src = (await invoke<MessageSource[]>("get_message_sources")).find(s => s.id === sourceId);
                  if (src) { await invoke("save_message_source", { source: { ...src, label: result.email } }); }
                }
                loadSources();
              } catch (err) { alert("Microsoft sign-in failed: " + String(err)); }
              finally { setOauthLoading(null); }
            }}>{oauthLoading === "microsoft" ? "Signing in..." : "Sign in with Microsoft"}</button>
            <button className="btn btn-primary btn-sm" disabled={oauthLoading !== null} onClick={async () => {
              setOauthLoading("google");
              try {
                const config = JSON.stringify({ imap_host: "", imap_port: 993, smtp_host: "", smtp_port: 587, username: "", password_enc: "", use_tls: true, email: "", auth_type: "oauth2", oauth_provider: "google" });
                const sourceId = await invoke<number>("save_message_source", { source: { id: 0, label: "Gmail", source_type: "imap", config, enabled: true, created_at: "" } });
                const result = await invoke<{ email?: string }>("start_oauth_flow", { provider: "google", sourceId, clientId: null });
                if (result.email) {
                  const src = (await invoke<MessageSource[]>("get_message_sources")).find(s => s.id === sourceId);
                  if (src) { await invoke("save_message_source", { source: { ...src, label: result.email } }); }
                }
                loadSources();
              } catch (err) { alert("Google sign-in failed: " + String(err)); }
              finally { setOauthLoading(null); }
            }}>{oauthLoading === "google" ? "Signing in..." : "Sign in with Google"}</button>
            <button className="btn btn-secondary btn-sm" onClick={async () => {
              try {
                const status = await invoke<{ running: boolean }>("check_outlook_status");
                if (!status.running) {
                  alert("Outlook is not running. Please open Microsoft Outlook first.");
                  return;
                }
                await invoke("save_message_source", {
                  source: { id: 0, label: "Outlook (Local)", source_type: "outlook", config: "{}", enabled: true, created_at: "" },
                });
                loadSources();
              } catch (err) { alert("Failed: " + String(err)); }
            }}>Connect Local Outlook</button>
            <button className="btn btn-secondary btn-sm" onClick={() => setAddOpen(true)}>Manual IMAP Setup</button>
          </div>
        )}

        {addOpen && (
          <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: "var(--spacing-sm)" }}>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">Label</label>
                <input className="form-input" value={form.label} onChange={(e) => f("label", e.target.value)} placeholder="Work Gmail" />
              </div>
              <div className="form-group">
                <label className="form-label">Type</label>
                <select className="form-select" value="imap" onChange={() => {}}><option value="imap">IMAP</option></select>
              </div>
            </div>
            <div className="form-group">
              <label className="form-label">Email (also used as IMAP username)</label>
              <input className="form-input" value={form.email} onChange={(e) => f("email", e.target.value)} placeholder="you@example.com" />
            </div>
            <div className="form-group">
              <label className="form-label">Password</label>
              <input className="form-input" type="password" value={form.password} onChange={(e) => f("password", e.target.value)} />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">IMAP Host</label>
                <input className="form-input" value={form.imap_host} onChange={(e) => f("imap_host", e.target.value)} placeholder="imap.gmail.com" />
              </div>
              <div className="form-group">
                <label className="form-label">IMAP Port</label>
                <input className="form-input" type="number" value={form.imap_port} onChange={(e) => f("imap_port", parseInt(e.target.value, 10) || 993)} />
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">SMTP Host</label>
                <input className="form-input" value={form.smtp_host} onChange={(e) => f("smtp_host", e.target.value)} placeholder="smtp.gmail.com" />
              </div>
              <div className="form-group">
                <label className="form-label">SMTP Port</label>
                <input className="form-input" type="number" value={form.smtp_port} onChange={(e) => f("smtp_port", parseInt(e.target.value, 10) || 587)} />
              </div>
            </div>
            <div className="form-group" style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <input type="checkbox" checked={form.use_tls} onChange={(e) => f("use_tls", e.target.checked)} id="use-tls" />
              <label htmlFor="use-tls" className="form-label" style={{ marginBottom: 0 }}>Use TLS</label>
            </div>
            {newTestResult && (
              <div style={{ fontSize: 12, color: newTestResult.success ? "var(--success-text)" : "var(--error-text)" }}>
                {newTestResult.success ? "✓ Connection successful" : "✗ " + (newTestResult.error ?? "Connection failed")}
              </div>
            )}
            <div style={{ display: "flex", gap: "var(--spacing-sm)" }}>
              <button className="btn btn-secondary btn-sm" onClick={handleTestNew}>Test Connection</button>
              <button className="btn btn-primary btn-sm" onClick={handleSave} disabled={saving || !form.label || !form.imap_host}>
                {saving ? "Saving..." : "Save"}
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => { setAddOpen(false); setNewTestResult(null); setForm(EMPTY_IMAP); }}>Cancel</button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function Settings() {
  const { t, setLang: setAppLang } = useLang();
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<{ type: "success" | "error"; msg: string } | null>(null);

  // Provider state
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerConfigs, setProviderConfigs] = useState<Record<string, ProviderConfig>>({});
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [models, setModels] = useState<Record<string, string>>({});
  const [customModelOpen, setCustomModelOpen] = useState<Record<string, boolean>>({});
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});
  const [providersLoading, setProvidersLoading] = useState(true);
  const [reconcileRunning, setReconcileRunning] = useState(false);
  const [recentMemories, setRecentMemories] = useState<{ id: number; category: string; content: string; created_at: string }[]>([]);

  useEffect(() => {
    invoke<UserProfile | null>("get_profile")
      .then((p) => {
        if (p) {
          setProfile(p);
        }
        setLoading(false);
      })
      .catch((err) => {
        console.error(err);
        setLoading(false);
      });

    // Load recent memories for observation section
    invoke<{ id: number; category: string; content: string; created_at: string }[]>("get_memories")
      .then((mems) => setRecentMemories(mems.slice(0, 5)))
      .catch(() => {});

    // Load providers
    Promise.all([
      invoke<ProviderInfo[]>("discover_providers"),
      invoke<ProviderConfig[]>("get_provider_configs"),
    ])
      .then(([infos, configs]) => {
        setProviders(infos);
        const configMap: Record<string, ProviderConfig> = {};
        const keyMap: Record<string, string> = {};
        const modelMap: Record<string, string> = {};
        for (const cfg of configs) {
          configMap[cfg.provider_id] = cfg;
          if (cfg.api_key) keyMap[cfg.provider_id] = cfg.api_key;
          if (cfg.model) modelMap[cfg.provider_id] = cfg.model;
        }
        setProviderConfigs(configMap);
        setApiKeys(keyMap);
        setModels(modelMap);
        setProvidersLoading(false);
      })
      .catch(() => {
        setProvidersLoading(false);
      });
  }, []);

  const showToast = (type: "success" | "error", msg: string) => {
    setToast({ type, msg });
    setTimeout(() => setToast(null), 3000);
  };

  const updateIdentity = (updates: Partial<UserProfile["identity"]>) => {
    setProfile((prev) => prev ? { ...prev, identity: { ...prev.identity, ...updates } } : prev);
  };

  const updateSchedule = (updates: Partial<UserProfile["schedule"]>) => {
    setProfile((prev) => prev ? { ...prev, schedule: { ...prev.schedule, ...updates } } : prev);
  };

  const updateComm = (updates: Partial<UserProfile["communication"]>) => {
    setProfile((prev) => prev ? { ...prev, communication: { ...prev.communication, ...updates } } : prev);
  };

  const handleSave = async () => {
    if (!profile) return;
    setSaving(true);
    try {
      await invoke("save_profile", { profile });
      showToast("success", t("settings.saved"));
    } catch (err) {
      console.error(err);
      showToast("error", t("settings.saveError"));
    } finally {
      setSaving(false);
    }
  };

  const handleSaveProvider = async (providerId: string) => {
    const existing = providerConfigs[providerId];
    const key = apiKeys[providerId];
    const model = models[providerId];
    await invoke("save_provider_config", {
      config: {
        provider_id: providerId,
        api_key: key || null,
        model: model || null,
        base_url: existing?.base_url ?? null,
        enabled: true,
        priority: existing?.priority ?? null,
      } satisfies ProviderConfig,
    });
  };

  const handleTestProvider = async (providerId: string) => {
    setTestStates((prev) => ({ ...prev, [providerId]: "testing" }));
    try {
      const key = apiKeys[providerId];
      if (key !== undefined) {
        await handleSaveProvider(providerId);
      }
      const result = await invoke<{ success: boolean; error?: string }>("test_provider", {
        providerId,
      });
      setTestStates((prev) => ({ ...prev, [providerId]: result.success ? "ok" : "fail" }));
    } catch {
      setTestStates((prev) => ({ ...prev, [providerId]: "fail" }));
    }
  };

  const handleApiKeyChange = (providerId: string, value: string) => {
    setApiKeys((prev) => ({ ...prev, [providerId]: value }));
    setTestStates((prev) => ({ ...prev, [providerId]: "idle" }));
  };

  const handleModelChange = (providerId: string, value: string) => {
    setModels((prev) => ({ ...prev, [providerId]: value }));
  };

  const providerLoginHint = (providerId: string): string => {
    if (providerId === "claude-cli") return t("settings.loginHintClaude");
    if (providerId === "codex-cli") return t("settings.loginHintCodex");
    if (providerId === "gemini-cli") return t("settings.loginHintGemini");
    return t("settings.loginHintDefault");
  };

  if (loading) {
    return (
      <div className="page">
        <div className="page-header"><h1>{t("settings.title")}</h1></div>
        <div className="card"><p style={{ color: "var(--text-secondary)" }}>{t("settings.loading")}</p></div>
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="page">
        <div className="page-header">
          <h1>{t("settings.title")}</h1>
          <p>{t("settings.manageProfile")}</p>
        </div>
        <div className="card">
          <div className="empty-state">
            <h3>{t("settings.noProfile")}</h3>
            <p>{t("settings.noProfileHint")}</p>
            <Link to="/welcome" className="btn btn-primary">{t("settings.startSetup")}</Link>
          </div>
        </div>
      </div>
    );
  }

  // Sort providers by priority
  const sortedProviders = [...providers].sort((a, b) => a.priority - b.priority);
  const apiProviders = sortedProviders.filter((p) => p.kind === "HttpApi");

  const moveProvider = async (id: string, direction: "up" | "down") => {
    const idx = sortedProviders.findIndex((p) => p.id === id);
    if (idx < 0) return;
    const swapIdx = direction === "up" ? idx - 1 : idx + 1;
    if (swapIdx < 0 || swapIdx >= sortedProviders.length) return;
    const newOrder = sortedProviders.map((p) => p.id);
    [newOrder[idx], newOrder[swapIdx]] = [newOrder[swapIdx], newOrder[idx]];
    try {
      await invoke("save_provider_priorities", { orderedIds: newOrder });
      const updated = await invoke<ProviderInfo[]>("discover_providers");
      setProviders(updated);
    } catch (err) {
      console.error("Failed to save priorities:", err);
    }
  };

  return (
    <div className="page">
      {/* Section 1: Sage is observing — open by default */}
      <details open>
        <summary className="settings-section-title" style={{ cursor: "pointer", listStyle: "none", display: "flex", alignItems: "center", gap: 6, marginBottom: "var(--spacing-sm)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10" />
            <circle cx="12" cy="12" r="3" />
          </svg>
          {t("settings.sageObserving")}
        </summary>
        <div className="card" style={{ marginBottom: "var(--spacing-lg)" }}>
          {recentMemories.length === 0 ? (
            <p style={{ color: "var(--text-tertiary)", fontSize: 13 }}>{t("settings.noMemoriesYet")}</p>
          ) : (
            recentMemories.map((mem) => (
              <div key={mem.id} className="observation-card">
                <div style={{ fontSize: 13, color: "var(--text)", lineHeight: 1.55 }}>{mem.content}</div>
                <div className="observation-time">{mem.category} · {new Date(mem.created_at).toLocaleDateString()}</div>
              </div>
            ))
          )}
          <Link to="/about" style={{ fontSize: 12, color: "var(--accent)", textDecoration: "none", display: "inline-block", marginTop: "var(--spacing-sm)" }}>
            {t("settings.viewAllMemories")}
          </Link>
        </div>
      </details>

      {/* Section 2: Absolute limits — open by default */}
      <details open>
        <summary className="settings-section-title" style={{ cursor: "pointer", listStyle: "none", display: "flex", alignItems: "center", gap: 6, marginBottom: "var(--spacing-sm)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#ef4444" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
          {t("settings.ironRules")}
        </summary>
        <div className="card" style={{ marginBottom: "var(--spacing-lg)" }}>
          {profile.negative_rules.length === 0 ? (
            <p style={{ color: "var(--text-tertiary)", fontSize: 13 }}>{t("settings.noIronRules")}</p>
          ) : (
            profile.negative_rules.map((rule, i) => (
              <div key={i} className="iron-rule-item">{rule}</div>
            ))
          )}
        </div>
      </details>

      {/* Section 3: Email / Message Sources — open by default */}
      <details open>
        <summary className="settings-section-title" style={{ cursor: "pointer", listStyle: "none", display: "flex", alignItems: "center", gap: 6, marginBottom: "var(--spacing-sm)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z" />
            <polyline points="22,6 12,13 2,6" />
          </svg>
          Email / Message Sources
        </summary>
        <MessageSourcesSection />
      </details>

      {/* Section 4: Connections & capabilities — collapsed by default */}
      <details>
        <summary className="settings-section-title" style={{ cursor: "pointer", listStyle: "none", display: "flex", alignItems: "center", gap: 6, marginBottom: "var(--spacing-sm)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-secondary)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
          {t("settings.connectionsTitle")}
        </summary>

        {/* AI Providers */}
        <div className="settings-section">
          <div className="settings-section-title">{t("settings.aiProviders")}</div>
          <div className="card">
            {providersLoading ? (
              <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>{t("settings.detecting")}</p>
            ) : (
              <>
                <div style={{ marginBottom: 16 }}>
                  <div className="form-label" style={{ marginBottom: 8 }}>{t("settings.priorityOrder")}</div>
                  {sortedProviders.map((p, i) => (
                    <div key={p.id} style={{ padding: "6px 0", borderBottom: "1px solid var(--border-subtle)" }}>
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
                            <button className="btn btn-ghost" style={{ padding: "0 4px", fontSize: 10, lineHeight: 1, opacity: i === 0 ? 0.3 : 1 }} disabled={i === 0} onClick={() => moveProvider(p.id, "up")} title={t("settings.moveUp")}>▲</button>
                            <button className="btn btn-ghost" style={{ padding: "0 4px", fontSize: 10, lineHeight: 1, opacity: i === sortedProviders.length - 1 ? 0.3 : 1 }} disabled={i === sortedProviders.length - 1} onClick={() => moveProvider(p.id, "down")} title={t("settings.moveDown")}>▼</button>
                          </div>
                          <span style={{ fontSize: 13, color: "var(--text)" }}>
                            {p.display_name}
                            <span style={{ fontSize: 11, color: "var(--text-tertiary)", marginLeft: 6 }}>{p.kind === "Cli" ? "CLI" : "API"}</span>
                          </span>
                        </div>
                        {p.status === "Ready" ? (
                          <span style={{ fontSize: 12, color: "var(--success-text)", background: "var(--success-light)", padding: "2px 8px", borderRadius: 999, display: "inline-flex", alignItems: "center", gap: 4 }}>
                            <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--success)", display: "inline-block" }} />
                            {i === 0 ? t("settings.active") : t("settings.available")}
                          </span>
                        ) : p.status === "NeedsLogin" ? (
                          <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>
                            {t("settings.needsLogin")}
                          </span>
                        ) : p.status === "NeedsApiKey" ? (
                          <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>{t("settings.needsSetup")}</span>
                        ) : (
                          <span style={{ fontSize: 12, color: "var(--text-tertiary)", background: "var(--border-subtle)", padding: "2px 8px", borderRadius: 999 }}>{t("settings.notInstalled")}</span>
                        )}
                      </div>
                      {p.status === "NeedsLogin" && (
                        <div style={{ marginTop: 6, marginLeft: 28, fontSize: 12, color: "var(--warning-text)" }}>
                          {providerLoginHint(p.id)}
                        </div>
                      )}
                      {p.status !== "NotFound" && (() => {
                        const knownModels = PROVIDER_MODELS[p.id] || [];
                        const currentModel = models[p.id] ?? "";
                        const isCustom = customModelOpen[p.id] || (currentModel !== "" && !knownModels.some((m) => m.value === currentModel));
                        return (
                          <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4, marginLeft: 28 }}>
                            <span style={{ fontSize: 11, color: "var(--text-tertiary)", flexShrink: 0 }}>model</span>
                            {knownModels.length > 0 && !isCustom ? (
                              <select
                                className="form-select"
                                value={currentModel}
                                onChange={(e) => {
                                  if (e.target.value === "__custom__") {
                                    setCustomModelOpen((prev) => ({ ...prev, [p.id]: true }));
                                  } else {
                                    handleModelChange(p.id, e.target.value);
                                    // auto-save on selection
                                    const existing = providerConfigs[p.id];
                                    invoke("save_provider_config", {
                                      config: {
                                        provider_id: p.id,
                                        api_key: apiKeys[p.id] || null,
                                        model: e.target.value || null,
                                        base_url: existing?.base_url ?? null,
                                        enabled: true,
                                        priority: existing?.priority ?? null,
                                      } satisfies ProviderConfig,
                                    });
                                  }
                                }}
                                style={{ fontSize: 12, padding: "2px 8px", height: 26, flex: 1 }}
                              >
                                <option value="">{t("settings.modelDefault")}</option>
                                {knownModels.map((m) => (
                                  <option key={m.value} value={m.value}>{m.label}</option>
                                ))}
                                <option value="__custom__">{t("settings.modelCustom")}</option>
                              </select>
                            ) : (
                              <div style={{ display: "flex", gap: 4, flex: 1, alignItems: "center" }}>
                                <input
                                  className="form-input"
                                  value={currentModel}
                                  onChange={(e) => handleModelChange(p.id, e.target.value)}
                                  onBlur={() => handleSaveProvider(p.id)}
                                  placeholder={t("settings.enterModelId")}
                                  style={{ fontSize: 12, padding: "2px 8px", height: 26, flex: 1 }}
                                />
                                {knownModels.length > 0 && (
                                  <button
                                    className="btn btn-ghost"
                                    onClick={() => {
                                      setCustomModelOpen((prev) => ({ ...prev, [p.id]: false }));
                                      // Reset to first known model
                                      const first = knownModels[0].value;
                                      handleModelChange(p.id, first);
                                      handleSaveProvider(p.id);
                                    }}
                                    style={{ fontSize: 10, padding: "2px 6px", flexShrink: 0, color: "var(--text-tertiary)" }}
                                    title="Back to list"
                                  >
                                    ✕
                                  </button>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })()}
                    </div>
                  ))}
                </div>

                {apiProviders.length > 0 && (
                  <div>
                    <div className="form-label" style={{ marginBottom: 8, marginTop: 4 }}>{t("settings.apiKeys")}</div>
                    {apiProviders.map((p) => {
                      const testState = testStates[p.id] ?? "idle";
                      const key = apiKeys[p.id] ?? "";
                      return (
                        <div key={p.id} className="form-group" style={{ borderBottom: "1px solid var(--border-subtle)", paddingBottom: 12 }}>
                          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                            <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{p.display_name}</span>
                            {p.status === "Ready" ? (
                              <span style={{ fontSize: 12, color: "var(--success-text)", background: "var(--success-light)", padding: "2px 8px", borderRadius: 999, display: "inline-flex", alignItems: "center", gap: 4 }}>
                                <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--success)", display: "inline-block" }} />{t("settings.configured")}
                              </span>
                            ) : (
                              <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>{t("settings.needsSetup")}</span>
                            )}
                          </div>
                          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                            <input className="form-input" type="password" value={key} onChange={(e) => handleApiKeyChange(p.id, e.target.value)} placeholder={t("settings.enterApiKey")} style={{ flex: 1 }} />
                            <button className="btn btn-secondary btn-sm" onClick={() => handleTestProvider(p.id)} disabled={!key || testState === "testing"} style={{ flexShrink: 0 }}>
                              {testState === "testing" ? t("settings.testing") : t("settings.testConnection")}
                            </button>
                          </div>
                          {testState === "ok" && <div className="form-hint" style={{ color: "var(--success-text)", marginTop: 4 }}>{t("settings.connectionOk")}</div>}
                          {testState === "fail" && <div className="form-hint" style={{ color: "var(--error-text)", marginTop: 4 }}>{t("settings.connectionFail")}</div>}
                        </div>
                      );
                    })}
                  </div>
                )}

                {providers.length === 0 && <p style={{ fontSize: 13, color: "var(--text-tertiary)" }}>{t("settings.noProvidersDetected")}</p>}
              </>
            )}
          </div>
        </div>

        {/* Profile */}
        <div className="settings-section">
          <div className="settings-section-title">{t("settings.profile")}</div>
          <div className="card">
            <div className="form-group">
              <label className="form-label">{t("settings.name")}</label>
              <input className="form-input" value={profile.identity.name} onChange={(e) => updateIdentity({ name: e.target.value })} />
            </div>
            <div className="form-group">
              <label className="form-label">{t("settings.role")}</label>
              <input className="form-input" value={profile.identity.role} onChange={(e) => updateIdentity({ role: e.target.value })} />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">{t("settings.primaryLanguage")}</label>
                <input className="form-input" value={profile.identity.primary_language} onChange={(e) => updateIdentity({ primary_language: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">{t("settings.secondaryLanguage")}</label>
                <input className="form-input" value={profile.identity.secondary_language} onChange={(e) => updateIdentity({ secondary_language: e.target.value })} />
              </div>
            </div>
            <div className="form-group">
              <label className="form-label">{t("settings.promptLanguage")}</label>
              <select
                className="form-select"
                value={profile.identity.prompt_language ?? "zh"}
                onChange={async (e) => {
                  const lang = e.target.value;
                  const updated = { ...profile, identity: { ...profile.identity, prompt_language: lang } };
                  setProfile(updated);
                  setAppLang(lang === "en" ? "en" : "zh");
                  try {
                    await invoke("save_profile", { profile: updated });
                    showToast("success", t("settings.promptLanguageSaved"));
                  } catch (err) {
                    showToast("error", String(err));
                  }
                }}
              >
                <option value="zh">{t("settings.chinese")}</option>
                <option value="en">English</option>
              </select>
              <div className="form-hint">{t("settings.promptLanguageHint")}</div>
            </div>
          </div>
        </div>

        {/* Schedule */}
        <div className="settings-section">
          <div className="settings-section-title">{t("settings.schedule")}</div>
          <div className="card">
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">{t("settings.morningBrief")}</label>
                <input className="form-input" type="number" value={profile.schedule.morning_brief_hour} onChange={(e) => updateSchedule({ morning_brief_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
              </div>
              <div className="form-group">
                <label className="form-label">{t("settings.eveningReview")}</label>
                <input className="form-input" type="number" value={profile.schedule.evening_review_hour} onChange={(e) => updateSchedule({ evening_review_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">{t("settings.workStart")}</label>
                <input className="form-input" type="number" value={profile.schedule.work_start_hour} onChange={(e) => updateSchedule({ work_start_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
              </div>
              <div className="form-group">
                <label className="form-label">{t("settings.workEnd")}</label>
                <input className="form-input" type="number" value={profile.schedule.work_end_hour} onChange={(e) => updateSchedule({ work_end_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">{t("settings.weeklyReportDay")}</label>
                <select className="form-select" value={profile.schedule.weekly_report_day} onChange={(e) => updateSchedule({ weekly_report_day: e.target.value })}>
                  {Object.entries(WEEKDAY_LABELS).map(([val, label]) => (
                    <option key={val} value={val}>{label}</option>
                  ))}
                </select>
              </div>
              <div className="form-group">
                <label className="form-label">{t("settings.weeklyReportTime")}</label>
                <input className="form-input" type="number" value={profile.schedule.weekly_report_hour} onChange={(e) => updateSchedule({ weekly_report_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
              </div>
            </div>
            <div className="form-hint">{t("settings.timeFormatHint")}</div>
          </div>
        </div>

        {/* Communication */}
        <div className="settings-section">
          <div className="settings-section-title">{t("settings.communication")}</div>
          <div className="card">
            <div className="form-group">
              <label className="form-label">{t("settings.commStyle")}</label>
              <select className="form-select" value={profile.communication.style} onChange={(e) => updateComm({ style: e.target.value })}>
                <option value="Direct">{t("settings.commDirect")}</option>
                <option value="Formal">{t("settings.commFormal")}</option>
                <option value="Casual">{t("settings.commCasual")}</option>
              </select>
            </div>
            <div className="form-group">
              <label className="form-label">{t("settings.maxNotifLen")}</label>
              <input className="form-input" type="number" value={profile.communication.notification_max_chars} onChange={(e) => updateComm({ notification_max_chars: parseInt(e.target.value, 10) || 200 })} min="50" max="500" />
              <div className="form-hint">{t("settings.maxNotifLenHint")}</div>
            </div>
          </div>
        </div>

        {/* Memory Management */}
        <div className="settings-section">
          <div className="settings-section-title">{t("settings.memoryManagement")}</div>
          <div className="card">
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{t("settings.memoryEvolution")}</div>
                  <div className="form-hint">{t("settings.memoryEvolutionHint")}</div>
                </div>
                <button
                  className="btn btn-primary btn-sm"
                  onClick={async () => {
                    try {
                      const msg = await invoke<string>("trigger_memory_evolution");
                      showToast("success", msg + " " + t("settings.evolutionNotif"));
                    } catch (err) {
                      showToast("error", String(err));
                    }
                  }}
                >
                  {t("settings.runNow")}
                </button>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", borderTop: "1px solid var(--border-subtle)", paddingTop: 12 }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{t("settings.testNotification")}</div>
                  <div className="form-hint">{t("settings.testNotifHint")}</div>
                </div>
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={async () => {
                    try {
                      await invoke("test_notification", { route: "/about" });
                      showToast("success", t("settings.notifSent"));
                    } catch (err) {
                      showToast("error", String(err));
                    }
                  }}
                >
                  {t("settings.sendTest")}
                </button>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", borderTop: "1px solid var(--border-subtle)", paddingTop: 12 }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{t("settings.reconcile")}</div>
                  <div className="form-hint">{t("settings.reconcileHint")}</div>
                </div>
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={reconcileRunning}
                  onClick={() => {
                    setReconcileRunning(true);
                    showToast("success", t("settings.reconciling"));
                    invoke<{ revised: number }>("trigger_reconcile")
                      .then((r) => {
                        showToast("success", r.revised
                          ? t("settings.reconcileRevised").replace("{n}", String(r.revised))
                          : t("settings.reconcileNone"));
                      })
                      .catch((err) => showToast("error", String(err)))
                      .finally(() => setReconcileRunning(false));
                  }}
                >
                  {reconcileRunning ? t("settings.reconciling") : t("settings.runNow")}
                </button>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", borderTop: "1px solid var(--border-subtle)", paddingTop: 12 }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{t("settings.syncToClaudeCode")}</div>
                  <div className="form-hint">{t("settings.syncHint")}</div>
                </div>
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={async () => {
                    try {
                      const result = await invoke<string>("sync_memory");
                      showToast("success", result);
                    } catch (err) {
                      showToast("error", String(err));
                    }
                  }}
                >
                  {t("settings.syncNow")}
                </button>
              </div>
            </div>
          </div>
        </div>
      </details>

      <div className="form-actions">
        <Link to="/welcome" className="btn btn-secondary">{t("settings.redoSetup")}</Link>
        <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
          {saving ? t("settings.saving") : t("save")}
        </button>
      </div>

      {toast && (
        <div
          className="toast"
          style={{
            borderColor: toast.type === "error" ? "var(--error)" : "var(--accent)",
            color: toast.type === "error" ? "var(--error-text)" : "var(--text)",
          }}
        >
          {toast.msg}
        </div>
      )}
    </div>
  );
}

export default Settings;
