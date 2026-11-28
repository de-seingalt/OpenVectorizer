# 05 — Survey of open-source reference implementations, licensing, Rust/WASM engineering, benchmarks

Scope: the reference code we can study for each method in an image→SVG pipeline written from scratch in Rust/WASM (MIT/Apache-2.0), the license of each, engineering notes, and evaluation datasets.
Verification method (2026-11-28): GitHub API (`license.spdx_id`, and the LICENSE file or source headers where it mattered), crates.io API, and the project pages. "Verified" means I read the LICENSE file or the file headers myself. Anything else is marked *unverified*.

---

## 0. TL;DR licensing verdict

| Status | Meaning for us | Projects |
|---|---|---|
| **GREEN: port freely** (MIT/BSD/Apache/Unlicense/permissive EULA) | We may read the code, translate it to Rust and keep the structure. Keep the copyright notice in a `THIRD_PARTY.md` or in the file header. | OpenCV and opencv_contrib (Apache-2.0, incl. its **own LSD rewrite**, EdgeDrawing/EDLines, FLD, Harris/Shi-Tomasi, MSER, fitEllipse/fitEllipseDirect), scikit-image (BSD-3: Harris, Shi-Tomasi, **Förstner**, corner_subpix, blob_log/dog/doh, EllipseModel = Halir-Flusser), VLFeat (BSD-2: MSER, covariant detectors), ED_Lib (MIT, authors' code), ELSED (Apache-2.0), VTracer (MIT) and visioncortex (MIT OR Apache-2.0), ImageTracer.js (Unlicense), kurbo (Apache-2.0 OR MIT), lyon_geom (MIT OR Apache-2.0), imageproc (MIT), Graphics Gems / Schneider FitCurves.c (permissive EULA), curve-fit-nd (Blender, BSD-3), fit-curve JS (MIT), fitCurves Py (MIT), PolyVectorization (MIT, authors'), jerith/depixel (MIT), Kreuzer GPU depixel (MIT), diffvg/LIVE/StarVector/OmniSVG (Apache-2.0) |
| **RED: algorithm only** (GPL/AGPL/LGPL) | Read the **paper**. Reading the code to understand details is legally grey; the safe practice is not to have the file open while writing ours (clean room). Never translate it. | Potrace (GPL-2+), IPOL Devernay (AGPL-3), IPOL LSD lsd_1.6 (AGPL-3), ELSDc / ELSD (AGPL-3), AAMED (GPL-2), Jia et al. CNC ellipse (GPL-3), ImageJ Ridge Detection / Steger (GPL-2) and its Python port, AutoTrace (GPL-2), Inkscape (GPL), libdepixelize (GPL-2+/LGPL-2.1+), lib2geom (LGPL-2.1/MPL-1.1), vvanirudh/Pixel-Art (GPL-3), kilobtye/potrace JS (GPL-2), PolyFit (GPL-3, 3D, not relevant anyway) |
| **AMBER: no license / non-commercial / unclear provenance** | Default copyright applies ("all rights reserved"). Treat it like RED. | Fornaciari YAED (SourceForge, no license), h3ct0r/fast_ellipse_detector (says MIT but redistributes the unlicensed YAED: provenance is doubtful), Lu et al. arc-support ellipse (**CC BY-NC-SA**, non-commercial), GlobFit (no LICENSE file), cyang1/depixelize (none), ParadoxRobotics/Steger (none), Kanatani book code (terms unspecified), Hoshyari 2018 (binary only, custom license), pytlsd (repo says MIT but bundles AGPL `lsd.cpp`), lxfhfut/ridge-detector (MIT, but "adaptation of the ImageJ plugin", which is GPL-2) |

Main finding: for **every** RED method there is a GREEN reference we can port instead. LSD → OpenCV `lsd.cpp`. Canny → OpenCV/skimage, plus Devernay's sub-pixel step, which is a 3-line parabola formula taken from the paper. Ridge detection → write Steger from the 1998 paper (it is short). Potrace → VTracer/visioncortex plus the Selinger 2003 paper. Ellipses → OpenCV fitEllipseDirect, skimage EllipseModel and ED_Lib's EDCircles.

---

## 1. Per-method reference implementations

Legend: **A** = authors' reference code. Size is approximate. Quality is a subjective 1–5 for readability and portability.

### 1.1 Edges and sub-pixel edges

| Method | Repo / URL | Lang | License | A? | Size / quality | Notes |
|---|---|---|---|---|---|---|
| Canny + Devernay sub-pixel | IPOL 2017 art. 216 "A Sub-Pixel Edge Detector: an Implementation of the Canny/Devernay Algorithm", Grompone von Gioi & Randall — https://www.ipol.im/pub/art/2017/216/ | ANSI C | **AGPL-3+** (header copy at Software Heritage; IPOL ZIP not re-checked) | A | ~1 kLOC, 4/5 | The article PDF holds full pseudo-code and is the real reference. Devernay's correction: fit a parabola to the gradient magnitude at 3 points along the quantised gradient direction (−1, 0, +1), offset = (a−c)/(2(a−2b+c)). The article also gives chaining via a greedy forward/backward link plus hysteresis. Re-implement from the article. |
| Canny (plain) | OpenCV `imgproc/src/canny.cpp`; imageproc `edges::canny`; skimage `feature.canny` | C++ / Rust / Py | Apache-2.0 / MIT / BSD-3 | – | 4/5 | imageproc's version is simple and readable (MIT, Rust): a good starting skeleton. |
| Rust sub-pixel edges | `subpixel-edge` crate (spartajet, 2025) | Rust | MIT OR Apache-2.0 | – | small, unknown quality | Look at it, don't trust it. Recent and low adoption. |
| Learned/thin edge GT | DexiNed (xavysp) | Py | MIT | A | – | Mostly useful for its BIPED dataset (see §4). |

