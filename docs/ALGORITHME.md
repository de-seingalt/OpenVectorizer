# Algorithme de vectorisation sur mesure — document de conception

**Version 2** : intègre la première tournée de relecture indépendante (rigueur mathématique, faisabilité Rust/WASM, couverture des objectifs et robustesse). Les synthèses de recherche qui alimentent ce document sont dans [`docs/recherche/`](recherche/) (en anglais) ; un renvoi `[R1 §2.3]` désigne la synthèse 01, section 2.3. Un glossaire (annexe A) et un exemple complet (annexe B) aident à la lecture.

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
- **Pyramide.** Le niveau o + 1 s'obtient par un flou gaussien puis un moyennage de blocs 2×2. Le centre du pixel i du niveau o est en x_SVG = 2^o (i + ½). Le moyennage ajoute une variance de 1/3 px² (boîte de 2 px fins) dont la sélection d'échelle tient compte. Une erreur de convention décalerait tous les bords grossiers de 0,25 à 0,5 px [R1 §4.4].
- **Couleur.** Détection dans **Oklab** (L ∈ [0, 1] ; ΔE ≈ distance euclidienne ; seuil de visibilité ≈ 0,02). Matrices dans [R1 §1.3]. Pour la palette, la couleur est **dé-prémultipliée** ; l'alpha est traité à part (§4.4).
- **Échelles.** σ en pixels de l'image d'origine ; t = σ² ; g_σ gaussienne normalisée.
- **Précision.** f32 pour les plans ; f64 pour les ajustements géométriques et toutes les accumulations [R5 §3.4].
- **Notation.** B(n, k, p) = Σ_{j≥k} C(n, j) p^j (1−p)^{n−j} (queue binomiale) ; NFA = nombre de tests × probabilité sous le modèle de fond ; z_q = quantile q de la loi normale.

---

## 3. Le critère de décision unique

Toutes les décisions des étapes D (choix vif/arrondi, cercle/ellipse), G (segmentation), H et I (relations) utilisent ce critère. Il est exprimé en unités de χ², c'est-à-dire −2 × log-vraisemblance.

### 3.1 Variance d'un point

Chaque point de frontière p_i porte une variance normale totale :

  v_i′ = var_n,i + σ_sys² + (τ_perc / 3)²

- **var_n,i** : variance due au bruit de l'image, calculée par point (§5.6).
- **σ_sys** : erreur systématique résiduelle du localisateur, **mesurée** sur la vérité terrain synthétique (§13.1) et tabulée par méthode de localisation et par flou σ_b. Ordre attendu : 0,02 à 0,05 px. Sans ce plancher, une image nette donnerait var_n ≈ (0,003 px)², et le moindre biais de quelques centièmes de pixel compterait pour des centaines d'unités de χ² : toutes les droites et tous les arcs seraient rejetés au profit de Béziers. Les biais **connus** (table de Devernay, biais de courbure κs²/2) sont soustraits de la position et ne sont pas ajoutés à la variance.
- **τ_perc** : échelle de perception (§3.5), qui exprime la tolérance voulue par l'utilisateur. Un écart de τ_perc compte pour environ 3 écarts-types.

### 3.2 Corrélation des points

Deux points voisins d'une chaîne partagent le bruit à travers le noyau de dérivation. L'autocorrélation le long du bord est ρ(Δ) = exp(−Δ²/(4σ_loc²)) ; le facteur d'inflation est

  κ_c = Σ_k ρ(k·Δs) ≈ 2√π · σ_loc / Δs  (≈ 3,5 à σ_loc = 1, Δs = 1 px)

où Δs est l'espacement moyen des points. Partout :

- résidu effectif **J_eff = (1/κ_c) Σ_i d_i² / v_i′** ;
- covariances des paramètres ajustés multipliées par κ_c ;
- nombre d'échantillons indépendants n_eff = n / κ_c.

Pour les localisateurs par fenêtre (surface partielle, erf, §5.5), κ_c est tabulé par simulation. Pour les points de repli issus de la couverture (§9), κ_c = 1.

### 3.3 Coût total

Pour une frontière découpée en morceaux k de modèles m_k, aux ruptures b :

  E = Σ_k [ J_eff,k + λ · c(m_k) + λ · π(m_k) ] + Σ_b [ λ_b + λ · δ_coin(b) ]

- **c(m)** : nombre de degrés de liberté de forme d'un morceau entre deux ruptures, déduction faite du glissement tangentiel des extrémités. Droite 2, arc de cercle 3, arc d'ellipse 5, Bézier libre 6, Bézier aux deux tangentes imposées 2, morceau « sur repère » 0 (§10.2).
- **π(m)** : a priori de simplicité, en unités de λ. Droite 0, arc 0,5, ellipse 1, Bézier 1,5 par défaut, réglé par Géométrie (§12.4) et calibré sur le corpus.
- **λ_b = 2 ln N_c** : coût de description de la position d'une rupture choisie parmi N_c candidates. Sans ce coût, les critères de type AIC sur-segmentent.
- **δ_coin(b)** = 1 si la rupture est un coin (tangentes indépendantes), 0 si la jonction est lisse (G1, une tangente partagée).
- **λ** : pénalité par paramètre, entre l'AIC géométrique de Kanatani (λ = 2) et le MDL géométrique (λ = 2 ln(L/σ̄), plus conservateur, qui préfère le modèle dégénéré) [R3 §3, R4 §1.3]. Ici L est la diagonale de la boîte englobante de la frontière et σ̄ = √médiane(v_i′). Exemple : L = 512 px, σ̄ = 0,03 px donne λ ≈ 19,5. Le réglage Géométrie interpole de 2 à λ_MDL.

### 3.4 Tests entre modèles emboîtés

Toute comparaison d'un modèle simple (k paramètres) à un modèle plus riche (k + Δk) se fait par

  ΔJ_eff = J_eff,simple − J_eff,riche  comparé à  λ · Δk (+ coûts de rupture et de choix)

