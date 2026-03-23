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
  status: "Ready" | "NeedsLogin" | "NeedsApiKey" | "NotFound";
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

export interface MessageSource {
  id: number;
  label: string;
  source_type: string;
  config: string;
  enabled: boolean;
  created_at: string;
}

export interface EmailMessage {
  id: number;
  source_id: number;
  uid: string;
  folder: string;
  from_addr: string;
  to_addr: string;
  subject: string;
  body_text: string;
  body_html: string | null;
  is_read: boolean;
  date: string;
  fetched_at: string;
}

export interface Memory {
  id: number;
  category: string;
  content: string;
  source: string;
  confidence: number;
  created_at: string;
  updated_at: string;
  depth?: string;
  valid_until?: string;
  validation_count?: number;
}
