@echo off
:: Run until the candidate pool is exhausted (no --count limit)
:: Sequence terminates naturally when no valid tree remains for the current node budget
cd /d "%~dp0.."
cargo run -- generate --labels 3 --max-nodes 8 --strategy largest --out .\output\exhausted --export-json
