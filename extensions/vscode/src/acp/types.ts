// Formes ACP v1 utilisées par l'extension (sous-ensemble servi par le pont, ticket 24 §1.3-1.4).

export const PROTOCOL_VERSION = 1;

export interface ConfigSelectOption {
  value: string;
  name: string;
}

/** `SessionConfigOption`, forme `select` (`crates/pa-daemon/src/acp/config_options.rs`). */
export interface ConfigOption {
  id: string;
  name: string;
  type: string;
  category: string;
  currentValue: string;
  options: ConfigSelectOption[];
}

export type ToolStatus = "pending" | "in_progress" | "completed" | "failed";

export interface ContentBlock {
  type: string;
  text?: string;
}

/** `session/update` : seules les variantes affichées par la PR 2 sont typées. */
export type SessionUpdate =
  | { sessionUpdate: "agent_message_chunk"; content: ContentBlock }
  | { sessionUpdate: "agent_thought_chunk"; content: ContentBlock }
  | { sessionUpdate: "user_message_chunk"; content: ContentBlock }
  | {
      sessionUpdate: "tool_call";
      toolCallId: string;
      title?: string;
      kind?: string;
      status?: ToolStatus;
    }
  | { sessionUpdate: "tool_call_update"; toolCallId: string; status?: ToolStatus }
  | { sessionUpdate: "config_option_update"; configOptions: ConfigOption[] }
  | { sessionUpdate: string; [field: string]: unknown };

export interface SessionNotification {
  sessionId: string;
  update: SessionUpdate;
}

export interface PermissionOption {
  optionId: string;
  name: string;
  kind: string;
}

export interface RequestPermissionParams {
  sessionId: string;
  toolCall: { toolCallId: string; status?: ToolStatus };
  options: PermissionOption[];
}

export type PermissionOutcome =
  | { outcome: "cancelled" }
  | { outcome: "selected"; optionId: string };

export interface NewSessionResult {
  sessionId: string;
  configOptions?: ConfigOption[];
}

export interface PromptResult {
  stopReason: string;
}

/** `data` de l'erreur d'un prompt échoué (`crates/pa-daemon/src/acp/mod.rs:42-67`). */
export interface TurnFailureData {
  details?: string;
  kind?: string;
  category?: string;
  retryable?: boolean;
}
