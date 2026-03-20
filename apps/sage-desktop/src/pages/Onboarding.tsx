import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";

const STEPS = [
  { key: "BasicInfo", label: "Basic info", desc: "Let Sage know your role" },
  { key: "ReportingLine", label: "Reporting line", desc: "Your reporting structure" },
  { key: "Projects", label: "Projects", desc: "Projects you're currently working on" },
  { key: "Schedule", label: "Schedule", desc: "Work hours and preferences" },
  { key: "CommunicationStyle", label: "Communication style", desc: "How you prefer to communicate" },
  { key: "Stakeholders", label: "Stakeholders", desc: "Key collaborators" },
  { key: "Review", label: "Review", desc: "Check and submit" },
];

const FIELD_LABELS: Record<string, string> = {
  name: "Name",
  role: "Role",
  primary_language: "Primary language",
  secondary_language: "Secondary language",
  reporting_line: "Reporting line",
  projects: "Projects",
  morning_hour: "Morning brief time",
  evening_hour: "Evening review time",
  weekly_report_day: "Weekly report day",
  weekly_report_hour: "Weekly report time",
  work_start_hour: "Work start",
  work_end_hour: "Work end",
  comm_style: "Communication style",
  notification_max_chars: "Max notification length",
  stakeholders: "Stakeholders",
};

const WEEKDAY_OPTIONS = [
  { value: "Mon", label: "Monday" },
  { value: "Tue", label: "Tuesday" },
  { value: "Wed", label: "Wednesday" },
  { value: "Thu", label: "Thursday" },
  { value: "Fri", label: "Friday" },
  { value: "Sat", label: "Saturday" },
  { value: "Sun", label: "Sunday" },
];

/* eslint-disable @typescript-eslint/no-explicit-any */
function transformForStep(stepKey: string, data: Record<string, string>): Record<string, any> {
  switch (stepKey) {
    case "BasicInfo":
      return {
        name: data.name ?? "",
        role: data.role ?? "",
        primary_language: data.primary_language || undefined,
        secondary_language: data.secondary_language || undefined,
      };
    case "ReportingLine":
      return {
        reporting_line: (data.reporting_line ?? "").split("\n").filter(Boolean),
      };
    case "Projects":
      return {
        projects: (data.projects ?? "")
          .split("\n")
          .filter(Boolean)
          .map((line) => {
            const idx = line.indexOf(" - ");
            if (idx !== -1) {
              return { name: line.slice(0, idx).trim(), description: line.slice(idx + 3).trim(), status: "Active" };
            }
            return { name: line.trim(), description: "", status: "Active" };
          }),
      };
    case "Schedule":
      return {
        morning_brief_hour: parseInt(data.morning_hour ?? "8", 10),
        evening_review_hour: parseInt(data.evening_hour ?? "18", 10),
        weekly_report_day: data.weekly_report_day ?? "Fri",
        weekly_report_hour: parseInt(data.weekly_report_hour ?? "16", 10),
        work_start_hour: parseInt(data.work_start_hour ?? "8", 10),
        work_end_hour: parseInt(data.work_end_hour ?? "19", 10),
      };
    case "CommunicationStyle":
      return {
        style: data.comm_style ?? "Direct",
        notification_max_chars: parseInt(data.notification_max_chars ?? "200", 10),
      };
    case "Stakeholders":
      return {
        stakeholders: (data.stakeholders ?? "")
          .split("\n")
          .filter(Boolean)
          .map((line) => {
            const idx = line.indexOf(" - ");
            if (idx !== -1) {
              return { name: line.slice(0, idx).trim(), role: line.slice(idx + 3).trim(), relationship: "Colleague" };
            }
            return { name: line.trim(), role: "", relationship: "Colleague" };
          }),
      };
    case "Review":
      return {};
    default:
      return data;
  }
}
/* eslint-enable @typescript-eslint/no-explicit-any */

