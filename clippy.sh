#!/bin/sh -ex
export PKG_CONFIG_PATH=/homes/cowsay/lib/x86_64-linux-gnu/pkgconfig
export RUSTFLAGS=-Awarnings
cargo clippy --no-default-features --features catnip-libos,catnap-libos,mlx5,tcp-migration,manual-tcp-migration,capy-log --all-targets