- droite contre arc : Δk = 1 ;
- cercle contre ellipse : Δk = 2 ;
- coin vif contre arrondi : Δk = 1 ;
- jonction lisse contre coin : Δk = 1 ;
- relation entre primitives (§11) : Δk = q, degrés de liberté retirés, plus le coût de choix 2 ln N_rel.

Il n'existe pas d'autre règle (pas de « (A − B)/σ < 2 », pas de χ² à 99 % ici et à 95 % ailleurs). Les fenêtres de regroupement (2–4°, 3–5 %) servent seulement à générer des candidats.

**Écart maximal.** Quand une portée est rejetée parce qu'un point s'écarte trop, le seuil tient compte du nombre de points : τ_max,i = z_{1−α/(2 n_eff)} · √v_i′ (correction de Šidák, α = 10⁻²). Cela donne z ≈ 3,9 pour n_eff = 100 et 4,4 pour n_eff = 1000. Un seuil fixe de 3σ rejetterait à tort 42 % des droites justes de 200 points.

### 3.5 Préférer la forme simple : échelle de perception et pontage

C'est la règle « tricher et préférer la forme la plus simple en laissant de côté les détails trop petits ». Elle agit par deux mécanismes, l'un sur les écarts faibles et l'autre sur les détails plus marqués.

1. **Écarts sous la perception** : τ_perc = ρ_éch · D, où D est la taille de la forme (diagonale de la boîte englobante de la frontière ou du repère) et ρ_éch ∈ [0 ; 0,03] vient du réglage Échelle. L'ajout de (τ_perc/3)² à la variance (§3.1) rend presque gratuits les écarts inférieurs à τ_perc. Le modèle simple gagne alors, en segmentation comme en régularisation.
2. **Pontage des petits détails** : un détail plus marqué mais petit (encoche, bosse, petite excroissance) peut être enjambé. Le coût par point est tronqué : ρ(d_i) = min(d_i²/v_i′, z²), avec z = τ_max,i/√v_i′. Une série contiguë de points pontés est admise si l'aire entre la primitive et les points est ≤ (2 τ_perc)², et si la longueur pontée totale est ≤ β · longueur de la portée (β = 10 % × Échelle). Au-delà, le modèle est infaisable sur cette portée. Les points pontés sont marqués et l'écart de couleur qui en résulte est contrôlé à la vérification (§12.3, J).
3. **Petites zones** : une zone entièrement contenue dans la bande de tolérance d'une primitive (largeur τ_perc) est fusionnée avec sa voisine. Ce filtre remplace le filtre d'aire fixe pour les zones de détail.

Avec Échelle = 0, τ_perc = 0 et β = 0 : fidélité pure.

### 3.6 Rôle des NFA

Le nombre de fausses alarmes (approche a contrario de Desolneux, Moisan et Morel) répond à la question « cette structure existe-t-elle vraiment dans l'image, ou est-elle due au hasard ? ». Il sert à **retenir** un repère raster (NFA ≤ ε = 1). Il **n'entre pas** dans E :

- un NFA est une borne de probabilité, non bornée, pas une vraisemblance ;
- les points de bord qui ont produit le repère sont déjà dans J, donc les compter deux fois serait une erreur.

Un repère agit sur le tracé par ce qu'il **offre** à la segmentation : des ruptures candidates, un modèle candidat sur sa portée (« sur repère », §10.2), et un modèle imposé quand l'utilisateur le force.

---

## 4. Étape A — Préparation et modèle de bruit

### 4.1 Classification de l'entrée

Avant tout calcul :

- **JPEG** : énergie de |I ∗ N| (masque d'Immerkær, §4.3) aux positions de blocs 8×8 rapportée à l'énergie moyenne ; la détection est un drapeau.
- **Pixel art** : grille native détectée par autocorrélation du gradient, peu de couleurs, quasi aucun pixel de mélange. Première version : avertissement dans l'interface et traitement à la résolution native, sans flou, en polygones exacts sur les arêtes inter-pixels. Un vrai mode pixel art (à la Kopf-Lischinski) est une étape ultérieure, après vérification du brevet signalé [R5 §2].
- **Petite image** (côté < 128 px) : σ₀ = 0,7 avec le noyau gaussien discret de Lindeberg (le noyau échantillonné est faux sous 0,8), estimation de bruit globale. Taille minimale supportée : 16 px.
- **Transparence** : présence d'un alpha non constant (§4.4).

### 4.2 Conversion

sRGB 8 bits → linéaire → LMS → racine cubique → Oklab, en f32, un plan par canal (L, a, b). Les plans sont stockés séparément (structure de tableaux) pour les filtres séparables et le SIMD [R5 §3.4].

### 4.3 Modèle de bruit

Le bruit de l'image n'est ni uniforme ni indépendant d'un canal à l'autre dans Oklab. Les trois canaux mélangent les mêmes valeurs LMS, et les zones sombres sont environ deux fois plus bruitées après la racine cubique. On l'estime donc là où il est le plus simple, en sRGB, puis on le propage [revue math. B1].

1. **Fonction de niveau de bruit** par canal sRGB : σ²(I) = α·I + β. On calcule les réponses d'Immerkær r = I ∗ N, avec N = [[1, −2, 1], [−2, 4, −2], [1, −2, 1]] et Σ N² = 36, qui annule les structures lisses. On ne garde que les pixels dont le gradient est sous sa médiane, pour exclure les bords ; ce masquage ne biaise pas l'estimation. On les regroupe par intensité locale (16 classes) et on prend un écart-type robuste par classe : MAD/(0,6745 · 6). Puis moindres carrés pondérés pour (α, β) [R1 §5].
2. **Planchers** : β ≥ (0,5/255)² (sinon un aplat numérique donnerait un bruit nul et une sensibilité infinie) ; en JPEG, β ≥ (2/255)².
3. **Propagation par pixel** : Σ_n(x) = J(x) · diag(σ²(I_c(x))) · J(x)ᵀ, où J(x) est le jacobien 3×3 de la conversion sRGB → Oklab au pixel x. Les valeurs LMS sont bornées à ≥ 10⁻⁴ avant dérivation, car la racine cubique a une dérivée infinie en 0.
4. **Carte locale** : le même estimateur sur des tuiles (au moins 256 pixels plats par tuile, sinon valeur globale), interpolées bilinéairement. Elle capte les variations que la fonction de niveau ne décrit pas, comme le grain d'un scan.

### 4.4 Alpha

Si l'alpha n'est pas constant, il forme un canal à part, avec son propre bruit et un plancher de 0,5/255. Un canal constant est retiré du calcul, faute de quoi son poids serait infini. La frontière avec le transparent est le croisement à 50 % d'alpha. Ce bord prime sur le bord de couleur au même endroit : les pixels de bord semi-transparents prémultipliés créent sinon un faux liseré coloré. La zone transparente est une entrée « aucune » de la palette.

---

## 5. Étapes B et C — Bords sous-pixel en couleur

C'est le socle. Méthode : Canny/Devernay dans la variante de Grompone von Gioi et Randall (IPOL 2017, code AGPL, donc implémentée depuis l'article), appliquée à un gradient couleur de Di Zenzo généralisé [R1 §1–3, R5 §1.1].

