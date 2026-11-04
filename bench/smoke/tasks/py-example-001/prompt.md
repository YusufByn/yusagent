`word_count` de `textstats` compte trop de mots quand le texte contient plusieurs espaces de suite ou des retours à la ligne : `word_count("un  deux")` renvoie 3 au lieu de 2.
