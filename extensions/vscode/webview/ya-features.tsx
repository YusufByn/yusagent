// yusAgent (YA-10, ticket 24, PR I) : les fonctions que le panneau montre (message `init`,
// `src/ya/methods.ts`). Une fonction dont le pont ne sert pas la méthode reste masquée.
import { createContext, useContext } from "react";
import type { Features } from "../src/protocol";

export const NO_FEATURES: Features = {
  history: false,
  steer: false,
  rewind: false,
  kernel: false,
  providers: false,
  settings: false,
  attachments: false,
};

export const FeaturesContext = createContext<Features>(NO_FEATURES);

export function useFeatures(): Features {
  return useContext(FeaturesContext);
}
