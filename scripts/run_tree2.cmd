@echo off
:: TREE(2) validation: known value is 3, so this should yield exactly 3 trees
cd /d "%~dp0.."
cargo run -- generate --count 5 --labels 2 --max-nodes 5 --out .\output\tree2 --export-json
