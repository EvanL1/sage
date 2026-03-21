import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PROVIDER_MODELS, getModelShortName } from "../providerModels";
import type { ProviderInfo, ProviderConfig } from "../types";

interface ActiveSelection {
  providerId: string;
  providerName: string;
  model: string;
}

function ModelSwitcher() {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState<ActiveSelection | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [configs, setConfigs] = useState<ProviderConfig[]>([]);
  const ref = useRef<HTMLDivElement>(null);

  const load = async () => {
    try {
      const [infos, cfgs] = await Promise.all([
        invoke<ProviderInfo[]>("discover_providers"),
        invoke<ProviderConfig[]>("get_provider_configs"),
      ]);
      setProviders(infos);
      setConfigs(cfgs);

      // Find active provider: highest priority + Ready
      const sorted = [...infos]
        .filter((p) => p.status === "Ready")
        .sort((a, b) => a.priority - b.priority);
      if (sorted.length > 0) {
        const p = sorted[0];
        const cfg = cfgs.find((c) => c.provider_id === p.id);
        setActive({
          providerId: p.id,
          providerName: p.display_name,
          model: cfg?.model || "",
        });
      }
    } catch {
      /* ignore */
    }
  };

  useEffect(() => { load(); }, []);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  const selectModel = async (providerId: string, providerName: string, modelValue: string) => {
    const existing = configs.find((c) => c.provider_id === providerId);
    const config: ProviderConfig = {
      provider_id: providerId,
      api_key: existing?.api_key ?? null,
      model: modelValue,
      base_url: existing?.base_url ?? null,
      enabled: existing?.enabled ?? true,
      priority: existing?.priority ?? null,
    };
    try {
      await invoke("save_provider_config", { config });
      // Move this provider to top priority
      const otherIds = providers
        .filter((p) => p.id !== providerId)
        .sort((a, b) => a.priority - b.priority)
        .map((p) => p.id);
      await invoke("save_provider_priorities", { orderedIds: [providerId, ...otherIds] });
      setActive({ providerId, providerName, model: modelValue });
      setConfigs((prev) =>
        prev.map((c) => (c.provider_id === providerId ? { ...c, model: modelValue } : c))
      );
    } catch {
      /* ignore */
    }
    setOpen(false);
  };

  const readyProviders = providers
    .filter((p) => p.status === "Ready")
    .sort((a, b) => a.priority - b.priority);

  const displayLabel = active
    ? `${active.providerName} · ${getModelShortName(active.providerId, active.model)}`
    : "No provider";

  return (
    <div className="model-switcher" ref={ref}>
      <button className="model-switcher-btn" onClick={() => setOpen(!open)}>
        <span className={`model-switcher-dot ${active ? "online" : "offline"}`} />
        <span className="model-switcher-label">{displayLabel}</span>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {open && (
        <div className="model-switcher-dropdown">
          {readyProviders.map((p) => {
            const models = PROVIDER_MODELS[p.id] || [];
            const cfg = configs.find((c) => c.provider_id === p.id);
            const currentModel = cfg?.model || "";
            return (
              <div key={p.id} className="model-switcher-group">
                <div className="model-switcher-group-title">
                  {p.display_name}
                  {active?.providerId === p.id && <span className="model-switcher-active-badge">active</span>}
                </div>
                <div className="model-switcher-models">
                  {models.map((m) => (
                    <button
                      key={m.value}
                      className={`model-switcher-model${m.value === currentModel && active?.providerId === p.id ? " selected" : ""}`}
                      onClick={() => selectModel(p.id, p.display_name, m.value)}
                    >
                      {m.label.replace(/ \(recommended\)| \(default\)/, "")}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
          {readyProviders.length === 0 && (
            <div className="model-switcher-empty">No providers available</div>
          )}
        </div>
      )}
    </div>
  );
}

export default ModelSwitcher;
