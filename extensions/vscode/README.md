# yusAgent pour VS Code

Chat avec l'agent de code yusAgent, par ACP (Agent Client Protocol) sur stdio.

- **Binaire à part** : l'extension lance `yusagent --mode acp` (réglage `yusagent.binaryPath`, nom seul cherché dans le `PATH`). Réglage utilisateur seulement : un workspace ne peut pas choisir le binaire lancé.
- **Un chat = un processus**, lancé dans le premier dossier du workspace. Commande : « yusAgent: Nouveau chat ».
- **Connexion** : l'extension ne gère pas l'authentification. Une fois, dans un terminal : `yusagent`, puis `/login`.
- **Sélecteurs** de modèle et d'effort : rien n'est envoyé à l'ouverture, seulement quand on change une valeur.
- **Bandeau** « Mode auto sans sandbox » quand l'agent annonce `permissions` = `auto`.
- **Plantage** : relance automatique avec une nouvelle session ; deux arrêts en moins de 60 s, plus de relance (bouton « Réessayer »).

Version minimale (YA-10, ticket 24, PR 2) : pas encore de cartes de cellule, de boîte de permission ni de diffs (PR 3). En mode manuel, une demande d'autorisation reste en attente : « Arrêter » l'annule.

## Développement

```sh
cd extensions/vscode
npm ci --ignore-scripts
npm run typecheck && npm run lint && npm run test:unit
```

Le `.vsix` et l'e2e (VS Code téléchargé, xvfb) tournent en CI : workflow `ya-extension`.
