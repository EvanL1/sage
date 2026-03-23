Analyze the following conversation and extract key insights about the user.

Focus on these dimensions:
- identity: who the user is, self-perception
- values: what matters most to the user
- behavior: behavioral patterns, habits
- thinking: thinking style, decision-making approach
- emotion: emotional cues, triggers
- growth: growth direction, aspirations

## Memory depth hierarchy
Each insight must be assigned a depth level:
- episodic: a specific event — MUST include date, person name, or concrete action. "Met with Bob on March 20" qualifies. "Often meets with people" does NOT.
- semantic: a recurring behavioral pattern — describes WHAT the user tends to do. "Tends to cut scope under pressure" or "Communication style is minimal and direct". No causal reasoning needed.
- procedural: judgment logic — MUST have all three parts: "when [situation], tends to [action], because [reason]". If you cannot fill all three parts from the conversation, use semantic instead. This is the most commonly misassigned level.
- axiom: DO NOT assign axiom — it is only created through memory evolution, never from direct extraction

Existing memories:
{existing_text}

Conversation:
{conversation}

Output new insights as a JSON array, each with:
- category: one of the dimensions above
- content: specific observation (one sentence)
- confidence: confidence score 0.0–1.0
- depth: one of episodic/semantic/procedural/axiom

Before outputting, verify each insight:
- If depth is "procedural", check: does it have situation + action + reason? If not, change to "semantic"
- If depth is "axiom", change to "procedural" or "semantic" — axiom is never directly extracted

Output only the JSON array, no other text. If no new insights, output [].
Example: [{{"category":"behavior","content":"Cuts scope aggressively when facing uncertainty","confidence":0.7,"depth":"semantic"}}]