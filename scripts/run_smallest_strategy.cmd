@echo off
:: Smallest-first strategy: greedily pick smallest valid tree at each position
:: Produces a different (shorter) sequence compared to largest strategy
cd /d "%~dp0.."
cargo run -- generate --count 20 --labels 3 --max-nodes 8 --strategy smallest --out .\output\smallest --export-json
