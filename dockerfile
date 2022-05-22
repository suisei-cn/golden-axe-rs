FROM rustlang/rust:nightly AS builder

WORKDIR /usr/src/app
COPY Cargo.lock .
COPY Cargo.toml .
RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY ./src src
RUN cargo build --release

# -----------------
# Final Stage
# -----------------

FROM fedora:34

WORKDIR /root
COPY --from=builder /usr/src/app/target/release/golden-axe ./
CMD [ "./golden-axe" ]
