FROM rustlang/rust:nightly AS builder

WORKDIR /prod
COPY Cargo.lock .
COPY Cargo.toml .
RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY ./src src
RUN cargo build --release

# -----------------
# Final Stage
# -----------------

FROM fedora:34 AS runner

COPY --from=builder /prod/target/release/golden-axe /bin
