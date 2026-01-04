mod config;
mod filesystem;
mod metadata;
mod naming;
mod stats;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{fs, process};
use walkdir::WalkDir;

use config::{Args, Config, HASH_PREFIX_INCREMENT, HASH_PREFIX_INITIAL_LEN, HASH_PREFIX_MAX_LEN};
use filesystem::{
    blake3_file, contains_supported_media, hash_prefix, is_dir_empty, is_supported, move_or_copy,
    prune_empty_dirs_recursively,
};
use metadata::{ensure_exiftool_available, extract_datetime_with_exiftool};
use naming::{build_target_dir, format_filename, format_filename_with_suffix, infer_tag};
use stats::Stats;

fn main() {
    // Initialiser le logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    if let Err(e) = run() {
        log::error!("{:#}", e);
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let cfg = Config::from_args(args)?;

    ensure_exiftool_available()?;

    process(&cfg)
}

fn process(cfg: &Config) -> Result<()> {
    let stats = Stats::new();

    // Collecter tous les fichiers à traiter
    let files: Vec<PathBuf> = WalkDir::new(&cfg.input)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .filter(|p| is_supported(p, &cfg.exts))
        .collect();

    if files.is_empty() {
        log::info!("No supported files found in input directory");
        return Ok(());
    }

    log::info!("Found {} files to process", files.len());

    // Créer la barre de progression (seulement si stdout est un terminal)
    let pb = if atty::is(atty::Stream::Stdout) {
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Tags vus (pour le nettoyage final)
    let tags_seen = Mutex::new(HashSet::new());

    // Traitement parallèle des fichiers
    files.par_iter().for_each(|src| {
        match handle_one(cfg, src, &stats) {
            Ok(tag) => {
                if let Some(t) = tag {
                    tags_seen.lock().unwrap().insert(t);
                }
                stats.inc_processed();
            }
            Err(e) => {
                log::error!("{}: {:#}", src.display(), e);
                stats.inc_errors();
            }
        }

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    });

    if let Some(pb) = pb {
        pb.finish_with_message("Done");
    }

    // Nettoyage des dossiers TAG vides
    let tags = tags_seen.into_inner().unwrap();
    prune_empty_tag_dirs(cfg, &tags)?;

    // Afficher les statistiques
    stats.print_summary();

    Ok(())
}

/// Traite un fichier individuel
///
/// # Returns
/// Le tag du fichier (si présent) pour le nettoyage ultérieur
fn handle_one(cfg: &Config, src: &Path, stats: &Stats) -> Result<Option<String>> {
    let tag = infer_tag(&cfg.input, src);

    let dt = extract_datetime_with_exiftool(src)
        .with_context(|| "Unable to extract a date via exiftool or mtime")?;

    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("File without extension: {}", src.display()))?
        .to_lowercase();

    let target_dir = build_target_dir(&cfg.output, &dt, tag.as_deref());
    let base_name = format_filename(&dt, &ext);
    let mut dest = target_dir.join(&base_name);

    // S'il n'y a pas de conflit, on déplace direct.
    if !dest.exists() {
        move_or_copy(src, &dest, cfg.dry_run)?;
        return Ok(tag);
    }

    // Conflit: comparer hashes
    log::warn!("[CONFLICT] {} -> {}", src.display(), dest.display());

    let src_hash = blake3_file(src).with_context(|| "hash source")?;
    let dst_hash = blake3_file(&dest).with_context(|| "hash dest")?;

    if src_hash == dst_hash {
        // Identique: skip + supprimer source
        log::info!(
            "[SKIP-DUP] same hash, delete source: {}",
            src.display()
        );
        if !cfg.dry_run {
            fs::remove_file(src).with_context(|| "delete source (dup)")?;
        }
        stats.inc_duplicates();
        return Ok(tag);
    }

    // Différent: on cherche un nom suffixé libre
    let mut n = HASH_PREFIX_INITIAL_LEN;
    loop {
        let suffix = hash_prefix(&src_hash, n);
        let alt_name = format_filename_with_suffix(&dt, &suffix, &ext);
        let alt_dest = target_dir.join(&alt_name);

        if !alt_dest.exists() {
            log::info!(
                "[RENAME] dest exists diff hash, using: {}",
                alt_dest.display()
            );

            dest = alt_dest;
            stats.inc_renamed();
            break;
        }

        // si collision, on augmente la longueur du prefix
        if n >= HASH_PREFIX_MAX_LEN {
            bail!("Persistent collision…");
        }
        n += HASH_PREFIX_INCREMENT;
    }

    move_or_copy(src, &dest, cfg.dry_run)?;
    Ok(tag)
}

/// Nettoie les dossiers TAG vides après traitement
fn prune_empty_tag_dirs(cfg: &Config, tags_seen: &HashSet<String>) -> Result<()> {
    for tag in tags_seen {
        let tag_dir = cfg.input.join(tag);
        if !tag_dir.is_dir() {
            continue;
        }

        // S'il reste encore des médias supportés sous ce TAG, on ne touche pas.
        if contains_supported_media(&tag_dir, &cfg.exts) {
            continue;
        }

        log::info!(
            "[PRUNE] no media left in tag dir, pruning empties: {}",
            tag_dir.display()
        );

        if cfg.dry_run {
            continue;
        }

        // On supprime les sous-dossiers vides, puis si le tag_dir devient vide, on le supprime.
        prune_empty_dirs_recursively(&tag_dir)?;

        // Si le dossier TAG est maintenant vide -> on le supprime
        if is_dir_empty(&tag_dir)? {
            fs::remove_dir(&tag_dir)
                .with_context(|| format!("remove empty tag dir {}", tag_dir.display()))?;
        }
    }
    Ok(())
}
