use std::path::{Path, PathBuf};

use anyhow::Context;
use photohelper_ai::VerifiedModelBytes;

use crate::Cli;
use crate::IngestArgs;
use crate::commands::cull::CullArgs;
use crate::commands::dedup::DedupeArgs;
use crate::commands::develop::DevelopArgs;
use crate::commands::export::{CliWatermarkPosition, ExportArgs};

#[allow(clippy::struct_excessive_bools)]
#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    /// Directory to walk for ingestion.
    pub path: PathBuf,

    /// Export output directory. Cannot be the same as or a subdirectory of the input path.
    #[arg(short, long)]
    pub output: PathBuf,

    /// Recurse into subdirectories.
    #[arg(short, long, default_value_t = true)]
    pub recursive: bool,

    /// Treat unknown cameras or any per-photo error as fatal at end-of-run.
    #[arg(long, default_value_t = false)]
    pub strict: bool,

    /// Skip checking Lightroom modification times (overwrite all sidecars).
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Map NIMA scores to standard Lightroom stars (1-5).
    #[arg(long, default_value_t = false)]
    pub lr_rating: bool,

    /// Map NIMA score ranges to Lightroom color labels (Red/Green).
    #[arg(long, default_value_t = false)]
    pub lr_label: bool,

    /// Write the exact NIMA score into the Lightroom color label field (e.g. '09.50').
    /// This enables native Lightroom sorting by 'Label Text'.
    #[arg(long, default_value_t = false, conflicts_with = "lr_label")]
    pub lr_label_score: bool,

    /// Map duplicates to keywords for Lightroom Smart Collections.
    #[arg(long, default_value_t = false)]
    pub lr_keywords: bool,

    /// Enable all standard Lightroom metadata mappings (rating, label, keywords).
    #[arg(long, default_value_t = false)]
    pub all_lr: bool,

    /// String used for Red (low) color labels in Lightroom.
    #[arg(long, default_value = "Red", env = "PHOTOHELPER_LR_LABEL_RED")]
    pub lr_label_red: String,

    /// String used for Green (high) color labels in Lightroom.
    #[arg(long, default_value = "Green", env = "PHOTOHELPER_LR_LABEL_GREEN")]
    pub lr_label_green: String,

    /// Set minimum acceptable rating threshold (0-5) to export.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(0..=5))]
    pub min_rating: u8,

    /// Exposure compensation in stops (–5.0 to 5.0).
    #[arg(long)]
    pub exposure: Option<f32>,

    /// White balance temperature in Kelvin (2000–50000).
    #[arg(long)]
    pub temp: Option<i32>,

    /// White balance tint (–150 to 150).
    #[arg(long)]
    pub tint: Option<i32>,

    /// Contrast (–100 to 100).
    #[arg(long)]
    pub contrast: Option<i32>,

    /// Highlights (–100 to 100).
    #[arg(long)]
    pub highlights: Option<i32>,

    /// Shadows (–100 to 100).
    #[arg(long)]
    pub shadows: Option<i32>,

    /// Defers to Lightroom's internal `AutoTone` engine and does not apply numerical adjustments.
    #[arg(long, default_value_t = false)]
    pub auto_tone: bool,

    /// Cosine-similarity threshold for dedup clustering (0.0, 1.0].
    #[arg(long, default_value_t = 0.95_f32, value_parser = crate::commands::dedup::parse_similarity_threshold)]
    pub similarity_threshold: f32,

    /// Watermark text.
    #[arg(long)]
    pub watermark: Option<String>,

    /// Position of watermark (bottom-left or top-right).
    #[arg(long, default_value = "bottom-left")]
    pub watermark_position: CliWatermarkPosition,

    /// Image badge watermark. Format: path=<PATH>,pos=<POS>[,scale=<PCT>]
    #[arg(long = "badge")]
    pub badges: Vec<crate::commands::export::BadgeArg>,

    /// Set export JPEG quality (1-100).
    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub quality: u8,

    /// Output max dimension (default is full size / unspecified).
    #[arg(long, value_parser = crate::commands::export::validate_long_edge)]
    pub long_edge: Option<u32>,
}

