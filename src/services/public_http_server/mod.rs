use {
    crate::{metrics::http_request_middleware, state::AppState},
    axum::{
        http::{self, HeaderValue},
        middleware,
        routing::{get, post},
        Router,
    },
    hyper::Request,
    std::{
        net::{IpAddr, SocketAddr},
        sync::Arc,
    },
    tower::ServiceBuilder,
    tower_http::{
        cors::{Any, CorsLayer},
        request_id::MakeRequestUuid,
        trace::{DefaultOnResponse, MakeSpan, OnRequest, TraceLayer},
        ServiceBuilderExt,
    },
    tracing::{info, Level, Span},
    wc::geoip::{
        block::{middleware::GeoBlockLayer, BlockingPolicy},
        MaxMindResolver,
    },
};

pub const DID_JSON_ENDPOINT: &str = "/.well-known/did.json";
pub const RELAY_WEBHOOK_ENDPOINT: &str = "/v1/relay-webhook";

pub mod handlers;

pub async fn start(
    bind_ip: IpAddr,
    port: u16,
    blocked_countries: Vec<String>,
    state: Arc<AppState>,
    geoip_resolver: Option<Arc<MaxMindResolver>>,
) -> Result<(), hyper::Error> {
    let global_middleware = ServiceBuilder::new()
        .set_x_request_id(MakeRequestUuid)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(CustomMakeSpan)
                .on_request(CustomOnRequest)
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .propagate_x_request_id()
        .layer(
            // TODO test
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]),
        );

    // blocked by https://github.com/tokio-rs/axum/issues/2292
    // .option_layer(geoip_resolver.map(|geoip_resolver| {
    //     GeoBlockLayer::new(
    //         geoip_resolver.clone(),
    //         state_arc.config.blocked_countries.clone(),
    //         BlockingPolicy::AllowAll,
    //     )
    // }));

    let app = Router::new()
        .route("/health", get(handlers::health::handler))
        .route(DID_JSON_ENDPOINT, get(handlers::did_json::handler))
        .route(RELAY_WEBHOOK_ENDPOINT, post(handlers::relay_webhook::handler))
        .route("/v1/notification/:subscriber_notification_id", get(handlers::follow_notification_link::handler))
        .route("/:project_id/notify", post(handlers::notify_v0::handler))
        .route("/v1/:project_id/notify", post(handlers::notify_v1::handler))
        .route(
            "/:project_id/subscribers",
            get(handlers::get_subscribers_v0::handler),
        )
        .route(
            "/v1/:project_id/subscribers",
            post(handlers::get_subscribers_v1::handler),
        )
        .route(
            "/:project_id/subscribe-topic",
            post(handlers::subscribe_topic::handler),
        )
        .route(
            "/v0/:project_id/welcome-notification",
            get(handlers::get_welcome_notification::handler),
        )
        .route(
            "/v0/:project_id/welcome-notification",
            post(handlers::post_welcome_notification::handler),
        )
        // FIXME
        // .route(
        //     "/:project_id/register-webhook",
        //     post(services::handlers::webhooks::register_webhook::handler),
        // )
        // .route(
        //     "/:project_id/webhooks",
        //     get(services::handlers::webhooks::get_webhooks::handler),
        // )
        // .route(
        //     "/:project_id/webhooks/:webhook_id",
        //     delete(services::handlers::webhooks::delete_webhook::handler),
        // )
        // .route(
        //     "/:project_id/webhooks/:webhook_id",
        //     put(services::handlers::webhooks::update_webhook::handler),
        // )
        .route_layer(middleware::from_fn_with_state(state.clone(), http_request_middleware))
        .layer(global_middleware);
    let app = if let Some(ip_resolver) = geoip_resolver {
        app.layer(GeoBlockLayer::new(
            ip_resolver,
            blocked_countries,
            BlockingPolicy::AllowAll,
        ))
    } else {
        app
    };
    let app = app.with_state(state);

    let addr = SocketAddr::from((bind_ip, port));
    info!("Starting public HTTP server on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
}

#[derive(Clone, Debug)]
pub struct CustomMakeSpan;

impl<B> MakeSpan<B> for CustomMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        tracing::info_span!(
            "request",
            x_request_id = request
                .headers()
                .get("x-request-id")
                .unwrap_or(&HeaderValue::from_static("No x-request-id header"))
                .to_str()
                .unwrap_or("Invalid x-request-id header"),
        )
    }
}

#[derive(Clone, Debug)]
pub struct CustomOnRequest;

impl<B> OnRequest<B> for CustomOnRequest {
    fn on_request(&mut self, request: &Request<B>, _: &Span) {
        tracing::info!(
            "started processing request: method={method} uri={uri} version={version:?} headers={headers:?}",
            method = request.method(),
            uri = request.uri(),
            version = request.version(),
            headers = request.headers(),
        )
    }
}
