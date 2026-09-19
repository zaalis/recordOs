# Build from the agent/ directory:
#   docker build -f crates/vp-relay/Dockerfile -t vision-pro-relay .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p vp-relay --bin vp-relay

FROM debian:bookworm-slim
RUN useradd --system --no-create-home relay
COPY --from=build /src/target/release/vp-relay /usr/local/bin/vp-relay
USER relay
EXPOSE 443/tcp 3478/udp 3478/tcp 5349/tcp
ENTRYPOINT ["/usr/local/bin/vp-relay"]
