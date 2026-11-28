# Outils de référence : l'éditeur de vectorizer.ai

Relevé du 27/11/2026. Il s'appuie sur la documentation publique de l'API et sur l'observation de l'éditeur avec les
images d'exemple du site (sans compte ni envoi d'image). Il décrit **ce que font les outils**, pas leur présentation :
l'interface du site n'est pas un modèle. On reprend les capacités et la qualité du résultat.
**[OBS]** = observé, **[INF]** = déduction pour notre implémentation.

## Modèle général
- [OBS] Chaque outil travaille en brouillon (Annuler / Appliquer). Appliquer relance le traitement, et un
  annuler/rétablir global revient sur chaque application.
- [OBS] Modifier la palette efface les retouches de régions ; retracer efface les retouches de courbes.
- [INF] Le pipeline est découpé en étapes : palette → étiquettes → graphe de régions → courbes partagées → géométrie
  (formes, symétries, répétitions) → coins. Une modification invalide les retouches **des étapes suivantes**
  seulement. Chez nous, chaque retouche est une donnée d'`Options` (comme `paletteEdit`), rejouable et liée à une
  empreinte de l'étape dont elle dépend.

## 1. Fond (Background)
- [OBS] Le fond est retiré automatiquement à l'ouverture (le blanc extérieur devient transparent).
- [OBS] Mode **Masque** : un clic bascule une seule région connexe fond/gardée. Mode **Couleur** : on coche des
  couleurs, et toutes leurs régions deviennent transparentes. Les voisines gardent leur contour.
- [INF] Un booléen « fond » par région ; valeur initiale = régions de la couleur qui touche le plus la bordure.
  Pas de retraçage : les frontières sont déjà partagées, on omet simplement ces régions.

## 2. Palette (fait, voir `PaletteEditor.svelte`)
- [OBS] L'image est sur-quantifiée en « couleurs de traitement », souvent des quasi-doublons liés à une forme. Cela
  permet de recolorer une forme seule.
- [OBS] Une rangée de préréglages 2..N recalcule avec k couleurs, la valeur recommandée étant présélectionnée. La
  couleur k est celle qui apparaît en passant de k-1 à k.
- [OBS] Fusionner donne **la couleur de la cible**. Séparer recrée la couleur d'origine.
- [OBS] « Inclure dans le résultat » décoché : la couleur devient transparente, ce qui fait un trou dans le vecteur.
  Opacité réglable de 0 à 255.
- Reste à faire chez nous : opacité et exclusion d'une couleur ; aperçu de la sélection dans l'image.

## 3. Régions
- [OBS] Un clic sélectionne une région connexe, avec sa couleur et celles de ses voisines.
  - **Recolorer** avec la couleur d'un voisin.
  - **Fusionner avec un voisin** : celui qui partage la **plus longue frontière**.
  - **Répartir entre les voisins** : la région disparaît, et les voisins y croissent avec de nouvelles frontières
    courbes (retraçage).
- [INF] Graphe d'adjacence `région → {voisin : longueur de frontière}` (nous l'avons presque : les « runs » de
  `planar.rs`). Fusion = réétiquetage. Répartition = chaque pixel de la région prend l'étiquette du voisin le plus
  proche (transformée de distance), puis on retrace la zone.
- [OBS] L'édition des nœuds existe dans leur code mais est désactivée en production.

## 4. Géométrie
- [OBS] **Formes simplifiées** : uniquement pour les formes **isolées et entières**. On choisit entre « tracé » et
  des candidats (cercle, ellipse, rectangle, polygone, étoile 3-6 branches). Le polygone général donne des sommets
  parfaitement nets.
- [OBS] **Symétries** : axes détectés, actifs ou simplement candidats, et un clic les bascule. Exemples : triangle
  avec 1 axe ; hexagone avec 2 axes actifs et 4 candidats ; cercle avec un centre de rotation.
- [OBS] **Formes répétées** : des régions semblables partagent un même contour.
- [INF] Formes :
  - cercle : Kåsa puis Gauss-Newton ;
  - ellipse : Fitzgibbon ;
  - rectangle orienté ;
  - polygone : Douglas-Peucker, puis intersection des droites ajustées ;
  - choix du modèle par erreur RMS au contour, pénalisée par sa complexité.
- [INF] Symétries : tester les axes passant par le centroïde avec la distance de Hausdorff entre le contour et son
  miroir. Une symétrie appliquée moyenne les points du contour avec leur image avant l'ajustement.
- [INF] Répétitions : signature invariante (moments de Hu ou descripteurs de Fourier), puis regroupement.

## 5. Coins (fait, voir `decide_corners` dans `planar.rs` et `corners.ts`)
- [OBS] Le compteur affiche le total, les vifs et les arrondis. Les coins d'une forme géométrique forment un seul
  groupe.
- [OBS] Mode global : automatique, tous vifs ou tous ronds.
- [OBS] Curseur **« nombre de coins vifs, classés par écart »** (0..total).
- [OBS] Rayon relatif de −100 (vif) à +100 (rayon maximal), 0 étant le meilleur ajustement. Au maximum, un hexagone
  devient un cercle.
- [OBS] Par coin : un double-clic bascule vif/rond, et on peut régler un rayon personnalisé borné (ex. 4,88 px).
- [OBS] Sur un dessin net, le rayon « meilleur ajustement » reste sous le pixel : les coins paraissent nets sans
  dents.
- [INF] Chaque jonction entre deux segments ajustés est un `Corner {position, angle, écart, rayon_ajusté,
  rayon_max, groupe, retouche}`.
  - **Écart** = distance entre le sommet théorique (intersection des tangentes) et le contour des pixels. Plus il
    est petit, plus le coin est vraiment vif.
  - Le curseur rend vifs les N coins de plus petit écart.
  - `rayon_max = min(longueurs adjacentes) · tan(θ/2)`.
  - L'arc est tangent aux deux segments et exporté en arc de cercle.
  - En mode empilé, arrondir un coin découvre la région du dessous, sans trou.

## 6. Export
- [OBS] Formats SVG (1.0, 1.1, Tiny 1.2, compatible Adobe), PDF, EPS, DXF et PNG.
- [OBS] Préréglages :
  - **Général** : remplissage, découpé, gap filler ;
  - **Édition** : empilé, groupé par couleur ;
  - **Découpe/gravure** : arêtes en traits, chacune tracée une seule fois.
- [OBS] Options :
  - taille : inchangée, ou échelle / dimensions (unités, ajustement) ;
  - groupement : aucun, couleur, parent ou calque ;
  - formes simples natives (`<circle>`, `<rect>`…) ou aplaties ;
  - types de courbes (droites, quadratiques, cubiques, arcs) ;
  - tolérance : grossière 0,3, **moyenne 0,1 px**, fine 0,03, très fine 0,01 ;
  - **gap filler** de 2 px : un trait de la couleur moyenne sous les frontières, contre les liserés.
- [OBS] Pas de tracé « ligne centrale » : les traits sont toujours des formes pleines.
