# Algorithme de vectorisation sur mesure — document de conception

Version 1 (brouillon soumis à relecture). Ce document décrit l'algorithme cible d'OpenVectorizer : ses fondements mathématiques, l'organisation du code Rust, la manière de le valider et l'ordre dans lequel le construire. Les synthèses de recherche qui l'alimentent sont dans [`docs/recherche/`](recherche/) (en anglais) ; chaque section y renvoie par un code `[R1 §2.3]` = synthèse 01, section 2.3.

---

## 0. Objet et principes

### 0.1 Le constat

Le moteur actuel trace les contours d'une image **postérisée** (k-means dans Oklab) puis y ajoute des couches de correction : détection de coins par RDP, cercles et ellipses sur les contours isolés, symétries, alignements, vérification. Rien n'est mesuré sur l'image raster elle-même, sauf une netteté de bord (Scharr) et une position sous-pixel par couverture. Conséquences observées :

- des géométries « hallucinées » sur les dégradés (les frontières entre bandes de couleur sont des artefacts de la postérisation, pas des bords réels) ;
- les grandes lignes et grands cercles ne sont pas trouvés, parce que la recherche est locale et que les seuils sont en pixels absolus ;
- une cinquantaine de constantes réglées à la main, sans lien avec le bruit réel de l'image.

### 0.2 L'idée directrice

On sépare deux questions que le moteur actuel confond :

1. **Topologie** : quelles zones de couleur existent, lesquelles se touchent, où sont les jonctions. La postérisation y répond bien et on la garde.
2. **Géométrie** : où passe exactement chaque frontière, et quelle forme simple (droite, arc, ellipse, coin vif ou arrondi) elle suit. On y répond **sur l'image raster**, à plusieurs échelles, avec des détecteurs dont les seuils découlent d'un modèle de bruit.

Les détections raster deviennent des **repères** (« guides ») : segments, coins, jonctions, arcs, cercles, ellipses, taches, traits fins, chacun avec une position, une incertitude, une échelle et un score de signification. Le tracé des frontières de zones est ensuite une courbe lisse (Bézier) qui, à l'approche d'un repère, **devient** la primitive du repère. Le tracé final est une suite de droites, d'arcs de cercle, d'arcs d'ellipse et de Béziers, tangents entre eux sauf aux coins.

### 0.3 Les cinq règles de conception

