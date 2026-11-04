Les épisodes ingérés en bloc ne gardent pas la trace des faits qu'ils soutiennent.

1. Après `add_episode_bulk`, chaque épisode enregistré a `entity_edges == []`. Comme `remove_episode` s'appuie sur `entity_edges` pour retrouver les faits d'un épisode, supprimer un épisode ingéré en bloc laisse ses faits en place. Attendu : chaque épisode a dans `entity_edges` les UUID des arêtes (résolues ou invalidées) dont `episodes` contient cet épisode, sans doublon et dans l'ordre des arêtes. Un fait énoncé par deux épisodes du même appel figure dans les `entity_edges` des deux.

2. Quand deux épisodes d'un même appel en bloc énoncent le même fait, la déduplication garde une seule arête, mais celle-ci ne référence que l'un des deux épisodes : supprimer cet épisode efface un fait que l'autre soutient encore. Attendu : l'arête conservée a dans `episodes` les épisodes de toutes les arêtes fusionnées, sans doublon, dans l'ordre (par exemple `[episode_un.uuid, episode_deux.uuid]`).

3. Quand `resolve_extracted_edge` rattache une arête extraite à une arête existante, l'arête renvoyée ne reçoit pas les épisodes de l'arête extraite. Attendu : l'arête existante est renvoyée avec ses épisodes suivis de ceux de l'arête extraite, sans doublon (une seconde résolution identique n'ajoute rien). Pour un appel à un seul épisode, rien ne change.

Les épisodes déjà enregistrés par d'anciens appels n'ont pas à être réparés.