### 5.1 Organisation du calcul (coût et mémoire)

Une pile complète de dérivées à plusieurs échelles en pleine résolution coûterait environ 3 milliards de multiplications-additions à 2 MP, soit plusieurs secondes en WASM [revue ingé. B1]. On organise donc le calcul ainsi :

- **Plans pleine résolution à une seule échelle σ₀** (≈ 1, de 0,7 à 1,5 selon le bruit), calculés **par bandes** de 64 à 256 lignes avec une marge égale au rayon du noyau + 2 (pour la NMS, Devernay et le chaînage 5×5). Chaque bande produit des **enregistrements de points de bord**, puis ses plans sont libérés.
- **Pyramide** (§2) pour les grandes échelles : chaque niveau coûte le quart du précédent. On y détecte les structures larges, floues ou peu contrastées : grandes lignes, grands cercles, taches, traits épais.
- **Analyse par point** : sélection d'échelle, flou, échelle de localisation, relocalisation finale et variance se calculent pour chaque point de bord sur un **profil 1D le long de sa normale**, échantillonné dans le plan σ₀. Les échelles plus grandes s'obtiennent en convoluant ce profil en 1D. Pour un bord droit, c'est exact. Pour un bord de courbure κ, l'erreur est d'ordre (κσ)², qu'on contrôle sur la vérité terrain.

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
- pour l'indépendance à l'ordre de parcours, liens triés par longueur quantifiée (tri par paquets, linéaire) puis par indice, et acceptés gloutonnement ;
- le chaînage se fait sur la liste globale des points, après les bandes, pour que le découpage en bandes ne change pas le résultat.

**Hystérésis** le long des chaînes : une chaîne survit si un point dépasse H ; les points sous L la coupent. Les extrémités près des jonctions (le gradient y est indéfini) gardent leur tangente pour l'étape D.

**Validation des chaînes** (optionnelle) : NFA = N_chaînes · H(μ)^{n_eff}, où μ est le gradient minimal le long de la chaîne et H(μ) = P(|∇| ≥ μ) la distribution empirique sur l'image. Les points d'une chaîne ont été sélectionnés par la NMS, donc le modèle de fond est optimiste : ε se calibre sur des images de bruit.

### 5.5 Analyse par point : flou, localisation finale

Sur le profil normal de chaque point (§5.1) :

1. **Flou σ_b**, par forme fermée sur deux échelles. Pour une marche de flou t₀, le gradient vérifie E²(t) ∝ h²/(t₀ + t). Avec deux échelles t₁ < t₂ et R = E²(t₁)/E²(t₂) :
   t₀ = (t₂ − R·t₁)/(R − 1).
   Cette formule reste valable pour t₀ < σ₀², alors que le maximum de Lindeberg (γ = ½) bute sur le bas de la pile. Elle est moyennée le long de la chaîne (n_eff) et corrigée des gains des noyaux discrets (table). Le maximum γ = ½ sert seulement de contrôle.
2. **Échelle de localisation** σ_loc = max(σ₀, σ_b/2). On ne localise jamais à une échelle grossière : sur un bord courbe, le lissage introduit un biais ≈ κs²/2 vers le centre de courbure (s² = σ_loc² + σ_b²), qu'on soustrait.
3. **Localisateur** :
   - si σ_b < 0,7 px et pas de JPEG, **effet de surface partielle** (Trujillo-Pino 2013 ; formules et port MIT dans [R1 §3.2]). Il donne une précision < 0,01 px, plus une courbure et un contraste par point, pour un coût négligeable par point ;
   - si le bord est flou, **ajustement d'une marche floue** I(x) = B + (A − B)·Φ((n·x − ρ)/s) par Gauss-Newton sur le profil, qui donne position, flou, couleurs et covariance ;
   - sinon, Devernay.
4. **Espace de localisation** : on détecte dans Oklab, mais la position finale se calcule sur les canaux sRGB gamma. C'est là que les moteurs SVG anti-crénèlent, et le croisement à 50 % reproduit alors le raster. C'est une hypothèse d'une synthèse, à valider sur vérité terrain avant d'en faire le défaut ; un interrupteur interne permet de comparer.
5. **JPEG** : le sous-échantillonnage de la chrominance (4:2:0) décale les bords de a et b jusqu'à 1 px. On localise alors sur L seul, et la chrominance ne sert qu'à la détection.

### 5.6 Variance par point

