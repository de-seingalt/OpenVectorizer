//! `openvectorizer` command-line tool.
//!
//! ```text
//! openvectorizer convert logo.png -o logo.svg --preset logo --detail 0.7
//! openvectorizer eval testdata --out target/eval --baseline testdata/baseline.json
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use openvectorizer_core::{Options, Preset, RgbaImage, Stacking, vectorize};
use openvectorizer_eval::{Metrics, evaluate, heatmap, render_svg};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(version, about = "Convert raster images to clean SVG.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Vectorize one image.
    Convert {
        input: PathBuf,
        /// Output file (default: input with .svg extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        opts: OptArgs,
    },
    /// Run the quality benchmark on a corpus (PNG/JPG/WebP images and ground-truth SVGs).
    Eval {
        /// Directory scanned recursively.
        #[arg(default_value = "testdata")]
        corpus: PathBuf,
        /// Where to write produced SVGs, error heatmaps and report.json.
        #[arg(long, default_value = "target/eval")]
        out: PathBuf,
        /// Previous report.json to compare against.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Exit with an error if mean ΔE gets worse than baseline by more than this (absolute).
        #[arg(long, default_value_t = 0.25)]
        max_regression: f64,
        #[command(flatten)]
        opts: OptArgs,
    },
}

#[derive(Args, Debug)]
struct OptArgs {
    #[arg(long, value_enum)]
    preset: Option<PresetArg>,
    /// 0..1
    #[arg(long)]
    detail: Option<f32>,
    /// 0..1
    #[arg(long)]
    smoothness: Option<f32>,
    #[arg(long)]
    min_area: Option<u32>,
    #[arg(long)]
    cutout: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum PresetArg {
    Logo,
    Illustration,
    Photo,
}

impl OptArgs {
    fn to_options(&self) -> Options {
        let mut o = Options::default();
        if let Some(p) = self.preset {
            o.preset = match p {
                PresetArg::Logo => Preset::Logo,
                PresetArg::Illustration => Preset::Illustration,
                PresetArg::Photo => Preset::Photo,
            };
        }
        if let Some(v) = self.detail {
            o.detail = v;
        }
        if let Some(v) = self.smoothness {
            o.smoothness = v;
        }
        if let Some(v) = self.min_area {
            o.min_area_px = v;
        }
        if self.cutout {
            o.stacking = Stacking::Cutout;
        }
        o
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert { input, output, opts } => convert(&input, output, &opts.to_options()),
        Command::Eval { corpus, out, baseline, max_regression, opts } => {
            run_eval(&corpus, &out, baseline.as_deref(), max_regression, &opts.to_options())
        }
    }
}

fn convert(input: &Path, output: Option<PathBuf>, options: &Options) -> Result<()> {
    let img = load_raster(input)?;
    let t = Instant::now();
    let out = vectorize(&img, options)?;
    let output = output.unwrap_or_else(|| input.with_extension("svg"));
    fs::write(&output, &out.svg).with_context(|| format!("writing {}", output.display()))?;
    eprintln!(
        "{} -> {} ({} paths, {} bytes, {:.0} ms)",
        input.display(),
        output.display(),
        out.stats.path_count,
        out.stats.bytes,
        t.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

fn load_raster(path: &Path) -> Result<RgbaImage> {
    let img = image::open(path).with_context(|| format!("decoding {}", path.display()))?.to_rgba8();
    let (w, h) = img.dimensions();
    Ok(RgbaImage::new(w, h, img.into_raw())?)
}

/// Ground-truth SVGs are rendered to a raster; that raster is the input and the reference.
fn load_case(path: &Path) -> Result<RgbaImage> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        let svg = fs::read_to_string(path)?;
        let (w, h) = svg_size(&svg).with_context(|| format!("{}: needs width/height", path.display()))?;
        Ok(render_svg(&svg, w, h)?)
    } else {
        load_raster(path)
    }
}

fn svg_size(svg: &str) -> Option<(u32, u32)> {
    let attr = |name: &str| -> Option<u32> {
        let tag = &svg[svg.find("<svg")?..];
        let tag = &tag[..tag.find('>')?];
        let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let val = &tag[start..];
        val[..val.find('"')?].trim_end_matches("px").parse::<f64>().ok().map(|v| v.round() as u32)
    };
    Some((attr("width")?, attr("height")?))
}

#[derive(Serialize, Deserialize, Debug)]
struct Report {
    cases: BTreeMap<String, CaseResult>,
    mean_delta_e: f64,
    mean_p95: f64,
    total_nodes: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct CaseResult {
    #[serde(flatten)]
    metrics: Metrics,
    ms: f64,
}

fn collect_cases(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_cases(&path, acc)?;
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "svg")
        }) {
            acc.push(path);
        }
    }
    Ok(())
}

fn run_eval(
    corpus: &Path,
    out: &Path,
    baseline: Option<&Path>,
    max_regression: f64,
    options: &Options,
) -> Result<()> {
    let mut paths = Vec::new();
    collect_cases(corpus, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        bail!("no images found in {}", corpus.display());
    }
    fs::create_dir_all(out)?;
    let mut cases = BTreeMap::new();
    println!(
        "{:<36} {:>8} {:>8} {:>7} {:>7} {:>8} {:>7}",
        "case", "ΔE mean", "ΔE p95", "PSNR", "paths", "nodes", "ms"
    );
    for path in &paths {
        let name = path.strip_prefix(corpus).unwrap_or(path).to_string_lossy().replace(['/', '\\'], "__");
        let src = load_case(path)?;
        let t = Instant::now();
        let result = vectorize(&src, options)?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let m = evaluate(&src, &result.svg)?;
        let stem = out.join(&name);
        fs::write(stem.with_extension("out.svg"), &result.svg)?;
        let hm = heatmap(&src, &result.svg)?;
        image::save_buffer(
            stem.with_extension("diff.png"),
            hm.data(),
            hm.width(),
            hm.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        println!(
            "{:<36} {:>8.2} {:>8.2} {:>7.2} {:>7} {:>8} {:>7.0}",
            name, m.delta_e_mean, m.delta_e_p95, m.psnr, m.paths, m.nodes, ms
        );
        cases.insert(name, CaseResult { metrics: m, ms });
    }
    let n = cases.len() as f64;
    let report = Report {
        mean_delta_e: cases.values().map(|c| c.metrics.delta_e_mean).sum::<f64>() / n,
        mean_p95: cases.values().map(|c| c.metrics.delta_e_p95).sum::<f64>() / n,
        total_nodes: cases.values().map(|c| u64::from(c.metrics.nodes)).sum(),
        cases,
    };
    println!(
        "\nMEAN  ΔE {:.3}  p95 {:.3}  nodes {}   (outputs in {})",
        report.mean_delta_e,
        report.mean_p95,
        report.total_nodes,
        out.display()
    );
    fs::write(out.join("report.json"), serde_json::to_string_pretty(&report)?)?;

    if let Some(b) = baseline {
        let base: Report = serde_json::from_str(&fs::read_to_string(b)?)
            .with_context(|| format!("parsing baseline {}", b.display()))?;
        let d = report.mean_delta_e - base.mean_delta_e;
        let dn = report.total_nodes as f64 / base.total_nodes.max(1) as f64 - 1.0;
        println!(
            "vs baseline: ΔE {:+.3}  p95 {:+.3}  nodes {:+.1}%",
            d,
            report.mean_p95 - base.mean_p95,
            dn * 100.0
        );
        if d > max_regression {
            bail!("quality regression: mean ΔE worse by {d:.3} (> {max_regression})");
        }
    }
    Ok(())
}
