# m3m0

native remnote client. rust + egui, rice-themed, keyboard-first.

reads a snapshot of remnote's local sqlite db (`~/remnote/*/remnote.db`), so it's safe while remnote runs.
writes (later) go through a small remnote plugin so remnote keeps owning sync.

```
cargo build --release
./target/release/m3m0 --install   # icon + .desktop entry
./target/release/m3m0
```

`?` in the app lists the keys. `:db` picks which database to read.

dev env: `M3M0_DB` (db path), `M3M0_OPEN="FC2/Week 8"`, `M3M0_KEYS` (replay keys), `M3M0_SHOTS=DIR` (save frames and quit).
