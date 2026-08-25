# Contributing

Contributions are welcome. Keep changes focused and include tests for configuration parsing or SSH
argument generation when those areas change.

Before opening a pull request, run:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Do not commit real server addresses, user names, private-key paths, credentials, or logs containing
sensitive infrastructure details. Use the documentation example or reserved test addresses.
