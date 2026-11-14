# Ticket 10 : tool shell natif (`shellTool: "native"`), à reprendre s'il revient

Ticket CDC : YA-03 (avec YA-14 et YA-17). **En attente, non planifié.** La variante a été mesurée par l'A/B 2 du 14/11/2026 (`bench/smoke/ab/ya03-ab2-20261114.md`). Verdict : `shellTool: kernel` reste le défaut, provisoire avec une seule famille de modèles. Ce ticket note ce qu'il faudrait reprendre si la variante revient, par exemple pour une autre famille de modèles ou au banc complet. Rien n'y est à faire tant qu'elle n'est pas rouverte par une décision.

## État

- Le tool existe en réglage (`shellTool: "native"`, famille Codex seulement) : `crates/pa-core/src/tools/ya_exec.rs`, d'après `exec_command` et `write_stdin` de Codex (`codex-rs/core/src/tools/handlers/shell_spec.rs`, SHA `69231e1`).
- A/B 2 contre `bash()` dans le kernel :
  - résolution 25 contre 25 ;
  - coût à cache théorique des tâches stables −2,9 % [−14,0 ; +10,7] ;
  - aucune rupture de la suite de conversation ;
  - aucune erreur du tool.
- L'écart de coût est du même ordre que le bruit entre deux runs identiques (+2,1 % [−4,4 ; +6,8]).

## Points constatés à l'A/B 2

1. **La description annonce un PTY absent.**
   - `EXEC_COMMAND_DESCRIPTION` vaut « Runs a command in a PTY, returning output or a session ID for ongoing interaction. » (`crates/pa-core/src/tools/ya_exec.rs:53-54`). Le texte est repris mot pour mot de Codex.
   - Or le tool n'a pas de PTY : `tty` est retiré, stdin est fermé, stdout et stderr sont fusionnés (`ya_exec.rs:8-11`, `:13-15`).
   - Le modèle se voit donc promettre un terminal interactif qu'il n'a pas.
2. **`yield_time_ms` à 1000 dans 99 appels `exec_command` sur 115.**
   - Le défaut est 10 000 ms (`ya_exec.rs:60`), mais le modèle fixe presque toujours 1 s.
   - Résultat : 64 `exec_command` sur 115 rendent la main avant la fin du processus.
3. **Une sonde `write_stdin` attend au moins 5 s.**
   - Avec `chars` vide, l'attente est bornée entre 5 000 et 300 000 ms (`ya_exec.rs:63`, `:65`, `:279-280`), comme dans Codex d'après `ya_exec.rs:13-20`.
   - Le modèle demande pourtant 1000 ms dans les 103 `write_stdin`.
4. **D'où 103 `write_stdin`**, soit autant de tours, pour 115 `exec_command`.
   - Le prompt de la variante dit de suivre une commande encore en cours par `write_stdin` à `chars` vide (`crates/pa-core/src/prompts/ya_tools_prompt.rs:55`). Il ne dit rien de `yield_time_ms`.
   - Cas extrême : `node-testsprite-356`, passe 2, bras B. La suite complète `npm run lint && npm test` est sondée 16 fois, toutes les 5 s environ, pendant 2 min (27 tours, 218 s contre 97 à 130 s pour les autres bras de la tâche).
   - Dans le bras kernel, le même cas se règle par la notification de fin d'un `bash()` (règle `USAGE_POLL_RULE`, `ya_tools_prompt.rs:47`).

Comptes : `python3 -I bench/smoke/ab/ya03-ab2-20261114-mesures.py bench/smoke/runs/ab-20261114T140538Z bench/smoke/runs/ab-20261114T120431Z bench/smoke/baseline/gpt-6.1-terra.json`, sections « Par bras » et « Anomalies ».

## Pistes, si la variante revient

À décider le moment venu. Chaque piste change le préfixe de prompt ou les définitions de tools de la variante, et se mesure par un A/B.

- Une description qui ne promet pas de PTY. C'est un écart avec le texte de Codex, sur lequel le modèle a été entraîné : à mesurer, pas à supposer.
- Une règle de prompt sur `yield_time_ms` (une valeur haute pour les tests et les builds), ou un plancher côté tool.
- Revoir le plancher de 5 s de la sonde vide, au regard des tours qu'il fait gagner ou perdre.
- Reprendre aussi les biais déjà notés au plan (`bench/smoke/ab/ya03-plan.md`, « A/B 2 ») :
  - `tty`, `shell` et `login` retirés ;
  - règle « pas de boucles shell ni de heredocs » retirée en même temps ;
  - sortie comptée en octets.
