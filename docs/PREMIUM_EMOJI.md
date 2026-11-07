# Original Premium emoji

Reference: Ataraxia main `122c2692391b1bfc40456270b7ff998c2ee10af8`.

`src/emoji.rs` contains all 35 named icons from `inline/screens.py::ICONS`:
original Telegram document IDs, custom-emoji alternative text, and separate plain
fallbacks. `scripts/emoji_vectors.py` extracts the literal original table without
importing/executing the application. `tests/fixtures/emoji.json` pins it for tests.

Help and configuration share this inventory instead of separate incomplete maps.
Help section headings, hidden/show actions, copy actions, placeholder/developer
icons and footer/settings controls use their original icons. Configuration hidden
value and series removal controls retain the proper eye/hide/remove identity.
Colored rich buttons keep plain named fallback icons, as upstream does, while
uncolored rich buttons use the original alternative (not a generic square).

The grammers HTML parser ignores `tg-emoji`/legacy `emoji` tags. The shared HTML
adapter now adds `MessageEntityCustomEmoji` using an independent tokenizer's UTF-16
offsets, preserving surrounding formatting/links/mentions. It never treats escaped
user text as markup. Ordinary form HTML, structured rich fallback, Bot API facade
text/captions, inline content, module HTML and native user-facing HTML paths use the
adapter. Updater's specialized parser delegates emoji handling to the same adapter.
Quickstart retains its four-letter Premium logo, `/start` retains the original
logo/symbols, and aliases no longer lose their configured custom emoji.

Non-Premium user-account paths strip custom emoji to their alternatives; Screen
icons use their explicitly pinned plain fallback. Bot messages follow the original
bot rendering path and may contain emoji independently of the user's Premium flag.
Telegram server permissions still apply, including inline keyboard custom icon
restrictions. Merely having a document ID does not grant the bot permission to use it.

Tests cover every icon and every emoji tag in both pinned langpacks, ordinary/rich
formatting, Premium/non-Premium help, quickstart, callbacks and Unicode offsets.
This does **not** claim every original command screen has been ported: unimplemented
security/loader/executor screens remain tracked in ORIGINAL_PARITY.md. Their locale
assets are preserved, but adding IDs cannot create missing runtime behavior.
Live Telegram visual comparison and all server permission combinations are pending.
