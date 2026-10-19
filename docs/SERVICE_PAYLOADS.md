# События

bot_events выдаёт name/chat/sender/message_id/data. Message metadata хранится в message_payload,
typed объект — в data.object или SentMessage.object(). Полные имена peers доступны только из кеша/ответа.

Есть membership/migration/pin/topic/TTL/Web App/payment/refund/call/game/proximity/boost/giveaway events,
gifts/suggested posts/shared peers и основные business updates. Binary payment/callback bytes — base64.
Long call/game/gift IDs — строки. Access hashes звонков не экспортируются.

Paid/invoice media сохраняют preview и доступные файлы; не создают file ID для закрытого preview.
До 10 items, media depth 4. Превышение явно partial/truncated. Privacy/gifts/instant view — EXTENDED_PAYLOADS.md.
Passport credentials не дублируются в JSON. Неизвестный service action остаётся constructor ID + partial,
без Debug dump. Payload может содержать персональные данные; не публикуй events целиком в логах.
