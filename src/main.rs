mod utils;

use std::path::PathBuf;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Main
{
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands
{
    #[command(name = "tolinearv1")]
    ToLinearV1(ConvertArgs),
    #[command(name = "tolinearv2")]
    ToLinearV2(ConvertArgs),
    #[command(name = "toanvil")]
    ToAnvil(ConvertArgs),
}

#[derive(Args, Debug, Clone)]
struct ConvertArgs {
    input_path: PathBuf,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long, default_value_t = 1)]
    compress_level: i32,

    #[arg(long)]
    cpu_num: Option<usize>,
}

fn main() {
    let args = Main::parse();

    match args.command {
        Commands::ToLinearV1(args) => {

        }
        Commands::ToLinearV2(args) => {

        }
        Commands::ToAnvil(args) => {

        }
    }

}