### 1.2 Line segments

| Method | Repo | Lang | License | A? | Notes |
|---|---|---|---|---|---|
| **LSD** (reference) | IPOL 2012 gjmr-lsd, `lsd_1.6`; GitHub mirror theWorldCreator/LSD | C | **AGPL-3** (verified, header) | A | ~2 kLOC, very clean C, excellent comments. RED. |
| **LSD (OpenCV rewrite)** | opencv `modules/imgproc/src/lsd.cpp` | C++ | **BSD-3/Apache (OpenCV license), verified header** | – | Removed in 3.4.6–3.4.15 and 4.1.0–4.5.3 over the AGPL conflict. Restored after the NFA code was published under MIT. **This is our GREEN port source** (~1.2 kLOC). It has refine modes (none/std/adv). |
| LSD Python wrappers | pytlsd (iago-suarez, rpautrat) | C++ | repo MIT, **bundled `src/lsd.cpp` is AGPL-3** (verified) | – | Trap: the repo-level license is misleading. |
| LSD pure Rust | `lsdetect` crate (2026) | Rust | MIT OR Apache-2.0 | – | Says it is an "independent implementation from the article, no AGPL code". Brand new, 0 stars. Use it only as a cross-check. |
| **EDLines / ED / EDPF / EDCircles** | CihanTopal/ED_Lib | C++ | **MIT** (verified) | **A** (Topal) | ~5 kLOC, 3/5 (dense, OpenCV-dependent I/O). Has ED, EDPF (parameter-free with Helmholtz validation), EDLines, EDCircles (circles **and ellipses**), and colour ED. Best GREEN source for the whole "edge drawing" family. |
| EdgeDrawing (OpenCV) | opencv_contrib `ximgproc/edge_drawing.cpp` | C++ | Apache-2.0 | port of ED_Lib | Cleaner API than ED_Lib. Also includes `detectEllipses`. |
| ELSED (Enhanced Line SEgment Drawing) | iago-suarez/ELSED | C++ | **Apache-2.0** | A | Very fast (a few ms at VGA), modern alternative to EDLines. |
| FLD (Fast Line Detector) | opencv_contrib ximgproc `fast_line_detector.cpp` | C++ | Apache-2.0 | – | Simple, from Lee et al. 2014. |
| Learned (for eval only) | L-CNN (zhou13/lcnn, MIT), HAWP (cherubicXN/hawp, MIT) | Py | MIT | A | Their eval scripts (sAP, APH) are GREEN and reusable for the metrics. |

### 1.3 Corners and keypoints

| Method | Repo | License | Notes |
|---|---|---|---|
| Harris, Shi-Tomasi | OpenCV `corner.cpp`, `featureselect.cpp` (goodFeaturesToTrack, incl. min-distance NMS); skimage `corner_harris`, `corner_shi_tomasi`, `corner_peaks` | Apache-2.0 / BSD-3 | Trivial to write. The structure tensor plus separable Gaussian is the only cost. |
| **Förstner** (sub-pixel corner/junction) | skimage `feature.corner_foerstner` (returns w, q), `corner_subpix` (statistical test corner vs edge) | BSD-3 | The only well-known permissive implementation. Small (~100 LOC Python). |
| FAST-9/12, oriented FAST | imageproc `corners.rs` | MIT | imageproc has **no Harris**: only FAST. |
| Covariant (Harris-Laplace, Hessian) | VLFeat `covdet.c` | BSD-2 | High-quality C. Good reference for scale-space NMS and sub-pixel quadratic refinement. |
| cornerSubPix | OpenCV `cornersubpix.cpp` | Apache-2.0 | Iterative gradient-orthogonality refinement. Very short. |

### 1.4 Ridges / centerlines (Steger)

