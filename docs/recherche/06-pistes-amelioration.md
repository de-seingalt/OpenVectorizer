# 06 — Pistes d'amélioration : mathématiques et optimisation Rust/WASM

Lecture unique de `docs/ALGORITHME.md` (version 4). Ce document ne refait pas l'audit de justesse ; il propose des idées nouvelles. Chaque piste indique la section visée, l'idée, son intérêt et son risque. Rien n'a été compilé ni mesuré : les gains sont des ordres de grandeur estimés, et les formules marquées « dérivée ici » sont à revérifier par un test avant adoption.

Contraintes respectées : tout en Rust sans bibliothèque externe, sources GPL/AGPL par les articles seulement, WASM mono-thread, sortie identique au bit près en natif et en WASM.

---

## 1. Idées mathématiques

### M1. Détecter dans un espace à variance stabilisée, par table de 256 entrées (§4.2, §4.3, §5.2)

**Constat.** Le tenseur blanchi du §5.2 est G = ∇Lᵀ Σ_n⁻¹ ∇L avec Σ_n = J·D·Jᵀ (D diagonale en lumière linéaire, J jacobien linéaire → Oklab). Or ∇L = J·∇I au premier ordre, donc

  G = ∇Iᵀ Jᵀ (J D Jᵀ)⁻¹ J ∇I = ∇Iᵀ D⁻¹ ∇I.

Le jacobien Oklab **se simplifie entièrement** : détecter en Oklab blanchi équivaut, au premier ordre, à détecter canal par canal en RGB avec chaque canal divisé par son écart-type de bruit. Oklab n'apporte donc rien à la détection ; il coûte un jacobien et une inversion 3×3 par pixel.

**Idée.** Remplacer « Oklab + jacobien + blanchiment par pixel » par une transformation stabilisatrice de variance par canal :

  S_c(v) = Σ_{u<v} 1/σ_c(u), v ∈ 0..255

où σ_c(u) est l'écart-type de bruit au niveau codé u (plancher du §4.3 compris). C'est une **table de 256 flottants par canal**. Dans les plans S, le bruit est de variance 1 et de covariance identité : G = Σ_c ∇S_c ∇S_cᵀ, sans pondération, et la loi χ²_{2C} du §5.4 s'applique telle quelle. La carte locale de tuiles (§4.3, point 4) reste un facteur scalaire appliqué au seuil.

