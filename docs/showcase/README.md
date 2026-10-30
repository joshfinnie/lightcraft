# README showcase

The screenshots in `docs/images/` are captured by driving LightCraft over its control channel.

```sh
# public-domain set (downloaded into the gitignored corpus/, see assets/ATTRIBUTION.md for sources)
cargo run --release -p lightcraft -- --memory --control 7980 corpus/images/pd &
python3 docs/showcase/run.py docs/showcase/pd-edits.jsonl

# procedural demo library
cargo run --release -p lightcraft -- --memory --control 7980 &
python3 docs/showcase/run.py docs/showcase/demo.jsonl

# compress for the repo (macOS)
for f in docs/images/*.png; do sips -s format jpeg -s formatOptions 82 "$f" --out "${f%.png}.jpg" && rm "$f"; done
```

`run.py FILE.jsonl [PORT]` sends each JSON line (`{"method":…,"params":…}` or `{"sleep": seconds}`) to
`127.0.0.1:PORT` (default 7980). It first authenticates with the run's control token:
`$LIGHTCRAFT_CONTROL_TOKEN`, else the `control-token-PORT` file the app writes to its settings folder
([control-protocol.md](../control-protocol.md#authentication)).
