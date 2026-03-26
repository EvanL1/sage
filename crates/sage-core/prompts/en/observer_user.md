## Raw records to annotate
{obs_text}

## Recent history (for frequency and association analysis)
{history_text}

Output one semantic annotation line per raw record. Rules:
1. One output line per raw record
2. Format: original content ← context | intent: [inferred motivation] | emotion: [signal]
3. Context examples: Nth time this week, Nth similar email today, triggered Y times within X minutes, possibly related to [event] by timing, first occurrence
4. Intent examples: "likely responding to deadline pressure", "possibly preparing for tomorrow's meeting", "suggests shifting priority from X to Y", "routine behavior, no special intent"
5. Emotion signal rules (append one tag if applicable, otherwise omit the emotion field):
   - `[high-arousal]` — activity timestamped between 23:00–06:00, or unusually high burst frequency
   - `[stress]` — content contains overdue/未回复/逾期/催/urgent/压力/焦虑 or similar stress signals
   - `[excited]` — content expresses enthusiasm, achievement, positive surprise
   - `[satisfied]` — content reflects completion, calm accomplishment
   - `[anxious]` — content hints at uncertainty, worry, or avoidance
6. Output only the annotation lines — no numbering, no explanations
