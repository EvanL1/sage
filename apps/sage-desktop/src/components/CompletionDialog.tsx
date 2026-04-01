import { useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { useLang } from "../LangContext";
import { createT } from "../i18n";

export interface CompletionTaskItem {
  id: number; content: string; priority: string;
  due_date: string | null; source: string; created_at: string;
  verification: string | null;
}

export interface CompletionMeta {
  showCreatedAt?: boolean;
}

export interface VerificationItem { q: string; options?: string[]; }
export interface QAnswer { chips: string[]; text: string; }

export const getFallbackDone = (t: ReturnType<typeof createT>): VerificationItem[] => [
  { q: t("fallback.doneQ"), options: [t("fallback.doneAsPlanned"), t("fallback.donePartially"), t("fallback.doneDelegated"), t("fallback.doneDifferent")] },
];

export const getFallbackCancel = (t: ReturnType<typeof createT>): VerificationItem[] => [
  { q: t("fallback.cancelQ"), options: [t("fallback.cancelIrrelevant"), t("fallback.cancelBlocked"), t("fallback.cancelDeprioritized"), t("fallback.cancelMerged"), t("fallback.cancelSomeoneElse")] },
];

export function parseVerification(raw: string | null, status: "done" | "cancelled" = "done"): VerificationItem[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    // 支持两种格式：{ done: [...], cancelled: [...] } 或直接 [...]
    const items = status === "done"
      ? (parsed.done ?? (Array.isArray(parsed) ? parsed : null))
      : (parsed.cancelled ?? null);
    if (!Array.isArray(items) || items.length === 0) return null;
    return items.map((item: { q: string; options?: string[] }) => ({
      q: item.q, options: item.options ?? [],
    }));
  } catch { return null; }
}

export default function CompletionDialog({ task, onClose, onRefresh, status = "done", meta }: {
  task: CompletionTaskItem;
  onClose: () => void;
  onRefresh: () => void;
  status?: "done" | "cancelled";
  meta?: CompletionMeta;
}) {
  const { t } = useLang();
  const isDone = status === "done";
  const questions = parseVerification(task.verification, status)
    ?? (isDone ? getFallbackDone(t) : getFallbackCancel(t));
  const [answers, setAnswers] = useState<QAnswer[]>(() => questions.map(() => ({ chips: [], text: "" })));
  const [notes, setNotes] = useState("");

  const toggleChip = (qi: number, chip: string) => {
    setAnswers(prev => {
      const next = [...prev];
      const a = { ...next[qi] };
      a.chips = a.chips.includes(chip) ? a.chips.filter(c => c !== chip) : [...a.chips, chip];
      next[qi] = a;
      return next;
    });
  };

  const setQText = (qi: number, text: string) => {
    setAnswers(prev => {
      const next = [...prev];
      next[qi] = { ...next[qi], text };
      return next;
    });
  };

  const save = (skip: boolean) => {
    let outcome: string | null = null;
    if (!skip) {
      const parts: string[] = [];
      questions.forEach((q, i) => {
        const a = answers[i];
        const answerParts = [...a.chips];
        if (a.text.trim()) answerParts.push(a.text.trim());
        if (answerParts.length) parts.push(`${q.q}: ${answerParts.join(", ")}`);
      });
      if (notes.trim()) parts.push(notes.trim());
      if (parts.length) outcome = parts.join(" | ");
    }
    invoke("complete_task", { taskId: task.id, status, outcome })
      .then(() => { onRefresh(); onClose(); })
      .catch(e => console.error("complete_task:", e));
  };

  return createPortal(
    <div className="completion-dialog-overlay" onClick={onClose}>
      <div className="completion-dialog" onClick={e => e.stopPropagation()}>
        <button className="completion-dialog-close" onClick={onClose} title={t("close")}>&times;</button>
        <div className="cd-task-content">{task.content}</div>
        <div className="cd-task-meta">
          {task.due_date && <span className="cd-meta-item">{t("completion.due")}: {task.due_date}</span>}
          {task.priority !== "normal" && <span className="cd-meta-item cd-priority">{task.priority}</span>}
          <span className="cd-meta-item">{t("completion.source")}: {task.source}</span>
          {meta?.showCreatedAt && <span className="cd-meta-item">{t("completion.created")}: {task.created_at.slice(0, 10)}</span>}
        </div>
        <div className={`completion-dialog-status ${status}`}>
          <span className="completion-dialog-status-dot" />
          {isDone ? t("completion.completing") : t("completion.cancelling")}
        </div>
        <div className="cd-questions">
          {questions.map((q, qi) => (
            <div key={qi} className="cd-q-item">
              <div className="cd-q-label">{q.q}</div>
              {q.options && q.options.length > 0 && (
                <div className="cd-q-chips">
                  {q.options.map(opt => (
                    <button key={opt}
                      className={`cd-q-chip${answers[qi]?.chips.includes(opt) ? " active" : ""}`}
                      onClick={() => toggleChip(qi, opt)}>{opt}</button>
                  ))}
                </div>
              )}
              <input className="cd-q-text" type="text" placeholder={t("completion.typeAnswer")}
                value={answers[qi]?.text ?? ""}
                onChange={e => setQText(qi, e.target.value)}
                onKeyDown={e => { if (e.key === "Escape") onClose(); }} />
            </div>
          ))}
        </div>
        <textarea className="completion-dialog-textarea" placeholder={t("completion.notes")}
          value={notes} onChange={e => setNotes(e.target.value)} rows={2}
          onKeyDown={e => { if (e.key === "Escape") onClose(); }} />
        <div className="completion-dialog-actions">
          <button className="completion-dialog-skip" onClick={() => save(true)}>{t("skip")}</button>
          <button className="completion-dialog-save" onClick={() => save(false)}>{t("save")}</button>
        </div>
      </div>
    </div>,
    document.body
  );
}
