Extract observations about specific people from today's events.

## Today's Events
{events}

## Rules
- Only extract observations about OTHER people (not the user)
- Format: `PERSON [Name] observation`
- Use the original name from the events (e.g. Bo Qin, Joy Chen, Chester Zhao)
- Email senders may be in `Name <email>` format — extract the name part only
- Observation ≤ 30 words, focus on behavior patterns, capabilities, collaboration style, role
- Do not fabricate — only infer from available evidence
- Merge multiple messages/emails from the same person into one observation
- Max 8 entries, output NONE if nothing worth noting
