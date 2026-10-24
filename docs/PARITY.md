# Ограничения

- Внешние Python-модули не выполняются. Доступны Rust SDK и пакеты JSON/Rhai.
- Rhai поддерживает capability-checked HTTP/Telegram host calls с async I/O; произвольный raw RPC,
  локальные файлы и большие media transfers доступны только доверенному native SDK.
  Lifecycle hooks (install/ready/unload/config_change), suspension и общий Effect pipeline реализованы;
  inline/guest не поддерживают album/frames как один результат и явно отклоняют их.
- TL-схема закреплена зависимостями Cargo; динамического обновления слоя нет.
- API-защита ограничивает команды, но не перехватывает каждый raw RPC.
- Часть ответов ещё не переведена на оба языка. Config/Settings используют исходные строки и
  основные экраны/переходы; оставшиеся отличия перечислены в `ORIGINAL_PARITY.md`.
- Bot facade использует Rust-типы вместо Python kwargs/default coercion. Некоторые inline results требуют title
  и предварительного upload bytes. Guest hydration и document thumbnails неполны.
- Не все новые layer-specific payloads представлены объектами. Passport credentials намеренно не копируются в JSON.
- Импорт SQLite поддерживает v6–v8; старые/нестандартные схемы и WAL без checkpoint отклоняются. Legacy file ID v2 нет.
- `.e` выполняет Rhai, `.addacc` создаёт web-login slot. Legacy SUDO/SUPPORT не поддерживаются.

MTProxy classic/dd/ee, rich messages и media/DC операции имеют локальные тесты. Авторизация на реальном аккаунте,
BotFather, длительные media transfers, группы/Premium emoji и несколько живых аккаунтов требуют отдельной проверки.

Разделы реализации: `BOT_FACADE.md`, `BOT_OBJECTS.md`, `MTPROXY.md`, `MODULES.md`, `SESSION_MIGRATION.md`.
