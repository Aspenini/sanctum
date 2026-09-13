use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum StorageMode {
    #[default]
    PerUser,
    Portable,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScaleMode {
    #[default]
    Integer,
    Fit,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct LibraryEntry {
    pub id: Uuid,
    pub project_root: PathBuf,
    pub entrypoint: PathBuf,
    pub title: String,
    pub favorite: bool,
    pub added_at: u64,
    pub last_played: Option<u64>,
    pub cover: Option<PathBuf>,
}

impl Default for LibraryEntry {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            project_root: PathBuf::new(),
            entrypoint: PathBuf::new(),
            title: "HolyC Program".into(),
            favorite: false,
            added_at: now(),
            last_played: None,
            cover: None,
        }
    }
}

impl LibraryEntry {
    pub fn source_path(&self) -> PathBuf {
        self.project_root.join(&self.entrypoint)
    }

    pub fn from_source(path: &Path) -> io::Result<Self> {
        let source = path.canonicalize()?;
        let project_root = source.parent().unwrap_or(Path::new(".")).to_path_buf();
        let entrypoint = source.file_name().map(PathBuf::from).unwrap_or_default();
        let title = source
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "HolyC Program".into());
        Ok(Self {
            project_root,
            entrypoint,
            title,
            ..Self::default()
        })
    }

    pub fn from_project(project_root: &Path, entrypoint: &Path) -> io::Result<Self> {
        let project_root = project_root.canonicalize()?;
        let source = project_root.join(entrypoint).canonicalize()?;
        let entrypoint = source
            .strip_prefix(&project_root)
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "entrypoint is outside project")
            })?
            .to_path_buf();
        if !source.is_file()
            || !source
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("HC"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "entrypoint is not a .HC file",
            ));
        }
        let title = source
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "HolyC Program".into());
        Ok(Self {
            project_root,
            entrypoint,
            title,
            ..Self::default()
        })
    }

    pub fn from_directory(path: &Path) -> io::Result<Self> {
        let project_root = path.canonicalize()?;
        if !project_root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "project root is not a directory",
            ));
        }
        let title = project_root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "HolyC Project".into());
        Ok(Self {
            project_root,
            entrypoint: PathBuf::new(),
            title,
            ..Self::default()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveredEntry {
    Source(PathBuf),
    Project(PathBuf),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SanctumConfig {
    pub version: u32,
    pub storage_mode: StorageMode,
    pub library: Vec<LibraryEntry>,
    pub templeos_root: Option<PathBuf>,
    pub scale_mode: ScaleMode,
    pub muted: bool,
}

impl Default for SanctumConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            storage_mode: StorageMode::PerUser,
            library: Vec::new(),
            templeos_root: None,
            scale_mode: ScaleMode::Integer,
            muted: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StoragePaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub covers: PathBuf,
    pub portable: bool,
    pub executable_dir: PathBuf,
}

impl StoragePaths {
    pub fn detect() -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let executable_dir = executable.parent().unwrap_or(Path::new(".")).to_path_buf();
        let portable = executable_dir.join("portable.mode").is_file();
        let root = if portable {
            executable_dir.join("SanctumData")
        } else {
            ProjectDirs::from_path(PathBuf::from("Sanctum"))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "user data directory unavailable")
                })?
                .data_local_dir()
                .to_path_buf()
        };
        Ok(Self {
            config: root.join("sanctum.json"),
            covers: root.join("covers"),
            root,
            portable,
            executable_dir,
        })
    }

    pub fn ensure(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(&self.covers)
    }
}

pub fn load(paths: &StoragePaths) -> io::Result<SanctumConfig> {
    paths.ensure()?;
    if !paths.config.exists() {
        return Ok(SanctumConfig::default());
    }
    let bytes = fs::read(&paths.config)?;
    let mut config: SanctumConfig = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if config.version > CONFIG_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "library version {} is newer than this Sanctum build",
                config.version
            ),
        ));
    }
    config.version = CONFIG_VERSION;
    Ok(config)
}

/// Load configuration, preserving an unreadable database beside the replacement.
pub fn load_recovering(paths: &StoragePaths) -> io::Result<(SanctumConfig, Option<PathBuf>)> {
    match load(paths) {
        Ok(config) => Ok((config, None)),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            let backup = paths.root.join(format!("sanctum.corrupt-{}.json", now()));
            fs::rename(&paths.config, &backup)?;
            Ok((SanctumConfig::default(), Some(backup)))
        }
        Err(error) => Err(error),
    }
}

pub fn save(paths: &StoragePaths, config: &SanctumConfig) -> io::Result<()> {
    paths.ensure()?;
    let temporary = paths.config.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(config).map_err(io::Error::other)?;
    fs::write(&temporary, bytes)?;
    if paths.config.exists() {
        fs::remove_file(&paths.config)?;
    }
    fs::rename(temporary, &paths.config)
}

