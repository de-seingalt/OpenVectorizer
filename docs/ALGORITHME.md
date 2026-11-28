# Algorithme de vectorisation sur mesure — document de conception

**Version 4.1** : intègre quatre tournées de relecture indépendante (rigueur mathématique, faisabilité Rust/WASM, couverture des objectifs et robustesse), puis les pistes de la relecture finale (§18). Les synthèses de recherche qui alimentent ce document sont dans [`docs/recherche/`](recherche/) (en anglais) ; un renvoi `[R1 §2.3]` désigne la synthèse 01, section 2.3. Un glossaire (annexe A) et un exemple complet (annexe B) aident à la lecture. Les pistes d'amélioration proposées après la dernière relecture (mathématiques et optimisation Rust) sont détaillées dans [`docs/recherche/06-pistes-amelioration.md`](recherche/06-pistes-amelioration.md), notées M1–M11 et R1–R11 ; le §18 dit lesquelles sont adoptées et lesquelles restent à trancher par la mesure.

---

## 0. Objet et principes

### 0.1 Le constat

Le moteur actuel trace les contours d'une image **postérisée** (k-means dans Oklab) puis y ajoute des couches de correction : coins par RDP, cercles et ellipses sur les contours, symétries, alignements, vérification. Rien n'est mesuré sur l'image raster elle-même, sauf une netteté de bord (Scharr) et une position sous-pixel par couverture. Conséquences observées lors des tests :

- des géométries « hallucinées » sur les dégradés (les frontières entre bandes de couleur sont des artefacts de la postérisation, pas des bords réels) ;
- de petites géométries trouvées, mais pas les grandes lignes ni les grands cercles ;
- des lignes colinéaires interrompues par d'autres formes, non unifiées ;
- des repères d'ellipse trouvés mais non suivis ;
- des artefacts JPEG qui trompent la palette ;
- des coutures claires ou sombres entre formes voisines ;
- une cinquantaine de constantes réglées à la main, sans lien avec le bruit réel de l'image.

### 0.2 L'idée directrice

On sépare deux questions que le moteur actuel confond :

1. **Topologie** : quelles zones de couleur existent, lesquelles se touchent, où sont les jonctions. La postérisation y répond bien ; on la garde, et on la **répare** là où le raster la contredit (traits fins, petits points, §8.2).
2. **Géométrie** : où passe exactement chaque frontière et quelle forme simple elle suit (droite, arc, ellipse, coin vif ou arrondi). On y répond **sur l'image raster**, à plusieurs échelles, avec des détecteurs dont les seuils découlent d'un modèle de bruit mesuré.

Les détections raster deviennent des **repères** (« guides ») : segments, coins, jonctions, arcs, cercles, ellipses, taches, traits fins, chacun avec une position, une incertitude, une échelle et un score de signification. Le tracé de chaque frontière de zone est ensuite une courbe lisse (Bézier) qui, à l'approche d'un repère, **devient** la primitive du repère, y compris quand ce repère traverse plusieurs frontières. Le tracé final est une suite de droites, d'arcs de cercle, d'arcs d'ellipse et de Béziers, tangents entre eux sauf aux coins.

### 0.3 Les règles de conception

1. **Un seul critère de décision** (§3). « Est-ce une droite ? », « ce coin est-il arrondi ? », « ces deux traits sont-ils alignés ? » se tranchent tous de la même manière : résidus pondérés par l'incertitude réelle + coût de description des paramètres (longueur de description minimale, MDL). La signification statistique d'un repère raster (NFA, approche a contrario) sert à **détecter**, pas à décider du tracé.
2. **Les seuils viennent de l'image.** Le bruit est estimé sur l'image ; les seuils de détection en découlent pour une probabilité de fausse alarme fixée. Ils varient localement.
3. **La simplicité est une préférence explicite et dosable.** A priori par type de primitive et échelle de perception réglable : un détail plus petit que cette échelle ne casse ni une symétrie, ni une droite, ni une ellipse (§3.5).
4. **Tout est recodé en Rust, sans bibliothèque de traitement d'image ni de calcul.** On s'inspire de code permissif (MIT, BSD, Apache) en le citant ; pour le code GPL/AGPL ou sans licence, on implémente uniquement depuis l'article (§15).
5. **Déterministe, borné, mesurable.** Même entrée, mêmes réglages → même SVG au bit près, en natif comme en WASM (§14.4). Chaque étape a une borne de coût explicite (§14.3) et une vérité terrain synthétique avec un critère chiffré (§13).

### 0.4 Ce qui change et ce qui reste

**Reste** : les options et la sortie (`Output { svg, stats, palette, corners, guides }`), la palette k-means Oklab éditable, les composantes connexes, les cycles en arêtes inter-pixels (crack edges), les frontières partagées découpées aux jonctions, la stratégie anti-couture (§8.3), le benchmark de 515 cas.

**Change** :
- l'intérieur de `fit.rs`, `shape.rs`, `align.rs`, `edges.rs` et une partie de `planar.rs` ;
- un étage de détection raster est ajouté ;
- l'interface gagne des types de repères (Coin, Jonction, Trait, Tache), un réglage Qualité/temps, et une table de correspondance des anciens réglages (§12.4) ;
- les éditions enregistrées (coins, repères forcés ou désactivés) sont conservées par une règle de migration testée (§7.3).

**Périmètre** : le nouveau pipeline vise d'abord les préréglages Logo et Illustration. Le préréglage Photo garde son chemin actuel (VTracer) jusqu'à ce que le nouveau pipeline le batte sur le corpus photo (phase P8).

---

## 1. Vue d'ensemble

```
 image RGBA8 ──► classification d'entrée (pixel art ? petite image ? JPEG ? transparence ?)
   │
   ├─(A) Oklab, modèle de bruit (fonction de niveau, propagée par pixel), détection JPEG
   │
   ├─(B) Plans à σ₀ par bandes + pyramide (budget qualité/temps)
   │       │
   │       ├─(C) Bords sous-pixel couleur (Di Zenzo généralisé + Canny/Devernay),
   │       │     chaînes orientées ; par point : flou, position finale, variance ;
   │       │     traits fins (Steger)
   │       │
   │       └─(D) Primitives sur les chaînes : segments, coins & jonctions, arrondis,
   │             arcs / cercles / ellipses, taches ──► (E) GRAPHE DE REPÈRES
   │
   ├─(F) Postérisation (existante) → zones, frontières partagées, jonctions
   │       └─(F′) Réparation de topologie guidée par les repères (traits, points, fusions)
   │
   ├─(F″) Association frontière ↔ bords : points mesurés, statut « bord réel » ou
   │      « bord de postérisation » par sous-morceau
   │
   │   ─ ─ ─ ─ ─ ─ (tout ce qui précède est mis en cache, §14.6) ─ ─ ─ ─ ─ ─
   │
   ├─(G) Segmentation optimale de chaque frontière (programmation dynamique, §3),
   │     modèle « sur repère » partagé entre frontières
   ├─(H) Ajustement conjoint, contraintes G1 aux jonctions lisses
   ├─(I) Régularisation globale : colinéarités, parallèles, rayons, symétries, répétitions
   └─(J) Vérification, émission SVG (L, A, C ; anti-couture ; <use> pour les répétitions)
```

Déplacer un curseur de Géométrie, Échelle ou Lissage, éditer un coin ou un repère ne relance que G–J.

---

## 2. Conventions

- **Coordonnées.** Le pixel (i, j) couvre le carré [i, i+1] × [j, j+1] de l'espace SVG ; son centre est en (i + ½, j + ½). Toute la géométrie est exprimée dans cet espace. Les plans d'image sont indexés par pixel ; la conversion indice → SVG est une seule fonction, testée.
- **Pyramide.** Le niveau o + 1 s'obtient par un flou gaussien (σ = 1 px du niveau o) puis un moyennage de blocs 2×2, en une seule passe (§18.1, R4). Le centre du pixel i du niveau o est en x_SVG = 2^o (i + ½). Le noyau combiné ajoute une variance de σ² + ¼ = 1,25 px² du niveau o (le moyennage discret de deux pixels vaut ¼), soit 1,25 · 4^o px² de l'image d'origine ; la pyramide l'enregistre pour la sélection d'échelle. Une erreur de convention décalerait tous les bords grossiers de 0,25 à 0,5 px [R1 §4.4].
- **Couleur.** Détection dans **Oklab** (L ∈ [0, 1] ; ΔE ≈ distance euclidienne ; seuil de visibilité ≈ 0,02). Matrices dans [R1 §1.3]. Pour la palette, la couleur est **dé-prémultipliée** ; l'alpha est traité à part (§4.4).
- **Échelles.** σ en pixels de l'image d'origine ; t = σ² ; g_σ gaussienne normalisée.
- **Précision.** f32 pour les plans ; f64 pour les ajustements géométriques et toutes les accumulations [R5 §3.4].
- **Notation.** B(n, k, p) = Σ_{j≥k} C(n, j) p^j (1−p)^{n−j} (queue binomiale) ; NFA = nombre de tests × probabilité sous le modèle de fond ; z_q = quantile q de la loi normale.

---

## 3. Le critère de décision unique

Toutes les décisions des étapes D (choix vif/arrondi, cercle/ellipse), G (segmentation), H et I (relations) utilisent ce critère. Il est exprimé en unités de χ², c'est-à-dire −2 × log-vraisemblance.

### 3.1 Variance d'un point

Chaque point de frontière p_i porte une variance normale :

  v_i = var_n,i + σ_sys²

- **var_n,i** : variance due au bruit de l'image, calculée par point (§5.6).
- **σ_sys** : erreur systématique résiduelle du localisateur, **mesurée** sur la vérité terrain synthétique (§13.1) et tabulée par méthode de localisation et par flou σ_b. Ordre attendu : 0,02 à 0,05 px. Sans ce plancher, une image nette donnerait var_n ≈ (0,003 px)², et le moindre biais de quelques centièmes de pixel compterait pour des centaines d'unités de χ² : toutes les droites et tous les arcs seraient rejetés au profit de Béziers. Les biais **connus** (table de Devernay, biais de courbure κs²/2) sont soustraits de la position et ne sont pas ajoutés à la variance.

### 3.2 Corrélation des points

Deux points voisins d'une chaîne partagent le bruit à travers le noyau de dérivation. Avec l'analyse par profil normal (§5.1), le lissage **le long** du bord est σ₀ ; l'autocorrélation le long du bord est ρ(Δ) = exp(−Δ²/(4σ₀²)) et le facteur d'inflation est

  κ_c = Σ_k ρ(k·Δs) ≈ 2√π · σ₀ / Δs  (≈ 3,5 à σ₀ = 1, Δs = 1 px)

où Δs est l'espacement moyen des points, constant par frontière. Partout :

