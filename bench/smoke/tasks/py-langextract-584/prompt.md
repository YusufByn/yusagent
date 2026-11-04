Un document annoté sans aucune extraction ne survit pas à une sauvegarde suivie d'une relecture.

Un `AnnotatedDocument` dont `extractions` vaut `None` (la valeur par défaut) s'enregistre sans erreur avec `io.save_annotated_documents`, mais sa relecture avec `io.load_annotated_documents_jsonl` lève `TypeError: 'NoneType' object is not iterable`. La conversion d'un dictionnaire `{"extractions": None, ...}` en document échoue de la même façon.

Attendu : sauvegarde puis relecture rendent le document, avec `extractions` toujours à `None`. Un dictionnaire sans clé `extractions` continue de donner `extractions == []`, et les documents qui ont des extractions se relisent comme avant.
