//! Bounded rich-message tree, not HTML regeneration; preserves structured formatting and buttons.
use grammers_client::tl;
use serde_json::{Value, json};
struct Budget {
    nodes: usize,
}
impl Budget {
    fn enter(&mut self, depth: usize) -> bool {
        if depth >= 16 || self.nodes == 0 {
            return false;
        }
        self.nodes -= 1;
        true
    }
    fn text(&mut self, value: &tl::enums::RichText, depth: usize) -> Value {
        use tl::enums::RichText as T;
        if !self.enter(depth) {
            return json!({"type":"truncated","partial":true});
        }
        macro_rules! wrap {($kind:expr,$t:expr)=>{json!({"type":$kind,"text":self.text(&$t.text,depth+1)})};}
        match value {
            T::TextEmpty => json!({"type":"empty"}),
            T::TextPlain(t) => json!({"type":"plain","text":t.text}),
            T::TextBold(t) => wrap!("bold", t),
            T::TextItalic(t) => wrap!("italic", t),
            T::TextUnderline(t) => wrap!("underline", t),
            T::TextStrike(t) => wrap!("strike", t),
            T::TextFixed(t) => wrap!("code", t),
            T::TextSubscript(t) => wrap!("subscript", t),
            T::TextSuperscript(t) => wrap!("superscript", t),
            T::TextMarked(t) => wrap!("marked", t),
            T::TextSpoiler(t) => wrap!("spoiler", t),
            T::TextMention(t) => wrap!("mention", t),
            T::TextHashtag(t) => wrap!("hashtag", t),
            T::TextBotCommand(t) => wrap!("bot_command", t),
            T::TextCashtag(t) => wrap!("cashtag", t),
            T::TextAutoUrl(t) => wrap!("auto_url", t),
            T::TextAutoEmail(t) => wrap!("auto_email", t),
            T::TextAutoPhone(t) => wrap!("auto_phone", t),
            T::TextBankCard(t) => wrap!("bank_card", t),
            T::TextTonAddress(t) => wrap!("ton_address", t),
            T::TextConcat(t) => {
                json!({"type":"concat","texts":t.texts.iter().take(100).map(|t|self.text(t,depth+1)).collect::<Vec<_>>(),"partial":t.texts.len()>100})
            }
            T::TextUrl(t) => {
                json!({"type":"url","url":t.url,"webpage_id":t.webpage_id.to_string(),"text":self.text(&t.text,depth+1)})
            }
            T::TextEmail(t) => {
                json!({"type":"email","email":t.email,"text":self.text(&t.text,depth+1)})
            }
            T::TextPhone(t) => {
                json!({"type":"phone","phone":t.phone,"text":self.text(&t.text,depth+1)})
            }
            T::TextAnchor(t) => {
                json!({"type":"anchor","name":t.name,"text":self.text(&t.text,depth+1)})
            }
            T::TextMentionName(t) => {
                json!({"type":"text_mention","user_id":t.user_id,"text":self.text(&t.text,depth+1)})
            }
            T::TextCustomEmoji(t) => {
                json!({"type":"custom_emoji","document_id":t.document_id.to_string(),"alt":t.alt})
            }
            T::TextImage(t) => {
                json!({"type":"image","document_id":t.document_id.to_string(),"width":t.w,"height":t.h})
            }
            T::TextMath(t) => json!({"type":"math","source":t.source}),
            T::TextDate(t) => {
                json!({"type":"date","date":t.date,"relative":t.relative,"short_time":t.short_time,"long_time":t.long_time,"short_date":t.short_date,"long_date":t.long_date,"day_of_week":t.day_of_week,"text":self.text(&t.text,depth+1)})
            }
            T::TextDiff(t) => {
                json!({"type":"diff","text":self.text(&t.text,depth+1),"old_text":self.text(&t.old_text,depth+1)})
            }
            T::TextButton(t) => {
                let markup = tl::types::ReplyInlineMarkup {
                    force_reply: false,
                    rows: vec![
                        tl::types::KeyboardInlineButtonRow {
                            buttons: vec![
                                tl::types::KeyboardInlineButton {
                                    text: String::new(),
                                    r#type: t.r#type.clone(),
                                    style: None,
                                }
                                .into(),
                            ],
                        }
                        .into(),
                    ],
                }
                .into();
                json!({"type":"button","text":self.text(&t.text,depth+1),"kind":crate::keyboard_payload::from_tl(&markup)["rows"][0][0]["kind"],"style":t.style.as_ref().map(|s|{let tl::enums::RichButtonStyle::Style(s)=s;json!({"primary":s.bg_primary,"success":s.bg_success,"danger":s.bg_danger,"link":s.link})})})
            }
        }
    }
    fn blocks(&mut self, values: &[tl::enums::PageBlock], depth: usize) -> Value {
        json!(
            values
                .iter()
                .take(100)
                .map(|b| self.block(b, depth))
                .collect::<Vec<_>>()
        )
    }
    fn block(&mut self, value: &tl::enums::PageBlock, depth: usize) -> Value {
        use tl::enums::PageBlock as B;
        if !self.enter(depth) {
            return json!({"type":"truncated","partial":true});
        }
        macro_rules! line {($kind:expr,$b:expr)=>{json!({"type":$kind,"text":self.text(&$b.text,depth+1)})};}
        match value {
            B::Title(b) => line!("title", b),
            B::Subtitle(b) => line!("subtitle", b),
            B::Header(b) => line!("header", b),
            B::Subheader(b) => line!("subheader", b),
            B::Paragraph(b) => line!("paragraph", b),
            B::Footer(b) => line!("footer", b),
            B::Kicker(b) => line!("kicker", b),
            B::Thinking(b) => line!("thinking", b),
            B::Math(b) => json!({"type":"math","source":b.source}),
            B::ButtonRow(b) => {
                json!({"type":"button_row","align_left":b.align_left,"align_center":b.align_center,"align_right":b.align_right,"buttons":b.buttons.iter().take(8).map(|button|{let tl::enums::PageButton::Button(button)=button;self.text(&tl::types::TextButton{text:button.text.clone(),r#type:button.r#type.clone(),style:button.style.clone()}.into(),depth+1)}).collect::<Vec<_>>(),"partial":b.buttons.len()>8})
            }
            B::Map(b) => {
                json!({"type":"map","location":crate::message_payload::location(&b.geo),"zoom":b.zoom,"width":b.w,"height":b.h,"caption":self.caption(&b.caption,depth+1)})
            }
            B::InputPageBlockMap(b) => {
                json!({"type":"input_map","location":match &b.geo {tl::enums::InputGeoPoint::Point(p)=>json!({"latitude":p.lat,"longitude":p.long,"horizontal_accuracy":p.accuracy_radius}),tl::enums::InputGeoPoint::Empty=>Value::Null},"zoom":b.zoom,"width":b.w,"height":b.h,"caption":self.caption(&b.caption,depth+1)})
            }
            B::Embed(b) => {
                json!({"type":"embed","full_width":b.full_width,"allow_scrolling":b.allow_scrolling,"url":b.url,"html":b.html,"poster_photo_id":b.poster_photo_id.map(|id|id.to_string()),"width":b.w,"height":b.h,"caption":self.caption(&b.caption,depth+1)})
            }
            B::EmbedPost(b) => {
                json!({"type":"embed_post","url":b.url,"webpage_id":b.webpage_id.to_string(),"author_photo_id":b.author_photo_id.to_string(),"author":b.author,"date":b.date,"blocks":self.blocks(&b.blocks,depth+1),"caption":self.caption(&b.caption,depth+1),"partial":b.blocks.len()>100})
            }
            B::RelatedArticles(b) => {
                json!({"type":"related_articles","title":self.text(&b.title,depth+1),"articles":b.articles.iter().take(100).map(|a|{let tl::enums::PageRelatedArticle::Article(a)=a;json!({"url":a.url,"webpage_id":a.webpage_id.to_string(),"title":a.title,"description":a.description,"photo_id":a.photo_id.map(|id|id.to_string()),"author":a.author,"published_date":a.published_date})}).collect::<Vec<_>>(),"partial":b.articles.len()>100})
            }
            B::Channel(b) => {
                json!({"type":"channel","chat":crate::story_payload::chat(&b.channel)})
            }
            B::List(b) => {
                json!({"type":"list","items":b.items.iter().take(100).map(|item|match item {
                tl::enums::PageListItem::Text(i)=>json!({"checkbox":i.checkbox,"checked":i.checked,"text":self.text(&i.text,depth+1)}),
                tl::enums::PageListItem::Blocks(i)=>json!({"checkbox":i.checkbox,"checked":i.checked,"blocks":self.blocks(&i.blocks,depth+1),"partial":i.blocks.len()>100}),
            }).collect::<Vec<_>>(),"partial":b.items.len()>100})
            }
            B::OrderedList(b) => {
                json!({"type":"ordered_list","reversed":b.reversed,"start":b.start,"list_type":b.r#type,"items":b.items.iter().take(100).map(|item|match item {
                tl::enums::PageListOrderedItem::Text(i)=>json!({"checkbox":i.checkbox,"checked":i.checked,"number":i.num,"value":i.value,"item_type":i.r#type,"text":self.text(&i.text,depth+1)}),
                tl::enums::PageListOrderedItem::Blocks(i)=>json!({"checkbox":i.checkbox,"checked":i.checked,"number":i.num,"value":i.value,"item_type":i.r#type,"blocks":self.blocks(&i.blocks,depth+1),"partial":i.blocks.len()>100}),
            }).collect::<Vec<_>>(),"partial":b.items.len()>100})
            }
            B::Collage(b) => {
                json!({"type":"collage","items":self.blocks(&b.items,depth+1),"caption":self.caption(&b.caption,depth+1),"partial":b.items.len()>100})
            }
            B::Slideshow(b) => {
                json!({"type":"slideshow","items":self.blocks(&b.items,depth+1),"caption":self.caption(&b.caption,depth+1),"partial":b.items.len()>100})
            }
            B::BlockquoteBlocks(b) => {
                json!({"type":"blockquote_blocks","blocks":self.blocks(&b.blocks,depth+1),"caption":self.text(&b.caption,depth+1),"partial":b.blocks.len()>100})
            }
            B::Audio(b) => {
                json!({"type":"audio","document_id":b.audio_id.to_string(),"caption":self.caption(&b.caption,depth+1)})
            }
            B::Document(b) => {
                json!({"type":"document","document_id":b.document_id.to_string(),"caption":self.caption(&b.caption,depth+1)})
            }
            B::Heading1(b) => line!("heading1", b),
            B::Heading2(b) => line!("heading2", b),
            B::Heading3(b) => line!("heading3", b),
            B::Heading4(b) => line!("heading4", b),
            B::Heading5(b) => line!("heading5", b),
            B::Heading6(b) => line!("heading6", b),
            B::Divider => json!({"type":"divider"}),
            B::Anchor(b) => json!({"type":"anchor","name":b.name}),
            B::AuthorDate(b) => {
                json!({"type":"author","author":self.text(&b.author,depth+1),"date":b.published_date})
            }
            B::Preformatted(b) => {
                json!({"type":"pre","language":b.language,"text":self.text(&b.text,depth+1)})
            }
            B::Blockquote(b) => {
                json!({"type":"blockquote","collapsed":b.collapsed,"text":self.text(&b.text,depth+1),"caption":self.text(&b.caption,depth+1)})
            }
            B::Pullquote(b) => {
                json!({"type":"pullquote","text":self.text(&b.text,depth+1),"caption":self.text(&b.caption,depth+1)})
            }
            B::Details(b) => {
                json!({"type":"details","open":b.open,"title":self.text(&b.title,depth+1),"blocks":self.blocks(&b.blocks,depth+1),"partial":b.blocks.len()>100})
            }
            B::Cover(b) => json!({"type":"cover","block":self.block(&b.cover,depth+1)}),
            B::Table(b) => {
                json!({"type":"table","bordered":b.bordered,"striped":b.striped,"compact":b.compact,"title":self.text(&b.title,depth+1),"rows":b.rows.iter().take(100).map(|r|{let tl::enums::PageTableRow::Row(r)=r;json!({"cells":r.cells.iter().take(16).map(|c|{let tl::enums::PageTableCell::Cell(c)=c;json!({"header":c.header,"align_center":c.align_center,"align_right":c.align_right,"valign_middle":c.valign_middle,"valign_bottom":c.valign_bottom,"colspan":c.colspan,"rowspan":c.rowspan,"text":c.text.as_ref().map(|t|self.text(t,depth+1))})}).collect::<Vec<_>>(),"partial":r.cells.len()>16})}).collect::<Vec<_>>(),"partial":b.rows.len()>100})
            }
            B::Photo(b) => {
                json!({"type":"photo","photo_id":b.photo_id.to_string(),"spoiler":b.spoiler,"url":b.url,"caption":self.caption(&b.caption,depth+1)})
            }
            B::Video(b) => {
                json!({"type":"video","document_id":b.video_id.to_string(),"autoplay":b.autoplay,"loop":b.r#loop,"spoiler":b.spoiler,"caption":self.caption(&b.caption,depth+1)})
            }
            B::Unsupported => json!({"type":"unsupported","partial":true}),
        }
    }
    fn caption(&mut self, value: &tl::enums::PageCaption, depth: usize) -> Value {
        let tl::enums::PageCaption::Caption(c) = value;
        json!({"text":self.text(&c.text,depth),"credit":self.text(&c.credit,depth)})
    }
}
pub fn has_partial(value: &Value) -> bool {
    match value {
        Value::Object(o) => {
            o.get("partial") == Some(&Value::Bool(true)) || o.values().any(has_partial)
        }
        Value::Array(a) => a.iter().any(has_partial),
        _ => false,
    }
}
pub fn from_tl(value: &tl::enums::RichMessage) -> Value {
    let tl::enums::RichMessage::Message(r) = value;
    let mut budget = Budget { nodes: 512 };
    let blocks = budget.blocks(&r.blocks, 0);
    let partial = r.part
        || r.blocks.len() > 100
        || has_partial(&blocks)
        || r.photos.len() > 100
        || r.documents.len() > 100;
    json!({"rtl":r.rtl,"is_partial":r.part,"blocks":blocks,"photos":r.photos.iter().take(100).filter_map(crate::rare_payload::photo).collect::<Vec<_>>(),"documents":r.documents.iter().take(100).filter_map(crate::rare_payload::document).collect::<Vec<_>>(),"partial":partial})
}
pub fn page(value: &tl::enums::Page) -> Value {
    let tl::enums::Page::Page(p) = value;
    let mut budget = Budget { nodes: 512 };
    let blocks = budget.blocks(&p.blocks, 0);
    let photos = p
        .photos
        .iter()
        .take(100)
        .filter_map(crate::rare_payload::photo)
        .collect::<Vec<_>>();
    let documents = p
        .documents
        .iter()
        .take(100)
        .filter_map(crate::rare_payload::document)
        .collect::<Vec<_>>();
    let partial = p.part
        || p.blocks.len() > 100
        || has_partial(&blocks)
        || photos.len() != p.photos.len()
        || documents.len() != p.documents.len();
    json!({"url":p.url,"rtl":p.rtl,"v2":p.v2,"views":p.views,"blocks":blocks,"photos":photos,"documents":documents,"partial":partial})
}
