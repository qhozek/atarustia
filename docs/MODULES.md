# Модули

Rust-модуль реализует sdk::Module и регистрируется через runtime::run_with_modules.
Пример: cargo run --example native_module -- --no-web. Это source interface, не ABI для dll/so.

Устанавливаемый пакет — JSON manifest + Rhai source. Примеры находятся в examples/*.module.json.
.dlm/.lm сохраняют pending snapshot, .review <sha256> показывает его, .approve <sha256> активирует.
Libraries входят в одобряемый пакет; локальные/сетевые imports не разрешаются.

Command: fn handler(args, config). Watcher/loop/callback: fn handler(event, config).
state доступен в scope; structured result содержит text/state/screen/media/album/frames. Неизвестные keys — ошибка.
Inline preview не сохраняет state. Watchers отвечают в triggering chat, loops — в Избранное.

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
загружается один раз при `.dlm`/`.lm` и замораживается в пакете, сетевых запросов в рантайме нет.

## Scope-требования

`"scope": ["ffmpeg", "inline", "shell"]` в манифесте проверяется при `.dlm`/`.lm`/`.approve`:
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
Defaults и ввод проверяются одинаково. Limits: 100000 ops, depth 64, call depth 32,
128 KiB string, 4096 array items, 1024 map items, 256 KiB state, 512 forms, 50 pages, 90 buttons.
Loop interval 1..86400 s. Это ограничения ресурсов, не формально проверенная sandbox.

Bot фасад: BOT_CONTRACT.md, BOT_FACADE.md, BOT_OBJECTS.md. Native access — bot.native().
Python-модули не выполняются. ОС и сеть не предоставляются Rhai неявно.
