# Rich messages

Reference: Ataraxia `122c2692391b1bfc40456270b7ff998c2ee10af8`,
`ataraxia/inline/screens.py`.

## Rendering

`src/rich_html.rs` owns shared rich source normalization and structured plain fallback.
Help, configuration and `RichScreen` use the same normalization; forms derive fallback
from the actual rich source rather than a separately maintained text approximation.
Newlines around block elements are removed, ordinary newlines become `<br>`, and
newlines inside `<pre>` remain literal.

Fallback is Telegram HTML, not a tag-stripped string: headings, details/summary,
non-nested blockquotes, tables, lists, formatting, links and code are retained as
supported text/entities. Images are omitted, matching the reference parser.
Explicit rich button rows split at eight buttons; loose buttons group in pairs.
Extracted rows precede the bottom keyboard, with room reserved for bottom controls
and pagination within the 100-button limit.

Declared buttons retain their action and appearance by identity, not label. Copy,
URL, disabled and switch-inline actions retain their declared types in fallback.
In-text callbacks use the same form authorization and token lifecycle as ordinary
callbacks. Undeclared raw callback data is disabled: it cannot manufacture a core
action or register an arbitrary callback. Synthetic fallback identities have a fresh
random namespace per conversion and are never sent as callback tokens.

Long fallback text is parsed before pagination. Pages count UTF-16 units without
splitting Unicode scalars, and entities are intersected with each page's text range;
HTML source and entity references are never split as strings.

Colored rich buttons use a plain fallback icon instead of embedding custom emoji;
their ordinary keyboard fallback retains the declared custom-emoji ID. The generic
builder has only an emoji ID, so it retains its existing `▫` alternative, not an
invented mapping from arbitrary Telegram document IDs to upstream named icons.

## Delivery and error boundaries

Help delivery is rich-first for both `.help` and bot `/help`; the legacy help navigation
also no longer forces plain mode. When a connected inline bot fails to deliver `.help`,
the command propagates that failure instead of silently replacing it with ordinary HTML.
The no-bot path still provides non-interactive text help. A recognized rich rejection
while answering an inline query retries the same query with structured fallback; actual
answer errors are retained per form and consumed by the sending account, rather than
being replaced by a generic empty-result error. Every automatic rich fallback logs its
reason. Tests verify the help inline result contains a rich HTML payload with bound
in-text buttons, and changes to ordinary text only after explicit fallback.

Callback edits now retain the displayed revision's tokens until Telegram accepts the
replacement. Failed/cancelled edits restore the screen, keyboard, rich/plain state and
inline snapshot; successful edits retire the old tokens. Per-form callback lanes suppress
concurrent double clicks without running the action twice. These are in-memory UI
transactions, not rollback of database/module side effects or exactly-once RPC delivery.
Expiry, close and module invalidation still remove the form; rollback cannot resurrect it.
Updater progress screens commit independently, and an initial failed progress edit stops
the update/restart before checkout mutation/shutdown. Failure to display an updater error
is reported separately to the owner's bot PM (Saved Messages if that also fails).
Late/duplicate callback acknowledgement errors no longer prevent message edits or invalidate
an already delivered keyboard. New tests simulate failed edits, cancellation, successful
replacement, closure and duplicate-click admission; live Telegram reproduction remains pending.

Existing typed rich send/edit builders remain the transport layer; rich edits omit
ordinary text/entities/media fields. Form send/edit fallback is attempted only for
recognized server rich/entity/custom-emoji rejection. Network errors, FloodWait,
authorization errors and unrelated RPC errors propagate without an automatic second
plain send. A failed bot form send is not silently retried through the user account.
This is deliberately narrower than upstream's broad BadRequest retry.

Inline edit builders normalize an empty `replyInlineMarkup` (including rows with
no buttons) to an omitted markup field. This matches TDLib's inline edit path:
`ReplyMarkup.cpp` maps an empty inline keyboard to null, and
`InlineMessageManager.cpp` passes that null to `messages.editInlineBotMessage`.
It avoids the reported `REPLY_MARKUP_INVALID` when updater confirmation buttons
are replaced by buttonless progress/failure/restart screens. Nonempty retry and
navigation keyboards are retained. Chat edits keep their existing independent
markup behavior. Regression tests serialize/deserialize the updater's actual
inline edit request; live server verification is still required.

## Local verification and limits

`tests/fixtures/rich_html.json` contains 18 vectors from pure reference definitions.
Regenerate explicitly with:

```sh
python scripts/rich_html_vectors.py --source /path/to/ataraxia
cargo test --locked --test rich_html --test rich_ui --test config_ui --test help
```

The generator reads the pinned Git blobs and executes only an allow-listed extracted
renderer in a synthetic namespace. It does not import/run the userbot or log in to
Telegram. This differs from the non-executing static manifest extractor.

The tokenizer canonicalizes kept tag names/attributes, whereas upstream preserves their
original spelling. The quote-entity and uppercase fixtures compare Telegram text/entities for this
spelling-only difference; other vectors compare exact rich/plain strings and rows.
This is not a claim of equivalence for all malformed HTML.

Tests cover source normalization, tables/details/pre/entities, callback identity,
authorization and rotation, fallback styles, copy/URL/input/disabled actions,
pagination, keyboard limits and rich edit wire exclusivity. Live send/edit,
Premium/non-Premium, Telegram client visuals, group restrictions and server error
variants still require Telegram acceptance testing. Markdown-rich payloads remain
available through the existing typed API; this fallback converter is HTML-only.
