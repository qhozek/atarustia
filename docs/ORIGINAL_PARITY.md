# Цель: оригинальная Ataraxia на Rust

Эталон интерфейсов и поведения — xenside/ataraxia, ревизия
`122c2692391b1bfc40456270b7ff998c2ee10af8` (current main checked 2026-11-07).

## P0.1 — reference manifest и drift guard

Добавлены `contracts/upstream-manifest.json`, карточки поведения всех 66 команд и явная policy
с отдельно зафиксированными решениями P0.2. Static extractor читает Git blobs/AST/langpacks без исполнения
оригинала. CI сверяет pinned manifest и ежедневно проверяет drift remote main; Rust-тесты
проверяют каталог/aliases/resources локально. [Инструкция и границы](UPSTREAM_CONTRACT.md).
Manifest фиксирует требования, включая ещё не перенесённые API: это не completion certificate.

## P0.2 — архитектура расширений и данных

Выбраны Rust-ядро/встроенные команды и CPython compatibility bridge для Python `.e`/внешних
модулей. Для сессий и backup требуется двусторонний versioned import/export без silent loss,
не совместная работа с одной live session. [ADR-0001](decisions/0001-python-and-data-compatibility.md).
Решение принято, но bridge/codecs ещё не реализованы: `.e` пока Rhai, Python modules не запускаются,
данные имеют лишь прежний частичный importer. Заявления о производительности требуют benchmark.

## Переход на текущую main — этап 1

### P0.1 плана v2 — event admission

Добавлены bounded account/inline queues и reserved form-delivery lane вместо silent
try-acquire drops; periodic work coalesces. MTProto sender буферизует ограниченный backlog,
не блокируя RPC. Saturation сверх inline/transport limits — явная ошибка, не unlimited delivery.
Shutdown учитывает отменённые jobs, но не даёт durable/exactly-once replay.
[Пределы, семантика и проверки](EVENT_QUEUES.md). Live load/recovery не проверены.

- Каталог: 66 команд и 13 core-модулей, `addacc` принадлежит `ataraxiaAccounts`.
  Удалены из публичного реестра `weburl`, `suspend`, `api_fw_protection`, `suspend_api_protect`.
- RU/EN YAML и terminal assets синхронизированы побайтово с указанной ревизией; hashes закреплены тестом.
  Старые web-строки хранятся отдельно в `locales/legacy/`, чтобы не затенять текущие переводы.
- По умолчанию используется terminal/custom API login, QR первым. Старый web только через `--web`.
- Ping измеряет MTProto Ping RTT, а не задержку edit/reply. Ошибка измерения отображается `?`.
- Aliases нормализуются по регистру, старые mixed-case записи читаются без удаления данных.
- Updater полностью собирает и линкует alternate executable до shutdown; plain restart использует текущий exe.

**Не завершено:** live-приёмка inline `.addacc`, widget API, все security/loader/backup/eval
контракты, единая external translation chain, остальные конфиги/validators, live load/recovery и UI-приёмка.
Ни этот этап, ни успешные локальные тесты не означают 1:1. Дальнейшие разделы описывают уже перенесённые
части старого снимка; не считать сохранённые legacy web resources частью текущего оригинала.

## Accounts — этап 2

- `.addacc` принимает ID/username/reply, отклоняет ботов, подтверждает чужого target исходными
  строками и кнопками; `force_insecure` сохраняет явный обход предупреждения.
- Ввод phone/code/2FA через switch-inline query/chosen result работает без `--web`.
  Только выбранный пользователь может отправлять credentials; владельцы подтверждают приглашение.
  Это более узкая политика, чем оригинальный общий owner allow-list для input.
- Коды принимают пробелы/дефисы, длина проверяется по Telegram; invalid code/password остаются
  на своём шаге. Expired code предлагает повтор, FloodWait/timeout заканчивают клиент попытки.
- У каждой попытки отдельный клиент с in-memory session. На диск попадает только успешная
  encrypted session; существующая session не перезаписывается. Старые account slots не меняются.
- Попытки привязаны к случайным token, target и форме, заменяются для того же target и истекают
  через 10 минут; повтор выбранного token не повторяет RPC. Shutdown закрывает попытки.
- Credential queries перехватываются до native/Rhai watchers; ошибки не печатают значения запроса.
- После успешного входа слот добавляется атомарно в конфигурацию, показывается успех и запрашивается restart.

**Оставшиеся отличия/проверки:** live BotFather feedback/chosen-result, номер другого пользователя,
2FA/FloodWait/таймаут и реальный restart ещё не проверены в Telegram. После expired code текущая
кнопка Retry возвращает ввод телефона, а не автоматически повторяет SendCode. После TTL старый
input отвергается без восстановления формы (нужен новый `.addacc`). Граница подтверждения нового
target и screenshots требуют живой приёмки. Не считать этот этап доказательством полного 1:1.

Новые дизайны, новые публичные команды, переименования существующих команд и замена сценариев
собственными вариантами не являются целью этого порта. Название репозитория/бинарника atarustia
не означает разрешение менять интерфейс Ataraxia.

## Rich messages — общий рендерер

Help, конфигуратор и rich-builder используют общий normalizer и структурный HTML fallback
из исходного rich screen: details/quotes/tables/lists/pre, переносы и извлечение кнопок.
Действия/стили привязаны к идентичности кнопки, не подписи; права и ротация callback сохраняются.
Длинный fallback делится по UTF-16 тексту с пересечением entities, клавиатура ограничена 100
кнопками с резервом для нижних controls. Сетевые ошибки/FloodWait не запускают plain retry.
18 reference vectors и регрессии проверяются локально; это не live visual acceptance.
Raw callbacks без объявленного действия отключены; атрибуты HTML канонизируются токенизатором.
[Реализация, отличие retry policy и границы](RICH_MESSAGES.md).

