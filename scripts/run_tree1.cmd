@echo off
:: TREE(1): only label=1 allowed, sequence length is 1 (just a single node)
cd /d "%~dp0.."
cargo run -- generate --count 5 --labels 1 --max-nodes 5 --out .\output\tree1 --export-json
