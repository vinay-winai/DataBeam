# Contributing

## Branching

- `main`: stable branch for releases.
- `develop`: integration branch for ongoing work.
- Feature/fix branches: create from `develop` using:
  - `feat/<short-name>`
  - `fix/<short-name>`
  - `chore/<short-name>`

## Pull Requests

- Open PRs into `develop`.
- Keep PRs focused and small.
- Ensure CI is green before requesting review.
- Include a short test note in the PR description.

## Versioning and Releases

1. Merge release-ready changes into `main`.
2. Bump version in `Cargo.toml`.
3. Create and push a tag:
   - `git tag v0.1.6`
   - `git push origin v0.1.6`
4. GitHub Actions `Release` workflow builds platform artifacts and publishes them to GitHub Releases.

## Local Checks

Run before opening PRs:

```bash
cargo check
cargo test
```
