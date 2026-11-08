# Contributing

Contributions are always welcome, no matter how large or small!

We want this community to be friendly and respectful to each other. Please follow it in all your interactions with the project. Before contributing, please read the [code of conduct](./CODE_OF_CONDUCT.md).

## Development workflow

`yourai` is a Rust CLI (`yourai-cli`). The binary is built from this repository; the website and docs live in a separate server repo.

To get started:

```sh
cargo build
cargo test
```

Useful checks (also run in CI):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Security / supply-chain checks (workflow `.github/workflows/security.yml`):

```sh
cargo audit
cargo deny check
```

See [docs/SECURITY_AUDIT.md](docs/SECURITY_AUDIT.md) and [SECURITY.md](SECURITY.md).

Optional script lint (when you change installers):

```sh
shellcheck scripts/*.sh src/templates/*.sh
```

The model and engine list is `catalog/catalog.json` (bundled into the binary and published as `catalog.v2.json` on the website). Keep SHA-256s and sizes accurate when you add or change catalog entries.

### Commit message convention

We follow the [conventional commits specification](https://www.conventionalcommits.org/en) for our commit messages:

- `fix`: bug fixes, e.g. fix crash due to deprecated method.
- `feat`: new features, e.g. add a new CLI command.
- `refactor`: code refactor, e.g. simplify gateway routing.
- `docs`: documentation changes, e.g. update the README.
- `test`: adding or updating tests.
- `chore`: tooling changes, e.g. change CI config.

Release commits may use the short form `yourai vX.Y.Z`.

### Sending a pull request

> **Working on your first pull request?** You can learn how from this *free* series: [How to Contribute to an Open Source Project on GitHub](https://app.egghead.io/playlists/how-to-contribute-to-an-open-source-project-on-github).

When you're sending a pull request:

- Prefer small pull requests focused on one change.
- Verify that `cargo fmt`, `clippy`, and `cargo test` are passing.
- Review the documentation to make sure it looks good.
- For pull requests that change the public CLI API or catalog schema, discuss with maintainers first by opening an issue.

## Support the project

If yourai helps you, you can support continued development here:

[Buy Me a Coffee](https://buymeacoffee.com/bdtren)

## License

By contributing, you agree that your contributions will be licensed under the
[Apache License 2.0](./LICENSE).
