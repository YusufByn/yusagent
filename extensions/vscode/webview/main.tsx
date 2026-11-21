import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./powder";
import "./styles.css";
import { post } from "./vscode";

// A crash empties the panel: what threw goes to the log.
createRoot(document.getElementById("root")!, {
  onUncaughtError: (error, info) => {
    const text = error instanceof Error ? (error.stack ?? error.message) : String(error);
    post({ type: "crashed", error: `${text}${info.componentStack ?? ""}` });
  },
}).render(<App />);
