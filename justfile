# https://just.systems

default:
    @just --list

run-sqlite:
    cargo run -p balance-transfer

test:
    cargo test --workspace --all-features
