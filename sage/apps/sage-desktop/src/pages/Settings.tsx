import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";

interface UserProfile {
  identity: {
    name: string;
    role: string;
    reporting_line: string[];
    primary_language: string;
    secondary_language: string;
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

interface ProviderInfo {
  id: string;
  display_name: string;
  kind: "Cli" | "HttpApi";
  status: "Ready" | "NeedsApiKey" | "NotFound";
  priority: number;
}

interface ProviderConfig {
  provider_id: string;
  api_key: string | null;
  model: string | null;
  base_url: string | null;
  enabled: boolean;
}

type TestState = "idle" | "testing" | "ok" | "fail";

const WEEKDAY_LABELS: Record<string, string> = {
  Mon: "周一", Tue: "周二", Wed: "周三", Thu: "周四",
  Fri: "周五", Sat: "周六", Sun: "周日",
};

// Parse "名称 - 描述" lines into structured objects
function parseProjectLines(text: string) {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const idx = line.indexOf(" - ");
      return idx !== -1
        ? { name: line.slice(0, idx).trim(), description: line.slice(idx + 3).trim(), status: "Active" }
        : { name: line, description: "", status: "Active" };
    });
}

function parseStakeholderLines(text: string) {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const idx = line.indexOf(" - ");
      return idx !== -1
        ? { name: line.slice(0, idx).trim(), role: line.slice(idx + 3).trim(), relationship: "同事" }
        : { name: line, role: "", relationship: "同事" };
    });
}

function projectsToText(projects: { name: string; description: string }[]) {
  return projects.map((p) => (p.description ? `${p.name} - ${p.description}` : p.name)).join("\n");
}

function stakeholdersToText(stakeholders: { name: string; role: string }[]) {
  return stakeholders.map((s) => (s.role ? `${s.name} - ${s.role}` : s.name)).join("\n");
}

