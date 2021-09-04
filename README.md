# Drumchords

Midi synth template project written in rust.

![screenshot](docs/screenshot.jpg)

## To build and run natively:
```
cargo run --release
```

## To build wasm version for web.
```
cargo install cargo-make

# option 1
cargo make build_web
# option 2
cargo make watch

# in separate shell
cargo make serve
```
 
Then open a browser (with webmidi support) and point it to http://localhost:8000