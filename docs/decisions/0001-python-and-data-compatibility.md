# ADR-0001: Rust-ядро, Python-расширения и двусторонние данные

Статус решения: **принято пользователем в P0.2**.
Статус реализации: **Python bridge ещё не реализован; данные — частичный импорт, не двусторонняя поддержка**.
Эталон: Ataraxia `122c2692391b1bfc40456270b7ff998c2ee10af8`.

## Основание

Цель перехода на Rust — эффективность, скорость обработки и управляемое потребление ресурсов,
а не обязательное устранение Python из установки. Пользователь попросил сохранить Python-поддержку
и явно выбрал двустороннюю совместимость сессий/бэкапов. Встроенные команды, тексты, оформление
и сценарии должны соответствовать оригиналу; техническая идентичность транспорта остаётся правдивой.

Это решение о границах. Оно не означает, что любой сторонний модуль уже работает, не объявляет
Rhai Python-совместимым и не обещает меньший RTT до Telegram от смены языка.

## 1. Разделение ответственности

| Rust | CPython compatibility runtime |
|---|---|
| MTProto connections, session ownership, reconnect, caches | Python `.e` и print/result/exception semantics |
| Встроенные модули и их UI/state machines | Загрузка внешних `.py`, libraries, decorators/hooks |
| Dispatcher/security, form lifecycle, persistence, backups | Python-visible Module/ConfigValue/utils/client/message objects |
| Async I/O, очереди, cancellation, shutdown, update | Выполнение Python-кода и его зависимостей |

CPython не используется как backend встроенных Rust-команд. Не запускаем параллельный Python-юзербот
или отдельный ataraxia-tl-клиент с тем же auth key для выполнения module.client вызовов.
Один Rust account context владеет account session; второй bot context — соответствующей bot session.

## 2. Целевая интеграция

Базовый дизайн: встроенный **CPython через PyO3**, Python version pin и поддерживаемые платформы
определяются после отдельного integration spike. PyO3 сейчас в Cargo не добавляется: нужна проверка
Linux/Windows linking, wheels/venv и async bridge до принятия конкретной версии зависимости.

- Python interpreter живёт в процессе Rust; eval и module imports не выполняются непосредственно
  на Tokio event-loop worker. Нужен явный execution context и контролируемый GIL.
- Асинхронные proxy calls создают awaitable, передают typed request Rust-сервису и возвращают
  typed result/exception. Не держать GIL/DB mutex/form lock при ожидании сети.
- Идентичность account/module/invocation явно передаётся; нет случайного «текущего аккаунта» процесса.
- Внешний Python-код остаётся **доверенным и не sandboxed**, как в оригинале: может читать файлы,
  запускать процессы и блокировать interpreter. Очереди/таймауты не превращают его в песочницу.
- Не обещать принудительную безопасную отмену произвольного Python/C-extension кода. Cooperative
  cancellation работает для bridge awaits; hung code/GIL starvation должны диагностироваться.
- Импорты `ataraxia`, `telethon`, `herokutl`, public TL/error names предоставляются совместимым
  namespace/proxy, а не ложным объектом другого клиента. Не выдавать JSON вместо Python Message.
- Произвольные internals, monkeypatching, object identity и third-party packages должны иметь явную
  матрицу поддержки. Unsupported behavior выдаёт точную ошибку, не незаметный иной результат.
- Зависимости модулей устанавливаются в управляемую Python-среду с timeouts и исходной политикой
  requirements/system packages. Не изменять глобальный Python и не исполнять requirements на review/import.
- PyO3 embedding может потребовать platform-specific конфигурации venv/native extensions: проверить
  это отдельно; нельзя считать `sys.path` полной изоляцией dependency environment.
- Rhai-пакеты сохранить как дополнительный формат, не удалять установленные данные. Их capabilities
  и лимиты не распространяются на Python/native модули. Не переименовывать `.e` и не выдавать Rhai
  за Python после появления bridge.

## 3. Контракты Python-совместимости

Обязательные acceptance surfaces:

1. `.e`: Python expressions/statements, `await`, reply fallback, independent print capture,
   ошибки/tracebacks/redaction, original scope (`client`, `message`, `reply`, `db`, `lookup`, TL),
   output blocks/pages/file. Старый Rhai `.e` остаётся известным пробелом до этого этапа.
2. Loader: `.lm/.dlm` настоящих `.py`; pinned source/URL, metadata, requirements, install/update
   confirmation, восстановление старой ревизии, core overwrite protection и ошибки загрузки.
3. Module/Library: registration/config/ready/unload/on_dlmod, watchers/raw/bot/inline/guest,
   loops/StopLoop/SelfUnload/SelfSuspend; порядок и конкурентность как проверяемый контракт.
4. Client/Message/TL: async RPC, entity/media/form operations, typed results/errors, reply/thread,
   multiple accounts и usernames; import redirects и каждый bridge API из upstream manifest.
5. Security: те же решения о владельцах/масках/targeted rules и callbacks. Нельзя утверждать,
   что эти проверки защищают от намеренно вредоносного установленного Python-модуля.
6. Config/strings/placeholders: значения, validators, on_change, storage namespace, fallback,
   formatting, регистрация/очистка placeholders и help/config metadata.

Минимальные fixture-модули сначала синтетические: hello/await/print, config+on_change,
watcher/loop, inline input/callback, library dependency, media/raw RPC, unload/failed update.
Сторонние модули подключать после просмотра и без production credentials в CI.

