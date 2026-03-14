@echo off
:: Optimal search for TREE(2): exhaustive DFS, confirms the known value of 3
:: Should terminate quickly (N=286 candidates with max-nodes 5, labels 2)
cd /d "%~dp0.."
cargo run -- generate --labels 2 --max-nodes 5 --strategy optimal --out .\output\optimal_tree2 --export-json
