import { useEffect, useState } from "react";
import type { AgentState } from "../src/protocol";
import { AsciiField } from "./AsciiField";
import { reducedMotion } from "./motion";
import { Wordmark } from "./Wordmark";

const MIN_MS = 1800;
const MIN_MS_REDUCED = 600;
/** Matches the `.splash.is-leaving` transition. */
const LEAVE_MS = 450;

/** Opening screen: stays at least MIN_MS, and until the agent has answered. */
export function Splash({ agent, onDone }: { agent: AgentState; onDone: () => void }) {
  const [minElapsed, setMinElapsed] = useState(false);
  const leaving = minElapsed && agent.kind !== "starting";

  useEffect(() => {
    const timer = setTimeout(() => setMinElapsed(true), reducedMotion() ? MIN_MS_REDUCED : MIN_MS);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(onDone, LEAVE_MS);
    return () => clearTimeout(timer);
  }, [leaving, onDone]);

  return (
    <div className={leaving ? "splash is-leaving" : "splash"}>
      <AsciiField />
      <div className="splash-center">
        <Wordmark animated />
      </div>
    </div>
  );
}
