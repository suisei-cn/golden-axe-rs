use std::{convert::Infallible, env};

use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    extract::Extension,
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get, post},
    AddExtensionLayer, Json, Router,
};
use log::info;
use teloxide::{
    dispatching::update_listeners::{self, StatefulListener},
    prelude::*,
    types::Update,
};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use url::Url;

use crate::{send_to_debug_channel, RUN_HASH};

pub async fn webhook(
    bot: impl Requester,
) -> Result<impl update_listeners::UpdateListener<Infallible>> {
    let path = RUN_HASH.get().unwrap();

    let url = Url::parse(&format!("https://{}/{}", &env::var("DOMAIN")?, path))?;

    let notify = format!("Webhook URL: {}", url);

    info!("{}", notify);

    bot.set_webhook(url)
        .send()
        .await
        .map_err(|_| anyhow!("Failed to set webhook"))?;

    send_to_debug_channel(bot, notify).await?;

    let (tx, rx) = mpsc::unbounded_channel::<Result<Update, Infallible>>();

    let app = Router::<Body>::new()
        .route(&format!("/{}", path), post(update))
        .layer(AddExtensionLayer::new(tx))
        .route("/health", any(|| async { "OK" }));

    tokio::spawn(axum::Server::bind(&"0.0.0.0:8080".parse()?).serve(app.into_make_service()));
    let stream = UnboundedReceiverStream::new(rx);

    Ok(StatefulListener::from_stream_without_graceful_shutdown(
        stream,
    ))
}

async fn update(
    Json(message): Json<Update>,
    Extension(tx): Extension<UnboundedSender<Result<Update, Infallible>>>,
) -> impl IntoResponse {
    tx.send(Ok(message))
        .expect("Cannot send an incoming update from the webhook");
    (StatusCode::OK, "OK")
}
