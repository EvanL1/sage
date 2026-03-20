export function formatTime(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  } catch {
    return "";
  }
}

export function formatDate(ts: string, monthFormat: "long" | "short" = "long"): string {
  try {
    const d = new Date(ts);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    if (d.toDateString() === today.toDateString()) return "Today";
    if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
    return d.toLocaleDateString("en-US", { month: monthFormat, day: "numeric" });
  } catch {
    return ts;
  }
}