pub fn discover_holyc(folder: &Path) -> io::Result<Vec<PathBuf>> {
    fn visit(root: &Path, current: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
        for item in fs::read_dir(current)? {
            let item = item?;
            let path = item.path();
            if item.file_type()?.is_dir() {
                let name = item.file_name();
                if !name.to_string_lossy().starts_with('.') && name != "target" {
                    visit(root, &path, found)?;
                }
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("HC"))
            {
                found.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            }
        }
        Ok(())
    }
    let root = folder.canonicalize()?;
    let mut found = Vec::new();
    visit(&root, &root, &mut found)?;
    found.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    Ok(found)
}

/// Discover the programs directly contained by a library directory.
///
/// Top-level `.HC` files are standalone programs. Each immediate child
/// directory containing at least one `.HC` file is one multi-file project.
pub fn discover_library(folder: &Path) -> io::Result<Vec<DiscoveredEntry>> {
    let root = folder.canonicalize()?;
    let mut entries = Vec::new();
    for item in fs::read_dir(&root)? {
        let item = item?;
        let path = item.path();
        let file_type = item.file_type()?;
        if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("HC"))
        {
            entries.push(DiscoveredEntry::Source(path.canonicalize()?));
        } else if file_type.is_dir() {
            let name = item.file_name();
            if !name.to_string_lossy().starts_with('.')
                && name != "target"
                && !discover_holyc(&path)?.is_empty()
            {
                entries.push(DiscoveredEntry::Project(path.canonicalize()?));
            }
        }
    }
    entries.sort_by_key(|entry| match entry {
        DiscoveredEntry::Source(path) | DiscoveredEntry::Project(path) => {
            path.to_string_lossy().to_ascii_lowercase()
        }
    });
    Ok(entries)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let root = std::env::temp_dir().join(format!("sanctum-model-{}", Uuid::new_v4()));
        let paths = StoragePaths {
            config: root.join("sanctum.json"),
            covers: root.join("covers"),
            root: root.clone(),
            portable: true,
            executable_dir: root.clone(),
        };
        let mut config = SanctumConfig {
            muted: true,
            ..SanctumConfig::default()
        };
        config.library.push(LibraryEntry::default());
        save(&paths, &config).unwrap();
        let loaded = load(&paths).unwrap();
        assert!(loaded.muted);
        assert_eq!(loaded.library.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_entrypoint_keeps_the_selected_root() {
        let root = std::env::temp_dir().join(format!("sanctum-project-{}", Uuid::new_v4()));
        let nested = root.join("Source");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("Main.HC"), b"U0 Main() {}\n").unwrap();
        let entry = LibraryEntry::from_project(&root, Path::new("Source/Main.HC")).unwrap();
        assert_eq!(entry.project_root, root.canonicalize().unwrap());
        assert_eq!(entry.entrypoint, PathBuf::from("Source/Main.HC"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn library_discovery_separates_sources_and_project_folders() {
        let root = std::env::temp_dir().join(format!("sanctum-library-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("Game/Source")).unwrap();
        fs::create_dir_all(root.join("Empty")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("Standalone.hc"), b"U0 Main() {}\n").unwrap();
        fs::write(root.join("Game/Main.HC"), b"#include \"Source/Util.HC\"\n").unwrap();
        fs::write(root.join("Game/Source/Util.HC"), b"U0 Util() {}\n").unwrap();
        fs::write(root.join("Empty/readme.txt"), b"nothing here\n").unwrap();
        fs::write(root.join(".hidden/Hidden.HC"), b"U0 Main() {}\n").unwrap();
        fs::write(root.join("target/Generated.HC"), b"U0 Main() {}\n").unwrap();

        let discovered = discover_library(&root).unwrap();
        assert_eq!(discovered.len(), 2);
        assert!(matches!(&discovered[0], DiscoveredEntry::Project(path) if path.ends_with("Game")));
        assert!(
            matches!(&discovered[1], DiscoveredEntry::Source(path) if path.ends_with("Standalone.hc"))
        );

        let project = LibraryEntry::from_directory(&root.join("Game")).unwrap();
        assert!(project.entrypoint.as_os_str().is_empty());
        assert_eq!(project.title, "Game");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_config_is_preserved_and_recovered() {
        let root = std::env::temp_dir().join(format!("sanctum-corrupt-{}", Uuid::new_v4()));
        let paths = StoragePaths {
            config: root.join("sanctum.json"),
            covers: root.join("covers"),
            root: root.clone(),
            portable: true,
            executable_dir: root.clone(),
        };
        paths.ensure().unwrap();
        fs::write(&paths.config, b"not json").unwrap();
        let (config, backup) = load_recovering(&paths).unwrap();
        assert!(config.library.is_empty());
        assert!(backup.unwrap().is_file());
        assert!(!paths.config.exists());
        let _ = fs::remove_dir_all(root);
    }
}
