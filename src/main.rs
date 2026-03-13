mod canonical;
mod cli;
mod embedding;
mod fingerprint;
mod generator;
mod svg_render;
mod tests;
mod tree;

use clap::Parser;
use cli::{Cli, Commands, StrategyArg};
use generator::{generate_sequence, SelectionStrategy};
use serde_json;
use std::fs;
use std::path::Path;
use svg_render::render_svg;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate(args) => run_generate(args),
    }
}

fn run_generate(args: cli::GenerateArgs) {
    let strategy = match args.strategy {
        StrategyArg::Smallest => SelectionStrategy::SmallestFirst,
        StrategyArg::Largest => SelectionStrategy::LargestFirst,
    };

    println!("=== TREE({}) Sequence Explorer ===", args.labels);
    println!(
        "Generating up to {} trees (i-th tree has <= i nodes, hard cap = {} nodes)",
        args.count, args.max_nodes
    );
    println!("Strategy: {:?}", args.strategy);
    println!();

    // Create output directory
    let out_dir = Path::new(&args.out);
    if let Err(e) = fs::create_dir_all(out_dir) {
        eprintln!("Failed to create output directory '{}': {}", args.out, e);
        std::process::exit(1);
    }

    let sequence = generate_sequence(
        args.count,
        args.max_nodes,
        args.labels,
        strategy,
        |entry| {
            println!(
                "[{:03}] Found tree ({} nodes): {}",
                entry.index,
                entry.tree.size(),
                entry.canonical
            );

            // Write SVG
            let svg_path = out_dir.join(format!("tree_{:03}.svg", entry.index));
            let title = format!(
                "T{} ({} nodes): {}",
                entry.index,
                entry.tree.size(),
                entry.canonical
            );
            let svg = render_svg(&entry.tree, &title);
            if let Err(e) = fs::write(&svg_path, svg) {
                eprintln!("Warning: failed to write {}: {}", svg_path.display(), e);
            }
        },
    );

    println!();
    println!("--- Summary ---");
    println!(
        "Found {} trees in the TREE({}) sequence.",
        sequence.len(),
        args.labels
    );

    if args.export_json {
        let json_path = out_dir.join("sequence.json");

        let json_data: Vec<serde_json::Value> = sequence
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "index": entry.index,
                    "nodes": entry.tree.size(),
                    "canonical": entry.canonical,
                    "tree": entry.tree,
                })
            })
            .collect();

        match serde_json::to_string_pretty(&json_data) {
            Ok(json_str) => {
                if let Err(e) = fs::write(&json_path, json_str) {
                    eprintln!("Warning: failed to write JSON: {}", e);
                } else {
                    println!("Sequence JSON written to: {}", json_path.display());
                }
            }
            Err(e) => eprintln!("Warning: failed to serialize JSON: {}", e),
        }
    }

    println!("SVG files written to: {}", out_dir.display());
    println!();

    // Print a compact summary table
    println!("{:<6} {:<8} {}", "Index", "Nodes", "Canonical Form");
    println!("{}", "-".repeat(60));
    for entry in &sequence {
        println!(
            "{:<6} {:<8} {}",
            entry.index,
            entry.tree.size(),
            entry.canonical
        );
    }
}
