# Возвращаемые значения Bot

Все async-методы `bot::Bot` возвращают `ApiResult<T>`. Ошибка содержит method, kind, description и retry_after.
Имя внешнего helper и flood wait сохраняются через внутренние вызовы.

| Операция | Результат |
|---|---|
| send / forward | SentMessage |
| album | Vec<SentMessage> |
| copy | MessageId |
| edit | EditResult: Message, Acknowledged или Inline |
| get_chat / get_chat_member / get_file | ChatFullInfo / ChatMember / File |
| get_my_commands | Vec<BotCommand> |
| операции без данных | () |

SentMessage проверяет chat/message identity. Пустой native message и неполный album — InvalidResponse.
Acknowledged означает подтверждённый edit без загруженного body, а не подставное сообщение.
`sent.object()` даёт typed Message.

Validation ошибки — BadRequest, socket/timeout — Network, файлы — LocalIo, плохой ответ — InvalidResponse.
RPC использует Forbidden/Unauthorized/NotFound/RetryAfter/Server/Other.

Для native grammers Message/anyhow используй `bot.native()`. Обратное преобразование — Bot::from_native.
Неявного Deref к native слою нет. Возвращаемые типы отличаются от ранних версий проекта;
вызовы с `?;` обычно менять не нужно, код с native Message нужно переключить явно.
