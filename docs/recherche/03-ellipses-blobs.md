# 03 — Circles, ellipses, arcs, blobs, symmetry: research digest

Scope: the feature-detection stage that outputs *guides* (circle, ellipse, arc, blob, symmetry axis, repeated shape) used to constrain the tracing of colour-region contours. Everything is re-implemented in Rust from the maths, with no external libraries. GPL/AGPL code may only be read as documentation of the algorithm. No code may be copied from it.

License status, checked 2026-11 via the GitHub API or source headers:

| Implementation | License | Use |
|---|---|---|
| OpenCV `fitEllipse`, `fitEllipseDirect`, `fitEllipseAMS`, `HoughCircles`, `SimpleBlobDetector`, `MSER` (main repo) | Apache-2.0 | may read and port |
| OpenCV contrib `ximgproc::findEllipses` (Jia 2017) and `EdgeDrawing::detectEllipses` (EDCircles) | Apache-2.0 | may read and port |
| scikit-image `EllipseModel`, `CircleModel`, `hough_ellipse`, `blob_log/dog/doh` | BSD-3 | may read and port |
| Geometric Tools (Eberly) | Boost Software License 1.0 | may read and port |
| ELSDc (viorik/ELSDc) | **AGPL-3.0** | paper only |
| AAMED (Li-Zhaoxi/AAMED) | **GPL-2.0** | paper only |
| Jia et al. (dlut-dimt/ellipse-detector) | **GPL-3.0** | paper only (the OpenCV contrib port is Apache) |
| Lu et al. (AlanLuSun/High-quality-ellipse-detection) | no license on GitHub, a Gitee mirror says GPL-2.0, "free for academic use" | paper only |
| Fornaciari reference code | not verified | treat as paper only |
| VLFeat MSER | BSD-2 | may read and port |

---

## 1. Conic fitting

### 1.1 Notation
Conic: `F(x,y) = a x² + b xy + c y² + d x + e y + f = 0`, θ = (a,b,c,d,e,f)ᵀ, ξ(x,y) = (x², xy, y², x, y, 1)ᵀ.
Design matrix D (n×6), scatter S = DᵀD. Ellipse condition: `b² − 4ac < 0`.
**Always normalise first.** Translate the points to their centroid and scale them so the RMS distance is √2 (Hartley-style). Fit, then de-normalise θ. This matters more than which algebraic method you pick.

Conversion from conic to geometric parameters (centre, semi-axes, angle):
- centre: `x0 = (2cd − be)/(b² − 4ac)`, `y0 = (2ae − bd)/(b² − 4ac)`.
- angle: `φ = ½ atan2(b, a − c)`.
- semi-axes: `A, B = −sqrt( 2(ae² + cd² − bde + (b² − 4ac) f) · (a + c ± sqrt((a − c)² + b²)) ) / (b² − 4ac)`.
- Guard against degenerate or hyperbolic results and flag them.

### 1.2 Circle algebraic fits (parameterise `A(x²+y²) + Bx + Cy + D = 0`, z = x²+y²)
- **Kåsa (1976)**, "A circle fitting procedure and its error analysis", IEEE TIM 25.
  - Linear least squares on `z + Bx + Cy + D = 0`, i.e. minimise Σ(r_i² − R²)².
  - O(n) with 3×3 normal equations.
  - **Strongly biased toward small radii on short arcs.** Acceptable only for full circles or as an initial guess.
- **Pratt (1987)**, "Direct least-squares fitting of algebraic surfaces", SIGGRAPH.
  - Minimise ΘᵀMΘ subject to ΘᵀBΘ = 1 with `B²−4AD = 1`, Θ = (A,B,C,D).
  - Generalised eigenproblem M Θ = η N Θ, where N has N[0][3] = N[3][0] = −2 and N[1][1] = N[2][2] = 1. Take the smallest positive η.
- **Taubin (1991)**, "Estimation of planar curves, surfaces and nonplanar space curves defined by implicit equations", IEEE PAMI 13(11).
  - Normalises by the mean squared gradient: constraint `4A·mean(z) + B² + C² = 1`, i.e. ΘᵀN_TΘ = 1 with N_T built from data means.
  - Very close to geometric accuracy. It is the scikit-image/Chernov default.
- **Al-Sharadqah & Chernov (2009)**, "Error analysis for circle fitting algorithms", Electronic J. Statistics 3, 886–911, https://projecteuclid.org/euclid.ejs/1251119958.
  - "**Hyper**" fit: constraint matrix `N_H = 2 N_T − N_P`.
  - Removes the essential bias, so the second-order bias of the centre and radius is zero.
  - Same cost as Pratt and Taubin. **Recommended default circle fit.**
- **Chernov–Lesort (2005)**, "Least squares fitting of circles", JMIV 23: geometric circle fit via Levenberg–Marquardt (LM) and convergence analysis. Chernov's book *Circular and Linear Regression: Fitting Circles and Lines by Least Squares* (CRC 2010) is the reference. Its MATLAB snippets (Kåsa, Pratt, Taubin, Hyper) are public domain-ish academic code. Re-derive them anyway.
- Practical solve of the 4×4 generalised eigenproblem. Chernov uses Newton on the characteristic polynomial: for Taubin and Pratt, a cubic/quartic in η starting from η = 0 always converges to the right root. O(n) data pass plus O(1) solve.

