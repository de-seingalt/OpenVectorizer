# Cas d'usage et couverture du banc d'essai

Fréquence estimée d'après les services du marché et leurs pages d'exemples (vectorizer.ai, Vector Magic, outils de découpe) :
★★★ très courant, ★★ courant, ★ occasionnel.

| # | Cas d'usage | Fréq. | Exemple concret | Ce qui est difficile | Corpus |
|---|---|---|---|---|---|
| 1 | **Logo à re-vectoriser** | ★★★ | Logo récupéré en JPEG basse définition sur un site, à imprimer sur un t-shirt, un véhicule ou une enseigne | Artefacts JPEG, faible résolution, aplats à reconstituer, coins vifs, cercles parfaits | `logos/` × toutes les variantes |
| 2 | **Lettrage et typographie** | ★★★ | Logo typographique, enseigne, texte d'une affiche | Empattements fins, petits corps, contrepoinçons (intérieur des « o », « e »), jonctions de script | `lettering/`, logos script (`coca-cola`, `disney`, `kellogg-s`, `ford`) |
| 3 | **Emblèmes, blasons, sceaux** | ★★ | Écusson de club, blason municipal, badge d'association | Très nombreux petits détails, nombreuses couleurs, symétrie | `emblems/` |
| 4 | **Dessin à la main / croquis** | ★★ | Dessin à l'encre scanné ou photographié, à transformer en illustration | Hachures, trait d'épaisseur variable, grain du papier | `drawings/` (gravures), `illustrations/fluent-*-high-contrast` |
| 5 | **Signature, écriture manuscrite** | ★★ | Signature numérisée pour un document, lettrage manuscrit pour un faire-part | Traits fins et continus, boucles, pression variable | `signatures/`, `lettering/handwritten-note` |
| 6 | **Découpe / gravure / tampon** (vinyle, laser, CNC, broderie) | ★★ | Pochoir, sticker, gravure laser, tampon encreur | Noir et blanc pur, contours fermés, pas de micro-formes, arcs propres | `pictograms/`, `illustrations/fluent-*-high-contrast` |
| 7 | **Illustration / clipart / mascotte** | ★★ | Personnage, mascotte, illustration plate pour un site | Nombreuses couleurs, dégradés, transparence, contours | `illustrations/` |
| 8 | **Icônes et pictogrammes** | ★★ | Recréer une icône pour une interface ou une signalétique | Traits d'épaisseur constante, extrémités arrondies, grille | `icons/` |
| 9 | **Pixel art / sprite** | ★ | Agrandir un sprite de jeu sans flou | Garder des carrés exacts, ne pas lisser | `pixelart/` |
| 10 | **Schéma / graphique / capture** | ★ | Refaire un graphique ou un schéma d'une capture d'écran | Droites parfaites, alignements, couleurs exactes | `synthetic/07-chart`, `synthetic/` |
| 11 | **Photo → effet poster** | ★ | Effet affiche, illustration stylisée à partir d'une photo | Réduction des couleurs, dégradés, détail utile vs bruit | `photos/` |
| 12 | **Drapeaux et symboles officiels** | ★ | Drapeau pour une présentation, une carte | Aplats nets, étoiles, petits détails | `flags/` |

## Dégradations simulées
Chaque SVG de référence est testé propre et dégradé : `jpeg75`, `jpeg35` (logo du web), `half` (basse résolution),
`blur` (scan, photo floue) et `noise` (photo d'un imprimé). Les images raster (`drawings/`, `photos/`) n'ont pas de
vérité vectorielle : elles servent de cas réels, comparés à eux-mêmes.

## Manques connus (à compléter)
- **Photos de logos dans le monde réel** (enseigne photographiée en perspective, logo sur un t-shirt). Difficile à trouver en domaine public.
- **Croquis au crayon photographiés** (ombres, papier non uniforme).
- **Captures d'écran d'interface** (texte d'interface anticrénelé, ClearType).
- **Texte non latin** (arabe, CJK, cyrillique).
