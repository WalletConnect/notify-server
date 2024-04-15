use {
    crate::{services::public_http_server::RELAY_WEBHOOK_ENDPOINT, spec::INCOMING_TAGS},
    relay_client::{
        error::Error,
        http::{Client, WatchRegisterRequest},
    },
    relay_rpc::{
        auth::ed25519_dalek::SigningKey,
        rpc::{WatchError, WatchStatus, WatchType},
    },
    std::time::Duration,
    tracing::instrument,
    url::Url,
};

#[instrument(skip_all)]
pub async fn run(
    notify_url: &Url,
    keypair: &SigningKey,
    client: &Client,
) -> Result<(), Error<WatchError>> {
    client
        .watch_register(
            WatchRegisterRequest {
                service_url: notify_url.to_string(),
                webhook_url: notify_url
                    .join(RELAY_WEBHOOK_ENDPOINT)
                    .expect("Should be able to join static URLs")
                    .to_string(),
                watch_type: WatchType::Subscriber,
                tags: INCOMING_TAGS.to_vec(),
                // Alternatively we could not care about the tag, as an incoming message is an incoming message
                // tags: (4000..4100).collect(),
                statuses: vec![WatchStatus::Queued],
                ttl: Duration::from_secs(60 * 60 * 24 * 30),
            },
            keypair,
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::relay_client_helpers::create_http_client,
        chrono::Utc,
        hyper::StatusCode,
        relay_rpc::{
            domain::{DecodedClientId, DidKey, ProjectId},
            jwt::VerifyableClaims,
            rpc::{
                self, Params, Payload, Response, SuccessfulResponse, WatchAction,
                WatchRegisterClaims, WatchRegisterResponse,
            },
        },
        wiremock::{
            http::Method,
            matchers::{method, path},
            Mock, MockServer, Request, ResponseTemplate,
        },
    };

    #[tokio::test]
    async fn register_webhook_30_day_expiration() {
        let relay = MockServer::start().await;
        Mock::given(method(Method::POST))
            .and(path("/rpc"))
            .respond_with(|req: &Request| {
                let req = req.body_json::<rpc::Request>().unwrap();
                ResponseTemplate::new(StatusCode::OK).set_body_json(Payload::Response(
                    Response::Success(SuccessfulResponse {
                        id: req.id,
                        jsonrpc: req.jsonrpc,
                        result: serde_json::to_value(WatchRegisterResponse {
                            relay_id: DidKey::from(DecodedClientId::from_key(
                                &SigningKey::generate(&mut rand::thread_rng()).verifying_key(),
                            )),
                        })
                        .unwrap(),
                    }),
                ))
            })
            .mount(&relay)
            .await;
        let relay_url = relay.uri().parse::<Url>().unwrap();
        let notify_url = "https://example.com".parse::<Url>().unwrap();
        let keypair = SigningKey::generate(&mut rand::thread_rng());
        let relay_client = create_http_client(
            &keypair,
            relay_url,
            notify_url.clone(),
            ProjectId::generate(),
        )
        .unwrap();
        run(&notify_url, &keypair, &relay_client).await.unwrap();
        let requests = relay.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = requests.first().unwrap();
        let request = request.body_json::<rpc::Request>().unwrap();
        match request.params {
            Params::WatchRegister(p) => {
                let claims = WatchRegisterClaims::try_from_str(&p.register_auth).unwrap();
                assert_eq!(
                    claims.whu,
                    notify_url
                        .join(RELAY_WEBHOOK_ENDPOINT)
                        .expect("Should be able to join static URLs")
                        .to_string()
                );
                assert_eq!(claims.typ, WatchType::Subscriber);
                assert_eq!(claims.act, WatchAction::Register);
                assert_eq!(claims.sts, vec![WatchStatus::Queued]);
                const LEEWAY: i64 = 2;
                let expected_iat = Utc::now().timestamp();
                assert!(claims.basic.iat <= expected_iat);
                assert!(claims.basic.iat >= expected_iat - LEEWAY);
                let expected_exp = Utc::now().timestamp() + 30 * 24 * 60 * 60;
                assert!(claims.basic.exp.unwrap() <= expected_exp);
                assert!(claims.basic.exp.unwrap() > expected_exp - LEEWAY);
            }
            _ => panic!("Expected WatchRegister request, got {:?}", request.params),
        }
    }
}
