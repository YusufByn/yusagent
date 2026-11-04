Avec l'extraction des domaines privés activée et la liste de suffixes embarquée (aucun accès réseau), certains noms d'hôte reçoivent le bon suffixe mais des métadonnées fausses.

```python
import tldextract
ext = tldextract.TLDExtract(cache_dir=None, suffix_list_urls=(), include_psl_private_domains=True)
r = ext("r.appspot.com")
```

Observé : `r.suffix == "appspot.com"` mais `r.is_private` vaut `False`, `r.registry_suffix == "appspot.com"` et `r.top_domain_under_registry_suffix == "r.appspot.com"`.

Attendu : `r.suffix == "appspot.com"`, `r.is_private` vaut `True`, `r.registry_suffix == "com"` et `r.top_domain_under_registry_suffix == "appspot.com"`. D'autres noms d'hôte sont touchés de la même façon. Les extractions qui fonctionnent aujourd'hui ne doivent pas changer.
