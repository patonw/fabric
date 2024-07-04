use anyhow::{Result};
use clap::{Parser, Subcommand};
use dotenv::dotenv;
use lazy_static::lazy_static;
use directories::BaseDirs;

use crate::dispatch::*;

lazy_static! {
    pub static ref ARGS: Arguments = {
        let env_file = BaseDirs::new()
            .map(|p| p.config_dir().join("fabric/.env"))
            .filter(|p| p.is_file());

        if let Some(path) = env_file {
            // Possibly before configuring tracing subscribers
            //eprintln!("Loading config from {path:?}");
            dotenv::from_path(path).ok();
        }

        dotenv().ok();
        Arguments::parse()
    };
}

#[derive(Parser, Debug, Clone)]
#[clap()]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The name of the LLM to use
    #[clap(short,long, env="FABRIC_MODEL")]
    pub model: Option<String>,

    /// Semi-colon list of paths containing more patterns
    #[clap(long, env="EXTRA_PATTERNS")]
    pub extra_patterns: Option<String>,
}

#[derive(Subcommand, Default, Debug, Clone)]
pub enum Command {
    #[default]
    /// List available patterns
    ListPatterns,

    /// Show all available models
    ListModels,

    /// See results in realtime
    Stream {
        pattern: String,
    },

    /// Pipe output into another command
    Pipe {
        pattern: String,
    },

    /// Initialize fabric
    Setup,

    /// Update patterns
    Update,
}

pub struct App {
    pub dispatcher: Dispatcher,
}

impl Default for App {
    fn default() -> Self {
        Self::empty()
            .with_dispatcher(Dispatcher::default())
    }
}

impl App {
    pub fn empty() -> Self {
        Self {
            dispatcher: Dispatcher::empty(),
        }
    }

    pub fn with_dispatcher(self, dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher
        }
    }

    pub fn run(&self, args: &Arguments) -> Result<()> {
        let dispatcher = &self.dispatcher;

        match &args.command {
            Some(Command::Stream { pattern: _ }) => {
                println!("Results")
            },
            Some(Command::ListPatterns) => {
                for name in dispatcher.list_patterns()? {
                        println!("{}", name)
                }
            },
            Some(Command::Pipe { pattern }) => {
                let result = dispatcher.get_pattern(&pattern)?;
                dbg!(result);
            },
            _ => {
                todo!("Not implemented")
            }
        }

        Ok(())
    }
}
