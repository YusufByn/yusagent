// yusAgent (YA-10, ticket 40, X5) : l'échec d'authentification d'un tour (décision 7, réponse 2 de
// Yusuf du 26/11/2026) : le texte fixe, et un bouton vers l'écran Providers, au lieu du renvoi au
// terminal. Le texte est celui que l'hôte a figé au moment de l'échec (`turn.error`, D6) : il ne
// change pas quand la liste des fournisseurs change ensuite.
import { createContext, useContext } from "react";
import type { TurnHead } from "../src/protocol";
import { authFailureText } from "../src/ya/auth";

export type AuthScreen = { onProviders: () => void };

export const AuthContext = createContext<AuthScreen>({ onProviders: () => undefined });

export function AuthFailure({ turn }: { turn: Pick<TurnHead, "auth" | "error"> }) {
  const { onProviders } = useContext(AuthContext);
  return (
    <div className="turn-error ya-settings">
      <p className="error">{authFailureText(turn)}</p>
      <div className="actions">
        <button className="button" type="button" onClick={onProviders}>
          Open Providers
        </button>
      </div>
    </div>
  );
}
