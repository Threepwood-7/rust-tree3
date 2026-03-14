@echo off
:: Optimal search for TREE(3) with a medium node budget (max-nodes 6)
:: N=11220 candidates; precomputation takes ~30s, DFS may run for many minutes
:: Use release build for meaningful throughput
cd /d "%~dp0.."
cargo run --release -- generate --labels 3 --max-nodes 6 --strategy optimal --out .\output\optimal_medium --export-json
