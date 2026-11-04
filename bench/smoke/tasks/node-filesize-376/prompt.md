L'option `locale` est ignorée dès que l'option `precision` est donnée :

```js
filesize(1234, { locale: "sv", precision: 2 })   // "1.2 kB"
```

Attendu : `"1,2 kB"`, comme sans `precision`. D'autres exemples attendus : `{ locale: "de-DE", precision: 2 }` donne `"1,2 kB"`, `{ locale: "en-US", precision: 2 }` donne `"1.2 kB"`, `{ locale: "sv", precision: 3 }` donne `"1,23 kB"`, et `filesize(-1234, { locale: "sv", precision: 2 })` donne `"−1,2 kB"`. Une précision qui produit zéro garde ses zéros (par exemple `0.00`).
