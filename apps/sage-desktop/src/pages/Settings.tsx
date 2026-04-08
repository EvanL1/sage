import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import MessageSourcesSection from "./MessageSourcesSection";

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

import type { ProviderInfo, ProviderConfig } from "../types";

type TestState = "idle" | "testing" | "ok" | "fail";

const WEEKDAY_LABELS: Record<string, string> = {
  Mon: "Monday", Tue: "Tuesday", Wed: "Wednesday", Thu: "Thursday",
  Fri: "Friday", Sat: "Saturday", Sun: "Sunday",
};

import { PROVIDER_MODELS } from "../providerModels";

function ProviderStatusBadge({
  status,
  readyLabel,
  needsLoginLabel,
  needsSetupLabel,
  notInstalledLabel,
}: {
  status: ProviderInfo["status"];
  readyLabel: string;
  needsLoginLabel?: string;
  needsSetupLabel?: string;
  notInstalledLabel?: string;
}) {
  if (status === "Ready") {
    return (
      <span style={{ fontSize: 12, color: "var(--success-text)", background: "var(--success-light)", padding: "2px 8px", borderRadius: 999, display: "inline-flex", alignItems: "center", gap: 4 }}>
        <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--success)", display: "inline-block" }} />
        {readyLabel}
      </span>
    );
  }
  if (status === "NeedsLogin") {
    return (
      <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>
        {needsLoginLabel ?? "Needs Login"}
      </span>
    );
  }
  if (status === "NeedsApiKey") {
    return (
      <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>
        {needsSetupLabel ?? "Needs Setup"}
      </span>
    );
  }
  return (
    <span style={{ fontSize: 12, color: "var(--text-tertiary)", background: "var(--border-subtle)", padding: "2px 8px", borderRadius: 999 }}>{notInstalledLabel ?? "Not Installed"}</span>
  );
}

function Settings() {
  const { t, setLang: setAppLang } = useLang();
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<{ type: "success" | "error"; msg: string } | null>(null);
  const [nlInput, setNlInput] = useState("");
  const [nlBusy, setNlBusy] = useState(false);

  // Provider state
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerConfigs, setProviderConfigs] = useState<Record<string, ProviderConfig>>({});
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [models, setModels] = useState<Record<string, string>>({});
  const [customModelOpen, setCustomModelOpen] = useState<Record<string, boolean>>({});
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});
  const [providersLoading, setProvidersLoading] = useState(true);
  const [evolutionProgress, setEvolutionProgress] = useState("");
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

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

  // 进入页面时检查 evolution 是否在跑，如果是则恢复轮询（5 分钟无变化视为残留并清除）
  useEffect(() => {
    const clearPoll = () => { if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; } };
    invoke<string>("get_evolution_progress").then((p) => {
      if (p) {
        setEvolutionProgress(p);
        let lastValue = p;
        let staleCount = 0;
        clearPoll();
        pollRef.current = setInterval(async () => {
          try {
            const v = await invoke<string>("get_evolution_progress");
            if (!v) { clearPoll(); setEvolutionProgress(""); return; }
            if (v === lastValue) { staleCount++; } else { staleCount = 0; lastValue = v; }
            setEvolutionProgress(v);
            // 5 分钟（150 × 2s）无变化 → 视为残留，清除
            if (staleCount >= 150) { clearPoll(); setEvolutionProgress(""); }
          } catch { clearPoll(); setEvolutionProgress(""); }
        }, 2000);
      }
    }).catch(() => {});
    return () => clearPoll();
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
      {/* ── Natural language config ── */}
      <div style={{ marginBottom: 16 }}>
        <div style={{ display: "flex", gap: 8 }}>
          <input
            type="text"
            placeholder={`Tell Sage, e.g. "Set morning brief to 7am" or "Enable WeChat channel"`}
            value={nlInput}
            onChange={(e) => setNlInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing && nlInput.trim()) {
                setNlBusy(true);
                invoke<string>("update_config_natural", { text: nlInput })
                  .then((msg) => {
                    showToast("success", msg);
                    setNlInput("");
                    // reload profile to reflect changes
                    invoke<UserProfile>("get_profile").then((p) => { if (p) setProfile(p); });
                  })
                  .catch((e) => showToast("error", String(e)))
                  .finally(() => setNlBusy(false));
              }
            }}
            disabled={nlBusy}
            className="form-input"
            style={{ flex: 1, opacity: nlBusy ? 0.6 : 1 }}
          />
          {nlBusy && <span style={{ fontSize: 12, color: "var(--text-tertiary)", alignSelf: "center", whiteSpace: "nowrap" }}>Updating...</span>}
        </div>
      </div>

      {/* Email / Message Sources */}
      <MessageSourcesSection showToast={showToast} />

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
                        <ProviderStatusBadge
                          status={p.status}
                          readyLabel={i === 0 ? t("settings.active") : t("settings.available")}
                          needsLoginLabel={t("settings.needsLogin")}
                          needsSetupLabel={t("settings.needsSetup")}
                          notInstalledLabel={t("settings.notInstalled")}
                        />
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
                            <ProviderStatusBadge
                              status={p.status}
                              readyLabel={t("settings.configured")}
                              needsSetupLabel={t("settings.needsSetup")}
                            />
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
                  disabled={!!evolutionProgress}
                  onClick={async () => {
                    try {
                      // 清除可能存在的旧轮询
                      if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
                      setEvolutionProgress("启动中…");
                      const msg = await invoke<string>("trigger_memory_evolution");
                      showToast("success", msg);
                      let lastVal = "", stale = 0, done = false;
                      const check = async () => {
                        try {
                          const p = await invoke<string>("get_evolution_progress");
                          if (!p) {
                            if (done) return; // 避免重复 toast
                            done = true;
                            if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
                            setEvolutionProgress(""); showToast("success", t("settings.evolutionNotif")); return;
                          }
                          setEvolutionProgress(p);
                          if (p === lastVal) { stale++; } else { stale = 0; lastVal = p; }
                          if (stale >= 300) {
                            if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
                            setEvolutionProgress("");
                          }
                        } catch {
                          if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null; }
                          setEvolutionProgress("");
                        }
                      };
                      // 首次 500ms 后检查，之后每 1s 轮询（比之前 2s 更及时）
                      pollRef.current = setInterval(check, 1000);
                      setTimeout(check, 500);
                    } catch (err) {
                      showToast("error", String(err));
                    }
                  }}
                >
                  {evolutionProgress ? "..." : t("settings.runNow")}
                </button>
              </div>
              {evolutionProgress && (
                <div style={{
                  padding: "8px 12px",
                  background: "var(--bg-secondary)",
                  borderRadius: 6,
                  fontSize: 12,
                  color: "var(--text-secondary)",
                  fontFamily: "monospace",
                }}>
                  {evolutionProgress}
                </div>
              )}
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
