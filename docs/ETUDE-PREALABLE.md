# Étude préalable — Vectoriseur hors ligne (2026-11-26)

## 1. Analyse de vectorizer.ai

### Parcours utilisateur
1. **Déposer** : glisser-déposer, Ctrl+V ou parcourir. La vectorisation démarre automatiquement.
2. **Prévisualiser** : visionneuse avec zoom et déplacement, et comparaison avec l'original.
3. **Télécharger** : préréglages d'export (édition, découpe, gravure), formats SVG, PDF, EPS, DXF et PNG.

L'éditeur permet de :
- éditer la palette (nombre de couleurs automatique ou forcé, remplacement, suppression) ;
- recolorer, fusionner, scinder ou supprimer des régions ;
- supprimer le fond ;
- régler les coins (vifs ou arrondis, rayon) ;
- activer ou désactiver les symétries, formes simplifiées et formes répétées ;
- éditer des nœuds, recadrer avant traitement, annuler et refaire.

### Ce qui fait la qualité (affirmations du fournisseur)
- **« Vector Graph »** : un graphe planaire dans lequel les régions voisines **partagent exactement leurs frontières**. Il n'y a donc ni jour ni chevauchement entre les couleurs.
- **Précision sous-pixel** : les frontières sont placées d'après les valeurs d'anticrénelage.
- **Ajustement de primitives** : lignes, arcs circulaires et elliptiques, Bézier quadratiques et cubiques.
- **Formes entières** reconnues : cercle, ellipse, rectangle arrondi, triangle isocèle, étoile.
- **Coins** : chaque coin est modélisé et chaque courbe est lissée.
- **Symétries** : symétries miroir et rotationnelles.
- **Un seul moteur couleur**, et non un traceur noir et blanc lancé couleur par couleur. C'est la source des halos chez Potrace et Inkscape.
- **Réseaux de neurones propriétaires** pour interpréter les pixels bruités. Le pipeline exact n'est pas publié.

### Options de l'API, à reprendre comme modèle de paramètres
| Option | Valeurs |
|---|---|
| `max_colors` | 0 à 256 (0 = automatique) |
| `palette` | `couleur[-> remappage][~tolérance]` |
| `min_area_px` | 0,125 par défaut |
| `draw_style` | `fill_shapes`, `stroke_shapes`, `stroke_edges` |
| `shape_stacking` | `cutouts` (découpe) ou `stacked` (empilé) |
| `group_by` | `none`, `color`, `parent`, `layer` |
| Types de courbes | chacun activable, avec une chaîne de repli s'il est désactivé |
| `line_fit_tolerance` | 0,01 à 0,3 px |
| `gap_filler` | traits de la couleur moyenne derrière les jonctions |
| Export SVG | version 1.0, 1.1 ou Tiny, mode compatible Adobe |
| Export DXF | lignes seules, lignes et arcs, ou lignes, arcs et splines |
| Taille, unités, DPI | — |

Limites du service :
- pas de vectorisation en ligne centrale (centerline) ;
- 3 Mpx maximum ;
- formats d'entrée : PNG, JPG, GIF, BMP et WebP (TIFF en plus via l'API) ;
- service uniquement dans le cloud : il faut envoyer ses images.

Tarif : 8,99 CHF par mois. La prévisualisation est gratuite, le téléchargement payant.

## 2. Algorithmes open source

### Traceurs existants
| Projet | Licence | Intérêt |
|---|---|---|
| **VTracer / visioncortex** (Rust) | MIT / Apache | **Meilleure base permissive.** Couleur, WASM, très actif (nov. 2026). Nouveau mode « hierarchical cutout » à frontières partagées. Manques : coins, primitives. |
| Potrace (C) | GPL-2 | Excellent en noir et blanc (coins, Bézier). La couleur passe par empilement, avec halos. La GPL contamine l'ensemble. |
| AutoTrace (C) | GPL-2 | Actif, fait aussi la ligne centrale, mais sortie bruitée. |
| ImageTracer.js | Domaine public | Simple, sortie « molle », abandonné. |
| SVGcode (PWA de Google) | GPL-2 | Référence pour une PWA hors ligne. Qualité faible : Potrace couche par couche. |
| Kopf-Lischinski (pixel art) | portages MIT (JS, Python) | Mode « pixel art » optionnel. |

### Apprentissage automatique
- DiffVG, LIVE, StarVector, OmniSVG, LayerTracer, SuperSVG : soit trop lents (optimisation de plusieurs minutes sur GPU), soit génératifs et imprécis, ce qui donne des hallucinations. Aucun n'est utilisable en temps réel sur CPU ou en WASM.
- **Usage réaliste** : du prétraitement via ONNX Runtime (WebGPU ou WASM sur le web, natif sur ordinateur), par exemple un agrandissement ×2 à ×4 avec Real-ESRGAN (BSD-3) pour les petits logos et les JPEG dégradés.

