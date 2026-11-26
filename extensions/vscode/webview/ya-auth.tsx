// yusAgent (YA-10, ticket 40, X5) : l'échec d'authentification d'un tour (décision 7, réponse 2 de
// Yusuf du 26/11/2026) : le texte fixe avec le nom du fournisseur, et un bouton vers l'écran
// Providers, au lieu du renvoi au terminal. Le panneau nomme le fournisseur depuis la liste
// (`authProviderName`, `src/ya/auth.ts`).
import { createContext, useContext } from "react";
import type { Provider } from "../src/protocol";
import { authProviderName, authText } from "../src/ya/auth";

export type AuthScreen = { providers: Provider[]; onProviders: () => void };

export const AuthContext = createContext<AuthScreen>({ providers: [], onProviders: () => undefined });

export function AuthFailure({ provider }: { provider: string | null }) {
  const { providers, onProviders } = useContext(AuthContext);
  return (
    <div className="turn-error ya-settings">
      <p className="error">{authText(authProviderName(providers, provider))}</p>
      <div className="actions">
        <button className="button" type="button" onClick={onProviders}>
          Open Providers
        </button>
      </div>
    </div>
  );
}