function Settings() {
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [toast, setToast] = useState<{ type: "success" | "error"; msg: string } | null>(null);

  // Work context text state (derived from profile, edited as free text)
  const [projectsText, setProjectsText] = useState("");
  const [stakeholdersText, setStakeholdersText] = useState("");
  const [reportingLineText, setReportingLineText] = useState("");

  // Provider state
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerConfigs, setProviderConfigs] = useState<Record<string, ProviderConfig>>({});
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});
  const [providersLoading, setProvidersLoading] = useState(true);

  useEffect(() => {
    invoke<UserProfile | null>("get_profile")
      .then((p) => {
        if (p) {
          setProfile(p);
          setProjectsText(projectsToText(p.work_context.projects));
          setStakeholdersText(stakeholdersToText(p.work_context.stakeholders));
          setReportingLineText((p.identity.reporting_line ?? []).join("\n"));
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
        for (const cfg of configs) {
          configMap[cfg.provider_id] = cfg;
          if (cfg.api_key) keyMap[cfg.provider_id] = cfg.api_key;
        }
        setProviderConfigs(configMap);
        setApiKeys(keyMap);
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
      const merged: UserProfile = {
        ...profile,
        identity: {
          ...profile.identity,
          reporting_line: reportingLineText.split("\n").map((l) => l.trim()).filter(Boolean),
        },
        work_context: {
          ...profile.work_context,
          projects: parseProjectLines(projectsText),
          stakeholders: parseStakeholderLines(stakeholdersText),
        },
      };
      await invoke("save_profile", { profile: merged });
      setProfile(merged);
      showToast("success", "设置已保存");
    } catch (err) {
      console.error(err);
      showToast("error", "出了点问题，再试一次？");
    } finally {
      setSaving(false);
    }
  };

  const handleTestProvider = async (providerId: string) => {
    setTestStates((prev) => ({ ...prev, [providerId]: "testing" }));
    try {
      const key = apiKeys[providerId];
      if (key !== undefined) {
        const existing = providerConfigs[providerId];
        await invoke("save_provider_config", {
          config: {
            provider_id: providerId,
            api_key: key || null,
            model: existing?.model ?? null,
            base_url: existing?.base_url ?? null,
            enabled: true,
          } satisfies ProviderConfig,
        });
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

  if (loading) {
    return (
      <div className="page">
        <div className="page-header"><h1>设置</h1></div>
        <div className="card"><p style={{ color: "var(--text-secondary)" }}>加载中...</p></div>
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="page">
        <div className="page-header">
          <h1>设置</h1>
          <p>管理你的个人资料和偏好</p>
        </div>
        <div className="card">
          <div className="empty-state">
            <h3>尚未创建个人资料</h3>
            <p>请先完成初始设置</p>
            <Link to="/welcome" className="btn btn-primary">开始设置</Link>
          </div>
        </div>
      </div>
    );
  }

  const apiProviders = providers.filter((p) => p.kind === "HttpApi");
  const cliProviders = providers.filter((p) => p.kind === "Cli");

  return (
    <div className="page">
      <div className="page-header">
        <h1>设置</h1>
        <p>管理你的个人资料和偏好</p>
      </div>

      {/* ── AI 服务配置 ── */}
      <div className="settings-section">
        <div className="settings-section-title">AI 服务配置</div>
        <div className="card">
          {providersLoading ? (
            <p style={{ color: "var(--text-secondary)", fontSize: 13 }}>检测中...</p>
          ) : (
            <>
              {cliProviders.length > 0 && (
                <div style={{ marginBottom: apiProviders.length > 0 ? 16 : 0 }}>
                  <div className="form-label" style={{ marginBottom: 8 }}>本地 CLI</div>
                  {cliProviders.map((p) => (
                    <div key={p.id} style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "6px 0", borderBottom: "1px solid var(--border-subtle)" }}>
                      <span style={{ fontSize: 13, color: "var(--text)" }}>{p.display_name}</span>
                      {p.status === "Ready" ? (
                        <span style={{ fontSize: 12, color: "var(--success-text)", background: "var(--success-light)", padding: "2px 8px", borderRadius: 999, display: "inline-flex", alignItems: "center", gap: 4 }}>
                          <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--success)", display: "inline-block" }} />
                          可用
                        </span>
                      ) : (
                        <span style={{ fontSize: 12, color: "var(--text-tertiary)", background: "var(--border-subtle)", padding: "2px 8px", borderRadius: 999 }}>
                          未安装
                        </span>
                      )}
                    </div>
                  ))}
                </div>
              )}

              {apiProviders.length > 0 && (
                <div>
                  {cliProviders.length > 0 && <div className="form-label" style={{ marginBottom: 8, marginTop: 4 }}>API 服务</div>}
                  {apiProviders.map((p) => {
                    const testState = testStates[p.id] ?? "idle";
                    const key = apiKeys[p.id] ?? "";
                    return (
                      <div key={p.id} className="form-group" style={{ borderBottom: "1px solid var(--border-subtle)", paddingBottom: 12 }}>
                        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{p.display_name}</span>
                          {p.status === "Ready" ? (
                            <span style={{ fontSize: 12, color: "var(--success-text)", background: "var(--success-light)", padding: "2px 8px", borderRadius: 999, display: "inline-flex", alignItems: "center", gap: 4 }}>
                              <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--success)", display: "inline-block" }} />
                              已配置
                            </span>
                          ) : (
                            <span style={{ fontSize: 12, color: "var(--warning-text)", background: "var(--warning-light)", padding: "2px 8px", borderRadius: 999 }}>
                              需要配置
                            </span>
                          )}
                        </div>
                        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                          <input
                            className="form-input"
                            type="password"
                            value={key}
                            onChange={(e) => handleApiKeyChange(p.id, e.target.value)}
                            placeholder="输入 API Key..."
                            style={{ flex: 1 }}
                          />
                          <button
                            className="btn btn-secondary btn-sm"
                            onClick={() => handleTestProvider(p.id)}
                            disabled={!key || testState === "testing"}
                            style={{ flexShrink: 0 }}
                          >
                            {testState === "testing" ? "测试中..." : "测试连接"}
                          </button>
                        </div>
                        {testState === "ok" && (
                          <div className="form-hint" style={{ color: "var(--success-text)", marginTop: 4 }}>
                            连接成功
                          </div>
                        )}
                        {testState === "fail" && (
                          <div className="form-hint" style={{ color: "var(--error-text)", marginTop: 4 }}>
                            出了点问题，再试一次？
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              {providers.length === 0 && (
                <p style={{ fontSize: 13, color: "var(--text-tertiary)" }}>未检测到可用 AI 服务</p>
              )}
            </>
          )}
        </div>
      </div>

      {/* ── 个人资料 ── */}
      <div className="settings-section">
        <div className="settings-section-title">个人资料</div>
        <div className="card">
          <div className="form-group">
            <label className="form-label">姓名</label>
            <input className="form-input" value={profile.identity.name} onChange={(e) => updateIdentity({ name: e.target.value })} />
          </div>
          <div className="form-group">
            <label className="form-label">职位</label>
            <input className="form-input" value={profile.identity.role} onChange={(e) => updateIdentity({ role: e.target.value })} />
          </div>
          <div className="form-row">
            <div className="form-group">
              <label className="form-label">主要语言</label>
              <input className="form-input" value={profile.identity.primary_language} onChange={(e) => updateIdentity({ primary_language: e.target.value })} />
            </div>
            <div className="form-group">
              <label className="form-label">次要语言</label>
              <input className="form-input" value={profile.identity.secondary_language} onChange={(e) => updateIdentity({ secondary_language: e.target.value })} />
            </div>
          </div>
        </div>
      </div>

      {/* ── 日程偏好 ── */}
      <div className="settings-section">
        <div className="settings-section-title">日程偏好</div>
        <div className="card">
          <div className="form-row">
            <div className="form-group">
              <label className="form-label">早间简报</label>
              <input className="form-input" type="number" value={profile.schedule.morning_brief_hour} onChange={(e) => updateSchedule({ morning_brief_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
            </div>
            <div className="form-group">
              <label className="form-label">晚间回顾</label>
              <input className="form-input" type="number" value={profile.schedule.evening_review_hour} onChange={(e) => updateSchedule({ evening_review_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
            </div>
          </div>
          <div className="form-row">
            <div className="form-group">
              <label className="form-label">上班时间</label>
              <input className="form-input" type="number" value={profile.schedule.work_start_hour} onChange={(e) => updateSchedule({ work_start_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
            </div>
            <div className="form-group">
              <label className="form-label">下班时间</label>
              <input className="form-input" type="number" value={profile.schedule.work_end_hour} onChange={(e) => updateSchedule({ work_end_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
            </div>
          </div>
          <div className="form-row">
            <div className="form-group">
              <label className="form-label">周报日</label>
              <select className="form-select" value={profile.schedule.weekly_report_day} onChange={(e) => updateSchedule({ weekly_report_day: e.target.value })}>
                {Object.entries(WEEKDAY_LABELS).map(([val, label]) => (
                  <option key={val} value={val}>{label}</option>
                ))}
              </select>
            </div>
            <div className="form-group">
              <label className="form-label">周报时间</label>
              <input className="form-input" type="number" value={profile.schedule.weekly_report_hour} onChange={(e) => updateSchedule({ weekly_report_hour: parseInt(e.target.value, 10) || 0 })} min="0" max="23" />
            </div>
          </div>
          <div className="form-hint">所有时间均为 0-23 小时制</div>
        </div>
      </div>

      {/* ── 沟通偏好 ── */}
      <div className="settings-section">
        <div className="settings-section-title">沟通偏好</div>
        <div className="card">
          <div className="form-group">
            <label className="form-label">沟通风格</label>
            <select className="form-select" value={profile.communication.style} onChange={(e) => updateComm({ style: e.target.value })}>
              <option value="Direct">直接 — 简短有力</option>
              <option value="Formal">正式 — 结构化专业</option>
              <option value="Casual">随意 — 轻松自然</option>
            </select>
          </div>
          <div className="form-group">
            <label className="form-label">通知最大字数</label>
            <input className="form-input" type="number" value={profile.communication.notification_max_chars} onChange={(e) => updateComm({ notification_max_chars: parseInt(e.target.value, 10) || 200 })} min="50" max="500" />
            <div className="form-hint">建议通知的最大字符数（50-500）</div>
          </div>
        </div>
      </div>

      {/* ── 工作上下文 ── */}
      <div className="settings-section">
        <div className="settings-section-title">工作上下文</div>
        <div className="card">
          <div className="form-group">
            <label className="form-label">汇报线</label>
            <textarea
              className="form-textarea"
              value={reportingLineText}
              onChange={(e) => setReportingLineText(e.target.value)}
              placeholder={"你的名字\n直属上级\n上级的上级"}
              rows={3}
            />
            <div className="form-hint">每行一个，从你开始往上排列</div>
          </div>
          <div className="form-group">
            <label className="form-label">当前项目</label>
            <textarea
              className="form-textarea"
              value={projectsText}
              onChange={(e) => setProjectsText(e.target.value)}
              placeholder={"项目A - 简要描述\n项目B - 简要描述"}
              rows={4}
            />
            <div className="form-hint">每行一个，格式：项目名 - 描述</div>
          </div>
          <div className="form-group">
            <label className="form-label">利益相关者</label>
            <textarea
              className="form-textarea"
              value={stakeholdersText}
              onChange={(e) => setStakeholdersText(e.target.value)}
              placeholder={"张三 - 产品经理\n李四 - 客户"}
              rows={4}
            />
            <div className="form-hint">每行一个，格式：姓名 - 角色</div>
          </div>
        </div>
      </div>

      <div className="form-actions">
        <Link to="/welcome" className="btn btn-secondary">重新设置</Link>
        <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
          {saving ? "保存中..." : "保存"}
        </button>
      </div>

      {toast && (
        <div
          className="toast"
          style={{
            borderColor: toast.type === "error" ? "var(--error)" : "var(--border)",
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
