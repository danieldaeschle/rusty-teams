use scraper::Html;

pub fn plain_text(html: &str) -> String {
    Html::parse_fragment(html)
        .root_element()
        .text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_collapses_whitespace() {
        assert_eq!(
            plain_text("<p>Hello <b>big</b></p>\n<p>world &amp; co</p>"),
            "Hello big world & co"
        );
    }
}
