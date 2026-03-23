The following is memory or personal information exported by the user from another AI assistant (Claude/Gemini/ChatGPT):

{text}

Parse this into structured memory entries. Output one JSON line per entry:
{{"category": "...", "content": "...", "depth": "..."}}

Available categories: identity, personality, values, behavior, thinking, emotion, growth, decision, pattern, preference, skill, relationship, goal

Depth levels (required):
- episodic: a specific event — MUST include date, person name, or concrete action. "Met with Bob on March 20" qualifies. "Often meets with people" does NOT.
- semantic: a recurring behavioral pattern — describes WHAT the user tends to do. "Tends to cut scope under pressure" or "Communication style is minimal and direct". No causal reasoning needed.
- procedural: judgment logic — MUST have all three parts: "when [situation], tends to [action], because [reason]". If you cannot fill all three parts, use semantic instead. This is the most commonly misassigned level.
- axiom: DO NOT assign — axiom is only created through evolution, never from direct extraction

Requirements:
- Preserve the core content of the original information faithfully
- Each memory entry should be concise (1-2 sentences)
- Before outputting, verify each entry: if depth is "procedural", check it has situation + action + reason — if not, change to "semantic"; if depth is "axiom", change to "procedural" or "semantic"
- Output only JSON lines, no other content (no markdown code fences)