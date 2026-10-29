# syntax=docker/dockerfile:1
FROM node:24-alpine AS web
WORKDIR /src
RUN npm install --global pnpm@11.15.0
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY web/package.json web/package.json
RUN pnpm install --frozen-lockfile
COPY web web
RUN pnpm build

FROM rust:1.98.0-alpine AS rust
RUN apk add --no-cache musl-dev build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
COPY docs/openapi.json docs/openapi.json
RUN cargo build --locked --release -p beweft-server -p beweft-cli --bins

FROM alpine:3.22
RUN apk add --no-cache ca-certificates ffmpeg libgcc && \
    addgroup -g 10001 beweft && adduser -D -u 10001 -G beweft beweft && \
    mkdir -p /app/media && chown beweft:beweft /app/media
COPY --from=rust /src/target/release/beweft-server /src/target/release/beweft-worker /src/target/release/beweft /usr/local/bin/
COPY --from=web /src/web/dist /app/web
COPY scripts/container-entrypoint /usr/local/bin/container-entrypoint
USER 10001:10001
WORKDIR /app
ENV BEWEFT_BIND=0.0.0.0:3000 BEWEFT_WEB_DIR=/app/web BEWEFT_MEDIA_DIR=/app/media BEWEFT_ENABLE_FIXTURES=false
EXPOSE 3000
ENTRYPOINT ["container-entrypoint"]
CMD ["beweft-server"]
