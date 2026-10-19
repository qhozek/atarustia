# Проверки

Локальная среда: Windows x86_64, Rust 1.99.1 GNU. Проверки выполняются без Telegram credentials.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked -p grammers-mtsender --features proxy --lib mtproxy::tests
cargo test --locked --doc
```

Набор покрывает dispatcher/security, encrypted sessions/storage, web access/login states, модули/формы,
media/file IDs, Bot requests/responses/objects, migration и MTProxy. Python fixtures содержат только
синтетические данные. Тесты TCP/TLS прокси работают на loopback, без внешнего Telegram.

CI повторяет проверки на Linux и Windows. Живые login/BotFather/business/media/DC сценарии в этот набор не входят.
Release-бинарники не собираются и не поставляются.
