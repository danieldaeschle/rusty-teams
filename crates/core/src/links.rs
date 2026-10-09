use std::net::Ipv4Addr;

use chatsvc::{LinkInfo, is_link_image_url};
use scraper::{Html, Selector};
use serde_json::{Value, json};
use store::MessageRecord;

use crate::stored::ImageRef;

pub const EMPTY_LINKS: &str = "[]";

const HYPERLINK_TYPE: &str = "http://schema.skype.com/HyperLink";
const INTERNAL_HOSTS: [&str; 8] = [
    "sharepoint.com",
    "onedrive.com",
    "1drv.ms",
    "teams.microsoft.com",
    "teams.cloud.microsoft",
    "teams.live.com",
    "office.com",
    "microsoft365.com",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
}

impl LinkPreview {
    pub fn from_info(url: &str, info: &LinkInfo) -> Option<LinkPreview> {
        let preview = LinkPreview {
            url: url.to_owned(),
            title: info.title.clone(),
            description: info.description.clone(),
            image_url: info
                .thumbnail
                .clone()
                .filter(|thumbnail| is_link_image_url(thumbnail)),
            image_width: info.thumbnail_width,
            image_height: info.thumbnail_height,
        };
        preview.is_showable().then_some(preview)
    }

    pub fn domain(&self) -> String {
        let host = link_host(&self.url).unwrap_or_default();
        host.strip_prefix("www.").unwrap_or(&host).to_owned()
    }

    pub fn image(&self) -> Option<ImageRef> {
        Some(ImageRef {
            id: String::new(),
            url: self.image_url.clone()?,
            width: self.image_width,
            height: self.image_height,
        })
    }

    pub fn links_json(&self) -> String {
        let mut preview = serde_json::Map::new();
        if let Some(image_url) = &self.image_url {
            preview.insert("previewurl".into(), json!(image_url));
        }
        if let Some((width, height)) = self.image_width.zip(self.image_height) {
            preview.insert(
                "previewmeta".into(),
                json!({"height": height, "width": width}),
            );
        }
        if let Some(title) = &self.title {
            preview.insert("title".into(), json!(title));
        }
        if let Some(description) = &self.description {
            preview.insert("description".into(), json!(description));
        }
        json!([{
            "@type": HYPERLINK_TYPE,
            "itemid": "0",
            "url": self.url,
            "preview": preview,
            "previewenabled": true,
        }])
        .to_string()
    }

    fn is_showable(&self) -> bool {
        self.title.is_some() || self.image_url.is_some()
    }
}

pub fn has_links_markup(record: &MessageRecord) -> bool {
    !record.deleted && record.body_html.contains("<a ")
}

pub fn has_stored_links(links_json: &str) -> bool {
    !matches!(links_json, "" | EMPTY_LINKS)
}

pub fn link_preview(record: &MessageRecord) -> Option<LinkPreview> {
    if record.deleted {
        return None;
    }
    select_preview(&record.links_json)
}

pub fn select_preview(links_json: &str) -> Option<LinkPreview> {
    let entries: Vec<Value> = serde_json::from_str(links_json).ok()?;
    entries.iter().find_map(entry_preview)
}