### Briques de base (licences permissives)
- **Quantification des couleurs** : k-means en Oklab (`kmeans-colors`, `quantette`, `exoquant`). Éviter libimagequant, qui est sous GPL ou licence commerciale.
- **Lissage et segmentation** : filtre bilatéral, mean-shift, watershed, à coder en Rust (la version WASM d'OpenCV est lourde).
- **Ajustement de courbes** : **kurbo** (Rust, Apache/MIT) et l'algorithme de Schneider.
- **Primitives** :
  - droites par moindres carrés ;
  - cercles (méthodes de Kåsa, Pratt ou Taubin) ;
  - ellipses (méthode de Fitzgibbon) ;
  - choix entre candidats par coût « erreur + complexité » (principe MDL).

### Où se situe l'écart de qualité (par ordre d'importance)
1. Topologie à frontières partagées (carte planaire).
2. Placement sous-pixel des bords à partir de l'anticrénelage.
3. Détection robuste des coins.
4. Ajustement de primitives et régularisation : alignement horizontal et vertical, rayons égaux.
5. Palette intelligente : absorber les pixels d'anticrénelage au lieu de créer des couleurs.

## 3. Pipeline cible (cœur Rust, natif et WASM)

1. Décodage (crate `image`), avec agrandissement ONNX en option et lissage préservant les bords.
2. Palette : k-means en Oklab, K automatique (fusion selon ΔE et surface), pixels d'anticrénelage rattachés à la région la plus proche.
3. Nettoyage des étiquettes : suppression des taches, îlots, filtre de mode.
4. **Carte planaire** : frontières entre régions, et jonctions là où se rencontrent au moins 3 régions (ou 2 régions et le bord de l'image).
5. **Bords sous-pixel** : passage à 50 % entre les deux couleurs voisines, ou maximum du gradient.
6. Coins : angle et courbure, plus comparaison des erreurs d'ajustement.
7. Primitives par segment : droite, puis arc, puis ellipse, puis cubique. Régularisation globale ensuite.
8. Export SVG : chaque frontière est ajustée une seule fois et réutilisée par les deux régions voisines. Modes cutout et stacked, gap filler, export DXF et PDF plus tard.

## 4. Plateforme : Godot ou autre ?

**Godot 4.7 est déconseillé** pour cet outil :
- pas de rendu SVG vectoriel conservé : ThorVG ne fait que rastériser, il faut donc re-rastériser à chaque zoom ;
- Rust sur le web via gdext encore expérimental (Rust nightly et emscripten épinglé) ;
- export web de 20 à 40 Mo, et en-têtes COOP/COEP probablement requis avec GDExtension ;
- pas de boîte de dialogue de fichiers native sur le web, presse-papiers peu fiable ;
- fichiers `.tscn` fragiles à éditer pour un agent, et une API moins connue des modèles de langage.

Godot reste excellent pour un jeu, mais pas pour un outil dont la sortie est nativement un format web.

**Recommandation :**
- **Cœur** : une crate Rust sans interface (`vectorizer-core`), exposée de trois façons :
  - une **CLI**, pour les tests de non-régression utilisables par les agents ;
  - **wasm-bindgen**, pour le web (exécuté dans un Web Worker) ;
  - un lien direct dans **Tauri 2**.
- **Interface** : TypeScript avec Svelte (ou React). L'aperçu SVG est rendu par le navigateur :
  - zoom net à l'infini ;
  - superposition avant/après en CSS ;
  - édition de la palette par modification directe des `fill` dans le DOM.
- **Distribution** :
  - une webapp statique (PWA hors ligne) ;
  - une application de bureau Tauri, de 3 à 15 Mo (portable sous Windows, AppImage sous Linux) ;
  - la CLI.
- **Plan B** : egui/eframe, 100 % Rust, mais aperçu rastérisé par resvg et interface moins soignée.

## 5. Pistes pour le développement agentique
- **Monorepo** :
  - `crates/core` : le moteur ;
  - `crates/cli` ;
  - `crates/wasm` ;
  - `app/` : l'interface Svelte ;
  - `src-tauri/` : l'enveloppe de bureau ;
  - `testdata/` : le corpus d'images.
- **Boucle de mesure objective** :
  - rastériser le SVG produit avec resvg et le comparer à l'image source (SSIM, ΔE, IoU par région) ;
  - mesurer aussi le nombre de nœuds et la taille du fichier ;
  - suivre l'évolution sur un corpus figé (logos, icônes, JPEG dégradés, pixel art, captures d'écran).

  Les agents itèrent ainsi sur l'algorithme avec un score, et non « à l'œil ».
- **Instantanés de référence** (golden snapshots) des SVG avec `insta`. Côté interface : Vitest et Playwright en mode headless, avec captures d'écran.
- **Un module par étape du pipeline**, chacun avec une interface claire et des tests unitaires. Cela permet de faire travailler des agents en parallèle, via git worktrees.
- **CLAUDE.md**, intégration continue GitHub Actions (cargo test, clippy, wasm-pack, build Tauri multiplateforme), et publication web sur GitHub Pages (avec `coi-serviceworker` si les threads sont nécessaires).
- **Étapes jalonnées** :
  - **M0** : démo VTracer en WASM, plus l'interface de comparaison ;
  - **M1** : carte planaire et frontières partagées ;
  - **M2** : bords sous-pixel et coins ;
  - **M3** : primitives et régularisation ;
  - **M4** : édition de la palette et des régions ;
  - **M5** : exports DXF et PDF, agrandissement ONNX.
