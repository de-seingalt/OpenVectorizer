# Feuille de route

Le score de référence est `testdata/baseline.json` (ΔE moyen, p95, nombre de nœuds).
Chaque étape doit l'améliorer, ou au moins ne pas le dégrader.

## M0 — Socle (fait)
- [x] Moteur Rust avec VTracer comme base (`crates/core/src/baseline.rs`)
- [x] Banc d'évaluation (`openvectorizer eval`) + corpus synthétique + cartes d'erreur
- [x] Interface : dépôt/collage/ouverture, comparaison avant/après avec curseur, zoom/déplacement, réglages, export SVG
- [x] Tauri 2, CI, publication web et binaires
- [ ] Corpus d'images réelles libres de droits (logos JPEG compressés, icônes, captures, pixel art)
- [ ] PWA (service worker) pour un usage web hors ligne

## M1 — Palette et carte planaire (frontières partagées)
- [ ] Quantification k-means en Oklab, nombre de couleurs automatique (fusion selon ΔE et surface)
- [ ] Pixels d'anticrénelage rattachés à la région voisine, et non transformés en nouvelles couleurs
- [ ] Nettoyage des étiquettes (taches, îlots)
- [ ] Graphe planaire : chaque frontière entre deux régions est stockée une seule fois, avec ses jonctions
- [ ] Écriture SVG à partir du graphe : modes découpé (cutout) et empilé, sans jour entre couleurs
- [ ] Interface : palette affichée et nombre de couleurs réglable

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
