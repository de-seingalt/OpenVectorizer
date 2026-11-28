# 01 — Sub-pixel colour edge & ridge detection in scale space (research digest)

Scope: the foundation of a raster feature-detection stage for a from-scratch Rust image→SVG vectorizer. Everything below is re-implementable from the equations. License flags: **[GPL]/[AGPL] = read the paper only, do not copy or port the code.**

Notation: image channels I_c(x,y), c = 1..C. Gaussian g_σ(x) = exp(-x²/2σ²)/(√(2π)σ); scale t = σ². L = g_σ * I. Subscripts denote partial derivatives. Pixel centres are at integer coordinates (pick ONE convention, document it, and apply it consistently in pyramids and SVG output, where pixel (i,j) covers [i, i+1]: SVG_x = x + 0.5).

---

## 1. Gradient computation for colour images

### 1.1 Operators
- **Sobel** 3×3: Gx = [1 2 1]ᵀ ⊗ [-1 0 1] / 8. Cheap, anisotropic. Orientation error is several degrees off-axis.
- **Scharr** 3×3 (Scharr 2000, PhD thesis, Heidelberg, "Optimale Operatoren in der digitalen Bildverarbeitung"; also Weickert & Scharr 2002, JVCIR 13, "A scheme for coherence-enhancing diffusion filtering with optimized rotation invariance"): Gx = [3 10 3]ᵀ ⊗ [-1 0 1] / 32. This is the OpenCV variant. Much better rotation invariance than Sobel. A 5×5 optimised version also exists.
- **Gaussian derivatives** (recommended). Separable: Lx = (g'_σ ⊗ g_σ) * I, Ly = (g_σ ⊗ g'_σ) * I. Scale is a free, continuous parameter, the kernel is exactly rotation-invariant in the continuum, and the same machinery gives 2nd and 3rd derivatives for ridges and scale selection. Cost per pixel: 2·(2r+1) multiply-adds per output, with radius r = ceil(4σ) (3σ is the minimum; use 4σ when derivatives must be accurate).
- Equivalent cheap form, used in the IPOL Devernay code: blur with g_σ, then take central differences gx = L(x+1,y) − L(x−1,y), gy = L(x,y+1) − L(x,y−1) (a factor 2 is omitted). The IPOL article shows that the finite-difference error partly *cancels* the parabolic-fit bias (see §3).

**Discretisation pitfalls**
- A sampled Gaussian is poor for σ < ~0.8 (aliasing, loss of normalisation). Keep σ ≥ 0.7–1.0 at the working resolution. Below that, use Lindeberg's discrete analogue (T(n;t) = e^{-t} I_n(t), modified Bessel function) or integrate the Gaussian over pixel bins. See Lindeberg 2024, "Approximation properties relative to continuous scale space for hybrid discretizations of Gaussian derivative operators", arXiv:2405.05095.
- Renormalise sampled kernels: Σ k = 1 for g, and Σ x·k'(x) = −1 for g'. The second-derivative kernel needs Σ k'' = 0 and Σ x²k''/2 = 1. Without this, gradient magnitudes drift with σ, which breaks thresholds and scale selection.
- Borders: mirror (half-sample symmetric) extension. Zero padding creates false edges.
- Work in f32. f64 is only needed for accumulating global statistics.
- Separable convolution is O(N·r). For σ > ~5, use a recursive IIR Gaussian (Young–van Vliet, Deriche) or an image pyramid (§4.4).

### 1.2 Di Zenzo multi-channel gradient (structure tensor)
Paper: S. Di Zenzo, "A note on the gradient of a multi-image", CVGIP 33(1):116–125, 1986.
For channel weights w_c:
 g11 = Σ_c w_c (∂x L_c)², g22 = Σ_c w_c (∂y L_c)², g12 = Σ_c w_c ∂x L_c ∂y L_c
 λ± = ½[(g11+g22) ± √((g11−g22)² + 4 g12²)]
 θ = ½·atan2(2 g12, g11 − g22)  (direction of maximal change; defined only modulo π)
 edge strength E = √λ+ (or √(λ+ − λ−) to discount isotropic texture/noise; Sapiro & Ringach 1996, "Anisotropic diffusion of multivalued images...")
