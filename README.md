# Wayfärer

Midi synth template project written in rust.

To build and run natively:
```
cargo run --release
```

To build wasm version for web.
```
cargo install cargo-make
cargo make watch
# in separate shell
cargo make serve
```