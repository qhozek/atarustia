# Объекты Bot API

User: имена, username (включая active alias), язык, Premium и bot capabilities.
Методы full_name, url, mention_html экранируют label.
Chat хранит private names/title/forum. ChatFullInfo добавляет bio/invite/linked chat/protection/accent/reactions.

Message хранит thread/reply/sender/date/forward/via-bot, media group, подписи, keyboard и business metadata.
Typed media: PhotoSize, Document, Video, Animation, Audio, Voice, VideoNote, Sticker, Contact, Dice, Poll,
Venue и Location. В Rust поля доступны через message.media или Deref; JSON содержит их на верхнем уровне.

`sent.object()` восстанавливает Message из response object с проверкой identity.
`message.bind(&bot)` даёт answer/reply/media/edit/delete/forward/pin/unpin. Thread/business наследуются,
явные SendOptions сохраняются. Bot handle не сериализуется.
CallbackQuery.answer и InlineQuery.answer используют общий ApiResult.

content_type, html_text и md_text работают после JSON round trip. HTML экранируется; плохие UTF-16/crossing
entities дают ошибку. Forward/reply bodies без данных Telegram не загружаются автоматически.

Document thumbnails пока None: codec не различает их и полный документ. Guest metadata не вся гидратируется.
Это Rust API, а не Python attribute holder. Сетевые сценарии проверяются отдельно от конверсии объектов.
