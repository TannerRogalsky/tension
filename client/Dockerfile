FROM rust:1.52 as builder

RUN cargo install wasm-pack

COPY ./docs ./docs
COPY ./websocket ./websocket
COPY ./shared ./shared
COPY ./client ./client

WORKDIR ./client
RUN wasm-pack build --target web --out-dir ../docs