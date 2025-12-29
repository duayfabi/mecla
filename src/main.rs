use anyhow::{anyhow, bail, Context, Result};
use blake3::Hasher;
use chrono::{Datelike, NaiveDateTime, Timelike};
use clap::{Parser, ValueEnum};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::{Read},
    path::{Component, Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum LogMode {
    All,
    Conflicts,
    Errors,
}

#[derive(Parser, Debug)]
#[command(name = "mecla")]
#[command(about = "Move media files from EXIF/metadata (via exiftool) to YYYY/MM or YYYY/MM <TAG>.")]
struct Args {
    /// Input directory (e.g., /path/_depot)
    #[arg(long)]
    input: PathBuf,

    /// Output directory (where to create YYYY/MM...)
    #[arg(long)]
    output: PathBuf,

    /// Do not modify anything, only display the actions
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Log level: all, conflicts, errors
    #[arg(long, value_enum, default_value_t = LogMode::Conflicts)]
    log: LogMode,

    /// Extensions supported (optional). Ex: --ext jpg --ext mp4 ...
    /// If not provided, a default set is used.
    #[arg(long = "ext")]
    exts: Vec<String>,
}

#[derive(Debug)]
struct Config {
    input: PathBuf,
    output: PathBuf,
    dry_run: bool,
    log: LogMode,
    exts: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[ERR] {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    if args.input.as_os_str().is_empty() || args.output.as_os_str().is_empty() {
        bail!("--input and --output are required");
    }

    let cfg = Config {
        input: args
            .input
            .canonicalize()
            .with_context(|| format!("Unable to resolve --input: {:?}", args.input))?,
        output: args.output,
        dry_run: args.dry_run,
        log: args.log,
        exts: normalize_exts(args.exts),
    };

    ensure_exiftool_available()?;

    if cfg.exts.is_empty() {
        // Set par défaut
        let defaults = vec![
            "jpg", "jpeg", "png", "heic", "gif", "tif", "tiff", // images
            "mp4", "mov", "m4v", "avi", "mkv", "3gp", "mpo",    // vidéos
        ];
        cfg_log_all(&cfg, &format!("No extensions provided, defaults: {:?}", defaults));
        // Note: on stocke en minuscules sans point
        // (On reconstruit une liste owned)
        let mut exts = Vec::with_capacity(defaults.len());
        for e in defaults {
            exts.push(e.to_string());
        }
        process(&Config { exts, ..cfg })
    } else {
        process(&cfg)
    }
}

fn process(cfg: &Config) -> Result<()> {
    if !cfg.input.is_dir() {
        bail!("--input must be a directory: {:?}", cfg.input);
    }

    let mut tags_seen: HashSet<String> = HashSet::new();

    // On accepte output inexistant (on créera au besoin)
    for entry in WalkDir::new(&cfg.input).follow_links(false).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                cfg_log_err(cfg, &format!("walkdir error: {err}"));
                continue;
            }
        };

        if entry.file_type().is_dir() {
            continue;
        }

        let src = entry.path().to_path_buf();
        if !is_supported(&src, &cfg.exts) {
            continue;
        }

        match handle_one(cfg, &src, &mut tags_seen) {
            Ok(()) => {}
            Err(e) => cfg_log_err(cfg, &format!("{}: {:#}", src.display(), e)),
        }
    }

    prune_empty_tag_dirs(cfg, &tags_seen)?;
    
    Ok(())
}

fn hash_prefix(hash: &blake3::Hash, n: usize) -> String {
    let hex = hash.to_hex(); // 64 chars hex
    hex[..n.min(hex.len())].to_string().to_uppercase()
}

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

        cfg_log_conflict(
            cfg,
            &format!("[PRUNE] no media left in tag dir, pruning empties: {}", tag_dir.display()),
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

fn contains_supported_media(root: &Path, exts: &[String]) -> bool {
    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && is_supported(entry.path(), exts) {
            return true;
        }
    }
    false
}

