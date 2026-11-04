`ExtractResult.reverse_domain_name` produit un point en trop quand le suffixe ou le domaine est vide :

```python
import tldextract
tldextract.extract("localhost").reverse_domain_name   # '.localhost'
tldextract.extract("127.0.0.1").reverse_domain_name   # '.127.0.0.1'
tldextract.extract("com").reverse_domain_name         # 'com.'
tldextract.extract("").reverse_domain_name            # '.'
```

Attendu : `'localhost'`, `'127.0.0.1'`, `'com'` et `''`. Une notation de domaine inversée n'a jamais de point au début, à la fin ni en double. Les domaines ordinaires ne changent pas : `login.example.co.uk` donne toujours `'co.uk.example.login'`.
