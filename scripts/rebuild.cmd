@echo off
:: Clean rebuild in release mode
cd /d "%~dp0.."
cargo clean
cargo build --release