### 1.3 Ellipse algebraic fits
- **Fitzgibbon, Pilu, Fisher (1999)**, "Direct least square fitting of ellipses", IEEE PAMI 21(5):476–480.
  - Minimise θᵀSθ subject to `4ac − b² = 1` (θᵀCθ = 1, C has C13 = C31 = 2, C22 = −1).
  - Leads to the generalised eigenproblem Sθ = λCθ. Exactly one positive eigenvalue exists, and it is the ellipse.
  - **Always returns an ellipse**, which is a big advantage for a guide generator.
  - Biased toward smaller, rounder ellipses on short arcs.
- **Halíř & Flusser (1998)**, "Numerically stable direct least squares fitting of ellipses", WSCG, http://autotrace.sourceforge.net/WSCG98.pdf. Split D = [D1 | D2] (quadratic part and linear part).
  - Blocks: `S1 = D1ᵀD1, S2 = D1ᵀD2, S3 = D2ᵀD2`. Then `T = −S3⁻¹ S2ᵀ` and `M = C1⁻¹ (S1 + S2 T)`, where `C1 = [[0,0,2],[0,−1,0],[2,0,0]]`.
  - Solve the 3×3 eigenproblem M a1 = λ a1. Pick the eigenvector with `4 a1[0] a1[2] − a1[1]² > 0`, then a2 = T a1.
  - **This is what to implement.** It only needs a 3×3 non-symmetric eigen-solver (closed-form cubic, or QR iterations) and a 3×3 inverse. OpenCV `fitEllipseDirect` follows this (Apache).
- **Taubin ellipse** and **AMS (Approximate Mean Square, Taubin 1991)**. OpenCV `fitEllipseAMS` minimises Σ F²/Σ|∇F|². It is less biased than Fitzgibbon but can return hyperbolas on poor data.
- **Kanatani, Rangarajan (2011)**, "Hyper least squares fitting of circles and ellipses", CSDA 55(6). **HyperLS**:
  - Generalised eigenproblem Mθ = λNθ, with `M = (1/n) Σ ξ_α ξ_αᵀ` and N = Taubin term + second-order bias-correction terms built from `V0[ξ_α]`. `V0[ξ]` is the normalised covariance of ξ: for isotropic pixel noise it is a 6×6 matrix built from x, y and f0, where f0 is the scale constant ≈ image size, or 1 after normalisation.
  - Removes essential bias, non-iterative, cost similar to Taubin. May return a hyperbola on very short or noisy arcs. Use Fitzgibbon or Halíř–Flusser as a fallback.
- **Kanatani, Al-Sharadqah, Chernov, Sugaya (2012)**, "Renormalization returns: hyper-renormalization and its applications", ECCV, LNCS 7574.
  - **Hyper-renormalization** iterates with per-point weights `W_α = 1/(θᵀ V0[ξ_α] θ)` and the same bias-correcting N.
  - Converges in about 3–5 iterations. Accuracy reaches the KCR lower bound, practically equal to ML/geometric fitting, and it is more robust than ML on short arcs.
  - The Kanatani–Sugaya–Kanazawa book *Ellipse Fitting for Computer Vision* (Morgan & Claypool 2016) gives the full V0 formulas and pseudocode.
