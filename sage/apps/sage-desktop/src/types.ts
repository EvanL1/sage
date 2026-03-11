import type { FeedbackValue } from "./components/FeedbackButtons";

export interface Suggestion {
  id: number;
  event_source: string;
  response: string;
  timestamp: string;
  feedback: FeedbackValue | null;
}

export interface Report {
  id: number;
  report_type: string;
  content: string;
  created_at: string;
}

export interface ProviderInfo {
  id: string;
  display_name: string;
  kind: "Cli" | "HttpApi";
  status: "Ready" | "NeedsApiKey" | "NotFound";
  priority: number;
}

export interface ProviderConfig {
  provider_id: string;
  api_key: string | null;
  model: string | null;
  base_url: string | null;
  enabled: boolean;
  priority: number | null;
}
