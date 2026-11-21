import { createRoot } from "react-dom/client";
import { useEffect, useState } from "react";

import type { Probe, ToHost, ToWebview } from "../src/shared/messages";
import { initialState, type ChatState } from "../src/shared/state";
import { App } from "./App";
import "./styles.css";

declare function acquireVsCodeApi(): { postMessage(message: ToHost): void };

const vscode = acquireVsCodeApi();
const post = (message: ToHost) => vscode.postMessage(message);

function probe(): Probe {
  return {
    text: document.body.innerText,
    selects: Array.from(document.querySelectorAll("select")).map((select) => ({
      label: select.getAttribute("aria-label"),
      value: select.value,
      disabled: select.disabled,
      options: Array.from(select.options).map((option) => option.text),
    })),
  };
}

function Root() {
  const [state, setState] = useState<ChatState>(() => initialState(undefined));
  useEffect(() => {
    const listener = (event: MessageEvent<ToWebview>) => {
      if (event.data?.type === "state") setState(event.data.state);
      if (event.data?.type === "probe") post({ type: "probeResult", probe: probe() });
    };
    window.addEventListener("message", listener);
    post({ type: "ready" });
    return () => window.removeEventListener("message", listener);
  }, []);
  return <App state={state} post={post} />;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Root />);
