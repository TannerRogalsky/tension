FROM rust:1.52

RUN USER=root cargo new --bin rust-docker-web
COPY ./shared ./shared
WORKDIR ./rust-docker-web
COPY ./server/Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ADD ./server ./

RUN rm ./target/release/deps/server*
EXPOSE 8000

CMD ["cargo", "run", "--release"]