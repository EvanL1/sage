import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import { useLang } from "../LangContext";
import { TaskItem } from "../types";
import { ReportData } from "../layouts/types";
import CompletionDialog from "../components/CompletionDialog";

interface TaskSignal {
  id: number;
  signalType: "completion" | "cancellation" | "new_task";
  taskId: number | null;
  title: string;
  evidence: string;
  suggestedOutcome: string | null;
  status: string;
  createdAt: string;
}

interface CompletionTarget {
  task: TaskItem;
  status: "done" | "cancelled";
}
type FilterKey = "open" | "done" | "cancelled";

/* ─── Calendar helpers ─── */
function buildCalendarCells(year: number, month: number): (number | null)[] {
  const firstDay = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const startOffset = firstDay === 0 ? 6 : firstDay - 1;
  const cells: (number | null)[] = [];
  for (let i = 0; i < startOffset; i++) cells.push(null);
  for (let d = 1; d <= daysInMonth; d++) cells.push(d);
  while (cells.length % 7 !== 0) cells.push(null);
  return cells;
}

function formatDateStr(year: number, month: number, day: number): string {
  return `${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/* ─── Calendar ─── */
function MiniCalendar({ selected, onSelect, taskDates }: {
  selected: string | null; onSelect: (d: string | null) => void; taskDates: Map<string, number>;
}) {
  const [viewDate, setViewDate] = useState(() => new Date());
  const today = new Date().toISOString().slice(0, 10);
  const year = viewDate.getFullYear(), month = viewDate.getMonth();
  const monthLabel = viewDate.toLocaleDateString("en-US", { year: "numeric", month: "long" });
  const cells = buildCalendarCells(year, month);

  return (
    <div className="tcal">
      <div className="tcal-header">
        <button className="tcal-nav" onClick={() => setViewDate(new Date(year, month - 1, 1))}>&lt;</button>
        <button className="tcal-month" onClick={() => { setViewDate(new Date()); onSelect(null); }}>{monthLabel}</button>
        <button className="tcal-nav" onClick={() => setViewDate(new Date(year, month + 1, 1))}>&gt;</button>
      </div>
      <div className="tcal-weekdays">
        {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(d => <span key={d}>{d}</span>)}
      </div>
      <div className="tcal-grid">
        {cells.map((day, i) => {
          if (day === null) return <span key={i} className="tcal-cell empty" />;
          const ds = formatDateStr(year, month, day);
          const count = taskDates.get(ds) ?? 0;
          return (
            <button key={i} className={`tcal-cell${ds === today ? " today" : ""}${ds === selected ? " selected" : ""}${count > 0 ? " has-tasks" : ""}`}
              onClick={() => onSelect(ds === selected ? null : ds)}>
              {day}{count > 0 && <span className="tcal-dot">{count > 3 ? "●●" : "●".repeat(count)}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/* ─── Helpers ─── */
function getWeekEnd(today: string): string {
  const d = new Date(today); d.setDate(d.getDate() + (7 - (d.getDay() || 7)));
  return d.toISOString().slice(0, 10);
}

function categorize(tasks: TaskItem[], today: string, weekEnd: string) {
  const overdue: TaskItem[] = [], todayT: TaskItem[] = [], week: TaskItem[] = [],
    later: TaskItem[] = [], noDate: TaskItem[] = [];
  for (const t of tasks) {
    if (!t.due_date) noDate.push(t);
    else if (t.due_date < today) overdue.push(t);
    else if (t.due_date === today) todayT.push(t);
    else if (t.due_date <= weekEnd) week.push(t);
    else later.push(t);
  }
  return { overdue, todayT, week, later, noDate };
}

/* ─── Inline Date Picker ─── */
function DatePicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useLang();
  const sel = value || "";
  const base = sel ? new Date(sel) : new Date();
  const [viewDate, setViewDate] = useState(base);
  const [showCal, setShowCal] = useState(false);
  const today = new Date().toISOString().slice(0, 10);
  const year = viewDate.getFullYear(), month = viewDate.getMonth();
  const monthLabel = viewDate.toLocaleDateString("en-US", { year: "numeric", month: "long" });
  const cells = buildCalendarCells(year, month);

  const quickDates = [
    { label: t("datePicker.today"), d: today },
    { label: t("datePicker.tomorrow"), d: (() => { const x = new Date(); x.setDate(x.getDate() + 1); return x.toISOString().slice(0, 10); })() },
    { label: t("datePicker.nextMon"), d: (() => { const x = new Date(); x.setDate(x.getDate() + ((8 - x.getDay()) % 7 || 7)); return x.toISOString().slice(0, 10); })() },
    { label: t("datePicker.noDate"), d: "" },
  ];

  const isQuickDate = quickDates.some(q => q.d === sel);
  const pickLabel = sel && !isQuickDate ? sel : t("datePicker.pickDate");

  const handleQuick = (d: string) => { onChange(d); setShowCal(false); };
  const handleCalPick = (ds: string) => { onChange(ds); setShowCal(false); };

  return (
    <div className="dp-container">
      <div className="dp-quick">
        {quickDates.map(q => (
          <button key={q.label} className={`dp-quick-btn${sel === q.d ? " active" : ""}`}
            onClick={() => handleQuick(q.d)}>{q.label}</button>
        ))}
        <button className={`dp-quick-btn${showCal || (sel && !isQuickDate) ? " active" : ""}`}
          onClick={() => setShowCal(!showCal)}>{pickLabel}</button>
      </div>
      {showCal && (
        <div className="dp-cal dp-cal-compact">
          <div className="dp-cal-header">
            <button className="dp-cal-nav" onClick={() => setViewDate(new Date(year, month - 1, 1))}>&lt;</button>
            <span className="dp-cal-month">{monthLabel}</span>
            <button className="dp-cal-nav" onClick={() => setViewDate(new Date(year, month + 1, 1))}>&gt;</button>
          </div>
          <div className="dp-cal-weekdays">
            {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(d => <span key={d}>{d}</span>)}
          </div>
          <div className="dp-cal-grid">
            {cells.map((day, i) => {
              if (day === null) return <span key={i} className="dp-cal-cell empty" />;
              const ds = formatDateStr(year, month, day);
              return (
                <button key={i} className={`dp-cal-cell${ds === today ? " today" : ""}${ds === sel ? " selected" : ""}`}
                  onClick={() => handleCalPick(ds)}>{day}</button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

/* ─── Add/Edit Panel ─── */
function TaskForm({ initial, onSave, onCancel }: {
  initial?: { content: string; description: string; dueDate: string; priority: string };
  onSave: (content: string, dueDate: string | null, priority: string, description: string | null) => void;
  onCancel: () => void;
}) {
  const { t } = useLang();
  const [content, setContent] = useState(initial?.content ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [dueDate, setDueDate] = useState(initial?.dueDate ?? "");
  const [priority, setPriority] = useState(initial?.priority ?? "normal");

  const submit = () => {
    if (!content.trim()) return;
    onSave(content.trim(), dueDate || null, priority, description.trim() || null);
  };

  return (
    <div className="task-form-overlay" onClick={onCancel}>
      <div className="task-form" onClick={e => e.stopPropagation()}>
        <button className="completion-dialog-close" onClick={onCancel} title={t("close")}>&times;</button>
        <div className="task-form-title">{initial ? t("taskForm.editTask") : t("taskForm.newTask")}</div>
        <input className="task-form-input" placeholder={t("taskForm.titlePlaceholder")} value={content}
          onChange={e => setContent(e.target.value)} autoFocus
          onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); submit(); } }} />
        <textarea className="task-form-desc" placeholder={t("taskForm.descPlaceholder")} value={description}
          onChange={e => setDescription(e.target.value)} rows={6} />
        <div className="task-form-row">
          <label className="task-form-label">
            {t("taskForm.priority")}
            <div className="tf-priority-chips">
              {["normal", "P0", "P1", "P2"].map(p => (
                <button key={p} className={`tf-chip${priority === p ? " active" : ""}`} onClick={() => setPriority(p)}>
                  {p === "normal" ? t("taskForm.normal") : p}
                </button>
              ))}
            </div>
          </label>
        </div>
        <div className="task-form-label" style={{ marginTop: 12 }}>{t("taskForm.dueDate")}</div>
        <DatePicker value={dueDate} onChange={setDueDate} />
        <div className="task-form-actions">
          <button className="task-form-cancel" onClick={onCancel}>{t("cancel")}</button>
          <button className="task-form-submit" onClick={submit} disabled={!content.trim()}>
            {initial ? t("save") : t("taskForm.addTask")}
          </button>
        </div>
      </div>
    </div>
  );
}

/* ─── Main Page ─── */
export default function Tasks() {
  const { t } = useLang();
  const [allTasks, setAllTasks] = useState<TaskItem[]>([]);
  const [filters, setFilters] = useState<Set<FilterKey>>(new Set(["open"]));
  const [calDate, setCalDate] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [genMsg, setGenMsg] = useState("");
  const [reports, setReports] = useState<Record<string, ReportData>>({});
  const [showForm, setShowForm] = useState(false);
  const [showFormWithContent, setShowFormWithContent] = useState<string | null>(null);
  const [editingTask, setEditingTask] = useState<TaskItem | null>(null);
  const [selectedTask, setSelectedTask] = useState<TaskItem | null>(null);
  const [nlInput, setNlInput] = useState("");
  const [nlBusy, setNlBusy] = useState(false);
  const [completionTarget, setCompletionTarget] = useState<CompletionTarget | null>(null);
  const [signals, setSignals] = useState<TaskSignal[]>([]);

  const today = new Date().toISOString().slice(0, 10);
  const weekEnd = getWeekEnd(today);

  const verifyingRef = useRef(new Set<number>());
  const load = useCallback(() => {
    invoke<TaskItem[]>("list_tasks", { status: null, limit: 200 })
      .then(tasks => {
        setAllTasks(tasks);
        for (const task of tasks) {
          if (task.status === "open" && !task.verification && !verifyingRef.current.has(task.id)) {
            verifyingRef.current.add(task.id);
            invoke("generate_verification", { taskId: task.id })
              .then(() => load())
              .catch(() => {});
          }
        }
      }).catch(e => console.error("list_tasks:", e));
  }, []);

  const loadSignals = useCallback(() => {
    invoke<TaskSignal[]>("get_task_signals")
      .then(s => setSignals(Array.isArray(s) ? s : []))
      .catch(e => console.error("get_task_signals:", e));
  }, []);

  useEffect(() => { load(); loadSignals(); }, [load, loadSignals]);
  useEffect(() => {
    invoke<Record<string, ReportData>>("get_latest_reports").then(setReports).catch(() => {});
  }, []);

  const handleCalSelect = (date: string | null) => { setCalDate(date); };

  const createTask = (content: string, dueDate: string | null, priority: string, description: string | null) => {
    invoke<number>("create_task", { content, source: null, sourceId: null, priority, dueDate, description })
      .then((taskId) => {
        setShowForm(false);
        load();
        invoke("generate_verification", { taskId }).catch(() => {});
      })
      .catch(e => console.error("create_task:", e));
  };

  const setStatus = (id: number, newStatus: string) => {
    invoke("update_task_status", { taskId: id, status: newStatus })
      .then(() => load())
      .catch(e => console.error("update_task_status:", e));
  };

  const openCompletion = (task: TaskItem, status: "done" | "cancelled") => {
    setCompletionTarget({ task, status });
  };

  const remove = (id: number) => {
    invoke("delete_task", { taskId: id }).then(() => load())
      .catch(e => console.error("delete_task:", e));
  };

  const dismissSignal = (signalId: number) => {
    invoke("dismiss_signal", { signalId }).then(loadSignals).catch(e => console.error("dismiss_signal:", e));
  };

  const [pendingSignalId, setPendingSignalId] = useState<number | null>(null);

  const confirmSignal = (signal: TaskSignal) => {
    if (signal.taskId !== null) {
      const relatedTask = allTasks.find(task => task.id === signal.taskId);
      if (relatedTask) {
        const status = signal.signalType === "completion" ? "done" : "cancelled";
        const taskWithOutcome = signal.suggestedOutcome
          ? { ...relatedTask, outcome: signal.suggestedOutcome }
          : relatedTask;
        setPendingSignalId(signal.id);
        setCompletionTarget({ task: taskWithOutcome, status });
      }
    }
  };

  const signalMap = useMemo(() => {
    const m = new Map<number, TaskSignal>();
    for (const s of signals) {
      if (s.taskId !== null && s.signalType !== "new_task") m.set(s.taskId, s);
    }
    return m;
  }, [signals]);

  const newTaskSignals = useMemo(() => signals.filter(s => s.signalType === "new_task"), [signals]);

  const acceptNewTask = async (signal: TaskSignal) => {
    try {
      await invoke<number>("create_task", {
        content: signal.title,
        source: "ai_signal",
        sourceId: null,
        priority: signal.suggestedOutcome === "high" ? "high" : "normal",
        dueDate: null,
        description: signal.evidence,
      });
      await invoke("accept_signal", { signalId: signal.id });
      load();
      loadSignals();
    } catch (e) {
      console.error("acceptNewTask:", e);
    }
  };

  const generateFromReport = (rt: string) => {
    setGenerating(true);
    setGenMsg(`${t("tasks.extracting")} ${rt}...`);
    invoke<{ id: number; content: string }[]>("generate_tasks", { reportType: rt })
      .then(r => {
        const n = Array.isArray(r) ? r.length : 0;
        setGenMsg(n > 0 ? `${n} ${t("tasks.created")}` : t("tasks.noNew"));
        load(); setTimeout(() => setGenMsg(""), 4000);
      })
      .catch(e => { setGenMsg(`${t("tasks.failed")}: ${String(e).slice(0, 80)}`); setTimeout(() => setGenMsg(""), 8000); })
      .finally(() => setGenerating(false));
  };

  const toggleFilter = (f: FilterKey) => {
    setFilters(prev => {
      const next = new Set(prev);
      if (next.has(f)) { next.delete(f); } else { next.add(f); }
      return next.size === 0 ? new Set([f]) : next;
    });
  };
  const filtered = allTasks.filter(task => {
    if (filters.has("cancelled") && (task.status === "cancelled" || task.status === "stale")) return true;
    if (filters.has("done") && task.status === "done") return true;
    if (filters.has("open") && task.status === "open") return true;
    return false;
  });
  const displayed = calDate ? filtered.filter(task => task.due_date === calDate) : filtered;
  const counts = {
    open: allTasks.filter(task => task.status === "open").length,
    done: allTasks.filter(task => task.status === "done").length,
    cancelled: allTasks.filter(task => task.status === "cancelled" || task.status === "stale").length,
  };
  const onlyOpen = filters.size === 1 && filters.has("open");
  const groups = !calDate && onlyOpen ? categorize(displayed, today, weekEnd) : null;
  const taskDates = useMemo(() => {
    const m = new Map<string, number>();
    for (const task of allTasks.filter(task => task.status === "open"))
      if (task.due_date) m.set(task.due_date, (m.get(task.due_date) ?? 0) + 1);
    return m;
  }, [allTasks]);
  const reportTypes = Object.keys(reports);

  const reportLabel = (rt: string) => {
    if (rt === "morning") return t("tasks.amBrief");
    if (rt === "evening") return t("tasks.pmReview");
    if (rt === "weekly") return t("tasks.weekly");
    if (rt === "week_start") return t("tasks.weekStart");
    return rt;
  };

  const filterLabel = (f: FilterKey) => {
    if (f === "open") return `${t("tasks.open")} (${counts.open})`;
    if (f === "done") return `${t("tasks.done")} (${counts.done})`;
    return `${t("tasks.cancelled")} (${counts.cancelled})`;
  };

  return (
    <div className="chat-page">
      <div style={{ flex: 1, display: "flex", gap: 16, padding: "16px 20px", minHeight: 0 }}>
        {/* Left sidebar */}
        <div style={{ width: 220, flexShrink: 0, display: "flex", flexDirection: "column", gap: 12, overflowY: "auto" }}>
          <MiniCalendar selected={calDate} onSelect={handleCalSelect} taskDates={taskDates} />
          {reportTypes.length > 0 && (
            <div className="tasks-ai-box">
              <div className="tasks-ai-label">{t("tasks.extractFrom")}</div>
              <div className="tasks-ai-btns">
                {reportTypes.map(rt => (
                  <button key={rt} className="tasks-ai-btn" onClick={() => generateFromReport(rt)} disabled={generating}>
                    {reportLabel(rt)}
                  </button>
                ))}
              </div>
              {genMsg && <div className={`tasks-ai-msg${genMsg.startsWith(t("tasks.failed")) ? " error" : ""}`}>{genMsg}</div>}
            </div>
          )}
        </div>

        {/* Middle: Task list */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", overflowY: "auto", minHeight: 0 }}>
          <div className="tasks-header">
            <h1 className="tasks-title">
              {calDate ?? t("tasks.title")}
              {calDate && <button className="tasks-clear-date" onClick={() => setCalDate(null)}>{t("tasks.showAll")}</button>}
            </h1>
            <div className="tasks-counts">
              <span className="tasks-count open">{counts.open}</span>
              <span className="tasks-count done">{counts.done}</span>
            </div>
          </div>
          <div style={{ display: "flex", gap: 6, marginBottom: 8, padding: "0 2px" }}>
            <input
              type="text"
              className="form-input"
              placeholder="Describe your task, e.g. &quot;明天下午前完成报告，优先级高&quot;"
              value={nlInput}
              onChange={(e) => setNlInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.nativeEvent.isComposing && nlInput.trim()) {
                  setNlBusy(true);
                  invoke("create_task_natural", { text: nlInput })
                    .then(() => { setNlInput(""); load(); })
                    .catch((err) => console.error("create_task_natural:", err))
                    .finally(() => setNlBusy(false));
                }
              }}
              disabled={nlBusy}
              style={{ flex: 1, fontSize: 13, opacity: nlBusy ? 0.6 : 1 }}
            />
            <button className="tasks-add-fab" onClick={() => setShowForm(true)} title={t("tasks.add")}>+</button>
          </div>

          <div className="tasks-filters">
            {(["open", "done", "cancelled"] as const).map(f => (
              <button key={f} className={`tasks-filter-btn${filters.has(f) ? " active" : ""}`} onClick={() => toggleFilter(f)}>
                {filterLabel(f)}
              </button>
            ))}
          </div>

          {newTaskSignals.length > 0 && (
            <div className="tasks-ai-signals">
              <div className="tasks-ai-signals-header">
                <span className="tasks-ai-signals-icon">✨</span>
                <span>{t("tasks.aiSuggestions")} ({newTaskSignals.length})</span>
              </div>
              {newTaskSignals.map(sig => (
                <div key={sig.id} className="tasks-ai-signal-card">
                  <div className="tasks-ai-signal-content">
                    <div className="tasks-ai-signal-title">{sig.title}</div>
                    {sig.evidence && <div className="tasks-ai-signal-evidence">{sig.evidence}</div>}
                  </div>
                  <div className="tasks-ai-signal-actions">
                    <button className="ir-btn ir-btn-task" onClick={() => acceptNewTask(sig)}>+ {t("report.createTask")}</button>
                    <button className="ir-btn" onClick={() => dismissSignal(sig.id)}>✗</button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {groups ? (
            <div className="tasks-sections">
              <Section label={t("tasks.overdue")} tasks={groups.overdue} accent="var(--error)" onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selectedId={selectedTask?.id ?? null} signalMap={signalMap} />
              <Section label={t("tasks.today")} tasks={groups.todayT} accent="var(--accent)" onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selectedId={selectedTask?.id ?? null} signalMap={signalMap} />
              <Section label={t("tasks.thisWeek")} tasks={groups.week} accent="var(--warning, #eab308)" onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selectedId={selectedTask?.id ?? null} signalMap={signalMap} />
              <Section label={t("tasks.later")} tasks={groups.later} accent="var(--text-secondary)" onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selectedId={selectedTask?.id ?? null} signalMap={signalMap} />
              <Section label={t("tasks.noDate")} tasks={groups.noDate} accent="var(--text-tertiary)" onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selectedId={selectedTask?.id ?? null} signalMap={signalMap} />
            </div>
          ) : (
            <div className="tasks-list">
              {displayed.map(task => <Row key={task.id} t={task} onComplete={openCompletion} onDone={setStatus} onSelect={setSelectedTask} selected={task.id === selectedTask?.id}
                hasSignal={signalMap.has(task.id)} />)}
              {!displayed.length && <div className="tasks-empty">{calDate ? `${calDate} ${t("tasks.noTasks")}` : t("tasks.noTasks")}</div>}
            </div>
          )}
        </div>

        {/* Right: Detail panel */}
        {selectedTask && (
          <TaskDetail task={selectedTask} onClose={() => setSelectedTask(null)}
            onComplete={openCompletion} onDone={setStatus} onDel={(id: number) => { remove(id); setSelectedTask(null); }} onRefresh={load}
            signal={signalMap.get(selectedTask.id)} onConfirmSignal={confirmSignal} onDismissSignal={dismissSignal} />
        )}
      </div>

      {/* Add/Edit form overlay */}
      {(showForm || editingTask || showFormWithContent !== null) && (
        <TaskForm
          initial={
            editingTask
              ? { content: editingTask.content, description: editingTask.description ?? "", dueDate: editingTask.due_date ?? "", priority: editingTask.priority }
              : showFormWithContent !== null
              ? { content: showFormWithContent, description: "", dueDate: "", priority: "normal" }
              : undefined
          }
          onSave={(content, dueDate, priority, description) => {
            if (editingTask) {
              invoke("update_task", { taskId: editingTask.id, content, priority, dueDate, description })
                .then(() => { setEditingTask(null); load(); })
                .catch(e => console.error("update_task:", e));
            } else {
              createTask(content, dueDate, priority, description);
              setShowFormWithContent(null);
            }
          }}
          onCancel={() => { setShowForm(false); setEditingTask(null); setShowFormWithContent(null); }}
        />
      )}

      {/* Completion dialog */}
      {completionTarget && (
        <CompletionDialog
          task={completionTarget.task}
          status={completionTarget.status}
          meta={{ showCreatedAt: true }}
          onClose={() => {
            setCompletionTarget(null);
            setPendingSignalId(null);
          }}
          onRefresh={() => {
            if (pendingSignalId !== null) {
              invoke("accept_signal", { signalId: pendingSignalId })
                .then(loadSignals)
                .catch(e => console.error("accept_signal:", e));
              setPendingSignalId(null);
            }
            load();
          }}
        />
      )}
    </div>
  );
}

/* ─── Detail Panel (right side) ─── */

function TaskDetail({ task, onClose, onComplete, onDone, onDel, onRefresh, signal, onConfirmSignal, onDismissSignal }: {
  task: TaskItem;
  onClose: () => void;
  onComplete: (t: TaskItem, s: "done" | "cancelled") => void;
  onDone: (id: number, s: string) => void;
  onDel: (id: number) => void;
  onRefresh: () => void;
  signal?: TaskSignal;
  onConfirmSignal?: (signal: TaskSignal) => void;
  onDismissSignal?: (id: number) => void;
}) {
  const { t } = useLang();
  const closed = task.status !== "open";
  const [title, setTitle] = useState(task.content);
  const [desc, setDesc] = useState(task.description ?? "");
  const [dueDate, setDueDate] = useState(task.due_date ?? "");
  const [priority, setPriority] = useState(task.priority);
  const [dirty, setDirty] = useState(false);
  const [editingDesc, setEditingDesc] = useState(false);
  const titleRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setTitle(task.content);
    setDesc(task.description ?? "");
    setDueDate(task.due_date ?? "");
    setPriority(task.priority);
    setDirty(false);
    setEditingDesc(false);
  }, [task.id, task.content, task.description, task.due_date, task.priority]);

  useEffect(() => {
    if (titleRef.current) {
      titleRef.current.style.height = "auto";
      titleRef.current.style.height = titleRef.current.scrollHeight + "px";
    }
  }, [title]);

  const saveWith = (overrides: { content?: string; priority?: string; dueDate?: string | null; description?: string | null } = {}) => {
    const c = overrides.content ?? title.trim();
    if (!c) return;
    invoke("update_task", {
      taskId: task.id,
      content: c,
      priority: overrides.priority ?? priority,
      dueDate: "dueDate" in overrides ? overrides.dueDate : (dueDate || null),
      description: "description" in overrides ? overrides.description : (desc.trim() || null),
    }).then(() => { setDirty(false); onRefresh(); })
      .catch(e => console.error("update_task:", e));
  };
  const save = () => { if (dirty) saveWith(); };

  const aiSignalLabel = (sig: TaskSignal) => {
    if (sig.signalType === "completion") return t("detail.aiSuggestsDone");
    if (sig.signalType === "cancellation") return t("detail.aiSuggestsCancel");
    return t("detail.aiSuggestsNew");
  };

  return (
    <div className="task-detail">
      <div className="task-detail-header">
        <div className="task-detail-priorities">
          {["normal", "P0", "P1", "P2"].map(p => (
            <button key={p} className={`task-detail-pri-chip${priority === p ? " active" : ""}`}
              onClick={() => { setPriority(p); saveWith({ priority: p }); }}>
              {p === "normal" ? "—" : p}
            </button>
          ))}
        </div>
        <button className="task-detail-close" onClick={() => { save(); onClose(); }}>&times;</button>
      </div>

      <DatePicker value={dueDate} onChange={v => { setDueDate(v); saveWith({ dueDate: v || null }); }} />

      <textarea ref={titleRef} className="task-detail-title-input" value={title}
        onChange={e => { setTitle(e.target.value); setDirty(true); }}
        onBlur={save} placeholder={t("taskForm.titlePlaceholder")} rows={1} />

      {editingDesc ? (
        <textarea className="task-detail-desc-input" value={desc} autoFocus
          onChange={e => { setDesc(e.target.value); setDirty(true); }}
          onBlur={() => { save(); setEditingDesc(false); }}
          placeholder={t("detail.descEdit")} rows={8} />
      ) : (
        <div className="task-detail-desc-rendered" onClick={() => setEditingDesc(true)}>
          {desc ? <ReactMarkdown>{desc}</ReactMarkdown> : <span className="task-detail-desc-placeholder">{t("detail.descPlaceholder")}</span>}
        </div>
      )}

      <div className="task-detail-meta">
        <div className="task-detail-meta-row"><span className="task-detail-label">{t("detail.source")}</span><span>{task.source}</span></div>
        <div className="task-detail-meta-row"><span className="task-detail-label">{t("detail.created")}</span><span>{task.created_at.slice(0, 10)}</span></div>
        {task.status !== "open" && (
          <div className="task-detail-meta-row"><span className="task-detail-label">{t("detail.status")}</span><span className="tasks-status-label" data-status={task.status}>{task.status.toUpperCase()}</span></div>
        )}
        {task.outcome && (
          <div className="task-detail-meta-row"><span className="task-detail-label">{t("detail.outcome")}</span><span className="task-detail-outcome">{task.outcome}</span></div>
        )}
      </div>

      {signal && onConfirmSignal && onDismissSignal && (
        <div className="task-detail-signal">
          <div className="task-detail-signal-header">
            <span className="task-detail-signal-type">{aiSignalLabel(signal)}</span>
          </div>
          <div className="task-detail-signal-evidence">{signal.evidence || signal.title}</div>
          <div className="task-detail-signal-actions">
            <button className="task-detail-btn done" onClick={() => onConfirmSignal(signal)}>{t("confirm")}</button>
            <button className="task-detail-btn del" onClick={() => onDismissSignal(signal.id)}>{t("dismiss")}</button>
          </div>
        </div>
      )}

      <div className="task-detail-actions">
        {!closed && <>
          <button className="task-detail-btn done" onClick={() => onComplete(task, "done")}>{t("done")}</button>
          <button className="task-detail-btn cancel" onClick={() => onComplete(task, "cancelled")}>{t("cancel")}</button>
        </>}
        {closed && <button className="task-detail-btn reopen" onClick={() => onDone(task.id, "open")}>{t("reopen")}</button>}
        <button className="task-detail-btn del" onClick={() => onDel(task.id)}>{t("delete")}</button>
      </div>
    </div>
  );
}

/* ─── Sub-components ─── */

function Section({ label, tasks, accent, onComplete, onDone, onSelect, selectedId, signalMap }: {
  label: string; tasks: TaskItem[]; accent: string;
  onComplete: (t: TaskItem, s: "done" | "cancelled") => void;
  onDone: (id: number, s: string) => void;
  onSelect: (t: TaskItem) => void; selectedId: number | null;
  signalMap: Map<number, TaskSignal>;
}) {
  if (!tasks.length) return null;
  return (
    <div className="tasks-section">
      <div className="tasks-section-label" style={{ color: accent }}>{label} ({tasks.length})</div>
      {tasks.map(task => <Row key={task.id} t={task} onComplete={onComplete} onDone={onDone} onSelect={onSelect} selected={task.id === selectedId}
        hasSignal={signalMap.has(task.id)} />)}
    </div>
  );
}

function Row({ t: task, onComplete, onDone, onSelect, selected, hasSignal }: {
  t: TaskItem;
  onComplete: (t: TaskItem, s: "done" | "cancelled") => void;
  onDone: (id: number, s: string) => void;
  onSelect: (t: TaskItem) => void;
  selected: boolean;
  hasSignal?: boolean;
}) {
  const { t } = useLang();
  const closed = task.status !== "open";
  const handleCheck = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (closed) { onDone(task.id, "open"); } else { onComplete(task, "done"); }
  };
  return (
    <div className={`tasks-row${closed ? " tasks-row-closed" : ""}${selected ? " tasks-row-selected" : ""}`} onClick={() => onSelect(task)}>
      <button className={`tasks-checkbox${closed ? " checked" : ""}`} data-status={task.status}
        onClick={handleCheck} onContextMenu={e => { e.preventDefault(); if (!closed) onComplete(task, "cancelled"); }}
        title={closed ? t("tasks.checkboxReopen") : t("tasks.checkboxDone")}>
        {task.status === "done" && <span className="tasks-check-icon">✓</span>}
        {(task.status === "cancelled" || task.status === "stale") && <span className="tasks-check-icon">—</span>}
      </button>
      <div className="tasks-row-body">
        <span className="tasks-row-text">{task.content}</span>
        <span className="tasks-row-meta">
          {task.source !== "manual" && <span className="tasks-source-badge">{task.source}</span>}
          {task.due_date && <span className="tasks-due-clickable">{task.due_date}</span>}
          {task.priority !== "normal" && <span className="tasks-priority-badge">{task.priority}</span>}
          {hasSignal && <span className="tasks-signal-dot" title="AI suggestion">●</span>}
        </span>
      </div>
    </div>
  );
}
