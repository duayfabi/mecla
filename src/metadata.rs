use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// Vérifie qu'exiftool est disponible sur le système
pub fn ensure_exiftool_available() -> Result<()> {
    let out = Command::new("exiftool")
        .arg("-ver")
        .output()
        .context("Unable to execute exiftool. Is the binary accessible?")?;

    if !out.status.success() {
        bail!("exiftool exists but returns an error (exiftool -ver)");
    }
    Ok(())
}

/// Extrait la date/heure d'un fichier média via exiftool.
///
/// Tente d'abord d'extraire les métadonnées EXIF/QuickTime via exiftool.
/// En cas d'échec, utilise la date de modification du fichier comme fallback.
///
/// # Arguments
/// * `path` - Chemin vers le fichier média
///
/// # Returns
/// La date/heure extraite des métadonnées ou de mtime
///
/// # Errors
/// Retourne une erreur si exiftool échoue ET que mtime n'est pas accessible
pub fn extract_datetime_with_exiftool(path: &Path) -> Result<NaiveDateTime> {
    match try_exiftool(path) {
        Ok(dt) => Ok(dt),
        Err(e) => {
            log::warn!(
                "exiftool failed for {}, using file mtime: {}",
                path.display(),
                e
            );
            extract_datetime_from_mtime(path)
        }
    }
}

/// Tente d'extraire la date via exiftool
fn try_exiftool(path: &Path) -> Result<NaiveDateTime> {
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

/// Extrait la date de modification du fichier comme fallback
fn extract_datetime_from_mtime(path: &Path) -> Result<NaiveDateTime> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Cannot read metadata for {}", path.display()))?;

    let mtime = metadata
        .modified()
        .with_context(|| format!("Cannot get modification time for {}", path.display()))?;

    datetime_from_systemtime(mtime)
}

/// Convertit SystemTime en NaiveDateTime
fn datetime_from_systemtime(time: SystemTime) -> Result<NaiveDateTime> {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("SystemTime before UNIX epoch")?;

    let secs = duration.as_secs() as i64;
    let nsecs = duration.subsec_nanos();

    use chrono::DateTime;
    let dt = DateTime::from_timestamp(secs, nsecs)
        .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?;

    Ok(dt.naive_utc())
}
