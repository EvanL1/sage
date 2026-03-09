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
  "Creator", "Thinker", "Leader", "Learner",
  "Explorer", "Doer", "Connector", "Guardian",
  "Artist", "Observer", "Communicator", "Analyst",
];

const QUESTIONS: ScenarioQuestion[] = [
  { id: 0, text: "After work on Friday, you're more likely to...", optionA: "Find a quiet place to recharge alone", optionB: "Meet up with friends to unwind", dimension: 0 },
  { id: 1, text: "When facing a tough problem...", optionA: "Think deeply on your own to find the answer", optionB: "Discuss it with others to spark ideas", dimension: 0 },
  { id: 2, text: "Working on a project, you care more about...", optionA: "Every detail holding up to scrutiny", optionB: "Getting the overall direction and strategy right", dimension: 1 },
  { id: 3, text: "Reading a long report...", optionA: "Go through all the data carefully from start to finish", optionB: "Check the conclusion first, dig into details as needed", dimension: 1 },
  { id: 4, text: "Facing an important decision...", optionA: "Data and logic are the final basis", optionB: "Impact on people matters just as much", dimension: 2 },
  { id: 5, text: "A team member makes a mistake...", optionA: "Point out the problem and improvement direction directly", optionB: "First understand why, then find a solution together", dimension: 2 },
  { id: 6, text: "Planning a trip...", optionA: "Plan every stop in advance", optionB: "Prefer to figure it out as you go", dimension: 3 },
  { id: 7, text: "The project plan suddenly changes...", optionA: "A bit unsettled, want to get back to the original plan", optionB: "Think the new direction might be more interesting", dimension: 3 },
  { id: 8, text: "You have a new idea...", optionA: "Think through the feasibility before acting", optionB: "Try it first, adjust as you go", dimension: 4 },
  { id: 9, text: "Facing an urgent situation...", optionA: "Calmly analyze and clarify thoughts before acting", optionB: "Immediately tackle the most pressing thing", dimension: 4 },
  { id: 10, text: "You enjoy more...", optionA: "Taking one thing to mastery", optionB: "Exploring many different areas", dimension: 5 },
  { id: 11, text: "Learning something new...", optionA: "Must go deep until I've mastered it", optionB: "Grasp the core is enough, curiosity takes me to the next thing", dimension: 5 },
];

const DIMENSIONS: DimensionSpec[] = [
  { left: "Solitary", right: "Social", leftDesc: "Energized by alone time, prefers deep thinking", rightDesc: "Energized by socializing, strong collaborator", balancedDesc: "Flexibly switches between solitude and social" },
  { left: "Detail", right: "Big picture", leftDesc: "Meticulous, pursues precision and perfection", rightDesc: "Focuses on the big picture and direction", balancedDesc: "Balances detail and big picture" },
  { left: "Rational", right: "Empathetic", leftDesc: "Data and logic drive decisions", rightDesc: "Values people's feelings and relationships", balancedDesc: "Balances rational analysis and empathy" },
  { left: "Planned", right: "Flexible", leftDesc: "Likes orderly planning, follows plans", rightDesc: "Embraces change, enjoys uncertainty", balancedDesc: "Planned but stays flexible" },
  { left: "Deliberate", right: "Action-oriented", leftDesc: "Thinks things through carefully before acting", rightDesc: "Acts fast, learns through practice", balancedDesc: "Balances thinking and action" },
  { left: "Deep", right: "Broad", leftDesc: "Focused depth, pursues mastery", rightDesc: "Wide exploration, diverse interests", balancedDesc: "Both deep and broad" },
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
        content: `${dim.left} ↔ ${dim.right}: ${desc}`,
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
      const role = identityTags.length > 0 ? identityTags.join(", ") : undefined;
      await invoke("quick_setup", {
        name,
        role,
        apiKey: apiKey || undefined,
        providerId: selectedProvider || undefined,
      });
      navigate("/");
    } catch (err) {
      console.error(err);
      setError("Something went wrong, try again?");
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
              Hello.<br /><br />
              I'm Sage —<br />
              not just a tool, but a mirror to help you know yourself better.<br /><br />
              In the next few steps, I'll walk you through some simple scenarios<br />
              to reveal your <strong>thinking spectrum</strong>.
            </p>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(1)}>Get started</button>
            </div>
          </div>
        );

      case 1:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              First —<br />what's your name?
            </p>
            <div className="welcome-card">
              <div className="form-group">
                <input
                  className="form-input"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) goTo(2); }}
                  placeholder="Your name"
                  autoFocus
                />
              </div>
              <p style={{ fontSize: "0.9rem", color: "var(--text-secondary)", margin: "16px 0 8px" }}>
                How would you describe yourself?
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
                Continue
              </button>
              <p style={{ fontSize: "0.8rem", color: "var(--text-secondary)", marginTop: "10px", textAlign: "center" }}>
                Tags are optional, feel free to skip
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
                  ← Previous
                </button>
              ) : (
                <span />
              )}
              <button className="welcome-skip" onClick={() => goTo(4)}>
                Skip assessment →
              </button>
            </div>
          </div>
        );

      case 3:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              {name}, here's your thinking spectrum —
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
                Everyone is a unique combination.<br />
                This isn't a label — it's a starting point for self-understanding.
              </p>
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={() => goTo(4)}>Continue</button>
            </div>
          </div>
        );

      case 4:
        return (
          <div className="welcome-screen">
            <p className="welcome-greeting">
              Got it. I'll remember that.<br /><br />
              One last step — let me connect to my thinking capabilities —
            </p>
            <div className="welcome-card">
              {readyProviders.length > 0 ? (
                <div className="provider-status ready">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--success)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                  <span>Detected {readyProviders[0].display_name} — ready to use</span>
                </div>
              ) : (
                <>
                  <div className="form-group">
                    <label className="form-label">AI Provider</label>
                    <select
                      className="form-select"
                      value={selectedProvider}
                      onChange={(e) => {
                        setSelectedProvider(e.target.value);
                        setTestResult("idle");
                      }}
                    >
                      <option value="">Select an AI provider...</option>
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
                      {testResult === "testing" ? "Testing..." : "Test connection"}
                    </button>
                    {testResult === "ok" && (
                      <span className="provider-status ready">Connection successful</span>
                    )}
                    {testResult === "fail" && (
                      <span className="provider-status error">Something went wrong, try again?</span>
                    )}
                  </div>
                </>
              )}
            </div>
            <div className="welcome-actions">
              <button className="btn btn-primary" onClick={handleComplete} disabled={submitting}>
                {submitting ? "Setting up..." : "Begin our journey"}
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
