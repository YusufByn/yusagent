# Ticket 27 : `edit()` et les fins de ligne `\r\n`

Demande de Yusuf du 21/11/2026 (ticket 26, hors périmètre). **Constat seulement** : aucun code, aucun appel de modèle. Pas de plan avant une décision de Yusuf.

## 1. Constat

- `edit()` lit le fichier par `Path.read_text(encoding="utf-8")` (`skills/edit/src/edit/__init__.py:68`) et l'écrit par `write_text` (`:82`), sans `newline=`. En mode texte, Python convertit à la lecture `\r\n` (et un `\r` seul) en `\n` ; à l'écriture, `\n` devient `os.linesep`, donc `\n` sur macOS et Linux.
- Effet : **tout le fichier** passe en fins de ligne `\n`, pas seulement le fragment modifié. Mesure, sur le skill de la branche `ya-26-colonne-diff` (macOS, Python 3.14) :

  ```
  python3 -I crlf_check.py <dépôt> <dossier vide>
  avant : b'un\r\ndeux\r\ntrois\r\n'
  après : b'un\nDEUX\ntrois\n'
  ```

  Script `crlf_check.py` (bibliothèque standard ; le skill appelé sans kernel, `rlm` absent, l'émission du diff est ignorée) :

  ```python
  """edit() sur un fichier à fins de ligne CRLF : octets avant et après (ticket 27)."""
  import asyncio, os, sys
  from pathlib import Path
  repo, work = Path(sys.argv[1]), Path(sys.argv[2])
  sys.path.insert(0, str(repo / "skills" / "edit" / "src"))
  import edit
  os.chdir(work); edit._WORKSPACE = work.resolve()
  f = work / "w.txt"
  f.write_bytes(b"un\r\ndeux\r\ntrois\r\n")
  print("avant :", f.read_bytes())
  print(asyncio.run(edit.run("w.txt", "deux", "DEUX")))
  print("après :", f.read_bytes())
  f.write_bytes(b"un\r\ndeux\r\n")
  try:
      asyncio.run(edit.run("w.txt", "un\r\ndeux", "x"))
  except ValueError as e:
      print("old_str avec \\r\\n :", e)
  ```
- Un `old_str` qui contient `\r\n` n'est jamais trouvé : `string not found in w.txt (0 occurrences)` (même script), alors que le fichier sur disque contient bien ce texte.
- `_numbered` retire un `\r` final de chaque ligne (`:112`) : il ne peut plus y en avoir après la lecture en mode texte.
- Sous Windows, l'écriture redonnerait `\r\n` partout : un fichier en `\n` passerait alors en `\r\n`. Non mesuré (aucune machine Windows dans la boucle).

## 2. Effet sur « Revert » (extension, ticket 24, PR 3, et ticket 26)

- L'extension lit l'« après » sur disque à la fin de la cellule (`extensions/vscode/src/ya/reverts.ts:41`, `record`, lecture `:53`) : il est en `\n`. Les fragments du pont ont été calculés sur le texte converti, ligne et colonne comprises (ticket 26) : l'« avant » retrouvé (`extensions/vscode/src/ya/diffs.ts:25`, `undo`) est cohérent, mais **en `\n`**.
- « Revert » réécrit cet « avant » : le contenu revient, **pas les octets d'origine** ; les fins de ligne `\r\n` du fichier sont perdues. `git diff` montre tout le fichier modifié, avant comme après le « Revert ».
- Le diff montré dans la carte (fragments du pont, `extensions/vscode/src/ya/cells.ts:64`) et dans `vscode.diff` (« avant » retrouvé ↔ « après », `reverts.ts:121`) compare deux textes en `\n` : il ne fait pas voir ce changement.

## 3. Pistes (à trancher, aucune choisie)

- Garder les fins de ligne du fichier dans `edit()` (lecture et écriture avec `newline=""`), avec les conséquences sur la recherche d'`old_str` (le modèle envoie `\n`) et sur `start_line` / `start_column`.
- Ou refuser / signaler un fichier en `\r\n` dans le retour d'`edit()`.
- Ou garder le comportement et le dire dans l'extension (« Revert » d'un fichier qui était en `\r\n`).

Fichier upstream concerné : `skills/edit/src/edit/__init__.py` (déjà modifié par le fork, `docs/fork-changes.md`). Toute piste qui change le texte de retour d'`edit()` change ce que voit le modèle : à mesurer par capture.