Pour l'estimateur de Devernay à l'échelle σ_loc, avec s² = σ_loc² + σ_b² [R1 §7.2 ; algèbre vérifiée par la revue] :

  var_n = (3/8) · (1/SNR²) · (s/σ_loc)⁶,  SNR² = hᵀ Σ_n⁻¹ h

où h est le vecteur de contraste. Pour la surface partielle et l'ajustement erf, la variance vient de la covariance de l'ajustement, σ² (JᵀJ)⁻¹, avec un κ_c propre à chaque méthode. S'y ajoute σ_sys (§3.1).

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

1. **Croissance le long d'une chaîne** : ajustement par moindres carrés totaux pondérés (w_i = 1/v_i′), puis extension tant que le point suivant reste sous τ_max (§3.4), et réajustement.
2. **Ajustement** : centroïde pondéré, matrice de dispersion, direction = vecteur propre de λmax. Covariances : var(φ) = κ_c / Σ w_i t_i² et var(décalage au centroïde) = κ_c / W, soit une incertitude en sablier [R2 §1.5].
3. **Validation a contrario** : NFA = N⁴ · B(n, k, 1/8), où k compte les points dont l'orientation est à moins de 22,5° de la normale. Comme LSD, les orientations utilisées ici viennent du masque 2×2, qui minimise la dépendance entre voisins [R2 §1.1]. La longueur minimale est alors ≈ 12 px à 512². ε se calibre sur des images de bruit et d'aplats JPEG.
4. **Grandes lignes interrompues** : ce cas (« lignes coupées par d'autres formes ») passe par la relation de colinéarité de l'étape I (§11), **pas** par la fusion de D. La fusion de D ne réunit que des fragments d'un même bord séparés par une lacune de détection : test de Mahalanobis sur (θ, ρ), les deux droites étant exprimées au même point de référence (entre les segments), avec Δθ ramené dans (−π, π]. Puis test du trou NFA(union) < NFA(s₁) · NFA(s₂), avec le nombre de paires testées explicitement compté [R2 §1.6].
5. **Hough** : seulement en secours, pour des alignements très lointains (règles, grilles), par vote à noyau elliptique tiré des covariances. L'idée vient de KHT ; on ne prend que l'article, le code étant GPL.

### 6.2 Coins et jonctions

- **Topologiques** (exacts) : coins de pixels où au moins 3 étiquettes de zones se rencontrent (étape F). L'ordre m est le nombre de frontières incidentes.
- **Géométriques** (raster) : nécessaires pour les coins en L, qui ne séparent que 2 zones et sont donc invisibles en topologie. On calcule l'opérateur de Förstner **localement**, sur des fenêtres autour des candidats (points à fort ρ, extrémités de chaînes). Sous le bruit, la trace de M suit une loi σ_g² · χ²_{2 n_w} avec n_w ≈ aire de la fenêtre / (4πσ²) ; le seuil s'en déduit. La rondeur doit vérifier q = 4 det M / (tr M)² > 0,5. Position par
  x̂ = (Σ w_i g_i g_iᵀ)⁻¹ Σ w_i g_i g_iᵀ x_i, covariance σ̂² M⁻¹ [R2 §3.2].
  Le même système avec les tangentes donne le **centre d'un arrondi**.
- **Position d'un coin** : intersection, pondérée par les covariances, des primitives ajustées sur chaque branche, en excluant les points à moins de 2σ_loc + σ_b du sommet (zone où l'anti-crénelage mélange les zones). Le maximum de Harris n'est jamais utilisé comme position : il est biaisé vers l'intérieur.
- **Typage** L, T, Y, flèche, X d'après les angles des branches [R2 §3.5].

### 6.3 Vif ou arrondi

**Géométrie du congé** [R2 §4.1] : sommet virtuel V, angle intérieur φ, bissectrice intérieure b, directions des branches u_k :

  C(r) = V + (r / sin(φ/2)) · b,  T_k = V + (r / tan(φ/2)) · u_k,  flèche h = r (1/sin(φ/2) − 1)

**Ajustement** : on minimise la distance des points à la courbe composite droite–arc–droite. Elle est continue en r, ce que ne serait pas une affectation des points qui change avec r. Ce n'est pas garanti unimodal : on encadre sur une grille de 16 valeurs, puis recherche dorée.

**Décision** : arrondi si ΔJ_eff > λ · 1 (§3.4). Il n'y a pas de rayon minimal arbitraire : grâce au plancher σ_sys, un « arrondi » de la taille du flou n'apporte pas assez pour payer son paramètre.

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

Deux repères de même type sont fusionnés si le test §3.4 l'accepte (Δk = nombre de paramètres). On garde le mieux déterminé et on réunit les supports. Un repère qui n'existe qu'aux échelles grossières garde sa grande incertitude : il oriente le tracé sans imposer de position fine.

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

Une opération est acceptée si elle réduit l'écart de couleur entre le rendu et la source (ΔE moyen sur la bande concernée) d'au moins ΔE_min, et si le critère §3 ne s'y oppose pas. Chaque opération modifie les étiquettes, puis les frontières sont recalculées localement. Cette étape vient en phase P6 (§16) : jusque-là, ces cas restent une limite connue.

### 8.3 Coutures

Les liserés clairs ou sombres entre deux formes qui partagent exactement la même frontière viennent du rendu : chaque forme est anti-crénelée séparément, puis composée, si bien que le pixel de bord n'est couvert qu'à environ 75 %. Émettre la frontière une seule fois **ne suffit pas**. On garde donc une stratégie anti-couture explicite :

- par défaut, le trait de comblement actuel (`fillGaps`) ;
- ou le mode empilé, où la forme du dessous déborde sous celle du dessus.

Une mesure la contrôle (§13.2) : rendu du SVG à 1× et 4×, et écart de couleur maximal le long des frontières partagées, comparé à la source.

---

## 9. Association frontière ↔ bords (F″)

C'est le cœur de l'hybridation : la topologie vient de la postérisation, la géométrie des bords raster.

