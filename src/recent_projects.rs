//! Recently opened and saved project files, stored separately from projects.
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

const LIMIT: usize = 15;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentProject {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    projects: Vec<RecentProject>,
}

#[derive(Default)]
pub struct RecentProjects {
    pub projects: Vec<RecentProject>,
    pub storage_error: Option<String>,
    path: Option<PathBuf>,
    blocked: bool,
}

impl RecentProjects {
    pub fn load_user() -> Self {
        match directories::ProjectDirs::from("", "", "loquaris") {
            Some(dirs) => Self::load_from(&dirs.config_dir().join("recent-projects.toml")),
            None => Self { storage_error: Some("Could not find a user configuration directory; recent projects will not be saved.".into()), blocked: true, ..Self::default() },
        }
    }

    fn load_from(path: &Path) -> Self {
        let mut recent = Self {
            path: Some(path.to_owned()),
            ..Self::default()
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return recent,
            Err(error) => {
                recent.storage_error = Some(format!(
                    "Could not read {}: {error}. Recent projects will not be saved.",
                    path.display()
                ));
                recent.blocked = true;
                return recent;
            }
        };
        match toml::from_str::<Settings>(&text) {
            Ok(settings) if settings.version == 1 => recent.projects = settings.projects,
            Ok(settings) => {
                recent.storage_error = Some(format!(
                    "Unsupported recent-projects version {} in {}. The file was left unchanged; recent projects will not be saved until it is fixed or moved.",
                    settings.version,
                    path.display()
                ));
                recent.blocked = true;
            }
            Err(error) => {
                recent.storage_error = Some(format!(
                    "Invalid recent-projects file {}: {error}. The file was left unchanged; recent projects will not be saved until it is fixed or moved.",
                    path.display()
                ));
                recent.blocked = true;
            }
        }
        recent
    }

    pub fn record(&mut self, path: &Path, name: &str) {
        self.projects.retain(|project| project.path != path);
        self.projects.insert(
            0,
            RecentProject {
                path: path.to_owned(),
                name: name.to_owned(),
            },
        );
        self.projects.truncate(LIMIT);
        self.persist();
    }

    pub fn forget(&mut self, path: &Path) {
        self.projects.retain(|project| project.path != path);
        self.persist();
    }

    fn persist(&mut self) {
        if self.blocked {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        let result = (|| -> Result<(), String> {
            let contents = toml::to_string_pretty(&Settings {
                version: 1,
                projects: self.projects.clone(),
            })
            .map_err(|error| error.to_string())?;
            let parent = path.parent().ok_or("Invalid recent-projects path")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let mut file =
                tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
            file.write_all(contents.as_bytes())
                .and_then(|_| file.as_file().sync_all())
                .map_err(|error| error.to_string())?;
            file.persist(path)
                .map_err(|error| error.error.to_string())?;
            Ok(())
        })();
        self.storage_error = result.err().map(|error| {
            format!(
                "Could not save recent projects to {}: {error}",
                path.display()
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_projects_persist_in_most_recent_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.toml");
        let mut recent = RecentProjects::load_from(&path);
        recent.record(Path::new("/one.toml"), "One");
        recent.record(Path::new("/two.toml"), "Two");
        recent.record(Path::new("/one.toml"), "Renamed");
        let mut loaded = RecentProjects::load_from(&path);
        assert_eq!(
            loaded
                .projects
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Renamed", "Two"]
        );
        loaded.forget(Path::new("/one.toml"));
        assert_eq!(RecentProjects::load_from(&path).projects.len(), 1);
    }

    #[test]
    fn invalid_settings_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.toml");
        let contents = "invalid = [";
        std::fs::write(&path, contents).unwrap();
        let mut recent = RecentProjects::load_from(&path);
        assert!(recent.storage_error.is_some());
        recent.record(Path::new("/new.toml"), "New");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }

    #[test]
    fn recent_list_is_bounded() {
        let mut recent = RecentProjects::default();
        for index in 0..20 {
            recent.record(
                Path::new(&format!("/{index}.toml")),
                &format!("Project {index}"),
            );
        }
        assert_eq!(recent.projects.len(), LIMIT);
        assert_eq!(recent.projects[0].name, "Project 19");
        assert_eq!(recent.projects.last().unwrap().name, "Project 5");
    }
}