## Зафиксировано

- `web-resources/index.html`, `static/app.js`, `static/app.css`, `static/metal.js`,
  `static/wordmark.png` скопированы без изменений. SHA-256 закреплены контрактным тестом.
- Rust web API адаптируется под оригинальный фронтенд, а не наоборот.
- Каталог публичных команд: 66 имён текущего оригинала. `.ataraxia` и `.ch_ataraxia_bot`
  не переименовываются в atarustia; вспомогательные команды порта не публикуются.
- Langpacks оригинала сохранены в `locales/original/`; новые переносимые экраны используют
  исходные тексты, а не их пересказ.

## Пока НЕ полная копия

### Telegram-оформление и названия

- Перенесены 13 текущих групп встроенных модулей и распределение 66 команд:
  `.ataraxia` в Settings, `.addacc` в Accounts, bot-команды в InlineStuff. Quickstart
  виден как модуль без команд. Не добавляется выдуманная версия Cargo в карточки модулей.
- `.help`, `.info` и экраны updater используют исходные langpack-строки; логотип без
  Premium — `ataraxia`. Ссылки на коммиты/обновления Rust остаются ссылками Rust-репозитория,
  версия транспорта — grammers, а не ложное заявление об использовании ataraxia-tl.
- `.ping` использует исходные emoji/шаблон Ping/Uptime и измеренный отклик; параметры
  его карточки доступны в Tester. Встроенный guest/inline `Pong 🦀` удалён; каталог
  inline-команд строится из доступных handlers. Формат расширений остаётся JSON/Rhai.
- Новые боты создаются с именем `⛈️ ataraxia`, исходным placeholder и аватаром;
  username генерируется по исходной схеме Word_XXXXXX_bot. Существующие пользовательские
  имена ботов не перезаписываются автоматически.
- Quickstart отправляется ботом с исходным guide, языковыми/сообщественными кнопками;
  `/start` отвечает исходным баннером и ссылками. Новый служебный форум — `ataraxia-userbot`,
  с исходным описанием, аватаром и topic emoji. Сохранённый account-owned форум со старым
  стандартным именем переименовывается без удаления тем; чтение старого topic-cache сохранено.
- PNG `ataraxia.png`, `bot_pfp.png`, `ataraxia_backup.png` скопированы побайтово и проверяются
  SHA-256. Старый неиспользуемый `assets/atarustia.png` не удалён.

Это не завершённая визуальная parity: live оригинальный help fallback, custom placeholders
модулей, весь набор текстов security/loader/backup/executor, логи, предложение backup period,
восстановление/настройка существующего бота, forum-folder/admin/General и CLI-баннеры ещё
требуют переноса. Не маскируются Rhai-eval, иной backup-format и прочие технические отличия.
Доставка/смена языка приветствия, аватары, миграция форума, Premium и картинки проверены
локально на структурах/исходниках, но не в живом Telegram.

### Конфигуратор и Settings

- `.settings` отделена от `.config`: исходные строки, порядок и стили кнопок NoNick,
  Grep, InlineLogs, предложения подписки, подтверждений update/restart. Grep и NoNick
  используют сохранённые настройки диспетчера.
- `.config [модуль] [параметр]` / `.cfg`: rich-каталог, папки, экран модуля и параметра,
  Boolean/Choice/MultiChoice, шаги Integer/Float, Series (первые 30 элементов, добавление,
  удаление), Hidden, copy, defaults и подтверждение сброса модуля. Возврат из помощи
  сохраняет карточку и страницу. Строки берутся из `locales/original/`.
- Ввод привязан к форме через switch-inline-query/chosen result; длинные значения
  принимаются ответом на экран. Проверка владельца выполняется и при вводе, и при нажатии.
  Есть plain fallback с сохранением действий и группировки кнопок.
- `.fconfig` / `.fcfg`: plain-значения, reply и последовательные изменения через `&&`.
  Уже сохранённые параметры не откатываются, если следующее значение некорректно.

Остаточные отличия этого блока: доступны конфиги перенесённых `ataraxiaInfo`, `Updater`,
`Settings`, параметры ping в `Tester` и JSON/Rhai-пакетов. Не добавлены неработающие параметры
Executor/Loader/APILimiter и остальные параметры Tester. Runtime Python-библиотек и их конфигов нет; раздел
Libraries поддержан рендерером, но текущий SDK не предоставляет такие объекты.
Python `ast.literal_eval` заменён безопасным ограниченным разбором JSON, True/False/None,
одинарных строк и списков/кортежей (без исполнения; не полная грамматика Python).
Описания/папки пакета задаются `config_docs` / `config_folders`; порядок его ключей пока
определяется JSON-map. Внешние langpack overrides, все тексты validator errors и предельные
размеры форм ещё требуют сравнения. InlineLogs управляет показом подробностей ошибки
команды; полная исходная диагностика RPC/FloodWait/traceback и предложение подписки
загрузчика ещё не перенесены.
Живые input/chosen-result/reply, удаление экрана, группы и Premium emoji не проверены.

Этот документ не подтверждает полную функциональную или визуальную идентичность. Ещё предстоит
завершить перечисленные конфигурационные отличия, перенести security-экраны, проверить inline .addacc, полную семантику
аргументов команд, оригинальное поведение eval/loader, оставшиеся тексты и обработку Telegram
ошибок. Rhai-пакеты не эквивалентны старым Python-модулям; совместимость экосистемы не заявляется.
Веб добавления/входа требует живой проверки. Сборка и unit tests не заменяют её.

Отклонения от оригинала перечисляются явно и не выдаются за завершённый перенос.