pub struct ValidatedIO {
    input: PathBuf,
    output: PathBuf,
}

impl ValidatedIO {
    pub fn new(input: &Path, output: &Path) -> anyhow::Result<Self> {
        let canonical_input = dunce::canonicalize(input)
            .with_context(|| format!("Failed to canonicalize input path: {}", input.display()))?;

        if !output.exists() {
            std::fs::create_dir_all(output).with_context(|| {
                format!("Failed to create output directory: {}", output.display())
            })?;
        }

        let canonical_output = dunce::canonicalize(output)
            .with_context(|| format!("Failed to canonicalize output path: {}", output.display()))?;

        if canonical_output.starts_with(&canonical_input) {
            anyhow::bail!(
                "Output path cannot be a subdirectory of the input path to prevent recursive ingest loops"
            );
        }

        Ok(Self {
            input: canonical_input,
            output: canonical_output,
        })
    }

    pub fn input(&self) -> &Path {
        &self.input
    }

    pub fn output(&self) -> &Path {
        &self.output
    }
}

pub fn run_pipeline(
    cli: &Cli,
    args: &RunArgs,
    nima_model: &VerifiedModelBytes,
    nima_model_path: PathBuf,
    clip_model: &VerifiedModelBytes,
) -> anyhow::Result<u8> {
    let io = ValidatedIO::new(&args.path, &args.output)?;

    let mut cli_resolved = cli.clone();
    if cli_resolved.catalog.is_none() {
        cli_resolved.catalog = Some(io.input().join(".photohelper").join("catalog.db"));
    }

    let ingest_args = IngestArgs {
        path: io.input().to_path_buf(),
        recursive: args.recursive,
        strict: args.strict,
    };

    let cull_args = CullArgs {
        strict: args.strict,
    };

    let dedup_args = DedupeArgs {
        strict: args.strict,
        similarity_threshold: args.similarity_threshold,
    };

    let develop_args = DevelopArgs {
        strict: args.strict,
        force: args.force,
        exposure: args.exposure,
        temp: args.temp,
        tint: args.tint,
        contrast: args.contrast,
        highlights: args.highlights,
        shadows: args.shadows,
        lr_rating: args.lr_rating,
        lr_label: args.lr_label,
        lr_label_score: args.lr_label_score,
        lr_keywords: args.lr_keywords,
        all_lr: args.all_lr,
        lr_label_red: args.lr_label_red.clone(),
        lr_label_green: args.lr_label_green.clone(),
        auto_tone: args.auto_tone,
    };

    let export_args = ExportArgs {
        output: io.output().to_path_buf(),
        long_edge: args.long_edge,
        quality: args.quality,
        watermark: args.watermark.clone(),
        watermark_position: args.watermark_position,
        badges: args.badges.clone(),
        min_rating: args.min_rating,
        force: args.force,
        strict: args.strict,
        mark1_png: None,
        mark2_png: None,
        with_shadow: false,
    };

    tracing::info!("Starting pipeline...");

    let mut final_code = 0;
    macro_rules! run_step {
        ($msg:expr, $expr:expr) => {
            tracing::info!($msg);
            let code = $expr;
            if args.strict && code != 0 {
                return Ok(code);
            }
            if final_code == 0 {
                final_code = code;
            }
        };
    }

    run_step!(
        "[1/5] Ingesting files",
        crate::commands::ingest::run_ingest(&cli_resolved, &ingest_args)?
    );
    run_step!(
        "[2/5] Culling files",
        crate::commands::cull::run_cull(&cli_resolved, &cull_args, nima_model, nima_model_path)?
    );
    run_step!(
        "[3/5] Deduplicating files",
        crate::commands::dedup::run_dedup(&cli_resolved, &dedup_args, clip_model)?
    );
    run_step!(
        "[4/5] Developing metadata",
        crate::commands::develop::run_develop(&cli_resolved, &develop_args)?
    );
    run_step!(
        "[5/5] Exporting JPEGs",
        crate::commands::export::run_export(&cli_resolved, &export_args)?
    );

    tracing::info!("Pipeline complete.");
    Ok(final_code)
}
