use std::io::Write;
use std::sync::OnceLock;

use tracing::info;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use directories::BaseDirs;

use crate::dispatch::*;
use crate::provider::Client;

// In general, use `App::args()` to fetch args.
// Set this directly if you really want to override initialization.
pub static ARGS: OnceLock<Arguments> = OnceLock::new();

#[derive(Parser, Debug, Clone)]
#[clap()]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The name of the LLM to use
    #[clap(short, long, global=true, env="DEFAULT_MODEL")]
    pub model: Option<String>,

    #[clap(long, global=true, default_value_t=0.0)]
    pub temperature: f32,

    #[clap(long, global=true, default_value_t=1024)]
    pub max_tokens: u32,

    /// User input, document to summarize, etc.
    #[clap(short, long, global=true)]
    pub text: Option<String>,

    /// Semi-colon list of paths containing more patterns
    #[clap(long, global=true, env="EXTRA_PATTERNS", hide_env_values=true)]
    pub extra_patterns: Option<String>,

    #[clap(long, global=true, env="CLAUDE_API_KEY", hide=true)]
    pub claude_api_key: Option<String>,
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
    pub fn args() -> &'static Arguments {
        ARGS.get_or_init(|| {
            let env_file = BaseDirs::new()
                .map(|p| p.config_dir().join("fabric/.env"))
                .filter(|p| p.is_file());

            if let Some(path) = env_file {
                // Possibly before configuring tracing subscribers
                //eprintln!("Loading config from {path:?}");
                dotenvy::from_path(path).ok();
            }

            if cfg!(debug_assertions) {
                dotenv().ok();
            }

            Arguments::parse()
        })
    }

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

    fn get_model_client(&self, args: &Arguments) -> Result<Box<dyn Client>> {
        let model = args.model.clone().ok_or(anyhow!("Model required"))?;
        let client = self.dispatcher.get_client(&model)?;
        Ok(client)
    }

    fn get_user_text(&self, args: &Arguments) -> Result<String> {
        Ok(args.text.clone()
            .ok_or(())
            .or_else(|_| std::io::read_to_string(std::io::stdin()))?)
    }

    pub async fn run(&self, args: &Arguments) -> Result<()> {
        let dispatcher = &self.dispatcher;

        match &args.command {
            Some(Command::ListPatterns) => {
                for name in dispatcher.list_patterns()? {
                    println!("{}", name)
                }
            },
            Some(Command::ListModels) => {
                for name in dispatcher.list_models()? {
                    println!("{}", name)
                }
            },
            Some(Command::Pipe { pattern }) => {
                let client = self.get_model_client(args)?;
                let pattern = dispatcher.get_pattern(&pattern)?;
                let text = self.get_user_text(args)?;

                let result = client.send_message(&pattern, &text).await?;
                info!("Message metadata {:?}", result.meta);
                println!("{}", &result.body);
            },
            Some(Command::Stream { pattern }) => {
                let client = self.get_model_client(args)?;
                let pattern = dispatcher.get_pattern(&pattern)?;
                let text = self.get_user_text(args)?;

                let result = client.stream_message(&pattern, &text).await?;
                info!("Message metadata {:?}", result.meta);

                let mut rx = result.rx;

                while let Some(Ok(msg)) = rx.recv().await {
                    print!("{}", msg);
                    std::io::stdout().flush().ok();
                }

            },
            _ => {
                todo!("Not implemented")
            }
        }

        Ok(())
    }
}
