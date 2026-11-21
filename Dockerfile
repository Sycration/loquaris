# Loquaris: one image with the server and the web app embedded in it.
#   docker build -t loquaris .
#   docker run -p 8080:8080 -v loquaris:/data \
#     -e LOQ_PUBLIC_URL=https://loquaris.example loquaris

# 1. The analysis worker's wasm package (loquaris-core compiled to wasm).
FROM rust:1-bookworm AS wasm
RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-bindgen-cli --version 0.2.130 --locked
WORKDIR /src
COPY . .
RUN scripts/build-wasm.sh

# 2. The browser app.
FROM node:24-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg
RUN npm run build

# 3. The server, embedding web/dist (release builds embed it at compile time).
FROM rust:1-bookworm AS server
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist ./web/dist
RUN cargo build --release -p loquaris-server \
    && cp target/release/loquaris-server /usr/local/bin/

# 4. Runtime.
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /data --shell /usr/sbin/nologin loquaris \
    && mkdir /data && chown loquaris /data
COPY --from=server /usr/local/bin/loquaris-server /usr/local/bin/
USER loquaris
ENV LOQ_BIND=0.0.0.0:8080 \
    LOQ_DATA_DIR=/data \
    LOQ_PUBLIC_URL=http://localhost:8080 \
    LOQ_REGISTRATION=open
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["loquaris-server"]
CMD ["serve"]
