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
| Dossier | Contenu | Source |
|---|---|---|
| `synthetic/` | formes géométriques, badges, traits fins, couleurs adjacentes, petite icône | créé pour le projet |
| `corpus/flags/` | 10 drapeaux (aplats, étoiles, détails fins, courbes) | flag-icons, MIT |
| `corpus/emoji/` | 10 emojis (illustrations, dégradés, transparence) | Noto Emoji, Apache-2.0 |
| `corpus/icons/` | 6 icônes au trait (traits arrondis) | Lucide, ISC |
| `images/` | images réelles sans vérité terrain (variante `clean` seulement) | voir `images/SOURCES.md` |

- Ajouter un cas = déposer un fichier. Pour un SVG, la balise racine doit avoir `width` et `height`.
- Images réelles : uniquement des images libres de droits, avec leur source dans `images/SOURCES.md`.
  **Aucune image personnelle ou confidentielle** : le dépôt est public.
- `baseline.json` est la référence. On la régénère volontairement, dans un commit dédié, quand une amélioration
  est validée : `cp target/eval/report.json testdata/baseline.json`.
- Filtrer : `--filter flags`, `--variants clean,jpeg35`.