| Repo | Lang | License | Notes |
|---|---|---|---|
| thorstenwagner/ij-ridgedetection (Wagner & Hiner; derived from Steger's C code) | Java | **GPL-2** (verified) | The de-facto reference, with width estimation and junctions. RED. |
| `ridge-detection` on PyPI (Java→Python translation) | Py | derived from GPL-2 → treat as GPL | RED. |
| lxfhfut/ridge-detector (multi-scale) | Py | MIT label, but "adaptation of ImageJ plugin" | AMBER (provenance). |
| ParadoxRobotics/Steger_line_algorithm | Py | no license | AMBER. Tiny. |
| Hessian vesselness (Frangi/Sato/Meijering) | skimage `filters.frangi/sato/meijering`, `feature.hessian_matrix(_eigvals)` | BSD-3 | GREEN building blocks: Hessian at scale σ, eigen-decomposition. |

Recommendation: write Steger directly from the paper (Steger, TPAMI 1998; the TUM 1996 tech report is free). The core is short. Take the Hessian at σ, the eigenvector n of the largest |λ|, and the second-order Taylor expansion t = −(n·∇I)/(nᵀHn). The point is a line point if |t·n| ≤ 0.5 px, and points are linked along the perpendicular direction. Use skimage's Hessian code (BSD) as a GREEN cross-check.

### 1.5 Ellipse detection and fitting

| Method | Repo | Lang | License | A? | Notes |
|---|---|---|---|---|---|
| **ELSDc** (ellipse+line, a-contrario) | viorik/ELSDc | C | **AGPL-3** (verified) | A | Builds on LSD code. RED. Paper (Pătrăucean et al., IJCV 2017) is detailed. |
| **AAMED** (arc adjacency matrix) | Li-Zhaoxi/AAMED | C++ | **GPL-2** (verified LICENSE) | A | Fastest published real-image detector. RED. |
| **Fornaciari YAED** | sourceforge.net/projects/yaed (2017) | C++ | **none listed** | A ("mikispace") | AMBER. Mirror h3ct0r/fast_ellipse_detector shows MIT on GitHub, but it is a copy of the unlicensed code, so that MIT label is not trustworthy. |
| Lu et al. arc-support LS ellipse (TIP 2019) | AlanLuSun/High-quality-ellipse-detection | Matlab+C++ | **CC BY-NC-SA** (file in repo) | A | AMBER (non-commercial, not OSS). |
| Jia et al. projective-invariant pruning (CNC) | dlut-dimt/ellipse-detector | C++ | GPL-3 | A | RED. |
| EDCircles (circles + ellipses) | ED_Lib / opencv_contrib EdgeDrawing | C++ | MIT / Apache-2.0 | A | **GREEN ellipse detector**. Arc-based, with Helmholtz validation. |
| **Fitzgibbon / Halir-Flusser direct LSQ** | OpenCV `fitEllipseDirect` (Apache-2.0); skimage `measure.EllipseModel` (BSD-3, implements Halir & Flusser 1998) | C++/Py | GREEN | – | Halir-Flusser is roughly 40 lines: a 3×3 reduced eigenproblem. Normalise the points first (centroid, mean distance √2). |
| OpenCV `fitEllipseAMS` | opencv | Apache-2.0 | – | Approximate mean square. A useful alternative. |
| **Kanatani hyper-renormalization** | Companion code of Kanatani, Sugaya, Kanazawa, *Ellipse Fitting for Computer Vision* (Morgan & Claypool 2016), on a Google Sites page; older code on Sugaya's TUT page | C++/Matlab | **terms unspecified** | A | AMBER for code. The **ECCV 2012 paper** "Renormalization Returns: Hyper-renormalization and Its Applications" (free PDF on iim.cs.tut.ac.jp) has a complete step-by-step algorithm: iterate a 6×6 generalized eigenproblem M θ = (1/λ) N θ with weights updated from V₀[ξ]. Re-implement from it. Caveat: it can return a hyperbola on short, noisy arcs. Kanatani's later "ellipse-specific" variant (psiellipse.pdf) handles that, or fall back to Halir-Flusser. |
| Szpak "guaranteed ellipse fitting" (Szpak, Chojnacki, van den Hengel) | sites.google.com/site/szpakz | Matlab | unspecified | A | AMBER. Paper is enough. |
| Survey/datasets hub | zikai1/EllipDet | – | no license (data) | – | 7 datasets with unified GT format (see §4). Links to EDSF, EllDet and others. Check each repo's license. |

### 1.6 Blobs and regions

| Method | Repo | License | Notes |
|---|---|---|---|
| **MSER** | VLFeat `mser.c` (BSD-2; ~1 kLOC, excellent, union-find + extremal region tree, with ellipse fit of regions); OpenCV `mser.cpp` (Apache-2.0, Nistér-Stewénius-style linear time for gray, plus MSCR for colour) | GREEN | VLFeat is the clearest. Port it. |
| LoG / DoG / DoH | skimage `feature.blob_log/blob_dog/blob_doh` (BSD-3); VLFeat SIFT DoG (`sift.c`, BSD-2) | GREEN | Scale-normalised σ²∇²G, 3D NMS, overlap pruning. VLFeat has the sub-pixel/sub-scale quadratic refinement. |

### 1.7 Tracing / vectorization systems

| Project | Repo | Lang | License | A? | Notes |
|---|---|---|---|---|---|
| **Potrace** | potrace.sourceforge.net (1.16), Selinger | C | **GPL-2+** (proprietary "Potrace Professional" sold separately by Icosasoft) | A | RED. The paper "Potrace: a polygon-based tracing algorithm" (2003) is complete: path decomposition, optimal polygon (dynamic programming over straight subpaths), vertex adjustment, corner/curve smoothing with parameter α, curve optimisation. Clean-room from the paper is fine and widely done. |
| Potrace JS port | kilobtye/potrace | JS | GPL-2 | – | RED. |
| **VTracer** | visioncortex/vtracer | Rust | **MIT** (verified) | A | GREEN, Rust, already compiles to WASM (has webapp/nodejs bindings). Repo restructured in 2026 into `crates/vtracer`, `vtracer-cli`, `vtracer-bench`, with a "VT2" changelog. Pipeline: hierarchical colour clustering (stacking, no holes), path walking, simplification ("penalty" on staircase), corner detection, 4-point spline splicing. Docs: visioncortex.org/vtracer-docs. |
| **visioncortex** core | visioncortex/visioncortex | Rust | **MIT OR Apache-2.0** (Cargo.toml) | A | Modules: `clusters`, `color_clusters`, `disjoint_sets`, `path`, `shape`, `polypartition`, `sampler`, `tile_map`. Most directly reusable reference for our pipeline. |
| ImageTracer.js | jankovicsandras/imagetracerjs | JS | **Unlicense** (public domain) | A | GREEN. Simple: palette quantisation, layering, edge-node path scan, interpolation, line/quadratic spline fitting. Good readable baseline. |
| AutoTrace | autotrace/autotrace | C | GPL-2 | A | RED. Has centerline tracing. |
| Inkscape Trace Bitmap | Inkscape | C++ | GPL-2+ | – | RED (wraps Potrace and libdepixelize). |
| **Kopf-Lischinski "Depixelizing Pixel Art"** | No authors' code. libdepixelize (Inkscape, Launchpad, GPL-2+/LGPL-2.1+); vvanirudh/Pixel-Art (Py, GPL-3); **jerith/depixel** (Py, MIT); **falichs/Depixelizing-Pixel-Art-on-GPUs** (Kreuzer, GLSL, MIT, real-time variant); cyang1/depixelize (C++, no license); Jimbly/depixel-js (no license, wraps Kreuzer) | mixed | see list | – | GREEN to port: jerith/depixel (similarity graph, heuristics: curves / sparse pixels / islands, Voronoi cell reshaping, B-spline extraction; no full optimisation step) and Kreuzer (MIT; heuristics on GPU, spline smoothing). LGPL libdepixelize is RED for us (porting = derivative work). |
| Hoshyari et al. 2018 "Perception-Driven Semi-Structured Boundary Vectorization" | cs.ubc.ca/labs/imager/tr/2018/PerceptionDrivenVectorization/ | binary (+ Py GUI) | custom license, binary only (replicability.graphics review) | A | AMBER. Paper plus supplementary only. Their **input dataset and result SVGs** on the project page are useful for visual comparison. |
| **PolyVector fields line-drawing vectorization** (Bessmeltsev & Solomon 2019) | bmpix/PolyVectorization | C++ | **MIT** (verified) | A | GREEN. Centerline vectorization of sketches/line art, frame field + topology. |
| Deep Vectorization of Technical Drawings (Egiazarian et al. 2020) | Vahe1994/Deep-Vectorization-of-Technical-Drawings | Py | MPL-2.0 (file-level weak copyleft) | A | Includes a primitive-fitting refinement. MPL lets us port a file only if it stays MPL, so treat it as algorithm-only. |
| GlobFit (Li et al. SIGGRAPH 2011) | yangyanli/globfit | C++ | **no LICENSE file** | A | AMBER. It is 3D (point-cloud primitives), but the relation-discovery idea transfers to 2D: cluster orientations, parallel/orthogonal/equal-radius/concentric constraints, alternate constraint selection and constrained re-fit. Use the paper. |
| PolyFit (Nan & Wonka 2017) | LiangliangNan/PolyFit | C++ | GPL-3 | A | 3D polygonal surface reconstruction. Not relevant (name clash only). |
| Differentiable: diffvg, LIVE, StarVector, OmniSVG | BachiLi/diffvg, Picsart LIVE, joanrod/star-vector, OmniSVG | Py/C++ | Apache-2.0 | A | Not portable to our CPU budget. Useful for their **datasets and metrics** (§4). diffvg's analytic coverage and its SVG rasteriser are a good GT oracle on the dev machine. |

### 1.8 Curve fitting (Bézier, arcs)

| Project | License | Notes |
|---|---|---|
| **Schneider, "An Algorithm for Automatically Fitting Digitized Curves"**, Graphics Gems I (1990), `FitCurves.c` — erich666/GraphicsGems | **EULA (verified): use in any program, commercial or not; you cannot claim the text as yours or resell it; credit not required** | GREEN. Chord-length parameterisation, LSQ cubic with fixed end tangents, Newton-Raphson reparameterisation, split at max error. ~600 LOC. |
| ideasman42/curve-fit-nd (Blender's `curve_fit_nd`) | **BSD-3** (verified) | GREEN. A much improved Schneider: N-dimensional, corner detection, "refit" mode (iteratively removes knots instead of splitting), offset-based error, and better tangent handling. High-quality C. **Best port target for production Bézier fitting.** |
| soswow/fit-curve (JS), volkerp/fitCurves (Py), sbliven/fitCurves | MIT | GREEN, small Schneider ports. |
| lib2geom `bezier-utils.cpp` (Inkscape) | LGPL-2.1 / MPL-1.1 | RED-ish. Skip. |
| **kurbo** (linebender) `fit.rs`, `simplify.rs`, `arc.rs`, `offset.rs`, `moments.rs` | **Apache-2.0 OR MIT** (verified Cargo.toml, v0.13.1, MSRV 1.85) | GREEN, Rust. `fit_to_bezpath(source: &impl ParamCurveFit, accuracy)` and `fit_to_bezpath_opt` (optimal subdivision) fit cubics by matching **signed area and first moment** with a quartic solver (Levien's method: more robust than LSQ for smooth curves). `SimplifyBezPath` precomputes prefix sums of area/moment for O(1) sub-range queries. `Arc::append_iter(tolerance)` / `to_cubic_beziers` convert arcs to cubics. Ellipse→cubic via `Ellipse` + `Shape::path_elements`. Also `ParamCurveNearest`, `ParamCurveArclen`, and the cubic/quartic root solvers in `common.rs` (`solve_cubic`, `solve_quartic`): portable building blocks. We can depend on kurbo directly if the "no external libs" rule applies only to image processing. Otherwise port the relevant files. |
| **lyon_geom** (nical/lyon, `crates/geom`, v1.0.x) | **MIT OR Apache-2.0** (verified) | GREEN, Rust. `Arc::for_each_cubic_bezier` / `for_each_quadratic_bezier`, SVG arc (endpoint) ↔ center parameterisation (`SvgArc::to_arc`), flattening with tolerance, cubic→quadratic approximation, line/segment intersection, monotonic splitting. The SVG arc flag conversion is exactly what we need when emitting `A` commands. |
| imageproc (image-rs) v0.28 | **MIT** (verified) | GREEN, Rust. Useful modules: `contours.rs` (Suzuki-Abe border following), `geometry.rs` (Douglas-Peucker `approximate_polygon_dp`, convex hull, min-area rect), `region_labelling.rs`, `union_find.rs`, `distance_transform.rs`, `integral_image.rs`, `hough.rs`, `filter` (separable filter, Gaussian), `edges::canny`, `gradients`, `suppress.rs` (NMS), `morphology`. Code quality 3–4/5. Generic over `image` crate types, so we will strip that part when porting. |

---

## 2. Licensing rules we apply (MIT/Apache-2.0 project)

1. **GREEN code** (MIT, BSD-2/3, Apache-2.0, Unlicense, Graphics Gems EULA): may be translated. Keep a `THIRD_PARTY_NOTICES` file with the original copyright line and license text for each source we port from. Apache-2.0 also requires us to state significant changes, and its patent clause is fine for us. BSD-3: do not use the authors' names for endorsement.
2. **GPL/AGPL/LGPL code**: our Rust translation would be a derivative work, and AGPL adds network-use obligations, which matter for a hosted WASM web app. We only implement from **papers**. In practice: write from the paper, keep the RED source closed while writing, and record in a comment which paper we used (e.g. `// Implemented from Selinger 2003, §2.2–2.3`). Algorithms themselves are not copyrightable. Parameter defaults taken from a paper are fine.
3. **No license** = all rights reserved. Same treatment as RED, even if the code is on GitHub.
4. **Watch for license laundering**: pytlsd (MIT repo, AGPL file), h3ct0r (MIT label on unlicensed code), ridge-detector (MIT, but GPL-derived), and new 2025–2026 crates whose provenance cannot be checked. Always read the file headers, not just the GitHub badge.
5. **Patents**: SIFT's patent has expired (2020). MSER, LSD, Harris, Canny, Potrace and Kopf-Lischinski have no known active patents. Microsoft held a patent application on the Kopf-Lischinski work; I did not verify its status, so check it before shipping a pixel-art mode.
6. **Datasets**: most are "research use" and not redistributable. Download them in CI or locally; never commit them to the repo.

---

## 3. Engineering notes for Rust → wasm32

### 3.1 Performance budget (target: 2 MP in < 1 s in a browser, single thread)

Rough throughput of scalar wasm32 (no SIMD) on a 2024–2026 laptop: ~0.5–1.5 G simple f32 ops/s, i.e. ~1.5–2.5× slower than native scalar. simd128 gives a 2–4× speed-up on convolution-type kernels.

| Stage (2 MP = 1920×1080) | Work | Budget |
|---|---|---|
| Decode + RGB→Lab/gray (f32) | 2 M px × ~30 ops | 20–40 ms |
| Separable Gaussian σ≈1.5 (radius 4, 9 taps) ×2 passes | 36 M MAC | 15–40 ms (SIMD: <15) |
| Gradients (Sobel/Scharr, fused into one pass) + magnitude/orientation | ~20 M ops | 10–20 ms |
| Canny NMS + Devernay + hysteresis/chaining | ~2 M px branchy | 30–60 ms |
| Structure tensor + Harris/Förstner (3 separable blurs of Ixx, Ixy, Iyy) | ~60 M MAC | 30–60 ms |
| LSD / EDLines on edge chains | proportional to edge px (~5–10 % of px) | 30–80 ms |
| Ellipse candidate grouping + fits (only on arcs) | small | 10–50 ms |
| Colour segmentation / clustering (VTracer-style hierarchical) | 2 M px union-find | 80–200 ms |
| Contour tracing + polygon + Bézier fitting + SVG serialisation | per boundary px | 50–150 ms |
| **Total** | | **~300–700 ms**, leaving headroom |

Rules: run the multi-scale analysis on a **pyramid** (σ-doubling: blur σ=1 then decimate by 2), and do expensive global steps (segmentation, pixel-art heuristics) at ≤1 MP. Refine the geometry back at full resolution using local sub-pixel fits (Devernay/Förstner) only around detected features. Benchmark with `criterion` natively, then in-browser using `performance.now()` over a fixed corpus. Track the wasm/native ratio.

### 3.2 Convolutions and integral images

- **Separable Gaussian**: horizontal pass row-major into a transposed temp buffer, then horizontal again, which makes the vertical pass cache-friendly. Or do the vertical pass with "row accumulation": for each output row, accumulate k input rows into an f32 row buffer. Both avoid strided column access.
- Kernel radius = ceil(3σ) (Canny/LSD use σ-dependent sizes; LSD uses σ = 0.6/scale after a 0.8 downscale). Normalise taps to sum 1 in f64, then store f32.
- Borders: reflect-101 (OpenCV default, avoids gradient spikes). Handle borders in a separate loop so the interior loop has no branches.
- **Large σ (≥ ~5)**: use 3–4 passes of a box filter via running sums (Wells / Kovesi "Fast almost-Gaussian"), O(1) per pixel regardless of σ. Or use the Deriche / Young-van Vliet recursive IIR (note that IIR runs in f32 may drift; use f64 state for determinism and accuracy).
- **Integral images** (summed-area tables): u32 is enough for u8 input (2,073,600 × 255 ≈ 529 M < 4.29 G). Use u64 above ~16 MP. Use **f64** for sums of squares or of f32 data, otherwise catastrophic cancellation hurts local-variance and box-filter computations. They are used for adaptive thresholds (Sauvola/Bradley), box-filter Hessian (SURF-like), and fast local means.
- Fuse operations: compute Ix, Iy, magnitude² and orientation in one pass. Compute Ixx, Ixy, Iyy as three planes but blur them in one loop over three planes.

### 3.3 SIMD in wasm32 (simd128)

- Enable it with `RUSTFLAGS="-C target-feature=+simd128"` (or `[target.wasm32-unknown-unknown] rustflags` in `.cargo/config.toml`). Intrinsics live in `core::arch::wasm32` (`v128`, `f32x4_add`, `f32x4_mul`, `f32x4_splat`, `v128_load`, `i16x8_*`, `u8x16_*`, `f32x4_pmin/pmax`, `i32x4_trunc_sat_f32x4`).
- Fixed-width simd128 is supported in all evergreen browsers (Chrome 91+, Firefox 89+, Safari 16.4+) and in Node 16.4+. There is no runtime feature detection inside a module, so **ship two builds** (simd and scalar) and pick one in JS with `wasm-feature-detect`. Alternatively accept Safari ≥16.4 as the minimum.
- **Avoid relaxed-SIMD** (`relaxed_madd`, relaxed min/max, relaxed swizzle): its results are implementation-defined across CPUs and engines, which breaks determinism.
- Writing SIMD by hand: keep a scalar reference implementation and a `#[cfg(target_feature = "simd128")]` path, and test them for bit-equality. Use only ops that are exactly IEEE (add/mul/sub/div/sqrt are, and **there is no fused FMA in base wasm**, so it is deterministic). Note: native x86 builds with FMA enabled will **not** be bit-identical to wasm unless we avoid `mul_add` and avoid `-C target-cpu=native`.
- Auto-vectorisation by LLVM works for simple `chunks_exact(4)` loops over `&[f32]` once simd128 is on. Check the output with `wasm-objdump -d` or `twiggy`. Portable `std::simd` is still nightly-only. Avoid crates as the rule says, but `fearless_simd` (linebender) is a reference design for a safe dispatch layer.
- **Threads**: wasm threads require SharedArrayBuffer, i.e. COOP/COEP headers on the hosting page, plus a nightly `-Z build-std` rebuild with `+atomics,+bulk-memory`. Plan single-threaded first, and process in **tiles** (with an overlap = kernel radius) so that parallelism can be added later (rayon-style or Web Workers per tile) without changing the results.

### 3.4 Memory layout and allocation

- **Planar SoA** for images: `struct Plane<T> { w, h, stride, data: Vec<T> }`, one plane per channel (L, a, b, Ix, Iy, …). SIMD-friendly and cache-friendly for separable filters. Use AoS (`[u8;4]` RGBA) only at the I/O boundary (ImageData is RGBA8).
- Feature records (edgels, segments): SoA (`xs: Vec<f32>, ys: Vec<f32>, dir: Vec<f32>`) for hot numeric passes. A small AoS struct is fine for graph and linking structures. Use `u32` indices instead of pointers or `Rc` (arena-style), and a CSR adjacency (offsets + targets) for graphs.
- f32 for pixel planes; **f64 for geometric fitting** (ellipse/Bézier normal equations) and accumulations.
- No allocation in hot loops: preallocate scratch buffers in a `Workspace` struct passed by `&mut`, reuse `Vec`s with `clear()`, and use `Vec::with_capacity` from estimates (edge pixel count). For small temporary lists, an inline-capacity array (write a 20-line `ArrayVec<T, N>`) instead of `Vec`.
- WASM linear memory only grows. Peak memory for 2 MP: each f32 plane is 8 MB, so 10 live planes = 80 MB. Free or reuse planes between stages, and consider u8/i16 planes for gradients (Sobel on u8 fits in i16). Set an initial memory size to avoid repeated `memory.grow` stalls.
- Bounds checks: iterate over `chunks_exact`/`zip` slices or row slices (`&data[y*stride..][..w]`) so LLVM can elide checks. Use `get_unchecked` only behind a debug-assert and benchmarks.
- Allocator: the default dlmalloc in wasm32 is fine. Avoid a "wee_alloc"-style allocator (unmaintained, leaks).
- Build profile: `opt-level=3` (or `"s"` only if size matters more than speed), `lto="fat"`, `codegen-units=1`, `panic="abort"`; then `wasm-opt -O3 --enable-simd`.

### 3.5 Fixed-size linear algebra without a crate

- Use const-generic types: `struct Mat<const R: usize, const C: usize>([[f64; C]; R])`, `#[inline]` everywhere, f64.
- **2×2 symmetric eigen** (structure tensor, Hessian, Steger): closed form. λ = (a+c)/2 ± √(((a−c)/2)² + b²). For the eigenvector, use the numerically stable form: pick (b, λ−a) or (λ−c, b), whichever has the larger norm. Use `hypot` for the root.
- **3×3 symmetric eigen** (Förstner normal equations, plane fits, covariance): analytic trigonometric method (Smith 1961 / Kopp 2008 "Efficient numerical diagonalization of hermitian 3×3 matrices", which also provides a hybrid analytic+QL variant for accuracy). Or cyclic **Jacobi** (simple, robust, ~10 sweeps max, deterministic).
- **Halir-Flusser ellipse**: needs a 3×3 *non-symmetric* eigenproblem M = C₁⁻¹(S₁ − S₂S₃⁻¹S₂ᵀ). Compute the characteristic cubic (`solve_cubic`, which kurbo has under MIT/Apache), then the eigenvector via the cross product of two rows of (M − λI). Choose the vector with 4ac − b² > 0. Always normalise the data first (Hartley-style: centroid to the origin, mean distance √2) and de-normalise the conic afterwards.
- **5×5 / 6×6** (Kanatani hyper-renorm, conic LSQ, Bézier normal equations): symmetric Jacobi eigen for 6×6. Generalized symmetric problem M θ = λ N θ with N indefinite (hyper-renorm): use the paper's formulation, or **inverse iteration** on (M − μN) solved with LU with partial pivoting. For SPD systems (LSQ normal equations, Gauss-Newton), use **Cholesky/LDLᵀ** with a small Levenberg-Marquardt damping. Write **QR (Householder) for tall thin LSQ** (n×4, n×6) when conditioning matters (Bézier fitting with nearly collinear points). It is ~60 lines.
- Polynomial roots: quadratic with the stable `q = −½(b + sign(b)√Δ)` form; cubic/quartic. kurbo `common.rs` has robust `solve_cubic`/`solve_quartic` (port them), and also Gauss-Legendre tables for arclength.
- Test every routine against random matrices: reconstruct A = VΛVᵀ, check orthogonality ‖VᵀV − I‖ < 1e-12, and use property tests with fixed seeds.

### 3.6 Image pyramids and downsampling

- Gaussian pyramid: blur σ = 1.0 (relative to a 0.5 assumed initial blur), decimate ×2. For non-integer factors (LSD uses a 0.8 scale), use Gaussian pre-filter σ = 0.6/s (LSD rule) followed by bilinear or area resampling.
- Area-average downsampling (box) is acceptable for colour segmentation but aliases for gradients. Use Gaussian before decimation for edge work.
- Coarse-to-fine: detect at a coarse level, map coordinates as x_full = (x_coarse + 0.5)·2 − 0.5 (pixel-centre convention, keep one convention everywhere: **pixel centres at integer + 0.5** in SVG space), then refine locally at full resolution.
- Pixel-art inputs: detect a native pixel grid (period estimation from gradient autocorrelation), downscale to the native resolution by nearest/mode, and run the Kopf-Lischinski-style path there. Never blur pixel art.

### 3.7 Determinism

- Use IEEE f32/f64 ops only, with no FMA contraction. Rust never contracts automatically, and wasm has no base FMA. Don't call `f32::mul_add` in shared code.
- Transcendentals: `std` on wasm32-unknown-unknown resolves `sin/exp/atan2/powf` to compiler-builtins' **libm** port (pure Rust), so they are deterministic across browsers. Native builds use the platform libm, so native and wasm may differ in the last ulp. For cross-target bit-reproducibility, implement our own `atan2`/`exp` (minimax polynomials) or call libm-style code explicitly. `sqrt` is exact everywhere.
- Avoid `HashMap`/`HashSet` iteration order: use `BTreeMap`, sorted `Vec`, or a hash map with a fixed hasher (FxHash-style, ~20 lines) and never iterate it to produce output.
- Sorting: `sort_by(|a,b| a.key.total_cmp(&b.key).then(a.id.cmp(&b.id)))` gives a total order with tie-breakers. Use stable sort where order matters.
- RNG (RANSAC, k-means init): our own PCG32/xoshiro with a seed derived from the image hash plus the parameters, never `getrandom`.
- Parallel reductions (later): fixed tile decomposition plus a fixed reduction order. Float sums otherwise vary with thread scheduling.
- Output: format coordinates with a fixed number of decimals using our own formatter or `{:.prec$}`. Normalise `-0.0` to `0`. Emit paths in a deterministic order (by area or z-order, then id).
- CI: golden-file tests (hash of the SVG output for a corpus), run both natively and under `wasm-pack test --node`/`wasmtime`.

---

## 4. Datasets and benchmarks (with ground truth)

### 4.1 Lines

| Dataset | Content / GT | Metric / tooling | Notes |
|---|---|---|---|
| **YorkUrban** (Denis, Elder, Estrada, ECCV 2008) | 102 urban images 640×480, Manhattan-direction line segments (only VP-consistent lines labelled) | Classic precision/recall | GT is incomplete (only Manhattan lines), which penalises precision unfairly. |
| **YorkUrban-LineSegment** (Cho, Lee & Lee, TPAMI 2018, "linelet") | Same 102 images, *all* segments re-annotated | Their eval code (Matlab) | Used by ELSED and other recent LSD-family papers. Best for classical detectors. |
| **Wireframe** (Huang et al., CVPR 2018; huangkuns/wireframe, repo has no license) | 5,462 indoor/man-made images (5,000 train / 462 test), junctions + segments | sAP⁵/¹⁰/¹⁵, APᴴ, Fᴴ — eval code in L-CNN / HAWP (MIT) | Annotation is "wireframe" (structural), so texture lines count as false positives. Report both datasets. |
| HPatches / line-matching sets | Repeatability under homography | Repeatability, localisation error | For stability, not accuracy. |

### 4.2 Edges (incl. sub-pixel)

| Dataset | Notes |
|---|---|
| BSDS500 (Berkeley) | 500 natural images, multiple human boundary maps. ODS/OIS F-measure, benchmark code. Semantic boundaries, so not ideal for low-level edges. |
| **BIPED / BIPEDv2** (Soria et al., DexiNed) | 250 HD images (1280×720) with thin, low-level edge GT. Closer to what a vectorizer needs. |
| Synthetic (ours) | **Main recommendation**: render random SVGs (lines, arcs, ellipses, Béziers, corners of known angle) with exact analytic anti-aliasing (or 16×16 supersampling), add noise, blur and JPEG. Gives exact sub-pixel GT for edges, corners, lines and ellipses. Measure RMS position error, angle error, and endpoint error. This is the only way to evaluate Devernay/Förstner-level accuracy (target < 0.05–0.1 px at SNR ≥ 20 dB). |

### 4.3 Corners

| Dataset | Notes |
|---|---|
| Awrangjeb, Lu & Fraser, "Performance comparisons of contour-based corner detectors" (TIP 2012) | 23 test images with human-marked GT, plus "average repeatability" and "localisation error" under rotation/scale/JPEG/noise. Includes the classic Mokhtarian "blocks/house/lab" images. Matlab code by the authors (check its license). |
| Oxford affine (Mikolajczyk) / HPatches | Repeatability under known homographies. |
| Synthetic (ours) | Polygons with known vertices at varied angles (30°–170°), blur σ and noise. Measure detection rate and sub-pixel error vs angle. |

### 4.4 Ellipses (unified copies in zikai1/EllipDet; cite the originals)

| Dataset | Size | Source |
|---|---|---|
| Synthetic Occluded / Overlapping | 600 + 600 images, 4–24 ellipses each | Pătrăucean / Chia et al. |
| **Prasad** | originally 400 Caltech-256 images, 198 surviving; EllipDet completed them to 400 with re-labelled GT | Prasad, Leung & Cho, PR 2012 |
| **Dataset #1** (Fornaciari) | 400 images, MIRFlickr + LabelMe | Fornaciari et al., PR 2014 |
| **Dataset #2** (Fornaciari) | 629 frames from smartphone videos; the **Traffic sign** subset has 273 images | Fornaciari et al., PR 2014 |
| PCB | 100 images | Lu et al. TIP 2019 |
| Calibration | 40 images, ≥70 ellipses each | Pătrăucean / ELSDc |
| Industrial (EllipDet) | 77 images | zikai1 |
| Metric | Standard: a detection is a TP if overlap ratio (IoU of filled ellipses) ≥ 0.8. Report P/R/F and time. Sub-pixel fitting accuracy is better measured on synthetic data (centre, axes and angle errors vs arc coverage 30°–360°, which is where hyper-renorm vs Halir-Flusser matters). |

### 4.5 Vectorization quality

| Resource | Use |
|---|---|
| Hoshyari et al. 2018 project page: inputs + their results + perceptual study | Visual comparison on clip-art/icons with semi-structured boundaries. Their user study defines what "good" looks like: accuracy, simplicity, regularity, continuity. |
| Kopf-Lischinski supplementary (johanneskopf.de/publications/pixelart) | Pixel-art inputs and result comparisons (vs hqx, Potrace, Adobe Live Trace). |
| "Benchmarking Rough Sketch Cleanup" (Yan, Vanderhaeghe, Gingold, SIGGRAPH Asia 2020) | Rough sketches with clean vector GT, plus metrics for line-drawing vectorization. Relevant to centerline mode. |
| Deep Vectorization of Technical Drawings: PFP floor plans, ABC-derived synthetic drawings | Technical/line drawings with vector GT. |
| StarVector **SVG-Bench** (SVG-Stack, SVG-Icons, SVG-Fonts, SVG-Emoji, SVG-Diagrams) and OmniSVG **MMSVG-Bench** | Large corpora of real SVGs: render → vectorize → compare. GT exists by construction. Check each subset's license (icons/fonts/emoji are mostly OFL/Apache/CC-BY). |
| Self-made round-trip corpus | Render permissively licensed SVGs (Noto Emoji: Apache-2.0; Material Symbols: Apache-2.0; OpenMoji: CC BY-SA, data only; Twemoji graphics: CC-BY 4.0) at several resolutions, with and without AA/JPEG. Primary regression corpus. |
| Metrics | Raster fidelity: PSNR, SSIM, (LPIPS offline) between input and the re-rasterised SVG at 1× and 2× (2× catches cheating at pixel scale). Geometry vs GT SVG: symmetric Chamfer / Hausdorff distance between sampled boundaries, corner P/R within 1–2 px, tangent-angle error. Compactness: path count, control-point count, bytes (gzip). Regularity: fraction of near-horizontal/vertical, parallel and equal-radius primitives snapped. Time and peak memory. Hoshyari-style perception is approximated by "simplicity + fidelity" Pareto plots. |

---

## 5. Recommended port map (what to read for each module of our pipeline)

| Our module | Read and port (GREEN) | Algorithm-only (paper) |
|---|---|---|
| Gaussian/derivatives/pyramid | imageproc `filter`, OpenCV `smooth.dispatch.cpp`, VLFeat `imopv.c` | Young-van Vliet IIR |
| Canny + sub-pixel | imageproc `edges.rs`, OpenCV `canny.cpp` | Devernay 1995 + IPOL 2017 article (AGPL code not used) |
| Line segments | OpenCV `lsd.cpp`, ED_Lib EDLines, ELSED | LSD IPOL 2012 paper for the a-contrario theory |
| Corners | skimage Förstner + corner_subpix, OpenCV goodFeaturesToTrack/cornerSubPix, VLFeat covdet | Förstner & Gülch 1987 |
| Ridges/centerlines | skimage Hessian utilities, PolyVectorization (MIT) | Steger 1998 |
| Ellipses | ED_Lib EDCircles, OpenCV fitEllipseDirect/AMS, skimage EllipseModel | ELSDc, AAMED, Fornaciari, Kanatani hyper-renorm (ECCV 2012) |
| Regions/blobs | VLFeat `mser.c`, skimage blob_*, imageproc `region_labelling`/`union_find` | – |
| Colour segmentation + tracing | **visioncortex + VTracer (MIT/Apache)**, ImageTracer.js (Unlicense), imageproc `contours.rs` | Potrace 2003 paper (GPL code not used) |
| Pixel art | jerith/depixel (MIT), Kreuzer GPU (MIT) | Kopf & Lischinski 2011, Hoshyari 2018 |
| Bézier fitting / simplification | **curve-fit-nd (BSD-3)**, kurbo `fit.rs`/`simplify.rs`, Schneider FitCurves.c (EULA) | – |
| Arcs/ellipses → SVG | lyon_geom `arc.rs`/`svg arc`, kurbo `arc.rs`/`ellipse.rs` | – |
| Regularisation (parallel/orthogonal/concentric snapping) | – | GlobFit 2011 (code has no license), Hoshyari 2018 |

Caveats: the IPOL Devernay license is taken from the Software Heritage archive copy and was not re-read from the ZIP on IPOL. The Graphics Gems terms come from the EULA in erich666/GraphicsGems. The ages of the 2025–2026 Rust crates (lsdetect, subpixel-edge, fast-canny, img2svg) are from crates.io: they are immature, so use them only as cross-checks. Dataset licenses are mostly "research use", so check each one before using it in CI.
