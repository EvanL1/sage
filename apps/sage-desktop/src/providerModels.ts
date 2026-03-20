/** Model options for each provider (label, value) */
export const PROVIDER_MODELS: Record<string, { label: string; value: string }[]> = {
  "claude-cli": [
    { label: "Sonnet 4.6 (recommended)", value: "claude-sonnet-4-6" },
    { label: "Sonnet 4.6 (max effort)", value: "claude-sonnet-4-6:max" },
    { label: "Sonnet 4.6 (high effort)", value: "claude-sonnet-4-6:high" },
    { label: "Opus 4.6", value: "claude-opus-4-6" },
    { label: "Opus 4.6 (max effort)", value: "claude-opus-4-6:max" },
    { label: "Haiku 4.5", value: "claude-haiku-4-5-20251001" },
    { label: "Sonnet 4.5", value: "claude-sonnet-4-5-20250514" },
  ],
  "anthropic-api": [
    { label: "Sonnet 4.6 (recommended)", value: "claude-sonnet-4-6-20250627" },
    { label: "Opus 4.6", value: "claude-opus-4-6-20250627" },
    { label: "Haiku 4.5", value: "claude-haiku-4-5-20251001" },
    { label: "Sonnet 4.5", value: "claude-sonnet-4-5-20250514" },
    { label: "Opus 4", value: "claude-opus-4-20250514" },
    { label: "Sonnet 4", value: "claude-sonnet-4-20250514" },
  ],
  "openai-api": [
    { label: "GPT-5.4 (recommended)", value: "gpt-5.4" },
    { label: "GPT-5.4 + xhigh thinking", value: "gpt-5.4:xhigh" },
    { label: "GPT-5.4 + high thinking", value: "gpt-5.4:high" },
    { label: "GPT-5.4 Mini", value: "gpt-5.4-mini" },
    { label: "GPT-5.4 Nano", value: "gpt-5.4-nano" },
    { label: "GPT-5", value: "gpt-5" },
    { label: "GPT-5 Mini", value: "gpt-5-mini" },
    { label: "GPT-4.1", value: "gpt-4.1" },
    { label: "GPT-4.1 Mini", value: "gpt-4.1-mini" },
    { label: "o3 (high)", value: "o3:high" },
    { label: "o3 (medium)", value: "o3:medium" },
    { label: "o3 (low)", value: "o3:low" },
    { label: "o3 Pro", value: "o3-pro" },
    { label: "o4-mini (high)", value: "o4-mini:high" },
    { label: "o4-mini (medium)", value: "o4-mini:medium" },
    { label: "o4-mini (low)", value: "o4-mini:low" },
  ],
  "codex-cli": [
    { label: "GPT-5.4 (recommended)", value: "gpt-5.4" },
    { label: "GPT-5.4 + xhigh thinking", value: "gpt-5.4:xhigh" },
    { label: "GPT-5.4 + high thinking", value: "gpt-5.4:high" },
    { label: "GPT-5.4 Mini", value: "gpt-5.4-mini" },
    { label: "GPT-5.3 Codex", value: "gpt-5.3-codex" },
    { label: "GPT-5.2 Codex", value: "gpt-5.2-codex" },
    { label: "GPT-5.1 Codex Max (xhigh)", value: "gpt-5.1-codex-max:xhigh" },
    { label: "GPT-5.1 Codex", value: "gpt-5.1-codex" },
    { label: "GPT-5 Codex", value: "gpt-5-codex" },
    { label: "o3 (high)", value: "o3:high" },
    { label: "o3 (medium)", value: "o3:medium" },
    { label: "o3 (low)", value: "o3:low" },
    { label: "o4-mini (high)", value: "o4-mini:high" },
    { label: "o4-mini (medium)", value: "o4-mini:medium" },
    { label: "o4-mini (low)", value: "o4-mini:low" },
    { label: "GPT-4.1", value: "gpt-4.1" },
  ],
  "gemini-cli": [
    { label: "Gemini 3.1 Pro (recommended)", value: "gemini-3.1-pro" },
    { label: "Gemini 3 Pro", value: "gemini-3-pro" },
    { label: "Gemini 3 Flash", value: "gemini-3-flash" },
    { label: "Gemini 2.5 Pro", value: "gemini-2.5-pro" },
    { label: "Gemini 2.5 Flash", value: "gemini-2.5-flash" },
  ],
  "cursor-cli": [
    { label: "Auto", value: "auto" },
    { label: "Opus 4.6 Thinking (default)", value: "opus-4.6-thinking" },
    { label: "Sonnet 4.6 Thinking", value: "sonnet-4.6-thinking" },
    { label: "Sonnet 4.6", value: "sonnet-4.6" },
    { label: "Opus 4.6", value: "opus-4.6" },
    { label: "Opus 4.5 Thinking", value: "opus-4.5-thinking" },
    { label: "Sonnet 4.5", value: "sonnet-4.5" },
    { label: "GPT-5.4 xhigh", value: "gpt-5.4-xhigh" },
    { label: "GPT-5.4 high", value: "gpt-5.4-high" },
    { label: "GPT-5.4", value: "gpt-5.4-medium" },
    { label: "GPT-5.3 Codex xhigh", value: "gpt-5.3-codex-xhigh" },
    { label: "GPT-5.3 Codex high", value: "gpt-5.3-codex-high" },
    { label: "GPT-5.3 Codex", value: "gpt-5.3-codex" },
    { label: "GPT-5.2 Codex xhigh", value: "gpt-5.2-codex-xhigh" },
    { label: "GPT-5.2 Codex", value: "gpt-5.2-codex" },
    { label: "Gemini 3.1 Pro", value: "gemini-3.1-pro" },
    { label: "Gemini 3 Pro", value: "gemini-3-pro" },
    { label: "Gemini 3 Flash", value: "gemini-3-flash" },
    { label: "Grok", value: "grok" },
    { label: "Kimi K2.5", value: "kimi-k2.5" },
    { label: "Composer 1.5", value: "composer-1.5" },
  ],
  "deepseek-api": [
    { label: "DeepSeek Chat (recommended)", value: "deepseek-chat" },
    { label: "DeepSeek Reasoner", value: "deepseek-reasoner" },
  ],
};

/** Get short display name for a model (strip suffix tags) */
export function getModelShortName(providerId: string, modelValue: string): string {
  const models = PROVIDER_MODELS[providerId];
  if (models) {
    const found = models.find(m => m.value === modelValue);
    if (found) return found.label.replace(/ \(recommended\)| \(default\)/, "");
  }
  return modelValue || "Default";
}
