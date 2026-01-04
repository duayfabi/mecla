use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Statistiques de traitement
#[derive(Debug, Clone)]
pub struct Stats {
    pub processed: Arc<AtomicUsize>,
    pub duplicates: Arc<AtomicUsize>,
    pub errors: Arc<AtomicUsize>,
    pub renamed: Arc<AtomicUsize>,
}

impl Stats {
    /// Crée de nouvelles statistiques initialisées à zéro
    pub fn new() -> Self {
        Self {
            processed: Arc::new(AtomicUsize::new(0)),
            duplicates: Arc::new(AtomicUsize::new(0)),
            errors: Arc::new(AtomicUsize::new(0)),
            renamed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Incrémente le compteur de fichiers traités
    pub fn inc_processed(&self) {
        self.processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Incrémente le compteur de doublons
    pub fn inc_duplicates(&self) {
        self.duplicates.fetch_add(1, Ordering::Relaxed);
    }

    /// Incrémente le compteur d'erreurs
    pub fn inc_errors(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Incrémente le compteur de fichiers renommés (collision de hash)
    pub fn inc_renamed(&self) {
        self.renamed.fetch_add(1, Ordering::Relaxed);
    }

    /// Affiche un résumé des statistiques
    pub fn print_summary(&self) {
        let processed = self.processed.load(Ordering::Relaxed);
        let duplicates = self.duplicates.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let renamed = self.renamed.load(Ordering::Relaxed);

        println!("\n=== Summary ===");
        println!("Files processed: {}", processed);
        println!("Duplicates skipped: {}", duplicates);
        println!("Files renamed (hash collision): {}", renamed);
        println!("Errors: {}", errors);
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}