### 9.1 Points de repli

Pour chaque frontière (zones A | B), la position par couverture mesurée entre les deux teintes est calculée comme aujourd'hui. Elle sert là où aucun bord n'est apparié. Ces points sont lissés par l'opérateur λ|μ de Taubin (λ = 0,33, μ = −0,34, sans rétrécissement), morceau par morceau et sans traverser les coins [R4 §3.1]. Leur variance est tabulée par simulation, et κ_c = 1.

### 9.2 Appariement

Un point de bord q est apparié à la frontière A | B si trois conditions sont réunies :

1. sa distance à la frontière est inférieure à 1 px + 3√var_n(q) + σ_b ;
2. sa normale est compatible avec celle de la frontière : écart < 3 écarts-types d'orientation, et même sens A → B ;
3. son contraste Δ(q) est compatible avec la **différence de couleur locale** mesurée de part et d'autre de la frontière, à cet endroit : moyenne des pixels source dans une bande de 2–3 px côté A et côté B. On n'utilise pas les couleurs moyennes de la palette : sur un logo rempli d'un dégradé, elles sont loin des couleurs locales, et le vrai contour serait déclassé à tort. La compatibilité se juge par un test de Mahalanobis entre Δ(q) et c_B,local − c_A,local, avec les covariances du bruit.

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

**Candidats de rupture** : coins et jonctions (D, F), extrêmes de courbure, extrémités de repères et changements de statut (§9.3). Les marches d'escalier d'une droite discrète ne sont jamais des coins : le critère des segments maximaux de Lachaud et al. les élimine [R4 §3.2].

**Bornes de coût** :
- fenêtre i ∈ [j − W, j), avec W de 200 à 400 points ;
- Béziers seulement entre candidats séparés de 16 au plus ;
- frontières fermées ouvertes au coin le plus sûr, avec au plus 2 points de départ.

**Coûts comparables entre modèles** :
- droite : λmin de la matrice de dispersion ;
- cercle : résidu algébrique sous la normalisation de Pratt (B² + C² − 4AD = 1), qui égale le résidu géométrique au premier ordre ;
- ellipse : résidu normalisé de Taubin θᵀMθ / θᵀNθ.

Ces trois coûts se calculent en O(1) par sommes préfixes de moments. Deux précautions numériques sont nécessaires :
- coordonnées normalisées par la boîte englobante de la frontière ;
- sommes préfixes **par blocs** de 64 à 256 points, chacun avec son origine locale, combinées par les formules de translation de moments (Pébay). Des moments bruts d'ordre 4 perdraient 6 à 10 chiffres significatifs sur les portées courtes, celles justement où la programmation dynamique tranche.

Un test compare le coût par préfixes au coût direct sur une portée éloignée de l'origine. Les Béziers n'ont pas de forme en O(1) : on les ajuste sur les seules portées admises.

**Élagage (PELT)** : on retire i des candidats si F[i] + coût(i, j) ≥ F[j]. Cette règle n'est valable que si le coût vérifie coût(i, k) ≥ coût(i, j) + coût(j, k). Ce n'est pas le cas avec les portées infaisables ni avec les coûts algébriques approchés ; dans ces cas, l'élagage est désactivé et la fenêtre W sert de borne.

**Repli** : si le budget de temps d'une frontière est dépassé, la frontière est tracée en Béziers seuls, et le cas est compté dans les statistiques.

### 10.2 Modèle « sur repère »

C'est la correction du défaut « repères d'ellipse non suivis ». Chaque repère attaché à une frontière (cercle, ellipse, droite, arc) propose sur sa portée un modèle dont **les paramètres sont ceux du repère**, ajustés globalement sur tous ses supports. Ce modèle a :

- 0 paramètre de forme libre, seulement les extrémités ;
- un coût J_eff = Σ d²(p_i, repère)/v_i′, la variance incluant l'incertitude du repère projetée sur la normale ;
- un a priori de simplicité nul ;
- aucune limite d'étendue.

Un cercle coupé en trois arcs de 40° par d'autres formes est donc proposé sur chacun des trois morceaux. Ses paramètres sont ensuite **partagés** à l'étape H : une seule variable pour toutes les frontières qui l'utilisent, par élimination (§11.3). Un repère forcé par l'utilisateur rend ce modèle obligatoire sur sa portée ; un repère désactivé ne le propose plus.

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
- Bézier–Bézier G1 : P2^A = J − β_A·d(φ), P1^B = J + β_B·d(φ) ;
- paramètres partagés avec un repère.

**Contraintes restantes**, comme la tangence droite–cercle en forme signée s(n·C − ρ) − r = 0, où s est fixé par le côté de la droite où se trouve le cercle. Elles sont traitées par complément de Schur : A = JᵀJ + μI est symétrique définie positive et à bande pour une frontière ouverte, donc on fait une factorisation de Cholesky à bande de A, puis on résout le petit système dense (C A⁻¹ Cᵀ) ν = c − C A⁻¹ Jᵀr, et enfin δ = −A⁻¹(Jᵀr + Cᵀν). La matrice KKT complète est indéfinie : une factorisation de Cholesky directe y échouerait. Pour une frontière fermée, le bloc de bouclage est traité par la formule de Woodbury.

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

Une relation qui retire q degrés de liberté est acceptée si

  ΔJ_eff + 2 ln N_rel < λ · q

où N_rel est le nombre de candidats générés pour ce type de relation. C'est le coût de description du choix, l'équivalent MDL du nombre de tests d'un NFA. Sans lui, parmi des centaines de paires, beaucoup de relations accidentelles seraient acceptées.

Les écarts inférieurs à l'échelle de perception sont déjà presque gratuits grâce à v′ (§3.1). Il n'y a donc **pas** de seconde condition sur l'écart maximal, qui contredirait la règle « un détail sous τ_perc n'empêche pas une symétrie ».

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
| ρ_éch, β | réglage Échelle | utilisateur |
| tuiles de bruit, bandes de calcul | 256 px plats min., 64–256 lignes | technique, sans effet sur le résultat |
| fenêtre W, plafonds de paires | 200–400, §14.3 | budget de temps |

