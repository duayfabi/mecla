use chrono::{Datelike, NaiveDateTime, Timelike};
use std::path::{Component, Path, PathBuf};

/// Infère le tag à partir du chemin relatif du fichier par rapport à input_root.
///
/// Si le fichier est directement sous input_root, retourne None.
/// Sinon, retourne le nom du premier sous-dossier comme tag.
///
/// # Arguments
/// * `input_root` - Racine du répertoire d'entrée
/// * `src` - Chemin du fichier source
///
/// # Returns
/// Le tag (nom du dossier parent) ou None si le fichier est à la racine
pub fn infer_tag(input_root: &Path, src: &Path) -> Option<String> {
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

/// Construit le chemin du répertoire de destination.
///
/// Format: output_root/YYYY/MM ou output_root/YYYY/MM TAG
///
/// # Arguments
/// * `output_root` - Racine du répertoire de sortie
/// * `dt` - Date/heure du fichier
/// * `tag` - Tag optionnel (nom du dossier source)
///
/// # Returns
/// Le chemin complet du répertoire de destination
pub fn build_target_dir(output_root: &Path, dt: &NaiveDateTime, tag: Option<&str>) -> PathBuf {
    let year = format!("{:04}", dt.year());
    let month = format!("{:02}", dt.month());

    let month_dir_name = match tag {
        Some(t) if !t.trim().is_empty() => format!("{} {}", month, t.trim()),
        _ => month,
    };

    output_root.join(year).join(month_dir_name)
}

/// Formate le nom de fichier basé sur la date.
///
/// Format: YYYY-MM-DD HH.MM.SS.ext
///
/// # Arguments
/// * `dt` - Date/heure du fichier
/// * `ext` - Extension du fichier (sans point)
///
/// # Returns
/// Le nom de fichier formaté
pub fn format_filename(dt: &NaiveDateTime, ext: &str) -> String {
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

/// Formate le nom de fichier avec un suffixe (pour gérer les collisions).
///
/// Format: YYYY-MM-DD HH.MM.SS SUFFIX.ext
///
/// # Arguments
/// * `dt` - Date/heure du fichier
/// * `suffix` - Suffixe à ajouter (généralement un préfixe de hash)
/// * `ext` - Extension du fichier (sans point)
///
/// # Returns
/// Le nom de fichier formaté avec suffixe
pub fn format_filename_with_suffix(dt: &NaiveDateTime, suffix: &str, ext: &str) -> String {
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
