// Client ACP : les seules trames que l'extension envoie (ticket 24 §5.4).
// - `initialize` : capacités client toutes à false, pas de `clientInfo` (rien de volatil) ;
// - `session/new` : `mcpServers: []` ;
// - `session/prompt` : blocs `text` seulement ;
// - `session/set_config_option` : seulement sur une action de l'utilisateur (E7) ;
// - `session/cancel`, `session/close`.
// Le fixture `test/fixtures/acp-frames.json` fige les trames de l'ouverture et d'un prompt.

import { Connection, type Handlers } from "./connection";
import {
  PROTOCOL_VERSION,
  type ConfigOption,
  type NewSessionResult,
  type PromptResult,
} from "./types";

export const INITIALIZE_PARAMS = {
  protocolVersion: PROTOCOL_VERSION,
  clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
};

export class AcpClient {
  readonly connection: Connection;

  constructor(write: (line: string) => void, handlers: Handlers) {
    this.connection = new Connection(write, handlers);
  }

  feed(chunk: string): void {
    this.connection.feed(chunk);
  }

  async initialize(): Promise<{ protocolVersion?: number }> {
    return (await this.connection.request("initialize", INITIALIZE_PARAMS)) as {
      protocolVersion?: number;
    };
  }

  newSession(cwd: string): Promise<NewSessionResult> {
    return this.connection.request<NewSessionResult>("session/new", { cwd, mcpServers: [] });
  }

  prompt(sessionId: string, text: string): Promise<PromptResult> {
    return this.connection.request<PromptResult>("session/prompt", {
      sessionId,
      prompt: [{ type: "text", text }],
    });
  }

  cancel(sessionId: string): void {
    this.connection.notify("session/cancel", { sessionId });
  }

  async setConfigOption(
    sessionId: string,
    configId: string,
    value: string,
  ): Promise<ConfigOption[] | undefined> {
    const result = await this.connection.request<{ configOptions?: ConfigOption[] }>(
      "session/set_config_option",
      { sessionId, configId, value },
    );
    return result?.configOptions;
  }

  close(sessionId: string): Promise<unknown> {
    return this.connection.request("session/close", { sessionId });
  }
}
