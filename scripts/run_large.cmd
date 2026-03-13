@echo off
:: Large run: 50 trees, up to 10 nodes, with JSON export
cd /d "%~dp0.."
cargo run --release -- generate --count 50 --labels 3 --max-nodes 10 --out .\output\large --export-json
