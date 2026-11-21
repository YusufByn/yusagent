import type { HostMessage, WebviewMessage } from "../src/protocol";

declare function acquireVsCodeApi(): {
  postMessage(message: unknown): void;
  getState(): unknown;
  setState(state: unknown): void;
};

const api = acquireVsCodeApi();

/** What the panel keeps when VS Code hides and rebuilds it: the text being written. */
type Saved = { draft?: string };

export function savedDraft(): string {
  const saved = api.getState() as Saved | undefined;
  return typeof saved?.draft === "string" ? saved.draft : "";
}

export function saveDraft(text: string): void {
  api.setState({ draft: text } satisfies Saved);
}

export function post(message: WebviewMessage): void {
  api.postMessage(message);
}

export function onHostMessage(handler: (message: HostMessage) => void): () => void {
  const listener = (event: MessageEvent<HostMessage>) => handler(event.data);
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
