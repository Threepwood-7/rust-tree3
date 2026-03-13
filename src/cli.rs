use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "tree3",
    about = "TREE(k) sequence explorer - generates valid sequences of labeled rooted trees",
    version = "0.1.0"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate a valid TREE(k) sequence and export SVGs/JSON
    Generate(GenerateArgs),
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyArg {
    /// Greedily pick smallest valid tree at each position (canonical order)
    Smallest,
    /// Greedily pick largest valid tree at each position (uses full node budget early)
    Largest,
}

#[derive(clap::Args, Debug)]
pub struct GenerateArgs {
    /// Number of trees to generate (omit to run until no valid tree remains)
    #[arg(long)]
    pub count: Option<usize>,

    /// Maximum number of nodes per tree (hard cap, independent of i-node rule)
    #[arg(long, default_value_t = 8)]
    pub max_nodes: usize,

    /// Label alphabet size (labels will be 1..=labels)
    #[arg(long, default_value_t = 3)]
    pub labels: u32,

    /// Output directory for SVG files
    #[arg(long, default_value = "./output")]
    pub out: String,

    /// Also write sequence.json to the output directory
    #[arg(long)]
    pub export_json: bool,

    /// Greedy selection strategy: smallest (pick smallest valid tree) or largest
    #[arg(long, default_value = "largest")]
    pub strategy: StrategyArg,
}
