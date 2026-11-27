//! Quality benchmark (`openvectorizer eval`).
//!
//! Each ground-truth SVG is rendered once at its declared size: that render is the
//! **reference**. Degraded **variants** of it (JPEG artifacts, low resolution, blur,
//! noise) are fed to the engine, and the result is always compared to the clean
//! reference — like a real user feeding a damaged logo and expecting the clean one.
//! Plain raster files (no ground truth) only run the `clean` variant against themselves.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use openvectorizer_core::{Options, RgbaImage, vectorize};
use openvectorizer_eval::{Metrics, evaluate, heatmap, render_svg_with, usvg};
use serde::{Deserialize, Serialize};

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Variant {
    /// Exact render of the ground truth.
    Clean,
    /// JPEG quality 75 (typical web logo).
    Jpeg75,
    /// JPEG quality 35 (heavily compressed).
    Jpeg35,
    /// Downscaled to 50 %: the SVG must still match the full-size reference.
    Half,
    /// Gaussian blur, sigma 1.2 (scan / soft edges).
    Blur,
    /// Uniform noise ±16 per channel (deterministic).
    Noise,
}

impl Variant {
    pub const ALL: [Variant; 6] =
        [Variant::Clean, Variant::Jpeg75, Variant::Jpeg35, Variant::Half, Variant::Blur, Variant::Noise];

    fn name(self) -> &'static str {
        match self {
            Variant::Clean => "clean",
            Variant::Jpeg75 => "jpeg75",
            Variant::Jpeg35 => "jpeg35",
            Variant::Half => "half",
            Variant::Blur => "blur",
            Variant::Noise => "noise",
        }
    }

    fn apply(self, reference: &RgbaImage) -> Result<RgbaImage> {
        let img = to_dynamic(reference)?;
        let out = match self {
            Variant::Clean => img,
            Variant::Jpeg75 => jpeg(&img, 75)?,
            Variant::Jpeg35 => jpeg(&img, 35)?,
            Variant::Half => {
                img.resize_exact((img.width() / 2).max(1), (img.height() / 2).max(1), FilterType::Triangle)
            }
            Variant::Blur => img.blur(1.2),
            Variant::Noise => DynamicImage::ImageRgba8(noise(img.to_rgba8(), 16)),
        };
        from_dynamic(out)
    }
}

fn to_dynamic(img: &RgbaImage) -> Result<DynamicImage> {
    let buf = image::RgbaImage::from_raw(img.width(), img.height(), img.data().to_vec())
        .context("invalid RGBA buffer")?;
    Ok(DynamicImage::ImageRgba8(buf))
}

fn from_dynamic(img: DynamicImage) -> Result<RgbaImage> {
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(RgbaImage::new(w, h, rgba.into_raw())?)
}

/// JPEG has no alpha: flatten on white first, like most real-world JPEG logos.
fn jpeg(img: &DynamicImage, quality: u8) -> Result<DynamicImage> {
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let a = u16::from(p[3]);
        for c in 0..3 {
            p[c] = ((u16::from(p[c]) * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(&rgb)?;
    Ok(image::load(Cursor::new(bytes), ImageFormat::Jpeg)?)
}

fn noise(mut img: image::RgbaImage, amplitude: i32) -> image::RgbaImage {
    let mut state: u32 = 0x9E37_79B9; // fixed seed: results must be reproducible
    for p in img.pixels_mut() {
        for c in 0..3 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let delta = (state % (2 * amplitude as u32 + 1)) as i32 - amplitude;
            p[c] = (i32::from(p[c]) + delta).clamp(0, 255) as u8;
        }
    }
    img
}

pub fn load_raster(path: &Path) -> Result<RgbaImage> {
    let img = image::open(path).with_context(|| format!("decoding {}", path.display()))?;
    from_dynamic(img)
}

fn svg_size(svg: &str) -> Option<(u32, u32)> {
    let tag = &svg[svg.find("<svg")?..];
    let tag = &tag[..tag.find('>')?];
    let attr = |name: &str| -> Option<u32> {
        let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let val = &tag[start..];
        val[..val.find('"')?].trim_end_matches("px").parse::<f64>().ok().map(|v| v.round() as u32)
    };
    Some((attr("width")?, attr("height")?))
}

struct Source {
    path: PathBuf,
    /// Report key prefix, e.g. `corpus/flags/fr.svg`.
    name: String,
    /// Parent directory name, e.g. `flags`.
    category: String,
    ground_truth: bool,
}

fn collect(root: &Path, dir: &Path, acc: &mut Vec<Source>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, acc)?;
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) else {
            continue;
        };
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "svg") {
            continue;
        }
        let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let category = path
            .parent()
            .and_then(Path::file_name)
            .map_or_else(|| "root".into(), |s| s.to_string_lossy().into_owned());
        acc.push(Source { ground_truth: ext == "svg", path, name, category });
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct CaseResult {
    #[serde(flatten)]
    pub metrics: Metrics,
    pub ms: f64,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy)]
