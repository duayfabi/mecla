use anyhow::{Context, Result};
use blake3::Hasher;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

use crate::config::FILE_READ_BUFFER_SIZE;

/// Vérifie si un fichier a une extension supportée.
///
/// # Arguments
/// * `path` - Chemin du fichier
/// * `exts` - Liste des extensions supportées (sans point, en minuscules)
///
/// # Returns
/// true si l'extension est supportée, false sinon
pub fn is_supported(path: &Path, exts: &[String]) -> bool {
    let ext = match path.extension().and_then(OsStr::to_str) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };
    exts.iter().any(|x| x == &ext)
}

/// Calcule le hash BLAKE3 d'un fichier.
///
/// # Arguments
/// * `path` - Chemin du fichier à hasher
///
/// # Returns
/// Le hash BLAKE3 du fichier
///
/// # Errors
/// Retourne une erreur si le fichier ne peut pas être lu
pub fn blake3_file(path: &Path) -> Result<blake3::Hash> {
    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; FILE_READ_BUFFER_SIZE];
    loop {
        let n = f.read(&mut buf).with_context(|| "read file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

/// Extrait les n premiers caractères du hash en hexadécimal majuscule.
///
/// # Arguments
/// * `hash` - Le hash BLAKE3
/// * `n` - Nombre de caractères à extraire
///
/// # Returns
/// Les n premiers caractères du hash en hexadécimal majuscule
pub fn hash_prefix(hash: &blake3::Hash, n: usize) -> String {
    let hex = hash.to_hex(); // 64 chars hex
    hex[..n.min(hex.len())].to_string().to_uppercase()
}

/// Déplace ou copie un fichier de src vers dest.
///
/// Tente d'abord un rename (rapide), puis fallback sur copy+remove si nécessaire
/// (utile pour les déplacements cross-device ou sur Windows).
///
/// # Arguments
/// * `src` - Chemin source
/// * `dest` - Chemin destination
/// * `dry_run` - Si true, simule l'opération sans la réaliser
///
/// # Returns
/// Ok si l'opération réussit
///
/// # Errors
/// Retourne une erreur si le déplacement/copie échoue
pub fn move_or_copy(src: &Path, dest: &Path, dry_run: bool) -> Result<()> {
    // Crée le dossier cible si nécessaire
    if let Some(parent) = dest.parent() {
        if !dry_run {
            fs::create_dir_all(parent)
                .with_context(|| format!("create_dir_all {}", parent.display()))?;
        }
    }

    log::info!("[MOVE] {} -> {}", src.display(), dest.display());

    if dry_run {
        return Ok(());
    }

    // On tente un rename (rapide)…
    match fs::rename(src, dest) {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            // …et en cas d'échec, on tente un fallback copy+remove,
            // qui marche aussi cross-device et sur Windows.
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

/// Vérifie si un répertoire contient des fichiers média supportés.
///
/// # Arguments
/// * `root` - Racine du répertoire à vérifier
/// * `exts` - Liste des extensions supportées
///
/// # Returns
/// true si au moins un fichier supporté est trouvé, false sinon
pub fn contains_supported_media(root: &Path, exts: &[String]) -> bool {
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() && is_supported(entry.path(), exts) {
            return true;
        }
    }
    false
}

/// Vérifie si un répertoire est vide.
///
/// # Arguments
/// * `dir` - Chemin du répertoire
///
/// # Returns
/// true si le répertoire est vide, false sinon
///
/// # Errors
/// Retourne une erreur si le répertoire ne peut pas être lu
pub fn is_dir_empty(dir: &Path) -> Result<bool> {
    Ok(fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .next()
        .is_none())
}

/// Supprime récursivement les dossiers vides (mais ne supprime jamais un dossier non-vide).
///
/// # Arguments
/// * `root` - Racine du répertoire à nettoyer
///
/// # Returns
/// Ok si l'opération réussit
///
/// # Errors
/// Retourne une erreur si la suppression échoue
pub fn prune_empty_dirs_recursively(root: &Path) -> Result<()> {
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
