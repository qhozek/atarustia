# Bot facade

`bot::Bot` — MTProto-фасад, не HTTP-клиент api.telegram.org.

- SendOptions: silent, protected content, reply/thread, keyboard, effect, business connection.
- Text: plain/HTML/Markdown и явные UTF-16 entities. Explicit entities имеют приоритет.
- Preview: URL, disable, size и расположение над текстом.
- Edit: Text, Rich, Caption, Media, Keyboard. Пустая подпись отличается от отсутствия text.
- Albums: 2–10 совместимых элементов, проверка перед upload, порядок по random ID.
- Forward/copy выполняются на сервере; copy снимает author через drop_author.
- Inline/guest/web-app: typed results, offsets, cache, start/web-app switch button, gallery.
- Bot commands/scopes, pin/unpin, callbacks, shipping/checkout, участники и загрузки файлов.

Расширенные параметры доступны через `_with_options`. Numeric IDs требуют cached access hash в session;
username разрешается через Telegram. Business вызов действительно использует InvokeWithBusinessConnection.
Inline edit направляется в DC из message ID.

Результаты и ошибки — BOT_CONTRACT.md. Объекты — BOT_OBJECTS.md. Права — BOT_MEMBERS.md.
Остаются Python default/coercion различия, optional-title results, auto-upload bytes при inline answer,
guest edge cases и проверки business DC на реальном аккаунте. Полный официальный HTTP Bot API не реализован.
