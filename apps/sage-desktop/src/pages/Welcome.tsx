import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";

import type { ProviderInfo } from "../types";

const TOTAL_STEPS = 3;

function Welcome() {
  const { t, setLang } = useLang();
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [animating, setAnimating] = useState(false);
  const [direction, setDirection] = useState<"forward" | "back">("forward");

  // Step 1: name + language
  const [name, setName] = useState("");
  const [lang, setLangChoice] = useState<"zh" | "en">("zh");

  // Step 2: data sources + API key
  const [emailEnabled, setEmailEnabled] = useState(false);
  const [calendarEnabled, setCalendarEnabled] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [selectedProvider, setSelectedProvider] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [testResult, setTestResult] = useState<"idle" | "testing" | "ok" | "fail">("idle");

  // Submit state
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  const readyProviders = providers.filter((p) => p.status === "Ready");
  const apiProviders = providers.filter((p) => p.kind === "HttpApi");

  useEffect(() => {
    if (step === 1) {
      invoke<ProviderInfo[]>("discover_providers")
        .then((result) => {
          setProviders(result);
          const firstApi = result.find((p) => p.kind === "HttpApi");
          if (firstApi) setSelectedProvider(firstApi.id);
        })
        .catch(() => setProviders([]));
    }
  }, [step]);

  const goTo = (target: number) => {
    if (animating || target === step) return;
    setDirection(target > step ? "forward" : "back");
    setAnimating(true);
    setTimeout(() => {
      setStep(target);
      setAnimating(false);
    }, 200);
  };

  const handleLangChange = (val: "zh" | "en") => {
    setLangChoice(val);
    setLang(val);
  };

  const handleTestConnection = async () => {
    setTestResult("testing");
    try {
      if (apiKey) {
        await invoke("save_provider_config", {
          config: { provider_id: selectedProvider, api_key: apiKey, model: null, base_url: null, enabled: true },
        });
      }
      const result = await invoke<{ success: boolean; error?: string }>("test_provider", { providerId: selectedProvider });
      setTestResult(result.success ? "ok" : "fail");
    } catch {
      setTestResult("fail");
    }
  };

  const handleComplete = async () => {
    setError("");
    setSubmitting(true);
    try {
      await invoke("quick_setup", {
        name,
        role: undefined,
        apiKey: apiKey || undefined,
        providerId: selectedProvider || undefined,
        promptLanguage: lang,
      });
      navigate("/");
    } catch (err) {
      console.error(err);
      setError(t("welcome.errorRetry"));
    } finally {
      setSubmitting(false);
    }
  };

  const renderStep = () => {
    switch (step) {
      case 0:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              {t("welcome.greet1a")}<br />{t("welcome.greet1b")}
            </p>
            <div className="welcome-card">
              <div className="form-group">
                <input
                  className="form-input"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing && name.trim()) goTo(1); }}
                  placeholder={t("welcome.yourName")}
                  autoFocus
                />
              </div>
              <div className="form-group" style={{ marginTop: 16 }}>
                <label className="form-label">{t("welcome.languageLabel")}</label>
                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  {(["zh", "en"] as const).map((l) => (
                    <button
                      key={l}
                      className={`tag ${lang === l ? "selected" : ""}`}
                      onClick={() => handleLangChange(l)}
                    >
                      {l === "zh" ? "中文" : "English"}
                    </button>
                  ))}
                </div>
              </div>
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(1)} disabled={!name.trim()}>
                {t("welcome.continue")}
              </button>
            </div>
          </div>
        );

      case 1:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              {t("welcome.greet4a")}<br /><br />
              {t("welcome.greet4b")}
            </p>
            <div className="welcome-card">
              {/* Data sources */}
              <div style={{ marginBottom: 20 }}>
                <label className="form-label" style={{ marginBottom: 10, display: "block" }}>
                  {t("welcome.dataSourcesLabel")}
                </label>
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  <label style={{ display: "flex", alignItems: "center", gap: 10, cursor: "pointer", fontSize: 13 }}>
                    <input type="checkbox" checked={emailEnabled} onChange={(e) => setEmailEnabled(e.target.checked)} />
                    <span>{t("welcome.emailSource")}</span>
                  </label>
                  <label style={{ display: "flex", alignItems: "center", gap: 10, cursor: "pointer", fontSize: 13 }}>
                    <input type="checkbox" checked={calendarEnabled} onChange={(e) => setCalendarEnabled(e.target.checked)} />
                    <span>{t("welcome.calendarSource")}</span>
                  </label>
                </div>
                <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 6 }}>
                  {t("welcome.dataSourcesHint")}
                </p>
              </div>

              {/* API key */}
              {readyProviders.length > 0 ? (
                <div className="provider-status ready">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--success)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                  <span>{t("welcome.detectedProvider")} {readyProviders[0].display_name} {t("welcome.readyToUse")}</span>
                </div>
              ) : (
                <>
                  <div className="form-group">
                    <label className="form-label">{t("welcome.aiProviderLabel")}</label>
                    <select
                      className="form-select"
                      value={selectedProvider}
                      onChange={(e) => { setSelectedProvider(e.target.value); setTestResult("idle"); }}
                    >
                      <option value="">{t("welcome.selectProvider")}</option>
                      {apiProviders.map((p) => (
                        <option key={p.id} value={p.id}>{p.display_name}</option>
                      ))}
                    </select>
                  </div>
                  <div className="form-group">
                    <label className="form-label">
                      {t("welcome.apiKeyLabel")}
                      <span style={{ fontSize: 11, color: "var(--text-secondary)", marginLeft: 8, fontWeight: 400 }}>
                        {t("welcome.apiKeyCost")}
                      </span>
                    </label>
                    <input
                      className="form-input"
                      type="password"
                      value={apiKey}
                      onChange={(e) => { setApiKey(e.target.value); setTestResult("idle"); }}
                      placeholder="sk-..."
                    />
                  </div>
                  {(selectedProvider && apiKey) && (
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <button
                        className="btn btn-secondary"
                        onClick={handleTestConnection}
                        disabled={testResult === "testing"}
                      >
                        {testResult === "testing" ? t("welcome.testing") : t("welcome.testConnection")}
                      </button>
                      {testResult === "ok" && <span className="provider-status ready">{t("welcome.connSuccess")}</span>}
                      {testResult === "fail" && <span className="provider-status error">{t("welcome.connFail")}</span>}
                    </div>
                  )}
                </>
              )}
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(2)}>
                {t("welcome.continue")}
              </button>
              <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 8, textAlign: "center" }}>
                {t("welcome.canSkip")}
              </p>
            </div>
          </div>
        );

      case 2:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              {t("welcome.privacyTitle")}
            </p>
            <div className="welcome-card">
              <p style={{ fontSize: 14, lineHeight: 1.8, color: "var(--text-secondary)" }}>
                {t("welcome.privacyBody")}
              </p>
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={handleComplete} disabled={submitting}>
                {submitting ? t("welcome.submitting") : t("welcome.beginJourney")}
              </button>
              {error && <p className="welcome-error">{error}</p>}
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  const enterClass = direction === "forward" ? "welcome-enter-right" : "welcome-enter-left";
  const exitClass = direction === "forward" ? "welcome-exit-left" : "welcome-exit-right";

  return (
    <div className="welcome-container">
      <div className={`welcome-content ${animating ? exitClass : enterClass}`} key={step}>
        {renderStep()}
      </div>

      <div className="welcome-dots">
        {Array.from({ length: TOTAL_STEPS }, (_, i) => (
          <div
            key={i}
            className={`welcome-dot ${i < step ? "done" : i === step ? "active" : ""}`}
          />
        ))}
      </div>
    </div>
  );
}

export default Welcome;
