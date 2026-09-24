# knit-md-docx — master

Referenced by: `/srv/GitHub/knit-md-docx-rs/MASTER.md`, the hub of the pair.

The Markdown-to-Word converter: the library `rust_knit_md_docx` and its command-line tool
`knit-md-docx`. It writes through the owner's `docx-rs` fork in the sibling repository
`knit-md-docx-rs`, a path dependency on `../knit-md-docx-rs/docx-core`
(see [`Cargo.toml`](Cargo.toml)). How to use it is in [`README.md`](README.md).

**This repository keeps no state of its own.** The pair is one project, and its heartbeat, task
file and ledger live in the hub: `/srv/GitHub/knit-md-docx-rs/MASTER.md` and
`/srv/GitHub/knit-md-docx-rs/TODO.csv`. A task for this repository is a row
there with `knit-md-docx` in its `Repo` cell.

## Commands

```sh
cargo test                                        # integration, unit and doc tests
cargo run --bin knit-md-docx -- examples/sample.md    # writes examples/sample.docx
cargo check --no-default-features --lib           # the library alone, without clap
```

**Releasing.** Set `version` in `Cargo.toml`, put the knit-md-docx-rs commit to build against in
`DOCX_RS_REV` (it must be on GitHub), commit, then push a tag `v<version>`. The workflow refuses a
tag that does not match `Cargo.toml`, or a binary whose `--version` does not print it.

## Map

| File | What it is |
| --- | --- |
| [`README.md`](README.md) | usage, features and known limitations |
| [`CODE_BREAKDOWN.md`](CODE_BREAKDOWN.md) | a plain-language tour of every source file in this crate and the fork |
| [`Cargo.toml`](Cargo.toml), [`Cargo.lock`](Cargo.lock) | the manifest; `cli` is a default feature carrying clap |
| [`src/lib.rs`](src/lib.rs) | the public entry points: `to_docx`, `to_bytes`, `convert_file`, `Converter` |
| [`src/main.rs`](src/main.rs) | the `knit-md-docx` command line |
| [`src/engine.rs`](src/engine.rs) | folds the Markdown event stream into Word paragraphs, runs, tables, lists and footnotes |
| [`src/math.rs`](src/math.rs) | the LaTeX-subset to OMML equation translator |
| [`src/options.rs`](src/options.rs) | `ConvertOptions` and `PageSetup` |
| [`src/styles.rs`](src/styles.rs) | the Word styles the output carries |
| [`src/error.rs`](src/error.rs) | the error type |
| [`tests/conversion.rs`](tests/conversion.rs) | integration tests that open the written `.docx` |
| [`examples/sample.md`](examples/sample.md) | a sample exercising every supported feature |
| [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE) | the dual licence |
| [`DOCX_RS_REV`](DOCX_RS_REV) | the knit-md-docx-rs commit a release is built against; move it when the fork changes |
| [`.github/workflows/release.yml`](.github/workflows/release.yml) | on a `v*` tag: tests, builds the Linux musl and Windows binaries, publishes them with `SHA256SUMS` |
| [`.gitignore`](.gitignore), [`.gitattributes`](.gitattributes), [`.dockerignore`](.dockerignore) | ignore, attribute and Docker build-context rules |

**Carried graph warning.** `examples/sample.md` points at no other file (`NO-OUTBOUND`). It is
the demo input the tool is run on, and a link added to it would be knitted into every sample
document, so the warning is carried by decision.
