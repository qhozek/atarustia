# Local MTProxy patch

Base: crates.io `grammers-mtsender` 0.11.0, https://codeberg.org/Lonami/grammers.
Original copyright headers retained; distributed under the MIT option (LICENSE-MIT).
The application uses `[patch.crates-io]` so `grammers-client` and direct users share one sender implementation.

Changes:

- `mtproxy.rs`: native client MTProxy config, classic abridged/intermediate, dd padded intermediate,
  ee Fake TLS with SNI, authenticated hello and TLS-like record wrapping, obfuscated2 AES-CTR.
- `ConnectionParams.mtproxy`, `ServerAddr::MtProxy`, pool transport selection and initConnection proxy info.
- `net/tcp.rs`: AsyncRead/Write dispatch and bounded duplex bridge owning a cancellable socket task.
- Every pool DC creation/reconnect/auth-key regeneration selects the same proxy configuration.
- MTProxy/SOCKS5 conflict is an error; no direct fallback. Unsupported DC IDs fail rather than truncate.
- Local transport tests, independent mock server directions and Python obfuscated2 vectors.
- Upstream live Telegram integration tests explicitly ignored; missing monorepo deps-check removed.
- Added aes/ctr/sha2/hmac/rand/base64/zeroize/x25519-dalek for protocol primitives.

Fake TLS is not ordinary TLS: no certificate chain or TLS encryption is performed by this layer.
The handshake verifies the shared-secret HMAC, then wraps the obfuscated MTProto byte stream in records.
The generated ClientHello is valid-shaped TLS 1.3 with an X25519 key share, not a promise of a current browser fingerprint.
MTProto still provides end-to-end client↔Telegram authorization/encryption.

Protocol references consulted: Telegram transport docs, the original Python tcpmtproxy.py,
Telegram Desktop mtproto_tls_socket.cpp handshake algorithm. New implementation is Rust, not copied GPL source.

Run `cargo test --locked -p grammers-mtsender --features proxy --lib mtproxy::tests` from workspace root.
Keep this patch during dependency updates; do not edit the global Cargo registry cache instead.
