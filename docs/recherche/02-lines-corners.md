# 02 — Line segments, corners, junctions, fillets: methods for the "guides" stage

Scope: the feature-detection stage that produces geometric guides (straight segments, corners, junctions, fillets, line/arc transitions). Later stages use the guides to constrain how colour-region contours are traced. Everything listed here will be reimplemented from the papers.

License legend:
- **[OK]**: permissive (MIT/BSD/Apache). You can read the code, and porting ideas is fine with attribution.
- **[GPL!]** / **[AGPL!]**: copyleft. Read the paper only. Do not copy, translate or closely paraphrase the code.

Notation:
- I(x,y): image. ∇I = (Ix, Iy) is its gradient.
- N×M: image size.
- B(n,k,p) = Σ_{j=k..n} C(n,j) p^j (1-p)^{n-j}: binomial tail.

---

## 0. Pre-requisites shared by all detectors

**Colour gradient.** Our input is RGB. Use the Di Zenzo structure tensor (Di Zenzo 1986, "A note on the gradient of a multi-image", CVGIP 33):
- G = Σ_c ∇I_c ∇I_cᵀ, summed over the channels (preferably in Lab or another perceptual space).
- Gradient magnitude = sqrt(λmax(G)). Direction = the principal eigenvector, which is defined modulo π.
- Closed form for a 2×2 symmetric [a b; b c]:
  - λ± = (a+c)/2 ± sqrt(((a-c)/2)² + b²)
  - θ = ½·atan2(2b, a-c)
- The sign of the direction is ambiguous. Disambiguate it with the channel of maximum gradient, or by majority sign of the luminance gradient. LSD needs an oriented angle (mod 2π) to tell dark→light from light→dark.

**Gradient masks.**
- LSD uses a 2×2 mask, evaluated at the pixel corner (x+½, y+½):
  - gx = (I(x+1,y)+I(x+1,y+1) - I(x,y) - I(x,y+1))/2
  - gy = (I(x,y+1)+I(x+1,y+1) - I(x,y) - I(x+1,y))/2
  - It is chosen to minimise dependency between neighbouring gradient values, which the a-contrario independence assumption needs.
- Sobel/Scharr 3×3 is better for Harris and for accuracy.

**Log-gamma.** Every NFA computation needs log C(n,k). Stable Rust has no `ln_gamma`, so implement Lanczos (g=7, 9 coefficients) or Windschitl's approximation, as LSD does. Use Stirling for n>15 and an exact table for small n.

---

## 1. Line segment detection

### 1.1 LSD: Line Segment Detector (a-contrario)

**Papers**
- R. Grompone von Gioi, J. Jakubowicz, J.-M. Morel, G. Randall, "LSD: A Fast Line Segment Detector with a False Detection Control", IEEE TPAMI 32(4):722–732, 2010.
- IPOL 2012 article with full pseudo-code: https://doi.org/10.5201/ipol.2012.gjmr-lsd (article text CC-BY-NC-SA).

