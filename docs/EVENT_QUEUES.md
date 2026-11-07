# P0.1 плана v2 — ограниченные очереди событий

Не путать с первым P0.1 (upstream manifest). Этот этап меняет admission/backpressure,
не пользовательские команды, их аргументы, права или оформление.

## Было

Account/native/raw/message handlers и loops использовали `try_acquire_owned()` на 32 permits.
При занятых permits событие/итерация не запускались и не попадали в очередь. Inline loop
делал то же самое. На уровне MTProto заполненный updates channel тоже отбрасывал envelopes.

## Стало

| Уровень | Работает одновременно | Ожидает | Переполнение |
|---|---:|---:|---|
| Account handlers + native updates + background jobs | 32 | 256 jobs | Producer ждёт места; ожидание отменяется при shutdown |
| Обычный inline handler | 32 | 256 jobs | Явная ошибка и остановка inline stream, без молчаливого drop |
| Внутренние form query/chosen-result acknowledgements | 4 | 64 jobs | Явная ошибка и остановка inline stream |
| MTProto sender/DC | RPC продолжает работать | 4096 envelopes + bounded transport channel | Явный UpdateBacklogExceeded и остановка sender pool |

Числа — пределы количества jobs/envelopes, **не лимит байтов памяти**. Payload имеет
свои protocol/application limits. Текущий этап не делает on-disk журнал и не обещает
end-to-end exactly-once delivery, восстановление после потери питания или безграничный backlog.

`work_queue::WorkQueue` держит bounded Tokio channel и фиксированное число executing tasks.
Нет `spawn` на каждое событие, которое затем бесконечно ждёт semaphore. Account producer
резервирует место сам, а worker runner независимо освобождает очередь; RPC transport продолжает
обрабатывать ответы, пока producer ждёт. FIFO — порядок admission; несколько handlers могут
завершаться в разном порядке, как и раньше.

## Почему inline не ждёт обычную очередь

Callback/module handler может открыть форму и ждать `unit:<id>` inline query или chosen-result.
Если stream-reader заблокировать на полной обычной очереди, он не дойдёт до служебного update,
нужного для завершения уже работающего handler: deadlock.

Демультиплексор продолжает читать update stream и направляет **только** `unit:` form queries и
их acknowledgements в отдельную ограниченную очередь. Этот путь не вызывает модульные handlers,
не применяет config changes и не выполняет callbacks. Проверка актуальных owners остаётся.
Account input, config input, обычные inline queries и callbacks не получают bypass этой полосой.

Заполненный inline backlog — явная аварийная граница: stream закрывается и пишет причину.
Это предпочтительнее «успешно» забытых событий или неограниченного роста памяти. Автоматического
повтора команд с side effects нет. Для автоматического reconnect/recovery нужен отдельный
контракт; `.restart`/перезапуск службы остаётся способом восстановить остановившийся inline client.

## Transport backpressure

Изменён vendored `grammers-mtsender`, не global Cargo cache. Нельзя просто `await updates.send()`
внутри сетевого `sender.step()`: обработчик может ждать RPC, а чтение RPC-ответа тогда тоже остановится.
Поэтому у sender есть FIFO pending envelopes и отдельная `reserve()` ветка select; network/RPC
продолжают обслуживаться. Overflow завершает pool явно, не пытается незаметно пересоздать соединение
и скрыть потерю control update. См. `vendor/grammers-mtsender/ATARUSTIA_PATCH.md`.

## Периодические работы

`config-changes`, module update poll и loop с ключом
`module:revision:function` coalesce, если такая же работа уже queued/running. Это не event drop:
таймерные тики объединяются намеренно, новая итерация может быть запланирована после завершения.
Revision входит в loop key; штатная проверка актуальности модуля в `run_loop` сохранена.
Обычные Telegram/native/raw/message jobs не coalesce.
Уже созданные backup uploads и разные log snapshots также не coalesce: каждый готовый результат
остаётся отдельной работой, а не теряется из-за совпадающего имени фонового задания.

## Shutdown и наблюдаемость

Ожидающий producer немедленно выходит по cancellation. Runner закрывает admission, отменяет
executing jobs и освобождает queued jobs; незавершённые задачи **не повторяются** автоматически.
Счётчики accepted/completed/cancelled/queued/active/coalesced/backpressure доступны в Metrics.
При остановке итог записывается в tracing; переполнение и panic имеют отдельную диагностику
без записи содержимого сообщения, кода/2FA или callback token.

Отмена не откатывает уже совершённые Telegram/HTTP side effects. Синхронизация Telegram update
state не является журналом обработки приложением: после shutdown/ошибки некоторые неполностью
обработанные updates могут не воспроизводиться. Durable acknowledgement/replay относится к
дальнейшему этапу runtime/recovery, не замаскировано под готовое exactly-once решение.

## Проверки

- Burst 160 jobs (>32): каждый обработан ровно один раз, активность ограничена, backpressure срабатывает.
- Заполненная очередь + shutdown: заблокированный producer освобождается, принятые active/queued
  задачи учтены как отменённые, permits/keys не остаются висеть.
- Все 32 обычных handlers ждут формы: reserved delivery освобождает их без deadlock.
- Periodic coalescing, повторный запуск после completion и panic.
- Inline overflow возвращает ошибку, не создаёт бесконечный backlog/waiters.
- Routing допускает в reserved lane только form delivery, не account/config input.
- Transport backlog остаётся ограниченным и при достижении предела выдаёт явную ошибку.

Тесты без Telegram. Продолжительная live-нагрузка, reconnect и устойчивость при реальных
RPC/FloodWait ещё требуют отдельной приёмки. Пользовательские интерфейсы не менялись.
