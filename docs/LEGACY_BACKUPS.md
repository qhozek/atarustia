# Импорт резервных копий

```sh
cargo run --locked -- --data-root ./new-data --import-backup old.backup --import-owner 123456789
```

Поддерживаются JSON БД/URL модулей и ZIP с db.json, mods.zip, db_mods.json,
ClassName_<owner>.py и pip-backup-*.txt. Альтернатива — .importbackup в ответ на файл в Избранном.

Результат лежит в legacy_imports/<owner>/<sha256>: report.json, converted.backup.json при наличии БД,
Python source как .py.txt и requirements как .review.txt. Ничего не выполняется и не применяется само.
Проверенную converted-копию можно отдельно восстановить через .restoredb; эта команда заменяет БД.
Старые security grants, произвольные module data и secrets не активируются.

Лимиты: вход 16 MiB, суммарная распаковка 32 MiB, 256 записей, один mods.zip уровень.
Пути/links/devices/дубликаты/case collisions и чужие account suffix отклоняются. Каталог не перезаписывается.
Session import — отдельный режим, SESSION_MIGRATION.md. Review-каталог может содержать секреты в исходниках.
