# Rich UI

RichText поддерживает HTML/Markdown. Send/edit не смешивает rich_message с обычными text/entities/media.
Caption edit с пустой строкой очищает подпись; keyboard-only edit не трогает body.
Inline edit использует message DC. Прямой native rich call не делает автоматический fallback.

Forms дают fallback на plain text и keyboard. Styled taps и custom emoji сохраняют alt/style;
callbacks получают новые opaque tokens, права/TTL/module gate проверяются заново.
Markers должны ссылаться на существующие кнопки. Button appearance arrays должны совпадать по shape.
RichScreen умеет headings/tables/details/lists/links/code/emoji; raw html — для доверенного renderer.

Rich tree разбирает PageBlock/formatting/buttons/instant-view с depth/node/collection limits.
Неполные данные помечаются partial. Parser не выполняет embedded HTML и не ходит по URL.
Details — EXTENDED_PAYLOADS.md. Серверные Premium emoji особенности в группах ещё требуют live-проверки.
