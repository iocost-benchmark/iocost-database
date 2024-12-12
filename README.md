# iocost-database

This repository fetches releases of [resctl-demo](https://github.com/facebookexperimental/resctl-demo), [iocost-benchmarks](https://github.com/iocost-benchmark/iocost-benchmarks) and [iocost-benchmarks-ci](https://github.com/iocost-benchmark/iocost-benchmarks-ci) and contains a
script to automate the merging of the iocost-benchmarks database results
as an hwdb file.

The purpose of this repo is to serve as an upstream reference for repos
that want to deploy these benchmark results.

## Build requirements

- rust < 1.80 (1.79.0 recommended)

## How to build

    ./build.sh

The results will be generated in ~iocost-benchmarks~.
