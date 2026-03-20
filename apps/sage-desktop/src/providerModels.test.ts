import { describe, expect, it } from "vitest";

import { getModelShortName } from "./providerModels";

describe("getModelShortName", () => {
  it("strips recommendation markers from known models", () => {
    expect(getModelShortName("openai-api", "gpt-5.4")).toBe("GPT-5.4");
    expect(getModelShortName("cursor-cli", "opus-4.6-thinking")).toBe("Opus 4.6 Thinking");
  });

  it("falls back to the raw model id when unknown", () => {
    expect(getModelShortName("openai-api", "custom-model")).toBe("custom-model");
  });

  it("returns Default when no model is configured", () => {
    expect(getModelShortName("openai-api", "")).toBe("Default");
    expect(getModelShortName("unknown-provider", "")).toBe("Default");
  });
});
