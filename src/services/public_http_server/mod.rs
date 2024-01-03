use {
    crate::{metrics::http_request_middleware, state::AppState},
    axum::{
        http, middleware,
        routing::{get, post},
        Router,
    },
    std::{
        net::{IpAddr, SocketAddr},
        sync::Arc,
    },
    tower::ServiceBuilder,
    tower_http::{
        cors::{Any, CorsLayer},
        request_id::MakeRequestUuid,
        trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
        ServiceBuilderExt,
    },
    tracing::{info, Level},
    wc::geoip::{
        block::{middleware::GeoBlockLayer, BlockingPolicy},
        MaxMindResolver,
    },
};

pub const DID_JSON_ENDPOINT: &str = "/.well-known/did.json";

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
                .make_span_with(
                    DefaultMakeSpan::new()
                        .level(Level::INFO)
                        .include_headers(true),
                )
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .include_headers(true),
                ),
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
        .route("/:project_id/notify", post(handlers::notify_v0::handler))
        .route("/v1/:project_id/notify", post(handlers::notify_v1::handler))
        .route(
            "/:project_id/subscribers",
            get(handlers::get_subscribers_v0::handler),
        )
        .route(
            "/v1/:project_id/subscribers",
            get(handlers::get_subscribers_v1::handler),
        )
        .route(
            "/:project_id/subscribe-topic",
            post(handlers::subscribe_topic::handler),
        )
        .route(
            "/v1/:project_id/welcome_notification",
            get(handlers::get_welcome_notification::handler),
        )
        .route(
            "/v1/:project_id/welcome_notification",
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
