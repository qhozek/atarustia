# Расширенные payloads

rich_payload разбирает все PageBlock закреплённого слоя и cached Page: embeds, articles, maps, lists,
tables, buttons, photos/documents. Raw HTML остаётся данными и не открывается в браузере.
Лимиты: depth 16, 512 nodes, до 100 элементов коллекции. Truncation отмечается partial.

story_payload сохраняет privacy rules, media areas, coordinates, reactions/views и sent reaction.
Web-page → story → media использует общий depth budget 4. Min/truncated data остаётся partial.

gift_payload читает regular/unique gifts, attributes/rarity, XTR/TON prices, theme/wallpaper metadata.
Long IDs и amounts — строки, nanos отдельно. Никаких покупок или переводов converter не делает.
Служебные события включают gifts, suggested posts, ownership, protected content и shared peers.

RequestPeer поддерживает user/group/channel/create-bot criteria, admin rights и requested name/username/photo.
Optional false отличается от отсутствия ограничения; max_quantity 1–10.

Passport received экспортирует только type/count и redacted/omitted_fields. Документы и credentials
не дублируются в script JSON. Native-код может читать исходный update.
Некоторые wallet/Gram/community объекты ещё неполны; см. PARITY.md.