1. **Toute décision est une comparaison de modèles.** « Est-ce une droite ? », « ce coin est-il arrondi ? », « ces deux traits sont-ils alignés ? » se tranchent par un critère unique : coût de description (résidus pondérés par l'incertitude) + pénalité par paramètre (MDL / AIC géométrique de Kanatani), ou par un nombre de fausses alarmes (NFA, approche a contrario). Pas de seuil arbitraire en pixels.
2. **Les seuils viennent de l'image.** Le bruit σ_n est estimé sur l'image ; les seuils de détection en découlent (probabilité de fausse alarme fixée). Ils peuvent varier localement.
3. **La simplicité est une préférence explicite.** Un a priori par type de primitive (droite < arc < ellipse < Bézier) et un poids réglable par l'utilisateur. Un détail plus petit que l'échelle de perception choisie ne peut pas casser une symétrie, une droite ou une ellipse.
4. **Tout est recodé en Rust, sans bibliothèque de traitement d'image.** On s'inspire de code permissif (MIT, BSD, Apache) en le citant ; pour le code GPL/AGPL, on implémente uniquement à partir des articles (§14).
5. **Déterministe et mesurable.** Même entrée, mêmes réglages → même SVG, au bit près, en natif comme en WASM. Chaque étape a une vérité terrain synthétique et un critère d'acceptation chiffré (§12).

### 0.4 Ce qui ne change pas

Le format des options et de la sortie (`Output { svg, stats, palette, corners, guides }`), l'interface (guides affichés, désactivables, forçables ; coins éditables), la palette k-means Oklab, les composantes connexes, les cycles en arêtes inter-pixels (crack edges), le découpage aux jonctions, le remplissage anti-couture, le benchmark de 515 cas. Le nouvel algorithme remplace l'intérieur de `fit.rs`, `shape.rs`, `align.rs`, une partie de `planar.rs`, et ajoute un étage de détection raster.

---

## 1. Vue d'ensemble

```
 image RGBA8
   │
   ├─(A) Oklab f32, estimation du bruit σ_n (global + carte locale), détection JPEG
   │
   ├─(B) Pyramide / espace d'échelle (budget qualité-temps)
   │       │
   │       ├─(C) Bords sous-pixel en couleur (Di Zenzo + Canny/Devernay), chaînes orientées,
   │       │     échelle de flou par point, variance par point ; traits fins (Steger)
   │       │
   │       └─(D) Détecteurs de primitives sur les chaînes : segments, coins & jonctions,
   │             arcs / cercles / ellipses, taches (blobs)  ──►  (E) GRAPHE DE REPÈRES
   │
   ├─(F) Postérisation (existante) → zones, frontières partagées, jonctions topologiques
   │       │
   │       └── Association frontière ↔ chaînes de bords (hybride) : chaque morceau de
   │           frontière reçoit des points sous-pixel mesurés et un statut « bord réel »
   │           ou « bord de postérisation »
   │
   ├─(G) Segmentation optimale de chaque frontière en primitives (programmation
   │     dynamique, critère MDL, a priori des repères)
   │
   ├─(H) Ajustement conjoint par frontière, contraintes G1 aux jonctions lisses
   │
   ├─(I) Régularisation globale : colinéarités, parallèles, rayons égaux, concentricité,
   │     symétries, répétitions (acceptées par le même critère MDL)
   │
   └─(J) Vérification contre les pixels, émission SVG (L, A, C ; frontières partagées)
```

Les étapes C–E ne dépendent pas de la postérisation et peuvent tourner en parallèle de F (plus tard, quand le WASM aura des threads).

---

## 2. Conventions

- **Coordonnées.** Le pixel (i, j) couvre le carré [i, i+1] × [j, j+1] ; son centre est en (i + ½, j + ½). Toute la géométrie (repères, frontières, SVG) est dans ce repère. Les plans d'image sont indexés par centres entiers ; on ajoute ½ à la sortie du détecteur. Pyramide à décimation par 2 avec moyennage de blocs 2×2 : x_plein = 2^o · x_o (en coordonnées « coin », celles du SVG). Une seule fonction de conversion, testée. Une erreur ici décale tous les bords grossiers de 0,25 à 0,5 px [R1 §4.4].
- **Couleur.** Détection et seuils dans **Oklab** (L ∈ [0, 1] ; ΔE ≈ distance euclidienne ; seuil de visibilité ≈ 0,02). Les matrices de conversion sont dans [R1 §1.3]. Alpha : prémultiplié avant conversion, puis canal supplémentaire du tenseur (§4.1).
- **Échelles.** σ en pixels de l'image d'origine ; t = σ². Gaussienne normalisée g_σ.
- **Précision numérique.** f32 pour les plans d'image ; f64 pour tous les ajustements géométriques et les accumulations [R5 §3.4].
- **Notation statistique.** B(n, k, p) = Σ_{j≥k} C(n, j) p^j (1−p)^{n−j} (queue binomiale) ; NFA = nombre de tests × probabilité sous le modèle de fond ; un événement est retenu si NFA ≤ ε, ε = 1 [R2 §2].

---

## 3. Étape A — Préparation et modèle de bruit

### 3.1 Conversion

sRGB 8 bits → linéaire → LMS → racine cubique → Oklab, en f32, un plan par canal (L, a, b, α). Planaire (structure de tableaux) pour les filtres séparables et le SIMD [R5 §3.4].

### 3.2 Estimation du bruit σ_n

Méthode d'Immerkær robuste [R1 §5] : masque N = [[1, −2, 1], [−2, 4, −2], [1, −2, 1]] (différence de deux laplaciens, annule les structures lisses ; Σ N² = 36). Par canal c :

  σ_n,c = médiane(|I_c ∗ N|) / (0,6745 · 6)

calculé uniquement sur les pixels dont le gradient est sous sa médiane (on exclut les bords, qui gonflent l'estimation), avec une itération de raffinement.

- **Plancher** : σ_min ≈ 0,5/255 en équivalent sRGB, propagé dans Oklab par le jacobien local. Sans plancher, un aplat numérique donne σ_n = 0 et une sensibilité infinie.
- **JPEG** : détection d'une périodicité 8×8 de |I ∗ N| (rapport de l'énergie aux positions de bloc sur l'énergie moyenne). Si détectée, σ_n est relevé (plancher 2–3/255) et le débruitage existant (bilatéral Oklab, réglage « Nettoyage ») est proposé plus fort par défaut. Les échos de bord (ringing) sont ensuite éliminés par le plancher perceptif (§4.3).
- **Carte locale** : le même estimateur sur des tuiles de 64×64 px (médiane robuste, au moins 256 pixels plats par tuile, sinon valeur globale), interpolée bilinéairement. C'est la base des seuils locaux (§11).

### 3.3 Estimation du flou de l'image σ_b

Le flou optique (photo, mise à l'échelle) se mesure sur les bords droits détectés en C, par la sélection d'échelle (§4.5). On en garde la médiane globale σ_b et une version par tuile. Il fixe le rayon minimal d'un arrondi détectable (§5.3) et l'échelle de localisation.

---

## 4. Étape C — Bords sous-pixel en couleur

C'est le socle : tout le reste en dépend. Méthode : Canny/Devernay modifié par Grompone von Gioi et Randall (IPOL 2017), appliqué au gradient couleur de Di Zenzo. Code de référence AGPL : on implémente depuis l'article [R1 §2–3, R5 §1.1].

### 4.1 Gradient couleur

Dérivées gaussiennes séparables à l'échelle σ (noyaux renormalisés : Σ g = 1, Σ x·g′(x) = −1 ; bords par réflexion ; rayon ⌈4σ⌉ ; σ ≥ 0,8 au niveau de travail) [R1 §1.1].

Tenseur de Di Zenzo **blanchi** : pondération de chaque canal par l'inverse de sa variance de bruit (w_c = 1/σ_n,c²), pour que la statistique de détection soit exacte quel que soit le canal :

  g11 = Σ_c w_c (∂x L_c)², g22 = Σ_c w_c (∂y L_c)², g12 = Σ_c w_c ∂x L_c ∂y L_c
  λ± = ½ [ (g11 + g22) ± √((g11 − g22)² + 4 g12²) ], θ = ½ atan2(2 g12, g11 − g22)

- Force du bord E = √λ+ (en unités de bruit).
- Normale orientée : n = (cos θ, sin θ), signe choisi pour que n · ∇L_L > 0 (canal de luminance), ou le canal de plus grand |n·∇L_c| si le bord est isoluminant.
- Indicateur de jonction : ρ = λ− / λ+ (≈ 0 sur un bord simple, grand près d'un coin ou d'une jonction).
- Le contraste du bord en Oklab non pondéré, Δ = (ΔL, Δa, Δb), est conservé par point : il sert à l'association avec la palette (§8.2) et au plancher perceptif.

On ne lisse pas le tenseur (échelle d'intégration nulle) pour la localisation [R1 §1.2].

### 4.2 Suppression des non-maxima et position sous-pixel

NMS sur l'axe dominant (variante IPOL) : si |gx| ≥ |gy|, le pixel est un maximum horizontal si E(x−1, y) < E(x, y) ≥ E(x+1, y) ; sinon test vertical. Puis correction de Devernay le long de cet axe :

  η = ½ (a − c) / (a − 2b + c),  a = E(p − d), b = E(p), c = E(p + d),  |η| ≤ ½

Point sous-pixel p + η·d. Biais résiduel de quelques centièmes de pixel, tabulable par simulation et soustrait [R1 §3.1].

**Raffinement optionnel** (mode qualité « Précis » et au-delà) :
- *Effet de surface partielle* (Trujillo-Pino 2013) pour les images nettes anti-crénelées : position < 0,01 px, plus courbure et contraste par point. Formules dans [R1 §3.2] ; référence lisible sous MIT.
- *Ajustement d'un bord flou* I(x) = B + (A − B)·Φ((n·x − ρ)/s) par Gauss-Newton sur une bande normale, pour les photos floues : donne position, flou s, couleurs et matrice de covariance.

Choix automatique : effet de surface partielle si σ_b < 0,7 px, ajustement erf sinon.

### 4.3 Seuils automatiques

En zone plate, avec le tenseur blanchi, trace/1 suit une loi χ² à 2C degrés de liberté (C canaux), mise à l'échelle par la variance d'une dérivée gaussienne :

  σ_g² = 1 / (8π σ⁴)  (variance d'une composante de gradient pour un bruit blanc unitaire)

Seuil haut H et bas L tels que P(χ²_{2C} > T²/σ_g²) = p, avec p_H = 10⁻⁵ et p_L = 10⁻² par pixel. Pour C = 3 : P(χ²_6 > x) = e^{−x/2} (1 + x/2 + x²/8), inversé par Newton. Comme λ+ ≤ trace, le test est conservateur.

**Plancher perceptif** : un bord n'est retenu que si son contraste |Δ| en Oklab dépasse ΔE_min = 0,02 (réglable via « Détail »). Cela élimine les bords statistiquement réels mais invisibles, et les échos JPEG.

**Validation a contrario des chaînes** (optionnelle, deuxième filtre) : NFA d'une chaîne de longueur l = N_chaînes · H(μ)^{l/2}, où μ est le gradient minimal le long de la chaîne et H la distribution empirique de |∇| sur l'image (échantillons indépendants tous les 2 px) [R2 §2].

Attention : les échantillons d'une même chaîne sont corrélés sur environ 2σ (le noyau). Toute statistique sur une chaîne utilise un nombre effectif d'échantillons n_eff = longueur / (2σ), pas le nombre de points.

### 4.4 Chaînage

Voisinage 5×5. Lien A → B accepté seulement si g(A)·g(B) > 0 (orientations à moins de 90°) et AB · g(A)^⊥ > 0 (le côté sombre reste du même côté). Chaque point a au plus un successeur et un prédécesseur. Pour supprimer la dépendance à l'ordre de parcours de l'article, on collecte tous les liens candidats, on les trie par longueur (puis par indice) et on les accepte gloutonnement (à la Kruskal) [R1 §7.1]. Hystérésis ensuite le long des chaînes : une chaîne survit si un de ses points dépasse H ; les points sous L la coupent.

Les chaînes s'arrêtent à σ–2σ des jonctions (gradient indéfini) : leurs extrémités et tangentes sont enregistrées pour l'étape D.

### 4.5 Échelles

Pile d'échelles σ_k = σ_0 · 2^{k/3} (σ_0 = 1), dérivées gaussiennes jusqu'à σ = 2, pyramide au-delà [R1 §4].

- **Détection** à l'échelle fine σ_0 (adaptée au bruit : 1 à 1,5).
- **Sélection d'échelle par point** (Lindeberg, γ = ½) : on maximise G_k = σ_k · E_k² sur la pile, interpolation parabolique en log σ ⇒ σ_sel. Pour un bord flou de variance t_0, γ = ½ donne t_sel = t_0 : **σ_sel est le flou du bord**.
- **Localisation** à σ_loc = max(σ_0, σ_sel/2), jamais sur un niveau décimé. On ne localise pas à σ_sel : sur un bord courbe de courbure κ, le lissage introduit un biais ≈ κσ²/2 vers le centre de courbure.
- Les **échelles grossières** (niveaux de pyramide) servent à détecter les grandes structures floues ou peu contrastées (grandes lignes, grands cercles) ; leurs chaînes sont re-localisées à pleine résolution dans une bande de ±3σ.

### 4.6 Incertitude par point

Pour un bord droit de contraste h, de flou σ_b, détecté à l'échelle σ (s² = σ² + σ_b²), le déplacement normal dû au bruit a pour variance [R1 §7.2, à confronter à Kakarala & Hero 1992] :

  Var(δ) = (3/8) · (σ_n / h)² · (s / σ)⁶

Pour la couleur, (h/σ_n)² est remplacé par le rapport signal/bruit blanchi Σ_c h_c²/σ_n,c² le long de n. S'y ajoutent des biais déterministes (parabole de Devernay, courbure κσ²/2, espace colorimétrique §4.7). Si l'ajustement erf est utilisé, la covariance vient directement de σ_n² (JᵀJ)⁻¹.

Chaque point de bord stocke : position, normale orientée, contraste Δ (Oklab), force E, σ_sel, σ_loc, var_n, indicateur de jonction ρ, identifiant de chaîne. Les ajustements de courbes pondèrent par 1/var_n (distance le long de la normale uniquement).

### 4.7 Espace de localisation

Le flou optique a lieu en lumière linéaire, mais la plupart des moteurs SVG anti-crénèlent en sRGB gamma. La racine cubique d'Oklab et le gamma sRGB déplacent le point d'inflexion d'un bord flou très contrasté vers le côté sombre, de quelques dixièmes de σ_b. Décision : on détecte et on seuille dans Oklab, puis on relocalise la position finale sur les canaux sRGB gamma (le croisement à 50 % reproduit alors l'anti-crénelage du raster). C'est une hypothèse de l'auteur de la synthèse, à vérifier sur rendus synthétiques avant d'en faire le défaut (interrupteur interne).

### 4.8 Traits fins (Steger)

Un trait de largeur W ≲ 3–4 px donne deux bords trop proches pour être localisés indépendamment. On le détecte comme une **crête** (ligne centrale + largeur) avec la méthode de Steger 1998 (code de référence GPL : depuis l'article) [R1 §6] :

- hessienne à l'échelle σ, vecteur propre n de la plus grande |valeur propre| ;
- t = −(r_x n_x + r_y n_y) / (r_xx n_x² + 2 r_xy n_x n_y + r_yy n_y²), point de crête si |t n_x| ≤ ½ et |t n_y| ≤ ½ ;
- sélection d'échelle γ = ¾ (pour une crête gaussienne de variance t_0, t_sel = t_0) ;
- largeur et correction de biais par une table (w, a) → (largeur, décalage) construite par simulation du profil analytique ;
- arbitrage crête / paire de bords par la réponse normalisée de chaque détecteur à son échelle sélectionnée.

Sortie : repère « Trait » (ligne centrale + largeur). L'émission en trait SVG (`stroke`) est une option future ; dans un premier temps, le repère contraint les deux frontières de la zone fine à rester parallèles.

---

## 5. Étape D — Détecteurs de primitives

Ils travaillent sur les chaînes de C (ordonnées, sous-pixel, pondérées), pas sur des pixels bruts. C'est le principe d'EDLines / EDCircles (code des auteurs sous MIT) [R2 §1.2, R5 §1.2].

### 5.1 Segments de droite

1. **Croissance sur chaîne** (EDLines) : ajustement par moindres carrés totaux pondérés sur une longueur minimale, extension tant que l'écart normal reste compatible avec var_n (test χ² au lieu du seuil fixe de 1 px), réajustement à la sortie.
2. **Longueur minimale significative** : L_min tel que NFA = N⁴ · p^{n_eff} ≤ 1 (p = 1/8, N = côté de l'image) ; environ 12 px pour 512² à σ = 1.
3. **Validation a contrario** : NFA = N⁴ · B(n, k, 1/8), k = nombre de points dont l'orientation du gradient est à moins de 22,5° de la normale du segment [R2 §1.1–1.2]. On garde −log₁₀ NFA comme confiance.
4. **Ajustement** : centroïde, matrice de dispersion pondérée S, direction = vecteur propre de λmax, résidu = λmin. Covariance : var(φ) ≈ σ²/Σ w_i t_i², var(décalage au centroïde) = σ²/W (incertitude en sablier) [R2 §1.5].
5. **Fusion des segments colinéaires interrompus** (la demande « grandes lignes ») : candidats indexés par angle (cases de 5°) et grille spatiale ; test de colinéarité par distance de Mahalanobis D² = Δθᵀ (Σ1 + Σ2)⁻¹ Δθ comparée à χ²₂ (99 % → 9,21), puis test a contrario du trou : la fusion est acceptée si NFA(union) < NFA(s1)·NFA(s2) (un grand trou non aligné gonfle n sans k et fait échouer la fusion automatiquement) [R2 §1.6]. Itéré du plus long au plus court.
6. **Transformée de Hough** : seulement en secours, pour les alignements très lointains (règles, grilles), par vote à noyau gaussien elliptique depuis les covariances des segments (idée KHT, article seulement, code GPL).

### 5.2 Coins et jonctions

Deux sources complémentaires :

- **Topologiques** (exactes) : les coins de pixels où au moins 3 étiquettes de zones se rencontrent. Le nombre de frontières incidentes donne l'ordre m. Elles viennent de F.
- **Géométriques** (raster) : pour les coins en L (2 zones seulement, donc invisibles topologiquement) et pour valider/positionner. Opérateur de Förstner sur le tenseur couleur lissé (σ_i ≈ 1,5–2) : candidats si rondeur q = 4 det M/(tr M)² > 0,5 et poids w = det M / tr M au-dessus d'un seuil dérivé de σ_n ; position précise par
  x̂ = argmin Σ_i w_i (g_iᵀ (x̂ − x_i))²  ⇒  (Σ w_i g_i g_iᵀ) x̂ = Σ w_i g_i g_iᵀ x_i, covariance σ̂² M⁻¹ [R2 §3.2].
  Le même système avec les tangentes à la place des gradients donne le **centre d'un arrondi**.

**Position d'un coin** : intersection, pondérée par les covariances, des droites (ou arcs) ajustées sur chaque branche, en excluant un rayon de 2–3 px autour du sommet où l'anti-crénelage corrompt les mesures. Harris ne sert jamais de position (biais vers l'intérieur du coin ≈ σ_i).

**Typage** (pour l'interface et la régularisation) depuis les angles des branches : L (m = 2), T (m = 3 dont une paire colinéaire), Y, flèche, X [R2 §3.5].

### 5.3 Vif ou arrondi

Diagnostic par le comportement en échelle [R2 §3.7] : pour un coin vif d'angle de virage Δ, la courbure maximale après lissage σ vaut κ_max ≈ Δ/(σ√(2π)) (σ·κ_max constant) ; pour un arrondi de rayon r, κ_max ≈ 1/r (constant) tant que σ ≪ rΔ. On mesure κ_max à 3–4 échelles, avec σ_eff² = σ_b² + σ_lissage².

Puis ajustement à un paramètre de l'arrondi tangent aux deux branches (géométrie du congé [R2 §4.1]) :

  C(r) = V + (r / sin(φ/2)) · b,  T_k = V + (r / tan(φ/2)) · u_k,  h = r (1/sin(φ/2) − 1)

où V est le sommet virtuel, φ l'angle intérieur, b la bissectrice intérieure. Minimisation de E(r) = Σ_i (|p_i − C(r)| − r)² (points au-delà des points de tangence mesurés aux droites) par recherche dorée sur [0, r_max].

**Décision** : arrondi si (J_vif − J_arrondi)/σ̂² > λ · 1 (un paramètre de plus ; λ du §11) **et** r > r_min = 1,5 · σ_eff. Sinon vif. Ce test remplace AUTO_SHARP_RADIUS et la règle des 135°.

### 5.4 Arcs, cercles, ellipses

Découpage des chaînes en arcs convexes (aux coins et aux inflexions), puis par arc [R3 §1–3] :

- **Cercle** : ajustement algébrique *Hyper* (Al-Sharadqah & Chernov 2009, biais du second ordre nul), raffiné par Levenberg-Marquardt sur Σ(|p_i − c| − R)². Paramétrage de Chernov (A, B, C, D avec B² + C² − 4AD = 1) pour gérer proprement la limite « droite » (R → ∞).
- **Ellipse** : Halíř & Flusser 1998 (problème propre 3×3, toujours une ellipse) après normalisation de Hartley ; puis, en mode « Précis », hyper-renormalisation de Kanatani (3–5 itérations, précision à la borne KCR), repli sur Halíř & Flusser si le résultat n'est pas une ellipse ; raffinement géométrique par distance orthogonale (distance point–ellipse d'Eberly, bissection garantie).
- **Limites d'étendue** : cercle seulement si l'arc couvre ≥ 25–30° ; ellipse libre seulement si ≥ 120° ou si le cercle est rejeté par le critère MDL. Sur un arc court la variance de R croît comme 1/Δ⁴.
- **Covariance** Cov(p) ≈ σ̂² (JᵀJ)⁻¹, pour les tests « cette ellipse est-elle un cercle ? » ((A − B)/σ_{A−B} < 2) et « est-elle alignée sur les axes ? ».
- **Test des runs** sur le signe des résidus : des résidus systématiques (peu de longues séries) révèlent un mauvais modèle (superellipse, rectangle arrondi pris pour une ellipse), même avec un faible résidu total.

**Regroupement d'arcs** en une même ellipse (cercle coupé par une autre forme) à la Fornaciari 2014 : convexités compatibles, centres compatibles (Mahalanobis, ou propriété des cordes parallèles : leurs milieux sont alignés sur une droite passant par le centre), ajustement conjoint, validation NFA (fraction du périmètre soutenue par des points de bord bien orientés), union-find pour supprimer les doublons [R3 §2.2–2.4].

### 5.5 Taches (blobs)

Laplacien normalisé t(L_xx + L_yy) et déterminant de la hessienne t²(L_xx L_yy − L_xy²) sur la pile d'échelles, extrema dans (x, y, t). Pour un disque de rayon r, l'extremum est à σ = r/√2 : **rayon gratuit**. Raffinement sous-pixel par interpolation quadratique 3D (≈ 0,1 px, 5 % sur le rayon) [R3 §4.1]. Usage :

- repères « Cercle » pour les petits points (< 6–10 px) que l'ajustement de contour ne peut pas traiter, et que la postérisation fusionne ou fragmente ;
- semences pour garder une petite zone que le filtre d'aire supprimerait ;
- détection de trames (points sur un réseau régulier à une même échelle) à filtrer avant postérisation.

MSER/MSCR (VLFeat, BSD) est une extension possible pour fournir des régions stables et leur ellipse de moments ; pas dans la première version.

---

## 6. Étape E — Le graphe de repères

### 6.1 Structure

Un repère (`Guide`) porte :

| Champ | Contenu |
|---|---|
| `kind` | Segment, Coin, Jonction, Arc, Cercle, Ellipse, Trait, Tache ; plus les relations Symétrie, Répétition, Alignement produites en I |
| géométrie | paramètres de la primitive (droite (θ, ρ) + extrémités ; cercle (C, r) + étendue ; ellipse (C, a, b, φ) + étendue ; coin : sommet, directions des branches, rayon) |
| `cov` | covariance des paramètres |
| `scale` | échelle σ à laquelle il a été détecté |
| `nfa` | −log₁₀ NFA (confiance) |
| `support` | chaînes de bords qui le soutiennent |
| `status` | Suivi / Candidat / Forcé / Désactivé (inchangé côté interface) |
| clé | type + point clé (milieu, point le plus haut), recherche à GUIDE_REACH comme aujourd'hui |

Les repères sont stockés dans une arène (`Vec` + indices `u32`), avec une grille spatiale pour les requêtes « repères près de ce point ».

### 6.2 Fusion multi-échelle

Un même objet est souvent détecté à plusieurs échelles. Deux repères de même type sont fusionnés si leurs paramètres sont compatibles (Mahalanobis < χ²_k(0,99)) ; on garde celui de plus faible NFA et on agrège les supports. Un repère grossier qui n'a pas d'équivalent fin reste un repère « de grande échelle » : il contraint les frontières mais n'impose pas de position fine (son incertitude est grande).

### 6.3 Statut Suivi / Candidat

Aujourd'hui, un repère est « suivi » s'il passe la tolérance du réglage Géométrie, « candidat » sinon. Dans le nouveau modèle, le statut vient de la comparaison de modèles de l'étape G : un repère est **suivi** si la segmentation MDL l'a retenu, **candidat** s'il est significatif (NFA ≤ 1) mais que la segmentation a préféré autre chose. Forcer un candidat revient à imposer ce modèle sur la portée concernée ; désactiver un repère le retire des a priori.

---

## 7. Étape F — Postérisation et topologie (existant, adapté)

Inchangé : débruitage bilatéral dosable, palette k-means Oklab (éditable), étiquettes, composantes, filtre d'aire, cycles de crack edges, découpage aux jonctions en **frontières partagées** (chaque frontière entre deux zones est un objet unique utilisé par les deux contours : c'est ce qui garantit l'absence de trous et de chevauchements).

Changement : le **remplissage anti-couture** (traits de comblement) reste, mais doit devenir inutile une fois que les frontières partagées sont émises une seule fois avec une géométrie commune ; on mesurera la différence.

---

## 8. Association hybride frontière ↔ bords

C'est le cœur de l'hybridation demandée : la topologie vient de la postérisation, la géométrie des bords raster.

### 8.1 Points de frontière

Pour chaque frontière (zones A | B), on calcule les points sous-pixel actuels (couverture mesurée entre les deux teintes) comme **position de repli**.

### 8.2 Appariement

Un point de chaîne de bord q est apparié à la frontière A | B si :
1. distance(q, frontière) < 2 px + 3·√var_n(q) ;
2. sa normale est compatible avec celle de la frontière (écart < 30°, orientation A → B cohérente) ;
3. son contraste Δ(q) est compatible avec la différence de palette c_B − c_A : cos(Δ, c_B − c_A) > 0,8 et |Δ| / |c_B − c_A| ∈ [0,5 ; 2].

Appariement par programmation dynamique monotone le long de la frontière (les deux séquences sont ordonnées), pour éviter les croisements.

### 8.3 Statut de la frontière

Chaque morceau de frontière reçoit :
- **bord réel** si ≥ 60 % de sa longueur est soutenue par des points appariés ;
- **bord de postérisation** sinon (typiquement la limite entre deux bandes d'un dégradé).

Un bord de postérisation n'a **aucun repère géométrique** : il est tracé en Bézier lisse, avec une tolérance large (il n'y a pas de vérité géométrique à respecter). C'est la version principielle du critère actuel SOFT_EDGE = 0,45, et la réponse aux hallucinations sur les dégradés. (Les dégradés eux-mêmes, en remplissage SVG, sont une étape ultérieure de la feuille de route.)

### 8.4 Position

Sur les parties soutenues, la position de la frontière est celle des points de bord (pondérés par 1/var_n) ; ailleurs, la couverture. Les repères dont les supports touchent la frontière lui sont attachés avec leur portée (abscisses de début et de fin le long de la frontière).

---

## 9. Étapes G et H — Segmentation et ajustement des frontières

### 9.1 Formulation

Pour une frontière de points p_0..p_{N−1} (variances v_i), on cherche des points de rupture b_0 < … < b_K et un modèle m_k par morceau qui minimisent [R4 §1] :

  E = Σ_k [ J(b_k, b_{k+1}, m_k) + λ · c(m_k) + π(m_k) ] + Σ_ruptures λ_join(type)

- J = Σ (distance orthogonale)² / v_i, en tenant compte de la corrélation des points (n_eff) ;
- c(m) = nombre de paramètres : droite 2, arc 3, ellipse 5, Bézier aux tangentes fixées 2, Bézier libre jusqu'à 8 ;
- π(m) = **a priori de simplicité par type** (droite 0, arc +1, ellipse +3, Bézier +4, en unités de λ), réglable ;
- un morceau qui coïncide avec un repère suivi reçoit un **bonus** = −log₁₀ NFA du repère (le repère est une preuve indépendante venue du raster), et son modèle est imposé si le repère est forcé ;
- λ_join : une jointure lisse (G1) coûte un paramètre de moins qu'un coin (angle de tangente partagé).

λ est dérivé de l'AIC/MDL géométrique de Kanatani (G-AIC : λ = 2 ; G-MDL : λ = −ln(σ/L)², plus conservateur, préfère le modèle dégénéré). Le réglage « Géométrie » de l'utilisateur interpole entre les deux (§11).

### 9.2 Programmation dynamique

F[j] = min_{i<j, m} F[i] + coût(i, j, m), avec pointeurs arrière [R4 §1.2].

- **Coûts en O(1)** par sommes préfixes : droite (moments d'ordre 2 → λmin de la dispersion), cercle (moments jusqu'à l'ordre 4 pour l'ajustement algébrique ; classement suffisant pour la DP, raffinement géométrique ensuite), ellipse (matrice de dispersion 6×6 cumulable + problème propre 3×3).
- **Béziers** : pas de forme O(1) ; restreints aux portées entre points de rupture candidats (coins, extrema de courbure, extrémités de repères), en nombre N_c ≈ N/5 à N/20.
- **Élagage** : portée rejetée si l'écart maximal dépasse τ_max ; élagage PELT (on retire i des candidats quand F[i] + coût(i, j) > F[j] + constante), espérance O(N).
- **Frontière fermée** : on ouvre au coin le plus sûr (ou à une extrémité de repère) ; à défaut, quelques départs essayés.
- **Ruptures candidates** : coins et jonctions de D, extrema de la courbure, extrémités de repères, points de jonction topologique. Les marches d'escalier d'une droite discrète ne sont jamais des coins (critère des segments maximaux, Lachaud et al.) [R4 §3.2].

### 9.3 Coin ou jointure lisse

À chaque rupture : ajustement libre (J_libre) et ajustement avec point et tangente partagés (J_G1). Coin si (J_G1 − J_libre) > λ. Filtre rapide : virage < 10° → lisse ; > 60° → coin ; entre les deux, le test. Les coins détectés sur le raster (Förstner, jonctions) imposent la rupture ; leur type (vif/arrondi, §5.3) et les éditions de l'utilisateur (`corners.edits`, rayon glissé à la souris) priment.

### 9.4 Ajustement des Béziers

Schneider (Graphics Gems, licence libre) comme base : tangentes d'extrémité (estimateur λ-MST ou repère adjacent), paramétrage par longueur de corde, système 2×2 pour les longueurs de poignées, reparamétrage de Newton [R4 §2.1]. Améliorations :

- **Coupure** au maximum de courbure plutôt qu'au point d'erreur maximale, ou laissée à la DP ;
- **Raffinement par minimisation de la distance au carré** (SDM, Wang-Pottmann-Liu 2006) : convergence quasi-newtonienne, bien plus rapide et robuste que la projection point par point ;
- **Poignées positives** (α ≥ 0) imposées, sinon repli sur |P3 − P0|/3.
- Référence de qualité à porter : `curve-fit-nd` (Blender, BSD-3) et `kurbo::fit` (MIT/Apache, ajustement par aire signée et premier moment).

### 9.5 Ajustement conjoint G1 (étape H)

Toute la frontière est ensuite réajustée d'un bloc par Levenberg-Marquardt [R4 §2.4] :

- contraintes **éliminées par paramétrage** quand c'est possible (plus robuste que les multiplicateurs) : jonction Bézier–droite (P1 = J + β·u, J sur la droite), Bézier–Bézier G1 (P2^A = J − β_A·d(φ), P1^B = J + β_B·d(φ)) ;
- contraintes restantes (tangence droite–cercle |n·C − ρ| = r, cercle–cercle) par système KKT :
  [JᵀJ + μI, Cᵀ ; C, 0] [δ ; ν] = [−Jᵀr ; −c] ;
- la matrice est bande le long de la frontière : Cholesky bande, O(K) par itération pour K morceaux.

G2 (continuité de courbure) n'est pas visé dans la première version ; G1 suffit pour un rendu « designer ».

### 9.6 Lissage préalable

Lissage λ|μ de Taubin (λ ≈ 0,33, μ ≈ −0,34 ; sans rétrécissement) sur les points de repli uniquement, morceau par morceau, sans traverser les coins [R4 §3.1]. Les points de bord raster n'ont pas besoin d'être lissés : leur variance est connue.

---

## 10. Étape I — Régularisation globale

### 10.1 Relations candidates

Découvertes par regroupement des paramètres [R4 §5, R3 §5] :

| Relation | Découverte | Paramètre partagé |
|---|---|---|
| Colinéarité (même sur des segments éloignés) | regroupement (θ, ρ) | droite |
| Parallélisme, orthogonalité | histogramme de θ mod π, largeur 2–4° | θ (ou θ + π/2) |
| Axe horizontal / vertical / 45° | attracteurs dans l'histogramme | θ ∈ {0, π/4, π/2} |
| Rayons égaux | regroupement relatif 3–5 % | r |
| Concentricité | centres à 1–2σ | C |
| Tangence | distance (C, droite) ≈ r | contrainte |
| Symétrie miroir / rotation | corrélation circulaire FFT de la courbure, énergie des harmoniques de Fourier ; vote d'axes entre zones | appariement de points |
| Répétition | signatures (Hu, descripteurs de Fourier elliptiques), alignement de Procrustes | forme moyenne + transformations |

### 10.2 Acceptation

Une relation qui retire q degrés de liberté est acceptée si :

  ΔJ / σ² < λ · q  **et**  écart maximal < τ_perception

avec le même λ que la segmentation : segmentation et régularisation obéissent à un seul objectif. τ_perception = max(3σ_loc, ρ_échelle · taille de la forme), où ρ_échelle vient du réglage « Échelle » : c'est la règle « préférer la forme la plus simple en ignorant les détails trop petits ». Un détail plus petit que τ_perception ne peut pas empêcher une symétrie, une droite ou une ellipse.

Ordre glouton : du plus faible ΔJ au plus fort, réajustement incrémental, rejet des relations contradictoires (cycles parallèle/orthogonal incohérents détectés dans l'union-find avec étiquettes d'angle mod π/2). Hiérarchie de confiance : repère raster > axes > parallèle/orthogonal > rayon égal/concentrique > symétrie (la plus souvent accidentelle sur les petites formes : au moins 3 éléments appariés exigés).

### 10.3 Résolution

- relations « même valeur » par **élimination** : union-find → une seule variable par classe, puis LM sans contrainte ;
- autres (tangence, symétrie, longueurs égales) par KKT ou lagrangien augmenté ;
- par étapes à la GlobFit : orientations, puis positions, puis égalités, avec réajustement de la topologie entre chaque étape.

### 10.4 Répétitions et `<use>`

Les instances d'une forme répétée sont moyennées dans leur repère canonique (le bruit baisse en 1/√k pour k copies), puis émises comme un tracé maître et des `<use>` avec transformation. Le détecteur de réseau (vecteurs de différence les plus fréquents) aligne les positions sur une grille ou un cercle quand c'est significatif.

Les modules actuels `shape.rs` (symétries, profil radial) et `repeat.rs` (seaux de tailles, Procrustes) sont conservés comme générateurs de candidats ; leur acceptation passe au critère 10.2.

---

## 11. Seuils automatiques, adaptation locale et réglages

### 11.1 D'où vient chaque seuil

| Grandeur | Origine | Remplace |
|---|---|---|
| σ_n (global et carte) | Immerkær robuste (§3.2) | — |
| seuils de bord H, L | χ²_{2C}, p_H = 10⁻⁵, p_L = 10⁻² | gradient Scharr + SOFT_EDGE |
| plancher de contraste | ΔE_min = 0,02 Oklab, piloté par « Détail » | — |
| longueur minimale d'un segment | NFA ≤ 1 (≈ 12 px à 512²) | segments ≥ 6 px |
| repère significatif | NFA ≤ ε = 1 | CANDIDATE_GEOMETRY |
| rayon minimal d'arrondi | 1,5 · σ_eff (flou mesuré) | AUTO_SHARP_RADIUS |
| choix de modèle | λ (G-AIC ↔ G-MDL) + a priori de type | tolérances en px, ARC_TOL |
| étendue min. arc / ellipse | 25–30° / 120° (conditionnement) | — |
| tolérance de perception | max(3σ_loc, ρ_échelle · taille) | relative = 0,02 × scale |
| écart maximal par morceau τ_max | 3·√v_i (95 % unilatéral ≈ 2,5–3σ) | VERIFY_SLACK |

Il reste des constantes (p_H, p_L, ε, angles de 22,5°, 10°/60°), mais elles sont **sans unité** et ont une interprétation (probabilité de fausse alarme, précision angulaire) : elles ne dépendent ni de la taille, ni du bruit, ni du contraste de l'image.

### 11.2 Adaptation locale

Trois cartes, calculées par tuiles et interpolées :

1. **σ_n local** (§3.2) → seuils de détection locaux.
2. **σ_b local** (flou) → échelle de localisation et rayon minimal d'arrondi locaux.
3. **Complexité locale** κ_loc = longueur de chaînes de bords par unité d'aire à l'échelle σ_0, normalisée par la médiane de l'image. Dans les zones très chargées (texture, feuillage), λ_local = λ · (1 + log(1 + κ_loc)) : on simplifie plus, on cherche moins de géométrie ; dans les zones dépouillées (logo), λ reste au niveau de base. Cette loi est une première proposition, à calibrer sur le corpus (§12).

En complément, le long d'une frontière, l'échelle de bruit « significative » de Kerautret & Lachaud (2012) fournit une tolérance locale sans paramètre ; à évaluer.

### 11.3 Réglages utilisateur

| Réglage (interface) | Effet dans le nouveau modèle |
|---|---|
| **Géométrie** (0–1) | interpole λ entre G-AIC (0) et G-MDL (1) et règle le poids de l'a priori de type : plus haut = préfère les primitives géométriques |
| **Échelle** (0–1) | ρ_échelle de la tolérance de perception (§10.2) et taille minimale des repères : plus haut = seules les grandes structures comptent, les petits détails sont absorbés |
| **Détail** | ΔE_min et aire minimale des zones |
| **Nettoyage** | force du débruitage bilatéral ; relève le plancher de σ_n |
| **Lissage** | poids des Béziers dans l'a priori de type, lissage Taubin |
| **Qualité / temps** (nouveau) | voir 11.4 |
| Coins, guides, palette | inchangés (éditions par position) |

### 11.4 Qualité / temps

| Mode | Résolution d'analyse max | Échelles / octave | Raffinement sous-pixel | Ellipses | Régularisation |
|---|---|---|---|---|---|
| Rapide | 1 MP (détection), pleine pour la localisation | 2 | Devernay | Halíř–Flusser | colinéarité + axes |
| Équilibré (défaut) | 4 MP | 3 | Devernay | + LM géométrique | toutes, sauf symétries entre zones |
| Précis | pleine | 3 | + surface partielle / erf | + hyper-renormalisation | toutes |

Au-delà de la résolution d'analyse, la détection se fait sur l'image réduite (Gaussienne puis décimation) et seules les chaînes trouvées sont relocalisées à pleine résolution dans une bande de ±3σ ; coût ≈ 3–4 fois moindre sans perte de précision [R1 §4.4].

---

## 12. Validation

### 12.1 Vérité terrain synthétique (priorité)

C'est le seul moyen de mesurer une précision sous-pixel [R5 §4.2]. Un générateur Rust (dans `crates/core`, tests) dessine des scènes SVG aléatoires à paramètres connus : droites, polygones aux angles 30°–170°, coins arrondis de rayon connu, cercles, ellipses (étendues 30°–360°), Béziers, traits fins, dégradés ; rendu avec anti-crénelage analytique (ou sur-échantillonnage 16×16), puis flou σ_b, bruit σ_n, compression JPEG à plusieurs qualités. Graines fixes, rien n'est stocké dans le dépôt sauf le code.

Métriques et cibles (à SNR ≥ 20 dB) :

| Étape | Mesure | Cible |
|---|---|---|
| C | erreur RMS de position des bords | < 0,05 px net, < 0,1 px flou σ_b = 1 |
| C | estimation de σ_b | < 15 % |
| A | estimation de σ_n | < 10 % (bruit blanc), < 25 % (JPEG) |
| D | segments : rappel / précision des extrémités | > 95 % / < 0,5 px |
| D | coins : détection, position, vif/arrondi | > 95 %, < 0,2 px, > 95 % corrects, r à 10 % |
| D | cercles / ellipses : centre, axes, angle | < 0,1 px, < 1 %, < 1° (arc ≥ 120°) |
| G–I | primitives retrouvées (type correct) | > 90 % |
| J | aucune fausse géométrie sur dégradés | 0 repère suivi sur les bords de postérisation |

### 12.2 Benchmark existant

Les 515 cas (`openvectorizer eval testdata --baseline testdata/baseline.json`) restent la non-régression de fidélité : ΔE moyen ≤ 0,927 et p95 ≤ 2,93 (valeurs actuelles), taille du SVG et nombre de nœuds suivis en plus.

### 12.3 Corpus d'aller-retour

Rendus d'icônes et d'emojis sous licence libre (Noto Emoji et Material Symbols, Apache-2.0) à plusieurs résolutions, avec et sans JPEG : on compare la géométrie retrouvée au SVG d'origine (Chamfer/Hausdorff des contours, précision/rappel des coins à 1–2 px, part des primitives régularisées). Téléchargés à la demande, jamais commités. Les jeux de données de recherche (YorkUrban, Prasad, Fornaciari, BIPED) servent ponctuellement en local, leurs licences interdisant souvent la redistribution.

### 12.4 Déterminisme

Test doré : hachage du SVG sur un petit corpus, identique en natif et en WASM (`wasm-pack test --node`).

---

## 13. Implémentation Rust

### 13.1 Modules

| Module (crate `core`) | Contenu |
|---|---|
| `num/` | matrices de taille fixe (génériques const), valeurs propres 2×2 fermées, 3×3 symétriques (Jacobi ou Kopp), 3×3 non symétrique (cubique + produit vectoriel), Cholesky/LDLᵀ, Householder QR, LM générique, racines quadratiques/cubiques/quartiques stables, ln Γ (Lanczos), queue binomiale en log, FFT radix-2, union-find, PRNG PCG32, tri total (`total_cmp` + indice) |
| `raster/` | `Plane<T>` planaire, conversion Oklab, filtres gaussiens séparables et dérivées, IIR pour σ > 5, pyramide, bruit, carte de tuiles |
| `edges/` | tenseur Di Zenzo, NMS, Devernay, chaînage, hystérésis, sélection d'échelle, incertitude, surface partielle, erf, Steger |
| `detect/` | segments, Förstner, jonctions, arcs, cercles, ellipses, regroupement, taches, NFA |
| `guides/` | graphe de repères, fusion multi-échelle, statuts (étend `guide.rs`) |
| `trace/` | association frontière ↔ bords, DP MDL, Schneider + SDM, ajustement conjoint G1 |
| `regular/` | relations, union-find, résolution par étapes, répétitions (reprend `shape.rs`, `repeat.rs`, `align.rs`) |
| `planar.rs`, `lib.rs` | orchestration, émission SVG (inchangé dans le principe) |

### 13.2 Performances

Objectif : 2 MP en moins d'une seconde dans le navigateur, sur un seul thread [R5 §3.1].

| Étape (1920×1080) | Budget |
|---|---|
| Oklab + bruit | 20–40 ms |
| Dérivées gaussiennes (3 canaux, 3–6 échelles) | 60–150 ms |
| NMS, Devernay, chaînage | 30–60 ms |
| Förstner, segments, arcs, taches | 60–150 ms |
| Postérisation (existante) | 80–200 ms |
| Association, DP, ajustements, régularisation | 80–200 ms |
| **Total** | **≈ 0,4–0,8 s** (estimation, à mesurer) |

Règles :
- **pas d'allocation dans les boucles chaudes** : une structure `Workspace` réutilisée, passée par `&mut` ;
- itération par tranches (`chunks_exact`, lignes) pour que le compilateur élimine les vérifications de bornes ;
- passe verticale des filtres par accumulation de lignes (pas d'accès en colonne) ; bords traités hors de la boucle intérieure ;
- **SIMD wasm (simd128)** : chemin scalaire de référence + chemin SIMD testés pour l'égalité au bit près ; deux binaires WASM (avec et sans SIMD), choisis par détection de fonctionnalité côté JavaScript ; jamais de relaxed-SIMD ni de `mul_add` (déterminisme) ;
- **tuiles** avec marge = rayon du noyau, pour pouvoir paralléliser plus tard (Web Workers) sans changer le résultat ;
- mémoire : un plan f32 de 2 MP = 8 Mo ; libérer ou réutiliser les plans entre étapes ; viser < 150 Mo de pic.

### 13.3 Déterminisme

- pas de `HashMap` itérée pour produire une sortie (tableaux triés ou `BTreeMap`) ;
- fonctions transcendantes : celles de `libm` en WASM sont déterministes ; en natif, elles peuvent différer au dernier bit, donc le test doré compare natif et WASM sur des sorties arrondies au centième (format de sortie actuel) ;
- générateurs aléatoires (RANSAC éventuel, k-means) à graine dérivée de l'image et des réglages.

---

## 14. Licences et sources

Règle : on lit et on peut traduire le code **permissif** (en citant la source dans `THIRD_PARTY.md` et en tête du fichier) ; pour le **copyleft** et le code sans licence, on implémente uniquement depuis l'article, sans avoir le code ouvert, et on cite l'article en commentaire [R5 §2].

| Module | Sources lisibles (permissives) | Article seulement |
|---|---|---|
| Filtres, pyramide | imageproc (MIT), OpenCV (Apache-2.0), VLFeat (BSD-2) | Young–van Vliet |
| Bords sous-pixel | OpenCV/imageproc Canny ; surface partielle : `subpixel-edges` (MIT) | Devernay 1995, IPOL 2017 (code AGPL) |
| Traits fins | hessienne de scikit-image (BSD-3) | Steger 1998 (code ImageJ GPL-2) |
| Segments | `lsd.cpp` d'OpenCV (réécriture permissive), ED_Lib/EDLines (MIT), ELSED (Apache-2.0) | LSD IPOL (code AGPL) |
| Coins | Förstner de scikit-image (BSD-3), cornerSubPix d'OpenCV | Förstner & Gülch 1987 |
| Ellipses | EDCircles (MIT), fitEllipseDirect d'OpenCV, EllipseModel de scikit-image | ELSDc (AGPL), AAMED (GPL-2), Fornaciari (sans licence), Kanatani (code sans conditions) |
| Taches | blob_log/doh de scikit-image, `mser.c` de VLFeat | — |
| Tracé, postérisation | VTracer et visioncortex (MIT), ImageTracer.js (Unlicense) | Potrace 2003 (GPL) |
| Béziers, arcs | curve-fit-nd (BSD-3), kurbo (MIT/Apache), lyon_geom (MIT/Apache), FitCurves.c (Graphics Gems) | — |
| Régularisation | — | GlobFit 2011 (sans licence), Hoshyari 2018, régularisation CGAL (GPL-3) |

Pièges relevés [R5 §1–2] : `pytlsd` (dépôt MIT mais `lsd.cpp` AGPL inclus), `h3ct0r/fast_ellipse_detector` (étiquette MIT sur du code sans licence), `ridge-detector` (MIT mais dérivé de GPL-2). Toujours lire les en-têtes de fichier. Aucun brevet actif connu sur les méthodes retenues ; celui de Microsoft sur Kopf-Lischinski (pixel art) est à vérifier avant un éventuel mode pixel art.

---

## 15. Plan de construction

Chaque phase se termine par ses tests synthétiques (§12.1), le benchmark de non-régression et `scripts/check.sh` vert. Le nouveau pipeline est développé à côté de l'ancien, derrière une option interne, jusqu'à ce qu'il le batte sur le benchmark.

| Phase | Contenu | Critère de sortie |
|---|---|---|
| P0 | Boîte à outils `num/`, `Plane`, générateur de scènes synthétiques | tests unitaires des solveurs (reconstruction A = VΛVᵀ, ‖VᵀV − I‖ < 10⁻¹²) |
| P1 | Oklab, bruit, dérivées, pyramide | σ_n à 10 %, filtres égaux à la référence analytique |
| P2 | Bords sous-pixel, chaînage, échelle, incertitude | cibles C du §12.1 |
| P3 | Association frontière ↔ bords, statut bord réel / postérisation | 0 géométrie sur dégradés, aucune régression ΔE |
| P4 | Segments, coins, arrondis, arcs, cercles, ellipses, taches ; graphe de repères ; interface des guides branchée dessus | cibles D |
| P5 | DP MDL, Schneider + SDM, ajustement conjoint G1 | cibles G, ΔE ≤ baseline |
| P6 | Régularisation globale, répétitions `<use>` | cibles I, taille SVG en baisse |
| P7 | Seuils locaux, réglage qualité/temps, SIMD | 2 MP < 1 s en WASM (mode Équilibré) |
| P8 | Retrait de l'ancien code et des constantes remplacées | 0 constante en pixels non justifiée |

---

## 16. Risques et questions ouvertes

1. **Modèle de fond des NFA sur des images plates** : sur des aplats, des images postérisées ou en blocs JPEG, les orientations de gradient ne sont pas indépendantes et uniformes ; des alignements de blocs de 8 px peuvent paraître significatifs. Parades : débruitage léger, distribution empirique H(μ), plancher perceptif. À mesurer.
2. **Espace de localisation** (§4.7) : hypothèse non publiée, à confirmer sur rendus synthétiques avant d'en faire le défaut.
3. **Formule de variance** (§4.6) dérivée par la synthèse, pas vérifiée contre l'article de Kakarala & Hero.
4. **Calibration de la loi de complexité locale** (§11.2) : première proposition, sans référence.
5. **Coût de la DP avec Béziers** sur de très longues frontières : borné par le nombre de ruptures candidates ; à surveiller sur les photos.
6. **Interface** : les clés de repères (type + position) doivent rester stables d'un tracé à l'autre malgré le nouveau détecteur ; les éditions de coins aussi.
7. **Dégradés** : le statut « bord de postérisation » empêche les fausses géométries, mais le rendu des dégradés eux-mêmes (remplissages `linearGradient`/`radialGradient`) reste une étape distincte.

---

## Annexe — Références principales

Bords : Canny 1986 ; Di Zenzo 1986 ; Devernay 1995 ; Grompone von Gioi & Randall, IPOL 2017 ; Trujillo-Pino et al. 2013 ; Lindeberg, IJCV 30(2) 1998 (deux articles) ; Immerkær 1996 ; Steger, PAMI 20(2) 1998 ; Kakarala & Hero, PAMI 14(7) 1992.
Segments et a contrario : Desolneux, Moisan & Morel 2000, 2001, 2008 ; Grompone von Gioi et al. (LSD), PAMI 2010 et IPOL 2012 ; Akinlar & Topal (EDLines 2011, EDPF 2012, EDCircles 2013) ; Fernandes & Oliveira (KHT) 2008.
Coins : Harris & Stephens 1988 ; Förstner & Gülch 1987 ; Xia, Delon & Gousseau (ACJ) 2014 ; Mokhtarian & Suomela 1998 ; He & Yung 2008 ; Lachaud, Vialard & de Vieilleville 2007 ; Kerautret & Lachaud 2012.
Coniques : Kåsa 1976 ; Taubin 1991 ; Al-Sharadqah & Chernov 2009 ; Fitzgibbon, Pilu & Fisher 1999 ; Halíř & Flusser 1998 ; Kanatani et al. 2011, 2012 ; Eberly (distance point–ellipse) ; Fornaciari, Prati & Cucchiara 2014 ; Pătrăucean et al. (ELSDc) 2017.
Sélection de modèles : Kanatani, IJCV 26(3) 1998 et PAMI 26(10) 2004 ; Rosin & West, PAMI 17(12) 1995.
Courbes : Schneider, Graphics Gems 1990 ; Wang, Pottmann & Liu, ACM TOG 2006 ; Taubin 1995 ; Selinger (Potrace) 2003 ; Hoshyari et al., SIGGRAPH 2018 ; Dominici et al. (PolyFit) 2020 ; Li et al. (GlobFit) 2011 ; Li et al. (Diffvg) 2020.
Symétrie et répétition : Hu 1962 ; Kuhl & Giardina 1982 ; Zabrodsky, Peleg & Avnir 1995 ; Loy & Eklundh 2006.
