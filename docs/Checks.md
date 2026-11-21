# Checks

```sh
scripts/check.sh                               # everything CI checks: fmt, UI build, clippy, tests
cargo test --workspace --exclude atrium-ui     # core, adapters, store
```

`scripts/check.sh` must pass before every commit; the GitHub workflow `.github/workflows/check.yml` runs it too, on macOS, Linux and Windows. The lint rules are in the `[workspace.lints]` section of `Cargo.toml` (clippy pedantic plus a few denials, each allowed exception with its reason), the thresholds for function length, complexity and arguments in `clippy.toml`.

On Linux, `scripts/linux-check.sh` runs the checks in a container and can check the AppImage; see [Building](Building#containers-and-the-linux-checks).

## Debug builds from CI

Every push to `main` and every pull request also gets unsigned debug builds among the run's artifacts, kept for two weeks: macOS zips for arm64 and x86_64, Linux AppImages and plain binaries (stripped) for x86_64 and aarch64, and a Windows x86_64 executable. A debug build never updates itself, and the macOS one shares its data with an installed Atrium.