fn entry_preview(entry: &Value) -> Option<LinkPreview> {
    if entry.get("previewenabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let url = entry.get("url")?.as_str()?;
    if !is_public_link(url) {
        return None;
    }
    let preview = entry.get("preview").filter(|preview| preview.is_object())?;
    let text = |key: &str| {
        preview
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let meta = ["previewmeta", "previewMetadata"]
        .into_iter()
        .find_map(|key| preview.get(key).filter(|meta| meta.is_object()));
    let size = |key: &str| {
        meta.and_then(|meta| meta.get(key))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    };
    let candidate = LinkPreview {
        url: url.to_owned(),
        title: text("title"),
        description: text("description"),
        image_url: text("previewurl").filter(|image_url| is_link_image_url(image_url)),
        image_width: size("width"),
        image_height: size("height"),
    };
    candidate.is_showable().then_some(candidate)
}

pub fn first_public_link(html: &str) -> Option<String> {
    let selector = Selector::parse("a[href]").ok()?;
    Html::parse_fragment(html)
        .select(&selector)
        .filter_map(|anchor| anchor.value().attr("href"))
        .find(|href| is_public_link(href))
        .map(str::to_owned)
}

pub fn public_links(record: &MessageRecord) -> Vec<String> {
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    if record.deleted {
        return Vec::new();
    }
    let mut links: Vec<String> = Vec::new();
    for anchor in Html::parse_fragment(&record.body_html).select(&selector) {
        let Some(href) = anchor
            .value()
            .attr("href")
            .filter(|href| is_public_link(href))
        else {
            continue;
        };
        if !links.iter().any(|known| known == href) {
            links.push(href.to_owned());
        }
    }
    links
}

pub fn is_public_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    let Some(host) = link_host(url) else {
        return false;
    };
    if host.starts_with('[') || host == "localhost" || !host.contains('.') {
        return false;
    }
    if INTERNAL_HOSTS
        .iter()
        .any(|internal| host == *internal || host.ends_with(&format!(".{internal}")))
    {
        return false;
    }
    host.parse::<Ipv4Addr>().map_or(true, |address| {
        !(address.is_private()
            || address.is_loopback()
            || address.is_link_local()
            || address.is_unspecified())
    })
}

fn link_host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host_port.starts_with('[') {
        host_port
    } else {
        host_port.split(':').next()?
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THUMBNAIL: &str =
        "https://de-prod.asyncgw.teams.microsoft.com/urlp/v1/url/image/Thumbnail?url=x";

    fn links(entries: Value) -> String {
        entries.to_string()
    }

    #[test]
    fn public_links_lists_each_external_link_once() {
        let record = MessageRecord {
            body_html: "<p><a href=\"https://a.example/x\">x</a> <a href=\"https://contoso.sharepoint.com/f\">f</a> <a href=\"https://a.example/x\">again</a> <a href=\"https://b.example\">b</a></p>".to_owned(),
            ..demo_record()
        };
        assert_eq!(
            public_links(&record),
            ["https://a.example/x", "https://b.example"]
        );
    }

    fn demo_record() -> MessageRecord {
        MessageRecord {
            conversation_id: "c".to_owned(),
            message_id: "m".to_owned(),
            reply_to_id: None,
            sender_id: None,
            sender_name: None,
            created_at: chrono::Utc::now(),
            edited_at: None,
            deleted: false,
            body_html: String::new(),
            attachments_json: "[]".to_owned(),
            reactions_json: "[]".to_owned(),
            mentions_json: "[]".to_owned(),
            sender_application_id: None,
            links_json: "[]".to_owned(),
            subject: None,
        }
    }

    #[test]
    fn picks_the_first_public_link_with_a_title() {
        let preview = select_preview(&links(json!([
            {"url": "https://contoso.sharepoint.com/f", "preview": {"title": "File"}, "previewenabled": true},
            {"url": "https://a.example/x", "preview": {}, "previewenabled": true},
            {"url": "https://b.example/y", "preview": {"title": " B ", "description": "About B"}},
            {"url": "https://c.example/z", "preview": {"title": "C"}, "previewenabled": true},
        ])))
        .unwrap();
        assert_eq!(preview.url, "https://b.example/y");
        assert_eq!(preview.title.as_deref(), Some("B"));
        assert_eq!(preview.description.as_deref(), Some("About B"));
        assert_eq!(preview.domain(), "b.example");
    }

    #[test]
    fn an_image_alone_is_enough() {
        let preview = select_preview(&links(json!([
            {"url": "https://a.example", "preview": {"previewurl": THUMBNAIL, "previewmeta": {"height": 160, "width": 320}}, "previewenabled": null},
        ])))
        .unwrap();
        assert_eq!(preview.title, None);
        assert_eq!(preview.image_url.as_deref(), Some(THUMBNAIL));
        assert_eq!(
            (preview.image_width, preview.image_height),
            (Some(320), Some(160))
        );
    }

    #[test]
    fn reads_the_camel_case_meta_and_null_meta() {
        let preview = select_preview(&links(json!([
            {"url": "https://a.example", "preview": {"title": "A", "previewurl": THUMBNAIL, "previewMetadata": {"height": 10, "width": 20}}},
            {"url": "https://b.example", "preview": {"title": "B", "previewmeta": null}},
        ])))
        .unwrap();
        assert_eq!(
            (preview.image_width, preview.image_height),
            (Some(20), Some(10))
        );
        let second = select_preview(&links(json!([
            {"url": "https://b.example", "preview": {"title": "B", "previewmeta": null}},
        ])))
        .unwrap();
        assert_eq!(second.image_width, None);
    }

    #[test]
    fn a_removed_preview_is_not_shown() {
        assert_eq!(
            select_preview(&links(json!([
                {"url": "https://a.example", "preview": {"title": "A"}, "previewenabled": false},
            ]))),
            None
        );
    }

    #[test]
    fn images_outside_the_preview_service_are_dropped() {
        assert_eq!(
            select_preview(&links(json!([
                {"url": "https://a.example", "preview": {"previewurl": "https://evil.example/i.png"}},
            ]))),
            None
        );
    }

    #[test]
    fn garbage_and_empty_lists_have_no_preview() {
        assert_eq!(select_preview("[]"), None);
        assert_eq!(select_preview(""), None);
        assert_eq!(select_preview("{}"), None);
        assert_eq!(select_preview("[{\"url\": 3}]"), None);
    }

    #[test]
    fn internal_and_private_hosts_are_skipped() {
        for url in [
            "https://contoso.sharepoint.com/a",
            "https://contoso-my.sharepoint.com/a",
            "https://onedrive.com/a",
            "https://1drv.ms/a",
            "https://teams.microsoft.com/l/x",
            "https://teams.cloud.microsoft/x",
            "https://teams.live.com/x",
            "https://www.office.com/x",
            "https://m365.microsoft365.com/x",
            "https://localhost/x",
            "https://intranet/x",
            "http://192.168.1.2/x",
            "http://127.0.0.1:8080/x",
            "ftp://a.example/x",
            "mailto:a@b.example",
        ] {
            assert!(!is_public_link(url), "{url}");
        }
        for url in [
            "https://github.com/rust-lang/rust",
            "http://a.example:8080/x",
            "https://notsharepoint.com/x",
            "https://user@example.org/x",
        ] {
            assert!(is_public_link(url), "{url}");
        }
    }

    #[test]
    fn finds_the_first_public_anchor_in_html() {
        let html = "<p><a href=\"https://contoso.sharepoint.com/x\">f</a> \
                    <a href=\"https://a.example/?q=1&amp;r=2\">a</a> <a href=\"https://b.example\">b</a></p>";
        assert_eq!(
            first_public_link(html).as_deref(),
            Some("https://a.example/?q=1&r=2")
        );
        assert_eq!(first_public_link("<p>no link</p>"), None);
    }

    #[test]
    fn outgoing_json_reads_back_as_the_same_preview() {
        let preview = LinkPreview {
            url: "https://github.com/rust-lang/rust".into(),
            title: Some("Rust".into()),
            description: Some("Empowering".into()),
            image_url: Some(THUMBNAIL.into()),
            image_width: Some(320),
            image_height: Some(160),
        };
        let json: Value = serde_json::from_str(&preview.links_json()).unwrap();
        assert_eq!(json[0]["@type"], HYPERLINK_TYPE);
        assert_eq!(json[0]["itemid"], "0");
        assert_eq!(json[0]["previewenabled"], true);
        assert_eq!(json[0]["preview"]["previewurl"], THUMBNAIL);
        assert_eq!(select_preview(&preview.links_json()), Some(preview));
    }

    #[test]
    fn info_without_title_or_usable_image_makes_no_preview() {
        let info = LinkInfo {
            url: "https://a.example".into(),
            description: Some("only text".into()),
            thumbnail: Some("https://evil.example/i.png".into()),
            ..LinkInfo::default()
        };
        assert_eq!(LinkPreview::from_info("https://a.example", &info), None);
    }

    #[test]
    fn deleted_messages_show_no_preview() {
        let record = MessageRecord {
            deleted: true,
            links_json: links(json!([{"url": "https://a.example", "preview": {"title": "A"}}])),
            ..MessageRecord::default()
        };
        assert_eq!(link_preview(&record), None);
    }
}
