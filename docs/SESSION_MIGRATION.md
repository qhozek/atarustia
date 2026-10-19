# Импорт сессий

Останови старый клиент и используй независимые каталоги:

```sh
cargo run --locked -- --data-root ./new-data --import-python /path/to/old-data
```

Читаются config.json (API ID/hash), encrypted binary payload v1 с исходным master key и SQLite v6–v8.
Старые имена файлов принимаются только импортёром. В назначении создаются atarustia-user-python-<owner>.atsession,
новый master key и настройки. Auth key/DC, peer hashes и update states переносятся; temporary keys,
takeout и sent-file cache нет. Security delegations и Python source не включаются сами.

SQLite читается через read-only in-memory snapshot. WAL/SHM/journal и неизвестные схемы отклоняются:
закрой исходный клиент, сделай checkpoint штатными SQLite средствами. Не удаляй sidecar вручную.
Лимиты: 32 MiB/session, 200000 peers/states, 64 accounts, 5 s SQL budget. Source не изменяется.

Перед записью проверяются все источники. При ошибке диска во время записи возможен частичный каталог:
повтори в новом пустом назначении после устранения ошибки. Не запускай Rust прямо в старом data directory.
Synthetic binary/SQLite fixtures проверены; перенос живого аккаунта тестируется отдельно.
