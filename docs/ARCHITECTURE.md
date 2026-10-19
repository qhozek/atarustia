# Структура проекта

- `runtime` — запуск аккаунтов, фоновые задачи, завершение и перезапуск.
- `setup`, `auth`, `web` — выбор API-пары, номер/код/2FA/QR и локальный веб-мастер.
- `session`, `storage`, `database` — AES-GCM сессии, атомарная запись и блокировка каталога.
- `dispatcher`, `security`, `access` — разбор команд, владельцы, группы и Telegram admin rights.
- `bot`, `native_bot` — публичный фасад с ApiResult и отдельный grammers API.
- `bot_requests`, `bot_response`, `botapi` — параметры запросов, correlation ответов и объекты.
- `forms`, `rich`, `markup` — меню, кнопки, callbacks и fallback.
- `media`, `file_id`, `assets` — upload/download, file IDs, альбомы и кеш.
- `modules`, `sdk`, `scheduler` — пакеты Rhai, Rust-модули, watchers и loops.
- `migration`, `sqlite_migration`, `legacy_backup` — офлайн-импорт старых данных.
- `updater` — обновление исходников из origin/main.
- `vendor/grammers-mtsender` — локальная поддержка MTProxy.

Состояние аккаунтов находится в data-root, не в Git. Клиенты получают зависимости явно;
общих глобальных аккаунтов нет. Локальные проверки описаны в VERIFICATION.md.
