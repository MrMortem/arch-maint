use crate::domain::ArchNewsItem;

pub fn parse_arch_news_rss(input: &str) -> Vec<ArchNewsItem> {
    input
        .split("<item")
        .skip(1)
        .filter_map(|item| {
            let item = item.split_once("</item>")?.0;
            let title = tag(item, "title")?;
            let link = tag(item, "link")?;
            Some(ArchNewsItem {
                title: decode_xml(&title),
                link: decode_xml(&link),
                published: tag(item, "pubDate").map(|value| decode_xml(&value)),
                summary: tag(item, "description")
                    .map(|value| strip_html(&decode_xml(&value)))
                    .filter(|value| !value.is_empty()),
            })
        })
        .collect()
}

fn tag(input: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = input.find(&open)? + open.len();
    let end = input[start..].find(&close)? + start;
    Some(
        input[start..end]
            .trim()
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>")
            .trim()
            .to_owned(),
    )
}

fn decode_xml(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn strip_html(input: &str) -> String {
    let mut result = String::new();
    let mut inside_tag = false;
    for character in input.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => result.push(character),
            _ => {}
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rss_items_and_cdata_without_html_claims() {
        let rss = r#"<rss><channel><item><title>Manual intervention &amp; notice</title><link>https://archlinux.org/news/test/</link><pubDate>Mon, 17 Aug 2026 00:00:00 +0000</pubDate><description><![CDATA[<p>Read this first.</p>]]></description></item></channel></rss>"#;
        let items = parse_arch_news_rss(rss);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Manual intervention & notice");
        assert_eq!(items[0].summary.as_deref(), Some("Read this first."));
    }
}
