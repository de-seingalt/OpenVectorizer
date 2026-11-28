# 04 — Segmenting boundaries into primitives & fitting smooth curves

Scope: the maths that turns one closed boundary (sequence of boundary points from a colour region) into a G1 chain of {line, circular arc, elliptic arc, cubic Bézier} with explicit corners, snapped to detected guides and globally regularized. Everything here is implementable from scratch in Rust (only small dense linear algebra needed: 2x2..~10x10 solves, 3x3 symmetric eigen, sparse/banded Cholesky for the global stage).

Verification legend: [V] = verified by web lookup this session; [M] = from memory / standard reference, check before quoting.

---

## 0. Recommended pipeline (summary)

1. Boundary extraction as *crack edges* (pixel-corner lattice, Potrace style) or as 0.5-iso-contour of a fractional coverage map when anti-aliasing is present (§3.3).
2. Per-point tangent/curvature estimate on the digital curve (λ-MST tangents, integral-invariant curvature) (§3.2).
3. Corner candidates = curvature/tangent-turn extrema + guide corners from the raster detector.
4. Optimal segmentation by DP over breakpoints, cost = Σ residual/σ² + penalty(model) (MDL/BIC/G-AIC), models = line | circle arc | ellipse arc | cubic (§1).
5. Join classification: G1 vs corner by model selection (constrained vs unconstrained fit) (§1.5).
6. Joint refinement of the whole chain with G1 constraints at smooth joins (Gauss–Newton/LM with KKT or parameter elimination) (§2.4).
7. Global regularization: discover candidate relations (parallel, collinear, equal radius, concentric, symmetric, axis-aligned, snap to guide), accept greedily under the same MDL criterion, re-solve constrained LS (§5).
8. Emit SVG: lines/arcs native (`L`, `A`), or arcs converted to cubics with known error (§2.6).

---

## 1. Optimal segmentation of a digital curve

### 1.1 Problem statement
Points p_0..p_{N-1} (closed: indices mod N). Choose breakpoints b_0<b_1<...<b_K and a model m_k per piece [b_k, b_{k+1}] minimizing

  E = Σ_k [ R(b_k, b_{k+1}, m_k) / σ² + λ · c(m_k) ] + Σ_joins J(join type)

R = sum of squared orthogonal residuals of the best fit of model m on the span; c(m) = number of free parameters; σ = pixel noise (≈ 0.3–0.5 px for crack-edge boundaries after smoothing; quantization noise of a uniform ±0.5 px error is 1/√12 ≈ 0.29 px).

### 1.2 Dynamic programming over breakpoints
Open curve: F[0]=0; F[j] = min_{i<j, m} F[i] + cost(i,j,m). Back-pointers recover the segmentation. Complexity O(N² · M · T_fit) with M models; with T_fit=O(1) via prefix sums this is O(N²M).

- **O(1) line cost via prefix sums** (Potrace trick, [V] Selinger 2003): store cumulative Σx, Σy, Σx², Σxy, Σy². For span (i,j) get n, means, covariance S; the orthogonal LS line residual is the smallest eigenvalue of the 2x2 scatter matrix: R_line = n·λ_min(S). Closed form λ_min = (a+c)/2 − sqrt(((a−c)/2)² + b²).
- **O(1) algebraic circle cost**: Kåsa fit needs Σx³, Σx²y, Σxy², Σy³, Σ(x²+y²)² — all prefix-summable; gives an algebraic residual good enough for DP ranking; refine geometric afterwards. Pratt/Taubin/"Hyper" (Al-Sharadqa & Chernov 2009) [M] are also moment-based (need moments up to order 4) and less biased than Kåsa on short arcs.
- **Conic (ellipse) cost**: Fitzgibbon–Pilu–Fisher 1999 direct ellipse fit [M] (minimize ||Da||² s.t. 4AC−B²=1); Halir & Flusser 1998 [M] numerically stable reduction to a 3x3 non-symmetric eigenproblem. Scatter matrix DᵀD (6x6) is prefix-summable (moments to order 4) → O(1) per span + 3x3 eigen.
- **Cubic Bézier cost**: no O(1) form; restrict DP to spans between candidate breakpoints (corner/curvature-extrema candidates, typically N_c ≈ N/5..N/20), making O(N_c²) Schneider fits affordable. Or bound span length L_max (a cubic rarely covers > ~120° of turning).
- **Pruning**: only allow spans whose max residual < τ_max (e.g. 1.0–1.5 px) — "possible segment" restriction as in Potrace; PELT (Killick, Fearnhead, Eckley 2012) [M] pruning for penalized change-point DP gives expected O(N) when cost is additive and penalty constant: drop i from the candidate set when F[i] + cost(i,j) > F[j] (+ const).
- **Closed curves**: either (a) start at a confident corner (fixed breakpoint) → open problem; (b) Potrace approach: compute for each start the minimum and try a few starts; (c) Kolesnikov & Fränti reduced-search DP [V] (cyclic optimal polygon in near O(N²)).

