use std::convert::Infallible;

use color_eyre::{eyre::Context, Result};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Response, Server,
};
use tracing::info;

fn no_content() -> Result<Response<Body>, Infallible> {
    Result::<_, Infallible>::Ok(Response::builder().status(204).body(Body::empty()).unwrap())
}

fn not_found() -> Result<Response<Body>, Infallible> {
    Result::<_, Infallible>::Ok(
        Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(Body::from("Not Found"))
            .unwrap(),
    )
}

pub async fn run() -> Result<()> {
    let make_service = make_service_fn(|_| async {
        Ok::<_, Infallible>(service_fn(|req| async move {
            match req.uri().path() {
                "/health" => no_content(),
                _ => not_found(),
            }
        }))
    });
    info!("Server running");
    Server::bind(&"0.0.0.0:8000".parse().unwrap())
        .serve(make_service)
        .await
        .wrap_err("")
}
