# yusAgent pour VS Code

Agent de code yusAgent dans VS Code, par ACP (Agent Client Protocol) sur stdio.

Base : l'extension d'Unharnessed (MIT, `github.com/unharnessed-agent/Unharnessed`, commit `157dcdc1d`, dossier `vscode/`), adaptée à notre binaire. Le visuel est provisoire ; celui de yusAgent viendra plus tard.

- **Où** : une vue « yusAgent » dans la barre latérale secondaire ; bouton dans la barre de titre de l'éditeur, ou commande « yusAgent: Ouvrir yusAgent ».
- **Binaire à part** : l'extension lance `yusagent --mode acp` (réglage `yusagent.binaryPath`, nom seul cherché dans le `PATH`). Réglage utilisateur seulement : un workspace ne peut pas choisir le binaire lancé.
- **Une conversation = un processus**, lancé dans le premier dossier du workspace. Sans dossier ouvert, rien ne démarre.
- **Connexion** : pas encore dans l'extension. Une fois, dans un terminal : `yusagent`, puis `/login`.
- **Sélecteurs** de modèle et d'effort : rien n'est envoyé à l'ouverture, seulement quand on change une valeur.
- **Étiquette** « auto · no sandbox » dans la barre du compositeur quand l'agent annonce `permissions` = `auto`, le défaut du binaire (ticket 30). Passer en manuel : `"permissions": "manual"` dans `~/.yusagent/settings.json`, puis une nouvelle conversation.
- **Plantage** : pas de relance automatique ; la fin du journal du binaire s'affiche, avec « Restart ».
- **Arrêt** : fin de stdin, puis SIGKILL au bout de 10 s ; la session reste dans le démon, qui la met en veille après 90 min.

Pas encore servi par le pont, donc masqué : historique et reprise, message pendant un tour, retour en arrière, panneau du kernel, fournisseurs et login, réglages, skills, MCP, mentions `@fichier` et images. En mode manuel, une demande d'autorisation reste en attente : « Stop » l'annule (boîte en PR 3).

## Développement

```sh
cd extensions/vscode
npm ci --ignore-scripts
npm run typecheck && npm run lint && npm run test:unit
```

Le `.vsix` et l'e2e (VS Code téléchargé, xvfb) tournent en CI : workflow `ya-extension`.
