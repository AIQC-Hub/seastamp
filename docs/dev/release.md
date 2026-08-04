# Git workflow, CI, and releases

## Git workflow

Match ctddump: `git flow` with permanent `main` (stable) and `develop`
(integration) branches; day-to-day work on `develop`. **Commit and push only when
the user asks.** Commit messages end with
`Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

Branch prefixes are the git-flow defaults (`feature/`, `bugfix/`, `release/`,
`hotfix/`, `support/`) with `v` as the version tag prefix, so a feature branch is
`feature/coast` and the 0.10.0 release tag is `v0.10.0`.

These settings live in `.git/config`, which git never tracks, so they do not
survive a fresh clone and cannot be committed. Re-create them in a new clone with
the non-interactive init (`-d` takes the defaults, which already match):

```bash
git flow init -d
```

Beware that `.git/config` also records `gitflow.path.hooks` as an absolute path.
Renaming or moving the working directory leaves it pointing at the old location,
and it has to be repointed by hand:

```bash
git config --local gitflow.path.hooks "$(git rev-parse --absolute-git-dir)/hooks"
```

## Releasing

A release goes out as its own branch, which is what produces the paired
`Merge branch 'release/X'` and `Merge tag 'vX' into develop` commits in the log:

```bash
git flow release start 0.10.0
# stamp the CHANGELOG section, bump the version in Cargo.toml and Cargo.lock
git flow release finish 0.10.0    # merges to main, tags vX, merges back to develop
git push origin main develop --tags
```

Pushing the tag is what triggers `publish.yml`, so push it only when the release
is meant to go public.

## CI and releases

Two GitHub workflows, both modeled on ctddump.

`ci.yml` builds and runs `cargo test` on push and PR to `main` (installs
libhdf5-dev / libnetcdf-dev for `depth`, strips debuginfo so the statically
linked Polars test binaries fit the runner disk).

`publish.yml` fires on a `v*` tag: it re-runs the tests, then in parallel

- publishes to crates.io via Trusted Publishing (OIDC, no stored token; the tag
  must match `Cargo.toml`), and
- builds prebuilt binaries for Linux and macOS (x86_64 and arm64) with
  `--features static-netcdf` (`netcdf/static`, vendoring HDF5 / netCDF via
  cmake) and creates the GitHub release, attaching the archives and
  `SHA256SUMS`, with notes extracted from the matching `CHANGELOG.md` section.

Because the workflow creates the release, do not also create it by hand for a
tagged release. `Cargo.lock` is committed (the workflow uses `--locked`), so bump
it alongside the version.

Note that a serial-HDF5 crash cannot be caught by CI; see
[HDF5 threading](./depth-hdf5.md) for why, and what to run locally instead.
