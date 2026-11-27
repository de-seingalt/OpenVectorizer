# Banc d'essai

```sh
cargo run --release -p openvectorizer-cli -- eval testdata --baseline testdata/baseline.json
xdg-open target/eval/index.html     # rapport visuel : entrée / SVG / carte d'erreur, du pire au meilleur
```

## Principe
Chaque **SVG de référence** est rendu en image : c'est la vérité terrain. Des **variantes dégradées** de ce rendu
sont données au moteur, et le SVG produit est toujours comparé au rendu **propre**. C'est le cas réel d'un
utilisateur qui fournit un logo abîmé et attend le logo net.

| Variante | Simule |
|---|---|
| `clean` | image parfaite |
| `jpeg75` / `jpeg35` | compression JPEG web / forte |
| `half` | image en basse résolution (moitié) ; le SVG doit égaler la référence pleine taille |
| `blur` | flou (scan, capture adoucie) |
| `noise` | bruit (photo d'un imprimé, capteur) |

Métriques : ΔE moyen et p95 (écart de couleur perceptuel, Oklab ×100 ; < 1 invisible, > 5 net), nombre de
nœuds (simplicité d'édition). Le rapport est ventilé par catégorie et par variante.

## Contenu
Le corpus couvre les cas d'usage décrits dans [docs/CAS-USAGE.md](../docs/CAS-USAGE.md) :
90 sources, soit 515 cas une fois les dégradations appliquées (environ 1 min en release).

| Dossier | Contenu |
|---|---|
| `synthetic/` | formes de test, traits fins, couleurs adjacentes, petite icône, graphique |
| `corpus/logos/` | 15 logos (lettrage script, rayures, espace négatif, dégradés, logo-illustration) |
| `corpus/emblems/` | 6 blasons et sceaux très détaillés |
| `corpus/lettering/` | 6 compositions typographiques (script, manuscrit, condensé, didone, petit texte, badge) |
| `corpus/signatures/` | 4 signatures manuscrites |
| `corpus/pictograms/` | 5 pictogrammes noir et blanc (découpe, pochoir) |
| `corpus/illustrations/` | 18 emojis (aplats, dégradés, trait noir et blanc) |
| `corpus/icons/` | 11 icônes (trait, plein, deux tons) |
| `corpus/flags/` | 10 drapeaux |
| `corpus/pixelart/` | 3 sprites |
| `corpus/drawings/`, `corpus/photos/` | gravures et photos réelles (sans vérité vectorielle) |
| `fonts/` | polices OFL utilisées pour rendre `lettering/` |

Provenance et licences : [corpus/SOURCES.md](corpus/SOURCES.md). Tout se reconstruit avec `scripts/fetch-corpus.sh`.

- Ajouter un cas = déposer un fichier. Pour un SVG, la balise racine doit avoir `width` et `height`.
- Images réelles : uniquement des images libres de droits, avec leur source dans `images/SOURCES.md`.
  **Aucune image personnelle ou confidentielle** : le dépôt est public.
- `baseline.json` est la référence. On la régénère volontairement, dans un commit dédié, quand une amélioration
  est validée : `cp target/eval/report.json testdata/baseline.json`.
- Filtrer : `--filter flags`, `--variants clean,jpeg35`.
