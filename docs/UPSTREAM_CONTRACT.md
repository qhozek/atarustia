# P0.1 — машинный контракт и контроль upstream

## Что зафиксировано

`contracts/upstream-manifest.json` — **эталон требований**, а не перечень реализованных функций.
Текущий pin — `122c2692391b1bfc40456270b7ff998c2ee10af8`.

Manifest содержит:

- 13 встроенных модулей и 66 команд: принадлежность модулю, aliases, Python method/signature,
  defaults, исходные doc/help RU/EN, security default, decorators и AST fingerprint;
- 24 ConfigValue: порядок, default и исходное выражение, validator, doc expression, on_change;
- все ключи **и значения** двух langpacks, включая `$main`, `$inline`, quoted `on`/`off`;
- SHA-256 и размеры assets, хеши всех отслеживаемых upstream-файлов;
- public Bot facade, inline/form/list/guest/types/Screen API, Module/Library/config API и validators;
- raw update → event mapping, handled updates, event-handler signatures/fingerprints;
- CLI options и исходные screen constants/icons;
- явную политику отличий, незакрытые архитектурные решения и ограничения доказательства parity.

### Источники данных

| Данные | Источник | Ограничение |
|---|---|---|
| Имена, signatures, aliases, defaults, AST hashes | Статический Python AST | Код не исполняется; динамическое выражение помечается `resolved: false` |
| Строки RU/EN | Строгий parser текущего подмножества YAML | Новая грамматика, дублирующиеся ключи или непрочитанный scalar вызывают ошибку, не приблизительный результат |
| Assets и files | Неизменяемые Git blobs | Не checkout: CRLF, локальные изменения и Git autocrlf не меняют inventory |
| Аргументы, сценарии, side effects | `contracts/command-behavior.json` | Вручную описанные требования по handlers/help; не автоматическое доказательство полной семантики |
| Допустимые различия и открытые решения | `contracts/port-policy.json` | Пробел реализации не становится допустимым отличием автоматически |

Для всех 66 команд есть syntax/scenarios/effects. В этих карточках перечислены обязательные
направления проверок, а не исчерпывающая формальная грамматика каждой команды. Точный handler
и его fingerprint остаются первоисточником для differential tests. Смена revision требует
повторного просмотра карточек, даже если имена команд не изменились.

## Запуск

Нужны **Git и Python 3.14**, без pip, Telegram credentials и запуска оригинального приложения.
CI и accepted generation используют одну minor-версию Python, чтобы изменения AST/unparse между
версиями интерпретатора не превращались в ложный drift. Смена minor требует проверки extractor.
Python здесь — dev-инструмент чтения исходников, не решение P0.2 о runtime `.e`/модулей.

```sh
# Offline self-tests с синтетическими исходниками/временным Git-репозиторием.
python -m unittest discover -s scripts -p 'test_upstream_contract.py' -v

# Проверить воспроизводимость принятого manifest из зафиксированного remote commit.
python scripts/upstream_contract.py verify

# Сравнить текущую remote main с pin. Ничего не обновляет и не коммитит.
python scripts/upstream_contract.py drift --output upstream-drift.json --markdown upstream-drift.md
```

При наличии локального Git-клона оригинала можно работать без сети:

```sh
python scripts/upstream_contract.py verify --source /path/to/ataraxia
python scripts/upstream_contract.py drift --source /path/to/ataraxia \
  --revision <candidate-full-40-char-sha> --output upstream-drift.json --markdown upstream-drift.md
```

`--source` читает committed blobs указанного revision, не dirty working tree.
Без `--source` инструмент создаёт временный bare Git-репозиторий, fetch конкретного commit
и удаляет временный каталог. Upstream Python никогда не импортируется, не выполняется;
зависимости оригинала не устанавливаются. Сеть ограничена Git fetch/ls-remote публичного upstream.

### Коды завершения

- `0`: verify совпал / drift не обнаружен / generation успешна;
- `1`: drift обнаружен — отличается SHA **или** извлечённые контракты;
- `2`: ошибка чтения, экстракции, схемы, неполная review-карточка или stale manifest.

Drift-report показывает added/removed/changed файлы, команды, конфиги, API, строки и assets.
Изменение только тела handler тоже замечается по AST; изменение docs/tests/workflows не теряется,
поскольку отслеживается всё дерево. Совпадение API при новом SHA всё равно требует review.
Если новая грамматика не поддерживается, JSON/Markdown сохраняют extraction error, а не зелёный статус.
При сетевой ошибке до определения candidate SHA отчёта может не быть; CI явно падает.

## Как принять новый upstream commit

1. Запустить `drift`, прочитать отчёт и diff оригинала. Не менять pin автоматически.
2. Проверить все `command-behavior.json` карточки, особенно изменённые handlers/help/security.
   Добавить новые/удалить исчезнувшие команды. Только после review обновить `reviewed_revision`.
3. Обновить extractor, если изменились структура AST, decorators, YAML или протокол inventory.
   Расширить synthetic tests. Не подменять неизвестный security decorator значением OWNER.
4. Проверить политику исключений. Принятые решения P0.2 менять только через review ADR и policy,
   не молча в generated manifest.
5. Сгенерировать **явный** candidate-файл:

   ```sh
   python scripts/upstream_contract.py generate --source /path/to/ataraxia \
     --revision <reviewed-full-sha> --output candidate-manifest.json
   ```

6. Просмотреть candidate diff и только затем заменить `contracts/upstream-manifest.json`.
   Обновить `original::REVISION`, документацию, ресурсы и реализацию затронутых контрактов либо
   явно отметить незакрытые расхождения. `generate` никогда не выбирает accepted output сам.
7. Выполнить verify, Rust structural checks, differential/failure tests; live/screenshots — отдельно.

Схема (`schema_version: 1`) и версия extractor (`extractor_version: 1`) зафиксированы.
Их изменение требует явной миграции manifest. Генерация детерминированна: нет timestamps,
абсолютных локальных путей или credentials. Python AST serializes через фиксированную форму,
словари JSON сортируются; line numbers сохранены для навигации и могут давать дополнительные
изменения после сдвига исходника.

## CI

`.github/workflows/upstream-contract.yml`:

- push/PR: offline tests + повторная генерация pinned manifest из remote commit;
- каждый день и вручную: сравнение remote main, JSON/Markdown artifact `upstream-drift`;
- разрешения только `contents: read`, без issue/PR auto-write и без изменения принятого pin;
- ошибка сети не трактуется как отсутствие drift.

`tests/upstream_manifest.rs` работает **без сети** в обычном `cargo test`: сравнивает текущий
Rust-каталог/ownership/aliases, pin/policy, langpack values и assets. В manifest также записаны
пока отсутствующие widgets/configs — тест не выдаёт их за реализованные.

## Что P0.1 не закрывает

Нет новых runtime-команд, изменения авторизации, security/backup/loader поведения или новой
модульной системы. Решения P0.2 отдельно приняты в [ADR-0001](decisions/0001-python-and-data-compatibility.md),
но не реализованы обновлением manifest. Нет доказательства визуального/поведенческого/Telegram 1:1.
Для этого нужны будущие differential payload/state tests и live acceptance из плана переписи.
