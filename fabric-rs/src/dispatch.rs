use std::path::PathBuf;
use std::path::Path;
use directories::BaseDirs;
use anyhow::{anyhow, Result};
use tracing::{instrument, info, debug};
use shellexpand;

use crate::patterns::*;
use crate::provider::*;
use crate::app::App;

pub struct Dispatcher {
    pub pattern_registries: Vec<Box<dyn PatternRegistry>>,
    pub providers: Vec<Box<dyn Provider>>,
}

impl Default for Dispatcher {
    fn default() -> Self {
        let pattern_dir = BaseDirs::new()
            .map(|p| p.config_dir().join("fabric/patterns"))
            .unwrap_or(PathBuf::from("./patterns"));

        let base = Self::empty()
            .with_patterns(Box::new(DirectoryPatternRegistry::new(pattern_dir)));

        let base = if let Some(api_key) = &App::args().claude_api_key {
            base.with_provider(Box::new(anthropic::AnthropicProvider::new(api_key)))
        } else {
            base
        };

        let extra = App::args().extra_patterns.clone().unwrap_or(String::new());

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
            providers: Vec::new(),
        }
    }

    pub fn with_patterns(self, more: Box<dyn PatternRegistry>) -> Self {
        let Self { mut pattern_registries, providers } = self;
        pattern_registries.push(more);

        Self {
            pattern_registries,
            providers,
        }
    }

    pub fn with_provider(self, more: Box<dyn Provider>) -> Self {
        let Self { pattern_registries, mut providers } = self;
        providers.push(more);

        Self {
            pattern_registries,
            providers,
        }
    }

    #[instrument(skip(self))]
    pub fn list_patterns(&self) -> Result<Vec<String>> {
        // Construct a new span named "my span" with trace log level.
        info!(answer = 42, question = "life, the universe, and everything");

        let all = self.pattern_registries.iter()
            .map(|r| r.list_patterns())
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

    pub fn list_models(&self) -> Result<Vec<String>> {
        Ok(self.providers.iter()
            .map(|p| p.list_models())
            .filter_map(|p| p.ok())
            .flatten()
            .collect())
    }

    pub fn get_client(&self, model: &str) -> Result<Box<dyn Client>> {
        self.providers.iter().map(|p| p.get_client(&model))
            .filter_map(|r| r.ok())
            .next()
            .ok_or(anyhow!("No providers for model {model}"))
    }
}

#[cfg(test)]
mod tests {
    use cool_asserts::assert_matches;
    use anyhow::bail;
    use async_trait::async_trait;

    use super::*;
    use crate::patterns::Pattern;
    use crate::session::ChatSession;

    struct DummyClient {
    }

    #[async_trait]
    impl Client for DummyClient {
        async fn send_message(&self, _pattern: &Pattern, _text: &ChatSession) -> Result<ChatResponse> {
            todo!()
        }

        async fn stream_message(&self, _pattern: &Pattern, _text: &ChatSession) -> Result<StreamResponse> {
            todo!()
        }
    }

    struct DummyProvider {
        models: Vec<String>,
    }

    impl Provider for DummyProvider {
        fn list_models(&self) -> Result<Vec<String>> {
            Ok(self.models.clone())
        }

        fn get_client(&self, name: &str) -> Result<Box<dyn Client>> {
            let model = name.to_string();
            if self.models.contains(&model) {
                Ok(Box::new(DummyClient {}))
            }
            else {
                bail!("Not here")
            }
        }
    }
    struct DummyPatterns {
        patterns: Vec<String>,
    }

    impl PatternRegistry for DummyPatterns {
        fn list_patterns(&self) -> Result<Vec<String>> {
            Ok(self.patterns.clone())
        }

        fn get_pattern(&self, name: &str) -> Result<Pattern> {
            let name = name.to_string();
            if self.patterns.contains(&name) {
                Ok(Pattern {
                    name,
                    system: String::new(),
                })
            }
            else {
                bail!("Not here")
            }
        }
    }

    #[test]
    fn empty_has_no_patterns() -> Result<()> {
        let dispatcher = Dispatcher::empty();
        let patterns = dispatcher.list_patterns()?;
        assert_eq!(patterns, vec![] as Vec<String>);
        Ok(())
    }

    #[test]
    fn patterns_from_all_registries() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["one".to_string(), "two".to_string()]
            }))
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec![]
            }))
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["three".to_string()]
            }));

        let mut patterns = disp.list_patterns()?;
        let mut expected = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        patterns.sort();
        expected.sort();
        assert_eq!(patterns, expected);

        Ok(())
    }

    #[test]
    fn load_missing_pattern_fails() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["one".to_string()]
            }))
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["two".to_string()]
            }));

        let result = disp.get_pattern("zero");
        assert_matches!(result, Err(_));

        Ok(())
    }

    #[test]
    fn load_existing_pattern_passes() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["one".to_string()]
            }))
            .with_patterns(Box::new(DummyPatterns {
                patterns: vec!["two".to_string()]
            }));

        let result = disp.get_pattern("two");
        assert_matches!(result, Ok(_));

        Ok(())
    }

    #[test]
    fn empty_has_no_models() -> Result<()> {
        let dispatcher = Dispatcher::empty();
        let models = dispatcher.list_models()?;
        assert_eq!(models, vec![] as Vec<String>);
        Ok(())
    }

    #[test]
    fn list_all_models() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_provider(Box::new(DummyProvider {
                models: vec!["one".to_string()]
            }))
            .with_provider(Box::new(DummyProvider {
                models: vec!["three".to_string()]
            }));

        let mut models = disp.list_models()?;
        let mut expected = vec!["one".to_string(), "three".to_string()];
        models.sort();
        expected.sort();
        assert_eq!(models, expected);

        Ok(())
    }

    #[test]
    fn load_missing_model_fails() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_provider(Box::new(DummyProvider {
                models: vec!["one".to_string()]
            }))
            .with_provider(Box::new(DummyProvider {
                models: vec!["three".to_string()]
            }));

        let result = disp.get_client("zero");
        assert!(matches!(result, Err(_)));

        Ok(())
    }

    #[test]
    fn load_existing_model_passes() -> Result<()> {
        let disp = Dispatcher::empty()
            .with_provider(Box::new(DummyProvider {
                models: vec!["one".to_string()]
            }))
            .with_provider(Box::new(DummyProvider {
                models: vec!["three".to_string()]
            }));

        let result = disp.get_client("three");
        assert!(matches!(result, Ok(_)));

        Ok(())
    }

}