Il reste des constantes. Chacune est étiquetée selon sa nature : image, théorie, probabilité, perception, calibration ou budget. Le protocole de calibration (§13.1) fixe celles qui sont marquées « calibration » ou « goût ». Aucune n'est un seuil en pixels qui dépendrait de la taille de l'image.

### 12.2 Adaptation locale

1. **Bruit local** : fonction de niveau + carte de tuiles (§4.3), d'où des seuils de détection locaux.
2. **Flou local** : σ_b par point (§5.5), d'où l'échelle de localisation et la variance.
3. **Complexité locale** κ_loc : longueur de chaînes par unité d'aire, normalisée par la médiane de l'image. Dans les zones chargées (texture, feuillage), λ_local = λ · (1 + log(1 + κ_loc)) : on simplifie davantage et on cherche moins de géométrie. Cette loi est une première proposition, à calibrer sur le corpus (§13).

### 12.3 Vérification (J)

Avant l'émission, chaque frontière est confrontée aux pixels :
- l'écart de couleur entre le rendu local et la source, sur la bande de la frontière, doit rester sous ΔE_min, hors des parties pontées volontairement (§3.5) ;
- l'écart géométrique doit rester sous τ_max.

En cas d'échec, la frontière est retracée avec λ abaissé. C'est la généralisation de la vérification actuelle (`Prepared::deviation`).

### 12.4 Réglages utilisateur

| Réglage | Effet | Correspondance avec l'ancien réglage |
|---|---|---|
| **Géométrie** (0–1) | λ de 2 à λ_MDL ; poids de l'a priori de type | même sens ; table de conversion pour les réglages enregistrés et le benchmark |
| **Échelle** (0–1) | ρ_éch ∈ [0 ; 0,03] et β ∈ [0 ; 10 %] (§3.5) | remplace « relative = 0,02 × échelle » |
| **Détail** | ΔE_min et aire minimale | inchangé dans son sens |
| **Nettoyage** | débruitage bilatéral ; relève le plancher de bruit | inchangé |
| **Lissage** | a priori des Béziers, lissage Taubin des points de repli | inchangé dans son sens |
| **Qualité/temps** (nouveau) | §12.5 | — |
| Coins, repères, palette | éditions par position (§7.3) | migration testée |

### 12.5 Qualité/temps

| Mode | Résolution d'analyse max | Localisateur | Ellipses | Régularisation |
|---|---|---|---|---|
| Rapide | 1 MP | surface partielle ou Devernay | Halíř–Flusser | colinéarité + axes |
| Équilibré (défaut) | 4 MP | surface partielle / erf / Devernay (§5.5) | + LM géométrique | toutes, sauf symétries entre zones |
| Précis | 8 MP | idem | + hyper-renormalisation | toutes |

Au-delà de la résolution d'analyse, **tout le pipeline** tourne sur l'image réduite (flou gaussien puis rééchantillonnage par surfaces), et le SVG est remis à l'échelle. La géométrie ne dépend pas de la résolution, et ce choix borne le temps comme la mémoire. En mode Précis, une relocalisation par bandes à pleine résolution, autour des seules chaînes trouvées, est une option ultérieure.

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
| Grandes lignes et cercles manqués | grand cercle peu contrasté + petits détails | grand cercle suivi |
| Lignes interrompues | une droite coupée par 3 formes | une seule droite partagée (écart < 0,1 px) |
| Ellipse non suivie | cercle coupé par 2 à 4 bandes | un seul cercle partagé par tous les morceaux, à 0,1 px près |
| Coutures | formes sombres adjacentes ; bleu/gris avec liseré | écart de couleur maximal le long des frontières < ΔE_min, rendu à 1× et 4× |
| Artefacts JPEG | pétales colorés en JPEG q50 | pas de zone parasite ; ΔE moyen comparable au PNG |
| Petit détail qui casse la symétrie | forme symétrique + encoche sous et au-dessus de τ_perc | symétrie retrouvée sous τ_perc ; encoche conservée au-dessus |

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
- **Comparaison avec vectorizer.ai**, en local uniquement : une vingtaine d'images libres, avec leurs résultats vectorizer.ai conservés hors du dépôt. Mesures : ΔE, nombre de nœuds, comparaison visuelle A/B.

### 13.5 Performance, mémoire, robustesse

- **Chronomètres par étape** dans `eval` (natif, release) et dans le worker WASM (`performance.now()`), plus une sonde mémoire (`memory.buffer.byteLength`), dès la phase P1.
- **Tests d'endurance** avec assertion de temps : une photo de 2 MP, une frontière de 4000 px, une image de 16 MP (chemin de plafonnement).
- **Cas limites** : 1×1, 1×N, image entièrement transparente ou constante ; propagation de NaN.
- **Sorties de débogage** : un SVG des points de bord, chaînes, repères (avec ellipses de covariance) et ruptures de la programmation dynamique, pour le développement.

### 13.6 Déterminisme

Test doré : hachage du SVG sur un petit corpus, **identique au bit près** en natif et en WASM (`wasm-pack test --node`).

---

## 14. Implémentation Rust

### 14.1 Modules et dépendances

L'ordre des couches est num → raster → edges → detect → guides → topo → trace → regular → svg, sans dépendance inverse.

