#!/usr/bin/env bash

set -e

# Build the tool to run the database merge
cd iocost-benchmarks-tools
cargo build -r
cd ..

# Build resctl-bench and copy the binary to the expected path
cd resctl-demo
cargo build -p resctl-bench -r
cd ../iocost-benchmarks
mkdir -p resctl-demo-v2.2
cp ../resctl-demo/target/release/resctl-bench resctl-demo-v2.2

# Merge the database results and generate the hwdb file
../iocost-benchmarks-tools/target/release/merge-results

cd ..
