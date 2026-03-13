@echo off
:: Basic run: 10 trees, TREE(3), default largest strategy, SVG output
cd /d "%~dp0.."
cargo run -- generate --count 10 --labels 3 --max-nodes 8 --out .\output\basic