- **Do not** smooth the tensor (integration scale ρ = 0) for edge *localization*. A smoothed structure tensor (ρ > 0) is fine for orientation estimation and corners but blurs position.
- **Sign ambiguity**: θ is only defined mod π. NMS needs only the axis. Chaining and "which side is dark" (§7) need a signed normal. Orient n = (cos θ, sin θ) so that n · ∇L_lum > 0 (luminance or Oklab L channel). When the L gradient is ~0 (isoluminant edge), use the channel with the largest |n·∇L_c|.
- λ− / λ+ ≈ 0 for a clean edge (rank-1 tensor). A large λ− signals a junction/corner or opposing channel gradients. Keep it as a per-point "junction-ness" feature.
- Cheap alternative: the per-pixel max-magnitude channel (used by OpenCV's colour Canny). It causes orientation jumps where the dominant channel switches. Di Zenzo is smoother and costs barely more.

### 1.3 Colour space
- Compute edge *strength* in a perceptually uniform space, so that a threshold means "visible difference". **Oklab** (B. Ottosson 2020, https://bottosson.github.io/posts/oklab/, reference code MIT-licensed) is cheap and better behaved than CIELAB in blue hues. Pipeline: sRGB → linear (EOTF) → LMS → cube root → Lab. Matrices:
 l = 0.4122214708 r + 0.5363325363 g + 0.0514459929 b; m = 0.2119034982 r + 0.6806995451 g + 0.1073969566 b; s = 0.0883024619 r + 0.2817188376 g + 0.6299787005 b
 l', m', s' = cbrt(l, m, s)
 L = 0.2104542553 l' + 0.7936177850 m' − 0.0040720468 s'
 a = 1.9779984951 l' − 2.4285922050 m' + 0.4505937099 s'
 b = 0.0259040371 l' + 0.7827717662 m' − 0.8086757660 s'
 Oklab ΔE ≈ Euclidean distance. The JND is ~0.02 in Oklab units (L ∈ [0,1]). Weights w = (1,1,1) are the natural choice. Raising the L weight to 1.5–2 makes the detector favour luminance edges, as in human vision.
- CIELAB (ΔE76) is acceptable. ΔE2000 is not a Euclidean metric, so it cannot feed a tensor.
- **Localization space pitfall (important for a vectorizer).** Optical blur happens in *linear* light, but most SVG renderers anti-alias by blending in *gamma-encoded* sRGB. The cube root (Oklab) and the sRGB OETF move the inflection point of a blurred high-contrast step towards the dark side by up to a few tenths of the blur σ. Recommendation: detect and threshold in Oklab, then re-localise the final sub-pixel position on the channel space that the renderer will reproduce. In practice that is gamma-encoded sRGB, so the 50% crossing reproduces the raster's anti-aliasing. Make this a switch and measure it on synthetic anti-aliased renders.
- Alpha: premultiply first, or treat α as an extra Di Zenzo channel with a large weight.

---

## 2. Canny: NMS, hysteresis, automatic thresholds

Paper: J. Canny, "A computational approach to edge detection", IEEE PAMI 8(6):679–698, 1986. Canny's criteria are detection, localization and single response. The derivative of Gaussian is a near-optimal step-edge filter.

### 2.1 Non-maximum suppression (NMS)
Classic: at pixel p with unit normal n, compare E(p) with E(p ± n), using bilinear interpolation of E on the grid. Keep p if E(p) > E(p+n) and E(p) ≥ E(p−n). The asymmetric > / ≥ breaks plateau ties exactly once.
**Grompone/Randall modification** (IPOL 2017, below): do not interpolate along n. Instead:
- if |gx| ≥ |gy| and E(x−1,y) < E(x,y) ≥ E(x+1,y), the point is a "horizontal" edge point, interpolated along x;
- else if |gx| ≤ |gy| and E(x,y−1) < E(x,y) ≥ E(x,y+1), it is a "vertical" edge point, interpolated along y.
This removes the oscillating zig-zag artefact that Devernay's correction produces on 45° edges (both diagonal neighbours become maxima) and improves accuracy. **Recommended.**

### 2.2 Hysteresis
Points with E > H are seeds. Grow through connected candidates with E > L. Use an explicit stack (no recursion), 8-connectivity on the NMS map, or chain connectivity if chaining comes first (IPOL order: gradient → NMS points → chaining → hysteresis along chains). O(N). Typical ratio L/H = 0.4–0.5.

### 2.3 Automatic threshold selection
1. **Noise-model thresholds (recommended; principled, scale-aware).** For i.i.d. Gaussian noise of std σ_n per channel, a Gaussian-derivative gradient component at scale σ has variance
 σ_g² = σ_n² ∫∫ (∂x g_σ)² = σ_n² / (8π σ⁴).
 The gradient magnitude in a flat region is Rayleigh distributed: P(|∇L| > T) = exp(−T²/2σ_g²), so T(p) = σ_g √(−2 ln p).
 For C channels with equal noise (Di Zenzo trace), trace/σ_g² ~ χ²_{2C}, and λ+ is bounded by the trace, so thresholding the trace is conservative. Take H at p_H ≈ 1e-4–1e-6 per pixel. A false *chain* also needs several adjacent maxima, so p per pixel can be loose. Take L at p_L ≈ 1e-2.
 If σ_n is estimated in sRGB, propagate it to Oklab through the local Jacobian, or estimate it directly on the Oklab channels (simplest).
 Add a **perceptual floor**: H ≥ ΔE_min·(peak gradient of a unit step at scale s) = ΔE_min / (√(2π) s), where s² = σ² + σ_blur², with ΔE_min ≈ 0.02–0.04 Oklab. This keeps invisible but statistically significant edges out of the SVG.
2. **a-contrario / Helmholtz validation** of whole chains. NFA = N_tests · Π_i P(|∇| ≥ |∇(x_i)|) < ε = 1. This is parameter-free and controls false detections per image. It is used in EDPF (Akinlar & Topal 2012, "EDPF: a parameter-free edge segment detector with a false detection control", IJPRAI 26(1)) and in Grompone von Gioi & Randall's "Unsupervised smooth contour detection" (IPOL 2016) [AGPL code]. P can be the empirical distribution of |∇| over the image or the Rayleigh model. Good second stage after the noise thresholds.
3. **Gradient statistics** (fallback): H = q-quantile of E over the image, as in MATLAB (assumes about 70% non-edge pixels, L = 0.4H); scikit-image offers `use_quantiles`. Otsu on the E histogram (Otsu 1979, IEEE SMC 9(1)). Rosin's unimodal thresholding (P. Rosin, "Edges: saliency measures and automatic thresholding", Machine Vision & Applications 9:139–159, 1997) suits the unimodal, heavy-tailed E histogram better than Otsu. Medina-Carnicer et al. (2009–2011) also proposed hysteresis candidate selection. These are image-content dependent, which makes them bad for flat logos (few edges) and for very busy photos.

---

## 3. Sub-pixel localization

### 3.1 Devernay (parabolic fit on gradient-magnitude maxima)
- F. Devernay, "A non-maxima suppression method for edge detection with sub-pixel accuracy", INRIA RR-2724, 1995.
- R. Grompone von Gioi & G. Randall, "A Sub-Pixel Edge Detector: an Implementation of the Canny/Devernay Algorithm", IPOL 7:347–372, 2017. https://www.ipol.im/pub/art/2017/216/ — **code AGPL-3.0 [AGPL]**. The paper itself is fully detailed (pseudo-code, Algorithms 1–5).
- With a = E(p−d), b = E(p), c = E(p+d), where d is the interpolation direction from §2.1 (unit step along x or y):
 η = ½ (a − c) / (a − 2b + c)  (|η| ≤ ½ guaranteed when b is a strict maximum)
 The sub-pixel point is p + η·d. The offset is applied along the axis, not along n. Pairing with the axis-aligned NMS keeps the geometry consistent.
- Accuracy (IPOL analysis, Gaussian-blurred step, noise-free). The error of the parabolic model is largest at offsets between 0 and 0.5 and shrinks as the edge σ grows. Errors are a few hundredths of a pixel for σ = 0.5–1 (figure axes reach 0.05–0.08 px for the original NMS on oblique edges). Central differences on a Gaussian-blurred image partially compensate the bias. With the modified NMS, the oblique-edge oscillation disappears.
- Cost: O(1) per candidate. This is the main reason to choose it.
- Optional bias correction: for known (σ_total, η), the parabola-vs-Gaussian bias is a deterministic function. Tabulate it once by simulation and subtract it (η_corr = η − β(η, s)).

### 3.2 Trujillo-Pino partial area effect (PAE)
- A. Trujillo-Pino, K. Krissian, M. Alemán-Flores, D. Santana-Cedrés, "Accurate subpixel edge location based on partial area effect", Image and Vision Computing 31(1):72–90, 2013, doi:10.1016/j.imavis.2012.10.005.
- Code: MATLAB File Exchange #48908 by the author (https://www.mathworks.com/matlabcentral/fileexchange/48908; license behind the "View License" link, not verified). Python port `subpixel-edges` (github.com/gravi-toni/subpixel-edges), **MIT**, readable.
- Model: each pixel value equals the area-weighted mean of intensities A and B on either side of a curve y = a + b·x + c·x² passing through the window. For a mostly horizontal edge (|Fy| ≥ |Fx|), take three 9-pixel columns (x = −1, 0, 1, rows −4..4) centred on the NMS pixel, with column sums S_L, S_M, S_R and side intensities A, B estimated near the window ends. Exact area integration gives (fixed window, rows −4..4):
 c = (S_L + S_R − 2 S_M) / (2(A − B))
 b = (S_R − S_L) / (2(A − B))
 a = (2 S_M − 9(A + B)) / (2(A − B)) − c/12
 Edge position: (0, a) relative to the pixel centre. Slope b gives the normal n ∝ (−b, 1). Curvature κ = 2c / (1 + b²)^{3/2}. Contrast = A − B.
 The general "floating limits" version (window adapted per column, from l1..l2, m1..m2, r1..r2 bounds where |F_y| stops decreasing) is in the MIT port. Use it near neighbouring edges. Swap the roles of x and y for vertical edges.
- It is exact (to model precision) for straight and parabolic edges in *noise-free, un-smoothed* images. On noisy images the paper smooths with a 3×3 kernel and derives modified formulas. The iterative variant (restore a synthetic image from detected edges, subtract, re-estimate) handles nearby edges and noise.
- Accuracy: errors in the 1e-3–1e-2 px range noise-free. It degrades gracefully with noise and beats moment-based methods in the authors' tests. It also gives **curvature and contrast per point for free**, which is valuable for a vectorizer (curve fitting priors).
- Limits: it assumes a sharp step, i.e. a box-filter (area-sampling) camera model. It is perfect for anti-aliased digital art and screenshots, and biased for defocus-blurred photos (blur > ~1 px), where Devernay or an erf fit is better. It assumes a single edge in a 3×9 window and fails near junctions and on strokes thinner than ~3 px.

### 3.3 Other accurate methods
- **Facet model**: R. Haralick, "Digital step edges from zero crossing of second directional derivatives", IEEE PAMI 6(1):58–68, 1984. Fit a cubic polynomial in a 5×5 window and solve for the zero of the second directional derivative along the gradient. Cost similar to Devernay.
- **Moments**: Tabatabai & Mitchell, "Edge location to subpixel values in digital imagery", IEEE PAMI 6(2), 1984 (grey moments); Lyvers et al., PAMI 11(12), 1989 (spatial moments); Ghosal & Mehrotra, "Orthogonal moment operators for subpixel edge detection", Pattern Recognition 26(2):295–306, 1993 (Zernike). They are sensitive to window-size effects and nearby edges.
- **Model fitting** (Gauss–Newton/LM): fit I(x) = B + (A−B)·Φ((n·x − ρ)/s) (erf edge) over a strip along the normal. Parameters ρ, θ, s, A, B: position, orientation, *blur*, and both colours. This is the most accurate and gives a covariance matrix (§7). It costs ~10–50 µs per point. Use it as an optional refinement on final chains, not at detection time. It returns the blur s, which is useful for deciding between a hard SVG edge and a gradient.
- **Zero crossing of L_vv** (Lindeberg differential edge definition, §4): L_vv = 0, L_vvv < 0, with sub-pixel position from linear interpolation of L_vv along the normal. This is equivalent in spirit to Devernay at the same σ.
- Rule of thumb, noise-free: pixel-level Canny gives ±0.5 px; Devernay ~0.02–0.05 px; PAE/erf fit < 0.01 px. With noise, the variance floor of §7 dominates all of them.

---

## 4. Scale space and scale selection

Key papers:
- T. Lindeberg, "Edge detection and ridge detection with automatic scale selection", IJCV 30(2):117–154, 1998. Tech report: https://people.kth.se/~tony/papers/cvap191.pdf.
- T. Lindeberg, "Feature detection with automatic scale selection", IJCV 30(2):79–116, 1998.
- Lindeberg, *Scale-Space Theory in Computer Vision*, Kluwer 1994.

### 4.1 γ-normalized derivatives
∂_{ξ,γ-norm} = t^{γ/2} ∂_x. Local gradient frame: v is along the gradient and u is tangent. Edge strength measures:
 G_γ-norm L = t^γ (Lx² + Ly²) = t^γ L_v²
 T_γ-norm L = −t^{3γ} (Lx³Lxxx + 3Lx²Ly Lxxy + 3Lx Ly² Lxyy + Ly³ Lyyy)  (= −t^{3γ} L_v³ L_vvv)
Differential edge definition at scale t: L_vv = 0 and L_vvv < 0, where
 L_v² L_vv = Lx² Lxx + 2 Lx Ly Lxy + Ly² Lyy.
For colour, apply the definition to the Di Zenzo λ+ in place of L_v² (Lindeberg's theory is scalar).

### 4.2 Edge scale selection
For a diffuse step of blur variance t0 (I = h·Φ(x/√t0)), G_γ-norm is maximal over scales at t = γ t0 / (1−γ). **γ = ½ gives t_sel = t0**: the selected scale equals the edge blur. (T_γ-norm with γ = ½ selects t = 3t0/10, a different constant.) The scale-space edge is a surface in (x,y,t) where ∂_t(G_γ-norm L) = 0 and ∂_tt < 0, intersected with L_vv = 0.
Practical simplified version (recommended, much cheaper than tracking surfaces):
1. Compute at scales σ_k = σ_0·2^{k/3}, k = 0..K. With σ_0 ≈ 1 and K up to log2(σ_max)·3, three levels per octave is enough.
2. At each NMS edge point found at fine scale, compute G_k = σ_k·|∇L_k|² (γ = ½ ⇒ t^γ = σ). Fit a parabola in log σ around the maximum ⇒ σ_sel, and from it blur_σ ≈ σ_sel.
3. Localize at the *smallest* scale with an acceptable SNR (σ_loc ≈ max(σ_min, ~0.5σ_sel)), not at σ_sel, because a coarse scale shifts curved edges inward (bias ≈ κσ²/2 for curvature κ) and merges nearby edges. Use σ_sel as a *blur attribute* and to set the localization window.
Lindeberg's IJCV paper also defines the saliency of a scale-space edge curve as ∫ (G_γ-norm L) ds over the curve. This makes a good chain-ranking score.

### 4.3 Ridge scale selection
Hessian eigenvalues L_pp, L_qq (p, q are principal directions):
 L_pp,qq = ½[(Lxx + Lyy) ∓ √((Lxx − Lyy)² + 4Lxy²)]
Ridge strength measures:
 M_γ-norm = t^γ max(|L_pp|, |L_qq|)
 N_γ-norm = t^{2γ}(L_pp² − L_qq²)² (most ridge-specific)
 A_γ-norm = t^{2γ}((Lxx − Lyy)² + 4Lxy²)
For a Gaussian ridge of variance t0, **γ = ¾ gives t_sel = t0**, so the selected scale reflects the ridge width. γ = 1 would give 2t0.
Ridge definition: L_p = 0, L_pp < 0 (bright ridge; flip the sign for dark strokes), with |L_pp| ≥ |L_qq|.

### 4.4 Pyramids and when to downsample
- An octave pyramid (as in SIFT): blur level o to σ_rel = 2^{k/S}. At the octave's last level (σ = 2 relative), decimate by 2. Derivatives at σ_eff = 2^o·σ_rel cost O(N·4^{−o}).
- Coordinate mapping with integer pixel centres and 2× decimation sampling even pixels: x_full = 2^o·x_o. If decimation averages 2×2 blocks, it is x_full = 2^o (x_o + ½) − ½. Get this wrong and every coarse edge is shifted by 0.25–0.5 px.
- Rule: never localize the *final* position on a decimated level. Use coarse levels only for detection, scale selection, and seeding at σ ≥ 2 (where decimation after σ ≥ 1 smoothing is nearly alias-free). Then re-localize at full resolution with a small window around the seed.
- For very large images (> ~16–25 MP), run detection on a 2× (or 4×) decimated image and re-localize at full resolution only in a band of ±3σ around detected chains. This cuts cost ~3–4× with no accuracy loss.
- Memory: f32 planes, 3 Oklab channels × 5 derivative planes is ~60 bytes/px. Tile with a ±4σ apron for large images; tiles also parallelise trivially (rayon-like scoped threads, written by hand if needed).

---

## 5. Noise estimation from the image

- **Immerkaer**: J. Immerkær, "Fast noise variance estimation", CVIU 64(2):300–302, 1996. The mask N = [[1,−2,1],[−2,4,−2],[1,−2,1]] is the difference of two Laplacian discretisations, which almost annihilates smooth structure. Σ N_ij² = 36.
 σ_n = √(π/2) · 1/(6 (W−2)(H−2)) · Σ_{interior} |I * N|
 It is O(N), one pass, per channel. Pitfall: edges and texture inflate it.
- **Robust variant (recommended)**: σ_n = median(|I * N|) / (0.6745 · 6), i.e. the MAD with 6 = √36 = the noise gain of N. Compute it only over pixels whose Sobel magnitude is below its 50–90% quantile, iterating once (this is the edge-masking variant of Immerkaer from later literature). Equivalent wavelet form: Donoho & Johnstone 1994, σ = median(|HH₁|)/0.6745 on the finest diagonal Haar/Daubechies subband.
- **Ponomarenko / Colom-Buades** (IPOL: M. Colom & A. Buades, "Analysis and extension of the Ponomarenko et al. method, estimating a noise curve from a single image", IPOL 2013): DCT-block based, gives *signal-dependent* noise curves σ_n(intensity). Use it for camera photos (Poisson–Gaussian noise). Code license per the IPOL page, typically GPL/AGPL; read the paper only.
- **PCA patch method**: Liu, Tanaka, Okutomi, "Single-image noise level estimation for blind denoising", IEEE TIP 22(12), 2013 (lowest eigenvalue of patch covariance over weakly textured patches). More robust, heavier.
- Practical notes: (1) estimate per channel, in the same space where thresholds apply (Oklab). (2) Clamp to a floor: 8-bit quantization noise is 1/√12 LSB ≈ 0.29/255. Use σ_n,min ≈ 0.5/255 in sRGB terms, otherwise flat digital art gives σ_n = 0 and infinite sensitivity. (3) JPEG: noise is blocky, non-white and concentrated near edges (ringing). Immerkaer underestimates it in flat areas. Detect JPEG input (8×8 periodicity of |I*N|) and inflate σ_n or raise the floor to ~2–3/255. Ringing produces parallel ghost edges that the perceptual floor (§2.3) should remove. (4) Noise after Gaussian smoothing: σ_L = σ_n/(2√π σ) in 2D, if σ_n is white.

---

## 6. Ridge detection for thin strokes (Steger)

- C. Steger, "An unbiased detector of curvilinear structures", IEEE PAMI 20(2):113–125, 1998. Also C. Steger, dissertation "Unbiased extraction of curvilinear structures from 2D and 3D images", TU München 1998 (https://iuks.in.tum.de/_media/members/steger/publications/1998/diss-1998-steger.pdf). He later joined MVTec (HALCON `lines_gauss`). We found no patent in our search. Any 1990s patent would in any case have expired by now (20-year term).
- Code: ImageJ "Ridge Detection" plugin (Wagner & Hiner; github thorstenwagner/ij-ridgedetection) is **GPL-2.0 [GPL]**. The PyPI "ridge-detection" package is a translation of that Java code, so treat it as GPL as well. Read the paper and dissertation only.

Algorithm (2D, one channel; for colour use the Oklab L channel, or the Hessian of the channel with the largest |λ|):
1. At scale σ compute r_x, r_y, r_xx, r_xy, r_yy (Gaussian derivatives).
2. Hessian eigen-decomposition: n = (n_x, n_y) is the unit eigenvector for the eigenvalue of largest |·| (the line normal). Line strength s = −λ_max for bright lines (λ_max < 0), or +λ_max for dark lines.
3. Second-order Taylor expansion along n:
 t = −(r_x n_x + r_y n_y) / (r_xx n_x² + 2 r_xy n_x n_y + r_yy n_y²)
 Accept the point if |t n_x| ≤ ½ and |t n_y| ≤ ½ (the extremum lies inside the pixel). The sub-pixel centre is p + t·n.
4. Hysteresis on s with thresholds (L, H), then linking (step 6).
5. **Scale choice** for a bar profile of half-width w: requires σ ≥ w/√3 so that r'' has a single extremum at the centre. The ImageJ plugin's helper uses σ = W/(2√3) + 0.5 for full width W. Thresholds follow from the model: |r''(0)| = 2h·w·exp(−w²/2σ²)/(√(2π) σ³) for contrast h. Set H, L from the expected contrast (or from the noise model: Var(r_nn) = 3σ_n²/(16π σ⁶) in 2D, the same Rayleigh/Gaussian tail logic as §2.3). Alternatively use γ = ¾ scale selection (§4.3).
6. **Linking** (Steger): from the strongest unprocessed point, step to the neighbour among the 3 pixels in the direction of the line tangent (⊥ n) that minimises distance + angle difference. Continue while s > L, in both directions. Junctions are handled by stopping when the next point already belongs to another line.
7. **Width and bias correction**: from each centre point, search along ±n for the maximum of |∇r| (the edge points, sub-pixel by parabolic fit) up to ~2.5σ. This gives raw half-widths w_l, w_r and the gradient values at both edges, hence the asymmetry a = (h_right/h_left contrast ratio) of the "asymmetric bar". Steger derives the scale-space mapping (w, a) → (v_σ = observed width/σ, r = gradient ratio) and *inverts it with a precomputed 2D lookup table* to recover the true width and the true centre (the bias shift is ∝ a). Build the table by simulating the analytic profile; there is no need to copy any code.
8. Accuracy: Steger reports sub-pixel centre errors well below 0.1 px and accurate widths for widths ≥ ~2σ when bias correction is used. Without correction, asymmetric lines (different background on each side) bias toward the weaker side.

Decision rule for the vectorizer: a stroke of width W ≲ 3–4 px yields two edge chains too close for independent Canny localization (their responses interact at σ ≳ W/2). Detect it as a ridge (centreline + width) and emit an SVG stroke. For W ≳ 4 px use two edge chains, or a filled region. Run both detectors and arbitrate per chain by γ-normalized response: ridge N_γ-norm (γ = ¾) vs edge G_γ-norm (γ = ½), both at their selected scales.

---

## 7. Edge linking and per-point uncertainty

### 7.1 Chaining (from the IPOL Canny/Devernay paper, recommended as-is)
- Candidates are the sub-pixel points; each belongs to one pixel. Search for links in the **5×5 neighbourhood** (8-connectivity extended one step, which bridges small gaps under noise).
- Link A→B only if **g(A)·g(B) > 0** (orientation within 90°) and the link is "forward", **AB·g(A)^⊥ > 0** (the dark side stays on the same side of the chain). This gives an oriented contour, with the bright side on a fixed side, which maps directly to SVG fill-region boundaries.
- Each point has at most one forward and one backward link (no Y shapes). When conflicting, keep the **shorter** link and cut the other. Known flaw: the result depends on exploration order. Fix: collect all candidate links, sort them by length, and accept greedily (Kruskal-like). This is deterministic and order-independent at O(n log n).
- Then hysteresis is applied per chain. A chain survives if any point exceeds H; points below L are dropped (splitting the chain).
- Closed contours: detect a chain whose tail links back to its head.
- Junctions: Canny/Devernay chains stop ~σ–2σ short of T/Y-junctions (gradient direction undefined, λ−/λ+ large). Record the endpoints and their tangents. Junction completion belongs to a later topological stage; the λ−/λ+ map from §1.2 helps locate junctions.

### 7.2 Positional uncertainty (linearized, our derivation; consistent with R. Kakarala & A. Hero, "On achievable accuracy in edge localization", IEEE PAMI 14(7):777–781, 1992)
Model: straight step of contrast h, optical blur σ_b, white noise σ_n (per pixel, unit spacing), detection scale σ, s² = σ² + σ_b². The edge is the zero of L_vv. A noise perturbation n_vv shifts it by δ = −n_vv / L_vvv(0), with
 L_vvv(0) = −h / (√(2π) s³), Var(n_vv) = σ_n² ∫∫ (g_σ,xx)² = 3σ_n² / (16π σ⁶)
⇒ **Var(δ) = (3/8) · (σ_n/h)² · (s/σ)⁶**, std(δ) ≈ 0.61 (σ_n/h)(1 + σ_b²/σ²)^{3/2}.
- For a sharp edge (σ_b ≪ σ): std ≈ 0.61 σ_n/h, independent of σ for straight edges. In 1D a larger σ worsens localization (Canny's tradeoff), but in 2D it also averages along the edge, and the two effects cancel. Curvature breaks this: averaging along a curved edge adds a bias ≈ κσ²/2 towards the centre of curvature. The optimal σ therefore balances noise against curvature and neighbouring edges.
- For σ = σ_b: std ≈ √3·σ_n/h. For example, h/σ_n = 20 gives ≈ 0.09 px. Very blurry edges detected at too small σ blow up as (σ_b/σ)³, which is the justification for scale selection (σ_loc ≳ σ_b/2).
- Colour: replace h²/σ_n² by the Di Zenzo SNR, Σ_c w_c h_c² / σ_n,c² (along n).
- Add the deterministic model bias in quadrature-free form (bias is not variance): Devernay parabola bias (≤ ~0.03 px, tabulable), curvature bias κσ²/2, and gamma/colour-space bias (§1.3).
- Tangential uncertainty is effectively infinite for a single point. The *orientation* variance is Var(θ) ≈ Var(n_v-perpendicular derivative)/L_v². In practice, estimate orientation from the fitted curve, not per point.
- If the erf model fit of §3.3 is used, take the covariance directly: Σ = σ_n²(JᵀJ)^{−1}. This is the best per-point uncertainty and also gives the variance of the blur and colour estimates.
- Store per edge point: (x, y), signed normal n, contrast vector (ΔOklab), E, σ_sel (blur), σ_loc, var_normal, junction-ness λ−/λ+, chain id. Downstream curve fitting should weight points by 1/var_normal (weighted least squares / Mahalanobis error along the normal only).

---

## 8. Open-source implementations and licenses

| Implementation | What | License | Use |
|---|---|---|---|
| OpenCV `Canny`, `Sobel`, `Scharr`, `GaussianBlur` (imgproc) | pixel Canny, no sub-pixel | Apache-2.0 (≥4.5; BSD-3 before) | may read/port with attribution |
| OpenCV contrib `ximgproc::EdgeDrawing` (ED, EDPF, EDLines) | parameter-free chains, a-contrario | Apache-2.0 | may read/port |
| scikit-image `feature.canny`, `filters.sato/meijering/frangi`, `hessian_matrix` | Canny with quantile thresholds, Hessian ridges | BSD-3 | may read/port |
| IPOL 2017/216 Canny/Devernay (Grompone & Randall) | sub-pixel NMS + chaining | **AGPL-3.0** | paper only |
| IPOL smooth contours / LSD / noise-curve articles | contours, segments, noise | mostly **GPL/AGPL** (check each page) | paper only |
| ImageJ Ridge Detection (Steger port) + PyPI ridge-detection | Steger lines + widths | **GPL-2.0** | paper only |
| Trujillo-Pino MATLAB (File Exchange 48908) | PAE | not verified (FEX "View License") | paper; verify before reading |
| `subpixel-edges` Python (gravi-toni) | PAE port | MIT | may read/port |
| Oklab reference (Ottosson) | colour space | MIT / public | free |

Other references: Topal & Akinlar, "Edge Drawing: a combined real-time edge and segment detector", JVCIR 23(6):862–872, 2012. Sapiro & Ringach, IEEE TIP 5(11), 1996 (colour tensor). Otsu, IEEE SMC 9(1), 1979.

---

## 9. Recommended pipeline (summary for implementation)

1. Decode → linear RGB → Oklab (f32). Estimate σ_n per channel with robust Immerkaer (MAD, edge-masked) and apply the floor.
2. For scales σ_k ∈ {1, 1.26, 1.59, 2, 2.5, 3.2, 4, ...} (pyramid beyond σ = 2): separable Gaussian-derivative Lx, Ly (plus Lxx, Lxy, Lyy for ridges) per channel. Build the Di Zenzo tensor (weights 1,1,1 or L = 1.5).
3. At σ_0 ≈ 1 (or σ_n-adapted 1–1.5): axis-aligned NMS on √λ+ (§2.1), Devernay parabolic offset, then IPOL chaining with deterministic sorted links. Hysteresis with Rayleigh noise thresholds (p_H = 1e-5, p_L = 1e-2) plus the perceptual floor ΔE ≈ 0.02–0.03 Oklab. Optional a-contrario NFA on chains.
4. Per point: γ = ½ scale selection over the σ_k stack ⇒ blur estimate. If the blur is large, re-localize at σ ≈ σ_sel/2 to σ_sel. Optional PAE (sharp, anti-aliased art) or erf LM fit (blurred photos) for the final sub-pixel position and covariance, done in the renderer's blending space.
5. Ridges in parallel: Steger on Oklab L (and on the strongest chroma channel) with γ = ¾ selection and width/bias LUT. Arbitrate ridge vs edge-pair per structure.
6. Emit oriented chains with per-point attributes and var_normal = (3/8)(σ_n/h)²(s/σ)⁶ + bias terms, for the curve-fitting stage.
Complexity: O(N·Σ_k r_k) for filtering (dominant; ~50–150 ns/px single-thread in optimised Rust per scale), O(#candidates) for NMS, offsets and chaining, O(n log n) for the link sort.
