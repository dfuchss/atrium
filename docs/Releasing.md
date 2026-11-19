# Releasing

Releases are built by `.github/workflows/release.yml` when a tag `v<version>` is pushed; the tag must match the version in the workspace `Cargo.toml`, the only place the version is set (`tauri.conf.json` inherits it and must not set its own). The workflow builds the macOS zips (arm64, x86_64), the Linux AppImages and plain binaries (x86_64, aarch64) and the Windows executable (x86_64), writes `SHA256SUMS`, which the in-app updater requires, and publishes them as the latest GitHub release.

## Changelog

Notes for the user-visible changes go under `## [Unreleased]` in [`CHANGELOG.md`](https://github.com/<<REPO>>/blob/main/CHANGELOG.md) as they happen. Each version has this structure; leave out what is empty:

```markdown
## 1.2.0

Two to four lines of highlights: what this version is about, for someone who reads nothing else.

> **Upgrade notes:** what you have to do or know when you update, e.g. a renamed setting.

### Added

#### All platforms

- Short bullet, without the platform name

#### macOS

#### Linux

#### Windows

### Changed

### Fixed

### Security
```

- The platform groups come in this order: *All platforms*, *macOS*, *Linux*, *Windows*. A bullet under a platform group does not repeat the platform ("On Windows, …").
- Breaking changes go into the *Upgrade notes* callout only, not again as a bullet.
- *Security* is used like the other sections, when a version has such changes.
- 0.1.0 and 0.2.0 were macOS only and have no platform groups.

## Release steps

1. Rename the Unreleased content to a `## X.Y.Z` section (write its highlights) and leave a fresh empty `## [Unreleased]` above it.
2. Set the version in the workspace `Cargo.toml`.
3. Commit and tag `vX.Y.Z`.

## Release notes

`.github/scripts/release-notes.sh <tag>` builds the release notes: the tag's `CHANGELOG.md` section, then `.github/release-install.md` (the download overview with links to the tag's files and the install steps per platform; `{{version}}` and `{{tag}}` are replaced). The release workflow publishes them and fails if the section is missing or empty. To preview them:

```sh
.github/scripts/release-notes.sh v0.3.0
```

## Wiki

The pages of this wiki live in `docs/` of the repository; `.github/workflows/docs.yml` mirrors them into the wiki on every push to `main` that changes `docs/`. Edit them there, not in the wiki: the next run replaces the wiki's content. Links to the repository use a placeholder for its name, which the workflow fills in.
