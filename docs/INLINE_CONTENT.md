# Inline content

InlineResult: Article, Media, Content, Typed и Game. Typed задаёт outer kind независимо от отправляемого content.
Media sources — HTTPS URL или cached file ID. Bytes нужно предварительно загрузить.

InputContent поддерживает Text, WebPage preview, Rich/RichText, FormattedText, Caption, Contact,
Location/live и Venue. Формат JSON показан в examples/inline_content.json.
Contact/geo/venue, Foursquare/Google Places, keyboard styles и metadata используют общий путь
inline/guest/web-app answers.

ID: 1–64 bytes; answer до 50 результатов без повторов. Text до 4096 UTF-16, caption до 1024.
Latitude −90..90, longitude −180..180, accuracy 0..1500 m. Live period 60..86400 либо i32::MAX.
Heading 1..360, proximity 1..100000 m. URL scheme проверяется до RPC.

Cached photo не принимает URL/thumb/description, cached document — URL/thumb. Непредставимые metadata
отклоняются, а не теряются. Rich+media запрещён. Callback security для прямого native result настраивает
вызывающий код; формы делают это сами.
