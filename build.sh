#!/bin/sh -xe

# build it
cargo build --release

# test it with our camera
./target/release/kiyoctl probe
