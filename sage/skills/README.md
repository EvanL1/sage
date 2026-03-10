# Sage Skills

You talk to AI every day. It doesn't know who you are.
Every conversation starts from zero. It writes your emails, but they don't sound like you.
It gives advice, but doesn't know what you actually care about.

**Sage is a set of OpenClaw Skills that fix this.** Not a memory tool — a cognitive framework.
It doesn't just remember facts about you. It learns *how you think*, *how you decide*, and *how you sound*.

---

## The Cognitive Loop

Everything in Sage runs on one loop:

```
┌─────────────────────────────────────────────┐
│                                             │
│   KNOW → OBSERVE → REFLECT → QUESTION       │
│     ↑                            │          │
│     └──────────── CARE ──────────┘          │
│                  ↻ daily                    │
└─────────────────────────────────────────────┘
```

- **KNOW** — Build a profile of who you are (identity, values, work rhythm)
- **OBSERVE** — Silently detect behavioral patterns across every conversation
- **REFLECT** — Mirror one pattern back, gently: *"I've noticed..."*
- **QUESTION** — Ask one Socratic question that opens a door you haven't walked through
- **CARE** — Watch for overload signals, offer warmth without nagging

The loop compounds. After weeks, your AI knows you better than most people do.

---

## Skills

### 🧠 sage-cognitive — The Foundation
*The mirror with memory.*

Builds your cognitive profile, runs the 5-phase loop, and stores what it learns in a three-tier memory system (core / working / archive). Every other Sage skill depends on this one.

> *"You've said 'good enough' four times this week. Sounds like you're actively fighting perfectionism — and winning."*

---

### 🎙️ sage-voice — Write Like You
*Not AI prose. Your prose.*

Learns your writing style from real samples, adapts to your audience (boss, team, client), and gets more accurate with every correction you give it. The output should be indistinguishable from something you'd write on a good day.

> *Draft: "Hey Shawn — quick heads-up on the PULSE timeline..."*
> *(not: "Dear Shawn, I am writing to inform you...")*

---

### 🪞 sage-decision-journal — Because You'll Forget Why
*Your biggest blind spot isn't making bad decisions. It's forgetting you made one.*

Captures every significant choice with context, reasoning, and alternatives. Surfaces your decision patterns over time: what you optimize for, what you consistently avoid, where your instincts are right or wrong.

> *"You've chosen speed over thoroughness 7 times in the last 30 days. Is that intentional?"*

---

### 🗓️ sage-week-rhythm — Did This Week Feel Like You?
*Not a standup. Not a KPI report. A real check-in.*

Four touchpoints per week (Week Start, Daily Pulse, Week End, Cross-Week Growth) — all question-driven, all filtered through who you are. Tracks energy and alignment, not just tasks.

> *"You planned for deep work but spent 80% in meetings. What needs to change next week?"*

---

## Quick Start

```bash
git clone https://github.com/nicholasgasior/sage
cp -r sage/skills/sage-cognitive ~/.openclaw/skills/
# done. start talking.
```

To add more skills (each depends on sage-cognitive):

```bash
cp -r sage/skills/sage-voice ~/.openclaw/skills/
cp -r sage/skills/sage-decision-journal ~/.openclaw/skills/
cp -r sage/skills/sage-week-rhythm ~/.openclaw/skills/
```

---

## What Happens

**Day 1** — Your AI asks you ~10 questions across natural conversation. No survey. No forms.
It generates your first cognitive profile: identity, values, work rhythm, communication style.

**Week 1** — You get your first behavioral reflection.
*"I noticed you tend to make your biggest decisions within minutes of hearing the options."*

**Month 1** — Your AI knows your decision patterns, blind spots, and energy rhythms.
It starts asking questions you haven't asked yourself.

---

## Not a Memory Tool

| Tool | What it does |
|------|-------------|
| mem0 | Remembers facts about you |
| MemGPT | Manages context windows |
| **Sage** | Understands who you are — and helps you understand yourself |

Sage doesn't store your todos. It builds a model of *you*: how you think under pressure,
what you consistently avoid, where your instincts are reliable. That's a different thing.

---

## License & Contributing

MIT. Built for OpenClaw.

To add a new skill: model it after `sage-cognitive/SKILL.md`.
Each skill should answer one question: *"What aspect of this person does it help them understand?"*
