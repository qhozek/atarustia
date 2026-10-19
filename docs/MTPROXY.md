# Прокси

--proxy / ATARUSTIA_PROXY задаёт SOCKS5 или native MTProxy для user/bot и всех pool DC.

```sh
cargo run --locked -- --proxy 'socks5://127.0.0.1:1080'
cargo run --locked -- --proxy 'mtproxy://proxy.example:443?secret=YOUR_SECRET'
```

Принимаются mtproxy://host:port, tg://proxy и https://t.me/proxy с server/port/secret.
Secret: 16 bytes — intermediate; dd+16 — padded; ee+16+DNS domain — Fake TLS/SNI.
Hex и standard/URL-safe base64, padding optional. Classic transport=abridged также поддерживается.
С dd/ee несовместимый transport отклоняется. URL лучше передавать через закрытое environment file.

Есть obfuscated2 AES-CTR, 64-byte header, SHA-256 key derivation, transport padding и HMAC handshake.
Fake TLS — маскировочный протокол, не обычная TLS certificate validation и не обещание browser fingerprint.
DNS/TCP/handshake timeout 15 s. Нет fallback напрямую в Telegram. SOCKS5+MTProxy вместе запрещены.
DC 1–5; дополнительные test/CDN/media-only selector ограничены возможностями grammers.

Patch находится в vendor/grammers-mtsender и подключён Cargo patch. Он должен попасть в source deploy.
Проверки: cargo test --locked -p grammers-mtsender --features proxy --lib mtproxy::tests.
Loopback/crypto vectors проверены; реальный endpoint/login/media/reconnect требуют отдельной проверки.
