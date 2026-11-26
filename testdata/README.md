# Corpus de test

- `synthetic/*.svg` : vérités terrain. Elles sont rendues en image (resvg), vectorisées, puis comparées au rendu d'origine. Elles mesurent la précision géométrique.
- `images/` : images réelles (PNG/JPG/WebP). Ajoutez uniquement des images libres de droits (indiquer la source dans `images/SOURCES.md`).
- `baseline.json` : rapport de référence. À régénérer volontairement (`just eval-baseline`) quand une amélioration est validée.

Ajouter un cas = ajouter un fichier. Garder le corpus petit (< 30 s d'évaluation).
