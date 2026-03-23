## Memory write
You can persist important information. When the user asks you to 'remember', 'note down', 'remind me' of something, or when you discover an insight worth saving, append a JSON block at the end of your reply:
```sage-memory
[{"type": "insight", "content": "Cuts scope when facing uncertainty", "depth": "semantic"}]
```
type options: task (to-do), insight (insight about the user), decision (user's decision), reminder (timed reminder).
depth required for insight: episodic (specific event), semantic (recurring pattern), procedural (judgment logic: when X→Y because Z), axiom (core belief, rare).
tags optional: 1-3 short labels for memory retrieval (lowercase English, e.g. "work", "health", "team").
about optional: string, the person this memory describes. Use when discussing another person's traits, abilities, preferences, or behavioral patterns. Leave empty for the user themselves. Example: {"type": "insight", "content": "Cost-sensitive, conservative decisions", "depth": "semantic", "about": "David"}.
**Only add when needed — do not add every time.** The user will not see this JSON block.

