# Участники и права бота

`ban_chat_member` запрещает просмотр; `kick_chat_member` — alias ban, не ban+unban.
`unban_chat_member` снимает индивидуальные ограничения, но не возвращает пользователя в чат.
`restrict_chat_member` принимает ChatPermissions. `leave_chat` выходит из group/channel без удаления истории.

Ограничения доступны для channel/supergroup. Target — user, until_date — Unix timestamp 0..i32::MAX.
None/0 означает бессрочно; Telegram сам интерпретирует слишком близкие/далёкие сроки.

ChatPermissions использует legacy-семантику: media flags объединяются OR в send_media,
other_messages управляет stickers/GIFs/games/inline сразу. True означает «не ограничивать»,
а не обход default permissions. Незаданные поля false.

get_chat_member возвращает ChatMember со статусом, User, until_date/is_anonymous/custom_title и details.
Kicked определяется view_messages, membership хранится отдельно. Native ParticipantPermissions доступен
через `.native()`. Все операции требуют реальных прав Telegram; тесты не назначают бота администратором.
