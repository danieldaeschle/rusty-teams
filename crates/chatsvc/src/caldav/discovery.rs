use reqwest::Url;

use crate::error::{Error, Result};

use super::client::CalDavClient;
use super::xml::{parse_multistatus, propfind_body};

#[derive(Debug, Clone)]
pub struct DiscoverResult {
    pub principal: String,
    pub home_set: String,
    pub user_addresses: Vec<String>,
    pub schedule_outbox: Option<String>,
}

pub async fn discover(client: &CalDavClient, start_url: &str) -> Result<DiscoverResult> {
    let well_known = join_well_known(start_url)?;
    let principal_href = match current_user_principal(client, &well_known).await {
        Ok(href) => href,
        Err(_) => current_user_principal(client, start_url).await?,
    };
    let principal = resolve_url(start_url, &principal_href)?;
    let home_href = calendar_home_set(client, &principal).await?;
    let home_set = resolve_url(&principal, &home_href)?;
    let (user_addresses, schedule_outbox) = user_scheduling(client, &principal).await?;
    Ok(DiscoverResult {
        principal,
        home_set,
        user_addresses,
        schedule_outbox,
    })
}

fn join_well_known(url: &str) -> Result<String> {
    let parsed = Url::parse(url).map_err(|e| Error::UnexpectedAnswer(e.to_string()))?;
    let mut base = parsed.clone();
    base.set_path("/.well-known/caldav");
    base.set_query(None);
    base.set_fragment(None);
    Ok(base.to_string())
}

async fn current_user_principal(client: &CalDavClient, url: &str) -> Result<String> {
    let body = propfind_body(&["<d:current-user-principal/>"]);
    let xml = client.propfind(url, "0", &body).await?;
    parse_multistatus(&xml)
        .into_iter()
        .find_map(|r| r.current_user_principal)
        .ok_or_else(|| Error::UnexpectedAnswer("CalDAV current-user-principal missing".into()))
}

async fn calendar_home_set(client: &CalDavClient, principal: &str) -> Result<String> {
    let body = propfind_body(&["<c:calendar-home-set/>"]);
    let xml = client.propfind(principal, "0", &body).await?;
    parse_multistatus(&xml)
        .into_iter()
        .find_map(|r| r.calendar_home_set)
        .ok_or_else(|| Error::UnexpectedAnswer("CalDAV calendar-home-set missing".into()))
}

async fn user_scheduling(
    client: &CalDavClient,
    principal: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let body = propfind_body(&["<c:calendar-user-address-set/>", "<c:schedule-outbox-URL/>"]);
    let xml = client.propfind(principal, "0", &body).await?;
    let mut addresses = Vec::new();
    let mut outbox = None;
    for response in parse_multistatus(&xml) {
        addresses.extend(response.calendar_user_addresses);
        if outbox.is_none() {
            outbox = response.schedule_outbox;
        }
    }
    Ok((addresses, outbox))
}

pub fn resolve_url(base: &str, href: &str) -> Result<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Ok(href.to_owned());
    }
    let base = Url::parse(base).map_err(|e| Error::UnexpectedAnswer(e.to_string()))?;
    Ok(base
        .join(href)
        .map_err(|e| Error::UnexpectedAnswer(e.to_string()))?
        .to_string())
}

pub fn reject_https_downgrade(from: &str, to: &str) -> Result<()> {
    if from.starts_with("https://") && to.starts_with("http://") {
        return Err(Error::UnexpectedAnswer(
            "CalDAV refused HTTPS to HTTP redirect (credentials would be sent in cleartext)".into(),
        ));
    }
    Ok(())
}
