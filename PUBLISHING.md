# Publishing to crates.io

Releases are cut by pushing a semver tag. The
[`Release` workflow](.github/workflows/release.yml) authenticates to crates.io
with **Trusted Publishing (OIDC)** — there is no `CARGO_REGISTRY_TOKEN` secret
stored in the repository.

## Published crates

| Package on crates.io | Imported as | Notes |
|----------------------|-------------|-------|
| `dicos`              | `dicos`     | core library + `dicosctl` binary |
| `pure-jpegrle`       | `jpegrle`   | DICOM RLE PackBits codec |
| `pure-jpegls`        | `jpegls`    | JPEG-LS codec |
| `pure-jpegli`        | `jpegli`    | JPEG Lossless codec |
| `pure-jpeg2k`        | `jpeg2k`    | JPEG 2000 codec |

The codec crates use a `pure-` prefix because they are pure-Rust
implementations (the plain `jpegli`/`jpeg2k` names on crates.io are native C
wrappers). Each keeps its plain `lib` name, so `use jpegli::…` is unchanged for
consumers; they just depend on `pure-jpegli` in `Cargo.toml`.

`roxel` (the GPU viewer) is marked `publish = false` and is not released.

## One-time setup

Trusted Publishing requires you to own a crate before you can register a
publisher for it. So the very first release of each new crate name needs a
token; every release after that is tokenless.

1. **Bootstrap each crate name once, locally**, from a clean checkout:

   ```sh
   cargo login            # paste a token from https://crates.io/settings/tokens
   cargo publish -p pure-jpegrle
   cargo publish -p pure-jpegls
   cargo publish -p pure-jpegli
   cargo publish -p pure-jpeg2k
   cargo publish -p dicos
   ```

2. **Register the Trusted Publisher** for each of the five crates. On
   crates.io go to each crate → *Settings* → *Trusted Publishing* → *Add* and
   enter:

   - Repository owner: `jpfielding`
   - Repository name: `dicos.rs`
   - Workflow filename: `release.yml`
   - Environment: *(leave blank)*

After that, releases are fully automated.

## Cutting a release

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`) and the
   matching `version` pins under `[workspace.dependencies]`.
2. Commit and tag with a matching `v` prefix:

   ```sh
   git tag v1.0.0
   git push origin v1.0.0
   ```

The workflow verifies the tag matches the `dicos` crate version, then publishes
all five crates in dependency order (codecs first, `dicos` last).

## Local dry run

Before tagging, confirm everything packages cleanly:

```sh
cargo package --workspace --locked
```
