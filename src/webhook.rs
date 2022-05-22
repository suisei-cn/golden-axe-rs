use std::{
    convert::Infallible,
    future::{ready, Ready},
    time::Duration,
};

use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    extract::Extension,
    http::StatusCode,
    response::IntoResponse,
    routing::{any, post},
    Json, Router,
};
use teloxide::{
    dispatching::update_listeners::{self, StatefulListener},
    prelude::*,
    types::Update,
};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedSender},
    time::sleep,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::info;
use url::Url;

use crate::{debug, BotType, Config};

/// # Errors
/// Failed when unable to remove/add webhook
///
/// # Panics
/// When domain are not parsable into Url
pub async fn setup(bot: &BotType) -> Result<impl update_listeners::UpdateListener<Infallible>> {
    let config = Config::get();
    let path = config.run_hash();

    let url = Url::parse(&format!(
        "https://{}/{}",
        config.domain.as_deref().unwrap(),
        path
    ))?;

    let notify = format!("Webhook URL: {}", url);
    info!("{}", notify);

    bot.delete_webhook()
        .send()
        .await
        .map_err(|_| anyhow!("Failed to delete webhook"))?;

    sleep(Duration::from_secs_f32(0.5)).await;

    bot.set_webhook(url)
        .send()
        .await
        .map_err(|_| anyhow!("Failed to set webhook"))?;

    debug(&notify);

    let (tx, rx) = unbounded_channel::<Result<Update, Infallible>>();

    let app = Router::<Body>::new()
        .route(&format!("/{}", path), post(handle_update))
        .layer(Extension(tx))
        .route("/health", any(|| async { StatusCode::NO_CONTENT }));

    tokio::spawn(
        axum::Server::bind(&"0.0.0.0:8080".parse().unwrap()).serve(app.into_make_service()),
    );

    let stream = UnboundedReceiverStream::new(rx);

    Ok(StatefulListener::from_stream_without_graceful_shutdown(
        stream,
    ))
}

fn handle_update(
    Json(message): Json<Update>,
    Extension(tx): Extension<UnboundedSender<Result<Update, Infallible>>>,
) -> Ready<impl IntoResponse> {
    info!("New tg message");
    tx.send(Ok(message))
        .expect("Cannot send an incoming update from the webhook");
    ready((StatusCode::OK, "OK"))
}