| Module (crate `core`) | Contenu |
|---|---|
| `num/` | Fonctions transcendantes propres (§14.4) ; matrices de taille fixe ; valeurs propres 2×2 fermées, 3×3 symétriques (Jacobi), 3×3 non symétriques (cubique + itération inverse) ; Cholesky et LDLᵀ (bande ou non) ; Householder QR ; `lm` générique ; racines stables ; ln Γ (Lanczos) ; queue binomiale en log (récurrence sur le rapport des termes, sans jamais calculer 1 − queue) ; FFT ; union-find ; PCG32 ; tri total. Chaque solveur entre dans la phase qui l'utilise en premier, avec ses tests. |
| `raster/` | `Plane<T>`, Oklab et jacobien, filtres gaussiens et dérivées, bandes, pyramide, modèle de bruit, classification d'entrée |
| `edges/` | tenseur généralisé, NMS, Devernay, chaînage, hystérésis, analyse par point, surface partielle, erf, Steger |
| `detect/` | segments, Förstner, jonctions, congés, arcs, cercles, ellipses, regroupement, taches, NFA |
| `guides/` | arène de repères, fusion, statuts, migration des éditions ; le type exporté reste dans `guide.rs` |
| `topo/` | postérisation, zones, frontières partagées, réparation F′ (extrait de `planar.rs`) |
| `trace/` | association, programmation dynamique, Béziers, ajustement conjoint |
| `regular/` | relations, résolution par composantes, répétitions (reprend `shape.rs`, `repeat.rs`, `align.rs`) |
| `svg/` | émission, anti-couture, `<use>` (extrait de `planar.rs`) |

### 14.2 Budget de temps

Image de 1920×1080 (2 MP), WASM sur un seul thread. Les débits scalaires de référence sont de 0,5 à 1,5 milliard d'opérations simples par seconde ; le SIMD multiplie par 2 à 4 les filtres [R5 §3.1].

| Étape | Opérations par pixel (ordre) | Total | Scalaire | SIMD |
|---|---|---|---|---|
| Oklab + jacobien + bruit | ~75 | 0,15 G | 100–300 ms | 60–150 ms |
| Gaussienne σ₀ (9 coeff. × 2 passes × 3 canaux), différences, tenseur, NMS | ~100 | 0,2 G | 130–400 ms | 50–150 ms |
| Pyramide (⅓ du précédent) + Steger et taches sur L | ~55 | 0,11 G | 70–220 ms | 30–90 ms |
| Analyse par point (5 à 30 % des pixels × ~300 op.) | — | 0,03–0,2 G | 20–400 ms | 20–300 ms |
| Chaînage, détecteurs D | par point | ~0,05 G | 30–100 ms | 30–100 ms |
| Postérisation (existante) | mesurée | — | 80–200 ms | 80–200 ms |
| Association, G–J | par point de frontière | — | 100–300 ms | 100–300 ms |
| **Total** (hors Nettoyage) | | | **≈ 0,5–1,9 s** | **≈ 0,4–1,3 s** |

**Objectifs** (mode Équilibré, 2 MP, hors Nettoyage) : ≤ 1 s en SIMD et ≤ 2 s en scalaire, plus ≤ 300 ms pour relancer G–J après un déplacement de curseur (§14.6). Le débruitage bilatéral actuel (jusqu'à 7×7 avec exponentielles, de l'ordre de 500 opérations par pixel) coûte à lui seul environ 1 G opérations quand il est actif. Il faudra l'optimiser (approximation séparable, table d'exponentielles), ou ne l'activer par défaut que si le bruit mesuré le justifie. Ces chiffres sont des estimations, remplacées par des mesures dès la phase P1.

### 14.3 Bornes explicites

Chaque étape par paires ou par programmation dynamique a un plafond. Au-delà, elle se dégrade proprement et le compte.

| Étape | Borne |
|---|---|
| Programmation dynamique de segmentation | fenêtre W = 200–400 points ; Béziers entre candidats à ≤ 16 d'écart ; ≤ 2 départs |
| Chaînage | tri par paquets, linéaire |
| Fusion de segments, regroupement d'arcs, fusion multi-échelle | grille spatiale + cases d'angle ; ≤ 16 partenaires par élément ; ≤ 10⁶ tests au total |
| Association | programmation dynamique en bande |
| Relations | ≤ 16 partenaires par primitive ; résolution par composante ≤ 200 inconnues |
| Symétries et répétitions entre zones | seaux de tailles et de signatures ; ≤ 10⁵ paires vérifiées |
| Temps par frontière | budget ; repli sur Béziers seuls (§10.1) |

### 14.4 Déterminisme

- **Fonctions transcendantes** (exp, ln, ln1p, atan2, sin, cos, cbrt, pow, hypot, ln Γ) : `num/` fournit ses propres versions en pur Rust. Elles sont portées des sources de la caisse `libm` (MIT, dérivée de musl et FreeBSD) et citées dans `THIRD_PARTY.md`. Les versions de `std` et `mul_add` sont interdites dans `core` par la règle clippy `disallowed-methods`, active dès la phase P0. Sinon, une différence d'un bit entre la bibliothèque mathématique native et celle du WASM ferait basculer des décisions discrètes : seuils, NFA, minimum de la programmation dynamique, ordre glouton.
- **SIMD** : une voie par pixel, coefficients appliqués dans le même ordre que le code scalaire. Les deux chemins sont testés pour l'égalité au bit près. Pas de relaxed-SIMD.
- **Ordre** : chaque étape gloutonne ou triée utilise `total_cmp` + un indice pour départager. Les NaN sont exclus avant tout tri (assertion de débogage). Pas de `HashMap` de `std` dans `core` : tableaux triés, `BTreeMap` ou hachage fixe.
- **Bandes et tuiles** : sans effet sur le résultat. Le chaînage et l'hystérésis se font sur la liste globale.
- **Aléa** (k-means, RANSAC éventuel) : PCG32 à graine dérivée de l'image et des réglages.

### 14.5 Mémoire

Plan f32 de 2 MP = 8 Mo. La mémoire WASM ne rétrécit jamais : le pic est permanent pour l'onglet.

