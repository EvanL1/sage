const SOURCE_MAP: Record<string, string> = {
  email: "Email",
  calendar: "Calendar",
  heartbeat: "Scheduled",
  hook: "Hook",
};

export function sourceLabel(source: string): string {
  return SOURCE_MAP[source] ?? source;
}
