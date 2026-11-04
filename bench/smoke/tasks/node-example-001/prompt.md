`slugify` dans `src/slug.js` produit des slugs avec des tirets en double et des tirets en début ou en fin : `slugify("Hello,  World!")` donne `hello---world-` au lieu de `hello-world`.
