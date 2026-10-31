# Loquaris

A desktop app for writing and recording spoken Latin lessons. Each lesson is a
script of teacher and student lines, in Latin or the learner's own language,
with recorded audio for each line.

- Write and reorder lines, with undo and redo
- Record or import audio for each line, and play a lesson back
- Mark vowel lengths automatically (auto-macronize)
- Search the whole project for a word or any of its forms, see how often it is
  used in each lesson, and find which words each lesson introduces
- Export lessons as WAV audio and PDF scripts

Latin parsing comes from `collo`, a Rust port of the
[Collatinus](https://github.com/biblissima/collatinus) morphological analyzer,
in the `collo` directory.

## Building

You need a recent stable Rust toolchain (edition 2024, Rust 1.85 or newer),
from [rustup](https://rustup.rs).

On Linux, audio needs the ALSA, PipeWire and JACK development files, and
Clang for generating the PipeWire bindings:

```sh
# Arch
sudo pacman -S alsa-lib pipewire jack2 clang pkgconf

# Debian / Ubuntu
sudo apt install libasound2-dev libpipewire-0.3-dev libjack-jackd2-dev \
    libclang-dev pkg-config
```

macOS and Windows need nothing extra.

Then build and run:

```sh
cargo run --release
```

The binary is written to `target/release/loquaris`. The first build takes a
while, because `collo` compiles the Collatinus lexicon into the program.

To try the Latin analyzer on its own, without the GUI:

```sh
cargo run --release --example collo -- "Arma virumque cano"
```

## Tests

```sh
cargo test
```

## License

Loquaris is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The Collatinus code and data that `collo` is based on are licensed under the
GPL, version 2 or later. The bundled fonts, IBM Plex Sans and Source Serif 4,
are under the SIL Open Font License.