// Supprime récursivement les dossiers vides (mais ne supprime jamais un dossier non-vide)
fn prune_empty_dirs_recursively(root: &Path) -> Result<()> {
    // post-order: on traite les enfants avant le parent
    for entry in WalkDir::new(root)
        .follow_links(false)
        .contents_first(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir() {
            let p = entry.path();
            if is_dir_empty(p)? {
                // Ne supprime pas 'root' ici, on le gère après
                if p != root {
                    fs::remove_dir(p).with_context(|| format!("remove empty dir {}", p.display()))?;
                }
            }
        }
    }
    Ok(())
}

fn is_dir_empty(dir: &Path) -> Result<bool> {
    let mut it = fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?;
    Ok(it.next().is_none())
}

fn handle_one(cfg: &Config, src: &Path, tags_seen: &mut HashSet<String>) -> Result<()> {
    let tag = infer_tag(&cfg.input, src);

    if let Some(ref t) = tag {
        tags_seen.insert(t.clone());
    }

    let dt = extract_datetime_with_exiftool(src)
        .with_context(|| "Unable to extract a date via exiftool")?;

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
        return move_or_copy(cfg, src, &dest);
    }

    // Conflit: comparer hashes
    cfg_log_conflict(cfg, &format!("[CONFLICT] {} -> {}", src.display(), dest.display()));

    let src_hash = blake3_file(src).with_context(|| "hash source")?;
    let dst_hash = blake3_file(&dest).with_context(|| "hash dest")?;

    if src_hash == dst_hash {
        // Identique: skip + supprimer source
        cfg_log_conflict(
            cfg,
            &format!("[SKIP-DUP] same hash, delete source: {}", src.display()),
        );
        if !cfg.dry_run {
            fs::remove_file(src).with_context(|| "delete source (dup)")?;
        }
        return Ok(());
    }

    // Différent: on cherche un nom suffixé libre
    let mut n = 8;
    loop {
        let suffix = hash_prefix(&src_hash, n);
        let alt_name = format_filename_with_suffix(&dt, &suffix, &ext);
        let alt_dest = target_dir.join(&alt_name);

        if !alt_dest.exists() {
            cfg_log_conflict(
                cfg,
                &format!(
                    "[RENAME] dest exists diff hash, using: {}",
                    alt_dest.display()
                ),
            );

            dest = alt_dest;
            break;
        }

        // si collision, on augmente la longueur du prefix
        if n >= 20 { bail!("Persistent collision…"); }
        n += 4;
    }

    move_or_copy(cfg, src, &dest)
}

fn infer_tag(input_root: &Path, src: &Path) -> Option<String> {
    let rel = src.strip_prefix(input_root).ok()?;
    // rel: <maybe-tag>/.../file
    // On prend le 1er composant, si le parent direct est root => pas de tag.
    // Si le fichier est directement sous input_root, rel.components() = [file], donc None.
    let mut comps = rel.components();
    let first = comps.next()?;
    let second = comps.next(); // si None => file à la racine

    match (first, second) {
        (Component::Normal(tag), Some(_)) => tag.to_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn build_target_dir(output_root: &Path, dt: &NaiveDateTime, tag: Option<&str>) -> PathBuf {
    let year = format!("{:04}", dt.year());
    let month = format!("{:02}", dt.month());

    let month_dir_name = match tag {
        Some(t) if !t.trim().is_empty() => format!("{} {}", month, t.trim()),
        _ => month,
    };

    output_root.join(year).join(month_dir_name)
}

fn format_filename(dt: &NaiveDateTime, ext: &str) -> String {
    // "2025-07-23 08.54.04.jpg"
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02}.{}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        ext
    )
}

fn format_filename_with_suffix(dt: &NaiveDateTime, suffix: &str, ext: &str) -> String {
    // "2025-07-23 08.54.04 ABCDE.jpg"
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02} {}.{}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        suffix,
        ext
    )
}

