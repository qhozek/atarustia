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
media/file IDs, Bot requests/responses/objects, migration, `.info` layout/config/host metrics,
updater UI/local Git/Cargo failure/restart references, help catalogue/cards/security/pagination и MTProxy. Python fixtures содержат только
синтетические данные. Тесты TCP/TLS прокси работают на loopback, без внешнего Telegram.

CI повторяет проверки на Linux и Windows. Живые login/BotFather/business/media/DC сценарии в этот набор не входят.
Host API тестируется без Telegram: последовательный HTTP → Telegram через mock transport,
проверки capability grants/preview/cancellation/budget, реальные response bodies и loopback HTTP
redirect test. Это не подтверждение живых Telegram host операций или внешних HTTPS сервисов.
Lifecycle/Effect: hooks установки/старта/замены/выгрузки/shutdown/config, persistent suspension,
ошибки watchers, изоляция повреждённых пакетов, ревизии loops, validation и preview без state/control
side effects. Реальная доставка новых background screens/media и callback animations требует Telegram.
Release-бинарники не собираются и не поставляются.
Original parity: SHA-256 оригинальных frontend assets, точный каталог 70 команд, исходные строки,
API state/app/cancel, поля code view и rounded QR SVG. Первый app screen проверен в браузере
на Rust HTTP-сервере без ввода Telegram credentials. Остальной живой login flow не проверен.

Config/Settings: `tests/config_ui.rs` проверяет исходные строки и структуру меню,
переходы home/module/parameter/folder, Boolean/Choice/MultiChoice/Series/Hidden,
Integer/Float и отключённые границы, reset confirmation, rich/plain fallback,
Premium-иконки, switch-inline input tokens и ограниченный разбор литералов.
`tests/native.rs` дополнительно проверяет `.fconfig` с reply/`&&`, сохранение первых
изменений при ошибке последующих, возврат в help, запись/сброс конфигурации,
привязку reply к чату/сообщению и ревизию config metadata.
Проверки без Telegram. Доставка chosen-inline-result, ввод и редактирование одной
живой формы, удаление, групповые/Premium особенности ещё не подтверждены.

Presentation: `tests/presentation.rs` закрепляет SHA-256 трёх исходных PNG, имена бота/форума,
схему username, структуру и стили quickstart/start, исходный шаблон ping, безопасную подстановку
значений и строки help/updater. `tests/native.rs` закрепляет 14 имён встроенных модулей,
распределение всех 70 команд, отсутствие выдуманной версии, полный список команд Settings
и чтение старых hidden-module имён. Это не живая проверка Telegram RPC или screenshot parity.
