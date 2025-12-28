# mecla

Utilitaire en ligne de commande pour **classer automatiquement des photos et vidéos**
selon leurs métadonnées (EXIF / QuickTime), avec un nommage déterministe et une
gestion robuste des doublons.

L’outil est écrit en **Rust** et s’appuie sur **exiftool** pour la lecture des métadonnées,
ce qui lui permet de fonctionner aussi bien avec des photos que des vidéos
(JPEG, HEIC, MP4, MOV, etc.).

---

## Fonctionnalités

- Classement par date : `YYYY/MM`
- Support des dossiers *tag* (ex: `Mariage XYZ`)
- Lecture des dates via **exiftool** (photos + vidéos)
- Nommage basé sur la date EXIF :
  ```
  YYYY-MM-DD HH.MM.SS.ext
  ```
- Gestion des conflits :
  - hash identique → le fichier source est supprimé
  - hash différent → suffixe aléatoire de 5 caractères
- Mode `--dry-run`
- Logs configurables (`all`, `conflicts`, `errors`)
- Nettoyage automatique des dossiers TAG vides après traitement
- Compatible Linux / macOS / Windows

---

## Principe de fonctionnement

### Arborescence d’entrée

Le programme prend en entrée un répertoire de dépôt :

```
depot/
├── IMG_001.jpg
├── IMG_002.jpg
└── Mariage XYZ/
    ├── DSC_0101.jpg
    └── DSC_0102.jpg
```

### Arborescence de sortie

En sortie, les fichiers sont déplacés vers :

```
output/
└── 2025/
    ├── 07/
    │   ├── 2025-07-23 08.54.04.jpg
    │   └── 2025-07-23 08.55.12.jpg
    └── 07 Mariage XYZ/
        ├── 2025-07-23 10.12.33.jpg
        └── 2025-07-23 10.14.02.jpg
```

### Règle des TAG

- Si un fichier est directement sous le répertoire d’entrée → pas de tag
- S’il est sous un sous-dossier du dépôt → ce dossier devient le tag

---

## Installation

### Dépendances

L’outil nécessite **exiftool**.

#### NixOS
```nix
environment.systemPackages = with pkgs; [
  exiftool
];
```

#### Autres systèmes
- Linux : paquet `exiftool`
- macOS : `brew install exiftool`
- Windows : installer exiftool depuis https://exiftool.org

---

## Compilation

```bash
cargo build --release
```

Le binaire est généré dans :
```
target/release/mecla
```

---

## Utilisation

### Dry-run (recommandé)
```bash
mecla \
  --input /chemin/depot \
  --output /chemin/output \
  --dry-run \
  --log all
```

### Exécution réelle
```bash
mecla \
  --input /chemin/depot \
  --output /chemin/output
```

### Options disponibles

| Option | Description |
|------|-------------|
| `--input` | Répertoire d’entrée (obligatoire) |
| `--output` | Répertoire de sortie (obligatoire) |
| `--dry-run` | Simule les actions sans modifier les fichiers |
| `--log all|conflicts|errors` | Niveau de verbosité |
| `--ext jpg --ext mp4` | Limite les extensions traitées |

---

## Gestion des doublons

- Si un fichier cible existe déjà :
  - **hash identique** → le fichier source est supprimé
  - **hash différent** → le fichier est renommé avec un comme suffixe les premiers caractères de son hash :
    ```
    2025-07-23 08.54.04 ABCDEFGH.jpg
    ```

Le hash utilisé est **BLAKE3** (pour sa rapidité et fiabilité).

---

## Nettoyage automatique

Après traitement :
- les dossiers TAG qui ne contiennent plus **aucune image ou vidéo**
  sont automatiquement supprimés (ainsi que leurs sous-dossiers vides)

---

## Philosophie

Cet outil est conçu pour :
- des archives personnelles
- un usage offline
- des traitements reproductibles
- une logique simple et explicite

---

## Licence

Usage personnel.
Aucune garantie.
Libre d’adapter le code à vos besoins.


