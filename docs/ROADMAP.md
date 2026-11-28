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

## M3 — Géométrie
- [x] Droites : parties droites détectées (écart ≤ 0,45 px, pas de courbure systématique, ≥ 6 px, pas un morceau
      d'arc), tracées en segments, les courbes voisines leur sont tangentes ; coins du cadre toujours vifs
- [x] Raccords : entre deux droites, arc de cercle tangent aux deux (rayon ajusté aux points, écart ≤ 0,5 px),
      les droites s'arrêtent aux points de contact
- [x] Rayons égaux : rayons de raccord proches (4 % ou 0,3 px) égalisés sur toute l'image (deuxième passe)
- [x] Curseur **Géométrie** (0 = tracé brut … 100 %) : dose raccords, cercles, ellipses et symétries
- [ ] Arcs de cercle et d'ellipse libres (hors raccords), choix par erreur + complexité
- [x] Formes entières (contours isolés) : cercle (Kåsa) et ellipse (Fitzgibbon / Halíř-Flusser) exacts si l'écart
      reste ≤ 0,6 px (RMS ≤ 0,22) ; le cercle l'emporte à qualité égale. Réglage « Cercles et ellipses exacts ».
      Rectangles, polygones et étoiles : coins vifs + droites
- [ ] Choix « tracé ou forme » forme par forme (clic dans l'aperçu)
- [x] Symétries des formes isolées : axe miroir par le centroïde et rotation d'ordre 2..12, contour remplacé par la
      moyenne de ses images (écart ≤ 0,7 px) ; coins candidats reportés sur leurs images, décision « auto » commune
      à un groupe de coins (majorité) ; pointes aiguës vives si leur rayon ajusté ≤ 0,5 px
- [ ] Formes répétées partageant un contour ; symétries des formes non isolées
- [x] Alignement : droites et côtés de coins presque horizontaux/verticaux (dérive ≤ 0,3 px) rendus exacts

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
