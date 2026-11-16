FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /src/target/release/fogwood_relay /usr/local/bin/fogwood_relay
EXPOSE 7777/udp 8080
CMD ["fogwood_relay"]
