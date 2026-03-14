@echo off
:: Random strategy: picks a uniformly random valid tree at each position
:: Re-run to explore different sequences; each run uses a fresh time-based seed
cd /d "%~dp0.."
cargo run -- generate --count 20 --labels 3 --max-nodes 8 --strategy random --out .\output\random --export-json
