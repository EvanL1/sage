import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import Welcome from "./Welcome";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe("Welcome", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("loads provider options on the setup screen and tests the selected provider", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "discover_providers") {
        return Promise.resolve([
          {
            id: "openai-api",
            display_name: "OpenAI API",
            kind: "HttpApi",
            status: "NeedsApiKey",
            priority: 2,
          },
          {
            id: "anthropic-api",
            display_name: "Anthropic API",
            kind: "HttpApi",
            status: "NeedsApiKey",
            priority: 1,
          },
        ]);
      }
      if (command === "save_provider_config") {
        return Promise.resolve();
      }
      if (command === "test_provider") {
        return Promise.resolve({ success: true });
      }
      return Promise.resolve();
    });

    const user = userEvent.setup();

    render(
      <MemoryRouter>
        <Welcome />
      </MemoryRouter>,
    );

    await user.click(screen.getByRole("button", { name: "Get started" }));

    await user.type(await screen.findByPlaceholderText("Your name"), "Alex");
    await user.click(await screen.findByRole("button", { name: "Continue" }));

    await user.click(await screen.findByRole("button", { name: "Skip assessment →" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("discover_providers");
    });

    expect(await screen.findByText("AI Provider")).toBeInTheDocument();

    await user.selectOptions(screen.getByRole("combobox"), "openai-api");
    await user.type(screen.getByPlaceholderText("sk-..."), "sk-test");
    await user.click(screen.getByRole("button", { name: "Test connection" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("save_provider_config", {
        config: {
          provider_id: "openai-api",
          api_key: "sk-test",
          model: null,
          base_url: null,
          enabled: true,
        },
      });
      expect(invokeMock).toHaveBeenCalledWith("test_provider", {
        providerId: "openai-api",
      });
    });

    expect(screen.getByText("Connection successful")).toBeInTheDocument();
  });
});
