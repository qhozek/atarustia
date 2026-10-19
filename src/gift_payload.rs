//! Read-only gift metadata; no purchase or transfer operations.
use grammers_client::tl;
use serde_json::{Value, json};
fn peer(v: &tl::enums::Peer) -> i64 {
    grammers_session::types::PeerId::from(v.clone()).bot_api_dialog_id_unchecked()
}
pub(crate) fn text(v: &tl::enums::TextWithEntities) -> Value {
    let tl::enums::TextWithEntities::Entities(t) = v;
    json!({"text":t.text,"entities":t.entities.iter().filter_map(crate::botapi::MessageEntity::from_tl).collect::<Vec<_>>()})
}
pub fn amount(v: &tl::enums::StarsAmount) -> Value {
    match v {
        tl::enums::StarsAmount::Amount(a) => {
            json!({"currency":"XTR","amount":a.amount.to_string(),"nanos":a.nanos})
        }
        tl::enums::StarsAmount::StarsTonAmount(a) => {
            json!({"currency":"TON","amount":a.amount.to_string()})
        }
    }
}
fn rarity(v: &tl::enums::StarGiftAttributeRarity) -> Value {
    use tl::enums::StarGiftAttributeRarity as R;
    match v {
        R::Rarity(r) => json!({"type":"permille","permille":r.permille}),
        R::Uncommon => json!({"type":"uncommon"}),
        R::Rare => json!({"type":"rare"}),
        R::Epic => json!({"type":"epic"}),
        R::Legendary => json!({"type":"legendary"}),
    }
}
fn attribute(v: &tl::enums::StarGiftAttribute) -> Value {
    match v {
        tl::enums::StarGiftAttribute::Model(a) => {
            json!({"type":"model","name":a.name,"crafted":a.crafted,"document":crate::rare_payload::document(&a.document),"rarity":rarity(&a.rarity)})
        }
        tl::enums::StarGiftAttribute::Pattern(a) => {
            json!({"type":"pattern","name":a.name,"document":crate::rare_payload::document(&a.document),"rarity":rarity(&a.rarity)})
        }
        tl::enums::StarGiftAttribute::Backdrop(a) => {
            json!({"type":"backdrop","name":a.name,"backdrop_id":a.backdrop_id,"center_color":a.center_color,"edge_color":a.edge_color,"pattern_color":a.pattern_color,"text_color":a.text_color,"rarity":rarity(&a.rarity)})
        }
        tl::enums::StarGiftAttribute::OriginalDetails(a) => {
            json!({"type":"original_details","sender_id":a.sender_id.as_ref().map(peer),"recipient_id":peer(&a.recipient_id),"date":a.date,"message":a.message.as_ref().map(text)})
        }
    }
}
pub fn gift(v: &tl::enums::StarGift) -> Value {
    match v {
        tl::enums::StarGift::Gift(g) => {
            json!({"type":"regular","id":g.id.to_string(),"limited":g.limited,"sold_out":g.sold_out,"birthday":g.birthday,"require_premium":g.require_premium,"limited_per_user":g.limited_per_user,"peer_color_available":g.peer_color_available,"auction":g.auction,"sticker":crate::rare_payload::document(&g.sticker),"stars":g.stars.to_string(),"availability_remains":g.availability_remains,"availability_total":g.availability_total,"availability_resale":g.availability_resale.map(|n|n.to_string()),"convert_stars":g.convert_stars.to_string(),"first_sale_date":g.first_sale_date,"last_sale_date":g.last_sale_date,"upgrade_stars":g.upgrade_stars.map(|n|n.to_string()),"resell_min_stars":g.resell_min_stars.map(|n|n.to_string()),"title":g.title,"released_by":g.released_by.as_ref().map(peer),"per_user_total":g.per_user_total,"per_user_remains":g.per_user_remains,"locked_until_date":g.locked_until_date,"auction_slug":g.auction_slug,"gifts_per_round":g.gifts_per_round,"auction_start_date":g.auction_start_date,"upgrade_variants":g.upgrade_variants,"background":g.background.as_ref().map(|b|{let tl::enums::StarGiftBackground::Background(b)=b;json!({"center_color":b.center_color,"edge_color":b.edge_color,"text_color":b.text_color})})})
        }
        tl::enums::StarGift::Unique(g) => {
            json!({"type":"unique","id":g.id.to_string(),"gift_id":g.gift_id.to_string(),"title":g.title,"slug":g.slug,"number":g.num,"require_premium":g.require_premium,"resale_ton_only":g.resale_ton_only,"theme_available":g.theme_available,"burned":g.burned,"crafted":g.crafted,"owner_id":g.owner_id.as_ref().map(peer),"owner_name":g.owner_name,"owner_address":g.owner_address,"attributes":g.attributes.iter().take(100).map(attribute).collect::<Vec<_>>(),"availability_issued":g.availability_issued,"availability_total":g.availability_total,"gift_address":g.gift_address,"resell_amount":g.resell_amount.as_ref().map(|v|v.iter().take(100).map(amount).collect::<Vec<_>>()),"released_by":g.released_by.as_ref().map(peer),"value_amount":g.value_amount.map(|n|n.to_string()),"value_currency":g.value_currency,"value_usd_amount":g.value_usd_amount.map(|n|n.to_string()),"theme_peer":g.theme_peer.as_ref().map(peer),"host_id":g.host_id.as_ref().map(peer),"offer_min_stars":g.offer_min_stars,"craft_chance_permille":g.craft_chance_permille,"peer_color":g.peer_color.as_ref().map(color),"partial":g.attributes.len()>100||g.resell_amount.as_ref().is_some_and(|v|v.len()>100)})
        }
    }
}
fn color(v: &tl::enums::PeerColor) -> Value {
    match v {
        tl::enums::PeerColor::Color(c) => {
            json!({"type":"color","color":c.color,"background_emoji_id":c.background_emoji_id.map(|n|n.to_string())})
        }
        tl::enums::PeerColor::Collectible(c) => {
            json!({"type":"collectible","collectible_id":c.collectible_id.to_string(),"gift_emoji_id":c.gift_emoji_id.to_string(),"background_emoji_id":c.background_emoji_id.to_string(),"accent_color":c.accent_color,"colors":c.colors,"dark_accent_color":c.dark_accent_color,"dark_colors":c.dark_colors})
        }
        tl::enums::PeerColor::InputPeerColorCollectible(c) => {
            json!({"type":"input_collectible","collectible_id":c.collectible_id.to_string()})
        }
    }
}
pub fn theme(v: &tl::enums::ThemeSettings) -> Value {
    let tl::enums::ThemeSettings::Settings(t) = v;
    json!({"base_theme":match t.base_theme{tl::enums::BaseTheme::Classic=>"classic",tl::enums::BaseTheme::Day=>"day",tl::enums::BaseTheme::Night=>"night",tl::enums::BaseTheme::Tinted=>"tinted",tl::enums::BaseTheme::Arctic=>"arctic"},"message_colors_animated":t.message_colors_animated,"accent_color":t.accent_color,"outbox_accent_color":t.outbox_accent_color,"message_colors":t.message_colors,"has_wallpaper":t.wallpaper.is_some(),"wallpaper":t.wallpaper.as_ref().map(wallpaper)})
}
pub fn wallpaper(v: &tl::enums::WallPaper) -> Value {
    let settings = |s: &tl::enums::WallPaperSettings| {
        let tl::enums::WallPaperSettings::Settings(s) = s;
        json!({"blur":s.blur,"motion":s.motion,"background_color":s.background_color,"second_background_color":s.second_background_color,"third_background_color":s.third_background_color,"fourth_background_color":s.fourth_background_color,"intensity":s.intensity,"rotation":s.rotation,"emoticon":s.emoticon})
    };
    match v {
        tl::enums::WallPaper::Paper(p) => {
            json!({"type":"document","id":p.id.to_string(),"creator":p.creator,"default":p.default,"pattern":p.pattern,"dark":p.dark,"slug":p.slug,"document":crate::rare_payload::document(&p.document),"settings":p.settings.as_ref().map(settings)})
        }
        tl::enums::WallPaper::NoFile(p) => {
            json!({"type":"no_file","id":p.id.to_string(),"default":p.default,"dark":p.dark,"settings":p.settings.as_ref().map(settings)})
        }
    }
}
pub fn web_attribute(v: &tl::enums::WebPageAttribute) -> Value {
    match v {
        tl::enums::WebPageAttribute::Theme(a) => {
            json!({"type":"theme","documents":a.documents.as_ref().map(|v|v.iter().take(100).filter_map(crate::rare_payload::document).collect::<Vec<_>>()),"settings":a.settings.as_ref().map(theme),"partial":a.documents.as_ref().is_some_and(|v|v.len()>100)})
        }
        tl::enums::WebPageAttribute::Story(a) => {
            json!({"type":"story","chat_id":peer(&a.peer),"story_id":a.id,"has_story":a.story.is_some(),"partial":a.story.is_some()})
        }
        tl::enums::WebPageAttribute::StickerSet(a) => {
            json!({"type":"sticker_set","emojis":a.emojis,"text_color":a.text_color,"stickers":a.stickers.iter().take(100).filter_map(crate::rare_payload::document).collect::<Vec<_>>(),"partial":a.stickers.len()>100})
        }
        tl::enums::WebPageAttribute::UniqueStarGift(a) => {
            json!({"type":"unique_star_gift","gift":gift(&a.gift)})
        }
        tl::enums::WebPageAttribute::StarGiftCollection(a) => {
            json!({"type":"star_gift_collection","icons":a.icons.iter().take(100).filter_map(crate::rare_payload::document).collect::<Vec<_>>(),"partial":a.icons.len()>100})
        }
        tl::enums::WebPageAttribute::StarGiftAuction(a) => {
            json!({"type":"star_gift_auction","gift":gift(&a.gift),"end_date":a.end_date})
        }
        tl::enums::WebPageAttribute::AiComposeTone(a) => {
            json!({"type":"ai_compose_tone","emoji_id":a.emoji_id.to_string()})
        }
    }
}
