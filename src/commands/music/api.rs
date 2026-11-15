use anyhow::{anyhow, bail, Result};
use reqwest::{Method, Response, StatusCode};

use crate::bot::Context;

pub(crate) async fn request(ctx: &Context<'_>, method: Method, endpoint: &str) -> Result<Response> {
    request_path(ctx, method, &format!("me/player/{endpoint}")).await
}

pub(crate) async fn request_path(
    ctx: &Context<'_>,
    method: Method,
    path: &str,
) -> Result<Response> {
    let user_id = ctx.author().id.to_string();
    let token = match ctx.data().get_access_token(&user_id).await {
        Ok(token) => token,
        Err(spoticord_database::error::DatabaseError::NotFound) => {
            bail!("Link your Spotify account first with `/link`.")
        }
        Err(spoticord_database::error::DatabaseError::RefreshTokenFailure) => {
            bail!("Your Spotify link expired. Run `/unlink`, then `/link` to reconnect.")
        }
        Err(error) => return Err(anyhow!("Could not load your Spotify account: {error}")),
    };

    let response = reqwest::Client::new()
        .request(method, format!("https://api.spotify.com/v1/{path}"))
        .bearer_auth(token)
        .send()
        .await?;

    match response.status() {
        StatusCode::UNAUTHORIZED => {
            bail!("Spotify rejected the saved login. Run `/unlink`, then `/link` to reconnect.")
        }
        StatusCode::FORBIDDEN => {
            bail!("Spotify denied this action. Check that your account has Premium and that you granted the playback permissions when linking it.")
        }
        StatusCode::NOT_FOUND => {
            bail!("Spotify could not find an active playback device. Open Spotify on one of your devices and try again.")
        }
        status if status.is_success() => Ok(response),
        status => bail!("Spotify returned an unexpected response ({status}). Please try again."),
    }
}
