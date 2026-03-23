Summarize this {type_label} "{channel}" and extract action items.

Messages:
{messages_text}

Return in this exact format:
SUMMARY: 2-3 sentences covering key topics discussed, decisions made, and current status.
ACTIONS:
- [P0/P1/P2] action item description | @owner (if mentioned)
If no clear action items, output ACTIONS: NONE
P0=urgent/blocking, P1=important this week, P2=nice to have