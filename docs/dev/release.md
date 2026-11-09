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
# stamp the CHANGELOG section, bump the version in Cargo.toml, Cargo.lock,
# CITATION.cff and the README's VERSION= download line
git flow release finish 0.10.0    # merges to main, tags vX, merges back to develop
git push origin main develop --tags
```

The version lives in four places, and all four move together:

- `Cargo.toml` and `Cargo.lock` (the workflow builds with `--locked`);
- `CITATION.cff`: `version: "X.Y.Z"` and `date-released` set to the release
  date, which Zenodo copies into the archived record;
- the `VERSION=vX.Y.Z` line of the README's prebuilt-binary snippet.

Grepping for the previous version before finishing catches a missed one, for
example `grep -n '0\.18\.1' README.md CITATION.cff Cargo.toml`. The CHANGELOG's `## [Unreleased]`
section becomes `## [X.Y.Z] - YYYY-MM-DD`.

Pushing the tag is what triggers `publish.yml`, so push it only when the release
is meant to go public. Check <https://www.githubstatus.com/> first: during an
Actions outage the run fails at once with a "workflow file issue" and no jobs.
That is not a fault in the workflow, and once Actions recovers
`gh run rerun <id>` repeats it on the same tag (no new tag is needed).

### Zenodo

The repository is connected to [Zenodo](https://zenodo.org/) through its GitHub
integration, so every published GitHub Release (created by the tag's publish
run) is archived and gets its own version DOI, plus one concept DOI that always
resolves to the latest version. Use the concept DOI wherever a DOI is shown (the
README badge, `doi:` in `CITATION.cff`), so it never goes stale. Zenodo takes
the title, authors, licence and version from `CITATION.cff`, which is why every
release bumps it; without the file it falls back to "AIQC-Hub/seastamp: vX.Y.Z"
with the organisation as creator, and an archived record never changes
afterwards. Zenodo archives only releases published after the integration was
switched on for the repository, so confirm it is on before a release that should
be archived. After the release, the DOIs can be read from
`curl -s -A "seastamp-release-check" "https://zenodo.org/api/records?q=seastamp&sort=mostrecent&size=3"`
(fields `doi` and `conceptdoi`).

The example report site (`AIQC-Hub/seastamp-report-example`) is archived the
same way from its own GitHub Releases. Its citation is
`scripts/site-CITATION.cff` in this repository, which `summary_site.sh` copies
into every build; see [data-and-scripts.md](./data-and-scripts.md).

## CI and releases

Two GitHub workflows, both modeled on ctddump.

`ci.yml` builds and runs `cargo test` on push and PR to `main` (installs
libhdf5-dev / libnetcdf-dev for `depth`, strips debuginfo so the statically
linked Polars test binaries fit the runner disk). The tests run instrumented
by `cargo-llvm-cov` (`show-env`, so the build stays in `target/` for the cache),
and the LCOV report is uploaded to Codecov with the AIQC-Hub organisation secret
`CODECOV_TOKEN`. A failed upload does not fail the job.

`publish.yml` fires on a `v*` tag: it re-runs the tests, then in parallel

- publishes to crates.io via Trusted Publishing (OIDC, no stored token; the tag
  must match `Cargo.toml`), and
- builds prebuilt binaries for Linux and macOS (x86_64 and arm64) with
  `--features static-netcdf` (`netcdf/static`, vendoring HDF5 / netCDF via
  cmake) and creates the GitHub release, attaching the archives and
  `SHA256SUMS`, with notes extracted from the matching `CHANGELOG.md` section.

The Linux archives are built with `cargo-zigbuild` targeting glibc 2.17, not
natively, so the runner's own glibc no longer sets the floor: they run on
RHEL7/CentOS7 and every newer distribution, Rocky Linux 8 (glibc 2.28) included.
Building natively on `ubuntu-22.04` pinned the floor at glibc 2.35 and broke
Rocky 8 outright, which is what this replaced. zig compiles the vendored HDF5 /
netCDF C sources as well as linking Rust, so `static-netcdf` still works. A
"Check the glibc floor" step fails the build if a symbol above 2.17 sneaks back
in, so a run failing there is that guard, not a flake. macOS needs none of this
and builds natively per target.

Because the workflow creates the release, do not also create it by hand for a
tagged release. `Cargo.lock` is committed (the workflow uses `--locked`), so bump
it alongside the version.

Note that a serial-HDF5 crash cannot be caught by CI; see
[HDF5 threading](./depth-hdf5.md) for why, and what to run locally instead.