La fonction de niveau σ_c(u) s'estime directement sur les valeurs codées : réponses d'Immerkær en entiers (i16, exactes), histogramme 2D (classe d'intensité × |r|), médiane lue dans l'histogramme. Plus de tri de flottants, plus de modèle poissonien-gaussien (peu crédible après courbe de ton et JPEG) : une courbe monotone lissée sur 16 classes suffit, avec repli sur la valeur globale pour les classes pauvres.

**Pourquoi c'est mieux.**
- Environ 75 opérations par pixel remplacées par 3 lectures de table ; trois plans Oklab et le jacobien disparaissent du chemin de détection (20 à 30 Mo à 2 MP).
- Sur une image propre, le plancher (0,5/255 en sRGB codé) domine, donc S est proportionnel au sRGB codé : l'espace de détection **coïncide** avec l'espace de localisation retenu au §5.5 (point 4). Un seul SNR, une seule covariance pour les §5.4 et §5.6.
- Estimation du bruit en entiers : déterministe par construction.

Oklab reste utilisé là où il a un sens perceptif : palette, ΔE_min (calculé aux seuls points de bord, à partir des deux couleurs de part et d'autre), vérification J.

**Risque.** L'équivalence est au premier ordre (J constant sur le support du noyau). Elle est exacte là où les seuils se jouent (zones presque plates), mais la force E diffère sur les bords très contrastés, ce qui peut déplacer légèrement le maximum de la NMS sur un bord flou entre couleurs très éloignées. À mesurer sur la vérité terrain (cibles C). La corrélation entre canaux reste ignorée, comme aujourd'hui.

### M2. Calibrer les seuils sur la statistique de détection elle-même, par niveau (§4.3, §5.4, §17 risque 1)

**Constat.** σ_g² = σ_n² · Σk′² · Σk² suppose un bruit **blanc**. Un JPEG, un agrandissement bilinéaire ou une image rééchantillonnée ont un bruit corrélé : le masque d'Immerkær (passe-haut) y voit moins de bruit que la dérivée gaussienne (passe-bande), et l'écart grandit avec le niveau de pyramide, où le bruit corrélé ne baisse pas d'un facteur 4 par niveau. Les seuils H et L sont alors trop bas, précisément sur les images à risque de fausses détections.

**Idée.** Garder Immerkær pour la **forme** de la fonction de niveau, et mesurer un facteur d'échelle γ_o par niveau de pyramide sur la statistique réellement seuillée : T = trace(G). En zone plate T/γ suit χ²_{2C}. On ajuste γ par deux quantiles bas de T sur l'image (par exemple 10 % et 25 %, lus dans un histogramme) :

  γ = q₂₅(T) / χ²_{2C;0,25}, validé si q₁₀(T)/q₂₅(T) ≈ χ²_{2C;0,10}/χ²_{2C;0,25}.

Si le rapport des quantiles ne concorde pas (moins d'un quart de l'image est plate), on garde la prédiction de bruit blanc. γ > 1 signale un bruit corrélé ; il corrige aussi var_n (§5.6) et, en première approximation, κ_c.

**Pourquoi c'est mieux.** Les seuils deviennent justes quel que soit le spectre du bruit, sans modèle de spectre. C'est une réponse directe au risque 1 du §17, et cela remplace une partie de la calibration de ε « sur images de bruit et aplats JPEG ».

**Risque.** Les quantiles bas sont biaisés vers le haut si l'image est texturée partout ; le test de concordance borne ce cas. Sur un aplat numérique pur, T = 0 partout : le plancher du §4.3 reste nécessaire.

### M3. Loi exacte de la plus grande valeur propre, en forme fermée (§5.4)

Le document propose une table simulée pour rendre exact le test sur λ+. Pour C = 3 canaux, G/σ_g² suit une loi de Wishart 2×2 à 3 degrés de liberté, dont la densité des valeurs propres est proportionnelle à (l₁ − l₂)·e^{−(l₁+l₂)/2}. L'intégrale double se calcule à la main (dérivée ici) :

  P(λ+/σ_g² > x) = x·e^{−x/2} + e^{−x}  (C = 3)

Contrôle : au quantile 10⁻² de χ²₆ (x = 16,81), cette queue vaut 3,8·10⁻³, soit le facteur 2,6 annoncé au §5.4. Pour C = 1, λ+ = trace et la queue est e^{−x/2}. Pour C = 2 et C = 4 (alpha), la même intégrale donne des termes en erf, ou se calcule une fois par quadrature 1D à l'initialisation.

**Intérêt.** Plus de table simulée ni de graine ; le seuil se résout par Newton sur une expression fermée. **Risque** : nul si un test Monte-Carlo confirme la formule ; à refaire pour C = 2 et 4.

### M4. Devernay sur le logarithme : biais nul et flou gratuit (§5.3, §5.5)

**Idée.** Le profil du gradient à travers une marche floutée par une gaussienne de flou total s est lui-même gaussien : son **logarithme est exactement une parabole**. On interpole donc sur ℓ = ln λ+ (pas besoin de racine) :

  η = ½ (ℓ_a − ℓ_c) / (ℓ_a − 2ℓ_b + ℓ_c)

Pour une marche droite gaussienne, η est exact, sans table de biais. Les mêmes trois valeurs donnent le flou, en un seul passage et à une seule échelle :

  s² = −2 cos²θ / (ℓ_a − 2ℓ_b + ℓ_c),  s² = σ₀² + σ_b² (+ 1/12 pour l'intégration du pixel)

où θ est l'angle entre la normale et l'axe dominant de la NMS (le pas d'échantillonnage en travers du bord vaut cos θ).

**Pourquoi c'est mieux.** La table de biais de Devernay se réduit à l'écart au modèle gaussien (intégration par le pixel, courbure). Le flou σ_b devient disponible dès la bande de calcul, pour chaque point, sans seconde échelle ; moyenné le long d'une portion droite comme au §5.5, il peut remplacer l'estimateur à deux échelles ou le recouper (deux estimateurs indépendants : un écart signale un bord voisin ou un profil non gaussien, ce qui est justement le critère d'exclusion du §5.5).

**Risque.** Le logarithme amplifie le bruit sur les petites valeurs : à réserver aux points au-dessus du seuil H (les valeurs voisines valent encore ≈ 0,6 fois le maximum à s ≈ 1,1). Sur les bords faibles, le biais de Rice du module n'est pas gaussien ; repli sur la parabole ordinaire. Coût : trois ln par point de bord, négligeable.

### M5. Nombre d'échantillons effectifs à portée finie (§3.2)

**Constat.** n_eff = n/κ_c est la limite pour n grand. Sur une portée courte (5 à 15 points, κ_c ≈ 3,5), elle donne n_eff inférieur au nombre de paramètres du modèle, alors que c'est sur ces portées que la programmation dynamique tranche.

**Idée.** Utiliser la valeur exacte pour la moyenne de n points régulièrement espacés :

  κ_c(n) = 1 + 2 Σ_{k=1}^{n−1} (1 − k/n) ρ(k·Δs),  n_eff = n / κ_c(n) ≥ 1

κ_c(n) croît de 1 (n = 1) vers la valeur asymptotique du §3.2. C'est une table par frontière (Δs constant), remplie une fois, indexée par la longueur de portée : aucun surcoût dans la boucle.

**Pourquoi c'est mieux.** Les tests sur portées courtes (encoches, petits arcs, côtés de lettres de 8 px) cessent d'être sur-pénalisés par un facteur allant jusqu'à 3. Le seuil de Šidák z_S en profite aussi.

**Risque.** La valeur exacte pour un paramètre de haute fréquence spatiale (courbure d'un arc court) est encore plus petite que κ_c(n) ; la formule reste donc prudente. Un traitement complet par moindres carrés généralisés n'est pas praticable (la matrice de corrélation gaussienne est très mal conditionnée). L'élagage PELT, déjà désactivé dans le cas courant, perd sa condition d'additivité.

### M6. Bornes rigoureuses en O(1) pour la zone morte (§3.5, §10.1)

**Constat.** Le tri rapide utilise une variance gonflée v_i + (τ_perc/3)², heuristique sans garantie ; le classement peut écarter le vrai meilleur modèle.

**Idée.** La fonction f(u) = max(0, √u − τ)² est convexe en u = d². Par l'inégalité de Jensen pondérée (w_i = 1/v_i, W = Σw_i, J_ols = Σ w_i d_i²) :

  W · max(0, √(J_ols/W) − τ_perc)²  ≤  J_zone-morte  ≤  J_ols

Les deux bornes sortent des sommes déjà calculées. On en déduit un schéma exact : un modèle dont la borne basse (plus pénalités) dépasse la borne haute d'un autre est éliminé sans évaluation ; l'évaluation exacte en O(n) n'est lancée que pour les modèles dont les intervalles se chevauchent.

**Pourquoi c'est mieux.** Le résultat de la programmation dynamique devient exact par rapport au coût à zone morte, sans la règle « deux meilleurs modèles », et la constante 1/3 disparaît. À Échelle = 0 les bornes coïncident : aucune évaluation exacte.

**Risque.** Quand le résidu RMS est sous τ_perc, la borne basse vaut 0 et ne discrimine pas ; c'est le cas fréquent à forte Échelle. Le nombre d'évaluations exactes peut alors dépasser celui du schéma actuel : garder le budget en unités de travail comme garde-fou, et mesurer.

### M7. Un seul terme d'Occam pour les modèles libres et « sur repère » (§3.3, §10.2, §6.4)

**Idée.** Le coût du §10.2 contient min(λ·c(m), ln det(I + Σ_g Aᵀ(κ_c V)⁻¹A)). Un modèle libre est le cas limite d'un repère dont la covariance a priori est Σ₀ = diag(L²) (paramètre inconnu dans la boîte englobante). On peut donc écrire **un seul** coût pour tous les modèles :

  coût de description = ln det(I + Σ_prior Aᵀ(κ_c V)⁻¹A) ≈ Σ_p 2 ln(L/σ_p)

avec Σ_prior = Σ₀ (libre) ou Σ_g (repère). C'est l'évidence bayésienne en approximation de Laplace ; λ_MDL = 2 ln(L/σ̄) en est la version où tous les paramètres sont supposés déterminés à σ̄ près. Le curseur Géométrie interpole comme aujourd'hui entre 2·c(m) et ce terme.

**Pourquoi c'est mieux.**
- Le min() et la convention « moins de la moitié du support » du §10.2 deviennent inutiles : un repère faiblement soutenu a un Σ_g large et paie continûment jusqu'au prix du modèle libre.
- Un paramètre mal déterminé (rayon d'un arc de 15°) a σ_p proche de L : il ne coûte presque rien mais n'apporte presque rien. Les étendues minimales 25–30° et 120° du §6.4 redeviennent de pures gardes numériques (σ_p < L), sans valeur en degrés.
- La matrice AᵀV⁻¹A est déjà disponible par les moments de la portée.

**Risque.** Le terme croît en ln n_eff par paramètre (comportement de type BIC), ce que Kanatani évite délibérément pour l'ajustement géométrique. Les a priori π et la table de conversion du réglage Géométrie seraient à recalibrer. Idée structurante mais incertaine : à prototyper sur la batterie §13.2 avant de modifier le §3.

### M8. A priori de type tirés du corpus d'aller-retour (§3.3, §12.1)

Les valeurs π = 0 / 0,5 / 1 / 1,5 sont étiquetées « goût ». Le corpus du §13.4 (Noto Emoji, Material Symbols) donne la fréquence réelle p_m de chaque type de morceau dans des SVG dessinés à la main. La longueur de code du choix du type est −2 ln p_m ; en unités de λ, π(m) = −2 ln(p_m/p_droite)/λ_ref. La constante passe de « goût » à « statistique de corpus », et le réglage Géométrie garde son rôle de multiplicateur. **Risque** : le corpus (icônes) n'est pas représentatif des logos ; les SVG sources codent presque tout en Béziers, il faut donc d'abord reclasser leurs morceaux (droite, arc, autre) à tolérance nulle.

### M9. Distance de Sampson partout, Eberly seulement en vérification (§6.4, §10.1, §10.5)

Le coût de la programmation dynamique est déjà de la forme f²/|∇f|² (Sampson). Le raffinement géométrique d'ellipse par la distance d'Eberly (bissection par point et par itération, branchée, non vectorisable) peut utiliser le même résidu : pour des points à une distance très petite devant le rayon de courbure, Sampson et la distance vraie diffèrent au second ordre en d·κ. On minimise Σ w_i f_i²/|∇f_i|² par Levenberg-Marquardt, et Eberly ne sert qu'au contrôle final (J) et aux ellipses très aplaties. **Intérêt** : un seul type de résidu de D à H, jacobiens analytiques, coût divisé par 5 à 10 sur les ellipses. **Risque** : biais près des extrémités du grand axe d'ellipses très excentriques (b/a < 0,2) ; garder Eberly dans ce cas.

### M10. Loi de complexité locale : tester d'abord l'hypothèse nulle (§12.2, §17 risque 5)

λ_b = 2 ln N_c et le coût de choix 2 ln(n_type + 1) croissent déjà avec la densité de structures. Avant de calibrer λ_local = λ(1 + log(1 + κ_loc)), mesurer le cas « adaptation locale » de la batterie en calculant simplement N_c et n_type **localement** (par tuile ou par composante) au lieu de globalement. Si le critère passe, la loi sans référence disparaît. **Risque** : aucun, c'est une expérience à faire avant d'ajouter la loi.

### M11. Nettoyage indexé sur le bruit mesuré (§12.4, `denoise.rs`)

Le filtre bilatéral actuel utilise σ_c = 0,015 + 0,05·force et un rayon de 1 à 3 px, sans lien avec l'image. Avec le modèle de bruit : σ_c(x) = k·σ_n(x) en unités stabilisées (M1), k ∈ [0 ; 4] piloté par le réglage. La valeur par défaut « automatique » devient k = 2 quand le drapeau JPEG est levé, 0 sinon, ce qui répond aussi à la remarque du §14.2 sur son coût. **Risque** : changement de comportement du réglage, table de conversion à prévoir.

---

## 2. Idées d'optimisation Rust/WASM

### R1. NMS sans `atan2` ni racine par pixel (§5.2, §5.3)

- Axe dominant : |cos θ| ≥ |sin θ| ⇔ cos 2θ ≥ 0 ⇔ **g11 ≥ g22**. Aucun angle à calculer.
- La NMS compare λ+ (monotone avec E) : une seule racine par pixel (le discriminant), au lieu de deux.
- Rejet précoce : λ+ ≤ trace, donc si g11 + g22 < seuil L, le pixel est écarté sans racine.
- La normale n'est calculée qu'aux maxima retenus : n ∝ (g12, λ+ − g11) ou (λ+ − g22, g12), en prenant le couple de plus grande norme (même règle que le §14.7).

Même résultat que la formulation en θ, à l'arrondi près de la décision d'axe (à figer comme définition). Gain : un `atan2` et une racine en moins sur 2 MP, soit de l'ordre de 50 à 100 ms en scalaire.

### R2. Carte d'activité par tuiles (§5.1)

Calculer, sur les octets source, le min et le max par canal de chaque tuile 8×8 (entiers, trivialement vectorisable), puis dilater du rayon du noyau + 1. Pour une tuile, la borne

  λ+ ≤ trace ≤ 2 Σ_c (étendue_c(S) · ‖k′‖₁ · ‖k‖₁)²

prouve, si elle est sous le seuil L, qu'aucun point ne peut y être retenu : on saute dérivées, tenseur et NMS. Le saut est **exact** : un point gardé a E ≥ L, strictement supérieur à toute valeur d'une tuile sautée, donc les comparaisons de NMS ne changent pas ; la marge d'un pixel garantit que les voisins a et c de Devernay sont calculés. Les plans lissés ne sont jamais relus dans ces tuiles (l'analyse par point ne lit qu'autour des bords).

Gain attendu sur les préréglages Logo et Illustration (souvent plus de 80 % de pixels en aplat) : étapes B et C divisées par 3 à 5. Nul sur une photo. Même carte réutilisable pour le filtre bilatéral (R8) et Steger au niveau 0.

### R3. Filtres séparables écrits en « axpy décalés » : SIMD sans intrinsèques (§5.1, §14.4)

Rust ne réassocie pas les flottants et ne contracte jamais en FMA. Une boucle **élément par élément** donne donc le même résultat qu'elle soit vectorisée ou non. Il suffit que les deux passes soient élémentaires :

```rust
// passe verticale : pour chaque coefficient, une ligne entière
for (j, &kj) in kernel.iter().enumerate() {
    let src = ring.row(y + j);
    for (o, &s) in out.iter_mut().zip(src) { *o += kj * s; }
}
// passe horizontale : même schéma sur des tranches décalées de la même ligne
for (j, &kj) in kernel.iter().enumerate() {
    for (o, &s) in out.iter_mut().zip(&line[j..j + w]) { *o += kj * s; }
}
```

L'ordre des additions est fixé par j, identique en scalaire, en SSE/NEON et en `simd128`. Conséquences : pas de second chemin de code, pas de test d'égalité scalaire/SIMD à maintenir, et le gain SIMD prévu en P7 est disponible dès P1 en activant `+simd128`. Variante symétrique : out += k_j·(ligne[−j] + ligne[+j]), 5 multiplications au lieu de 9 (à figer comme définition, car l'arrondi diffère de la forme à 9 termes).

**Tampon circulaire.** La passe horizontale écrit dans un anneau de 2r + 1 lignes, la verticale lit l'anneau : la « bande de 128 lignes × 10 plans » (~10 Mo, §14.5) tombe à quelques lignes par plan, entièrement en cache L1/L2. Le découpage en bandes n'est plus qu'un détail de parcours.

**Risque.** Vérifier que les contrôles de bornes sont éliminés (`zip` sur tranches de même longueur) ; bords par réflexion traités en recopiant la ligne dans un tampon élargi, pas par test dans la boucle.

### R4. Pyramide : flou et décimation en une passe (§2, §5.1)

« Gaussienne puis moyenne 2×2 » est un noyau séparable fixe (g ∗ [½, ½]) évalué avec un pas de 2. En ne calculant que les sorties conservées, la passe horizontale produit une demi-largeur et la verticale une demi-hauteur : environ 3 fois moins de multiplications pour la construction de la pyramide, même convention de centre (§2). À figer comme définition du niveau, car l'arrondi diffère de la version en deux temps.

### R5. Profils le long de l'axe dominant, sans interpolation (§5.1, §5.5)

L'analyse par point échantillonne un profil le long de la normale, donc par interpolation dans les plans lissés (≈ 300 opérations par point). L'interpolation bilinéaire ajoute un flou dépendant de la position sous-pixel (variance t(1 − t) par axe, 1/6 en moyenne), qui biaise s et σ_b. Alternative : lire le profil **le long de l'axe dominant de la NMS** (ligne ou colonne de pixels), sans interpolation. Pour un bord droit c'est le même profil, étiré de 1/cos θ (θ ≤ 45°) : s_axe = s/cos θ, position normale = position axiale × cos θ. Le pas en travers du bord vaut cos θ ≤ 1 px : l'échantillonnage est plus fin à 45°. Les convolutions 1D vers les échelles supérieures se font sur ce profil, avec σ_axe = σ/cos θ.

**Intérêt** : lectures directes, pas de flou d'interpolation à tabuler, coût par point divisé par 3 environ. **Risque** : l'anisotropie du §5.1 (σ₀ le long du bord) n'est plus exactement alignée sur le bord ; l'effet de courbure est à remesurer sur les petits cercles (risque 4 du §17, qui existe déjà).

### R6. Moments par accumulation de blocs alignés sur les nœuds (§10.1)

Les portées de la programmation dynamique commencent et finissent sur des nœuds, et couvrent au plus 8 intervalles. Au lieu de différences de sommes préfixes (avec la combinaison de Pébay pour limiter la perte de chiffres), stocker **un vecteur de moments par intervalle entre nœuds**, centré sur son propre barycentre (15 moments jusqu'à l'ordre 4, plus les sommes pondérées). Pour un nœud de départ i, la boucle sur j = i+1..i+8 **accumule** les blocs par translation de moments :

```rust
let mut acc = Moments::zero_at(origin_i);
for j in i + 1..=(i + 8).min(n) {
    acc.add_translated(&block[j - 1]);   // ~100 opérations
    costs(i, j, &acc);                   // droite, cercle, ellipse
}
```

Il n'y a plus aucune soustraction de grands nombres : la perte de chiffres du §10.1 disparaît par construction, sans coordonnées normalisées par frontière. Coût : 8·N_c translations, inférieur aux évaluations de modèles elles-mêmes. Les sommes de AᵀV⁻¹A et AᵀV⁻¹r du modèle « sur repère » suivent le même schéma.

### R7. Mémo des coûts de portée et frontières « sales » (§10.1, §14.6)

Les résidus J_ols(i, j, m), les paramètres ajustés et l'écart maximal d'une portée ne dépendent **ni de Géométrie ni de Lissage** : seules les pénalités λ·c, λ·π, λ_b en dépendent. Le coût exact à zone morte dépend de τ_perc (Échelle), pas de λ.

- Table plate par frontière, `[N_c × 8 × M]` de `f64`, remplie paresseusement (sentinelle « non calculé »), conservée entre deux relances de G–J.
- Changement de Géométrie : seule la récurrence min-plus est rejouée (N_c × 8 × M additions), soit moins d'une milliseconde par frontière ; les évaluations exactes manquantes sont complétées à la demande.
- Changement d'Échelle : invalidation de la couche « zone morte » des seules formes dont τ_perc change (il est plafonné par τ_abs pour les grandes formes).
- Édition d'un coin ou d'un repère : drapeau « sale » par frontière et par composante de relations ; les autres gardent leur segmentation et leur ajustement H.

**Intérêt** : la cible de 300 ms pour un déplacement de curseur devient facile à tenir, y compris hors corpus Logo. **Risque** : mémoire (10⁴ nœuds × 8 × 5 modèles × 8 octets = 3,2 Mo, acceptable) et rigueur des invalidations ; un test compare la relance incrémentale à un calcul complet, au bit près.

### R8. Filtre bilatéral existant (`denoise.rs`, §14.2)

Le code actuel calcule une exponentielle par voisin (jusqu'à 49 par pixel), teste les bornes dans la boucle interne et appelle `powf` par pixel en sortie.

- **Poids de couleur par table** : exp(−d²·inv_c) tabulé sur d² quantifié (1024 entrées jusqu'à 9σ_c², 0 au-delà). Déterministe, et plus aucune exponentielle par pixel.
- **Intérieur et bords séparés** : la boucle intérieure sans test de bornes, les bords dans une boucle à part.
- **Tuiles plates** (carte de R2) : sortie = entrée.
- **Encodage sRGB sans `powf`** : une table de 255 seuils (valeur linéaire du milieu entre deux codes) et une recherche (table grossière indexée par les bits de poids fort, puis une ou deux comparaisons). Le résultat est l'arrondi exact, sans fonction transcendante, donc identique sur toutes les cibles.

Gain attendu : de l'ordre de 5 à 10 fois sur ce filtre (1 G opérations aujourd'hui, §14.2).

### R9. Structures et allocations

- **Tableaux plats au lieu de `Vec<Vec<_>>`.** Le moteur actuel en contient une vingtaine (`planar.rs`, `fit.rs`, `palette.rs`). Forme CSR : un tableau de données et un tableau de décalages `u32`. Les chaînes de bords deviennent des plages (début, longueur) dans les tableaux de points, **permutés une fois** dans l'ordre des chaînes après le chaînage : toutes les passes suivantes sont séquentielles.
- **Remplacer les `HashMap` de `std`** (`planar.rs`, `align.rs`, `repeat.rs`), déjà proscrites par le §14.4 : pour les clés de coins de pixels (x, y), un tableau dense indexé par y·(w + 1) + x ou un `Vec` trié avec recherche dichotomique ; pour les seaux de tailles, tri puis parcours par groupes.
- **Tampons de travail réutilisés.** Une structure `Scratch` passée en `&mut` aux étapes G–H (tables de la programmation dynamique, matrices à bande, jacobiens), vidée par `clear()` entre frontières. L'allocateur WASM est lent et la mémoire ne se rend pas : chaque pic de fragmentation est permanent.
- **Deux passes pour les grands tableaux** : compter les points de bord par bande, puis allouer exactement. Un `Vec` qui double copie son contenu et laisse jusqu'à 3 fois la taille en pic permanent.
- **Petits solveurs à taille fixe** : `[[f64; N]; N]` par génériques constants pour N ≤ 6 (droite, cercle, ellipse), sans allocation ; Cholesky à bande sur un `Vec<f64>` plat n × largeur, en place.
- **Index spatial des points de bord** : les points sortent des bandes déjà triés par (y, x). Un tableau de débuts de ligne suffit pour le voisinage 5×5 du chaînage (curseurs sur les lignes y − 2 à y + 2), à la place d'un plan d'indices de 8 Mo.

### R10. Réglages de compilation WASM et pièges de déterminisme (§14.4)

- `+simd128`, `+bulk-memory` (copies et remplissages natifs), `+nontrapping-fptoint` (sans lui, chaque conversion flottant → entier `as` est encadrée de tests), `+sign-ext` ; `opt-level = 3`, `codegen-units = 1`, LTO, `panic = "abort"`. Tous sont pris en charge par les navigateurs actuels.
- En WASM, `f64` scalaire coûte autant que `f32` : le choix « f64 hors des plans » du §2 est gratuit ; seule la largeur SIMD des plans compte.
- **`usize` vaut 32 bits en WASM et 64 en natif.** Tout calcul qui peut déborder ou dont le résultat dépend de la largeur (hachage fixe, PCG32, produits d'indices) doit utiliser `u32`/`u64` explicites. Point à vérifier : `sort_unstable` a utilisé des choix de pivot dépendant de la largeur du mot ; avec une clé totale et un indice de départage (déjà exigés au §14.4) le résultat est le même, mais la règle doit être « jamais de tri sur une clé non unique », y compris avec `sort_unstable`.
- Cible native : x86-64 et AArch64 seulement (pas de x87, dont la précision étendue casserait l'égalité).

### R11. Petits gains ciblés

- **Cercle de Pratt/Taubin par Newton** sur le polynôme caractéristique (méthode de Chernov : départ en 0, convergence monotone, nombre d'itérations fixe) au lieu d'un problème propre généralisé 4×4 par portée (§10.1).
- **Seuils χ² et quantiles normaux** : calculés une fois par (C, p) ou par n_eff quantifié, et mis en table ; z_S ne se recalcule pas par portée.
- **Contraste et ΔE_min seulement aux points de bord** (avec M1) : deux conversions Oklab par point au lieu de trois plans.

---

## 3. Liste restreinte, classée

| Rang | Idée | Gain attendu | Coût | Phase |
|---|---|---|---|---|
| 1 | **M1** — détection en espace à variance stabilisée par table de 256 entrées ; bruit estimé en entiers | 100–300 ms (scalaire, 2 MP) et 20–30 Mo en moins ; jacobien et blanchiment par pixel supprimés ; détection et localisation dans le même espace | faible à écrire, mais c'est un choix d'architecture : à valider sur les cibles C avant P2 | P1 |
| 2 | **R3 + R4** — filtres en axpy décalés avec tampon circulaire ; pyramide en une passe | SIMD ×2 à ×4 sur les filtres dès P1, sans second chemin de code ni test d'égalité ; ~10 Mo de bande en moins | faible | P1 |
| 3 | **R1 + R2 + M3** — NMS sans `atan2`/racine, tuiles plates sautées de façon exacte, loi fermée de λ+ | étapes B–C ÷ 3 à 5 sur logos et illustrations ; une table simulée en moins | faible à moyen | P2 |
| 4 | **M2** — facteur de seuil mesuré sur la statistique de détection, par niveau de pyramide | seuils justes sous bruit corrélé (JPEG, agrandissements) ; réduit le risque 1 du §17 | faible ; biais possible sur images entièrement texturées | P1–P2 |
| 5 | **M4** — Devernay sur le logarithme, flou par point à une seule échelle | biais de localisation nul sur marche gaussienne ; σ_b gratuit et recoupé | très faible ; repli nécessaire à faible SNR | P2 |
| 6 | **R6 + R7** — moments par blocs alignés sur les nœuds ; mémo des coûts et frontières sales | perte de chiffres supprimée par construction ; changement de Géométrie quasi instantané, cible 300 ms tenue largement | moyen (invalidations à tester au bit près) | P5a |
| 7 | **M5 + M6** — n_eff à portée finie ; bornes de Jensen pour la zone morte | décisions correctes sur portées courtes (petit texte, encoches) ; programmation dynamique exacte, constante 1/3 supprimée | faible ; gain de M6 incertain à forte Échelle | P5a |
| 8 | **R8 + M11** — filtre bilatéral par tables, force indexée sur le bruit mesuré | filtre ×5 à ×10 ; deux constantes réglées à la main en moins | faible | P7 (R8 possible dès P0) |

Idées gardées en réserve, à prototyper avant décision : **M7** (terme d'Occam unifié, le plus structurant mais le plus incertain, P5b), **M8** (a priori tirés du corpus, P6a–P8), **M9** (Sampson partout, P5b), **M10** (expérience sur la loi de complexité locale, P7), **R5** (profils sur l'axe dominant, P2, à comparer sur les petits cercles).
