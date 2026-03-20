import { useState } from "react";

export type FeedbackValue =
  | "Useful"
  | "NotUseful"
  | { NeverDoThis: string }
  | { Correction: string };

interface Props {
  suggestionId: number;
  feedback: FeedbackValue | null;
  onSubmit: (id: number, action: string) => Promise<void>;
}

function feedbackLabel(fb: FeedbackValue): string {
  if (fb === "Useful") return "✓ Helpful";
  if (fb === "NotUseful") return "✗ Not helpful";
  if (typeof fb === "object" && "NeverDoThis" in fb) return "⛔ Flagged";
  return "✏️ Corrected";
}

function feedbackType(fb: FeedbackValue): "positive" | "negative" {
  return fb === "Useful" ? "positive" : "negative";
}

export function actionToFeedback(action: string): FeedbackValue {
  if (action === "useful") return "Useful";
  if (action === "not_useful") return "NotUseful";
  if (action.startsWith("never:")) return { NeverDoThis: action.slice(6) };
  return { Correction: action.slice(11) };
}

export default function FeedbackButtons({ suggestionId, feedback, onSubmit }: Props) {
  const [modal, setModal] = useState<"never" | "correction" | null>(null);
  const [inputText, setInputText] = useState("");
  const [loading, setLoading] = useState(false);

  if (feedback) {
    return (
      <div className="suggestion-footer">
        <span className={`feedback-badge ${feedbackType(feedback)}`}>
          {feedbackLabel(feedback)}
        </span>
      </div>
    );
  }

  const submit = async (action: string) => {
    setLoading(true);
    try {
      await onSubmit(suggestionId, action);
    } finally {
      setLoading(false);
    }
  };

  const submitModal = async () => {
    if (!inputText.trim()) return;
    const action = modal === "never"
      ? `never:${inputText}`
      : `correction:${inputText}`;
    await submit(action);
    setModal(null);
    setInputText("");
  };

  return (
    <>
      <div className="suggestion-footer">
        <button className="fb-btn" onClick={() => submit("useful")} disabled={loading}>
          ✓ Helpful
        </button>
        <button className="fb-btn" onClick={() => submit("not_useful")} disabled={loading}>
          ✗ Not helpful
        </button>
        <button className="fb-btn" onClick={() => setModal("never")} disabled={loading}>
          ⛔ Don't do this
        </button>
        <button className="fb-btn" onClick={() => setModal("correction")} disabled={loading}>
          ✏️ Correct
        </button>
      </div>

      {modal && (
        <div className="modal-overlay" onClick={() => setModal(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>{modal === "never" ? "Why don't you want this type of suggestion?" : "Please provide a correction"}</h3>
            <textarea
              className="form-textarea"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              placeholder={modal === "never" ? "Tell Sage why..." : "The correct approach is..."}
              autoFocus
            />
            <div className="modal-actions">
              <button className="btn btn-secondary" onClick={() => setModal(null)}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                onClick={submitModal}
                disabled={!inputText.trim()}
              >
                Submit
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