**Reference code license**
- **[AGPL!]** The reference C code lsd_1.6 is AGPL-v3 (https://github.com/cmla/lsd). Do not copy it.
- The IPOL article gives enough pseudo-code to reimplement LSD cleanly.
- OpenCV's `createLineSegmentDetector` was removed from 3.4.6–3.4.15 and 4.1.0–4.5.3 over this license conflict. It was restored in 4.5.4 after the NFA code was republished under MIT **[OK]**.
- Clean-room MIT reimplementation: https://github.com/hdkai/line-segment-detector **[OK]**.

**Algorithm (with the paper's defaults)**
1. **Scale.** Downsample by S = 0.8 after Gaussian blur with σ = Σ/S, where Σ = 0.6 (so σ = 0.75). This kills staircase/aliasing alignments. For our clean synthetic or flat-colour images, run at S = 1 as well and compare.
2. **Gradient and level-line angle.**
   - Compute g with the 2×2 mask. The level-line angle is θ = atan2(gx, -gy), orthogonal to the gradient.
   - Discard pixels with |g| < ρ, where ρ = q / sin τ, quantisation error q = 2, angle tolerance τ = 22.5°. This gives ρ ≈ 5.2 on a 0–255 scale.
   - The threshold comes from requiring that the angle error caused by quantisation stays below τ.
3. **Pseudo-ordering.** Bucket-sort pixels by |g| into 1024 bins, which is linear time. Seeds are taken from the strongest gradient down.
4. **Region growing.**
   - Start at the seed with θ_r = θ_seed.
   - Add 8-neighbours (not yet used) whose angle differs from θ_r by less than τ.
   - After each addition, update θ_r = atan2(Σ sin θ_j, Σ cos θ_j).
   - Mark the pixels as used.
5. **Rectangle approximation.**
   - Centre c = gradient-magnitude-weighted centroid.
   - Orientation = main eigenvector of the weighted inertia matrix M = Σ_j w_j (p_j - c)(p_j - c)ᵀ / Σ w_j.
   - Length and width are the extents of the region's pixels along and across that axis.
6. **Density check.**
   - If (#aligned pixels / rectangle area) < D = 0.7, the region is curved or contains two segments.
   - First, re-grow with a tighter τ, estimated from the angle spread near the seed.
   - Otherwise, cut the region radius by 25% around the seed until density is reached.
7. **NFA validation.**
   - Let n = number of pixels in the rectangle, and k = number of those aligned with the rectangle angle at precision p.
   - p = τ/π = 1/8.
   - NFA(r) = (NM)^{5/2} · γ · B(n, k, p).
     - (NM)^{5/2} counts the rectangles: (NM)² endpoint pairs × (NM)^{1/2} widths.
     - γ = 11 is the number of precision values tried in step 8.
   - Accept if NFA ≤ ε = 1, i.e. on average less than one false detection per image of pure noise.
8. **Rectangle improvement.** Used when NFA > ε, or to get a better one.
   - Try finer precisions p/2, p/4, … (5 values).
   - Reduce the width in 0.5 px steps.
   - Shift each side by 0.5 px.
   - Keep the best NFA.
9. **Output.** Endpoints, width, p, and −log10 NFA, which serves as a confidence score.

**Properties**
- Complexity is roughly O(NM).
- Speed is about 10–50 ms for 512² in C.
- Sub-pixel endpoint accuracy is about 0.5 px. Angle accuracy is better.

**Known pitfalls**
- Long segments get fragmented at junctions, crossings and low contrast. A merging pass is needed (§1.6).
- Slightly curved edges are cut into chains of short segments.
- On anti-aliased flat-colour art, a corner is often truncated by about 1–2 px on each side.
- Opposite polarity on two sides of a thin line yields two parallel segments, about 1 px apart. This is useful as a thin-stroke detector.

### 1.2 Edge Drawing (ED), EDPF, EDLines

**Papers**
- C. Topal & C. Akinlar, "Edge Drawing: A combined real-time edge and segment detector", JVCIR 23(6), 2012.
- C. Akinlar & C. Topal, "EDLines: A real-time line segment detector with a false detection control", Pattern Recognition Letters 32(13):1633–1642, 2011, doi 10.1016/j.patrec.2011.06.001.
- EDPF (parameter-free ED via Helmholtz principle): Akinlar & Topal, PRL 2012.
- EDCircles: Akinlar & Topal, Pattern Recognition 2013.

**Code**
- Authors' ED_Lib (github CihanTopal/ED_Lib). I could not confirm its license, so check before reading.
- OpenCV contrib `ximgproc::EdgeDrawing` is Apache-2.0 **[OK]** and includes EDLines, EDPF and EDCircles.

**ED algorithm**
1. Gaussian 5×5, σ = 1.
2. Sobel (or Prewitt) gradient.
   - |g| ≈ |gx| + |gy|.
   - Edge direction is "vertical" if |gx| ≥ |gy|, else "horizontal".
   - Pixels with |g| < gradient threshold (default 36) are suppressed.
3. **Anchors.**
   - A pixel is an anchor if its |g| exceeds both neighbours across the edge direction by ≥ anchor threshold (default 8).
   - Scan every k-th row/column (scan interval 1–4).
4. **Smart routing.**
   - From each anchor, walk in both directions along the edge direction.
   - At each step move to the max-|g| pixel among the 3 forward neighbours.
   - Stop at a suppressed pixel or an already-visited one.
   - The result is 1-px-wide, contiguous, ordered edge chains. This is exactly what we want for curvature analysis too.
5. **EDPF** replaces the thresholds with a-contrario validation of each chain (Desolneux "meaningful boundary", §2.3), using a very low gradient threshold.

**EDLines on top of the chains**
1. Walk the chain and fit a line by least squares on the first L_min pixels.
   - L_min is the minimal meaningful length: N^4 · p^{L} ≤ 1 ⇒ L_min = −4 log N / log p, with p = 1/8 and N = image side.
   - This gives about 12 px for 512².
2. Extend the line pixel by pixel while the perpendicular distance stays ≤ 1 px. Refit on exit.
3. Validate each segment a-contrario: NFA = N^4 · B(n, k, 1/8) ≤ 1, where k counts pixels whose gradient angle is within 22.5° of the segment normal.
4. **Accuracy.** EDLines is comparable to LSD, gives slightly longer segments, and is about 10× faster. It produces fewer fragmented segments on clean images.

**Our use.** ED chains with the EDPF a-contrario validation give clean, ordered, connected contours. Run EDLines plus curvature analysis (§3.6) on the same chains.

### 1.3 Hough transform family

**Standard Hough transform (SHT)**
- Hough 1962 patent. Normal parametrisation from R. Duda & P. Hart, "Use of the Hough transformation to detect lines and curves in pictures", CACM 15(1), 1972.
- Each edge point votes for ρ = x cosθ + y sinθ, with θ ∈ [0, π) and ρ ∈ [−D, D], D = image diagonal.
- Cost is O(N_edges · N_θ). Bins are typically Δρ = 1 px and Δθ = π/180.
- **Gradient-restricted voting** (O'Gorman & Clowes 1976): vote only θ ∈ θ_grad ± δ. This cuts cost and noise by about 20×.
- **Peak extraction:** 2-D non-maximum suppression (NMS), a threshold on votes, then a butterfly-shaped peak refinement.
- **Pitfalls:**
  - It finds infinite lines, not segments.
  - Bin size trades off against accuracy.
  - Noise and texture create spurious peaks.
  - Dense parallel lines interfere with each other.
  - The accumulator is heavily biased by line length, so short edges are lost.

**Progressive Probabilistic Hough Transform (PPHT)**
- Paper: J. Matas, C. Galambos, J. Kittler, "Robust detection of lines using the progressive probabilistic Hough transform", CVIU 78(1):119–137, 2000.
- Algorithm:
  1. Pick a random unprocessed edge point and vote with it.
  2. If the bin now exceeds the count expected from noise (a threshold derived from the Poisson tail of the current number of votes), walk along that line in the image from the point and collect the longest run with gaps ≤ maxLineGap.
  3. Remove those points from the image, and un-vote their earlier votes.
  4. Output the segment if its length ≥ minLineLength.
- OpenCV `HoughLinesP` **[OK]**: rho=1, theta=π/180, threshold, minLineLength, maxLineGap. scikit-image `probabilistic_hough_line` **[OK]**.
- Output is randomised and not reproducible unless the RNG is seeded.

**Randomized Hough Transform (RHT)**
- Paper: L. Xu, E. Oja, P. Kultanen, "A new curve detection method: Randomized Hough Transform (RHT)", Pattern Recognition Letters 11(5):331–338, 1990.
- Sample 2 points, which determine one (ρ, θ) cell. Accumulate in a dynamic structure (hash map or tree).
- When a cell reaches the threshold, verify the line, remove its points, and restart.
- Generalises well to circles (3 points). Efficient when there are few strong primitives.

**Kernel-based Hough Transform (KHT)**
- Paper: L. A. F. Fernandes & M. M. Oliveira, "Real-time line detection through an improved Hough transform voting scheme", Pattern Recognition 41(1):299–314, 2008, doi 10.1016/j.patcog.2007.04.003.
- **[GPL!]** Code (https://github.com/laffernandes/kht) is GPL-3.0. Use the paper only.
- **Algorithm:**
  1. Link edge pixels into chains, then subdivide them into approximately collinear clusters by recursive splitting, as in Douglas–Peucker.
  2. For each cluster, fit a line (§1.5) and propagate its covariance to the (ρ, θ) space via the Jacobian.
     - In the cluster's own frame (centroid as origin, axis along the line), the fitted line is y = m x + b.
     - σ_m² = 1/Σ w_i x_i², σ_b² = 1/Σ w_i, and they are uncorrelated.
     - Σ_ρθ = J diag(σ_m², σ_b²) Jᵀ.
  3. Vote with an elliptical Gaussian kernel N((ρ, θ), Σ_ρθ), truncated at about 2σ.
  4. Detect peaks on the slightly smoothed map.
- **Result:** a much cleaner accumulator, real time, and each peak carries an uncertainty. This is conceptually elegant, but chain + fit + merge (below) already gives us the same information without an accumulator.

**Recommendation.** Hough is a fallback only. Use it for global alignment (e.g. collinear fragments far apart, or rulers and grids), never as the primary detector.

### 1.4 Other line detectors worth knowing (read-only)
- **CannyLines:** X. Lu, J. Yao, K. Li, L. Li, ICIP 2015. Parameter-free Canny with Helmholtz-based validation.
- **MLSD:** Y. Salaün, R. Marlet, P. Monasse, "Multiscale line segment detector for robust and accurate SfM", ICPR 2016. Multiscale LSD with a-contrario merging, which is relevant to §1.6.
- **FastLineDetector:** J. H. Lee et al. 2014, in OpenCV ximgproc **[OK]**.
- **ELSD / ELSDc:** V. Pătrăucean, P. Gurdjos, R. Grompone von Gioi, ECCV 2012, and "Joint A Contrario Ellipse and Line Detection", TPAMI 39(4):788–802, 2017.
  - Joint lines + circular/elliptical arcs with model selection by NFA. This is directly relevant to §4.
  - **[AGPL!]** Code: github potato34/ELSDc.

### 1.5 Line fitting: total least squares with covariance

Given points p_i (optionally with weights w_i, e.g. |∇I|):

1. **Centroid and scatter.**
   - c = Σ w_i p_i / W, with W = Σ w_i.
   - S = Σ w_i (p_i - c)(p_i - c)ᵀ = [Sxx Sxy; Sxy Syy].
2. **Direction.** d = eigenvector of λmax, i.e. angle φ = ½ atan2(2Sxy, Sxx - Syy).
   - Normal n = (-sin φ, cos φ). The line is nᵀ(p - c) = 0.
3. **Residuals.** RSS = λmin. With unit weights the noise variance estimate is σ̂² = λmin / (n - 2).
4. **Parameter covariance** (first order, isotropic noise σ²; t_i = dᵀ(p_i - c) is the abscissa along the line):
   - var(φ) = σ² / Σ w_i t_i² ≈ σ² / λmax
   - var(offset at centroid) = σ² / W
   - cov = 0, by the centroid choice
   - var(perpendicular position at abscissa t) = σ²/W + t² σ²/Σ t_i²: hourglass-shaped uncertainty
   - For a segment of length L with n uniform points: σ_φ ≈ σ·sqrt(12/n)/L
5. **Endpoints.** Project the extreme points onto the line. Refine against the gradient profile if needed, e.g. at half-contrast along the line direction.
6. **Robustness.**
   - IRLS with Huber/Tukey weights, or
   - RANSAC (Fischler & Bolles 1981) for initialisation. Inlier threshold 1–1.5 px on edge chains.

Useful reference: K. Kanatani, "Statistical Optimization for Geometric Computation" (1996), for covariance propagation and the "geometric AIC". Equivalent material appears in W. Förstner & B. Wrobel, "Photogrammetric Computer Vision" (Springer 2016), ch. 10.

### 1.6 Merging collinear / interrupted segments

Candidate generation: index segments by angle (bins of 5–10°) and spatially (grid), then test pairs (s1, s2) that pass a loose angular gap and endpoint distance.

**Geometric tests**
- Angle: |φ1 − φ2| < α (e.g. 2–5°, or adaptive: 3·sqrt(σ_φ1² + σ_φ2²)).
- Lateral offset: the perpendicular distance of each endpoint to the other line is < δ (1–2 px).
- Gap: the longitudinal gap between the facing endpoints is < g. Make g relative to the lengths, e.g. g < 0.5·min(L1, L2), or absolute.
- Overlap: overlapping parallel segments at distance < 1 px are duplicates. Separation of about 1–3 px with opposite polarity means a thin stroke. Keep it as a "stroke" guide.

**Statistical collinearity test (recommended)**
- Express both segments in a common frame, e.g. the frame of their joint TLS fit. Their parameter vectors (φ, offset) have covariances Σ1 and Σ2.
- Compute D² = (θ1 − θ2)ᵀ (Σ1 + Σ2)⁻¹ (θ1 − θ2) and compare it with χ²₂ (95% → 5.99, 99% → 9.21).
- Equivalent F-test on the merged fit:
  - F = [(RSS_merged − RSS1 − RSS2)/2] / [(RSS1 + RSS2)/(n1 + n2 − 4)] ~ F(2, n1+n2−4).
  - This is cheap: RSS = λmin of each scatter matrix, and scatter matrices are additive with a centroid correction.
- Also require colour/polarity consistency: the same pair of region labels on both sides. In our pipeline that is a hard constraint and kills most false merges.

**A-contrario merging (gap test)**
- Merge if NFA(merged rectangle) < NFA(s1) · NFA(s2) · (correction for the number of tested pairs).
- In words: the union is more meaningful than the two independent detections. A large gap of non-aligned pixels inflates n without raising k, so the merged NFA grows and the merge is rejected automatically.
- MLSD (Salaün et al. 2016) uses this idea across scales.

**Iterate** until stable, greedily from the longest or most-meaningful segments.

**Complexity.** O(S log S) with spatial and angle indexing.

---

## 2. The a-contrario framework

**References**
- A. Desolneux, L. Moisan, J.-M. Morel, "Meaningful alignments", IJCV 40(1):7–23, 2000.
- Same authors, "Edge detection by Helmholtz principle", JMIV 14(3):271–284, 2001.
- Same authors, *From Gestalt Theory to Image Analysis: A Probabilistic Approach*, Springer 2008. This is the reference book.

**Principle (Helmholtz).** Observing a structure is meaningful if it would be very unlikely in a noise image (the background model H0), so no explicit model of the object is needed.

**NFA**
- For a candidate event e among N_tests candidates: NFA(e) = N_tests · P_H0[a candidate is at least as structured as e].
- e is ε-meaningful if NFA(e) ≤ ε.
- **Key theorem:** the expected number of ε-meaningful events in an H0 image is ≤ ε.
- This is why the threshold is "parameter-free": ε = 1 (one false alarm per image), and the result depends on ε only logarithmically. ε = 10⁻² or 10² barely change detections.

**H0 for alignments**
- Gradient orientations are i.i.d. uniform.
- A point is "p-aligned" with a segment if its orientation is within pπ of the segment's.
- A segment of n points containing k aligned ones has P = B(n, k, p).
- In an N×N image there are about N⁴ segments, so NFA = N⁴ B(n, k, p).
- Independence requires sampling points at the Nyquist distance (about 2 px after Shannon interpolation). LSD instead uses the 2×2 gradient and counts every pixel.

**Minimal meaningful length.** With k = n, NFA = N⁴ pⁿ ≤ ε ⇒ n ≥ (log ε − 4 log N)/log p. For N = 512 and p = 1/8 this gives about 12 px. Below that, nothing can be detected, which is a natural minimum.

**Maximality / exclusion principle.**
- Many nested segments are meaningful. Keep "maximal meaningful" ones: e is maximal if it has no meaningful sub- or super-segment with a lower NFA.
- Among overlapping detections, keep the one with the lowest NFA and remove the pixels it explains. This is the exclusion principle, also used by ACJ (§3.5) and ELSDc.

**Bounds for fast computation**
- Hoeffding/Chernoff: B(n, k, p) ≤ exp(−n · KL(k/n ‖ p)) for k/n > p.
- Use it for early rejection, and compute exact log B only for survivors.

**Meaningful boundaries / edges (curves)**
- Desolneux et al. 2001.
- Use the empirical distribution H(μ) = P(|∇I| ≥ μ) over the image.
- A level-line piece of length l (l/2 independent samples, at the Nyquist distance 2) whose minimum gradient is μ has NFA = N_ll · H(μ)^{l/2}, where N_ll = number of level-line pieces tested.
- This yields parameter-free edges. It is the basis of EDPF and of "meaningful contrasted boundaries".
- Cao, Musé, Sur, "Extracting meaningful curves from images", JMIV 22, 2005, extends it to curves.

**Good continuation / smooth curves and corners**
- F. Cao, "Good continuations in digital image level lines", ICCV 2003.
- F. Cao, "Application of the Gestalt principles to the detection of good continuations and corners in image level lines", Computing and Visualisation in Science 7, 2004.
- H0: the direction changes along a curve are i.i.d. uniform. A curve of length l where every turn is ≤ α has P = (α/π)^l, so NFA = N_curves · (α/π)^l. This lets us declare "smooth" curves.
- A corner is where the meaningful good continuation breaks.

**Groupings**
- Parallelism: pairs of segments whose angle difference is small. Under H0 (angles uniform), P(|Δφ| ≤ δ) = 2δ/π. NFA = N_pairs · (2δ/π) · P(length/overlap conditions).
- Alignment of segments, i.e. collinear fragments: use the NFA gap test above.
- Point alignments: J. Lezama, J.-M. Morel, G. Randall, R. Grompone von Gioi, "A contrario 2D point alignment detection", TPAMI 37(3), 2015.
- Vanishing points: A. Almansa, A. Desolneux, S. Vamech, "Vanishing point detection without any a priori information", TPAMI 25(4), 2003.
- Joint models (segment vs circular arc vs ellipse) are compared by NFA, which gives model selection for free (ELSDc).

**Pitfalls**
- H0 must match the data. On flat-colour, JPEG-blocky or posterised images, gradients are *not* i.i.d.: block edges create "meaningful" 8-px alignments. Mitigations:
  - mild denoising before detection;
  - LSD's 0.8 rescale;
  - a per-image empirical H0 (gradient-magnitude histogram for H(μ)).
- N_tests must be counted honestly, including all p and width variants (LSD's γ).

---

## 3. Corner and junction detection

### 3.1 Harris & Stephens; Shi–Tomasi

**Papers**
- C. Harris & M. Stephens, "A combined corner and edge detector", Alvey Vision Conference 1988.
- J. Shi & C. Tomasi, "Good features to track", CVPR 1994.

**Structure tensor.** M(x) = g_σi * [Ix² IxIy; IxIy Iy²], with derivative scale σd ≈ 1 and integration scale σi ≈ 1.5–2 (Gaussian window). For colour, use Di Zenzo's sum over channels.

**Responses**
- Harris: R = det M − k (tr M)², with k = 0.04–0.06.
  - R > 0 and large → corner. R < 0 → edge.
- Shi–Tomasi: R = λmin = (a+c)/2 − sqrt(((a−c)/2)² + b²). Shi–Tomasi is more stable to threshold than Harris.
- Threshold: relative, e.g. R > 0.01 · max R (OpenCV qualityLevel 0.01). Add 3×3 (or minDistance) NMS.

**Sub-pixel position.** Fit a quadratic to R over the 3×3 neighbourhood, or use Förstner (below).

**Properties**
- Rotation-invariant, not scale-invariant.
- Localization bias: the maximum of R lies *inside* the corner wedge, displaced from the true vertex by about σi. The displacement depends on the angle; it is worse for acute corners.
- Do not use Harris maxima as vertex positions. Use them as candidates only.

**Implementations.** OpenCV `cornerHarris` / `goodFeaturesToTrack`; scikit-image `corner_harris` / `corner_shi_tomasi` **[OK]**.

### 3.2 Förstner operator: sub-pixel corner/junction and circle centres

**Papers**
- W. Förstner & E. Gülch, "A fast operator for detection and precise location of distinct points, corners and centres of circular features", ISPRS Intercommission Conf. on Fast Processing of Photogrammetric Data, Interlaken 1987, pp. 281–305.
- W. Förstner, "A framework for low level feature extraction", ECCV 1994.
- Bonn IPB page: https://www.ipb.uni-bonn.de/data-software/forstner-operator/

**Measures**
- w = det M / tr M: the size of the error ellipse; large means precise.
- q = 4 det M / (tr M)² = 1 − ((λ1 − λ2)/(λ1 + λ2))²: roundness, in [0,1].
- Select points with q > q_min (0.5–0.75) and w > w_min (e.g. 0.5–1.5 × mean or median w). Then apply NMS on w.

**Precise location: the key equation for us**
- Every edge pixel x_i with gradient g_i defines a line through x_i perpendicular to g_i. The junction is the point closest to all of them:
  - x̂ = argmin Σ_i w_i (g_iᵀ (x̂ − x_i))²
  - ⇒ (Σ w_i g_i g_iᵀ) x̂ = Σ w_i g_i g_iᵀ x_i, i.e. x̂ = M⁻¹ b.
- Covariance: Σ_x̂ = σ̂² M⁻¹, with σ̂² = residual / (n − 2).
- **Circular features:** replace g_i by the tangent t_i = g_i rotated 90° (lines through x_i along the gradient). x̂ is then the centre of a circular blob or arc. This gives an estimate of a fillet's centre.
- Classification (Förstner 1994): compare residuals of the two models (junction vs circle), and use the eigenvalues → point / edge / homogeneous region.
- OpenCV `cornerSubPix` solves the same junction equation iteratively. skimage `corner_foerstner` and `corner_subpix` **[OK]**.
- Accuracy is about 0.1 px on clean corners. It is biased on rounded corners: the estimate lands inside the arc, not at the virtual vertex.

### 3.3 FAST
- Paper: E. Rosten & T. Drummond, "Machine learning for high-speed corner detection", ECCV 2006. FAST-ER: TPAMI 32(1), 2010.
- **Test:**
  - Bresenham circle of 16 pixels at radius 3.
  - Corner if ≥ N contiguous circle pixels are all > I_p + t or all < I_p − t, with N = 9 (FAST-9 is best) or 12, and t ≈ 10–20 on 8-bit.
  - Quick rejection: test pixels 1, 9, 5, 13 first.
- Score for NMS: max t for which it is still a corner, or the sum of absolute differences.
- Original code BSD **[OK]**; OpenCV and imageproc have FAST-9/12.
- **Verdict:** pixel-accurate only, not rotation-invariant at small scale, no corner angle. It is a fast candidate generator, not suitable for vertex geometry.

### 3.4 SUSAN
- Paper: S. M. Smith & J. M. Brady, "SUSAN—A new approach to low level image processing", IJCV 23(1):45–78, 1997. (A UK patent existed and has long expired.)
- **Method:**
  - Circular mask of 37 pixels (radius about 3.4) around nucleus r0.
  - Similarity c(r) = exp(−((I(r) − I(r0))/t)⁶), with t ≈ 25 for 8-bit. USAN area n(r0) = Σ c.
  - Corner response R = g − n if n < g, where g = n_max/2 for corners (3n_max/4 for edges).
  - False-positive removal: the USAN centroid must be far from the nucleus, and all pixels on the line nucleus→centroid must be in the USAN.
- No derivatives, so it is robust to noise. Pixel-level. Interesting for flat-colour images because it is essentially region-based. With a label map, the "USAN area" is simply the fraction of the disk with the same label as the centre, which is a trivial corner measure on segmented images.

### 3.5 Junctions: detection and typing (L, T, Y, X, arrow)

**Papers**
- G.-S. Xia, J. Delon, Y. Gousseau, "Accurate junction detection and characterization in natural images", IJCV 106(1):31–56, 2014 (**ACJ**). Also "An accurate and contrast invariant junction detector", ICPR 2012.
  - For each candidate centre and scale r, test each angular sector ("branch") for normalized-gradient alignment along the radial direction, a-contrario.
  - A junction = ≥ 2 meaningful branches.
  - Then apply exclusion/NMS between junctions. Outputs location, scale and branch angles.
- L. Parida, D. Geiger, R. Hummel, "Junctions: detection, classification, and reconstruction", TPAMI 20(7), 1998.
  - Wedge model: piecewise-constant sectors in a disk.
  - Fit sector angles and intensities by energy minimisation with an MDL choice of the number of wedges.
  - This is close to what a flat-colour vectorizer needs.

**Typing from branch angles.** Let branches have directions β_1..β_m, sorted, with consecutive sector angles α_j.
- **L:** m = 2.
  - Sharp corner if |β1 − β2 − π| > δ (e.g. δ = 10–15°).
  - Otherwise it is a straight continuation, not a corner.
- **T:** m = 3 and one pair is collinear (|α_j − π| < δ). The third branch ends on the line, typically an occlusion.
- **Y:** m = 3, all α_j < π − δ.
- **Arrow:** m = 3, one α_j > π + δ.
- **X:** m = 4 with two collinear pairs.
- **Ψ / higher order:** m ≥ 4, otherwise.
- **Geometric position:**
  - For L and Y, the least-squares intersection of the branch lines (Förstner equation restricted to the branch lines, with covariance).
  - For T, intersect the stem with the bar.

**Our pipeline advantage.** We have a colour-region label map, so topological junctions are known exactly: the pixel corners where ≥ 3 distinct labels meet (2×2 block with ≥ 3 labels).
- The number of incident boundary arcs gives m directly.
- Geometry (branch directions, position) comes from line/arc fits on each incident boundary arc, excluding a radius of about 2–3 px near the junction, where anti-aliasing corrupts labels.
- Grey-level detectors (Harris, ACJ) are then only needed to validate and refine, and to catch L-corners, which are not topological junctions (two labels only).

### 3.6 Curvature-based corners on contours

**Discrete curvature on a chain (x(u), y(u))**
- Smooth with a Gaussian of scale σ (in arc-length samples): X = x * g_σ, Y = y * g_σ.
- κ(u, σ) = (X_u Y_uu − X_uu Y_u) / (X_u² + Y_u²)^{3/2}.
- Use derivative-of-Gaussian kernels. Handle closed contours circularly and open ones by reflection.
- Equivalent and more robust: the tangent angle θ(s) = atan2(Y_u, X_u), unwrapped, with κ = dθ/ds.

**Curvature Scale Space (CSS)**
- F. Mokhtarian & A. Mackworth, "Scale-based description and recognition of planar curves and two-dimensional shapes", TPAMI 8(1), 1986.
- Same authors, "A theory of multiscale, curvature-based shape representation for planar curves", TPAMI 14(8), 1992.
- The corner detector: F. Mokhtarian & R. Suomela, "Robust image corner detection through curvature scale space", TPAMI 20(12), 1998.
  - Detect |κ| maxima at a high scale σ_high (absolute threshold, e.g. t ≈ 0.03 with σ_high ≈ 3–4).
  - Track them to the lowest scale for localization.
  - Reject maxima with |κ| < 2× neighbouring minima.
- **He & Yung** (X. C. He & N. H. C. Yung, "Corner detector based on global and local curvature properties", Optical Engineering 47(5), 2008). This is the most practical variant:
  - Adaptive threshold: κ_i > R · mean |κ| over its region of support (ROS, between neighbouring curvature minima), with R = 1.5.
  - Angle test: the angle between the ROS end-tangents must be ≤ 160° (else it is a round, wide turn).
  - End-point handling for open contours.

**Dominant points / polygonal approximation**
- A. Rosenfeld & E. Johnston, "Angle detection on digital curves", IEEE Trans. Computers 22, 1973 (k-cosine).
  - cos_ik = (a_ik · b_ik)/(|a_ik||b_ik|), where a and b are the vectors to the points k steps ahead and behind.
- C.-H. Teh & R. Chin, "On the detection of dominant points on digital curves", TPAMI 11(8), 1989.
  - Adaptive region of support k_i: the largest k such that the chord ratio d_ik/l_ik keeps increasing.
  - Then curvature measure and NMS. Parameter-free.
- U. Ramer 1972 / D. Douglas & T. Peucker 1973: recursive split at the farthest point (tolerance ε ≈ 1 px). Fast, but split points are not optimal corners. Use them as initialisation only.
- **Discrete geometry** (exact for digital lines):
  - Digital straight segment (DSS) recognition: I. Debled-Rennesson & J.-P. Reveillès 1995, arithmetic lines 0 ≤ ax − by + μ < ω, recognised incrementally in O(1) amortised per point.
  - Maximal segments / tangential cover: J.-O. Lachaud, A. Vialard, F. de Vieilleville, "Fast, accurate and convergent tangent estimation on digital contours", Image and Vision Computing 25, 2007.
  - Dominant points from the tangential cover: T. P. Nguyen & I. Debled-Rennesson, Pattern Recognition 44, 2011.
  - Noise scale per point ("meaningful scales"): B. Kerautret & J.-O. Lachaud, "Meaningful scales detection along digital contours for unsupervised local noise estimation", TPAMI 34(12), 2012. Excellent for deciding the local tolerance automatically.
  - Reference library DGtal is LGPL-3: read only.
- **Vectorizer precedent:** P. Selinger, "Potrace: a polygon-based tracing algorithm", 2003. GPL-2 **[GPL!]**, paper only.
  - Optimal polygon via straight-run constraints (penalised).
  - Corner decision at each vertex by parameter α (alphamax = 1.0 default; α > alphamax → sharp corner, else smooth Bézier).

### 3.7 Corners vs rounded corners: diagnosing from scale behaviour

The way curvature changes with smoothing scale tells the two apart:
- **Ideal sharp corner** with turning angle Δ, smoothed by g_σ: the curvature profile is ≈ Δ · g_σ(s), with peak κ_max ≈ Δ / (σ sqrt(2π)) ∝ 1/σ. The total turning ∫κ ds = Δ is constant.
- **Fillet of radius r:** κ ≈ 1/r on a plateau of length rΔ, roughly constant for σ ≪ rΔ.
- **Test:** compute κ_max(σ) at 3–4 scales.
  - σ·κ_max ≈ constant → sharp corner.
  - κ_max ≈ constant → round, with r̂ ≈ 1/κ_max.
  - Equivalently: the width of the region where the tangent angle θ(s) changes (the 10–90% rise distance) is about 2.56σ_eff for a sharp corner, and rΔ + O(σ) for a fillet.
- **Account for the image's own blur:** σ_eff² = σ_PSF² + σ_smooth². Anti-aliasing alone gives σ_PSF ≈ 0.3–0.5 px; scanned or JPEG input gives about 1 px. Estimate σ_PSF from straight-edge profiles (erf fit of the gradient profile across segments).
- **Decision:** sharp unless r̂ > r_min, with r_min ≈ 1.5–2 · σ_eff (≈ 1–2 px), *and* model selection favours the fillet (§4.2).

---

## 4. Rounded corners (fillets) and tangent line–arc transitions

### 4.1 Fillet geometry

Two lines L1 and L2 meet at virtual vertex V:
- φ = interior angle between the arms (0 < φ < π). Turning angle Δ = π − φ.
- b = unit bisector pointing inside the corner.

A circular arc of radius r tangent to both lines has:
- **Centre:** C = V + (r / sin(φ/2)) · b
- **Tangent points:** T_k = V + (r / tan(φ/2)) · u_k, where u_k = unit direction of arm k away from V
- **Arc length:** r·Δ
- **Sagitta from the vertex** (distance V → arc midpoint along b): h = r (1/sin(φ/2) − 1)
- Hence r = h / (1/sin(φ/2) − 1)

### 4.2 Estimation recipe (robust, cheap)

1. **Fit the arms.** Fit L1 and L2 by TLS (§1.5) on contour points well outside the transition, i.e. beyond the curvature region of support. Iterate: start with the He–Yung ROS, then shrink the arm sets until the residuals are white. Intersect the lines → V and φ, with covariance by propagation.
2. **Initial r.**
   - r0 = h / (1/sin(φ/2) − 1), with h = minimum distance from V to the contour along b (interpolate the sub-pixel contour).
   - Or r0 = 1/κ_max from the plateau.
   - Or from the Förstner "circle" estimate (§3.2) of C: r0 = |C − V| sin(φ/2).
3. **Constrained fit.** One-parameter least squares on the transition points p_i:
   - minimise E(r) = Σ_i (|p_i − C(r)| − r)², with C(r) = V + r/sin(φ/2) · b.
   - Use golden-section search or Newton on r ∈ [0, r_max], where r_max is set so that the tangent points stay within the arm lengths.
   - Points beyond the tangent points are measured to the lines instead. This gives a G1 "line–arc–line" model whose only extra parameter is r.
4. **Full joint refinement (optional).** Gauss–Newton/LM on (L1, L2, r) over all points, with the residual to the composite curve.
5. **Model selection: sharp (r = 0) vs fillet.**
   - Compute RSS_0 and RSS_r.
   - Accept the fillet if (RSS_0 − RSS_r)/σ̂² > χ²₁(0.99) = 6.63, or use BIC: n log(RSS/n) + k log n, with k differing by 1.
   - σ̂ is taken from the arm residuals or the Kerautret–Lachaud noise level.
   - Also require r > r_min (§3.7).
   - The a-contrario alternative is ELSDc-style: compare the NFA of the line+arc+line interpretation vs line+line.
6. **Unconstrained circle fits** (for free arcs, or to initialise):
   - **Kåsa** (algebraic, linear): x² + y² + Dx + Ey + F = 0. Solve by least squares. Biased toward small r on short arcs.
   - **Pratt 1987:** "Direct least-squares fitting of algebraic surfaces", SIGGRAPH.
   - **Taubin 1991:** "Estimation of planar curves, surfaces…", TPAMI 13(11). Generalised eigenproblem, nearly unbiased.
   - **Hyper fit:** A. Al-Sharadqah & N. Chernov, "Error analysis for circle fitting algorithms", Electronic J. Statistics 3, 2009. Essentially bias-free. Recommended initialiser.
   - Refine geometrically with LM on Σ(|p_i − c| − r)².
   - Short arcs (Δ < 30°) are ill-conditioned: r is poorly determined. Prefer the tangent-constrained 1-parameter fit there.

### 4.3 Segmenting a contour into lines and arcs with tangent transitions

Use the tangent-angle signature θ(s) (unwrapped, from a lightly smoothed chain or from DSS tangents):
- a line → θ constant (κ = 0);
- an arc of radius r → θ linear with slope ±1/r;
- a sharp corner → a step in θ;
- a G1 line–arc transition → a slope change without a step (continuous θ, kink in θ(s)).

**Fitting**
- Fit θ(s) as **piecewise linear**: segments with slope 0 (lines) or free slope (arcs). Use continuity constraints at G1 joints, and allow jumps only at accepted corners.
- Solve by dynamic programming over breakpoint candidates, i.e. optimal segmentation with a per-segment penalty (BIC/MDL), in O(n²) or O(n·K) with pruning (PELT).
- Equivalent to change-point detection in κ(s).
- Then refit each piece in (x, y) space (TLS line or circle) with shared tangent points.

**Classic references**
- P. L. Rosin & G. A. W. West, "Nonparametric segmentation of curves into various representations", TPAMI 17(12), 1995. Recursive line/arc segmentation with significance measures.
- X. Hilaire & K. Tombre, "Robust and accurate vectorization of line drawings", TPAMI 28(6), 2006. Fuzzy digital segments and arcs, with explicit tangency/junction constraints in vectorization. Very close to our goal.
- I. Baran, J. Lehtinen, J. Popović, "Sketching clothoid splines using shortest paths", Eurographics 2010. Use it if G2 (curvature-continuous) transitions are wanted, i.e. Euler spirals between line and arc.

**Tangency tests** (accept "line tangent to arc" as a constraint)
- The angle between the line direction and the arc tangent at the join is < 2–3σ_φ.
- |dist(C, L) − r| < 2σ, i.e. the line touches the circle.
- No θ-step at the join larger than 3·(its standard deviation).

When accepted, snap the geometry:
- project the arc centre so that dist(C, L) = r exactly;
- recompute the tangent point as the foot of the perpendicular from C to L.

---

## 5. Recommended design for our vectorizer (synthesis)

1. **Primary segment detector:** a clean-room LSD from the IPOL pseudo-code, running on the Di Zenzo colour gradient. Keep −log10 NFA as the guide confidence. Optionally, an ED/EDPF chain extractor for ordered contours (both LSD and ED sit at about 1 px precision).
2. **Region-boundary-first:** our contours come from the label map, so detect lines, corners and fillets primarily *on the boundary chains* (sub-pixel boundaries), using:
   - TLS + DSS/tangential cover for straightness;
   - He–Yung/CSS curvature for corners;
   - the θ(s) piecewise-linear DP for line/arc/G1 transitions.
   Use LSD on the raster as an independent cross-check: a boundary piece matched by an LSD segment within 1 px and 3° gets "confirmed straight".
3. **Merging:** χ² collinearity test plus the same-label-pair constraint plus the NFA gap test. Never merge across a different region pair.
4. **Junctions:** topological (≥ 3 labels at a pixel corner) for detection and typing; geometric position from a covariance-weighted intersection of the branch fits. Use Förstner (junction model) for L-corners, and the circle model for fillet centres.
5. **Sharp vs rounded:** multi-scale κ_max behaviour plus the 1-parameter tangent-constrained fillet fit plus χ²/BIC selection, with r_min tied to the measured edge blur σ_PSF.
6. **Hough (gradient-restricted SHT or a KHT-like covariance voting, reimplemented):** only for global grouping of far-apart collinear fragments, grids and alignments.
7. **Numerics:** log-domain NFA with Lanczos ln Γ, a Hoeffding early-reject, and f64 everywhere for the fits. Use the closed-form 2×2 eigen-decomposition with care when Sxx ≈ Syy and Sxy ≈ 0 (use atan2 — never divide).

---

## 6. Implementations (reference only) and licenses

| Item | Where | License |
|---|---|---|
| LSD 1.6 reference C | ipol.im / github cmla/lsd | **AGPL-3 — do not copy** |
| LSD in OpenCV ≥ 4.5.4 (`createLineSegmentDetector`) | opencv/imgproc | Apache-2.0 (NFA code re-released MIT) [OK] |
| Clean-room LSD | github hdkai/line-segment-detector | MIT [OK] |
| ELSD / ELSDc (lines + arcs a-contrario) | github potato34/ELSDc | **AGPL-3** |
| KHT | github laffernandes/kht | **GPL-3** |
| EdgeDrawing / EDLines / EDPF / EDCircles | opencv_contrib ximgproc | Apache-2.0 [OK] |
| ED_Lib (authors) | github CihanTopal/ED_Lib | unverified — check before reading |
| FastLineDetector | opencv_contrib ximgproc | Apache-2.0 [OK] |
| HoughLines / HoughLinesP / cornerHarris / goodFeaturesToTrack / cornerSubPix / FAST | OpenCV | Apache-2.0 [OK] |
| hough_line, probabilistic_hough_line, corner_harris, corner_shi_tomasi, corner_foerstner, corner_subpix, corner_fast, approximate_polygon | scikit-image | BSD-3 [OK] |
| FAST original | E. Rosten | BSD [OK] |
| Förstner operator (Matlab) | ipb.uni-bonn.de | check page (academic) |
| DGtal (DSS, tangential cover, meaningful scales) | dgtal.org | **LGPL-3** (read only) |
| Potrace (corner/alphamax) | potrace.sourceforge.net | **GPL-2** — paper only |
| Rust `imageproc` (canny, hough `detect_lines`, FAST-9/12 corners, Douglas–Peucker) | crates.io | MIT [OK]; reference only |

---

## 7. Key URLs
- LSD IPOL: https://doi.org/10.5201/ipol.2012.gjmr-lsd (article PDF: https://www.ipol.im/pub/art/2012/gjmr-lsd/article.pdf)
- LSD code (AGPL): https://github.com/cmla/lsd
- MIT clean-room LSD: https://github.com/hdkai/line-segment-detector
- EDLines (ICIP 2011 version PDF): https://projet.liris.cnrs.fr/imagine/pub/proceedings/ICIP-2011/papers/1569406487.pdf
- OpenCV EdgeDrawing docs: https://docs.opencv.org/4.x/d4/d8b/group__ximgproc__edge__drawing.html
- OpenCV LSD (removal and restoration note): https://docs.opencv.org/4.x/db/d73/classcv_1_1LineSegmentDetector.html
- KHT project: https://profs.ic.uff.br/~laffernandes/projects/kht/
- ELSD (ECCV 2012): https://projet.liris.cnrs.fr/imagine/pub/proceedings/ECCV-2012/papers/7573/75730572.pdf
- Förstner operator: https://www.ipb.uni-bonn.de/data-software/forstner-operator/
- ACJ junctions (HAL): https://hal.archives-ouvertes.fr/hal-00631609v1
