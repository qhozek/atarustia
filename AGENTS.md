# Ataraxia Rust port — mandatory project direction

The user's requirement is a faithful copy of Ataraxia implemented in Rust, not a redesign or
an independently designed userbot. Reference: https://git.break.wtf/xenside/ataraxia,
current main, pinned for this port at 122c2692391b1bfc40456270b7ff998c2ee10af8.

- Preserve original public command names, argument semantics, messages, buttons, navigation,
  visuals and assets. Do not invent extra commands or rename commands to atarustia.
- Current Ataraxia uses terminal login and 66 commands in 13 built-in modules. The old web
  frontend is a legacy, explicitly opt-in extension, not part of current-main parity. Preserve
  its attribution in NOTICE; do not discard old account data while migrating login flows.
- Use locales/original/ for original strings. Technical implementation differences must be
  explicit and must not be disguised as equivalent behavior.
- Keep internal Rust APIs separate from the user-facing command contract.
- Do not call the port complete because tests pass. Track unimplemented parity in
  docs/ORIGINAL_PARITY.md and verify Telegram behavior separately from local tests.
- Never commit credentials or tokens. Do not reset or discard unrelated work.
