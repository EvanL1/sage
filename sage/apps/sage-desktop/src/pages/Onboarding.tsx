import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";

const STEPS = [
  { key: "BasicInfo", label: "基本信息", desc: "让 Sage 了解你的角色" },
  { key: "ReportingLine", label: "汇报关系", desc: "你的汇报线结构" },
  { key: "Projects", label: "项目", desc: "当前负责的项目" },
  { key: "Schedule", label: "日程", desc: "工作时间偏好" },
  { key: "CommunicationStyle", label: "沟通风格", desc: "你的表达方式" },
  { key: "Stakeholders", label: "利益相关者", desc: "关键合作方" },
  { key: "Review", label: "确认", desc: "检查并提交" },
];

const FIELD_LABELS: Record<string, string> = {
  name: "姓名",
  role: "职位",
  primary_language: "主要语言",
  secondary_language: "次要语言",
  reporting_line: "汇报线",
  projects: "项目",
  morning_hour: "早间简报时间",
  evening_hour: "晚间回顾时间",
  weekly_report_day: "周报日",
  weekly_report_hour: "周报时间",
  work_start_hour: "上班时间",
  work_end_hour: "下班时间",
  comm_style: "沟通风格",
  notification_max_chars: "通知最大字数",
  stakeholders: "利益相关者",
};

