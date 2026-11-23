// yusAgent (YA-10, ticket 28, D2) : une conversation ouverte neuve et quittée vide est supprimée,
// comme chez Unharnessed où il n'en reste rien (pas de fichier avant le premier message, processus
// arrêté). Notre démon écrit le fichier dès `session/new` (M1) et lance un kernel par session
// (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1201`) : sans suppression, le worker et son kernel
// restent jusqu'à la veille du démon (90 min). La suppression (`_unharnessed/sessions/delete`, R1)
// arrête la session dans le démon puis retire le fichier ; vérifiée sur une session sans message
// (`bench/tools/capture/empty_delete_probe.py`, binaire 9937e4ff8 : CONFORME). Elle part par l'agent
// de la conversation qui la remplace, une fois sa session ouverte ; son agent à elle est déjà arrêté.
// Depuis le défaut 2 du contrôle de X1, le pont d'une conversation ouverte neuve et vide la supprime
// lui-même à la fin de stdin (`drop_if_empty`, `crates/pa-daemon/src/acp/ya_history.rs`) : c'est ce
// qui couvre la fermeture de VS Code. La demande d'ici reste pour un pont tué avant sa fin ; quand le
// pont l'a déjà fait, elle trouve la conversation partie.

import type * as vscode from "vscode";
import { errorText } from "../agent";
import type { Conversation } from "../conversations";

const DELETE = "_unharnessed/sessions/delete";
/** La réponse du pont quand le fichier n'est plus là (`ya_history.rs`, suppression). */
const GONE = "No saved conversation";

export function dropEmpty(next: Conversation, id: string, log: vscode.LogOutputChannel): Promise<void> {
  return new Promise((resolve) => {
    let done = false;
    const finish = (line: string, warn = false) => {
      done = true;
      listener.dispose();
      if (warn) log.warn(line);
      else log.info(line);
      resolve();
    };
    const look = () => {
      if (done) return;
      const state = next.session.current;
      if (state?.kind === "open") {
        done = true;
        listener.dispose();
        next.agent.request(DELETE, { sessionId: id }).then(
          () => {
            log.info(`conversations: empty ${id} deleted`);
            resolve();
          },
          (error: unknown) => {
            const text = errorText(error);
            if (text.includes(GONE)) log.info(`conversations: empty ${id} deleted by its agent`);
            else log.warn(`conversations: empty ${id} not deleted: ${text}`);
            resolve();
          },
        );
      } else if (state?.kind === "failed" || next.agent.current.kind === "failed") {
        finish(`conversations: empty ${id} not deleted: the next conversation did not open`, true);
      }
    };
    const listener = next.session.onChange(look);
    look();
  });
}