fn is_supported(path: &Path, exts: &[String]) -> bool {
    let ext = match path.extension().and_then(OsStr::to_str) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };
    exts.iter().any(|x| x == &ext)
}

fn normalize_exts(mut exts: Vec<String>) -> Vec<String> {
    for e in &mut exts {
        *e = e.trim().trim_start_matches('.').to_lowercase();
    }
    exts.retain(|e| !e.is_empty());
    exts
}

fn ensure_exiftool_available() -> Result<()> {
    let out = Command::new("exiftool")
        .arg("-ver")
        .output()
        .context("Unable to execute exiftool. Is the binary accessible ?")?;

    if !out.status.success() {
        bail!("exiftool exists but returns an error (exiftool -ver)");
    }
    Ok(())
}

fn extract_datetime_with_exiftool(path: &Path) -> Result<NaiveDateTime> {
    // On demande plusieurs tags dans l'ordre, et on prend le premier non-vide.
    // -s -s -s : sortie brute sans label
    // -d : format homogène pour parser
    // Tags choisis pour couvrir photos + vidéos (QuickTime/MP4)
    let tags = [
        "-DateTimeOriginal",
        "-CreateDate",
        "-MediaCreateDate",
        "-TrackCreateDate",
        "-ModifyDate",
    ];

    let mut cmd = Command::new("exiftool");
    cmd.arg("-s")
        .arg("-s")
        .arg("-s")
        .arg("-api")
        .arg("QuickTimeUTC=1")
        .arg("-d")
        .arg("%Y-%m-%d %H:%M:%S");

    for t in tags {
        cmd.arg(t);
    }
    cmd.arg(path);

    let out = cmd
        .output()
        .with_context(|| format!("exiftool failed to run on {}", path.display()))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("exiftool error: {}", stderr.trim());
    }

    // exiftool renvoie une ligne par tag demandé (souvent vide si absent).
    // On cherche la première ligne qui ressemble à une date formatée.
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let s = line.trim();
        if s.is_empty() {
            continue;
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Ok(dt);
        }
    }

    bail!(
        "No date found via EXIF/metadata tags for {}",
        path.display()
    );
}

fn blake3_file(path: &Path) -> Result<blake3::Hash> {
    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Hasher::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).with_context(|| "read file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

fn move_or_copy(cfg: &Config, src: &Path, dest: &Path) -> Result<()> {
    // Crée le dossier cible si nécessaire
    if let Some(parent) = dest.parent() {
        if !cfg.dry_run {
            fs::create_dir_all(parent)
                .with_context(|| format!("create_dir_all {}", parent.display()))?;
        }
    }

    cfg_log_all(cfg, &format!("[MOVE] {} -> {}", src.display(), dest.display()));

    if cfg.dry_run {
        return Ok(());
    }

    // On tente un rename (rapide)…
    match fs::rename(src, dest) {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            // …et en cas d'échec, on tente un fallback copy+remove,
            // qui marche aussi cross-device et sur Windows.
            //
            // On garde un contexte clair : si le fallback échoue,
            // on remonte *les deux* erreurs.
            fs::copy(src, dest).with_context(|| {
                format!(
                    "rename failed ({}) and copy failed: {} -> {}",
                    rename_err,
                    src.display(),
                    dest.display()
                )
            })?;

            fs::remove_file(src).with_context(|| {
                format!(
                    "rename failed ({}) and copy succeeded but remove failed: {}",
                    rename_err,
                    src.display()
                )
            })?;

            Ok(())
        }
    }
}

fn cfg_log_all(cfg: &Config, msg: &str) {
    if matches!(cfg.log, LogMode::All) {
        println!("{msg}");
    }
}

fn cfg_log_conflict(cfg: &Config, msg: &str) {
    if matches!(cfg.log, LogMode::All | LogMode::Conflicts) {
        println!("{msg}");
    }
}

fn cfg_log_err(_cfg: &Config, msg: &str) {
    // Les erreurs s'affichent toujours sur stderr
    eprintln!("{msg}");
}
