FROM rust:alpine AS build

RUN apk add build-base
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY src src
RUN cargo build

FROM ubuntu:latest

ENV CI=true GITHUB_ACTIONS=true HOARD_LOG=debug
COPY ci-tests ci-tests
COPY --from=build target/debug/hoard target/debug/hoard

RUN apt-get update && apt-get install -y tree python3
RUN python3 ci-tests/test.py last_paths
RUN python3 ci-tests/test.py operation
RUN python3 ci-tests/test.py ignore
