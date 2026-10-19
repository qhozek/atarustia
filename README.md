# atarustia

Telegram-юзербот на Rust. Работает на обычном аккаунте, использует отдельного inline-бота для меню,
хранит сессии зашифрованными. Запускается из исходников через Cargo.

## Установка

Нужны Git, Rust 1.89+ и компилятор C. На Debian/Ubuntu:

```sh
sudo apt update
sudo apt install git curl build-essential pkg-config ca-certificates
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup.sh
sh rustup.sh
. "$HOME/.cargo/env"
git clone https://github.com/qhozek/atarustia.git
cd atarustia
cargo run --locked
```

Если Rust и Git уже установлены, можно воспользоваться скриптом:

```sh
curl -fsSL https://raw.githubusercontent.com/qhozek/atarustia/main/install.sh -o install.sh
sh install.sh
```

В Windows установи [Git](https://git-scm.com/download/win) и [Rust](https://rustup.rs), затем открой новый PowerShell:

```powershell
git clone https://github.com/qhozek/atarustia.git
cd atarustia
cargo run --locked
```

`install.ps1 -Destination C:\path\atarustia` делает то же самое. Скрипты не перезаписывают существующий каталог.
Первый запуск скачает зависимости и скомпилирует приложение; последующие запуски используют Cargo-кеш.

## Первый вход

1. Открой одноразовую ссылку из терминала.
2. Выбери Telegram Desktop или свою API-пару. Свои API ID и hash берутся на <https://my.telegram.org/apps>.
3. Введи номер в международном формате и код Telegram либо отсканируй QR в «Настройки → Устройства».
4. Если включена двухэтапная аутентификация, введи облачный пароль.
5. После входа отправь `.help` в Избранное.

После авторизации приложение настроит inline-бота через BotFather и создаст служебный форум для файлов,
резервных копий и логов. `--no-bot` отключает настройку бота.

Без браузера:

```sh
cargo run --locked -- --no-web
```

В терминале сначала выбирается приложение Telegram, затем способ входа: QR или номер → код → 2FA.
Свои ID и hash можно вставить вместе как `12345:hash` или ввести по очереди.
`--api-preset tdesktop` пропускает выбор приложения, `--api-preset custom` сразу запрашивает собственную пару.

На сервере пробрось локальный порт:

```sh
ssh -L 8080:127.0.0.1:8080 user@server
```

Веб-вход слушает только loopback. Не публикуй код и 2FA через открытый HTTP.
Новая ссылка из работающего аккаунта — `.weburl` в Избранном.

### API-пара через окружение

```sh
export ATARUSTIA_API_ID=12345
export ATARUSTIA_API_HASH=0123456789abcdef0123456789abcdef
cargo run --locked
```

PowerShell: `$env:ATARUSTIA_API_ID='12345'`, `$env:ATARUSTIA_API_HASH='...'`.
Пара сохраняется в `data/config.json`, повторно вводить её не нужно.

## Настройки запуска

| Параметр | Назначение |
|---|---|
| `--no-web` | Терминальный вход |
| `--no-auth` | Не запрашивать вход; только сохранённые аккаунты |
| `--account second` | Новый именованный слот; сохранённые аккаунты запускаются вместе |
| `--data-root DIR` | Каталог данных, по умолчанию `data` |
| `--port 8080` | Порт веб-входа |
| `--phone +123456789` | Номер для терминального входа |
| `--proxy URL` | SOCKS5 или MTProxy; также `ATARUSTIA_PROXY` |
| `--no-bot` | Не создавать inline-бота |
| `--allow-shell` | Разрешить владельцу `.terminal`, C/C++ и JS |

Примеры прокси и форматы secret — [docs/MTPROXY.md](docs/MTPROXY.md).
Код/2FA/API hash и токены не добавляй в Git. GitHub-токен для работы бота и обновлений не нужен.

## Команды

- `.help`, `.info`, `.atarustia`, `.ping`, `.source`
- `.settings`, `.config`, `.fconfig`, `.setprefix`, `.setlang`, `.aliases`
- `.modules`, `.dlm`, `.lm`, `.review`, `.approve`, `.ulm`, `.ml`
- `.security`, `.ownerlist`, `.owneradd`, `.ownerrm`, `.tsec`, `.sgroups`
- `.watchers`, `.loops`, `.nonickcmd`, `.nonickuser`, `.nonickchat`
- `.e` — Rhai; `.terminal`, `.ec`, `.ecpp`, `.enode` требуют `--allow-shell`
- `.backupall`, `.backupdb`, `.backupmods`, `.restoredb`, `.importbackup`
- `.setup`, `.logs`, `.clearlogs`, `.tglogs`, `.restart`, `.update`, `.addacc`, `.weburl`

Полная справка доступна в Telegram. Загруженный пакет не выполняется до `.approve <sha256>`.
Формат модулей и Rust SDK — [docs/MODULES.md](docs/MODULES.md). Python-файлы не исполняются.

`.help` открывает каталог встроенных и внешних модулей. Внешние идут по 15 на страницу,
встроенные остаются на каждой странице. Нажатие на модуль открывает команды, copy-кнопки,
inline/guest handlers и настройки. `.help <команда или алиас>` находит модуль команды.
`-f` показывает скрытые, `-c` — встроенные, `-l` — внешние. Скрытие через `.helphide <модуль>`
или кнопку карточки не отключает команды. Подробнее — [docs/HELP.md](docs/HELP.md).

### Карточка `.info`

`.info` и `.atarustia` показывают владельца, версию, ветку/коммит, обновления, префикс, аптайм,
CPU/RAM, ядро и ОС. Баннер и оформление настраиваются в `.config atarustiaInfo`.

```text
.fconfig atarustiaInfo rich_mode true
.fconfig atarustiaInfo banner_url "https://example.com/banner.png"
.fconfig atarustiaInfo custom_message "<b>{me}</b> · {uptime} · {ram_usage}"
```

Пустой `banner_url` выключает картинку; `custom_message: null` возвращает стандартный текст.
`quote_media` переключает preview/фото, `invert_media` ставит картинку над текстом.
`help_banner_url` задаёт баннер rich-экрана справки. Rich-режим и custom emoji зависят от Premium;
при явном отказе Telegram карточка переходит на обычное оформление. Подробнее — [docs/INFO.md](docs/INFO.md).

## Обновление

`.update` показывает карточку: ветка, старый/новый коммит, количество коммитов и файлов,
изменённые строки, дата и свёрнутый changelog. Кнопки: **Обновить / Изменения / Отмена**.
После подтверждения идут этапы **Скачивание → Зависимости → Перезапуск** с длительностью каждого.
Cargo проверяет новые исходники и зависимости до перезапуска; при ошибке доступна кнопка «Ещё раз».
Итог редактируется в том же сообщении после запуска аккаунта и модулей.

`.update -f` пропускает подтверждение; `.update <полный hash>` применяет конкретную просмотренную ревизию.
Без inline-бота обычная `.update` выполняет обновление сразу. Ошибка работающего inline-интерфейса
не считается подтверждением: в таком случае показывается команда с hash.
Локальные изменения и расходящаяся история не сбрасываются. Updater использует только fast-forward.
Официальный remote: `https://github.com/qhozek/atarustia.git`; цель — `origin/<текущая ветка>` (`main` по умолчанию).

Уведомления в ЛС проверяются раз в 30 минут, один раз на ревизию. Отключение:
`.fconfig Updater update_notifications false`. `.restart` тоже запрашивает подтверждение;
`.restart -f` перезапускает сразу. Подробности — [docs/UPDATER.md](docs/UPDATER.md).

Из терминала при остановленном боте:

```sh
git pull --ff-only
cargo run --locked
```

## Данные

`data/config.json` содержит API-пару, аккаунты и bot tokens. `config-<id>.json` — настройки аккаунта.
`atarustia.key` — ключ сессий; `atarustia-user-<slot>.atsession` и `atarustia-bot-<slot>-<bot-id>.atsession` — сессии.
Сохраняй ключ вместе с сессиями, но не выкладывай их в общий доступ. Backup-команды не включают master key и сессии.

Импорт старых Python-сессий: [docs/SESSION_MIGRATION.md](docs/SESSION_MIGRATION.md).
Импорт ZIP/JSON-копий: [docs/LEGACY_BACKUPS.md](docs/LEGACY_BACKUPS.md).
Текущие данные используют только имена `atarustia`; старые имена читаются исключительно импортёром.

## Развёртывание

```sh
docker build -t atarustia .
docker run --rm -it --network host -v atarustia:/data atarustia
```

Host networking — для Linux; на Docker Desktop используй `--no-web`.
Docker запускает исходники через Cargo. Для `.update` собирай образ из Git-checkout вместе с `.git`;
установка из архива не содержит родительской истории и обновляется пересборкой образа.
systemd unit: [deploy/atarustia.service](deploy/atarustia.service). Укажи реальный путь Cargo и пользователя.

## Разработка

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked -p grammers-mtsender --features proxy --lib mtproxy::tests
```

Bot API facade: [docs/BOT_FACADE.md](docs/BOT_FACADE.md), объекты: [docs/BOT_OBJECTS.md](docs/BOT_OBJECTS.md).
Технические ограничения — [docs/PARITY.md](docs/PARITY.md), проверки — [docs/VERIFICATION.md](docs/VERIFICATION.md).

AGPL-3.0-only. Лицензии зависимостей и происхождение кода — [NOTICE](NOTICE).