- résidu effectif **J_eff = (1/κ_c) Σ_i ρ_i(d_i)**, où ρ_i est la perte du §3.5 (ρ_i = d_i²/v_i quand l'échelle de perception est nulle) ;
- covariances des paramètres ajustés multipliées par κ_c ;
- nombre d'échantillons indépendants n_eff = n / κ_c.

Pour les localisateurs par fenêtre (surface partielle, erf, §5.5), κ_c est tabulé par simulation. Pour les points de repli issus de la couverture et lissés (§9.1), un filtre linéaire de réponse h corrèle les voisins : κ_c = (Σh)²/Σh², calculé exactement. Il est tabulé avec la variance de couverture, dont les erreurs de quantification sont elles aussi corrélées (marches d'escalier périodiques).

### 3.3 Coût total

Pour une frontière découpée en morceaux k de modèles m_k, aux ruptures b :

  E = Σ_k [ J_eff,k + λ · c(m_k) + λ · π(m_k) ] + Σ_b [ λ_b + λ · δ_coin(b) ]

- **c(m)** : nombre de degrés de liberté de forme d'un morceau entre deux ruptures, déduction faite du glissement tangentiel des extrémités. Droite 2, arc de cercle 3, arc d'ellipse 5, Bézier libre 6, Bézier aux deux tangentes imposées 2, morceau « sur repère » 0 (§10.2).
- **π(m)** : a priori de simplicité, en unités de λ. Droite 0, arc 0,5, ellipse 1, Bézier 1,5 par défaut, réglé par Géométrie (§12.4) et calibré sur le corpus.
- **λ_b = 2 ln N_c** : coût de description de la position d'une rupture choisie parmi N_c candidates. Sans ce coût, les critères de type AIC sur-segmentent.
- **δ_coin(b)** = 1 si la rupture est un coin (tangentes indépendantes), 0 si la jonction est lisse (G1, une tangente partagée).
- **λ** : pénalité par paramètre, entre l'AIC géométrique de Kanatani (λ = 2) et le MDL géométrique (λ = 2 ln(L/σ̄), plus conservateur, qui préfère le modèle dégénéré) [R3 §3, R4 §1.3]. Ici L est la diagonale de la boîte englobante de la frontière et σ̄ = √médiane(v_i), **sans** l'échelle de perception : Géométrie et Échelle restent indépendants. Exemple : L = 512 px, σ̄ = 0,03 px donne λ ≈ 19,5. Le réglage Géométrie interpole de 2 à λ_MDL.

La famille est hybride : λ va de l'AIC au MDL, tandis que λ_b et le coût de choix des relations (§11.2) sont des longueurs de code MDL pures. Conséquence assumée : avec Géométrie = 0 (λ = 2), aucune relation à q = 1 ne peut être acceptée dès que n_type ≥ 2 (2 ln 3 > 2), et les ruptures coûtent plus que les paramètres. « Géométrie = 0 » signifie donc « fidélité, sans régularisation ». Les deux extrémités du curseur sont testées.

### 3.4 Tests entre modèles emboîtés

Toute comparaison d'un modèle simple (k paramètres) à un modèle plus riche (k + Δk) se fait par

  ΔJ_eff = J_eff,simple − J_eff,riche  comparé à  λ · Δk (+ coûts de rupture et de choix)

Les deux modèles sont évalués sur les **mêmes points**, avec la même perte et les mêmes zones exclues.

- droite contre arc : Δk = 1 ;
- cercle contre ellipse : Δk = 2 ;
- coin vif contre arrondi : Δk = 1 ;
- jonction lisse contre coin : Δk = 1 ;
- relation entre primitives (§11) : Δk = q, degrés de liberté retirés, plus le coût de choix 2 ln(n_type + 1).

Il n'existe pas d'autre règle (pas de « (A − B)/σ < 2 », pas de χ² à 99 % ici et à 95 % ailleurs). Les fenêtres de regroupement (2–4°, 3–5 %) servent seulement à générer des candidats.

**Écart maximal.** On note z_S = z_{1−α/(2 n_eff)} (correction de Šidák, α = 10⁻²) ; la même valeur sert au pontage (§3.5). Quand une portée est rejetée parce qu'un point s'écarte trop, le seuil tient compte du nombre de points : τ_max,i = τ_perc + z_S · √v_i. Cela donne z ≈ 3,9 pour n_eff = 100 et 4,4 pour n_eff = 1000. Un seuil fixe de 3σ rejetterait à tort 14 % des droites justes de 200 points (n_eff ≈ 57).

### 3.5 Préférer la forme simple : échelle de perception et pontage

C'est la règle « tricher et préférer la forme la plus simple en laissant de côté les détails trop petits ». Elle agit par trois mécanismes.

1. **Zone morte sous la perception.** Chaque **forme** (zone de couleur, ou repère partagé entre plusieurs zones) reçoit une échelle de perception τ_perc = min(ρ_éch · D, τ_abs). Ici D = min(diagonale de la boîte englobante, 20 × épaisseur), où l'épaisseur vaut 2 × aire / périmètre (le rayon pour un disque, la largeur pour un trait). Ainsi un trait long et fin ne reçoit pas une tolérance de l'ordre de sa largeur. ρ_éch ∈ [0 ; 0,01] vient du réglage Échelle et τ_abs = 4 px est un plafond absolu. Tous les morceaux d'une même forme partagent la même valeur ; une frontière partagée par deux formes prend la plus petite des deux. Avec Échelle = 0,3 par défaut, une forme de 300 px reçoit τ_perc ≈ 1,3 px. La perte par point est une **zone morte** :

   ρ_i(d) = max(0, |d| − τ_perc)² / v_i

   Un écart inférieur à τ_perc coûte exactement 0, quel que soit le nombre de points concernés : seule la pénalité λ·Δk départage alors les modèles, et le plus simple gagne. Les points dans la zone morte sont marqués **tolérés**.

   La zone morte sert à **évaluer** un modèle, pas à en fixer les paramètres : à l'intérieur de la bande, la perte est plate et le minimum n'est pas unique, si bien qu'une primitive pourrait dériver jusqu'à τ_perc des données. Les paramètres d'un modèle sont donc toujours ceux des moindres carrés ordinaires pondérés ; la zone morte calcule ensuite le coût à ces paramètres. Tout réajustement robuste (pontage) ajoute un petit terme η·d²/v_i (η = 10⁻³) qui rend la solution unique.
2. **Pontage des petits détails.** Un détail plus marqué mais petit (encoche, bosse, excroissance) peut être enjambé. La perte devient tronquée : ρ_i = min(max(0, |d| − τ_perc)²/v_i, z_S²). Une série contiguë de points pontés est admise si trois conditions sont réunies :
   - l'aire entre la primitive et les points est ≤ (2 τ_perc)² ;
   - la longueur pontée totale est ≤ β · longueur de la portée (β = 10 % × Échelle) ;
   - le détail **ne décrit pas un objet distinct**. Sont protégés : une tache, un trait fin, une zone fermée, ou tout repère dont le support se trouve surtout hors de la série pontée. C'est ce qui protège le point d'un « i » posé près d'une grande forme. En revanche, les coins, segments et arcs qui ne décrivent que le détail lui-même (les flancs d'une encoche) disparaissent avec lui : sinon, sur une image nette, aucune encoche ne serait jamais pontée.

   Chaque série pontée coûte 2 λ_b (ses deux extrémités). Le calcul suit le §10.1 : coût ordinaire en O(1) d'abord, puis réajustement tronqué borné, uniquement là où il sert.
3. **Petites zones.** Une zone entièrement contenue dans la bande de tolérance d'une primitive (largeur τ_perc) est fusionnée avec sa voisine, sauf si elle décrit un objet distinct au sens du point 2. Ces fusions dépendent du réglage Échelle : elles se font dans G–J, sur une copie de la topologie, et non dans l'étape mise en cache (§14.6).

**Budget de couleur.** Les points tolérés et pontés sont exemptés de la vérification point par point (§12.3), mais pas sans limite. Par forme, l'aire qui change de zone à cause de ces écarts doit rester ≤ 2 % × Échelle de l'aire de la forme. Au-delà, la forme est retracée une seule fois avec τ_perc divisé par deux ; il n'y a pas de boucle.

Avec Échelle = 0 : τ_perc = 0, β = 0, fidélité pure. Un test vérifie que le nombre de nœuds décroît quand Échelle va de 0 à 1 (tendance : corrélation de rang ≤ −0,9, sans exiger une décroissance stricte à chaque pas).

### 3.6 Rôle des NFA

Le nombre de fausses alarmes (approche a contrario de Desolneux, Moisan et Morel) répond à la question « cette structure existe-t-elle vraiment dans l'image, ou est-elle due au hasard ? ». Il sert à **retenir** un repère raster (NFA ≤ ε = 1). Il **n'entre pas** dans E :

- un NFA est une borne de probabilité, non bornée, pas une vraisemblance ;
- les points de bord qui ont produit le repère sont déjà dans J, donc les compter deux fois serait une erreur.

Un repère agit sur le tracé par ce qu'il **offre** à la segmentation : des ruptures candidates, un modèle candidat sur sa portée (« sur repère », §10.2), et un modèle imposé quand l'utilisateur le force.

---

## 4. Étape A — Préparation et modèle de bruit

### 4.1 Classification de l'entrée

Avant tout calcul :

- **JPEG** : rapport entre la marche moyenne de luminance au travers des frontières de blocs 8×8 et la marche moyenne ailleurs, en ne comptant que les marches situées dans des régions qui varient (la marche et ses deux voisines le long de la ligne sont faibles, et les voisines ne sont pas nulles). Le décodage JPEG laisse des discontinuités aux frontières de blocs partout où l'image varie (dégradés, oscillations autour des bords), alors qu'un bord du dessin tombé sur la grille sépare deux aplats. Seuil 1,10, mesuré sur le benchmark (P1) : 60 % des JPEG détectés, 2 % de fausses alarmes. Deux méthodes essayées ont été écartées : la marche aux frontières sans condition (89 % détectés, mais 10 % de fausses alarmes sur les icônes et drapeaux calés sur une grille de 8 px), et le regroupement des coefficients DCT sur les multiples d'un pas de quantification (inexploitable sur les logos aplatis sur blanc, dont l'écrêtage à 255 déplace les coefficients). Le drapeau ne pilote que le plancher de bruit et la localisation sur L seul ; la calibration des seuils sur l'image (§18.2, M2) le rend moins critique.
- **Pixel art** : grille native d'au moins 2 px, la plus grande période c telle qu'au moins 98 % des changements de couleur, dans les deux directions, tombent sur une même phase modulo c ; peu de couleurs. L'exigence d'une grille d'au moins 2 px évite de prendre pour du pixel art un logo simplement crénelé. Mesuré en P1 : les trois images de pixel art du corpus sont reconnues (grille de 20 px), plus le drapeau suisse, dont tous les bords tombent sur une grille de 9 px : sans dommage, ce sont des rectangles exacts. Première version : avertissement dans l'interface et traitement à la résolution native, sans flou, en polygones exacts sur les arêtes inter-pixels. Un vrai mode pixel art (à la Kopf-Lischinski) est une étape ultérieure, après vérification du brevet signalé [R5 §2].
- **Petite image** (côté < 128 px) : σ₀ = 0,7 avec le noyau gaussien discret de Lindeberg (le noyau échantillonné est faux sous 0,8), estimation de bruit globale. Taille minimale supportée : 16 px.
- **Transparence** : présence d'un alpha non constant (§4.4).
- **Grande image** : au-delà du plafond du mode (§12.5), la classification et l'estimation du bruit se font sur l'image d'origine, avant réduction.

### 4.2 Conversion

sRGB 8 bits → linéaire → LMS → racine cubique → Oklab, en f32, un plan par canal (L, a, b). Les plans sont stockés séparément (structure de tableaux) pour les filtres séparables et le SIMD [R5 §3.4].

### 4.3 Modèle de bruit

Le bruit de l'image n'est ni uniforme ni indépendant d'un canal à l'autre dans Oklab. Les trois canaux mélangent les mêmes valeurs LMS, et les zones sombres sont environ deux fois plus bruitées après la racine cubique. On l'estime donc là où il est le plus simple, sur les valeurs sRGB codées, puis on le propage [revue math. B1].

1. **Fonction de niveau de bruit** par canal, mesurée sur les valeurs codées (codes / 255), là où vivent la quantification, le JPEG et le bruit des scanners. On calcule les réponses d'Immerkær r = I ∗ N, avec N = [[1, −2, 1], [−2, 4, −2], [1, −2, 1]] et Σ N² = 36, qui annule les structures lisses (y compris un dégradé sRGB linéaire). On ne garde que les pixels dont le gradient est sous sa médiane, pour exclure les bords (ce masquage ne biaise pas l'estimation), et pas ceux dont le voisinage 3×3 touche 0 ou 255, où le bruit est écrêté (un fond blanc n'a pas de bruit mesurable, quel que soit le bruit de l'image). On les regroupe par valeur locale (16 classes). Par classe, l'écart-type robuste vaut MAD/(0,6745 · 6), avec une **médiane de données groupées** : les réponses d'une image 8 bits sont des nombres entiers de codes, et leur médiane brute avance par crans de 30 % pour un bruit d'un code. Les classes sont ensuite lissées (σ² pondéré par les effectifs sur trois classes voisines) et interpolées linéairement entre leurs valeurs moyennes. Il n'y a **pas de modèle paramétrique** : la loi α·I + β, valable en lumière linéaire pour un capteur, ne décrit ni le bruit codé (JPEG, scanner) ni l'effet d'une courbe de ton, et elle sous-estimait d'un tiers le bruit de la variante bruitée du benchmark (P1). Le bruit en lumière linéaire s'en déduit par la pente de la courbe de décodage, exacte au premier ordre. La corrélation entre canaux due au dématriçage des photos est négligée (covariance diagonale) ; c'est une approximation assumée, mesurable par les produits croisés des réponses d'Immerkær si elle s'avère gênante. Mesures P1 : bruit gaussien de loi α·I + β en lumière linéaire retrouvé à moins de 10 % ; bruit uniforme ±16 codes lu 8 % trop haut (le facteur 0,6745 vaut pour une loi gaussienne) ; images propres au plancher.
2. **Planchers** : 0,5/255 en sRGB codé (2/255 en JPEG), ramenés en lumière linéaire par la dérivée de la courbe de transfert sRGB au niveau du pixel. Sans plancher, un aplat numérique donnerait un bruit nul et une sensibilité infinie.
3. **Propagation par pixel** : Σ_n(x) = J(x) · diag(σ²(I_c(x))) · J(x)ᵀ, où J(x) est le jacobien 3×3 de la conversion lumière linéaire → Oklab au pixel x. Les valeurs LMS sont bornées à ≥ 10⁻⁴ avant dérivation, car la racine cubique a une dérivée infinie en 0.
4. **Carte locale** : le même estimateur sur des tuiles (au moins 256 pixels plats par tuile, sinon valeur globale), interpolées bilinéairement. Elle capte les variations que la fonction de niveau ne décrit pas, comme le grain d'un scan.
5. **Image analysée** : la détection de bords (étapes B à E) travaille sur l'image **non débruitée**, car le modèle de bruit n'est valable que sur elle ; son propre lissage σ₀ fait office de débruitage. Le débruitage bilatéral (réglage Nettoyage) ne sert qu'à la postérisation.

### 4.4 Alpha

Si l'alpha n'est pas constant, il forme un canal à part, avec son propre bruit et un plancher de 0,5/255. Un canal constant est retiré du calcul, faute de quoi son poids serait infini. La frontière avec le transparent est le croisement à 50 % d'alpha. Ce bord prime sur le bord de couleur au même endroit : les pixels de bord semi-transparents prémultipliés créent sinon un faux liseré coloré. La zone transparente est une entrée « aucune » de la palette. Les remplissages semi-transparents (ombres, halos) sont hors du périmètre de cette version : ils sont traités comme des couleurs opaques mélangées au fond.

---

## 5. Étapes B et C — Bords sous-pixel en couleur

C'est le socle. Méthode : Canny/Devernay dans la variante de Grompone von Gioi et Randall (IPOL 2017, code AGPL, donc implémentée depuis l'article), appliquée à un gradient couleur de Di Zenzo généralisé [R1 §1–3, R5 §1.1].

### 5.1 Organisation du calcul (coût et mémoire)

Une pile complète de dérivées à plusieurs échelles en pleine résolution coûterait environ 3 milliards de multiplications-additions à 2 MP, soit plusieurs secondes en WASM [revue ingé. B1]. On organise donc le calcul ainsi :

- **Plans pleine résolution à une seule échelle σ₀** (≈ 1, de 0,7 à 1,5 selon le bruit), calculés **par bandes** de 64 à 256 lignes avec une marge égale au rayon du noyau + 2. Dans la bande, on ne fait que la NMS, la position de Devernay et les valeurs brutes par point (force, normale, contraste) ; les plans de dérivées sont ensuite libérés. On **garde** en pleine résolution les plans source (sRGB et Oklab) et les trois plans Oklab lissés à σ₀ (sans dérivées) : l'analyse finale par point en a besoin après le chaînage global.
- **Pyramide** (§2) pour les grandes échelles : chaque niveau coûte le quart du précédent. On y détecte les structures larges, floues ou peu contrastées : grandes lignes, grands cercles, taches, traits épais.
- **Analyse par point**, après le chaînage global : flou, échelle de localisation, relocalisation finale et variance se calculent pour chaque point de bord sur un **profil 1D le long de sa normale**, échantillonné dans les plans lissés à σ₀. Les échelles plus grandes s'obtiennent en convoluant ce profil en 1D. Le noyau effectif est alors anisotrope : σ_loc en travers du bord, σ₀ le long du bord. Les formules des §3.2, §5.5 et §5.6 en tiennent compte. Pour un bord droit, c'est exact ; pour un bord de courbure κ, ce schéma diffère du lissage 2D au premier ordre (≈ κσ_loc²/2), et il est en fait le moins biaisé des deux, car il lisse moins le long de la courbe. Le reste est tabulé sur la vérité terrain. Les localisateurs qui exigent des pixels bruts (surface partielle, ajustement erf) lisent les plans source.

### 5.2 Gradient couleur généralisé

Dérivées gaussiennes séparables à σ₀ : noyaux renormalisés (Σ g = 1, Σ x·g′(x) = −1), bords par réflexion, rayon ⌈4σ⌉ [R1 §1.1]. Tenseur de Di Zenzo **blanchi par la covariance du bruit** :

  G = ∇Lᵀ Σ_n⁻¹ ∇L,  g_ij = Σ_{c,c′} ∂_i L_c (Σ_n⁻¹)_{cc′} ∂_j L_{c′}
  λ± = ½ [ (g11 + g22) ± √((g11 − g22)² + 4 g12²) ], θ = ½ atan2(2 g12, g11 − g22)

