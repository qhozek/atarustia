# Медиа и file IDs

media::Media задаёт kind/source/MIME/duration/dimensions/title/performer/spoiler/streaming/thumbnail.
Types: photo/document/video/audio/voice/animation/sticker/video_note. Sources: HTTPS, file_id, bytes+filename.
Upload bytes ограничен 64 MiB; document thumbnail 200 KiB. Native streaming доступен отдельно.

Albums: 2–10 photos/videos, либо только audio, либо только documents. Gallery формы — до 50 items.
Validation выполняется до upload/RPC. Файловое имя из Telegram не становится локальным путём автоматически.
download_to_path пишет во временный файл рядом с назначением и заменяет его только после успеха.

File IDs: v4/subversion 54, base64url/zero-run encoding. Inline IDs: 20/24-byte wire variants.
Проверяются flags/version/padding/trailing data. Legacy v2 и document thumbnail IDs не поддерживаются.
File IDs содержат capabilities — не публикуй их и не переноси между ботами без проверки доступа.

Assets хранятся в служебной теме. Cache до 256 entries, ключ bot ID + SHA-256.
Повторный upload при expired reference допускается, flood/network ошибки не маскируются.
Объекты — BOT_OBJECTS.md; requests/edits — BOT_FACADE.md; редкие events — EXTENDED_PAYLOADS.md.
