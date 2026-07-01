# novelpia_api

Unofficial API client for the [Novelpia](https://novelpia.com) web-novel
platform, written in Rust.

The workspace is split into two crates:

| Crate      | Kind    | Description                                                        |
| ---------- | ------- | ------------------------------------------------------------------ |
| `novelpia` | library | Typed async client over Novelpia's internal HTTP endpoints.        |
| `npia`     | binary  | Command-line front-end (`npia`) that drives the library.           |

> **Note:** This project is built from a reverse-engineered specification of
> Novelpia's private endpoints (see [`novelpia_openapi.yaml`](novelpia_openapi.yaml)).
> The upstream service is unofficial and may change without notice. Use it in
> accordance with Novelpia's terms of service.

## Requirements

- Rust `1.86` (pinned via [`rust-toolchain.toml`](rust-toolchain.toml); edition 2024).

## Building

```sh
cargo build --release
```

The `novelpia` library exposes an optional `decrypt` feature for AES decryption
of the viewer `c` field. Key derivation is not yet reverse-engineered, so it is
off by default and the raw ciphertext is returned.

```sh
cargo build -p novelpia --features decrypt
```

## Using the `npia` CLI

```
Usage: npia [OPTIONS] <COMMAND>

Commands:
  reviews          Fetch novel reviews (작품평)
  views            Fetch per-episode view counts (batch)
  episodes         Fetch episode list (HTML → typed)
  viewer           Fetch free episode text
  member           Fetch member profile
  alarm-cnt        Fetch alarm count
  curation         Fetch novel curation (writer's other novels)
  emoticon-groups  Fetch emoticon groups owned by the logged-in user
  emoticon-items   Fetch emoticons in a group owned by the logged-in user
  export           Export a novel's data bundle (episodes, view counts,
                   reviews) to CSV/JSON files
```

Global options:

| Option               | Env var       | Default | Description                              |
| -------------------- | ------------- | ------- | ---------------------------------------- |
| `--login-key <KEY>`  | `LOGINKEY`    | —       | `LOGINKEY` cookie value.                 |
| `--csrf <TOKEN>`     | `CSRF_TOKEN`  | —       | CSRF token for write endpoints.          |
| `--delay-min <MS>`   | —             | `1000`  | Minimum inter-request delay (ms).        |
| `--delay-max <MS>`   | —             | `3000`  | Maximum inter-request delay (ms).        |
| `--format <FORMAT>`  | —             | `json`  | Output format (`json` or `csv`).         |

Authenticated endpoints read `LOGINKEY` (and `CSRF_TOKEN` for writes) from the
environment, so you can avoid putting secrets on the command line:

```sh
export LOGINKEY="..."
npia reviews --novel 127306
npia export --novel 127306
```

The `export` command produces the CSV/JSON handoff artifacts consumed by the
downstream analysis layer.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all          # offline tests; live integration tests are #[ignore]d
```

## License

Licensed under the [MIT License](LICENSE).
