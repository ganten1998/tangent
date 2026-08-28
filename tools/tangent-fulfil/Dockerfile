# Build context MUST be the repo root — this crate depends on ../../tangent-core
# by path, so a context of tools/tangent-fulfil cannot see it:
#   docker build -f tools/tangent-fulfil/Dockerfile -t tangent-fulfil .
FROM rust:1-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# tangent-core inherits version/edition/license/rust-version/lints from the root
# workspace manifest, so that manifest must be here or cargo fails with
# "failed to find a workspace root". A workspace will not load with a member
# missing, so EVERY member the root lists has to be copied — hence the GUI
# crates, purely so the manifest parses. They are never built: tools/tangent-fulfil
# declares its own empty [workspace], which is what keeps it standalone.
#
# If you add a workspace member, add it here. The failure is at the cargo step,
# not the COPY, and it reads as a workspace error rather than a missing file.
COPY Cargo.toml ./
COPY tangent-core ./tangent-core
COPY tangent-ui ./tangent-ui
COPY ivory ./ivory
COPY tools/tangent-fulfil ./tools/tangent-fulfil
WORKDIR /src/tools/tangent-fulfil
RUN cargo build --release

FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/tools/tangent-fulfil/target/release/tangent-fulfil /usr/local/bin/tangent-fulfil
# Persist this: it is the record of every key ever issued.
VOLUME ["/data"]
ENV LEDGER_PATH=/data/ledger.jsonl PORT=8080
EXPOSE 8080
CMD ["tangent-fulfil"]
