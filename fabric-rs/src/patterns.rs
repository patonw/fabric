use std::path::PathBuf;
use anyhow::{Result};
use tracing::{instrument, debug};

type StringSeq = Box<dyn Iterator<Item=String>>;
type PathSeq = Box<dyn Iterator<Item=PathBuf>>;

#[derive(Debug, Clone)]
pub struct Pattern {
    pub name: String,
    pub system: String,
}

pub trait PatternRegistry {
    fn iter_patterns(&self) -> Result<StringSeq>;
    fn get_pattern(&self, name: &str) -> Result<Pattern>;
}

pub struct DirectoryPatternRegistry {
    pattern_dir: PathBuf,
}

impl PatternRegistry for DirectoryPatternRegistry {
    #[instrument(skip(self))]
    fn iter_patterns(&self) -> Result<StringSeq> {
        let result = self.iter_paths()?
            .filter_map(|p| p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string()));

        Ok(Box::new(result))
    }

    #[instrument(skip(self))]
    fn get_pattern(&self, name: &str) -> Result<Pattern> {
        let dir = &self.pattern_dir;
        let path = dir.join(name).join("system.md");

        debug!(path=path.to_str(), "Reading pattern file");
        let system = std::fs::read_to_string(path)?;
        let name = name.to_string();

        Ok(Pattern { name, system })
    }
}

impl DirectoryPatternRegistry {
    pub fn new<T: Into<PathBuf>>(pattern_dir: T) -> Self {
        let pattern_dir: PathBuf = pattern_dir.into();
        Self {
            pattern_dir,
        }
    }

    #[instrument(skip(self))]
    fn iter_paths(&self) -> Result<PathSeq> {
        let dir = &self.pattern_dir;
        debug!(path=dir.to_str(), "patterns dir");

        let result = std::fs::read_dir(&dir)?
            .filter_map(|d| d.ok())
            .map(|ent| ent.path());

        Ok(Box::new(result))
    }
}