- **Force** E = √λ+, en unités de bruit.
- **Normale orientée** n = (cos θ, sin θ), de signe choisi pour que n · ∇L_L > 0. Sur un bord isoluminant, on prend le canal de plus grand |n · ∇L_c|.
- **Indicateur de jonction** ρ = λ−/λ+ : proche de 0 sur un bord simple, grand près d'un coin ou d'une jonction.
- **Contraste par point** en Oklab non pondéré. Pour un bord en marche floue, Δ_c = √(2π) · s · ∂_n L_c, avec s² = σ² + σ_b² (σ_b : §5.5).
- **Variance du gradient sous le bruit** σ_g² = Σ_x k′(x)² · Σ_y k(y)², calculée sur les noyaux discrets réels. La valeur continue 1/(8πσ⁴) est fausse de 4 % à σ = 0,8.
- On ne lisse pas le tenseur (échelle d'intégration nulle) pour la localisation.

### 5.3 Suppression des non-maxima et position initiale

**NMS sur l'axe dominant** (variante IPOL), déterminé par θ :

- si |cos θ| ≥ |sin θ|, le point est un maximum si E(x−1, y) < E(x, y) ≥ E(x+1, y) ;
- sinon, test vertical équivalent.

L'inégalité stricte d'un côté départage les plateaux de façon déterministe.

**Correction de Devernay** le long de cet axe :

  η = ½ (a − c) / (a − 2b + c),  a = E(p − d), b = E(p), c = E(p + d)

- η = 0 si a − 2b + c = 0 ;
- |η| ≤ ½ ;
- biais résiduel de quelques centièmes de pixel, tabulé par simulation et soustrait [R1 §3.1].

### 5.4 Seuils et chaînage

Après blanchiment, en zone plate, trace(G)/σ_g² suit une loi χ² à 2C degrés de liberté (C = nombre de canaux actifs). Pour un nombre pair de degrés,

  P(χ²_{2C} > x) = e^{−x/2} Σ_{i<C} (x/2)^i / i!

Les seuils haut H et bas L correspondent à p_H = 10⁻⁵ et p_L = 10⁻² par pixel. Ils sont calculés une fois par (C, p) par Newton sécurisé en log. Le test sur λ+ ≤ trace est conservateur (d'un facteur ≈ 2,6 à 10⁻²). Une table de la plus grande valeur propre d'une loi de Wishart, simulée une fois à graine fixe, peut le rendre exact.

**Plancher perceptif** : un point n'est gardé que si |Δ| ≥ ΔE_min = 0,02 (réglé par Détail). Cela écarte les bords statistiquement réels mais invisibles, et les échos JPEG.

**Chaînage** [R1 §7.1] :
- voisinage 5×5 ;
- lien A → B si g(A)·g(B) > 0 et AB · g(A)^⊥ > 0 (le côté sombre reste du même côté) ;
- au plus un successeur et un prédécesseur par point ;
- pour l'indépendance à l'ordre de parcours, liens triés par longueur quantifiée au 1/16 de pixel (tri par paquets, linéaire) puis par indice, et acceptés gloutonnement ;
- le chaînage se fait sur la liste globale des points, après les bandes, pour que le découpage en bandes ne change pas le résultat.

**Hystérésis** le long des chaînes : une chaîne survit si un point dépasse H ; les points sous L la coupent. Les extrémités près des jonctions (le gradient y est indéfini) gardent leur tangente pour l'étape D.

**Validation des chaînes** (optionnelle) : NFA = N_chaînes · H(μ)^{n_eff}, où μ est le gradient minimal le long de la chaîne et H(μ) = P(|∇| ≥ μ) la distribution empirique sur l'image. Les points d'une chaîne ont été sélectionnés par la NMS, donc le modèle de fond est optimiste : ε se calibre sur des images de bruit.

### 5.5 Analyse par point : flou, localisation finale

Sur le profil normal de chaque point (§5.1) :

1. **Flou σ_b**, par forme fermée sur deux échelles. Pour une marche de flou t₀, le gradient vérifie E²(t) ∝ h²/(t₀ + t). Avec deux échelles t₁ < t₂ et R = E²(t₁)/E²(t₂) :
   t₀ = (t₂ − R·t₁)/(R − 1).
   Cette formule reste valable pour t₀ < σ₀², alors que le maximum de Lindeberg (γ = ½) bute sur le bas de la pile. Pour éviter le biais d'un rapport moyenné à faible rapport signal/bruit, on moyenne d'abord E²(t₁) et E²(t₂) sur une portion droite de la chaîne, puis on applique la formule une fois. Les points à moins de 2σ₂ d'un autre bord ou d'une jonction sont exclus : leur contraste change d'une échelle à l'autre. Les gains des noyaux discrets sont corrigés par une table. Le maximum γ = ½ sert seulement de contrôle.
2. **Échelle de localisation** σ_loc = max(σ₀, σ_b/2), en travers du bord. On ne localise jamais à une échelle grossière. Sur un bord courbe, le lissage le long du bord et le flou optique introduisent un biais ≈ κ(σ₀² + σ_b²)/2 vers le centre de courbure, qu'on soustrait ; le résidu d'ordre κσ_loc² dû au profil courbe est tabulé par simulation.
3. **Localisateur** :
   - si σ_b < 0,7 px et pas de JPEG, **effet de surface partielle** (Trujillo-Pino 2013 ; formules et port MIT dans [R1 §3.2]), appliqué aux **pixels bruts** d'une fenêtre 3×9 : son modèle (marche échantillonnée par surface) n'est pas valable sur un profil lissé. Il donne une précision < 0,01 px, plus une courbure et un contraste par point, pour un coût négligeable par point ;
   - si le bord est flou, **ajustement d'une marche floue** I(x) = B + (A − B)·Φ((n·x − ρ)/s) par Gauss-Newton, sur les pixels bruts d'une bande autour du bord, à leurs centres, le modèle étant intégré sur la surface de chaque pixel. Il donne position, flou, couleurs et covariance ;
   - sinon, Devernay.
4. **Espace de localisation** : on détecte dans Oklab, mais la position finale se calcule sur les canaux sRGB gamma. C'est là que les moteurs SVG anti-crénèlent, et le croisement à 50 % reproduit alors le raster. C'est une hypothèse d'une synthèse, à valider sur vérité terrain avant d'en faire le défaut ; un interrupteur interne permet de comparer.
5. **JPEG** : le sous-échantillonnage de la chrominance (4:2:0) décale les bords de a et b jusqu'à 1 px. On localise alors sur L seul, et la chrominance ne sert qu'à la détection.

### 5.6 Variance par point

Pour l'estimateur de Devernay avec le noyau anisotrope du §5.1 (σ_loc en travers, σ₀ le long), et s² = σ_loc² + σ_b² [R1 §7.2, adapté ; algèbre vérifiée par la revue] :

  var_n = (3/8) · (1/SNR²) · s⁶ / (σ_loc⁵ · σ₀),  SNR² = hᵀ Σ_n⁻¹ h

Pour un noyau isotrope (σ₀ = σ_loc), on retrouve (3/8)(1/SNR²)(s/σ_loc)⁶. Le SNR est calculé sur les canaux de **localisation** (sRGB gamma, ou L seul en JPEG, §5.5), avec leur propre covariance de bruit, et non sur les canaux Oklab de détection.

où h est le vecteur de contraste. Pour la surface partielle et l'ajustement erf, la variance vient de la covariance de l'ajustement, σ² (JᵀJ)⁻¹, avec un κ_c propre à chaque méthode. Les échantillons d'un profil lissé étant corrélés, cette covariance est calculée par moindres carrés généralisés avec la covariance connue du profil, ou bien l'ajustement se fait sur les échantillons bruts. S'y ajoute σ_sys (§3.1).

Chaque point stocke : position, normale orientée, contraste Δ, force E, σ_b, σ_loc, var_n, ρ, identifiant de chaîne. Stockage en structure de tableaux, environ 50 octets par point.

### 5.7 Traits fins (Steger)

Un trait de largeur W ≲ 3–4 px donne deux bords trop proches pour être localisés séparément. On le détecte comme une **crête** (ligne centrale + largeur) selon Steger 1998. Le code de référence est GPL, donc on part de l'article [R1 §6]. Calcul sur L seul, aux niveaux de pyramide utiles :

- hessienne, vecteur propre n de la plus grande |valeur propre| ;
- t = −(r_x n_x + r_y n_y)/(r_xx n_x² + 2 r_xy n_x n_y + r_yy n_y²) ;
- point de crête si |t n_x| ≤ ½ et |t n_y| ≤ ½ ;
- sélection d'échelle par N_γ = t^{4γ}(L_pp² − L_qq²)², avec γ = ¾. La forme t^{2γ} de [R1 §4.3] est fausse : elle sélectionne t₀/3. Le résultat t_sel = t₀ ne vaut que pour un profil gaussien ; la table de correction absorbe le cas d'une barre plate ;
- largeur et décalage corrigés par une table (largeur, asymétrie) → (largeur vraie, décalage), construite par simulation du profil analytique.

**Choix entre crête et paire de bords** : déterministe, selon la largeur corrigée. On traite le trait comme une crête si W < c · σ_loc, où c est tabulé par simulation : c'est la largeur à laquelle l'erreur de localisation à deux bords égale celle de la crête. Comparer les deux réponses normalisées n'aurait pas de sens, car elles n'ont pas la même unité.

Le repère « Trait » contraint les deux frontières de la zone fine à rester parallèles, et peut créer cette zone si la postérisation l'a perdue (§8.2). L'émission en trait SVG (`stroke`) viendra plus tard.

---

## 6. Étape D — Détecteurs de primitives

Ils travaillent sur les chaînes de C (ordonnées, sous-pixel, pondérées) selon le principe d'EDLines/EDCircles, dont le code des auteurs est sous MIT [R2 §1.2, R5 §1.2]. Toutes leurs décisions de modèle suivent le §3.4.

### 6.1 Segments de droite

1. **Croissance le long d'une chaîne** : ajustement par moindres carrés totaux pondérés (w_i = 1/v_i), puis extension tant que le point suivant reste sous τ_max (§3.4), et réajustement.
2. **Ajustement** : centroïde pondéré, matrice de dispersion, direction = vecteur propre de λmax. Covariances : var(φ) = κ_c / Σ w_i t_i² et var(décalage au centroïde) = κ_c / W, soit une incertitude en sablier [R2 §1.5].
3. **Validation a contrario** : NFA = N⁴ · B(n, k, 1/8), où k compte les points dont l'orientation est à moins de 22,5° de la normale. Comme LSD, les orientations utilisées ici viennent du masque 2×2, qui minimise la dépendance entre voisins [R2 §1.1]. Elles sont calculées aux points de la chaîne sur l'image source légèrement lissée (σ = 0,6, comme LSD), et **pas** sur le plan de dérivées à σ₀, dont les voisins sont trop corrélés. La longueur minimale est alors ≈ 12 px à 512². ε se calibre sur des images de bruit et d'aplats JPEG.
4. **Grandes lignes interrompues** : ce cas (« lignes coupées par d'autres formes ») passe par la relation de colinéarité de l'étape I (§11), **pas** par la fusion de D. La fusion de D ne réunit que des fragments d'un même bord séparés par une lacune de détection : test de Mahalanobis sur (θ, ρ), les deux droites étant exprimées au même point de référence (entre les segments), avec Δθ ramené dans (−π, π]. Puis test du trou NFA(union) < NFA(s₁) · NFA(s₂), avec le nombre de paires testées explicitement compté [R2 §1.6].
5. **Hough** : seulement en secours, pour des alignements très lointains (règles, grilles), par vote à noyau elliptique tiré des covariances. L'idée vient de KHT ; on ne prend que l'article, le code étant GPL.

### 6.2 Coins et jonctions

- **Topologiques** (exacts) : coins de pixels où au moins 3 étiquettes de zones se rencontrent (étape F). L'ordre m est le nombre de frontières incidentes.
- **Géométriques** (raster) : nécessaires pour les coins en L, qui ne séparent que 2 zones et sont donc invisibles en topologie. On calcule l'opérateur de Förstner **localement**, sur des fenêtres autour des candidats (points à fort ρ, extrémités de chaînes). Sous le bruit, avec des échantillons corrélés, la trace de M suit approximativement (n_px/n_w) · σ_g² · χ²_{2C·n_w} (approximation de Satterthwaite), où n_px est le nombre de pixels de la fenêtre (Σw pour une fenêtre pondérée) et n_w ≈ A/(2πσ₀²) son nombre d'échantillons effectifs (A = (Σw)²/Σw² pour une fenêtre pondérée). Le seuil s'en déduit ; il est vérifié par simulation. La rondeur doit vérifier q = 4 det M / (tr M)² > 0,5. Position par
  x̂ = (Σ w_i g_i g_iᵀ)⁻¹ Σ w_i g_i g_iᵀ x_i, covariance σ̂² M⁻¹ [R2 §3.2].
  Le même système avec les tangentes donne le **centre d'un arrondi**.
- **Position d'un coin** : intersection, pondérée par les covariances, des primitives ajustées sur chaque branche, en excluant les points à moins de 2σ_loc + σ_b du sommet (zone où l'anti-crénelage mélange les zones). Le maximum de Harris n'est jamais utilisé comme position : il est biaisé vers l'intérieur.
- **Typage** L, T, Y, flèche, X d'après les angles des branches [R2 §3.5].

### 6.3 Vif ou arrondi

**Géométrie du congé** [R2 §4.1] : sommet virtuel V, angle intérieur φ, bissectrice intérieure b, directions des branches u_k :

  C(r) = V + (r / sin(φ/2)) · b,  T_k = V + (r / tan(φ/2)) · u_k,  flèche h = r (1/sin(φ/2) − 1)

**Ajustement** : on minimise la distance des points à la courbe composite droite–arc–droite. Elle est continue en r, ce que ne serait pas une affectation des points qui change avec r. Ce n'est pas garanti unimodal : on encadre sur une grille de 16 valeurs, puis recherche dorée.

**Décision** : un coin vif flou n'est pas un coin vif géométrique, car le lieu des points de bord d'un coin vif flouté est lui-même arrondi, de 0,3 à 1 px. Comparer un coin géométrique à un congé géométrique ferait donc gagner le congé à chaque fois. On compare plutôt deux **modèles du lieu observé** :
- coin vif convolué par le flou total s (s² = σ_loc² + σ_b²), dont le lieu, à l'échelle s près, ne dépend que de φ : une table en φ ;
- congé de rayon r convolué par le même flou : une table en (φ, r/s).

Arrondi si ΔJ_eff > λ · 1 (§3.4). Le rayon minimal détectable découle alors du flou, au lieu d'être fixé à la main. Une variante plus simple, utilisée en mode Rapide, exclut des deux ajustements les points à moins de 2σ_loc + σ_b du sommet.

**Contrôle multi-échelle** : pour un coin vif, κ_max · σ_eff = c(φ) est constant, la table c(φ) étant faite par simulation ; pour un arrondi, κ_max ≈ 1/r. Ce contrôle produit des candidats et un diagnostic, pas la décision. Les éditions de l'utilisateur (vif imposé, rayon glissé à la souris) priment toujours.

### 6.4 Arcs, cercles, ellipses

Les chaînes sont découpées en arcs convexes, aux coins et aux inflexions. Pour chaque arc [R3 §1–3] :

- **Cercle** : ajustement algébrique *Hyper* (Al-Sharadqah & Chernov 2009, sans biais du second ordre), puis Levenberg-Marquardt dans le paramétrage de Chernov (A, B, C, D, contrainte B² + C² − 4AD = 1 éliminée). Ce paramétrage traite proprement la limite « droite » (R → ∞), là où un LM en (centre, rayon) est mal conditionné.
- **Ellipse** :
  - Halíř & Flusser 1998 après normalisation de Hartley : problème propre 3×3, toujours une ellipse ;
  - garde-fous : nombre de conditionnement de S₃, itération inverse si les valeurs propres sont proches, repli sur le cercle si aucun vecteur ne vérifie 4ac − b² > 0 ;
  - en mode Précis, hyper-renormalisation de Kanatani (nombre d'itérations fixe, arrêt déterministe), avec repli sur Halíř–Flusser si le résultat est une hyperbole ;
  - raffinement géométrique par la distance point–ellipse d'Eberly (bissection garantie). Si a ≈ b, φ est indéfini et on bascule sur le modèle cercle.
- **Étendue** : c'est une précaution de conditionnement, pas une règle de décision. Un cercle libre exige un arc ≥ 25–30°, une ellipse libre ≥ 120°, car en dessous la variance du rayon explose (en 1/Δ⁴). Un arc plus court peut toujours suivre un repère cercle ou ellipse établi ailleurs : c'est le modèle « sur repère » (§10.2), sans limite d'étendue.
- **Test des séries** sur le signe des résidus : de longues séries de même signe révèlent un mauvais modèle (superellipse, rectangle arrondi pris pour une ellipse), même si le résidu total est faible.

**Regroupement d'arcs** (un cercle coupé par d'autres formes), à la Fornaciari 2014 :
- convexités compatibles ;
- centres compatibles (Mahalanobis, ou cordes parallèles : leurs milieux sont alignés avec le centre) ;
- ajustement conjoint ;
- validation par NFA : fraction du périmètre soutenue par des points bien orientés ;
- union-find contre les doublons [R3 §2.2–2.4].

Les paires sont cherchées par grille spatiale des centres, avec un plafond par arc (§14.3).

### 6.5 Taches (blobs)

Laplacien normalisé t(L_xx + L_yy) et déterminant de la hessienne t²(L_xx L_yy − L_xy²) sur les niveaux de pyramide, sur L. On cherche les extrema dans (x, y, t). Pour un disque de rayon r, l'extremum est à σ = r/√2, ce qui donne le rayon directement. Raffinement par interpolation quadratique 3D [R3 §4.1]. Usage :

- repères Cercle pour les petits points que l'ajustement de contour ne traite pas bien ;
- insertion d'une zone que la postérisation a perdue (§8.2).

Les trames (points réguliers à une même échelle) et MSER/MSCR viendront plus tard.

---

## 7. Étape E — Le graphe de repères

### 7.1 Structure interne

| Champ | Contenu |
|---|---|
| type | Segment, Coin, Jonction, Arc, Cercle, Ellipse, Trait, Tache ; plus les relations Symétrie, Répétition, Alignement produites en I |
| géométrie | droite (θ, ρ) + extrémités ; cercle (C, r) + étendue ; ellipse (C, a, b, φ) + étendue ; coin : sommet, branches, rayon ; trait : ligne centrale + largeur |
| covariance | des paramètres (déjà multipliée par κ_c) |
| échelle | σ de détection (niveau de pyramide) |
| signification | −log₁₀ NFA (détection et affichage seulement) |
| support | chaînes de bords qui le soutiennent, et frontières qu'il touche |

Les repères sont stockés dans une arène (`Vec` + indices `u32`) avec une grille spatiale. Le type exporté vers l'interface (`guide::Guide`, généré en TypeScript) reste un objet de transfert séparé : type, point clé, statut, tracé SVG.

### 7.2 Fusion multi-échelle et statut

Deux repères de même type sont fusionnés si le test §3.4 l'accepte (Δk = nombre de paramètres). Comme deux détections d'un même objet partagent une partie de leurs points, la covariance de leur différence est Σ₁ + Σ₂ − 2Σ₁₂ ; l'ignorer rendrait la fusion trop facile, ce qui est sans gravité pour éliminer des doublons, mais le terme croisé est calculé quand les supports sont connus. On garde le mieux déterminé et on réunit les supports. Un repère qui n'existe qu'aux échelles grossières garde sa grande incertitude : il oriente le tracé sans imposer de position fine.

**Statut**, déduit après l'optimisation de G–I :
- **Suivi** : le tracé final l'utilise ;
- **Candidat** : significatif (NFA ≤ 1) mais non retenu ;
- **Forcé** : imposé par l'utilisateur ;
- **Désactivé** : retiré des propositions.

### 7.3 Stabilité des éditions

Les éditions sont mémorisées par type et position, comme aujourd'hui (GUIDE_REACH, CORNER_EDIT_REACH). Le nouveau détecteur déplace légèrement les points clés et introduit de nouveaux types. Il y a deux règles de migration :

- tolérance de recherche proportionnelle à l'incertitude du repère, avec un plancher à la valeur actuelle ;
- correspondance des anciens types vers les nouveaux (Line → Segment, Arc → Arc ou Coin arrondi, etc.).

Un test vérifie que plus de 95 % des éditions enregistrées sur l'ancien pipeline s'appliquent encore, sur tout le benchmark et avec un réglage légèrement modifié.

---

## 8. Étape F — Topologie

### 8.1 Postérisation (existante)

Inchangée : débruitage bilatéral dosable, palette k-means Oklab, étiquettes, composantes, cycles d'arêtes inter-pixels, découpage aux jonctions en **frontières partagées**. Chaque frontière entre deux zones est un objet unique, utilisé par les deux contours ; c'est ce qui exclut les trous et les chevauchements géométriques. Le filtre d'aire fixe est complété par la fusion des zones sous la tolérance (§3.5).

### 8.2 Réparation de topologie guidée par les repères (F′)

La postérisation fragmente parfois un trait fin, perd un petit point, fusionne deux formes à 1 px l'une de l'autre ou bouche l'œil d'une lettre. Aucun repère géométrique ne peut corriger cela après coup. On ajoute donc, entre F et G, quatre opérations proposées par les repères :

| Repère | Opération |
|---|---|
| Trait (Steger) sans zone fine correspondante | créer ou reconnecter la zone le long de la ligne centrale, à la largeur mesurée |
| Chaîne de bord significative qui traverse une zone | couper la zone le long de la chaîne |
| Tache sans zone | insérer une zone circulaire |
| Zone fragmentée le long d'une même crête | fusionner les fragments |
| Zone de mélange : bande fine (largeur < 2σ_b + 1 px) le long d'un bord réel, de couleur intermédiaire entre ses deux voisines | la fusionner avec ses voisines (échos JPEG, anti-crénelage pris pour une couleur), quel que soit le réglage Échelle, sauf si elle porte un repère Trait ou si elle est bordée de deux bords significatifs parallèles (vrai filet fin, par exemple un séparateur gris de 1 px entre du blanc et du noir) |

Une opération est acceptée si elle réduit l'écart de couleur entre la carte d'étiquettes et la source (ΔE moyen sur la bande concernée) d'au moins ΔE_min. Ce critère ne dépend d'aucun curseur de G–J, ce qui permet de garder F′ dans le cache (§14.6).

Chaque opération est stockée comme une **surcouche réversible** attachée à son repère. Les surcouches ne commutent pas : elles sont appliquées dans un ordre fixe (par type, puis par identifiant de repère) sur une copie de la carte d'étiquettes, et rejouées dans cet ordre quand l'une d'elles est retirée. Désactiver un repère retire sa surcouche, sans recalculer la postérisation. Dans la première version, les zones et frontières sont ensuite reconstruites globalement à partir de la carte modifiée ; une reconstruction locale est une optimisation ultérieure.

**Couleur d'une zone créée** : l'entrée de palette la plus proche de la couleur mesurée sur ses pixels, si l'écart est sous ΔE_min ; sinon une entrée dérivée, ajoutée à la palette et marquée comme telle dans l'interface.

Cette étape vient en phase P6b (§16) : jusque-là, ces cas restent une limite connue.

### 8.3 Coutures

Les liserés clairs ou sombres entre deux formes qui partagent exactement la même frontière viennent du rendu : chaque forme est anti-crénelée séparément, puis composée, si bien que le pixel de bord n'est couvert qu'à environ 75 %. Émettre la frontière une seule fois **ne suffit pas**. On garde donc une stratégie anti-couture explicite :

- par défaut, le trait de comblement actuel (`fillGaps`) ;
- ou le mode empilé, où la forme du dessous déborde sous celle du dessus.

Une mesure la contrôle (§13.2). Le SVG est rendu à 1× et 4× par le rastériseur de couverture de `core` (§14.1), en reproduisant la composition forme par forme des navigateurs, et aussi par un navigateur dans les tests e2e. On compare l'écart de couleur le long des frontières partagées à celui mesuré à l'intérieur des zones voisines, sur la source agrandie de la même façon. Une couture est un excès d'écart localisé sur la frontière.

---

## 9. Association frontière ↔ bords (F″)

C'est le cœur de l'hybridation : la topologie vient de la postérisation, la géométrie des bords raster.

### 9.1 Points de repli

Pour chaque frontière (zones A | B), la position par couverture mesurée entre les deux teintes est calculée comme aujourd'hui. Elle sert là où aucun bord n'est apparié. Ces points sont lissés par l'opérateur λ|μ de Taubin (λ = 0,33, μ = −0,34, sans rétrécissement), morceau par morceau et sans traverser les coins [R4 §3.1]. Leur variance et leur κ_c sont tabulés par simulation (§3.2).

### 9.2 Appariement

Un point de bord q est apparié à la frontière A | B si trois conditions sont réunies :

1. sa distance à la frontière est inférieure à 2σ_loc + σ_b + 3√var_n(q) ;
2. sa normale est compatible avec celle de la frontière : écart inférieur à 3 écarts-types d'orientation, et même sens A → B. La variance d'orientation d'un point vaut ≈ σ_g²/E² (en unités blanchies), celle de la frontière vient de son ajustement local, κ_c/Σ w t² ;
3. son contraste Δ(q) est compatible avec la **différence de couleur locale** mesurée de part et d'autre de la frontière, à cet endroit : moyenne des pixels source dans une bande de 2–3 px côté A et côté B. On n'utilise pas les couleurs moyennes de la palette : sur un logo rempli d'un dégradé, elles sont loin des couleurs locales, et le vrai contour serait déclassé à tort. La compatibilité se juge par un test de Mahalanobis entre Δ(q) et c_B,local − c_A,local, avec les covariances du bruit, au seuil χ²₃ à 99 % (11,34).

L'appariement se fait par programmation dynamique monotone le long de la frontière, en bande (largeur fixée par la condition 1), pour éviter les croisements.

### 9.3 Statut des sous-morceaux

La frontière est découpée en **sous-morceaux** de statut homogène :

- **bord réel** : une série de points appariés ;
- **bord de postérisation** : le reste, typiquement la limite entre deux bandes d'un dégradé.

Des sous-morceaux plus courts que 2σ_loc + σ_b sont absorbés par leurs voisins.

Un bord de postérisation ne reçoit **aucun modèle géométrique** : la segmentation n'y propose que des Béziers, et sa variance est celle de la couverture, élargie. C'est la version principielle de l'ancien seuil SOFT_EDGE, et la réponse aux hallucinations sur les dégradés. Le rendu des dégradés eux-mêmes (`linearGradient`, `radialGradient`) est une étape ultérieure de la feuille de route.

### 9.4 Position et repères attachés

Sur un bord réel, les points de la frontière sont les points de bord appariés, avec leur variance. Les repères dont le support touche la frontière lui sont attachés, avec leur portée (abscisses de début et de fin le long de la frontière).

---

## 10. Étapes G et H — Segmentation et ajustement des frontières

### 10.1 Programmation dynamique

Pour chaque frontière, on minimise E (§3.3) par programmation dynamique : F[j] = min_{i<j, m} F[i] + coût(i, j, m), avec des pointeurs arrière [R4 §1.2].

**Nœuds** : seulement les ruptures candidates, pas tous les points :
- coins et jonctions (D, F), extrêmes de courbure, extrémités de repères, changements de statut (§9.3) ;
- et, sur une longue portion lisse sans candidat, un nœud tous les 16 points, pour que les Béziers puissent s'y raccorder.

Les marches d'escalier d'une droite discrète ne sont jamais des candidats : le critère des segments maximaux de Lachaud et al. les élimine [R4 §3.2]. Une frontière de N points a ainsi N_c ≈ N/5 à N/16 nœuds.

**Portées admises** : au plus k = 8 intervalles entre nœuds, et fenêtre de W = 200 à 400 points. Frontières fermées ouvertes au coin le plus sûr, avec au plus 2 points de départ.

**Coûts des modèles sur une portée**, tous au même format J ≈ Σ w_i f_i² / (Σ w_i |∇f_i|² / Σ w_i) (somme pondérée des distances carrées, pas une moyenne) :
- droite : λmin de la matrice de dispersion pondérée ;
- les « paramètres des moindres carrés » d'un cercle ou d'une ellipse sont ceux des ajustements algébriques de Pratt et de Taubin : c'est légèrement pessimiste sur les arcs courts, mais appliqué de la même façon à tous les modèles ;
- cercle : résidu algébrique sous la normalisation de Pratt (B² + C² − 4AD = 1), égal au résidu géométrique au premier ordre ;
- ellipse : résidu de Taubin, **seulement** sur les portées qui tournent d'au moins 120° ou qui portent un repère ellipse ;
- Bézier : dans la programmation dynamique, un seul ajustement linéaire aux tangentes imposées (système 2×2 de Schneider, sans itération de Newton). Les tangentes aux nœuds viennent du repère adjacent s'il y en a un, sinon de l'estimateur λ-MST calculé une fois par nœud. Schneider complet et SDM ne tournent que sur les portées retenues.

Droite, cercle et ellipse se calculent en O(1) par sommes préfixes de moments, avec deux précautions numériques :
- coordonnées normalisées par la boîte englobante de la frontière ;
- sommes préfixes **par blocs** de 64 à 256 points, chacun avec son origine locale, combinées par les formules de translation de moments (Pébay). Des moments bruts d'ordre 4 perdraient 6 à 10 chiffres significatifs sur les portées courtes, celles justement où la programmation dynamique tranche.

Un test compare le coût par préfixes au coût direct sur une portée éloignée de l'origine.

**Zone morte** (§3.5) : elle doit peser sur **toutes** les décisions de modèle, pas seulement sur les portées en échec. Elle n'a pas de forme en O(1). On procède en deux temps :
1. **Tri rapide** en O(1) de tous les modèles de chaque portée, avec un coût approché où la variance est gonflée, v_i + (τ_perc/3)². Ce coût sert uniquement à classer.
2. **Coût exact** à zone morte pour les **deux meilleurs modèles** de chaque portée selon ce tri. Il est évalué aux paramètres des moindres carrés ordinaires, en une passe sur les points de la portée (O(n_span)). On l'évalue aussi pour tout modèle de droite, de cercle ou « sur repère » dont l'écart maximal reste sous τ_max, même s'il est mal classé. Le modèle « sur repère » passe toujours par ce coût exact : sa perte à zone morte n'est pas linéaire, donc ses sommes préfixes ne servent qu'au tri rapide.

Le minimum de la programmation dynamique se calcule sur ces coûts exacts. Coût : 10⁴ nœuds × 8 portées × 2 modèles × ~64 points × ~10 opérations ≈ 0,1 G (§14.2).

**Pontage** (§3.5, point 2) : sur les portées où le meilleur modèle simple échoue de peu (écart maximal au-dessus de τ_max sur au plus deux séries courtes), un réajustement par moindres carrés repondérés avec la perte tronquée, en 3 itérations au plus et avec le terme η du §3.5, fournit le coût définitif.

**Élagage (PELT)** : on retire i des candidats si F[i] + coût(i, j) ≥ F[j], où coût(i, j) **exclut** les pénalités (λc, λπ, λ_b). Cette règle n'est valable que si coût(i, k) ≥ coût(i, j) + coût(j, k), ce qui suppose κ_c constant sur la frontière. Ce n'est pas le cas avec les portées infaisables, le pontage ni le coût à zone morte évalué à paramètres fixés. L'élagage est donc désactivé dès que τ_perc > 0 ou qu'un pontage est possible, et les bornes ci-dessus suffisent.

**Budget en unités de travail** : chaque frontière dispose d'un budget compté en évaluations de coût, proportionnel à sa longueur. S'il est épuisé, la frontière est tracée en Béziers seuls, et le cas est compté dans les statistiques. Le budget n'est pas mesuré en temps : le résultat ne doit pas dépendre de la vitesse de la machine.

### 10.2 Modèle « sur repère »

C'est la correction du défaut « repères d'ellipse non suivis ». Chaque repère attaché à une frontière (cercle, ellipse, droite, arc) propose sur sa portée un modèle dont **les paramètres sont ceux du repère**.

**Paramètres hors portée.** Pour chaque morceau de frontière situé dans la portée du repère, les paramètres sont réajustés **sans les points de ce morceau** (une fois par morceau, pas par portée de la programmation dynamique). Si les points hors du morceau représentent moins de la moitié du support du repère, le repère n'est pas proposé sur ce morceau : le modèle libre s'en charge, avec son plein coût. Ce seuil de moitié est une convention qui économise du calcul ; c'est le terme d'Occam ci-dessous qui pénalise, en principe, un soutien faible. Un repère soutenu ailleurs est une vraie prédiction.

**Coût.** L'incertitude du repère est une erreur **commune** à tous les points de la portée. Le coût combine les écarts et un terme d'Occam :

  J_g = r̃ᵀ (κ_c V + A Σ_g Aᵀ)⁻¹ r̃ + min( λ · c(m), ln det(I + Σ_g Aᵀ (κ_c V)⁻¹ A) )

- r̃ : écarts passés par la zone morte du §3.5, la même perte que pour les modèles libres ;
- V = diag(v_i) ;
- A = ∂d/∂p : jacobien des écarts par rapport aux paramètres du repère ;
- Σ_g : covariance des paramètres hors portée.

Le terme d'Occam vaut 0 quand le repère est parfaitement connu, et tend vers le prix λ·c(m) du modèle libre quand son incertitude grandit. Un repère flou ne devient donc jamais un modèle libre gratuit. Le calcul se fait par la formule de Woodbury en O(c³), à partir de sommes préfixes de Aᵀ V⁻¹ A et Aᵀ V⁻¹ r le long du morceau.

Ce modèle n'a ni a priori de simplicité ni limite d'étendue. Un cercle coupé en trois arcs de 40° par d'autres formes est ainsi proposé sur chacun des trois morceaux, chacun étant prédit par les deux autres. Ses paramètres sont ensuite **partagés** à l'étape H (§10.5). Un repère forcé par l'utilisateur rend ce modèle obligatoire sur sa portée ; un repère désactivé ne le propose plus.

### 10.3 Coin ou jonction lisse

À chaque rupture retenue, le test §3.4 tranche, avec Δk = 1 entre « point et tangente partagés » (G1) et « point partagé seul » (coin). Il existe aussi un filtre perceptif, assumé comme tel : un virage de moins de 3 écarts-types d'orientation est toujours lisse. Les coins détectés sur le raster imposent la rupture. Leur type (§6.3) et les éditions de l'utilisateur priment.

### 10.4 Béziers

On part de Schneider (Graphics Gems, licence libre) [R4 §2.1] :
- tangentes d'extrémité données par la primitive voisine ou le repère ; à défaut, par l'estimateur λ-MST ;
- paramétrage par longueur de corde ;
- système 2×2 pour les longueurs de poignées, qui sont imposées positives ;
- reparamétrage de Newton.

**Raffinement par minimisation de la distance au carré** (SDM, Wang-Pottmann-Liu 2006) : convergence quasi-newtonienne, plus robuste que la projection point par point. Références lisibles à porter : `curve-fit-nd` (Blender, BSD-3) et `kurbo::fit` (MIT/Apache).

### 10.5 Ajustement conjoint (H)

Chaque frontière est réajustée d'un bloc par un Levenberg-Marquardt générique (`num::lm`) :
- paramètres mis à l'échelle (px, rad, rayon) ;
- amortissement de Nielsen ;
- nombre maximal d'itérations fixe ;
- critères d'arrêt déterministes.

**Contraintes éliminées par paramétrage** (le plus robuste) :
- Bézier–droite : P1 = J + β·u, avec J sur la droite ;
- Bézier–Bézier G1 : P2^A = J − β_A·d(φ), P1^B = J + β_B·d(φ).

**Contraintes restantes**, comme la tangence droite–cercle en forme signée s(n·C − ρ) − r = 0, où s est fixé par le côté de la droite où se trouve le cercle. Elles sont traitées par complément de Schur : A = JᵀJ + μI est symétrique définie positive et à bande pour une frontière ouverte, donc on fait une factorisation de Cholesky à bande de A, puis on résout le petit système dense (C A⁻¹ Cᵀ) ν = c − C A⁻¹ Jᵀr, et enfin δ = −A⁻¹(Jᵀr + Cᵀν). La matrice KKT complète est indéfinie : une factorisation de Cholesky directe y échouerait. Pour une frontière fermée, le bloc de bouclage est traité par la formule de Woodbury.

**Repères partagés entre frontières** : ils couplent les frontières qui les utilisent. On résout par **composante connexe** du graphe « frontière — repère partagé ». Chaque frontière garde sa matrice à bande, et le complément de Schur porte sur les seuls paramètres partagés : un petit système dense. Une composante est plafonnée à 50 frontières et 200 paramètres partagés ; au-delà, on alterne un nombre fixe de fois entre « paramètres partagés figés, frontières ajustées » et « frontières figées, paramètres partagés ajustés ».

**Jonctions** : les points de jonction entre trois zones ou plus restent fixes pendant H. Ceux qui existent dès la postérisation ont été placés en D par intersection des branches. Ceux qui naissent plus tard (réparation F′, fusion de petites zones) partent du sommet d'arête inter-pixels, sont placés une fois par intersection des primitives ajustées en G, puis figés.

**Pas de seconde segmentation** : G n'est pas relancée après H. Si un repère partagé bouge de plus de τ_max lors de H, les portées concernées sont signalées dans les statistiques ; ce cas sert de test.

La continuité de courbure (G2) n'est pas visée dans la première version.

---

## 11. Étape I — Régularisation globale

### 11.1 Relations candidates

| Relation | Génération des candidats | Paramètre partagé |
|---|---|---|
| Colinéarité, y compris de segments éloignés ou interrompus par d'autres formes | regroupement de (θ, ρ) via une grille | la droite |
| Parallélisme, orthogonalité | histogramme de θ mod π, fenêtre 2–4° | θ (ou θ + π/2) |
| Axes horizontal, vertical, 45° | attracteurs de l'histogramme | θ |
| Rayons égaux | regroupement relatif 3–5 % | r |
| Concentricité | centres à moins de 2σ | C |
| Tangence | distance (C, droite) ≈ r | contrainte signée |
| Symétrie miroir ou rotation | corrélation circulaire de la courbure par FFT, énergie des harmoniques de Fourier ; vote d'axes entre zones | appariement de points |
| Répétition | signatures (Hu, descripteurs de Fourier elliptiques), alignement de Procrustes | forme moyenne + transformations |

Les modules actuels `shape.rs` (symétries, profil radial), `repeat.rs` (seaux de tailles, Procrustes) et `align.rs` deviennent des générateurs de candidats.

### 11.2 Acceptation

Une relation qui retire q degrés de liberté en faisant entrer un élément dans une classe (« cette droite rejoint la classe des droites parallèles à θ₁ ») est acceptée si

  ΔJ_eff + 2 ln(n_type + 1) < λ · q

où n_type est le nombre d'éléments de ce type dans l'image (droites pour le parallélisme, arcs pour les rayons égaux). C'est le coût de description du choix de la classe, l'équivalent MDL du nombre de tests d'un NFA : sans lui, parmi des centaines de candidats, beaucoup de relations accidentelles seraient acceptées. Il ne dépend pas des fenêtres de génération. Avec 200 segments et Géométrie = 1 (λ ≈ 19,5), il reste une marge d'environ 9 par élément ; compter par paires (2 ln du nombre de paires ≈ 19,8) aurait interdit toute relation.

Pour une relation qui touche plusieurs frontières, λ est calculé avec L = diagonale de la boîte englobante de l'ensemble des éléments concernés et σ̄ = racine de la médiane de leurs variances.

Les écarts inférieurs à l'échelle de perception sont gratuits grâce à la zone morte (§3.5). Il n'y a donc **pas** de seconde condition sur l'écart maximal, qui contredirait la règle « un détail sous τ_perc n'empêche pas une symétrie ».

**Ordre** : glouton, du ΔJ_eff le plus faible au plus fort. On rejette les relations contradictoires : un cycle parallèle/orthogonal incohérent est détecté par l'union-find avec étiquettes d'angle mod π/2. On réajuste par **composante connexe** du graphe des relations. La symétrie exige au moins 3 éléments appariés : sur les petites formes, elle est souvent accidentelle.

### 11.3 Résolution

- **Relations de même valeur** (parallèle, colinéaire, concentrique, rayon égal, axe, repère partagé) : par élimination. L'union-find donne une variable par classe, puis Levenberg-Marquardt.
- **Autres** (tangence, symétrie, longueurs égales) : par espace nul (QR de Cᵀ, puis Cholesky sur le système réduit) ou par lagrangien augmenté.
- **Par étapes**, à la GlobFit : orientations, puis positions, puis égalités, avec mise à jour des jonctions entre chaque étape.
- Composantes plafonnées à 200 inconnues en dense ; au-delà, résolution par étapes seulement.

### 11.4 Répétitions et `<use>`

Les instances d'une forme répétée sont moyennées dans leur repère canonique : le bruit baisse en 1/√k pour k copies. Elles sont émises comme un tracé maître et des `<use>` avec transformation. Un détecteur de réseau (vecteurs de différence les plus fréquents) aligne les positions sur une grille ou un cercle quand le critère l'accepte.

---

## 12. Seuils, adaptation locale et réglages

### 12.1 Bilan des constantes

| Grandeur | Valeur | Nature |
|---|---|---|
| bruit σ_n (fonction de niveau, carte) | mesuré | image |
| flou σ_b | mesuré (§5.5) | image |
| σ_sys | mesuré sur vérité terrain | calibration |
| probabilités p_H, p_L | 10⁻⁵, 10⁻² | probabilité de fausse alarme |
| ε des NFA | 1 (calibré sur images de bruit) | nombre de fausses alarmes |
| précision angulaire des NFA | 22,5° (p = 1/8) | théorie LSD |
| λ | de 2 à 2 ln(L/σ̄) | théorie (G-AIC ↔ G-MDL) |
| a priori de type π | 0 / 0,5 / 1 / 1,5 | goût, calibré sur corpus |
| α de Šidák | 10⁻² | probabilité |
| ΔE_min | 0,02 Oklab | perception |
| étendue min. cercle / ellipse libres | 25–30° / 120° | conditionnement numérique |
| fenêtre de candidats (angles, rayons) | 2–4°, 3–5 % | génération seulement |
| ρ_éch, β, τ_abs | réglage Échelle ; plafond 4 px | utilisateur ; perception |
| z du pontage, budget de couleur | 4 ; 2 % × Échelle | perception, calibré sur corpus |
| bandes de calcul | 64–256 lignes | technique, sans effet sur le résultat |
| tuiles de bruit | 256 px plats min. | technique, **influe** sur les seuils locaux |
| nœuds, fenêtre W, k, plafonds de paires | 1 nœud / 16 points, 200–400, 8, §14.3 | budget, **influe** sur l'optimum quand une portée optimale dépasse la borne |

Il reste des constantes. Chacune est étiquetée selon sa nature : image, théorie, probabilité, perception, calibration ou budget, et celles qui influent sur le résultat sont signalées. Le protocole de calibration (§13.1) fixe celles qui sont marquées « calibration » ou « goût ». Les seules longueurs en pixels qui restent sont le plafond τ_abs et des marges de calcul tirées de σ₀, σ_loc et σ_b.

### 12.2 Adaptation locale

1. **Bruit local** : fonction de niveau + carte de tuiles (§4.3), d'où des seuils de détection locaux.
2. **Flou local** : σ_b par point (§5.5), d'où l'échelle de localisation et la variance.
3. **Complexité locale** κ_loc : longueur de chaînes par unité d'aire, normalisée par la médiane de l'image. Dans les zones chargées (texture, feuillage), λ_local = λ · (1 + log(1 + κ_loc)) : on simplifie davantage et on cherche moins de géométrie. Cette loi est une première proposition, à calibrer sur le corpus (§13).

### 12.3 Vérification (J)

Avant l'émission, chaque frontière est confrontée aux pixels, à l'aide du rastériseur de couverture de `core` :
- l'écart de couleur entre le rendu local et la source, sur la bande de la frontière, doit rester sous ΔE_min, hors des points tolérés et pontés, qui relèvent du budget de couleur par forme (§3.5) ;
- l'écart géométrique doit rester sous τ_max.

En cas d'échec, la frontière est retracée **une seule fois** avec λ divisé par deux, puis émise telle quelle avec un compteur d'échecs dans les statistiques. C'est la généralisation de la vérification actuelle (`Prepared::deviation`).

### 12.4 Réglages utilisateur

| Réglage | Effet | Correspondance avec l'ancien réglage |
|---|---|---|
| **Géométrie** (0–1) | λ de 2 à λ_MDL ; poids de l'a priori de type | même sens ; table de conversion pour les réglages enregistrés et le benchmark |
| **Échelle** (0–1, défaut 0,3) | ρ_éch ∈ [0 ; 0,01], plafond τ_abs, β ∈ [0 ; 10 %] (§3.5) | remplace « relative = 0,02 × échelle » |
| **Détail** | ΔE_min et aire minimale | inchangé dans son sens |
| **Nettoyage** | débruitage bilatéral, pour la postérisation seulement : il agit sur les couleurs et les zones, plus sur la géométrie, que les bords mesurent sur l'image non débruitée (§4.3) | même nom, effet réduit aux couleurs |
| **Lissage** | a priori des Béziers, lissage Taubin des points de repli | inchangé dans son sens |
| **Qualité/temps** (nouveau) | §12.5 | — |
| Coins, repères, palette | éditions par position (§7.3) | migration testée |

### 12.5 Qualité/temps

| Mode | Résolution d'analyse max | Localisateur | Ellipses | Régularisation |
|---|---|---|---|---|
| Rapide | 1 MP | surface partielle ou Devernay | Halíř–Flusser | colinéarité + axes |
| Équilibré (défaut) | 4 MP | surface partielle / erf / Devernay (§5.5) | + LM géométrique | toutes, sauf symétries entre zones |
| Précis | 8 MP | idem | + hyper-renormalisation | toutes |

Au-delà de la résolution d'analyse, **tout le pipeline** tourne sur l'image réduite (flou gaussien puis rééchantillonnage par surfaces), et le SVG est remis à l'échelle. Ce choix borne le temps comme la mémoire ; il a été **validé par le propriétaire du projet**. Les plafonds sont propres à chaque plateforme : ceux du tableau valent pour la version web, et l'application de bureau, qui dispose en principe de plus de puissance de calcul et de mémoire, peut les relever. La classification d'entrée et le bruit sont estimés sur l'image d'origine (§4.1). Les éditions de l'utilisateur sont stockées en coordonnées de l'image d'origine et converties. En mode Précis, une relocalisation par bandes à pleine résolution, autour des seules chaînes trouvées, est une option ultérieure.

Mémoire de pointe visée : Rapide ≤ 100 Mo, Équilibré ≤ 300 Mo (4 MP), Précis ≤ 500 Mo (8 MP), avec un avertissement dans l'interface pour ce dernier.

---

## 13. Validation

### 13.1 Vérité terrain synthétique

C'est le seul moyen de mesurer une précision sous-pixel [R5 §4.2]. Un générateur (tests de `crates/core`) dessine des scènes à paramètres connus :

- **Formes** : droites, polygones aux angles de 30° à 170°, coins arrondis de rayon connu, cercles, ellipses (étendues de 30° à 360°), Béziers, traits de 0,75 à 4 px, points de 2 à 8 px, texte de 8 à 24 px (police sous licence OFL, rendue dans le test), dégradés de fond et dégradés de remplissage.
- **Rendu** : anti-crénelage analytique ou sur-échantillonnage 16×16.
- **Dégradations** : flou σ_b ∈ {0 ; 0,5 ; 1 ; 2}, bruit (SNR de 10 à 40 dB), JPEG qualité {30, 50, 75, 90}, agrandissement au plus proche et bilinéaire, tailles {32, 64, 128, 512, 4096}.

Graines fixes ; images de 256² au plus pour les tests rapides ; le profil de test compile `core` en optimisé. Rien n'est stocké dans le dépôt, à part le code.

**Calibration** : ce corpus fixe σ_sys, les tables (Devernay, c(φ), Steger, κ_c), ε et les a priori π. Le corpus de calibration et le corpus de mesure sont distincts (graines différentes).

| Étape | Mesure | Cible (net, SNR ≥ 20 dB) | Cible (JPEG q50, flou 1) |
|---|---|---|---|
| A | erreur sur σ_n | < 10 % | < 25 % |
| C | erreur RMS de position des bords | < 0,05 px | < 0,15 px |
| C | erreur sur σ_b | < 15 % ou < 0,1 px | < 25 % |
| D | segments : rappel, erreur aux extrémités | > 95 %, < 0,5 px | > 85 %, < 1 px |
| D | coins : détection, position, vif/arrondi juste, rayon | > 95 %, < 0,2 px, > 95 %, ± 10 % | > 85 %, < 0,5 px, > 85 %, ± 20 % |
| D | cercles / ellipses (arc ≥ 120°) : centre, axes, angle | < 0,1 px, < 1 %, < 1° | < 0,3 px, < 3 %, < 3° |
| D | **fausses détections** (repères significatifs par MP sur bruit pur, texture, blocs JPEG, dégradés) | ≤ 1 | ≤ 1 |
| F′ | traits fins, texte, points : composantes connexes justes, largeur | 100 %, ± 0,2 px | > 95 %, ± 0,4 px |
| G–I | primitives retrouvées avec le bon type | > 90 % | > 75 % |

### 13.2 Batterie de cas « défauts signalés »

Un générateur par défaut signalé lors des tests, avec un critère réussi/échoué, exécuté à chaque phase :

| Défaut | Scène | Critère |
|---|---|---|
| Géométrie sur les dégradés | fond en dégradé, forme pleine posée dessus | 0 repère suivi sur les bandes ; contour de la forme suivi |
| Contour d'une forme remplie d'un dégradé | rectangle et cercle en dégradé sur fond uni | repères suivis sur le contour, 0 sur les bandes internes |
| Grandes lignes et cercles manqués | cercle de diamètre 60 % du côté de l'image, contraste ΔE = 0,05, + petits détails | grand cercle suivi |
| Lignes interrompues | une droite coupée par 3 formes | une seule droite partagée (écart < 0,1 px) |
| Ellipse non suivie | cercle coupé par 2 à 4 bandes | un seul cercle partagé par tous les morceaux, à 0,1 px près |
| Coutures | formes sombres adjacentes ; bleu/gris avec liseré | excès d'écart de couleur sur les frontières partagées, par rapport à l'intérieur des zones (définition du §8.3), < ΔE_min, rendu à 1× et 4× |
| Artefacts JPEG | pétales colorés en JPEG q50, à Échelle = 0 et 0,3 | aucune zone parasite (échos supprimés par le plancher perceptif, le débruitage et la fusion des zones de mélange) ; ΔE moyen ≤ 1,1 × celui obtenu sur le PNG |
| Petit détail qui casse la symétrie | forme symétrique **nette**, encoche de profondeur 0,5 τ_perc puis 2 τ_perc | symétrie retrouvée à 0,5 τ_perc ; encoche conservée à 2 τ_perc |
| Formes fines | trait de 300 × 3 px ; texte de 8 px ; filet gris de 1 px entre blanc et noir | largeur du trait à ± 0,2 px ; lettres lisibles, aucun œil bouché ; filet conservé |
| Pixel art | sprite à grille native de 4 px ; logo crénelé sans grille | sprite détecté et averti ; logo **non** classé pixel art |
| Petit élément voisin d'une grande forme | point d'un « i » à 2 px d'un grand cercle | le point est conservé à toute valeur d'Échelle |
| Simplification progressive | logo testé à Échelle = 0 ; 0,25 ; 0,5 ; 0,75 ; 1 | nombre de nœuds en baisse (corrélation de rang ≤ −0,9, §3.5) |
| Icônes sur fond transparent | icônes anti-crénelées sur alpha | aucun liseré ; bord au croisement à 50 % d'alpha, à 0,1 px |
| Adaptation locale | moitié texture, moitié logo | repères suivis par unité d'aire côté texture ≤ 50 % de ceux côté logo ; côté logo, résultat identique à celui du logo seul |

### 13.3 Benchmark de non-régression

Les 515 cas (`openvectorizer eval testdata --baseline testdata/baseline.json`) :

- ΔE moyen ≤ 0,927, p95 ≤ 2,93 (valeurs actuelles) ;
- **par image** : aucune image dégradée de plus de 10 % de ΔE ;
- suivi supplémentaire : nombre de nœuds, taille du SVG, part de la longueur des contours émise en droites, arcs ou ellipses.

Ce benchmark est surtout fait de rendus synthétiques. Il ne suffit pas pour les images réelles, d'où les points 13.4 et 13.5.

### 13.4 Corpus d'aller-retour et images réelles

- Rendus d'icônes et d'emojis sous licence libre (Noto Emoji et Material Symbols, Apache-2.0) à plusieurs résolutions, avec et sans JPEG. On compare au SVG d'origine :
  - distance de Chamfer et de Hausdorff des contours ;
  - précision et rappel des coins à 1–2 px ;
  - nombre de nœuds ;
  - part des primitives régularisées.
- Les données sont téléchargées à la demande et jamais commitées. Les jeux de recherche (YorkUrban, Prasad, Fornaciari, BIPED) sont utilisés ponctuellement, en local.
- **Comparaison avec vectorizer.ai**, en local uniquement : une vingtaine d'images libres, avec leurs résultats vectorizer.ai conservés hors du dépôt. Mesures : ΔE, nombre de nœuds, comparaison visuelle A/B. Objectif de fin de chantier (P8) : ΔE et nombre de nœuds au moins aussi bons sur 60 % des images, et aucune image nettement moins bonne à l'œil.

### 13.5 Performance, mémoire, robustesse

- **Chronomètres par étape** dans `eval` (natif, release) et dans le worker WASM (`performance.now()`), plus une sonde mémoire (`memory.buffer.byteLength`), dès la phase P1.
- **Réduction au-delà du plafond** (§12.5) : sur cinq images de 16 MP, ΔE, nombre de nœuds, temps et mémoire selon le plafond, pour fixer les valeurs par défaut du web et du bureau.
- **Tests d'endurance** : une photo de 2 MP, une frontière de 4000 px, une image de 16 MP (chemin de plafonnement). En intégration continue, les assertions portent sur les **unités de travail** comptées, qui sont déterministes ; les temps sont mesurés et publiés, mais ne font échouer que les mesures locales en mode release.
- **Cas limites** : 1×1, 1×N, image entièrement transparente ou constante ; propagation de NaN.
- **Sorties de débogage** : un SVG des points de bord, chaînes, repères (avec ellipses de covariance) et ruptures de la programmation dynamique, pour le développement.

### 13.6 Déterminisme

Test doré : hachage du SVG sur un petit corpus, **identique au bit près** en natif et en WASM (`wasm-pack test --node`), pour les préréglages Logo et Illustration. Le préréglage Photo passe encore par la dépendance VTracer, hors de portée des règles de `core`.

---

## 14. Implémentation Rust

### 14.1 Modules et dépendances

L'ordre des couches est num → raster → edges → topo → detect → guides → repair → trace → regular → svg, sans dépendance inverse. La topologie (postérisation) vient avant les détecteurs, car ceux-ci utilisent les jonctions topologiques ; la réparation F′ vient après les repères. Le rastériseur de couverture est dans `raster/`, et la boucle de vérification J (qui peut relancer un tracé) dans `trace/`, pour éviter qu'`svg/` ne dépende de `trace/`.

| Module (crate `core`) | Contenu |
|---|---|
| `num/` | Fonctions transcendantes propres (§14.4), erf, fonction de répartition normale inverse ; sélection déterministe (quickselect à pivot fixe) pour les médianes ; matrices de taille fixe ; valeurs propres 2×2 fermées, 3×3 symétriques (Jacobi), 3×3 non symétriques (cubique + itération inverse) ; Cholesky et LDLᵀ (bande ou non) ; Householder QR ; `lm` générique ; racines stables ; ln Γ (Lanczos) ; queue binomiale en log (récurrence sur le rapport des termes, sans jamais calculer 1 − queue) ; FFT ; union-find ; PCG32 ; tri total. Chaque solveur entre dans la phase qui l'utilise en premier, avec ses tests. |
| `raster/` | `Plane<T>`, Oklab et jacobien, filtres gaussiens et dérivées, bandes, pyramide, modèle de bruit, classification d'entrée ; **rastériseur de couverture** (droites, arcs et Béziers aplatis, couverture analytique par pixel, composition forme par forme) pour la vérification J, la mesure des coutures et les tests |
| `edges/` | tenseur généralisé, NMS, Devernay, chaînage, hystérésis, analyse par point, surface partielle, erf, Steger |
| `detect/` | segments, Förstner, jonctions, congés, arcs, cercles, ellipses, regroupement, taches, NFA |
| `guides/` | arène de repères, fusion, statuts, migration des éditions ; le type exporté reste dans `guide.rs` |
| `topo/` | postérisation, zones, frontières partagées (extrait de `planar.rs`) |
| `repair/` | réparation F′ par surcouches réversibles |
| `trace/` | association, programmation dynamique, Béziers, ajustement conjoint |
| `regular/` | relations, résolution par composantes, répétitions (reprend `shape.rs`, `repeat.rs`, `align.rs`) |
| `svg/` | émission, anti-couture, `<use>` (extrait de `planar.rs`) |
| `calib/` | tables de calibration (Devernay, c(φ), Steger, κ_c, σ_sys, coin flou) **générées** par un outil du dépôt à graine fixe, et commitées comme code Rust ; l'outil est relancé quand un localisateur change. Comme les localisateurs utilisent eux-mêmes ces tables, l'outil les fait tourner en mode « tables identité » (aucune correction) pour éviter la circularité |

### 14.2 Budget de temps

Image de 1920×1080 (2 MP), WASM sur un seul thread. Les débits scalaires de référence sont de 0,5 à 1,5 milliard d'opérations simples par seconde ; le SIMD multiplie par 2 à 4 les filtres [R5 §3.1].

| Étape | Opérations par pixel (ordre) | Total | Scalaire | SIMD |
|---|---|---|---|---|
| Oklab + jacobien + bruit | ~75 | 0,15 G | 100–300 ms | 60–150 ms |
| Gaussienne σ₀ (9 coeff. × 2 passes × 3 canaux), différences, tenseur, NMS | ~100 | 0,2 G | 130–400 ms | 50–150 ms |
| Pyramide (⅓ du précédent) + Steger et taches sur L, dont Steger au niveau 0 pour les traits fins (hessienne par différences à 3 points sur L lissé) | ~85 | 0,17 G | 110–340 ms | 50–140 ms |
| Analyse par point (5 à 30 % des pixels × ~300 op.) | — | 0,03–0,2 G | 20–400 ms | 20–300 ms |
| Chaînage, détecteurs D | par point | ~0,05 G | 30–100 ms | 30–100 ms |
| Postérisation (existante) | mesurée | — | 80–200 ms | 80–200 ms |
| Association, G–J (détail ci-dessous) | par point de frontière | ~0,3 G | 200–600 ms | 200–600 ms |
| **Total** (hors Nettoyage) | | | **≈ 0,7–2,3 s** | **≈ 0,6–1,6 s** |

Détail de G–J pour une image de 2 MP à N ≈ 10⁵ points de frontière (cas photo ; un logo en a 10 à 20 fois moins), soit N_c ≈ 10⁴ nœuds :

| Poste | Calcul | Total |
|---|---|---|
| Programmation dynamique, tri rapide | 10⁴ nœuds × 8 portées × (droite, cercle, repère, Bézier linéaire ≈ 1000 op.) | ~0,08 G |
| Programmation dynamique, coût exact à zone morte | 10⁴ nœuds × 8 portées × 2 modèles × ~64 points × ~10 op. | ~0,1 G |
| Ellipses | seulement sur les portées qui tournent ≥ 120° ou portent un repère | < 0,01 G |
| Schneider + SDM sur les portées retenues | 10⁵ points × 4 itérations × ~50 op. | ~0,02 G |
| Ajustement conjoint | 10⁵ points × largeur de bande² (≈ 36) × 10 itérations | ~0,04 G |
| Association et vérification J | bandes de pixels autour des frontières | ~0,05 G |

**Objectifs** (mode Équilibré, 2 MP, hors Nettoyage) : ≤ 1,2 s en SIMD et ≤ 2,5 s en scalaire. Pour relancer G–J après un déplacement de curseur (§14.6) : ≤ 300 ms sur le corpus Logo et Illustration, dont les frontières sont 10 à 20 fois plus courtes que celles d'une photo. Le débruitage bilatéral actuel (jusqu'à 7×7 avec exponentielles, de l'ordre de 500 opérations par pixel) coûte à lui seul environ 1 G opérations quand il est actif. Il faudra l'optimiser (approximation séparable, table d'exponentielles), ou ne l'activer par défaut que si le bruit mesuré le justifie. Ces chiffres sont des estimations, remplacées par des mesures dès la phase P1.

### 14.3 Bornes explicites

Chaque étape par paires ou par programmation dynamique a un plafond. Au-delà, elle se dégrade proprement et le compte.

| Étape | Borne |
|---|---|
| Programmation dynamique de segmentation | nœuds = ruptures candidates (≥ 1 tous les 16 points) ; ≤ 8 intervalles par portée ; fenêtre W = 200–400 points ; ≤ 2 départs |
| Chaînage | tri par paquets, linéaire |
| Fusion de segments, regroupement d'arcs, fusion multi-échelle | grille spatiale + cases d'angle ; ≤ 16 partenaires par élément ; ≤ 10⁶ tests au total |
| Association | programmation dynamique en bande |
| Relations | ≤ 16 partenaires par primitive ; résolution par composante ≤ 200 inconnues |
| Symétries et répétitions entre zones | seaux de tailles et de signatures ; ≤ 10⁵ paires vérifiées |
| Travail par frontière | budget en évaluations de coût ; repli sur Béziers seuls (§10.1) |
| Ajustement conjoint partagé | composante ≤ 50 frontières et 200 paramètres partagés, sinon alternance (§10.5) |

### 14.4 Déterminisme

- **Fonctions transcendantes** (exp, ln, ln1p, atan2, sin, cos, cbrt, pow, hypot, ln Γ) : `num/` fournit ses propres versions en pur Rust. Elles sont portées des sources de la caisse `libm` (MIT, dérivée de musl et FreeBSD) et citées dans `THIRD_PARTY.md`. Les versions de `std` et `mul_add` sont interdites dans `core` par la règle clippy `disallowed-methods`, active dès la phase P0. Sinon, une différence d'un bit entre la bibliothèque mathématique native et celle du WASM ferait basculer des décisions discrètes : seuils, NFA, minimum de la programmation dynamique, ordre glouton.
- **SIMD** : les filtres sont écrits en boucles élément par élément (« axpy décalés », §18.1, R3). Rust ne réassocie pas les flottants et ne les contracte jamais en FMA, donc le compilateur peut vectoriser ces boucles sans changer un seul bit : il n'y a qu'un chemin de code, et l'activation de `simd128` ne change pas le résultat. Les intrinsèques explicites ne sont utilisées qu'en dernier recours, avec un test d'égalité au bit près contre la boucle simple. Pas de relaxed-SIMD.
- **Ordre** : chaque étape gloutonne ou triée utilise `total_cmp` + un indice pour départager. Les NaN sont exclus avant tout tri (assertion de débogage). Pas de `HashMap` de `std` dans `core` : tableaux triés, `BTreeMap` ou hachage fixe.
- **Bandes et tuiles** : sans effet sur le résultat. Le chaînage et l'hystérésis se font sur la liste globale.
- **Aléa** (k-means, RANSAC éventuel) : PCG32 à graine dérivée de l'image et des réglages.

### 14.5 Mémoire

Plan f32 de 2 MP = 8 Mo. La mémoire WASM ne rétrécit jamais : le pic est permanent pour l'onglet.

| Poste | Taille à 2 MP |
|---|---|
| Image source + Oklab (3 plans) + Oklab lissé à σ₀ (3 plans, §5.1) | ~58 Mo |
| Bande de calcul σ₀ (128 lignes, ~10 plans) | ~10 Mo |
| Pyramide (3 canaux, ⅓) | ~8 Mo |
| Points de bord (~50 o × 0,1–0,6 M) | 5–30 Mo |
| Postérisation (étiquettes, composantes, copie débruitée) | ~40 Mo |
| G–J (points de frontière, sommes préfixes, matrices à bande, copie de topologie) | 30–40 Mo |
| **Objectif de pic** | **≤ 200 Mo à 2 MP, ≤ 300 Mo à 4 MP, ≤ 500 Mo à 8 MP (Précis)** |

Ces chiffres sont mesurés dès la phase P2 (§13.5). Les plafonds de résolution par mode (§12.5) bornent le cas des très grandes images.

### 14.6 Cache des étapes

Le module WASM expose un objet `Analysis` qui conserve les étapes A à F″. Sa **clé** est explicite : empreinte de l'image, Nettoyage, Détail, palette (nombre de couleurs et éditions), empilement et mode de qualité. Tout ce qui dépend d'autres réglages est hors du cache :
- les fusions de petites zones (§3.5), qui dépendent d'Échelle, se font dans G–J sur une copie de la topologie ;
- la réparation F′ est acceptée par un critère indépendant des curseurs (§8.2) et stockée en surcouches par repère. Désactiver ou forcer un repère n'applique ou ne retire que sa surcouche.

Un changement de Géométrie, Échelle ou Lissage, ou une édition de coin ou de repère, ne relance que G–J. Sans ce cache, l'interaction serait plus lente qu'aujourd'hui. Le cache est conçu dès la phase P5a, pour que P6 ne le casse pas. Le worker libère explicitement l'objet `Analysis` précédent (`free()`) quand l'image change, puisque la mémoire WASM ne se rend pas.

### 14.7 Robustesse numérique

- Normalisation de Hartley avant toute conique ; dénormalisation après.
- Valeurs propres 2×2 : forme fermée avec le vecteur propre stable (choisir le couple de plus grande norme), sans division quand Sxx ≈ Syy et Sxy ≈ 0.
- Racines de polynômes sous forme stable, q = −½(b + signe(b)√Δ).
- Garde sur chaque division par un dénominateur pouvant s'annuler (Devernay, Schneider, Halíř–Flusser), avec un repli documenté.
- Tests des solveurs : reconstruction A = VΛVᵀ, ‖VᵀV − I‖ < 10⁻¹², matrices aléatoires à graine fixe.

---

## 15. Licences et sources

Règle : on lit et on peut traduire le code **permissif**, en citant la source dans `THIRD_PARTY.md` et en tête du fichier. Pour le **copyleft** et le code sans licence, on implémente uniquement depuis l'article, sans avoir le code ouvert, et on cite l'article en commentaire [R5 §2].

| Module | Sources lisibles (permissives) | Article seulement |
|---|---|---|
| Fonctions mathématiques | `libm` (MIT, dérivé de musl) | — |
| Filtres, pyramide | imageproc (MIT), OpenCV (Apache-2.0), VLFeat (BSD-2) | Young–van Vliet |
| Bords sous-pixel | Canny d'OpenCV et d'imageproc ; surface partielle : `subpixel-edges` (MIT) | Devernay 1995, IPOL 2017 (code AGPL) |
| Traits fins | hessienne de scikit-image (BSD-3) | Steger 1998 (code ImageJ GPL-2) |
| Segments | `lsd.cpp` d'OpenCV (réécriture permissive), ED_Lib/EDLines (MIT), ELSED (Apache-2.0) | LSD IPOL (code AGPL) |
| Coins | Förstner de scikit-image (BSD-3), cornerSubPix d'OpenCV | Förstner & Gülch 1987 |
| Ellipses | EDCircles (MIT), fitEllipseDirect d'OpenCV, EllipseModel de scikit-image | ELSDc (AGPL), AAMED (GPL-2), Fornaciari (sans licence), Kanatani (conditions non précisées) |
| Taches | blob_log/doh de scikit-image, `mser.c` de VLFeat | — |
| Tracé, postérisation | VTracer et visioncortex (MIT), ImageTracer.js (Unlicense) | Potrace 2003 (GPL) |
| Béziers, arcs | curve-fit-nd (BSD-3), kurbo (MIT/Apache), lyon_geom (MIT/Apache), FitCurves.c (Graphics Gems) | — |
| Régularisation | — | GlobFit 2011 (sans licence), Hoshyari 2018, régularisation CGAL (GPL-3) |

**Pièges relevés** [R5 §1–2] :
- `pytlsd` : dépôt sous MIT, mais il inclut `lsd.cpp` sous AGPL ;
- `h3ct0r/fast_ellipse_detector` : étiquette MIT sur du code sans licence ;
- `ridge-detector` : sous MIT, mais dérivé de code GPL-2.

On lit donc toujours les en-têtes de fichier. Aucun brevet actif n'est connu sur les méthodes retenues. Celui de Microsoft sur Kopf-Lischinski (pixel art) est à vérifier avant un mode pixel art.

---

## 16. Plan de construction

Les pistes du §18 sont rattachées à leur phase. Chaque phase se termine par :
- ses tests synthétiques (§13.1) et la batterie de défauts (§13.2), même partielle ;
- le benchmark de non-régression ;
- les chronomètres et la sonde mémoire ;
- `scripts/check.sh` vert.

Les phases P3 et suivantes améliorent le pipeline existant pièce par pièce, au lieu de construire un second pipeline complet à côté.

| Phase | Contenu | Critère de sortie |
|---|---|---|
| P0 | `Plane`, conversions, fonctions transcendantes + règle clippy, PCG32, tri total, générateur de scènes, réglages de compilation et règles de largeur (R10), filtre bilatéral par tables (R8) ; nouvelle référence `baseline.json` enregistrée après le passage aux fonctions transcendantes propres (les derniers bits changent) | tests unitaires ; règle clippy active |
| P1 | Oklab et jacobien, modèle de bruit, classification d'entrée, gaussiennes par bandes, pyramide, chronomètres | σ_n à 10 % ; filtres conformes à la référence analytique ; temps mesurés |
| P2 | Bords sous-pixel, chaînage, analyse par point, variance, Steger ; calibration de σ_sys ; sorties de débogage | cibles C ; mémoire mesurée |
| P3 | Association et statut bord réel / bord de postérisation, **branchés dans le pipeline actuel** à la place de `edges.rs` (SOFT_EDGE) ; rastériseur de couverture et mesure des coutures | cas « dégradés » de la batterie : aucune géométrie de l'ancien ajusteur sur les bandes ; aucune régression ΔE |
| P4 | Segments, coins, congés, arcs, cercles, ellipses, taches ; arène de repères ; tous affichés comme Candidats | cibles D, dont les fausses détections |
| P5a | Critère §3 (zone morte comprise), programmation dynamique (droites, arcs, « sur repère », Béziers linéaires puis Schneider), statuts Suivi/Candidat dans l'interface, migration des éditions, conception du cache `Analysis` | cas « lignes interrompues », « ellipse non suivie » et « petit détail » (encoche à 0,5 τ_perc sur forme nette) ; ΔE ≤ référence ; migration ≥ 95 % |
| P5b | Ellipses dans la programmation dynamique, SDM, ajustement conjoint G1 (Schur, composantes partagées), pontage | cibles G ; relance G–J ≤ 300 ms |
| P6a | Régularisation globale, répétitions `<use>` | cibles I ; cas « symétrie » et « simplification progressive » ; taille du SVG en baisse |
| P6b | Réparation de topologie F′ (surcouches), fusion des petites zones | cibles F′ ; cas « petit élément voisin » |
| P7 | Seuils locaux, réglage Qualité/temps, SIMD, optimisation du débruitage | 2 MP ≤ 1,2 s en SIMD (Équilibré) |
| P8 | Retrait de l'ancien code et des constantes remplacées ; évaluation sur photos (préréglage Photo) ; comparaison avec vectorizer.ai | aucune constante non étiquetée ; décision sur le préréglage Photo |

---

## 17. Risques et questions ouvertes

1. **Modèle de fond des NFA sur des images plates** : sur des aplats, de la postérisation ou des blocs JPEG, les orientations ne sont pas indépendantes et uniformes. Des alignements de blocs peuvent paraître significatifs. Parades : calibration de ε sur ces images, distribution empirique, plancher perceptif. Cible : ≤ 1 fausse détection par MP (§13.1).
2. **Espace de localisation** (§5.5, point 4) : hypothèse non publiée, à confirmer sur vérité terrain.
3. **Formule de variance** (§5.6) : algèbre vérifiée, mais le modèle (marche droite, bruit blanc) reste à confronter à Kakarala & Hero (1992) et à la mesure. σ_sys absorbe l'écart.
4. **Analyse par profil 1D** (§5.1) : écart du premier ordre (≈ κσ_loc²/2) avec un lissage 2D sur les bords courbes, corrigé par table ; à mesurer sur les petits cercles.
5. **Loi de complexité locale** (§12.2) : sans référence, à calibrer.
6. **Coût sur les photos** : bornes de §14.3 et repli Béziers ; le préréglage Photo n'est basculé qu'après mesure (P8).
7. **Réparation de topologie** (§8.2) : la plus nouvelle des étapes, sans référence directe dans la littérature. Le critère d'acceptation par ΔE est simple ; ses effets de bord sur les frontières voisines sont à surveiller.
8. **Coutures** : la stratégie actuelle (traits de comblement) reste nécessaire. Sa mesure (§13.2) dira si le mode empilé doit devenir le défaut.
9. **Dégradés** : le statut « bord de postérisation » empêche les fausses géométries ; le rendu des dégradés en SVG reste une étape distincte.
10. **Réduction au-delà du plafond** (§12.5) : décision prise, tout le pipeline tourne sur l'image réduite. Reste à fixer les plafonds par plateforme (web et bureau) à l'aide du test du §13.5.
11. **Coin vif flou** (§6.3) : le modèle du lieu observé d'un coin flou est tabulé par simulation ; sa validité sur des flous non gaussiens (JPEG, mise à l'échelle) reste à mesurer.
12. **Zone morte** (§3.5) : elle rend gratuits les écarts sous τ_perc. Le budget de couleur par forme et la protection des repères significatifs limitent les abus ; leur réglage par défaut se calibre sur le corpus.

---

## 18. Pistes de la relecture finale

La relecture finale (agent Fable) a proposé 22 pistes, détaillées dans [`docs/recherche/06-pistes-amelioration.md`](recherche/06-pistes-amelioration.md). Elles se répartissent en deux groupes.

### 18.1 Optimisations adoptées

Elles ne changent pas la méthode. Certaines changent les derniers bits d'un calcul (ordre des opérations, table au lieu d'une fonction) : elles sont alors adoptées **comme définition** du calcul, et le résultat reste identique entre natif et WASM.

| Piste | Contenu | Remplace ou précise | Phase |
|---|---|---|---|
| R1 | NMS sans angle ni seconde racine : axe dominant ⇔ g11 ≥ g22 ; comparaison sur λ+ ; rejet précoce si g11 + g22 < L ; normale calculée aux seuls maxima | §5.3 | P2 |
| R2 | Carte d'activité par tuiles 8×8 (min et max par canal sur les octets source) : une tuile dont la borne λ+ ≤ 2 Σ_c (étendue_c · ‖k′‖₁ · ‖k‖₁)² est sous L est sautée ; le saut est exact | §5.1 | P2 |
| R3 | Filtres séparables en boucles élément par élément (« axpy décalés »), tampon circulaire de 2r + 1 lignes ; forme symétrique k_j·(x₋ⱼ + x₊ⱼ) | §5.1, §14.4, §14.5 (la bande de 10 Mo devient quelques lignes) | P1 |
| R4 | Pyramide : flou et moyenne 2×2 en une seule passe au pas de 2 | §2, §5.1 | P1 |
| R6 | Moments par bloc entre deux nœuds, centrés sur leur barycentre, accumulés par translation : plus aucune différence de grandes sommes | §10.1 (remplace les sommes préfixes par blocs) | P5a |
| R7 | Mémo des coûts de portée par frontière, conservé entre deux relances de G–J ; les résidus ne dépendent ni de Géométrie ni de Lissage ; invalidation par frontière « sale » | §14.6 | P5a |
| R8 | Filtre bilatéral existant : poids par table, intérieur sans test de bornes, tuiles plates sautées, encodage sRGB par table de seuils sans `powf` | §14.2 | P0 |
| R9 | Tableaux plats (CSR) au lieu de `Vec<Vec<_>>`, points de bord permutés dans l'ordre des chaînes, tampons `Scratch` réutilisés, comptage avant allocation, solveurs à taille fixe, index de lignes pour le chaînage | §14.1 | au fil des phases |
| R10 | Compilation WASM `+simd128`, `+bulk-memory`, `+nontrapping-fptoint`, `+sign-ext` ; `u32`/`u64` explicites partout où la largeur de `usize` compte ; jamais de tri sur une clé non unique ; cibles natives x86-64 et AArch64 seulement | §14.4 | P0 |
| R11 | Cercle de Pratt/Taubin par Newton sur le polynôme caractéristique (Chernov) ; seuils χ² et quantiles normaux mis en table une fois | §10.1, §5.4 | P5a, P2 |

### 18.2 Alternatives de méthode, à trancher par la mesure

Chacune est comparée à la méthode du document, dans la phase indiquée, sur la vérité terrain du §13.1 et la batterie du §13.2. Elle est adoptée si elle atteint au moins les mêmes cibles pour un coût moindre, ou de meilleures cibles pour un coût comparable ; sinon la méthode du document reste en place. Le résultat de chaque comparaison est noté ici.

| Piste | Alternative | Méthode actuelle | Test de décision | Phase |
|---|---|---|---|---|
| M1 | Détection dans un espace à variance stabilisée : S_c(v) = Σ_{u<v} 1/σ_c(u), table de 256 valeurs par canal ; bruit estimé en entiers. Le tenseur blanchi se réduit au premier ordre à ∇Iᵀ D⁻¹ ∇I, sans Oklab ni jacobien | Oklab + jacobien + blanchiment par pixel (§4.3, §5.2) | cibles C et fausses détections ; temps et mémoire. **P1** : la moitié « bruit estimé sur les valeurs codées, courbe libre par classe » est adoptée (§4.3) ; l'espace de détection reste à trancher en P2 | P1–P2 |
| M2 | Facteur d'échelle des seuils mesuré sur la statistique seuillée trace(G), par niveau de pyramide, à partir de deux quantiles bas, avec contrôle de concordance | seuils déduits du bruit blanc (§5.4) | fausses détections par MP sur JPEG, agrandissements, bruit corrélé | P1–P2 |
| M3 | Loi fermée de la plus grande valeur propre, P(λ+/σ_g² > x) = x e^{−x/2} + e^{−x} pour C = 3 (formule dérivée par l'agent) | table simulée (§5.4) | test de Monte-Carlo de la formule ; à refaire pour C = 2 et 4 | P2 |
| M4 | Devernay sur ln λ+, exact pour une marche gaussienne, qui donne aussi le flou par point à une seule échelle | parabole sur λ+ et table de biais ; flou à deux échelles (§5.3, §5.5) | erreur de position et de σ_b ; repli à faible SNR | P2 |
| R5 | Profils lus le long de l'axe dominant, sans interpolation | profils interpolés le long de la normale (§5.1) | erreur sur les petits cercles ; coût par point | P2 |
| M5 | κ_c(n) exact sur une portée finie, 1 + 2 Σ (1 − k/n) ρ_k | valeur asymptotique (§3.2) | cas « formes fines » et « petit détail » | P5a |
| M6 | Bornes de Jensen W·max(0, √(J/W) − τ)² ≤ J_zone morte ≤ J_mco, pour une programmation dynamique exacte | tri à variance gonflée + deux meilleurs modèles (§10.1) ; bornes dérivées par l'agent | nombre d'évaluations exactes et résultat, à Échelle faible et forte | P5a |
| M7 | Terme d'Occam unique (évidence de Laplace) pour modèles libres et « sur repère » | λ·c(m) et min(λc, ln det …) (§3.3, §10.2) | batterie complète ; recalibration de Géométrie | P5b |
| M9 | Distance de Sampson partout, Eberly en vérification et pour les ellipses très aplaties | Eberly dans le raffinement (§6.4) | précision sur ellipses ; coût | P5b |
| M8 | A priori de type tirés de la fréquence des primitives dans le corpus d'aller-retour | valeurs de goût (§3.3) | représentativité du corpus pour les logos | P6a–P8 |
| M10 | Coûts N_c et n_type calculés localement, avant toute loi de complexité locale | λ_local = λ(1 + log(1 + κ_loc)) (§12.2) | cas « adaptation locale » | P7 |
| M11 | Force du débruitage indexée sur le bruit mesuré, automatique en JPEG | constantes du filtre bilatéral actuel | cas « artefacts JPEG » ; table de conversion du réglage | P7 |

---

## Annexe A — Glossaire

- **a contrario, NFA (nombre de fausses alarmes)** : on déclare une structure réelle si, dans une image de pur bruit de même taille, on s'attendrait à en voir moins d'une par hasard. NFA = nombre de structures testées × probabilité d'en voir une aussi nette par hasard.
- **χ² (khi-deux)** : somme de carrés d'écarts normalisés ; sert à mesurer si des résidus sont compatibles avec le bruit attendu.
- **MDL (longueur de description minimale)** : on préfère le modèle qui décrit les données avec le moins d'information au total, données résiduelles comprises. Un paramètre de plus doit « se payer » par une réduction suffisante des résidus.
- **G-AIC, G-MDL (Kanatani)** : versions « géométriques » de ces critères, adaptées à l'ajustement de courbes avec peu de bruit et beaucoup de points ; elles fixent le prix λ d'un paramètre.
- **Di Zenzo (tenseur de)** : façon de calculer le gradient d'une image en couleur en combinant les canaux, avec une direction et une force uniques.
- **NMS (suppression des non-maxima)** : on ne garde, en travers d'un bord, que le pixel où le gradient est maximal.
- **Devernay** : position sous-pixel du maximum par une parabole sur trois valeurs.
- **Sélection d'échelle (Lindeberg)** : on cherche l'échelle de lissage à laquelle une structure répond le plus fort ; elle révèle sa taille ou son flou.
- **Pyramide** : copies de l'image de plus en plus petites (÷2 à chaque niveau), pour détecter vite les grandes structures.
- **Steger** : détecteur de traits fins (crêtes) qui donne la ligne centrale et la largeur.
- **Förstner** : position précise d'un coin, comme le point le plus proche de toutes les droites tangentes aux bords voisins.
- **Programmation dynamique** : méthode qui trouve le meilleur découpage d'une courbe en morceaux en réutilisant les meilleurs découpages de ses débuts.
- **PELT** : règle d'élagage qui accélère cette programmation dynamique.
- **G1** : deux morceaux se raccordent avec la même tangente (pas d'angle visible).
- **Levenberg-Marquardt (LM)** : méthode itérative d'ajustement de paramètres par moindres carrés.
- **KKT, complément de Schur** : manière de résoudre un ajustement par moindres carrés sous contraintes (par exemple « cette droite reste tangente à ce cercle »).
- **Union-find** : structure qui regroupe des éléments en classes (« toutes ces droites sont parallèles »).
- **Šidák** : correction d'un seuil quand on teste beaucoup de points à la fois, pour ne pas rejeter à tort une bonne droite parce qu'un de ses 500 points s'écarte un peu par hasard.
- **Mahalanobis (distance de)** : distance entre deux estimations, mesurée en unités de leur incertitude.
- **Oklab** : espace de couleur où la distance entre deux couleurs correspond bien à la différence perçue.
- **Arêtes inter-pixels (crack edges)** : les côtés des pixels ; les contours des zones passent par eux avant d'être lissés.
- **κ_c (facteur de corrélation)** : nombre de points voisins qui partagent en pratique le même bruit ; on divise par lui pour ne pas compter plusieurs fois la même information.
- **σ_sys (erreur systématique)** : précision réelle du localisateur, mesurée sur des images de synthèse ; elle empêche de croire les points plus précis qu'ils ne sont.
- **τ_perc (échelle de perception)** : taille sous laquelle un écart est jugé invisible pour une forme donnée ; fixée par le réglage Échelle.
- **Zone morte** : perte nulle tant que l'écart reste sous τ_perc, puis croissante au-delà.

## Annexe B — Exemple complet

*Un logo : un cercle orange coupé en trois par un mot blanc, sur un fond en dégradé bleu, enregistré en JPEG qualité 70, 1200×800 px.*

1. **A** : JPEG détecté. La fonction de niveau de bruit donne un bruit plus fort dans le bleu sombre. Le plancher de bruit est relevé pour le JPEG, et la localisation se fera sur L seul.
2. **C** : des chaînes de bords sont trouvées sur le contour du cercle (forte force, flou σ_b ≈ 0,6 px), sur les lettres, et presque rien dans le dégradé du fond : le gradient y est faible et partout sous le seuil H.
3. **D** :
   - les trois arcs du cercle (environ 100° chacun) sont regroupés, car leurs centres sont compatibles, et donnent un repère Cercle de NFA très faible ;
   - les fûts des lettres donnent des segments ; leurs angles, des coins vifs ;
   - les jambages arrondis donnent des arcs.
4. **F** : la postérisation produit le fond en 4 bandes de bleu, le cercle en 3 morceaux, et les lettres.
5. **F″** :
   - les frontières entre bandes bleues ne trouvent aucun point de bord apparié : ce sont des **bords de postérisation**, tracés en Béziers lisses, sans aucun repère ;
   - le contour du cercle est un **bord réel**, apparié tout du long ; le test de contraste se fait sur les couleurs locales, ce qui fonctionne malgré le dégradé derrière.
6. **G** :
   - sur chacun des trois morceaux du contour du cercle, le modèle « sur repère » (0 paramètre, coût = écarts au cercle global) bat l'arc libre et la Bézier ;
   - les lettres se découpent en droites et arcs, avec des coins vifs aux empattements ;
   - une petite bavure JPEG de 1 px sur le cercle est absorbée : elle reste sous τ_perc (Échelle = 0,5 donne ρ_éch = 0,005 ; boîte du cercle de 400 px, diagonale ≈ 566 px, donc τ_perc ≈ 2,8 px) ;
   - le point du « i », posé à 2 px du cercle, est un repère Tache significatif : il n'est ni ponté ni fusionné.
7. **H** : les trois morceaux partagent les mêmes (C, r) ; les lettres sont réajustées avec des raccords G1 entre droites et arcs.
8. **I** :
   - les fûts verticaux des lettres sont mis exactement à la verticale ;
   - les lettres de même hauteur partagent leurs droites de pied et de tête (colinéarité, même interrompue entre les lettres) ;
   - les deux « o » identiques deviennent un tracé et un `<use>`.
9. **J** :
   - vérification : écart de couleur sous ΔE_min, sauf la bavure tolérée, qui relève du budget de couleur du cercle ;
   - émission : un vrai cercle en arcs SVG (`A`), des lettres en `L` et `A`, un fond en Béziers ;
   - les traits de comblement suppriment les coutures.

Dans l'interface, on voit le repère Cercle **suivi**, les droites des lettres **suivies**, et aucun repère sur le dégradé. Si l'utilisateur désactive le cercle, les trois morceaux redeviennent des arcs libres ou des Béziers.

## Annexe C — Références principales

**Bords** : Canny 1986 ; Di Zenzo 1986 ; Devernay 1995 ; Grompone von Gioi & Randall, IPOL 2017 ; Trujillo-Pino et al. 2013 ; Lindeberg, IJCV 30(2) 1998 (deux articles) ; Immerkær 1996 ; Steger, PAMI 20(2) 1998 ; Kakarala & Hero, PAMI 14(7) 1992 ; Verbeek & van Vliet 1994.

**Segments et a contrario** : Desolneux, Moisan & Morel 2000, 2001, 2008 ; Grompone von Gioi et al. (LSD), PAMI 2010 et IPOL 2012 ; Akinlar & Topal (EDLines 2011, EDPF 2012, EDCircles 2013) ; Fernandes & Oliveira (KHT) 2008.

**Coins** : Harris & Stephens 1988 ; Förstner & Gülch 1987 ; Xia, Delon & Gousseau (ACJ) 2014 ; Mokhtarian & Suomela 1998 ; He & Yung 2008 ; Lachaud, Vialard & de Vieilleville 2007 ; Kerautret & Lachaud 2012.

**Coniques** : Kåsa 1976 ; Pratt 1987 ; Taubin 1991 ; Al-Sharadqah & Chernov 2009 ; Chernov & Lesort 2005 ; Fitzgibbon, Pilu & Fisher 1999 ; Halíř & Flusser 1998 ; Kanatani et al. 2011, 2012 ; Eberly (distance point–ellipse) ; Fornaciari, Prati & Cucchiara 2014 ; Pătrăucean et al. (ELSDc) 2017.

**Sélection de modèles et changements de régime** : Kanatani, IJCV 26(3) 1998 et PAMI 26(10) 2004 ; Rosin & West, PAMI 17(12) 1995 ; Killick, Fearnhead & Eckley (PELT) 2012.

**Courbes** : Schneider, Graphics Gems 1990 ; Wang, Pottmann & Liu, ACM TOG 2006 ; Taubin 1995 ; Selinger (Potrace) 2003 ; Hoshyari et al., SIGGRAPH 2018 ; Dominici et al. (PolyFit) 2020 ; Li et al. (GlobFit) 2011 ; Li et al. (Diffvg) 2020.

**Symétrie et répétition** : Hu 1962 ; Kuhl & Giardina 1982 ; Zabrodsky, Peleg & Avnir 1995 ; Loy & Eklundh 2006.

**Statistiques numériques** : Pébay 2008 (moments par combinaison de blocs).
