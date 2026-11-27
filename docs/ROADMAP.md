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

## M2 — Bords sous-pixel et coins
- [ ] Bords placés au point de transition à 50 % entre couleurs voisines (anticrénelage)
- [ ] Détection des coins : angle, courbure et comparaison des erreurs d'ajustement

## M3 — Primitives et régularisation
- [ ] Ajustement par segment : droite, puis arc de cercle, puis arc d'ellipse, puis Bézier cubique (kurbo), choix par erreur + complexité
- [ ] Formes entières reconnues (cercle, ellipse, rectangle arrondi)
- [ ] Alignement horizontal/vertical, rayons égaux, symétries

## M4 — Édition
- [ ] Recolorer, fusionner ou supprimer une couleur ; supprimer le fond
- [ ] Cliquer sur une région pour la modifier ; annuler/rétablir

## M5 — Exports et prétraitement
- [ ] PDF, EPS, DXF (lignes et arcs), PNG ; options de groupement ; remplissage des jointures (gap filler)
- [ ] Agrandissement ×2/×4 par IA (Real-ESRGAN via ONNX Runtime) pour les petites images
- [ ] Mode pixel art (Kopf-Lischinski, portage MIT)
