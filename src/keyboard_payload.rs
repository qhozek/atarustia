//! Read-only keyboard conversion. Binary callback data is preserved, never lossy UTF-8 replacement.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use grammers_client::tl;
use serde_json::{Value, json};
fn appearance(value: &Option<tl::enums::KeyboardButtonStyle>) -> Value {
    value.as_ref().map(|s|{let tl::enums::KeyboardButtonStyle::Style(s)=s;json!({"primary":s.bg_primary,"success":s.bg_success,"danger":s.bg_danger,"icon_custom_emoji_id":s.icon.map(|n|n.to_string())})}).unwrap_or(Value::Null)
}
pub fn from_tl(value: &tl::enums::ReplyMarkup) -> Value {
    let mut payload = convert(value);
    if crate::rich_payload::has_partial(&payload) {
        payload["partial"] = json!(true);
    }
    payload
}
fn convert(value: &tl::enums::ReplyMarkup) -> Value {
    match value {
        tl::enums::ReplyMarkup::ReplyInlineMarkup(m) => {
            let rows=m.rows.iter().take(100).map(|r|{let tl::enums::KeyboardInlineButtonRow::Row(r)=r;r.buttons.iter().take(8).map(|b|{let tl::enums::KeyboardInlineButton::Button(b)=b;json!({"text":b.text,"appearance":appearance(&b.style),"kind":match &b.r#type {
                tl::enums::InlineButtonType::Callback(c)=>json!({"type":"callback","data":std::str::from_utf8(&c.data).ok(),"data_base64":STANDARD.encode(&c.data),"requires_password":c.requires_password}),
                tl::enums::InlineButtonType::Url(u)=>json!({"type":"url","url":u.url}),
                tl::enums::InlineButtonType::UrlAuth(u)=>json!({"type":"login_url","url":u.url,"forward_text":u.fwd_text,"button_id":u.button_id}),
                tl::enums::InlineButtonType::InputInlineButtonTypeUrlAuth(u)=>json!({"type":"login_url","url":u.url,"forward_text":u.fwd_text,"request_write_access":u.request_write_access,"partial":u.bot.is_some()}),
                tl::enums::InlineButtonType::WebView(u)=>json!({"type":"web_app","url":u.url}),
                tl::enums::InlineButtonType::SwitchInline(u)=>json!({"type":"switch_inline","query":u.query,"same_chat":u.same_peer,"partial":u.peer_types.as_ref().is_some_and(|v|!v.is_empty())}),
                tl::enums::InlineButtonType::Copy(u)=>json!({"type":"copy","text":u.copy_text}),
                tl::enums::InlineButtonType::UserProfile(u)=>json!({"type":"user_profile","user_id":u.user_id}),
                tl::enums::InlineButtonType::InputInlineButtonTypeUserProfile(u)=>json!({"type":"user_profile","user_id":match &u.user_id{tl::enums::InputUser::User(u)=>Some(u.user_id),_=>None}}),
                tl::enums::InlineButtonType::Game=>json!({"type":"game"}),tl::enums::InlineButtonType::Buy=>json!({"type":"pay"}),tl::enums::InlineButtonType::Disabled=>json!({"type":"disabled"}),
            }})}).collect::<Vec<_>>()}).collect::<Vec<_>>();
            json!({"type":"inline","force_reply":m.force_reply,"rows":rows,"partial":m.rows.len()>100||m.rows.iter().any(|r|{let tl::enums::KeyboardInlineButtonRow::Row(r)=r;r.buttons.len()>8})})
        }
        tl::enums::ReplyMarkup::ReplyKeyboardMarkup(m) => {
            json!({"type":"reply","resize":m.resize,"one_time":m.single_use,"selective":m.selective,"persistent":m.persistent,"force_reply":m.force_reply,"placeholder":m.placeholder,"rows":m.rows.iter().take(100).map(|r|{let tl::enums::KeyboardButtonRow::Row(r)=r;r.buttons.iter().take(8).map(|b|{let tl::enums::KeyboardButton::Button(b)=b;json!({"text":b.text,"appearance":appearance(&b.style),"kind":match &b.r#type {
            tl::enums::ButtonType::Default=>json!({"type":"text"}),tl::enums::ButtonType::RequestPhone=>json!({"type":"request_contact"}),tl::enums::ButtonType::RequestGeoLocation=>json!({"type":"request_location"}),tl::enums::ButtonType::RequestPoll(p)=>json!({"type":"request_poll","quiz":p.quiz}),tl::enums::ButtonType::SimpleWebView(p)=>json!({"type":"web_app","url":p.url}),tl::enums::ButtonType::RequestPeer(p)=>json!({"type":"request_peer","button_id":p.button_id,"criteria":crate::request_peer::from_tl(&p.peer_type),"max_quantity":p.max_quantity}),tl::enums::ButtonType::InputButtonTypeRequestPeer(p)=>json!({"type":"request_peer","button_id":p.button_id,"criteria":crate::request_peer::from_tl(&p.peer_type),"max_quantity":p.max_quantity,"name_requested":p.name_requested,"username_requested":p.username_requested,"photo_requested":p.photo_requested}),
        }})}).collect::<Vec<_>>()}).collect::<Vec<_>>(),"partial":m.rows.len()>100||m.rows.iter().any(|r|{let tl::enums::KeyboardButtonRow::Row(r)=r;r.buttons.len()>8})})
        }
        tl::enums::ReplyMarkup::ReplyKeyboardHide(m) => {
            json!({"type":"remove","selective":m.selective})
        }
        tl::enums::ReplyMarkup::ReplyKeyboardForceReply(m) => {
            json!({"type":"force_reply","selective":m.selective,"one_time":m.single_use,"placeholder":m.placeholder})
        }
    }
}
