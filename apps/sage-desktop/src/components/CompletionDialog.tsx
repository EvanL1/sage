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

interface VerificationItem { q: string; options?: string[]; }
interface QAnswer { chips: string[]; text: string; }

const getFallbackDone = (t: ReturnType<typeof createT>): VerificationItem[] => [
  { q: t("fallback.doneQ"), options: [t("fallback.doneAsPlanned"), t("fallback.donePartially"), t("fallback.doneDelegated"), t("fallback.doneDifferent")] },
];

function parseVerification(raw: string | null): VerificationItem[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    // 支持两种格式：{ done: [...] } 或直接 [...]
    const items = parsed.done ?? (Array.isArray(parsed) ? parsed : null);
    if (!Array.isArray(items) || items.length === 0) return null;
    return items.map((item: { q: string; options?: string[] }) => ({
      q: item.q, options: item.options ?? [],
    }));
  } catch { return null; }
}

export default function CompletionDialog({ task, onClose, onRefresh }: {
  task: CompletionTaskItem;
  onClose: () => void;
  onRefresh: () => void;
}) {
  const { t } = useLang();
  const questions = parseVerification(task.verification) ?? getFallbackDone(t);
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
    invoke("complete_task", { taskId: task.id, status: "done", outcome })
      .then(() => { onRefresh(); onClose(); })
      .catch(e => console.error("complete_task:", e));
  };

  return createPortal(
    <div className="completion-dialog-overlay" onClick={onClose}>
      <div className="completion-dialog" onClick={e => e.stopPropagation()}>
        <button className="completion-dialog-close" onClick={onClose}>&times;</button>
        <div className="cd-task-content">{task.content}</div>
        <div className="cd-task-meta">
          {task.due_date && <span className="cd-meta-item">{t("completion.due")} {task.due_date}</span>}
          {task.priority !== "normal" && <span className="cd-meta-item cd-priority">{task.priority}</span>}
          <span className="cd-meta-item">{t("completion.source")} {task.source}</span>
        </div>
        <div className="completion-dialog-status done">
          <span className="completion-dialog-status-dot" />
          {t("completion.completing")}
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
