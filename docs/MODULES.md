# Модули

Rust-модуль реализует sdk::Module и регистрируется через runtime::run_with_modules.
Пример: cargo run --example native_module -- --no-web. Это source interface, не ABI для dll/so.

Устанавливаемый пакет — JSON manifest + Rhai source. Примеры находятся в examples/*.module.json.
.dlm/.lm устанавливают переданный пакет; snapshot/hash остаются внутренним механизмом загрузчика.
Libraries входят в одобряемый пакет; локальные/сетевые imports не разрешаются.

Command: fn handler(args, config). Watcher/loop/callback: fn handler(event, config).
state доступен в scope; structured result содержит text/state/screen/media/album/frames. Неизвестные keys — ошибка.
Также `suspend: "reason"` приостанавливает модуль до повторной загрузки. Inline preview не сохраняет
state и не применяет suspend. Watchers отвечают в triggering chat, loops — в Избранное.

## Watcher-теги

Фильтры манифеста повторяют декларативные теги @loader.watcher оригинала. Каждый фильтр
три-стейтний: отсутствует — принимает всё, `true` — только совпадающее, `false` — только несовпадающее.

| Фильтр | Событие проходит при `true` |
|---|---|
| `outgoing` | исходящее сообщение |
| `commands` | команда юзербота |
| `private` / `groups` / `channels` | ЛС / группа и супергруппа / канал |
| `forwards` | пересланное сообщение |
| `reply` | ответ на сообщение |
| `mention` | упоминание (@username или text_mention) |
| `inline` | отправлено через inline-бота |
| `media` | с медиа |
| `photos` / `videos` / `audios` / `docs` / `stickers` | ровно этот тип медиа |
| `contains` / `startswith` / `endswith` / `regex` | текст (до 512 символов, regex компилируется при проверке пакета) |
| `sender` / `chat` | id пользователя; `chat` принимает и bare id канала, и `-100…` |

Пример: `{"function":"on_photo","event":"message","outgoing":true,"photos":true}`.
Категории чата и медиа заполняются для событий message/bot_message/business_*; для raw-событий они пусты.
Полный набор полей события — `text, chat, sender, outgoing, command, edited, id, kind, data` плюс
предвычисленные `chat_kind, forward, reply, mention, via_bot, media`.

## Loops: stop_clause и wait_before

`{"function":"ticker","interval_seconds":60,"stop_clause":"ticker_alive","wait_before":false}`.
`stop_clause` — ключ внутри state модуля: планировщик помечает его `true` при старте цикла,
а значение `false` (например, `#{state:#{ticker_alive:false}}` из команды) останавливает цикл
до обратного включения. `wait_before: true` делает паузу до первой итерации вместо немедленного запуска.

## Строки модуля и packurl

`strings` манифеста — `{language: {key: text}}`, доступно скрипту как `strings.key`
с фолбэком на `en` и учётом языка аккаунта (`.setlang`). `packurl` — HTTPS-ссылка на такой же JSON:
загружается один раз при `.dlm`/`.lm` и замораживается в пакете; сам packurl не скачивается в рантайме.

## Scope-требования

`"scope": ["ffmpeg", "inline", "shell"]` в манифесте проверяется при `.dlm`/`.lm`:
ffmpeg — наличие бинаря, inline — работающий inline-бот, shell — флаг `--allow-shell`.

## Анимация

`#{frames: ["◐", "◓", "◑"], frame_ms: 300}` — первое сообщение отправляется, остальные
применяются редактированием с паузой `frame_ms` (100–10000 мс, до 100 кадров, каждый ≤ 4096
символов). `frames` не сочетается с screen/media/album.

Screen: text/pages/buttons/html/rich_html/ttl_seconds/media/gallery/rich_buttons.
button_appearance повторяет keyboard shape, rich_button_appearance — rich button count.
Style: default/primary/success/danger/link, emoji_id положительный.
Markers {{atarustia:tap:0}} связываются с opaque callbacks. Fallback сохраняет actions/styles,
перерисовка меняет tokens. Каждое нажатие проверяет permissions заново.
Module screen может вызвать только свой handler; действия config ядра ему недоступны.

Validators: boolean/integer/float/string/choice/multi_choice/series/link/telegram_id/hidden/union/none.
Необязательные `config_docs: {parameter: description}` и `config_folders: {parameter: folder}`
показывают описания и группируют параметры в оригинальном конфигураторе. Метаданные входят
в ревизию пакета; пустые поля не меняют сериализацию старых пакетов. Для Python-модулей
это не слой совместимости с `ConfigValue` — они по-прежнему не исполняются.
Defaults и ввод проверяются одинаково. Limits: 100000 ops, depth 64, call depth 32,
128 KiB string, 4096 array items, 1024 map items, 256 KiB state, 512 forms, 50 pages, 90 buttons.
Loop interval 1..86400 s. Это ограничения ресурсов, не формально проверенная sandbox.

Bot фасад: BOT_CONTRACT.md, BOT_FACADE.md, BOT_OBJECTS.md. Native access — bot.native().
Python-модули не выполняются. ОС не предоставляется Rhai; HTTP и Telegram доступны только через
явно одобренные host capabilities, описанные ниже.

## Асинхронный host API

Устанавливаемые пакеты могут выполнять HTTP и Telegram-операции без пересборки. Rhai исполняется
один раз в blocking worker, а `host_call` передаёт операцию асинхронному обработчику Tokio и
возвращает результат скрипту. Цикл событий не блокируется; скрипт не переисполняется ради I/O.
API подключён к командам, watchers, loops, callback handlers, inline и guest handlers.

Разрешения входят в manifest и SHA-256 одобряемого пакета. По умолчанию доступ отсутствует:

```json
"capabilities": {
  "http": ["https://wttr.in"],
  "telegram": {
    "operations": ["history", "get_messages", "send_message"],
    "chats": ["current", "me", "@my_channel", "-1001234567890"],
    "bot": false
  }
}
```

`http` — точные HTTPS origins без wildcard, пути, credentials и fragment. Порт учитывается.
HTTP redirects запрещены, системный HTTP proxy не используется. Origin — разрешение на весь
хост, включая его частные адреса при соответствующем DNS: это не SSRF sandbox. Одобряй только
необходимые сервисы. Неизвестные capabilities/операции отклоняются.

```rhai
fn weather(args, config) {
    let response = http_get("https://wttr.in/Moscow?format=3");
    if !response.ok { throw "Weather unavailable"; }
    #{text: response.body}
}
```

`http_get(url)` сокращает `host_call("http.request", #{url: url})`.
HTTP request: `url`, `method` (GET по умолчанию; также POST/PUT/PATCH/DELETE/HEAD),
`headers` (map), `body` (строка), `response` (`text` по умолчанию или `base64` для бинарных файлов).
Ответ: `status`, `ok`, `body`, `encoding`.
Для JSON: `json_parse(response.body)` и `json_stringify(value)`.
Authorization/API keys передаются явно через headers, не через URL credentials.

Telegram вызовы: `host_call("telegram.<operation>", #{...})`. Общие параметры:
`chat` — signed Bot API dialog ID, `@username`, `me` или `current` (по умолчанию);
`bot: true` — выполнить от имени inline-бота, если это разрешено пакетом.
`me` означает Избранное для пользовательского клиента или ЛС владельца для inline-бота.
Username grants сравниваются без учёта регистра; numeric IDs не смешиваются с username grants.

| Операция | Параметры / результат |
|---|---|
| `get_me` | User выбранного клиента; chat не требуется |
| `get_chat` | Chat |
| `get_messages` | `ids: [1,2]`, до 50; массив Message/null |
| `get_reply` | `message_id` (по умолчанию ID команды/события); Message/null |
| `history` | `limit: 1..50` (по умолчанию 10); массив Message |
| `send_message` | `text`, `html: bool`, `reply_to: id`; возвращает Message |
| `edit_message` | `message_id`, `text`, `html`, `reply_to`; true |
| `delete_messages` | `ids`; true, принадлежность сообщений granted chat проверяется до удаления |
| `download_file` | `message_id`; `{base64, size}`, файл из сообщения granted chat |
| `send_file` | `name`, `base64`, `text`, `reply_to`; Message |
| `send_media` | `media` в формате Media с source bytes/file_id, `text`, `html`, `reply_to`; Message |

`base64_encode(text)` и `base64_text(base64)` позволяют обрабатывать небольшие текстовые файлы.
Никаких локальных путей, shell и произвольного raw RPC API скрипт не получает. Для расширенных
RPC остаётся доверенный native Rust SDK. Для больших файлов нужен native SDK: host download/file
ограничен 64 KiB; HTTP request/response body тоже до 64 KiB. Общий JSON request/response — до
128 KiB, максимум 16 host calls на invocation, 15 секунд на I/O и 60 секунд на invocation.
Остановка аккаунта отменяет ожидающий I/O и освобождает ожидающий worker. Ошибки удалённого
транспорта пишутся в редактируемый лог; скрипт получает обобщённую ошибку и может использовать
`try/catch`. Уже выполненные отправки не откатываются при последующей ошибке скрипта.

## Общий контракт Effect

Команды, watchers, loops, callbacks и lifecycle hooks проходят через `module_effects::Plan`:
сначала проверка результата и принадлежности кнопок модулю, затем сохранение state/control,
затем доставка. Отправки Telegram и HTTP не являются транзакцией с БД и не откатываются.
Выбирается один output: text, screen, media, album либо frames. Media/album используют text
как подпись (до 1024 UTF-16 units). Screen/frames имеют собственный текст. Конфликтующие outputs отклоняются.

| Контекст | Доставка |
|---|---|
| Команда | Текст через обычный answer/grep/paging; screen через inline form; media/album/frames в чат команды |
| Watcher | В чат события, ответом на triggering message; bot events используют bot client |
| Loop / hook | В Избранное; экраны по возможности отправляются ботом в ЛС владельца |
| Callback | Текст/screen/media меняют текущую форму, frames редактируют её; album отправляется отдельно в известный чат исходной команды |
| Inline / guest | Один inline result: text/screen/media; album и frames явно отклоняются, вместо них используй screen.gallery |

Фоновые формы owner-only: произвольный отправитель события не становится владельцем кнопок.
При недоступности inline UI сохраняется текст страниц и медиа, но кнопки без бота работать не могут.
Inline preview не сохраняет state и suspend; HTTP GET/read capabilities остаются доступны.
Старые формы модуля инвалидируются при смене ревизии, выгрузке и приостановке.
Callback формы inline handler повторно проверяет право на исходную inline-команду.

## Жизненный цикл устанавливаемых пакетов

```json
"lifecycle": {
  "install": "installed",
  "ready": "ready",
  "unload": "unload",
  "config_change": "changed"
}
```

Все hooks необязательные, `fn handler(event, config)`; доступны state/strings и тот же host API.
Имена валидируются без исполнения кода; staging, review и backup parsing hooks не запускают.
Hooks входят в hash одобряемой ревизии. Пустое поле lifecycle не меняет сериализацию старых пакетов.

- `install`: после `.lm`/`.dlm`, в том числе для обновления, до ready. При старте процесса не повторяется.
- `ready`: при запуске аккаунта и после установки ревизии. До завершения ready
  module handlers закрыты. Сохранённая приостановка переживает перезапуск.
- `unload`: при замене (`reason=replace`), `.ulm`/`.clearmodules` (`remove`),
  штатной остановке аккаунта (`shutdown`). `event.data.reason` содержит причину.
  Исполнение unload ограничено 2 секундами, общий shutdown hooks — 5 секундами. После убийства
  процесса или потери питания вызов не гарантируется. Ошибка unload не запрещает удаление.
- `config_change`: после успешной записи конфигурации. `event.data.old` и `event.data["new"]`
  содержат старый/новый объект. Изменения до обработки объединяются. Очередь сохраняется
  атомарно вместе с конфигурацией; live UI обрабатывает её сразу, runtime — также раз в секунду.
  Ошибка hook не откатывает конфигурацию; событие ошибки не повторяется автоматически.

Статус, причина приостановки и последняя ошибка хранятся во внутренней БД и логах.
Отдельных публичных команд управления жизненным циклом нет: каталог команд следует оригиналу.
`#{suspend:"reason"}` блокирует handlers без рекурсивного вызова unload из текущего handler.
Пауза ждёт завершения текущего handler под общей блокировкой, не прерывает уже отправленный
RPC и не удаляет установленный пакет. Если handler сам приостановил модуль, новые интерактивные
формы не регистрируются; текст и медиа ответа всё ещё могут быть доставлены.

Если install/ready падает, одобренная ревизия остаётся на диске, но suspended; автоматического
отката внешних side effects нет. Исправь конфигурацию и возобнови модуль либо установи исправленную
ревизию через `.lm`/`.dlm`. Ошибки watchers изолированы: следующие handlers и другие модули получают событие.
Повреждённый файл модуля сохраняется и пропускается, не срывая загрузку здоровых пакетов.
Постановка и выполнение loops учитывают полную ревизию пакета, включая manifest.

Пример: `examples/lifecycle.module.json`.

`current` у команды/watcher — чат события, у callback — чат исходной команды, если form имеет
gate. В loop и inline preview текущего чата нет: используй явно одобренный chat или `me`.
`invocation()` возвращает `{chat, message_id, preview}`; отсутствующие значения равны `()`.
В inline preview разрешены только HTTP GET и Telegram read-операции, записи всегда запрещены;
state по-прежнему не сохраняется. GET всё равно отправляет запрос внешнему сервису при вводе.

Примеры: `examples/weather.module.json`, `examples/telegram_host.module.json`.
