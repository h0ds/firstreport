# Models

Gitignored. Run `cargo run -- fetch-model` to download the quantized
MiniLM-L6 ONNX model, its WordPiece vocab, and the ONNX Runtime shared
library here (~40 MB total). Never fetched at build time. To use a different
runtime build or model, point `[embed]` in `config/feed.toml` at it.
