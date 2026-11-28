//! `openvectorizer` command-line tool.
//!
//! ```text
//! openvectorizer convert logo.png -o logo.svg --preset logo --detail 0.7
//! openvectorizer eval testdata --baseline testdata/baseline.json [--variants clean,jpeg35] [--filter flags]
//! ```

mod bench;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use bench::Variant;
use clap::{Args, Parser, Subcommand, ValueEnum};
use openvectorizer_core::{CornerMode, Options, Preset, Stacking, vectorize};

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
        /// Directory scanned recursively (SVG = ground truth, other images = raster-only cases).
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
        /// Degradations applied to ground-truth images (comma-separated).
        #[arg(long, value_enum, value_delimiter = ',', default_values_t = Variant::ALL)]
        variants: Vec<Variant>,
        /// Only run sources whose path contains this text (e.g. `flags`, `fr.svg`).
        #[arg(long)]
        filter: Option<String>,
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
    /// Maximum number of colors (0 = automatic)
    #[arg(long)]
    colors: Option<u32>,
    /// Corner mode
    #[arg(long, value_enum)]
    corners: Option<CornerArg>,
    /// Exactly this many sharp corners (closest to their apex first)
    #[arg(long)]
    sharp_corners: Option<u32>,
    /// Rounded corner radius: -1 sharp, 0 best fit, 1 largest
    #[arg(long, allow_hyphen_values = true)]
    radius: Option<f32>,
    /// Keep traced outlines instead of exact circles, ellipses and symmetries
    #[arg(long)]
    no_shapes: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum CornerArg {
    Auto,
    Sharp,
    Round,
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
        if let Some(v) = self.colors {
            o.max_colors = v;
        }
        if let Some(m) = self.corners {
            o.corners.mode = match m {
                CornerArg::Auto => CornerMode::Automatic,
                CornerArg::Sharp => CornerMode::Sharp,
                CornerArg::Round => CornerMode::Round,
            };
        }
        o.corners.sharp_count = self.sharp_corners;
        if let Some(r) = self.radius {
            o.corners.radius = r;
        }
        if self.cutout {
            o.stacking = Stacking::Cutout;
        }
        o.shapes = !self.no_shapes;
        o
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert { input, output, opts } => convert(&input, output, &opts.to_options()),
        Command::Eval { corpus, out, baseline, max_regression, variants, filter, opts } => {
            bench::run(&bench::EvalArgs {
                corpus: &corpus,
                out: &out,
                baseline: baseline.as_deref(),
                max_regression,
                variants: &variants,
                filter: filter.as_deref(),
                options: &opts.to_options(),
            })
        }
    }
}

fn convert(input: &Path, output: Option<PathBuf>, options: &Options) -> Result<()> {
    let img = bench::load_raster(input)?;
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
