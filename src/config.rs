use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

// Constantes du projet
pub const HASH_PREFIX_INITIAL_LEN: usize = 8;
pub const HASH_PREFIX_MAX_LEN: usize = 20;
pub const HASH_PREFIX_INCREMENT: usize = 4;
pub const FILE_READ_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

/// Extensions par défaut supportées
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "gif", "tif", "tiff", // images
    "mp4", "mov", "m4v", "avi", "mkv", "3gp", "mpo", // vidéos
];

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum LogMode {
    All,
    Conflicts,
    Errors,
}

#[derive(Parser, Debug)]
#[command(name = "mecla")]
#[command(
    about = "Move media files from EXIF/metadata (via exiftool) to YYYY/MM or YYYY/MM <TAG>."
)]
pub struct Args {
    /// Input directory (e.g., /path/_depot)
    #[arg(long)]
    pub input: PathBuf,

    /// Output directory (where to create YYYY/MM...)
    #[arg(long)]
    pub output: PathBuf,

    /// Do not modify anything, only display the actions
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Log level: all, conflicts, errors
    #[arg(long, value_enum, default_value_t = LogMode::Conflicts)]
    pub log: LogMode,

    /// Extensions supported (optional). Ex: --ext jpg --ext mp4 ...
    /// If not provided, a default set is used.
    #[arg(long = "ext")]
    pub exts: Vec<String>,
}

#[derive(Debug)]
pub struct Config {
    pub input: PathBuf,
    pub output: PathBuf,
    pub dry_run: bool,
    #[allow(dead_code)]
    pub log: LogMode,
    pub exts: Vec<String>,
}

impl Config {
    /// Crée une configuration à partir des arguments CLI
    pub fn from_args(args: Args) -> Result<Self> {
        if args.input.as_os_str().is_empty() || args.output.as_os_str().is_empty() {
            bail!("--input and --output are required");
        }

        let input = args
            .input
            .canonicalize()
            .with_context(|| format!("Unable to resolve --input: {:?}", args.input))?;

        let exts = if args.exts.is_empty() {
            log::info!(
                "No extensions provided, using defaults: {:?}",
                DEFAULT_EXTENSIONS
            );
            DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect()
        } else {
            normalize_exts(args.exts)
        };

        let cfg = Config {
            input,
            output: args.output,
            dry_run: args.dry_run,
            log: args.log,
            exts,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    /// Valide la configuration
    fn validate(&self) -> Result<()> {
        // Vérifier que input est un dossier
        if !self.input.is_dir() {
            bail!("--input must be a directory: {:?}", self.input);
        }

        // Vérifier que output n'est pas dans input
        if self.output.starts_with(&self.input) {
            bail!("Output directory cannot be inside input directory");
        }

        // Vérifier les permissions sur output (en mode non dry-run)
        if !self.dry_run && !self.output.exists() {
            std::fs::create_dir_all(&self.output)
                .context("Cannot create output directory (permission denied?)")?;
        }

        Ok(())
    }
}

/// Normalise les extensions (minuscules, sans point)
fn normalize_exts(mut exts: Vec<String>) -> Vec<String> {
    for e in &mut exts {
        *e = e.trim().trim_start_matches('.').to_lowercase();
    }
    exts.retain(|e| !e.is_empty());
    exts
}
