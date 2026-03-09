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
  if (fb === "Useful") return "✓ 有用";
  if (fb === "NotUseful") return "✗ 没用";
  if (typeof fb === "object" && "NeverDoThis" in fb) return "⛔ 已标记";
  return "✏️ 已修正";
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
          ✓ 有用
        </button>
        <button className="fb-btn" onClick={() => submit("not_useful")} disabled={loading}>
          ✗ 没用
        </button>
        <button className="fb-btn" onClick={() => setModal("never")} disabled={loading}>
          ⛔ 永远不要
        </button>
        <button className="fb-btn" onClick={() => setModal("correction")} disabled={loading}>
          ✏️ 修正
        </button>
      </div>

      {modal && (
        <div className="modal-overlay" onClick={() => setModal(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>{modal === "never" ? "为什么不要这类建议？" : "请提供修正内容"}</h3>
            <textarea
              className="form-textarea"
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              placeholder={modal === "never" ? "告诉 Sage 原因..." : "正确的做法是..."}
              autoFocus
            />
            <div className="modal-actions">
              <button className="btn btn-secondary" onClick={() => setModal(null)}>
                取消
              </button>
              <button
                className="btn btn-primary"
                onClick={submitModal}
                disabled={!inputText.trim()}
              >
                提交
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
