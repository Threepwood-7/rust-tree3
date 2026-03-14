@echo off
:: Optimal search for TREE(3) with a small node budget (max-nodes 5)
:: N=1788 candidates; precomputation is fast, DFS completes in seconds
cd /d "%~dp0.."
cargo run -- generate --labels 3 --max-nodes 5 --strategy optimal --out .\output\optimal_small --export-json
