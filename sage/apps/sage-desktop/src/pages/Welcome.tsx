import { useState, useEffect, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";

interface ProviderInfo {
  id: string;
  display_name: string;
  kind: "Cli" | "HttpApi";
  status: "Ready" | "NeedsApiKey" | "NotFound";
  priority: number;
}

interface ScenarioQuestion {
  id: number;
  text: string;
  optionA: string;
  optionB: string;
  dimension: number;
}

interface DimensionSpec {
  left: string;
  right: string;
  leftDesc: string;
  rightDesc: string;
  balancedDesc: string;
}

const TOTAL_SCREENS = 5;

const IDENTITY_TAGS = [
  "创造者", "思考者", "领导者", "学习者",
  "探索者", "实干家", "连接者", "守护者",
  "艺术家", "观察者", "沟通者", "分析者",
];

const QUESTIONS: ScenarioQuestion[] = [
  { id: 0, text: "周五下班后，你更可能...", optionA: "找个安静的地方独处充电", optionB: "约朋友出去放松", dimension: 0 },
  { id: 1, text: "遇到棘手问题时...", optionA: "独自深入思考找答案", optionB: "找人讨论碰撞想法", dimension: 0 },
  { id: 2, text: "做一个项目，你更在意...", optionA: "每个细节都经得起推敲", optionB: "整体方向和战略正确", dimension: 1 },
  { id: 3, text: "看一份长报告...", optionA: "从头到尾仔细看数据", optionB: "先看结论，需要时再看细节", dimension: 1 },
  { id: 4, text: "面临重要决策...", optionA: "数据和逻辑是最终依据", optionB: "对人的影响同样重要", dimension: 2 },
  { id: 5, text: "团队成员犯了错...", optionA: "直接指出问题和改进方向", optionB: "先了解原因，再一起找方案", dimension: 2 },
  { id: 6, text: "面对一次旅行...", optionA: "喜欢提前规划好每一站", optionB: "更喜欢走到哪算哪", dimension: 3 },
  { id: 7, text: "项目计划突然变了...", optionA: "有点不安，想回到原计划", optionB: "觉得新方向可能更有趣", dimension: 3 },
  { id: 8, text: "有了一个新想法...", optionA: "先想清楚可行性再动手", optionB: "先动手试试，边做边调", dimension: 4 },
  { id: 9, text: "面对紧急状况...", optionA: "冷静分析，理清思路再行动", optionB: "立刻着手处理最紧迫的事", dimension: 4 },
  { id: 10, text: "你更享受...", optionA: "把一件事做到极致", optionB: "探索很多不同的领域", dimension: 5 },
  { id: 11, text: "学习新东西...", optionA: "一定要深入到精通", optionB: "了解核心就好，好奇心带我去下一个", dimension: 5 },
];

const DIMENSIONS: DimensionSpec[] = [
  { left: "独处", right: "社交", leftDesc: "从独处中获取能量，偏好深度思考", rightDesc: "从社交中获取能量，善于协作共创", balancedDesc: "在独处与社交间灵活切换" },
  { left: "细节", right: "全局", leftDesc: "注重细节，追求精确完美", rightDesc: "着眼全局，把握大方向", balancedDesc: "兼顾细节与全局" },
  { left: "理性", right: "共情", leftDesc: "以数据和逻辑驱动决策", rightDesc: "重视人的感受和关系", balancedDesc: "理性分析与共情理解并重" },
  { left: "计划", right: "灵活", leftDesc: "喜欢有序规划，按计划行事", rightDesc: "拥抱变化，享受不确定性", balancedDesc: "有计划但保持灵活" },
  { left: "冷静", right: "行动", leftDesc: "深思熟虑，三思而后行", rightDesc: "快速行动，实践出真知", balancedDesc: "思考与行动平衡" },
  { left: "匠心", right: "广域", leftDesc: "专注深耕，追求极致", rightDesc: "广泛探索，兴趣多元", balancedDesc: "深度与广度兼备" },
];

function Welcome() {
  const navigate = useNavigate();
  const [phase, setPhase] = useState(0);
  const [animating, setAnimating] = useState(false);
  const [direction, setDirection] = useState<"forward" | "back">("forward");
  const [name, setName] = useState("");
  const [identityTags, setIdentityTags] = useState<string[]>([]);
  const [questionIndex, setQuestionIndex] = useState(0);
  const [answers, setAnswers] = useState<(number | null)[]>(new Array(12).fill(null));
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [selectedProvider, setSelectedProvider] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [testResult, setTestResult] = useState<"idle" | "testing" | "ok" | "fail">("idle");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  const readyProviders = providers.filter((p) => p.status === "Ready");
  const apiProviders = providers.filter((p) => p.kind === "HttpApi");
  const scores = useMemo(() => {
    return DIMENSIONS.map((_, dimIdx) => {
      const qs = QUESTIONS.filter((q) => q.dimension === dimIdx);
      const answered = qs.filter((q) => answers[q.id] !== null);
      if (answered.length === 0) return 0.5;
      return answered.reduce((sum, q) => sum + (answers[q.id] ?? 0), 0) / answered.length;
    });
  }, [answers]);

  useEffect(() => {
    if (phase === 4) {
      invoke<ProviderInfo[]>("discover_providers")
        .then((result) => {
          setProviders(result);
          const firstApi = result.find((p) => p.kind === "HttpApi");
          if (firstApi) setSelectedProvider(firstApi.id);
        })
        .catch(() => {
          setProviders([]);
        });
    }
  }, [phase]);

  const goTo = (target: number) => {
    if (animating || target === phase) return;
    setDirection(target > phase ? "forward" : "back");
    setAnimating(true);
    setTimeout(() => {
      setPhase(target);
      setAnimating(false);
    }, 200);
  };

  const saveAssessment = async (finalAnswers: (number | null)[]) => {
    const finalScores = DIMENSIONS.map((_, dimIdx) => {
      const qs = QUESTIONS.filter((q) => q.dimension === dimIdx);
      const answered = qs.filter((q) => finalAnswers[q.id] !== null);
      if (answered.length === 0) return 0.5;
      return answered.reduce((sum, q) => sum + (finalAnswers[q.id] ?? 0), 0) / answered.length;
    });
    const dimensionResults = DIMENSIONS.map((dim, i) => {
      const score = finalScores[i];
      const desc = score <= 0.25 ? dim.leftDesc : score >= 0.75 ? dim.rightDesc : dim.balancedDesc;
      return {
        content: `${dim.left} ↔ ${dim.right}：${desc}`,
        confidence: Math.abs(score - 0.5) * 2 + 0.5,
      };
    });
    try {
      await invoke("save_assessment", { dimensions: dimensionResults });
    } catch (err) {
      console.error("Failed to save assessment:", err);
    }
  };

  const handleAnswer = (choice: number) => {
    const newAnswers = [...answers];
    newAnswers[questionIndex] = choice;
    setAnswers(newAnswers);

    setTimeout(() => {
      if (questionIndex < 11) {
        setQuestionIndex((prev) => prev + 1);
      } else {
        saveAssessment(newAnswers);
        goTo(3);
      }
    }, 300);
  };

  const handleTestConnection = async () => {
    setTestResult("testing");
    try {
      if (apiKey) {
        await invoke("save_provider_config", {
          config: {
            provider_id: selectedProvider,
            api_key: apiKey,
            model: null,
            base_url: null,
            enabled: true,
          },
        });
      }
      const result = await invoke<{ success: boolean; error?: string }>("test_provider", {
        providerId: selectedProvider,
      });
      setTestResult(result.success ? "ok" : "fail");
    } catch {
      setTestResult("fail");
    }
  };

  const handleComplete = async () => {
    setError("");
    setSubmitting(true);
    try {
      const role = identityTags.length > 0 ? identityTags.join("、") : undefined;
      await invoke("quick_setup", {
        name,
        role,
        apiKey: apiKey || undefined,
        providerId: selectedProvider || undefined,
      });
      navigate("/");
    } catch (err) {
      console.error(err);
      setError("出了点问题，再试一次？");
    } finally {
      setSubmitting(false);
    }
  };

  const renderPhase = () => {
    switch (phase) {
      case 0:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              你好。<br /><br />
              我是 Sage —<br />
              不只是工具，更是帮你认识自己的一面镜子。<br /><br />
              接下来，我会通过几个简单的场景，<br />
              帮你看到自己的<strong>思维光谱</strong>。
            </p>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(1)}>开始</button>
            </div>
          </div>
        );

      case 1:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              先告诉我 —<br />你的名字？
            </p>
            <div className="welcome-card">
              <div className="form-group">
                <input
                  className="form-input"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) goTo(2); }}
                  placeholder="你的名字"
                  autoFocus
                />
              </div>
              <p style={{ fontSize: "0.9rem", color: "var(--text-secondary)", margin: "16px 0 8px" }}>
                你觉得自己是...
              </p>
              <div className="tag-group">
                {IDENTITY_TAGS.map((tag) => (
                  <button
                    key={tag}
                    className={`tag ${identityTags.includes(tag) ? "selected" : ""}`}
                    onClick={() => setIdentityTags((prev) =>
                      prev.includes(tag) ? prev.filter((t) => t !== tag) : [...prev, tag]
                    )}
                  >
                    {tag}
                  </button>
                ))}
              </div>
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(2)} disabled={!name.trim()}>
                继续
              </button>
              <p style={{ fontSize: "0.8rem", color: "var(--text-secondary)", marginTop: "10px", textAlign: "center" }}>
                标签是可选的，随时可以跳过
              </p>
            </div>
          </div>
        );

      case 2:
        return (
          <div className="welcome-screen">
            <div className="assessment-progress">
              <div
                className="assessment-progress-bar"
                style={{ width: `${((questionIndex + 1) / 12) * 100}%` }}
              />
            </div>
            <p className="assessment-counter">{questionIndex + 1} / 12</p>
            <p className="welcome-greeting assessment-question">
              {QUESTIONS[questionIndex].text}
            </p>
            <div className="assessment-options">
              <button
                className={`assessment-option ${answers[questionIndex] === 0 ? "selected" : ""}`}
                onClick={() => handleAnswer(0)}
              >
                {QUESTIONS[questionIndex].optionA}
              </button>
              <button
                className={`assessment-option ${answers[questionIndex] === 1 ? "selected" : ""}`}
                onClick={() => handleAnswer(1)}
              >
                {QUESTIONS[questionIndex].optionB}
              </button>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: "8px" }}>
              {questionIndex > 0 ? (
                <button className="welcome-skip" onClick={() => setQuestionIndex((prev) => prev - 1)}>
                  ← 上一题
                </button>
              ) : (
                <span />
              )}
              <button className="welcome-skip" onClick={() => goTo(4)}>
                跳过测评 →
              </button>
            </div>
          </div>
        );

      case 3:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              {name}，这是你的思维光谱 —
            </p>
            <div className="welcome-card spectrum-card">
              {DIMENSIONS.map((dim, i) => (
                <div key={i} className="spectrum-row">
                  <span className="spectrum-label left">{dim.left}</span>
                  <div className="spectrum-track">
                    <div
                      className="spectrum-dot"
                      style={{ left: `${scores[i] * 100}%` }}
                    />
                  </div>
                  <span className="spectrum-label right">{dim.right}</span>
                </div>
              ))}
              <p className="spectrum-note">
                每个人都是独一无二的组合。<br />
                这不是标签 — 是认识自己的起点。
              </p>
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(4)}>继续</button>
            </div>
          </div>
        );

      case 4:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              好的。我记住了。<br /><br />
              最后一步，让我连接思考的能力 —
            </p>
            <div className="welcome-card">
              {readyProviders.length > 0 ? (
                <div className="provider-status ready">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--success)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                  <span>已检测到 {readyProviders[0].display_name}，直接可用</span>
                </div>
              ) : (
                <>
                  <div className="form-group">
                    <label className="form-label">AI 服务</label>
                    <select
                      className="form-select"
                      value={selectedProvider}
                      onChange={(e) => {
                        setSelectedProvider(e.target.value);
                        setTestResult("idle");
                      }}
                    >
                      <option value="">选择 AI 服务...</option>
                      {apiProviders.map((p) => (
                        <option key={p.id} value={p.id}>{p.display_name}</option>
                      ))}
                    </select>
                  </div>
                  <div className="form-group">
                    <label className="form-label">API Key</label>
                    <input
                      className="form-input"
                      type="password"
                      value={apiKey}
                      onChange={(e) => {
                        setApiKey(e.target.value);
                        setTestResult("idle");
                      }}
                      placeholder="sk-..."
                    />
                  </div>
                  <div className="welcome-actions">
                    <button
                      className="btn btn-secondary"
                      onClick={handleTestConnection}
                      disabled={!selectedProvider || !apiKey || testResult === "testing"}
                    >
                      {testResult === "testing" ? "测试中..." : "测试连接"}
                    </button>
                    {testResult === "ok" && (
                      <span className="provider-status ready">连接成功</span>
                    )}
                    {testResult === "fail" && (
                      <span className="provider-status error">出了点问题，再试一次？</span>
                    )}
                  </div>
                </>
              )}
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={handleComplete} disabled={submitting}>
                {submitting ? "准备中..." : "开始我们的旅程"}
              </button>
              {error && <p className="welcome-error">{error}</p>}
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  const enterClass = direction === "forward" ? "welcome-enter-right" : "welcome-enter-left";
  const exitClass = direction === "forward" ? "welcome-exit-left" : "welcome-exit-right";

  return (
    <div className="welcome-container">
      <div className={`welcome-content ${animating ? exitClass : enterClass}`} key={phase}>
        {renderPhase()}
      </div>

      <div className="welcome-dots">
        {Array.from({ length: TOTAL_SCREENS }, (_, i) => (
          <div
            key={i}
            className={`welcome-dot ${i < phase ? "done" : i === phase ? "active" : ""}`}
          />
        ))}
      </div>
    </div>
  );
}

export default Welcome;