const WEEKDAY_OPTIONS = [
  { value: "Mon", label: "周一" },
  { value: "Tue", label: "周二" },
  { value: "Wed", label: "周三" },
  { value: "Thu", label: "周四" },
  { value: "Fri", label: "周五" },
  { value: "Sat", label: "周六" },
  { value: "Sun", label: "周日" },
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
              return { name: line.slice(0, idx).trim(), role: line.slice(idx + 3).trim(), relationship: "同事" };
            }
            return { name: line.trim(), role: "", relationship: "同事" };
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
      const result = await invoke<{ step: string; index: number; total: number; sop_preview?: string }>(
        "submit_onboarding_step",
        { data: transformed }
      );
      if (result.sop_preview) {
        setSopPreview(result.sop_preview);
      }
      if (currentStep < STEPS.length - 1) {
        setCurrentStep(currentStep + 1);
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
              <label className="form-label">姓名</label>
              <input className="form-input" value={formData.name ?? ""} onChange={(e) => updateField("name", e.target.value)} placeholder="你的名字" />
            </div>
            <div className="form-group">
              <label className="form-label">职位</label>
              <input className="form-input" value={formData.role ?? ""} onChange={(e) => updateField("role", e.target.value)} placeholder="如：Team Lead" />
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">主要语言</label>
                <input className="form-input" value={formData.primary_language ?? ""} onChange={(e) => updateField("primary_language", e.target.value)} placeholder="如：中文" />
              </div>
              <div className="form-group">
                <label className="form-label">次要语言</label>
                <input className="form-input" value={formData.secondary_language ?? ""} onChange={(e) => updateField("secondary_language", e.target.value)} placeholder="如：English" />
              </div>
            </div>
          </>
        );
      case "ReportingLine":
        return (
          <div className="form-group">
            <label className="form-label">汇报线</label>
            <textarea className="form-textarea" value={formData.reporting_line ?? ""} onChange={(e) => updateField("reporting_line", e.target.value)} placeholder={"你的名字\n直属上级\n上级的上级"} />
            <div className="form-hint">每行一个，从你开始往上排列</div>
          </div>
        );
      case "Projects":
        return (
          <div className="form-group">
            <label className="form-label">当前项目</label>
            <textarea className="form-textarea" value={formData.projects ?? ""} onChange={(e) => updateField("projects", e.target.value)} placeholder={"项目A - 简要描述\n项目B - 简要描述"} />
            <div className="form-hint">每行一个，格式：项目名 - 描述</div>
          </div>
        );
      case "Schedule":
        return (
          <>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">早间简报时间</label>
                <input className="form-input" type="number" value={formData.morning_hour ?? "8"} onChange={(e) => updateField("morning_hour", e.target.value)} min="0" max="23" />
              </div>
              <div className="form-group">
                <label className="form-label">晚间回顾时间</label>
                <input className="form-input" type="number" value={formData.evening_hour ?? "18"} onChange={(e) => updateField("evening_hour", e.target.value)} min="0" max="23" />
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">上班时间</label>
                <input className="form-input" type="number" value={formData.work_start_hour ?? "8"} onChange={(e) => updateField("work_start_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">0-23 小时制</div>
              </div>
              <div className="form-group">
                <label className="form-label">下班时间</label>
                <input className="form-input" type="number" value={formData.work_end_hour ?? "19"} onChange={(e) => updateField("work_end_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">0-23 小时制</div>
              </div>
            </div>
            <div className="form-row">
              <div className="form-group">
                <label className="form-label">周报日</label>
                <select className="form-select" value={formData.weekly_report_day ?? "Fri"} onChange={(e) => updateField("weekly_report_day", e.target.value)}>
                  {WEEKDAY_OPTIONS.map((w) => (
                    <option key={w.value} value={w.value}>{w.label}</option>
                  ))}
                </select>
              </div>
              <div className="form-group">
                <label className="form-label">周报时间</label>
                <input className="form-input" type="number" value={formData.weekly_report_hour ?? "16"} onChange={(e) => updateField("weekly_report_hour", e.target.value)} min="0" max="23" />
                <div className="form-hint">0-23 小时制</div>
              </div>
            </div>
          </>
        );
      case "CommunicationStyle":
        return (
          <>
            <div className="form-group">
              <label className="form-label">沟通风格偏好</label>
              <select className="form-select" value={formData.comm_style ?? "Direct"} onChange={(e) => updateField("comm_style", e.target.value)}>
                <option value="Direct">直接 — 简短有力，直奔主题</option>
                <option value="Formal">正式 — 结构化、专业措辞</option>
                <option value="Casual">随意 — 轻松自然的表达</option>
              </select>
            </div>
            <div className="form-group">
              <label className="form-label">通知最大字数</label>
              <input className="form-input" type="number" value={formData.notification_max_chars ?? "200"} onChange={(e) => updateField("notification_max_chars", e.target.value)} min="50" max="500" />
              <div className="form-hint">Sage 建议通知的最大字符数（50-500）</div>
            </div>
          </>
        );
      case "Stakeholders":
        return (
          <div className="form-group">
            <label className="form-label">关键利益相关者</label>
            <textarea className="form-textarea" value={formData.stakeholders ?? ""} onChange={(e) => updateField("stakeholders", e.target.value)} placeholder={"张三 - 产品经理\n李四 - 客户"} />
            <div className="form-hint">每行一个，格式：姓名 - 角色</div>
          </div>
        );
      case "Review":
        return (
          <div>
            <p style={{ fontSize: 14, color: "var(--text-secondary)", marginBottom: 20 }}>
              请确认以下信息。提交后 Sage 将生成个性化 SOP。
            </p>
            {Object.entries(formData).filter(([, v]) => v).map(([k, v]) => (
              <div key={k} className="review-section">
                <div className="review-label">{FIELD_LABELS[k] ?? k}</div>
                <div className="review-value">{v}</div>
              </div>
            ))}
            {sopPreview && (
              <div style={{ marginTop: 20 }}>
                <div className="review-label">SOP 预览</div>
                <div className="sop-preview">{sopPreview}</div>
              </div>
            )}
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div className="onboarding-container">
      <div className="onboarding-progress">
        {STEPS.map((_, i) => (
          <div key={i} className={`progress-step ${i < currentStep ? "done" : i === currentStep ? "active" : ""}`} />
        ))}
      </div>

      <div className="onboarding-header">
        <div className="step-count">步骤 {currentStep + 1} / {STEPS.length}</div>
        <h2>{step.label}</h2>
        <p>{step.desc}</p>
      </div>

      <div className="onboarding-card" key={currentStep}>
        {renderStepContent()}
      </div>

      <div className="onboarding-actions">
        {currentStep > 0 ? (
          <button className="btn btn-secondary" onClick={handleBack}>上一步</button>
        ) : (
          <div className="spacer" />
        )}
        <button className="btn btn-primary" onClick={handleNext} disabled={submitting}>
          {submitting ? "提交中..." : currentStep === STEPS.length - 1 ? "完成设置" : "下一步"}
        </button>
      </div>
    </div>
  );
}

export default Onboarding;
