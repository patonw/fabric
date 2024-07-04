use std::path::PathBuf;
use std::path::Path;
use directories::BaseDirs;
use anyhow::{anyhow, Result};
use tracing::{instrument, info, debug};
use shellexpand;

use crate::patterns::*;
use crate::app::ARGS;

pub struct Dispatcher {
    pub pattern_registries: Vec<Box<dyn PatternRegistry>>,
}

impl Default for Dispatcher {
    fn default() -> Self {
        let pattern_dir = BaseDirs::new()
            .map(|p| p.config_dir().join("fabric/patterns"))
            .unwrap_or(PathBuf::from("./patterns"));

        let base = Self::empty()
            .with_patterns(Box::new(DirectoryPatternRegistry::new(pattern_dir)));

        let extra = ARGS.extra_patterns.clone().unwrap_or(String::new());

        extra.split(";")
            .filter_map(|s| shellexpand::full(s).ok())
            .map(|s| s.into_owned())
            .filter(|s| Path::new(s).is_dir())
            .fold(base, |dsp, dir| dsp.with_patterns(Box::new(DirectoryPatternRegistry::new(dir))))
    }
}

impl Dispatcher {
    pub fn empty() -> Self {
        Self {
            pattern_registries: Vec::new(),
        }
    }

    pub fn with_patterns(self, more: Box<dyn PatternRegistry>) -> Self {
        let Self { mut pattern_registries } = self;
        pattern_registries.push(more);

        Self {
            pattern_registries
        }
    }

    #[instrument(skip(self))]
    pub fn list_patterns(&self) -> Result<Vec<String>> {
        // Construct a new span named "my span" with trace log level.
        info!(answer = 42, question = "life, the universe, and everything");

        let all = self.pattern_registries.iter()
            .map(|r| r.iter_patterns())
            .map(|r| r.inspect_err(
                    |e| debug!("Failed to get patterns from registry: {e}")))
            .filter_map(|r| r.ok())
            .flatten()
            .collect();

        Ok(all)
    }

    pub fn get_pattern(&self, name: &str) -> Result<Pattern> {
        let mut result: Result<Pattern> = Err(anyhow!("No registries"));
        for reg in self.pattern_registries.iter() {
            result = reg.get_pattern(name);
            if result.is_ok() {
                return result
            }
        }

        result
    }
}

