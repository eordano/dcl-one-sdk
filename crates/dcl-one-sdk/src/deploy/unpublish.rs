use super::net::{resolve_target_from, url_path};
use super::run::load_signer;
use crate::ux::{self, TrySteps, UserError};
use crate::world::signed_headers;
use anyhow::{Context, Result};
use catalyrst_crypto::Wallet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct UnpublishOptions {
    pub parcel: String,
    pub target: Option<String>,
    pub target_content: Option<String>,
    pub sign_key: Option<PathBuf>,
}

pub fn canon_parcel(raw: &str) -> Result<String> {
    let bad = || -> anyhow::Error {
        UserError::new(
            format!("\"{raw}\" is not a parcel coordinate"),
            TrySteps::one("expect two integers x,y \u{2014} e.g. --parcel 52,-52"),
        )
        .into()
    };
    let (x, y) = raw.trim().split_once(',').ok_or_else(bad)?;
    let x: i64 = x.trim().parse().map_err(|_| bad())?;
    let y: i64 = y.trim().parse().map_err(|_| bad())?;
    Ok(format!("{x},{y}"))
}

fn require_signer(sign_key: Option<&Path>) -> Result<Wallet> {
    match load_signer(sign_key)? {
        Some(signer) => Ok(signer),
        None => Err(UserError::new(
            "no wallet available to sign the unpublish request",
            TrySteps::one("set DCL_PRIVATE_KEY=<hex> (a wallet with rights on the parcel)")
                .and("or pass --sign-key <path-to-key-file>"),
        )
        .into()),
    }
}

pub async fn unpublish(opts: &UnpublishOptions) -> Result<()> {
    let parcel = canon_parcel(&opts.parcel)?;
    let signer = require_signer(opts.sign_key.as_deref())?;
    let base = resolve_target_from(
        opts.target.as_deref(),
        opts.target_content.as_deref(),
        None,
        true,
    )
    .await?;
    let path = format!("{}/scenes/{parcel}", url_path(&base));
    let url = format!("{base}/scenes/{parcel}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building the http client")?;
    let mut req = client.delete(&url);
    for (k, v) in signed_headers(&signer, "delete", &path)? {
        req = req.header(k, v);
    }
    let resp =
        match req.send().await {
            Ok(resp) => resp,
            Err(e) => return Err(UserError::new(
                "could not reach the content server",
                TrySteps::one("check the server is running and the URL is right").and(
                    "targets: --target <catalyst-domain>, --target-content <content-server-url>",
                ),
            )
            .why(format!("request failed: {url}"))
            .caused_by(e)
            .into()),
        };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused(&parcel, status.as_u16(), &body));
    }
    let mut steps = ux::Steps::new(1);
    steps.done(format!(
        "Unpublished {parcel} \u{2014} the parcel reverts to the synced Genesis City state on this network"
    ));
    Ok(())
}

fn refused(parcel: &str, status: u16, body: &str) -> anyhow::Error {
    let steps = match status {
        404 => TrySteps::one(
            "only scenes published to this network can be unpublished \u{2014} synced Genesis City entities are not deletable",
        )
        .and(format!(
            "check what is active: POST <content-url>/entities/active {{\"pointers\":[\"{parcel}\"]}}"
        )),
        401 | 403 => TrySteps::one(format!(
            "check the signing wallet owns or has operator rights on {parcel}"
        ))
        .and("re-run with --verbose for the full response"),
        _ => TrySteps::one("read the server message above")
            .and("re-run with --verbose for the full response"),
    };
    let mut u = UserError::new(
        format!("the content server refused to unpublish {parcel} (HTTP {status})"),
        steps,
    );
    let body = body.trim();
    if !body.is_empty() {
        u = u.why(body);
    }
    u.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ux;

    #[test]
    fn parcel_canonicalization() {
        assert_eq!(canon_parcel("52,-52").unwrap(), "52,-52");
        assert_eq!(canon_parcel(" 52 , -52 ").unwrap(), "52,-52");
        assert_eq!(canon_parcel("0,0").unwrap(), "0,0");
        assert!(canon_parcel("52").is_err());
        assert!(canon_parcel("a,b").is_err());
        assert!(canon_parcel("52,-52,3").is_err());
        assert!(canon_parcel("12.5,3").is_err());
    }

    #[test]
    fn bad_parcel_renders_a_user_error() {
        let err = canon_parcel("plaza").unwrap_err();
        let rendered = ux::render(&err, false, false);
        assert!(rendered.contains("not a parcel coordinate"), "{rendered}");
        assert!(rendered.contains("--parcel 52,-52"), "{rendered}");
    }
}
