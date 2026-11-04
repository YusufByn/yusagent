`CookieJar.setCookie` refuse un cookie dont l'attribut `Domain` est une adresse IPv4 :

```js
await jar.setCookie('a=b; Domain=127.0.0.1', 'http://127.0.0.1/')
// Error: Cookie has domain set to a public suffix
```

Une adresse IP n'est pas un suffixe public, et l'équivalent IPv6 (`'a=b; Domain=[::1]'` sur `http://[::1]/`) est accepté.

Attendu : le cookie est accepté avec `domain === '127.0.0.1'` et `getCookies('http://127.0.0.1/')` le renvoie. Même chose pour d'autres adresses IPv4 comme `8.8.8.8`. `Cookie.validate()` d'un cookie analysé depuis `a=b; domain=127.0.0.1` renvoie `true`, de même pour `a=b; domain=[::1]`. Les vrais suffixes publics (`domain=kyoto.jp`) restent refusés.