## 4. Двусторонние сессии

Требование — **offline codecs и импорт/экспорт**, не один общий рабочий каталог и не одновременная
работа Python и Rust с одной авторизацией. Остановить исходный клиент; исходные файлы не менять.

- Поддерживаемый Python baseline: encrypted `ATBINC1` payload v1 и master key; Rust: `ATRUST1` v1.
  SQLite v6–v8 остаётся legacy input, не обязательным writable output.
- Экспорт в отдельное пустое назначение; отказ от overwrite; отдельный ключ назначения и
  authenticated encryption. Мастер-ключ — только в явном session export, не обычном backup.
- Перенос auth keys/DC, peer/access hashes, account identity и update states должен проверяться
  на реальном формате target reader, не только на собственном roundtrip encoder/decoder.
- Вначале определить lossless shared profile. Необратимые/непредставимые поля — сохранить в
  versioned model либо **отклонить совместимый экспорт** с отчётом. Не молча отбрасывать temporary
  keys/takeout/sent-file cache или иные поля и называть это полным переносом.
- Для Rust-данных, не имеющих Python-представления, separate native archive допустим, но он не
  считается Python-compatible session. Sidecar не делает оригинальный reader совместимым.
- Проверки: Python → Rust → Python и Rust → Python → Rust, tamper/wrong key, missing key,
  corrupt/unknown version, duplicate account, partial disk write, original file preservation.
- Auth key duplication может отозвать сессию. Live-проверка только последовательно, на тестовом
  аккаунте и по отдельному разрешению. Секреты/реальные session fixtures не коммитить.

Текущий importer не переносит все поля: это не закрытое требование и не согласованная потеря.

## 5. Двусторонние бэкапы

Целевой portable profile — оригинальные DB JSON, `mods.zip`, `.backup` с DB/modules/URLs/
requirements. Форматы должны читаться текущим оригинальным reader без его изменений.

- Python source сохраняется побайтово; имена/namespace/URL mapping и owner suffix переводятся
  явно и обратно. Unknown portable JSON должен сохраняться либо вызывать точный отказ.
- Не активировать owner/security grants, чужой bot token или Python-код от одного разбора архива.
  Применение — отдельная подтверждённая restore-операция с исходной политикой безопасности.
- Не считать нынешний sanitizer settings полным bidirectional codec: его purpose — безопасный
  частичный импорт. Требуется отдельная lossless representation до применения.
- Обычные backups не включают master key/session. Module config/state может содержать секреты:
  нужны безопасные destination/captions, file permissions и предупреждения.
- Rhai и statically linked Rust extensions не становятся Python `.py` через переименование.
  Совместимый export таких пакетов обязан отклонить запрос с явным объяснением; для сохранения
  всех Rust-данных нужен отдельный native backup. Не генерировать пустые `.py` или молча терять модуль.
- Проверки traversal/links/bombs/duplicate/case collision и limits сохраняются. Untrusted архив
  не запускает imports/hooks/requirements при parsing, staging, inventory и preview.
- Приёмка проверяет target Python parser, restore effects/token handling, restart и оригинальные
  filename/caption/buttons. Собственный успешно распарсенный JSON не доказывает совместимость.

## 6. Быстродействие — измерять, не обещать

Одинаковые машина/ОС/Telegram DC/конфигурация/модули, release Rust и зафиксированный Python baseline.
Отдельные сценарии: Rust-only core, idle с загруженным Python, Python-module command/watcher,
burst events, media и длительный uptime. Минимум несколько прогонов с сырыми результатами.

Измерять: startup до ready (cold/warm), RSS/CPU idle и под нагрузкой, p50/p95/p99 dispatch/UI
latency, queue delay/drops, bridge crossing overhead, allocations/caches и shutdown.
RTT Telegram измерять отдельно: Rust не убирает сетевой маршрут и server latency.

Численные budgets принимаются после baseline, не выдумываются в ADR. Первичный обязательный
guard: отсутствие потери/повтора событий и несовпадения state под нагрузкой. После него —
сравнение throughput/RAM/CPU. Нельзя объявлять ускорение только по языку реализации.

## 7. Исполнимый порядок следующей работы

1. **Bridge spike:** PyO3+CPython на Windows/Linux, isolated env/native wheel, awaitable roundtrip,
   GIL/cancellation/shutdown и benchmark. Не заменять `.e` до passing fixtures.
2. **Python `.e`:** scope/await/print/error/redaction и исходный UI; отдельные regression tests.
3. **Python module runtime:** loader/lifecycle/config/translation, затем typed client/TL/forms API.
   Вести per-feature matrix, не «совместимо со всеми».
4. **Session codecs:** versioned neutral model, target-reader fixtures, lossless/reject policy,
   export+roundtrip, затем разрешённая live sequential acceptance.
5. **Backup codecs:** neutral archive/DB/source model, original parser roundtrip, confirmed apply,
   explicit native-only limitations и failure recovery.
6. **Parity/performance gate:** differential payload/state tests, load/GIL/queue tests, live UI
   screenshots и reproducible benchmarks. Только затем расширять заявленный support level.

P0.2 завершён как выбор архитектуры и совместимости. Он не закрывает реализацию runtime/codecs,
P0.3/P0.4 или оставшуюся функциональную перепись.
