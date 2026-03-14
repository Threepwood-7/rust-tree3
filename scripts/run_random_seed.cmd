@echo off
:: Random strategy with a fixed seed for reproducible output
:: Change --seed to explore other sequences deterministically
cd /d "%~dp0.."
cargo run -- generate --count 20 --labels 3 --max-nodes 8 --strategy random --seed 42 --out .\output\random_seed --export-json
