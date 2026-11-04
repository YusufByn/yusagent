Avec l'extraction des domaines privés activée et la liste de suffixes embarquée (aucun accès réseau), certains noms d'hôte reçoivent le bon suffixe mais des métadonnées fausses.

```python
import tldextract
ext = tldextract.TLDExtract(cache_dir=None, suffix_list_urls=(), include_psl_private_domains=True)
r = ext("r.appspot.com")
```

Observé : `r.suffix == "appspot.com"` mais `r.is_private` vaut `False`, `r.registry_suffix == "appspot.com"` et `r.top_domain_under_registry_suffix == "r.appspot.com"`.

Attendu : `r.suffix == "appspot.com"`, `r.is_private` vaut `True`, `r.registry_suffix == "com"` et `r.top_domain_under_registry_suffix == "appspot.com"`. La liste contient les règles privées `appspot.com` et `*.r.appspot.com` ; `r.appspot.com` ne satisfait pas la seconde, c'est donc `appspot.com` qui s'applique. Le même défaut touche d'autres noms qui commencent comme une règle plus longue de la liste sans la satisfaire. Les extractions qui fonctionnent aujourd'hui ne doivent pas changer.
