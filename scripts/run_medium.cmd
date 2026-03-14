@echo off
:: Medium run: max-nodes 9, release build (~3.5M candidates, ~700 MB RAM, ~20s on 8 cores)
cd /d "%~dp0.."
cargo run --release -- generate --labels 3 --max-nodes 9 --strategy largest --out .\output\medium --export-json