### 1.3 Model-selection penalties (what λ·c(m) is)
With Gaussian residuals of known σ (geometric fitting regime):
- **AIC**: R/σ² + 2k.
- **BIC / two-part MDL (Rissanen)**: R/σ² + k·ln n (n points on the span); with unknown σ: n·ln(R/n) + k·ln n.
- **Kanatani geometric AIC** [V for existence, formula M]: G-AIC = Ĵ + 2(dN + p)ε², Ĵ = residual sum of squares, d = dimension of the model manifold (curve: d=1, each point carries one "position along curve" nuisance parameter), p = model parameters, ε = noise level. Asymptotics are in ε→0, not N→∞ — the right regime for vectorization (many points, small noise).
- **Kanatani geometric MDL**: G-MDL = Ĵ − (dN + p) ε² ln(ε/L)², L = a reference length (image size scale). Kanatani reports G-MDL is more conservative (prefers the degenerate model, e.g. line over circle) than G-AIC. Papers: Kanatani, "Geometric information criterion for model selection", IJCV 26(3) 1998; "Model selection for geometric fitting: geometric AIC and geometric MDL", Memoirs Fac. Eng. Okayama Univ. 2001/ IEICE 2002 (PDFs: https://iim.cs.tut.ac.jp/member/kanatani/papers/ e.g. modelj.pdf, mdlj3.pdf) [V].
- **Nested-model comparison**: line (p=2) ⊂ circle (p=3) ⊂ ellipse (p=5); cubic Bézier with fixed endpoints & tangent directions has p=2 (the two handle lengths), free cubic p=8 (shared endpoints reduce to 4–6 effective). Because the dN term is common to all models on the same span, only p and Ĵ differ per span when comparing models; but it does matter when comparing *number of pieces*: each extra breakpoint adds 1 parameter (its position), plus the join type.
- **Practical defaults**: λ = 2 (AIC-like) gives faithful but busier output; λ = ln n (BIC) or G-MDL gives simpler output. Make λ a user "simplicity" slider; also add a type prior (prefer line < arc < ellipse < Bézier by adding Δc_type, e.g. +0, +1, +3, +4) to bias toward "designer" primitives, as Hoshyari 2018 does implicitly.

### 1.4 Rosin & West
- Rosin & West, "Nonparametric segmentation of curves into various representations", IEEE PAMI 17(12):1140–1153, 1995, doi 10.1109/34.476507 [V]. Recursive splitting (polygon first, à la RDP) to build a tree of candidate breakpoints, then fit lines/circular arcs/elliptic arcs/superellipses/polynomials to each node; bottom-up replace children by parent when the parent's *significance* (ratio max-deviation / length) is better. Scale-invariant, no threshold. Kanatani's 1997 comment points out the theoretical difficulty (model comparison across representations) and proposes G-AIC; Rosin & West reply [V]. Rosin & West also "Curve segmentation and representation by superellipses", IEE Proc. VISP 142(5), 1995 [V].
- Useful idea to borrow: the significance measure `sig = L / d_max` (length over max deviation) as a threshold-free alternative to σ when noise is unknown.

### 1.5 Polygonal approximation (initialization & corner candidates)
- **Ramer–Douglas–Peucker** (Ramer 1972, Douglas & Peucker 1973): recursive split at farthest point from chord while d_max > ε. O(N log N) typical, O(N²) worst. Not optimal, sensitive to start point on closed curves. Use only as a quick initializer.
- **Perez & Vidal**, "Optimum polygonal approximation of digitized curves", Pattern Recognition Letters 15(8):743–750, 1994 [V]: DP choosing M of N vertices minimizing total squared error (min-ε problem) or min-#vertices given ε. O(MN²). Speedups: Salotti 2001 (A*), Kolesnikov & Fränti 2003 (reduced search, iterative) [V].
- **Potrace optimal polygon** [V/M]: (1) compute for each i the furthest j such that subpath i..j is "straight" (all points within 1/2 px of some line, ≥ 3 directions constraint); (2) DP minimizing (#segments, then Σ squared distance penalty) — O(N·m); (3) vertex adjustment: each vertex moved within its unit square to minimize the sum of squared distances to the two adjacent fitted lines (2x2 quadratic with box constraint).
- **Dominant points**: Teh & Chin 1989 (adaptive support region, k-cosine/curvature), Rosenfeld–Johnston 1973 k-cosine. Good for corner candidate generation; feed them to the DP rather than trusting them directly.

### 1.6 Tangent-continuous vs corner joins
At a breakpoint between pieces A and B:
1. Fit A, B independently (free join) → R_free; fit with G1 constraint (shared point + shared tangent direction) → R_G1. The G1 model has 1 fewer parameter (tangent angle). Choose corner iff (R_G1 − R_free)/σ² > λ_join (≈ 2–ln n). This is consistent with the global MDL objective.
2. Cheap screen first: turning angle Δθ between end tangents. Δθ < 10° → smooth; Δθ > 45–60° → corner (VTracer default `corner_threshold`=60° [V]); in between, model selection.
3. Potrace rule [M]: for each polygon vertex compute α from the geometry of the vertex and its neighbours (α = 4γ/3, with γ derived from the distance of the vertex to the line joining adjacent edge midpoints relative to the unit-square constraint); α > alphamax (default 1.0; 0 = all corners, 4/3 = no corners) → corner; otherwise a smooth cubic with α clamped to [0.55, 1].
4. Guide snapping overrides: a corner detected on the raster (from the guide detector) forces a breakpoint and the corner type; a detected line/ellipse guide forces the model on the overlapping span.
5. Perceptual classifiers (Hoshyari 2018, PolyFit 2020) learn corner-ness from local features (turning angles at several scales, segment lengths, convexity) with a random forest trained on human annotations; useful features to hand-craft: multi-scale turning angle, length of incident straight runs, whether the corner lies on a pixel-staircase pattern (1:n steps) — staircase "corners" of a digital straight line are *never* corners.

---

## 2. Bézier curve fitting

Cubic: Q(t) = Σ_{i=0..3} B_i(t) P_i, B_i = C(3,i) t^i (1−t)^{3−i}.
Q'(t) = 3Σ_{i=0..2} B^2_i(t)(P_{i+1}−P_i), Q''(t) = 6Σ_{i=0..1} B^1_i(t)(P_{i+2}−2P_{i+1}+P_i).
End curvature: κ(0) = (2/3)·|(P1−P0)×(P2−P1)| / |P1−P0|³.

### 2.1 Schneider's algorithm
Philip J. Schneider, "An Algorithm for Automatically Fitting Digitized Curves", Graphics Gems I (1990), pp. 612–626 [M]. Reference code (FitCurves.c) is in the Graphics Gems repository (license: "Graphics Gems" code is free to use without restriction [M]). Steps:
1. Estimate unit end tangents t̂1 (at d_0, pointing in) and t̂2 (at d_n, pointing back), from neighbouring points (better: from λ-MST tangent estimate, §3.2, or from guide).
2. Chord-length parameters u_i = Σ_{k≤i}|d_k − d_{k−1}| / total length.
3. With P0=d_0, P3=d_n, P1 = P0 + α1 t̂1, P2 = P3 + α2 t̂2, solve the 2x2 normal equations for α1, α2:
   A_{i1} = t̂1 B1(u_i), A_{i2} = t̂2 B2(u_i)
   C11=ΣA1·A1, C12=ΣA1·A2, C22=ΣA2·A2
   tmp_i = d_i − [P0(B0+B1) + P3(B2+B3)](u_i)
   X1 = Σ tmp_i·A1, X2 = Σ tmp_i·A2
   α1 = (X1 C22 − X2 C12)/det, α2 = (C11 X2 − C12 X1)/det, det = C11C22−C12².
   Fallback (Wu–Barsky heuristic) if |det| tiny or α ≤ ε·seglen: α1=α2=|P3−P0|/3.
4. Max error e_max = max |Q(u_i) − d_i|² and split index. If e_max < tol²: accept.
5. If e_max < ψ (Schneider uses ψ = 4·tol² — "iteration error"), reparametrize by Newton–Raphson and refit, up to 4 iterations:
   u ← u − [(Q(u)−d)·Q'(u)] / [Q'(u)·Q'(u) + (Q(u)−d)·Q''(u)].
6. Else split at the max-error point, centre tangent t̂c = normalize(d_{s−1} − d_{s+1}), recurse on both halves (G1 at split by construction).
Complexity O(N·iters) per leaf, O(N log N) typical. Weaknesses: greedy splitting (not optimal number of pieces), split point at max error is poor (better: split at a curvature extremum or use the DP of §1), Newton on noisy data may push u out of [0,1] (clamp & keep monotone).

Rust/MIT reference: Raph Levien's `kurbo` (MIT/Apache-2.0) has `fit_to_bezpath` / `fit_to_bezpath_opt` which fit a cubic with given end tangents by matching *signed area and first moment* (closed form → quartic), and an optimal-subdivision variant; Levien's blog "Fitting cubic Bézier curves" (2021, raphlinus.github.io) [M]. Ideal reference for a pure-Rust re-implementation (do not depend on it, but the algorithm is permissively licensed).

### 2.2 Parametrization
- Uniform: u_i = i/n (bad for uneven samples).
- Chord-length: Δu ∝ |Δd|.
- Centripetal (Lee 1989): Δu ∝ |Δd|^{1/2}; better for sharp turns, avoids cusps/loops.
- **Hoschek intrinsic parametrization / parameter correction** (Hoschek, "Intrinsic parametrization for approximation", CAGD 5(1):27–31, 1988 [M]): after each fit, project each d_i to its foot point: Δu_i = (d_i − Q(u_i))·Q'(u_i) / |Q'(u_i)|² (one Gauss–Newton step of point projection; Newton version above includes Q''). Alternate fit ↔ correct. This is the "point distance minimization" (PDM) scheme: linear convergence, can be slow.
- **Orthogonal distance fitting (ODF)** (Ahn, "Least squares orthogonal distance fitting of curves and surfaces in space", LNCS 3151, 2004 [M]): treat u_i as unknowns jointly with control points; Gauss–Newton/LM on the full (2N) residual vector; block structure: per-point 1 unknown → eliminate via Schur complement, keeping a small dense system in control points. Quadratic convergence.

### 2.3 Squared distance minimization (Pottmann / Wang)
Wang, Pottmann, Liu, "Fitting B-spline curves to point clouds by curvature-based squared distance minimization", ACM TOG 25(2):214–238, 2006, doi 10.1145/1138450.1138453 [V]. PDF: https://www.geometrie.tuwien.ac.at/ig/publications/oldpub/2006/wpl_curves_06/wpl_curves_06.html [V]. Earlier: Pottmann & Leopoldseder 2003; HKU TR-2004-11 (planar) [V].
For data point X with foot point P(t_k) on current curve, Frenet frame (T, N), signed distance d (d<0 when X is on the side opposite the curvature centre), curvature radius ρ:
  e_SD(X) = d/(d−ρ) · [(P(t_k)+δ − X)·T]² + [(P(t_k)+δ − X)·N]²   if d < 0
  e_SD(X) = [(P(t_k)+δ − X)·N]²                                     if 0 ≤ d < ρ
(the second branch is the tangent-distance TDM term; using a small positive weight on the T term for stability is common.) Minimize Σ e_SD + λ_s·smoothness over control points → linear system per iteration; recompute foot points. Quasi-Newton convergence, much faster and more robust than PDM, more stable than TDM near high curvature. Strongly recommended for the refinement step of Bézier pieces (and works unchanged for a chain of cubics with shared control points = G1/C1 B-spline-like chain).

### 2.4 Constrained fitting across joins (G1/G2)
Parametrize the chain so constraints are built in where possible (parameter elimination beats Lagrange multipliers for robustness):
- **Fixed end tangents** (from guide or neighbouring line/arc): unknowns α1, α2 only → Schneider's 2x2 system (linear). Enforce α ≥ 0 (if solution negative, clamp & refit other, or flip to the heuristic).
- **G1 join between Bézier A and B**: shared point J and direction angle φ: P2^A = J − β_A·(cos φ, sin φ), P1^B = J + β_B·(cos φ, sin φ). For fixed φ the problem is linear in (J, β_A, β_B, other controls) — so either (a) Gauss–Newton over φ too (nonlinear), or (b) alternate: solve linear LS with φ fixed, update φ from the bisector of the new free tangents. C1 (stronger, linear): β_A = β_B·(ℓ_A/ℓ_B) style constant ratio → P2^A, J, P1^B collinear with fixed ratio; fully linear system — a good first pass, then relax to G1.
- **G1 join Bézier ↔ line**: P1^B = J + β·u_line where u_line is the line direction and J lies on the line: J = O + s·u_line → unknowns (s, β) linear given the line.
- **G1 join Bézier ↔ circle arc**: J = C + r(cos ψ, sin ψ), tangent = (−sin ψ, cos ψ): nonlinear in ψ; Gauss–Newton over ψ, r, C.
- **Line ↔ arc tangency** (classic "fillet"): distance(C, line) = r — a single scalar equality constraint; with line in normal form n·x = ρ, constraint n·C − ρ = ±r.
- **G2 (curvature continuity)**: κ_A(1) = κ_B(0), with κ at end = (2/3)|(P1−P0)×(P2−P1)|/|P1−P0|³. Nonlinear; impose via penalty or KKT in the joint Gauss–Newton. For a designer look G1 usually suffices; G2 matters on line→curve transitions (visible "kink" in highlights). Alternative: Euler-spiral/clothoid splines (Levien, PhD thesis "From Spiral to Spline", UC Berkeley 2009 [M]; libspiro is GPL [M] — flag).
- **Joint solve** (all pieces at once): stack residuals r(x) (orthogonal distances per point, via SDM or foot-point linearization), constraints c(x)=0; Gauss–Newton KKT step:
  [JᵀJ + μI   Cᵀ] [δ]   [−Jᵀr]
  [C          0 ] [ν] = [−c  ]
  (μ = LM damping). The matrix is block-banded along the chain → banded/sparse Cholesky or Schur on the constraint block; O(K) per iteration for K pieces.

### 2.5 Circle/ellipse fitting (for arc primitives)
Algebraic init (Kåsa / Taubin / Hyper; Fitzgibbon / Halir–Flusser for ellipses) → geometric LM refinement: circle residual r_i = |d_i − C| − r (trivial Jacobian); ellipse residual via orthogonal foot point (Eberly's robust distance-to-ellipse root finding, or Ahn ODF). Short arcs (< 60°) are ill-conditioned: prefer arcs only when the subtended angle > ~30° and when MDL prefers them; otherwise line or Bézier.

### 2.6 Arc → cubic Bézier conversion & error bounds
Circular arc of radius r, sweep θ, centred at origin from angle 0: P0=(r,0), P1=(r, kr), P2=(r(cos θ + k sin θ), r(sin θ − k cos θ)), P3=(r cos θ, r sin θ), with
  k = (4/3)·tan(θ/4)   (endpoint- and midpoint-interpolating; for θ = 90°, k ≈ 0.552285).
Radial error is O(θ⁶): for θ = 90° max ≈ 2.7×10⁻⁴·r (always outward); optimizing k (≈0.551915 for 90°) gives equioscillating error ≈ ±1.96×10⁻⁴·r [M]. Halving θ divides error by ~64. Rule: split into ceil(θ/90°) pieces (≤ 0.03 px at r = 100 px); for higher precision use θ ≤ 45° (≈ 4×10⁻⁶ r). References: Goldapp, "Approximation of circular arcs by cubic polynomials", CAGD 8, 1991 [M]; Dokken et al. 1990 [M]; L. Maisonobe, "Drawing an elliptical arc using polylines, quadratic or cubic Bézier curves" (2003, spaceroots.org) gives error estimates for ellipses [M].
Ellipse arcs: Bézier curves are affine-invariant → approximate the arc on the unit circle in the ellipse's parametric angle η, then apply x = C + a cos η·u + b sin η·v; error ≤ a·e_circle(Δη) (a = semi-major). SVG `A` command exists natively, so emit arcs directly unless the consumer needs cubics only; SVG arc endpoint parametrization: convert centre form → (rx, ry, x-axis-rotation, large-arc-flag, sweep-flag, end point) per SVG spec appendix F.6.

---

## 3. Smoothing digital boundaries before fitting

### 3.1 Gaussian smoothing and shrinkage
Convolving coordinates x(s), y(s) with g_σ shrinks curves: a circle of radius r becomes r·exp(−σ²/(2r²)) (arc-length σ). Corrections:
- Lowe 1989 ("Organization of smooth image curves at multiple scales", IJCV 3) [M]: estimate local curvature κ_σ of the smoothed curve and move each point along the normal by δ = r(1 − e^{−σ²κ²/2})/... i.e. invert the circle shrinkage formula pointwise (r = 1/κ).
- Taubin λ|μ smoothing (Taubin 1995) [M]: alternate Laplacian steps x ← x + λΔx, x ← x + μΔx with λ ≈ 0.33, μ ≈ −0.34 (μ < −λ < 0): low-pass without shrinkage. Cheap, simple — recommended.
- Mokhtarian & Mackworth curvature scale space (PAMI 1992) [M]: corners = curvature extrema that persist across σ — use for corner candidate ranking.
Caution: smoothing rounds true corners. Do NOT smooth across forced corners: split at corner candidates first, smooth each run with clamped ends (or smooth only the tangent field, not the points).

### 3.2 Discrete-geometry tangent & curvature estimators
- **Digital straight segment (DSS)**: arithmetic line set {(x,y): μ ≤ ax − by < μ + ω}, naive ω = max(|a|,|b|), standard ω = |a|+|b|. Debled-Rennesson & Reveillès 1995 [M] incremental recognition in O(1) amortized per added point. **Maximal segments** / tangential cover (Feschet & Tougne 1999 [M]; Lachaud, Vialard, de Vieilleville 2005/2007 [M]) computed in O(N) for a closed contour.
- **λ-MST tangent** (Lachaud, Vialard, de Vieilleville, "Fast, accurate and convergent tangent estimation on digital contours", Image Vision Comput. 25(10), 2007 [M]): θ(p) = Σ_i λ(e_i(p))·θ_i / Σ_i λ(e_i(p)), over maximal segments i containing p, e_i(p) ∈ [0,1] = relative position of p in segment i, λ(t) = 64(−t⁶ + 3t⁵ − 3t⁴ + t³). Multigrid convergent (O(h^{2/3})), parameter-free. Ideal for Schneider end tangents and for the corner screen.
- **Maximal segment length as corner cue**: at a corner, maximal segments are short on both sides and the tangent jumps; on a staircase line, maximal segments are long → staircase steps never produce false corners (this handles the "pixel staircase" problem directly).
- **Integral-invariant curvature** (Coeurjolly, Lachaud, Levallois, "Integral based curvature estimators in digital geometry", DGCI 2013 [M]): κ̂_R(p) = 3π/(2R) − 3A_R(p)/R³, A_R = area of shape ∩ disk(p, R) (count pixels; O(R) per point incrementally), with R ∝ h^{1/3} for convergence. Robust, simple, works directly on the region mask, no contour parametrization needed.
- Implementation reference: **DGtal** (C++, LGPL-3.0 [M]) implements DSS, maximal segments, λ-MST, integral invariants. Read for algorithms; do not link.

### 3.3 Sub-pixel boundaries from anti-aliasing
For two-colour edges with anti-aliased pixels, coverage α_p = projection of pixel colour onto the segment between colours A and B: α = ((c − A)·(B − A))/|B − A|². Then:
- Treat α as a scalar field and extract the 0.5 iso-contour by marching squares with linear interpolation along edges between pixel centres → sub-pixel polyline (accuracy ≈ 0.05–0.1 px on clean AA). Handle saddle cells using the cell mean.
- For straight/gently curved edges, partial-area-effect edge location (Trujillo-Pino et al., "Accurate subpixel edge location based on partial area effect", IVC 31, 2013 [M]) fits a line/parabola per 3x3–5x3 window from the area intercepted.
- Multi-colour junctions: compute membership per region (softmax of colour distances or barycentric coordinates in colour space among the 2–3 local palette colours) and contour each membership at 0.5; enforce consistency (shared boundary between two regions is extracted once and used by both — essential for gap-free SVG).
- With AA, σ in §1 shrinks to ≈ 0.1 px: reduce smoothing, keep λ penalties the same (MDL then naturally allows finer detail).

---

## 4. Image vectorization literature for clean shapes

- **Potrace** — Peter Selinger, "Potrace: a polygon-based tracing algorithm", 2003, https://potrace.sourceforge.net/potrace.pdf [V/M]. Pipeline: path decomposition (crack edges, turnpolicy minority, turdsize 2), optimal polygon (§1.5), vertex adjustment, corner/smooth decision by alphamax (default 1.0), curve optimization joining consecutive Bézier segments when error < opttolerance (default 0.2) and the joined span is convex with total turn < 179°. **License: GPL-2.0-or-later (dual commercial)** — GPL: do not copy code; the algorithm from the paper can be reimplemented.
- **VTracer** (visioncortex) — https://github.com/visioncortex/vtracer, **MIT** [V], Rust. Hierarchical clustering of colours, path walk, staircase removal, "splice" points by angle (`splice_threshold` default 45°), corners (`corner_threshold` default 60°), `segment_length` default 4 (3.5–10), `filter_speckle`, modes pixel/polygon/spline [V]. Curve fitting is a subdivision-smoothing then cubic fit — simpler than Potrace; good permissive Rust reference.
- **Kopf & Lischinski**, "Depixelizing Pixel Art", ACM TOG 30(4) (SIGGRAPH 2011) [M]: similarity graph on 8-connectivity, crossing resolution heuristics (curves, sparse pixels, islands), reshaped Voronoi cells, quadratic B-spline extraction, then optimization of node positions minimizing Σ curvature energy + positional energy (with detected 'corner patterns' kept sharp). Inkscape's "Trace Pixel Art" uses libdepixelize [M] (Inkscape is GPL-2.0-or-later; libdepixelize license to check — treat as copyleft).
- **Hoshyari, Dominici, Sheffer, Carr, Ceylan, Wang, Shen**, "Perception-Driven Semi-Structured Boundary Vectorization", ACM TOG 37(4) (SIGGRAPH 2018), doi 10.1145/3197517.3201312, https://www.cs.ubc.ca/labs/imager/tr/2018/PerceptionDrivenVectorization/ [V]. Key ideas: (i) corner detection = learned metric (random forest on human annotations) + perception-driven discontinuity analysis; (ii) boundaries modelled as piecewise smooth curves, G1 except at corners; (iii) explicit regularities — axis-alignment, parallelism, symmetry, continuation of straight segments across gaps; (iv) energy trading accuracy (distance to pixel boundary within tolerance), simplicity (few, low-curvature-variation pieces) and regularity; discrete search over corner/regularity subsets. No public code found [V]. Thesis (UBC 2019) CC BY-NC-ND [V].
- **Dominici et al.**, "PolyFit: Perception-aligned Vectorization of Raster Clip-art via Intermediate Polygonal Fitting", ACM TOG 39(4) (SIGGRAPH 2020), doi 10.1145/3386569.3392401, https://www.cs.ubc.ca/labs/imager/tr/2020/ClipArtVectorization/ [V]. Two stages: (1) optimal polygon via shortest path over candidate edges with perceptual costs (accuracy, simplicity, continuity, regularity) — essentially the §1 DP with richer costs; (2) fit smooth curves guided by the polygon, corners classified by random forest. **Code: https://github.com/dedoardo/polyfit, MIT** [V] (C++). Most relevant prior art for this project's design; read its cost terms.
- **Favreau, Lafarge, Bousseau**, "Fidelity vs. Simplicity: a Global Approach to Line Drawing Vectorization", ACM TOG 35(4) (SIGGRAPH 2016) [M]: hypergraph of stroke fragments, global energy fidelity + simplicity (#curves, degree), optimized by stochastic search; Bézier fitting per hyperedge. Also Favreau et al. "Photo2ClipArt" (SIGGRAPH Asia 2017) [M]. For line drawings: Bessmeltsev & Solomon 2019 (polyvector fields) [M].
- **Diffvg** — Li, Lukáč, Gharbi, Ragan-Kelley, "Differentiable Vector Graphics Rasterization for Editing and Learning", ACM TOG 39(6) (SIGGRAPH Asia 2020) [M]; https://github.com/BachiLi/diffvg **Apache-2.0** [V]. Optimizes control points by gradient descent of an image loss through an anti-aliased rasterizer. Relevant idea for a from-scratch implementation: a final *image-space* refinement — rasterize with analytic coverage, minimize Σ_pixels (coverage − observed α)² wrt control points (Gauss–Newton with finite-difference or analytic boundary derivatives); this directly exploits anti-aliasing (§3.3). LIVE (Ma et al. CVPR 2022) builds on it [M].
- **GlobFit** — Li, Wu, Chrysanthou, Sharf, Cohen-Or, Mitra, "GlobFit: Consistently Fitting Primitives by Discovering Global Relations", ACM TOG 30(4):52 (SIGGRAPH 2011), doi 10.1145/1964921.1964947, PDF https://geometry.stanford.edu/lgl_2024/papers/lwcscm-gfcfpdgr-11/lwcscm-gfcfpdgr-11.pdf [V]. 3D, but the scheme transfers to 2D: RANSAC primitives → detect candidate relations (orientation: parallel/orthogonal/equal angle; placement: coaxial/coplanar/collinear; equality: equal radius/length) by clustering → iterative constrained optimization in stages (orientation, then placement, then equality), each relation accepted only if the constrained fit error stays acceptable. Code (research use): https://code.google.com/archive/p/globfit [V] (check license; research-only).
- **CGAL Shape Regularization** (Bauchet, Lafarge, Anisimov, Sytov) [M]: 2D segment angle & offset regularization as a QP: minimize (1−λ)Σ(θ_i/θ_max)² + λΣ_{(i,j)∈neighbours} w_ij((θ_i − θ_j + θ⁰_ij)/(…))², |θ_i| ≤ θ_max, where θ⁰_ij is the residual angle to the nearest parallel/orthogonal; same for offsets (collinearity). From KIPPI (Bauchet & Lafarge, CVPR 2018) [M]. **CGAL is GPL-3.0-or-later (commercial dual)** — algorithm reusable, code not.
- **Inkscape** (GPL-2.0-or-later) embeds Potrace and libdepixelize; **AutoTrace** (GPL-2.0) [M] — copyleft, avoid copying.

---

## 5. Global regularization

### 5.1 Relations and their parametrizations (2D)
Per primitive parameters: line (θ, ρ) normal form n=(cos θ, sin θ), n·x=ρ, plus endpoints; circle (C, r); ellipse (C, a, b, φ); Bézier control points.
- Parallel lines i,j: θ_i = θ_j (mod π). Orthogonal: θ_i = θ_j + π/2. Axis-aligned: θ_i ∈ {0, π/2}.
- Collinear: θ_i = θ_j ∧ ρ_i = ρ_j.
- Equal length: |e_i| = |e_j|. Equal spacing of parallels: ρ_k − ρ_j = ρ_j − ρ_i.
- Concentric: C_i = C_j. Equal radii: r_i = r_j. Tangency: |n·C − ρ| = r (line–circle), |C_i − C_j| = r_i ± r_j (circle–circle).
- Reflection symmetry about axis (n_s, ρ_s): p_j = p_i − 2(n_s·p_i − ρ_s)n_s for matched points/control points (pairing via the boundary correspondence); rotational symmetry: p_j = C + R(2π/k)(p_i − C).
- Snap to guide g: primitive parameters = guide parameters (hard), or distance(primitive, guide) penalty (soft).

### 5.2 Solving
1. **Parameter elimination (preferred)**: union-find over relations to build equivalence classes ("all these lines share θ", "these arcs share C"); each class gets a single parameter. Then the problem is unconstrained nonlinear LS → LM. Robust, no multipliers, smaller system. Works for equalities that are "same value" (parallel, collinear, concentric, equal radius, axis-aligned, snap). Orthogonality → θ_j = θ_i + π/2 (offset sharing).
2. **Lagrange / KKT Gauss–Newton** for non-sharing constraints (tangency, symmetry, equal length): solve the KKT system of §2.4 each iteration; or null-space method: linearize C δ = −c, δ = δ_p + Z y with Z a basis of null(C) (QR of Cᵀ), minimize ||J(δ_p + Zy) + r||².
3. **Penalty / augmented Lagrangian**: add w·c(x)² with w increasing (×10 per outer iteration), or ALM: L = ½||r||² + νᵀc + (w/2)||c||², ν ← ν + w c. Easy to implement on top of LM; final constraint violation < 1e-6 px.
4. **Staging (GlobFit)**: solve orientation relations first (angles only, small system), then placement (offsets/centres), then equalities (radii/lengths); re-fit topology (endpoints, joins) between stages.
5. Joint with the curve chain: all constraints (G1 joins from §2.4 + regularities) go in the same KKT/penalty system; joins keep the shape closed and tangent while regularities move whole primitives.

### 5.3 Which constraints to impose
- **Candidate discovery**: cluster line angles (mean-shift / 1D histogram mod π with bandwidth 2–4°); include 0°, 90° (and 45°) as attractors; cluster offsets of parallels (tolerance ~1 px); cluster arc radii (relative 3–5%) and centres (1–2 px); symmetry axes from pairwise mid-perpendiculars of matched corners (Hough-style voting), validated by Hausdorff residual of reflected boundary.
- **Acceptance test (MDL-consistent)**: a relation removing q degrees of freedom is accepted iff ΔR/σ² < λ·q (the same λ as in §1.3) and max point deviation stays < τ_max (e.g. 1 px). This keeps segmentation and regularization under one objective; G-AIC/G-MDL give the λ.
- **Greedy order**: sort candidates by ΔR (cheapest first, computed by a quick local constrained re-fit), accept, re-solve incrementally; reject relations that conflict (e.g. inconsistent cycles of parallel/orthogonal → detect via the union-find with offset labels mod π/2). Hoshyari 2018 and PolyFit similarly treat regularities as discrete choices evaluated against accuracy/simplicity.
- **Hierarchy of trust**: guide snaps from the raster detector > axis alignment > parallel/orthogonal > equal radius/concentric > symmetry (most likely to be accidental on small shapes — require ≥ 3 matched features).
- **Cross-region consistency**: regularize shared boundaries once (each boundary edge between two colour regions is a single object referenced by both region outlines), otherwise gaps/overlaps appear.

---

## 6. Defaults cheat-sheet

| Parameter | Suggested default | Source/rationale |
|---|---|---|
| σ (noise) crack-edge / AA | 0.3–0.5 px / 0.1 px | quantization 1/√12 |
| τ_max max deviation per piece | 1.0 px (0.5 px with AA) | Potrace straightness uses 1/2 px |
| λ penalty per parameter | 2 (AIC) … ln n (BIC); user slider | §1.3 |
| Corner screen | <10° smooth, >60° corner, else model selection | VTracer corner_threshold 60° |
| Potrace alphamax / opttolerance / turdsize | 1.0 / 0.2 / 2 | Potrace defaults |
| Schneider tol / reparam iterations | 0.5–1 px / 4 (ψ = 4·tol²) | Graphics Gems |
| Arc → cubic | ≤ 90° per cubic (error 2.7e-4 r) | §2.6 |
| Min arc sweep to prefer arc | ≈ 30° | conditioning |
| Angle cluster bandwidth | 2–4° | regularization |
| Radius equality tolerance | 3–5 % | regularization |
| Taubin λ/μ | 0.33 / −0.34, 5–10 iterations | Taubin 1995 |

## 7. License flags
- GPL/copyleft — do not copy code: Potrace (GPL-2+), Inkscape (GPL-2+), AutoTrace (GPL-2), libspiro (GPL), CGAL Shape Regularization (GPL-3+), DGtal (LGPL-3), libdepixelize (check; treat as copyleft).
- Permissive — safe to study/port with attribution: VTracer (MIT), PolyFit (MIT), Diffvg (Apache-2.0), kurbo (MIT/Apache-2.0), Graphics Gems code (free use).
- Research-only: GlobFit code (check archive license).
- No code: Hoshyari 2018 (thesis CC BY-NC-ND).
