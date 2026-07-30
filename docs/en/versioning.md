# Versioning and release process

## Version format

```text
A-MAJOR.MINOR.PATCH
```

Cargo representation: `MAJOR.MINOR.PATCH-alpha`

Examples:

| Public tag | Cargo.toml |
|------------|------------|
| `A-1.0.0` | `1.0.0-alpha` |
| `A-2.0.0` | `2.0.0-alpha` |

## Environments

Kroa uses two GitHub Environments:

| Environment | Branch / trigger | Publishes commercial releases? |
|-------------|------------------|--------------------------------|
| `development` | `develop` pushes and PRs | No — temporary CI artifacts only |
| `production` | tags matching `A-*` | Yes — GitHub Releases only |

```text
feature/* / fix/*
        |
        v
   PR + CI checks
        |
        v
     develop  ---- development artifacts (internal)
        |
   release PR
        |
        v
       main
        |
   annotated tag A-X.Y.Z
        |
        v
 production GitHub Release
```

## Branch roles

- `main`: production line. Only verified release commits.
- `develop`: integration line for the next Alpha.
- `feature/*`, `fix/*`: short-lived work branches. Merge into `develop` by PR.

## Release checklist

1. Update `Cargo.toml` / `Cargo.lock` version.
2. Update `PROJECT_STATUS.md`, `README.md`, `CHANGELOG.md`, and bilingual docs.
3. Open a PR from `develop` to `main`.
4. After merge, create an annotated tag: `git tag -a A-X.Y.Z -m "..."`.
5. Push the tag. The production workflow builds, checksums, and publishes the Release.

## What production publishes

- Compiler binaries for supported platforms
- SHA-256 checksums
- Release notes generated from `CHANGELOG.md` and the tag message

Development builds must never be marketed as production releases.
