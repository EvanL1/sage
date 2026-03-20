import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import FeedbackButtons, { actionToFeedback } from "./FeedbackButtons";

describe("actionToFeedback", () => {
  it("maps persisted action strings back to UI state", () => {
    expect(actionToFeedback("useful")).toBe("Useful");
    expect(actionToFeedback("not_useful")).toBe("NotUseful");
    expect(actionToFeedback("never:too noisy")).toEqual({ NeverDoThis: "too noisy" });
    expect(actionToFeedback("correction:use weekly report")).toEqual({
      Correction: "use weekly report",
    });
  });
});

describe("FeedbackButtons", () => {
  it("renders the saved feedback badge when feedback already exists", () => {
    render(
      <FeedbackButtons
        suggestionId={7}
        feedback="Useful"
        onSubmit={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(screen.queryByText("✓ Helpful")).not.toBeNull();
    expect(screen.queryByText("✗ Not helpful")).toBeNull();
  });

  it("submits quick feedback actions", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(<FeedbackButtons suggestionId={42} feedback={null} onSubmit={onSubmit} />);

    await user.click(screen.getByRole("button", { name: "✓ Helpful" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith(42, "useful");
  });

  it("submits correction feedback through the modal", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(<FeedbackButtons suggestionId={5} feedback={null} onSubmit={onSubmit} />);

    await user.click(screen.getByRole("button", { name: "✏️ Correct" }));
    await user.type(
      screen.getByPlaceholderText("The correct approach is..."),
      "Use the weekly report instead.",
    );
    await user.click(screen.getByRole("button", { name: "Submit" }));

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith(5, "correction:Use the weekly report instead.");
  });
});
