use super::NewsBackend;
use crate::domain::ArchNewsItem;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct ArchNewsBackend {
    client: Client,
}

impl ArchNewsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .context("failed to configure Arch news HTTP client")?,
        })
    }
}

#[async_trait]
impl NewsBackend for ArchNewsBackend {
    async fn latest(&self) -> Result<Vec<ArchNewsItem>> {
        let response = self
            .client
            .get("https://archlinux.org/feeds/news/")
            .send()
            .await
            .context("Arch news request failed")?;
        if !response.status().is_success() {
            bail!("Arch news returned HTTP {}", response.status());
        }
        let body = response
            .text()
            .await
            .context("invalid Arch news response")?;
        let mut items = crate::parser::parse_arch_news_rss(&body);
        items.truncate(10);
        Ok(items)
    }
}
