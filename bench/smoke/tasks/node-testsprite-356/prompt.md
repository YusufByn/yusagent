Sous Windows, après `testsprite setup`, toutes les commandes échouent avec `EPERM: operation not permitted` à la lecture de `~/.testsprite/credentials` : la restriction des droits du fichier retire aussi l'accès de son propriétaire. Les tests tournent sous macOS ou Linux, les appels système Windows y sont simulés.

1. **Restriction des droits sous Windows.** `ensureRestrictiveMode(path, { platform, spawnSync, warn })` doit, quand `platform` vaut `'win32'`, appeler `icacls` en deux étapes, toujours avec les options `{ shell: false, stdio: 'ignore', windowsHide: true }` : d'abord `icacls <path> /reset`, puis, seulement si la première a réussi, `icacls <path> /inheritance:r /grant:r *S-1-3-4:F`. En cas d'échec, une étape émet un avertissement par `warn` et n'enchaîne pas :
   - erreur au lancement de la première : un message contenant `icacls failed while resetting credentials file permissions` ;
   - code de sortie non nul de la première : `icacls /reset exited with status <code>` ;
   - erreur au lancement de la seconde : un message contenant `icacls failed while tightening credentials file permissions` ;
   - code de sortie non nul de la seconde : `icacls exited with status <code>`.
   Sans `warn` fourni, l'avertissement va sur la sortie d'erreur, préfixé de `[warning]`.

2. **`testsprite doctor` face à un fichier d'identifiants illisible** (erreur `EPERM` ou `EACCES` à la lecture). `runDoctor` accepte dans ses dépendances (`DoctorDeps`) une fonction de chargement de la configuration injectable, `loadConfigFn` (même signature que `loadConfig`), utilisée à la place de `loadConfig` quand elle est fournie :
   - sans variable `TESTSPRITE_API_KEY`, la vérification `Credentials` échoue avec un message qui contient `cannot be read (EPERM)` (ou le code reçu) et `delete the file and re-run \`testsprite setup\``, et la commande se termine en erreur ;
   - avec `TESTSPRITE_API_KEY` définie, la même vérification devient un avertissement (`[WARN]`) qui contient `cannot be read (<code>)` et `TESTSPRITE_API_KEY is set`, sans compter d'échec ; les vérifications de connectivité et de tunnel local passent (statut `ok`) en utilisant la clé de l'environnement, et le texte brut de l'erreur système (par exemple `EACCES: permission denied`) n'apparaît nulle part dans le rapport.
