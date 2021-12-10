FROM rustlang/rust:nightly AS builder

WORKDIR /usr/src/app
COPY Cargo.lock .
COPY Cargo.toml .
RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY ./src src
RUN cargo build --release
RUN cargo install --path . --verbose

# -----------------
# Final Stage
# -----------------

FROM fedora:latest

COPY --from=builder /usr/local/cargo/bin/golden-axe /bin
