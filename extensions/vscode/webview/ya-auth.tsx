// yusAgent (YA-10, ticket 40, X5) : l'échec d'authentification d'un tour (décision 7, réponse 2 de
// Yusuf du 26/11/2026) : le texte fixe avec le nom du fournisseur, et un bouton vers l'écran
// Providers, au lieu du renvoi au terminal. Le panneau nomme le fournisseur depuis la liste, comme le
// sélecteur de modèles (`providerName`).
import { createContext, useContext } from "react";
import type { Provider } from "../src/protocol";
import { authText } from "../src/ya/auth";
import { providerName } from "./models";

export type AuthScreen = { providers: Provider[]; onProviders: () => void };

export const AuthContext = createContext<AuthScreen>({ providers: [], onProviders: () => undefined });

export function AuthFailure({ provider }: { provider: string | null }) {
  const { providers, onProviders } = useContext(AuthContext);
  return (
    <div className="turn-error">
      <p className="error">{authText(provider ? providerName(providers, provider) : undefined)}</p>
      <div className="actions">
        <button className="button" type="button" onClick={onProviders}>
          Open Providers
        </button>
      </div>
    </div>
  );
}