function Onboarding() {
  const [currentStep, setCurrentStep] = useState(0);
  const [formData, setFormData] = useState<Record<string, string>>({});
  const [sopPreview, setSopPreview] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  // Onboarding 完成后的"第一印象"过渡页状态
  const [firstImpression, setFirstImpression] = useState<string | null>(null);
  const [showFirstImpression, setShowFirstImpression] = useState(false);
  const navigate = useNavigate();

  const step = STEPS[currentStep];

  useEffect(() => {
    invoke("reset_onboarding").catch(console.error);
  }, []);

  const updateField = (key: string, value: string) => {
    setFormData((prev) => ({ ...prev, [key]: value }));
  };

  const handleNext = async () => {
    setSubmitting(true);
    try {
      const transformed = transformForStep(step.key, formData);
      const result = await invoke<{
        step: string;
        index: number;
        total: number;
        sop_preview?: string;
        first_impression?: string;
      }>(
        "submit_onboarding_step",
        { data: transformed }
      );
      if (result.sop_preview) {
        setSopPreview(result.sop_preview);
      }
      if (currentStep < STEPS.length - 1) {
        setCurrentStep(currentStep + 1);
      } else if (result.first_impression) {
        // Onboarding 完成且有 first impression：先展示过渡页，再跳 Dashboard
        setFirstImpression(result.first_impression);
        setShowFirstImpression(true);
      } else {
        navigate("/");
      }
    } catch (err) {
      console.error(err);
    } finally {
      setSubmitting(false);
    }
  };

  const handleBack = () => {
    if (currentStep > 0) setCurrentStep(currentStep - 1);
  };

  const renderStepContent = () => {
    switch (step.key) {
      case "BasicInfo":
        return (
          <>
            <div className="form-group">
              <label className="form-label">Name</label>
              <input className="form-input" value={formData.name ?? ""} onChange={(e) => updateField("name", e.target.value)} placeholder="Your name" />
            </div>
            <div className="form-group">
              <label className="form-label">Role</label>
              <input className="form-input" value={formData.role ?? ""} onChange={(e) => updateField("role", e.target.value)} placeholder="e.g. Team Lead" />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">Primary language</label>
                <input className="form-input" value={formData.primary_language ?? ""} onChange={(e) => updateField("primary_language", e.target.value)} placeholder="e.g. English" />
              </div>
              <div className="form-group">
                <label className="form-label">Secondary language</label>
                <input className="form-input" value={formData.secondary_language ?? ""} onChange={(e) => updateField("secondary_language", e.target.value)} placeholder="e.g. Chinese" />
              </div>
            </div>
          </>
        );
      case "ReportingLine":
        return (
          <div className="form-group">
            <label className="form-label">Reporting line</label>
            <textarea className="form-textarea" value={formData.reporting_line ?? ""} onChange={(e) => updateField("reporting_line", e.target.value)} placeholder={"Your name\nDirect manager\nManager's manager"} />
            <div className="form-hint">One per line, starting from you upward</div>
          </div>
        );
      case "Projects":
        return (
          <div className="form-group">
            <label className="form-label">Current projects</label>
            <textarea className="form-textarea" value={formData.projects ?? ""} onChange={(e) => updateField("projects", e.target.value)} placeholder={"Project A - Brief description\nProject B - Brief description"} />
            <div className="form-hint">One per line, format: Project name - description</div>
          </div>
        );
      case "Schedule":
        return (
          <>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">Morning brief time</label>
                <input className="form-input" type="number" value={formData.morning_hour ?? "8"} onChange={(e) => updateField("morning_hour", e.target.value)} min="0" max="23" />
              </div>
              <div className="form-group">
                <label className="form-label">Evening review time</label>
                <input className="form-input" type="number" value={formData.evening_hour ?? "18"} onChange={(e) => updateField("evening_hour", e.target.value)} min="0" max="23" />
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">Work start</label>
                <input className="form-input" type="number" value={formData.work_start_hour ?? "8"} onChange={(e) => updateField("work_start_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">24-hour format</div>
              </div>
              <div className="form-group">
                <label className="form-label">Work end</label>
                <input className="form-input" type="number" value={formData.work_end_hour ?? "19"} onChange={(e) => updateField("work_end_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">24-hour format</div>
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">Weekly report day</label>
                <select className="form-select" value={formData.weekly_report_day ?? "Fri"} onChange={(e) => updateField("weekly_report_day", e.target.value)}>
                  {WEEKDAY_OPTIONS.map((w) => (
                    <option key={w.value} value={w.value}>{w.label}</option>
                  ))}
                </select>
              </div>
              <div className="form-group">
                <label className="form-label">Weekly report time</label>
                <input className="form-input" type="number" value={formData.weekly_report_hour ?? "16"} onChange={(e) => updateField("weekly_report_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">24-hour format</div>
              </div>
            </div>
          </>
        );
      case "CommunicationStyle":
        return (
          <>
            <div className="form-group">
              <label className="form-label">Communication style preference</label>
              <select className="form-select" value={formData.comm_style ?? "Direct"} onChange={(e) => updateField("comm_style", e.target.value)}>
                <option value="Direct">Direct — concise and straight to the point</option>
                <option value="Formal">Formal — structured and professional</option>
                <option value="Casual">Casual — relaxed and natural</option>
              </select>
            </div>
            <div className="form-group">
              <label className="form-label">Max notification length</label>
              <input className="form-input" type="number" value={formData.notification_max_chars ?? "200"} onChange={(e) => updateField("notification_max_chars", e.target.value)} min="50" max="500" />
              <div className="form-hint">Maximum characters for Sage's suggestion notifications (50–500)</div>
            </div>
          </>
        );
      case "Stakeholders":
        return (
          <div className="form-group">
            <label className="form-label">Key stakeholders</label>
            <textarea className="form-textarea" value={formData.stakeholders ?? ""} onChange={(e) => updateField("stakeholders", e.target.value)} placeholder={"Alice - Product Manager\nBob - Client"} />
            <div className="form-hint">One per line, format: Name - role</div>
          </div>
        );
      case "Review":
        return (
          <div>
            <p style={{ fontSize: 14, color: "var(--text-secondary)", marginBottom: 20 }}>
              Please review the information below. Sage will generate a personalized SOP after you submit.
            </p>
            {Object.entries(formData).filter(([, v]) => v).map(([k, v]) => (
              <div key={k} className="review-section">
                <div className="review-label">{FIELD_LABELS[k] ?? k}</div>
                <div className="review-value">{String(v)}</div>
              </div>
            ))}
            {sopPreview && (
              <div style={{ marginTop: 20 }}>
                <div className="review-label">SOP preview</div>
                <div className="sop-preview">{sopPreview}</div>
              </div>
            )}
          </div>
        );
      default:
        return null;
    }
  };

  // Onboarding 完成后的"第一印象"过渡页
  if (showFirstImpression) {
    return (
      <div className="onboarding-container">
        <div className="onboarding-header" style={{ textAlign: "center", paddingTop: 48 }}>
          <div style={{ fontSize: 56, marginBottom: 16 }}>✦</div>
          <h2 style={{ marginBottom: 8 }}>Nice to meet you.</h2>
          <p style={{ color: "var(--text-secondary)", fontSize: 14 }}>Sage's first impression</p>
        </div>

        <div className="onboarding-card" style={{ textAlign: "center", padding: "32px 28px" }}>
          <p style={{
            fontSize: 16,
            lineHeight: 1.75,
            color: "var(--text-primary)",
            whiteSpace: "pre-wrap",
          }}>
            {firstImpression}
          </p>
        </div>

        <div className="onboarding-actions" style={{ justifyContent: "center" }}>
          <button
            className="btn btn-primary"
            onClick={() => navigate("/")}
            style={{ minWidth: 160 }}
          >
            Let's begin →
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="onboarding-container">
      <div className="onboarding-progress">
        {STEPS.map((_, i) => (
          <div key={i} className={`progress-step ${i < currentStep ? "done" : i === currentStep ? "active" : ""}`} />
        ))}
      </div>

      <div className="onboarding-header">
        <div className="step-count">Step {currentStep + 1} / {STEPS.length}</div>
        <h2>{step.label}</h2>
        <p>{step.desc}</p>
      </div>

      <div className="onboarding-card" key={currentStep}>
        {renderStepContent()}
      </div>

      <div className="onboarding-actions">
        {currentStep > 0 ? (
          <button className="btn btn-secondary" onClick={handleBack}>Back</button>
        ) : (
          <div className="spacer" />
        )}
        <button className="btn btn-primary" onClick={handleNext} disabled={submitting}>
          {submitting ? "Submitting..." : currentStep === STEPS.length - 1 ? "Finish setup" : "Next"}
        </button>
      </div>
    </div>
  );
}

export default Onboarding;
