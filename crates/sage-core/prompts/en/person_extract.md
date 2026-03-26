Extract observations about specific people from today's events.

## Today's Events
{events}

## Rules
- Only extract observations about OTHER people (not the user)
- Format: `PERSON [Name] observation`
- Use the original name from the events (e.g. Alex_US, Emily, Dorman)
- Observation ≤ 30 words, focus on behavior patterns, capabilities, collaboration style
- Do not fabricate — only infer from available evidence
- Max 5 entries, output NONE if nothing worth noting
