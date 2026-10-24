# Ataraxia Rust port — mandatory project direction

The user's requirement is a faithful copy of Ataraxia implemented in Rust, not a redesign or
an independently designed userbot. Reference: https://git.break.wtf/xenside/ataraxia,
snapshot a58e7019690abfec05e06b7383538fad39dc2d01.

- Preserve original public command names, argument semantics, messages, buttons, navigation,
  visuals, assets and web frontend. Do not invent extra commands or rename commands to atarustia.
- The original frontend in web-resources/ is copied byte-for-byte. Adapt the Rust backend to its
  API contract; do not rewrite the frontend to suit the backend. Preserve attribution in NOTICE.
- Use locales/original/ for original strings. Technical implementation differences must be
  explicit and must not be disguised as equivalent behavior.
- Keep internal Rust APIs separate from the user-facing command contract.
- Do not call the port complete because tests pass. Track unimplemented parity in
  docs/ORIGINAL_PARITY.md and verify Telegram behavior separately from local tests.
- Never commit credentials or tokens. Do not reset or discard unrelated work.
