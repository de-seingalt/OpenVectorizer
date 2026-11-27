# Provenance du corpus

Tout est reconstruit par `scripts/fetch-corpus.sh` à partir de sources épinglées et **libres de droits**.
La normalisation se limite à : taille de rendu fixée, `currentColor` remplacé, fond blanc pour les icônes.
Ces fichiers servent uniquement de données de test et ne sont pas distribués avec l'application.
Les logos peuvent être des marques de leurs propriétaires : ils sont utilisés ici uniquement comme cas de test.

| Dossier | Source | Commit | Licence |
|---|---|---|---|
| `flags/` | [lipis/flag-icons](https://github.com/lipis/flag-icons) | `4612960f` | MIT |
| `illustrations/noto-*` | [googlefonts/noto-emoji](https://github.com/googlefonts/noto-emoji) (`2D/svg`) | `99701e07` | Apache-2.0 |
| `illustrations/fluent-*` | [microsoft/fluentui-emoji](https://github.com/microsoft/fluentui-emoji) | `1ffb34c7` | MIT |
| `icons/lucide-*` | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) | `9cb3934c` | ISC |
| `icons/phosphor-*` | [phosphor-icons/core](https://github.com/phosphor-icons/core) | `2b75f3ad` | MIT |
| `logos/`, `emblems/`, `signatures/`, `pictograms/`, `drawings/`, `photos/` | Wikimedia Commons, liste : [`commons.tsv`](commons.tsv), crédits fichier par fichier : [`COMMONS.md`](COMMONS.md) | — | domaine public / CC0 (vérifié automatiquement) |
| `lettering/`, `pixelart/` | créés pour le projet (`scripts/gen_corpus.py`) | — | MIT OR Apache-2.0 |
| `../fonts/` | [google/fonts](https://github.com/google/fonts) : Pacifico, Caveat, Bebas Neue, Abril Fatface | `50268007` | OFL-1.1 (licences jointes) |