pub struct Summary {
    pub cases: usize,
    pub mean_delta_e: f64,
    pub mean_p95: f64,
    pub total_nodes: u64,
}

impl Summary {
    fn of<'a>(results: impl Iterator<Item = &'a CaseResult>) -> Self {
        let mut s = Summary::default();
        for r in results {
            s.cases += 1;
            s.mean_delta_e += r.metrics.delta_e_mean;
            s.mean_p95 += r.metrics.delta_e_p95;
            s.total_nodes += u64::from(r.metrics.nodes);
        }
        if s.cases > 0 {
            s.mean_delta_e /= s.cases as f64;
            s.mean_p95 /= s.cases as f64;
        }
        s
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Report {
    pub overall: Summary,
    pub by_variant: BTreeMap<String, Summary>,
    pub by_category: BTreeMap<String, Summary>,
    /// Key: `<source path>@<variant>`.
    pub cases: BTreeMap<String, CaseResult>,
}

pub struct EvalArgs<'a> {
    pub corpus: &'a Path,
    pub out: &'a Path,
    pub baseline: Option<&'a Path>,
    pub max_regression: f64,
    pub variants: &'a [Variant],
    pub filter: Option<&'a str>,
    pub options: &'a Options,
}

pub fn run(args: &EvalArgs) -> Result<()> {
    let mut sources = Vec::new();
    collect(args.corpus, args.corpus, &mut sources)?;
    sources.retain(|s| args.filter.is_none_or(|f| s.name.contains(f)));
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    if sources.is_empty() {
        bail!("no images found in {}", args.corpus.display());
    }
    let mut variants = args.variants.to_vec();
    variants.sort();
    variants.dedup();

    // Fonts for ground-truth SVGs with <text>: only the corpus' own fonts, never system fonts.
    let mut svg_options = usvg::Options::default();
    let fonts = args.corpus.join("fonts");
    if fonts.is_dir() {
        svg_options.fontdb_mut().load_fonts_dir(&fonts);
    }

    let mut cases = BTreeMap::new();
    let mut categories: BTreeMap<String, Vec<String>> = BTreeMap::new();
    println!(
        "{:<40} {:>8} {:>8} {:>7} {:>6} {:>7} {:>6}",
        "case", "ΔE mean", "ΔE p95", "PSNR", "paths", "nodes", "ms"
    );
    for src in &sources {
        let reference = if src.ground_truth {
            let svg = fs::read_to_string(&src.path)?;
            let (w, h) = svg_size(&svg)
                .with_context(|| format!("{}: root <svg> needs width and height", src.path.display()))?;
            render_svg_with(&svg, w, h, &svg_options)
                .with_context(|| format!("rendering {}", src.path.display()))?
        } else {
            load_raster(&src.path)?
        };
        let applicable = variants.iter().copied().filter(|v| src.ground_truth || *v == Variant::Clean);
        for variant in applicable {
            let key = format!("{}@{}", src.name, variant.name());
            let input = variant.apply(&reference)?;
            let t = Instant::now();
            let result = vectorize(&input, args.options).with_context(|| key.clone())?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let metrics =
                evaluate(&reference, &result.svg).with_context(|| format!("evaluating output of {key}"))?;
            write_outputs(args.out, &key, &input, &reference, &result.svg)?;
            println!(
                "{:<40} {:>8.2} {:>8.2} {:>7.2} {:>6} {:>7} {:>6.0}",
                key,
                metrics.delta_e_mean,
                metrics.delta_e_p95,
                metrics.psnr,
                metrics.paths,
                metrics.nodes,
                ms
            );
            categories.entry(src.category.clone()).or_default().push(key.clone());
            cases.insert(key, CaseResult { metrics, ms });
        }
    }

    let by_variant = variants
        .iter()
        .map(|v| {
            let suffix = format!("@{}", v.name());
            (
                v.name().to_owned(),
                Summary::of(cases.iter().filter(|(k, _)| k.ends_with(&suffix)).map(|(_, r)| r)),
            )
        })
        .filter(|(_, s)| s.cases > 0)
        .collect();
    let by_category =
        categories.iter().map(|(c, keys)| (c.clone(), Summary::of(keys.iter().map(|k| &cases[k])))).collect();
    let report = Report { overall: Summary::of(cases.values()), by_variant, by_category, cases };
    print_summary(&report);
    fs::write(args.out.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    fs::write(args.out.join("index.html"), gallery(&report))?;
    println!("\nOutputs (input / SVG / error heatmap per case): {}", args.out.display());
    println!("Visual report: {}", args.out.join("index.html").display());

    if let Some(path) = args.baseline {
        let base: Report = serde_json::from_str(&fs::read_to_string(path)?).with_context(|| {
            format!("parsing baseline {} (regenerate it if the format changed)", path.display())
        })?;
        compare(&report, &base, args.max_regression)?;
    }
    Ok(())
}

fn output_stem(key: &str) -> String {
    key.replace(".svg@", "@").replace(".png@", "@").replace(".jpg@", "@")
}

/// Self-contained HTML page: one row per case (input, SVG result, error heatmap), worst first.
fn gallery(r: &Report) -> String {
    let mut rows: Vec<(&String, &CaseResult)> = r.cases.iter().collect();
    rows.sort_by(|a, b| b.1.metrics.delta_e_mean.total_cmp(&a.1.metrics.delta_e_mean));
    let mut html = String::from(
        r#"<!doctype html><meta charset="utf-8"><title>OpenVectorizer — banc d'essai</title>
<style>body{font:14px system-ui;margin:24px;background:#fafafa}table{border-collapse:collapse}
td,th{padding:6px 10px;border-bottom:1px solid #ddd;text-align:left;vertical-align:middle}
img{height:120px;background:repeating-conic-gradient(#eee 0 25%,#fff 0 50%) 0 0/16px 16px;image-rendering:auto}
input{padding:6px;width:320px;margin-bottom:12px}.n{text-align:right;font-variant-numeric:tabular-nums}</style>
<h1>Banc d'essai</h1>"#,
    );
    let _ = write!(
        html,
        "<p>{} cas · ΔE moyen <b>{:.3}</b> · p95 {:.3} · {} nœuds. Trié du pire au meilleur. Rouge/jaune = erreur.</p>",
        r.overall.cases, r.overall.mean_delta_e, r.overall.mean_p95, r.overall.total_nodes
    );
    html.push_str(r#"<input placeholder="Filtrer (ex. flags, @jpeg35)" oninput="for(const t of document.querySelectorAll('tbody tr'))t.hidden=!t.dataset.k.includes(this.value)">"#);
    html.push_str("<table><thead><tr><th>Cas</th><th>Entrée</th><th>SVG</th><th>Erreur</th><th class=n>ΔE</th><th class=n>p95</th><th class=n>nœuds</th></tr></thead><tbody>");
    for (key, c) in rows {
        let stem = output_stem(key);
        let _ = write!(
            html,
            r#"<tr data-k="{key}"><td>{key}</td><td><img src="{stem}.in.png" loading=lazy></td><td><img src="{stem}.out.svg" loading=lazy></td><td><img src="{stem}.diff.png" loading=lazy></td><td class=n>{:.2}</td><td class=n>{:.2}</td><td class=n>{}</td></tr>"#,
            c.metrics.delta_e_mean, c.metrics.delta_e_p95, c.metrics.nodes
        );
    }
    html.push_str("</tbody></table>");
    html
}

fn write_outputs(out: &Path, key: &str, input: &RgbaImage, reference: &RgbaImage, svg: &str) -> Result<()> {
    let stem = out.join(output_stem(key));
    if let Some(dir) = stem.parent() {
        fs::create_dir_all(dir)?;
    }
    let with = |suffix: &str| PathBuf::from(format!("{}.{suffix}", stem.display()));
    fs::write(with("out.svg"), svg)?;
    save_png(&with("in.png"), input)?;
    save_png(&with("diff.png"), &heatmap(reference, svg)?)
}

fn save_png(path: &Path, img: &RgbaImage) -> Result<()> {
    image::save_buffer(path, img.data(), img.width(), img.height(), image::ExtendedColorType::Rgba8)?;
    Ok(())
}

fn print_summary(r: &Report) {
    println!("\n{:<16} {:>6} {:>9} {:>9} {:>9}", "", "cases", "ΔE mean", "ΔE p95", "nodes");
    let row = |name: &str, s: &Summary| {
        println!(
            "{:<16} {:>6} {:>9.3} {:>9.3} {:>9}",
            name, s.cases, s.mean_delta_e, s.mean_p95, s.total_nodes
        );
    };
    for (k, s) in &r.by_category {
        row(k, s);
    }
    println!();
    for (k, s) in &r.by_variant {
        row(&format!("@{k}"), s);
    }
    println!();
    row("OVERALL", &r.overall);
}

fn compare(report: &Report, base: &Report, max_regression: f64) -> Result<()> {
    let d = report.overall.mean_delta_e - base.overall.mean_delta_e;
    let nodes = report.overall.total_nodes as f64 / base.overall.total_nodes.max(1) as f64 - 1.0;
    println!(
        "\nvs baseline: ΔE {:+.3}  p95 {:+.3}  nodes {:+.1}%",
        d,
        report.overall.mean_p95 - base.overall.mean_p95,
        nodes * 100.0
    );
    let mut changes: Vec<(f64, &String)> = report
        .cases
        .iter()
        .filter_map(|(k, c)| base.cases.get(k).map(|b| (c.metrics.delta_e_mean - b.metrics.delta_e_mean, k)))
        .filter(|(d, _)| d.abs() > 0.2)
        .collect();
    changes.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (delta, key) in changes.iter().take(5).filter(|(d, _)| *d > 0.0) {
        println!("  worse  {delta:+.2}  {key}");
    }
    for (delta, key) in changes.iter().rev().take(5).filter(|(d, _)| *d < 0.0) {
        println!("  better {delta:+.2}  {key}");
    }
    if d > max_regression {
        bail!("quality regression: mean ΔE worse by {d:.3} (> {max_regression})");
    }
    Ok(())
}
