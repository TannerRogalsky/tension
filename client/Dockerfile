FROM rust:1.52 as builder

RUN cargo install wasm-pack

COPY ./websocket ./websocket
COPY ./shared ./shared
COPY ./client ./client

WORKDIR ./client
RUN wasm-pack build --target web --out-dir ../docs/tension

WORKDIR .
COPY ./docs ./docs