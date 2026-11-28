# Feuille de route

Le score de référence est `testdata/baseline.json` (ΔE moyen, p95, nombre de nœuds).
Chaque étape doit l'améliorer, ou au moins ne pas le dégrader.

## M0 — Socle (fait)
- [x] Moteur Rust avec VTracer comme base (`crates/core/src/baseline.rs`)
- [x] Banc d'évaluation (`openvectorizer eval`) + corpus synthétique + cartes d'erreur
- [x] Interface : dépôt/collage/ouverture, comparaison avant/après avec curseur, zoom/déplacement, réglages, export SVG
- [x] Tauri 2, CI, publication web et binaires
- [x] Banc d'essai : 32 SVG de référence × 6 dégradations (JPEG, basse résolution, flou, bruit) + rapport HTML
- [ ] Compléter le corpus : images réelles libres de droits (photos, captures, pixel art, texte)
- [ ] Interface responsive (mobile : panneau de réglages sous l'aperçu, en tiroir)
- [ ] PWA (service worker) pour un usage web hors ligne

## M1 — Palette et carte planaire (frontières partagées)
- [x] Quantification k-means en Oklab, nombre de couleurs automatique (fusion selon ΔE et surface) — `palette.rs`
- [x] Pixels d'anticrénelage rattachés à la région voisine, et non transformés en nouvelles couleurs
- [x] Nettoyage des étiquettes (taches, îlots) — ΔE 3,28 → 1,71, p95 17,7 → 7,5
- [x] Graphe planaire : chaque frontière entre deux régions est stockée une seule fois, avec ses jonctions — `planar.rs`
- [x] Écriture SVG à partir du graphe : modes découpé (cutout) et empilé, sans jour entre couleurs ; ajustement Bézier (Schneider) + coins — `fit.rs`. ΔE 1,71 → 1,31, p95 7,5 → 5,0 ; VTracer ne sert plus qu'au préréglage Photo
- [x] Interface : palette affichée et nombre de couleurs réglable (automatique ou 2 à 32) ; noms accessibles pour tous les curseurs

- [x] Éditeur de palette (comme vectorizer.ai) : teintes d'origine regroupées en couleurs de sortie ; glisser une teinte vers une autre couleur, dans le vide (nouvelle couleur), fusionner deux couleurs, changer une couleur ; réglette du nombre maximal de couleurs
- [ ] Palette : retirer une couleur du résultat (transparence), afficher la sélection dans l'aperçu

Les outils de référence sont décrits dans `docs/OUTILS-REFERENCE.md`.

## M2 — Bords sous-pixel et coins
- [x] Bords placés au point de transition à 50 % entre couleurs voisines (anticrénelage) ; tangentes de découpe sur une fenêtre large et pas de facettes droites dans les courbes : fin des ondulations
- [x] Jonctions placées entre les pixels (moyenne des points des arêtes qui s'y rejoignent)
- [x] Coins : écart au sommet théorique (droites ajustées de part et d'autre), sommet placé entre les pixels,
      rayon ajusté et rayon max (moitié des côtés), arcs tangents ; les coins du cadre de l'image sont ignorés
- [x] Outil **Coins** : auto / tous vifs / tous ronds, curseur « nombre de coins vifs » classés par écart,
      rayon relatif −100..100
- [x] Coins coin par coin : repères dans l'aperçu (clic = sélection, double-clic = vif/arrondi), rayon en px,
      retouches retrouvées par la position du sommet à chaque tracé (`corners.edits`)
- [x] Groupes de coins : coins d'un même contour fermé de même angle (carré, pointes d'étoile), retouchés ensemble
      par défaut. Les groupes de formes géométriques et de symétries viendront avec M3
- [x] Bords sous-pixel mesurés avec les teintes sources du groupe (une teinte fusionnée ne fausse plus la position)

## M3 — Géométrie ✅
- [x] Droites : parties droites détectées (écart ≤ 0,45 px, pas de courbure systématique, ≥ 6 px, pas un morceau
      d'arc), tracées en segments, les courbes voisines leur sont tangentes ; coins du cadre toujours vifs
- [x] Raccords : entre deux droites, arc de cercle tangent aux deux (rayon ajusté aux points, écart ≤ 0,5 px),
      les droites s'arrêtent aux points de contact
- [x] Rayons égaux : rayons de raccord proches (4 % ou 0,3 px) égalisés sur toute l'image (deuxième passe)
- [x] Curseur **Géométrie** (0 = tracé brut … 100 %) : dose raccords, cercles, ellipses et symétries
- [x] **Guides** affichés dans l'aperçu (droites, arcs, cercles, ellipses, axes de symétrie) ; un clic en désactive
      un (le tracé l'ignore), retrouvé par type et position à chaque tracé (`disabledGuides`)
- [x] Guides **candidats** (trouvés avec une tolérance plus large que le réglage) : affichés en pointillés, un clic
      les **force** (`forcedGuides`)
- [x] Arcs libres : une portion courbe qui suit un cercle (centre sur la médiatrice de ses extrémités, écart ≤ la
      tolérance des arcs, ≥ 25°, tangentes imposées respectées à 4° près) devient cet arc ; guide « Arc »
- [x] Formes entières (contours isolés) : cercle (Kåsa) et ellipse (Fitzgibbon / Halíř-Flusser) exacts si l'écart
      reste ≤ 0,6 px (RMS ≤ 0,22) ; le cercle l'emporte à qualité égale. Réglage « Cercles et ellipses exacts ».
      Rectangles, polygones et étoiles : coins vifs + droites
- [x] Cercles et ellipses sur le **contour complet d'une région**, même coupé par plusieurs couleurs : chaque morceau
      devient la portion exacte de la forme entre ses jonctions (qui ne bougent pas) ; à conflit, la meilleure forme
- [x] Choix « tracé ou forme » forme par forme : guides désactivables / forçables
- [x] Symétries des formes isolées : axe miroir par le centroïde et rotation d'ordre 2..12, contour remplacé par la
      moyenne de ses images (écart ≤ 0,7 px) ; coins candidats reportés sur leurs images, décision « auto » commune
      à un groupe de coins (majorité) ; pointes aiguës vives si leur rayon ajusté ≤ 1 px
- [x] Formes répétées : formes isolées de même aire et périmètre (5 %), superposables par rotation (Procrustes,
      écart ≤ 0,8 px) remplacées par leur forme moyenne replacée sur chacune ; guide « Répétition » par copie
- [x] Coins et géométrie unifiés : un arrondi tangent entre deux droites est un coin (sommet = intersection des
      droites) ; vif = droites prolongées, rayon = arc tangent aux droites, inclus dans « tous vifs » et le compte
- [x] Coins dessinés par leurs deux côtés près du sommet (vert vif, orange arrondi, bleu retouché) ; poignée sur l'arc
      du coin sélectionné, à faire glisser pour régler le rayon (jusqu'au sommet : vif)
- [x] Symétries et répétitions des formes non isolées : contour complet d'une région (plusieurs morceaux) régularisé
      en entier, chaque morceau reprend sa part, jonctions fixes (fondu sur 4 points) ; tolérance plafonnée à 1,5 % du
      périmètre (petites taches) ; pré-test sur le profil radial (sans effet sur le résultat, temps inchangé)
- [x] Alignement : droites et côtés de coins presque horizontaux/verticaux (dérive ≤ 0,3 px) rendus exacts

## Retours de test (après M3)
- [x] Zoom (molette, boutons) annulé aussitôt : corrigé
- [x] Rendu net à tout zoom : le résultat est tracé dans la page (plus d'`<img>` agrandie)
- [x] Liserés entre formes voisines : trait de la couleur de la forme du dessous sous chaque jointure (« Combler les
      jointures », actif par défaut)
- [x] Nettoyage des artefacts de compression (JPEG) : filtre bilatéral en Oklab avant l'analyse des couleurs, curseur
      « Nettoyage » (30 % par défaut : p95 −0,04, −28 % de nœuds ; les images bruitées et JPEG en profitent)
- [ ] Dégradés : zones à variation de couleur régulière fusionnées en une forme à dégradé linéaire ou radial (opacité
      comprise)

## M3bis — Géométrie fiable (détection hybride)
- [x] Carte de netteté des bords (gradient de Scharr en Oklab) : indice 2·G/C par morceau de frontière (~1 vrai bord,
      ~2/B frontière de bandes d'un dégradé) ; sous 0,45 la frontière est molle : tracé libre, aucune géométrie
- [x] Vérification contre les points mesurés : un morceau qui a reçu de la géométrie et s'écarte des points de plus de
      la tolérance + 0,6 px est retracé librement (sans ses guides) ; les pointes voulues vives sont excusées
- [x] Réglage « Échelle » : tolérance + jusqu'à 2 % de la taille (droites, arrondis, arcs, cercles, symétries,
      répétitions, vérification) ; un contour fermé doit faire le tour de sa forme (périmètres à 15 %) ; un arc de forme
      prend le chemin de la longueur du morceau ; la vérification refuse aussi les détours (tracé trop long)
- [x] Repères globaux : segments presque alignés de toute l'image (même interrompus, extrémités à ≤ 0,5 px) placés
      exactement sur une droite commune ; sommet de deux repères = leur intersection ; deux bords côte à côte (trait
      fin) ne sont jamais fusionnés ; guide « Alignement »
- Les deux points restants (cercles communs, parallèles, concentriques ; position de Devernay) sont repris par
  l'algorithme sur mesure ci-dessous (modèle « sur repère », régularisation, bords sous-pixel).

## Algorithme sur mesure (docs/ALGORITHME.md)
Conception validée après quatre tournées de relecture (docs/ALGORITHME.md, synthèses dans docs/recherche/).
Construction en phases, chacune fermée par des tests chiffrés (§16).
- [x] P0 — Boîte à outils : fonctions mathématiques déterministes portées de `libm` (≤ 1 ulp, mêmes bits en natif et
      en WASM), interdiction des fonctions de `std` et des `HashMap` dans le moteur (règles clippy), PCG32, tri total,
      plans d'image, rastériseur de couverture exacte, générateur de scènes à vérité terrain (formes, coins arrondis,
      traits, dégradés, flou, bruit, agrandissement), filtre de débruitage par tables, WASM avec `simd128`
- [ ] P1 — Oklab et modèle de bruit, classification de l'entrée, gaussiennes par bandes, pyramide, chronomètres
- [ ] P2 — Bords sous-pixel, chaînage, analyse par point, traits fins
- [ ] P3 — Bord réel / bord de postérisation dans le pipeline actuel, mesure des coutures
- [ ] P4 — Détecteurs (segments, coins, arrondis, cercles, ellipses, taches), graphe de repères
- [ ] P5a / P5b — Critère unique, segmentation, modèle « sur repère », ajustement conjoint
- [ ] P6a / P6b — Régularisation globale, répétitions ; réparation de topologie
- [ ] P7 — Seuils locaux, réglage Qualité/temps, optimisations
- [ ] P8 — Retrait de l'ancien code, préréglage Photo, comparaison avec vectorizer.ai

## M4 — Édition
- [ ] Palette : opacité, exclure une couleur (transparence), sélection visible dans l'aperçu
- [ ] Outil **Fond** : retrait automatique, bascule par région (masque) ou par couleur
- [ ] Outil **Régions** : clic sur une région → recolorer, fusionner avec le voisin de plus longue frontière,
      répartir entre les voisins (transformée de distance)
- [ ] Brouillon (annuler / appliquer), annuler/rétablir global ; une retouche n'efface que celles des étapes suivantes

## M5 — Exports et prétraitement
- [ ] Préréglages Général / Édition / Découpe ; style (remplissage, contours, arêtes), groupement, taille
- [ ] Tolérance 0,3 / 0,1 / 0,03 / 0,01 px ; formes natives (`<circle>`, `<rect>`) ; gap filler 2 px
- [ ] PDF, EPS, DXF (lignes et arcs), PNG
- [ ] Agrandissement ×2/×4 par IA (Real-ESRGAN via ONNX Runtime) pour les petites images
- [ ] Mode pixel art (Kopf-Lischinski, portage MIT)
