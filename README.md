# Loquaris

A web app for writing and recording spoken Latin lessons, together. Each
lesson is a script of teacher and student lines, in Latin or the learner's own
language, with recorded audio for each line. Several people can work on the
same project at once and see each other's changes as they type.

- Write and reorder lessons and lines; undo and redo your own changes
- Work together in real time: see who is in the project, where they are
  working and what they are recording
- Record or import audio for each line, see its waveform as you record, crop
  it without losing the original, and play a lesson back
- Mark vowel lengths automatically (auto-macronize), choosing between readings
  when a word is ambiguous
- Search the whole project for a word or any of its forms, see how often it is
  used in each lesson, and find which words first appear in each lesson
- Export lessons as WAV audio and PDF scripts
- Accounts, projects shared with view or edit access, invite links, and
  version history
- Import projects from the earlier desktop app (`.loqr` files)

Latin parsing comes from `collo`, a Rust port of the
[Collatinus](https://github.com/biblissima/collatinus) morphological analyzer,
in the `collo` directory. It is an independent library; Loquaris runs it in
the browser, compiled to WebAssembly.

[docs/DESIGN.md](docs/DESIGN.md) describes the architecture.

## Layout

```
collo/             Latin analyzer library
crates/core/       Project model, word index, search, macronize (also runs as wasm)
crates/core-wasm/  Browser bindings for crates/core
crates/export/     PDF and WAV export, desktop .loqr files
crates/server/     HTTP API, accounts, real-time collaboration, storage
web/               The browser app (React, TypeScript, Yjs)
```

## Running it

The quickest way is Docker:

```sh
docker compose up -d
```

or, without Compose:

```sh
docker build -t loquaris .
docker run -d -p 8080:8080 -v loquaris:/data \
    -e LOQ_PUBLIC_URL=http://localhost:8080 loquaris
```

Then open http://localhost:8080 and create an account; the first account
becomes the administrator. Everything the server stores is in the `/data`
volume. For the administration commands below, run them in the container,
e.g. `docker compose exec loquaris loquaris-server reset-link you@example.com`.

To build without Docker you need a recent stable Rust toolchain (edition 2024)
with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` 0.2.130, and
Node.js 24:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked

cd web
npm ci
npm run build:wasm      # the Latin analyzer, for the browser
npm run build           # the app, into web/dist
cd ..
cargo build --release -p loquaris-server   # embeds web/dist
target/release/loquaris-server
```

The server is configured with environment variables:

| Variable | Default | |
|---|---|---|
| `LOQ_BIND` | `127.0.0.1:8080` | Address to listen on |
| `LOQ_DATA_DIR` | `./data` | Database and audio files; back up this directory |
| `LOQ_PUBLIC_URL` | `http://localhost:5173` | The address people use, for links and security checks |
| `LOQ_REGISTRATION` | `open` | `open`, `invite` (only with an invite link) or `closed` |
| `LOQ_TRUST_PROXY` | off | Set to `1` behind a reverse proxy, to see client addresses |

Serve it over HTTPS behind a reverse proxy that passes WebSocket upgrades
(`/api/projects/*/sync`). If nobody can sign in as an administrator:

```sh
loquaris-server create-admin someone@example.com "Their Name"
loquaris-server reset-link someone@example.com
```

## Development

Run the server and the Vite dev server side by side; Vite proxies the API to
the server:

```sh
cargo run -p loquaris-server
cd web && npm run dev       # http://localhost:5173
```

Run `npm run build:wasm` again after changing `collo` or `crates/core`.

## Tests

```sh
cargo test --workspace
cd web
npm test                    # unit tests
npx playwright install chromium
npx playwright test         # end to end, against a real server
```

## License

Loquaris is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The Collatinus code and data that `collo` is based on are licensed under the
GPL, version 2 or later. The bundled fonts, IBM Plex Sans and Source Serif 4,
are under the SIL Open Font License.
