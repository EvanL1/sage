import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MessageSource } from "../types";

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

interface Props {
  showToast: (type: "success" | "error", msg: string) => void;
}

function MessageSourcesSection({ showToast }: Props) {
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
      showToast("error", "Failed to save: " + String(err));
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
      showToast("error", "Failed to delete: " + String(err));
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
              } catch (err) { showToast("error", "Microsoft sign-in failed: " + String(err)); }
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
              } catch (err) { showToast("error", "Google sign-in failed: " + String(err)); }
              finally { setOauthLoading(null); }
            }}>{oauthLoading === "google" ? "Signing in..." : "Sign in with Google"}</button>
            <button className="btn btn-secondary btn-sm" onClick={async () => {
              try {
                const status = await invoke<{ running: boolean }>("check_outlook_status");
                if (!status.running) {
                  showToast("error", "Outlook is not running. Please open Microsoft Outlook first.");
                  return;
                }
                await invoke("save_message_source", {
                  source: { id: 0, label: "Outlook (Local)", source_type: "outlook", config: "{}", enabled: true, created_at: "" },
                });
                loadSources();
              } catch (err) { showToast("error", "Failed: " + String(err)); }
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

export default MessageSourcesSection;
