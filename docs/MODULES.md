# Модули

Rust-модуль реализует sdk::Module и регистрируется через runtime::run_with_modules.
Пример: cargo run --example native_module -- --no-web. Это source interface, не ABI для dll/so.

Устанавливаемый пакет — JSON manifest + Rhai source. Примеры находятся в examples/*.module.json.
.dlm/.lm сохраняют pending snapshot, .review <sha256> показывает его, .approve <sha256> активирует.
Libraries входят в одобряемый пакет; локальные/сетевые imports не разрешаются.

Command: fn handler(args, config). Watcher/loop/callback: fn handler(event, config).
state доступен в scope; structured result содержит text/state/screen/media/album. Неизвестные keys — ошибка.
Inline preview не сохраняет state. Watchers отвечают в triggering chat, loops — в Избранное.

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
