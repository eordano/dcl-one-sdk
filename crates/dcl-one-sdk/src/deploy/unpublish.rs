use super::net::{resolve_target_from, url_path, TargetConsent};
use super::run::load_signer;
use super::{read_server_message, refusal, send_text, with_headers, VERBOSE_HINT};
use crate::ux::{self, TrySteps, UserError};
use crate::world::signed_headers;
use anyhow::Result;
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
    let (x, y) = catalyrst_auth_chain::pointer::parse_pointer(raw).ok_or_else(|| {
        UserError::new(
            format!("\"{raw}\" is not a parcel coordinate"),
            TrySteps::one("expect two integers x,y \u{2014} e.g. --parcel 52,-52"),
        )
    })?;
    Ok(format!("{x},{y}"))
}

fn require_signer(sign_key: Option<&Path>) -> Result<Wallet> {
    load_signer(sign_key)?.ok_or_else(|| {
        UserError::new(
            "no wallet available to sign the unpublish request",
            TrySteps::one("set DCL_PRIVATE_KEY=<hex> (a wallet with rights on the parcel)")
                .and("or pass --sign-key <path-to-key-file>"),
        )
        .into()
    })
}

pub async fn unpublish(opts: &UnpublishOptions) -> Result<()> {
    let parcel = canon_parcel(&opts.parcel)?;
    let signer = require_signer(opts.sign_key.as_deref())?;
    let base = resolve_target_from(
        opts.target.as_deref(),
        opts.target_content.as_deref(),
        None,
        true,
        TargetConsent::default(),
    )
    .await?;
    let path = format!("{}/scenes/{parcel}", url_path(&base));
    let url = format!("{base}/scenes/{parcel}");
    let client = super::client(Duration::from_secs(30), Duration::from_secs(30))?;
    let req = with_headers(
        client.delete(&url),
        signed_headers(&signer, "delete", &path)?,
    );
    let (status, body) = send_text(req)
        .await
        .map_err(|e| super::unreachable_server(&url, e))?;
    if !(200..300).contains(&status) {
        return Err(refused(&parcel, status, &body));
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
        .and(VERBOSE_HINT),
        _ => read_server_message(),
    };
    refusal(
        UserError::new(
            format!("the content server refused to unpublish {parcel} (HTTP {status})"),
            steps,
        ),
        body,
    )
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