- **Ranking**:
  - Accuracy (Kanatani's experiments): Kåsa/LS < Fitzgibbon < Taubin ≈ AMS < HyperLS < hyper-renormalization ≈ ML (geometric) ≈ KCR bound.
  - Robustness to always getting an ellipse: Fitzgibbon wins.
  - **Design**: hyper-renormalization or HyperLS, with Halíř–Flusser as fallback if the result is not an ellipse. Then optionally refine geometrically.

### 1.4 Geometric (orthogonal distance) fitting
- Minimise Σ d_i², where d_i is the orthogonal distance from point i to the curve. This is the ML estimate under isotropic Gaussian noise.
- **Circle**: d_i = ‖p_i − c‖ − R, parameters (cx, cy, R). Jacobian row: `[−(x_i−cx)/r_i, −(y_i−cy)/r_i, −1]`.
  - LM converges in about 5–10 iterations from a Hyper initial guess.
  - On very short arcs it can drift to R → ∞. Chernov's reparameterisation (A,B,C,D) with `B²+C²−4AD = 1` handles the line limit gracefully, which is important for line-vs-arc decisions.
- **Ellipse**: parameters p = (cx, cy, A, B, φ). Two formulations:
  - **Ahn, Rauh, Warnecke (2001)**, "Least-squares orthogonal distances fitting of circle, sphere, ellipse, hyperbola, and parabola", Pattern Recognition 34(12). Variable projection: compute the foot point x'_i for each point, then Gauss–Newton (GN) on p with Jacobian ∂d_i/∂p from the implicit function theorem. It is fast and well-documented.
  - **Gander, Golub, Strebel (1994)**, "Least-squares fitting of circles and ellipses", BIT 34. Parametric form with per-point angles t_i as extra unknowns: `x = c + Q(φ)[A cos t_i, B sin t_i]`. LM on n+5 unknowns, using the sparse block structure, which reduces to a 5×5 system per iteration. Simple and robust.
  - Sampson approximation `d ≈ F(x)/‖∇F(x)‖` gives a cheap first-order distance for scoring and inlier tests. It is accurate when d is small relative to curvature radius.
- **Point-to-ellipse distance (Eberly)**: "Distance from a point to an ellipse, an ellipsoid, or a hyperellipsoid", Geometric Tools, 2013+, https://www.geometrictools.com/Documentation/DistancePointEllipseEllipsoid.pdf (code under the Boost license).
  1. Rotate and translate to the canonical frame, then reflect into the first quadrant (y0, y1 ≥ 0) with e0 ≥ e1.
  2. Solve `F(t) = (e0 y0/(t+e0²))² + (e1 y1/(t+e1²))² − 1 = 0` for t ∈ [−e1² + e1 y1, −e1² + sqrt(e0²y0² + e1²y1²)]. F is monotone decreasing and convex there, so **bisection** is guaranteed robust; Newton is optional.
  3. The foot point is `x_i = e_i² y_i/(t + e_i²)`.
  4. Special cases: y1 = 0 (on the major axis, with a check for whether the point is inside the evolute region `e0 y0 < e0² − e1²`) and y0 = 0.
  5. Eberly's robust variant rescales by the larger of the components to avoid overflow, and bisects on a normalised function `G(s)` with s = t/e1² + 1.
  - Cost: about 30–60 bisection steps to double precision, or about 5 Newton steps.
- **Covariance of fitted parameters**: at convergence, `Cov(p) ≈ σ̂² (JᵀJ)⁻¹` with `σ̂² = Σ d_i²/(n − k)` (k = 3 for a circle, 5 for an ellipse). For algebraic θ, Kanatani gives `V[θ] = (1/n) M⁻ (Σ W_α V0[ξ_α]) M⁻` (pseudo-inverse in the 5-D tangent space of ‖θ‖ = 1).
  - Use it for snapping decisions, e.g. "is this ellipse a circle?": test (A−B)/σ_{A−B} < 2–3.
  - Use it for merging: Mahalanobis distance between two fits.
- **Bias on short arcs**: for an arc of angular extent Δ, the variance of R grows like ~1/Δ⁴ for small Δ (Chernov). On arcs < ~60–90°, an ellipse fit is ill-posed.
  - **Practical rule**: allow a free ellipse only if the arc subtends ≥ ~120–150° or the residual clearly rejects a circle. For shorter arcs, fit a circle (3 dof) or keep a line.
  - Kåsa underestimates R, Fitzgibbon shrinks and rounds the ellipse, and Hyper or hyper-renormalization or ML are nearly unbiased.

### 1.5 Robustness
- Wrap any fit in RANSAC or MSAC.
  - Minimal sets: 3 points for a circle, 5 for an ellipse (or 3 points plus their tangent directions, using edge gradients).
  - Score: Sampson distance, inlier threshold ≈ 1–1.5 px for anti-aliased raster boundaries.
- Or use an M-estimator (IRLS with Huber or Tukey weights) inside LM.
- Points from a pixel contour are quantised. Sub-pixel boundary points from a marching-squares iso-contour on the anti-aliased colour field (or from gradient-based edge localisation) dramatically improve accuracy, giving 0.05–0.1 px RMS on clean renders.

---

## 2. Circle and ellipse detection in images

### 2.1 Hough family
- **Circle Hough Transform (CHT)** with gradient voting. Each edge pixel votes along its gradient direction for centres at distances r ∈ [rmin, rmax]. That is a 2-D accumulator for centres, then a 1-D radius histogram per centre candidate.
  - OpenCV `HOUGH_GRADIENT` does this (Yuen et al. 1990, "Comparative study of Hough transform methods for circle finding", IVC 8). `HOUGH_GRADIENT_ALT` is a more accurate variant.
  - Complexity: O(edges × radius range).
  - Good for many small dots of known radius range. Weak for ellipses.
- **Ellipse Hough**:
  - **Xie & Ji (2002)**, "A new efficient ellipse detection method", ICPR. Pick a pixel pair as major-axis endpoints, then vote a 1-D accumulator over the minor axis. O(n³) and slow, but simple; scikit-image `hough_ellipse` (BSD) implements it.
  - **Randomized Hough Transform (RHT)**: Xu, Oja, Kultanen (1990), "A new curve detection method: Randomized Hough Transform", PRL 11. McLaughlin (1998), "Randomized Hough transform: improved ellipse detection with comparison", PRL 19. Sample 3 points, use their local tangents to get the centre (intersection of the line joining the tangent intersection to the chord midpoint), then solve for the remaining 3 parameters. Accumulate in a sparse hash map.
  - **Verdict**: Hough methods are not needed as a primary detector here, because we already have closed region contours. CHT stays useful for tiny dots (< ~6 px) where contour fitting is degenerate.

### 2.2 Arc-based detectors (the right family for us)
Common pipeline:
1. Edge chains.
2. Split into smooth, convex, curvature-consistent arcs.
3. Classify arcs by quadrant or convexity.
4. Group arc pairs or triplets that can belong to one ellipse.
5. Fit.
6. Validate.

- **Prasad, Leung, Quek (2012)**, "Ellipse detection using edge curvature and convexity" ("ElliFit"/"UB-detector"), Pattern Recognition 45(9). Earlier: Prasad & Leung, "An ellipse detection method for real images", IVCNZ 2010.
  - Arcs are split at curvature-sign changes and at sharp corners.
  - A "search region" is defined by convexity and the relative position of arcs.
  - Hough-like voting over arc-pair centres, then saliency scoring.
  - Also **ElliFit**: Prasad, Leung, Quek, "ElliFit: an unconstrained, non-iterative, least squares based geometric ellipse fitting method", PR 46(5), 2013.
- **Fornaciari, Prati, Cucchiara (2014)**, "A fast and effective ellipse detector for embedded vision applications", Pattern Recognition 47(11):3693–3708.
  - Canny edges, then arcs labelled by gradient direction and convexity into 4 quadrant classes (I–IV).
  - Triplets of arcs from compatible quadrants (I-II-III, II-III-IV, ...) are kept only if they pass constraints on mutual position.
  - **Centre estimation** uses the classical *parallel-chords* property: midpoints of parallel chords of an ellipse are collinear on a line through the centre. Two arcs give one such line, and intersecting lines from arc pairs gives the centre.
  - Remaining parameters (N = ratio of axes, ρ = orientation, A) are found by 1-D Hough accumulations, which are cheap.
  - Validation: fraction of ellipse perimeter supported by edge pixels with consistent gradient.
  - Real-time: about 10 ms at 640×480 in 2014.
  - **Recommended blueprint for our arc-grouping step.**
- **Jia, Fan, Luo, Song, Zhang, Wang, Li (2017)**, "A fast ellipse detector using projective invariant pruning", IEEE TIP 26(8):3665–3679, arXiv:1608.07470.
  - Builds on Fornaciari. Adds a **characteristic number (CN)** test, a projective invariant from Menelaus-type products along the triangle formed by arc tangent lines.
  - CN = −1 for collinear points and CN = 1 for six points on a conic. This prunes arc-triplets that cannot be on one conic, very cheaply, before any fitting.
  - Faster and more precise than Fornaciari.
  - The OpenCV contrib port `ximgproc::findEllipses` is Apache-2.0 and can be read and ported. The original repo is GPL-3.0.
- **Lu, Xia, Ying, Wu (2019/2020)**, "Arc-support line segments revisited: an efficient high-quality ellipse detection", IEEE TIP 29, arXiv:1810.03243.
  - Uses LSD line segments with "arc-support" (polarity and curvature consistency).
  - Local and global grouping, with initial ellipse candidates from pairs.
  - Superpixel-free clustering of candidates in parameter space.
  - Validation via angular coverage and polarity.
  - High precision. Code has no clear license or is GPL-2.0, so paper only.
- **Meng, Li, Bai, Zhou (2020)**, "Arc adjacency matrix-based fast ellipse detection" (**AAMED**), IEEE TIP 29:4406–4420.
  - Contours are approximated by polygons (digital-curve corners), arcs are found from the polygon vertices, and an *arc adjacency matrix* encodes which arcs can connect.
  - A depth-first search over the matrix builds arc combinations that satisfy convexity and "polar" constraints.
  - Fast ellipse validation uses precomputed cumulative tables and an adaptive threshold.
  - Very fast (a few ms per image). GPL-2.0, so paper only.
- **Pătrăucean, Gurdjos, Grompone von Gioi (2012 / 2017)**: "A parameterless line segment and elliptical arc detector with enhanced ellipse fitting", ECCV 2012 (**ELSD**). Then "Joint a contrario ellipse and line detection" (**ELSDc**), IEEE PAMI 39(4), 2017.
  - LSD-style region growing on gradient orientation into "curved regions", then a circle or ellipse fit to the region pixels.
  - The fit uses **gradient-constrained algebraic fitting** (points plus normals; Pătrăucean's thesis uses a constraint that the conic gradient aligns with the image gradient), which is better on short arcs.
  - Model selection between line, circular arc and elliptical arc is done **a-contrario**: each candidate gets an NFA and the most meaningful model wins.
  - The code is **AGPL-3.0, so paper only**. The thesis is at https://theses.hal.science (Pătrăucean, "Detection and identification of elliptical structure arrangements in images", 2012).
- **EDCircles**: Akinlar & Topal (2013), "EDCircles: a real-time circle detector with a false detection control", Pattern Recognition 46(3).
  - Edge Drawing chains, then line segments, then arcs, then a join step, then circle/ellipse fits, then a Helmholtz/NFA validation.
  - Parameter-free and very fast.
  - Implemented in OpenCV contrib `EdgeDrawing::detectEllipses` (Apache), which is also a readable reference for NFA validation.

### 2.3 Validation by NFA (a-contrario, Desolneux–Moisan–Morel)
- Null model: gradient orientations are i.i.d. uniform.
- For a candidate curve with n sample pixels on the ring of width w, count k pixels whose gradient direction is aligned with the curve normal within precision p (typically p = 1/8, i.e. ±22.5°).
- `NFA = N_tests · B(n, k, p)`, with `B(n,k,p) = Σ_{j=k..n} C(n,j) p^j (1−p)^{n−j}`, the binomial tail. Compute it with log-gamma or the regularised incomplete beta function, which is easy to re-implement.
- N_tests: about (NM)^5 for ellipses, (NM)^3 for circles and (NM)^2 for lines in an N×M image (ELSDc uses refinements including the number of rectangle widths).
- **Accept if NFA < ε = 1.** Parameter-free, and false-detection-controlled.
- For our use, where the input is colour-region contours rather than raw edges, the same scheme works on the region boundary. Count "aligned" boundary samples within distance ≤ 1 px of the fitted curve and with the region gradient aligned to the normal.
- **Model comparison**: ELSDc picks the model with the smallest NFA, which automatically penalises extra parameters through N_tests.

### 2.4 Grouping arcs into one ellipse (key operations to implement)
1. **Arc extraction**:
   - Split each contour at corners (curvature extrema above a threshold, or polygonal-approximation vertices with turning angle > ~40°) and at inflections (curvature sign change).
   - Each resulting piece is convex.
2. **Per-arc fit**: Hyper circle fit, plus an ellipse fit if the arc spans enough angle. Keep the parameter covariance.
3. **Candidate pairing**:
   - Arcs must have compatible convexity (both turning toward a common interior).
   - Centre estimates must agree: Mahalanobis distance from the parameter covariance, or the parallel-chord centre test.
   - CN test on triplets if the budget allows.
4. **Joint fit** on the union of points, re-validated by NFA or by residual.
5. **Greedy maximal grouping** with a union–find over arcs and duplicate suppression: merge ellipses whose parameters agree within ~1–2σ, or whose IoU is > 0.9.
- In our vectorizer this is mostly *within a region boundary* (a circle cut by an occluding region becomes 2+ arcs of the same boundary or of neighbouring boundaries). The O(arcs²) pairing is cheap because the arc count is small, and a spatial grid on centres prunes it further.

---

## 3. Partial arcs and model selection (line, arc, ellipse, free curve)

- **Problem**: given a contour piece of n points, choose among:

  | Model | Degrees of freedom (k) |
  |---|---|
  | line segment | 2 (+2 endpoints, nuisance) |
  | circular arc | 3 |
  | elliptic arc | 5 |
  | free curve (cubic Bézier spline) | 4m+2 for m segments |

- **Information criteria** (σ = noise level, RSS = Σ d_i² orthogonal residuals):
  - AIC = RSS/σ² + 2k
  - BIC = RSS/σ² + k ln n
  - MDL (Rissanen) ≈ RSS/(2σ²) + (k/2) ln n, plus parameter-precision terms. In practice it behaves like BIC.
- **Kanatani's geometric AIC (G-AIC) and geometric MDL (G-MDL)**: Kanatani (1998), "Geometric information criterion for model selection", IJCV 26(3):171–189; Kanatani (2004), "Uncertainty modeling and model selection for geometric inference", IEEE PAMI 26(10).
  - In geometric fitting the n data points each also carry a "position on the manifold" nuisance parameter. For a model manifold of dimension d (d = 1 for curves) and codimension r (r = 1 in 2-D), with k model parameters:
    - G-AIC = J/σ² + 2(nd + k)
    - G-MDL = J/σ² − (nd + k) ln(σ/L)², where L is the reference length scale (image size).
  - Since nd is common to all curve models, the comparison reduces to J/σ² + 2k for G-AIC. The asymptotics are taken in σ → 0, not n → ∞, which matches the vectorization regime (many points, small noise).
  - G-MDL penalises more strongly and is preferred for choosing the *simplest* model (Kanatani recommends it for degeneracy detection, e.g. "is this ellipse actually a circle or a line?").
- **σ estimation**: the a-priori noise for anti-aliased sub-pixel contours is ≈ 0.1–0.3 px. Better: estimate σ from the most general model's residuals, `σ̂² = J_general/(r n − k_general)`.
- **Practical decision cascade** for each convex contour piece (default thresholds):
  1. Line fit. If the max orthogonal deviation is < 0.5 px (or G-MDL prefers it), call it a LINE.
  2. Circle fit (Hyper, then LM). Require all of:
     - G-AIC/G-MDL prefers it over the line;
     - RMS residual < 0.3–0.5 px and max residual < 1 px;
     - the arc subtends ≥ ~25–30° (otherwise it is indistinguishable from a line or a shallow free curve);
     - R < ~10× the image diagonal.
  3. Ellipse fit (hyper-renormalization, then ODF). Accept only if:
     - the arc spans ≥ ~120°, or the circle is rejected by G-MDL (ΔJ/σ² > 2·2 = 4 for G-AIC, or > 2 ln(L/σ)² for G-MDL);
     - the axis ratio is within [0.05, 1].
  4. Otherwise, a FREE curve (Bézier fitting, a separate topic).
  5. **Snap**: if the ellipse's (A−B) is within 2σ of 0, make it a circle. If φ is within 2σ of 0° or 90°, make it axis-aligned (designers' intent).
- **Residual run test**: in addition to RSS, apply a runs test on the sign sequence of residuals. Systematic residual patterns (few long runs) reveal model misfit even when the RSS is small, for example a superellipse or rounded rectangle wrongly fitted as an ellipse. Cheap and powerful.
- **Curvature-profile approach** (alternative or prefilter):
  - Compute the curvature κ(s) along the contour with a Gaussian-smoothed derivative (σ_s ≈ 2–3 px).
  - Line ⇔ κ ≈ 0. Circular arc ⇔ κ ≈ const ≠ 0. Ellipse ⇔ κ sinusoidal-like with period π in the parameter.
  - Segment κ(s) piecewise-constant with a dynamic programme or the Ramer–Douglas–Peucker algorithm in (s, θ(s)) tangent-angle space: lines are horizontal segments and arcs are sloped straight segments in the θ(s) plot.
  - **The tangent-angle plot θ(s) is the classic, robust way to find arc breakpoints**, e.g. Rosin & West (1995), "Nonparametric segmentation of curves into various representations", IEEE PAMI 17(12). It segments curves into lines, arcs and ellipses with a significance measure. That paper is a direct ancestor of what we need; read it.
- **Rosin's other relevant work**:
  - Rosin (1993), "A note on the least squares fitting of ellipses", PRL 14. Compares normalisations.
  - Rosin & West (1992), "Detection and verification of surfaces of revolution / circular arcs".
  - Rosin (1999), "Measuring shape: ellipticity, rectangularity, and triangularity", MVA. Gives moment-based ellipticity measures, a cheap "is this blob round?" test.

---

## 4. Blob detection

### 4.1 Scale-space blobs (Lindeberg)
- **Lindeberg (1998)**, "Feature detection with automatic scale selection", IJCV 30(2):79–116. Also Lindeberg (2013), "Image matching using generalized scale-space interest points", JMIV.
- **Scale space**: `L(x,y;t) = g(·;t) * I`, with t = σ². Use a discrete Gaussian, or sampled Gaussians with σ ≥ 0.7 to avoid aliasing; Lindeberg recommends the discrete analogue using modified Bessel functions.
- **Scale-normalised LoG**: `∇²_norm L = t (L_xx + L_yy)`. Blobs are local extrema in (x, y, t) over a 3×3×3 neighbourhood.
  - For a disc of radius r (bright on dark, or the reverse), the extremum is at σ = r/√2.
  - This gives a *radius estimate for free*.
- **DoG**: `L(t_{i+1}) − L(t_i) ≈ (k−1) t ∇²L`, with k = 2^{1/s} and s = 3 scales per octave (Lowe 2004, "Distinctive image features from scale-invariant keypoints", IJCV 60). Cheaper because it reuses the Gaussian pyramid.
- **Determinant of Hessian (DoH)**: `det H_norm = t² (L_xx L_yy − L_xy²)`. It responds to blobs, not edges or ridges, so it is more selective than LoG and better for elongated shapes. SURF approximates it with box filters on an integral image.
  - Edge rejection for LoG/DoG: require `tr(H)²/det(H) < (r+1)²/r` with r = 10 (Lowe).
- **Practical defaults**, as in scikit-image `blob_log` (BSD):
  - min_sigma = 1, max_sigma = 30–50, num_sigma = 10–15 (log-spaced);
  - threshold ≈ 0.1 × dynamic range (normalised image);
  - overlap pruning: if two blobs overlap by more than 0.5 of the smaller area, drop the weaker.
  - Sub-pixel/sub-scale refinement: quadratic interpolation (3-D Taylor, Lowe) of the extremum. This gives centre accuracy ~0.1 px and radius ~5%.
- **Complexity**: O(N · num_scales · kernel). Use separable Gaussians and recursive Gaussian (Young–van Vliet 1995 or Deriche) for O(N) per scale independent of σ, and a pyramid for large σ.
- **Use for the vectorizer**:
  - Detects **small dots and holes** (eyes, rivets, bullet points, dotted textures) that colour segmentation may merge, drop or fragment. A LoG extremum gives a seed point and a radius, so the segmenter can keep a small region and the tracer can snap its contour to a circle.
  - Run it on each Lab channel or on the colour-gradient magnitude. A blob in L* separate from a blob in a*/b* catches hue-only dots.
  - Seeds for region growing, and scale hints (a per-region "natural scale" helps choose simplification tolerance).
  - Also good for detecting **textures of repeated dots** (halftone, dither): many blobs at one scale on a lattice indicate screen or dither patterns to be filtered out before segmentation.

### 4.2 MSER
- **Matas, Chum, Urban, Pajdla (2002/2004)**, "Robust wide baseline stereo from maximally stable extremal regions", BMVC 2002 / IVC 22(10).
- **Definition**: threshold the image at every level g. The connected components of {I ≤ g} form a component tree. For a component Q_g, define stability `q(g) = |Q_{g+Δ} \ Q_{g−Δ}| / |Q_g|`. An MSER is a local minimum of q along a tree branch.
- **Algorithm**: Nistér & Stewénius (2008), "Linear time maximally stable extremal regions", ECCV. A component-tree build in O(n) with a priority queue on the 256 grey levels and a "flood" from a boundary-pixel stack. The older version uses union–find, O(n α(n)) after a bucket sort. Both are easy to write.
- **Defaults** (OpenCV, Apache):
  - Δ = 5, min_area = 60, max_area = 14400 (scale to image);
  - max_variation = 0.25, min_diversity = 0.2 (prune near-duplicate nested regions).
- **Colour MSER (MSCR)**: Forssén (2007), "Maximally stable colour regions for recognition and matching", CVPR. An agglomerative clustering of the pixel graph by colour edge distance (chi-squared on RGB), with a time-varying threshold and stability on area. OpenCV MSER handles colour images this way.
- **Use for the vectorizer**:
  - MSER/MSCR regions are *exactly* "stable colour regions", i.e. good segmentation candidates. They are affine-covariant: fit an ellipse to their second moments to get an elliptic blob guide.
  - A region is "round" if `4π·area/perimeter² > 0.85–0.9` (circularity) and its moment-ellipse fit residual is small.
  - The MSER component tree gives a natural **nesting hierarchy** (holes inside discs, rings). It is also a good input for layered (stacked) SVG output.
  - Per-region moment ellipse:
    - μ20, μ11, μ02 are central second moments divided by area.
    - λ1,2 = ½(μ20+μ02 ± sqrt((μ20−μ02)² + 4μ11²)).
    - Semi-axes = 2 sqrt(λ1,2) for a uniform elliptical disc.
    - Angle = ½ atan2(2μ11, μ20 − μ02).
  - The moment ellipse of a filled region is **much more stable** than a boundary fit for small blobs (< 10 px diameter), because it uses all interior pixels. With anti-aliased images, use the fractional coverage α (from the pixel's colour distance) as a weight to get sub-pixel accuracy.

### 4.3 SimpleBlobDetector (OpenCV, Apache)
- Multi-threshold binarisation, then contours, then filtering by area, circularity, inertia ratio (λmin/λmax), convexity and colour.
- Centres are merged across thresholds.
- A useful heuristic template for our own "round-region classifier", but MSER is more principled.

---

## 5. Symmetry and repetition

### 5.1 Moment-based
- **Orientation and principal axes**: second moments, as above. For a reflectively symmetric shape, a symmetry axis is a principal axis, unless the shape is n-fold rotationally symmetric with n ≥ 3, in which case second moments are isotropic.
- **Rotational order detection**: complex moments `C_pq = Σ (x+iy)^p (x−iy)^q`, centred.
  - For n-fold symmetry, C_pq = 0 unless (p−q) ≡ 0 mod n.
  - Test magnitudes |C_{k,0}| / normalised for k = 1..12. The smallest k with large |C_{k,0}| suggests the order.
  - The direction arg(C_{n,0})/n gives the rotation phase.
- **Reference**: Shen, Ip, Cheung, Teoh (1999), "Symmetry detection by generalized complex (GC) moments: a close-form approach", IEEE PAMI 21(5). Earlier: Marola (1989), "On the detection of the axes of symmetry of symmetric and almost symmetric planar images", IEEE PAMI 11(1).
- **Zernike / Hu moments**: Hu (1962), "Visual pattern recognition by moment invariants", IRE Trans. Inf. Theory 8. Use the 7 Hu invariants as cheap shape signatures for repetition clustering (log-scaled, with a sign for reflection; the 7th changes sign under reflection, so it detects mirrored copies).

### 5.2 Contour / Fourier descriptors
- Resample the closed contour uniformly into N points z_k = x_k + i y_k (N = 64–256). FFT gives Z_m.
- **Elliptic Fourier descriptors**: Kuhl & Giardina (1982), "Elliptic Fourier features of a closed contour", CGIP 18. The first harmonic is the best-fit ellipse, so it doubles as an ellipse guide. Normalise for rotation, scale and start point via the first-harmonic ellipse.
- **Invariance**:
  - translation: drop Z_0;
  - scale: divide by |Z_1|;
  - rotation and start point: phase normalisation using Z_1 and Z_{±k};
  - |Z_m| alone is invariant to rotation and start point (but loses reflection).
- **Rotational n-fold symmetry**: the energy concentrates in harmonics m ≡ 1 (mod n). Score `S_n = Σ_{m≡1 mod n} |Z_m|² / Σ_{m≠0} |Z_m|²`. Accept if S_n > 0.98–0.995 (tune). The highest n passing the test is the order.
- **Reflective symmetry about an axis at angle α through the centroid**: the reflected contour (conjugate, rotated) traversed in reverse must equal the original up to start shift. Equivalently, `Z_m e^{−i(…)} = conj(Z_{2−m})`-type relations.
  - Practical method: for each candidate α, from principal axes or from 2N sampled angles, compute the cyclic cross-correlation (via FFT, O(N log N)) between the curvature (or tangent-angle) sequence and its reversed copy.
  - The peak gives the best start-shift, so the axis passes through matching points, and the peak value gives a symmetry score.
  - The curvature sequence is rotation-invariant, so this needs no α search. The axis is recovered from the matched point pair. This is O(N log N) per contour.
- **Zabrodsky, Peleg, Avnir (1995)**, "Symmetry as a continuous feature", IEEE PAMI 17(12). Continuous symmetry measure (CSM): the minimal mean squared displacement needed to make the point set symmetric, with a closed-form "folding/unfolding" algorithm for rotational C_n and reflection given point correspondences. This gives the *symmetrised* shape directly, which is exactly what we want for snapping: replace the contour by its symmetrised version if the CSM is below a tolerance.
- **Loy & Eklundh (2006)**, "Detecting symmetry and symmetric constellations of features", ECCV. Pairs of SIFT-like features vote for symmetry axes in a Hough space (θ, r). Good for image-level symmetry (whole logo symmetric about a vertical axis) across many regions.
  - Our version: each region (centroid, orientation, Hu signature) is a feature. Pairs with matching mirrored signatures vote for an axis.
- **Benchmarks**: Liu et al., "Symmetry detection from real-world images competition", CVPR workshops 2011/2013/2017.

### 5.3 Repeated shapes
- **Signature clustering**:
  1. Per region, compute an invariant descriptor: Hu moments, or |EFD| first 10–20 harmonics normalised, plus area and colour.
  2. Cluster with a k-d tree or simple grid hashing; tolerance ~2–5% descriptor distance.
  3. Verify each pair by aligning contours: Procrustes or ICP on resampled contours, with the start-shift from FFT correlation. Accept if the Hausdorff distance is < 0.5–1 px.
- **Output**: one master path plus `<use>` with a transform, which reduces SVG size and enforces exact consistency (all bullets identical, all dots the same radius). Average the instances in the canonical frame before fitting, so the noise drops by √k for k copies. This is a real quality gain.
- **Lattice detection** among instance centroids (translational repetition): find the two shortest independent difference vectors, using the mode of the pairwise difference histogram (Hough on displacement vectors).
  - Reference: Hays, Leordeanu, Efros, Liu (2006), "Discovering texture regularity as a higher-order correspondence problem", ECCV.
  - Snap positions to the lattice, and to circular arrangements: fit a circle to the instance centres and check angular spacing 2π/n.
- **Global consistency snapping**:
  - radii that cluster (within σ) are set to a common value;
  - concentric circles (centres within σ) share a centre;
  - axes are snapped to 0°, 45° and 90° when within tolerance.
  - Do this as a final pass on the guide set.

---

## 6. Recommended pipeline (summary for our design)

1. For each colour-region boundary, take **sub-pixel contours** (marching squares on the anti-aliased coverage). Smooth lightly and compute the tangent angle θ(s) and curvature κ(s).
2. **Whole-region test first**: moment ellipse plus circularity, then a full Halíř–Flusser fit, then an LM orthogonal-distance fit (ODF) with Eberly distances. If the RMS is < 0.3 px and the max is < 1 px, the region *is* an ellipse or circle: emit a guide, apply G-MDL snapping to a circle or to axis alignment, and you are done.
3. Otherwise, segment the boundary at corners and inflections (Rosin–West style on θ(s)). Classify each piece as line, circular arc, elliptic arc or free curve with G-AIC/G-MDL, a residual runs test and an arc-span threshold.
4. Group arcs (Fornaciari-style: convexity, centre-consistency via parameter covariance, optional Jia CN pruning). Refit jointly and validate with NFA (binomial tail on aligned boundary samples, ε = 1).
5. Run blobs (scale-normalised LoG or DoH on the L* channel, plus MSER/MSCR) for dots or holes the segmentation may miss. Add region seeds and circle guides with radius = √2·σ.
6. Symmetry and repetition: Hu/EFD signature clustering, `<use>` instancing with averaged masters, an FFT curvature-correlation test for reflection and Fourier harmonic energy for rotation, then global snapping of radii, centres and angles.

Numerical kit to write in Rust:
- 3×3 / 4×4 / 6×6 symmetric eigen-solvers (Jacobi) and a 3×3 general eigen-solver;
- small dense LM with Cholesky;
- bisection root-finder;
- FFT (radix-2, plus Bluestein if needed);
- separable or recursive Gaussian;
- union–find;
- log-gamma and regularised incomplete beta for the NFA.

All are short and well-specified.
