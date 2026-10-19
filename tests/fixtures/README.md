# Fixtures

Данные получены офлайн через Python TL-библиотеку исходного проекта. Все IDs, ключи, профили и платежи
синтетические. Файлы нужны для проверки wire compatibility, а не для входа в Telegram.

- `python_session.base64`: encrypted binary v1; master key `[9;32]`, auth key `[7;256]`, dummy peers/states.
- `python_sqlite.zip.base64`: SQLite v8 с дробным timestamp. ZIP используется только для сжатия fixture.
- `python_backup.base64`: db.json + вложенный mods.zip + requirements; fake token/grant проверяют фильтрацию.
- `python_moderation.json`: banned-rights bytes и исходная boolean mapping.
- `python_inline_content.json`: contact/geo/venue/text/preview и outer inline result.
- `python_service_actions.json`: membership/topics/TTL/payment/pin.
- `python_rare_rich.json`: preview/giveaway/story/checklist/rich paragraph.
- `python_extended_payloads.json`: instant view/areas/privacy/gifts/prices.
- `python_bot_facade.json`: send/preview/callback/server copy/game.
- `python_objects.json`: user/forum/photo/voice.

JSON expected-поля описывают Rust contract; это не Python object interpreter. Fixture не подтверждает live
поведение Telegram или доступность конкретного server-side feature.
