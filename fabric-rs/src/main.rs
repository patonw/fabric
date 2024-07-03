use fabric_rs::hello;
use clap::{Parser, Subcommand};
use dotenv;

#[derive(Parser, Debug)]
#[clap()]
struct Arguments {
    #[clap(short,long, env="FABRIC_MODEL")]
    model: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Default, Debug)]
enum Command {
    /// List available patterns
    #[default]
    ListPatterns,

    /// Show all available models
    ListModels,

    /// See results in realtime
    Stream,

    /// Pipe output into another command
    Pipe,

    /// Initialize fabric
    Setup,

    /// Update patterns
    Update,
}

fn main() {
    dotenv::dotenv().ok();
    let args = Arguments::parse();

    match args.command {
        Some(Command::Stream) => {
            println!("Results")
        },
        _ => {
            println!("Patterns:")
        },
    }
}