| Poste | Taille à 2 MP |
|---|---|
| Image source + Oklab (3 plans) | ~33 Mo |
| Bande de calcul σ₀ (128 lignes, ~10 plans) | ~10 Mo |
| Pyramide (3 canaux, ⅓) | ~8 Mo |
| Points de bord (~50 o × 0,1–0,6 M) | 5–30 Mo |
| Postérisation (étiquettes, composantes, copie débruitée) | ~40 Mo |
| **Objectif de pic** | **≤ 150 Mo à 2 MP, ≤ 250 Mo à 4 MP** |

Ces chiffres sont mesurés dès la phase P2 (§13.5). Les plafonds de résolution par mode (§12.5) bornent le cas des très grandes images.

### 14.6 Cache des étapes

Les étapes A à F″ ne dépendent que de l'image et de Nettoyage, Détail, palette et mode de qualité. Le module WASM expose un objet `Analysis` qui les conserve. Un changement de Géométrie, Échelle ou Lissage, ou une édition de coin ou de repère, ne relance que G–J. Sans ce cache, l'interaction serait plus lente qu'aujourd'hui.

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

Chaque phase se termine par :
- ses tests synthétiques (§13.1) et la batterie de défauts (§13.2), même partielle ;
- le benchmark de non-régression ;
- les chronomètres et la sonde mémoire ;
- `scripts/check.sh` vert.

Les phases P3 et suivantes améliorent le pipeline existant pièce par pièce, au lieu de construire un second pipeline complet à côté.

| Phase | Contenu | Critère de sortie |
|---|---|---|
| P0 | `Plane`, conversions, fonctions transcendantes + règle clippy, PCG32, tri total, générateur de scènes | tests unitaires ; règle clippy active |
| P1 | Oklab et jacobien, modèle de bruit, classification d'entrée, gaussiennes par bandes, pyramide, chronomètres | σ_n à 10 % ; filtres conformes à la référence analytique ; temps mesurés |
| P2 | Bords sous-pixel, chaînage, analyse par point, variance, Steger ; calibration de σ_sys ; sorties de débogage | cibles C ; mémoire mesurée |
| P3 | Association et statut bord réel / bord de postérisation, **branchés dans le pipeline actuel** à la place de `edges.rs` (SOFT_EDGE) | cas « dégradés » de la batterie : aucune géométrie de l'ancien ajusteur sur les bandes ; aucune régression ΔE |
| P4 | Segments, coins, congés, arcs, cercles, ellipses, taches ; arène de repères ; tous affichés comme Candidats | cibles D, dont les fausses détections |
| P5a | Critère §3, programmation dynamique (droites, arcs, « sur repère », Béziers de Schneider), statuts Suivi/Candidat dans l'interface, migration des éditions | cas « lignes interrompues » et « ellipse non suivie » ; ΔE ≤ référence ; migration ≥ 95 % |
| P5b | Ellipses dans la programmation dynamique, SDM, ajustement conjoint G1 (Schur), cache `Analysis` | cibles G ; relance G–J ≤ 300 ms |
| P6 | Régularisation globale, répétitions `<use>`, réparation de topologie F′, pontage et fusion des petites zones | cibles I et F′ ; cas « symétrie » ; taille du SVG en baisse |
| P7 | Seuils locaux, réglage Qualité/temps, SIMD, optimisation du débruitage | 2 MP ≤ 1 s en SIMD (Équilibré) |
| P8 | Retrait de l'ancien code et des constantes remplacées ; évaluation sur photos (préréglage Photo) ; comparaison avec vectorizer.ai | aucune constante non étiquetée ; décision sur le préréglage Photo |

---

## 17. Risques et questions ouvertes

1. **Modèle de fond des NFA sur des images plates** : sur des aplats, de la postérisation ou des blocs JPEG, les orientations ne sont pas indépendantes et uniformes. Des alignements de blocs peuvent paraître significatifs. Parades : calibration de ε sur ces images, distribution empirique, plancher perceptif. Cible : ≤ 1 fausse détection par MP (§13.1).
2. **Espace de localisation** (§5.5, point 4) : hypothèse non publiée, à confirmer sur vérité terrain.
3. **Formule de variance** (§5.6) : algèbre vérifiée, mais le modèle (marche droite, bruit blanc) reste à confronter à Kakarala & Hero (1992) et à la mesure. σ_sys absorbe l'écart.
4. **Analyse par profil 1D** (§5.1) : approximation d'ordre (κσ)² sur les bords courbes, à mesurer sur les petits cercles.
5. **Loi de complexité locale** (§12.2) : sans référence, à calibrer.
6. **Coût sur les photos** : bornes de §14.3 et repli Béziers ; le préréglage Photo n'est basculé qu'après mesure (P8).
7. **Réparation de topologie** (§8.2) : la plus nouvelle des étapes, sans référence directe dans la littérature. Le critère d'acceptation par ΔE est simple ; ses effets de bord sur les frontières voisines sont à surveiller.
8. **Coutures** : la stratégie actuelle (traits de comblement) reste nécessaire. Sa mesure (§13.2) dira si le mode empilé doit devenir le défaut.
9. **Dégradés** : le statut « bord de postérisation » empêche les fausses géométries ; le rendu des dégradés en SVG reste une étape distincte.

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
   - une petite bavure JPEG de 1 px sur le cercle est absorbée : elle reste sous τ_perc (Échelle = 0,5 donne ρ_éch = 0,015 ; boîte du cercle de 400 px, diagonale ≈ 566 px, donc τ_perc ≈ 8 px).
7. **H** : les trois morceaux partagent les mêmes (C, r) ; les lettres sont réajustées avec des raccords G1 entre droites et arcs.
8. **I** :
   - les fûts verticaux des lettres sont mis exactement à la verticale ;
   - les lettres de même hauteur partagent leurs droites de pied et de tête (colinéarité, même interrompue entre les lettres) ;
   - les deux « o » identiques deviennent un tracé et un `<use>`.
9. **J** :
   - vérification : écart de couleur sous ΔE_min, sauf la bavure pontée volontairement ;
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
