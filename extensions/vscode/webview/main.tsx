import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./powder";
import "./ya-probe"; // yusAgent : sonde des tests e2e
import "./styles.css";
import "./ya-settings.css"; // yusAgent (ticket 31) : les écrans Settings, règles d'Unharnessed
import { post } from "./vscode";

// A crash empties the panel: what threw goes to the log.
createRoot(document.getElementById("root")!, {
  onUncaughtError: (error, info) => {
    const text = error instanceof Error ? (error.stack ?? error.message) : String(error);
    post({ type: "crashed", error: `${text}${info.componentStack ?? ""}` });
  },
}).render(<App />);
